//! Dump key_defined / has_key for the xterm fixture, for NCURSES.KEY.DEFINED.
//! Usage: `key_eval d <hexseq>...`  or  `key_eval h <code>...`
use ncurses_native::{KeyMap, Terminfo};
fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).map(|k| u8::from_str_radix(&s[2 * k..2 * k + 2], 16).unwrap()).collect()
}
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let t = Terminfo::parse(include_bytes!("../tests/terminfo/xterm")).unwrap();
    let km = KeyMap::from_terminfo(&t);
    let mut out = String::new();
    match a[0].as_str() {
        "d" => for s in &a[1..] { out.push_str(&format!("{}\t{}\n", s, km.key_defined(&unhex(s)))); },
        "h" => for c in &a[1..] { out.push_str(&format!("{}\t{}\n", c, i32::from(km.has_key(c.parse().unwrap())))); },
        _ => {}
    }
    print!("{out}");
}
