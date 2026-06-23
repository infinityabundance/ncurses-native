//! Dump color-pair registry results via the crate, for the NCURSES.COLOR.PAIR
//! court. Initializes a fixed set of pairs and prints pair_content plus the
//! COLOR_PAIR / PAIR_NUMBER macro results.

use ncurses_native::color::Palette;
use ncurses_native::window::{color_pair, pair_number};

fn main() {
    let mut p = Palette::new();
    p.init_pair(1, 1, 4); // RED on BLUE
    p.init_pair(2, 2, 0); // GREEN on BLACK
    p.init_pair(5, 3, 4); // YELLOW on BLUE
    let mut out = String::new();
    for pair in [0i16, 1, 2, 3, 5, 9] {
        let (f, b) = p.pair_content(pair);
        out.push_str(&format!("pair_content({pair})={f},{b}\n"));
    }
    out.push_str(&format!("COLOR_PAIR(1)={}\n", color_pair(1)));
    out.push_str(&format!("COLOR_PAIR(2)={}\n", color_pair(2)));
    out.push_str(&format!("PAIR_NUMBER(0x{:x})={}\n", 0x300, pair_number(0x300)));
    print!("{out}");
}
