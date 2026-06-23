//! ncurses-native development tooling.
//!
//! `cargo xtask gen`   regenerates docs/generated/*.md from the committed models.
//! `cargo xtask check` re-renders in memory and fails if the committed docs (or
//!                     the model invariants) have drifted -- the freshness gate.
//!
//! Authority split (the whole point of the port-parity method):
//!   * the C public-API inventory is authoritative from clang (docs-src/models/c-api.json),
//!   * the Rust inventory is authoritative from syn (parsed live from ../src),
//!   * the join (counterpart + status + court) is curated and *validated*:
//!     every counterpart symbol must exist in the Rust inventory, every C name
//!     must exist in the C inventory, and every cited court must exist as an
//!     oracle receipt. None of this is taken on faith.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::exit;

use serde::Deserialize;

// ---------------------------------------------------------------------------
// Models
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CApi {
    provenance: CProvenance,
    functions: Vec<String>,
    function_header: BTreeMap<String, String>,
    count: usize,
}

#[derive(Deserialize)]
struct CProvenance {
    tool: String,
    method: String,
    headers: Vec<CHeader>,
}

#[derive(Deserialize)]
struct CHeader {
    header: String,
    sha256: String,
    functions: usize,
}

#[derive(Deserialize)]
struct ParityModel {
    legend: Vec<Legend>,
    coverage_definition: String,
    default_group: String,
    groups: Vec<Group>,
    group_rules: Vec<Rule>,
    counterparts: BTreeMap<String, Counterpart>,
    #[serde(default)]
    classifications: BTreeMap<String, Classification>,
}

#[derive(Deserialize, Clone)]
struct Classification {
    status: String,
    #[serde(default)]
    courts: Vec<String>,
    #[serde(default)]
    note: String,
}

/// A unified resolution for one C function: either a Rust counterpart or a
/// non-output classification. `rust` is None for classifications.
struct Resolution {
    status: String,
    rust: Option<String>,
    courts: Vec<String>,
    note: String,
}

#[derive(Deserialize, Clone)]
struct Legend {
    marker: String,
    status: String,
    meaning: String,
}

#[derive(Deserialize, Clone)]
struct Group {
    id: String,
    title: String,
    role: String,
}

#[derive(Deserialize)]
struct Rule {
    kind: String,
    value: String,
    group: String,
}

#[derive(Deserialize, Clone)]
struct Counterpart {
    rust: String,
    status: String,
    #[serde(default)]
    courts: Vec<String>,
    #[serde(default)]
    note: String,
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is .../ncurses-native/xtask
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    here.parent().expect("xtask has a parent").to_path_buf()
}

// ---------------------------------------------------------------------------
// Rust inventory (syn)
// ---------------------------------------------------------------------------

/// Collect every `pub` item (fn/const/static/struct/enum/type) reachable in the
/// crate's `src/` tree as `module::name`, so curated counterparts can be checked
/// against what actually exists.
fn rust_inventory(src: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut files = Vec::new();
    collect_rs(src, &mut files);
    for f in files {
        let module = module_of(src, &f);
        let text = std::fs::read_to_string(&f).unwrap_or_default();
        let parsed = match syn::parse_file(&text) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("warning: syn failed on {}: {e}", f.display());
                continue;
            }
        };
        collect_items(&parsed.items, &module, &mut out);
    }
    out
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_rs(&p, out);
        } else if p.extension().map(|x| x == "rs").unwrap_or(false) {
            out.push(p);
        }
    }
}

fn module_of(src: &Path, file: &Path) -> String {
    let rel = file.strip_prefix(src).unwrap_or(file);
    let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    if stem == "lib" || stem == "main" {
        return String::new();
    }
    // flat crate: module == file stem; nested dirs would prepend components.
    let mut parts: Vec<String> = rel
        .parent()
        .map(|p| {
            p.components()
                .filter_map(|c| c.as_os_str().to_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    if stem != "mod" {
        parts.push(stem.to_string());
    }
    parts.join("::")
}

fn collect_items(items: &[syn::Item], module: &str, out: &mut BTreeSet<String>) {
    let push = |out: &mut BTreeSet<String>, name: String| {
        if module.is_empty() {
            out.insert(name);
        } else {
            out.insert(format!("{module}::{name}"));
        }
    };
    for item in items {
        match item {
            syn::Item::Fn(f) if is_pub(&f.vis) => push(out, f.sig.ident.to_string()),
            syn::Item::Const(c) if is_pub(&c.vis) => push(out, c.ident.to_string()),
            syn::Item::Static(s) if is_pub(&s.vis) => push(out, s.ident.to_string()),
            syn::Item::Struct(s) if is_pub(&s.vis) => push(out, s.ident.to_string()),
            syn::Item::Enum(e) if is_pub(&e.vis) => push(out, e.ident.to_string()),
            syn::Item::Type(t) if is_pub(&t.vis) => push(out, t.ident.to_string()),
            syn::Item::Mod(m) if is_pub(&m.vis) => {
                if let Some((_, inner)) = &m.content {
                    let nested = if module.is_empty() {
                        m.ident.to_string()
                    } else {
                        format!("{module}::{}", m.ident)
                    };
                    collect_items(inner, &nested, out);
                }
            }
            // Inherent-impl methods become `module::Type::method`, so curated
            // counterparts can point at a method and the AST compass verifies it.
            syn::Item::Impl(im) if im.trait_.is_none() => {
                if let syn::Type::Path(tp) = im.self_ty.as_ref() {
                    if let Some(seg) = tp.path.segments.last() {
                        let ty = seg.ident.to_string();
                        for ii in &im.items {
                            if let syn::ImplItem::Fn(f) = ii {
                                if is_pub(&f.vis) {
                                    push(out, format!("{ty}::{}", f.sig.ident));
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn is_pub(v: &syn::Visibility) -> bool {
    matches!(v, syn::Visibility::Public(_))
}

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

fn classify(name: &str, rules: &[Rule], default: &str) -> String {
    for r in rules {
        let hit = match r.kind.as_str() {
            "exact" => name == r.value,
            "prefix" => name.starts_with(&r.value),
            _ => false,
        };
        if hit {
            return r.group.clone();
        }
    }
    default.to_string()
}

fn marker_for(status: &str, legend: &[Legend]) -> String {
    legend
        .iter()
        .find(|l| l.status == status)
        .map(|l| l.marker.clone())
        .unwrap_or_else(|| "?".into())
}

/// Severity order for showing a multiset of markers in a group's status cell.
fn status_rank(status: &str) -> u8 {
    match status {
        "full" => 0,
        "partial" => 1,
        "scaffold" => 2,
        "divergent" => 3,
        "deferred" => 4,
        "n_a" => 5,
        _ => 6,
    }
}

// ---------------------------------------------------------------------------
// Receipts
// ---------------------------------------------------------------------------

fn receipt_verdict(root: &Path, court: &str) -> Option<String> {
    let p = root.join("reports/oracle").join(format!("{court}.json"));
    let text = std::fs::read_to_string(p).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get("verdict").and_then(|x| x.as_str()).map(String::from)
}

fn receipt_exists(root: &Path, court: &str) -> bool {
    root.join("reports/oracle")
        .join(format!("{court}.json"))
        .exists()
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

struct Ctx {
    c: CApi,
    m: ParityModel,
    /// function -> group id
    group_of: BTreeMap<String, String>,
    root: PathBuf,
}

impl Ctx {
    fn load(root: &Path) -> Ctx {
        let c: CApi = serde_json::from_str(
            &std::fs::read_to_string(root.join("docs-src/models/c-api.json"))
                .expect("read c-api.json"),
        )
        .expect("parse c-api.json");
        let m: ParityModel = serde_json::from_str(
            &std::fs::read_to_string(root.join("docs-src/models/parity.model.json"))
                .expect("read parity.model.json"),
        )
        .expect("parse parity.model.json");
        let mut group_of = BTreeMap::new();
        for f in &c.functions {
            group_of.insert(f.clone(), classify(f, &m.group_rules, &m.default_group));
        }
        Ctx { c, m, group_of, root: root.to_path_buf() }
    }

    fn group_title(&self, id: &str) -> String {
        self.m
            .groups
            .iter()
            .find(|g| g.id == id)
            .map(|g| g.title.clone())
            .unwrap_or_else(|| id.to_string())
    }

    fn group_role(&self, id: &str) -> String {
        self.m
            .groups
            .iter()
            .find(|g| g.id == id)
            .map(|g| g.role.clone())
            .unwrap_or_default()
    }

    /// The unified resolution for a C function: a Rust counterpart wins, else a
    /// non-output classification, else None (a true gap).
    fn resolution(&self, f: &str) -> Option<Resolution> {
        if let Some(cp) = self.m.counterparts.get(f) {
            return Some(Resolution {
                status: cp.status.clone(),
                rust: Some(cp.rust.clone()),
                courts: cp.courts.clone(),
                note: cp.note.clone(),
            });
        }
        if let Some(cl) = self.m.classifications.get(f) {
            return Some(Resolution {
                status: cl.status.clone(),
                rust: None,
                courts: cl.courts.clone(),
                note: cl.note.clone(),
            });
        }
        None
    }

    fn functions_in(&self, gid: &str) -> Vec<String> {
        let mut v: Vec<String> = self
            .c
            .functions
            .iter()
            .filter(|f| self.group_of.get(*f).map(|g| g == gid).unwrap_or(false))
            .cloned()
            .collect();
        v.sort();
        v
    }

    /// Count of functions in a group that are resolved (counterpart or classification).
    fn covered_count(&self, gid: &str) -> usize {
        self.functions_in(gid)
            .iter()
            .filter(|f| self.resolution(f).is_some())
            .count()
    }

    /// Groups that actually contain at least one C function, sorted by parity desc.
    fn live_groups(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .m
            .groups
            .iter()
            .map(|g| g.id.clone())
            .filter(|id| !self.functions_in(id).is_empty())
            .collect();
        ids.sort_by(|a, b| {
            let pa = pct(self.covered_count(a), self.functions_in(a).len());
            let pb = pct(self.covered_count(b), self.functions_in(b).len());
            pb.partial_cmp(&pa)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| self.group_title(a).cmp(&self.group_title(b)))
        });
        ids
    }

    fn total_covered(&self) -> usize {
        self.c
            .functions
            .iter()
            .filter(|f| self.resolution(f).is_some())
            .count()
    }
}

fn pct(n: usize, d: usize) -> f64 {
    if d == 0 {
        0.0
    } else {
        100.0 * n as f64 / d as f64
    }
}

const BANNER: &str = "<!-- DO NOT EDIT BY HAND.\nGenerated by `cargo xtask gen` (xtask/src/main.rs) from docs-src/models/c-api.json\n(clang C inventory) joined with the syn-extracted Rust inventory and the curated,\nvalidated map in docs-src/models/parity.model.json. Run `cargo xtask check` to\nverify freshness; it fails if this file drifts or a model invariant is violated. -->\n";

fn legend_line(m: &ParityModel) -> String {
    let parts: Vec<String> = m
        .legend
        .iter()
        .map(|l| format!("`{}` {} = {}", l.marker, l.status, l.meaning))
        .collect();
    format!("Legend: {}.", parts.join(" · "))
}

fn provenance_line(c: &CApi) -> String {
    let heads: Vec<String> = c
        .provenance
        .headers
        .iter()
        .map(|h| {
            format!(
                "`{}` ({} fns, sha256 `{}`)",
                Path::new(&h.header)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&h.header),
                h.functions,
                &h.sha256[..12]
            )
        })
        .collect();
    format!(
        "C inventory provenance: {} via {}; headers: {}.",
        c.provenance.tool,
        c.provenance.method,
        heads.join(", ")
    )
}

fn render_port_parity(ctx: &Ctx) -> String {
    let mut s = String::new();
    s.push_str(BANNER);
    s.push('\n');
    s.push_str("# ncurses C public API \u{2192} ncurses-native port-parity matrix\n\n");
    s.push_str(
        "A 1:1 completeness catalog of the ncurses public C API surface against its\n\
         ncurses-native counterpart, grouped by ncurses man-page functional cluster.\n\
         There is no ncurses C source in this repository; the C inventory is the public\n\
         declaration surface a curses-compatible library must implement, extracted from\n\
         the installed headers by clang.\n\n",
    );
    s.push_str(&provenance_line(&ctx.c));
    s.push_str("\n\n");
    s.push_str(&legend_line(&ctx.m));
    s.push_str("\n\n");

    // Summary
    let total = ctx.c.count;
    let covered = ctx.total_covered();
    let mut by_status: BTreeMap<String, usize> = BTreeMap::new();
    for f in &ctx.c.functions {
        if let Some(r) = ctx.resolution(f) {
            *by_status.entry(r.status).or_default() += 1;
        }
    }
    s.push_str("## Summary\n\n");
    s.push_str(&format!("- **C public API functions:** {total} (clang)\n"));
    s.push_str(&format!(
        "- **Functions resolved (counterpart or classified):** {covered} / {total} ({:.1}%)\n",
        pct(covered, total)
    ));
    s.push_str(&format!(
        "- **API groups:** {} ({} fully resolved)\n",
        ctx.live_groups().len(),
        ctx.live_groups()
            .iter()
            .filter(|g| ctx.covered_count(g) == ctx.functions_in(g).len())
            .count()
    ));
    let order = ["full", "partial", "scaffold", "divergent", "deferred", "n_a"];
    let mut breakdown = Vec::new();
    for st in order {
        if let Some(n) = by_status.get(st) {
            breakdown.push(format!("{st} {n}"));
        }
    }
    s.push_str(&format!("- **Resolutions by status:** {}\n\n", breakdown.join(", ")));

    // Table
    s.push_str("## Groups\n\n");
    s.push_str("| ncurses API group | C fns | resolved | parity % | role | ncurses-native modules | status |\n");
    s.push_str("|---|---:|---:|---:|---|---|---|\n");
    for gid in ctx.live_groups() {
        let fns = ctx.functions_in(&gid);
        let cov = ctx.covered_count(&gid);
        // modules + markers present
        let mut modules: BTreeSet<String> = BTreeSet::new();
        let mut markers: Vec<(u8, String)> = Vec::new();
        let mut seen_status: BTreeSet<String> = BTreeSet::new();
        for f in &fns {
            if let Some(r) = ctx.resolution(f) {
                if let Some(rust) = &r.rust {
                    if let Some((m, _)) = rust.split_once("::") {
                        modules.insert(m.to_string());
                    }
                }
                if seen_status.insert(r.status.clone()) {
                    markers.push((status_rank(&r.status), marker_for(&r.status, &ctx.m.legend)));
                }
            }
        }
        markers.sort_by_key(|(r, _)| *r);
        let marker_cell = if markers.is_empty() {
            marker_for("none", &ctx.m.legend)
        } else {
            markers.iter().map(|(_, m)| m.clone()).collect::<Vec<_>>().join(" ")
        };
        let mod_cell = if modules.is_empty() {
            "\u{2014}".to_string()
        } else {
            modules
                .into_iter()
                .map(|m| format!("`{m}`"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        s.push_str(&format!(
            "| {} | {} | {} | {:.1} | {} | {} | {} |\n",
            ctx.group_title(&gid),
            fns.len(),
            cov,
            pct(cov, fns.len()),
            ctx.group_role(&gid),
            mod_cell,
            marker_cell,
        ));
    }
    s.push('\n');

    s.push_str("## What a counterpart means here\n\n");
    s.push_str(&ctx.m.coverage_definition);
    s.push_str("\n\nA counterpart is not a claim of equivalence. It means some observable byte\n");
    s.push_str("behavior of that ncurses function is reconstructed by the named Rust symbol\n");
    s.push_str("and, for `partial`/`divergent` rows, pinned by an oracle receipt under\n");
    s.push_str("`reports/oracle/` (see `docs/generated/claim-index.md`). The per-function\n");
    s.push_str("breakdown is in `docs/generated/port-parity-functions.md`.\n");
    s
}

fn render_functions(ctx: &Ctx) -> String {
    let mut s = String::new();
    s.push_str(BANNER);
    s.push('\n');
    s.push_str("# ncurses C \u{2192} ncurses-native per-function parity (gap view)\n\n");
    s.push_str(&provenance_line(&ctx.c));
    s.push_str("\n\n");

    s.push_str("## How to read this (and what the percentage is NOT)\n\n");
    s.push_str(
        "The percentage is a strict ratio: C functions that are *resolved* divided by all\n\
         C public API functions. Resolved means either a reconstructed byte producer\n\
         (full/partial/scaffold/divergent) or an evidence-backed non-output classification\n\
         (deferred = proven to emit no immediate bytes; n_a = a pure query with no\n\
         terminal-output contract). It is **not** a claim that a counterpart implements the\n\
         C function's signature, return values, or window semantics -- ncurses-native is a\n\
         byte-output reconstruction, not a C API. Credit is withheld from every function\n\
         that is neither reconstructed nor classified with evidence.\n\n",
    );
    let total = ctx.c.count;
    let covered = ctx.total_covered();
    let gaps = total - covered;
    s.push_str(&format!(
        "**Overall: {covered} / {total} C functions are resolved ({:.1}%). The other {gaps} are gaps.**\n\n",
        pct(covered, total)
    ));
    s.push_str(&legend_line(&ctx.m));
    s.push_str("\n\n");

    // Per-group coverage table (all live groups)
    s.push_str("## Per-group coverage\n\n");
    s.push_str("| ncurses API group | fns | resolved | gap | parity % |\n");
    s.push_str("|---|---:|---:|---:|---:|\n");
    for gid in ctx.live_groups() {
        let fns = ctx.functions_in(&gid);
        let cov = ctx.covered_count(&gid);
        s.push_str(&format!(
            "| {} | {} | {} | {} | {:.1} |\n",
            ctx.group_title(&gid),
            fns.len(),
            cov,
            fns.len() - cov,
            pct(cov, fns.len())
        ));
    }
    s.push('\n');

    // Detailed lists for groups with >=1 resolution.
    s.push_str("## Groups with resolutions \u{2014} function by function\n\n");
    s.push_str(
        "Every function in a group that has at least one resolution is listed; gaps are\n\
         shown explicitly. Groups with zero resolutions are omitted (see the table above\n\
         for their totals).\n\n",
    );
    for gid in ctx.live_groups() {
        if ctx.covered_count(&gid) == 0 {
            continue;
        }
        let fns = ctx.functions_in(&gid);
        let cov = ctx.covered_count(&gid);
        s.push_str(&format!(
            "### {} \u{2014} {}/{} ({:.1}%)\n\n",
            ctx.group_title(&gid),
            cov,
            fns.len(),
            pct(cov, fns.len())
        ));
        for f in &fns {
            if let Some(r) = ctx.resolution(f) {
                let marker = marker_for(&r.status, &ctx.m.legend);
                let courts = if r.courts.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", r.courts.join(", "))
                };
                let note = if r.note.is_empty() {
                    String::new()
                } else {
                    format!(" \u{2014} {}", r.note)
                };
                let target = match &r.rust {
                    Some(rust) => format!(" \u{2192} `{rust}`"),
                    None => String::new(),
                };
                s.push_str(&format!(
                    "- `{marker}` `{f}` ({}){target}{courts}{note}\n",
                    r.status
                ));
            } else {
                s.push_str(&format!("- `.` `{f}` (gap)\n"));
            }
        }
        s.push('\n');
    }
    s
}

fn render_gap_functions(ctx: &Ctx) -> String {
    // The complete per-function gap inventory: every C function whose native
    // counterpart is anything other than `full` is a gap of some degree. Each row
    // is a one-line diff (ncurses fn -> native status / counterpart / court).
    let gap_kind = |status: &str| -> &'static str {
        match status {
            "partial" => "behavioural (some behaviour reconstructed, not all)",
            "scaffold" => "stand-in (byte producer not pinned to its own court)",
            "divergent" => "measured byte divergence vs the in-repo oracle",
            "deferred" => "no immediate output (effect realised at refresh)",
            "n_a" => "no terminal-output contract (out of byte-output scope)",
            "none" => "unresolved (no counterpart at all)",
            _ => "unknown",
        }
    };
    let mut s = String::new();
    s.push_str(BANNER);
    s.push('\n');
    s.push_str("# Per-function gap ledger (ncurses C \u{2192} ncurses-native)\n\n");
    s.push_str(&provenance_line(&ctx.c));
    s.push_str("\n\n");
    s.push_str(
        "Every public ncurses C function whose native status is not `full` is listed here as a\n\
         gap, with its kind. `full` functions (no gap) are omitted. This is the machine-checked,\n\
         freshness-gated function-level half of the forensic gap ledger; the behavioural,\n\
         semantic, byte-level, and C\u{2192}Rust porting gap classes are in `docs/gap-ledger.md`.\n\n",
    );
    let total = ctx.c.count;
    let mut by_status: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for f in &ctx.c.functions {
        let st = ctx.resolution(f).map(|r| r.status).unwrap_or_else(|| "none".into());
        by_status.entry(st).or_default().push(f.clone());
    }
    let full = by_status.get("full").map(|v| v.len()).unwrap_or(0);
    s.push_str(&format!(
        "**{} / {} functions are gaps** (only {} are `full`). Gap kinds:\n\n",
        total - full,
        total,
        full
    ));
    for st in ["divergent", "scaffold", "partial", "deferred", "n_a", "none"] {
        if let Some(v) = by_status.get(st) {
            s.push_str(&format!("- **{}** ({}): {}\n", st, v.len(), gap_kind(st)));
        }
    }
    s.push('\n');
    // Sharpest gaps first: divergent, then scaffold, then the rest by status.
    for st in ["divergent", "scaffold", "partial", "deferred", "n_a", "none"] {
        let Some(fns) = by_status.get(st) else { continue };
        if st == "full" {
            continue;
        }
        s.push_str(&format!("## {} \u{2014} {} ({} functions)\n\n", st, gap_kind(st), fns.len()));
        s.push_str("| ncurses fn | group | native counterpart | court(s) | gap note |\n");
        s.push_str("|---|---|---|---|---|\n");
        for f in fns {
            let gid = ctx.group_of.get(f).cloned().unwrap_or_default();
            let r = ctx.resolution(f);
            let rust = r
                .as_ref()
                .and_then(|r| r.rust.clone())
                .map(|x| format!("`{x}`"))
                .unwrap_or_else(|| "\u{2014}".into());
            let courts = r
                .as_ref()
                .map(|r| r.courts.join(", "))
                .filter(|c| !c.is_empty())
                .unwrap_or_else(|| "\u{2014}".into());
            let note = r.as_ref().map(|r| r.note.clone()).unwrap_or_default();
            let note = note.replace('|', "\\|");
            s.push_str(&format!(
                "| `{f}` | {} | {rust} | {courts} | {note} |\n",
                ctx.group_title(&gid)
            ));
        }
        s.push('\n');
    }
    s
}

fn render_claim_index(ctx: &Ctx) -> String {
    let mut s = String::new();
    s.push_str(BANNER);
    s.push('\n');
    s.push_str("# Claim index \u{2014} courts, receipts, and the functions that cite them\n\n");
    s.push_str(
        "Each oracle court has a receipt under `reports/oracle/<id>.json` with the ncurses\n\
         version, TERM, terminfo hash, locale, geometry, and both byte hashes. A counterpart\n\
         marked `partial` or `divergent` must cite at least one court; this index is the\n\
         join, and `cargo xtask check` fails if any cited receipt is missing.\n\n",
    );

    // court -> functions (counterparts and classifications both cite courts)
    let mut court_users: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (f, cp) in &ctx.m.counterparts {
        for court in &cp.courts {
            court_users.entry(court.clone()).or_default().push(f.clone());
        }
    }
    for (f, cl) in &ctx.m.classifications {
        for court in &cl.courts {
            court_users.entry(court.clone()).or_default().push(f.clone());
        }
    }
    s.push_str("| court | verdict | cited by |\n");
    s.push_str("|---|---|---|\n");
    for (court, users) in &court_users {
        let verdict = receipt_verdict(&ctx.root, court).unwrap_or_else(|| "MISSING".into());
        let mut u = users.clone();
        u.sort();
        u.dedup();
        s.push_str(&format!(
            "| `{court}` | {verdict} | {} |\n",
            u.iter().map(|x| format!("`{x}`")).collect::<Vec<_>>().join(", ")
        ));
    }
    s.push('\n');
    s
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

fn validate(ctx: &Ctx, rust: &BTreeSet<String>) -> Vec<String> {
    let mut errs = Vec::new();
    let cset: BTreeSet<&String> = ctx.c.functions.iter().collect();
    for (f, cp) in &ctx.m.counterparts {
        if !cset.contains(f) {
            errs.push(format!("counterpart C function `{f}` is not in the clang C inventory"));
        }
        if !rust.contains(&cp.rust) {
            errs.push(format!(
                "counterpart `{f}` -> `{}` is not a pub symbol in the Rust inventory",
                cp.rust
            ));
        }
        let needs_court =
            cp.status == "full" || cp.status == "partial" || cp.status == "divergent";
        if needs_court && cp.courts.is_empty() {
            errs.push(format!(
                "counterpart `{f}` is `{}` but cites no court",
                cp.status
            ));
        }
        for court in &cp.courts {
            if !receipt_exists(&ctx.root, court) {
                errs.push(format!(
                    "counterpart `{f}` cites court `{court}` but reports/oracle/{court}.json is missing"
                ));
            }
        }
        if !ctx.m.legend.iter().any(|l| l.status == cp.status) {
            errs.push(format!("counterpart `{f}` has unknown status `{}`", cp.status));
        }
    }
    // classifications: non-output resolutions (deferred / n_a).
    for (f, cl) in &ctx.m.classifications {
        if !cset.contains(f) {
            errs.push(format!("classification C function `{f}` is not in the clang C inventory"));
        }
        if ctx.m.counterparts.contains_key(f) {
            errs.push(format!("`{f}` is both a counterpart and a classification"));
        }
        if !matches!(cl.status.as_str(), "deferred" | "n_a" | "unsupported") {
            errs.push(format!(
                "classification `{f}` has status `{}` (expected deferred/n_a/unsupported)",
                cl.status
            ));
        }
        if cl.status == "deferred" && cl.courts.is_empty() {
            errs.push(format!("classification `{f}` is deferred but cites no court"));
        }
        for court in &cl.courts {
            if !receipt_exists(&ctx.root, court) {
                errs.push(format!(
                    "classification `{f}` cites court `{court}` but reports/oracle/{court}.json is missing"
                ));
            }
        }
        if !ctx.m.legend.iter().any(|l| l.status == cl.status) {
            errs.push(format!("classification `{f}` has unknown status `{}`", cl.status));
        }
    }
    if ctx.c.functions.len() != ctx.c.count {
        errs.push(format!(
            "c-api.json count {} != functions length {}",
            ctx.c.count,
            ctx.c.functions.len()
        ));
    }
    // every function classifies (sanity); function_header coverage
    for f in &ctx.c.functions {
        if !ctx.c.function_header.contains_key(f) {
            errs.push(format!("c-api.json function `{f}` missing header attribution"));
        }
    }
    errs
}

// ---------------------------------------------------------------------------
// Codegen: terminfo capability-name tables -> src/terminfo/caps.rs
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CapsModel {
    provenance: CapsProvenance,
    #[serde(rename = "bool")]
    booleans: Vec<String>,
    num: Vec<String>,
    #[serde(rename = "str")]
    strings: Vec<String>,
    bool_codes: Vec<String>,
    num_codes: Vec<String>,
    str_codes: Vec<String>,
}

#[derive(Deserialize)]
struct CapsProvenance {
    source: String,
    method: String,
}

fn render_caps_rs(root: &Path) -> String {
    let m: CapsModel = serde_json::from_str(
        &std::fs::read_to_string(root.join("docs-src/models/terminfo-caps.json"))
            .expect("read terminfo-caps.json"),
    )
    .expect("parse terminfo-caps.json");
    let arr = |name: &str, items: &[String]| -> String {
        let mut s = format!(
            "/// {} terminfo capability short-names, in index order.\npub const {}: &[&str] = &[\n",
            items.len(),
            name
        );
        for chunk in items.chunks(8) {
            s.push_str("    ");
            for it in chunk {
                s.push_str(&format!("{:?}, ", it));
            }
            s.push('\n');
        }
        s.push_str("];\n");
        s
    };
    let mut s = String::new();
    s.push_str("// DO NOT EDIT BY HAND.\n");
    s.push_str("// Generated by `cargo xtask gen` from docs-src/models/terminfo-caps.json\n");
    s.push_str(&format!("// (extracted from {} via {}).\n", m.provenance.source, m.provenance.method));
    s.push_str("// `cargo xtask check` fails if this drifts from the model.\n\n");
    s.push_str("//! Canonical terminfo capability-name tables. The index of a name here is the\n");
    s.push_str("//! index it occupies in a compiled terminfo entry and in tigetflag/tigetnum/\n");
    s.push_str("//! tigetstr -- the authoritative order taken from ncurses' own name arrays.\n\n");
    s.push_str(&arr("BOOL_NAMES", &m.booleans));
    s.push('\n');
    s.push_str(&arr("NUM_NAMES", &m.num));
    s.push('\n');
    s.push_str(&arr("STR_NAMES", &m.strings));
    s.push('\n');
    s.push_str("// Termcap two-letter codes, parallel to the *_NAMES tables by index\n");
    s.push_str("// (empty string = the cap has no termcap code).\n\n");
    s.push_str(&arr("BOOL_CODES", &m.bool_codes));
    s.push('\n');
    s.push_str(&arr("NUM_CODES", &m.num_codes));
    s.push('\n');
    s.push_str(&arr("STR_CODES", &m.str_codes));
    s
}

// ---------------------------------------------------------------------------
// Codegen: key tables -> src/input/keys.rs
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct KeysModel {
    provenance: CapsProvenance,
    code_names: BTreeMap<String, String>,
    cap_codes: BTreeMap<String, i64>,
}

fn render_keys_rs(root: &Path) -> String {
    let m: KeysModel = serde_json::from_str(
        &std::fs::read_to_string(root.join("docs-src/models/keys.json")).expect("read keys.json"),
    )
    .expect("parse keys.json");
    let mut s = String::new();
    s.push_str("// DO NOT EDIT BY HAND.\n");
    s.push_str("// Generated by `cargo xtask gen` from docs-src/models/keys.json\n");
    s.push_str(&format!("// (extracted from {} via {}).\n\n", m.provenance.source, m.provenance.method));
    s.push_str("//! Key tables: KEY_* code -> name (for keyname), and terminfo key-cap name\n");
    s.push_str("//! -> KEY_* code (for key_defined/has_key). Authoritative order from ncurses.\n\n");
    // code -> name, sorted numerically
    let mut codes: Vec<(i64, &String)> = m
        .code_names
        .iter()
        .map(|(k, v)| (k.parse::<i64>().unwrap(), v))
        .collect();
    codes.sort();
    s.push_str(&format!(
        "/// {} KEY_* code -> name pairs, ascending by code.\npub const CODE_NAMES: &[(i32, &str)] = &[\n",
        codes.len()
    ));
    for (code, name) in &codes {
        s.push_str(&format!("    ({code}, {name:?}),\n"));
    }
    s.push_str("];\n\n");
    s.push_str(&format!(
        "/// {} terminfo key-cap name -> KEY_* code pairs.\npub const CAP_CODES: &[(&str, i32)] = &[\n",
        m.cap_codes.len()
    ));
    for (cap, code) in &m.cap_codes {
        s.push_str(&format!("    ({cap:?}, {code}),\n"));
    }
    s.push_str("];\n");
    s
}

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

fn build_outputs(root: &Path) -> (Ctx, BTreeSet<String>, Vec<(PathBuf, String)>) {
    let ctx = Ctx::load(root);
    let rust = rust_inventory(&root.join("src"));
    let gen = root.join("docs/generated");
    let outs = vec![
        (gen.join("port-parity.md"), render_port_parity(&ctx)),
        (gen.join("port-parity-functions.md"), render_functions(&ctx)),
        (gen.join("claim-index.md"), render_claim_index(&ctx)),
        (gen.join("gap-ledger-functions.md"), render_gap_functions(&ctx)),
        (root.join("src/terminfo/caps.rs"), render_caps_rs(root)),
        (root.join("src/input/keys.rs"), render_keys_rs(root)),
    ];
    (ctx, rust, outs)
}

fn main() {
    let cmd = std::env::args().nth(1).unwrap_or_default();
    let root = repo_root();
    let (ctx, rust, outs) = build_outputs(&root);

    let errs = validate(&ctx, &rust);

    match cmd.as_str() {
        "gen" => {
            if !errs.is_empty() {
                eprintln!("refusing to generate: model invariants violated:");
                for e in &errs {
                    eprintln!("  - {e}");
                }
                exit(1);
            }
            for (p, c) in &outs {
                if let Some(dir) = p.parent() {
                    std::fs::create_dir_all(dir).ok();
                }
                std::fs::write(p, c).unwrap_or_else(|e| panic!("write {}: {e}", p.display()));
                println!("wrote {}", p.strip_prefix(&root).unwrap_or(p).display());
            }
        }
        "check" => {
            let mut failed = false;
            if !errs.is_empty() {
                failed = true;
                eprintln!("model invariant violations:");
                for e in &errs {
                    eprintln!("  - {e}");
                }
            }
            for (p, c) in &outs {
                let on_disk = std::fs::read_to_string(p).unwrap_or_default();
                if on_disk != *c {
                    failed = true;
                    eprintln!(
                        "STALE: {} differs from freshly generated output; run `cargo xtask gen`",
                        p.strip_prefix(&root).unwrap_or(p).display()
                    );
                }
            }
            if failed {
                exit(1);
            }
            println!("freshness OK: generated docs, codegen, and model invariants are current");
        }
        "oracle" => {
            // Re-run the oracle courts (needs a live ncurses + clang/cc + python3).
            // Regenerates reports/oracle/*.json; run `gen` afterwards if statuses move.
            let status = std::process::Command::new("python3")
                .arg(root.join("tools/oracle-runner/run_oracle.py"))
                .status();
            match status {
                Ok(s) if s.success() => {}
                Ok(s) => exit(s.code().unwrap_or(1)),
                Err(e) => {
                    eprintln!("failed to launch oracle runner: {e}");
                    exit(1);
                }
            }
        }
        other => {
            eprintln!("usage: cargo xtask <gen|check|oracle> (got {other:?})");
            exit(2);
        }
    }
}
