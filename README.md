# ncurses-native

A clean-room, dependency-free Rust reproduction of the **observable terminal byte
output** of ncurses 6.6 on one terminal: `TERM=xterm`.

## What this actually is (read this first)

This crate is **not** ncurses, and it is **not** a port of any ncurses C source.
It is a from-scratch Rust reproduction of the *bytes ncurses writes to the
terminal* -- the escape-sequence stream you would capture if you ran a program
against `libncursesw.so.6` (ncurses 6.6) with `TERM=xterm` and recorded the
output under a pty. It was reverse-engineered from behavior (captured facts), not
copied from a source tree. There is **zero ncurses C source** in this repository.

By surface area it reproduces roughly **3-5% of ncurses-the-library**. It is a
**SEED**: the byte-exact nucleus, meant to be grown toward parity. It is **not a
replacement** for ncurses.

The crate is `#![forbid(unsafe_code)]`, std-only, has **no dependencies**, and is
ASCII-only in source.

## What IS reproduced

For the admitted terminal (`TERM=xterm`, ncurses 6.6) the emitted bytes are
byte-identical to ncurses for:

- **The cursor-movement cost optimizer** (`cursor::mvcur`) -- ncurses's
  `relative_move` strategy enumeration: vertical moves (`VPA \e[<r>d`, `cuu1
  \e[A`), horizontal moves (space-fill, `HPA \e[<c>G` for short 5..=7 column
  advances, backspaces), carriage-return-to-column-1, `home \e[H` for `(1,1)`,
  and direct `CUP \e[<r>;<c>H` -- shortest-wins with the local/CR/home-before-CUP
  tie-break.
- **SGR attributes** (`attr`) -- bold/dim/underline/blink/reverse on-and-off
  byte sequences.
- **ANSI color and color pairs** (`color`) -- foreground/background SGR for the
  8 ANSI colors and the free default pair (white-on-black, pair 0).
- **Screen framing** (`term`) -- the `smcup`/`rmcup` init prologue and teardown
  epilogue.
- **Erase operations** (`screen`) -- `clear`, `clr_eos`, `clr_eol`, `clr_bol`.
- **Single-field repaint** (`screen::single_field_repaint`) -- the `wclear` +
  top-down `TransformLine` paint of one static field on a blank screen.

## What is NOT yet reproduced (the parity roadmap / non-claims)

This is the honest list of everything a real curses needs that this seed does
**not** do yet:

- **terminfo database parsing.** Exactly one terminal is hardcoded (`xterm`,
  ncurses 6.6). Any other `TERM` or any other ncurses build emits different
  bytes; reproducing them needs a terminfo reader. That is the largest TODO.
- **The full `doupdate` / `TransformLine` line-diff** for arbitrary screen
  deltas. We reproduce one shape (a single static field on an otherwise blank
  screen); the general line-by-line diff/repaint of an arbitrary previous->next
  screen is not done.
- **Input handling** -- `getch`, `keypad`, mouse decoding, timeouts.
- **`WINDOW` / `SCREEN` / pad structures** -- there is no window model; the API
  speaks directly in terminal bytes.
- **panels, menus, forms** -- none of the higher libraries.
- **Terminals other than xterm**, and **ncurses builds other than 6.6**.

## License

Apache-2.0. See `LICENSE`.
