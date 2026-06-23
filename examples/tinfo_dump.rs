//! Dump every standard terminfo capability of a compiled entry in a canonical
//! text form, via the crate's own reader. Paired with a C program that dumps the
//! same caps through ncurses' `tigetflag`/`tigetnum`/`tigetstr`, so the oracle
//! harness can diff the two over the whole entry.
//!
//! Usage: `cargo run --quiet --example tinfo_dump -- <compiled-terminfo-file>`
//!
//! Format (one line per cap, table order): `B|<name>|<-1|0|1>`,
//! `N|<name>|<value|-1|-2>`, `S|<name>|ABSENT` or `S|<name>|HEX:<lowerhex>`.

use ncurses_native::terminfo::{caps, Terminfo, Tigetstr};

fn main() {
    let path = std::env::args().nth(1).expect("usage: tinfo_dump <file>");
    let data = std::fs::read(&path).expect("read terminfo file");
    let t = Terminfo::parse(&data).expect("parse terminfo");
    let mut out = String::new();
    for &name in caps::BOOL_NAMES {
        out.push_str(&format!("B|{}|{}\n", name, t.tigetflag(name)));
    }
    for &name in caps::NUM_NAMES {
        out.push_str(&format!("N|{}|{}\n", name, t.tigetnum(name)));
    }
    for &name in caps::STR_NAMES {
        match t.tigetstr(name) {
            Tigetstr::Value(b) => {
                out.push_str(&format!("S|{}|HEX:", name));
                for byte in b {
                    out.push_str(&format!("{byte:02x}"));
                }
                out.push('\n');
            }
            Tigetstr::Absent => out.push_str(&format!("S|{}|ABSENT\n", name)),
            Tigetstr::NotString => out.push_str(&format!("S|{}|NOTSTR\n", name)),
        }
    }
    print!("{out}");
}
