//! `tput` -- emit a terminal capability, byte-identically to ncurses `tput(1)`.
//!
//! `tput [-T term] capname [params...]`:
//!   * string cap   -> tparm(cap, params) piped through tputs, written raw (no newline);
//!   * number cap   -> the number, then a newline;
//!   * boolean cap  -> no output; exit 0 if set, 1 if unset;
//!   * `longname`   -> the entry's long name (no trailing newline, as ncurses);
//!   * `clear`      -> the clear cap then the `E3` (clear-scrollback) extension, like ncurses.
//!
//! Exit codes mirror ncurses: 0 success, 1 cap absent/false, 2 usage, 4 unknown term/cap.

use std::io::Write;
use std::process::exit;

use ncurses_native::terminfo::search_dirs;
use ncurses_native::{tparm_n, tputs, Terminfo, Tigetstr};

fn write_raw(bytes: &[u8]) {
    let _ = std::io::stdout().write_all(bytes);
    let _ = std::io::stdout().flush();
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut term: Option<String> = None;
    let mut rest: Vec<String> = Vec::new();
    let mut i = 0;
    while i < argv.len() {
        let a = &argv[i];
        if a == "-T" && i + 1 < argv.len() {
            term = Some(argv[i + 1].clone());
            i += 2;
        } else if let Some(t) = a.strip_prefix("-T") {
            term = Some(t.to_string());
            i += 1;
        } else {
            rest.push(a.clone());
            i += 1;
        }
    }
    if rest.is_empty() {
        exit(2);
    }
    let term = term
        .or_else(|| std::env::var("TERM").ok())
        .unwrap_or_default();
    let ti = match Terminfo::load_from(&term, &search_dirs()) {
        Ok(t) => t,
        Err(_) => exit(4),
    };
    let cap = rest[0].as_str();
    let params: Vec<i32> = rest[1..].iter().map(|s| s.parse().unwrap_or(0)).collect();

    if cap == "longname" {
        write_raw(ti.longname().as_bytes());
        exit(0);
    }
    if cap == "clear" {
        // ncurses tput clear emits the clear cap, then the E3 clear-scrollback extension if present.
        let mut out = Vec::new();
        if let Tigetstr::Value(v) = ti.tigetstr("clear") {
            out.extend(tputs(v));
        }
        if let Some(e3) = ti.ext_string("E3") {
            out.extend(tputs(e3));
        }
        write_raw(&out);
        exit(0);
    }

    match ti.tigetstr(cap) {
        Tigetstr::Value(v) => {
            let expanded = if params.is_empty() {
                v.to_vec()
            } else {
                tparm_n(v, &params)
            };
            write_raw(&tputs(&expanded));
            exit(0);
        }
        Tigetstr::Absent => exit(1),
        Tigetstr::NotString => {
            let n = ti.tigetnum(cap);
            if n != -2 {
                println!("{n}");
                exit(0);
            }
            match ti.tigetflag(cap) {
                1 => exit(0),
                0 => exit(1),
                _ => exit(4), // not a capability name at all
            }
        }
    }
}
