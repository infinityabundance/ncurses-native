# ncurses-native

![ncurses-native rendering colour, SGR attributes and cursor-positioned text — byte-for-byte the same escape stream as ncurses](https://raw.githubusercontent.com/infinityabundance/ncurses-native/main/assets/ncurses-native.png)

A clean-room, dependency-free Rust reproduction of the **observable terminal byte
output** of ncurses 6.4. The output engines are terminfo-driven and verified
byte-for-byte against real ncurses across the **entire compiled terminfo database —
2,563 terminal types**, not just xterm. Reverse-engineered from captured behaviour,
**not** ported from ncurses C source — there is zero ncurses C source in this repo.

`#![forbid(unsafe_code)]`, std-only, no dependencies, ASCII-only source.

## Byte-parity (verified across the whole terminfo database)

The output engines are measured byte-for-byte against real ncurses across the entire
**2,563-entry** compiled terminfo database (`tools/db-coverage/sweep.py`, run live —
not a cached number):

| engine | byte-exact across the DB |
|---|---|
| cursor-movement optimizer (`mvcur`) | **2559 / 2563 — 99.84%** |
| `doupdate` (full screen update) | **2549 / 2563 — 99.45%** |
| incremental update | **2536 / 2563 — 98.95%** |
| colour (`setaf`/`setab`, toggled, bce) | **2535 / 2563 — 98.91%** |
| SGR attributes (`sgr`/mode-caps) | **2345 / 2563 — 91.49%** |

The `infocmp` decompiler and `tic` compiler are **100.000% byte-exact** across the full
database (both `-1` and `-1 -x`). A real curses C program (`initscr`/`init_pair`/
`mvaddstr`/`attron`/windows/pads/panel/menu/form/`getch`) linked against the native
C-ABI (`crates/ncurses-cabi`, `libncurses_cabi.{so,a}`) draws **byte-identically** to the
system `libncursesw`, and the cdylib is a working `libtinfo.so.6` drop-in.

The remaining tails are understood and bounded (the SGR tail is dominated by the
obsolete magic-cookie `xmc` terminals); see [`docs/capabilities.md`](docs/capabilities.md).

## Use it

```toml
[dependencies]
ncurses-native = "0.1"
```

```rust
use ncurses_native::{mvcur, sgr_on, sgr_off, Attr};

// The cursor-movement optimizer picks the cheapest escape sequence, like ncurses.
let bytes = mvcur((0, 0), (5, 10));            // -> b"\x1b[5;10H"
std::io::Write::write_all(&mut std::io::stdout(), &bytes).unwrap();

// SGR attributes:
print!("{}bold{}", String::from_utf8_lossy(&sgr_on(Attr::Bold)),
                    String::from_utf8_lossy(&sgr_off()));
```

Higher-level surfaces are available too: `Window` (cells/attrs/geometry), `Screen`
(`doupdate`-style updates), `Terminfo` (`tigetstr`/`tparm`/`tputs`/`tgoto`), colour and
key decoding. The macro-style C API maps to methods (`getyx` → `win.getyx()`).

## Scope and bounds (honest non-claims)

This is a **seed grown toward parity**, not a finished library, and **not a replacement**
for ncurses:

- The terminfo-driven engines (cursor optimizer, `doupdate`, colour, attributes) and the
  `infocmp`/`tic` tools are verified against the **admitted ncurses 6.4** oracle across the
  **whole 2,563-entry terminfo database** — thousands of terminal types — at the percentages
  in the table above (`tools/db-coverage/sweep.py`, re-runnable). The remaining few-percent
  tails are enumerated per-terminal, not hidden.
- The one xterm-specific piece is the **live-pty composite framing** capture (`initscr`…`endwin`
  whole-stream), pinned to `TERM=xterm`, 80×24, `C.UTF-8` — proven by the receipts under
  `reports/oracle/`. Other ncurses *builds* are a non-claim.
- It reproduces terminal **output**; functions with no terminal-output contract (interactive
  input reads, pure queries, global state) are classified, not byte-claimed.
- Wide-char/double-width breadth is in progress.

## Documentation

- [`docs/capabilities.md`](docs/capabilities.md) — full court-backed capability detail, the
  C-ABI/panel/menu/form/tools story, and the parity bounds.
- [`docs/gap-ledger.md`](docs/gap-ledger.md) — the forensic gap ledger (every gap vs ncurses).
- [`docs/generated/port-parity.md`](docs/generated/port-parity.md) and
  [`port-parity-functions.md`](docs/generated/port-parity-functions.md) — the generated,
  freshness-gated public-C-API parity matrices (`cargo xtask check` enforces freshness).
- [`docs/generated/claim-index.md`](docs/generated/claim-index.md) — courts and receipts.

## License

Apache-2.0. See [`LICENSE`](LICENSE).
