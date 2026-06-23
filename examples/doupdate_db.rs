//! General multi-terminal doupdate dump: for each terminal name in the list file, build a Screen via
//! `Screen::from_terminfo` and emit the first-paint bytes of a fixed plain-text scenario. Used to
//! measure the terminfo-driven doupdate engine against real ncurses across the database.
//!
//! Usage: `cargo run --quiet --example doupdate_db -- <tinfo_dir> <termlist_file>`
//! Output: `T <name>` (or `T <name> LOADFAIL`) then a single hex line of the first-paint bytes.

use std::io::Write;
use std::path::PathBuf;

use ncurses_native::color::Palette;
use ncurses_native::update::Screen;
use ncurses_native::window::Cell;
use ncurses_native::Terminfo;

const ROWS: i32 = 24;
const COLS: i32 = 80;

// Fixed plain-text scenario: "hello world" at (2,5) and "abc" at (5,0).
fn scenario() -> (Vec<Cell>, i32, i32) {
    let mut g = vec![Cell::plain(' ', 0); (ROWS * COLS) as usize];
    let put = |g: &mut Vec<Cell>, r: i32, c: i32, s: &str| {
        for (i, ch) in s.chars().enumerate() {
            g[(r * COLS + c + i as i32) as usize] = Cell::plain(ch, 0);
        }
    };
    put(&mut g, 2, 5, "hello world");
    put(&mut g, 5, 0, "abc");
    (g, 5, 3) // cursor parked after "abc"
}

fn main() {
    let dir = std::env::args().nth(1).expect("tinfo dir");
    let list = std::env::args().nth(2).expect("termlist file");
    let dirs = vec![PathBuf::from(&dir)];
    let names = std::fs::read_to_string(&list).expect("read termlist");
    let (cells, cy, cx) = scenario();
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
        let bytes = s.doupdate(&cells, cy, cx);
        out.push_str(&format!("T {name}\n"));
        for b in &bytes {
            out.push_str(&format!("{b:02x}"));
        }
        out.push('\n');
    }
    std::io::stdout().write_all(out.as_bytes()).unwrap();
}
