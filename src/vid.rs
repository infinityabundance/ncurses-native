//! The attribute / color SGR transition engine -- `vidputs` / `_nc_do_color`.
//!
//! Clean-room reproduction of ncurses's `lib_vidattr.c` (`vidputs`) and `lib_color.c`
//! (`_nc_do_color`) for the admitted xterm / ncurses 6.4 terminal with `start_color()` (and
//! *without* `use_default_colors`, so `fix_pair0` is in effect and pair 0 is white-on-black).
//!
//! Given the previous on-screen attribute+color state and a desired one, [`Vid::set`] emits the
//! exact bytes ncurses would emit to transition between them. xterm has `set_attributes` (`sgr`),
//! so an attribute change emits `tparm(sgr, ...)` (which resets color), after which the color pair
//! is re-applied via `_nc_do_color`; a pure color change emits only the color transition. This is
//! the `UpdateAttrs` hook the screen-update layer ([`crate::update::Screen`]) calls before painting
//! each cell.
//!
//! Attribute bits are the crate's [`crate::window::attrs`] layout; the color pair is carried in the
//! `A_COLOR` field (`0xff00`, i.e. `pair = (attr & 0xff00) >> 8`).

use crate::color::{toggled_colors, Palette};
use crate::cursor::apply_padding;
use crate::terminfo::tparm_n;
use crate::window::attrs;

/// `set_attributes` (sgr) for the admitted xterm, raw terminfo string (escapes decoded). Driven by
/// the byte-exact [`tparm_n`]; params are p1..p9 = standout, underline, reverse, blink, dim, bold,
/// invis, protect, altcharset.
const SGR: &[u8] =
    b"%?%p9%t\x1b(0%e\x1b(B%;\x1b[0%?%p6%t;1%;%?%p5%t;2%;%?%p2%t;4%;%?%p1%p3%|%t;7%;%?%p4%t;5%;%?%p7%t;8%;m";
/// `exit_attribute_mode` (sgr0).
const SGR0: &[u8] = b"\x1b(B\x1b[m";
/// `orig_pair` (op) -- reset foreground/background to the terminal default.
const OP: &[u8] = b"\x1b[39;49m";
/// `exit_alt_charset_mode` (rmacs) -- leave line-drawing (G0 = ASCII).
const RMACS: &[u8] = b"\x1b(B";

/// All non-color attribute bits (`ALL_BUT_COLOR`), including the alternate-charset bit.
const ALL_BUT_COLOR: u32 = attrs::STANDOUT
    | attrs::UNDERLINE
    | attrs::REVERSE
    | attrs::BLINK
    | attrs::DIM
    | attrs::BOLD
    | attrs::INVIS
    | attrs::PROTECT
    | attrs::ALTCHARSET;

/// Pair 0's colors with `fix_pair0` (no `use_default_colors`): white on black.
const DEFAULT_FG: i16 = 7;
const DEFAULT_BG: i16 = 0;

/// The SGR transition engine: holds the current on-screen attribute+color state (`SCREEN_ATTRS` /
/// `PreviousAttr`) and the color palette, and emits transition bytes.
#[derive(Debug, Clone)]
pub struct Vid {
    prev: u32,
    palette: Palette,
    coloron: bool,
    /// `set_attributes` (sgr), `exit_attribute_mode` (sgr0), `exit_alt_charset_mode` (rmacs).
    /// Defaults are the xterm caps; [`Vid::set_attr_caps`] overrides them for another terminal.
    sgr: Vec<u8>,
    sgr0: Vec<u8>,
    rmacs: Vec<u8>,
    /// Whether `set_attributes` (sgr) exists. When false, `vidputs` drives the terminal with the
    /// individual mode caps below (ncurses' no-`sgr` branch).
    has_sgr: bool,
    /// Individual enter-mode caps (empty when absent): `smacs` (enter_alt_charset), `blink`, `bold`,
    /// `dim`, `rev`, `smso`, `prot` (enter_protected), `invis` (enter_secure), `smul`.
    smacs: Vec<u8>,
    e_blink: Vec<u8>,
    e_bold: Vec<u8>,
    e_dim: Vec<u8>,
    e_rev: Vec<u8>,
    smso: Vec<u8>,
    e_prot: Vec<u8>,
    e_invis: Vec<u8>,
    smul: Vec<u8>,
    /// Individual exit caps for underline/standout (`rmul`/`rmso`); `rmacs` exits altcharset above.
    rmul: Vec<u8>,
    rmso: Vec<u8>,
    /// `_use_rmul`/`_use_rmso`: emit the individual exit cap only when it exists and differs from
    /// sgr0 (ncurses' `SGR0_TEST`); otherwise sgr0 covers the reset.
    use_rmul: bool,
    use_rmso: bool,
    /// `xon`/`pad` for tputs-style padding of the emitted attribute caps.
    xon: bool,
    pad_char: u8,
    /// Color caps (`_nc_do_color`): the foreground/background templates (`setaf`/`setf` and
    /// `setab`/`setb`), `set_color_pair` (scp), and `orig_pair` (op). All are terminfo templates
    /// emitted via `tparm`; `None` means the cap is absent (emit nothing). `color_toggled` applies
    /// `toggled_colors` for the non-ANSI `setf`/`setb`. `has_color` gates emission entirely (a
    /// terminal with `colors#0`/no cap emits no color).
    setaf: Option<Vec<u8>>,
    setab: Option<Vec<u8>>,
    scp: Option<Vec<u8>>,
    op: Vec<u8>,
    color_toggled: bool,
    has_color: bool,
    /// `no_color_video` (ncv) -- attribute bits (in [`crate::window::attrs`] layout) that the
    /// terminal cannot combine with color; suppressed while color is on. xterm: none (0). Linux
    /// console: underline + dim.
    ncv: u32,
}

impl Vid {
    /// A fresh engine in the `A_NORMAL` state with the given palette (pair -> fg/bg). Color is off
    /// until [`Vid::start_color`] is called (mirrors ncurses' `SP->_coloron`).
    pub fn new(palette: Palette) -> Vid {
        Vid {
            prev: 0,
            palette,
            coloron: false,
            sgr: SGR.to_vec(),
            sgr0: SGR0.to_vec(),
            rmacs: RMACS.to_vec(),
            has_sgr: true,
            smacs: Vec::new(),
            e_blink: Vec::new(),
            e_bold: Vec::new(),
            e_dim: Vec::new(),
            e_rev: Vec::new(),
            smso: Vec::new(),
            e_prot: Vec::new(),
            e_invis: Vec::new(),
            smul: Vec::new(),
            rmul: Vec::new(),
            rmso: Vec::new(),
            use_rmul: false,
            use_rmso: false,
            xon: false,
            pad_char: 0,
            // xterm setaf/setab templates (tparm(setaf, c) == \e[3Xm for c<8, byte-identical to the
            // old hardcoded path); from_terminfo overrides, and None means the cap is absent.
            setaf: Some(b"\x1b[3%p1%dm".to_vec()),
            setab: Some(b"\x1b[4%p1%dm".to_vec()),
            scp: None,
            op: OP.to_vec(),
            color_toggled: false,
            has_color: true, // xterm has colors#8
            ncv: 0,
        }
    }

    /// Populate the color caps (`_nc_do_color`) for a terminal: `setaf`/`setab` templates (or
    /// `setf`/`setb` with `toggled`), `orig_pair`, and whether the terminal supports color at all.
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
        self.setaf = setaf.map(|s| s.to_vec());
        self.setab = setab.map(|s| s.to_vec());
        self.scp = scp.map(|s| s.to_vec());
        self.op = op.map(|s| s.to_vec()).unwrap_or_default();
        self.color_toggled = toggled;
        self.has_color = has_color;
    }

    /// The ClearScreen color-init for a *default* background (`_nc_do_color(0, 0)`): emitted before
    /// the clear when color is on (lib's ClearScreen), so the screen is initialised to the default
    /// pair. Empty when color is off/unsupported. Scoped to the default-bkgd case by the caller.
    pub fn color_init_default(&self) -> Vec<u8> {
        if self.coloron && self.has_color {
            self.do_color(0, 0)
        } else {
            Vec::new()
        }
    }

    /// Populate the individual mode caps + flags for a terminal without `set_attributes` (sgr), and
    /// the `xon`/`pad` used to pad all attribute caps. `has_sgr` selects the sgr vs individual path.
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
        self.has_sgr = has_sgr;
        let g = |o: Option<&[u8]>| o.map(|s| s.to_vec()).unwrap_or_default();
        // enter = [smacs, blink, bold, dim, rev, smso, prot, invis, smul]
        self.smacs = g(enter[0]);
        self.e_blink = g(enter[1]);
        self.e_bold = g(enter[2]);
        self.e_dim = g(enter[3]);
        self.e_rev = g(enter[4]);
        self.smso = g(enter[5]);
        self.e_prot = g(enter[6]);
        self.e_invis = g(enter[7]);
        self.smul = g(enter[8]);
        self.rmul = g(rmul);
        self.rmso = g(rmso);
        self.xon = xon;
        self.pad_char = pad_char;
        // SGR0_TEST: use the individual exit only when it exists and differs from sgr0.
        let differs = |c: &[u8]| !c.is_empty() && (self.sgr0.is_empty() || c != self.sgr0);
        self.use_rmul = differs(&self.rmul);
        self.use_rmso = differs(&self.rmso);
    }

    /// Set the `no_color_video` mask (attribute bits suppressed while color is on).
    pub fn set_ncv(&mut self, ncv: u32) {
        self.ncv = ncv;
    }

    /// Override the attribute caps (`sgr`/`sgr0`/`rmacs`) for a terminal other than xterm (e.g. the
    /// Linux console, whose `sgr` uses `^N`/`^O` for altcharset and whose `sgr0` is `\e[m\017`).
    pub fn set_attr_caps(&mut self, sgr: &[u8], sgr0: &[u8], rmacs: &[u8]) {
        self.sgr = sgr.to_vec();
        self.sgr0 = sgr0.to_vec();
        self.rmacs = rmacs.to_vec();
    }

    /// `start_color()` -- enable color processing (so transitions re-apply the pair via `_nc_do_color`).
    pub fn start_color(&mut self) {
        self.coloron = true;
    }

    /// The current attribute+color state.
    pub fn current(&self) -> u32 {
        self.prev
    }

    /// Whether color is active (started and supported) -- gates the ClearScreen color-init and the
    /// `bce` fast-clear suppression.
    pub fn color_active(&self) -> bool {
        self.coloron && self.has_color
    }

    /// Define color pair `pair` as `(fg, bg)` (delegates to the palette), so subsequent transitions
    /// to that pair emit the right `setaf`/`setab`.
    pub fn init_pair(&mut self, pair: i16, fg: i16, bg: i16) -> bool {
        self.palette.init_pair(pair, fg, bg)
    }

    /// `vidputs(newmode)` -- emit the bytes to move from the current state to `newmode`
    /// (attribute bits in [`crate::window::attrs`] layout, pair in `0xff00`). Returns empty when
    /// the state is unchanged.
    pub fn set(&mut self, newmode: u32) -> Vec<u8> {
        let newmode = newmode & (ALL_BUT_COLOR | attrs::COLOR);
        // no_color_video: while color is active the terminal cannot render these attributes, so
        // ncurses strips them before emitting (e.g. the Linux console's underline/dim, ncv=18).
        let newmode = if self.coloron {
            newmode & !self.ncv
        } else {
            newmode
        };
        if newmode == self.prev {
            return Vec::new();
        }
        let pair = ((newmode & attrs::COLOR) >> 8) as i32;
        let mut turn_off = (!newmode & self.prev) & ALL_BUT_COLOR;
        let mut turn_on = (newmode & !self.prev) & ALL_BUT_COLOR;
        let (xon, pad) = (self.xon, self.pad_char);
        let mut out = Vec::new();
        // old_pair seen by _nc_do_color; reset to 0 once sgr/sgr0 strips the color from PreviousAttr.
        let mut old_pair = ((self.prev & attrs::COLOR) >> 8) as i32;

        if newmode == 0 {
            // A_NORMAL. Alternate-charset is turned off first via rmacs (exit_alt_charset_mode),
            // before exit_attribute_mode -- exactly as ncurses' vidputs does.
            if self.prev & attrs::ALTCHARSET != 0 && !self.rmacs.is_empty() {
                out.extend(apply_padding(&self.rmacs, xon, pad));
                self.prev &= !attrs::ALTCHARSET;
            }
            if self.prev != 0 {
                if !self.sgr0.is_empty() {
                    out.extend(apply_padding(&self.sgr0, xon, pad));
                } else {
                    // No sgr0: turn off underline/standout individually (ncurses' fallback).
                    if self.use_rmul && self.prev & attrs::UNDERLINE != 0 {
                        out.extend(apply_padding(&self.rmul, xon, pad));
                    }
                    if self.use_rmso && self.prev & attrs::STANDOUT != 0 {
                        out.extend(apply_padding(&self.rmso, xon, pad));
                    }
                }
                old_pair = 0;
            }
            if self.coloron {
                out.extend_from_slice(&self.do_color(old_pair, pair));
            }
        } else if self.has_sgr {
            // set_attributes path: sgr resets color, so re-apply the pair afterwards. The sgr cap's
            // p9 (altcharset) emits the `\e(0` / `\e(B` charset switch.
            if turn_on != 0 || turn_off != 0 {
                out.extend(apply_padding(
                    &tparm_n(&self.sgr, &sgr_params(newmode)),
                    xon,
                    pad,
                ));
                old_pair = 0;
            }
            if self.coloron && (pair != old_pair || pair == 0) {
                out.extend_from_slice(&self.do_color(old_pair, pair));
            }
        } else {
            // No set_attributes: drive the terminal with individual mode caps (ncurses' no-sgr
            // branch). Turn off altcharset / underline / standout first; remaining turn-offs (bold,
            // dim, blink, reverse, protect, invis -- which have no individual exit) force a full sgr0
            // reset, after which everything in newmode is re-turned-on.
            let mut turn_off_indiv = |out: &mut Vec<u8>, mask: u32, cap: &[u8]| {
                if turn_off & mask != 0 && !cap.is_empty() {
                    out.extend(apply_padding(cap, xon, pad));
                    turn_off &= !mask;
                }
            };
            turn_off_indiv(&mut out, attrs::ALTCHARSET, &self.rmacs);
            if self.use_rmul {
                turn_off_indiv(&mut out, attrs::UNDERLINE, &self.rmul);
            }
            if self.use_rmso {
                turn_off_indiv(&mut out, attrs::STANDOUT, &self.rmso);
            }
            if turn_off != 0 && !self.sgr0.is_empty() {
                out.extend(apply_padding(&self.sgr0, xon, pad));
                turn_on |= newmode & ALL_BUT_COLOR;
                old_pair = 0;
            }
            if self.coloron && (pair != old_pair || pair == 0) {
                out.extend_from_slice(&self.do_color(old_pair, pair));
            }
            // TurnOn order matches ncurses vidputs exactly.
            for (mask, cap) in [
                (attrs::ALTCHARSET, &self.smacs),
                (attrs::BLINK, &self.e_blink),
                (attrs::BOLD, &self.e_bold),
                (attrs::DIM, &self.e_dim),
                (attrs::REVERSE, &self.e_rev),
                (attrs::STANDOUT, &self.smso),
                (attrs::PROTECT, &self.e_prot),
                (attrs::INVIS, &self.e_invis),
                (attrs::UNDERLINE, &self.smul),
            ] {
                if turn_on & mask != 0 && !cap.is_empty() {
                    out.extend(apply_padding(cap, xon, pad));
                }
            }
        }

        self.prev = newmode;
        out
    }

    /// `_nc_do_color(old_pair, pair)` for the admitted xterm with `fix_pair0` (reverse never set
    /// here because xterm has no `no_color_video`). Emits `orig_pair` when moving to the default
    /// pair (0), then `set_foreground`/`set_background` for the resolved colors.
    fn do_color(&self, _old_pair: i32, pair: i32) -> Vec<u8> {
        if !self.has_color {
            return Vec::new();
        }
        let (xon, pad) = (self.xon, self.pad_char);
        // set_color_pair: a terminal with `scp` selects a pair directly (ncurses returns right after,
        // emitting no fg/bg). Only for a non-default pair.
        if pair != 0 {
            if let Some(scp) = &self.scp {
                return apply_padding(&tparm_n(scp, &[pair]), xon, pad);
            }
        }
        let (fg, bg) = if pair == 0 {
            (DEFAULT_FG, DEFAULT_BG)
        } else {
            self.palette.pair_content(pair as i16)
        };
        let mut out = Vec::new();
        // Moving to the default pair resets to the terminal default first (orig_pair); from there the
        // resolved fg/bg are set explicitly. For a non-default pair no reset is needed.
        if pair == 0 && !self.op.is_empty() {
            out.extend(apply_padding(&self.op, xon, pad));
        }
        // setf/setb (color_toggled) interchange colors 1<->4, 3<->6; setaf/setab do not.
        let cf = |c: i16| {
            if self.color_toggled {
                toggled_colors(c as i32)
            } else {
                c as i32
            }
        };
        if let Some(t) = &self.setaf {
            out.extend(apply_padding(&tparm_n(t, &[cf(fg)]), xon, pad));
        }
        if let Some(t) = &self.setab {
            out.extend(apply_padding(&tparm_n(t, &[cf(bg)]), xon, pad));
        }
        out
    }
}

/// Map a crate attribute mode to the nine `sgr` parameters.
fn sgr_params(mode: u32) -> [i32; 9] {
    let b = |m: u32| (mode & m != 0) as i32;
    [
        b(attrs::STANDOUT),   // p1
        b(attrs::UNDERLINE),  // p2
        b(attrs::REVERSE),    // p3
        b(attrs::BLINK),      // p4
        b(attrs::DIM),        // p5
        b(attrs::BOLD),       // p6
        b(attrs::INVIS),      // p7
        b(attrs::PROTECT),    // p8
        b(attrs::ALTCHARSET), // p9 (alternate character set; emits \e(0 / \e(B)
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vid() -> Vid {
        let mut p = Palette::new();
        p.init_pair(1, 1, 0); // red on black
        p.init_pair(2, 2, 4); // green on blue
        let mut v = Vid::new(p);
        v.start_color();
        v
    }

    #[test]
    fn altcharset_uses_smacs_rmacs() {
        // Without start_color (coloron off): line-drawing on/off via sgr p9 (\e(0) and rmacs (\e(B).
        let mut v = Vid::new(Palette::new());
        assert_eq!(v.set(attrs::ALTCHARSET), b"\x1b(0\x1b[0m".to_vec());
        assert_eq!(v.set(0), b"\x1b(B".to_vec());
    }

    #[test]
    fn normal_to_bold_emits_sgr_plus_default_color() {
        let mut v = vid();
        assert_eq!(
            v.set(attrs::BOLD),
            b"\x1b(B\x1b[0;1m\x1b[39;49m\x1b[37m\x1b[40m".to_vec()
        );
    }

    #[test]
    fn bold_to_red_resets_attrs_then_sets_color() {
        let mut v = vid();
        let _ = v.set(attrs::BOLD);
        // pair 1 in the color field:
        assert_eq!(v.set(1 << 8), b"\x1b(B\x1b[0m\x1b[31m\x1b[40m".to_vec());
    }

    #[test]
    fn pure_color_change_emits_only_color() {
        let mut v = vid();
        let _ = v.set(1 << 8); // red
        assert_eq!(v.set(2 << 8), b"\x1b[32m\x1b[44m".to_vec());
    }

    #[test]
    fn back_to_normal_emits_sgr0_plus_default_color() {
        let mut v = vid();
        let _ = v.set(1 << 8); // red
        assert_eq!(
            v.set(0),
            b"\x1b(B\x1b[m\x1b[39;49m\x1b[37m\x1b[40m".to_vec()
        );
    }

    #[test]
    fn unchanged_is_empty() {
        let mut v = vid();
        let _ = v.set(attrs::BOLD);
        assert_eq!(v.set(attrs::BOLD), b"".to_vec());
    }
}
