//! Dump slk_label read-back via the crate, for the NCURSES.SLK court.
use ncurses_native::slk::SoftLabels;
fn main() {
    let mut s = SoftLabels::new();
    s.set(1, "File");
    s.set(2, "Edit");
    s.set(3, "Quit");
    s.set(4, "VeryLongLabel");
    let mut out = String::new();
    for n in 0..=9 {
        let l = s.label(n).unwrap_or_else(|| "(null)".to_string());
        out.push_str(&format!("label({n})=|{l}|\n"));
    }
    print!("{out}");
}
