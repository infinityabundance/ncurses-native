//! Dump color_content + find/alloc/free_pair via the crate, for color courts.
use ncurses_native::color::Palette;
fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    let mut p = Palette::new();
    let mut out = String::new();
    match mode.as_str() {
        "content" => for c in 0..8 {
            let (r, g, b) = p.color_content(c);
            out.push_str(&format!("color_content({c})={r},{g},{b}\n"));
        },
        "alloc" => {
            p.init_pair(1, 1, 4); p.init_pair(2, 2, 0);
            out.push_str(&format!("find(1,4)={}\n", p.find_pair(1, 4)));
            out.push_str(&format!("find(7,7)={}\n", p.find_pair(7, 7)));
            out.push_str(&format!("alloc(3,5)={}\n", p.alloc_pair(3, 5)));
            out.push_str(&format!("alloc(3,5)={}\n", p.alloc_pair(3, 5)));
            out.push_str(&format!("alloc(6,6)={}\n", p.alloc_pair(6, 6)));
        }
        _ => {}
    }
    print!("{out}");
}
