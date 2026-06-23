//! The screen-update optimizer -- `doupdate` / `TransformLine` / `ClrUpdate`.
//!
//! This is a clean-room reproduction of ncurses's `tty_update.c` for the admitted xterm /
//! ncurses 6.4 terminal: it diffs the *current* physical screen (`curscr`) against the *desired*
//! virtual screen (`newscr`) and emits the byte stream ncurses would emit to make the terminal
//! match -- the whole reason curses exists. The cursor motion between spans reuses the byte-exact
//! [`crate::cursor::mvcur`] cost optimizer.
//!
//! # Scope (honest bound)
//!
//! This reproduces the **plain-text path**: cells carrying the default attribute (`A_NORMAL`,
//! no color). For that path the structural decisions are reproduced faithfully and verified
//! byte-exact against real ncurses (court `NCURSES.DOUPDATE`):
//!
//! * `ClrUpdate` first paint (`clear_screen` + per-line `TransformLine` from a blank `curscr`);
//! * incremental `TransformLine`: the `clr_bol` leading-blank optimization, the first/last
//!   differing-column diff, the `clr_eol` trailing-clear decision (`_el_cost`), the
//!   overwrite-vs-`PutRange` choice, and the insert/delete-character path (`parm_ich`/`parm_dch`,
//!   gated by `InsCharCost`/`DelCharCost` exactly as ncurses gates them);
//! * `PutRange` identical-run skipping (`_inline_cost = 5`) and `EmitRange` `erase_chars`
//!   (`_ech_cost = 5`) and `repeat_char` (`_rep_cost = 6`) run-coalescing;
//! * the `mvcur` `ovw=TRUE` GoTo overwrite branch ([`crate::cursor::mvcur_ovw`]);
//! * `ClrBottom` trailing-blank-region clear via `clr_eos`;
//! * the final cursor park.
//!
//! The attribute/color SGR path (`UpdateAttrs`/`vidputs`, OPT-03) and the right-margin auto-wrap
//! (`PutCharLR`) corner are also reproduced byte-exact. The hardware scroll optimizer
//! (`_nc_scroll_optimize`/`_nc_scrolln`) is reproduced for any single contiguous moved band --
//! whole-screen, bottom-anchored (`dl`/`il`), and a confined `csr` region -- and for multiple
//! independent bands plus interleaved edits (the `_nc_hash_map` multi-region case).
//!
//! Out of this increment, tracked in `docs/gap-ledger.md`: the magic-cookie (`xmc`) and
//! `ceol_standout_glitch` line-disregard paths, and the general `_nc_hash_map` `idl`
//! overwrite-residual path (a single-line insert/delete that leaves new content in the vacated row).

use crate::color::Palette;
use crate::cursor::{apply_padding, mvcur_ovw_caps, normalized_cost, Caps};
use crate::terminfo::{tparm_n, Terminfo};
use crate::vid::Vid;
use crate::window::{char_width, Cell};

// --- Capability byte producers (xterm). ---
const CLEAR_SCREEN: &[u8] = b"\x1b[H\x1b[2J"; // clear_screen
const CLR_EOL: &[u8] = b"\x1b[K"; // clr_eol (el)
const CLR_BOL: &[u8] = b"\x1b[1K"; // clr_bol (el1)
const CLR_EOS: &[u8] = b"\x1b[J"; // clr_eos (ed)

// --- Static costs from _nc_mvcur_init / TransformLine (char_padding == 1). ---
const EL1_COST: i32 = 4; // clr_bol "\e[1K"
/// Cost assigned to an absent erase cap (`el`/`el1`): large enough that no cost comparison ever
/// chooses it, so the blank-overwrite fallback fires -- matching ncurses' behaviour for terminals
/// without that capability.
const ABSENT_COST: i32 = i32::MAX / 4;
/// All non-color attribute bits (`A_ATTRIBUTES` minus color), for the msgr move-reset test.
const ATTR_BITS: u32 = crate::window::attrs::STANDOUT
    | crate::window::attrs::UNDERLINE
    | crate::window::attrs::REVERSE
    | crate::window::attrs::BLINK
    | crate::window::attrs::DIM
    | crate::window::attrs::BOLD
    | crate::window::attrs::INVIS
    | crate::window::attrs::PROTECT
    | crate::window::attrs::ALTCHARSET;
const ECH_COST: i32 = 5; // erase_chars "\e[23X"
const CUP_CH_COST: i32 = 8; // cursor_address "\e[24;24H"
const INLINE_COST: i32 = 5; // min(cup_ch, hpa_ch, cuf_ch) = min(8,5,5)
const ICH_COST: i32 = 5; // parm_ich "\e[23@"  (InsCharCost: constant, parm_ich present)
const DCH_COST: i32 = 5; // parm_dch "\e[23P"  (DelCharCost: constant, parm_dch present)
const REP_COST: i32 = 6; // repeat_char " \e[22b" (sample char ' ', count 23)

const BLANK: Cell = Cell::plain(' ', 0);

/// `can_clear_with` for the xterm path. xterm has `back_color_erase` (bce), so a blank cell is
/// clearable even when it carries a color (the terminal's erase fills with the current SGR
/// background) -- as long as it has no *other* attributes (bold/reverse/etc. change a blank's
/// appearance and cannot be reproduced by an erase). Without bce this would require the default
/// attribute; with bce only the non-color attribute bits must be clear.
fn can_clear_with(c: Cell) -> bool {
    c.ch == ' ' && (c.attr & !crate::window::attrs::COLOR) == 0
}

/// Map a terminfo `ncv` (no_color_video) bitmask to the crate's attribute bits. terminfo numbers the
/// bits A_STANDOUT=1, A_UNDERLINE=2, A_REVERSE=4, A_BLINK=8, A_DIM=16, A_BOLD=32, A_INVIS=64,
/// A_PROTECT=128, A_ALTCHARSET=256 (terminfo(5)).
fn ncv_to_attr(ncv: u32) -> u32 {
    use crate::window::attrs;
    let map = [
        (1u32, attrs::STANDOUT),
        (2, attrs::UNDERLINE),
        (4, attrs::REVERSE),
        (8, attrs::BLINK),
        (16, attrs::DIM),
        (32, attrs::BOLD),
        (64, attrs::INVIS),
        (128, attrs::PROTECT),
        (256, attrs::ALTCHARSET),
    ];
    let mut out = 0;
    for (bit, a) in map {
        if ncv & bit != 0 {
            out |= a;
        }
    }
    out
}

/// The identity `acsc` map (each byte maps to itself) -- xterm/vt100/screen render the ACS drawing
/// chars as themselves, so the glyph byte is emitted unchanged.
fn identity_acsc() -> [u8; 128] {
    let mut m = [0u8; 128];
    for (i, b) in m.iter_mut().enumerate() {
        *b = i as u8;
    }
    m
}

/// Append a cell's character to `out` as UTF-8. For ASCII (`< 0x80`) this is the single byte the
/// plain path always emitted; single-width non-ASCII glyphs emit their multibyte UTF-8 encoding,
/// matching `ncursesw`'s per-cell wide-character output (one cell per single-width glyph).
fn push_ch(out: &mut Vec<u8>, ch: char) {
    let mut buf = [0u8; 4];
    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
}

/// Append `\e[<n><final>` (used for `erase_chars` 'X', `parm_ich` '@', `parm_dch` 'P').
fn csi1(out: &mut Vec<u8>, n: i32, fin: u8) {
    out.extend_from_slice(b"\x1b[");
    out.extend_from_slice(n.to_string().as_bytes());
    out.push(fin);
}

/// The physical screen model: `curscr` plus the physical cursor. Construct with [`Screen::new`]
/// (mirrors the post-`initscr` state: screen garbaged, cursor unknown), then call
/// [`Screen::doupdate`] with successive desired screens to get the exact bytes ncurses emits.
#[derive(Debug, Clone)]
pub struct Screen {
    rows: i32,
    cols: i32,
    cur: Vec<Cell>,
    cursrow: i32,
    curscol: i32,
    cleared: bool,
    vid: Vid,
    /// `clear_screen` bytes (xterm `\e[H\e[2J`; the Linux console uses `\e[H\e[J`).
    clear_seq: Vec<u8>,
    /// `clr_eol` / `clr_bol` / `clr_eos` byte sequences (with `$<N>` padding applied for the terminal).
    /// Default to the xterm constants; `from_terminfo` derives them so a no-`xon` padded `el`/`ed`
    /// emits its pad bytes.
    el_seq: Vec<u8>,
    el1_seq: Vec<u8>,
    ed_seq: Vec<u8>,
    /// Whether the terminal has `repeat_char` (`rep`). xterm does; the Linux console does not, so a
    /// run of identical glyphs is emitted one at a time instead of coalesced.
    has_rep: bool,
    /// `leaveok` -- suppress the final cursor park.
    leaveok: bool,
    /// The cursor-motion cost profile for `GoTo` (xterm by default; vt100 lacks `hpa`/`vpa`).
    caps: Caps,
    /// `clr_eol` / `clr_bol` normalized costs (xterm 3/4; vt100's `$<3>`-padded el/el1 cost 18/19,
    /// so ncurses paints spaces instead of erasing far more often).
    el_cost: i32,
    el1_cost: i32,
    /// Whether the terminal has `erase_chars` (`ech`) and `parm_ich`/`parm_dch` (`idc`). xterm does;
    /// vt100 has neither, so identical runs and shifts are painted out as literal cells.
    has_ech: bool,
    has_idc: bool,
    /// `erase_chars` (`ech`) / `repeat_char` (`rep`) cap templates and their `NormalizedCost`s, and
    /// `cursor_address`'s per-char cost -- all derived from terminfo so the run-coalescing in
    /// `emit_range` emits the terminal's own `ech`/`rep` bytes (e.g. concept's `\Er%p1%c...` rather
    /// than the ECMA `\e[Nb`). Emitted via `tparm` + `$<N>` padding ([`xon`]/[`pad_char`]).
    ech_seq: Vec<u8>,
    ech_cost: i32,
    rep_seq: Vec<u8>,
    rep_cost: i32,
    cup_ch_cost: i32,
    /// `xon`/`pad` for the emit-time padding of the variable-count `ech`/`rep` caps.
    xon: bool,
    pad_char: u8,
    /// `move_standout_mode` (`msgr`): when false, a cursor move while any attribute is on is unsafe,
    /// so ncurses resets to A_NORMAL before the move and restores the attribute after (lib_mvcur.c).
    msgr: bool,
    /// `back_color_erase` (`bce`): the terminal erases with the current background color. When color
    /// is on and `bce` is absent, the fast clears can't honor the bg color, so ClearScreen blank-fills.
    bce: bool,
    /// `auto_right_margin` (`am`): writing the last column wraps the cursor to the next line; lets the
    /// blank-fill stream rows contiguously (the per-line GoTo becomes a no-op).
    am: bool,
    /// `eat_newline_glitch` (`xenl`): the magic margin -- after the last column the cursor goes to
    /// "hyperspace" (unknown) rather than wrapping, so the next move emits an absolute cup.
    xenl: bool,
    /// Bottom-right-corner caps (`PutCharLR`): `smam`/`rmam` (enter/exit auto-margin), the insert
    /// caps `ich1`/`ich`/`smir`/`rmir`/`ip`. On an `am` terminal the last cell of the last row would
    /// scroll; ncurses suppresses am (rmam/smam), inserts (ich), or -- with none -- skips the cell.
    smam: Vec<u8>,
    rmam: Vec<u8>,
    ich1: Vec<u8>,
    ich: Vec<u8>,
    smir: Vec<u8>,
    rmir: Vec<u8>,
    ip: Vec<u8>,
    /// `acsc` glyph map for `A_ALTCHARSET` cells: ACS char (e.g. `q` = HLINE) -> the terminal's byte.
    /// Default identity (xterm/vt100/screen map the drawing chars to themselves); cygwin/pcansi remap
    /// to CP437 (`q` -> `0xC4`), so the glyph byte must be translated at emit time.
    acsc: [u8; 128],
}

impl Screen {
    /// A fresh physical screen: `curscr` all blanks, cursor position unknown (`-1, -1`), and the
    /// clear flag set so the first [`doupdate`](Self::doupdate) repaints from scratch -- exactly
    /// the state ncurses is in right after `initscr`. Uses an empty color palette.
    pub fn new(rows: i32, cols: i32) -> Screen {
        Screen::with_palette(rows, cols, Palette::new())
    }

    /// As [`Screen::new`] but with a pre-populated color palette (pair -> fg/bg), so attributed and
    /// colored cells emit the right SGR transitions.
    pub fn with_palette(rows: i32, cols: i32, palette: Palette) -> Screen {
        let rows = rows.max(1);
        let cols = cols.max(1);
        Screen {
            rows,
            cols,
            cur: vec![BLANK; (rows * cols) as usize],
            cursrow: -1,
            curscol: -1,
            cleared: true,
            vid: Vid::new(palette),
            clear_seq: CLEAR_SCREEN.to_vec(),
            el_seq: CLR_EOL.to_vec(),
            el1_seq: CLR_BOL.to_vec(),
            ed_seq: CLR_EOS.to_vec(),
            has_rep: true,
            leaveok: false,
            caps: Caps::xterm(),
            el_cost: 0, // xterm has back_color_erase (bce): ncurses forces _el_cost to 0
            el1_cost: EL1_COST,
            has_ech: true,
            has_idc: true,
            ech_seq: b"\x1b[%p1%dX".to_vec(),
            ech_cost: ECH_COST,
            rep_seq: b"%p1%c\x1b[%p2%{1}%-%db".to_vec(),
            rep_cost: REP_COST,
            cup_ch_cost: CUP_CH_COST,
            xon: false,
            pad_char: 0,
            msgr: true, // xterm has move_standout_mode
            bce: true,  // xterm has back_color_erase
            am: true,   // xterm has auto_right_margin
            xenl: true, // xterm has eat_newline_glitch
            smam: Vec::new(),
            rmam: Vec::new(),
            ich1: Vec::new(),
            ich: Vec::new(),
            smir: Vec::new(),
            rmir: Vec::new(),
            ip: Vec::new(),
            acsc: identity_acsc(),
        }
    }

    /// Override the `acsc` glyph map for `A_ALTCHARSET` cells. `pairs` is the terminfo `acsc` string
    /// (alternating ACS-char, terminal-byte), e.g. cygwin's `q\xc4...`; unlisted chars stay identity.
    pub fn set_acsc(&mut self, pairs: &[u8]) {
        self.acsc = identity_acsc();
        let mut i = 0;
        while i + 1 < pairs.len() {
            let (acs, glyph) = (pairs[i], pairs[i + 1]);
            if (acs as usize) < 128 {
                self.acsc[acs as usize] = glyph;
            }
            i += 2;
        }
    }

    /// Build a `Screen` configured entirely from a loaded terminfo entry -- the terminal-general
    /// `doupdate` path. Derives the cursor cost model ([`Caps::from_terminfo`]), the `clear_screen`
    /// bytes and `rep` presence, the `el`/`el1` shaping costs (`NormalizedCost`) and `ech`/idc
    /// availability, the `acsc` glyph map, the `sgr`/`sgr0`/`rmacs` SGR engine, and the `ncv` mask --
    /// so the screen optimizer drives an arbitrary terminal rather than a hand-coded bundle.
    pub fn from_terminfo(rows: i32, cols: i32, t: &Terminfo, palette: Palette) -> Screen {
        let mut s = Screen::with_palette(rows, cols, palette);
        s.set_caps(Caps::from_terminfo(t));
        // clear_screen bytes with `$<N>` padding applied per the terminal's xon/pad: a no-xon
        // terminal emits the pad bytes after the clear (often a large `$<N>` delay), an xon terminal
        // strips them.
        let xon = t.tigetflag("xon") > 0;
        let pad_char = t
            .string("pad")
            .and_then(|p| p.first().copied())
            .unwrap_or(0);
        // clear_screen is cap-driven: a terminal without a `clear` cap must NOT be sent a hardcoded
        // `\e[H\e[2J` (it would be garbage). ncurses' ClearScreen falls back to clr_eos, then
        // per-line clr_eol, then a blank-fill -- reproduced in `clear_screen`. So leave clear_seq
        // empty here when the cap is absent.
        let clear = match t.string("clear") {
            Some(c) => apply_padding(c, xon, pad_char),
            None => Vec::new(),
        };
        s.set_term_caps(&clear, t.string("rep").is_some());
        // TransformLine shaping: el/el1 normalized costs; ech and ich/dch availability. An absent
        // erase cap gets cost ABSENT_COST so the cost comparisons never pick it (ncurses then falls
        // back to overwriting with blanks), and an empty byte sequence. A `bce` (back_color_erase)
        // terminal forces _el_cost to 0 (lib_mvcur.c) -- ncurses biases toward clr_eol over trailing
        // spaces, since the erase fills with the current background. el1 is *not* zeroed.
        let bce = t.tigetflag("bce") > 0;
        let el_cost = match t.string("el") {
            Some(_) if bce => 0,
            Some(el) => normalized_cost(el),
            None => ABSENT_COST,
        };
        let el1_cost = t.string("el1").map(normalized_cost).unwrap_or(ABSENT_COST);
        let has_ech = t.string("ech").is_some();
        let has_idc = t.string("ich").is_some()
            || t.string("ich1").is_some()
            || t.string("dch").is_some()
            || t.string("dch1").is_some();
        s.set_shape_caps(el_cost, el1_cost, has_ech, has_idc);
        // The clr_eol/clr_bol/clr_eos byte sequences, with `$<N>` padding applied (a no-xon padded
        // `el`/`ed` emits its pad bytes after the erase). Absent caps are reset to empty so the
        // ClearScreen / ClrBottom fallbacks (which key off emptiness) fire instead of a stale default.
        s.el_seq = t
            .string("el")
            .map(|el| apply_padding(el, xon, pad_char))
            .unwrap_or_default();
        s.el1_seq = t
            .string("el1")
            .map(|el1| apply_padding(el1, xon, pad_char))
            .unwrap_or_default();
        s.ed_seq = t
            .string("ed")
            .map(|ed| apply_padding(ed, xon, pad_char))
            .unwrap_or_default();
        // erase_chars / repeat_char cap templates + NormalizedCosts (ncurses _ech_cost = cost at
        // sample 23; _rep_cost = cost at sample char ' ', count 23), and cursor_address's per-char
        // cost (_cup_ch_cost at sample 23,23). These drive the run-coalescing in emit_range so it
        // emits the terminal's own ech/rep bytes. xon/pad_char are kept for emit-time padding.
        s.xon = xon;
        s.pad_char = pad_char;
        if let Some(ech) = t.string("ech") {
            s.ech_seq = ech.to_vec();
            s.ech_cost = normalized_cost(&tparm_n(ech, &[23]));
        }
        if let Some(rep) = t.string("rep") {
            s.rep_seq = rep.to_vec();
            s.rep_cost = normalized_cost(&tparm_n(rep, &[b' ' as i32, 23]));
        }
        s.cup_ch_cost = t
            .string("cup")
            .map(|cup| normalized_cost(&tparm_n(cup, &[23, 23])))
            .unwrap_or(CUP_CH_COST);
        if let Some(acsc) = t.string("acsc") {
            s.set_acsc(acsc);
        }
        // Attribute (SGR) engine. `set_attributes` (sgr) drives modern terminals; ~46% of the
        // database has no sgr and is driven by the individual mode caps (bold/smul/smso/rev/...),
        // ncurses' no-sgr `vidputs` branch. All emissions are padded (xon/pad). sgr0/rmacs default
        // to empty when absent so the engine emits nothing rather than a hardcoded reset.
        let has_sgr = t.string("sgr").is_some();
        s.set_attr_caps(
            t.string("sgr").unwrap_or(b""),
            t.string("sgr0").unwrap_or(b""),
            t.string("rmacs").unwrap_or(b""),
        );
        s.set_mode_caps(
            has_sgr,
            [
                t.string("smacs"),
                t.string("blink"),
                t.string("bold"),
                t.string("dim"),
                t.string("rev"),
                t.string("smso"),
                t.string("prot"),
                t.string("invis"),
                t.string("smul"),
            ],
            t.string("rmul"),
            t.string("rmso"),
            xon,
            pad_char,
        );
        let ncv = t.tigetnum("ncv");
        if ncv > 0 {
            s.set_ncv(ncv_to_attr(ncv as u32));
        }
        // Color engine (_nc_do_color): setaf/setab (ANSI) or setf/setb (toggled_colors), plus
        // orig_pair. has_color gates emission -- a terminal with no `colors`/no cap emits no color.
        let has_color = t.tigetnum("colors") > 0;
        let (setaf, toggled) = match (t.string("setaf"), t.string("setf")) {
            (Some(a), _) => (Some(a), false),
            (None, Some(f)) => (Some(f), true),
            (None, None) => (None, false),
        };
        let setab = t.string("setab").or_else(|| t.string("setb"));
        s.set_color_caps(
            setaf,
            setab,
            t.string("scp"),
            t.string("op"),
            toggled,
            has_color,
        );
        s.msgr = t.tigetflag("msgr") > 0;
        s.bce = t.tigetflag("bce") > 0;
        s.am = t.tigetflag("am") > 0;
        s.xenl = t.tigetflag("xenl") > 0;
        // Bottom-right corner caps (padded), for the blank-fill PutCharLR.
        let pad_cap = |name: &str| {
            t.string(name)
                .map(|c| apply_padding(c, xon, pad_char))
                .unwrap_or_default()
        };
        s.smam = pad_cap("smam");
        s.rmam = pad_cap("rmam");
        s.ich1 = t.string("ich1").map(|c| c.to_vec()).unwrap_or_default();
        s.ich = t.string("ich").map(|c| c.to_vec()).unwrap_or_default();
        s.smir = pad_cap("smir");
        s.rmir = pad_cap("rmir");
        s.ip = pad_cap("ip");
        s
    }

    /// Emit a cell's glyph: an `A_ALTCHARSET` cell is translated through `acsc` (ACS char -> the
    /// terminal's line-drawing byte); any other cell emits its UTF-8. Any combining marks
    /// (`cchar_t.chars[1..]`) are emitted as UTF-8 right after the base, exactly as `ncursesw` does.
    fn emit_glyph(&self, out: &mut Vec<u8>, c: Cell) {
        if c.attr & crate::window::attrs::ALTCHARSET != 0 && (c.ch as u32) < 128 {
            out.push(self.acsc[c.ch as usize]);
        } else {
            push_ch(out, c.ch);
        }
        for &m in c.comb.iter() {
            if m == '\0' {
                break;
            }
            push_ch(out, m);
        }
    }

    /// Override the cursor-motion cost profile (e.g. [`Caps::vt100`]) so `GoTo` chooses the right
    /// motions for a terminal without `hpa`/`vpa`.
    pub fn set_caps(&mut self, caps: Caps) {
        self.caps = caps;
    }

    /// Override the TransformLine shaping caps for another terminal: the `clr_eol`/`clr_bol`
    /// normalized costs and whether `erase_chars` (`ech`) and `parm_ich`/`parm_dch` (`idc`) exist.
    /// (xterm: 3/4, both present; vt100: 18/19 from `$<3>` padding, neither present.)
    pub fn set_shape_caps(&mut self, el_cost: i32, el1_cost: i32, has_ech: bool, has_idc: bool) {
        self.el_cost = el_cost;
        self.el1_cost = el1_cost;
        self.has_ech = has_ech;
        self.has_idc = has_idc;
    }

    /// Override the terminal capabilities that differ between terminals sharing the cursor cap set:
    /// the `clear_screen` byte sequence and whether `repeat_char` (`rep`) exists. (xterm: `\e[H\e[2J`,
    /// rep present; Linux console: `\e[H\e[J`, no rep.)
    pub fn set_term_caps(&mut self, clear_seq: &[u8], has_rep: bool) {
        self.clear_seq = clear_seq.to_vec();
        self.has_rep = has_rep;
    }

    /// Override the attribute caps (`sgr`/`sgr0`/`rmacs`) on the SGR engine for another terminal.
    pub fn set_attr_caps(&mut self, sgr: &[u8], sgr0: &[u8], rmacs: &[u8]) {
        self.vid.set_attr_caps(sgr, sgr0, rmacs);
    }

    /// Set the individual mode caps + `has_sgr`/`xon`/`pad` on the SGR engine (the no-`sgr` path).
    #[allow(clippy::too_many_arguments)]
    pub fn set_mode_caps(
        &mut self,
        has_sgr: bool,
        enter: [Option<&[u8]>; 9],
        rmul: Option<&[u8]>,
        rmso: Option<&[u8]>,
        xon: bool,
        pad_char: u8,
    ) {
        self.vid
            .set_mode_caps(has_sgr, enter, rmul, rmso, xon, pad_char);
    }

    /// Set the `no_color_video` mask (attribute bits the terminal suppresses while color is on).
    pub fn set_ncv(&mut self, ncv: u32) {
        self.vid.set_ncv(ncv);
    }

    /// Set the color caps (`setaf`/`setab` or `setf`/`setb`, `orig_pair`) on the SGR engine.
    #[allow(clippy::too_many_arguments)]
    pub fn set_color_caps(
        &mut self,
        setaf: Option<&[u8]>,
        setab: Option<&[u8]>,
        scp: Option<&[u8]>,
        op: Option<&[u8]>,
        toggled: bool,
        has_color: bool,
    ) {
        self.vid
            .set_color_caps(setaf, setab, scp, op, toggled, has_color);
    }

    fn idx(&self, r: i32, c: i32) -> usize {
        (r * self.cols + c) as usize
    }

    /// `init_pair(pair, fg, bg)` -- define a color pair on the screen's attribute engine so colored
    /// cells emit the right SGR transitions.
    pub fn init_pair(&mut self, pair: i16, fg: i16, bg: i16) -> bool {
        self.vid.init_pair(pair, fg, bg)
    }

    /// `start_color()` -- enable color processing on the attribute engine.
    pub fn start_color(&mut self) {
        self.vid.start_color();
    }

    /// `clearok` -- force the next [`doupdate`](Self::doupdate) to clear the screen and repaint every
    /// line from scratch (a `ClrUpdate`), exactly as ncurses does when a window is `clearok`.
    pub fn clearok(&mut self) {
        self.cleared = true;
    }

    /// `leaveok` -- when set, [`doupdate`](Self::doupdate) does not move the cursor to the parked
    /// position at the end (it is left wherever the last update left it), matching ncurses.
    pub fn set_leaveok(&mut self, on: bool) {
        self.leaveok = on;
    }

    /// The bytes a `GoTo(row, col)` would emit from the current cursor (without moving), using the
    /// `ovw=TRUE` overwrite branch against the desired screen `new`. Overwrite is only offered for
    /// cells whose attribute matches the current SCREEN_ATTRS (xterm has `msgr`, so motion does not
    /// reset attributes -- but an overwrite emits the cell glyph under the *current* attribute, so
    /// it is only valid when they match).
    fn goto_bytes(&self, new: &[Cell], row: i32, col: i32) -> Vec<u8> {
        if self.cursrow < 0 || self.curscol < 0 || self.curscol >= self.cols {
            // Position unknown, or the cursor is at/past the right margin: under auto_right_margin +
            // eat_newline_glitch (xterm) the physical column after writing the last cell is
            // ambiguous (delayed wrap), so ncurses forces an absolute cursor_address rather than a
            // relative motion. Drive it through mvcur with an *unknown* source (fy=fx=-1), which is
            // exactly ncurses' `mvcur(-1, -1, row, col)`: it emits the terminal's own `cup` (whatever
            // its format), or *nothing* when the terminal has no `cup` (no hardcoded CSI). For xterm
            // this is byte-identical to the old `\e[r;cH`.
            mvcur_ovw_caps(
                &self.caps,
                (0, 0),
                (row + 1, col + 1),
                want_fn(new, self.cols, self.vid.current()),
            )
        } else {
            mvcur_ovw_caps(
                &self.caps,
                (self.cursrow + 1, self.curscol + 1),
                (row + 1, col + 1),
                want_fn(new, self.cols, self.vid.current()),
            )
        }
    }

    /// `GoTo(row, col)` -- move the physical cursor via the byte-exact `mvcur` optimizer (with the
    /// `ovw=TRUE` overwrite branch), or via a direct cursor-address when the position is unknown.
    ///
    /// ncurses' `_nc_real_mvcur` resets video to A_NORMAL before moving whenever the current
    /// attribute has `A_ALTCHARSET` (line-drawing breaks the `\r`/`\n` local motions), regardless
    /// of `msgr`; reproduce that here so a move out of a line-drawing run emits `rmacs` first.
    fn goto(&mut self, out: &mut Vec<u8>, new: &[Cell], row: i32, col: i32) {
        // A no-op move emits nothing -- ncurses' mvcur returns early when already at the target,
        // so it does not reset/restore altcharset either (the final cleanup handles that).
        if self.cursrow == row && self.curscol == col && self.cursrow >= 0 {
            return;
        }
        // ncurses' _nc_real_mvcur turns video off before the move and restores it after
        // (VIDPUTS(A_NORMAL) ... move ... VIDPUTS(oldattr)) whenever the current attribute has
        // A_ALTCHARSET (line-drawing breaks the \r/\n local motions), OR any attribute is on and the
        // terminal lacks move_standout_mode (msgr). ncurses' `AttrOf` is A_ATTRIBUTES, which includes
        // A_COLOR, so a color-only cell on a no-msgr terminal also resets/restores around the move.
        let saved = self.vid.current();
        let attr_on = saved & (ATTR_BITS | crate::window::attrs::COLOR);
        let reset_needed =
            (saved & crate::window::attrs::ALTCHARSET != 0) || (attr_on != 0 && !self.msgr);
        if reset_needed {
            let reset = self.vid.set(0);
            out.extend_from_slice(&reset);
        }
        let bytes = self.goto_bytes(new, row, col);
        out.extend_from_slice(&bytes);
        self.cursrow = row;
        self.curscol = col;
        if reset_needed {
            let restore = self.vid.set(saved);
            out.extend_from_slice(&restore);
        }
    }

    /// `PutChar`: emit `UpdateAttrs` (the SGR transition to this cell's attribute/color) then the
    /// character byte, and advance the cursor.
    fn put_char(&mut self, out: &mut Vec<u8>, c: Cell) {
        let sgr = self.vid.set(c.attr);
        out.extend_from_slice(&sgr);
        // Bottom-right corner: writing the last cell of the last row would trigger an auto-margin
        // scroll, so ncurses disables auto_right_margin (rmam, `\e[?7l`) around that one character
        // and restores it (smam, `\e[?7h`) -- the lower-right-corner write (OPT-02).
        let corner = self.cursrow == self.rows - 1 && self.curscol == self.cols - 1;
        if corner {
            out.extend_from_slice(b"\x1b[?7l");
            self.emit_glyph(out, c);
            out.extend_from_slice(b"\x1b[?7h");
            // With auto-margin disabled the cursor does not advance past the last column; it stays
            // on it (a known position, so the next move uses relative motion, not an absolute cup).
            self.curscol = self.cols - 1;
            return;
        }
        self.emit_glyph(out, c);
        // A double-width glyph occupies (and advances) two columns; its padding cell is consumed
        // by the caller and never emitted on its own.
        self.curscol += char_width(c.ch);
    }

    /// `UpdateAttrs(blank)` -- emit the SGR transition to `mode` (used before clearing ops so the
    /// cleared region takes the right attribute/color).
    fn update_attrs(&mut self, out: &mut Vec<u8>, mode: u32) {
        let sgr = self.vid.set(mode);
        out.extend_from_slice(&sgr);
    }

    /// `EmitRange(ntext, num)` over `new` line `row` starting at `base`. Reproduces the
    /// `erase_chars` (`ech`) and `repeat_char` (`rep`) run-coalescing. Returns `1` if the cursor
    /// was left in the middle of the interval (an `ech` that ended the interval), else `0`.
    fn emit_range(
        &mut self,
        out: &mut Vec<u8>,
        new: &[Cell],
        row: i32,
        base: i32,
        num: i32,
    ) -> i32 {
        let mut idx = base;
        let mut n = num;
        while n > 0 {
            let c0 = new[self.idx(row, idx)];
            // Double-width glyph: emit its UTF-8 once (advancing two columns) and consume its
            // padding cell. ncursesw never rep/ech-coalesces wide glyphs and runs of width-1 cells
            // never include one (a glyph and its padding cell differ), so the ASCII path below is
            // unchanged.
            if char_width(c0.ch) == 2 {
                self.put_char(out, c0);
                let consumed = 2.min(n);
                idx += consumed;
                n -= consumed;
                continue;
            }
            // Emit a leading width-1 character that differs from its successor, one at a time.
            if n > 1 && new[self.idx(row, idx)] != new[self.idx(row, idx + 1)] {
                self.put_char(out, c0);
                idx += 1;
                n -= 1;
                continue;
            }
            if n == 1 {
                self.put_char(out, c0);
                return 0;
            }
            let mut runcount = 2;
            while runcount < n && new[self.idx(row, idx + runcount)] == c0 {
                runcount += 1;
            }
            if self.has_ech && runcount > self.ech_cost + self.cup_ch_cost && can_clear_with(c0) {
                // erase_chars is cheaper than emitting the run -- emit the terminal's own ech cap
                // (tparm at the run length, with padding), not a hardcoded `\e[NX`.
                self.update_attrs(out, c0.attr);
                let ech = apply_padding(
                    &tparm_n(&self.ech_seq, &[runcount]),
                    self.xon,
                    self.pad_char,
                );
                out.extend_from_slice(&ech);
                self.cur_fill(row, idx, runcount, c0);
                if runcount < n {
                    let nc = self.curscol + runcount;
                    self.goto(out, new, row, nc);
                } else {
                    return 1;
                }
            } else if self.has_rep
                && runcount > self.rep_cost
                && c0.ch.is_ascii()
                && c0.comb[0] == '\0'
            {
                // repeat_char: emit the terminal's own rep cap via tparm(rep, char, count) -- this
                // reproduces xterm's `char + \e[count-1 b` and concept's `\Er<char><count>` alike,
                // with `$<N>` padding. A possible right-margin wrap is handled with a trailing PutChar.
                // Only for ASCII: ncursesw does not coalesce non-ASCII (multibyte) runs with rep
                // (the `!_screen_unicode || CharOf < 256` guard in EmitRange). Verified by NCURSES.WIDECHAR.
                let wrap_possible = self.curscol + runcount >= self.cols;
                let rep_count = if wrap_possible {
                    runcount - 1
                } else {
                    runcount
                };
                self.update_attrs(out, c0.attr);
                // An altcharset cell repeats its acsc-translated glyph byte (ncurses' acs_map step).
                let ch = if c0.attr & crate::window::attrs::ALTCHARSET != 0 && (c0.ch as u32) < 128
                {
                    self.acsc[c0.ch as usize] as i32
                } else {
                    c0.ch as i32
                };
                let rep = apply_padding(
                    &tparm_n(&self.rep_seq, &[ch, rep_count]),
                    self.xon,
                    self.pad_char,
                );
                out.extend_from_slice(&rep);
                self.cur_fill(row, idx, rep_count, c0);
                self.curscol += rep_count;
                if wrap_possible {
                    self.put_char(out, c0);
                }
            } else {
                for k in 0..runcount {
                    let c = new[self.idx(row, idx + k)];
                    self.put_char(out, c);
                }
            }
            idx += runcount;
            n -= runcount;
        }
        0
    }

    /// `PutRange(otext=curscr, ntext=new, row, first, last)` -- emit the span, skipping runs of
    /// already-correct cells longer than `_inline_cost` with a `GoTo`. Returns the `EmitRange`
    /// cursor-state code (1 if the cursor was left mid-interval / a trailing identical run exists).
    fn put_range(
        &mut self,
        out: &mut Vec<u8>,
        new: &[Cell],
        row: i32,
        first: i32,
        last: i32,
    ) -> i32 {
        if last - first + 1 > INLINE_COST {
            let mut f = first;
            let mut same = 0;
            let mut j = first;
            while j <= last {
                if self.cur[self.idx(row, j)] == new[self.idx(row, j)] {
                    same += 1;
                } else {
                    if same > INLINE_COST {
                        self.emit_range(out, new, row, f, j - same - f);
                        f = j;
                        self.goto(out, new, row, j);
                    }
                    same = 0;
                }
                j += 1;
            }
            let i = self.emit_range(out, new, row, f, j - same - f);
            if same == 0 {
                i
            } else {
                1
            }
        } else {
            self.emit_range(out, new, row, first, last - first + 1)
        }
    }

    /// `ClrToEOL(blank, needclear)` at the current cursor row, from the current column.
    fn clr_to_eol(&mut self, out: &mut Vec<u8>, blank: Cell, mut needclear: bool) {
        let row = self.cursrow;
        if row >= 0 {
            for j in self.curscol..self.cols {
                if j >= 0 && self.cur[self.idx(row, j)] != blank {
                    let k = self.idx(row, j);
                    self.cur[k] = blank;
                    needclear = true;
                }
            }
        }
        if needclear {
            self.update_attrs(out, blank.attr);
            if self.el_cost <= self.cols - self.curscol {
                out.extend_from_slice(&self.el_seq);
            } else {
                let count = self.cols - self.curscol;
                for _ in 0..count {
                    self.put_char(out, blank);
                }
            }
        }
    }

    /// Set `curscr` cells `[col .. col+n)` on `row` to `c` (the code's internal sync for `ech`).
    fn cur_fill(&mut self, row: i32, col: i32, n: i32, c: Cell) {
        for k in 0..n {
            let i = self.idx(row, col + k);
            self.cur[i] = c;
        }
    }

    /// `TransformLine(lineno)` -- the plain-path branch (`_coloron == false`, no xmc, no
    /// `ceol_standout_glitch`).
    fn transform_line(&mut self, out: &mut Vec<u8>, new: &[Cell], lineno: i32) {
        let cols = self.cols;
        let mut first_char = 0;

        // clr_bol leading-whitespace optimization.
        let blank0 = new[self.idx(lineno, 0)];
        if can_clear_with(blank0) {
            let mut o_first = 0;
            while o_first < cols && self.cur[self.idx(lineno, o_first)] == blank0 {
                o_first += 1;
            }
            let mut n_first = 0;
            while n_first < cols && new[self.idx(lineno, n_first)] == blank0 {
                n_first += 1;
            }
            if n_first == o_first {
                first_char = n_first;
                while first_char < cols
                    && new[self.idx(lineno, first_char)] == self.cur[self.idx(lineno, first_char)]
                {
                    first_char += 1;
                }
            } else if o_first > n_first {
                // New has fewer leading blanks than the old line: ncurses repositions straight to
                // the first non-blank (TransformLine: `firstChar = nFirstChar`), unconditionally --
                // the following GoTo(lineno, firstChar) handles the move with the cheapest cap.
                first_char = n_first;
            } else {
                first_char = o_first;
                if self.el1_cost < n_first - o_first {
                    if n_first >= cols && self.el_cost <= self.el1_cost {
                        self.goto(out, new, lineno, 0);
                        self.update_attrs(out, blank0.attr);
                        out.extend_from_slice(&self.el_seq);
                        first_char = 0;
                    } else {
                        self.goto(out, new, lineno, n_first - 1);
                        self.update_attrs(out, blank0.attr);
                        out.extend_from_slice(&self.el1_seq);
                        // clr_bol leaves the cursor on column n_first-1; ncurses resumes the line
                        // diff from there (the cleared cell is re-emitted by the following range).
                        first_char = n_first - 1;
                    }
                    let mut c = 0;
                    while c < n_first {
                        let i = self.idx(lineno, c);
                        self.cur[i] = blank0;
                        c += 1;
                    }
                }
            }
        } else {
            while first_char < cols
                && new[self.idx(lineno, first_char)] == self.cur[self.idx(lineno, first_char)]
            {
                first_char += 1;
            }
        }

        if first_char >= cols {
            return;
        }

        let blank = new[self.idx(lineno, cols - 1)];

        if !can_clear_with(blank) {
            let mut n_last = cols - 1;
            while n_last > first_char
                && new[self.idx(lineno, n_last)] == self.cur[self.idx(lineno, n_last)]
            {
                n_last -= 1;
            }
            if n_last >= first_char {
                self.goto(out, new, lineno, first_char);
                self.put_range(out, new, lineno, first_char, n_last);
                self.cur_copy(new, lineno, first_char, n_last);
            }
            return;
        }

        // find last non-blank on old and new lines
        let mut o_last = cols - 1;
        while o_last > first_char && self.cur[self.idx(lineno, o_last)] == blank {
            o_last -= 1;
        }
        let mut n_last = cols - 1;
        while n_last > first_char && new[self.idx(lineno, n_last)] == blank {
            n_last -= 1;
        }

        if n_last == first_char && self.el_cost < o_last - n_last {
            self.goto(out, new, lineno, first_char);
            let nf = new[self.idx(lineno, first_char)];
            if nf != blank {
                self.put_char(out, nf);
            }
            self.clr_to_eol(out, blank, false);
        } else if n_last != o_last
            && (new[self.idx(lineno, n_last)] != self.cur[self.idx(lineno, o_last)]
                || !self.has_idc)
        {
            // ncurses gates the insert/delete-char detection on `can_idc` (idcok && has ic/dc); when
            // the terminal has no idc (e.g. vt100) it always takes this plain-redraw branch. On xterm
            // can_idc is true, so the gate reduces to the char inequality (unchanged behaviour).
            self.goto(out, new, lineno, first_char);
            if o_last - n_last > self.el_cost {
                if self.put_range(out, new, lineno, first_char, n_last) != 0 {
                    self.goto(out, new, lineno, n_last + 1);
                }
                self.clr_to_eol(out, blank, false);
            } else {
                let n = n_last.max(o_last);
                self.put_range(out, new, lineno, first_char, n);
            }
        } else {
            // insert/delete-character path
            let n_last_nonblank = n_last;
            let o_last_nonblank = o_last;
            let mut nl = n_last;
            let mut ol = o_last;
            while nl >= 0 && ol >= 0 && new[self.idx(lineno, nl)] == self.cur[self.idx(lineno, ol)]
            {
                nl -= 1;
                ol -= 1;
            }
            let n = ol.min(nl);
            if n >= first_char {
                self.goto(out, new, lineno, first_char);
                self.put_range(out, new, lineno, first_char, n);
            }
            if ol < nl {
                let m = n_last_nonblank.max(o_last_nonblank);
                self.goto(out, new, lineno, n + 1);
                if !self.has_idc || nl < n_last_nonblank || ICH_COST > m - n {
                    self.put_range(out, new, lineno, n + 1, m);
                } else {
                    // InsStr: parm_ich(count) then the inserted characters.
                    let count = nl - ol;
                    csi1(out, count, b'@');
                    for k in 0..count {
                        let c = new[self.idx(lineno, n + 1 + k)];
                        self.put_char(out, c);
                    }
                }
            } else if ol > nl {
                self.goto(out, new, lineno, n + 1);
                if !self.has_idc || DCH_COST > self.el_cost + n_last_nonblank - (n + 1) {
                    if self.put_range(out, new, lineno, n + 1, n_last_nonblank) != 0 {
                        self.goto(out, new, lineno, n_last_nonblank + 1);
                    }
                    self.clr_to_eol(out, blank, false);
                } else {
                    // DelChar: parm_dch(count).
                    csi1(out, ol - nl, b'P');
                }
            }
        }

        // sync curscr to newscr from first_char to end of line
        if cols > first_char {
            self.cur_copy(new, lineno, first_char, cols - 1);
        }
    }

    /// Copy `new[lineno][lo..=hi]` into `curscr`.
    fn cur_copy(&mut self, new: &[Cell], lineno: i32, lo: i32, hi: i32) {
        for j in lo..=hi {
            let i = self.idx(lineno, j);
            self.cur[i] = new[i];
        }
    }

    /// `ClrBottom(total)` -- clear a trailing run of now-blank lines with `clr_eos` when cheaper.
    fn clr_bottom(&mut self, out: &mut Vec<u8>, new: &[Cell], total: i32) -> i32 {
        let mut top = total;
        let last = self.cols;
        let blank = new[self.idx(total - 1, last - 1)];
        // ncurses' ClrBottom is gated on `clr_eos` existing; without it the trailing blank lines are
        // left to TransformLine.
        if !self.ed_seq.is_empty() && can_clear_with(blank) {
            let mut row = total - 1;
            while row >= 0 {
                let new_blank = (0..last).all(|c| new[self.idx(row, c)] == blank);
                if !new_blank {
                    break;
                }
                let cur_blank = (0..last).all(|c| self.cur[self.idx(row, c)] == blank);
                if !cur_blank {
                    top = row;
                }
                row -= 1;
            }
            if top < total {
                self.goto(out, new, top, 0);
                self.clr_to_eos(out, blank);
            }
        }
        top
    }

    /// `ClrToEOS(blank)` -- emit `clr_eos` and blank `curscr` from the cursor to the bottom.
    fn clr_to_eos(&mut self, out: &mut Vec<u8>, blank: Cell) {
        let mut row = self.cursrow.max(0);
        let mut col = self.curscol.max(0);
        self.update_attrs(out, blank.attr);
        out.extend_from_slice(&self.ed_seq);
        while col < self.cols {
            let i = self.idx(row, col);
            self.cur[i] = blank;
            col += 1;
        }
        row += 1;
        while row < self.rows {
            for c in 0..self.cols {
                let i = self.idx(row, c);
                self.cur[i] = blank;
            }
            row += 1;
        }
    }

    /// `ClearScreen(blank)` -- set the background color, emit `clear_screen`, then home the cursor.
    /// `UpdateAttrs(blank)` comes *before* the clear so a `bce` terminal fills the screen with the
    /// background color (the colored `wbkgd` first paint); `curscr` is left as plain blanks so the
    /// following `ClrBottom` still emits its `clr_eos` for the trailing region, matching ncurses.
    fn clear_screen(&mut self, out: &mut Vec<u8>, new: &[Cell], blank: Cell) {
        // ncurses ClearScreen begins with a color-init (`_nc_do_color(SCREEN_PAIR, 0)`) when color is
        // on. For a *default* background (blank carries no color pair) that is `_nc_do_color(0, 0)` --
        // op + default fg/bg -- emitted before the clear. A colored background (pair != 0) keeps the
        // existing UpdateAttrs(blank) path below, so the BG_PAINT court is unaffected.
        if (blank.attr & crate::window::attrs::COLOR) == 0 {
            out.extend(self.vid.color_init_default());
        }
        // ncurses ClearScreen (tty_update.c): a 4-way priority, fully cap-driven. When color is on and
        // the terminal lacks `bce` (back_color_erase), the fast clears (clear_screen/clr_eos/clr_eol)
        // would NOT erase with the background color, so ncurses sets `fast_clear = FALSE` and
        // blank-fills instead -- reproduced here by suppressing cases 1-3.
        let fast_clear = !self.vid.color_active() || self.bce;
        if fast_clear && !self.clear_seq.is_empty() {
            // 1. clear_screen: UpdateAttrs(blank), emit clear, cursor -> home.
            self.update_attrs(out, blank.attr);
            let clr = self.clear_seq.clone();
            out.extend_from_slice(&clr);
            self.cursrow = 0;
            self.curscol = 0;
        } else if fast_clear && !self.ed_seq.is_empty() {
            // 2. clr_eos: cursor unknown, GoTo(0,0), UpdateAttrs(blank), emit clr_eos.
            self.cursrow = -1;
            self.curscol = -1;
            self.goto(out, new, 0, 0);
            self.update_attrs(out, blank.attr);
            let ed = self.ed_seq.clone();
            out.extend_from_slice(&ed);
        } else if fast_clear && !self.el_seq.is_empty() {
            // 3. clr_eol per line: UpdateAttrs(blank), then GoTo(i,0)+clr_eol for every line, GoTo(0,0).
            self.cursrow = -1;
            self.curscol = -1;
            self.update_attrs(out, blank.attr);
            let el = self.el_seq.clone();
            for i in 0..self.rows {
                self.goto(out, new, i, 0);
                out.extend_from_slice(&el);
            }
            self.goto(out, new, 0, 0);
        } else {
            // 4. blank-fill: UpdateAttrs(blank), then write a full row of blanks per line and GoTo(0,0).
            // With auto_right_margin, writing the last column wraps the cursor to the next line's
            // column 0, so the per-line GoTo(i,0) is a no-op -- ncurses streams the rows contiguously
            // (no cup between them). Without `am` each row needs an explicit GoTo.
            self.update_attrs(out, blank.attr);
            for i in 0..self.rows {
                self.goto(out, new, i, 0);
                for j in 0..self.cols {
                    self.curscol = j;
                    if i == self.rows - 1 && j == self.cols - 1 {
                        // Bottom-right corner: PutCharLR (suppress am / insert / skip).
                        self.corner_blank(out, new, blank);
                    } else {
                        self.emit_glyph(out, blank);
                    }
                }
                // After a full row, ncurses' wrap_cursor fires (PutChar reached the right margin).
                // The last row uses PutCharLR, which does NOT wrap. xenl (magic margin) sends the
                // cursor to "hyperspace" (-1,-1) so the next row's GoTo emits an absolute cup; a
                // clean `am` wraps to (i+1, 0) making the GoTo a no-op; without `am` it stays.
                if i < self.rows - 1 {
                    if self.xenl {
                        self.cursrow = -1;
                        self.curscol = -1;
                    } else if self.am {
                        self.cursrow = i + 1;
                        self.curscol = 0;
                    } else {
                        self.curscol = self.cols;
                    }
                }
            }
            self.goto(out, new, 0, 0);
        }
        for c in self.cur.iter_mut() {
            *c = BLANK;
        }
    }

    /// `PutCharLR` for a blank at the bottom-right corner: on an `am` terminal the last cell would
    /// scroll, so ncurses (1) writes directly if `!am`; (2) suppresses am via rmam/char/smam; (3)
    /// goes to col-2, writes, returns, and inserts the char (ich/smir); or (4) -- with none of those
    /// -- skips the cell entirely. Entered with the cursor at `(rows-1, cols-1)`.
    fn corner_blank(&mut self, out: &mut Vec<u8>, new: &[Cell], c: Cell) {
        let (xon, pad) = (self.xon, self.pad_char);
        if !self.am {
            self.emit_glyph(out, c);
            self.curscol += 1;
        } else if !self.smam.is_empty() && !self.rmam.is_empty() {
            let oldcol = self.curscol;
            out.extend(apply_padding(&self.rmam, xon, pad));
            self.emit_glyph(out, c);
            self.curscol = oldcol; // am suppressed: the cursor stays on the last column
            out.extend(apply_padding(&self.smam, xon, pad));
        } else if (!self.smir.is_empty() && !self.rmir.is_empty())
            || !self.ich1.is_empty()
            || !self.ich.is_empty()
        {
            self.goto(out, new, self.rows - 1, self.cols - 2);
            self.emit_glyph(out, c);
            self.curscol += 1;
            self.goto(out, new, self.rows - 1, self.cols - 2);
            self.ins_one(out, c);
        }
        // (4) no am-suppression and no insert: the corner cell is left unwritten (ncurses skips it).
    }

    /// `InsStr` of a single char (the corner insert): `parm_ich`, else `smir`/`rmir` (with `ip`),
    /// else `ich1` (with `ip`).
    fn ins_one(&mut self, out: &mut Vec<u8>, c: Cell) {
        let (xon, pad) = (self.xon, self.pad_char);
        if !self.ich.is_empty() {
            out.extend(apply_padding(&tparm_n(&self.ich, &[1]), xon, pad));
            self.emit_glyph(out, c);
            self.curscol += 1;
        } else if !self.smir.is_empty() && !self.rmir.is_empty() {
            out.extend(apply_padding(&self.smir, xon, pad));
            self.emit_glyph(out, c);
            self.curscol += 1;
            if !self.ip.is_empty() {
                out.extend(apply_padding(&self.ip, xon, pad));
            }
            out.extend(apply_padding(&self.rmir, xon, pad));
        } else if !self.ich1.is_empty() {
            out.extend(apply_padding(&self.ich1, xon, pad));
            self.emit_glyph(out, c);
            self.curscol += 1;
            if !self.ip.is_empty() {
                out.extend(apply_padding(&self.ip, xon, pad));
            }
        }
    }

    /// `ClrUpdate()` -- clear then redraw every line from scratch.
    fn clr_update(&mut self, out: &mut Vec<u8>, new: &[Cell]) {
        // The background blank is the screen's bkgd cell (derived as ncurses' ClrBlank does, from the
        // lower-right cell) -- a colored bkgd makes the clear emit the bg color first. Only an
        // actually-clearable cell counts (the same `can_clear_with` guard ClrBottom uses): if the
        // corner holds content (e.g. a border's lower-right corner) the clear stays plain.
        let corner = new[self.idx(self.rows - 1, self.cols - 1)];
        let blank = if can_clear_with(corner) {
            corner
        } else {
            BLANK
        };
        self.clear_screen(out, new, blank);
        let nonempty = self.clr_bottom(out, new, self.rows);
        for i in 0..nonempty {
            self.transform_line(out, new, i);
        }
    }

    /// Whether `curscr` row `r` and `new` row `r2` are identical.
    fn row_eq(&self, new: &[Cell], r: i32, r2: i32) -> bool {
        let (a, b) = (self.idx(r, 0), self.idx(r2, 0));
        let w = self.cols as usize;
        self.cur[a..a + w] == new[b..b + w]
    }

    /// Whether `new` row `r` is all blank (default blank cells).
    fn new_row_blank(&self, new: &[Cell], r: i32) -> bool {
        let b = self.idx(r, 0);
        new[b..b + self.cols as usize].iter().all(|c| *c == BLANK)
    }

    /// Whether `curscr` row `r` is all blank.
    fn cur_row_blank(&self, r: i32) -> bool {
        let b = self.idx(r, 0);
        self.cur[b..b + self.cols as usize]
            .iter()
            .all(|c| *c == BLANK)
    }

    /// Detect a vertical-shift band that *starts* at row `top`: returns `(n, bot)` where `n` is `+`
    /// for a scroll up (content moves toward row 0) by `|n|` or `-` for a scroll down, over the
    /// inclusive band `[top, bot]`, or `None` if no shift begins at `top`.
    ///
    /// This is the per-band primitive of ncurses' `_nc_hash_map` + `_nc_scroll_optimize`: the band
    /// grows through matching rows (including blank-to-blank, which is why a shift with only blank
    /// rows below extends to the bottom and becomes a whole-screen / bottom-anchored scroll). A band
    /// is reported only when at least one non-blank line actually moves (content that merely becomes
    /// blank is cleared by TransformLine, not scrolled -- the cost decision). It deliberately does
    /// *not* require rows below `bot` to be unchanged: independent bands and plain edits elsewhere on
    /// the screen are handled by the surrounding scan ([`scroll_optimize`]) and TransformLine.
    fn detect_band_at(&self, new: &[Cell], top: i32) -> Option<(i32, i32)> {
        let maxy = self.rows - 1;
        for n in 1..self.rows {
            // --- Up by n: rows [top, L] shift up (new[i] == cur[i+n]); the n rows [L+1, L+n] are the
            // vacated blanks (bot = L+n). ---
            if top + n <= maxy && self.row_eq(new, top + n, top) {
                let mut l = top;
                while l + 1 + n <= maxy && self.row_eq(new, l + 1 + n, l + 1) {
                    l += 1;
                }
                let bot = l + n;
                if (l + 1..=bot).all(|i| self.new_row_blank(new, i))
                    && (top..=l).any(|i| !self.cur_row_blank(i + n))
                {
                    return Some((n, bot));
                }
            }
            // --- Down by n: the n rows [top, top+n-1] are vacated blanks; rows [top+n, bot] shift
            // down (new[i] == cur[i-n]). ---
            if top + n <= maxy
                && (top..top + n).all(|i| self.new_row_blank(new, i))
                && self.row_eq(new, top, top + n)
            {
                let mut b = top + n;
                while b < maxy && self.row_eq(new, b + 1 - n, b + 1) {
                    b += 1;
                }
                if (top + n..=b).any(|i| !self.cur_row_blank(i - n)) {
                    return Some((-n, b));
                }
            }
        }
        None
    }

    /// Shift `curscr` cells within the inclusive band `[top, bot]` by `n` rows (`+` up, `-` down),
    /// blanking the vacated rows -- the in-model effect of the emitted hardware scroll. Rows outside
    /// the band are untouched.
    fn shift_region(&mut self, n: i32, top: i32, bot: i32) {
        let w = self.cols;
        if n > 0 {
            for r in top..=bot {
                for x in 0..w {
                    let i = self.idx(r, x);
                    self.cur[i] = if r + n <= bot {
                        self.cur[self.idx(r + n, x)]
                    } else {
                        BLANK
                    };
                }
            }
        } else {
            let n = -n;
            for r in (top..=bot).rev() {
                for x in 0..w {
                    let i = self.idx(r, x);
                    self.cur[i] = if r - n >= top {
                        self.cur[self.idx(r - n, x)]
                    } else {
                        BLANK
                    };
                }
            }
        }
    }

    /// Emit `\e[<top+1>;<bot+1>r` (`change_scroll_region`, `csr`).
    fn set_scroll_region(&self, out: &mut Vec<u8>, top: i32, bot: i32) {
        out.extend_from_slice(b"\x1b[");
        out.extend_from_slice((top + 1).to_string().as_bytes());
        out.push(b';');
        out.extend_from_slice((bot + 1).to_string().as_bytes());
        out.push(b'r');
    }

    /// `_nc_scrolln`: emit the hardware scroll for a detected region shift and update `curscr` to
    /// match, so the following TransformLine pass finds no residual. Mirrors ncurses' three cases:
    /// whole-screen (`ind`/`indn`/`ri`/`rin`), bottom-anchored (`dl`/`il` with no `csr`), and a
    /// confined mid/top region (`csr` + scroll + `csr` reset, which invalidates the cursor).
    fn scrolln(&mut self, out: &mut Vec<u8>, new: &[Cell], n: i32, top: i32, bot: i32) {
        let maxy = self.rows - 1;
        let mag = n.abs();
        if n > 0 {
            // Scroll up (forward).
            if bot == maxy && top == 0 {
                self.goto(out, new, bot, 0);
                if mag == 1 {
                    out.push(b'\n'); // scroll_forward (ind)
                } else {
                    csi1(out, mag, b'S'); // parm_index (indn)
                }
            } else if bot == maxy {
                self.goto(out, new, top, 0);
                if mag == 1 {
                    out.extend_from_slice(b"\x1b[M"); // delete_line (dl1)
                } else {
                    csi1(out, mag, b'M'); // parm_delete_line (dl)
                }
            } else {
                self.set_scroll_region(out, top, bot);
                self.cursrow = -1; // csr (DECSTBM) homes the terminal cursor: position now unknown
                self.curscol = -1;
                self.goto(out, new, bot, 0);
                if mag == 1 {
                    out.push(b'\n');
                } else {
                    csi1(out, mag, b'S');
                }
                self.set_scroll_region(out, 0, maxy); // reset csr to the full screen
                self.cursrow = -1; // and again after the reset csr
                self.curscol = -1;
            }
        } else {
            // Scroll down (reverse).
            if bot == maxy && top == 0 {
                self.goto(out, new, 0, 0);
                if mag == 1 {
                    out.extend_from_slice(b"\x1bM"); // scroll_reverse (ri)
                } else {
                    csi1(out, mag, b'T'); // parm_rindex (rin)
                }
            } else if bot == maxy {
                self.goto(out, new, top, 0);
                if mag == 1 {
                    out.extend_from_slice(b"\x1b[L"); // insert_line (il1)
                } else {
                    csi1(out, mag, b'L'); // parm_insert_line (il)
                }
            } else {
                self.set_scroll_region(out, top, bot);
                self.cursrow = -1; // csr (DECSTBM) homes the terminal cursor: position now unknown
                self.curscol = -1;
                self.goto(out, new, top, 0);
                if mag == 1 {
                    out.extend_from_slice(b"\x1bM");
                } else {
                    csi1(out, mag, b'T');
                }
                self.set_scroll_region(out, 0, maxy);
                self.cursrow = -1;
                self.curscol = -1;
            }
        }
        self.shift_region(n, top, bot);
    }

    /// `_nc_scroll_optimize`: scan the screen top-to-bottom and emit a hardware scroll for every
    /// vertical-shift band, applying each to `curscr` so the following TransformLine pass only paints
    /// the genuine edits. Independent bands (each its own `csr` region / `dl` / `il`) and plain edits
    /// interleaved between them are all handled -- the multi-region case ncurses drives from
    /// `_nc_hash_map`. (Mixed bands where the vacated row carries *new* content -- the `idl`
    /// overwrite-residual path -- are deliberately left to TransformLine; see `detect_band_at`.)
    fn scroll_optimize(&mut self, out: &mut Vec<u8>, new: &[Cell]) {
        let mut row = 0;
        while row < self.rows {
            if self.row_eq(new, row, row) {
                row += 1; // unchanged row
            } else if let Some((n, bot)) = self.detect_band_at(new, row) {
                self.scrolln(out, new, n, row, bot);
                row = bot + 1;
            } else {
                row += 1; // a plain edit -- TransformLine paints it
            }
        }
    }

    /// `doupdate` -- transform the physical screen to match `new` (a `rows*cols` desired-cell grid,
    /// row-major) with the cursor parked at `(cury, curx)` (0-based). Returns the exact bytes
    /// ncurses would write. Mutates the stored `curscr` so successive calls produce real diffs.
    pub fn doupdate(&mut self, new: &[Cell], cury: i32, curx: i32) -> Vec<u8> {
        assert_eq!(
            new.len(),
            (self.rows * self.cols) as usize,
            "grid size mismatch"
        );
        let mut out = Vec::new();
        if self.cleared {
            self.clr_update(&mut out, new);
            self.cleared = false;
        } else {
            // Scroll optimization (`_nc_scroll_optimize`): if the desired screen is a uniform
            // vertical shift of the physical screen, emit a hardware scroll instead of redrawing,
            // then let TransformLine clean up any residual.
            self.scroll_optimize(&mut out, new);
            let nonempty = self.clr_bottom(&mut out, new, self.rows);
            for i in 0..nonempty {
                self.transform_line(&mut out, new, i);
            }
        }
        // Final cursor park (skipped under leaveok), then restore normal attributes (the doupdate
        // cleanup `UpdateAttrs(normal)`). xterm has msgr, so the park motion needs no attr reset.
        if !self.leaveok {
            self.goto(&mut out, new, cury, curx);
        }
        self.update_attrs(&mut out, 0);
        out
    }
}

/// The `ovw=TRUE` overwrite hook for `mvcur_ovw`: a desired cell at 0-based `(row, col)` may be
/// overwritten (advancing the cursor by re-emitting it) iff it carries the default attribute and is
/// a directly-printable byte (`Charable`). Returns its byte, else `None` to disable overwrite.
fn want_fn(new: &[Cell], cols: i32, cur_attr: u32) -> impl Fn(i32, i32) -> Option<u8> + '_ {
    move |row, col| {
        let cell = new[(row * cols + col) as usize];
        if cell.attr == cur_attr && ('\x20'..='\x7e').contains(&cell.ch) {
            Some(cell.ch as u8)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(rows: i32, cols: i32) -> Vec<Cell> {
        vec![BLANK; (rows * cols) as usize]
    }

    fn draw(g: &mut [Cell], cols: i32, row: i32, col: i32, s: &str) -> (i32, i32) {
        let mut c = col;
        for &b in s.as_bytes() {
            g[(row * cols + c) as usize] = Cell::plain(b as char, 0);
            c += 1;
        }
        (row, c)
    }

    #[test]
    fn first_paint_two_lines() {
        let (rows, cols) = (24, 80);
        let mut g = grid(rows, cols);
        draw(&mut g, cols, 2, 5, "hello world");
        let (cy, cx) = draw(&mut g, cols, 4, 0, "second line here");
        let mut s = Screen::new(rows, cols);
        let out = s.doupdate(&g, cy, cx);
        assert_eq!(
            out,
            b"\x1b[H\x1b[2J\x1b[3;6Hhello world\r\x1b[5dsecond line here".to_vec()
        );
    }

    #[test]
    fn overwrite_same_length() {
        let (rows, cols) = (24, 80);
        let mut g = grid(rows, cols);
        let (cy, cx) = draw(&mut g, cols, 2, 5, "hello");
        let mut s = Screen::new(rows, cols);
        let _ = s.doupdate(&g, cy, cx); // first paint
        let mut g2 = grid(rows, cols);
        let (cy2, cx2) = draw(&mut g2, cols, 2, 5, "HELLO");
        let out = s.doupdate(&g2, cy2, cx2);
        // GoTo(2,5) from the parked cursor at (2,10) -> hpa wins the tie over 5 backspaces,
        // then overwrite "HELLO"; park back at (2,10) is a no-op.
        assert_eq!(out, b"\x1b[6GHELLO".to_vec());
    }

    #[test]
    fn no_change_is_empty_then_park() {
        let (rows, cols) = (24, 80);
        let mut g = grid(rows, cols);
        let (cy, cx) = draw(&mut g, cols, 1, 1, "abc");
        let mut s = Screen::new(rows, cols);
        let _ = s.doupdate(&g, cy, cx);
        // Same screen, same cursor: no diff, no park motion.
        let out = s.doupdate(&g, cy, cx);
        assert_eq!(out, b"".to_vec());
    }

    #[test]
    fn colored_background_first_paint_sets_color_before_clear() {
        // A colored wbkgd makes ClearScreen set the bg color *before* clear_screen (so the bce clear
        // fills with it); a plain screen clears first. Mirrors NCURSES.DOUPDATE.BG_PAINT.
        let (rows, cols) = (24, 80);
        let mut palette = crate::color::Palette::new();
        palette.init_pair(2, 2, 4); // green on blue
        let bg = Cell::plain(' ', 2 << 8); // a pair-2 colored blank
                                           // Plain screen: output begins with the clear sequence.
        let mut sp = Screen::new(rows, cols);
        let plain = sp.doupdate(&grid(rows, cols), 0, 0);
        assert!(
            plain.starts_with(b"\x1b[H\x1b[2J"),
            "plain should clear first: {plain:?}"
        );
        // Colored bg: the color SGR is emitted before the clear (the clear is not at byte 0).
        let mut sc = Screen::with_palette(rows, cols, palette);
        sc.start_color();
        let g: Vec<Cell> = vec![bg; (rows * cols) as usize];
        let colored = sc.doupdate(&g, 0, 0);
        assert!(
            !colored.starts_with(b"\x1b[H\x1b[2J"),
            "colored should set color first"
        );
        let clr_at = colored
            .windows(7)
            .position(|w| w == b"\x1b[H\x1b[2J")
            .expect("clear present");
        assert!(clr_at > 0, "color SGR precedes the clear");
        // The bytes before the clear are an SGR escape (the bg color).
        assert_eq!(colored[0], 0x1b);
    }

    #[test]
    fn ovw_forward_move_overwrites_instead_of_motion() {
        // Grow "short" -> "short and ...": the GoTo over the matching space uses the ovw=TRUE
        // overwrite (a literal " ") rather than cuf1, then appends the new tail.
        let (rows, cols) = (24, 80);
        let mut g = grid(rows, cols);
        let (cy, cx) = draw(&mut g, cols, 8, 0, "short");
        let mut s = Screen::new(rows, cols);
        let _ = s.doupdate(&g, cy, cx);
        let mut g2 = grid(rows, cols);
        let (cy2, cx2) = draw(&mut g2, cols, 8, 0, "short and more");
        let out = s.doupdate(&g2, cy2, cx2);
        assert_eq!(out, b" and more".to_vec());
    }

    #[test]
    fn widechar_single_width_utf8() {
        // Single-width UTF-8 glyphs are emitted one cell at a time as their UTF-8 bytes; the
        // leading blanks are reached via the mvcur leading-blank path (vpa + overwrite spaces).
        // Mirrors the NCURSES.WIDECHAR court's utf8_mixed scenario.
        let (rows, cols) = (24, 80);
        let mut g = grid(rows, cols);
        // Draw "café αβγ →" at (1,2) by hand (the draw() helper is ASCII-only).
        for (k, ch) in "café αβγ →".chars().enumerate() {
            g[(cols + 2 + k as i32) as usize] = Cell::plain(ch, 0);
        }
        let mut s = Screen::new(rows, cols);
        let out = s.doupdate(&g, 1, 2 + "café αβγ →".chars().count() as i32);
        let expected =
            b"\x1b[H\x1b[2J\x1b[2d  caf\xc3\xa9 \xce\xb1\xce\xb2\xce\xb3 \xe2\x86\x92".to_vec();
        assert_eq!(out, expected);
    }

    #[test]
    fn widechar_no_rep_for_multibyte() {
        // A run of 20 identical non-ASCII glyphs is NOT coalesced with repeat_char (unlike ASCII):
        // each cell emits its UTF-8 bytes. Mirrors the court's utf8_greek_run scenario.
        let (rows, cols) = (24, 80);
        let mut g = grid(rows, cols);
        for k in 0..20 {
            g[(9 * cols + k) as usize] = Cell::plain('ω', 0);
        }
        let mut s = Screen::new(rows, cols);
        let out = s.doupdate(&g, 9, 20);
        let mut expected = b"\x1b[H\x1b[2J\x1b[10d".to_vec();
        for _ in 0..20 {
            expected.extend_from_slice("ω".as_bytes());
        }
        assert_eq!(out, expected);
    }

    #[test]
    fn combining_marks_emit_after_base() {
        // A cell carrying combining marks emits the base glyph then each mark's UTF-8, advancing one
        // column (the marks are zero-width). Mirrors the NCURSES.WIDECHAR combining scenarios.
        let (rows, cols) = (24, 80);
        let mut g = grid(rows, cols);
        // (0,0): base 'e' + U+0301 (combining acute). (0,1): base 'a' + U+0301 + U+0308.
        let mut e = Cell::plain('e', 0);
        e.push_comb('\u{0301}');
        g[0] = e;
        let mut a = Cell::plain('a', 0);
        a.push_comb('\u{0301}');
        a.push_comb('\u{0308}');
        g[1] = a;
        let mut s = Screen::new(rows, cols);
        let out = s.doupdate(&g, 0, 2);
        // first paint: clear, home-row, then e+acute, a+acute+diaeresis.
        let expected = b"\x1b[H\x1b[2Je\xcc\x81a\xcc\x81\xcc\x88".to_vec();
        assert_eq!(out, expected);
    }

    #[test]
    fn widechar_double_width_cjk() {
        // Double-width glyphs emit their UTF-8 once and advance two columns; interleaved ASCII is
        // contiguous. Mirrors the NCURSES.WIDECHAR cjk_basic scenario, line "a世b界c".
        let (rows, cols) = (24, 80);
        let mut g = grid(rows, cols);
        let mut c = 0i32;
        for ch in "a世b界c".chars() {
            g[(12 * cols + c) as usize] = Cell::plain(ch, 0);
            if crate::window::char_width(ch) == 2 {
                g[(12 * cols + c + 1) as usize] = Cell::plain(crate::window::WIDE_PAD, 0);
                c += 2;
            } else {
                c += 1;
            }
        }
        let mut s = Screen::new(rows, cols);
        let out = s.doupdate(&g, 12, c);
        let expected = b"\x1b[H\x1b[2J\x1b[13da\xe4\xb8\x96b\xe7\x95\x8cc".to_vec();
        assert_eq!(out, expected);
    }

    #[test]
    fn bottom_right_corner_disables_auto_margin() {
        // Writing the last cell of the last row would auto-margin-scroll, so it is bracketed by
        // rmam (\e[?7l) / smam (\e[?7h), and the cursor stays on the last column (no advance).
        let (rows, cols) = (24, 80);
        let mut g = grid(rows, cols);
        g[((rows - 1) * cols + (cols - 1)) as usize] = Cell::plain('Z', 0);
        let mut s = Screen::new(rows, cols);
        let out = s.doupdate(&g, rows - 1, cols - 1);
        let txt = String::from_utf8_lossy(&out);
        assert!(txt.contains("\x1b[?7lZ\x1b[?7h"), "got {txt:?}");
    }

    #[test]
    fn rep_coalesces_long_identical_run() {
        // A run of 30 identical non-blank chars on first paint uses repeat_char: one 'a' + REP 29.
        let (rows, cols) = (24, 80);
        let mut g = grid(rows, cols);
        let (cy, cx) = draw(&mut g, cols, 10, 0, &"a".repeat(30));
        let mut s = Screen::new(rows, cols);
        let out = s.doupdate(&g, cy, cx);
        assert_eq!(out, b"\x1b[H\x1b[2J\x1b[11da\x1b[29b".to_vec());
    }

    #[test]
    fn acsc_remaps_altcharset_glyph() {
        // An A_ALTCHARSET cell's glyph is translated through `acsc`: identity by default (xterm emits
        // 'q'), but with a cygwin-style map 'q' -> 0xC4 the emitted byte is remapped. Mirrors the
        // NCURSES.ACS court's non-identity case.
        let (rows, cols) = (24, 80);
        let mut g = grid(rows, cols);
        g[0] = Cell::plain('q', crate::window::attrs::ALTCHARSET);
        // Identity (default): emits the literal 'q' (0x71).
        let mut s = Screen::new(rows, cols);
        let out = s.doupdate(&g, 0, 1);
        assert!(out.contains(&0x71) && !out.contains(&0xc4), "got {out:?}");
        // cygwin acsc: q -> 0xC4.
        let mut s2 = Screen::new(rows, cols);
        s2.set_acsc(b"q\xc4");
        let out2 = s2.doupdate(&g, 0, 1);
        assert!(
            out2.contains(&0xc4) && !out2.contains(&0x71),
            "got {out2:?}"
        );
    }

    // Fill every row with distinct non-blank content ("line NN ...").
    fn full_screen(rows: i32, cols: i32) -> Vec<Cell> {
        let mut g = grid(rows, cols);
        for r in 0..rows {
            draw(&mut g, cols, r, 0, &format!("line {r:02} content"));
        }
        g
    }

    // Shift `g` up by `n` rows (row i <- row i+n); the bottom `n` rows go blank.
    fn shift_up(g: &[Cell], rows: i32, cols: i32, n: i32) -> Vec<Cell> {
        let mut out = grid(rows, cols);
        for r in 0..rows - n {
            let (a, b) = ((r * cols) as usize, ((r + n) * cols) as usize);
            out[a..a + cols as usize].copy_from_slice(&g[b..b + cols as usize]);
        }
        out
    }

    // Shift `g` down by `n` rows (row i <- row i-n); the top `n` rows go blank.
    fn shift_down(g: &[Cell], rows: i32, cols: i32, n: i32) -> Vec<Cell> {
        let mut out = grid(rows, cols);
        for r in n..rows {
            let (a, b) = ((r * cols) as usize, ((r - n) * cols) as usize);
            out[a..a + cols as usize].copy_from_slice(&g[b..b + cols as usize]);
        }
        out
    }

    #[test]
    fn scroll_full_screen_up_by_one() {
        // A whole-screen uniform shift up by 1 emits ind (\r\n), not a redraw, and leaves
        // curscr == new so the next refresh is empty. Mirrors NCURSES.SCROLL.OPTIMIZE full24_up1.
        let (rows, cols) = (24, 80);
        let g = full_screen(rows, cols);
        let mut s = Screen::new(rows, cols);
        let _ = s.doupdate(&g, rows - 1, 0); // first paint, park on the last row
        let g2 = shift_up(&g, rows, cols, 1);
        let out = s.doupdate(&g2, rows - 1, 0);
        // Cursor is already on (rows-1, 0), so no reposition: just ind (\n).
        assert_eq!(out, b"\n".to_vec());
        // curscr now matches new: a repeat refresh emits nothing.
        assert_eq!(s.doupdate(&g2, rows - 1, 0), b"".to_vec());
    }

    #[test]
    fn scroll_full_screen_up_by_three() {
        // Shift up by 3 emits indn (\r\E[3S).
        let (rows, cols) = (24, 80);
        let g = full_screen(rows, cols);
        let mut s = Screen::new(rows, cols);
        let _ = s.doupdate(&g, rows - 1, 0);
        let g2 = shift_up(&g, rows, cols, 3);
        let out = s.doupdate(&g2, rows - 1, 0);
        // Already on the last row at col 0: indn alone (\E[3S).
        assert_eq!(out, b"\x1b[3S".to_vec());
        assert_eq!(s.doupdate(&g2, rows - 1, 0), b"".to_vec());
    }

    #[test]
    fn scroll_full_screen_down_by_one() {
        // A whole-screen uniform shift down by 1 emits ri (\E[H\EM).
        let (rows, cols) = (24, 80);
        let g = full_screen(rows, cols);
        let mut s = Screen::new(rows, cols);
        let _ = s.doupdate(&g, 0, 0);
        let g2 = shift_down(&g, rows, cols, 1);
        let out = s.doupdate(&g2, 0, 0);
        // Already at home: ri alone (\EM).
        assert_eq!(out, b"\x1bM".to_vec());
        assert_eq!(s.doupdate(&g2, 0, 0), b"".to_vec());
    }

    // Shift band [top,bot] of `g` up by n: rows top..=bot-n take row+n, rows bot-n+1..=bot blank,
    // rows outside the band unchanged.
    fn band_up(g: &[Cell], cols: i32, top: i32, bot: i32, n: i32) -> Vec<Cell> {
        let mut out = g.to_vec();
        let w = cols as usize;
        for r in top..=bot {
            let a = (r * cols) as usize;
            if r + n <= bot {
                let b = ((r + n) * cols) as usize;
                let src = g[b..b + w].to_vec();
                out[a..a + w].copy_from_slice(&src);
            } else {
                out[a..a + w].fill(BLANK);
            }
        }
        out
    }

    fn band_down(g: &[Cell], cols: i32, top: i32, bot: i32, n: i32) -> Vec<Cell> {
        let mut out = g.to_vec();
        let w = cols as usize;
        for r in top..=bot {
            let a = (r * cols) as usize;
            if r - n >= top {
                let b = ((r - n) * cols) as usize;
                let src = g[b..b + w].to_vec();
                out[a..a + w].copy_from_slice(&src);
            } else {
                out[a..a + w].fill(BLANK);
            }
        }
        out
    }

    #[test]
    fn scroll_region_mid_up_uses_csr() {
        // A mid-screen band [5,15] shifting up by 1 (static non-blank content below) is confined to
        // a csr region: csr(5,15), GoTo(bot) [absolute, since csr homes the cursor], ind, reset csr.
        // The whole output is cursor-state-independent because csr invalidates the position.
        // Mirrors NCURSES.SCROLL.OPTIMIZE region_mid_up1.
        let (rows, cols) = (24, 80);
        let g = full_screen(rows, cols);
        let mut s = Screen::new(rows, cols);
        let _ = s.doupdate(&g, 0, 0);
        let g2 = band_up(&g, cols, 5, 15, 1);
        let out = s.doupdate(&g2, 15, 0);
        assert_eq!(out, b"\x1b[6;16r\x1b[16;1H\n\x1b[1;24r\x1b[16;1H".to_vec());
        assert_eq!(s.doupdate(&g2, 15, 0), b"".to_vec());
    }

    #[test]
    fn scroll_region_mid_down_uses_csr() {
        // A mid-screen band [5,15] shifting down by 1: csr(5,15), GoTo(top), ri, reset csr.
        let (rows, cols) = (24, 80);
        let g = full_screen(rows, cols);
        let mut s = Screen::new(rows, cols);
        let _ = s.doupdate(&g, 0, 0);
        let g2 = band_down(&g, cols, 5, 15, 1);
        let out = s.doupdate(&g2, 15, 0);
        assert_eq!(
            out,
            b"\x1b[6;16r\x1b[6;1H\x1bM\x1b[1;24r\x1b[16;1H".to_vec()
        );
        assert_eq!(s.doupdate(&g2, 15, 0), b"".to_vec());
    }

    #[test]
    fn scroll_region_bottom_anchored_uses_dl_il() {
        // A band reaching the bottom row uses dl/il (no csr): delete_line at top for up, insert_line
        // at top for down. Mirrors region_bot_up1 / region_bot_down1.
        let (rows, cols) = (24, 80);
        let g = full_screen(rows, cols);

        let mut s = Screen::new(rows, cols);
        let _ = s.doupdate(&g, 13, 0); // park on the band top so GoTo(13,0) is a no-op
        let up = band_up(&g, cols, 13, 23, 1);
        let out = s.doupdate(&up, 23, 0);
        assert!(
            out.starts_with(b"\x1b[M"),
            "expected dl1 first, got {out:?}"
        );
        assert_eq!(s.doupdate(&up, 23, 0), b"".to_vec());

        let mut s2 = Screen::new(rows, cols);
        let _ = s2.doupdate(&g, 13, 0);
        let down = band_down(&g, cols, 13, 23, 1);
        let out2 = s2.doupdate(&down, 23, 0);
        assert!(
            out2.starts_with(b"\x1b[L"),
            "expected il1 first, got {out2:?}"
        );
        assert_eq!(s2.doupdate(&down, 23, 0), b"".to_vec());
    }

    #[test]
    fn scroll_two_independent_bands() {
        // Two separated bands ([2,7] and [14,19]) both scrolling up by 1 are emitted as two
        // sequential csr-region scrolls, top-to-bottom (the multi-region _nc_hash_map case). Both
        // are cursor-state-independent (csr invalidates), so the whole stream is exact. Mirrors
        // NCURSES.SCROLL.OPTIMIZE multi_two_bands_up1.
        let (rows, cols) = (24, 80);
        let g = full_screen(rows, cols);
        let mut s = Screen::new(rows, cols);
        let _ = s.doupdate(&g, 0, 0);
        let g2 = band_up(&band_up(&g, cols, 2, 7, 1), cols, 14, 19, 1);
        let out = s.doupdate(&g2, 23, 0);
        assert_eq!(
            out,
            b"\x1b[3;8r\x1b[8;1H\n\x1b[1;24r\x1b[15;20r\x1b[20;1H\n\x1b[1;24r\x1b[24;1H".to_vec()
        );
        assert_eq!(s.doupdate(&g2, 23, 0), b"".to_vec());
    }

    #[test]
    fn scroll_band_plus_unrelated_edit() {
        // A band [14,19] scrolling up while an unrelated row (2) is edited: the scroll is emitted
        // first (csr region), then TransformLine paints the edit. Mirrors multi_scroll_plus_edit.
        let (rows, cols) = (24, 80);
        let g = full_screen(rows, cols);
        let mut s = Screen::new(rows, cols);
        let _ = s.doupdate(&g, 0, 0);
        let mut g2 = band_up(&g, cols, 14, 19, 1);
        draw(&mut g2, cols, 2, 0, "EDITED ROW TWO");
        for c in g2[(2 * cols + 14) as usize..(2 * cols + 80) as usize].iter_mut() {
            *c = BLANK; // clear the tail of row 2 past the new text
        }
        let out = s.doupdate(&g2, 23, 0);
        // Scroll first, then the row-2 edit appears after it.
        assert!(
            out.starts_with(b"\x1b[15;20r\x1b[20;1H\n\x1b[1;24r"),
            "got {out:?}"
        );
        let scroll_end = b"\x1b[15;20r\x1b[20;1H\n\x1b[1;24r".len();
        assert!(
            out[scroll_end..]
                .windows(14)
                .any(|w| w == b"EDITED ROW TWO"),
            "edit should follow the scroll, got {out:?}"
        );
        assert_eq!(s.doupdate(&g2, 23, 0), b"".to_vec());
    }
}
