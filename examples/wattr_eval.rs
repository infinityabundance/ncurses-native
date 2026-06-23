//! Run a window attribute script and dump each cell's character + attribute,
//! for the NCURSES.WINDOW.ATTR court.
//! Usage: `cargo run --quiet --example wattr_eval -- <rows> <cols> <op>...`
//!   ops: `m:y:x`, `s:HEX`, `t:MASK` (attrset), `o:MASK` (attron),
//!        `f:MASK` (attroff), `g:N:MASK` (chgat), `e` (erase).
//! Output: `rows` lines of `cols` `char,attrhex` cells.

use ncurses_native::window::Window;

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
            "t" => w.attrset(p[1].parse().unwrap()),
            "o" => w.attron(p[1].parse().unwrap()),
            "f" => w.attroff(p[1].parse().unwrap()),
            "g" => w.chgat(p[1].parse().unwrap(), p[2].parse().unwrap()),
            "p" => w.color_set(p[1].parse().unwrap()),
            "C" => {
                let items: Vec<(u8, u32)> = p[1]
                    .split(',')
                    .map(|v| {
                        let n: u32 = v.parse().unwrap();
                        ((n & 0xff) as u8, n & 0xffff_ff00)
                    })
                    .collect();
                w.addchstr(&items);
            }
            "e" => w.erase(),
            other => panic!("bad op {other}"),
        }
    }
    let mut out = String::new();
    for y in 0..rows {
        let cells: Vec<String> = (0..cols)
            .map(|x| {
                let c = w.cell(y, x);
                format!("{},{:x}", c.ch, c.attr)
            })
            .collect();
        out.push_str(&cells.join(" "));
        out.push('\n');
    }
    print!("{out}");
}
