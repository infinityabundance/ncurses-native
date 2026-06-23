//! Dump curs_set(visibility) bytes via the crate, for the NCURSES.CURS_SET court.
//! Usage: `cargo run --quiet --example curs_set_eval -- <file> <0|1|2>`

use ncurses_native::Terminfo;

fn main() {
    let path = std::env::args().nth(1).expect("usage: curs_set_eval <file> <level>");
    let level: i32 = std::env::args().nth(2).expect("level").parse().expect("int");
    let t = Terminfo::parse(&std::fs::read(&path).expect("read")).expect("parse");
    use std::io::Write;
    std::io::stdout().write_all(&t.curs_set(level).unwrap_or_default()).unwrap();
}
