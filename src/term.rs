//! The admitted terminal's capability set.
//!
//! Everything in `ncurses-native` is admitted against **one** terminal: `TERM=xterm` under
//! **ncurses 6.4** (`libncursesw.so.6`, terminfo `xterm`) -- the build every oracle court runs
//! against. The init/teardown framing (`smcup` ... `rmcup`) and the cursor-movement optimization
//! reproduced elsewhere in this crate are ncurses/terminfo artifacts; a *different* terminal or a
//! *different* ncurses build emits *different* bytes. That single-terminal/single-build dependence
//! is the explicit non-claim of the whole crate -- see the crate-level docs and
//! [`crate::COVERAGE_NOTE`].
//!
//! What is reproduced for the admitted terminal is byte-identical: the exact escape-sequence
//! stream ncurses emits, captured deterministically from behavior under a pty -- not copied from
//! any ncurses source.

/// The admitted terminal type the reproduced bytes are valid for. Anything else is the non-claim.
pub const ADMITTED_TERM: &str = "xterm";

/// The admitted ncurses build whose terminfo-optimized output is reproduced -- the build the oracle
/// courts run against (`infocmp -V` => `ncurses 6.4.20240113`).
pub const ADMITTED_NCURSES: &str = "ncurses 6.4";

/// The terminal height the scroll region implies (`\e[1;24r`): rows `1..=24`. Used by callers
/// to place content and the final-line clear in the teardown.
pub const SCREEN_ROWS: i32 = 24;

/// The screen **init prologue** -- the exact bytes ncurses emits via `initscr` + `start_color` +
/// `keypad` + `curs_set(1)` + `mousemask` on first screen I/O, on the admitted xterm/ncurses 6.4
/// terminal (captured live under a pty). Decomposed:
///
/// * `\e[?1049h` -- smcup, switch to the alternate screen buffer,
/// * `\e[22;0;0t` -- push the window title onto the title stack,
/// * `\e[1;24r` -- DECSTBM, set the scroll region to rows 1..=24,
/// * `\e(B` -- designate G0 = ASCII,
/// * `\e[m` -- SGR reset,
/// * `\e[4l` -- insert/replace mode off (replace mode),
/// * `\e[?7h` -- autowrap on,
/// * `\e[39;49m` -- default foreground/background,
/// * `\e[?1h\e=` -- application cursor keys + application keypad,
/// * `\e[?12l` -- cursor blink off,
/// * `\e[?25h` -- cursor visible,
/// * `\e[?1006;1000h` -- SGR-pixel mouse encoding + normal mouse tracking,
/// * `\e[39;49m` -- default foreground/background (again),
/// * `\e[37m` -- foreground white,
/// * `\e[40m` -- background black (the default color pair),
/// * `\e[H` -- cursor home,
/// * `\e[2J` -- erase the whole screen.
pub const INIT_PROLOGUE: &[u8] = b"\x1b[?1049h\x1b[22;0;0t\x1b[1;24r\x1b(B\x1b[m\x1b[4l\x1b[?7h\x1b[39;49m\x1b[?1h\x1b=\x1b[?12l\x1b[?25h\x1b[?1006;1000h\x1b[39;49m\x1b[37m\x1b[40m\x1b[H\x1b[2J";

/// The screen **teardown epilogue** -- the exact bytes ncurses emits via `endwin` on the
/// admitted terminal (captured live under a pty). Decomposed:
///
/// * `\e[?1006;1000l` -- mouse tracking off,
/// * `\e[39;49m` -- default fg/bg,
/// * `\e[24d` -- VPA to the last line (row 24),
/// * `\e[K` -- clear to end of line,
/// * `\e[24;1H` -- CUP to row 24, column 1,
/// * `\e[?1049l` -- rmcup, leave the alternate screen buffer,
/// * `\e[23;0;0t` -- pop the window title,
/// * `\r` -- carriage return,
/// * `\e[?1l` -- normal (non-application) cursor keys,
/// * `\e>` -- numeric keypad mode.
pub const TEARDOWN_EPILOGUE: &[u8] = b"\x1b[?1006;1000l\x1b[39;49m\x1b[24d\x1b[K\x1b[24;1H\x1b[?1049l\x1b[23;0;0t\r\x1b[?1l\x1b>";

/// The admitted-terminal capability set. A zero-sized marker for the one terminal this crate
/// reproduces; its associated constants are mirrors of the module constants for callers who
/// prefer to reach the capabilities through a type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Xterm;

impl Xterm {
    /// The admitted terminal type ([`ADMITTED_TERM`]).
    pub const TERM: &'static str = ADMITTED_TERM;
    /// The admitted ncurses build ([`ADMITTED_NCURSES`]).
    pub const NCURSES: &'static str = ADMITTED_NCURSES;
    /// The terminal height ([`SCREEN_ROWS`]).
    pub const ROWS: i32 = SCREEN_ROWS;
    /// The init prologue bytes ([`INIT_PROLOGUE`]).
    pub const INIT_PROLOGUE: &'static [u8] = INIT_PROLOGUE;
    /// The teardown epilogue bytes ([`TEARDOWN_EPILOGUE`]).
    pub const TEARDOWN_EPILOGUE: &'static [u8] = TEARDOWN_EPILOGUE;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admitted_identity_is_single_terminal() {
        assert_eq!(ADMITTED_TERM, "xterm");
        assert_eq!(ADMITTED_NCURSES, "ncurses 6.4");
        assert_eq!(SCREEN_ROWS, 24);
        assert_eq!(Xterm::TERM, ADMITTED_TERM);
        assert_eq!(Xterm::NCURSES, ADMITTED_NCURSES);
        assert_eq!(Xterm::ROWS, SCREEN_ROWS);
    }

    #[test]
    fn prologue_frames_smcup_and_home_clear() {
        // smcup leads; the prologue ends with home + clear-screen.
        assert!(INIT_PROLOGUE.starts_with(b"\x1b[?1049h"));
        assert!(INIT_PROLOGUE.ends_with(b"\x1b[H\x1b[2J"));
        assert_eq!(Xterm::INIT_PROLOGUE, INIT_PROLOGUE);
    }

    #[test]
    fn epilogue_frames_rmcup() {
        // rmcup appears, and the epilogue ends with normal cursor keys + numeric keypad.
        assert!(TEARDOWN_EPILOGUE.windows(8).any(|w| w == b"\x1b[?1049l"));
        assert!(TEARDOWN_EPILOGUE.ends_with(b"\x1b[?1l\x1b>"));
        assert_eq!(Xterm::TEARDOWN_EPILOGUE, TEARDOWN_EPILOGUE);
    }
}
