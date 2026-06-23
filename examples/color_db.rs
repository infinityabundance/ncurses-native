//! Multi-terminal *color* doupdate dump: build a Screen via `Screen::from_terminfo`, enable color,
//! define two pairs, and emit the first-paint bytes of a scenario that exercises `_nc_do_color`
//! (setaf/setab or setf/setb, and the orig_pair reset). Measures the terminfo-driven color path
//! against real ncurses across the database.
//!
//! Usage: `cargo run --quiet --example color_db -- <tinfo_dir> <termlist_file>`
//! Output: `T <name>` (or `T <name> LOADFAIL`) then a single hex line of the first-paint bytes.

use std::io::Write;
use std::path::PathBuf;

use ncurses_native::color::Palette;
use ncurses_native::update::Screen;
use ncurses_native::window::{attrs, Cell};
use ncurses_native::Terminfo;

const ROWS: i32 = 24;
const COLS: i32 = 80;

fn putp(g: &mut [Cell], r: i32, s: &str, pair: u32) {
    let a = (pair << 8) & attrs::COLOR;
    for (i, ch) in s.chars().enumerate() {
        g[(r * COLS + i as i32) as usize] = Cell::plain(ch, a);
    }
}

fn main() {
    let dir = std::env::args().nth(1).expect("tinfo dir");
    let list = std::env::args().nth(2).expect("termlist file");
    let dirs = vec![PathBuf::from(&dir)];
    let names = std::fs::read_to_string(&list).expect("read termlist");
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
        s.start_color();
        s.init_pair(1, 1, 4); // red on blue
        s.init_pair(2, 2, 0); // green on black
        let mut g = vec![Cell::plain(' ', 0); (ROWS * COLS) as usize];
        putp(&mut g, 1, "red", 1);
        putp(&mut g, 2, "grn", 2);
        putp(&mut g, 3, "def", 0);
        // Mirror the oracle (initscr; refresh blank; clearok; paint; refresh): a blank first paint,
        // then a clearok-forced repaint of the scene -- so the cursor state matches across engines.
        let blank = vec![Cell::plain(' ', 0); (ROWS * COLS) as usize];
        let _ = s.doupdate(&blank, 0, 0);
        s.clearok();
        let bytes = s.doupdate(&g, 4, 0);
        out.push_str(&format!("T {name}\n"));
        for b in &bytes {
            out.push_str(&format!("{b:02x}"));
        }
        out.push('\n');
    }
    std::io::stdout().write_all(out.as_bytes()).unwrap();
}
