# Terminal ecology

A core discipline of `ncurses-native`: **never treat one terminal as all
terminals.** The terminfo reader and loader are checked against ncurses across a
spread of real database entries, so the substrate is shown to generalize beyond
the single admitted xterm byte seed -- without yet claiming byte parity for those
terminals' *runtime output*.

## Court: NCURSES.TERMINFO.ECOLOGY

For each terminal below, the crate loads the entry by name (`Terminfo::load`,
`TERMINFO` pinned to `/usr/share/terminfo`) and dumps every standard capability;
the same dump is produced by ncurses `setupterm` + `tigetflag`/`tigetnum`/
`tigetstr`. The two are compared cap-for-cap.

| TERM | compiled magic | caps | result |
|---|---|---|---|
| xterm | `0o432` (16-bit nums) | 497 | match |
| xterm-256color | `0o1036` (32-bit nums) | 497 | match |
| linux | `0o432` | 497 | file-faithful* |
| screen | `0o432` | 497 | match |
| tmux | `0o432` | 497 | match |
| tmux-256color | `0o1036` | 497 | match |
| vt100 | `0o432` | 497 | match |
| ansi | `0o432` | 497 | match |
| rxvt | `0o432` | 497 | match |

Both compiled formats are exercised: the legacy 16-bit-number layout and the
32-bit-number layout (`0o1036`) used by the 256-color entries.

## The one recorded divergence (and why it is not a bug)

`*` For `linux`, the crate reports `cols`/`lines` as absent (`-1`) while ncurses
reports `80`/`24`. The `linux` console entry omits those numeric caps, so the
reader is faithfully reporting the file. ncurses' `setupterm` *augments* them at
runtime from the terminal size (the `use_env` behavior, via `$LINES`/`$COLUMNS`
or `TIOCGWINSZ`). That is a `setupterm` runtime behavior the file reader
deliberately does not model; it is recorded per-term in the receipt with an
explicit reason. Every other capability of every terminal matches byte-for-byte.

## Non-claims

- This court is about **terminfo reading**, not runtime terminal output. The byte
  courts (init/cursor/color/erase) remain admitted for **xterm only**.
- **Extended / user-defined capabilities** are not parsed.
- `setupterm`'s runtime work beyond load+parse (global `cur_term`, baud, modes,
  `use_env` size augmentation) is not modeled.
