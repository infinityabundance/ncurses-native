//! SGR display attributes -- the byte sequences ncurses emits to switch a monochrome attribute on
//! and off, on the admitted xterm/ncurses 6.4 terminal.
//!
//! These are terminal/curses attribute names (`Bold`, not "highlight"). Each maps to the SGR
//! numeric parameter ncurses emits via `set_attributes`. The on-sequence designates the ASCII
//! charset, sets the attribute, then restores the default colors; the off-sequence designates the
//! charset, resets all SGR, then restores the default colors. Both were observed byte-exact against
//! the admitted terminal.

/// A monochrome SGR display attribute. Each maps to its SGR numeric parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attr {
    /// Bold / increased intensity -- SGR `1`.
    Bold,
    /// Dim / decreased intensity -- SGR `2`.
    Dim,
    /// Underline -- SGR `4`.
    Underline,
    /// Blink -- SGR `5`.
    Blink,
    /// Reverse video -- SGR `7`.
    Reverse,
}

impl Attr {
    /// The SGR numeric parameter for this attribute (used in `\e[0;<n>m`).
    pub fn sgr_code(self) -> u8 {
        match self {
            Attr::Bold => 1,
            Attr::Dim => 2,
            Attr::Underline => 4,
            Attr::Blink => 5,
            Attr::Reverse => 7,
        }
    }
}

/// The SGR "attribute on" sequence ncurses emits before an attributed field: the ASCII-charset
/// designation `\e(B`, the `set_attributes` SGR `\e[0;<n>m`, then the default-color restore
/// `\e[39;49m\e[37m\e[40m`.
pub fn sgr_on(attr: Attr) -> Vec<u8> {
    let mut v = b"\x1b(B\x1b[0;".to_vec();
    v.extend_from_slice(attr.sgr_code().to_string().as_bytes());
    v.extend_from_slice(b"m\x1b[39;49m\x1b[37m\x1b[40m");
    v
}

/// The SGR "attribute off" sequence ncurses emits after an attributed field: charset designation
/// `\e(B`, the `sgr0` reset `\e[m`, then the default-color restore. Constant for every monochrome
/// attribute.
pub fn sgr_off() -> Vec<u8> {
    b"\x1b(B\x1b[m\x1b[39;49m\x1b[37m\x1b[40m".to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sgr_codes() {
        assert_eq!(Attr::Bold.sgr_code(), 1);
        assert_eq!(Attr::Dim.sgr_code(), 2);
        assert_eq!(Attr::Underline.sgr_code(), 4);
        assert_eq!(Attr::Blink.sgr_code(), 5);
        assert_eq!(Attr::Reverse.sgr_code(), 7);
    }

    #[test]
    fn on_sequences_match_oracle() {
        assert_eq!(sgr_on(Attr::Bold), b"\x1b(B\x1b[0;1m\x1b[39;49m\x1b[37m\x1b[40m");
        assert_eq!(sgr_on(Attr::Dim), b"\x1b(B\x1b[0;2m\x1b[39;49m\x1b[37m\x1b[40m");
        assert_eq!(sgr_on(Attr::Underline), b"\x1b(B\x1b[0;4m\x1b[39;49m\x1b[37m\x1b[40m");
        assert_eq!(sgr_on(Attr::Blink), b"\x1b(B\x1b[0;5m\x1b[39;49m\x1b[37m\x1b[40m");
        assert_eq!(sgr_on(Attr::Reverse), b"\x1b(B\x1b[0;7m\x1b[39;49m\x1b[37m\x1b[40m");
    }

    #[test]
    fn off_sequence_is_constant() {
        assert_eq!(sgr_off(), b"\x1b(B\x1b[m\x1b[39;49m\x1b[37m\x1b[40m");
        // off does not depend on which attribute was on.
        assert_eq!(sgr_off(), sgr_off());
    }
}
