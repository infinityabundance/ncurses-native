# ncurses-native

A clean-room, dependency-free Rust reproduction of the **observable terminal byte
output** of ncurses 6.4 on one terminal: `TERM=xterm`.

## What this actually is (read this first)

This crate is **not** ncurses, and it is **not** a port of any ncurses C source.
It is a from-scratch Rust reproduction of the *bytes ncurses writes to the
terminal* -- the escape-sequence stream you would capture if you ran a program
against `libncursesw.so.6` (ncurses 6.4) with `TERM=xterm` and recorded the
output under a pty. It was reverse-engineered from behavior (captured facts), not
copied from a source tree. There is **zero ncurses C source** in this repository.

By surface area it reproduces roughly **3-5% of ncurses-the-library**. It is a
**SEED**: the byte-exact nucleus, meant to be grown toward parity. It is **not a
replacement** for ncurses.

The crate is `#![forbid(unsafe_code)]`, std-only, has **no dependencies**, and is
ASCII-only in source.

## Scope

ncurses-native is a **native Rust implementation of ncurses** that aims to build
*everything* natively -- creating, in Rust, the surfaces that do not yet exist in
native Rust. That includes a **native C-ABI surface** (a `cdylib`/`staticlib`
crate with `extern "C"` wrappers, `#[repr(C)]` structs, a generated `curses.h`,
soname/symbol version script, and `.pc` files) so it can be a real drop-in for
existing C programs -- alongside an ergonomic Rust API (the C macro API becomes
Rust methods, e.g. `getyx` -> `win.getyx()`), plus panel/menu/form, the wide-char
path, multi-terminal output, a real `doupdate`, and the CLI tools. The only
deliberate divergence is that we do **not** reintroduce C's memory-unsafety/UB
footguns. See [`docs/gap-ledger.md`](docs/gap-ledger.md) §0 for the full
native-build scope.

The workspace is therefore growing from one crate toward the full stack:
`ncurses-native` (safe core) · `ncurses-terminal` (isolated unsafe tty I/O) ·
`ncurses-cabi` (the native C-ABI/`.so` surface, now including the native `libpanel`, `libmenu`, and
`libform`) · `ncurses-tools` (native `tput`/`clear` binaries, a native `infocmp` decompiler, and a
native `tic` compiler) · and (planned) wide-char crates plus the remaining CLI tools.

The C-ABI surface is real and proven, not aspirational: `ncurses-cabi` builds
`libncurses_cabi.{so,a}`. Its low-level terminfo layer (`setupterm`,
`tigetstr`/`tigetnum`/`tigetflag`, `tputs`/`putp`/`tgoto`, variadic `tparm(...)`)
is a drop-in for the system `libtinfo` (`NCURSES.CABI` court). And a real **curses**
C program -- `initscr`/`start_color`/`init_pair`/`mvaddstr`/`attron`+`attroff`/
`refresh`, plus a `newwin` subwindow composited via `mvwaddstr`/`wattron`/`wrefresh`,
a `box`/`whline` line-drawing window (ACS via `A_ALTCHARSET`), a
`wbkgd` colored-background window, and a `newpad`/`prefresh` pad,
then `endwin` -- linked against `libncurses_cabi` draws **byte-identically** to the
system `libncursesw` (`NCURSES.CURSES` court). And **interactive input works**: an
`initscr`/`keypad`/`cbreak`/`getch` program on the cabi returns the same keycodes as
real ncurses (`NCURSES.CABI.GETCH` court), so a full input+output curses program runs
on native Rust. The **panel library** (`libpanel`) is built natively too: a
`new_panel`/`update_panels`/`top_panel`/`move_panel`/`hide_panel`/`doupdate` program over
`box`+`mvwaddstr` windows composites **byte-identically** to the system `libpanelw` when linked
against `libncurses_cabi` (`NCURSES.PANEL` court) -- a z-ordered deck composited (stdscr ground +
deck bottom-to-top) onto the virtual screen, rendered by the byte-exact `doupdate`. The **menu library** (`libmenu`) is native too: a
`new_menu`/`post_menu`/`menu_driver`/`set_menu_mark` program renders + navigates **byte-identically**
to the system `libmenuw` (`NCURSES.MENU` court) -- post_menu fills the sub-window cells (mark + padded
name/description, current item in the fore attribute) and menu_driver redraws the old+new current.
The **form library** (`libform`) is native too: a `new_form`/`post_form`/`form_driver` program (field
data entry with O_BLANK first-edit blanking, REQ_*_FIELD navigation, and REQ_DEL/CLR editing) renders
+ edits **byte-identically** to the system `libformw` (`NCURSES.FORM` court) -- post_form draws each
field's padded buffer into the (sub)window and form_driver edits the current field + parks the cursor.
And it's a **source-level** drop-in too: a curses program written
to the CPP macro API (`getyx`/`getmaxyx`, `COLOR_PAIR`/`PAIR_NUMBER`, `A_*`,
`ACS_*`, `KEY_*`) compiles unmodified against the generated
[`curses.h`](crates/ncurses-cabi/include/curses.h) and draws byte-identically to
the system `<curses.h>` (`NCURSES.CURSES.HEADER` court; the `WINDOW` struct stays
opaque -- the `getyx` macros use accessor functions). The courts link one and the
same C program against ours and the real library and compare output. The cdylib also
carries SONAME `libtinfo.so.6` and is a working dynamic drop-in (`NCURSES.SONAME`
court: a C program linked against it, resolved via rpath as `libtinfo.so.6`,
matches the system `libtinfo` byte-for-byte), with `pkg-config` files under
[`crates/ncurses-cabi/pkgconfig/`](crates/ncurses-cabi/pkgconfig/). Header:
[`crates/ncurses-cabi/include/tinfo.h`](crates/ncurses-cabi/include/tinfo.h). The **CLI tools** are
native binaries too: `tput`/`clear` (`crates/ncurses-tools`) emit byte-identically to the system
tools across string/number/longname caps and `clear` (`NCURSES.TPUT` court), the latter exercising a
new reader for the *extended* (user-defined, `tic -x`) terminfo section -- e.g. `E3` clear-scrollback.
The same crate ships a native **`infocmp`** decompiler whose `-1` source output is **100.000%
byte-exact across the whole 2,869-entry terminfo database** (`NCURSES.INFOCMP` court): it reconstructs
`_nc_tic_expand` string escaping (the caret-vs-octal length heuristic, the `%`-operator verbatim rule,
the `\E`/`\n`/`\r`/`\0`/`\s`/`\,`/`\^` escapes), power-of-two hex number formatting (`colors#0x100`),
cancelled caps (`name@`), `acsc` glyph-pair sorting, and the full extended (user-defined, `-x`)
section including cancelled `name@` extensions — **100.000% byte-exact for both `-1` and `-1 -x`
across the whole database** (`NCURSES.INFOCMP` court). Its inverse, a native **`tic`**, compiles
source back to the binary form **100.000% byte-identically to system tic** on both the
`infocmp -1 | tic` and `infocmp -1 -x | tic -x` round-trips (`NCURSES.TIC` court): the source parser,
the `_nc_tic_expand` un-escaper, and the binary writer (magic selection, the SVr4 cutoff that drops
ncurses-extension predefined caps without `-x`, cancelled `-2` markers, and the extended-section
offset/string tables).

## Forensic gap ledger

Every gap between original ncurses and ncurses-native is recorded, as a diff, in:

- [`docs/gap-ledger.md`](docs/gap-ledger.md) -- the class-level forensic ledger:
  ~150 distinct gaps across 18 dimensions (memory model, integer/bit layout,
  global state, concurrency, signals, control flow, errors, locale/wide-char,
  timing, I/O, ABI/macros, build/packaging, security/CVEs, the byte-output
  optimizer & `doupdate`, input/key/mouse, lifecycle/modes/slk, screen-dump/
  trace/env, and the ecosystem/version/ABI matrix), each with ncurses behaviour,
  ncurses-native behaviour, gap class, severity, evidence, and what 1:1 requires.
  Produced with a four-specialist research panel cross-checked against the
  ncurses 6.4 man pages, the Rust reference, and the ncurses CVE record.
- [`docs/generated/gap-ledger-functions.md`](docs/generated/gap-ledger-functions.md)
  -- the machine-checked, freshness-gated per-function half: every public C
  function whose status is not `full` (448 / 481), with its gap kind, counterpart,
  and court.

## Parity matrices (generated, freshness-gated)

The full ncurses public C API (481 functions, extracted from the installed
headers by clang) is cataloged against this crate's counterparts in:

- [`docs/generated/port-parity.md`](docs/generated/port-parity.md) -- the
  group-level matrix (1:1 completeness catalog, parity %, status per cluster).
- [`docs/generated/port-parity-functions.md`](docs/generated/port-parity-functions.md)
  -- the per-function gap view.
- [`docs/generated/claim-index.md`](docs/generated/claim-index.md) -- the courts
  and oracle receipts that back each non-scaffold counterpart.

These are **generated** by `cargo xtask gen` from `docs-src/models/*.json` (the
clang C inventory joined with the `syn`-extracted Rust inventory and a curated,
validated counterpart map). `cargo xtask check` is the freshness gate -- it
re-renders in memory and fails if a doc drifted, a counterpart points at a
missing Rust symbol, or a cited court has no receipt. CI and the optional
`.githooks/pre-commit` run it on every change. Every public ncurses C function
(481, across all 31 man-page groups) is now **resolved** -- but "resolved" does
**not** mean "ncurses reimplemented". Read it as two parts:

- **237 reconstructed** (33 full, 174 partial, 22 scaffold, 8 divergent) -- actual
  Rust that emits/derives bytes or reconstructs window/geometry/attribute/colour
  state, terminfo lookups, key decoding, mouse geometry, or soft-label storage,
  each backed by an oracle court.
- **244 classified non-output** (42 deferred = proven to emit nothing, 202 n/a = no
  terminal-output contract: pure queries, tty/termios + input modes, interactive
  input/mouse, library config, diagnostics/trace, printer, file serialization, or
  global state).

So **~49% of the API is *reconstructed*** and the remaining ~51% is an honest,
evidence-backed accounting of functions a byte-output reconstruction has nothing
to emit for (the large n/a share is dominated by the input read/option functions,
which consume terminal input, and by file/state functions). This is a complete
**map**, not a complete library: it is **not** a claim of full ncurses parity,
and the byte-level terminal-output claims remain bounded to the courts (the
init/teardown framing is captured live and matches the in-repo ncurses 6.4
byte-for-byte; other `TERM`s and other ncurses builds are the non-claim).
Per-function detail is in
[`docs/generated/port-parity-functions.md`](docs/generated/port-parity-functions.md).

All thirty-one groups are fully resolved; the reconstructed highlights:

- **Terminfo & termcap** (`curs_terminfo`/termcap, 33/33) -- the substrate: the
  runtime `tigetflag`/`tigetnum`/`tigetstr` (lookup), `tparm` (parameterized
  strings), `tputs`/`putp` (padding), the `setupterm` database loader, and the
  legacy termcap layer (`tgetflag`/`tgetnum`/`tgetstr`/`tgoto`); the global
  `cur_term` managers (`set_curterm`/`del_curterm`/...) are n/a in the owned-value
  model.
- **Refresh & doupdate** (`curs_refresh`, 7/7) -- `doupdate`/`refresh`/
  `wnoutrefresh`/`wrefresh` are a clean-room reproduction of `tty_update.c`
  (`ClrUpdate`/`TransformLine`/`PutRange`/`EmitRange`/`ClrToEOL`/`ClrBottom`) for the
  plain-text (`A_NORMAL`) path (`update::Screen`), reusing the byte-exact `mvcur`;
  **verified byte-exact against real ncurses on all 29 court scenarios**
  (`NCURSES.DOUPDATE`): clear+paint, overwrite, multi-line, scattered runs,
  leading-blank reposition, `clr_eol`/`clr_bol`/`clr_eos`, `ich`/`dch` insert/delete
  shifting, `ech`/`rep` run-coalescing, the `mvcur` `ovw=TRUE` GoTo overwrite, the
  **right-margin auto-wrap corner** (`am`+`xenl`: last-column fill wraps the cursor,
  overflow wraps onto the next line, magic-margin moves force an absolute `cup`), and
  **attribute/color SGR transitions** (`A_BOLD`/`A_UNDERLINE`/`A_REVERSE` and the ANSI
  color pairs) via the `vidputs`/`_nc_do_color` engine (`vid::Vid`, OPT-03). The attribute
  engine is fully terminfo-driven: `set_attributes` (sgr) when present, else ncurses' **individual
  mode-cap path** (`bold`/`smul`/`smso`/`rev`/... with `rmul`/`rmso` exits) that drives the **46% of
  the database with no `sgr`**; all caps are `$<N>`-padded; and a terminal lacking `msgr`
  (move_standout_mode) resets to `A_NORMAL` before each cursor move and restores after (lib_mvcur.c).
  Swept across the whole database (`tools/db-coverage/sweep.py attr`): **91.5% byte-exact**, up from
  34% before this work; pinned by committed fixtures (`ti916-8-132` no-sgr, `wyse520-w` no-msgr,
  `vt220` sgr). The remaining ~8.5% is dominated by the **magic-cookie glitch** (`xmc`: 204 obsolete
  tvi/wyse/adm terminals where an attribute occupies a screen cell) -- understood but honestly
  bounded (its exact byte stream needs ncurses trace tooling; see the gap ledger). The **color** engine (`_nc_do_color`) is terminfo-driven too: `setaf`/`setab` (ANSI)
  or `setf`/`setb` with `toggled_colors` (the SVr4 1<->4/3<->6 swap), `orig_pair`, all padded, and
  gated on the terminal actually having color (a colorless terminal like vt100 emits none). A color
  terminal *without* `bce` can't fast-clear (the clear wouldn't paint the background), so ncurses
  emits a color-init then **blank-fills** -- reproduced here with the full `PutCharLR` bottom-right
  corner (`rmam`/`smam`, `ich`/`smir` insert, or skip) and `am` auto-margin-wrap. Color-capable
  terminals are **97.9% byte-exact** (`tools/db-coverage/sweep.py color`; ansi/pcansi/aixterm/cygwin/
  sun all pass); pinned by `sun-color` (setaf+blank-fill+corner), `emots` (setf+toggled), `vt100`
  (no color). The remaining color tail is exotic (DOS ansi.sys, HP, the DG `ccc`/`initc` palette via `set_color_pair`). Hardware
  `bce` colored-background clears (color-only blanks erased with the bg color via
  `clr_eol`/`clr_eos`), the colored-background **first paint** (the ClearScreen color-init:
  the bg color is set before `clear_screen` so the `bce` clear fills with it -- court
  `NCURSES.DOUPDATE.BG_PAINT`), `clearok`/`clear`/`wclear` full-repaint, the bottom-right
  no-scroll corner, and the **scroll optimization** (`_nc_scroll_optimize`/`_nc_scrolln`:
  whole-screen `ind`/`indn`/`ri`/`rin`, bottom-anchored `dl`/`il`, a confined `csr` region,
  and the multi-region `_nc_hash_map` case -- several independent bands plus interleaved
  edits, 19/19 courted) -- all courted. The only remaining doupdate gap is the `idl`
  overwrite-residual path (a single-line insert/delete leaving new content in the vacated
  row). `redrawwin`/`wredrawln` are proven to emit nothing (deferred to the next
  refresh). The engine is **multi-terminal** and now fully **terminfo-driven**
  (`Screen::from_terminfo` derives the cursor cost model, `clear`/`rep`/`el`/`el1`/`ech`/`idc`
  caps, `acsc`, `sgr`/`sgr0`/`rmacs` and `ncv` from any entry): byte-exact on the Linux
  console (`NCURSES.DOUPDATE.LINUX`) and on **vt100** (`NCURSES.DOUPDATE.VT100`, 24
  scenarios) -- a different cap class (no `hpa`/`vpa`, padded `el`/`el1`, no
  `ech`/`ich`/`dch`) -- and measured **byte-exact on the whole compiled terminfo database (reproduce
  with `tools/db-coverage/sweep.py`): 99.26% of first paints (2543/2562) and 98.95% of incremental
  scene-to-scene diffs (2535/2562)**.
  The `clear`/`el`/`el1`/`ed` shaping caps are
  fully cap-driven: a terminal with no `clear` cap is **not** sent a hardcoded `\e[H\e[2J`; instead
  the crate reproduces ncurses' `ClearScreen` 4-way fallback (clr_eos -> per-line clr_eol ->
  blank-fill), and `GoTo` from an unknown position emits the terminal's own `cup` (or nothing when it
  has none) rather than a hardcoded CSI. The incremental `TransformLine` path is cap-driven too: the
  `bce` terminals force `_el_cost` to 0 (biasing trailing clears toward `clr_eol`), and `erase_chars`
  /`repeat_char` run-coalescing emits the terminal's own `ech`/`rep` cap (e.g. concept's
  `\Er%p1%c...`, not the ECMA `\e[Nb`). Pinned by committed fixtures `tests/doupdate_terminfo.rs`
  (xterm reproduction; `newhp` clr_eos-only, `avatar` clr_eol-per-line, `gt40` blank-fill, `ampex219`
  padded clr_eos; `arm100` bce trailing-clear, `concept` rep, `hp2645` direct reposition). The
  remaining ~1-2% are a degenerate tail: unaddressable hardcopy/dumb terminals (cup-less, needing
  exact auto-margin-wrap modelling), hardware-tab `viewdata`, and padded-clear exotics.
  ACS line-drawing is
  cap-general too (`NCURSES.ACS`): each `A_ALTCHARSET` glyph is translated through the
  terminal's `acsc` map, byte-exact on xterm (identity, `\e(0`), screen (identity, `^N`/`^O`),
  and cygwin (non-identity acsc -> CP437 `q`->0xC4, smacs `\e[11m`).
- **Cursor move & optimizer** (`curs_move`/`mvcur`, 4/4) -- `mvcur` is a clean-room
  reproduction of `lib_mvcur.c`'s tactic enumeration and static cost model, **verified
  byte-exact against real libncurses for all 23409 pairs** of a 153x153 sampled
  `(from -> to)` grid on an 80x24 xterm pty (court `NCURSES.MVCUR`, offline replay
  `tests/mvcur_matrix.rs`); `move`/`wmove` set the window cursor (state matched to
  ncurses by the window-state court). The cost model is **cap-parameterized** (`Caps`)
  and proven byte-exact on a different cap class too: **vt100** (no `hpa`/`vpa`, padded
  `cup`/`cuf1`/`cuu1`) across all 23409 pairs (court `NCURSES.MVCUR.VT100`), and the Linux
  console (`NCURSES.MVCUR.LINUX`). The engine is now fully **terminfo-driven**
  (`Caps::from_terminfo`): every cap's bytes *and* ncurses cost are derived from the loaded
  terminfo and emitted via `tparm`, so it drives an arbitrary terminal -- proven byte-identical
  to the hand-built engine across the full grid for the committed xterm/linux fixtures
  (`tests/mvcur_terminfo.rs`). The **`tputs` padding-byte model** is reproduced too: a no-`xon`
  terminal emits `floor(N_ms * baud / 9000)` pad bytes for each `$<N>` (grounded vs real ncurses,
  pinned by the committed `ampex219` fixture). A sweep over the **whole** compiled ncurses terminfo
  database (reproduce with `tools/db-coverage/sweep.py`) measures the cursor engine **byte-exact on
  99.84% of real terminals (2558/2562 checked)**
  across every cap class -- the `bw`/`cursor_to_ll` tactics with the `xenl` magic-margin gate, the
  `cr`-absent and runtime-screen-width corrections, the `%c`-NUL-to-0x80 tparm fix, the
  **`cup`-gated `NOT_LOCAL` short-circuit** (cup-less LCD/relative-only terminals fall through to the
  local optimizer instead of emitting nothing), the **newline-`cud1` rejection** (a `cud1` whose
  first byte is `\n`, even with padding, is unusable), and the **general `bw` back-wrap** (tactic 5
  to any column, not just the right margin). Each correction is pinned by a committed fixture test
  (`pmcons`, `ncr160wy60pp`, `apple2e`, `hp2626-ns`, `aixterm`, `wy350-wvb`, `vi300-old`,
  `ampex219`). The remaining 4 are a genuine exotic tail: a terminal with a malformed fixed-string
  `vpa` (`apollo`), an Apple-II col-first `cup` with `cr` padding (`apple-80`), and two Tektronix
  graphics terminals where real ncurses itself emits nothing (`tek4113`).
- **Add character** (`curs_addch`, 6/6) and **Add string** (`curs_addstr`, 8/8)
  -- written into a real `Window` cell grid (wrap / clear-to-EOL / bottom clip);
  the resulting grid is matched to ncurses by reading the cells back (`winch`).
- **Wide characters (UTF-8, single- and double-width)** -- each cell holds a Rust
  `char`, and `doupdate` emits its UTF-8 encoding per cell with **no `repeat_char`
  coalescing for multibyte glyphs**, matching `ncursesw`'s `EmitRange` **byte-exact**.
  Stable East-Asian **double-width** glyphs (CJK ideographs, Kana, Hangul, fullwidth
  forms) fill a cell plus a padding cell and advance two columns, also byte-exact
  (court `NCURSES.WIDECHAR`: mixed Latin/Greek/Cyrillic/symbol text, identical
  multibyte runs, and CJK/Kana/Hangul/fullwidth lines, against real `libncursesw`
  under a UTF-8 locale; `cabi` `addstr`/`waddstr` decode UTF-8 input). **Zero-width
  combining marks** attach to the preceding base cell (up to `CMAX`=4, `cchar_t.chars[1..]`)
  and emit as UTF-8 right after the base, byte-exact (court `NCURSES.WIDECHAR` `combining_*`,
  LOC-03). Honest open gap (LOC-04): version-skewed widths (emoji, treated as width-1).
  The right-margin wide-glyph wrap is reproduced and courted
  (OPT-02). The **wide function API** -- `cchar_t` + `setcchar`/`getcchar` and the
  `add_wch`/`addwstr` families -- is wired in the C-ABI and draws byte-identically to
  `libncursesw` (court `NCURSES.WIDECHAR.CABI`). Wide *input* (`get_wch`/`wget_wch`) is
  wired too: it assembles UTF-8 into a codepoint (`OK`) and reports function keys as
  `KEY_*` (`KEY_CODE_YES`), matching `libncursesw` (court `NCURSES.WGET_WCH`). Wide
  read-back (`in_wch`/`win_wch`/`mvin_wch` + `getcchar`) returns the cell's wide char,
  attributes (incl. color), and pair byte-identically (court `NCURSES.WIN_WCH`) -- the
  full wide cell API. Combining marks (`cchar_t.chars[1..]`) remain open.
- **Insert / delete** (`curs_insch`/`insstr`/`deleteln`, 22/22) -- `insch`/`delch`
  (line shift), `insstr`, `insertln`/`deleteln`/`insdelln` (line shift) on the
  cell grid, matched to ncurses by readback.
- **Scroll** (`scrollok`/`wscrl`/`scroll`/`scrl`) -- shift a window's cells up/down
  (vacated rows take the background), matched to ncurses by readback (WINDOW.STATE).
  This is the window cell operation; the byte-level `doupdate` scroll *optimization*
  (`_nc_scroll_optimize`/`_nc_scrolln`) is reproduced byte-exact for any moved band --
  whole-screen, bottom-anchored (`dl`/`il`), a confined `csr` region, and multiple
  independent bands with interleaved edits (`_nc_hash_map`, court
  `NCURSES.SCROLL.OPTIMIZE`, 19/19); only the `idl` overwrite-residual path remains a
  documented open gap (OPT-02).
- **Read from window** (`curs_inch`/`instr`, 20/20) -- `inch`/`winch`/`instr`
  reconstructed (chars); the `inchstr` chtype-attribute variants are scaffold
  (char cells only, attribute bits not modelled).
- **Borders & lines** (`curs_border`, 11/11) -- `box`/`border`/`hline`/`vline`
  draw into the cell grid (ACS line-drawing defaults `l q k x m j`), matched to
  ncurses by readback.
- **Window create & manage** (`curs_window`, 19/19) -- `newwin`/`mvwin`/`wresize`/
  `dupwin`/`derwin`/`subwin` and the `getbegyx`/`getmaxyx`/`getparyx` geometry
  getters, matched to ncurses by a geometry court (shared-cell child windows not
  modelled; `delwin`/`wgetparent` n/a in the owned-value model).
- **Attributes** (`curs_attr`, 27/27) -- `attron`/`attroff`/`attrset`/`chgat`/
  `standout` carried on the cell grid, matched to ncurses' `A_*` bits via `winch`
  (`vidattr`/`vidputs` are scaffold byte producers; colour-pair bits out of scope).
- **Terminal queries** (`curs_termattrs`, 17/17) -- `longname`/`termname`/
  `has_ic`/`has_il` reconstructed from terminfo and matched across 9 terminals;
  `termattrs`/`baudrate`/`erasechar`/`killchar`/`curses_version` are screen/tty/
  identity state (n/a in the terminfo-only substrate).
- **Background** (`curs_bkgd`, 5/5) -- `bkgd`/`bkgdset`/`wbkgd`/`wbkgdset` proven
  to emit nothing (deferred to refresh); `getbkgd` is a pure query (n/a).
- **Touch & sync** (`curs_touch`, 11/11) -- touch/untouch and the sync functions
  proven to emit nothing (mark dirty, repaint at refresh); `is_*` are queries (n/a).
- **Scroll & output options** (`curs_scroll`/outopts, 16/16) -- the option flags,
  scroll-region setters, and scroll ops proven to emit nothing; `is_*`/`wgetscrreg`
  are queries (n/a).
- **Modes & terminal state** (`curs_kernel`, 18/18) -- `curs_set` (cursor
  visibility, civis/cnorm/cvvis) reconstructed from terminfo; the tty-mode
  save/restore, `napms`, and `ripoffline` are n/a (termios/timing/layout state).
- **Library config & util** (`curs_util`, 21/21) -- config setters/queries,
  output timing, and printer routing -- all n/a (no terminal-display output).
- **Bell** (`curs_beep`) -- `beep`/`flash` reconstructed to full byte parity.
- **Erase** (`curs_clear`) -- 6 byte producers oracle-pinned, 3 deferred, 1 n/a.
- **Overlap** (`curs_overlay`) -- `overlay`/`overwrite`/`copywin`, all proven to
  emit nothing.

"Resolved" covers both reconstructed byte producers and evidence-backed
non-output classifications (deferred = proven empty-output; n/a = no
terminal-output contract) -- it is not a claim of full API equivalence.

## Terminfo substrate

`src/terminfo/` is a native, dependency-free reader for the compiled terminfo
binary format. It parses the admitted xterm entry and reconstructs the lookup
primitives `tigetflag`/`tigetnum`/`tigetstr` with ncurses' exact return
semantics. The capability-name tables (`src/terminfo/caps.rs`) are generated from
ncurses' own ordered name arrays and freshness-gated. This is the first major
credibility step: the hardcoded byte sequences (`clear`, `el`, `smcup`, `op`, ...)
are now *derived* from the parsed entry, not merely asserted. It also includes a
full `tparm` (parameterized-string stack machine) and `tputs`/`putp` (padding).
The lookup is checked against ncurses over **all 497** standard capabilities
(`NCURSES.TERMINFO.LOOKUP`), `tparm` over **106** cases (`NCURSES.TPARM`), and
`tputs` over padding/literal cases (`NCURSES.TPUTS`) -- all `admitted_match`,
zero diffs. The loader + reader are also checked across **9 terminals** (incl.
the 32-bit-number format) in `NCURSES.TERMINFO.ECOLOGY`
([`docs/terminal-ecology.md`](docs/terminal-ecology.md)). See
[`docs/terminfo-atlas.md`](docs/terminfo-atlas.md) for the method and non-claims
(extended caps, `setupterm`, `tparm`/`tputs`, and `mvcur` are out of scope).

## Oracle receipts

Byte claims are backed by receipts under [`reports/oracle/`](reports/oracle/),
produced by `tools/oracle-runner/` against the **live ncurses on this host**
(`ncurses 6.4.20240113`, `TERM=xterm`, 80x24 pty, `C.UTF-8`):

- 13 atomic terminfo-capability courts (`clear`, `ed`, `el`, `el1`, `cuu1`,
  `home`, `setaf`, `setab`, `cup`, `bel`, `flash`) are **byte-identical** to the
  live terminfo entry (`NCURSES.CAP.*`, verdict `admitted_match`).
- The composite init/teardown framing constants (`INIT_PROLOGUE` /
  `TEARDOWN_EPILOGUE`) were captured from a **different ncurses build**; against
  the in-repo 6.4 pty oracle they **diverge** in emission order and feature
  extras. This is recorded, not hidden (`NCURSES.BYTE.FRAME.FULL`, verdict
  `admitted_divergence`) -- the framing is not admitted on this host.

## What IS reproduced

For the admitted terminal (`TERM=xterm`) the emitted bytes are byte-identical to
ncurses for the items below. The atomic terminfo capabilities are pinned to the
in-repo oracle (ncurses 6.4 on this host) by `NCURSES.CAP.*` receipts; the
composite **screen framing** was captured from a different build and currently
**diverges** against that oracle (see Oracle receipts above), so it is not
admitted on this host:

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
  epilogue. The `smcup`/`rmcup`/`op` building blocks match the live terminfo
  entry, but the full composite stream diverges from in-repo ncurses 6.4
  (`NCURSES.BYTE.FRAME.FULL`, `admitted_divergence`); admitted only against the
  build it was captured from.
- **Erase operations** (`screen`) -- `clear`, `clr_eos`, `clr_eol`, `clr_bol`.
- **Bell** (`bell`) -- `beep` (`bel`, `0x07`) and `flash` (`\e[?5h\e[?5l`); the
  first man-page cluster (`curs_beep`) at full byte parity, both oracle-pinned.
- **Single-field repaint** (`screen::single_field_repaint`) -- the `wclear` +
  top-down `TransformLine` paint of one static field on a blank screen.

## What is NOT yet reproduced (the parity roadmap / non-claims)

This is the honest list of everything a real curses needs that this seed does
**not** do yet:

- **terminfo database parsing.** Exactly one terminal is hardcoded (`xterm`,
  ncurses 6.4). Any other `TERM` or any other ncurses build emits different
  bytes; reproducing them needs a terminfo reader. That is the largest TODO.
- **Remaining `doupdate` shaping** -- the general `TransformLine` line-diff is
  reproduced byte-exact for the plain + attribute/color path including the
  right-margin auto-wrap corner, `bce` colored-background clears, the colored
  first paint (color-init clear), `clearok` full-repaint, and the bottom-right
  no-scroll corner (court `NCURSES.DOUPDATE` / `NCURSES.DOUPDATE.BG_PAINT` /
  `NCURSES.STDSCR.DRAW`); the **scroll optimization** is reproduced byte-exact
  (court `NCURSES.SCROLL.OPTIMIZE`, 19/19 -- whole-screen, bottom-anchored `dl`/`il`,
  confined `csr` region, and the multi-region `_nc_hash_map` case of several bands plus
  interleaved edits). Still open: the `idl` overwrite-residual path (a single-line
  insert/delete that leaves new content in the vacated row).
- **Input handling** -- the key *decoder* is reconstructed and courted byte-for-byte
  against a real `getch` loop (`KeyMap::decode`, court `NCURSES.INPUT`), and the
  **live read path** is built in the `ncurses-terminal` crate (`RawMode` + `Keys`
  over a real raw tty): the native `getch` binary returns the same codes as real
  ncurses end-to-end (court `NCURSES.INPUT.LIVE`, arrows/F-keys/nav/text). Only the
  `ESCDELAY` timer for a lone trailing ESC and the per-mode setters
  (`cbreak`/`noecho`) remain.
- **Full `SCREEN` model** -- there *is* now a `WINDOW` cell-grid model with
  multi-window compositing *and* pads (`newpad`/`prefresh`), courted through the
  C-ABI; the full multi-`SCREEN` (`newterm`) model is not yet wired.
- **panels, menus, forms** -- none of the higher libraries.
- **Terminals other than xterm**, and **ncurses builds other than 6.4**.

## License

Apache-2.0. See `LICENSE`.
