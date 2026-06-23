//! `clear` -- clear the screen, byte-identically to ncurses `clear(1)`: the `clear` capability
//! followed by the `E3` (clear-scrollback) extension when the terminal has it.

use std::io::Write;
use std::process::exit;

use ncurses_native::terminfo::search_dirs;
use ncurses_native::{tputs, Terminfo, Tigetstr};

fn main() {
    let term = std::env::var("TERM").unwrap_or_default();
    let ti = match Terminfo::load_from(&term, &search_dirs()) {
        Ok(t) => t,
        Err(_) => exit(1),
    };
    let mut out = Vec::new();
    if let Tigetstr::Value(v) = ti.tigetstr("clear") {
        out.extend(tputs(v));
    }
    if let Some(e3) = ti.ext_string("E3") {
        out.extend(tputs(e3));
    }
    let _ = std::io::stdout().write_all(&out);
    let _ = std::io::stdout().flush();
}
