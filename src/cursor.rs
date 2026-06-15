//! The cursor-movement cost optimizer -- the crown jewel.
//!
//! [`mvcur`] reproduces ncurses's `mvcur` / `relative_move` strategy enumeration for the admitted
//! xterm/ncurses 6.6 terminal: given a current cursor position and a target, it enumerates the move
//! strategies ncurses considers and emits the **shortest**, with the local/CR/home strategies winning
//! a byte-count tie over the direct cursor-address. All coordinates are **1-based** `(row, col)`.
//!
//! The strategies, exactly as ncurses weighs them here:
//!
//! 1. **Keep the column, move vertically, then horizontally from the source column.** Vertical is
//!    `cuu1 \e[A` for up-one or `VPA \e[<r>d` otherwise; horizontal is space-fill, a short
//!    `HPA \e[<c>G` (only for 5..=7 column forward advances), or backspaces.
//! 2. **Carriage-return to column 1, move vertically, then horizontally from column 1.**
//! 3. **`home \e[H`** for the exact `(1, 1)` target.
//! 4. **Direct cursor-address `CUP \e[<r>;<c>H`** -- generated last so it loses byte-count ties to
//!    the local/CR/home strategies (the empirically pinned tie-break).

/// Append `n` spaces -- the cheapest right-move for a short column advance (each space overwrites a
/// known blank cell, advancing the cursor by one).
fn spaces(out: &mut Vec<u8>, n: i32) {
    if n > 0 {
        out.resize(out.len() + n as usize, 0x20); // ' '
    }
}

/// Append a CSI numeric command `\e[<n><final>` -- used for VPA (`d`) and HPA/CHA (`G`).
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

/// Build the candidate byte sequences for a **horizontal** move within a row, from column `sc` to
/// column `tx`, matching ncurses `relative_move`'s column handling.
///
/// * Forward: space-fill always; a column-address `HPA \e[<tx>G` candidate is added only when it is
///   the cheaper choice (an advance of 5..=7 columns -- ncurses does not reach for HPA on longer
///   same-row runs, preferring a direct cursor-address there). The HPA candidate is listed first so
///   it wins a byte-count tie.
/// * Backward: backspaces (`0x08`).
fn horiz_candidates(sc: i32, tx: i32) -> Vec<Vec<u8>> {
    if tx == sc {
        return vec![Vec::new()];
    }
    let mut out = Vec::new();
    if tx > sc {
        let delta = tx - sc;
        if (5..=7).contains(&delta) {
            let mut h = Vec::new();
            csi1(&mut h, tx, 0x47); // 'G'
            out.push(h);
        }
        let mut s = Vec::new();
        spaces(&mut s, delta);
        out.push(s);
    } else {
        out.push(vec![0x08; (sc - tx) as usize]); // backspaces
    }
    out
}

/// Reproduce ncurses's `mvcur` choice for a move from `from = (fy, fx)` to `to = (ty, tx)` on the
/// admitted xterm/ncurses 6.6 terminal. Coordinates are **1-based** `(row, col)`. Returns the exact
/// bytes ncurses would emit -- empty when the move is a no-op (`from == to`).
///
/// This single function reproduces the from-home first move, every inter-position move, and any
/// move back toward column 1. It is pure and never panics.
pub fn mvcur(from: (i32, i32), to: (i32, i32)) -> Vec<u8> {
    let (fy, fx) = from;
    let (ty, tx) = to;
    let mut cands: Vec<Vec<u8>> = Vec::new();

    // Strategy 1: vertical (keeping the column) + horizontal from fx.
    let mut verticals: Vec<Vec<u8>> = Vec::new();
    if ty == fy {
        verticals.push(Vec::new());
    } else {
        if ty == fy - 1 {
            verticals.push(b"\x1b[A".to_vec()); // cuu1 (up one)
        }
        let mut v = Vec::new();
        csi1(&mut v, ty, 0x64); // VPA 'd'
        verticals.push(v);
    }
    for v in &verticals {
        for h in horiz_candidates(fx, tx) {
            let mut c = v.clone();
            c.extend_from_slice(&h);
            cands.push(c);
        }
    }

    // Strategy 2: CR to column 1, then vertical, then horizontal from column 1.
    {
        let mut c = vec![0x0d]; // '\r'
        if ty != fy {
            csi1(&mut c, ty, 0x64); // VPA 'd'
        }
        // After CR the column is 1; only a forward (or empty) horizontal applies.
        c.extend_from_slice(&horiz_candidates(1, tx)[0]);
        cands.push(c);
    }

    // Strategy 3: home for the exact (1,1) target.
    if ty == 1 && tx == 1 {
        cands.push(b"\x1b[H".to_vec());
    }

    // Strategy 4: direct cursor-address (CUP) -- listed last so it loses ties to the local strategies.
    {
        let mut c = Vec::new();
        cup(&mut c, ty, tx);
        cands.push(c);
    }

    // Shortest wins; the first-generated candidate wins a tie (local/CR/home before CUP).
    cands.into_iter().min_by_key(|c| c.len()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_op_move_is_empty() {
        assert_eq!(mvcur((1, 1), (1, 1)), b"");
        assert_eq!(mvcur((5, 9), (5, 9)), b"");
    }

    #[test]
    fn home_target_uses_home() {
        // (1,1) target is reachable by \e[H (3 bytes), shorter than CUP \e[1;1H.
        assert_eq!(mvcur((10, 40), (1, 1)), b"\x1b[H");
    }

    #[test]
    fn same_row_branches() {
        // From the home-and-write derivation: after writing a char at (1,1) the cursor is at (1,2);
        // these are the move encodings the cost model picks for various same-row advances.
        // <=4 forward cols -> spaces.
        assert_eq!(mvcur((1, 5), (1, 9)), b"    "); // 4 spaces
        // 5..=7 forward cols -> HPA wins the tie (listed first, same length as 5..=7 spaces? no:
        // HPA \e[<c>G is 4-5 bytes; for delta 7 it ties/beats 7 spaces).
        assert_eq!(mvcur((1, 1), (1, 8)), b"\x1b[8G"); // delta 7 -> HPA
        // backward: backspaces are a candidate, but the CR strategy (CR then 1 space from col 1)
        // is shorter here and wins -- exactly as ncurses picks the cheapest path.
        assert_eq!(mvcur((1, 5), (1, 2)), b"\r ");
        // a backward move where backspaces are cheapest (close to the source, far from col 1).
        assert_eq!(mvcur((1, 9), (1, 8)), b"\x08");
    }

    #[test]
    fn vertical_then_horizontal() {
        // (2,3) from home: VPA to row 2 then 2 spaces from col 1.
        assert_eq!(mvcur((1, 1), (2, 3)), b"\x1b[2d  ");
        // up-one uses cuu1 when shortest: from (3,1) to (2,1) -> \e[A.
        assert_eq!(mvcur((3, 1), (2, 1)), b"\x1b[A");
    }

    #[test]
    fn cup_when_cheapest() {
        // Far same-row advance: from (1,1) to (1,10) the local path (9 spaces) loses to CUP.
        assert_eq!(mvcur((1, 1), (1, 10)), b"\x1b[1;10H");
        // Row + column change where neither local strategy is short -> CUP.
        assert_eq!(mvcur((1, 1), (3, 5)), b"\x1b[3;5H");
        assert_eq!(mvcur((1, 1), (10, 40)), b"\x1b[10;40H");
    }

    #[test]
    fn prompt_moves_match_screenio_pins() {
        // The cursor-cost encodings pinned against the oracle (1-based). After writing one char the
        // cursor sits one column past the field; these are the moves to the row below at column 1.
        // From (1,2) [after writing "Z" at (1,1)] down to (2,1): VPA row 2 then a single backspace.
        assert_eq!(mvcur((1, 2), (2, 1)), b"\x1b[2d\x08");
        // From (1,6) [after "Z" at col 5] to (2,1): CR then VPA row 2.
        assert_eq!(mvcur((1, 6), (2, 1)), b"\r\x1b[2d");
        // From (2,4) [after "Z" at (2,3)] to (3,1): CR then VPA row 3.
        assert_eq!(mvcur((2, 4), (3, 1)), b"\r\x1b[3d");
    }
}
