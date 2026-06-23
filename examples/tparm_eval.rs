//! Evaluate one tparm case via the crate, for the NCURSES.TPARM oracle court.
//!
//! Usage: `cargo run --quiet --example tparm_eval -- <i|s> <cap-hex> [params...]`
//!   * mode `i`: params are decimal integers.
//!   * mode `s`: params are hex-encoded byte strings.
//! Writes the raw tparm result bytes to stdout.

use ncurses_native::{tparm, Param};

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|k| u8::from_str_radix(&s[2 * k..2 * k + 2], 16).unwrap())
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args[0].as_str();
    let cap = unhex(&args[1]);
    let params: Vec<Param> = args[2..]
        .iter()
        .map(|a| match mode {
            "s" => Param::Str(unhex(a)),
            _ => Param::Int(a.parse().unwrap()),
        })
        .collect();
    use std::io::Write;
    std::io::stdout().write_all(&tparm(&cap, &params)).unwrap();
}
