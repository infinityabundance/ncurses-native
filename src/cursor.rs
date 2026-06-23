//! The cursor-movement cost optimizer -- the crown jewel.
//!
//! [`mvcur`] is a clean-room reproduction of ncurses's `mvcur` / `relative_move` /
//! `onscreen_mvcur` cost machinery (`ncurses/tty/lib_mvcur.c`) for the admitted xterm /
//! ncurses 6.4 terminal. Given a current cursor position and a target it enumerates the same
//! *tactics* ncurses weighs, scores each with ncurses's **static, sample-parameter cost model**,
//! and emits the bytes of the cheapest tactic -- with the lower-numbered tactic winning a tie
//! (ncurses compares with a strict `<`, so the first tactic to reach a given cost keeps it).
//!
//! This is verified **byte-exact against real ncurses**: see `tests/mvcur_matrix.rs`, which
//! replays the captured 23 409-pair `(from -> to)` matrix recorded from libncurses 6.4 on an
//! 80x24 xterm pty (`tests/fixtures/mvcur_matrix.txt`). Every genuine move matches.
//!
//! # The cost model
//!
//! ncurses does **not** measure the actual emitted string when choosing a tactic. In
//! `_nc_mvcur_init` it precomputes a fixed cost per capability using *sample parameters*
//! (`23` for one-parameter caps, `23, 23` for `cup`), so e.g. `hpa` always costs 5 -- the length
//! of `\e[24G` -- even when the actual target column needs only `\e[6G`. Costs are measured in
//! `_char_padding` units, but every cost in the comparison is scaled by the same `_char_padding`,
//! so the ordering is identical to a plain character count; we use character counts directly.
//!
//! For the admitted xterm caps these fixed costs are:
//!
//! | cap | string (sample) | cost | cap | string (sample) | cost |
//! |-----|-----------------|------|-----|-----------------|------|
//! | `cr` | `\r` | 1 | `cup` | `\e[24;24H` | 8 |
//! | `home` | `\e[H` | 3 | `cub1` | `\b` | 1 |
//! | `cuf1` | `\e[C` | 3 | `cuu1` | `\e[A` | 3 |
//! | `cub`/`cuf`/`cud`/`cuu` | `\e[23D` ... | 5 | `hpa`/`vpa` | `\e[24G` / `\e[24d` | 5 |
//!
//! xterm has no `cursor_to_ll` (tactic 4 disabled) and no `auto_left_margin`/`bw`
//! (tactic 5 disabled); `cud1` is `\n`, which `relative_move` refuses to use for local motion,
//! so downward moves always use `vpa`. Hard tabs are not used (no `tab` capability is reached).
//!
//! # Multi-terminal ([`Caps`])
//!
//! The cost model is parameterized by a [`Caps`] profile so the same engine drives a *different cap
//! class*. [`Caps::vt100`] has **no `hpa`/`vpa`** and `$<N>` padding on `cup`/`cuf1`/`cuu1`, giving
//! `cup`=33, `cuf1`/`cuu1`=13 (derived as `chars + 5*padding_ms` at the 38400-baud pty), so vt100
//! steps with the parameterized `cuf`/`cub`/`cuu`/`cud` and `\b` instead of column/row addressing.
//! [`mvcur_caps`]/[`mvcur_ovw_caps`] take an explicit profile; the default [`mvcur`]/[`mvcur_ovw`]
//! use [`Caps::xterm`]. Both are verified byte-exact against real ncurses: 23409 pairs each on xterm
//! (`tests/mvcur_matrix.rs`) and on vt100 (court `NCURSES.MVCUR.VT100`).
//!
//! # Overwrite (`ovw`)
//!
//! The public [`mvcur`]`(3)` entrypoint calls the optimizer with `ovw = FALSE`, so the cursor is
//! moved purely with motion strings; it never overwrites on-screen cells with literal characters.
//! That is the byte-exact behavior pinned by the move matrix. ncurses's internal `doupdate` path
//! passes `ovw = TRUE` and may advance the cursor by **rewriting the known on-screen characters**
//! (`WANT_CHAR`) instead of emitting motion; [`mvcur_ovw`] reproduces that branch, taking a
//! `want(row, col)` hook that supplies the desired-screen content. This is used by
//! [`crate::update::Screen`]'s `GoTo` and is what makes the screen-diff byte-exact.

use crate::terminfo::{tparm_n, Terminfo};

/// Default screen width when a terminal omits `cols`.
const COLS: i32 = 80;
/// Runtime screen height (the capture pty's 24 rows); `cursor_to_ll` jumps to `(LINES-1, 0)`.
const LINES: i32 = 24;

/// `8 - COMPUTE_OVERHEAD`; the `NOT_LOCAL` distance threshold.
const LONG_DIST: i32 = 7;
const INF: i32 = i32::MAX;

/// `_char_padding` in tenths-of-ms per char at the capture pty's baud (`(9 * 1000 * 10) / 38400`).
/// ncurses scores every cap cost in these units and divides by it, so a plain char costs 1 unit and
/// each ms of `$<N>` padding costs 5. Verified against the captured xterm/vt100 mvcur matrices.
const CHAR_PADDING: i32 = 2;

/// The padding value of a terminfo `$<...>` body in tenths-of-ms (e.g. `5` -> 50, `0.5` -> 5). The
/// `*` (per-affcnt) and `/` (mandatory) flags don't change a single-cell motion's cost, so stop at
/// them (affcnt == 1).
fn padding_tenths(body: &[u8]) -> i64 {
    let mut int = 0i64;
    let mut frac = 0i64;
    let mut dot = false;
    for &b in body {
        if b.is_ascii_digit() {
            if dot {
                frac = (b - b'0') as i64;
                break;
            }
            int = int * 10 + (b - b'0') as i64;
        } else if b == b'.' {
            dot = true;
        } else {
            break;
        }
    }
    int * 10 + frac
}

/// ncurses `NormalizedCost`: `_nc_msec_cost(cap) / _char_padding`. `cap` is the cap already expanded
/// with sample parameters (so its length is fixed), with `$<N>` padding intact. Public so the
/// `doupdate` engine can score its shaping caps (`el`/`el1`) from terminfo the same way.
pub fn normalized_cost(expanded: &[u8]) -> i32 {
    let mut msec = 0i64;
    let mut i = 0;
    while i < expanded.len() {
        if expanded[i] == b'$' && i + 1 < expanded.len() && expanded[i + 1] == b'<' {
            if let Some(rel) = expanded[i + 2..].iter().position(|&b| b == b'>') {
                msec += padding_tenths(&expanded[i + 2..i + 2 + rel]);
                i += 2 + rel + 1;
                continue;
            }
        }
        msec += CHAR_PADDING as i64;
        i += 1;
    }
    (msec / CHAR_PADDING as i64) as i32
}

/// Strip `$<...>` padding markers from a cap's output -- the bytes ncurses sends a terminal with
/// `xon` (flow control handles delays, so no NUL/PC pad bytes are emitted). Public so the `doupdate`
/// engine can strip padding from its terminfo-derived `clear`/etc. caps.
pub fn strip_padding(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'$' && i + 1 < s.len() && s[i + 1] == b'<' {
            if let Some(rel) = s[i + 2..].iter().position(|&b| b == b'>') {
                i += 2 + rel + 1;
                continue;
            }
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

/// The capture pty's output baud (used for the `tputs` pad-byte count). Matches the cost model's
/// `_char_padding` (`(9*1000*10)/38400 == 2`).
const BAUD: i64 = 38400;

/// Apply terminfo `$<N>` padding to a cap's expanded output, the way `tputs` does for the capture
/// pty: a terminal **without `xon`** (or a **mandatory** `$<N/>`) emits `floor(N_ms * baud / (9*1000))`
/// pad characters (`pad_char`, else NUL); otherwise the padding is stripped (xon flow control handles
/// the delay). The `*` proportional flag scales by the affected-line count, which is 1 for cursor
/// motion, so it is ignored here.
pub fn apply_padding(s: &[u8], xon: bool, pad_char: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'$' && i + 1 < s.len() && s[i + 1] == b'<' {
            if let Some(rel) = s[i + 2..].iter().position(|&b| b == b'>') {
                let body = &s[i + 2..i + 2 + rel];
                let mandatory = body.contains(&b'/');
                if !xon || mandatory {
                    let tenths = padding_tenths(body);
                    let n = (tenths * BAUD / 90000) as usize; // tenths*baud / (9 * 10000)
                    out.extend(std::iter::repeat(pad_char).take(n));
                }
                i += 2 + rel + 1;
                continue;
            }
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

/// One cursor-motion capability: its byte string (expanded via `tparm` at emit time) and its
/// ncurses sample-parameter cost (`INF` when the cap is absent or unusable).
#[derive(Debug, Clone, Default)]
struct Cap {
    s: Vec<u8>,
    cost: i32,
}

impl Cap {
    fn absent() -> Cap {
        Cap {
            s: Vec::new(),
            cost: INF,
        }
    }
    fn present(&self) -> bool {
        self.cost != INF && !self.s.is_empty()
    }
}

/// A terminal's cursor-motion cost profile -- the cap strings and the sample-parameter costs ncurses
/// precomputes in `_nc_mvcur_init`. [`Caps::from_terminfo`] derives it from any terminfo entry; the
/// hand-built [`Caps::xterm`]/[`Caps::vt100`]/[`Caps::cygwin`] reproduce those terminals without a
/// loaded database (for the offline unit tests / courts). Emission expands the cap via `tparm`, then
/// either strips `$<>` padding (the `xon` path) or emits `pad_char` pad bytes ([`apply_padding`]).
#[derive(Debug, Clone)]
pub struct Caps {
    cols: i32,
    cup: Cap,
    home: Cap,
    cr: Cap,
    cub1: Cap,
    cuf1: Cap,
    cuu1: Cap,
    cud1: Cap,
    cuf: Cap,
    cub: Cap,
    cuu: Cap,
    cud: Cap,
    hpa: Cap,
    vpa: Cap,
    /// `cursor_to_ll` (`ll`): jump to the lower-left corner `(lines-1, 0)`; enables tactic 4.
    ll: Cap,
    /// `xon` flow control: when true, non-mandatory `$<N>` padding is suppressed at emit time.
    xon: bool,
    /// The pad character (`pad` cap's first byte, else NUL) emitted for delays on a no-`xon` terminal.
    pad_char: u8,
    /// `auto_left_margin` (`bw`): a backspace at column 0 wraps to the previous line's last column,
    /// enabling the right-margin back-wrap tactic (`[cr] + cub1 + vertical`).
    bw: bool,
    /// `eat_newline_glitch` (`xenl`): the last column is a "magic margin" (delayed wrap), so the bw
    /// back-wrap to column `cols-1` is unreliable -- ncurses uses `cup` instead when xenl is set.
    xenl: bool,
}

/// Build a [`Cap`] from a literal cap string (no parameters), computing its cost.
fn lit_cap(s: &[u8]) -> Cap {
    Cap {
        s: s.to_vec(),
        cost: normalized_cost(s),
    }
}

/// Build a parameterized [`Cap`] from a cap template, computing the cost at the sample parameters.
fn parm_cap(tmpl: &[u8], sample: &[i32]) -> Cap {
    Cap {
        s: tmpl.to_vec(),
        cost: normalized_cost(&tparm_n(tmpl, sample)),
    }
}

impl Caps {
    /// The admitted xterm / ncurses 6.4 caps (also matches the Linux console for cursor motion).
    pub fn xterm() -> Caps {
        Caps {
            cols: COLS,
            cup: parm_cap(b"\x1b[%i%p1%d;%p2%dH", &[23, 23]),
            home: lit_cap(b"\x1b[H"),
            cr: lit_cap(b"\r"),
            cub1: lit_cap(b"\x08"),
            cuf1: lit_cap(b"\x1b[C"),
            cuu1: lit_cap(b"\x1b[A"),
            cud1: Cap::absent(), // cud1 == "\n", never used for local motion
            cuf: parm_cap(b"\x1b[%p1%dC", &[23]),
            cub: parm_cap(b"\x1b[%p1%dD", &[23]),
            cuu: parm_cap(b"\x1b[%p1%dA", &[23]),
            cud: parm_cap(b"\x1b[%p1%dB", &[23]),
            hpa: parm_cap(b"\x1b[%i%p1%dG", &[23]),
            vpa: parm_cap(b"\x1b[%i%p1%dd", &[23]),
            ll: Cap::absent(),
            xon: false, // xterm motion caps carry no padding, so this is moot for emission
            pad_char: 0,
            bw: false,
            xenl: true,
        }
    }

    /// The cygwin caps: xterm-like, but `cud1` is `\e[B` (usable for down-by-one).
    pub fn cygwin() -> Caps {
        Caps {
            cud1: lit_cap(b"\x1b[B"),
            xon: true,
            ..Caps::xterm()
        }
    }

    /// The DEC vt100 caps: no `hpa`/`vpa`, and `cup`/`cuf1`/`cuu1` carry `$<5>`/`$<2>` padding, so
    /// `cup`=33 and `cuf1`/`cuu1`=13 (they never beat the unpadded parm caps).
    pub fn vt100() -> Caps {
        Caps {
            cols: COLS,
            cup: parm_cap(b"\x1b[%i%p1%d;%p2%dH$<5>", &[23, 23]),
            home: lit_cap(b"\x1b[H"),
            cr: lit_cap(b"\r"),
            cub1: lit_cap(b"\x08"),
            cuf1: lit_cap(b"\x1b[C$<2>"),
            cuu1: lit_cap(b"\x1b[A$<2>"),
            cud1: Cap::absent(),
            cuf: parm_cap(b"\x1b[%p1%dC", &[23]),
            cub: parm_cap(b"\x1b[%p1%dD", &[23]),
            cuu: parm_cap(b"\x1b[%p1%dA", &[23]),
            cud: parm_cap(b"\x1b[%p1%dB", &[23]),
            hpa: Cap::absent(),
            vpa: Cap::absent(),
            ll: Cap::absent(),
            xon: true, // vt100 has xon: padding is stripped at emit time
            pad_char: 0,
            bw: false,
            xenl: true,
        }
    }

    /// Derive the cursor-motion profile from a loaded terminfo entry -- the terminal-general path.
    /// Each cap's cost is `NormalizedCost` of the cap expanded with ncurses' sample parameters; a
    /// `cud1` of `\n` is treated as unusable (the tty driver may translate it).
    pub fn from_terminfo(t: &Terminfo) -> Caps {
        // ncurses sizes the screen from the runtime tty winsize, NOT the terminfo `cols` (a 132-col
        // entry still runs in an 80-col window), and `cols` drives the NOT_LOCAL threshold. The
        // capture pty -- and any standard terminal -- is 80 wide, so use that.
        let cols = COLS;
        let lit = |name: &str| -> Cap {
            match t.string(name) {
                Some(s) => lit_cap(s),
                None => Cap::absent(),
            }
        };
        let parm = |name: &str, sample: &[i32]| -> Cap {
            match t.string(name) {
                Some(s) => parm_cap(s, sample),
                None => Cap::absent(),
            }
        };
        // cr is the carriage_return cap; when absent ncurses does NOT synthesize "\r" for mvcur, so
        // the CR tactic is disabled (those terminals reach column 0 via home/cup instead).
        let cr = match t.string("cr") {
            Some(s) => lit_cap(s),
            None => Cap::absent(),
        };
        // A cud1 whose first byte is newline is unusable for local motion: ncurses tests
        // `*cursor_down != '\n'` (lib_mvcur.c relative_move), so `\n` and `\n$<5>` alike are
        // rejected (the tty driver may translate the newline) and downward motion uses vpa/cud.
        let cud1 = match t.string("cud1") {
            Some(s) if s.first() == Some(&b'\n') => Cap::absent(),
            Some(s) => lit_cap(s),
            None => Cap::absent(),
        };
        Caps {
            cols,
            cup: parm("cup", &[23, 23]),
            home: lit("home"),
            cr,
            cub1: lit("cub1"),
            cuf1: lit("cuf1"),
            cuu1: lit("cuu1"),
            cud1,
            cuf: parm("cuf", &[23]),
            cub: parm("cub", &[23]),
            cuu: parm("cuu", &[23]),
            cud: parm("cud", &[23]),
            hpa: parm("hpa", &[23]),
            vpa: parm("vpa", &[23]),
            ll: lit("ll"),
            xon: t.tigetflag("xon") > 0,
            pad_char: t
                .string("pad")
                .and_then(|p| p.first().copied())
                .unwrap_or(0),
            bw: t.tigetflag("bw") > 0,
            xenl: t.tigetflag("xenl") > 0,
        }
    }
}

/// Emit a parameterized cap at `n`, applying `$<N>` padding for the profile; empty if absent.
fn emit_parm(cap: &Cap, n: i32, xon: bool, pad_char: u8) -> Vec<u8> {
    if cap.s.is_empty() {
        Vec::new()
    } else {
        apply_padding(&tparm_n(&cap.s, &[n]), xon, pad_char)
    }
}

/// Emit a literal (non-parameterized) cap, applying `$<N>` padding for the profile.
fn emit_lit(cap: &Cap, xon: bool, pad_char: u8) -> Vec<u8> {
    apply_padding(&cap.s, xon, pad_char)
}

/// Reproduce ncurses `relative_move`: move via local motions only (no `cup`), all coordinates
/// **0-based**. Returns `(cost, bytes)`, or `(INF, _)` when no local path exists.
///
/// Each axis enumerates ncurses' tactics in order -- the absolute single-axis cap (`vpa`/`hpa`) as
/// the baseline when present, then the parameterized `parm_*_cursor`, then the `*1` single-step cap
/// (and the `ovw=TRUE` overwrite for a forward horizontal move) -- keeping the cheapest with a strict
/// `<` so the earlier tactic wins ties. On xterm/linux the baseline `hpa`/`vpa` (cost 5) is present
/// and the parm caps (also 5) never beat it, so the behaviour is identical to before; on vt100 the
/// baseline is absent and the parm caps become the chosen motion.
///
/// `want(row, col)` is the `ovw=TRUE` overwrite hook (lib_mvcur.c `relative_move`): for a forward
/// horizontal move ncurses may advance the cursor by **rewriting the known on-screen characters**
/// (`WANT_CHAR`) instead of emitting motion, when every covered cell is charable with the current
/// (normal) attribute. `want(to_y, from_x + i)` returns `Some(byte)` for such a cell, else `None`.
fn relative_move<F: Fn(i32, i32) -> Option<u8>>(
    caps: &Caps,
    fy: i32,
    fx: i32,
    ty: i32,
    tx: i32,
    want: &F,
) -> (i32, Vec<u8>) {
    let mut s = Vec::new();
    let mut vcost = 0;

    if ty != fy {
        // Vertical: vpa baseline (if present), then parm_up/down_cursor, then the single-step cap
        // (cuu1 for up; cud1 for down only when it is usable -- not `\n`, which the tty may translate).
        let mut best = INF;
        let mut vs = Vec::new();
        if caps.vpa.present() {
            best = caps.vpa.cost;
            vs = emit_parm(&caps.vpa, ty, caps.xon, caps.pad_char); // row_address (0-based; %i makes it 1-based)
        }
        if ty < fy {
            let n = fy - ty;
            if caps.cuu.present() && caps.cuu.cost < best {
                best = caps.cuu.cost;
                vs = emit_parm(&caps.cuu, n, caps.xon, caps.pad_char); // parm_up_cursor
            }
            if caps.cuu1.present() && n * caps.cuu1.cost < best {
                best = n * caps.cuu1.cost;
                vs = emit_lit(&caps.cuu1, caps.xon, caps.pad_char).repeat(n as usize);
                // cuu1 x n
            }
        } else {
            let n = ty - fy;
            if caps.cud.present() && caps.cud.cost < best {
                best = caps.cud.cost;
                vs = emit_parm(&caps.cud, n, caps.xon, caps.pad_char); // parm_down_cursor
            }
            if caps.cud1.present() && n * caps.cud1.cost < best {
                best = n * caps.cud1.cost;
                vs = emit_lit(&caps.cud1, caps.xon, caps.pad_char).repeat(n as usize);
                // cud1 x n
            }
        }
        if best == INF {
            return (INF, s);
        }
        vcost = best;
        s.extend_from_slice(&vs);
    }

    let mut hcost = 0;
    if tx != fx {
        // Horizontal: hpa baseline (if present), then parm_left/right_cursor, then the overwrite /
        // cuf1 (right) or cub1 (left).
        let mut best = INF;
        let mut hs = Vec::new();
        if caps.hpa.present() {
            best = caps.hpa.cost;
            hs = emit_parm(&caps.hpa, tx, caps.xon, caps.pad_char); // column_address (0-based; %i makes it 1-based)
        }
        if tx > fx {
            let n = tx - fx;
            if caps.cuf.present() && caps.cuf.cost < best {
                best = caps.cuf.cost;
                hs = emit_parm(&caps.cuf, n, caps.xon, caps.pad_char); // parm_right_cursor
            }
            let overwrite: Option<Vec<u8>> = {
                let mut chars = Vec::with_capacity(n as usize);
                let mut ok = n > 0;
                for i in 0..n {
                    match want(ty, fx + i) {
                        Some(b) => chars.push(b),
                        None => {
                            ok = false;
                            break;
                        }
                    }
                }
                if ok {
                    Some(chars)
                } else {
                    None
                }
            };
            if let Some(chars) = overwrite {
                let lhcost = n; // n * char_padding (== 1)
                if lhcost < best {
                    best = lhcost;
                    hs = chars;
                }
            } else if caps.cuf1.present() && n * caps.cuf1.cost < best {
                best = n * caps.cuf1.cost;
                hs = emit_lit(&caps.cuf1, caps.xon, caps.pad_char).repeat(n as usize);
                // cuf1 x n
            }
        } else {
            let n = fx - tx;
            if caps.cub.present() && caps.cub.cost < best {
                best = caps.cub.cost;
                hs = emit_parm(&caps.cub, n, caps.xon, caps.pad_char); // parm_left_cursor
            }
            if caps.cub1.present() && n * caps.cub1.cost < best {
                best = n * caps.cub1.cost;
                hs = emit_lit(&caps.cub1, caps.xon, caps.pad_char).repeat(n as usize);
                // cub1 x n
            }
        }
        if best == INF {
            return (INF, s);
        }
        hcost = best;
        s.extend_from_slice(&hs);
    }

    (vcost + hcost, s)
}

/// ncurses `NOT_LOCAL`: a target far from both screen edges and far from the source is not worth
/// optimizing -- the optimizer skips straight to direct cursor addressing.
fn not_local(caps: &Caps, fy: i32, fx: i32, ty: i32, tx: i32) -> bool {
    tx > LONG_DIST
        && tx < caps.cols - 1 - LONG_DIST
        && (ty - fy).abs() + (tx - fx).abs() > LONG_DIST
}

/// Reproduce ncurses's `mvcur` choice for a move from `from = (row, col)` to `to = (row, col)` on
/// the admitted xterm / ncurses 6.4 terminal. Coordinates are **1-based** `(row, col)` (the public
/// curses convention); they are converted to ncurses's 0-based internal coordinates here. Returns
/// the exact bytes ncurses would emit -- empty when the move is a no-op (`from == to`).
///
/// This is the public `mvcur(3)` path: `ovw = FALSE` (no overwrite). It is pure, never panics, and
/// is verified byte-exact against the captured ncurses move matrix (`tests/mvcur_matrix.rs`).
pub fn mvcur(from: (i32, i32), to: (i32, i32)) -> Vec<u8> {
    mvcur_ovw(from, to, |_, _| None)
}

/// [`mvcur`] for an explicit [`Caps`] profile (e.g. [`Caps::vt100`]). `ovw = FALSE`.
pub fn mvcur_caps(caps: &Caps, from: (i32, i32), to: (i32, i32)) -> Vec<u8> {
    mvcur_ovw_caps(caps, from, to, |_, _| None)
}

/// `mvcur` with the `ovw = TRUE` overwrite branch enabled -- the form `doupdate` uses internally
/// (`GoTo`). `want(row, col)` returns `Some(byte)` when the desired cell at 0-based `(row, col)` is
/// a charable, normal-attribute cell that may be overwritten (its on-screen content is `byte`), and
/// `None` otherwise. Coordinates of `from`/`to` are **1-based**; `want` receives **0-based** cells.
pub fn mvcur_ovw<F: Fn(i32, i32) -> Option<u8>>(
    from: (i32, i32),
    to: (i32, i32),
    want: F,
) -> Vec<u8> {
    mvcur_ovw_caps(&Caps::xterm(), from, to, want)
}

/// [`mvcur_ovw`] for an explicit [`Caps`] profile.
pub fn mvcur_ovw_caps<F: Fn(i32, i32) -> Option<u8>>(
    caps: &Caps,
    from: (i32, i32),
    to: (i32, i32),
    want: F,
) -> Vec<u8> {
    // Convert the public 1-based API to ncurses's 0-based internal coordinates.
    let (fy, fx) = (from.0 - 1, from.1 - 1);
    let (ty, tx) = (to.0 - 1, to.1 - 1);

    if fy == ty && fx == tx {
        return Vec::new();
    }

    // Tactic 0: direct cursor addressing (cup). The baseline.
    let mut tactic = 0;
    let mut usecost = caps.cup.cost;

    // ncurses only sets up the cup baseline and applies the NOT_LOCAL short-circuit *when cup
    // (`_address_cursor`) exists* (lib_mvcur.c: the `if (yold==-1 || NOT_LOCAL) goto nonlocal` is
    // nested inside the `if (TIPARM_2(_address_cursor,...))` block). A terminal with no cup never
    // skips the local tactics -- they are its only way to move -- so a "far" move on a cup-less
    // terminal (pure-relative LCDs, Tektronix text-mode, ...) still runs the cub1/cuf1/cuu1/cud1
    // optimizer rather than emitting nothing.
    let cup_present = !caps.cup.s.is_empty();
    let skip_local = cup_present && (fy < 0 || fx < 0 || not_local(caps, fy, fx, ty, tx));
    if !skip_local {
        // Tactic 1: local movement from the source (only with a known source position).
        if fy >= 0 && fx >= 0 {
            let (c, _) = relative_move(caps, fy, fx, ty, tx, &want);
            if c != INF && c < usecost {
                tactic = 1;
                usecost = c;
            }
        }
        // Tactic 2: carriage-return to column 0, then local movement (known source row + cr).
        if fy >= 0 && caps.cr.cost != INF {
            let (c, _) = relative_move(caps, fy, 0, ty, tx, &want);
            if c != INF && caps.cr.cost + c < usecost {
                tactic = 2;
                usecost = caps.cr.cost + c;
            }
        }
        // Tactic 3: home, then local movement.
        let (c, _) = relative_move(caps, 0, 0, ty, tx, &want);
        if c != INF && caps.home.present() && caps.home.cost + c < usecost {
            tactic = 3;
            usecost = caps.home.cost + c;
        }
        // Tactic 4: cursor_to_ll (ll) jumps to the lower-left corner (LINES-1, 0), then local
        // movement to the target -- cheap for last-line targets (e.g. wy350's ll = `\x1e\x0b`).
        if caps.ll.present() {
            let (c, _) = relative_move(caps, LINES - 1, 0, ty, tx, &want);
            if c != INF && caps.ll.cost + c < usecost {
                tactic = 4;
                usecost = caps.ll.cost + c;
            }
        }
        // Tactic 5: auto_left_margin (bw) back-wrap. A backspace at column 0 wraps to the previous
        // line's last column (fy-1, cols-1); ncurses then runs a full relative_move from there to
        // the target (lib_mvcur.c tactic #5 -- note there is *no* right-margin restriction: the
        // target may be any column, reached by the relative leg from the wrapped corner). The lead
        // is [cr if not already at col 0] + cub1. Gated on !xenl (the magic margin makes the wrap
        // unreliable). `(fx == 0 || cr present)` guards the cr_cost = INFINITY case.
        if caps.bw && !caps.xenl && fy >= 1 && caps.cub1.present() && (fx == 0 || caps.cr.present())
        {
            let (rc, _) = relative_move(caps, fy - 1, caps.cols - 1, ty, tx, &want);
            if rc != INF {
                let crc = if fx != 0 { caps.cr.cost } else { 0 };
                let total = crc + caps.cub1.cost + rc;
                if total < usecost {
                    tactic = 5;
                    usecost = total;
                }
            }
        }
    }
    let _ = usecost; // final cost is informational; only the winning tactic is emitted

    match tactic {
        1 => relative_move(caps, fy, fx, ty, tx, &want).1,
        2 => {
            let mut out = emit_lit(&caps.cr, caps.xon, caps.pad_char);
            out.extend_from_slice(&relative_move(caps, fy, 0, ty, tx, &want).1);
            out
        }
        3 => {
            let mut out = emit_lit(&caps.home, caps.xon, caps.pad_char);
            out.extend_from_slice(&relative_move(caps, 0, 0, ty, tx, &want).1);
            out
        }
        4 => {
            // cursor_to_ll: jump to (LINES-1, 0), then the local leg to the target.
            let mut out = emit_lit(&caps.ll, caps.xon, caps.pad_char);
            out.extend_from_slice(&relative_move(caps, LINES - 1, 0, ty, tx, &want).1);
            out
        }
        5 => {
            // bw back-wrap: [cr] + cub1 (wrap to (fy-1, cols-1)) + the relative leg to (ty, tx).
            let mut out = if fx != 0 {
                emit_lit(&caps.cr, caps.xon, caps.pad_char)
            } else {
                Vec::new()
            };
            out.extend(emit_lit(&caps.cub1, caps.xon, caps.pad_char));
            out.extend_from_slice(&relative_move(caps, fy - 1, caps.cols - 1, ty, tx, &want).1);
            out
        }
        _ => {
            // cup: direct cursor address at the 0-based target (%i makes it 1-based).
            if caps.cup.s.is_empty() {
                Vec::new()
            } else {
                apply_padding(&tparm_n(&caps.cup.s, &[ty, tx]), caps.xon, caps.pad_char)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_op_move_is_empty() {
        assert_eq!(mvcur((1, 1), (1, 1)), b"");
        assert_eq!(mvcur((5, 9), (5, 9)), b"");
    }

    #[test]
    fn home_target_uses_home() {
        // (1,1) target is reachable by \e[H (3 bytes), shorter than CUP \e[1;1H.
        assert_eq!(mvcur((10, 40), (1, 1)), b"\x1b[H");
    }

    #[test]
    fn padding_emits_or_strips_per_xon() {
        // No-xon: $<5> -> 21 NUL pad bytes (floor(5*38400/9000)); $<2> -> 8. Verified vs real ncurses
        // on ampex219 (cup $<5> -> 21 NULs) and modgraph2 (cuu1 $<2/> -> 8 NULs).
        assert_eq!(
            apply_padding(b"\x1b[1;1H$<5>", false, 0),
            [b"\x1b[1;1H".as_ref(), &[0u8; 21]].concat()
        );
        assert_eq!(apply_padding(b"\x1b[A$<2>", false, 0).len(), 3 + 8);
        // xon: non-mandatory padding stripped, mandatory `/` still emitted.
        assert_eq!(
            apply_padding(b"\x1b[1;1H$<5>", true, 0),
            b"\x1b[1;1H".to_vec()
        );
        assert_eq!(apply_padding(b"\x1b[A$<2/>", true, 0).len(), 3 + 8);
        // pad_char honored.
        assert_eq!(
            apply_padding(b"x$<5>", false, b'.'),
            b"x.....................".to_vec()
        );
    }

    #[test]
    fn from_terminfo_padded_cup_emits_pad_bytes() {
        // ampex219 (no xon, cup `\E[...H$<5>`): a not-local move uses cup + 21 NUL pad bytes, exactly
        // as captured from real ncurses-on-ampex219.
        let bytes = std::fs::read("tests/terminfo/ampex219").expect("read fixture");
        let t = Terminfo::parse(&bytes).expect("parse");
        let c = Caps::from_terminfo(&t);
        let out = mvcur_caps(&c, (1, 1), (12, 41)); // 0-based (0,0)->(11,40), not-local -> cup
        assert_eq!(out, [b"\x1b[12;41H".as_ref(), &[0u8; 21]].concat());
    }

    #[test]
    fn same_row_forward_uses_cuf1_then_hpa() {
        // ovw=FALSE: short forward advances use cuf1 / hpa, never literal spaces.
        // delta 1 -> single cuf1.
        assert_eq!(mvcur((1, 1), (1, 2)), b"\x1b[C");
        // delta >= 2 within the local window -> hpa to the target column (1-based via %i).
        assert_eq!(mvcur((1, 5), (1, 9)), b"\x1b[9G");
        // delta 7, still local (tx=7 0-based is == LONG_DIST, so not NOT_LOCAL) -> hpa.
        assert_eq!(mvcur((1, 1), (1, 8)), b"\x1b[8G");
    }

    #[test]
    fn same_row_backward() {
        // Close backward move: backspaces are cheapest (n=1 -> one cub1).
        assert_eq!(mvcur((1, 9), (1, 8)), b"\x08");
        // n cub1 (cost n) beats hpa (cost 5) for n < 5, and beats CR-then-forward here.
        assert_eq!(mvcur((1, 5), (1, 2)), b"\x08\x08\x08");
    }

    #[test]
    fn vertical_then_horizontal() {
        // (2,3) from home: the diagonal local path (vpa+hpa, cost 10) loses to cup (cost 8).
        assert_eq!(mvcur((1, 1), (2, 3)), b"\x1b[2;3H");
        // up-one uses cuu1 when shortest: from (3,1) to (2,1) -> \e[A.
        assert_eq!(mvcur((3, 1), (2, 1)), b"\x1b[A");
    }

    #[test]
    fn cup_when_cheapest_or_nonlocal() {
        // NOT_LOCAL far same-row advance -> cup.
        assert_eq!(mvcur((1, 1), (1, 10)), b"\x1b[1;10H");
        // Diagonal far move -> cup.
        assert_eq!(mvcur((1, 1), (3, 5)), b"\x1b[3;5H");
        assert_eq!(mvcur((1, 1), (10, 40)), b"\x1b[10;40H");
        // Diagonal to the right margin is cup, not vpa+hpa.
        assert_eq!(mvcur((1, 1), (2, 79)), b"\x1b[2;79H");
    }

    #[test]
    fn prompt_moves_match_screenio_pins() {
        // From (1,2) down to (2,1): vpa row 2 then a single backspace.
        assert_eq!(mvcur((1, 2), (2, 1)), b"\x1b[2d\x08");
        // From (1,6) to (2,1): CR then vpa row 2 (backspaces from col 5 don't beat CR).
        assert_eq!(mvcur((1, 6), (2, 1)), b"\r\x1b[2d");
        // From (2,4) to (3,1): CR then vpa row 3.
        assert_eq!(mvcur((2, 4), (3, 1)), b"\r\x1b[3d");
    }

    #[test]
    fn vt100_uses_parm_caps_without_hpa_vpa() {
        // vt100 has no hpa/vpa and padded cuf1/cuu1, so it steps with the parameterized
        // cuf/cub/cuu/cud (and unpadded cub1=\b). Every value here is captured from real
        // ncurses-on-vt100 (the MVCUR.VT100 court grounds the full matrix). 1-based coords.
        let c = Caps::vt100();
        // Local diagonals: relative motion (cud+cuf), not cup -- cup costs 33.
        assert_eq!(mvcur_caps(&c, (1, 1), (2, 3)), b"\x1b[1B\x1b[2C");
        assert_eq!(mvcur_caps(&c, (6, 6), (8, 8)), b"\x1b[2B\x1b[2C");
        // Up-left diagonal: cuu + cub1*2 (n*1=2 beats parm cub=5).
        assert_eq!(mvcur_caps(&c, (4, 4), (2, 2)), b"\x1b[2A\x08\x08");
        // Up + larger left: cuu + parm cub (n*1=6 loses to parm cub=5).
        assert_eq!(mvcur_caps(&c, (6, 11), (4, 5)), b"\x1b[2A\x1b[6D");
        // Same-row far left (local): parm cub.
        assert_eq!(mvcur_caps(&c, (6, 41), (6, 6)), b"\x1b[35D");
        // Single steps: cuf/cuu/cud parm (cuf1/cuu1 are padded), cub1=\b.
        assert_eq!(mvcur_caps(&c, (11, 11), (11, 12)), b"\x1b[1C");
        assert_eq!(mvcur_caps(&c, (11, 12), (11, 11)), b"\x08");
        assert_eq!(mvcur_caps(&c, (11, 11), (10, 11)), b"\x1b[1A");
        assert_eq!(mvcur_caps(&c, (11, 11), (12, 11)), b"\x1b[1B");
        // home still wins for the home target; not_local still forces cup.
        assert_eq!(mvcur_caps(&c, (9, 6), (1, 1)), b"\x1b[H");
        assert_eq!(mvcur_caps(&c, (1, 1), (6, 11)), b"\x1b[6;11H");
    }

    #[test]
    fn cygwin_uses_cud1_for_down_one() {
        // cygwin's cud1 is `\e[B` (cost 3, not `\n`), so down-by-one uses it instead of vpa (cost 5);
        // down-by-two falls back to vpa (cud1*2 = 6 > 5). Up still uses cuu1. Mirrors NCURSES.ACS.
        let c = Caps::cygwin();
        assert_eq!(mvcur_caps(&c, (1, 1), (2, 1)), b"\x1b[B"); // down 1 -> cud1
        assert_eq!(mvcur_caps(&c, (1, 1), (3, 1)), b"\x1b[3d"); // down 2 -> vpa
        assert_eq!(mvcur_caps(&c, (3, 1), (2, 1)), b"\x1b[A"); // up 1 -> cuu1
                                                               // Horizontal stays xterm-like (has hpa/vpa, no padding): a local forward move uses hpa.
        assert_eq!(mvcur_caps(&c, (1, 5), (1, 9)), b"\x1b[9G"); // hpa
    }

    #[test]
    fn xterm_caps_unchanged_by_parameterization() {
        // The explicit xterm Caps reproduces the default path exactly.
        let c = Caps::xterm();
        assert_eq!(mvcur_caps(&c, (1, 1), (2, 3)), b"\x1b[2;3H");
        assert_eq!(mvcur_caps(&c, (1, 1), (1, 2)), b"\x1b[C");
        assert_eq!(mvcur_caps(&c, (3, 1), (2, 1)), b"\x1b[A");
        assert_eq!(mvcur_caps(&c, (1, 5), (1, 9)), b"\x1b[9G");
    }
}
