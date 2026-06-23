//! Oracle court fixture: emit the crate's full `mvcur` move matrix for the sampled 153-position
//! grid, in the same `fy,fx,ty,tx=<hex>` format as `tests/fixtures/mvcur_matrix.txt`, so the oracle
//! harness can compare it byte-for-byte against a fresh live-ncurses capture.
//!
//! Coordinates printed are 0-based (ncurses-internal); the public API is 1-based, so we add 1.
//! Usage: `cargo run --quiet --example mvcur_dump [vt100]` (default caps = xterm).

use std::io::Write;

use ncurses_native::{mvcur_caps, Caps};

const ROWS: [i32; 9] = [0, 1, 2, 3, 5, 8, 12, 18, 23];
const COLS: [i32; 17] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 15, 20, 39, 40, 78, 79];

fn main() {
    let caps = match std::env::args().nth(1).as_deref() {
        Some("vt100") => Caps::vt100(),
        _ => Caps::xterm(),
    };
    let mut out = String::new();
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
    std::io::stdout().write_all(out.as_bytes()).unwrap();
}
