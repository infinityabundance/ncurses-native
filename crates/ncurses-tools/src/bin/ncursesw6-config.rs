//! `ncursesw6-config` -- the build-flag query tool that downstream `./configure`s and Makefiles use
//! to discover how to compile and link against ncurses, reproduced byte-identically to the system
//! `ncursesw6-config(1)`.
//!
//! It is name-aware: invoked as `ncurses6-config` it reports the narrow library (`-lncurses`),
//! otherwise the wide one (`-lncursesw`) -- exactly as ncurses ships one script per flavour. Path
//! and library values mirror ncurses' compiled-in defaults (overridable by `$TERMINFO` /
//! `$TERMINFO_DIRS` for the terminfo queries), so the crate is a drop-in for build systems.

use std::process::exit;

// ncurses' compiled-in defaults (the values its own `*-config` echoes on a standard install).
const VERSION: &str = "6.4.20240113";
const ABI_VERSION: &str = "6";
const MOUSE_VERSION: &str = "2";
const PREFIX: &str = "/usr";
const EXEC_PREFIX: &str = "/usr";
const BINDIR: &str = "/usr/bin";
const DATADIR: &str = "/usr/share";
const MANDIR: &str = "/usr/share/man";
const CFLAGS: &str = "-D_DEFAULT_SOURCE -D_XOPEN_SOURCE=600";
const DEFAULT_TERMINFO: &str = "/etc/terminfo";
const DEFAULT_TERMINFO_DIRS: &str = "/etc/terminfo:/lib/terminfo:/usr/share/terminfo";

fn usage(prog: &str) -> ! {
    eprintln!(
        "Usage: {prog} [options]\n\n\
         Options:\n  \
         --prefix --exec-prefix --cflags --libs --libs-only-L --libs-only-l --libs-only-other\n  \
         --version --abi-version --mouse-version --bindir --datadir --includedir --libdir\n  \
         --mandir --terminfo --terminfo-dirs --termpath"
    );
    exit(1);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let prog = args
        .first()
        .map(|p| p.rsplit('/').next().unwrap_or(p).to_string())
        .unwrap_or_else(|| "ncursesw6-config".to_string());
    // The narrow flavour links `-lncurses`; everything else `-lncursesw`.
    let wide = !prog.starts_with("ncurses6-config");
    let lib = if wide { "-lncursesw" } else { "-lncurses" };
    let libs = format!("{lib} -ltinfo");

    if args.len() < 2 {
        usage(&prog);
    }

    // ncurses echoes one line per recognised option, in argument order -- a blank line for an empty
    // value, except `--libdir`, which echoes nothing at all when empty (the lone quirk).
    let mut out: Vec<(bool, String)> = Vec::new(); // (suppress-if-empty, value)
    for opt in &args[1..] {
        let suppress_if_empty = opt == "--libdir";
        let line = match opt.as_str() {
            "--prefix" => PREFIX.to_string(),
            "--exec-prefix" => EXEC_PREFIX.to_string(),
            "--cflags" => CFLAGS.to_string(),
            "--libs" => libs.clone(),
            "--libs-only-L" => String::new(),
            "--libs-only-l" => libs.clone(),
            "--libs-only-other" => String::new(),
            "--version" => VERSION.to_string(),
            "--abi-version" => ABI_VERSION.to_string(),
            "--mouse-version" => MOUSE_VERSION.to_string(),
            "--bindir" => BINDIR.to_string(),
            "--datadir" => DATADIR.to_string(),
            // Headers/libs live on the default search path, so no -I/-L is needed.
            "--includedir" => String::new(),
            "--libdir" => String::new(),
            "--mandir" => MANDIR.to_string(),
            "--terminfo" => std::env::var("TERMINFO").unwrap_or_else(|_| DEFAULT_TERMINFO.to_string()),
            "--terminfo-dirs" => {
                std::env::var("TERMINFO_DIRS").unwrap_or_else(|_| DEFAULT_TERMINFO_DIRS.to_string())
            }
            "--termpath" => std::env::var("TERMPATH").unwrap_or_default(),
            "--help" | "-h" => usage(&prog),
            _ => usage(&prog),
        };
        out.push((suppress_if_empty, line));
    }
    for (suppress_if_empty, line) in out {
        if !(suppress_if_empty && line.is_empty()) {
            println!("{line}");
        }
    }
}
