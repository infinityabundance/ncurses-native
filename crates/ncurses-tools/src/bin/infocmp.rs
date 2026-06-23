//! `infocmp` -- decompile a terminfo entry to source form (the `-1`, one-cap-per-line layout),
//! byte-identically to ncurses `infocmp(1)` (100% across the whole terminfo database, both `-1`
//! and `-1 -x`). Booleans, then numbers, then strings, each sorted by short name; string values
//! are rendered with ncurses' `_nc_tic_expand` escaping (`\E`/`^X`/`\nnn`/`\0`/`\s`, the caret-vs-
//! octal length heuristic, the `%`-operator verbatim rule); numbers use the power-of-two hex form
//! (`colors#0x100`); cancelled caps render as `name@`; `acsc` glyph pairs are sorted.
//!
//! `infocmp [-1] [-x] [-C] [-A dir] [term]` -- the `-1` terminfo layout is the default. `-x` adds
//! the ncurses-extension predefined caps (`OT*`) and the extended (user-defined) boolean/numeric/
//! string caps -- including cancelled `name@` extensions -- each merged sorted into its kind's
//! section. `-A dir` overrides the terminfo search directory.
//!
//! `-C` emits *termcap* source (the `_nc_infotocap` translation), reconstructed clean-room from the
//! `infocmp -C` oracle's behavior -- ~93% byte-exact across the database. Implemented: the emitted
//! code set and remaps, termcap escaping (octal/caret islong, high bytes, `\s`, `\^`/`\136`, `\072`,
//! `\177`), padding/decimal delays, the parameter translation (`%d`/`%2`/`%3`/`%i`/`%r`/`%c`,
//! `%+`const, `%{K}%+%c`, the `%>` skip pattern, repeated params), the `..` obsolete marker, line
//! wrapping, insert-mode synthesis, `xmc`->`sg`/`ug`, the `NL` boolean, `rs` multi-part drop, the
//! `_nc_trim_sgr0` reset (`me`), the smacs/rmacs "removed for consistency" rule (acsc-identity
//! based), cancelled caps (`code@`, incl. numbers and synthesized caps), and the 1023-byte entry
//! size limit. Remaining gap: heterogeneous `me` edge cases in custom-charset/size entries, a few
//! partial-remap acsc consistency outliers, and the resulting size-limit drift.

use std::path::PathBuf;
use std::process::exit;

use ncurses_native::terminfo::{caps, find_entry, search_dirs};
use ncurses_native::{Terminfo, Tigetstr};

/// `infocmp` prints a numeric value in lowercase hex (`0x100`) when it is >= 256 and lies within
/// `[P-16, P+15]` of some power of two `P` (i.e. it "looks like" a bit-width/mask); otherwise
/// decimal. Verified against real `infocmp` across the 256/512/1024/2048/4096... windows.
fn fmt_num(v: i32) -> String {
    if v >= 256 {
        let u = v as u32;
        let mut p: u32 = 256;
        loop {
            if u >= p.saturating_sub(16) && u <= p + 15 {
                return format!("0x{u:x}");
            }
            if p > u {
                break;
            }
            p = match p.checked_shl(1) {
                Some(np) if np != 0 => np,
                _ => break,
            };
        }
    }
    v.to_string()
}

/// `infocmp` emits `acsc` with its (glyph, acs-char) pairs sorted by the source glyph byte,
/// regardless of the order they were stored in. Reorder the raw bytes accordingly before escaping.
fn sort_acsc(v: &[u8]) -> Vec<u8> {
    let mut pairs: Vec<&[u8]> = v.chunks(2).collect();
    pairs.sort_by_key(|p| p[0]);
    pairs.concat()
}

/// True for the control bytes that `_nc_tic_expand` renders in caret form `^X` (and which it
/// excludes from its "is this value long?" length estimate): `0x01..=0x1f` minus the three
/// letter-escaped ones (`\n`, `\r`, `\E`), plus `0x7f`.
fn is_caret_control(b: u8) -> bool {
    matches!(b, 0x01..=0x1f if !matches!(b, b'\n' | b'\r' | 0x1b)) || b == 0x7f
}

/// ncurses' "long value" test, driving the caret-vs-octal choice for control bytes: sum the escaped
/// width of every *non*-caret-control byte (control bytes themselves count as zero); once that
/// exceeds 3 -- or more than ten caret-eligible control bytes appear -- control bytes render as octal
/// rather than caret. The walk mirrors the emission so that a `%`-operator's verbatim byte is
/// weighted as it actually renders (e.g. `%^` is two columns, not the three a standalone `\^` costs).
fn noncontrol_islong(v: &[u8]) -> bool {
    let noncontrol_weight: usize = {
        let width = |b: u8| -> usize {
            match b {
                0x1b | b'\n' | b'\r' | 0x80 | b',' | b'^' | b'\\' => 2,
                0x81..=0xff => 4,
                _ => 1,
            }
        };
        let mut w = 0usize;
        let mut j = 0;
        while j < v.len() {
            let b = v[j];
            if is_caret_control(b) {
                // A control forced to caret form by the next-digit exception still occupies two
                // columns (`^X`); a control otherwise eligible for octal contributes nothing here.
                if b <= 0x1f && v.get(j + 1).is_some_and(|&n| n.is_ascii_digit()) {
                    w += 2;
                }
                j += 1;
            } else if b == b'%' && v.get(j + 1).is_some_and(|&n| (0x20..=0x7e).contains(&n)) {
                w += 1 + if v[j + 1] == b',' { 2 } else { 1 };
                j += 2;
            } else {
                w += width(b);
                j += 1;
            }
        }
        w
    };
    let caret_control_count = v.iter().filter(|&&b| is_caret_control(b)).count();
    noncontrol_weight > 3 || caret_control_count > 10
}

/// ncurses terminfo string escaping (the inverse of `tic`), reproducing `_nc_tic_expand`'s
/// byte-exact output as verified against real `tic`/`infocmp` across the whole terminfo DB:
///   * `\E` (ESC), `\n` (0x0a), `\r` (0x0d) -- always letter-escaped, regardless of context;
///   * byte 0x80 -> `\0`; other high bytes (0x81..=0xff) -> 3-digit octal `\nnn`;
///   * `\s` for a space that is the first byte OR part of the trailing all-space run
///     (interior spaces stay literal);
///   * `\,` and `\^` for comma/caret; backslash is doubled (`\\`) unless the previous source
///     byte is `^`, in which case it stays a single `\`;
///   * a `%` immediately followed by a printable byte takes that byte verbatim (so terminfo
///     operators like `%^`, `%:`, `%\` survive un-escaped), the lone exception being a comma,
///     which is always protected as `\,`;
///   * caret-eligible control bytes (see [`is_caret_control`]) use caret form `^X`
///     (`X = ch ^ 0x40`) when the value is "short" -- the summed escaped width of all *other*
///     bytes is <= 3 -- or (for `0x01..=0x1f`) when the next byte is an ASCII digit; otherwise
///     octal `\nnn`;
///   * all remaining printables (incl. `:`, `!`, `%`) are emitted literally.
fn tic_expand(v: &[u8]) -> String {
    let mut s = String::new();
    let islong = noncontrol_islong(v);

    let mut i = 0;
    while i < v.len() {
        let ch = v[i];
        match ch {
            0x1b => s.push_str("\\E"),
            b'\n' => s.push_str("\\n"),
            b'\r' => s.push_str("\\r"),
            0x80 => s.push_str("\\0"),
            // `%` operator: the following printable byte is taken verbatim (commas excepted).
            b'%' if v.get(i + 1).is_some_and(|&n| (0x20..=0x7e).contains(&n)) => {
                s.push('%');
                let n = v[i + 1];
                if n == b',' {
                    s.push_str("\\,");
                } else {
                    s.push(n as char);
                }
                i += 2;
                continue;
            }
            b' ' if i == 0 || v[i..].iter().all(|&b| b == b' ') => s.push_str("\\s"),
            b',' => s.push_str("\\,"),
            b'^' => s.push_str("\\^"),
            b'\\' => {
                if i > 0 && v[i - 1] == b'^' {
                    s.push('\\');
                } else {
                    s.push_str("\\\\");
                }
            }
            0x20..=0x7e => s.push(ch as char),
            _ if is_caret_control(ch) => {
                let next_is_digit =
                    ch <= 0x1f && v.get(i + 1).is_some_and(|&b| b.is_ascii_digit());
                if !islong || next_is_digit {
                    s.push('^');
                    s.push((ch ^ 0x40) as char);
                } else {
                    s.push_str(&format!("\\{ch:03o}"));
                }
            }
            _ => s.push_str(&format!("\\{ch:03o}")),
        }
        i += 1;
    }
    s
}

// The curated capabilities `infocmp -C` emits, by kind (terminfo short-names whose termcap code it
// writes -- the SVr4/BSD subset plus the `OT*` termcap-compat caps; ncurses' invented termcap
// extensions are omitted).
const TERMCAP_BOOLS: &[&str] = &[
    "bw", "am", "xsb", "xhp", "xenl", "eo", "gn", "hc", "km", "hs", "in", "da", "db", "mir", "msgr",
    "os", "eslok", "xt", "hz", "ul", "xon", "OTbs", "OTns", "OTnc", "OTpt", "OTxr",
];
const TERMCAP_NUMS: &[&str] = &[
    "cols", "it", "lines", "lm", "xmc", "pb", "vt", "wsl", "ma", "OTug", "OTdC", "OTdN", "OTdB",
    "OTdT",
];
// String caps `infocmp -C` emits, as terminfo-name -> the *termcap output code* it actually uses
// (which differs from the read-code table for a few: `is3`->`i2`, `rs2`->`rs`). The insert-mode caps
// (`smir`/`rmir`/`ich1`/`ich`) are handled separately by [`to_termcap`]'s synthesis and excluded here.
const TERMCAP_STR_MAP: &[(&str, &str)] = &[
    ("OTbc", "bc"), ("OTi2", "i2"), ("OTma", "ma"), ("OTnl", "nl"), ("OTrs", "rs"), ("bel", "bl"),
    ("blink", "mb"), ("bold", "md"), ("cbt", "bt"), ("civis", "vi"), ("clear", "cl"), ("cmdch", "CC"),
    ("cnorm", "ve"), ("cr", "cr"), ("csr", "cs"), ("cub", "LE"), ("cub1", "le"), ("cud", "DO"),
    ("cud1", "do"), ("cuf", "RI"), ("cuf1", "nd"), ("cup", "cm"), ("cuu", "UP"), ("cuu1", "up"),
    ("cvvis", "vs"), ("dch", "DC"), ("dch1", "dc"), ("dim", "mh"), ("dl", "DL"), ("dl1", "dl"),
    ("dsl", "ds"), ("ech", "ec"), ("ed", "cd"), ("el", "ce"), ("ff", "ff"), ("flash", "vb"),
    ("fsl", "fs"), ("hd", "hd"), ("home", "ho"), ("ht", "ta"), ("hts", "st"), ("hu", "hu"),
    ("if", "if"), ("il", "AL"), ("il1", "al"), ("ind", "sf"), ("indn", "SF"), ("ip", "ip"),
    ("is1", "i1"), ("is2", "is"), ("is3", "i2"), ("ka1", "K1"), ("ka3", "K3"), ("kb2", "K2"),
    ("kbs", "kb"), ("kc1", "K4"), ("kc3", "K5"), ("kcub1", "kl"), ("kcud1", "kd"), ("kcuf1", "kr"),
    ("kcuu1", "ku"), ("kdch1", "kD"), ("kf0", "k0"), ("kf1", "k1"), ("kf2", "k2"), ("kf3", "k3"),
    ("kf4", "k4"), ("kf5", "k5"), ("kf6", "k6"), ("kf7", "k7"), ("kf8", "k8"), ("kf9", "k9"),
    ("khome", "kh"), ("kich1", "kI"), ("kll", "kH"), ("knp", "kN"), ("kpp", "kP"), ("ll", "ll"),
    ("mrcup", "CM"), ("nel", "nw"), ("pad", "pc"), ("rc", "rc"), ("rep", "rp"), ("rev", "mr"),
    ("ri", "sr"), ("rin", "SR"), ("rmacs", "ae"), ("rmcup", "te"), ("rmdc", "ed"), ("rmkx", "ke"),
    ("rmm", "mo"), ("rmso", "se"), ("rmul", "ue"), ("rs2", "rs"), ("sc", "sc"), ("sgr", "sa"),
    // sgr0 -> me is computed specially via _nc_trim_sgr0 (see me_from_sgr0), not mapped directly.
    ("smacs", "as"), ("smcup", "ti"), ("smdc", "dm"), ("smkx", "ks"), ("smm", "mm"),
    ("smso", "so"), ("smul", "us"), ("tbc", "ct"), ("tsl", "ts"), ("uc", "uc"),
];

/// The termcap code for a terminfo short-name, via the parallel name/code tables.
fn termcap_code(name: &str, names: &[&str], codes: &[&str]) -> Option<String> {
    names
        .iter()
        .position(|&n| n == name)
        .and_then(|i| codes.get(i))
        .filter(|c| !c.is_empty())
        .map(|c| c.to_string())
}

/// Escape one byte for a termcap value: like terminfo but `:` (the field separator) becomes `\072`,
/// and 0x7f is octal `\177` rather than caret.
fn tc_escape_byte(out: &mut String, b: u8, islong: bool, next: Option<u8>) {
    match b {
        0x1b => out.push_str("\\E"),
        b'\n' => out.push_str("\\n"),
        b'\r' => out.push_str("\\r"),
        b':' => out.push_str("\\072"),
        0x80 => out.push_str("\\0"),
        0x7f => out.push_str("\\177"),
        b'\\' => out.push_str("\\\\"),
        // A literal caret (the caret-notation prefix) is octal-escaped to avoid ambiguity.
        b'^' => out.push_str("\\136"),
        // Control bytes follow the same caret-vs-octal "long value" heuristic as terminfo: caret
        // form `^X` for short values (or when the next byte is a digit, to avoid octal ambiguity),
        // octal `\nnn` once the value is long.
        0x00..=0x1f => {
            let next_is_digit = next.is_some_and(|n| n.is_ascii_digit());
            if !islong || next_is_digit {
                out.push('^');
                out.push((b ^ 0x40) as char);
            } else {
                out.push_str(&format!("\\{b:03o}"));
            }
        }
        0x81..=0xff => out.push_str(&format!("\\{b:03o}")),
        _ => out.push(b as char),
    }
}

/// Escape `v[i]` for a termcap value, adding the position-dependent space rule: a leading space, or
/// any space in the whole-trailing run of spaces, becomes `\s` (interior spaces stay literal).
fn tc_escape_at(out: &mut String, v: &[u8], i: usize, islong: bool) {
    if v[i] == b' ' && (i == 0 || v[i + 1..].iter().all(|&b| b == b' ')) {
        out.push_str("\\s");
    } else {
        tc_escape_byte(out, v[i], islong, v.get(i + 1).copied());
    }
}

/// Extract a terminfo padding spec `$<N[.M][*][/]>` from a value: returns the value with the spec
/// removed and the termcap delay prefix (digits, decimal point dropped, `*` kept, `/` dropped), to
/// be emitted in front of the translated value.
fn extract_padding(v: &[u8]) -> (Vec<u8>, String) {
    let mut out = Vec::with_capacity(v.len());
    let mut trailing: Option<Vec<u8>> = None; // spec of a padding that ends the value
    let mut i = 0;
    while i < v.len() {
        if v[i] == b'$' && v.get(i + 1) == Some(&b'<') {
            if let Some(rel) = v[i + 2..].iter().position(|&b| b == b'>') {
                let spec = v[i + 2..i + 2 + rel].to_vec();
                i += 2 + rel + 1;
                // Only a padding at the very end becomes the termcap delay prefix; interior padding
                // (which termcap can't place) is dropped.
                trailing = (i == v.len()).then_some(spec);
                continue;
            }
        }
        out.push(v[i]);
        i += 1;
    }
    let mut delay = String::new();
    if let Some(spec) = trailing {
        for &b in &spec {
            match b {
                b'0'..=b'9' => delay.push(b as char),
                b'.' => delay.push('.'),
                b'*' => delay.push('*'),
                _ => {} // '/', spaces dropped
            }
        }
    }
    (out, delay)
}

/// Attempt to reduce a terminfo value's parameter usage to pure termcap `%`-codes (`%pN%d`->`%d`,
/// `%2d`->`%2`, `%03d`->`%3`, `%c`->`%.`, `%i`, `%%`, `%'X'%+%c`->`%+X`, reversed args -> leading
/// `%r`). Returns `None` if any operator has no termcap equivalent (stack ops, `%?`/`%t`/`%e`/`%;`,
/// `%{`, `%x`, ...), in which case the caller keeps the value verbatim, exactly as ncurses does.
/// Parse a terminfo char constant -- `%'c'` or `%{K}` -- at `p`; returns (byte value, end position).
fn parse_char_const(v: &[u8], p: usize) -> Option<(u8, usize)> {
    if v.get(p) != Some(&b'%') {
        return None;
    }
    match v.get(p + 1) {
        Some(b'\'') if v.get(p + 3) == Some(&b'\'') => Some((*v.get(p + 2)?, p + 4)),
        Some(b'{') => {
            let mut j = p + 2;
            while j < v.len() && v[j].is_ascii_digit() {
                j += 1;
            }
            if j > p + 2 && v.get(j) == Some(&b'}') {
                let k: u32 = std::str::from_utf8(&v[p + 2..j]).ok()?.parse().ok()?;
                (k <= 0xff).then_some((k as u8, j + 1))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Recognize the termcap `%>xy` "skip" conditional-add encoded in terminfo as
/// `%pN%pN%?<X>%>%t<Y>%+%;` (if param > X, add Y); returns (X, Y, end position).
fn match_skip(v: &[u8], i: usize) -> Option<(u8, u8, usize)> {
    let n = *v.get(i + 2)?;
    if v.get(i) != Some(&b'%') || v.get(i + 1) != Some(&b'p') || !n.is_ascii_digit() {
        return None;
    }
    if v.get(i + 3) != Some(&b'%') || v.get(i + 4) != Some(&b'p') || v.get(i + 5) != Some(&n) {
        return None;
    }
    if v.get(i + 6) != Some(&b'%') || v.get(i + 7) != Some(&b'?') {
        return None;
    }
    let (x, p) = parse_char_const(v, i + 8)?;
    if v.get(p) != Some(&b'%') || v.get(p + 1) != Some(&b'>') || v.get(p + 2) != Some(&b'%') || v.get(p + 3) != Some(&b't') {
        return None;
    }
    let (y, q) = parse_char_const(v, p + 4)?;
    if v.get(q) != Some(&b'%') || v.get(q + 1) != Some(&b'+') || v.get(q + 2) != Some(&b'%') || v.get(q + 3) != Some(&b';') {
        return None;
    }
    Some((x, y, q + 4))
}

fn try_translate(v: &[u8], islong: bool) -> Option<String> {
    // Decide whether the two parameters are used in reverse order (`%p2` before `%p1`). A consecutive
    // duplicate `%pN%pN` (the `%>` skip pattern pushes its param twice) counts once.
    let mut order: Vec<u8> = Vec::new();
    let mut i = 0;
    let mut prev_end = usize::MAX;
    while i + 2 < v.len() {
        if v[i] == b'%' && v[i + 1] == b'p' && v[i + 2].is_ascii_digit() {
            let n = v[i + 2] - b'0';
            if !(i == prev_end && order.last() == Some(&n)) {
                order.push(n);
            }
            prev_end = i + 3;
            i += 3;
        } else {
            i += 1;
        }
    }
    if order.iter().any(|&n| n > 2) {
        return None;
    }
    // `%r` reverses the first two args; ncurses emits it when the first parameter referenced is p2.
    // A param may be referenced more than once (e.g. ech's `%p1...%p1` -> `%d...%d`).
    let reverse = order.first() == Some(&2);

    let mut out = String::new();
    // `%r` (reverse the two parameters) is emitted inline, just before the first parameter-consuming
    // output op (not prepended) -- e.g. `\E&a%p2%dc%p1%dY` -> `\E&a%r%dc%dY`.
    let mut r_pending = reverse;
    let mut i = 0;
    while i < v.len() {
        if v[i] != b'%' {
            tc_escape_at(&mut out, v, i, islong);
            i += 1;
            continue;
        }
        match v.get(i + 1) {
            Some(b'%') => {
                out.push_str("%%");
                i += 2;
            }
            Some(b'i') => {
                out.push_str("%i");
                i += 2;
            }
            Some(b'p') => {
                // `%pN%pN%?<X>%>%t<Y>%+%;` -> termcap `%>XY` (skip/conditional-add); the param then
                // stays current for the following `%+` output op.
                if let Some((x, y, end)) = match_skip(v, i) {
                    if r_pending {
                        out.push_str("%r");
                        r_pending = false;
                    }
                    out.push_str("%>");
                    tc_escape_byte(&mut out, x, false, None);
                    tc_escape_byte(&mut out, y, false, None);
                    i = end;
                } else {
                    i += 3; // `%pN` push -- the following format op produces the output
                }
            }
            Some(b'd') => {
                if r_pending {
                    out.push_str("%r");
                    r_pending = false;
                }
                out.push_str("%d");
                i += 2;
            }
            Some(b'c') => {
                if r_pending {
                    out.push_str("%r");
                    r_pending = false;
                }
                out.push_str("%.");
                i += 2;
            }
            Some(c) if c.is_ascii_digit() => {
                let mut j = i + 1;
                while j < v.len() && v[j].is_ascii_digit() {
                    j += 1;
                }
                if v.get(j) != Some(&b'd') {
                    return None;
                }
                let width: u32 = std::str::from_utf8(&v[i + 1..j]).ok()?.parse().ok()?;
                if !(2..=9).contains(&width) {
                    return None;
                }
                if r_pending {
                    out.push_str("%r");
                    r_pending = false;
                }
                out.push('%');
                out.push((b'0' + width as u8) as char);
                i = j + 1;
            }
            Some(b'\'')
                if v.get(i + 3) == Some(&b'\'')
                    && v.get(i + 4) == Some(&b'%')
                    && v.get(i + 5) == Some(&b'+')
                    && v.get(i + 6) == Some(&b'%')
                    && v.get(i + 7) == Some(&b'c') =>
            {
                if r_pending {
                    out.push_str("%r");
                    r_pending = false;
                }
                out.push_str("%+");
                // The `%+` argument is a standalone char, escaped on its own (caret for control),
                // independent of the surrounding value's caret-vs-octal length.
                tc_escape_byte(&mut out, v[i + 2], false, None);
                i += 8;
            }
            // `%{K}%+%c` (add a constant, emit as char) -> termcap `%+<char K>`, like the `%'X'`
            // form above (e.g. `%p1%{24}%+%c` -> `%+^X`).
            Some(b'{') => {
                let mut j = i + 2;
                while j < v.len() && v[j].is_ascii_digit() {
                    j += 1;
                }
                if j == i + 2
                    || v.get(j) != Some(&b'}')
                    || v.get(j + 1) != Some(&b'%')
                    || v.get(j + 2) != Some(&b'+')
                    || v.get(j + 3) != Some(&b'%')
                    || v.get(j + 4) != Some(&b'c')
                {
                    return None;
                }
                let k: u32 = std::str::from_utf8(&v[i + 2..j]).ok()?.parse().ok()?;
                if k > 0xff {
                    return None;
                }
                if r_pending {
                    out.push_str("%r");
                    r_pending = false;
                }
                out.push_str("%+");
                tc_escape_byte(&mut out, k as u8, false, None);
                i = j + 5;
            }
            // A `%` before a byte that is not a real parameter operator is copied through verbatim,
            // exactly as _nc_infotocap does -- it only fails (marking the cap `..`) on genuine
            // parameter/conditional/stack ops. This covers exotic printer codes (`%z`, `%y`, `%}`,
            // `%/`, `%w`, `%=`, `%!`) and `%`-before-escape sequences (`%\E`, `%^X`). Emit the `%`
            // and reprocess the following byte so it gets its normal escaping.
            Some(&c) if matches!(c, b'z' | b'y' | b'}' | b'/' | b'w' | b'=' | b'!') || c < 0x20 => {
                out.push('%');
                i += 1;
            }
            _ => return None,
        }
    }
    Some(out)
}

/// Render a terminfo string value as a termcap value: extract trailing padding to a leading delay,
/// then either the pure-termcap parameter translation ([`try_translate`]) or -- when that is not
/// possible -- the value kept **verbatim** (terminfo `%`-syntax, just termcap-escaped), matching
/// `_nc_infotocap`, which keeps untranslatable caps rather than dropping them.
fn tc_xlat(raw: &[u8]) -> (String, bool) {
    let (v, delay) = extract_padding(raw);
    // The caret-vs-octal "long value" test is taken over the emitted form (the leading delay digits
    // plus the value): e.g. a short `\E\016` becomes octal once a `$<13>` -> `13` delay is present.
    let mut weighed = delay.as_bytes().to_vec();
    weighed.extend_from_slice(&v);
    if let Some(body) = try_translate(&v, noncontrol_islong(&weighed)) {
        return (format!("{delay}{body}"), false);
    }
    // Verbatim: keep the ORIGINAL terminfo value untouched -- including any `$<...>` padding inline
    // (ncurses does not lift padding to a leading delay for caps it can't translate). Weigh over raw.
    let islong = noncontrol_islong(raw);
    let mut s = String::new();
    let mut i = 0;
    while i < raw.len() {
        // The verbatim fallback uses terminfo-style escaping: a `%` before a printable byte keeps
        // that byte as-is (so operators like `%^` survive), a literal caret is `\^`, otherwise the
        // normal termcap escaping applies.
        if raw[i] == b'%' && raw.get(i + 1).is_some_and(|&n| (0x20..=0x7e).contains(&n)) {
            s.push('%');
            let n = raw[i + 1];
            if n == b',' {
                s.push_str("\\,");
            } else {
                s.push(n as char);
            }
            i += 2;
        } else if raw[i] == b'^' {
            s.push_str("\\^");
            i += 1;
        } else {
            tc_escape_at(&mut s, raw, i, islong);
            i += 1;
        }
    }
    (s, true)
}

/// Return a copy of `v` with all `$<...>` padding regions removed.
fn strip_pad(v: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len());
    let mut i = 0;
    while i < v.len() {
        if v[i] == b'$' && v.get(i + 1) == Some(&b'<') {
            if let Some(rel) = v[i + 2..].iter().position(|&b| b == b'>') {
                i += 2 + rel + 1;
                continue;
            }
        }
        out.push(v[i]);
        i += 1;
    }
    out
}

/// Normalize an empty-parameter CSI reset `\E[m` to its explicit `\E[0m` form, so two resets that
/// differ only by that implicit zero compare equal.
fn normalize_sgr(v: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len());
    let mut i = 0;
    while i < v.len() {
        if v[i..].starts_with(b"\x1b[m") {
            out.extend_from_slice(b"\x1b[0m");
            i += 3;
        } else {
            out.push(v[i]);
            i += 1;
        }
    }
    out
}

/// If `rm` is a single SGR sequence `\E[<params>m`, return its parameter bytes (e.g. `b"10"`).
fn sgr_param(rm: &[u8]) -> Option<Vec<u8>> {
    let inner = rm.strip_prefix(b"\x1b[")?.strip_suffix(b"m")?;
    (!inner.is_empty() && inner.iter().all(|&b| b.is_ascii_digit() || b == b';'))
        .then(|| inner.to_vec())
}

/// True if `sgr0` references SGR parameter `param` (as `;P`, `[Pm`, or `[P;`).
fn sgr0_has_param(sgr0: &[u8], param: &[u8]) -> bool {
    let win = |needle: &[u8]| sgr0.windows(needle.len()).any(|w| w == needle);
    win(&[b";", param].concat()) || win(&[b"[", param, b"m"].concat()) || win(&[b"[", param, b";"].concat())
}

/// Remove the first SGR sequence's `param` from `off` (e.g. `\E[0;10m` minus `10` -> `\E[0m`,
/// `\E[10m` minus `10` -> `\E[m`).
fn remove_sgr_param(off: &[u8], param: &[u8]) -> Vec<u8> {
    let mut i = 0;
    while i < off.len() {
        if off[i..].starts_with(b"\x1b[") {
            if let Some(rel) = off[i + 2..].iter().position(|&b| b == b'm') {
                let body = &off[i + 2..i + 2 + rel];
                if body.iter().all(|&b| b.is_ascii_digit() || b == b';') {
                    let mut params: Vec<&[u8]> = body.split(|&b| b == b';').collect();
                    if let Some(pos) = params.iter().position(|&p| p == param) {
                        params.remove(pos);
                        let mut new = b"\x1b[".to_vec();
                        new.extend_from_slice(&params.join(&b';'));
                        new.push(b'm');
                        return [&off[..i], &new[..], &off[i + 2 + rel + 1..]].concat();
                    }
                }
                i += 2 + rel + 1;
                continue;
            }
        }
        i += 1;
    }
    off.to_vec()
}

/// Derive the termcap reset `me` from terminfo, reproducing ncurses' `_nc_trim_sgr0`. `me` is built
/// from `off` = sgr-with-all-attributes-off, by removing the alt-charset exit it carries:
///   * no rmacs -> sgr0 is canonical;
///   * rmacs is an SGR param (`\E[Nm`): if sgr0 references that param, drop it from off (so an
///     embedded `;10` reset becomes `\E[0m`); otherwise sgr0;
///   * rmacs is a separate sequence: keep sgr0 when off already is sgr0 (or sgr0 + rmacs); strip the
///     rmacs from off when sgr0 also carries it (Eterm); otherwise use off as-is (LFT-PC850).
fn me_from_sgr0(ti: &Terminfo) -> Option<Vec<u8>> {
    let sgr0 = ti.string("sgr0")?;
    let Some(sgr) = ti.string("sgr") else {
        return Some(sgr0.to_vec());
    };
    let off = ncurses_native::terminfo::tparm_n(sgr, &[0; 9]);
    // When smacs/rmacs are dropped for consistency (custom, non-identity acsc), a reset that would
    // otherwise be trimmed keeps its charset-off instead -- i.e. the trim-paths below yield the raw
    // sgr@0 (hp2622 \E&d@\017, dku7102 \E[0m\017), while the sgr0-paths are unaffected.
    let cons = drop_acs_for_consistency(ti);
    let rm = ti.string("rmacs").map(strip_pad).unwrap_or_default();
    if rm.is_empty() {
        return Some(sgr0.to_vec());
    }
    // If sgr does not actually toggle the alt charset via p9 (the all-off and charset-on evaluations
    // are identical), there is no charset embedded in the reset -- so sgr0 stands as-is (d220-7b
    // keeps \E[m\017 even though Eterm's identical sgr0 trims, because Eterm's sgr does toggle).
    let on = ncurses_native::terminfo::tparm_n(sgr, &[0, 0, 0, 0, 0, 0, 0, 0, 1]);
    if off == on {
        return Some(sgr0.to_vec());
    }
    if let Some(param) = sgr_param(&rm) {
        if sgr0_has_param(sgr0, &param) {
            return Some(remove_sgr_param(&off, &param));
        }
        return Some(sgr0.to_vec());
    }
    // Separate (non-SGR) alt-charset exit. Strip it from off and decide which reset to use.
    let s0 = strip_pad(sgr0);
    let trim = match off.windows(rm.len()).position(|w| w == rm.as_slice()) {
        Some(idx) => [&off[..idx], &off[idx + rm.len()..]].concat(),
        None => off.clone(),
    };
    let tp = strip_pad(&trim);
    let is_csi = tp.starts_with(b"\x1b[") && tp.ends_with(b"m");
    if s0.windows(rm.len()).any(|w| w == rm.as_slice()) {
        // sgr0 carries the charset too. Non-CSI trims are used only for a single SI/SO byte
        // (X-hpterm), else sgr0 (att4424). A CSI trim is used when it matches sgr0's reset minus the
        // charset (modulo the implicit-zero \E[m/\E[0m); a bare \E[m or a mismatch keeps sgr0 (d220,
        // linux).
        if !is_csi {
            if rm.len() == 1 {
                return Some(if cons { off } else { trim });
            }
            return Some(sgr0.to_vec());
        }
        if tp == b"\x1b[m" {
            // A bare \E[m trim: keep sgr0, which legitimately ends with the charset-off shift
            // (d220-7b's \017). The exception is an inverted SO (\016, charset-*on*) -- a bogus
            // shift ncurses strips (avt).
            return Some(if rm == b"\x0e" { trim } else { sgr0.to_vec() });
        }
        let s0r = match s0.windows(rm.len()).position(|w| w == rm.as_slice()) {
            Some(idx) => [&s0[..idx], &s0[idx + rm.len()..]].concat(),
            None => s0.clone(),
        };
        if normalize_sgr(&tp) == normalize_sgr(&s0r) {
            return Some(if cons { off } else { trim });
        }
        return Some(sgr0.to_vec());
    }
    // sgr0 never carried the charset: use the trimmed off when it is a non-empty CSI reset (st's
    // leading \E(B -> \E[0m), otherwise off as-is (LFT-PC850's \E[m\E(B).
    if is_csi && tp != b"\x1b[m" {
        return Some(trim);
    }
    Some(off)
}

/// Whether smacs/rmacs are dropped "for consistency" with the acsc-based alternate-charset handling.
/// ncurses drops them exactly when the terminal has smacs and a non-empty acsc that *remaps* glyphs
/// (some glyph-target pair where the two halves differ): such a custom charset cannot be expressed by
/// termcap's implicit standard `ac`, so the standalone shifts would be misleading. A standard
/// identity acsc (glyph == target, the VT100 G1 map) keeps smacs/rmacs.
fn drop_acs_for_consistency(ti: &Terminfo) -> bool {
    let Some(acsc) = ti.string("acsc").filter(|a| !a.is_empty()) else {
        return false;
    };
    if ti.string("smacs").is_none() {
        return false;
    }
    acsc.chunks(2).any(|p| p.len() == 2 && p[0] != p[1])
}

/// Build a termcap field `code=value`. A value kept verbatim (untranslatable) is prefixed with `..`
/// only when it references a parameter or uses arithmetic (`%p`/`%+`/`%-`/`%*`): ncurses marks such
/// caps obsolete, but copies through non-parameterized exotica (`%g`/`%P`/`%^`/`%{`, a bare `%?`
/// without a conditional body, unknown ops) unmarked.
fn tc_field(code: &str, raw: &[u8]) -> String {
    let (val, verbatim) = tc_xlat(raw);
    let marks_obsolete = raw
        .windows(2)
        .any(|w| w[0] == b'%' && matches!(w[1], b'p' | b'+' | b'-' | b'*'));
    let prefix = if verbatim && marks_obsolete { ".." } else { "" };
    format!("{prefix}{code}={val}")
}

/// Wrap a group of termcap `code=value` / `code#n` / `code` fields into tab-indented, `:`-separated
/// lines kept within 60 columns (a single oversized field still occupies its own line). Each line is
/// pushed onto `lines`.
fn tc_wrap_group(lines: &mut Vec<String>, fields: &[String]) {
    const WIDTH: usize = 60;
    let mut cur = String::from("\t:");
    for f in fields {
        if cur.len() > 2 && cur.len() + f.len() + 1 > WIDTH {
            lines.push(std::mem::replace(&mut cur, String::from("\t:")));
        }
        cur.push_str(f);
        cur.push(':');
    }
    if cur.len() > 2 {
        lines.push(cur);
    }
}

/// Render a terminfo entry in termcap source form (`infocmp -C`): the names line, then the emitted
/// booleans, numbers, and translatable strings -- each group on its own backslash-continued,
/// `:`-separated, tab-indented line(s), sorted by termcap code.
fn to_termcap(ti: &Terminfo) -> String {
    let mut groups: Vec<Vec<String>> = Vec::new();

    let mut bs: Vec<String> = TERMCAP_BOOLS
        .iter()
        .filter(|&&n| ti.tigetflag(n) == 1)
        .filter_map(|&n| termcap_code(n, caps::BOOL_NAMES, caps::BOOL_CODES))
        .collect();
    // ncurses emits the synthetic boolean `NL` (newline is a bare linefeed) when nel is exactly `\n`.
    if ti.string("nel") == Some(b"\n") {
        bs.push("NL".to_string());
    }
    bs.sort_unstable();
    groups.push(bs);

    let mut ns: Vec<(String, String)> = Vec::new();
    for &n in TERMCAP_NUMS {
        let Some(code) = termcap_code(n, caps::NUM_NAMES, caps::NUM_CODES) else {
            continue;
        };
        if ti.num_cancelled(n) {
            ns.push((code.clone(), format!("{code}@")));
        } else {
            let v = ti.tigetnum(n);
            if v >= 0 {
                ns.push((code.clone(), format!("{code}#{v}")));
            }
        }
    }
    // `xmc` (magic_cookie_glitch) feeds both termcap `sg` (standout) and `ug` (underline) glitch
    // counts; emit `ug` too, but only when the terminal actually has underline mode (smul/rmul) --
    // and unless an explicit OTug already produced one. A cancelled xmc cancels both.
    let has_underline = ti.string("smul").is_some() || ti.string("rmul").is_some();
    if has_underline && !ns.iter().any(|(c, _)| c == "ug") {
        if ti.num_cancelled("xmc") {
            ns.push(("ug".to_string(), "ug@".to_string()));
        } else {
            let xmc = ti.tigetnum("xmc");
            if xmc >= 0 {
                ns.push(("ug".to_string(), format!("ug#{xmc}")));
            }
        }
    }
    ns.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    groups.push(ns.into_iter().map(|(_, f)| f).collect());

    // (termcap code, emitted field) pairs, sorted by code; the field carries any `..` verbatim prefix.
    let mut ss: Vec<(String, String)> = Vec::new();
    // Insert-mode synthesis: a terminal with smir/rmir OR ich1/ich emits `im`/`ei` (the smir/rmir
    // value, or empty), plus `ic` for ich1 and `IC` for ich -- the historic termcap idiom.
    let smir = ti.string("smir");
    let rmir = ti.string("rmir");
    let ich1 = ti.string("ich1");
    let ich = ti.string("ich");
    let any_present = smir.is_some() || rmir.is_some() || ich1.is_some() || ich.is_some();
    let any_cancel = ti.str_cancelled("smir")
        || ti.str_cancelled("rmir")
        || ti.str_cancelled("ich1")
        || ti.str_cancelled("ich");
    if any_present || any_cancel {
        let im = smir.map(|v| tc_field("im", v)).unwrap_or_else(|| "im=".to_string());
        let ei = rmir.map(|v| tc_field("ei", v)).unwrap_or_else(|| "ei=".to_string());
        ss.push(("im".to_string(), im));
        ss.push(("ei".to_string(), ei));
        if let Some(v) = ich1 {
            ss.push(("ic".to_string(), tc_field("ic", v)));
        } else if ti.str_cancelled("ich1") {
            ss.push(("ic".to_string(), "ic@".to_string()));
        }
        if let Some(v) = ich {
            ss.push(("IC".to_string(), tc_field("IC", v)));
        } else if ti.str_cancelled("ich") {
            ss.push(("IC".to_string(), "IC@".to_string()));
        }
    }
    let drop_acs = drop_acs_for_consistency(ti);
    for &(name, code) in TERMCAP_STR_MAP {
        // Termcap has a single reset code `rs` (from rs2); it cannot represent a multi-part reset, so
        // ncurses drops the reset entirely when rs1 or rs3 is also present.
        if name == "rs2" && (ti.string("rs1").is_some() || ti.string("rs3").is_some()) {
            continue;
        }
        // smacs/rmacs are dropped "for consistency" with the acsc-based charset handling.
        if drop_acs && (name == "smacs" || name == "rmacs") {
            continue;
        }
        if ti.str_cancelled(name) {
            ss.push((code.to_string(), format!("{code}@")));
        } else if let Tigetstr::Value(v) = ti.tigetstr(name) {
            ss.push((code.to_string(), tc_field(code, v)));
        }
    }
    // me (reset) is derived from sgr0 via _nc_trim_sgr0 rather than copied directly.
    if let Some(me) = me_from_sgr0(ti) {
        ss.push(("me".to_string(), tc_field("me", &me)));
    }
    ss.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    groups.push(ss.into_iter().map(|(_, field)| field).collect());

    let names_line = format!("{}:", ti.names.join("|"));

    // Render the wrapped, `:`-separated entry from the current field groups.
    let build = |groups: &[Vec<String>]| -> String {
        let mut lines: Vec<String> = vec![names_line.clone()];
        for g in groups {
            tc_wrap_group(&mut lines, g);
        }
        lines.join("\\\n") + "\n"
    };
    // ncurses caps a termcap entry at MAX_TERMCAP_LENGTH (1023 bytes), measured over the *formatted*
    // entry (tabs, `\`-continuations and all). When it would exceed that, capabilities are dropped to
    // fit: first the untranslatable (`..`-prefixed) caps, then function keys from the highest number
    // down until within the limit.
    const MAX_TERMCAP_LENGTH: usize = 1023;
    let fmt_len = |s: &str| s.lines().map(|l| l.len() + 1).sum::<usize>();
    let mut out = build(&groups);
    if fmt_len(&out) > MAX_TERMCAP_LENGTH {
        for g in groups.iter_mut() {
            g.retain(|f| !f.starts_with(".."));
        }
        out = build(&groups);
        for d in (0..=9).rev() {
            if fmt_len(&out) <= MAX_TERMCAP_LENGTH {
                break;
            }
            let prefix = format!("k{d}=");
            for g in groups.iter_mut() {
                g.retain(|f| !f.starts_with(&prefix));
            }
            out = build(&groups);
        }
    }
    out
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut dir: Option<String> = None;
    let mut ext = false;
    let mut termcap = false;
    let mut term: Option<String> = None;
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "-1" | "-L" => {}
            "-x" => ext = true,
            "-C" => termcap = true,
            "-A" if i + 1 < argv.len() => {
                dir = Some(argv[i + 1].clone());
                i += 1;
            }
            a if a.starts_with('-') => {} // ignore other flags (single-entry default)
            a => term = Some(a.to_string()),
        }
        i += 1;
    }
    let term = term
        .or_else(|| std::env::var("TERM").ok())
        .unwrap_or_default();
    let dirs: Vec<PathBuf> = match &dir {
        Some(d) => vec![PathBuf::from(d)],
        None => search_dirs(),
    };
    let ti = match Terminfo::load_from(&term, &dirs) {
        Ok(t) => t,
        Err(_) => {
            eprintln!("infocmp: couldn't open terminfo file for {term}.");
            exit(1);
        }
    };

    let mut out = String::new();
    if let Some(path) = find_entry(&term, &dirs) {
        out.push_str(&format!(
            "#\tReconstructed via infocmp from file: {}\n",
            path.display()
        ));
    }

    // `-C` emits termcap source instead of the terminfo layout.
    if termcap {
        out.push_str(&to_termcap(&ti));
        print!("{out}");
        return;
    }

    out.push_str(&ti.names.join("|"));
    out.push_str(",\n");

    // Obsolete (termcap-compatibility) `OT*` caps are emitted only under `-x`.
    let mut bools: Vec<&str> = caps::BOOL_NAMES
        .iter()
        .filter(|&&n| (ext || !n.starts_with("OT")) && ti.tigetflag(n) == 1)
        .copied()
        .collect();
    bools.sort_unstable();
    for n in bools {
        out.push_str(&format!("\t{n},\n"));
    }
    // Extended booleans follow the predefined ones, sorted among themselves.
    if ext {
        let mut eb: Vec<&str> = ti
            .ext_bool_names()
            .iter()
            .filter(|(_, v)| *v)
            .map(|(n, _)| n.as_str())
            .collect();
        eb.sort_unstable();
        for n in eb {
            out.push_str(&format!("\t{n},\n"));
        }
    }

    // Numbers, sorted by short name; cancelled numerics (`name@`) are interleaved with present ones.
    let mut nums: Vec<(&str, String)> = caps::NUM_NAMES
        .iter()
        .filter_map(|&n| {
            if !ext && n.starts_with("OT") {
                return None;
            }
            if ti.num_cancelled(n) {
                return Some((n, format!("{n}@")));
            }
            let v = ti.tigetnum(n);
            (v >= 0).then(|| (n, format!("{n}#{}", fmt_num(v))))
        })
        .collect();
    nums.sort_unstable_by_key(|&(n, _)| n);
    for (_, rendered) in &nums {
        out.push_str(&format!("\t{rendered},\n"));
    }
    // Extended numerics follow the predefined ones, sorted among themselves.
    if ext {
        let mut en: Vec<(&str, String)> = ti
            .ext_num_names()
            .iter()
            .filter(|(_, v)| *v >= 0)
            .map(|(n, v)| (n.as_str(), format!("{n}#{}", fmt_num(*v))))
            .collect();
        en.sort_unstable_by(|a, b| a.0.cmp(b.0));
        for (_, rendered) in &en {
            out.push_str(&format!("\t{rendered},\n"));
        }
    }

    // Strings, sorted by short name; cancelled strings (`name@`) are interleaved with present ones.
    let mut strs: Vec<(&str, String)> = Vec::new();
    for &n in caps::STR_NAMES {
        if !ext && n.starts_with("OT") {
            continue;
        }
        if ti.str_cancelled(n) {
            strs.push((n, format!("{n}@")));
        } else if let Tigetstr::Value(v) = ti.tigetstr(n) {
            let rendered = if n == "acsc" {
                tic_expand(&sort_acsc(v))
            } else {
                tic_expand(v)
            };
            strs.push((n, format!("{n}={rendered}")));
        }
    }
    strs.sort_unstable_by(|a, b| a.0.cmp(b.0));
    for (_, rendered) in &strs {
        out.push_str(&format!("\t{rendered},\n"));
    }

    if ext {
        // Extended string caps follow, sorted by name; a cancelled one renders as `name@`.
        let mut ex = ti.ext_string_caps();
        ex.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        for (n, v) in ex {
            match v {
                Some(bytes) => out.push_str(&format!("\t{}={},\n", n, tic_expand(&bytes))),
                None => out.push_str(&format!("\t{n}@,\n")),
            }
        }
    }

    print!("{out}");
}

#[cfg(test)]
mod tests {
    use super::{fmt_num, sort_acsc, tic_expand};

    #[test]
    fn letter_and_special_escapes() {
        assert_eq!(tic_expand(b"\x1b[m"), "\\E[m");
        assert_eq!(tic_expand(b"\n"), "\\n");
        assert_eq!(tic_expand(b"\r"), "\\r");
        assert_eq!(tic_expand(b"\x80"), "\\0");
        assert_eq!(tic_expand(b"a,b"), "a\\,b");
        assert_eq!(tic_expand(b"a^b"), "a\\^b");
        assert_eq!(tic_expand(b"a\\b"), "a\\\\b");
        // backslash directly after a literal caret stays single (ncurses quirk).
        assert_eq!(tic_expand(b"^\\"), "\\^\\");
    }

    #[test]
    fn space_rule_first_or_trailing_run() {
        assert_eq!(tic_expand(b" a"), "\\sa"); // leading
        assert_eq!(tic_expand(b"a b"), "a b"); // interior literal
        assert_eq!(tic_expand(b"a "), "a\\s"); // trailing
        assert_eq!(tic_expand(b"a  "), "a\\s\\s"); // whole trailing run
        assert_eq!(tic_expand(b"a  b"), "a  b"); // interior run literal
    }

    #[test]
    fn control_caret_vs_octal_length_split() {
        // "short" values keep caret; the digit exception keeps caret in long values.
        assert_eq!(tic_expand(b"\x10AAA"), "^PAAA"); // weight 3 -> caret
        assert_eq!(tic_expand(b"\x10AAAA"), "\\020AAAA"); // weight 4 -> octal
        // clustered controls (no other content) stay caret well past four bytes.
        assert_eq!(tic_expand(b"\x12\x03\x1eP@1"), "^R^C^^P@1");
        // a control followed by a digit keeps caret even in a long value (acsc behaviour).
        assert_eq!(tic_expand(b"AAAA\x100"), "AAAA^P0");
        // many caret-controls (>10) force octal even with no other content.
        assert_eq!(tic_expand(&[0x10; 11]), "\\020".repeat(11));
    }

    #[test]
    fn percent_operator_takes_next_byte_verbatim() {
        // `%^` (XOR) and `%\` survive un-escaped; only a comma after `%` is protected.
        assert_eq!(tic_expand(b"\x1b[%gh%{4}%^%Ph"), "\\E[%gh%{4}%^%Ph");
        assert_eq!(tic_expand(b"%\\"), "%\\");
        assert_eq!(tic_expand(b"%,"), "%\\,");
    }

    #[test]
    fn acsc_sorts_pairs_by_glyph() {
        // unsorted (glyph,acs) pairs come back sorted by the source glyph byte.
        assert_eq!(sort_acsc(b"b2a1"), b"a1b2");
    }

    #[test]
    fn numbers_hex_near_powers_of_two() {
        assert_eq!(fmt_num(8), "8");
        assert_eq!(fmt_num(255), "255");
        assert_eq!(fmt_num(256), "0x100");
        assert_eq!(fmt_num(0x10000), "0x10000");
        assert_eq!(fmt_num(496), "0x1f0"); // 512 - 16, in window
        assert_eq!(fmt_num(768), "768"); // multiple of 256 but not near a power of two
        assert_eq!(fmt_num(1000), "1000");
    }
}
