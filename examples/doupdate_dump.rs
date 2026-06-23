//! Oracle court fixture for `doupdate`. Reads a scenario file describing two phases of screen
//! drawing (mirroring the C oracle's `mvaddstr`/`move`+`clrtoeol` calls), runs the crate's
//! [`ncurses_native::update::Screen`] through both phases, and prints two hex lines:
//!   line 1 = the bytes of the first `doupdate` (phase 0, the from-blank paint),
//!   line 2 = the bytes of the second `doupdate` (phase 1, the diff).
//!
//! Scenario line format: `<phase> <kind> <row> <col> [text]`
//!   phase: 0 or 1;  kind: `s` = addstr(text), `e` = move(row,col)+clrtoeol.
//! Usage: `cargo run --quiet --example doupdate_dump -- <scenario-file>`

use std::io::Write;

use ncurses_native::color::Palette;
use ncurses_native::update::Screen;
use ncurses_native::window::{attrs, char_width, is_zero_width, Cell, WIDE_PAD};

const ROWS: i32 = 24;
const COLS: i32 = 80;
const BLANK: Cell = Cell::plain(' ', 0);

/// Map a court attribute token to crate attribute bits (pairs match the court's init_pair calls).
fn attr_of(tok: &str) -> u32 {
    match tok {
        "p" => 0,
        "b" => attrs::BOLD,
        "u" => attrs::UNDERLINE,
        "r" => attrs::REVERSE,
        "a" => attrs::ALTCHARSET,
        "c1" => 1 << 8,
        "c2" => 2 << 8,
        _ => panic!("unknown attr token {tok}"),
    }
}

struct Vscreen {
    cells: Vec<Cell>,
    cury: i32,
    curx: i32,
    /// The background attribute (`wbkgd`): blanks (and the color of written text) carry it.
    bg: u32,
}

impl Vscreen {
    fn new() -> Vscreen {
        Vscreen {
            cells: vec![BLANK; (ROWS * COLS) as usize],
            cury: 0,
            curx: 0,
            bg: 0,
        }
    }
    /// `bkgd(attr)` -- set the background: every current default blank becomes a `bg`-colored blank,
    /// and subsequent writes/clears carry `bg`.
    fn set_bg(&mut self, attr: u32) {
        self.bg = attr;
        let colored = Cell::plain(' ', attr);
        for c in self.cells.iter_mut() {
            if *c == BLANK {
                *c = colored;
            }
        }
    }
    fn addstr(&mut self, row: i32, col: i32, attr: u32, text: &str) {
        // One cell per character; a double-width glyph fills its cell plus a padding cell and
        // advances two columns. At the right margin the cursor auto-wraps to column 0 of the next
        // line (auto_right_margin), matching ncursesw's per-cell wide-character model. Written cells
        // carry the background attribute too (the bkgd color shows through plain text).
        let attr = attr | self.bg;
        let mut y = row;
        let mut c = col;
        for ch in text.chars() {
            if is_zero_width(ch) {
                // Combining mark: attach to the preceding base cell (skip its WIDE_PAD), no advance.
                if c > 0 {
                    let mut bx = c - 1;
                    if self.cells[(y * COLS + bx) as usize].ch == WIDE_PAD && bx > 0 {
                        bx -= 1;
                    }
                    self.cells[(y * COLS + bx) as usize].push_comb(ch);
                }
                continue;
            }
            let w = char_width(ch);
            if w == 2 && c + 2 > COLS {
                // A wide glyph that will not fit in the last column wraps whole to the next line.
                y += 1;
                c = 0;
            }
            if y >= ROWS {
                break;
            }
            self.cells[(y * COLS + c) as usize] = Cell::plain(ch, attr);
            if w == 2 {
                self.cells[(y * COLS + c + 1) as usize] = Cell::plain(WIDE_PAD, attr);
            }
            c += w;
            if c >= COLS {
                y += 1;
                c = 0;
            }
        }
        self.cury = y;
        self.curx = c;
    }
    fn erase_to_eol(&mut self, row: i32, col: i32) {
        let blank = Cell::plain(' ', self.bg);
        for c in col..COLS {
            self.cells[(row * COLS + c) as usize] = blank;
        }
        self.cury = row;
        self.curx = col;
    }
}

fn hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        s.push_str(&format!("{x:02x}"));
    }
    s
}

fn main() {
    let path = std::env::args().nth(1).expect("scenario file");
    let text = std::fs::read_to_string(&path).expect("read scenario");

    let mut phase = [Vscreen::new(), Vscreen::new()];
    // Phase 1 starts from the phase-0 screen state (curses windows accumulate).
    // Op = (kind, row, col, attr_bits, text). 's' = addstr, 'e' = move+clrtoeol.
    let mut p0_ops: Vec<(String, i32, i32, u32, String)> = Vec::new();
    let mut p1_ops: Vec<(String, i32, i32, u32, String)> = Vec::new();
    // A `nocolor` directive line means the scenario's oracle program did not call start_color (a
    // monochrome program, e.g. the ACS court), so coloron stays off and attribute changes emit no
    // default-pair color reset. The default (no directive) is coloron on, matching the doupdate
    // court's start_color preamble.
    let mut coloron = true;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "nocolor" {
            coloron = false;
            continue;
        }
        let mut it = line.split(' ');
        let ph: usize = it.next().unwrap().parse().unwrap();
        let kind = it.next().unwrap().to_string();
        let row: i32 = it.next().unwrap().parse().unwrap();
        let col: i32 = it.next().unwrap().parse().unwrap();
        let (attr, txt) = if kind == "s" {
            let tok = it.next().unwrap();
            let rest: Vec<&str> = it.collect();
            (attr_of(tok), rest.join(" "))
        } else if kind == "g" {
            (attr_of(it.next().unwrap()), String::new())
        } else {
            (0, String::new())
        };
        if ph == 0 {
            p0_ops.push((kind, row, col, attr, txt));
        } else {
            p1_ops.push((kind, row, col, attr, txt));
        }
    }

    // `k` (clearok) / `lo` (leaveok) do not change cells; they set doupdate flags.
    let p1_clearok = p1_ops.iter().any(|(k, ..)| k == "k");
    let p1_leaveok = p1_ops.iter().any(|(k, ..)| k == "lo");
    let apply = |vs: &mut Vscreen, ops: &[(String, i32, i32, u32, String)]| {
        for (kind, row, col, attr, txt) in ops {
            match kind.as_str() {
                "s" => vs.addstr(*row, *col, *attr, txt),
                "e" => vs.erase_to_eol(*row, *col),
                "g" => vs.set_bg(*attr),
                "m" => {
                    vs.cury = *row;
                    vs.curx = *col;
                }
                "k" | "lo" => {}
                _ => panic!("unknown kind {kind}"),
            }
        }
    };

    apply(&mut phase[0], &p0_ops);
    // Phase 1 builds on phase 0's cells (and background).
    phase[1].cells = phase[0].cells.clone();
    phase[1].cury = phase[0].cury;
    phase[1].curx = phase[0].curx;
    phase[1].bg = phase[0].bg;
    apply(&mut phase[1], &p1_ops);

    // Palette matches the court's init_pair calls: pair 1 = red/black, pair 2 = green/blue.
    let mut palette = Palette::new();
    palette.init_pair(1, 1, 0);
    palette.init_pair(2, 2, 4);
    let mut s = Screen::with_palette(ROWS, COLS, palette);
    // Optional second arg selects the terminal's clear_screen / rep caps (default xterm).
    let term = std::env::args().nth(2);
    if term.as_deref() == Some("vt100") {
        // vt100 has `xon`, so ncurses emits no padding NUL bytes: clear_screen = \e[H\e[J (padding
        // stripped), no `rep`. GoTo uses the no-hpa/vpa vt100 cost model.
        s.set_term_caps(b"\x1b[H\x1b[J", false);
        s.set_caps(ncurses_native::Caps::vt100());
        // vt100 el/el1 carry $<3> padding (cost 18/19, so spaces beat clr_eol far more often) and it
        // has no ech / parm_ich / parm_dch -- runs and shifts are painted as literal cells.
        s.set_shape_caps(18, 19, false, false);
    }
    if term.as_deref() == Some("linux") {
        s.set_term_caps(b"\x1b[H\x1b[J", false);
        // Linux console attribute caps: sgr (^N/^O for altcharset), sgr0 = \e[m\017, rmacs = ^O.
        s.set_attr_caps(
            b"\x1b[0;10%?%p1%t;7%;%?%p2%t;4%;%?%p3%t;7%;%?%p4%t;5%;%?%p5%t;2%;%?%p6%t;1%;m%?%p9%t\x0e%e\x0f%;",
            b"\x1b[m\x0f",
            b"\x0f",
        );
        // Linux console ncv=18: underline + dim are suppressed while color is on.
        s.set_ncv(attrs::UNDERLINE | attrs::DIM);
    }
    if term.as_deref() == Some("screen") {
        // screen: clear `\e[H\e[J`, no rep; altcharset via ^N/^O embedded in sgr; identity acsc for
        // the line-drawing chars (so the glyph bytes are emitted unchanged).
        s.set_term_caps(b"\x1b[H\x1b[J", false);
        s.set_attr_caps(
            b"\x1b[0%?%p6%t;1%;%?%p1%t;3%;%?%p2%t;4%;%?%p3%t;7%;%?%p4%t;5%;%?%p5%t;2%;m%?%p9%t\x0e%e\x0f%;",
            b"\x1b[m\x0f",
            b"\x0f",
        );
        // No ech, has idc.
        s.set_shape_caps(3, 4, false, true);
    }
    if term.as_deref() == Some("cygwin") {
        // cygwin: clear `\e[H\e[J`, no rep, no ech; altcharset via the sgr `;11` param + rmacs
        // `\e[10m`; NON-identity acsc (CP437): q->0xC4, x->0xB3, l->0xDA, ... -- the glyph byte is
        // remapped at emit time. Cursor cost model is xterm-like (has hpa/vpa, no padding).
        s.set_term_caps(b"\x1b[H\x1b[J", false);
        s.set_caps(ncurses_native::Caps::cygwin());
        s.set_attr_caps(
            b"\x1b[0;10%?%p1%t;7%;%?%p2%t;4%;%?%p3%t;7%;%?%p6%t;1%;%?%p7%t;8%;%?%p9%t;11%;m",
            b"\x1b[0;10m",
            b"\x1b[10m",
        );
        s.set_shape_caps(3, 4, false, true);
        s.set_acsc(b"+\x10,\x11-\x18.\x190\xdb`\x04a\xb1f\xf8g\xf1h\xb0j\xd9k\xbfl\xdam\xc0n\xc5o~p\xc4q\xc4r\xc4s_t\xc3u\xb4v\xc1w\xc2x\xb3y\xf3z\xf2{\xe3|\xd8}\x9c~\xfe");
    }
    // coloron on (the doupdate court's start_color preamble) unless the scenario declared `nocolor`.
    // With coloron on, every attribute change carries the default-pair color reset (`_nc_do_color`).
    if coloron {
        s.start_color();
    }
    let first = s.doupdate(&phase[0].cells, phase[0].cury, phase[0].curx);
    if p1_clearok {
        s.clearok();
    }
    if p1_leaveok {
        s.set_leaveok(true);
    }
    let diff = s.doupdate(&phase[1].cells, phase[1].cury, phase[1].curx);

    let mut out = std::io::stdout();
    writeln!(out, "{}", hex(&first)).unwrap();
    writeln!(out, "{}", hex(&diff)).unwrap();
}
