//! Load a terminal by name from the terminfo database (via the crate's loader)
//! and dump every standard capability, same format as `tinfo_dump`. Paired with
//! a C `setupterm`+`tiget*` dumper for the NCURSES.TERMINFO.ECOLOGY court.
//!
//! Usage: `cargo run --quiet --example tinfo_load_dump -- <TERM>`

use ncurses_native::terminfo::{caps, Tigetstr};
use ncurses_native::Terminfo;

fn main() {
    let term = std::env::args().nth(1).expect("usage: tinfo_load_dump <TERM>");
    let t = match Terminfo::load(&term) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("load {term} failed: {e:?}");
            std::process::exit(3);
        }
    };
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
