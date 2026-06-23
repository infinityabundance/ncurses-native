//! Proof that the terminfo reader parses the *extended* (user-defined, `tic -x`) string section:
//! the committed xterm fixture's `E3` (clear-scrollback) extension must resolve to `\e[3J`, which is
//! what `tput clear` / `clear` append after the standard `clear` capability.

use ncurses_native::Terminfo;

#[test]
fn xterm_extended_e3_is_clear_scrollback() {
    let bytes = std::fs::read("tests/terminfo/xterm").expect("read fixture");
    let t = Terminfo::parse(&bytes).expect("parse terminfo");
    assert_eq!(t.ext_string("E3"), Some(b"\x1b[3J".as_ref()));
    // A non-existent extended cap is absent.
    assert_eq!(t.ext_string("NoSuchCap"), None);
}
