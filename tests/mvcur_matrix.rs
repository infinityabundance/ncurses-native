//! Byte-exact replay of the captured ncurses `mvcur` move matrix.
//!
//! `tests/fixtures/mvcur_matrix.txt` is the full 153x153 = 23 409-pair `(from -> to)` cursor-move
//! matrix recorded from libncurses 6.4 on an 80x24 xterm pty (see the file header for provenance).
//! This test replays every pair through [`ncurses_native::mvcur`] and asserts the crate reproduces
//! ncurses's exact bytes for all of them. It needs no terminal and no ncurses at runtime -- it is a
//! frozen oracle, so the parity claim is verified on every `cargo test`.

use ncurses_native::mvcur;

/// Decode a lowercase-hex byte string (possibly empty) into bytes.
fn unhex(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    assert!(b.len() % 2 == 0, "odd hex length: {s:?}");
    (0..b.len() / 2)
        .map(|i| {
            let hi = (b[2 * i] as char).to_digit(16).expect("hex") as u8;
            let lo = (b[2 * i + 1] as char).to_digit(16).expect("hex") as u8;
            (hi << 4) | lo
        })
        .collect()
}

#[test]
fn mvcur_matches_ncurses_for_every_captured_pair() {
    let fixture = include_str!("fixtures/mvcur_matrix.txt");
    let mut total = 0usize;
    let mut mismatches: Vec<String> = Vec::new();

    for line in fixture.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, hex) = line.split_once('=').expect("k=v");
        let mut it = key.split(',').map(|n| n.parse::<i32>().expect("coord"));
        // Fixture coordinates are 0-based (ncurses-internal); the public API is 1-based.
        let fy = it.next().unwrap() + 1;
        let fx = it.next().unwrap() + 1;
        let ty = it.next().unwrap() + 1;
        let tx = it.next().unwrap() + 1;

        let expected = unhex(hex);
        let got = mvcur((fy, fx), (ty, tx));
        total += 1;
        if got != expected && mismatches.len() < 20 {
            mismatches.push(format!(
                "({},{})->({},{}): expected {:?} got {:?}",
                fy - 1,
                fx - 1,
                ty - 1,
                tx - 1,
                String::from_utf8_lossy(&expected),
                String::from_utf8_lossy(&got),
            ));
        }
    }

    assert_eq!(total, 23_409, "fixture pair count drifted");
    assert!(
        mismatches.is_empty(),
        "{} of {} pairs diverge from ncurses:\n{}",
        mismatches.len(),
        total,
        mismatches.join("\n")
    );
}
