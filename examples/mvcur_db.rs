//! General multi-terminal mvcur dump: for each terminal name read from the list file, load its
//! terminfo via `Caps::from_terminfo` and emit the crate's mvcur output over a small sampled grid.
//! Used by the terminfo-general mvcur court to compare against real ncurses across the whole
//! installed database.
//!
//! Usage: `cargo run --quiet --example mvcur_db -- <tinfo_dir> <termlist_file>`
//! Output: `T <name>` header per terminal (or `T <name> LOADFAIL`), then `fy,fx,ty,tx=<hex>` lines.

use std::io::Write;
use std::path::PathBuf;

use ncurses_native::{mvcur_caps, Caps, Terminfo};

// A small grid that still exercises every tactic (diagonals, edges, same-row/col, not-local).
const ROWS: [i32; 4] = [0, 2, 11, 23];
const COLS: [i32; 5] = [0, 1, 5, 40, 79];

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
        let caps = Caps::from_terminfo(&t);
        out.push_str(&format!("T {name}\n"));
        for &fy in &ROWS {
            for &fx in &COLS {
                for &ty in &ROWS {
                    for &tx in &COLS {
                        let bytes = mvcur_caps(&caps, (fy + 1, fx + 1), (ty + 1, tx + 1));
                        out.push_str(&format!("{fy},{fx},{ty},{tx}="));
                        for b in &bytes {
                            out.push_str(&format!("{b:02x}"));
                        }
                        out.push('\n');
                    }
                }
            }
        }
    }
    std::io::stdout().write_all(out.as_bytes()).unwrap();
}
