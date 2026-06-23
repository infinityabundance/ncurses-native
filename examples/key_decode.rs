//! Oracle court fixture for input key decoding. Loads the xterm terminfo, builds the key map, and
//! decodes a hex-encoded input byte buffer (argv[1]) into the keycode sequence `wgetch` would
//! return -- one decimal code per line -- so the harness can compare it to a real ncurses getch
//! loop fed the same bytes.
//!
//! Usage: `cargo run --quiet --example key_decode -- <hex-bytes>`

use ncurses_native::{KeyMap, Terminfo};

fn unhex(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    (0..b.len() / 2)
        .map(|i| {
            let h = (b[2 * i] as char).to_digit(16).unwrap() as u8;
            let l = (b[2 * i + 1] as char).to_digit(16).unwrap() as u8;
            (h << 4) | l
        })
        .collect()
}

fn main() {
    let hex = std::env::args().nth(1).unwrap_or_default();
    let bytes = unhex(&hex);
    let t = Terminfo::load("xterm").expect("load xterm terminfo");
    let km = KeyMap::from_terminfo(&t);
    let mut out = String::new();
    for code in km.decode(&bytes) {
        out.push_str(&code.to_string());
        out.push('\n');
    }
    print!("{out}");
}
