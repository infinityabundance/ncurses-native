//! Dump the terminfo-derived terminal queries (longname/termname/has_ic/has_il)
//! for a loaded terminal, for the NCURSES.TERMATTRS oracle court.
//! Usage: `cargo run --quiet --example termattrs_dump -- <TERM>`

use ncurses_native::Terminfo;

fn main() {
    let term = std::env::args().nth(1).expect("usage: termattrs_dump <TERM>");
    let t = Terminfo::load(&term).unwrap_or_else(|e| {
        eprintln!("load {term} failed: {e:?}");
        std::process::exit(3);
    });
    println!("longname|{}", t.longname());
    println!("termname|{}", t.termname());
    println!("has_ic|{}", i32::from(t.has_ic()));
    println!("has_il|{}", i32::from(t.has_il()));
}
