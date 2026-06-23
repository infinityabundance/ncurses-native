//! Audible and visible bell -- the byte sequences ncurses emits for `beep()` and
//! `flash()` on the admitted xterm terminal.
//!
//! This is the first ncurses man-page cluster (`curs_beep`) reconstructed to
//! **full** byte parity: every public function in the group is a pure terminfo
//! output capability with no window state, so the observable bytes are exactly
//! the resolved capability string.
//!
//! * `beep()` emits the `bel` capability. For xterm `bel = ^G`, i.e. the single
//!   byte `0x07`. There is no escape sequence and no parameter.
//! * `flash()` emits the `flash` capability. For xterm the terminfo string is
//!   `\E[?5h$<100/>\E[?5l`: turn on reverse-screen (DECSCNM), wait, turn it off.
//!   The `$<100/>` is a *proportional* delay (the trailing `/`), which ncurses'
//!   `tputs` realizes as a ~100ms real-time pause -- NOT as emitted padding
//!   bytes -- because xterm declares no pad character and the delay is forced.
//!   So the byte stream a terminal actually receives is just `\e[?5h\e[?5l`; the
//!   pause happens between the two writes and leaves no trace in the bytes. This
//!   is why the reconstructed `flash()` carries no delay bytes and still matches
//!   the live `flash()` capture (court NCURSES.CAP.FLASH).
//!
//! The `_sp` reentrant variants (`beep_sp`/`flash_sp`) select which `SCREEN`'s
//! output stream the bytes go to; the byte sequence itself is identical, so the
//! same producers reconstruct them. The screen-pointer plumbing is not modeled.

/// `beep()` -- the audible bell. Emits the `bel` capability, which for the
/// admitted xterm terminal is the single byte `0x07` (`^G`). Pinned to the live
/// terminfo entry by court NCURSES.CAP.BEL.
pub fn beep() -> &'static [u8] {
    b"\x07"
}

/// `flash()` -- the visible bell. Emits the `flash` capability with its delay
/// realized as a real-time pause rather than padding bytes (see the module
/// docs), so the observable byte stream is `\e[?5h\e[?5l`: DECSCNM on, then off.
/// Pinned to the live terminfo entry by court NCURSES.CAP.FLASH.
pub fn flash() -> &'static [u8] {
    b"\x1b[?5h\x1b[?5l"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beep_is_bel() {
        // xterm bel = ^G = 0x07; no escape sequence.
        assert_eq!(beep(), b"\x07");
    }

    #[test]
    fn flash_is_decscnm_on_off() {
        // The $<100/> proportional delay is a real-time pause, not bytes, so the
        // emitted stream is exactly the reverse-screen on/off pair.
        assert_eq!(flash(), b"\x1b[?5h\x1b[?5l");
        assert!(!flash().windows(2).any(|w| w == b"$<"));
    }
}
