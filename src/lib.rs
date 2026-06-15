//! # ncurses-native
//!
//! A clean-room, dependency-free Rust reproduction of the **observable terminal byte output** of
//! ncurses 6.6 on one terminal: `TERM=xterm`.
//!
//! This crate is **not** ncurses and is **not** a port of any ncurses C source. It reproduces the
//! *bytes ncurses writes to the terminal* -- the escape-sequence stream captured from behavior under
//! a pty on the admitted host -- reverse-engineered from facts, not copied from a source tree. There
//! is zero ncurses C source here.
//!
//! It is `#![forbid(unsafe_code)]`, std-only, has no dependencies, and is ASCII-only in source.
//!
//! ## What is reproduced (for `TERM=xterm`, ncurses 6.6)
//!
//! * **The cursor-movement cost optimizer** -- [`cursor::mvcur`], ncurses's `relative_move`
//!   strategy enumeration (VPA/cuu1, spaces/HPA/backspaces, CR-to-col1, home, CUP; shortest-wins
//!   with the local/CR/home-before-CUP tie-break).
//! * **SGR attributes** -- [`attr`]: bold/dim/underline/blink/reverse on/off.
//! * **ANSI color + pairs** -- [`color`]: foreground/background SGR for the 8 ANSI colors; the free
//!   default pair (white-on-black, pair 0).
//! * **Screen framing** -- [`term`]: the `smcup` init prologue and `rmcup` teardown epilogue.
//! * **Erase operations** -- [`screen`]: `clear`, `clr_eos`, `clr_eol`, `clr_bol`.
//! * **Single-field repaint** -- [`screen::single_field_repaint`]: the `wclear` + top-down
//!   `TransformLine` paint of one static field on a blank screen.
//!
//! ## What is NOT yet reproduced (non-claims / parity roadmap)
//!
//! * **terminfo database parsing** -- exactly one terminal is hardcoded (`xterm`, ncurses 6.6); any
//!   other `TERM` or ncurses build emits different bytes. This is the largest TODO.
//! * **The full `doupdate` / `TransformLine` line-diff** for arbitrary screen deltas -- only the
//!   single-static-field shape is reproduced.
//! * **Input handling** -- `getch`, `keypad`, mouse decoding, timeouts.
//! * **`WINDOW` / `SCREEN` / pad structures** -- there is no window model.
//! * **panels, menus, forms**.
//! * **Terminals other than xterm**, and **ncurses builds other than 6.6**.
#![forbid(unsafe_code)]

pub mod attr;
pub mod color;
pub mod cursor;
pub mod screen;
pub mod term;

pub use attr::{sgr_off, sgr_on, Attr};
pub use color::{sgr_bg, sgr_fg, DEFAULT_PAIR};
pub use cursor::mvcur;
pub use screen::{clear, clr_bol, clr_eol, clr_eos, single_field_repaint, RESET_DEFAULTS};
pub use term::{
    Xterm, ADMITTED_NCURSES, ADMITTED_TERM, INIT_PROLOGUE, SCREEN_ROWS, TEARDOWN_EPILOGUE,
};

/// An honest one-line statement of this crate's coverage and claim boundary.
pub const COVERAGE_NOTE: &str = "ncurses-native reproduces roughly 3-5% of ncurses by surface: the \
output bytes of ncurses 6.6 for one admitted terminal (TERM=xterm) -- cursor-movement optimizer, \
SGR attributes, ANSI color, smcup/rmcup framing, erase ops, and a single-field repaint. It is an \
output-byte reproduction reverse-engineered from behavior, not a port of ncurses source, and a seed \
toward parity, not a replacement.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn re_exports_are_wired() {
        assert_eq!(ADMITTED_TERM, "xterm");
        assert_eq!(DEFAULT_PAIR, (7, 0));
        assert_eq!(Attr::Bold.sgr_code(), 1);
        assert_eq!(mvcur((1, 1), (1, 1)), b"");
        assert_eq!(clear(), b"\x1b[H\x1b[2J");
        assert!(INIT_PROLOGUE.starts_with(b"\x1b[?1049h"));
        assert_eq!(Xterm::ROWS, SCREEN_ROWS);
    }

    #[test]
    fn coverage_note_is_honest() {
        assert!(COVERAGE_NOTE.contains("3-5%"));
        assert!(COVERAGE_NOTE.contains("TERM=xterm"));
        assert!(COVERAGE_NOTE.contains("not a port"));
        assert!(COVERAGE_NOTE.contains("seed"));
    }
}
