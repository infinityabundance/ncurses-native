//! Multi-terminal *attribute* doupdate dump: build a Screen via `Screen::from_terminfo` and emit the
//! first-paint bytes of a scenario that exercises the SGR/attribute engine (bold/underline/reverse/
//! standout/blink/dim and the reset back to normal). Used to measure the terminfo-driven attribute
//! path (sgr vs individual mode caps) against real ncurses across the database.
//!
//! Usage: `cargo run --quiet --example attr_db -- <tinfo_dir> <termlist_file>`
//! Output: `T <name>` (or `T <name> LOADFAIL`) then a single hex line of the first-paint bytes.

use std::io::Write;
use std::path::PathBuf;

use ncurses_native::color::Palette;
use ncurses_native::update::Screen;
use ncurses_native::window::{attrs, Cell};
use ncurses_native::Terminfo;

const ROWS: i32 = 24;
const COLS: i32 = 80;

fn puta(g: &mut [Cell], r: i32, c: i32, s: &str, a: u32) {
    for (i, ch) in s.chars().enumerate() {
        g[(r * COLS + c + i as i32) as usize] = Cell::plain(ch, a);
    }
}

// Each row carries a distinct attribute, so the diff exercises every TurnOn/TurnOff transition.
fn scene() -> Vec<Cell> {
    let mut g = vec![Cell::plain(' ', 0); (ROWS * COLS) as usize];
    puta(&mut g, 1, 0, "bold", attrs::BOLD);
    puta(&mut g, 2, 0, "undl", attrs::UNDERLINE);
    puta(&mut g, 3, 0, "revs", attrs::REVERSE);
    puta(&mut g, 4, 0, "stnd", attrs::STANDOUT);
    puta(&mut g, 5, 0, "blnk", attrs::BLINK);
    puta(&mut g, 6, 0, "dimm", attrs::DIM);
    puta(&mut g, 7, 0, "norm", attrs::NORMAL);
    g
}

fn main() {
    let dir = std::env::args().nth(1).expect("tinfo dir");
    let list = std::env::args().nth(2).expect("termlist file");
    let dirs = vec![PathBuf::from(&dir)];
    let names = std::fs::read_to_string(&list).expect("read termlist");
    let cells = scene();
    let mut out = String::new();
    for name in names.split_whitespace() {
        let t = match Terminfo::load_from(name, &dirs) {
            Ok(t) => t,
            Err(_) => {
                out.push_str(&format!("T {name} LOADFAIL\n"));
                continue;
            }
        };
        let mut s = Screen::from_terminfo(ROWS, COLS, &t, Palette::new());
        let bytes = s.doupdate(&cells, 8, 0);
        out.push_str(&format!("T {name}\n"));
        for b in &bytes {
            out.push_str(&format!("{b:02x}"));
        }
        out.push('\n');
    }
    std::io::stdout().write_all(out.as_bytes()).unwrap();
}
