//! Proof that the terminfo-driven `Screen::from_terminfo` reproduces the hand-built doupdate engine:
//! configuring a Screen from the committed xterm terminfo fixture must paint byte-identically to the
//! default `Screen::new` (which hardcodes the xterm caps). Pins the terminfo-general doupdate path
//! offline, without the live database.

use ncurses_native::color::Palette;
use ncurses_native::update::Screen;
use ncurses_native::window::{attrs, Cell};
use ncurses_native::Terminfo;

const ROWS: i32 = 24;
const COLS: i32 = 80;

fn scenario() -> (Vec<Cell>, i32, i32) {
    let mut g = vec![Cell::plain(' ', 0); (ROWS * COLS) as usize];
    let put = |g: &mut Vec<Cell>, r: i32, c: i32, s: &str| {
        for (i, ch) in s.chars().enumerate() {
            g[(r * COLS + c + i as i32) as usize] = Cell::plain(ch, 0);
        }
    };
    put(&mut g, 2, 5, "hello world");
    put(&mut g, 5, 0, "abc");
    put(&mut g, 7, 40, "right side text");
    (g, 5, 3)
}

/// The DB-sweep scenario: "hello world" at (2,5), "abc" at (5,0), cursor parked at (5,3). Used by the
/// no-`clear`-cap clearing-strategy tests below (golden bytes captured from real ncurses).
fn db_scenario() -> (Vec<Cell>, i32, i32) {
    let mut g = vec![Cell::plain(' ', 0); (ROWS * COLS) as usize];
    let put = |g: &mut Vec<Cell>, r: i32, c: i32, s: &str| {
        for (i, ch) in s.chars().enumerate() {
            g[(r * COLS + c + i as i32) as usize] = Cell::plain(ch, 0);
        }
    };
    put(&mut g, 2, 5, "hello world");
    put(&mut g, 5, 0, "abc");
    (g, 5, 3)
}

fn first_paint(fixture: &str) -> Vec<u8> {
    let bytes = std::fs::read(format!("tests/terminfo/{fixture}")).expect("read fixture");
    let t = Terminfo::parse(&bytes).expect("parse terminfo");
    let mut s = Screen::from_terminfo(ROWS, COLS, &t, Palette::new());
    let (cells, cy, cx) = db_scenario();
    s.doupdate(&cells, cy, cx)
}

#[test]
fn from_terminfo_no_clear_cap_uses_clr_eos() {
    // newhp has no `clear` and no `cup`, but has `ed` (`\EJ`). ncurses' ClearScreen falls back to
    // clr_eos (case 2): GoTo(0,0) -- which emits *nothing* on a cup-less terminal, not a hardcoded
    // `\e[H` -- then `\EJ`; the text then streams out unpositioned. Captured from real ncurses.
    assert_eq!(first_paint("newhp"), b"\x1bJhello worldabc");
}

#[test]
fn from_terminfo_no_clear_no_ed_uses_clr_eol_per_line() {
    // avatar has no `clear`, no `ed`, but has `el` (`\x16\x07`) and a cup (`\x16\x08%p1%c%p2%c`).
    // ClearScreen case 3 walks every line: GoTo(i,0) + clr_eol, then GoTo(0,0); then the cells are
    // painted via cup. There is *no* hardcoded `\e[H\e[2J`. Captured from real ncurses-on-avatar.
    let out = first_paint("avatar");
    // No fake CSI clear.
    assert!(!out.windows(4).any(|w| w == b"\x1b[2J"));
    // The clear is el-per-line: the clr_eol byte pair appears many times before any text.
    let els = out.windows(2).filter(|w| *w == b"\x16\x07").count();
    assert!(
        els >= 20,
        "expected >=20 clr_eol per-line clears, got {els}"
    );
    // Ends with the cup-positioned text (cup = \x16\x08 + row + col; (5,0) -> col 0 emits 0x80).
    assert!(out.ends_with(b"\x16\x08\x02\x05hello world\x16\x08\x05\x80abc"));
}

#[test]
fn from_terminfo_no_erase_cap_blank_fills() {
    // gt40 (a DEC graphics terminal) has no clear/ed/el and auto-margins: ClearScreen case 4
    // overwrites the whole screen with blanks (24*80 spaces, the auto-wrap making each GoTo(i,0) a
    // no-op), then the unpositioned text. No hardcoded CSI clear. Captured from real ncurses.
    let out = first_paint("gt40");
    let mut expected = vec![b' '; (ROWS * COLS) as usize];
    expected.extend_from_slice(b"hello worldabc");
    assert_eq!(out, expected);
}

/// Build scene A then return the bytes of the incremental diff to scene B (mirrors the
/// examples/doupdate_db2 / duinc oracle scenario). Golden bytes captured from real ncurses.
fn incremental_diff(fixture: &str) -> Vec<u8> {
    let bytes = std::fs::read(format!("tests/terminfo/{fixture}")).expect("read fixture");
    let t = Terminfo::parse(&bytes).expect("parse terminfo");
    let mut s = Screen::from_terminfo(ROWS, COLS, &t, Palette::new());
    let put = |g: &mut Vec<Cell>, r: i32, c: i32, st: &str| {
        for (i, ch) in st.chars().enumerate() {
            g[(r * COLS + c + i as i32) as usize] = Cell::plain(ch, 0);
        }
    };
    let mut a = vec![Cell::plain(' ', 0); (ROWS * COLS) as usize];
    put(&mut a, 2, 5, "hello world");
    put(&mut a, 5, 0, "abc");
    let mut b = vec![Cell::plain(' ', 0); (ROWS * COLS) as usize];
    put(&mut b, 2, 5, "HELLO");
    put(&mut b, 5, 0, "abcdef");
    put(&mut b, 10, 20, "xyz");
    let _ = s.doupdate(&a, 5, 3);
    s.doupdate(&b, 11, 0)
}

fn unhex(h: &str) -> Vec<u8> {
    (0..h.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn from_terminfo_bce_biases_trailing_clear_to_clr_eol() {
    // arm100 has bce and a padded el (`\E[K$<3>`, NormalizedCost 18). ncurses forces _el_cost to 0
    // for a bce terminal (lib_mvcur.c), so the trailing " world"->blank clear uses clr_eol rather
    // than overwriting with spaces -- even though the literal el is "expensive". Captured from real
    // ncurses; note the `\x1b[K` after HELLO (not 6 spaces).
    let out = incremental_diff("arm100");
    assert!(
        out.windows(3).any(|w| w == b"\x1b[K"),
        "trailing clear should use clr_eol"
    );
    assert_eq!(
        out,
        unhex("1b5b3341202048454c4c4f1b5b4b0d1b5b33426162636465661b5b31313b32314878797a0d1b5b3142")
    );
}

#[test]
fn from_terminfo_rep_is_terminfo_derived() {
    // concept's repeat_char is `\Er%p1%c%p2%' '%+%c$<.2*>` (rep_cost 5), not the ECMA `\e[Nb`. The
    // 6 trailing blanks after HELLO are coalesced with the terminal's own rep -> `\x1br &` (\Er,
    // space, 6+32='&'), emitted via tparm. Captured from real ncurses-on-concept.
    let out = incremental_diff("concept");
    assert!(
        out.windows(4).any(|w| w == b"\x1br &"),
        "rep should be concept's \\Er cap"
    );
    assert!(
        !out.windows(2).any(|w| w == b"\x1b["),
        "no ECMA CSI on a non-CSI terminal"
    );
    assert_eq!(
        out,
        unhex("1b61222548454c4c4f1b7220261b6125236465661b612a3478797a1b612b20")
    );
}

#[test]
fn from_terminfo_leading_blank_repositions_directly() {
    // hp2645 (HP block-mode): the new "xyz" at (10,20) over a blank line is reached by a single cup
    // (`\E&a20c10Y$<6>` + 25 NUL pads), not a spurious GoTo-to-column-0 first. ncurses sets
    // firstChar=nFirstChar unconditionally when the new line has fewer leading blanks. Captured from
    // real ncurses; the cup to (10,20) appears exactly once before xyz.
    let out = incremental_diff("hp2645");
    let cups = out.windows(9).filter(|w| *w == b"\x1b&a20c10Y").count();
    assert_eq!(
        cups, 1,
        "exactly one cup to (10,20), no spurious extra GoTo"
    );
    assert_eq!(out, unhex("1b26613259202048454c4c4f1b4b1b266135591b266133436465661b26613230633130590000000000000000000000000000000000000000000000000078797a1b26613131591b26613043"));
}

/// First paint of the attribute scenario (each row a distinct attribute), mirroring examples/attr_db.
/// Golden bytes captured from real ncurses.
fn attr_first_paint(fixture: &str) -> Vec<u8> {
    let bytes = std::fs::read(format!("tests/terminfo/{fixture}")).expect("read fixture");
    let t = Terminfo::parse(&bytes).expect("parse terminfo");
    let mut s = Screen::from_terminfo(ROWS, COLS, &t, Palette::new());
    let mut g = vec![Cell::plain(' ', 0); (ROWS * COLS) as usize];
    let puta = |g: &mut Vec<Cell>, r: i32, st: &str, a: u32| {
        for (i, ch) in st.chars().enumerate() {
            g[(r * COLS + i as i32) as usize] = Cell::plain(ch, a);
        }
    };
    puta(&mut g, 1, "bold", attrs::BOLD);
    puta(&mut g, 2, "undl", attrs::UNDERLINE);
    puta(&mut g, 3, "revs", attrs::REVERSE);
    puta(&mut g, 4, "stnd", attrs::STANDOUT);
    puta(&mut g, 5, "blnk", attrs::BLINK);
    puta(&mut g, 6, "dimm", attrs::DIM);
    puta(&mut g, 7, "norm", attrs::NORMAL);
    s.doupdate(&g, 8, 0)
}

#[test]
fn from_terminfo_no_sgr_uses_individual_mode_caps() {
    // ti916-8-132 has no `set_attributes` (sgr): ncurses' vidputs drives it with the individual mode
    // caps -- enter_bold (`\e[1m`), enter_underline (`\e[4m`), the exit caps `rmul`=`\e[24m` /
    // `rmso`=`\e[27m` (used when they differ from sgr0), and `sgr0`=`\e[m\e(B` for a full reset.
    // Captured from real ncurses-on-ti916-8-132.
    let out = attr_first_paint("ti916-8-132");
    assert!(
        out.windows(5).any(|w| w == b"\x1b[24m"),
        "rmul individual exit used"
    );
    assert!(
        out.windows(5).any(|w| w == b"\x1b[27m"),
        "rmso individual exit used"
    );
    assert_eq!(out, unhex("1b5b481b5b324a1b5b32641b5b316d626f6c640d1b5b33641b5b6d1b28421b5b346d756e646c0d1b5b34641b5b32346d1b5b376d726576730d1b5b35641b5b6d1b28421b5b376d73746e640d1b5b36641b5b32376d1b5b356d626c6e6b0d1b5b37641b5b6d1b284264696d6d0d1b5b38641b5b6d1b28426e6f726d0d1b5b3964"));
}

#[test]
fn from_terminfo_no_msgr_resets_attrs_around_moves() {
    // wyse520-w lacks move_standout_mode (msgr): ncurses resets to A_NORMAL before each cursor move
    // and restores the attribute after (lib_mvcur.c). So each attributed run is followed by sgr0
    // (`\e[m\x0f`) before the `\r`+vpa, then the attribute is re-applied. Captured from real ncurses.
    let out = attr_first_paint("wyse520-w");
    // sgr0 reset (`\e[m\x0f`) appears before the moves (one per attributed row).
    let resets = out.windows(4).filter(|w| *w == b"\x1b[m\x0f").count();
    assert!(
        resets >= 6,
        "expected a reset before each move, got {resets}"
    );
    assert_eq!(out, unhex("1b5b481b5b4a1b5b32641b5b303b316d0f626f6c641b5b6d0f0d1b5b33641b5b303b316d0f1b5b303b346d0f756e646c1b5b6d0f0d1b5b34641b5b303b346d0f1b5b303b376d0f726576731b5b6d0f0d1b5b35641b5b303b376d0f1b5b303b376d0f73746e641b5b6d0f0d1b5b36641b5b303b376d0f1b5b303b356d0f626c6e6b1b5b6d0f0d1b5b37641b5b303b356d0f1b5b303b326d0f64696d6d1b5b6d0f0d1b5b38641b5b303b326d0f1b5b6d0f6e6f726d0d1b5b3964"));
}

#[test]
fn from_terminfo_sgr_path_with_charset() {
    // vt220 has sgr with a `\e(B` charset reset embedded; the sgr path emits tparm(sgr, ...) per row
    // (msgr present, so no reset around the vpa moves). Captured from real ncurses-on-vt220.
    let out = attr_first_paint("vt220");
    assert_eq!(out, unhex("1b5b481b5b4a1b5b31421b5b303b316d1b2842626f6c640d1b5b31421b5b303b346d1b2842756e646c0d1b5b31421b5b303b376d1b2842726576730d1b5b31421b5b303b376d1b284273746e640d1b5b31421b5b303b356d1b2842626c6e6b0d1b5b31421b5b306d1b284264696d6d0d1b5b31421b5b6d1b28426e6f726d0d1b5b3142"));
}

/// First paint of a two-pair color scenario, mirroring the oracle (blank refresh; clearok; paint) so
/// the cursor state matches across engines. Golden bytes captured from real ncurses.
fn color_first_paint(fixture: &str) -> Vec<u8> {
    let bytes = std::fs::read(format!("tests/terminfo/{fixture}")).expect("read fixture");
    let t = Terminfo::parse(&bytes).expect("parse terminfo");
    let mut s = Screen::from_terminfo(ROWS, COLS, &t, Palette::new());
    s.start_color();
    s.init_pair(1, 1, 4); // red on blue
    s.init_pair(2, 2, 0); // green on black
    let mut g = vec![Cell::plain(' ', 0); (ROWS * COLS) as usize];
    let putp = |g: &mut Vec<Cell>, r: i32, st: &str, pair: u32| {
        let a = (pair << 8) & attrs::COLOR;
        for (i, ch) in st.chars().enumerate() {
            g[(r * COLS + i as i32) as usize] = Cell::plain(ch, a);
        }
    };
    putp(&mut g, 1, "red", 1);
    putp(&mut g, 2, "grn", 2);
    putp(&mut g, 3, "def", 0);
    let blank = vec![Cell::plain(' ', 0); (ROWS * COLS) as usize];
    let _ = s.doupdate(&blank, 0, 0);
    s.clearok();
    s.doupdate(&g, 4, 0)
}

#[test]
fn from_terminfo_color_setaf_blankfill_corner() {
    // sun-color: setaf/setab, am, no bce -> ncurses can't fast-clear (the clear wouldn't paint the
    // bg), so it emits the color-init then blank-fills, omitting the bottom-right corner cell (no
    // smam/ich). The pairs paint via setaf/setab. Captured from real ncurses-on-sun-color.
    let out = color_first_paint("sun-color");
    assert!(
        out.windows(5).any(|w| w == b"\x1b[44m"),
        "setab(blue) for pair 1"
    );
    assert_eq!(out, unhex("1b5b306d1b5b33376d1b5b34306d20202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020202020200820081b5b3140201b5b481b5b32641b5b33316d1b5b34346d7265640d1b5b33641b5b33326d1b5b34306d67726e0d1b5b34641b5b6d1b5b306d1b5b33376d1b5b34306d6465660d1b5b3564"));
}

#[test]
fn from_terminfo_color_setf_toggles_colors() {
    // emots has no setaf -- only `setf`/`setb` (SVr4 BGR ordering). ncurses applies toggled_colors
    // (1<->4, 3<->6) to those caps, so pair 1 (red=1) emits setf with the toggled code 4 etc.
    // Captured from real ncurses-on-emots.
    let out = color_first_paint("emots");
    assert_eq!(out, unhex("1b5b3f3b6d1b5b3f366d1b5b3f3b306d1b5b481b5b4a1b5b31421b5b3f336d1b5b3f3b316d7265640d1b5b31421b5b3f316d1b5b3f3b306d67726e0d1b5b31421b5b6d1b5b31306d1b5b3f3b6d1b5b3f366d1b5b3f3b306d6465660d1b5b3142"));
}

#[test]
fn from_terminfo_no_color_emits_no_color() {
    // vt100 has no color: start_color is a no-op for emission. The scenario paints plain text (no
    // setaf/setab/op anywhere) -- the crate must not emit hardcoded color. Captured from real ncurses.
    let out = color_first_paint("vt100");
    // No color caps anywhere: no setaf (`\e[3.m`), setab (`\e[4.m`), or op (`\e[39;49m`).
    assert!(
        !out.windows(5).any(|w| w == b"\x1b[39;"),
        "no orig_pair on a colorless terminal"
    );
    assert_eq!(
        out,
        b"\x1b[H\x1b[J\x1b[1Bred\r\x1b[1Bgrn\r\x1b[1B\x1b[m\x0fdef\r\x1b[1B"
    );
}

#[test]
fn from_terminfo_color_xenl_blankfill_cups_each_row() {
    // color_xterm has color, am, xenl, and no bce: color+no-bce forces a blank-fill (no \e[2J), and
    // because of the magic margin (xenl) the cursor goes to hyperspace after each row's last column,
    // so every row is repositioned with an absolute cup (\e[N;1H) rather than relying on am wrap.
    let out = color_first_paint("color_xterm");
    assert!(
        !out.windows(4).any(|w| w == b"\x1b[2J"),
        "color+no-bce blank-fills, no clear_screen"
    );
    let row_cups = (2..=24)
        .filter(|n| {
            let s = format!("\x1b[{n};1H");
            out.windows(s.len()).any(|w| w == s.as_bytes())
        })
        .count();
    assert!(
        row_cups >= 20,
        "xenl blank-fill cups each row, got {row_cups}"
    );
}

#[test]
fn from_terminfo_color_no_msgr_resets_around_moves() {
    // at386 has color but no msgr: ncurses' AttrOf includes A_COLOR, so a color-only cell is reset to
    // A_NORMAL (sgr0 `\e[0;10m` + orig_pair) before each cursor move and re-applied after -- exactly
    // the no-msgr attribute behaviour, extended to color. Captured from real ncurses.
    let out = color_first_paint("at386");
    let resets = out.windows(7).filter(|w| w == b"\x1b[0;10m").count();
    assert!(
        resets >= 3,
        "color state reset before each move (no msgr), got {resets}"
    );
}

#[test]
fn from_terminfo_color_set_color_pair() {
    // d430-unix-ccc has no setaf/setab -- it selects a pair directly with `set_color_pair` (scp =
    // `\036RG2%p1%02X`). ncurses' _nc_do_color emits tparm(scp, pair) and returns (no fg/bg), and
    // pair 0 just emits orig_pair. The crate must use scp, not a hardcoded `\e[3Xm`. Captured from
    // real ncurses-on-d430-unix-ccc.
    let out = color_first_paint("d430-unix-ccc");
    assert!(
        out.windows(5).any(|w| w == b"\x1eRG20"),
        "scp (set_color_pair) used for the pairs"
    );
    assert!(
        !out.windows(3).any(|w| w == b"\x1b[3"),
        "no hardcoded ANSI setaf on a non-CSI terminal"
    );
    assert_eq!(out, unhex("1e524634383331411e524632453331421e524631443331431e524633463331441e46451e50421e52473230317265640d1e50421e524732303267726e0d1e50421e504a151d1e451e465330301e524634383331411e524632453331421e524631443331431e524633463331446465660d1e5042"));
}

#[test]
fn from_terminfo_xterm_doupdate_matches_default() {
    let bytes = std::fs::read("tests/terminfo/xterm").expect("read fixture");
    let t = Terminfo::parse(&bytes).expect("parse terminfo");
    let (cells, cy, cx) = scenario();

    let mut derived = Screen::from_terminfo(ROWS, COLS, &t, Palette::new());
    let mut reference = Screen::new(ROWS, COLS);

    // First paint and a follow-up diff must both be byte-identical to the hardcoded xterm engine.
    assert_eq!(
        derived.doupdate(&cells, cy, cx),
        reference.doupdate(&cells, cy, cx),
        "from_terminfo(xterm) first paint diverged from Screen::new"
    );
    let mut cells2 = cells.clone();
    for (i, ch) in "HELLO".chars().enumerate() {
        cells2[(2 * COLS + 5 + i as i32) as usize] = Cell::plain(ch, 0);
    }
    assert_eq!(
        derived.doupdate(&cells2, cy, cx),
        reference.doupdate(&cells2, cy, cx),
        "from_terminfo(xterm) diff diverged from Screen::new"
    );
}

#[test]
fn from_terminfo_ampex219_pads_clr_eos() {
    // ampex219 has no xon and a padded clr_eos (ed = `\e[J$<50>`). When an incremental update clears
    // a trailing region that previously held content, ClrBottom emits the ed bytes followed by
    // floor(50*38400/9000)=213 NUL pad bytes (the tputs padding), matching real ncurses.
    let bytes = std::fs::read("tests/terminfo/ampex219").expect("read fixture");
    let t = Terminfo::parse(&bytes).expect("parse");
    let mut s = Screen::from_terminfo(ROWS, COLS, &t, Palette::new());
    // Phase 0: fill rows 0..16 so curscr has content in the lower region.
    let mut g1 = vec![Cell::plain(' ', 0); (ROWS * COLS) as usize];
    for r in 0..16 {
        for (i, ch) in "line".chars().enumerate() {
            g1[(r * COLS + i as i32) as usize] = Cell::plain(ch, 0);
        }
    }
    let _ = s.doupdate(&g1, 15, 4);
    // Phase 1: keep only rows 0..3; rows 3..23 become blank -> ClrBottom emits a padded clr_eos.
    let mut g2 = vec![Cell::plain(' ', 0); (ROWS * COLS) as usize];
    for r in 0..3 {
        for (i, ch) in "line".chars().enumerate() {
            g2[(r * COLS + i as i32) as usize] = Cell::plain(ch, 0);
        }
    }
    let out = s.doupdate(&g2, 2, 4);
    let pos = out
        .windows(3)
        .position(|w| w == b"\x1b[J")
        .expect("clr_eos present in diff");
    let pads = out[pos + 3..].iter().take_while(|&&b| b == 0).count();
    assert_eq!(pads, 213, "clr_eos pad bytes");
}
