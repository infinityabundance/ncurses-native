//! Oracle court fixture: emit one named byte artifact from the crate's public
//! API to stdout, so the oracle harness (`tools/oracle-runner/`) can capture and
//! hash the *executed* Rust output and compare it to the C/terminfo oracle.
//!
//! Usage: `cargo run --quiet --example dump -- <court-id>`
//!
//! Each id corresponds to a court in `reports/oracle/`. This binary writes raw
//! bytes (no trailing newline) so the captured hash is the artifact itself.

use std::io::Write;

use ncurses_native::{
    beep, clear, clr_bol, clr_eol, clr_eos, flash, mvcur, sgr_bg, sgr_fg, sgr_off, sgr_on, Attr,
    INIT_PROLOGUE, TEARDOWN_EPILOGUE,
};

fn main() {
    let id = std::env::args().nth(1).unwrap_or_default();
    let out: Vec<u8> = match id.as_str() {
        // --- terminfo string-capability courts (compared to infocmp/tput) ---
        "cap.clear" => clear().to_vec(),
        "cap.ed" => clr_eos().to_vec(),
        "cap.el" => clr_eol().to_vec(),
        "cap.el1" => clr_bol().to_vec(),
        "cap.setaf.2" => sgr_fg(2),
        "cap.setab.4" => sgr_bg(4),
        "cap.setaf.7" => sgr_fg(7),
        "cap.setab.0" => sgr_bg(0),
        // cursor-optimizer outputs that reduce to a single terminfo cap:
        "cap.cup" => mvcur((1, 1), (3, 5)), // -> CUP \e[3;5H
        "cap.cuu1" => mvcur((3, 1), (2, 1)), // -> cuu1 \e[A
        "cap.home" => mvcur((10, 40), (1, 1)), // -> home \e[H
        // --- bell courts ---
        "cap.bel" => beep().to_vec(),
        "cap.flash" => flash().to_vec(),
        // --- SGR attribute courts ---
        "attr.bold.on" => sgr_on(Attr::Bold),
        "attr.off" => sgr_off(),
        // --- composite framing courts (compared to a live ncurses pty stream) ---
        "frame.init" => INIT_PROLOGUE.to_vec(),
        "frame.teardown" => TEARDOWN_EPILOGUE.to_vec(),
        "frame.full" => {
            let mut v = INIT_PROLOGUE.to_vec();
            v.extend_from_slice(TEARDOWN_EPILOGUE);
            v
        }
        other => {
            eprintln!("dump: unknown court id {other:?}");
            std::process::exit(2);
        }
    };
    std::io::stdout().write_all(&out).expect("write stdout");
}
