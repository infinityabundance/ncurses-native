//! Run a window-op script on the crate's Window model and dump the resulting
//! character grid, for the NCURSES.WINDOW.STATE court.
//!
//! Usage: `cargo run --quiet --example win_eval -- <rows> <cols> <op>...`
//!   ops: `m:y:x` (move), `s:HEX` (addstr of hex-decoded bytes), `e` (erase),
//!        `a:y:x:HEX` (mvaddstr).
//! Output: `rows` lines of `cols` bytes, then `cur:y:x`.

use ncurses_native::Window;

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|k| u8::from_str_radix(&s[2 * k..2 * k + 2], 16).unwrap())
        .collect()
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let rows: i32 = a[0].parse().unwrap();
    let cols: i32 = a[1].parse().unwrap();
    let mut w = Window::new(rows, cols);
    for op in &a[2..] {
        let p: Vec<&str> = op.split(':').collect();
        match p[0] {
            "m" => {
                w.move_to(p[1].parse().unwrap(), p[2].parse().unwrap());
            }
            "s" => w.addstr(&unhex(p[1])),
            "a" => w.mvaddstr(p[1].parse().unwrap(), p[2].parse().unwrap(), &unhex(p[3])),
            "e" => w.erase(),
            "i" => w.insstr(&unhex(p[1])),
            "x" => w.delch(),
            "L" => w.insertln(),
            "D" => w.deleteln(),
            "n" => w.insdelln(p[1].parse().unwrap()),
            "h" => w.hline(p[1].parse().unwrap(), p[2].parse().unwrap()),
            "v" => w.vline(p[1].parse().unwrap(), p[2].parse().unwrap()),
            "G" => {
                w.set_scrreg(p[1].parse().unwrap(), p[2].parse().unwrap());
            }
            "R" => {
                w.set_scrollok(true);
                w.scroll(p[1].parse().unwrap());
            }
            "b" => w.draw_box(p[1].parse().unwrap(), p[2].parse().unwrap()),
            "B" => w.border(
                p[1].parse().unwrap(),
                p[2].parse().unwrap(),
                p[3].parse().unwrap(),
                p[4].parse().unwrap(),
                p[5].parse().unwrap(),
                p[6].parse().unwrap(),
                p[7].parse().unwrap(),
                p[8].parse().unwrap(),
            ),
            other => panic!("bad op {other}"),
        }
    }
    use std::io::Write;
    let mut out = Vec::new();
    for row in w.grid() {
        out.extend_from_slice(&row);
        out.push(b'\n');
    }
    let (y, x) = w.getyx();
    out.extend_from_slice(format!("cur:{y}:{x}\n").as_bytes());
    std::io::stdout().write_all(&out).unwrap();
}
