//! General multi-terminal *incremental* doupdate dump: build a Screen via `Screen::from_terminfo`,
//! paint scene A (first doupdate), then emit the bytes of the diff to scene B (second doupdate).
//! Used to measure the terminfo-driven TransformLine / clr_eol / ich-dch diff path against real
//! ncurses across the whole database (the first-paint sweep is `doupdate_db`).
//!
//! Usage: `cargo run --quiet --example doupdate_db2 -- <tinfo_dir> <termlist_file>`
//! Output: `T <name>` (or `T <name> LOADFAIL`) then a single hex line of the scene-A->B diff bytes.

use std::io::Write;
use std::path::PathBuf;

use ncurses_native::color::Palette;
use ncurses_native::update::Screen;
use ncurses_native::window::Cell;
use ncurses_native::Terminfo;

const ROWS: i32 = 24;
const COLS: i32 = 80;

fn put(g: &mut [Cell], r: i32, c: i32, s: &str) {
    for (i, ch) in s.chars().enumerate() {
        g[(r * COLS + c + i as i32) as usize] = Cell::plain(ch, 0);
    }
}

// Scene A: "hello world" at (2,5), "abc" at (5,0).
fn scene_a() -> Vec<Cell> {
    let mut g = vec![Cell::plain(' ', 0); (ROWS * COLS) as usize];
    put(&mut g, 2, 5, "hello world");
    put(&mut g, 5, 0, "abc");
    g
}

// Scene B (the diff target): "hello world" shortened to "HELLO" (trailing " world" cleared),
// "abc" extended to "abcdef", and a new "xyz" at (10,20).
fn scene_b() -> Vec<Cell> {
    let mut g = vec![Cell::plain(' ', 0); (ROWS * COLS) as usize];
    put(&mut g, 2, 5, "HELLO");
    put(&mut g, 5, 0, "abcdef");
    put(&mut g, 10, 20, "xyz");
    g
}

fn main() {
    let dir = std::env::args().nth(1).expect("tinfo dir");
    let list = std::env::args().nth(2).expect("termlist file");
    let dirs = vec![PathBuf::from(&dir)];
    let names = std::fs::read_to_string(&list).expect("read termlist");
    let (a, b) = (scene_a(), scene_b());
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
        let _ = s.doupdate(&a, 5, 3); // scene A (first paint)
        let bytes = s.doupdate(&b, 11, 0); // diff to scene B
        out.push_str(&format!("T {name}\n"));
        for byte in &bytes {
            out.push_str(&format!("{byte:02x}"));
        }
        out.push('\n');
    }
    std::io::stdout().write_all(out.as_bytes()).unwrap();
}
