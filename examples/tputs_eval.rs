//! Evaluate one tputs case via the crate, for the NCURSES.TPUTS oracle court.
//! Usage: `cargo run --quiet --example tputs_eval -- <hex-string>` -> raw tputs bytes.

use ncurses_native::tputs;

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|k| u8::from_str_radix(&s[2 * k..2 * k + 2], 16).unwrap())
        .collect()
}

fn main() {
    let arg = std::env::args().nth(1).expect("usage: tputs_eval <hex>");
    use std::io::Write;
    std::io::stdout().write_all(&tputs(&unhex(&arg))).unwrap();
}
