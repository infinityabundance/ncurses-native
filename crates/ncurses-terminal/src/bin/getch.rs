//! A native `getch`: read keys from the real tty (fd 0) in raw mode and print the `KEY_*` /
//! literal-byte codes one per line, until the terminator byte `Q` (113... 81) is read. This is the
//! end-to-end input path -- the byte-exact `KeyMap` decoder driven over a live raw terminal -- and
//! is courted against a real ncurses `getch` loop (oracle `NCURSES.INPUT.LIVE`).

use ncurses_native::{KeyMap, Terminfo};
use ncurses_terminal::{Keys, RawMode};

fn main() {
    let t = Terminfo::load("xterm").expect("load xterm terminfo");
    let km = KeyMap::from_terminfo(&t);
    let _raw = RawMode::enter(0).expect("stdin is not a tty");
    let mut keys = Keys::new(0, km, 50);
    let mut out = String::new();
    while let Some(code) = keys.next_code() {
        if code == b'Q' as i32 {
            break;
        }
        out.push_str(&code.to_string());
        out.push('\n');
        // flush per key so the harness sees output promptly
        print!("{out}");
        use std::io::Write;
        let _ = std::io::stdout().flush();
        out.clear();
    }
}
