//! Screen erase operations and the single-field repaint -- the byte sequences ncurses emits to
//! clear regions of the screen and to paint one static field on an otherwise blank screen, on the
//! admitted xterm/ncurses 6.6 terminal.
//!
//! The erase ops are exact terminfo capabilities. [`single_field_repaint`] reproduces the
//! `wclear` + top-down `TransformLine` paint ncurses chooses when a single field is shown on a
//! cleared screen -- it is the **SEED** of the general `doupdate`. The general line-by-line diff of
//! an arbitrary previous->next screen is the parity TODO; this reproduces exactly one shape.

use crate::cursor::mvcur;

/// `clear` -- home then erase the whole screen: `\e[H\e[2J`.
pub fn clear() -> &'static [u8] {
    b"\x1b[H\x1b[2J"
}

/// `clr_eos` (erase to end of screen, `ed`): `\e[J`.
pub fn clr_eos() -> &'static [u8] {
    b"\x1b[J"
}

/// `clr_eol` (erase to end of line, `el`): `\e[K`.
pub fn clr_eol() -> &'static [u8] {
    b"\x1b[K"
}

/// `clr_bol` (erase to beginning of line, `el1`): `\e[1K`.
pub fn clr_bol() -> &'static [u8] {
    b"\x1b[1K"
}

/// The SGR sequence that restores ncurses's default attributes + colors on the admitted terminal:
/// ASCII-charset designation, `sgr0`, default fg/bg, then the default white-on-black pair. Emitted
/// at every attribute/color reset.
pub const RESET_DEFAULTS: &[u8] = b"\x1b(B\x1b[m\x1b[39;49m\x1b[37m\x1b[40m";

/// Append a CSI numeric command `\e[<n><final>` -- VPA (`d`), HPA/CHA (`G`).
fn csi1(out: &mut Vec<u8>, n: i32, fin: u8) {
    out.extend_from_slice(b"\x1b[");
    out.extend_from_slice(n.to_string().as_bytes());
    out.push(fin);
}

/// Append a CUP `\e[<row>;<col>H` -- direct cursor address, 1-based.
fn cup(out: &mut Vec<u8>, row: i32, col: i32) {
    out.extend_from_slice(b"\x1b[");
    out.extend_from_slice(row.to_string().as_bytes());
    out.push(0x3b); // ';'
    out.extend_from_slice(col.to_string().as_bytes());
    out.push(0x48); // 'H'
}

/// Append the foreground/background color SGRs for an ANSI color pair: `\e[<30+fg>m\e[<40+bg>m`.
/// `fg`/`bg` are ANSI colors `0..=7` directly (no remapping). Inputs are masked to three bits.
fn color_sgr(out: &mut Vec<u8>, fg: u8, bg: u8) {
    csi1(out, 30 + (fg & 0b111) as i32, 0x6d); // 'm'
    csi1(out, 40 + (bg & 0b111) as i32, 0x6d);
}

/// Reproduce the terminal byte stream that ncurses emits to paint **one static field** carrying an
/// ANSI color pair `(fg, bg)` onto an otherwise blank, freshly cleared screen -- the body between
/// the screen clear and the cursor parking on the row below the field.
///
/// `line`/`col` are 1-based; `data` is the field bytes; `fg`/`bg` are ANSI colors `0..=7` directly.
/// The sealed shape is `line >= 2` (a colored field on the very first row uses a different
/// positioning and is the documented non-claim).
///
/// This is the **SEED** of the full `doupdate`: it is ncurses's `wclear` + top-down `TransformLine`
/// repaint specialized to a single field on a blank screen. The returned bytes are, in order:
///
/// 1. `\e[<line+1>d` -- VPA parking on the row below the field,
/// 2. [`RESET_DEFAULTS`] -- the default-restore SGR,
/// 3. `\e[J` -- erase from there to the bottom (clears the field row and everything below),
/// 4. the top-down clear of the rows above the field: `\e[H\e[K` for row 1, then `\e[<r>d\e[K`
///    for each row `2..=line-1`,
/// 5. the field-row positioning: when the field starts within 5 columns (`col-1 <= 4`) it
///    space-fills from column 1 (`\e[<line>d` then `col-1` spaces); otherwise it cursor-addresses
///    just before the field and clears the leading blanks in one shot
///    (`\e[<line>;<col-1>H\e[1K` then one space onto column `col`),
/// 6. the colored field: the two color SGRs, the `data`, then [`RESET_DEFAULTS`] and `\e[K`,
/// 7. the move to the row below at column 1 -- the shared [`mvcur`] cost model, from the cursor's
///    end position `(line, col+len)` to `(line+1, 1)`.
pub fn single_field_repaint(line: i32, col: i32, data: &[u8], fg: u8, bg: u8) -> Vec<u8> {
    let mut out = Vec::new();

    // (1) Park on the row below the field, (2) restore defaults, (3) erase from there to the bottom.
    csi1(&mut out, line + 1, 0x64); // VPA 'd', row line+1
    out.extend_from_slice(RESET_DEFAULTS);
    out.extend_from_slice(b"\x1b[J"); // erase to end of screen

    // (4) Top-down clear of the rows above the field.
    out.extend_from_slice(b"\x1b[H\x1b[K");
    for r in 2..line {
        csi1(&mut out, r, 0x64); // VPA 'd'
        out.extend_from_slice(b"\x1b[K");
    }

    // (5) Position onto the field row.
    if col - 1 <= 4 {
        csi1(&mut out, line, 0x64); // VPA 'd', column stays 1
        if col > 1 {
            out.resize(out.len() + (col - 1) as usize, 0x20); // ' '
        }
    } else {
        cup(&mut out, line, col - 1);
        out.extend_from_slice(b"\x1b[1K");
        out.push(0x20); // ' '
    }

    // (6) The colored field.
    color_sgr(&mut out, fg, bg);
    out.extend_from_slice(data);
    out.extend_from_slice(RESET_DEFAULTS);
    out.extend_from_slice(b"\x1b[K");

    // (7) Move to the row below at column 1.
    let end_col = col + data.len() as i32;
    out.extend_from_slice(&mvcur((line, end_col), (line + 1, 1)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erase_ops() {
        assert_eq!(clear(), b"\x1b[H\x1b[2J");
        assert_eq!(clr_eos(), b"\x1b[J");
        assert_eq!(clr_eol(), b"\x1b[K");
        assert_eq!(clr_bol(), b"\x1b[1K");
    }

    #[test]
    fn reset_defaults_constant() {
        assert_eq!(RESET_DEFAULTS, b"\x1b(B\x1b[m\x1b[39;49m\x1b[37m\x1b[40m");
    }

    // The oracle-pinned repaint bodies. The reference captures used a green-on-blue field as ANSI
    // SGR \e[32m\e[44m (fg 2, bg 4) etc.; here fg/bg are ANSI colors directly.
    #[test]
    fn single_field_repaint_matches_oracle_bodies() {
        // Field at (2,3), green-on-blue (ANSI fg 2, bg 4) -> \e[32m\e[44m. col-1=2 <= 4 -> space-fill.
        assert_eq!(
            single_field_repaint(2, 3, b"X", 2, 4),
            b"\x1b[3d\x1b(B\x1b[m\x1b[39;49m\x1b[37m\x1b[40m\x1b[J\x1b[H\x1b[K\x1b[2d  \x1b[32m\x1b[44mX\x1b(B\x1b[m\x1b[39;49m\x1b[37m\x1b[40m\x1b[K\r\x1b[3d".to_vec()
        );
        // Field at (3,1): col-1=0 space-fill (none); rows above cleared; prompt move VPA+backspace.
        assert_eq!(
            single_field_repaint(3, 1, b"X", 2, 4),
            b"\x1b[4d\x1b(B\x1b[m\x1b[39;49m\x1b[37m\x1b[40m\x1b[J\x1b[H\x1b[K\x1b[2d\x1b[K\x1b[3d\x1b[32m\x1b[44mX\x1b(B\x1b[m\x1b[39;49m\x1b[37m\x1b[40m\x1b[K\x1b[4d\x08".to_vec()
        );
        // Field at (3,6): col-1=5 > 4 -> CUP+clr_bol+space positioning.
        assert_eq!(
            single_field_repaint(3, 6, b"X", 2, 4),
            b"\x1b[4d\x1b(B\x1b[m\x1b[39;49m\x1b[37m\x1b[40m\x1b[J\x1b[H\x1b[K\x1b[2d\x1b[K\x1b[3;5H\x1b[1K \x1b[32m\x1b[44mX\x1b(B\x1b[m\x1b[39;49m\x1b[37m\x1b[40m\x1b[K\r\x1b[4d".to_vec()
        );
        // Field at (5,10): more rows above, CUP positioning, longer field below.
        assert_eq!(
            single_field_repaint(5, 10, b"X", 2, 4),
            b"\x1b[6d\x1b(B\x1b[m\x1b[39;49m\x1b[37m\x1b[40m\x1b[J\x1b[H\x1b[K\x1b[2d\x1b[K\x1b[3d\x1b[K\x1b[4d\x1b[K\x1b[5;9H\x1b[1K \x1b[32m\x1b[44mX\x1b(B\x1b[m\x1b[39;49m\x1b[37m\x1b[40m\x1b[K\r\x1b[6d".to_vec()
        );
        // Multi-byte field "HELLO" at (4,10), red-on-yellow (ANSI fg 1, bg 3) -> \e[31m\e[43m.
        assert_eq!(
            single_field_repaint(4, 10, b"HELLO", 1, 3),
            b"\x1b[5d\x1b(B\x1b[m\x1b[39;49m\x1b[37m\x1b[40m\x1b[J\x1b[H\x1b[K\x1b[2d\x1b[K\x1b[3d\x1b[K\x1b[4;9H\x1b[1K \x1b[31m\x1b[43mHELLO\x1b(B\x1b[m\x1b[39;49m\x1b[37m\x1b[40m\x1b[K\r\x1b[5d".to_vec()
        );
        // Field "QRS" at (6,2), blue-on-magenta (ANSI fg 4, bg 5) -> \e[34m\e[45m. col-1=1 space-fill.
        assert_eq!(
            single_field_repaint(6, 2, b"QRS", 4, 5),
            b"\x1b[7d\x1b(B\x1b[m\x1b[39;49m\x1b[37m\x1b[40m\x1b[J\x1b[H\x1b[K\x1b[2d\x1b[K\x1b[3d\x1b[K\x1b[4d\x1b[K\x1b[5d\x1b[K\x1b[6d \x1b[34m\x1b[45mQRS\x1b(B\x1b[m\x1b[39;49m\x1b[37m\x1b[40m\x1b[K\r\x1b[7d".to_vec()
        );
    }
}
