# The ncurses → ncurses-native forensic gap ledger

> **Purpose.** A timeless, information-complete record of *every* gap between the
> original ncurses (C) and `ncurses-native` (native Rust). Each gap is written as
> a diff: what ncurses does, what ncurses-native does (or cannot do), the gap
> class, severity, evidence, and what true 1:1 parity would require. Nothing is
> hidden. Where a surface is reconstructed, it is cited to an oracle court; where
> it diverges, the divergence is recorded; where it cannot be reconstructed in
> safe Rust, the boundary is named.
>
> This document was produced with an elite four-specialist research panel
> (C→Rust language/runtime semantics; ncurses feature surfaces incl. lifecycle,
> input, mouse, slk, wide-char, panel/menu/form; byte-level optimizer/doupdate;
> ABI/packaging/security/ecology/meta), cross-checked against the
> invisible-island.net man pages (ncurses 6.6), the Rust reference/nomicon, the
> ncurses CVE record, and this repo's clang C inventory vs `syn` Rust inventory
> (the parity "compass") and oracle receipts.

## How to read this ledger

Resolution status (the project's own vocabulary) distinguishes three *different*
claims that outsiders routinely conflate — keep them separate:

- **reimplemented / reconstructed** = real Rust that emits/derives the bytes or
  reconstructs the state, **and** is pinned by an oracle court (`full` / `partial`
  / `scaffold` / `divergent`).
- **classified** = accounted for with evidence but *not* reconstructed
  (`deferred` = proven to emit nothing now; `n_a` = no terminal-output contract).
- **gap** = anything below byte-exact `full`. **449 / 481 public functions are
  gaps of some degree** (only 32 are `full`). The complete per-function diff is
  the generated, freshness-gated companion
  [`docs/generated/gap-ledger-functions.md`](generated/gap-ledger-functions.md).

The two halves of the ledger:

1. **Function-level gaps** — generated from the parity model (see the companion
   file). Every non-`full` function, its kind, counterpart, and court.
2. **Class-level gaps** — this document: the behavioural, semantic, byte-level,
   C→Rust language, ABI, packaging, security, ecology, and methodology gap
   *classes* a per-function table structurally cannot see.

Severity legend: **S0** blocks "drop-in / full parity" in principle · **S1**
major behavioural/byte divergence · **S2** edge-case / partial · **S3**
quality/meta. Status: **open** (not addressed) · **recorded** (court-pinned
divergence) · **boundary** (needs the unsafe `ncurses-terminal` crate) ·
**partial** (some of it reconstructed).

---

## 0. Scope and headline structural gaps — all in scope, built natively

**Project decision (the directive):** *we build all of it natively, creating
what does not yet exist in native Rust* — including the surfaces a per-function
table can't see and including a **native C-ABI surface** so ncurses-native can be
a real drop-in, not only a crate for Rust code. Nothing here is treated as "out
of scope" or "impossible"; each is a native-build work item with a concrete path.
The only thing we do *not* do is reintroduce C's memory-unsafety/UB footguns
(MEM-04/05, UB-01/02/06): we keep the safe behaviour and *record* that as an
intentional, documented divergence — that is strictly-better parity, not a gap.

- **STRUCT-01 · Ship a C shared object (`cdylib`/`staticlib`). ✅ STARTED (tinfo slice
  drop-in-proven).** ncurses ships `libncursesw.so.6`/`.a`. native builds
  `crates/ncurses-cabi` with `crate-type=["cdylib","staticlib","lib"]`, producing real
  `libncurses_cabi.{so,a}`. The **low-level terminfo layer is wired and proven a drop-in**:
  `setupterm`, `tigetstr`, `tigetnum`, `tigetflag`, `tputs`, `putp`, `tgoto`, `tparm` are
  `#[no_mangle] extern "C"` over the byte-exact core, with a thread-local `cur_term`
  (GLB-01) owning the returned strings. Court `NCURSES.CABI` links **one C program twice**
  — against `libncurses_cabi.a` and the system `libtinfo` — and the stdout is
  **byte-identical**. The **curses drawing path is also wired and proven, including multiple
  windows**: `initscr`/`endwin`/`refresh`/`doupdate`, `newwin`/`delwin`, `move`/`addstr`/
  `addch`/`mvaddstr` and the `w*` family (`wmove`/`waddstr`/`mvwaddstr`/`wattron`/`wattroff`/
  `wattrset`/`werase`), `wnoutrefresh`/`wrefresh`, `attron`/`attroff`/`attrset`,
  `start_color`/`init_pair` — over heap `WINDOW` cell grids composited into the virtual screen
  (`newscr`) + the byte-exact `doupdate`. Court `NCURSES.CURSES` links a real curses C program
  (stdscr + a `newwin` subwindow with bold/color) against `libncurses_cabi.a` vs `libncursesw`
  and the **screen-painting body is byte-identical**. **Interactive input is also wired**:
  `cbreak`/`raw`/`noecho`/`keypad`/`getch` over the `ncurses-terminal` raw-mode reader return
  the same keycodes as a real ncurses getch loop (court `NCURSES.CABI.GETCH`) -- a complete
  input+output curses program runs on the native `.so`. **Pads** are wired too
  (`newpad`/`pnoutrefresh`/`prefresh`: a pad rectangle is composited onto `newscr`, byte-exact in
  court `NCURSES.CURSES`). A C header (`crates/ncurses-cabi/include/tinfo.h`) declares the tinfo
  symbols. **Still open:** the full `curses.h` (incl. the CPP macro API) and the `ESCDELAY` timer.
  **S0→S1, in progress.**
- **STRUCT-02 · Provide the C ABI explicitly (not "impossible", just hand-built).**
  Rust has no *automatic* stable ABI, but a C ABI is produced deliberately: `#[repr(C)]`
  public structs (`MEVENT` ✅ defined; `cchar_t` pending), opaque handle types for
  `WINDOW`/`SCREEN`, the `extern "C"` calling convention (✅ for the tinfo slice, proven by
  `NCURSES.CABI`), and a linker **version script** (`.symver`) to reproduce the
  soname/symbol-versioning nodes (`NCURSES6_TIC_5.x`, `NCURSES_TINFO_5.x`). The variadic
  `tparm(...)` C ABI (**ABI-VAR-01**) is done (numeric); the version script is the next piece. **S0→S1,
  in progress.**
- **STRUCT-03 · Build the whole API surface natively.** Grow the denominator by
  reconstructing, in Rust: the **macro API** both as ergonomic methods
  (`win.getyx()`) *and* as C macros in the generated `curses.h`; **panel / menu /
  form** as native modules/crates; the **wide-char** (ncursesw) API as a native
  module; the **CLI tools** (`tput`/`tic`/`infocmp`/`clear`/`toe`/`tset`/…) as
  native Rust binaries (**`tput`/`clear`/`infocmp`/`tic` done** and byte-exact, see
  BLD-03; `toe`/`tset`/… remain); and the `_nc_*` de-facto-ABI behaviour via native
  internals (re-exported under the same names through the C-ABI crate where real
  programs depend on them). **S0, open (native build).**
- **STRUCT-04 · Build full fidelity breadth natively.** Multi-terminal output
  (terminfo-derived, every `TERM`); the wide path. **The terminfo *reader* is proven
  terminal-general:** it parses the Linux console entry byte-identically to ncurses across
  495/497 standard caps (court `NCURSES.TERMINFO.LINUX`), the only two exceptions being
  `cols`/`lines` (absent from the entry, resolved by `setupterm` from the tty) -- so the
  low-level layer is not xterm-specific. The `tparm` %-expression evaluator is proven
  terminal-general too (court `NCURSES.TPARM.LINUX`: 78 of the Linux console's own
  parameterized caps -- `cup`/`setaf`/`sgr`/`ich`/`dch`/... -- byte-exact). The `mvcur`
  byte engine is also proven byte-exact for the Linux console (court `NCURSES.MVCUR.LINUX`:
  all 23409 pairs), since linux shares xterm's cursor cap set + flags. The **doupdate** engine's
  plain path is now cap-driven (`clear_screen` bytes + a `has_rep` flag) and proven byte-exact
  for the Linux console too (court `NCURSES.DOUPDATE.LINUX`: linux uses `\e[H\e[J` and has **no**
  `rep`, so identical runs emit literally) -- the second byte engine generalized for a terminal
  with *differing* caps. The **color/attribute SGR** engine (`vidputs`) is cap-driven too now
  (`sgr`/`sgr0`/`rmacs` overridable; linux's `sgr` uses `^N`/`^O` and always trails `^O`), proven
  on linux's colored/bold doupdate diffs (same court). The **`ncv`** (`no_color_video`) rule is
  modeled: while color is on, attributes the terminal can't combine with color are suppressed
  (linux `ncv=18` strips underline/dim), courted on linux (`attr_underline_ncv`). The **ACS**
  line-drawing path is courted too (`acs_line` on both xterm `\e(0`/`\e(B` and linux `^N`/`^O`):
  linux's `acsc` is identity for the drawing chars, so only `smacs`/`rmacs` differ, already
  cap-driven. **The recomputed-cost-model cap class is now done too:** the `mvcur` cost model is
  cap-parameterized (`Caps`) and proven byte-exact on **vt100** -- a fundamentally different cap
  class with **no `hpa`/`vpa`** and `$<N>` padding on `cup`/`cuf1`/`cuu1` -- across all 23409 pairs
  (court `NCURSES.MVCUR.VT100`). The vt100 costs (`cup`=33, `cuf1`/`cuu1`=13, parm caps=5, `cub1`=1)
  are derived from ncurses' normalized cost (`chars + 5*padding_ms` at the 38400-baud pty) and make
  the engine step with the parameterized `cuf`/`cub`/`cuu`/`cud` and `\b` instead of column/row
  addressing; vt100's `xon` suppresses padding NUL bytes. The **doupdate** engine is byte-exact on
  vt100 too (court `NCURSES.DOUPDATE.VT100`, 24 plain scenarios): the TransformLine shaping is
  cap-driven (`el`/`el1` padded to 18/19 so spaces beat `clr_eol`; no `ech`/`parm_ich`/`parm_dch`,
  so ncurses' `can_idc`-gated insert/delete detection is skipped and runs/shifts are painted as
  literal cells). **Non-identity `acsc` glyph remapping is now done too:** `A_ALTCHARSET` cells are
  translated through the cap's `acsc` map at emit time, so a terminal that renders line-drawing via
  remapped bytes (cygwin: `q`->`0xC4` CP437, smacs `\e[11m`/rmacs `\e[10m`) is byte-exact -- court
  `NCURSES.ACS` proves three classes: xterm (identity acsc, `\e(0`/`\e(B`), screen (identity acsc,
  `^N`/`^O`), and cygwin (non-identity acsc + the `cud1=\e[B` cost-model variant).
  **The `mvcur` engine is now fully terminfo-driven** (`Caps::from_terminfo`): every cursor cap's
  byte string *and* its ncurses sample-parameter cost (`NormalizedCost`, `chars + 5*padding_ms`) are
  derived from any loaded terminfo entry, and motion is emitted by expanding the actual cap via
  `tparm` (no hardcoded ANSI) -- so the engine drives an arbitrary terminal, not a fixed list. Proven
  byte-identical to the hand-built engine across the full 23409-pair grid for the committed xterm and
  linux fixtures (`tests/mvcur_terminfo.rs`), with the live xterm/vt100 matrices (23409 each) still
  byte-exact. A one-off sweep over the **whole ncurses terminfo database** (the canonical
  `terminfo.src` compiled with `tic`, ~2,900 entries incl. building-block fragments) measured the
  cursor engine byte-exact on **~383 real terminals across diverse cap classes** (xterm family,
  vt100/vt220/vt320/vt420, linux, screen/tmux, rxvt/urxvt, cygwin/pcansi, sun, wsvt25, Eterm,
  dtterm, putty, konsole, kterm, hurd, mach, ansi, adm3a, …). **The `doupdate`/Screen engine is now
  terminfo-driven too** (`Screen::from_terminfo`): the cursor cost model, `clear_screen` bytes, `rep`
  presence, `el`/`el1` shaping costs (`NormalizedCost`), `ech`/`ich`/`dch` availability, the `acsc`
  glyph map, the `sgr`/`sgr0`/`rmacs` SGR engine, and the `ncv` mask are all derived from the loaded
  entry -- so the screen optimizer drives an arbitrary terminal, not a hand-coded bundle. Proven
  byte-identical to the hardcoded xterm engine (first paint + diff) for the committed fixture
  (`tests/doupdate_terminfo.rs`); the same DB sweep measured the **doupdate first-paint byte-exact on
  ~88% of real terminals** (with clear-cap padding). **The `tputs` padding-byte model is now done**
  (`apply_padding`): a terminal **without `xon`** (or a **mandatory** `$<N/>`) emits
  `floor(N_ms * baud / 9000)` pad bytes (`pad` cap, else NUL) at the capture pty's 38400 baud; an
  `xon` terminal strips them. Grounded against real ncurses (ampex219 `cup $<5>` -> 21 NULs,
  modgraph2 `cuu1 $<2/>` -> 8 NULs) and pinned by the committed `ampex219` fixture
  (`cursor::tests::from_terminfo_padded_cup_emits_pad_bytes`) -- both the mvcur emission
  and the `doupdate` `clear` carry padding now. **Remaining open (the rest of the
  database):** **The `auto_left_margin`/`bw` back-wrap tactic is now done** (the dominant remaining
  mvcur class): a backspace at column 0 wraps to the previous line's last column, so a right-margin
  target (`tx == cols-1`) is reached via `[cr] + cub1 + vertical(fy-1 -> ty)` -- byte-exact vs real
  ncurses (court fixture `aixterm`, `tests/mvcur_terminfo.rs`; aixterm/hft/ibm5151 went 0-diff), and
  it lifted the DB mvcur byte-exact rate from ~54% to ~63%. Two follow-on terminfo-derivation
  corrections lifted it further to **~74%**: (a) **`cr` is not synthesized when absent** -- ncurses
  disables the CR tactic for a terminal lacking `carriage_return` (it reaches column 0 via home/cup),
  so the crate no longer defaults `cr` to `\r`; (b) the **screen width is the runtime 80-col pty, not
  the terminfo `cols`** -- a 132-col entry (the `-w` wide variants) still runs in an 80-col window,
  and `cols` drives the `NOT_LOCAL` threshold, so reading terminfo `cols` wrongly forced `cup` where
  ncurses uses `hpa`. **`cursor_to_ll` (tactic 4) is now done too**, lifting the rate to **~83%**: the
  `ll` cap jumps to the lower-left corner `(LINES-1, 0)` and a last-line target is reached via
  `ll + relative` -- byte-exact on the Wyse family (court fixture `wy350-wvb`, ll = `\x1e\x0b`;
  wy350/wy120/wyse120/wyse160 went 0-diff). A `tparm` `save_char` fix (**`%c` of 0 emits `0x80`, not a
  NUL**, matching ncurses -- binary cursor-address caps like `\x1f%p1%c%p2%c` rely on it for row/col
  0) lifted it to ~85%. Gating the **bw back-wrap on `!xenl`** lifted it to ~89%: with
  `eat_newline_glitch` the last column is a magic margin, so ncurses uses `cup` rather than the
  back-wrap to reach it (fixing nsterm = macOS Terminal, hurd, Eterm, the prism/nsterm family; the
  bw-without-xenl aixterm still back-wraps -- `tests/mvcur_terminfo.rs`). **Three further grounded
  fixes closed the cup-less cluster and took the whole-database byte-exact rate to 99.84%
  (2558/2562 terminals checked):** (a) the **`NOT_LOCAL` short-circuit is gated on `cup` existing**
  -- in `lib_mvcur.c` the `goto nonlocal` is nested inside the `if (_address_cursor)` block, so a
  cup-less terminal (pure-relative LCDs like `MtxOrb`, Matrix-Orbital/pmcons/tek consoles) *always*
  runs the local optimizer instead of falling through to an absent `cup` and emitting nothing
  (`pmcons`); (b) a **`cud1` whose first byte is `\n` is rejected even with padding** (`\n$<5>`),
  matching ncurses' `*cursor_down != '\n'` test, so a downward move falls through to `cup`/`vpa`
  (`ncr160wy60pp`); (c) the **bw back-wrap (tactic 5) has no right-margin restriction** -- it
  wraps to `(fy-1, cols-1)` and runs a *full* `relative_move` from that corner to any target column,
  reproducing `\x08`+cuu1+(cub1...|hpa) walks (`apple2e`, `hp2626-ns`). Each is pinned by a committed
  fixture test in `tests/mvcur_terminfo.rs`. The measurement itself was corrected: the oracle's
  `\x01\x01\x01` segment mark *collided* with legitimate `\x01` value bytes in non-CSI `cup` output
  (column/row 1), under-counting matches; a high-byte (`\xff\xfe\xfd`+index) sentinel removed the
  collision and revealed the true rate. The remaining 4 are a genuine exotic tail: a malformed
  fixed-string `vpa` (`apollo`), an Apple-II col-first `cup` + padded `cr` (`apple-80`), and two
  Tektronix graphics terminals where real ncurses emits nothing (`tek4113`/`-34`).

  **The doupdate engine got the same whole-database treatment, reaching 99.26% byte-exact first paint
  (2543/2562 checked; reproduce with `tools/db-coverage/sweep.py`).** The clearing path is now fully
  cap-driven, reproducing ncurses' `ClearScreen`
  4-way fallback (tty_update.c): (1) `clear_screen`; (2) `clr_eos` + GoTo(0,0); (3) per-line
  GoTo(i,0)+`clr_eol`; (4) blank-overwrite when the terminal has *no* erase cap at all. The previous
  hardcoded `\e[H\e[2J` fallback for a `clear`-less terminal -- and the hardcoded `\e[r;cH` in `GoTo`
  for an unknown cursor position -- were **fake parity** (bytes the terminal cannot understand); both
  are gone. `GoTo` from an unknown position now drives `mvcur` with an unknown source, emitting the
  terminal's own `cup` or *nothing* (cup-less). The `el`/`el1`/`ed` byte sequences and costs are reset
  to empty / `ABSENT_COST` when the cap is absent, so the blank-overwrite fallbacks fire and `ClrBottom`
  (gated on `clr_eos`) is skipped -- matching ncurses. This recovered the whole addressable no-clear
  cluster (`avatar`/`avatar0`/`avatar1` via clr_eol-per-line, `newhp`/`newhpkeyboard`/`hp262x` via
  clr_eos, `gt40`/`gt42`/`oldpc3`/`oldibmpc3`/`vanilla` via blank-fill), pinned by committed fixtures
  (`newhp`, `avatar`, `gt40` in `tests/doupdate_terminfo.rs`). A no-`xon` padded `el`/`ed` still emits
  its pad bytes in the incremental diff (ampex219's `ed=\e[J$<50>` -> 213 NUL pads).

  **The incremental TransformLine diff path was then swept the same way (scene-A -> scene-B), going
  from ~95.5% to 98.95% byte-exact (2535/2562; `tools/db-coverage/sweep.py incremental`) via three
  more grounded fixes:** (i) a `bce` (back_color_erase) terminal forces `_el_cost` to 0
  (lib_mvcur.c), biasing the trailing-blank clear toward `clr_eol` even when the literal `el` is
  padded/expensive -- recovered arm100/scoansi/ms-vt100 (`arm100` fixture); (ii) the `erase_chars`
  /`repeat_char` run-coalescing is now **terminfo-derived** (cap template + `NormalizedCost` + emit
  via `tparm`), so `rep` emits the terminal's own cap -- concept's `\Er%p1%c%p2%' '%+%c` -> `\x1br &`,
  not the ECMA `\e[Nb` -- recovering the whole concept/c100/c108 family (~25 terminals; `concept`
  fixture); (iii) the leading-blank `o_first > n_first` case now sets `firstChar = nFirstChar`
  unconditionally (matching ncurses TransformLine), dropping a spurious GoTo-to-column-0 -- recovered
  the hp264x block-mode family and others (`hp2645` fixture). The remaining ~1-2% incremental tail is
  the same cup-less degenerate family as first paint (cad68/beehive/otek411x/datapoint/ibm-pc: after a
  cup-less paint ncurses loses cursor tracking and streams the changed content unpositioned). The
  parameterized `ich`/`dch` shaping caps remain the CSI constants (gated by `has_idc`).

  **The attribute/SGR engine (`vid::Vid`) was the next whole-database sweep, and the biggest single
  gap closed: 34% -> 91.5% byte-exact (`tools/db-coverage/sweep.py attr`).** The crate had hardcoded
  xterm `sgr`/`sgr0` for every terminal; three grounded fixes (lib_vidattr.c / lib_mvcur.c): (i) the
  **no-`sgr` individual-mode-cap path** -- 46% of the database has no `set_attributes`, so ncurses
  drives it with `bold`/`smul`/`smso`/`rev`/`blink`/`dim`/`smacs`/... and the `rmul`/`rmso` exit caps
  (used only when they differ from `sgr0`, `SGR0_TEST`), resetting via `sgr0` when an attribute with
  no individual exit must turn off, then re-enabling everything (`ti916-8-132` fixture); (ii) **padding**
  is now applied to all attribute caps (vt100/vt220's `sgr` ends in `$<2>`, previously emitted as
  literal bytes); (iii) **`msgr`** -- a terminal without move_standout_mode resets to A_NORMAL before
  every cursor move and restores the attribute after (`wyse520-w` fixture). The remaining ~8.5% (218
  terminals) is **dominated by the magic-cookie glitch (`xmc`)**: 204 of the 218 attribute fails are
  `xmc` terminals (1980s tvi/wyse/adm/abm/concept clones where an attribute change physically occupies
  `xmc` screen cells). The mechanism is understood -- ncurses' `_nc_do_xmc_glitch` (tty_update.c)
  scans newscr for attribute turn-ons and, where there is no room for the cookie in adjacent blanks
  (e.g. attributed text at column 0, the whole sweep scenario), nukes the attribute; `_xmc_triggers =
  _ok_attributes & XMC_CONFLICT` selects which attrs glitch. A faithful display-level port of that pass
  (nuke/fit) was prototyped, but ncurses' exact byte stream for these terminals adds subtle
  glitch<->vidputs interactions (stray `rmul`/`rmso` exit caps at span boundaries, and an `xmc#0`
  sub-case) that need ncurses trace tooling to reproduce byte-for-byte -- an honestly-bounded,
  near-zero-value tail (all obsolete terminals), so it is left documented rather than shipped
  non-byte-exact. The non-xmc remainder is the prism/P-series `\x03`-prefixed exotic encodings.

  **The color engine (`_nc_do_color`) was the next sweep: 56% -> 97.9% byte-exact on color-capable
  terminals (`tools/db-coverage/sweep.py color`).** It was hardcoded to xterm (`\e[3Xm`/`\e[4Xm`/
  `\e[39;49m`); now terminfo-driven: `setaf`/`setab` (ANSI) or `setf`/`setb` with `toggled_colors`
  (the SVr4 1<->4/3<->6 swap), `orig_pair`, all padded; gated on `colors#>0` so a colorless terminal
  (vt100) emits no color. ncurses' ClearScreen color-init (`_nc_do_color(SCREEN_PAIR, 0)` before the
  clear) is reproduced for the default background, and -- the big lever -- a color terminal **without
  `bce`** cannot fast-clear (the erase wouldn't paint the bg), so ncurses sets `fast_clear = FALSE`
  and **blank-fills**; reproducing that required the full `PutCharLR` bottom-right corner (cases:
  `rmam`/`smam` am-suppression, `ich`/`smir` insert, or *skip the cell*) and `am` auto-margin-wrap
  cursor tracking. This recovered the mainstream color families (ansi/pcansi/aixterm/cygwin/sun),
  pinned by `sun-color` (setaf+blank-fill+corner), `emots` (setf+toggled), `vt100` (no color). The
  corner/am generalization also lifted first-paint doupdate to 99.45%. The `set_color_pair` (scp) path (the DG `ccc` `d430-*` family) and the no-msgr color reset are now
  handled; the remaining ~2% color tail is exotic: stateful `%P`/`%g` `setf` caps (qnx/ctrm) that
  need tparm static-variable state persisted across calls, and a few exotic positioners. The remaining
  ~1% first-paint tail is degenerate: unaddressable hardcopy/dumb terminals (`dumb`/`ep4x`/`att53xx`/
  `megatek`) that need exact auto-margin-wrap cursor modelling, hardware-tab `viewdata`, and the
  padded-clear `hp9845`/`modgraph2`. Hardware tabs
  (`ht`) were investigated for **mvcur** and are not a factor there (ncurses emits a tab in only
  ~2/250 sampled terminals on the capture pty), though they remain the cause of the `viewdata`
  doupdate tail. The real `doupdate`
  line-diff is
  done (OPT-02, plain + attr/color, byte-exact); single-width UTF-8 rendering is now
  byte-exact too (LOC-01, court `NCURSES.WIDECHAR` ✅). The version claim is now pinned to the
  build actually verified: **the crate admits ncurses 6.4** (`ADMITTED_NCURSES`), and the
  init/teardown framing is captured live from that build and **matches byte-for-byte**
  (court `NCURSES.BYTE.FRAME.FULL` ✅, FRAME-01 resolved) — the earlier
  6.4-oracle-vs-6.6-claim mismatch is gone. **S1, in progress (multi-terminal + wide API/double-width remain).**

> Consequence for the rest of this ledger: the C-ABI / soname / `repr(C)` /
> `_nc_*` / variadic-ABI entries (ABI-VERS-01, ABI-PRIV-01, ABI-STRUCT-01,
> ABI-VAR-01) and the ecosystem drop-in items (ECO-01/03/04/07) are **in-scope
> native targets** delivered through `crates/ncurses-cabi`, not blockers.

---

## 1. Memory model & ownership (C pointers vs Rust ownership)

- **MEM-01 · Returned `char*` into static/area buffers.** ncurses: `tigetstr`,
  `tparm`/`tiparm`, `keyname`, `unctrl`, `termname`, `longname`, `tgetstr`,
  `keybound` return pointers into internal static / per-`TERMINAL` storage, valid
  *only until the next call that reuses the buffer*. native: returns owned
  `String`/`Option<String>`/`&str`. *Gap:* the "clobbered on next call" lifetime is
  unrepresentable in safe Rust except by tying the borrow to `&mut self`; callers
  that cache or compare the C pointer behave differently. **S2, open.**
- **MEM-02 · `longname()` clobbered by `newterm`, not restored by `set_term`.**
  ncurses: one static area, overwritten per `newterm`, not saved across terminals.
  native (owned model): per-screen value — the clobber-and-not-restore behaviour
  disappears. **S2, open.**
- **MEM-03 · Three-state `(char*)-1` / `NULL` / valid.** ncurses `tigetstr`
  returns `(char*)-1` for "not a string capability", `NULL` for "absent/cancelled",
  else the string. native models this with a 3-variant `Tigetstr` enum (good) — but
  the legacy termcap `tgetstr` and several others collapse to `Option`, losing the
  cancelled-vs-absent distinction unless explicitly modelled. **S2, partial** (the
  terminfo path is faithful; audit the others).
- **MEM-04 · Caller-frees vs `Drop`; deletion ordering.** ncurses: `delwin` must
  precede deleting the parent (deleting a parent with live subwindows is UB);
  `del_curterm` on the live `cur_term` leaves all capability accessors dangling.
  native: `Drop` is automatic; the ordering UB and the dangling-`cur_term`
  footgun become impossible. *Gap:* cleanup side effects (touch propagation on
  delete) and the disappearance of a whole UB class diverge from bug-for-bug
  behaviour. **S2, open.**
- **MEM-05 · UAF / double-free / dangling eliminated.** `delwin` twice, using a
  `WINDOW*` after `delscreen`, etc. are C UB; in Rust they are compile errors or
  `None`. *Gap:* a security/robustness win that **breaks bug-for-bug fidelity** for
  any test/fuzzer that observed post-free garbage. **S3, open (by design).**
- **MEM-06 · Aliasing: `copywin`/`overlay`/`overwrite` with overlap or self.**
  ncurses: raw-pointer copies with overlap permitted. native: `&mut` aliasing is
  forbidden; self-copy/overlap needs interior mutability or index-based design.
  Current native `overlay`/`overwrite`/`copywin` are classified deferred (no
  shared-cell model). **S2, open.**
- **MEM-07 · Pointer identity (`win == stdscr`).** ncurses code dispatches on
  pointer identity; Rust needs `Rc::ptr_eq` / an id field. There is no `stdscr`
  singleton at all in native. **S2, open.**
- **MEM-08 · Opaque-but-poked structs.** Real code reads `win->_maxx`,
  `win->_cury`; native fields are private. Source ports that poke internals break.
  **S2, open.**

## 2. Integer / bit-layout semantics

- **INT-01 · `chtype`/`attr_t`/`mmask_t` widths & bit positions.** ncurses packs
  char+color+attrs into a 32-bit `chtype`; `--enable-ext-colors` widens the color
  field and **moves the attribute bit boundaries**; `--enable-ext-mouse` widens
  `mmask_t`. native fixes `u32` and matches the *default* `A_*` positions
  (verified: `A_BOLD=0x200000`, `A_COLOR=0xff00`, court `NCURSES.WINDOW.ATTR`).
  *Gap:* the ext-colors/ext-mouse layouts (a second "6.6" ABI) are not modelled.
  **S1, partial.**
- **INT-02 · `short` color-pair numbers (overflow at 32767) vs `i32`.** ncurses
  has both `short`-based (`init_pair`/`pair_content`) and `int`-based extended
  (`init_extended_pair`) APIs precisely because `short` overflows; native uses
  `i16`/`i32` and silently fixes the overflow. **S2, partial.**
- **INT-03 · Signed overflow: C UB vs Rust panic/wrap.** Hostile terminfo (giant
  `$<…>` delays, huge `%{…}` constants) can trigger UB in C but a *defined* panic
  (debug) or wrap (release) in Rust — a determinism/fuzzing divergence. **S2, open.**
- **INT-04 · `char` signedness & implicit conversions.** C `char`-is-signed-or-
  unsigned is implementation-defined and affects high-bit (0x80–0xFF) chars built
  into `chtype` via `addch`; Rust `u8`/`i8` is fixed → high-bit Latin-1 may map
  differently than on a given C platform. **S2, open.**
- **INT-05 · `tparm` value typing.** ncurses caches per-format param-type
  analysis (int vs string per `%p`); native `tparm` uses an int/string `Param`
  enum (court `NCURSES.TPARM`, 106 cases). *Gap:* `tiparm`'s explicit typed API and
  `tiscan_s`/`tiparm_s` are not separately reconstructed. **S2, partial.**

## 3. Global mutable state & reentrancy (the biggest architectural divergence)

- **GLB-01 · The global set.** `cur_term`, `SP`, `stdscr`, `curscr`, `newscr`,
  `LINES`, `COLS`, `COLORS`, `COLOR_PAIRS`, `ESCDELAY`, `TABSIZE`, `ttytype[]`,
  `acs_map[]`, `boolnames/numnames/strnames` (+codes/fnames), `ospeed`, `PC`,
  `UP`, `BC`. native: owned-value model, no process globals. *Gap:* code reading/
  **writing** `LINES`/`COLS`/`ESCDELAY` has no target; the entire API shape
  differs. **S0, open.**
- **GLB-02 · `--enable-reentrant`: macros vs functions, write access.** In the
  reentrant build `LINES`/`COLS`/`ESCDELAY`/`TABSIZE`/`SP` become accessor
  functions and the only legal mutators are `set_escdelay`/`set_tabsize`; the
  default build exposes writable globals. A port can emulate only one. **S1, open.**
- **GLB-03 · `_sp` (screen-pointer) duality.** Every screen function has an
  `X_sp(SCREEN*, …)` twin; `new_prescr`, `ceiling_panel`, `ground_panel` are
  `_sp`-only. native's owned-method model *is* the `_sp` form, but must then layer
  the global-`SP` convenience overloads. **S1, partial** (no global shim yet).
- **GLB-04 · `tparm` static `%P/%g [a-z]` vars: per-`TERMINAL` since 6.3, shared
  before.** Interleaving `tparm` across screens differs by version. native does
  not persist static vars across calls. **S2, open.**
- **GLB-05 · `acs_map[]` is terminal-dependent, and `ACS_*` are macros over it.**
  `ACS_VLINE` ≡ `acs_map['x']`, filled from terminfo `acsc` at init — **not** a
  constant. native hardcodes the canonical letters `l q k x m j` (correct only for
  terminals whose `acsc` matches xterm's). **S1, recorded** (non-claim noted).
- **GLB-06 · `LIFO` multi-screen teardown & `new_prescr`.** If `newterm` is called
  twice for the same terminal, the *first* SCREEN must be the *last* `endwin`'d;
  `new_prescr` stores pre-init settings (use_env/ripoffline/slk_init) per screen.
  native models neither. **S2, open.**

## 4. Concurrency, signals, control-flow, errors (runtime semantics)

- **CON-01 · Thread-safety contract.** Base ncurses is **not** thread-safe;
  `--enable-threads` adds `use_window`/`use_screen` callback mutexes (and even then
  "not enough"). Rust's `Send`/`Sync` forces a different model entirely; native
  `use_window`/`use_screen` are classified n_a. **S1, open.**
- **CON-02 · `Send`/`Sync` of handles.** C passes `WINDOW*`/`SCREEN*` between
  threads freely (UB if concurrent, but compiles); Rust may forbid it at compile
  time. **S2, open.**
- **SIG-01 · SIGWINCH / `KEY_RESIZE` / auto-resize.** ncurses installs a handler
  that flags a resize; the next `wgetch`/`doupdate` runs `resizeterm` and updates
  `LINES`/`COLS`, returning `KEY_RESIZE` (even without keypad). **Impossible in
  pure-safe Rust** (no signal API); needs the unsafe `ncurses-terminal` crate or
  `TIOCGWINSZ` polling. **S0, boundary.**
- **SIG-02 · SIGTSTP/SIGCONT job control.** Ctrl-Z/`fg` must save/restore tty
  modes (`_nc_screen_resume`); without signal hooks the terminal is left in raw
  mode on suspend. **S1, boundary.**
- **SIG-03 · Crash/exit cleanup (`endwin` on death).** `Drop` does **not** run on
  `process::abort`, `exit`, or signal kill; a panicking Rust TUI leaves the
  terminal in raw mode / no cursor unless a panic hook + signal handler is wired
  (unsafe/libc). **S1, boundary.**
- **CTL-01 · Varargs.** `printw`/`scanw`/`mvprintw`/`vw_printw`/`vwprintw`/`tparm`
  use C `va_list`/`...`; Rust has no stable variadics. native `printw*` is a
  scaffold (formatting delegated); `%n`, `%hhd`, `%'d` (locale), and `va_list`-taking
  forms are not 1:1. **S1, partial.**
- **CTL-02 · Function-pointer callbacks.** `ripoffline(line, int(*)(WINDOW*,int))`
  (callback runs *during* `initscr`, may get a NULL window, max 5),
  `tputs(s,n,int(*putc)(int))`, `vidputs`/`vid_attr` `outc`, `use_window`/
  `use_screen` callbacks. native does not model the callback ABI / the putc output
  funnel. **S1, open.**
- **CTL-03 · `setjmp`/`longjmp`.** longjmp over Rust frames is UB and skips `Drop`;
  any C path using it must be restructured to `Result`/unwinding, changing cleanup
  timing. **S3, open.**
- **CTL-04 · `atexit`/`_nc_freeall` teardown ordering** differs from Rust `Drop`/
  `atexit`; valgrind-clean teardown and final flush parity. **S3, open.**
- **ERR-01 · `ERR`/`OK` int returns vs `Result`.** Collapsing every `ERR` (which
  carries no reason) into one error type, or enriching it, both diverge; the exact
  conditions that yield `ERR` (off-screen `wmove`, NULL win) are under-tested.
  **S2, open.**
- **ERR-02 · Partial mutation on error.** `waddstr`/`waddnstr` may write some
  chars then return `ERR` having already mutated the window; `?`/`Result` callers
  may assume atomicity. **S2, open.**
- **ERR-03 · `setupterm` tri-state `errret`.** `*errret` = `1` ok / `0`
  unusable-or-generic / `-1` **database not found**, and **`NULL` errret → print
  message and exit the process**. native `Terminfo::load` returns a `Result`,
  collapsing the tri-state and erasing the print-and-exit path. **S1, partial.**
- **ERR-04 · `getch` `ERR`: would-block vs EOF vs error** (three states) collapses
  badly into `Option`/`Result` unless modelled explicitly. **S2, boundary.**

## 5. Locale / wide-character / encoding (an entire absent half)

- **LOC-01 · Wide *rendering* reproduced for single-width UTF-8; the wide *API*
  is still absent.** The cell now stores a Rust `char` (one single-width glyph per
  cell) and `doupdate` emits each cell's UTF-8 encoding, with **no `repeat_char`
  coalescing for multibyte glyphs** — matching `ncursesw`'s `EmitRange` byte-for-byte
  (court **`NCURSES.WIDECHAR`**: positioned mixed Latin/Greek/Cyrillic/symbol text and
  identical multibyte runs, byte-exact against real `libncursesw` under a UTF-8 locale).
  The `cabi` `addstr`/`waddstr`/`mvaddstr` decode UTF-8 input into cells.
  The wide *function* surface is now **wired in the C-ABI**: `cchar_t` with
  `setcchar`/`getcchar`, and the `add_wch`/`wadd_wch`/`mvadd_wch`/`mvwadd_wch` +
  `addwstr`/`addnwstr`/`waddwstr`/`waddnwstr`/`mvaddwstr`/`mvwaddwstr` families compile
  against the generated `curses.h` and draw **byte-identically to system `libncursesw`**
  (court `NCURSES.WIDECHAR.CABI`: bold CJK, underlined fullwidth, Latin/CJK/Greek/Kana/
  Hangul wide strings). Wide **input** is wired too: `get_wch`/`wget_wch` assemble a UTF-8
  byte sequence into a codepoint (returning `OK`) and report a function key as its `KEY_*`
  code (returning `KEY_CODE_YES`), **byte-identical to libncursesw** (court
  `NCURSES.WGET_WCH`: UTF-8 single-/double-width chars interleaved with arrows/nav keys) --
  closing the wide round-trip. Wide **read-back** is wired too: `in_wch`/`win_wch`/`mvin_wch`/
  `mvwin_wch` extract a `cchar_t` (spacing wide char + attributes incl. color + pair, with the
  base glyph returned at a double-width glyph's padding column), **byte-identical to libncursesw**
  (court `NCURSES.WIN_WCH`, with `getcchar` returning `A_ATTRIBUTES` incl. color). **Combining
  marks ✅ done** (LOC-03): a cell carries up to `CMAX`=4 combining marks (`cchar_t.chars[1..]`)
  that attach to the preceding base and emit as UTF-8 right after it. Still **absent / open**:
  `wunctrl`, the `*_wchstr` bulk read, and the ext-color `cchar_t` layout. **S1, partial/courted
  (single- + stable double-width I/O + read-back + combining marks, full wide cell API).**
- **LOC-02 · narrow/wide is a compile-time fork in C; additive Cargo features
  can't be both.** `--enable-widec` changes `chtype`/adds `cchar_t`; `wchar_t` is
  32-bit on Linux / 16-bit on Windows; Rust `char` is always a 32-bit USV and
  cannot hold unpaired surrogates or raw `wchar_t`. **S1, open.**
- **LOC-03 · `cchar_t` layout & `CCHARW_MAX=5` grapheme cap. ✅ DONE (cell model + emission).**
  A `Cell` carries 1 spacing glyph + up to `CMAX`=4 combining marks (`cchar_t.chars[1..]`);
  zero-width marks (`is_zero_width`, the combining-diacritical blocks) attach to the preceding
  base cell, advance no columns, and are emitted as UTF-8 right after the base -- byte-exact vs
  `libncursesw` (court `NCURSES.WIDECHAR`, `combining_acute` / `combining_word` / `combining_double`
  / `combining_mixed` / `combining_midword`). Overflow past `CMAX` is silently dropped (matching
  ncurses). The raw in-memory `cchar_t` struct layout (an FFI concern, not byte output) stays
  unspecified by X/Open. **S1, courted.**
- **LOC-04 · `wcwidth` / double-width / zero-width version skew — stable wide ranges
  reproduced.** ncurses uses the host libc `wcwidth` (its Unicode version); a Rust
  `unicode-width` crate pins a *different* Unicode version → off-by-one column
  placement for new emoji/CJK. native now ships a `char_width` covering the **rock-solid
  BMP wide ranges** (CJK ideographs, Kana, Hangul, fullwidth forms — universally width-2
  across `wcwidth` versions): a wide glyph fills a cell + a padding cell and advances two
  columns, emitted byte-exact vs the host `libncursesw` (court `NCURSES.WIDECHAR`,
  cjk_basic / cjk_kana_hangul / cjk_fullwidth). The right-margin wide-glyph **wrap** is
  now reproduced too (cjk_cursor_wrap / cjk_glyph_wrap: filling the last two columns wraps
  the cursor to the next line, and a glyph that won't fit wraps whole — the OPT-02
  auto-margin corner, now closed). **Zero-width combining marks are now reproduced** (LOC-03,
  court `NCURSES.WIDECHAR` `combining_*`). Still **open**: version-skewed widths (emoji, recent
  CJK — deliberately treated as width-1). **S1, partial/courted (stable double-width + wrap +
  combining); emoji width-skew open.**
- **LOC-05 · `setlocale`/`$LC_CTYPE` dependence & non-UTF-8 locales** (EUC-JP,
  ISO-2022) and incremental `mbrtowc` decoding of partial/invalid sequences.
  native is ASCII/UTF-8 only with no incremental decoder. **S2, open.**
- **LOC-06 · `key_name` vs `keyname` asymmetry** (the wide `key_name` does NOT
  return function-key names); `get_wch` yields `wint_t`/`WEOF`/`KEY_*` needing a
  union type. **S2, open.**

## 6. Timing, I/O, and the tty boundary (→ the `ncurses-terminal` crate)

- **TIM-01 · `tputs` padding / baudrate delay.** Pad-char count from `ospeed`,
  `affcnt`, `$<ms>`, `padding_baud_rate`, `xon`, `BSDpad`; on slow/hardware
  terminals the pad bytes are part of the emitted stream. native `tputs` strips
  `$<…>` and emits no pad bytes (court `NCURSES.TPUTS`, admitted for the normal-
  baud pty) — **the baud/pad-char model is not reconstructed**. **S1, recorded.**
- **TIM-02 · `ESCDELAY` (default 1000 ms, capped 30 s in 6.6), `set_escdelay`,
  the `ESCDELAY` env var** — the ESC-vs-sequence disambiguation budget. The
  `ncurses-terminal` reader drains a multi-byte key within an inter-byte window
  (analogous to `ESCDELAY`), so the **keycode outcomes match ncurses**: a lone ESC
  (or ESC not starting a known sequence) returns `27` then the next byte, while a
  buffered `\eOA` decodes to `KEY_UP` — courted (`esc_then_char`/`esc_then_text` in
  `NCURSES.INPUT`/`.LIVE`/`NCURSES.CABI.GETCH`). **Remaining:** the exact configurable
  delay *duration* (fixed ~50 ms vs ncurses' default 1000 ms / `$ESCDELAY` /
  `set_escdelay`) — a timing value, not a keycode difference. **S2, partial/courted.**
- **TIM-03 · `napms` (caps at 30 s, auto-restarts on signal); `delay_output`
  (caps 30 000 ms, pads unless `npc`/`NCURSES_NO_PADDING`).** native classifies
  these n_a (timing). **S2, boundary.**
- **IO-01 · termios raw/cbreak/noecho/nl/meta/keypad. ✅ PARTIAL (raw mode built).**
  `cbreak` clears `ICANON`; `raw` additionally clears `ISIG/IXON/…` and is 8-bit clean;
  `meta` toggles `CS8`/`CS7` and emits `smm`/`rmm`; `keypad(TRUE)` **emits `smkx`**.
  Pure-safe Rust cannot set termios, so the **`ncurses-terminal` crate** (the quarantined
  unsafe boundary) now implements `RawMode` (hand-declared Linux `termios` FFI -- no libc
  dep -- a cfmakeraw-equivalent that restores the saved `termios` on drop, even on panic).
  That backs the live native `getch` (court `NCURSES.INPUT.LIVE`). **Still open:** the
  `cbreak`/`noecho`/`meta` *individual* modes, `smkx` emission from `keypad`, and the
  three mode snapshots (IO-06). **S0→S1, boundary/partial.**
- **IO-02 · `isatty` / tty-vs-pipe fallback**, **IO-03 · SP output buffer & flush
  points** (flicker/atomicity), **IO-04 · blocking/nonblocking + typeahead-cancels-
  refresh optimization**, **IO-05 · `newterm(type, FILE*, FILE*)` arbitrary
  streams**, **IO-06 · `def_prog_mode`/`def_shell_mode`/`savetty` — three distinct
  termios snapshots per screen** (native models none). **S1, boundary.**

## 7. ABI / macros / source compatibility

- **ABI-MACRO-01 · the CPP macro API. ✅ PARTIAL (generated `curses.h`).** ~⅓ of the
  "API" is CPP macros with no symbol: `getyx(win,y,x)` assigns **bare lvalues**;
  `getmaxyx`/`getbegyx` likewise; `COLOR_PAIR`/`PAIR_NUMBER`/`ACS_*`/`A_*`/`KEY_*` are
  macros. native ships **`crates/ncurses-cabi/include/curses.h`** providing them: the
  `A_*`/`COLOR_*`/`KEY_*`/`ACS_*` constants, `COLOR_PAIR`/`PAIR_NUMBER`, and the
  `getyx`/`getmaxyx`/`getbegyx` statement macros — implemented over **accessor functions**
  (`getcury`/`getmaxy`/… exported from the cabi) so the `WINDOW` struct stays opaque (no
  struct-layout ABI exposed). A macro-API program compiles unmodified and draws
  byte-identically to the system `<curses.h>`/`libncursesw` (court `NCURSES.CURSES.HEADER`,
  incl. `getyx`/`getmaxyx`/`COLOR_PAIR`/`A_BOLD`/`ACS_VLINE`/`box`). **Still open:** the
  full header breadth (`getsyx`/`setsyx`, the complete `mv*`/`mvw*` macro matrix,
  `wide`/`cchar_t`). **S0→S1, partial/courted.**
- **ABI-VERS-01 · soname & symbol versioning. ✅ PARTIAL (soname + dynamic drop-in).**
  `crates/ncurses-cabi/build.rs` sets **SONAME `libtinfo.so.6`** and applies a version
  script (`version.map`) declaring the `NCURSES6_TINFO_5.0`/`NCURSESW6_5.1` nodes. Court
  `NCURSES.SONAME` confirms the soname via `readelf` and proves the `.so` is a working
  dynamic drop-in: a C program linked against it (placed as `libtinfo.so.6`, resolved via
  rpath) produces **byte-identical** terminfo output to the system `libtinfo`. `pkg-config`
  `tinfo.pc`/`ncursesw.pc` are provided (`crates/ncurses-cabi/pkgconfig/`, validated).
  **Still open:** *binding* the exported symbols to the exact NCURSES version nodes so
  pre-existing versioned binaries (real `tput`) resolve against ours — rustc emits its own
  cdylib export version-script, so our symbols stay at the base version (the verdef nodes are
  emitted but unbound). **S1–S2, partial.**
- **ABI-PRIV-01 · `_nc_*` private-but-exported symbols** relied on by tic/infocmp/`tput`
  and `--with-termlib` (`_nc_tiparm`, `_nc_tparm_analyze`, `cur_term`, `use_env`, …).
  Driving the *real* system `tput` needs these. **S2, open.**
- **ABI-STRUCT-01 · public struct layout** (`MEVENT{short id;int x,y,z;mmask_t bstate}`
  ✅ `#[repr(C)]`; `cchar_t` pending). **S2, open.**
- **ABI-VAR-01 · variadic C ABI for `tparm(...)`. ✅ DONE (numeric).** ncurses' classic
  `tparm(const char *, ...)` reads up to nine `long` parameters (`va_arg(ap, long)`).
  native exports `tparm` as a fixed nine-`c_long` `extern "C"` definition, which is
  **call-compatible with variadic callers** on register-based ABIs (e.g. x86-64 SysV:
  32-bit int args are zero-extended into the 64-bit slots, and parameters past the highest
  `%p` the cap references are ignored — exactly as ncurses ignores them). Verified
  byte-identical to the system `libtinfo` for `cup`/`setaf`/`setab` via court
  `NCURSES.CABI`. The rare string-valued (`%s`) caps and `tiparm` are not yet wired. **S2, done (numeric).**
- **ABI-NOTE-01 · `abi_stable` is the wrong tool for the C surface (evaluated, not adopted).**
  The [`abi_stable`](https://crates.io/crates/abi_stable) crate provides a *stable
  **Rust↔Rust** ABI* (`#[repr(C)]` wrapper types, `RVec`/`RString`/`RStr`, trait-
  object vtables, `#[sabi_trait]`, load-time layout checking) so two **Rust** crates
  compiled separately can interoperate across the otherwise-unstable Rust ABI. Our
  C-ABI target (STRUCT-01/02, ABI-STRUCT-01) is the opposite problem: we must match
  the **C** ABI that *ncurses itself* defines — exact `MEVENT`/`cchar_t` layouts,
  `chtype`/`mmask_t` widths, the `curses.h`/soname/`.symver` contract, and the
  variadic `tparm`. `abi_stable`'s own representations (e.g. `RVec`) are **not** the
  C layouts a `gcc`-built program expects, so it cannot stand in for the `extern "C"`
  + `#[repr(C)]` + version-script work; introducing it would add a dependency (against
  the dependency-free charter) without removing any of that work. `ncurses-cabi`
  therefore uses plain `#[no_mangle] extern "C"` + `#[repr(C)]`. `abi_stable` *could*
  be useful later for a **plugin/driver split between native Rust crates** (e.g. a
  dynamically-loaded terminal driver), which is a separate, optional concern. **S2, evaluated/declined.**

## 8. Build / packaging / tooling / data

- **BLD-01 · configure forks → behaviour/ABI** (`--enable-widec`, `-ext-colors`,
  `-ext-mouse`, `-reentrant`, `-sp-funcs`, `-sigwinch`, `-tcap-names`, `-bsdpad`,
  `--with-termlib`, `--with-fallbacks`). A single Rust build can't be all of them.
  **S1, open.**
- **BLD-02 · terminfo data is a separate dependency** with two binary magics
  (legacy `0o432`/16-bit, extended `0o1036`/32-bit) and hashed-dir (BSD/macOS)
  vs letter-dir layouts; compiled-in `--with-fallbacks` entries. native reads both
  magics (court `NCURSES.TERMINFO.LOOKUP`, the 32-bit format via xterm-256color in
  `NCURSES.TERMINFO.ECOLOGY`) and the letter-dir + hashed-dir paths, but has **no
  compiled-in fallback** (opposite failure mode: errors instead of working without
  a DB). **S2, partial.**
- **BLD-03 · the CLI suite IS ncurses to most users.** `tic`, `infocmp`, `toe`,
  `tput`, `clear`, `tabs`, `reset`, `tset`, `captoinfo`, `infotocap` — scripts
  depend on `tput`/`clear` exit codes & output, and on `infocmp | tic` round-trip
  idempotence. **`tput`/`clear` are done** (native binaries, `NCURSES.TPUT` court).
  **`infocmp` is done — 100.000% byte-exact for BOTH `-1` and `-1 -x` across the
  whole 2,869-entry terminfo database** (`NCURSES.INFOCMP` court) — the decompiler
  reconstructs `_nc_tic_expand` escaping (the caret-vs-octal length heuristic, the
  `%`-operator verbatim rule, the letter/`\s`/`\,`/`\^` escapes), power-of-two hex
  number formatting (`colors#0x100`), cancelled caps (`name@`), `acsc` glyph
  sorting, and the full extended (user-defined, `-x`) section including cancelled
  `name@` extensions — the extended offset-table layout (value offsets + name
  offsets = `2·es + eb + en`, the header's string-*count* vs string-*size* fields,
  cancelled strings keeping a `-2` value slot but no value string) is now decoded
  byte-exactly. **`tic` is done too — 100.000% byte-exact on BOTH the
  `infocmp -1 | tic` and `infocmp -1 -x | tic -x` round-trips across the whole
  database** (`NCURSES.TIC` court) — the inverse of `infocmp`, reconstructing the
  source parser (comma/`^X`/`\X`-aware splitting), the `_nc_tic_expand` un-escaper
  (`\E`/`^X`/`\nnn`/`\0`→0x80/`\s`, the `%`-operator verbatim rule), and the binary
  writer (header, `|`-names, bool bytes, even-offset numbers with 16-vs-32-bit magic
  selection, string offsets/table, cancelled `-2`, the SVr4 cutoff that drops
  ncurses-extension predefined caps without `-x`, and — with `-x` — the full
  extended section writer). **`infocmp -C` (termcap source output) is in progress,
  *not yet byte-exact* (~16% across the DB, honestly bounded — no court yet):** the
  foundation is correct — the curated emitted-code set, termcap escaping (`:`→`\072`,
  0x7f→`\177`), the `_nc_infotocap` parameter reverse-map (`%pN%d`→`%d`, `%2d`→`%2`,
  `%c`→`%.`, `%i`, `%'X'%+%c`→`%+X`, reversed→`%r`, drop-on-untranslatable),
  trailing-padding→leading-delay, the `is3`→`i2`/`rs2`→`rs` code remaps, and the
  insert-mode synthesis (`im`/`ei`/`ic` from smir/rmir/ich1). The remaining tail is
  the gnarly `_nc_infotocap` legacy munging: the exact line-wrap rule (a width-60
  break only reproduces ~50% of wraps — there is a further subtlety), the
  conditional `as`/`ae` and `rs` drops, the `ug`-from-`sg` synthesis, and a handful
  of value-translation edges (`cm`/`te`/`me`/`ec`). Measured limits of pure
  oracle-probing here: content-correctness (ignoring wrapping) tops out at ~25% and
  the wrap is provably *not* a greedy fixed-width fill (an exhaustive
  model×width×grouping sweep peaks at 46% exact). Unlike `infocmp`/`tic` — whose
  rules were finite and output-derivable — `_nc_infotocap` needs either source
  reference or many more probing passes; **flagged as the project's deepest
  clean-room wall.** `captoinfo`/`infotocap` build on this. **S1, in progress.**
- **BLD-03b · `toe` (table of entries) is *not* cleanly clean-room byte-exact.**
  `toe -a` dedups files by the entry's primary name (1829 of 2869 files shown) and
  emits `primary-name<TAB>longname`, but its traversal order is neither
  alphabetical nor `readdir` order (observed subdir order `b q E i w x j m …` vs
  `readdir`'s `3 b N q E …`) — it depends on ncurses' hashed-directory / db
  iterator, which is filesystem- and build-specific. Byte-exact `toe` output is
  therefore environment-fragile; deferred rather than faked. **S1, open.**
- **BLD-04 · companion libraries panel / menu / form (+`w`)** — ~200+ symbols,
  entire subsystems (overlapping-window stack; menu driver; field/form driver).
  **Zero coverage.** **S0, open.**
- **BLD-05 · `.pc`/`*-config` discovery. Mostly done; `libtinfo` split open.**
  The pkg-config files `ncursesw.pc`/`tinfo.pc` ship under
  `crates/ncurses-cabi/pkgconfig/`, and a native **`ncursesw6-config`**
  (crates/ncurses-tools) reproduces the system tool byte-for-byte across all 18
  options — `--version`/`--cflags`/`--libs`/`--abi-version`/`--mouse-version`, the
  prefix/dir queries (incl. the `--libdir`-suppresses-when-empty quirk), and
  env-honoring `--terminfo`/`--terminfo-dirs` — plus the narrow `ncurses6-config`
  (`-lncurses`) via program-name dispatch (court `NCURSES.CONFIG`, `admitted_match`).
  The **`libtinfo` split is now done too**: the standalone `ncurses-tinfo-cabi`
  cdylib is a true minimal `libtinfo.so.6` — soname `libtinfo.so.6`, exporting
  *exactly* the nine low-level terminfo symbols (setupterm, tigetstr/num/flag, tputs,
  putp, tgoto, tparm, curses_version) and **no** curses screen symbols, with a
  tinfo-only C program linked against it producing output byte-identical to the
  system libtinfo (court `NCURSES.LIBTINFO`, `admitted_match`). This is the termlib
  bash/less/readline depend on. (It is a dedicated crate rather than a re-export
  because a cdylib depending on `ncurses-cabi` would re-export its curses symbols
  too — rustc exports a dependency's `#[no_mangle]` symbols.) **S1, done.**

## 9. Security boundary (Rust fixes the wrong half)

- **SEC-01 · The interesting ncurses CVEs are parser/trust bugs, not all spatial.**
  History clusters in terminfo/tic parsing: CVE-2017-10684/10685 (tic fmt-string /
  stack overflow), CVE-2018-19211 (NULL deref `_nc_parse_entry`),
  CVE-2019-17594/17595 (heap OOB in `_nc_find_entry`/`convert_strings`),
  **CVE-2023-29491** (local privesc: setuid programs trusting `$TERMINFO`).
  `#![forbid(unsafe_code)]` kills the spatial-overflow class — but native **has a
  terminfo parser** (a fuzz target), and the **trust-model** CVE (env-controlled
  terminfo under setuid) is a *policy* bug Rust does not touch. native's loader
  trust policy for `$TERMINFO`/`$TERMINFO_DIRS` is undocumented. **S1, open.**
- **SEC-02 · Escape-sequence injection is unchanged by Rust.** Writing untrusted
  data emits attacker-controlled escapes (OSC title/clipboard-52, DCS, `\e[?…`
  mode changes). Memory safety does nothing here; "safe Rust" must not imply
  "escape-safe". **S1, open.**
- **SEC-03 · DoS via crafted terminfo/tparm.** Rust prevents OOB but a panic = DoS
  and an unbounded `%`-loop = hang; `tparm` is a parser on possibly-untrusted
  capability strings. **S2, open.**
- **SEC-04 · Format-string vulns vanish** (compile-checked macros, no runtime
  `%n`) — a win that also means an app *can't* accept a runtime format string even
  if it wanted to. **S3, open (by design).**

## 10. Byte-level output optimizer & doupdate (the reason curses exists)

> The whole point of curses is to emit the **fewest bytes** to update a screen.
> native reproduces the full `mvcur` cursor optimizer byte-exactly (OPT-01 **CLOSED**)
> and a single-field repaint seed; the screen-diff (`doupdate`) is the largest
> remaining reconstructed-parity gap.

- **OPT-01 · `mvcur` cost model. ✅ CLOSED (byte-exact).** ncurses chooses CUP vs
  HPA vs repeated `cuf1` vs `cr`+motion vs `home` via `onscreen_mvcur`'s tactic
  enumeration scored by `relative_move` with the **static sample-parameter cost
  model** precomputed in `_nc_mvcur_init` (`hpa=vpa=5`, `cup=8`, `cuf1=cuu1=3`,
  `cub1=cr=1`, `home=3`; costs are character counts because every term is scaled by
  the same `_char_padding`), with a strict-`<` tie-break so the lower-numbered tactic
  wins. The earlier "`d=1`→`cuf1`, `d=2..7`→HPA, `d≥8`→CUP" rule was the *observable
  shadow* of this cost model; the model is the real generator. `cursor::mvcur` now
  reproduces it exactly for the **public `mvcur(3)` path (`ovw=FALSE`)** — short
  forward moves use `cuf1`/`hpa`, never overwrite-with-spaces. **Verified byte-exact
  for all 23409 pairs** of a 153×153 sampled `(from → to)` grid captured from real
  libncurses 6.4 on an 80×24 xterm pty (court `NCURSES.MVCUR`, fixture
  `tests/fixtures/mvcur_matrix.txt`, offline replay `tests/mvcur_matrix.rs`). The one
  residual is **OPT-01a** below. **S1, resolved/byte-exact.**
- **OPT-01a · `mvcur` overwrite branch (`ovw=TRUE`). ✅ CLOSED.** `relative_move` has an
  *overwrite* path (lib_mvcur.c lines ~678–701) that, when ncurses knows the on-screen
  cells match the desired characters, advances the cursor by **rewriting those
  characters** instead of emitting motion (e.g. a forward move of one column over a
  known space is emitted as a literal `" "` rather than `cuf1`; a backward move becomes
  `\r` + overwrite of the leading run). The public `mvcur(3)` entrypoint passes
  `ovw=FALSE`, so this never fires there (and the OPT-01 matrix is captured through it);
  `doupdate` calls the optimizer with `ovw=TRUE`. native now reproduces it: `cursor::`
  `mvcur_ovw(from, to, want)` threads a `want(row,col)` hook (the `WANT_CHAR` desired
  screen) into the forward-horizontal cost/emit, and `update::Screen`'s `GoTo` supplies
  it. This closed every residual `NCURSES.DOUPDATE` divergence. **S1, resolved.**
- **OPT-02 · `doupdate`/`TransformLine`. ✅ PARTIAL (plain-text path byte-exact, 29/29).**
  native reproduces the real `tty_update.c` machinery —
  `doupdate`/`ClrUpdate`/`TransformLine`/`PutRange`/`EmitRange`/`ClrToEOL`/`ClrBottom`
  with the exact static costs (`_el_cost=3`, `_el1_cost=4`, `_ech_cost=5`,
  `_rep_cost=6`, `_inline_cost=5`, `_ich_cost=_dch_cost=5`) — for the **plain-text
  (`A_NORMAL`, no color)** path, reusing the byte-exact `mvcur`/`mvcur_ovw` for cursor
  motion (`update::Screen`). Verified **byte-exact against real ncurses on all 29
  scenarios** of court `NCURSES.DOUPDATE`: clear+paint, same-length overwrite, multi-line
  diff, scattered runs, the leading-blank GoTo-vs-emit reposition, trailing `clr_eol`,
  `clr_bol`, `clr_eos`-bottom, `ich`/`dch` insert/delete shifting, `ech` and `rep`
  run-coalescing, the `ovw=TRUE` GoTo overwrite (OPT-01a), and the **right-margin
  auto-wrap corner** (`auto_right_margin` + `eat_newline_glitch`: filling the last column
  wraps the cursor to the next line, overflow wraps onto it, and a move from the magic
  margin forces an absolute `cup` since relative motion is unreliable there). **Scroll
  optimization ✅ done (single contiguous moved band, `_nc_scroll_optimize`/`_nc_scrolln`):**
  reproduced byte-exact across three cases. (1) **Whole-screen** (`top=0`, `bot=maxy`):
  GoTo(bottom)+`ind`/`indn` up (`\r\n` / `\r\E[NS`), GoTo(top)+`ri`/`rin` down
  (`\E[H\EM` / `\E[H\E[NT`). (2) **Bottom-anchored** (`bot=maxy`, `top>0`): GoTo(top)+`dl1`/`dl`
  up, GoTo(top)+`il1`/`il` down, with **no `csr`** (deleting/inserting lines at `top` scrolls
  the region `[top, maxy]`). (3) **Confined mid/top region** (`bot<maxy`):
  `csr(top,bot)` (`\E[<top+1>;<bot+1>r`) — which homes the terminal cursor, so the following
  GoTo is an absolute `cup` — then the `ind`/`indn`/`ri`/`rin` scroll, then a `csr` reset to the
  full screen (cursor invalidated again, so the park is absolute too). The region grows through
  matching rows (including blank-to-blank), so a shift with only blank rows below extends to the
  bottom and degrades to whole-screen / bottom-anchored; it is **confined to a `csr` region only
  when non-blank *static* content sits below the band** (the `_nc_hash_map` behaviour for one
  band). `curscr` is shifted in place over `[top, bot]` so the residual diff is empty. A
  non-blank line must actually move into view to scroll — content that merely becomes blank is
  cleared (the cost decision). **Multi-region (`_nc_hash_map`) ✅ done:** the optimizer scans
  top-to-bottom and emits *each* shift band as its own scroll, applying it to `curscr`, so
  TransformLine then paints only the genuine edits — two independent bands, opposite-direction
  bands, and a band beside an unrelated edit are all reproduced byte-exact. Verified **byte-exact
  against real ncurses on all 19 scenarios** of court `NCURSES.SCROLL.OPTIMIZE` (`admitted_match`):
  whole-screen up 1/2/3 + down 1/2, partial-content, the clear-not-scroll case, the partial-region
  band scrolls (`region_mid_up1/up2/down1/down2`, `region_top_up1/down1`, `region_bot_up1/down1`),
  and the multi-region cases (`multi_two_bands_up1`, `multi_up_top_down_bot`,
  `multi_scroll_plus_edit`). *Grounded:* the residual "fixup" bytes once read as intrinsic were a
  grounding-harness artifact (`mvaddstr(row,"")` is a no-op that left stale rows); with vacated rows
  actually cleared the scroll bytes are clean and deterministic, and the csr-region cursor
  invalidation was derived from the captured absolute-`cup` after each `csr`. **Still open:** the
  **`idl` overwrite-residual path** — a single-line *insert/delete* that leaves *new* content in the
  vacated row (ncurses uses `il`/`dl` and keeps the old line in its `curscr` model, emitting a
  trailing residual); the crate abstains (the vacated row is non-blank) and redraws those rows
  instead. The **bottom-right corner no-scroll write** is now reproduced
  (court `NCURSES.STDSCR.DRAW`): writing the last cell of the last row is bracketed by `rmam`
  (`\e[?7l`) / `smam` (`\e[?7h`) to suppress the auto-margin scroll, and the cursor stays on the
  last column (a known position, so the next move is relative, not an absolute `cup`).
  **S1, partial/courted (scroll optimization — whole-screen, bottom-anchored, confined csr region,
  and multi-region `_nc_hash_map` — reproduced byte-exact; only the `idl` overwrite-residual path
  open).**
- **OPT-03 · SGR/attribute emission strategy. ✅ PARTIAL (sgr + ANSI color byte-exact).**
  native reproduces ncurses's `vidputs` (`lib_vidattr.c`) and `_nc_do_color`
  (`lib_color.c`) transition state machine in `vid::Vid`, built on the byte-exact
  `tparm` (for `set_attributes`/`sgr`) and the color `Palette` (`pair_content`): an
  attribute change emits `tparm(sgr, …)` (which resets color) and the pair is re-applied
  via `_nc_do_color` (`op` to the default pair, then `setaf`/`setab`); a pure color
  change emits only the color transition; the `PreviousAttr`/`SCREEN_ATTRS` state and the
  `fix_pair0` (no `use_default_colors`) default white-on-black are tracked exactly. Wired
  into `doupdate`'s `UpdateAttrs` (PutChar / clr_eol / clr_bol / clr_eos / ech / rep /
  cleanup) and the `mvcur` `ovw` overwrite is gated to the current attribute. **Verified
  byte-exact against real ncurses** for `A_BOLD`/`A_UNDERLINE`/`A_REVERSE` and the ANSI
  color pairs across the attributed `NCURSES.DOUPDATE` scenarios (xterm has `msgr`, so
  motion needs no attr reset). The colored/attributed **first paint** (the `ClrUpdate` path, not
  just the diff) is now courted byte-exact too (court `NCURSES.DOUPDATE.COLOR_PAINT`:
  bold/underline/reverse/color-pair text). `ncv` masking is modeled (linux court). **`A_ALTCHARSET`
  (line drawing) ✅ done:** altcharset cells emit
  `\e(0`/`\e(B` via the sgr `p9` (and `rmacs` on the A_NORMAL exit), the `coloron` flag is
  tracked (`start_color`), and `mvcur`'s turn-off-then-restore of altcharset around a move is
  reproduced — so `box`/`border`/`hline`/`vline` are byte-exact (court `NCURSES.CURSES`, with and
  without `start_color`). The colored *background* **first paint** (the ClearScreen color-init) is
  now reproduced too: `ClearScreen` does `UpdateAttrs(bkgd)` before `clear_screen` so the `bce`
  clear fills with the background color, then `ClrBottom`/`TransformLine` paint over it -- byte-exact
  (court `NCURSES.DOUPDATE.BG_PAINT`). **Still open:** the individual on/off path (terminals without
  `sgr`), attribute coalescing edge cases, magic-cookie (`xmc`) handling, italics, and
  `use_default_colors`/ext-color. **S1, partial/courted.**
- **OPT-04 · remaining output-shaping caps.** `rep` (repeat_char) and `ich`/`dch`
  insert/delete are reproduced (OPT-02). **Colored backgrounds (`wbkgd`) ✅ done:** the
  `Window` carries a background cell (`bkgdset` -- writes OR its attribute, clears take it),
  matched byte-exact (court `NCURSES.CURSES`, `wbkgd` window). **`bce` (back_color_erase)
  ✅ done:** xterm has `bce`, so `can_clear_with` now treats a *color-only* blank as
  clearable (only the non-color attribute bits must be clear) -- clearing a colored region
  emits the background-color SGR then `clr_eol`/`clr_eos` (the terminal's erase fills with
  it) rather than painting spaces. Verified byte-exact (court `NCURSES.DOUPDATE`,
  `bce_clrtoeol_colored_bkgd` / `bce_clrtoeol_two_lines`). The **auto-margin wrap**
  (`am`/`xenl`) corner is reproduced (OPT-02). The colored *first-paint* ClearScreen color-init is
  now reproduced too (court `NCURSES.DOUPDATE.BG_PAINT`: `UpdateAttrs(bkgd)` before `clear_screen`,
  the `bce` fill, then `clr_eos`/`clr_eol` over the trailing/content rows). Still open: **hardware
  tabs** (`ht`/`hts`). **S1, partial/courted.**
- **OPT-05 · `clearok`/`leaveok`/`immedok`/`idlok`/`idcok` output effects.**
  **`clearok` ✅ done:** `clearok(win, TRUE)` (and `clear`/`wclear`) forces the next
  `doupdate` to clear + repaint from scratch (`ClrUpdate`), reproduced byte-exact (court
  `NCURSES.DOUPDATE`, `clearok_full_repaint`). **`leaveok` ✅ done:** `leaveok(win, TRUE)`
  suppresses the end-of-`doupdate` cursor park (the cursor is left wherever the last emit
  put it), reproduced byte-exact (court `NCURSES.DOUPDATE`, `lo` op). The scroll/`csr`
  machinery these toggles gate -- including the multi-region `_nc_hash_map` case -- is now
  reproduced (OPT-02). Still deferred: `immedok`, and `idlok`/`idcok` for the `idl`
  overwrite-residual line insert/delete path (OPT-02's one remaining open case). **S1, partial.**

## 11. Input / key / mouse subsystem (deterministic core done; rest is boundary)

- **INP-01 · The key trie. ✅ PARTIAL (decoder byte-exact).** ncurses builds a
  per-SCREEN trie from `key_*` caps + `define_key`; `wgetch` answers exact-match→code,
  **prefix-of-longer→keep waiting**, no-match→emit bytes. native reconstructs the static
  queries (`KeyMap::key_defined`/`has_key`, courts `NCURSES.KEY.DEFINED`/`KEYNAME`) **and
  the runtime decode**: `KeyMap::decode` maps a fully-buffered byte stream to the `KEY_*`
  code sequence `wgetch` returns (longest-match trie traversal), verified byte-for-byte
  against a real ncurses getch loop over a pty for arrows, F1–F12, navigation keys, and
  mixed text/keys (court `NCURSES.INPUT`). The **live read loop is also built**: the
  `ncurses-terminal` crate's `RawMode` + `Keys` drives the decoder over a real raw tty, and
  the native `getch` binary returns the same codes as a real ncurses `getch` loop end-to-end
  (court `NCURSES.INPUT.LIVE`). **Still open:** the **timed wait** for a lone trailing ESC
  (TIM-02). **S1, partial/courted.**
- **INP-02 · Two independent timers conflated risk.** (a) per-window read wait
  (`nodelay`/`timeout` ms / `wtimeout` / `halfdelay` tenths-of-seconds); (b) intra-
  sequence ESC disambiguation (`ESCDELAY`/`notimeout`). native models (b) as a fixed
  inter-byte window (keycodes courted, TIM-02); the per-window read-wait timers (a) are
  not yet modeled. **S1, boundary.**
- **INP-03 · `define_key`/`keyok`/`keybound` stack semantics.** `define_key`
  *pushes* (LIFO) non-destructively; `keybound(code,n)` indexes the stack; `keyok`
  toggling to the *same* state returns ERR (idempotent-fails); disabling a key that
  prefixes another must not break the longer key. native classifies these n_a.
  **S2, open.**
- **INP-04 · Echo is curses-level, not kernel-level.** ncurses disables the
  driver's echo and echoes into the window itself (tracking the cursor model);
  erase/`KEY_LEFT`/`KEY_BACKSPACE` editing. native (no tty) does not. **S1, boundary.**
- **INP-05 · `keypad` emits `smkx`; function keys decode only when keypad set;
  `KEY_MOUSE`/`KEY_RESIZE` rules** (mouse needs keypad; resize doesn't). **S1, boundary.**
- **INP-06 · Single per-SCREEN input FIFO + separate mouse queue; `ungetch`/
  `unget_wch`/`ungetmouse` ordering; `flushinp`; `typeahead(fd)`.** native: n_a.
  **S2, boundary.**
- **MOUSE-01 · Protocols & encoding.** xterm modes 1000/1002/1003/**1006 SGR**
  (no 223-col limit); legacy `\e[M`+3 offset-by-32 bytes; negotiated via `XM`.
  native reconstructs only the coordinate geometry (`wenclose`/`wmouse_trafo`,
  court `NCURSES.MOUSE.TRAFO`); **no SGR decoder, no mask negotiation**. **S1, partial.**
- **MOUSE-02 · Click synthesis is ncurses' job, not the terminal's.** press/
  release→click/double/triple via `mouseinterval` (default ~166 ms / 1/6 s); the
  terminal never sends "clicked". `mousemask` returns the *reduced* supported mask;
  coordinates are **stdscr-relative**. native: n_a. **S1, boundary.**
- **MOUSE-03 · `has_mouse` is runtime state** (depends on `mousemask` having
  enabled tracking), **not** a terminfo property — so it cannot be reconstructed
  from the entry. native classifies it n_a (recorded finding). **S2, recorded.**
- **INP-07 · Bracketed paste** (`\e[?2004h`, `\e[200~`/`\e[201~`) is opt-in via
  terminfo/`define_key`; there is **no** built-in `KEY_PASTE` pseudo-key. native:
  absent. **S3, open.**
- **INP-08 · `keyname`/`unctrl` C1 & meta forms.** C1 bytes (128–159) shown with
  `~` prefix outside meta mode, `M-` form inside; native `keyname` handles the
  `M-`/`^X`/`KEY_*` forms (court `NCURSES.KEYNAME`, codes −1..600) but the C1 `~`
  vs `M-` mode dependence and `unctrl` are not separately modelled. **S2, partial.**

## 12. Lifecycle / modes / soft-keys subtleties (research-surfaced)

- **LIFE-01 · `initscr` is `newterm` + stream wiring**, and **`initscr` exits the
  process on failure** while `newterm` returns NULL — an asymmetry a `Result`-only
  port erases. native: framing constants only (divergent). **S1, recorded.**
- **LIFE-02 · `endwin` is suspend, not destroy** (resumed by `refresh`/`doupdate`
  re-running `reset_prog_mode`+`smcup`); `delscreen` is the separate teardown; and
  `endwin` twice without an intervening refresh is `ERR`. native: divergent
  framing, no resume model. **S1, recorded.**
- **LIFE-03 · `filter()` mutates the loaded terminfo** (LINES=1; suppresses
  `clear`/`cud1`/`cud`/`cup`/`cuu1`/`cuu`/`vpa`; suppresses `ed` *only if* `bce`;
  rewrites `home`←`cr`); `nofilter()` undoes it. native: n_a. **S2, open.**
- **LIFE-04 · `use_env`/`use_tioctl` size-resolution precedence** (terminfo →
  ioctl → `$LINES`/`$COLUMNS`, with the F/T vs T/T distinction). native: n_a. **S2, open.**
- **LIFE-05 · `use_default_colors`/`assume_default_colors` and color `-1`** require
  `orig_pair`/`orig_colors`, called after `start_color`; a port using unsigned
  color indices can't represent `-1`. native: n_a (default-pair `(7,0)` only). **S2, open.**
- **LIFE-06 · `slk_init` format codes** (0=3-2-3 8×8, 1=4-4, 2=PC 4-4-4 12×5, 3/4
  index-line) each change label count/width and **shrink LINES**; hardware soft
  labels (`nlab`/`lh`/`lw`/`smln`/`rmln`) vs the software line-steal path are
  entirely different. native reconstructs only `slk_set`/`slk_label` storage at the
  default 8×8/80-col width (court `NCURSES.SLK`); formats 1–4, hardware labels, and
  the LINES coupling are gaps. **S2, partial.**
- **LIFE-07 · `getsyx`/`setsyx` macros & `leaveok`** (`(-1,-1)` when leaveok;
  ripped-off top lines counted in `y`). native: macro surface absent. **S3, open.**
- **LIFE-08 · `curses_version()` third field is a YYYYMMDD date**, not a semantic
  patch integer (string parsers mis-parse). native classifies `curses_version`
  n_a. **S3, recorded.**

## 13. Determinism, UB→defined, layout, misc

- **UB-01 · OOB window writes**: C may return `ERR` or compute a bad index; Rust
  bounds-checks (panic/`Err`) — fuzzing divergence. **S2, open.**
- **UB-02 · Hostile terminfo**: malformed entries that C "worked" on (or
  crashed/CVE'd on) become clean `Err` in Rust — security win, fidelity loss. **S2, open.**
- **UB-03 · `unctrl`/control-char rendering & `char` sign-extension** are platform/
  locale-dependent in C, fixed in Rust — a reproducibility *improvement* that
  breaks bug-for-bug match. **S3, open.**
- **AL-01 · struct padding/alignment** (`cchar_t`, `MEVENT`, `WINDOW`) — only
  `#[repr(C)]` matches; anyone `sizeof`/`memset`-ing breaks. **AL-02 · `wchar_t`
  width platform variance** (4 B Linux/macOS, 2 B Windows) vs Rust `char` 4 B. **S2, open.**
- **MISC-01 · `scr_dump`/`scr_restore`/`putwin`/`getwin` file format** is a
  version-tagged ncurses-private binary; native classifies these n_a (file I/O,
  not terminal output) — so **no C↔Rust saved-screen interop**. **S2, recorded.**
- **MISC-02 · Environment surface**: `$TERM`, `$LINES`/`$COLUMNS`, `$ESCDELAY`,
  `$TABSIZE`, `$NCURSES_NO_PADDING`, `$NCURSES_NO_UTF8_ACS`, `$BAUDRATE`, `$CC`
  (command char), `$NCURSES_TRACE`, `$TERMINFO`/`$TERMINFO_DIRS`. native honours
  only the terminfo-DB path vars. **S2, open.**
- **MISC-03 · `nl`/`nonl` output effect removed in ncurses 6.2** (input-only now) —
  a version-specific behaviour the admitted 6.4 build already reflects. **S3, open.**

## 14. Methodology / meta gaps (the ledger about the ledger)

- **META-01 · "resolved" ≠ "reimplemented" ≠ "behaviour-verified".** 481 resolved
  but only 32 `full`; outsiders collapse "100% resolved across 31 groups" into
  "ncurses done". The status vocabulary is the antidote, but the headline invites
  the error. **S3, recorded** (README states the split).
- **META-02 · Denominator framing** (see STRUCT-03): the true "full ncurses"
  surface (macros, `_nc_*`, panel/menu/form, tools, wide) is several × larger.
  **S3, recorded.**
- **META-03 · Freshness gating proves *internal* consistency, not *external*
  fidelity.** `cargo xtask check` verifies docs↔models↔symbols↔receipts; it cannot
  detect that a court captured one build while a claim names another (the claim is now
  pinned to the verified build, ncurses 6.4), or that a "match" is byte-equal only on a
  trivial case. Green CI ⇒ self-consistent, **not** ⇒
  ncurses-accurate. **S2, recorded.**
- **META-04 · Oracle provenance/freshness decay.** Captured facts are a snapshot of
  one ncurses build + distro patches + TERM + locale + tty settings, with no TTL
  vs upstream releases. Needs a *freshness-of-oracle* axis, not just doc-freshness.
  **S2, open.**
- **META-05 · Clean-room provenance is asserted, not attested.** The clean-room
  claim rests on process evidence (capture logs, no-source attestation) that should
  be preserved as an audit trail. **S3, open.**
- **META-06 · Operational "why" / terminal folklore unported.** ncurses encodes
  decades of ecology — which terminals lie about caps, why `sgr0` is emitted when,
  magic-cookie handling, `xenl`/auto-margin quirks, Linux-console `acsc` specials.
  A from-API reconstruction can resolve every *function* and miss every *quirk*.
  **S2, open.**
- **META-07 · The quirk/glitch corpus** (`xmc`, `am`/`xenl` corner avoidance,
  `bce`, `ich`/`smir` insert quirks, `khome`-vs-`kh` variants) lives in code paths,
  not the API list. **S2, open.**

## 15. Tooling / oracle harness gaps (this repo's own instruments)

- **TOOL-01 · `mvcur` matrix capture: NUL marker through `putp` does not split. ✅ FIXED.**
  The comprehensive `mvcur` oracle capture first used `putp("\0\0")` markers to
  bracket per-call output through the SP buffer; **`tputs`/`putp` swallow/short-circuit
  on NUL**, so the segments did not separate (0 captured). *Fix applied:* a non-NUL
  printable marker — SOH `\x01\x01\x01`, which `tputs` passes through unchanged — now
  brackets every call, and the whole 153×153 grid flushes in order at `endwin`. This
  unblocked OPT-01 (now byte-exact). One **second-order artifact** surfaced and is
  itself handled: the bulk run can interleave the `endwin` teardown (`smcup`+scroll-
  region+`rmcup`) into the *final* captured cell; the court (`NCURSES.MVCUR`) detects
  any segment containing `\x1b[?1049` and **re-captures that pair in isolation**, so
  the recorded oracle value is always authoritative. **S2, fixed.**
- **TOOL-02 · No timing/padding oracle.** The pty harness records bytes, not
  inter-byte delays; `tputs` padding/baud effects (TIM-01) are unverifiable by a
  byte-equality court. **S2, open.**
- **TOOL-03 · No live-tty input / resize / wide-char oracles.** Interactive
  `getch`, `KEY_RESIZE`/`resizeterm`, and wide rendering have no output to compare
  (INP-*, SIG-01, LOC-*). **S2, open.**
- **TOOL-04 · No full-screen `doupdate` byte oracle.** Courts are single-field;
  the min-byte diff engine (OPT-02), the behaviour that actually matters, is
  unmeasured. **S1, open.**
- **TOOL-05 · No multi-terminal *byte-output* oracle.** Terminal ecology is
  verified for terminfo *reads* (9 terminals) and queries, but not for *emitted
  bytes per TERM* (the design goal). **S1, open.**
- **TOOL-06 · No fuzz harness.** The high-value targets (terminfo binary parser,
  `tic` source parser, `tparm` mini-language, input decoder) — exactly where
  ncurses CVEs live — are not fuzzed (`cargo-fuzz`/AFL). **S2, open.**

---

## 16. Output-shaping & color capability gaps (deep cap-level detail)

> Refines §10/§5/§3 with the concrete terminfo-cap behaviours surfaced by the
> panel's output/ACS/attribute and color streams. Each is a distinct way the
> *emitted bytes* (or rendered cells) can diverge per terminal; native models
> none of these cap paths yet (it emits fixed xterm-shaped sequences).

- **OPT-06 · `set_attributes` (sgr) coalescing algorithm. ✅ largely done
  (courted); xmc/italic tail open.** The `vidputs` engine: compute
  `turn_off`/`turn_on` deltas; if `no_color_video>0` strip `ncv`-named attrs (done,
  OPT-08); then one of the emit paths — `A_NORMAL`→`sgr0` (or individual
  `rmso`/`rmul`/…), else the single `tparm(sgr, …9 params)` shot, else
  **turn-off-before-turn-on** via individual caps. native reconstructs this in
  `src/vid.rs` (`sgr`-present path *and* ncurses' no-`sgr` individual-cap branch with
  `rmso`/`rmul`/`rmacs` and `msgr`), reaching **91.5% byte-exact across the whole
  terminfo DB** (`tools/db-coverage/sweep.py attr`; courts `NCURSES.ATTR_SET`,
  `NCURSES.WINDOW.ATTR`). The remaining tail is the **`magic_cookie_glitch`
  (`xmc`)** clamp to `max_attributes` (OPT-12) and **`A_ITALIC`** (OPT-07), both
  still open. **S1, partial/courted.**
- **OPT-07 · `A_ITALIC` is not in `sgr`.** Emitted via `sitm`/`ritm` separately,
  and `sgr0` may reset italic — so italic must be re-asserted after any `sgr0`.
  native does not model italic. **S2, open.**
- **OPT-08 · `no_color_video` (`ncv`) bit-numbering. ✅ DONE (courted).** A bitmask
  of attrs the terminal can't combine with color; bit positions follow **SGR
  ordering, not the `chtype` A_* bit numbers** — using the wrong numbering masks the
  wrong attribute. native translates the SGR-ordered `ncv` mask to `chtype` attrs
  (`ncv_to_attr`: 1→standout, 2→underline, 4→reverse, 8→blink, 16→dim, 32→bold,
  64→invis, 128→protect, 256→altcharset) and strips those bits from the emitted SGR
  while a pair is active. Courted on the Linux console (`ncv=18` strips
  underline/dim, `attr_underline_ncv`). **S1, done.**
- **OPT-09 · `bce` (back-color-erase) explicit fill. ✅ DONE (courted).** When
  `!bce && color_on`, `clr_eol`/`clr_eos`/scroll would erase to the terminal default
  bg, so ncurses must **write explicit blanks in the active color**. native models
  this: a `bce` flag plus `can_clear_with()` decide whether a cell is fast-clearable;
  when color is on and `bce` is absent the fast clears are disabled and ClearScreen
  blank-fills in the active color. Courted via `NCURSES.DOUPDATE.BG_PAINT` /
  `NCURSES.DOUPDATE.COLOR_PAINT` (both `admitted_match`). **S1, done.**
- **OPT-10 · `xenl`/auto-margin bottom-right corner. ✅ DONE (courted).** Writing the
  last cell of the last line would scroll; ncurses avoids it via `smam`/`rmam`
  toggling or write-at-col-2-then-`ich1`-shift, and models the deferred-wrap "cursor
  hang" (`xenl`). native reproduces this in the `PutCharLR` path (`smam`/`rmam`/`ich`
  caps + `am`/`xenl` modelling) — byte-exact against real ncurses (court
  `NCURSES.DOUPDATE`). **S1, done.**
- **OPT-11 · `rep` (repeat_char) run-length output. ✅ DONE (courted).** Used only
  when a run exceeds `_rep_cost`, with count clamped near the right margin to avoid
  wrap. native's `EmitRange` emits the terminal's own `rep` cap via
  `tparm(rep, char, count)` gated on `_rep_cost = 6` (alongside `erase_chars`/`ech`
  at `_ech_cost = 5`) — byte-exact for the plain-text path (court `NCURSES.DOUPDATE`).
  **S1, done (plain path).**
- **OPT-12 · `xmc` (magic cookie glitch).** Attribute changes occupy visible
  cells on HP-style terminals; ncurses limits simultaneous attrs to
  `max_attributes` and accounts for consumed cells. native ignores `xmc`. **S2, open.**
- **OPT-13 · Insert/delete-char output modes. ✅ largely done (courted).** native's
  `TransformLine` takes the insert/delete-character path via `parm_ich`/`parm_dch`,
  gated by `InsCharCost`/`DelCharCost` exactly as ncurses gates them (`ICH_COST`/
  `DCH_COST = 5`) — byte-exact for the plain-text path (court `NCURSES.DOUPDATE`),
  beyond the window-state effect (`NCURSES.WINDOW.STATE`). Still open: the
  `smir`/`rmir`-around-`ich1` and `ich1`/`dch1`×n fallback emission for terminals
  lacking the `parm_*` caps, and `idcok` gating. **S1, partial/courted.**
- **OPT-14 · Hardware tabs.** `ht`/`hts`/`it`/`cbt` for cursor motion; avoided on
  `xt` (destructive-tabs) terminals. native never uses tabs for motion. **S2, open.**
- **OPT-15 · `acs_map` build + `acsc` overlay + A_ALTCHARSET. Mostly done & courted.**
  The runtime **`acs_map[]` global is now exported** (a TINFO symbol in both
  `libncursesw` and the standalone `libtinfo`), and `NCURSES_ACS(c)` indexes it as in
  real ncurses. Verified against the oracle, `acs_map` is the *terminal-independent*
  identity map with `A_ALTCHARSET` set — `acs_map[c] == (A_ALTCHARSET | c)`; it does
  **not** overlay `acsc` (e.g. cygwin maps `a`→0xb1 in `acsc` yet `acs_map['a']` stays
  `'a'|ALT`). The per-terminal `acsc` glyph translation happens at **output time**
  (`set_acsc`: identity base + `acsc` overlay), emitting each `A_ALTCHARSET` glyph
  byte-exact incl. non-identity maps (cygwin→CP437) — courted (`NCURSES.ACS` /
  `NCURSES.LIBTINFO`, `admitted_match`; closes STRUCT-04). **Still open:** the
  emission-time *ASCII fallback* (`'q'`→`'-'`, `'x'`→`'|'`, …) for terminals lacking
  `smacs`/`acsc`, where ncurses drops alt-charset and writes the fallback char; native
  emits the glyph byte. **S1, partial** (extends GLB-05).
- **OPT-16 · `smacs`/`rmacs`/`enacs` emission & `NCURSES_NO_UTF8_ACS`. Partial
  (smacs/rmacs wrapping done & courted).** native wraps alt-charset runs in the
  terminal's `smacs`/`rmacs` (e.g. screen `^N`/`^O`, cygwin `\e[11m`/`\e[10m`) — byte-
  exact, courted (`NCURSES.ACS`). **Still open:** sending `enacs` once at init, and
  the UTF-8-console / GNU-screen / `NCURSES_NO_UTF8_ACS` path that **bypasses
  alt-charset to write Unicode box-drawing code points** (`_nc_wacs`). **S1, partial.**
- **OPT-17 · `bkgd` retroactive re-render vs `bkgdset`.** `bkgdset` affects future
  writes only; `bkgd` immediately rewrites every cell with color-match-vs-mismatch
  merge rules (code 0 treated as space; control bg chars rejected). native
  classifies bkgd deferred (no retroactive re-render modelled). **S2, deferred.**
- **OPT-18 · `\n`/`cud1` vs `onlcr` cursor tracking.** When output `onlcr` is on,
  `cud1="\n"` also resets the column, so it can't be used as pure vertical motion;
  ncurses tracks tty output translation state. native does not model output `\n`
  translation. **S2, open.**
- **COLOR-DETAIL-01 · `setf`/`setb` vs `setaf`/`setab` ordering swap.** `setaf`/
  `setab` take the ANSI ordinal directly; `setf`/`setb` (older HP/Tektronix) use
  blue=1/green=2/red=4 weighting — ncurses maps ANSI→setf (red 1→4, yellow 3→6,
  blue 4→1, cyan 6→3). native only knows the `setaf`/`setab` path; a `setf`-only
  terminal would get red↔blue / yellow↔cyan swapped. **S1, open.**
- **COLOR-DETAIL-02 · HLS `init_color`/`color_content`.** If the `hls` cap is set,
  the triple is **Hue (0–360)/Lightness/Saturation, not RGB**. native models RGB
  defaults only (court `NCURSES.COLOR.CONTENT`); init_color is n_a (can_change=0).
  **S2, open.**
- **COLOR-DETAIL-03 · Direct/true color (`RGB`, `Tc`).** With the `RGB` user-cap,
  `setaf`/`setab` take packed 24-bit color; channel bit-widths come from boolean
  `RGB` (`(max_colors+2)/3` per channel, **blue/green lose bits** if not a multiple
  of 3), numeric `RGB#n`, or string `RGB=8/8/8`; `Tc` is the older boolean. native
  has no direct-color path. **S1, open.**
- **COLOR-DETAIL-04 · 256-pair `chtype` wall.** `COLOR_PAIR(n)` packs the pair into
  **8 bits** → pairs 0–255 only in `chtype`/`attron`; `COLOR_PAIR(259)` wraps to 4.
  Pairs >255 work only via the `cchar_t`/`*_set` (int) API. native's `color_pair`
  uses the 8-bit packing (court `NCURSES.COLOR.PAIR`) but has no wide/`_set` path
  for >255. **S2, partial.**
- **COLOR-DETAIL-05 · Per-screen `COLORS`/`COLOR_PAIRS`/palette.** Each SCREEN has
  its own color flag, palette, and pair table; the `extern` globals alias the
  current screen's. native's `Palette` is an owned value (no global aliasing,
  no per-screen `COLORS`). **S2, open** (extends GLB-01).
- **COLOR-DETAIL-06 · `has_colors` capability gate.** TRUE requires `max_colors`
  AND `max_pairs` AND one of `setf`/`setb`, `setaf`/`setab`, or `scp` — keying off
  only `setaf` wrongly rejects `setf`-only terminals. native's `has_colors` keys off
  `colors>0` only. **S2, partial.**
- **COLOR-DETAIL-07 · `op`/`oc` fast reset & default-color env.** ncurses emits
  `op` (orig_pair) as the cheap "color off" fast path (distinct from "set pair 0"),
  requires `op`+`oc` for the default-color extensions, and honours
  `NCURSES_ASSUMED_COLORS` (`fg,bg`) and interacts with `COLORFGBG`. native models
  none of these. **S2, open.**

## 17. Screen-dump, trace, and environment-surface gaps (concrete)

- **DUMP-01 · scr_dump/restore/init/set + getwin/putwin format.** Authoritative
  spec is `scr_dump(5)`. The ncurses-6 magic is the **32-bit `0x88888888`** — the
  four bytes `\210\210\210\210` immediately followed by ASCII `"ncurses"` + a
  version string (e.g. `6.0.20170415`); the 16-bit `0x8888` form was *rejected* as
  collision-prone (historical SVr4/AIX/HP-UX magic was `0435` = `\001\035`,
  high-byte-first). The modern form is **textual**: WINDOW header fields written by
  name **only if nonzero** (`_cury`,`_curx`,`_maxy`,`_maxx`,`_flags`,`_attrs`,
  `_idcok`,`_delay`,`_regbottom`,`_bkgrnd`), then line-numbered row data with spaces
  as `\s` and renditions as `{BOLD}`/`{REVERSE|C2}` (`C2`=pair value). Subtleties:
  `getwin` **sniffs and falls back** to the legacy ncurses-5 *binary, no-magic*
  dump; the **wide vs non-wide builds produce different dumps**; a restored window
  is **always top-level/pad, never a subwindow**; cells persist the **color-pair
  number, not resolved fg/bg** (wrong colors if the reader's `init_pair` differs);
  `scr_dump` dumps the **virtual** screen (`newscr`); `scr_restore` does **not**
  refresh (needs `doupdate`); `scr_init`/`scr_set` **reject** the dump if the
  terminal was written since the dump **or** if `rmcup`/`nrrmc` is present (so on
  xterm `scr_init` is *usually rejected* → full repaint), and resize (via
  `wresize`) rather than reject on size mismatch; `scr_init` (6.3+) checks
  dimensions. native classifies all of these n_a (file I/O) → **no C↔Rust interop,
  none of the format/sniff/rejection logic**. **S2, recorded** (extends MISC-01).
- **DUMP-02 · `mcprint` printer routing.** Prefers `mc5p` (counted, binary-safe,
  byte-count formatted via `tparm`), falls back to `mc5`+`mc4` (not transparent);
  returns bytes-sent (may be < len), `ERR` with `ENODEV` (no print caps) / `ENOMEM`.
  native: n_a. **S3, open.**
- **TRACE-01 · `curses_trace` (6.2+) vs deprecated `trace()`.** `curses_trace(mask)`
  returns the *previous* mask (and is a return-0 stub when trace is compiled out);
  `trace(mask)` returns void. `TRACE_*` flag values are exact and load-bearing
  (`TRACE_ORDINARY=0x1F` is a 5-bit composite, **not** bit 5; `TRACE_MAXIMUM` is
  derived from `TRACE_SHIFT=13`). The trace file is literally named `trace` in cwd
  and **tracing is refused if that file already exists**. The `_trace*`/`_nc_visbuf`
  helpers use rotating static buffers. native classifies the whole trace group n_a.
  **S3, open** (extends META; trace is a debug side channel).
- **ENV-01 · Privileged-program env filtering.** ncurses ignores exactly
  `{TERMINFO, TERMINFO_DIRS, TERMPATH, HOME}` when privileged — defined as root **or**
  `geteuid()!=getuid()` **or** `getegid()!=getgid()` (setuid **or** setgid). This is
  the CVE-2023-29491 mitigation. native's loader has no such policy. **S1, open**
  (extends SEC-01).
- **ENV-02 · `COLUMNS`/`LINES`** clamp at **512** and **override** the ioctl size by
  default; **`TABSIZE` is NOT read from the environment** (a NetBSD-vs-ncurses trap —
  a faithful port must *not* read `$TABSIZE`); `ESCDELAY` env (capped 30 s, not AIX's
  numbers). **S2, open.**
- **ENV-03 · The `NCURSES_*` behaviour switches.** `NCURSES_NO_PADDING` (all but
  mandatory padding), `NCURSES_NO_HARD_TABS` (no hardware tabs in motion),
  `NCURSES_NO_SETBUF` (**obsolete since 5.9** — modern ncurses owns its buffering, so
  interleaved stdio needs `fflush`), `NCURSES_ASSUMED_COLORS` (`fg,bg`),
  `NCURSES_NO_MAGIC_COOKIE`, `NCURSES_NO_UTF8_ACS` (**tri-state**: unset=auto special-
  case `linux`/`screen`, nonzero=force Unicode ACS, zero=trust advertised ACS),
  `NCURSES_GPM_TERMS`, `NCURSES_TRACE`/`BAUDRATE` (debug builds), `CC` (cmdch, ignored
  unless exactly one char). native honours none of these. **S2, open** (extends MISC-02).
- **ENV-04 · `TERMINFO` inline & search precedence.** `$TERMINFO` may be a directory,
  a hashed `.db`, **or an inline compiled entry prefixed `hex:`/`b64:`** (used only if
  it matches `$TERM`); empty `$TERMINFO_DIRS` entries mean the compiled default dir;
  full order is internal-cache → `$TERMINFO` → `$HOME/.terminfo` → `$TERMINFO_DIRS` →
  compiled default. native reads the directory/letter+hashed paths but not the inline
  `hex:`/`b64:` form or the last-DB cache. **S2, partial.**

## 18. Ecosystem / version-drift / ABI matrix (concrete)

- **ECO-01 · ABI 5 vs ABI 6 (and 7-in-progress).** ABI 6 (default since 6.0,
  2015) widened `cchar_t` (>16 colors), restructured `mmask_t` for a 5th mouse
  button (`NCURSES_MOUSE_VERSION` 1→2), made structs opaque (`NCURSES_OPAQUE` +
  accessor functions), added the `_sp` functions, and set `chtype`/`mmask_t` =
  `uint32_t`, `--with-tparm-arg=intptr_t`. The **most dangerous divergence**: the
  ABI-6 widec extended color pair is carried through the `void *opts` prototype arg
  (invisible at the signature — you can match the C signature and be wrong by
  ignoring `opts`). native targets neither ABI explicitly. **S1, open.**
- **ECO-02 · Build-variant tuple, not a single artifact.** `--enable-widec`
  (`w` suffix, default-on 6.5), `--with-pthread`⇒`--enable-reentrant` (`t` suffix;
  turns `LINES`/`COLS`/`ESCDELAY`/`TABSIZE`/`stdscr`/`cur_term`/`SP` into function-
  macros — must use `set_escdelay()` not assignment), `--enable-ext-colors`/
  `-ext-mouse`/`-sp-funcs` (default-on ABI 6), `--enable-weak-symbols` (drops the `t`
  suffix, so suffix ≠ reentrancy). "Matches ncurses 6.6" is undefined without the
  full configure tuple. **S1, open** (extends BLD-01).
- **ECO-03 · tinfo/termlib & ticlib split.** `--with-termlib` puts the low-level
  layer (`tigetstr`/`tputs`/`setupterm`/`set_curterm`/`tparm`, terminfo reader incl.
  the 32-bit "v2" numeric format) in `libtinfo(w)`; on Debian/Fedora/Gentoo
  `setupterm`/`tgetent`/`cur_term` live there, so `-lncurses` alone fails (needs
  `-ltinfo`). This is the most widely-depended-on layer (bash/less/readline). native
  is monolithic rlib with no `libtinfo` alias. **S1, open** (extends BLD-05).
- **ECO-04 · pkg-config/`*-config` discovery.** `.pc` files (`ncursesw.pc`,
  `tinfo.pc`, threaded/form/menu/panel/tic variants) and `ncursesw6-config` (digit =
  ABI) that downstream `configure`/CMake probe; `--disable-overwrite` puts headers
  under `include/ncursesw/` and the `.pc` `Cflags` must point there. native produces
  none. **S1, open** (extends BLD-05).
- **ECO-05 · Version drift 6.0→6.6 (concrete NEWS).** 6.0 textual screen dump +
  output-buffering ownership + 5th mouse button; 6.1 extended colors stabilized
  (`init_extended_*`, `reset_color_pairs`, `alloc_pair`/`find_pair`/`free_pair`,
  `RGB`/direct color, opaque `TERMINAL`+`TERMTYPE2` 32-bit numbers, `opts` extended
  pair, negative-pair errors); 6.2 SGR-1006 mouse modifiers, `exit_curses`/
  `exit_terminfo`, `curses_trace`, `nl/nonl` lose output effect; 6.3 scr dimension
  check, `KEY_EVENT`/wgetch-events default-OFF; 6.4 bracketed paste (caps only),
  tic NUL rejection; 6.5 `tiparm_s`/`tiscan_s` + tparm provenance hardening, `endwin`
  re-call errors, widec+opaque default-ON; 6.6 conpty driver, separate mouse read/
  write pointers, `use=` limit 32→40. native admits and is verified against **6.4**
  (the claim is now pinned to the build the courts run; FRAME-01 resolved). **S1, recorded.**
- **ECO-06 · Non-ncurses implementations.** BSD curses (termcap, no pads/color,
  obsolete), NetBSD curses (combining chars as a **linked list** vs ncurses' fixed
  `CCHARW_MAX` array; 16-bit numeric caps; different DB search; `wunctrl` signature
  differs; `assert()` vs `ERR`), **macOS ships ncurses 5.7 (2008)** (no extended/
  direct colors, MOUSE_VERSION 1, 5.x dump/ABI — cross-checking against system curses
  on macOS misleads), PDCurses (no terminfo at all). "curses parity" is
  implementation-relative. native targets ncurses-6.x/Linux only. **S2, open** (extends G2).
- **ECO-07 · De-facto interop target.** For maximum real-world compatibility the
  target is `libncursesw.so.6`: widec + ext-colors + ext-mouse + sp-funcs + opaque,
  ABI 6, 32-bit `chtype`/`mmask_t`, int-width extended pairs, v2 (32-bit) terminfo
  numbers, with `ncursesw.pc` (+ `tinfo.pc` alias). native is narrow, non-opaque-via-
  values, rlib-only — distance from this target is itself the headline gap. **S0, recorded.**

## Roadmap to genuine 1:1 (what closing these requires)

1. **`ncurses-terminal` crate (isolated unsafe tty)** — closes IO-01..06,
   SIG-01..03, TIM-02/03, INP-02/04/05/06, ERR-04: real raw/cbreak/echo, getch,
   live initscr/endwin, napms, SIGWINCH. (Crate scaffolded; driver pending.)
2. **`mvcur` cost model ✅ + real `doupdate`** — OPT-01 and TOOL-01 are **closed**:
   the capture is fixed and the cost model is reverse-engineered and reproduced
   byte-exactly (court `NCURSES.MVCUR`, all 23409 grid pairs). Remaining: OPT-01a
   (the `ovw=TRUE` overwrite branch) and OPT-02..05/TOOL-04 — build the `curscr`/
   `newscr` diff with hashmap line-move + scroll + insert/delete optimization,
   courted full-screen against ncurses.
3. **Wide-char (ncursesw) layer** — closes LOC-01..06: `cchar_t`, `add_wch`/
   `get_wch`, `wcwidth` accounting (pinning a Unicode version).
4. **Companion libraries** — panel (**done**: native `libpanel` in `ncurses-cabi` --
   new_panel/del_panel/show/hide/top/bottom/move/replace/above/below/window/hidden/userptr/
   update_panels, a z-ordered deck composited stdscr-ground + bottom-to-top onto newscr and
   rendered by the byte-exact doupdate; byte-identical to system `libpanelw`, `NCURSES.PANEL`
   court) and menu (**done**: native `libmenu` -- new_item/new_menu/post_menu/menu_driver/
   set_menu_win+sub/mark/fore/back/grey/format/current_item/...; post_menu fills the (sub)window
   cells -- mark in back + name/pad/desc in fore/grey/back -- and menu_driver redraws only the
   old+new current; byte-identical to system `libmenuw`, `NCURSES.MENU` court), and form
   (**done**: native `libform` -- new_field/new_form/post_form/form_driver data entry + O_BLANK
   first-edit blanking + REQ_*_FIELD navigation + REQ_DEL_PREV/DEL_CHAR/CLR_* editing; post_form
   draws each field's buffer padded into the (sub)window and form_driver edits the current field's
   buffer + parks the cursor; byte-identical to system `libformw`, `NCURSES.FORM` court). All three
   companion libraries are built natively (closes BLD-04 / STRUCT-03).
5. **CLI tools** — `tput`/`clear` are **done** (native binaries in `crates/ncurses-tools`, byte-identical to the system tools incl. the extended-terminfo `E3` append, `NCURSES.TPUT` court); **`infocmp` is done** (100.000% byte-exact decompile for both `-1` and `-1 -x` across the 2,869-entry terminfo DB, `NCURSES.INFOCMP` court); **`tic` is done** (100.000% byte-exact on both the `infocmp -1 | tic` and `infocmp -1 -x | tic -x` round-trips, `NCURSES.TIC` court); `toe`/… remain (BLD-03).
6. **Native API completeness** — the macro API as Rust methods, panel/menu/form
   modules, wide-char module, and the CLI tools as native binaries (closes
   STRUCT-03); plus a global-state compatibility shim (thread-local `SP`/`cur_term`,
   `LINES`/`COLS` accessors, non-`_sp` overloads — GLB-01..03).
7. **Native C-ABI surface (`crates/ncurses-cabi`)** — `cdylib`/`staticlib` +
   `#[no_mangle] extern "C"` wrappers + `#[repr(C)]` structs + generated `curses.h`
   + `.symver` version script + `.pc` files, so existing C programs can link/load
   it (closes STRUCT-01/02, ABI-*, ECO-01/03/04/07). Engineering, not impossible.
8. **Fuzzing + timing/multi-terminal/wide oracles** (closes TOOL-02..06, SEC-03).

This ledger is the live map; the generated per-function companion is regenerated
and freshness-gated on every change (`cargo xtask check`).
