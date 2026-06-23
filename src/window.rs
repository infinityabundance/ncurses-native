//! A minimal `WINDOW` cell model -- the behaviour-parity layer above the byte
//! substrate. ncurses-native is byte-output first, but the window operations
//! (`addch`/`addstr`/`move`/`erase`/`inch`/insert-delete/border/attributes) are
//! reconstructed here as a cell grid whose *externally observable state* is
//! compared to ncurses by reading the cells back (`winch`). This is "behaviour
//! parity": the grid a program can observe via `winch`/`winstr`, not the terminal
//! byte stream (that is the refresh/`doupdate` problem, still a seed).
//!
//! Pinned by courts NCURSES.WINDOW.STATE (character grid), NCURSES.WINDOW.ATTR
//! (character + attribute), and NCURSES.WINDOW.GEOMETRY (position/size/parent).
//!
//! Scope / non-claims: printable characters and `\n` (clear-to-EOL then next line, no
//! scroll past the bottom) are modelled, plus insert/delete, ACS borders/lines,
//! and the monochrome display attributes. Single-width UTF-8 glyphs are stored one
//! per cell, and stable double-width (East-Asian wide / fullwidth) glyphs fill a
//! cell plus a padding cell (advancing two columns). Zero-width combining marks
//! attach to the preceding base cell (up to [`CMAX`], `ncursesw`'s `cchar_t.chars[1..]`).
//! Tab expansion, control-character rendering (`^X`), scrolling regions, and
//! version-skewed wide glyphs (LOC-04) and shared-cell child windows are out of scope.

/// Monochrome display-attribute bit values, matching ncurses' public `A_*`
/// macros so a reconstructed cell's attribute compares equal under `winch`.
pub mod attrs {
    /// No attributes.
    pub const NORMAL: u32 = 0x0000_0000;
    /// `A_STANDOUT`.
    pub const STANDOUT: u32 = 0x0001_0000;
    /// `A_UNDERLINE`.
    pub const UNDERLINE: u32 = 0x0002_0000;
    /// `A_REVERSE`.
    pub const REVERSE: u32 = 0x0004_0000;
    /// `A_BLINK`.
    pub const BLINK: u32 = 0x0008_0000;
    /// `A_DIM`.
    pub const DIM: u32 = 0x0010_0000;
    /// `A_BOLD`.
    pub const BOLD: u32 = 0x0020_0000;
    /// `A_INVIS`.
    pub const INVIS: u32 = 0x0080_0000;
    /// `A_PROTECT`.
    pub const PROTECT: u32 = 0x0100_0000;
    /// `A_ALTCHARSET` -- the line-drawing (alternate character set) bit.
    pub const ALTCHARSET: u32 = 0x0040_0000;
    /// `A_COLOR` -- the colour-pair bits (8..16).
    pub const COLOR: u32 = 0x0000_ff00;
}

/// `COLOR_PAIR(n)` -- the attribute bits that select colour pair `n`.
pub fn color_pair(n: i32) -> u32 {
    ((n as u32) & 0xff) << 8
}

/// `PAIR_NUMBER(attr)` -- the colour pair encoded in an attribute word.
pub fn pair_number(attr: u32) -> i32 {
    ((attr & attrs::COLOR) >> 8) as i32
}

// Canonical ACS line-drawing characters as stored in `A_CHARTEXT` (the glyph is
// applied at output time via `acsc`, not held in the cell).
const ACS_VLINE: char = 'x';
const ACS_HLINE: char = 'q';
const ACS_ULCORNER: char = 'l';
const ACS_URCORNER: char = 'k';
const ACS_LLCORNER: char = 'm';
const ACS_LRCORNER: char = 'j';

/// One screen cell: a character and its attribute bits.
///
/// The character is a Rust `char` so a single cell can hold a single-width
/// (BMP / non-combining) UTF-8 glyph, matching how `ncursesw` stores one wide
/// character per cell under a UTF-8 locale. Narrow read-back (`winch`) truncates
/// to the low byte (the ASCII/Latin-1 value). A cell may also carry up to
/// [`CMAX`] **combining marks** (`ncursesw`'s `cchar_t.chars[1..]`): zero-width
/// characters that attach to the base glyph and are emitted right after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub attr: u32,
    /// Combining marks attached to this cell (`'\0'`-terminated), e.g. `e` + U+0301.
    pub comb: [char; CMAX],
}

/// Maximum combining marks per cell (`ncursesw`'s `CCHARW_MAX - 1` = 4).
pub const CMAX: usize = 4;
/// The empty combining-mark array (no marks).
pub const NO_COMB: [char; CMAX] = ['\0'; CMAX];

impl Cell {
    const BLANK: Cell = Cell::plain(' ', 0);

    /// A cell with a base glyph and attribute and no combining marks (the common case).
    pub const fn plain(ch: char, attr: u32) -> Cell {
        Cell {
            ch,
            attr,
            comb: NO_COMB,
        }
    }

    /// Append a combining mark to the next free slot; returns `false` if all [`CMAX`] slots are full
    /// (ncurses silently drops excess marks, matching `wadd_wch` past `CCHARW_MAX`).
    pub fn push_comb(&mut self, m: char) -> bool {
        for slot in self.comb.iter_mut() {
            if *slot == '\0' {
                *slot = m;
                return true;
            }
        }
        false
    }
}

/// The padding (continuation) cell that follows a double-width glyph: it holds the *second* column
/// of a wide character. It is never emitted on its own (the glyph emits both columns) and never
/// occurs in ASCII text, so the narrow read-back of `'\0'` is out of the courted scope.
pub const WIDE_PAD: char = '\0';

/// Whether `c` is a zero-width combining mark (`wcwidth(c) == 0`): it attaches to the preceding base
/// cell rather than occupying its own column. Covers the combining-diacritical blocks ncursesw
/// (via `wcwidth`) treats as width-0; the `NCURSES.WIDECHAR` court grounds the `combining_*` cases.
pub fn is_zero_width(c: char) -> bool {
    let u = c as u32;
    matches!(u,
        0x0300..=0x036F   // Combining Diacritical Marks
        | 0x0483..=0x0489 // Cyrillic combining
        | 0x0591..=0x05BD | 0x05BF | 0x05C1..=0x05C2 | 0x05C4..=0x05C5 | 0x05C7 // Hebrew
        | 0x0610..=0x061A | 0x064B..=0x065F | 0x0670 | 0x06D6..=0x06DC // Arabic
        | 0x0E31 | 0x0E34..=0x0E3A | 0x0E47..=0x0E4E // Thai
        | 0x1AB0..=0x1AFF // Combining Diacritical Marks Extended
        | 0x1DC0..=0x1DFF // Combining Diacritical Marks Supplement
        | 0x200B..=0x200F // zero-width space / joiners / marks
        | 0x20D0..=0x20FF // Combining Diacritical Marks for Symbols
        | 0xFE20..=0xFE2F // Combining Half Marks
    )
}

/// The display width of a character in terminal columns: `2` for the stable East-Asian wide /
/// fullwidth ranges (universally width-2 across `wcwidth` Unicode versions), `1` otherwise.
///
/// Honest bound: this covers the rock-solid BMP wide ranges (CJK ideographs, Kana, Hangul,
/// fullwidth forms) that every `wcwidth` agrees on; emoji and other version-skewed widths are
/// deliberately treated as width-1 (LOC-04). Zero-width combining marks are handled separately
/// (see [`is_zero_width`]). The `NCURSES.WIDECHAR` court grounds the glyphs against `libncursesw`.
pub fn char_width(c: char) -> i32 {
    let u = c as u32;
    let wide = matches!(u,
        0x1100..=0x115F   // Hangul Jamo
        | 0x2E80..=0x303E // CJK Radicals, Kangxi, CJK Symbols
        | 0x3041..=0x33FF // Kana, CJK punctuation, enclosed/compat
        | 0x3400..=0x4DBF // CJK Ext A
        | 0x4E00..=0x9FFF // CJK Unified Ideographs
        | 0xA000..=0xA4CF // Yi
        | 0xAC00..=0xD7A3 // Hangul Syllables
        | 0xF900..=0xFAFF // CJK Compatibility Ideographs
        | 0xFE30..=0xFE4F // CJK Compatibility Forms
        | 0xFF00..=0xFF60 // Fullwidth Forms
        | 0xFFE0..=0xFFE6 // Fullwidth signs
    );
    if wide {
        2
    } else {
        1
    }
}

/// Decode `s` as UTF-8 into a sequence of characters; any byte that is not part
/// of a valid UTF-8 sequence is taken as an individual Latin-1 character (so its
/// narrow low-byte read-back is unchanged). `ncursesw` under a UTF-8 locale
/// stores one wide character per single-width glyph; this mirrors that for the
/// cell model while leaving non-UTF-8 input byte-faithful.
fn decode_chars(s: &[u8]) -> Vec<char> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let b = s[i];
        let len = if b < 0x80 {
            1
        } else if b >> 5 == 0b110 {
            2
        } else if b >> 4 == 0b1110 {
            3
        } else if b >> 3 == 0b1_1110 {
            4
        } else {
            0
        };
        if len >= 1 && i + len <= s.len() {
            if let Ok(st) = std::str::from_utf8(&s[i..i + len]) {
                if let Some(ch) = st.chars().next() {
                    out.push(ch);
                    i += len;
                    continue;
                }
            }
        }
        out.push(b as char);
        i += 1;
    }
    out
}

/// A character cell grid with a cursor, reproducing ncurses `WINDOW` text state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    rows: i32,
    cols: i32,
    cells: Vec<Cell>,
    cury: i32,
    curx: i32,
    cur_attr: u32,
    begy: i32,
    begx: i32,
    pary: i32,
    parx: i32,
    is_pad: bool,
    wrap_pending: bool,
    /// `scrollok` flag -- whether `scroll`/`wscrl` (and bottom-line auto-scroll) are permitted.
    scroll_ok: bool,
    /// Software scroll region `[top, bot]` (`wsetscrreg`); scrolling shifts only these rows.
    scrreg_top: i32,
    scrreg_bot: i32,
    /// The window background: the blank char + attribute (`wbkgd`/`wbkgdset`). Written cells OR in
    /// its attribute; cleared cells take it.
    bkgd: Cell,
}

impl Window {
    /// Create a `rows` x `cols` window filled with blanks, cursor at `(0, 0)`,
    /// top-left at screen `(0, 0)`, with no parent and no attributes.
    pub fn new(rows: i32, cols: i32) -> Window {
        let rows = rows.max(1);
        let cols = cols.max(1);
        Window {
            rows,
            cols,
            cells: vec![Cell::BLANK; (rows * cols) as usize],
            cury: 0,
            curx: 0,
            cur_attr: 0,
            begy: 0,
            begx: 0,
            pary: -1,
            parx: -1,
            is_pad: false,
            wrap_pending: false,
            scroll_ok: false,
            scrreg_top: 0,
            scrreg_bot: rows - 1,
            bkgd: Cell::BLANK,
        }
    }

    /// `wbkgd` -- set the window background to `(ch, attr)`. Every cell that currently holds the old
    /// background is replaced with the new one; every other cell keeps its glyph but swaps the old
    /// background attribute for the new (matching ncurses' bkgd rule). Subsequent writes OR the new
    /// background attribute in, and clears fill with it.
    pub fn bkgdset(&mut self, ch: u8, attr: u32) {
        let new = Cell::plain(ch as char, attr);
        let old = self.bkgd;
        for c in &mut self.cells {
            if *c == old {
                *c = new;
            } else {
                c.attr = (c.attr & !old.attr) | new.attr;
            }
        }
        self.bkgd = new;
    }

    /// The current background cell.
    pub fn bkgd(&self) -> Cell {
        self.bkgd
    }

    /// `newpad` -- a `rows` x `cols` pad (a window with no screen association).
    pub fn newpad(rows: i32, cols: i32) -> Window {
        let mut w = Window::new(rows, cols);
        w.is_pad = true;
        w
    }

    /// `subpad` -- a sub-pad of this pad at parent-relative `(py, px)`.
    pub fn subpad(&self, rows: i32, cols: i32, py: i32, px: i32) -> Window {
        let mut w = self.derwin(rows, cols, py, px);
        w.is_pad = true;
        w
    }

    /// `is_pad` -- whether this window is a pad.
    pub fn is_pad(&self) -> bool {
        self.is_pad
    }

    /// `newwin` -- a blank window of `rows` x `cols` at screen `(begy, begx)`.
    pub fn newwin(rows: i32, cols: i32, begy: i32, begx: i32) -> Window {
        let mut w = Window::new(rows, cols);
        w.begy = begy;
        w.begx = begx;
        w
    }

    fn idx(&self, y: i32, x: i32) -> usize {
        (y * self.cols + x) as usize
    }

    // --- geometry --------------------------------------------------------

    /// `getyx` -- the logical cursor position `(row, col)`.
    pub fn getyx(&self) -> (i32, i32) {
        (self.cury, self.curx)
    }
    /// The cell grid, row-major (`rows * cols`) -- the desired-screen buffer the update layer diffs.
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }
    /// `getmaxyx` -- the window size `(rows, cols)`.
    pub fn getmaxyx(&self) -> (i32, i32) {
        (self.rows, self.cols)
    }
    /// `getbegyx` -- the window's top-left screen position.
    pub fn getbegyx(&self) -> (i32, i32) {
        (self.begy, self.begx)
    }
    /// `getparyx` -- position relative to the parent, or `(-1, -1)` if top-level.
    pub fn getparyx(&self) -> (i32, i32) {
        (self.pary, self.parx)
    }
    /// `is_subwin` -- whether this window was derived from a parent.
    pub fn is_subwin(&self) -> bool {
        self.pary >= 0
    }
    /// `dupwin` -- an exact copy (cells, cursor, attributes, geometry).
    pub fn dupwin(&self) -> Window {
        self.clone()
    }

    /// `wenclose` -- whether screen position `(y, x)` lies within the window.
    pub fn wenclose(&self, y: i32, x: i32) -> bool {
        y >= self.begy && y < self.begy + self.rows && x >= self.begx && x < self.begx + self.cols
    }

    /// `wmouse_trafo` -- transform between window and screen coordinates. With
    /// `to_screen`, map window `(y, x)` to screen; otherwise map screen to window.
    /// Returns the transformed `(y, x)` if it lies in the window, else `None`
    /// (ncurses leaves the caller's coordinates unchanged and returns false).
    pub fn mouse_trafo(&self, y: i32, x: i32, to_screen: bool) -> Option<(i32, i32)> {
        if to_screen {
            let (ny, nx) = (y + self.begy, x + self.begx);
            self.wenclose(ny, nx).then_some((ny, nx))
        } else {
            let (ny, nx) = (y - self.begy, x - self.begx);
            (ny >= 0 && ny < self.rows && nx >= 0 && nx < self.cols).then_some((ny, nx))
        }
    }
    /// `mvwin` -- move the window's top-left corner to screen `(begy, begx)`.
    pub fn mvwin(&mut self, begy: i32, begx: i32) {
        self.begy = begy;
        self.begx = begx;
    }
    /// `wresize` -- change the size, preserving overlapping top-left content.
    pub fn wresize(&mut self, rows: i32, cols: i32) {
        let rows = rows.max(1);
        let cols = cols.max(1);
        let mut next = vec![Cell::BLANK; (rows * cols) as usize];
        for y in 0..rows.min(self.rows) {
            for x in 0..cols.min(self.cols) {
                next[(y * cols + x) as usize] = self.cells[self.idx(y, x)];
            }
        }
        self.cells = next;
        self.rows = rows;
        self.cols = cols;
        self.cury = self.cury.min(rows - 1);
        self.curx = self.curx.min(cols - 1);
        self.wrap_pending = false;
    }
    /// Copy this window's cells onto `dst` at this window's screen position `(begy, begx)`,
    /// clipped to `dst`. This is the `wnoutrefresh` blit of a window onto the virtual screen
    /// (`newscr`); it copies every cell (glyph + attribute/color), blanks included.
    pub fn overwrite_into(&self, dst: &mut Window) {
        for y in 0..self.rows {
            let dy = self.begy + y;
            if dy < 0 || dy >= dst.rows {
                continue;
            }
            for x in 0..self.cols {
                let dx = self.begx + x;
                if dx < 0 || dx >= dst.cols {
                    continue;
                }
                let cell = self.cells[self.idx(y, x)];
                let di = dst.idx(dy, dx);
                dst.cells[di] = cell;
            }
        }
    }

    /// `overwrite(src=self, dst)` -- copy the cells of the **screen-overlap** of the two windows
    /// from `self` into `dst` (all cells). `overlay` skips `self`'s background (blank) cells. Unlike
    /// [`overwrite_into`](Self::overwrite_into) (the origin blit for `wnoutrefresh`), this accounts
    /// for `dst`'s screen position.
    fn copy_overlap(&self, dst: &mut Window, overlay: bool) {
        for sy in 0..self.rows {
            let dy = self.begy + sy - dst.begy;
            if dy < 0 || dy >= dst.rows {
                continue;
            }
            for sx in 0..self.cols {
                let dx = self.begx + sx - dst.begx;
                if dx < 0 || dx >= dst.cols {
                    continue;
                }
                let cell = self.cells[self.idx(sy, sx)];
                if overlay && cell == self.bkgd {
                    continue;
                }
                let di = dst.idx(dy, dx);
                dst.cells[di] = cell;
            }
        }
    }

    /// `overwrite(src, dst)` -- copy every overlapping cell.
    pub fn overwrite(&self, dst: &mut Window) {
        self.copy_overlap(dst, false);
    }

    /// `overlay(src, dst)` -- copy only the overlapping non-blank cells.
    pub fn overlay(&self, dst: &mut Window) {
        self.copy_overlap(dst, true);
    }

    /// `copywin(src, dst, sminrow, smincol, dminrow, dmincol, dmaxrow, dmaxcol, overlay)` -- copy a
    /// rectangle of `self` into `dst`'s `[dminrow..=dmaxrow] x [dmincol..=dmaxcol]`; with `overlay`,
    /// skip `self`'s background (blank) cells.
    #[allow(clippy::too_many_arguments)]
    pub fn copywin(
        &self,
        dst: &mut Window,
        sminrow: i32,
        smincol: i32,
        dminrow: i32,
        dmincol: i32,
        dmaxrow: i32,
        dmaxcol: i32,
        overlay: bool,
    ) {
        for dy in dminrow..=dmaxrow {
            let sy = sminrow + (dy - dminrow);
            if dy < 0 || dy >= dst.rows || sy < 0 || sy >= self.rows {
                continue;
            }
            for dx in dmincol..=dmaxcol {
                let sx = smincol + (dx - dmincol);
                if dx < 0 || dx >= dst.cols || sx < 0 || sx >= self.cols {
                    continue;
                }
                let cell = self.cells[self.idx(sy, sx)];
                if overlay && cell == self.bkgd {
                    continue;
                }
                let di = dst.idx(dy, dx);
                dst.cells[di] = cell;
            }
        }
    }

    /// Copy a rectangle of this pad onto `dst`: the screen rectangle `[sminrow..=smaxrow] x
    /// [smincol..=smaxcol]` receives the pad cells starting at `(pminrow, pmincol)`. This is the
    /// `pnoutrefresh` blit of a pad onto the virtual screen (`newscr`), clipped to both.
    #[allow(clippy::too_many_arguments)]
    pub fn copy_pad_into(
        &self,
        dst: &mut Window,
        pminrow: i32,
        pmincol: i32,
        sminrow: i32,
        smincol: i32,
        smaxrow: i32,
        smaxcol: i32,
    ) {
        for sr in sminrow..=smaxrow {
            let pr = pminrow + (sr - sminrow);
            if sr < 0 || sr >= dst.rows || pr < 0 || pr >= self.rows {
                continue;
            }
            for sc in smincol..=smaxcol {
                let pc = pmincol + (sc - smincol);
                if sc < 0 || sc >= dst.cols || pc < 0 || pc >= self.cols {
                    continue;
                }
                let cell = self.cells[self.idx(pr, pc)];
                let di = dst.idx(sr, sc);
                dst.cells[di] = cell;
            }
        }
    }

    /// `derwin` -- a child window at parent-relative `(py, px)` (independent cells).
    pub fn derwin(&self, rows: i32, cols: i32, py: i32, px: i32) -> Window {
        let mut w = Window::new(rows, cols);
        w.begy = self.begy + py;
        w.begx = self.begx + px;
        w.pary = py;
        w.parx = px;
        w
    }
    /// `subwin` -- a child window at screen-absolute `(by, bx)` (independent cells).
    pub fn subwin(&self, rows: i32, cols: i32, by: i32, bx: i32) -> Window {
        let mut w = Window::new(rows, cols);
        w.begy = by;
        w.begx = bx;
        w.pary = by - self.begy;
        w.parx = bx - self.begx;
        w
    }

    /// `wmove` -- move the cursor; returns false (ERR) for out-of-range targets.
    pub fn move_to(&mut self, y: i32, x: i32) -> bool {
        if y < 0 || x < 0 || y >= self.rows || x >= self.cols {
            return false;
        }
        self.cury = y;
        self.curx = x;
        self.wrap_pending = false;
        true
    }

    // --- attributes ------------------------------------------------------

    /// `attron` -- turn the given attribute bits on for subsequent writes. A
    /// colour pair in `a` *replaces* the active colour (ncurses semantics), while
    /// the non-colour bits are OR'd in.
    pub fn attron(&mut self, a: u32) {
        if a & attrs::COLOR != 0 {
            self.cur_attr = (self.cur_attr & !attrs::COLOR) | (a & attrs::COLOR);
        }
        self.cur_attr |= a & !attrs::COLOR;
    }

    /// `color_set` / `wcolor_set` -- set the active colour pair (replacing any
    /// previous colour; other attributes are kept).
    pub fn color_set(&mut self, pair: i32) {
        self.cur_attr = (self.cur_attr & !attrs::COLOR) | color_pair(pair);
    }
    /// `attroff` -- turn the given attribute bits off.
    pub fn attroff(&mut self, a: u32) {
        self.cur_attr &= !a;
    }
    /// `attrset` -- set the active attributes exactly.
    pub fn attrset(&mut self, a: u32) {
        self.cur_attr = a;
    }
    /// `attr_get` / `getattrs` -- the active attributes.
    pub fn attr_get(&self) -> u32 {
        self.cur_attr
    }
    /// `standout` -- turn on `A_STANDOUT`.
    pub fn standout(&mut self) {
        self.attron(attrs::STANDOUT);
    }
    /// `standend` -- turn all attributes off.
    pub fn standend(&mut self) {
        self.attrset(attrs::NORMAL);
    }
    /// `chgat` -- set the attributes of `n` cells from the cursor (chars and the
    /// cursor are unchanged); `n < 0` means to the end of the row.
    pub fn chgat(&mut self, n: i32, attr: u32) {
        let end = if n < 0 {
            self.cols
        } else {
            (self.curx + n).min(self.cols)
        };
        for x in self.curx..end {
            let i = self.idx(self.cury, x);
            self.cells[i].attr = attr;
        }
    }

    // --- read-back -------------------------------------------------------

    /// `winch` -- the character at `(y, x)` (blank for out-of-range). Narrow
    /// read-back: the low byte of the cell's character.
    pub fn inch(&self, y: i32, x: i32) -> u8 {
        self.cell(y, x).ch as u8
    }
    /// The full cell (character + attribute) at `(y, x)`.
    pub fn cell(&self, y: i32, x: i32) -> Cell {
        if y < 0 || x < 0 || y >= self.rows || x >= self.cols {
            return Cell::BLANK;
        }
        self.cells[self.idx(y, x)]
    }
    /// `winstr` -- read characters from `(y, x)` to the end of that row.
    pub fn instr(&self, y: i32, x: i32) -> Vec<u8> {
        if y < 0 || x < 0 || y >= self.rows || x >= self.cols {
            return Vec::new();
        }
        (x..self.cols)
            .map(|c| self.cells[self.idx(y, c)].ch as u8)
            .collect()
    }

    // --- output ----------------------------------------------------------

    /// Write a cell, optionally marking it alternate-character-set (line drawing). ACS cells carry
    /// `A_ALTCHARSET` so the output layer emits them via `smacs`/`rmacs` (`\e(0` ... `\e(B`).
    fn set_acs(&mut self, y: i32, x: i32, ch: char, acs: bool) {
        if y >= 0 && x >= 0 && y < self.rows && x < self.cols {
            let mut attr = self.cur_attr | self.bkgd.attr;
            if acs {
                attr |= attrs::ALTCHARSET;
            }
            let i = self.idx(y, x);
            self.cells[i] = Cell::plain(ch, attr);
        }
    }

    /// `wclrtoeol` -- clear from the cursor to the end of its line (fill with the background).
    pub fn clrtoeol(&mut self) {
        let (y, bkgd) = (self.cury, self.bkgd);
        for x in self.curx..self.cols {
            let i = self.idx(y, x);
            self.cells[i] = bkgd;
        }
        self.wrap_pending = false;
    }

    /// `wclrtobot` -- clear from the cursor to the end of its line and every line below it.
    pub fn clrtobot(&mut self) {
        self.clrtoeol();
        let bkgd = self.bkgd;
        for y in (self.cury + 1)..self.rows {
            for x in 0..self.cols {
                let i = self.idx(y, x);
                self.cells[i] = bkgd;
            }
        }
    }

    /// `werase` -- fill the window with the background and home the cursor.
    pub fn erase(&mut self) {
        let bkgd = self.bkgd;
        for c in &mut self.cells {
            *c = bkgd;
        }
        self.cury = 0;
        self.curx = 0;
        self.wrap_pending = false;
    }

    /// `waddch` for a printable byte or `\n`. Printable bytes write the current
    /// attribute and advance (wrapping at the right margin, clipping at the bottom
    /// without scrolling); `\n` clears to end of line and moves to the next line.
    pub fn addch(&mut self, ch: u8) {
        self.addch_char(ch as char, 0);
    }

    /// `waddch` of a `chtype` whose rendition bits (e.g. `A_ALTCHARSET` for `ACS_*`, or an explicit
    /// attribute/color) are carried in `extra` and combined with the current attribute + background.
    /// The character is a narrow (`chtype`) low byte; for multibyte text use [`Window::addstr`].
    pub fn addch_attr(&mut self, ch: u8, extra: u32) {
        self.addch_char(ch as char, extra);
    }

    /// The character-level core of `waddch`: write a single (single-width) character, combining
    /// the current attribute + background + `extra` rendition bits, advancing the cursor (wrapping
    /// at the right margin, clipping at the bottom without scrolling). `\n` clears to end of line
    /// and moves to the next line.
    pub fn addch_char(&mut self, ch: char, extra: u32) {
        if ch == '\n' {
            self.wrap_pending = false;
            let y = self.cury;
            let bkgd = self.bkgd;
            for x in self.curx..self.cols {
                let i = self.idx(y, x);
                self.cells[i] = bkgd;
            }
            self.curx = 0;
            if self.cury + 1 < self.rows {
                self.cury += 1;
            }
            return;
        }
        if is_zero_width(ch) {
            // Combining mark: attach to the preceding base cell; no column advance, no wrap, and the
            // pending auto-margin wrap is not consumed (ncursesw accumulates into cchar_t.chars[1..]).
            let (by, bx) = if self.wrap_pending {
                (self.cury, self.curx) // the glyph that filled the last column
            } else if self.curx > 0 {
                let mut bx = self.curx - 1;
                if self.cells[self.idx(self.cury, bx)].ch == WIDE_PAD && bx > 0 {
                    bx -= 1; // attach to the wide base, not its padding cell
                }
                (self.cury, bx)
            } else {
                return; // no preceding base on this line: dropped
            };
            let i = self.idx(by, bx);
            self.cells[i].push_comb(ch);
            return;
        }
        if self.wrap_pending {
            if self.cury + 1 >= self.rows {
                return; // bottom row, no scroll: dropped
            }
            self.cury += 1;
            self.curx = 0;
            self.wrap_pending = false;
        }
        let attr = self.cur_attr | self.bkgd.attr | extra;
        let width = char_width(ch);
        if width == 2 {
            // A double-width glyph needs two columns. If only one column remains, ncursesw blanks
            // the last cell and wraps the glyph to column 0 of the next line (the right-margin wrap
            // corner -- emission of it is the deferred OPT-02 gap; the cell state is kept faithful).
            if self.curx + 2 > self.cols {
                let i = self.idx(self.cury, self.curx);
                self.cells[i] = self.bkgd;
                if self.cury + 1 >= self.rows {
                    return; // bottom row, no scroll: dropped
                }
                self.cury += 1;
                self.curx = 0;
            }
            let i = self.idx(self.cury, self.curx);
            self.cells[i] = Cell::plain(ch, attr);
            let p = self.idx(self.cury, self.curx + 1);
            self.cells[p] = Cell::plain(WIDE_PAD, attr);
            if self.curx + 2 >= self.cols {
                // Auto-margin: filling the last column wraps the cursor to column 0 of the next
                // line (verified against ncurses getyx); the bottom line cannot scroll, so the
                // cursor sticks at the margin there.
                if self.cury + 1 < self.rows {
                    self.cury += 1;
                    self.curx = 0;
                } else {
                    self.curx = self.cols - 1;
                    self.wrap_pending = true;
                }
            } else {
                self.curx += 2;
            }
            return;
        }
        let i = self.idx(self.cury, self.curx);
        self.cells[i] = Cell::plain(ch, attr);
        if self.curx + 1 >= self.cols {
            // Auto-margin: filling the last column wraps the cursor to column 0 of the next line
            // (verified against ncurses getyx); the bottom line cannot scroll, so the cursor
            // sticks at the margin there until the next write is dropped.
            if self.cury + 1 < self.rows {
                self.cury += 1;
                self.curx = 0;
            } else {
                self.wrap_pending = true;
            }
        } else {
            self.curx += 1;
        }
    }

    /// `waddstr` -- add each character of the UTF-8 byte string `s` via [`Window::addch_char`].
    /// Bytes that are not valid UTF-8 are written as individual Latin-1 characters.
    pub fn addstr(&mut self, s: &[u8]) {
        for ch in decode_chars(s) {
            self.addch_char(ch, 0);
        }
    }

    /// `waddchstr` -- write a chtype array (each `(char, attr)`) starting at the
    /// cursor, overwriting cells and clipping at the right margin. Unlike
    /// `addstr` this does **not** advance the cursor and does not wrap.
    pub fn addchstr(&mut self, items: &[(u8, u32)]) {
        for (k, &(ch, attr)) in items.iter().enumerate() {
            let x = self.curx + k as i32;
            if x >= self.cols {
                break;
            }
            let i = self.idx(self.cury, x);
            self.cells[i] = Cell::plain(ch as char, attr);
        }
    }

    /// `mvwaddstr` -- move then add (no-op if the move is out of range).
    pub fn mvaddstr(&mut self, y: i32, x: i32, s: &[u8]) {
        if self.move_to(y, x) {
            self.addstr(s);
        }
    }

    /// `winsch` -- insert a character at the cursor, shifting the line right by
    /// one (the last column is lost). The cursor does not move.
    pub fn insch(&mut self, ch: u8) {
        let y = self.cury;
        let mut x = self.cols - 1;
        while x > self.curx {
            let (s, d) = (self.idx(y, x - 1), self.idx(y, x));
            self.cells[d] = self.cells[s];
            x -= 1;
        }
        let attr = self.cur_attr;
        let i = self.idx(y, self.curx);
        self.cells[i] = Cell::plain(ch as char, attr);
    }

    /// `wdelch` -- delete the character at the cursor, shifting the line left.
    pub fn delch(&mut self) {
        let y = self.cury;
        let mut x = self.curx;
        while x < self.cols - 1 {
            let (s, d) = (self.idx(y, x + 1), self.idx(y, x));
            self.cells[d] = self.cells[s];
            x += 1;
        }
        let i = self.idx(y, self.cols - 1);
        self.cells[i] = Cell::BLANK;
    }

    /// `winsstr` -- insert a string at the cursor (the line shifts right; the
    /// cursor does not move).
    pub fn insstr(&mut self, s: &[u8]) {
        for &b in s.iter().rev() {
            self.insch(b);
        }
    }

    /// `winsertln` -- insert a blank line at the cursor row, shifting lower lines
    /// down (the bottom line is lost). The cursor does not move.
    pub fn insertln(&mut self) {
        let mut y = self.rows - 1;
        while y > self.cury {
            for x in 0..self.cols {
                let (s, d) = (self.idx(y - 1, x), self.idx(y, x));
                self.cells[d] = self.cells[s];
            }
            y -= 1;
        }
        for x in 0..self.cols {
            let i = self.idx(self.cury, x);
            self.cells[i] = Cell::BLANK;
        }
    }

    /// `wdeleteln` -- delete the cursor's line, shifting lower lines up.
    pub fn deleteln(&mut self) {
        let mut y = self.cury;
        while y < self.rows - 1 {
            for x in 0..self.cols {
                let (s, d) = (self.idx(y + 1, x), self.idx(y, x));
                self.cells[d] = self.cells[s];
            }
            y += 1;
        }
        for x in 0..self.cols {
            let i = self.idx(self.rows - 1, x);
            self.cells[i] = Cell::BLANK;
        }
    }

    /// `scrollok` -- permit scrolling (`scroll`/`wscrl`, and bottom-line auto-scroll). Returns the
    /// previous setting.
    pub fn set_scrollok(&mut self, on: bool) -> bool {
        let prev = self.scroll_ok;
        self.scroll_ok = on;
        prev
    }
    /// Whether scrolling is enabled.
    pub fn scrollok(&self) -> bool {
        self.scroll_ok
    }

    /// `wsetscrreg(top, bot)` -- set the software scroll region (the rows `scroll`/`wscrl` shift).
    /// Out-of-range or inverted ranges are rejected (returns false).
    pub fn set_scrreg(&mut self, top: i32, bot: i32) -> bool {
        if top < 0 || bot >= self.rows || top > bot {
            return false;
        }
        self.scrreg_top = top;
        self.scrreg_bot = bot;
        true
    }

    /// `wscrl(n)` -- scroll the window's cells: `n > 0` moves content up `n` lines (the bottom `n`
    /// rows are filled with the background), `n < 0` moves it down `|n|` lines (the top rows filled).
    /// Returns false (ERR) when scrolling is not enabled. The cursor is unchanged.
    pub fn scroll(&mut self, n: i32) -> bool {
        if !self.scroll_ok {
            return false;
        }
        if n == 0 {
            return true;
        }
        // Scroll only within the software scroll region [top, bot] (default: the whole window).
        let (top, bot) = (self.scrreg_top, self.scrreg_bot);
        let bkgd = self.bkgd;
        if n > 0 {
            for y in top..=bot {
                for x in 0..self.cols {
                    let src = y + n;
                    let i = self.idx(y, x);
                    self.cells[i] = if src <= bot {
                        self.cells[self.idx(src, x)]
                    } else {
                        bkgd
                    };
                }
            }
        } else {
            let n = -n;
            for y in (top..=bot).rev() {
                for x in 0..self.cols {
                    let src = y - n;
                    let i = self.idx(y, x);
                    self.cells[i] = if src >= top {
                        self.cells[self.idx(src, x)]
                    } else {
                        bkgd
                    };
                }
            }
        }
        true
    }

    /// `winsdelln` -- insert (`n > 0`) or delete (`n < 0`) `|n|` lines.
    pub fn insdelln(&mut self, n: i32) {
        for _ in 0..n.abs() {
            if n > 0 {
                self.insertln();
            } else {
                self.deleteln();
            }
        }
    }

    /// `whline` -- draw `n` copies of `ch` rightward (clipped; cursor unchanged).
    /// `ch == 0` uses `ACS_HLINE` (`'q'`).
    pub fn hline(&mut self, ch: u8, n: i32) {
        let (c, acs) = if ch == 0 {
            (ACS_HLINE, true)
        } else {
            (ch as char, false)
        };
        let max = n.min(self.cols - self.curx).max(0);
        for k in 0..max {
            self.set_acs(self.cury, self.curx + k, c, acs);
        }
    }

    /// `wvline` -- draw `n` copies of `ch` downward (clipped; cursor unchanged).
    /// `ch == 0` uses `ACS_VLINE` (`'x'`).
    pub fn vline(&mut self, ch: u8, n: i32) {
        let (c, acs) = if ch == 0 {
            (ACS_VLINE, true)
        } else {
            (ch as char, false)
        };
        let max = n.min(self.rows - self.cury).max(0);
        for k in 0..max {
            self.set_acs(self.cury + k, self.curx, c, acs);
        }
    }

    /// `wborder` -- draw a border (sides + corners; `0` selects the ACS default).
    #[allow(clippy::too_many_arguments)]
    pub fn border(&mut self, ls: u8, rs: u8, ts: u8, bs: u8, tl: u8, tr: u8, bl: u8, br: u8) {
        // A `0` argument selects the ACS default, which carries A_ALTCHARSET; a user-supplied
        // character is drawn as-is (no alternate charset).
        let d = |ch: u8, def: char| {
            if ch == 0 {
                (def, true)
            } else {
                (ch as char, false)
            }
        };
        let (ls, lsa) = d(ls, ACS_VLINE);
        let (rs, rsa) = d(rs, ACS_VLINE);
        let (ts, tsa) = d(ts, ACS_HLINE);
        let (bs, bsa) = d(bs, ACS_HLINE);
        let (tl, tla) = d(tl, ACS_ULCORNER);
        let (tr, tra) = d(tr, ACS_URCORNER);
        let (bl, bla) = d(bl, ACS_LLCORNER);
        let (br, bra) = d(br, ACS_LRCORNER);
        let (r, c) = (self.rows, self.cols);
        self.set_acs(0, 0, tl, tla);
        self.set_acs(0, c - 1, tr, tra);
        self.set_acs(r - 1, 0, bl, bla);
        self.set_acs(r - 1, c - 1, br, bra);
        for x in 1..c - 1 {
            self.set_acs(0, x, ts, tsa);
            self.set_acs(r - 1, x, bs, bsa);
        }
        for y in 1..r - 1 {
            self.set_acs(y, 0, ls, lsa);
            self.set_acs(y, c - 1, rs, rsa);
        }
    }

    /// `box` with the given side chars and default ACS corners.
    pub fn draw_box(&mut self, verch: u8, horch: u8) {
        self.border(verch, verch, horch, horch, 0, 0, 0, 0);
    }

    /// The whole grid as `rows` byte rows -- a helper for state comparison.
    pub fn grid(&self) -> Vec<Vec<u8>> {
        (0..self.rows)
            .map(|y| {
                (0..self.cols)
                    .map(|x| self.cells[self.idx(y, x)].ch as u8)
                    .collect()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows_to_strings(w: &Window) -> Vec<String> {
        w.grid()
            .iter()
            .map(|r| String::from_utf8_lossy(r).into_owned())
            .collect()
    }

    #[test]
    fn plain_write() {
        let mut w = Window::new(4, 10);
        w.addstr(b"HELLO");
        assert_eq!(rows_to_strings(&w)[0], "HELLO     ");
        assert_eq!(w.getyx(), (0, 5));
    }

    #[test]
    fn wraps_at_right_margin() {
        let mut w = Window::new(4, 10);
        w.addstr(b"ABCDEFGHIJKL");
        let g = rows_to_strings(&w);
        assert_eq!(g[0], "ABCDEFGHIJ");
        assert_eq!(g[1], "KL        ");
    }

    #[test]
    fn newline_clears_to_eol() {
        let mut w = Window::new(3, 8);
        w.addstr(b"ABCDEF");
        w.move_to(0, 2);
        w.addch(b'\n');
        assert_eq!(rows_to_strings(&w)[0], "AB      ");
        assert_eq!(w.getyx(), (1, 0));
    }

    #[test]
    fn bottom_right_clips_without_scroll() {
        let mut w = Window::new(4, 10);
        w.move_to(3, 7);
        w.addstr(b"PQRS");
        assert_eq!(rows_to_strings(&w)[3], "       PQR");
    }

    #[test]
    fn mvaddstr_positions() {
        let mut w = Window::new(4, 10);
        w.mvaddstr(2, 3, b"XY");
        assert_eq!(rows_to_strings(&w)[2], "   XY     ");
    }

    #[test]
    fn attributes_on_cells() {
        let mut w = Window::new(1, 4);
        w.attrset(attrs::BOLD);
        w.addstr(b"AB");
        w.attroff(attrs::BOLD);
        w.addstr(b"C");
        assert_eq!(w.cell(0, 0).attr, 0x20_0000);
        assert_eq!(w.cell(0, 1).attr, 0x20_0000);
        assert_eq!(w.cell(0, 2).attr, 0);
        assert_eq!(w.cell(0, 0).ch, 'A');
    }

    #[test]
    fn addchstr_writes_chtype_array() {
        let mut w = Window::new(2, 6);
        w.move_to(0, 1);
        w.addchstr(&[(b'A', 0), (b'B', attrs::BOLD), (b'C', 0)]);
        assert_eq!(w.cell(0, 1).ch, 'A');
        assert_eq!(w.cell(0, 2), Cell::plain('B', attrs::BOLD));
        assert_eq!(w.cell(0, 3).ch, 'C');
        assert_eq!(w.getyx(), (0, 1)); // cursor unchanged
                                       // clip at right margin
        let mut w = Window::new(2, 6);
        w.move_to(0, 4);
        w.addchstr(&[(b'X', 0), (b'Y', 0), (b'Z', 0), (b'W', 0)]);
        assert_eq!(w.cell(0, 4).ch, 'X');
        assert_eq!(w.cell(0, 5).ch, 'Y');
    }

    #[test]
    fn color_pairs() {
        assert_eq!(color_pair(1), 0x100);
        assert_eq!(color_pair(2), 0x200);
        assert_eq!(pair_number(0x200 | attrs::BOLD), 2);
        let mut w = Window::new(1, 4);
        w.color_set(1);
        w.addstr(b"AB");
        w.attron(color_pair(2)); // color replaces, not OR
        w.addstr(b"C");
        assert_eq!(w.cell(0, 0).attr, 0x100);
        assert_eq!(w.cell(0, 2).attr, 0x200);
    }

    #[test]
    fn chgat_changes_attr_not_chars() {
        let mut w = Window::new(1, 4);
        w.addstr(b"WXYZ");
        w.move_to(0, 1);
        w.chgat(2, attrs::UNDERLINE);
        assert_eq!(rows_to_strings(&w)[0], "WXYZ");
        assert_eq!(w.cell(0, 1).attr, 0x2_0000);
        assert_eq!(w.cell(0, 2).attr, 0x2_0000);
        assert_eq!(w.cell(0, 3).attr, 0);
    }

    #[test]
    fn borders_and_lines() {
        let mut w = Window::new(4, 8);
        w.draw_box(0, 0);
        let g = rows_to_strings(&w);
        assert_eq!(g[0], "lqqqqqqk");
        assert_eq!(g[1], "x      x");
        assert_eq!(g[3], "mqqqqqqj");

        let mut w = Window::new(4, 8);
        w.draw_box(b'|', b'-');
        assert_eq!(rows_to_strings(&w)[0], "l------k");

        let mut w = Window::new(4, 8);
        w.move_to(1, 1);
        w.hline(b'-', 4);
        assert_eq!(rows_to_strings(&w)[1], " ----   ");
        assert_eq!(w.getyx(), (1, 1));
    }

    #[test]
    fn insert_delete_char() {
        let mut w = Window::new(3, 8);
        w.mvaddstr(0, 0, b"ABCDEF");
        w.move_to(0, 2);
        w.insch(b'X');
        assert_eq!(rows_to_strings(&w)[0], "ABXCDEF ");
        assert_eq!(w.getyx(), (0, 2));

        let mut w = Window::new(3, 8);
        w.mvaddstr(0, 0, b"ABCDEF");
        w.move_to(0, 2);
        w.delch();
        assert_eq!(rows_to_strings(&w)[0], "ABDEF   ");

        let mut w = Window::new(3, 8);
        w.mvaddstr(0, 0, b"ABCDEF");
        w.move_to(0, 2);
        w.insstr(b"XY");
        assert_eq!(rows_to_strings(&w)[0], "ABXYCDEF");
    }

    #[test]
    fn insert_delete_line() {
        let mut w = Window::new(3, 8);
        w.mvaddstr(0, 0, b"r0");
        w.mvaddstr(1, 0, b"r1");
        w.mvaddstr(2, 0, b"r2");
        w.move_to(1, 0);
        w.insertln();
        let g = rows_to_strings(&w);
        assert_eq!(
            (g[0].as_str(), g[1].as_str(), g[2].as_str()),
            ("r0      ", "        ", "r1      ")
        );

        let mut w = Window::new(3, 8);
        w.mvaddstr(0, 0, b"r0");
        w.mvaddstr(1, 0, b"r1");
        w.mvaddstr(2, 0, b"r2");
        w.move_to(1, 0);
        w.deleteln();
        let g = rows_to_strings(&w);
        assert_eq!(
            (g[0].as_str(), g[1].as_str(), g[2].as_str()),
            ("r0      ", "r2      ", "        ")
        );
    }

    #[test]
    fn double_width_cell_model() {
        // A double-width glyph fills its cell + a padding cell and advances two columns.
        assert_eq!(char_width('世'), 2);
        assert_eq!(char_width('Ａ'), 2); // fullwidth A
        assert_eq!(char_width('a'), 1);
        assert_eq!(char_width('é'), 1);
        let mut w = Window::new(2, 10);
        w.addstr("a世b".as_bytes());
        assert_eq!(w.cell(0, 0).ch, 'a');
        assert_eq!(w.cell(0, 1).ch, '世');
        assert_eq!(w.cell(0, 2).ch, WIDE_PAD);
        assert_eq!(w.cell(0, 3).ch, 'b');
        assert_eq!(w.getyx(), (0, 4)); // a(1) + 世(2) + b(1)
    }

    #[test]
    fn wscrl_shifts_cells() {
        let mut w = Window::new(5, 8);
        for (y, s) in ["r0", "r1", "r2", "r3", "r4"].iter().enumerate() {
            w.mvaddstr(y as i32, 0, s.as_bytes());
        }
        assert!(!w.scroll(2)); // ERR without scrollok
        w.set_scrollok(true);
        assert!(w.scroll(2)); // up by 2
        let g = rows_to_strings(&w);
        assert_eq!(g[0], "r2      ");
        assert_eq!(g[2], "r4      ");
        assert_eq!(g[3], "        ");
        assert_eq!(g[4], "        ");
        w.scroll(-1); // down by 1
        let g = rows_to_strings(&w);
        assert_eq!(g[0], "        ");
        assert_eq!(g[1], "r2      ");
        assert_eq!(g[3], "r4      ");
    }

    #[test]
    fn wscrl_respects_scroll_region() {
        let mut w = Window::new(6, 4);
        for y in 0..6 {
            w.mvaddstr(y, 0, format!("r{y}").as_bytes());
        }
        w.set_scrollok(true);
        assert!(w.set_scrreg(1, 4)); // region rows 1..=4
        w.scroll(1); // up 1 within [1,4]
        let g = rows_to_strings(&w);
        assert_eq!(g[0], "r0  "); // outside region, unchanged
        assert_eq!(g[1], "r2  ");
        assert_eq!(g[3], "r4  ");
        assert_eq!(g[4], "    "); // vacated bottom of region
        assert_eq!(g[5], "r5  "); // outside region, unchanged
    }

    #[test]
    fn mouse_geometry() {
        let w = Window::newwin(5, 10, 2, 3); // covers y 2..6, x 3..12
        assert!(w.wenclose(2, 3) && w.wenclose(6, 12));
        assert!(!w.wenclose(1, 3) && !w.wenclose(7, 7) && !w.wenclose(2, 13));
        assert_eq!(w.mouse_trafo(1, 2, true), Some((3, 5)));
        assert_eq!(w.mouse_trafo(4, 7, false), Some((2, 4)));
        assert_eq!(w.mouse_trafo(0, 0, false), None);
    }

    #[test]
    fn geometry() {
        let mut w = Window::newwin(5, 20, 3, 7);
        assert_eq!(w.getbegyx(), (3, 7));
        assert_eq!(w.getmaxyx(), (5, 20));
        w.mvwin(1, 2);
        assert_eq!(w.getbegyx(), (1, 2));
        let c = w.derwin(2, 5, 1, 1);
        assert_eq!(c.getbegyx(), (2, 3));
        assert_eq!(c.getparyx(), (1, 1));
        assert!(c.is_subwin() && !w.is_subwin());
    }

    #[test]
    fn combining_marks_attach_to_base() {
        // A zero-width combining mark attaches to the preceding base cell (no column advance), up to
        // CMAX marks; a precomposed glyph stays a single base cell.
        let mut w = Window::newwin(2, 20, 0, 0);
        w.addstr("e\u{0301}llo".as_bytes()); // e + combining acute, then "llo"
        assert_eq!(w.getyx(), (0, 4)); // 4 columns: e l l o (the mark advanced nothing)
        let e = w.cell(0, 0);
        assert_eq!(e.ch, 'e');
        assert_eq!(e.comb[0], '\u{0301}');
        assert_eq!(e.comb[1], '\0');
        assert_eq!(w.cell(0, 1).ch, 'l');
        // Two marks on one base.
        let mut w2 = Window::newwin(2, 20, 0, 0);
        w2.addstr("a\u{0301}\u{0308}".as_bytes());
        let a = w2.cell(0, 0);
        assert_eq!((a.ch, a.comb[0], a.comb[1]), ('a', '\u{0301}', '\u{0308}'));
        assert_eq!(w2.getyx(), (0, 1));
    }
}
