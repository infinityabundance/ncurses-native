# Whole-database byte-parity coverage sweep

This is the reproducible form of the two breadth numbers quoted in the top-level
`README.md` and `docs/gap-ledger.md`:

* **mvcur** — byte-exact on **99.84%** of the compiled terminfo database
  (2558/2562 checked).
* **doupdate** first paint — byte-exact on **99.26%** (2543/2562 checked).
* **doupdate** incremental scene-to-scene diff — byte-exact on **98.95%**
  (2535/2562 checked).
* **attribute/SGR** first paint — byte-exact on **91.5%** (set_attributes vs
  individual mode caps, padding, msgr).
* **color** first paint — byte-exact on **97.9%** of color-capable terminals
  (setaf/setab or setf/setb, orig_pair, color-init, color+no-bce blank-fill).

It compares the crate's two terminal-general engines to **real ncurses** under an
80×24 pty, for *every* terminal in a compiled terminfo database.

## What it is (and isn't)

The terminfo database is ephemeral on the verification host (tic-compiles a
couple thousand entries into a temp dir), so this is *tooling*, not a committed
court — it cannot be re-run offline without the database. The specific systematic
behaviours the sweep discovered are pinned offline, forever, by the committed
fixture tests:

* `tests/mvcur_terminfo.rs` — `bw` back-wrap, `cursor_to_ll`, the `xenl`
  magic-margin gate, the `cup`-gated `NOT_LOCAL` short-circuit (`pmcons`), the
  newline-`cud1` rejection (`ncr160wy60pp`), the general `bw` back-wrap to any
  column (`apple2e`, `hp2626-ns`).
* `tests/doupdate_terminfo.rs` — the `clear`-less `ClearScreen` fallbacks:
  clr_eos-only (`newhp`), clr_eol-per-line (`avatar`), blank-fill (`gt40`), the
  padded clr_eos (`ampex219`); and the incremental TransformLine fixes: bce
  trailing-clear (`arm100`), terminfo-derived `rep` (`concept`), direct leading
  reposition (`hp2645`).

## Running

```sh
# 1. Compile a terminfo database (any directory tic writes into):
tic -x -o /tmp/tinfo /path/to/terminfo.src

# 2. Sweep all engines (oracle binaries are built on first use):
python3 tools/db-coverage/sweep.py /tmp/tinfo all

# Or one engine (mvcur | doupdate | incremental | attr | color), optionally limited to N terminals:
python3 tools/db-coverage/sweep.py /tmp/tinfo color 250
```

Both the crate (`examples/mvcur_db.rs`, `examples/doupdate_db.rs`) and real
ncurses resolve the same `TERMINFO=/tmp/tinfo`, so the comparison is apples to
apples.

## How the oracles avoid contamination

* `mvcur_oracle.c` brackets each `mvcur(3)` call with a sentinel
  `0xff 0xfe 0xfd` + 4-byte index. Those high bytes never occur in cursor-motion
  output, so the mark cannot collide with a legitimate value byte (a low-byte
  mark silently miscounts non-CSI `cup` terminals whose column/row 1 emits
  `\x01`). No `endwin()` is called, so teardown never pollutes the last segment.
* `doupdate_oracle.c` writes its sentinel with raw `write(1,...)` *after*
  `refresh()` has flushed ncurses' buffer (a stdio mark would reorder, since
  ncurses bypasses stdio), and uses `clearok(curscr, TRUE)` so the measured
  refresh emits a full clear + repaint — matching the crate's first `doupdate`.

The remaining ~0.2% (mvcur) / ~0.7% (doupdate) are a degenerate exotic tail:
malformed fixed-string caps (`apollo`), Tektronix graphics terminals where real
ncurses itself emits nothing (`tek4113`), and unaddressable hardcopy/dumb
terminals (`dumb`, `ep4x`, `att53xx`, hardware-tab `viewdata`) that would need
exact auto-margin-wrap quirk modelling for near-zero practical gain.
