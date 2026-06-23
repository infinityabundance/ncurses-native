//! ANSI / curses color -- the foreground and background SGR sequences for the 8 ANSI colors, on the
//! admitted xterm/ncurses 6.4 terminal.
//!
//! Colors here are **ANSI/curses color numbers** `0..=7` directly: `0` black, `1` red, `2` green,
//! `3` yellow, `4` blue, `5` magenta, `6` cyan, `7` white. There is **no COBOL color mapping** in
//! this crate -- callers that need a different numbering convention apply it before calling here.
//!
//! The default color pair (white-on-black, [`DEFAULT_PAIR`]) is curses pair 0, which is always
//! allocated and needs no SGR.

/// The default color pair `(foreground, background) = (7, 0)` -- white on black, curses pair 0.
/// A field in the default pair needs no SGR (the terminal is already in it after init).
pub const DEFAULT_PAIR: (u8, u8) = (7, 0);

/// A color-pair table -- the `init_pair`/`pair_content` state ncurses keeps in
/// the `SCREEN`. Pair 0 is the fixed default `(-1, -1)` / white-on-black.
///
/// Reconstructs the pair registry, not the rgb palette: `init_color`/
/// `color_content` (which need a `can_change_color` terminal) and the extended
/// 32-bit color API are out of scope. Pinned by court NCURSES.COLOR.PAIR.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Palette {
    pairs: std::collections::BTreeMap<i16, (i16, i16)>,
}

impl Palette {
    /// A fresh palette (as after `start_color`), with only pair 0 implicit.
    pub fn new() -> Palette {
        Palette::default()
    }

    /// `init_pair(pair, fg, bg)` -- register a color pair (`pair >= 1`).
    pub fn init_pair(&mut self, pair: i16, fg: i16, bg: i16) -> bool {
        if pair < 1 {
            return false;
        }
        self.pairs.insert(pair, (fg, bg));
        true
    }

    /// `pair_content(pair)` -- the `(fg, bg)` of a pair. Pair 0 is the fixed
    /// default white-on-black `(7, 0)` (without `use_default_colors`), and an
    /// uninitialized pair reads as `(0, 0)`, matching ncurses.
    pub fn pair_content(&self, pair: i16) -> (i16, i16) {
        if pair == 0 {
            return (7, 0); // COLOR_WHITE on COLOR_BLACK
        }
        self.pairs.get(&pair).copied().unwrap_or((0, 0))
    }

    /// `color_content(color)` -- the rgb of an ANSI colour `0..8` on a terminal
    /// that cannot redefine colours (the ncurses default 8-colour palette, scaled
    /// to 0..1000). Out-of-range colours read as `(0, 0, 0)`.
    pub fn color_content(&self, color: i16) -> (i16, i16, i16) {
        // 0 off / 680 on per channel: black, red, green, yellow, blue, magenta,
        // cyan, white -- ncurses' built-in palette when can_change_color is false.
        const D: [(i16, i16, i16); 8] = [
            (0, 0, 0),
            (680, 0, 0),
            (0, 680, 0),
            (680, 680, 0),
            (0, 0, 680),
            (680, 0, 680),
            (0, 680, 680),
            (680, 680, 680),
        ];
        if (0..8).contains(&color) {
            D[color as usize]
        } else {
            (0, 0, 0)
        }
    }

    /// `find_pair(fg, bg)` -- the number of an already-initialized pair with this
    /// `(fg, bg)`, or `-1` if none.
    pub fn find_pair(&self, fg: i16, bg: i16) -> i32 {
        self.pairs
            .iter()
            .find(|(_, &v)| v == (fg, bg))
            .map(|(&p, _)| p as i32)
            .unwrap_or(-1)
    }

    /// `alloc_pair(fg, bg)` -- the existing pair for `(fg, bg)`, or a newly
    /// allocated one (the lowest free number `>= 1`).
    pub fn alloc_pair(&mut self, fg: i16, bg: i16) -> i32 {
        let existing = self.find_pair(fg, bg);
        if existing >= 0 {
            return existing;
        }
        let mut p: i16 = 1;
        while self.pairs.contains_key(&p) {
            p += 1;
        }
        self.pairs.insert(p, (fg, bg));
        p as i32
    }

    /// `free_pair(pair)` -- release an allocated pair; false if it was not set.
    pub fn free_pair(&mut self, pair: i16) -> bool {
        self.pairs.remove(&pair).is_some()
    }
}

/// The foreground SGR for an ANSI color `c` in `0..=7`: `\e[<30+c>m`. Inputs above 7 are masked to
/// three bits (defensive; valid ANSI colors are 0..=7).
pub fn sgr_fg(c: u8) -> Vec<u8> {
    sgr(30 + (c & 0b111) as i32)
}

/// The background SGR for an ANSI color `c` in `0..=7`: `\e[<40+c>m`. Inputs above 7 are masked to
/// three bits.
pub fn sgr_bg(c: u8) -> Vec<u8> {
    sgr(40 + (c & 0b111) as i32)
}

/// `toggled_colors` (lib_color.c): SVr4 interchanges color codes 1<->4 and 3<->6 for the non-ANSI
/// `setf`/`setb` caps (`set_foreground`/`set_background`), to keep the pre-ANSI BGR ordering. ncurses
/// applies this only to those caps, not to `setaf`/`setab`.
pub fn toggled_colors(c: i32) -> i32 {
    const TABLE: [i32; 16] = [0, 4, 2, 6, 1, 5, 3, 7, 8, 12, 10, 14, 9, 13, 11, 15];
    if (0..16).contains(&c) {
        TABLE[c as usize]
    } else {
        c
    }
}

/// Append a single-parameter SGR `\e[<n>m`.
fn sgr(n: i32) -> Vec<u8> {
    let mut v = b"\x1b[".to_vec();
    v.extend_from_slice(n.to_string().as_bytes());
    v.push(0x6d); // 'm'
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_pair_is_white_on_black() {
        assert_eq!(DEFAULT_PAIR, (7, 0));
    }

    #[test]
    fn foreground_sgr() {
        assert_eq!(sgr_fg(0), b"\x1b[30m"); // black
        assert_eq!(sgr_fg(1), b"\x1b[31m"); // red
        assert_eq!(sgr_fg(2), b"\x1b[32m"); // green
        assert_eq!(sgr_fg(4), b"\x1b[34m"); // blue
        assert_eq!(sgr_fg(7), b"\x1b[37m"); // white
    }

    #[test]
    fn background_sgr() {
        assert_eq!(sgr_bg(0), b"\x1b[40m"); // black
        assert_eq!(sgr_bg(1), b"\x1b[41m"); // red
        assert_eq!(sgr_bg(5), b"\x1b[45m"); // magenta
        assert_eq!(sgr_bg(7), b"\x1b[47m"); // white
    }

    #[test]
    fn palette_pairs() {
        let mut p = Palette::new();
        assert!(p.init_pair(1, 1, 4));
        assert!(p.init_pair(2, 2, 0));
        assert_eq!(p.pair_content(1), (1, 4));
        assert_eq!(p.pair_content(2), (2, 0));
        assert_eq!(p.pair_content(9), (0, 0)); // uninitialized
        assert_eq!(p.pair_content(0), (7, 0)); // default pair white-on-black
        assert!(!p.init_pair(0, 1, 1)); // pair 0 is not settable
    }

    #[test]
    fn color_content_and_alloc() {
        let mut p = Palette::new();
        assert_eq!(p.color_content(0), (0, 0, 0));
        assert_eq!(p.color_content(1), (680, 0, 0));
        assert_eq!(p.color_content(7), (680, 680, 680));
        p.init_pair(1, 1, 4);
        p.init_pair(2, 2, 0);
        assert_eq!(p.find_pair(1, 4), 1);
        assert_eq!(p.find_pair(7, 7), -1);
        assert_eq!(p.alloc_pair(3, 5), 3); // next free
        assert_eq!(p.alloc_pair(3, 5), 3); // reuse
        assert_eq!(p.alloc_pair(6, 6), 4);
        assert!(p.free_pair(3) && !p.free_pair(9));
    }

    #[test]
    fn out_of_range_is_masked() {
        // 8 masks to 0; 15 masks to 7. Defensive only; callers should pass 0..=7.
        assert_eq!(sgr_fg(8), b"\x1b[30m");
        assert_eq!(sgr_bg(15), b"\x1b[47m");
    }
}
