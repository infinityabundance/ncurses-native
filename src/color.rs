//! ANSI / curses color -- the foreground and background SGR sequences for the 8 ANSI colors, on the
//! admitted xterm/ncurses 6.6 terminal.
//!
//! Colors here are **ANSI/curses color numbers** `0..=7` directly: `0` black, `1` red, `2` green,
//! `3` yellow, `4` blue, `5` magenta, `6` cyan, `7` white. There is **no COBOL color mapping** in
//! this crate -- callers that need a different numbering convention apply it before calling here.
//!
//! The default color pair (white-on-black, [`DEFAULT_PAIR`]) is curses pair 0, which is always
//! allocated and needs no SGR.

/// The default color pair `(foreground, background) = (7, 0)` -- white on black, curses pair 0.
/// A field in the default pair needs no SGR (the terminal is already in it after init).
pub const DEFAULT_PAIR: (u8, u8) = (7, 0);

/// The foreground SGR for an ANSI color `c` in `0..=7`: `\e[<30+c>m`. Inputs above 7 are masked to
/// three bits (defensive; valid ANSI colors are 0..=7).
pub fn sgr_fg(c: u8) -> Vec<u8> {
    sgr(30 + (c & 0b111) as i32)
}

/// The background SGR for an ANSI color `c` in `0..=7`: `\e[<40+c>m`. Inputs above 7 are masked to
/// three bits.
pub fn sgr_bg(c: u8) -> Vec<u8> {
    sgr(40 + (c & 0b111) as i32)
}

/// Append a single-parameter SGR `\e[<n>m`.
fn sgr(n: i32) -> Vec<u8> {
    let mut v = b"\x1b[".to_vec();
    v.extend_from_slice(n.to_string().as_bytes());
    v.push(0x6d); // 'm'
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_pair_is_white_on_black() {
        assert_eq!(DEFAULT_PAIR, (7, 0));
    }

    #[test]
    fn foreground_sgr() {
        assert_eq!(sgr_fg(0), b"\x1b[30m"); // black
        assert_eq!(sgr_fg(1), b"\x1b[31m"); // red
        assert_eq!(sgr_fg(2), b"\x1b[32m"); // green
        assert_eq!(sgr_fg(4), b"\x1b[34m"); // blue
        assert_eq!(sgr_fg(7), b"\x1b[37m"); // white
    }

    #[test]
    fn background_sgr() {
        assert_eq!(sgr_bg(0), b"\x1b[40m"); // black
        assert_eq!(sgr_bg(1), b"\x1b[41m"); // red
        assert_eq!(sgr_bg(5), b"\x1b[45m"); // magenta
        assert_eq!(sgr_bg(7), b"\x1b[47m"); // white
    }

    #[test]
    fn out_of_range_is_masked() {
        // 8 masks to 0; 15 masks to 7. Defensive only; callers should pass 0..=7.
        assert_eq!(sgr_fg(8), b"\x1b[30m");
        assert_eq!(sgr_bg(15), b"\x1b[47m");
    }
}
