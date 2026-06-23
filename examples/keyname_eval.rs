//! Dump keyname(code) over a range, for the NCURSES.KEYNAME court.
//! Output: one line per code, `code\t<name|(null)>`.
use ncurses_native::keyname;
fn main() {
    let mut out = String::new();
    for code in -1..=600 {
        let n = keyname(code).unwrap_or_else(|| "(null)".to_string());
        out.push_str(&format!("{code}\t{n}\n"));
    }
    print!("{out}");
}
