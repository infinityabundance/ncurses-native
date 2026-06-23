# Terminfo substrate atlas

The first major credibility jump for `ncurses-native`: the terminal byte
sequences are no longer only *asserted* to equal ncurses -- the standard ones are
*derived* from the compiled terminfo entry by a native reader, and that reader is
checked against ncurses' own lookup over the whole entry.

## What exists

`src/terminfo/` is a native, dependency-free reader for the compiled terminfo
binary format:

- `Terminfo::parse(&[u8])` parses the header (magic, section sizes), the names,
  the boolean bytes, the even-offset pad, the numbers (`i16` for magic `0o432`,
  `i32` for `0o1036`), the string offsets, and the string table.
- `tigetflag` / `tigetnum` / `tigetstr` reproduce ncurses' lookup *and its return
  semantics* (present / absent / cancelled / not-a-capability).
- `tparm` is a full parameterized-string stack machine: param push/pop (`%p`),
  arithmetic/logic, the `%i` 1-based bias, `%?/%t/%e/%;` conditionals (including
  the chained else-if used by `setf`/`setb` and the 9-parameter `sgr`), and
  printf-style `%d/%s/%c/%o/%x/%X` with flags/width/precision. Padding (`$<...>`)
  is deliberately left for `tputs`.
- `tputs` / `putp` process padding: `$<...>` specs are stripped (on the admitted
  xterm pty the delays produce no bytes), and the remaining bytes are emitted.
- The legacy **termcap** layer rides on the same substrate: `tgetflag`/`tgetnum`
  look caps up by two-letter code, `tgetstr` returns the terminfo string, and
  `tgoto(cap, col, row)` == `tparm(cap, row, col)`. `tgetflag`/`tgetnum`/`tgoto`
  match ncurses byte-for-byte; `tgetstr` matches 411/414 codes and diverges only
  where ncurses' termcap emulation massages/synthesizes a cap (e.g. `me` ->
  `\e[0m` instead of the raw `sgr0`, the `ML` margin cap) -- recorded as a court.
- The capability-name tables and termcap codes (`src/terminfo/caps.rs`) are
  generated from ncurses'
  own ordered `boolnames`/`numnames`/`strnames` arrays, so a name resolves to the
  exact index it occupies in the compiled entry.

## Evidence (two oracles + a fixture)

- **Fixture:** `tests/terminfo/xterm` (sha256 `83fc9790...`), the admitted xterm
  entry, committed so the unit tests are hermetic.
- **Lookup oracle:** `NCURSES.TERMINFO.LOOKUP` compares the crate's
  `tigetflag`/`tigetnum`/`tigetstr` against ncurses' own over **all 497** standard
  capabilities (44 boolean + 39 numeric + 414 string), with `TERMINFO` pinned to
  the fixture so both readers parse identical bytes. Verdict: `admitted_match`,
  zero diffs.
- **Derivation:** the hardcoded byte courts are the same strings the reader
  returns -- e.g. `clear` = `\e[H\e[2J`, `el` = `\e[K`, `smcup` =
  `\e[?1049h\e[22;0;0t`, `op` = `\e[39;49m` -- so the seed's constants are now
  provably the terminfo caps (unit-tested in `src/terminfo/mod.rs`).
- **tparm oracle:** `NCURSES.TPARM` compares the crate's `tparm` against ncurses'
  own over **106** cases: every xterm parameterized cap with param sweeps
  (`cup`, `csr`, `cub`/`cud`/`cuf`/`cuu`, `setaf`/`setab`/`setf`/`setb`, `sgr`,
  `xm`/`XM`, `rep`, `Ss`, `smgl*`, ...), plus synthetic printf specs and
  string-parameter caps (`Cs`/`Ms`). Verdict: `admitted_match`, zero diffs.

## Non-claims (explicit)

- **Extended / user-defined capabilities** (the section after the standard one)
  are not parsed. `tigetstr` of a standard name works; user caps like `AX`/`XT`
  are out of scope for now.
- **`setupterm` / database search** is not reconstructed -- the reader takes
  bytes (or a file path); it does not implement ncurses' `$TERMINFO` /
  `~/.terminfo` / `/etc/terminfo` / `/usr/share/terminfo` search-and-load chain
  or the global `cur_term`.
- **`tgoto`** (the termcap goto) and the legacy **termcap** `tget*` lookups are
  not yet reconstructed. (`tparm` and `tputs`/`putp` are done; see above.)
- **`setupterm`** is reconstructed only as far as its **load path**:
  `Terminfo::load(term)` searches the database (`$TERMINFO`, `$TERMINFO_DIRS`,
  `$HOME/.terminfo`, `/etc/terminfo`, `/lib/terminfo`, `/usr/share/terminfo`) and
  parses the entry. Its runtime work -- the global `cur_term`, baud, terminal
  modes, and the `use_env` `cols`/`lines` augmentation -- is **not** modeled.
  See `docs/terminal-ecology.md` for the cross-terminal court.
- **`mvcur`** is a separate problem: ncurses' cursor optimizer uses an internal
  millisecond cost model, not byte length; see the cursor notes.

## Reproduce

```sh
cargo test -p ncurses-native            # hermetic parser tests
cargo xtask oracle                      # regenerate receipts (needs live ncurses)
cargo xtask check                       # freshness gate (hermetic)
```
