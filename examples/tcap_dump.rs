//! Dump the termcap layer (tgetflag/tgetnum/tgetstr by two-letter code, plus
//! tgoto over a sweep) via the crate, for the NCURSES.TERMCAP oracle court.
//! Usage: `cargo run --quiet --example tcap_dump -- <compiled-terminfo-file>`

use ncurses_native::terminfo::{caps, tgoto, Terminfo};

const GOTO: &[(i32, i32)] = &[(0, 0), (4, 2), (79, 23), (9, 9), (40, 12)];

fn main() {
    let path = std::env::args().nth(1).expect("usage: tcap_dump <file>");
    let t = Terminfo::parse(&std::fs::read(&path).expect("read")).expect("parse");
    let mut out = String::new();
    for &code in caps::BOOL_CODES {
        if !code.is_empty() {
            out.push_str(&format!("B|{}|{}\n", code, t.tgetflag(code)));
        }
    }
    for &code in caps::NUM_CODES {
        if !code.is_empty() {
            out.push_str(&format!("N|{}|{}\n", code, t.tgetnum(code)));
        }
    }
    for &code in caps::STR_CODES {
        if code.is_empty() {
            continue;
        }
        match t.tgetstr(code) {
            Some(b) => {
                out.push_str(&format!("S|{}|HEX:", code));
                for byte in b {
                    out.push_str(&format!("{byte:02x}"));
                }
                out.push('\n');
            }
            None => out.push_str(&format!("S|{}|ABSENT\n", code)),
        }
    }
    if let Some(cm) = t.tgetstr("cm").map(<[u8]>::to_vec) {
        for &(col, row) in GOTO {
            out.push_str(&format!("G|{},{}|HEX:", col, row));
            for byte in tgoto(&cm, col, row) {
                out.push_str(&format!("{byte:02x}"));
            }
            out.push('\n');
        }
    }
    print!("{out}");
}
