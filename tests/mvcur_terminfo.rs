//! Proof that the terminfo-driven `Caps::from_terminfo` reproduces the hand-built engine: loading
//! the committed xterm / linux terminfo fixtures and running the full sampled move grid must produce
//! byte-identical output to `Caps::xterm()` (both terminals share xterm's cursor cap set). This pins
//! the terminfo-general cursor path offline, without the live database.

use ncurses_native::{mvcur_caps, Caps, Terminfo};

const ROWS: [i32; 9] = [0, 1, 2, 3, 5, 8, 12, 18, 23];
const COLS: [i32; 17] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 15, 20, 39, 40, 78, 79];

fn caps_from_fixture(name: &str) -> Caps {
    let bytes = std::fs::read(format!("tests/terminfo/{name}")).expect("read fixture");
    let t = Terminfo::parse(&bytes).expect("parse terminfo");
    Caps::from_terminfo(&t)
}

/// `from_terminfo` of a fixture must match the given reference profile across the whole grid.
fn assert_grid_matches(derived: &Caps, reference: &Caps) {
    let mut mismatches = 0;
    for &fy in &ROWS {
        for &fx in &COLS {
            for &ty in &ROWS {
                for &tx in &COLS {
                    let a = mvcur_caps(derived, (fy + 1, fx + 1), (ty + 1, tx + 1));
                    let b = mvcur_caps(reference, (fy + 1, fx + 1), (ty + 1, tx + 1));
                    if a != b {
                        mismatches += 1;
                        if mismatches <= 5 {
                            eprintln!("({fy},{fx})->({ty},{tx}): derived={a:?} reference={b:?}");
                        }
                    }
                }
            }
        }
    }
    assert_eq!(
        mismatches, 0,
        "terminfo-derived caps diverged from the reference"
    );
}

#[test]
fn from_terminfo_xterm_matches_hardcoded() {
    // The xterm fixture, driven through from_terminfo, reproduces Caps::xterm() exactly -- the same
    // 23409-pair behaviour the live mvcur matrix pins.
    let derived = caps_from_fixture("xterm");
    assert_grid_matches(&derived, &Caps::xterm());
}

#[test]
fn from_terminfo_linux_matches_xterm() {
    // The Linux console shares xterm's cursor cap set (cup/hpa/vpa/cuf1/cub1/cuu1/home, cud1=\n),
    // so from_terminfo(linux) reproduces the same cursor motion as xterm.
    let derived = caps_from_fixture("linux");
    assert_grid_matches(&derived, &Caps::xterm());
}

#[test]
fn from_terminfo_aixterm_back_wraps() {
    // aixterm has auto_left_margin (bw) but no vpa: a target at the right margin is reached by a
    // backspace that wraps to the previous line's last column, then a vertical move. Captured from
    // real ncurses-on-aixterm.
    let c = caps_from_fixture("aixterm");
    // (2,0)->(0,79): from col 0, BS wraps to (1,79), cuu1 up to (0,79).
    assert_eq!(mvcur_caps(&c, (3, 1), (1, 80)), b"\x08\x1b[A");
    // (11,0)->(0,79): BS to (10,79), cuu 10.
    assert_eq!(mvcur_caps(&c, (12, 1), (1, 80)), b"\x08\x1b[10A");
    // (2,1)->(0,79): from col 1, CR first, then BS-wrap + cuu1.
    assert_eq!(mvcur_caps(&c, (3, 2), (1, 80)), b"\r\x08\x1b[A");
    // (2,0)->(11,79): BS to (1,79), cud 10 down.
    assert_eq!(mvcur_caps(&c, (3, 1), (12, 80)), b"\x08\x1b[10B");
}

#[test]
fn from_terminfo_wy350_uses_cursor_to_ll() {
    // wy350 has cursor_to_ll (ll = `\x1e\x0b`, jump to lower-left (23,0)); a last-line target is
    // reached via ll + cuf1 (`\x0c`) per column. Captured from real ncurses-on-wy350.
    let c = caps_from_fixture("wy350-wvb");
    // (any)->(23,0): ll alone.
    assert_eq!(mvcur_caps(&c, (1, 1), (24, 1)), b"\x1e\x0b");
    // (any)->(23,5): ll + 5 cuf1.
    assert_eq!(
        mvcur_caps(&c, (1, 1), (24, 6)),
        b"\x1e\x0b\x0c\x0c\x0c\x0c\x0c"
    );
}

#[test]
fn from_terminfo_cupless_uses_relative_motions() {
    // pmcons has no cup, no hpa: it can *only* move by local motions. ncurses gates the NOT_LOCAL
    // short-circuit on cup existing (lib_mvcur.c: the skip is nested inside the cup block), so even a
    // "far" move runs the relative optimizer rather than emitting nothing. Captured from real
    // ncurses-on-pmcons (cub1 = `\x08`, cuu1 = `\x0b`).
    let c = caps_from_fixture("pmcons");
    // (0,79)->(0,40): same row, 39 backspaces (a "far" move, but cup-less so still local).
    assert_eq!(mvcur_caps(&c, (1, 80), (1, 41)), vec![0x08u8; 39]);
    // (2,79)->(0,40): up 2 via cuu1 (`\x0b`), then 39 backspaces.
    let mut exp = vec![0x0bu8, 0x0b];
    exp.extend(std::iter::repeat(0x08u8).take(39));
    assert_eq!(mvcur_caps(&c, (3, 80), (1, 41)), exp);
}

#[test]
fn from_terminfo_newline_cud1_with_padding_is_unusable() {
    // ncr160wy60pp's cud1 is `\n$<5>` -- a newline (with padding). ncurses tests `*cursor_down !=
    // '\n'` (the first byte), so it is rejected for local downward motion exactly like a bare `\n`;
    // with no vpa, a downward move falls through to cup. Captured from real ncurses.
    let c = caps_from_fixture("ncr160wy60pp");
    // (0,0)->(2,0): cup (cud1=`\n$<5>` is rejected), NOT `\n\n`. cup = `\E=%p1' '+%c%p2' '+%c`.
    assert_eq!(mvcur_caps(&c, (1, 1), (3, 1)), b"\x1b=\x22\x20"); // \E= "(34=2+32) (32=0+32)
    assert_eq!(mvcur_caps(&c, (2, 2), (3, 2)), b"\x1b=\x22\x21"); // row2 col1 -> " !
}

#[test]
fn from_terminfo_bw_backwrap_to_inner_column() {
    // apple2e has bw (auto_left_margin), no xenl, no cup, cuu1=`\x1f`, no cuf1. ncurses tactic #5
    // back-wraps a cub1 at column 0 to the previous line's last column, then runs a *full*
    // relative_move from that corner to the target -- which need NOT be the right margin. Captured
    // from real ncurses-on-apple2e.
    let c = caps_from_fixture("apple2e");
    // (2,0)->(0,1): cub1 wrap to (1,79), cuu1 to (0,79), then 78 backspaces to col 1.
    let mut exp = vec![0x08u8, 0x1f];
    exp.extend(std::iter::repeat(0x08u8).take(78));
    assert_eq!(mvcur_caps(&c, (3, 1), (1, 2)), exp);
}

#[test]
fn from_terminfo_bw_backwrap_then_hpa() {
    // hp2626-ns has bw, no xenl, cup (padded), cuu1=`\EA`, hpa=`\E&a%p1%dC`. For (2,0)->(0,5) the
    // back-wrap leg (cub1 + cuu1) plus hpa to column 5 beats cup, so ncurses uses tactic #5 with the
    // relative leg ending in hpa -- not an all-backspace walk. Captured from real ncurses.
    let c = caps_from_fixture("hp2626-ns");
    // (2,0)->(0,5): \x08 (wrap) + \EA (cuu1) + \E&a5C (hpa col5).
    assert_eq!(mvcur_caps(&c, (3, 1), (1, 6)), b"\x08\x1bA\x1b&a5C");
}

#[test]
fn from_terminfo_xenl_disables_bw_back_wrap() {
    // vi300-old has auto_left_margin (bw) AND eat_newline_glitch (xenl). The magic margin makes the
    // bw back-wrap to the last column unreliable, so ncurses uses cup -- not the `\x08`-wrap that the
    // bw-without-xenl aixterm uses for the same move. Captured from real ncurses.
    let c = caps_from_fixture("vi300-old");
    assert_eq!(mvcur_caps(&c, (3, 1), (1, 80)), b"\x1b[1;80H"); // cup, not \x08\x1b[A
                                                                // Contrast: aixterm (bw, no xenl) does use the back-wrap.
    let a = caps_from_fixture("aixterm");
    assert_eq!(mvcur_caps(&a, (3, 1), (1, 80)), b"\x08\x1b[A");
}
