//! A native reader for compiled terminfo entries -- the substrate that lets the
//! crate's hardcoded xterm byte sequences be *derived* from the terminfo
//! database instead of asserted.
//!
//! This parses the compiled terminfo binary format (the file `tic` produces and
//! ncurses reads) and reconstructs the three lookup primitives
//! [`Terminfo::tigetflag`], [`Terminfo::tigetnum`], and [`Terminfo::tigetstr`]
//! with the same return semantics ncurses gives them. The capability-name tables
//! ([`caps`]) come from ncurses' own ordered `boolnames`/`numnames`/`strnames`
//! arrays, so a name maps to exactly the index it occupies in the compiled entry.
//!
//! ## Format (clean-room, from the documented layout, not from ncurses source)
//!
//! Header: six little-endian `u16` -- magic, names-size, #booleans, #numbers,
//! #strings, string-table-size. Magic `0o432` (legacy) stores numbers as `i16`;
//! magic `0o1036` (the "32-bit numbers" variant) stores them as `i32`. Then:
//! the names section, the boolean bytes, an optional pad byte so numbers start on
//! an even offset, the numbers, the string offsets (`i16` each, `-1` absent,
//! `-2` cancelled), and the string table.
//!
//! A trailing *extended* section (user-defined caps, `tic -x`) may follow, on an
//! even boundary: a five-`u16` header `[ext_bool, ext_num, ext_str, str_count,
//! str_size]`, the extended boolean bytes (+pad), the extended numbers, an offset
//! table, and a string table. The offset table holds `ext_str` string-VALUE
//! offsets followed by `ext_bool + ext_num + ext_str` cap-NAME offsets -- so its
//! length is `2*ext_str + ext_bool + ext_num`, *not* the header's `str_count`
//! (which counts the *strings present in the table*, i.e. the offsets minus the
//! cancelled string caps, since a cancelled string keeps a `-2` value slot but
//! stores no value string). Value offsets index the table start; name offsets are
//! relative to the end of the value region. All three extended cap kinds -- and
//! cancelled extended strings -- are surfaced ([`Terminfo::ext_string`],
//! [`Terminfo::ext_bool_names`], [`Terminfo::ext_num_names`],
//! [`Terminfo::ext_string_caps`]); the native `infocmp`/`tic` round-trip them
//! byte-exactly, and `tput clear`/`clear` use `E3` (clear-scrollback).
//!
//! ## Return semantics (matched to ncurses for the oracle)
//!
//! * `tigetflag(name)` -> `1` present-true, `0` absent/false, `-1` not a boolean
//!   capability name.
//! * `tigetnum(name)` -> the value, `-1` absent/cancelled, `-2` not a numeric
//!   capability name.
//! * `tigetstr(name)` -> [`Tigetstr::Value`] present, [`Tigetstr::Absent`]
//!   absent/cancelled (ncurses `NULL`), [`Tigetstr::NotString`] not a string
//!   capability name (ncurses `(char *)-1`).

pub mod caps;

/// Legacy compiled-terminfo magic: numbers are stored as `i16`.
pub const MAGIC_LEGACY: u16 = 0o432;
/// "32-bit numbers" compiled-terminfo magic: numbers are stored as `i32`.
pub const MAGIC_EXTENDED_NUMBERS: u16 = 0o1036;

/// Why a terminfo entry could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Fewer than the 12 header bytes are present.
    ShortHeader,
    /// The leading magic is neither the legacy nor the 32-bit-number variant.
    BadMagic(u16),
    /// A declared section runs past the end of the data.
    Truncated,
}

/// The result of a string-capability lookup, mirroring ncurses' three outcomes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tigetstr<'a> {
    /// The capability is present; here are its bytes.
    Value(&'a [u8]),
    /// The capability is absent or cancelled (ncurses returns `NULL`).
    Absent,
    /// The name is not a string capability at all (ncurses returns `(char *)-1`).
    NotString,
}

/// A parsed compiled terminfo entry: the terminal's names plus its boolean,
/// numeric, and string capabilities, indexed by the canonical cap order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Terminfo {
    /// The terminal names (the `|`-separated alias list; the last is the long name).
    pub names: Vec<String>,
    /// Boolean caps by index: `true` present, `false` absent/false/cancelled.
    booleans: Vec<bool>,
    /// Numeric caps by index: `>= 0` value, `-1` absent, `-2` cancelled.
    numbers: Vec<i32>,
    /// String caps by index: `Some(bytes)` present, `None` absent/cancelled.
    strings: Vec<Option<Vec<u8>>>,
    /// Parallel to `strings`: `true` where the cap was explicitly cancelled (offset -2), which
    /// `infocmp` renders as `name@`. (Absent caps are `false` here.)
    str_cancelled: Vec<bool>,
    /// Extended (user-defined, `tic -x`) boolean caps as `(name, value)`, in storage order.
    ext_bools: Vec<(String, bool)>,
    /// Extended (user-defined, `tic -x`) numeric caps as `(name, value)`, in storage order
    /// (`-2` marks an explicitly cancelled extended numeric).
    ext_nums: Vec<(String, i32)>,
    /// Extended (user-defined, `tic -x`) string capabilities by name (e.g. `E3` = clear-scrollback).
    /// `Some(bytes)` is a present value; `None` is an explicitly cancelled (`name@`) extended string.
    ext_strings: std::collections::BTreeMap<String, Option<Vec<u8>>>,
}

fn u16le(d: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*d.get(i)?, *d.get(i + 1)?]))
}

fn i16le(d: &[u8], i: usize) -> Option<i16> {
    Some(i16::from_le_bytes([*d.get(i)?, *d.get(i + 1)?]))
}

fn i32le(d: &[u8], i: usize) -> Option<i32> {
    Some(i32::from_le_bytes([
        *d.get(i)?,
        *d.get(i + 1)?,
        *d.get(i + 2)?,
        *d.get(i + 3)?,
    ]))
}

impl Terminfo {
    /// Parse a compiled terminfo entry from its raw bytes.
    pub fn parse(d: &[u8]) -> Result<Terminfo, ParseError> {
        if d.len() < 12 {
            return Err(ParseError::ShortHeader);
        }
        let magic = u16le(d, 0).unwrap();
        let num_width = match magic {
            MAGIC_LEGACY => 2usize,
            MAGIC_EXTENDED_NUMBERS => 4usize,
            other => return Err(ParseError::BadMagic(other)),
        };
        let names_sz = u16le(d, 2).unwrap() as usize;
        let nbool = u16le(d, 4).unwrap() as usize;
        let nnum = u16le(d, 6).unwrap() as usize;
        let nstr = u16le(d, 8).unwrap() as usize;
        let strtab_sz = u16le(d, 10).unwrap() as usize;

        let mut off = 12usize;

        // Names section.
        let names_end = off.checked_add(names_sz).ok_or(ParseError::Truncated)?;
        let names_raw = d.get(off..names_end).ok_or(ParseError::Truncated)?;
        let names = names_raw
            .split(|&b| b == 0)
            .next()
            .unwrap_or(&[])
            .split(|&b| b == b'|')
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect();
        off = names_end;

        // Boolean bytes: 1 -> true, anything else (0, or 0xFF cancelled) -> false.
        let bool_end = off.checked_add(nbool).ok_or(ParseError::Truncated)?;
        let bool_bytes = d.get(off..bool_end).ok_or(ParseError::Truncated)?;
        let booleans = bool_bytes.iter().map(|&b| b == 1).collect();
        off = bool_end;

        // Numbers begin on an even offset; one pad byte if currently odd.
        if (names_sz + nbool) % 2 == 1 {
            off += 1;
        }

        // Numbers.
        let mut numbers = Vec::with_capacity(nnum);
        for k in 0..nnum {
            let at = off + k * num_width;
            let v = if num_width == 2 {
                i16le(d, at).ok_or(ParseError::Truncated)? as i32
            } else {
                i32le(d, at).ok_or(ParseError::Truncated)?
            };
            numbers.push(v);
        }
        off += nnum * num_width;

        // String offsets into the string table.
        let mut offsets = Vec::with_capacity(nstr);
        for k in 0..nstr {
            offsets.push(i16le(d, off + k * 2).ok_or(ParseError::Truncated)?);
        }
        off += nstr * 2;

        // String table.
        let tab_end = off.checked_add(strtab_sz).ok_or(ParseError::Truncated)?;
        let tab = d.get(off..tab_end).ok_or(ParseError::Truncated)?;
        let strings: Vec<Option<Vec<u8>>> = offsets
            .iter()
            .map(|&o| {
                if o < 0 {
                    None // -1 absent, -2 cancelled
                } else {
                    let start = o as usize;
                    tab.get(start..).map(|rest| {
                        let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
                        rest[..end].to_vec()
                    })
                }
            })
            .collect();
        // A string offset of -2 marks an explicitly *cancelled* capability (vs -1 absent); `infocmp`
        // renders these as `name@`. Track it so the distinction survives to the decompiler.
        let str_cancelled: Vec<bool> = offsets.iter().map(|&o| o == -2).collect();

        // Extended (user-defined) section, if present. It begins on an even boundary after the main
        // string table: header [ext_bool, ext_num, ext_str, ext_str_count, ext_str_size], then the
        // ext booleans, (pad), ext numbers, then the *offset table*, then the ext string table.
        //
        // KEY: the offset table holds `es` string-VALUE offsets followed by `eb + en + es` cap-NAME
        // offsets -- so its length is `2*es + eb + en`, NOT the header's 4th field (which is the
        // *count of strings* in the table, = offsets minus the number of cancelled string caps,
        // since a cancelled string keeps a value slot (-2) but contributes no value string). The
        // 5th field is the string-table byte size. Value offsets index the table start; name
        // offsets are relative to the end of the value region (the names sub-table).
        let mut ext_bools: Vec<(String, bool)> = Vec::new();
        let mut ext_nums: Vec<(String, i32)> = Vec::new();
        let mut ext_strings = std::collections::BTreeMap::new();
        let mut eoff = tab_end;
        if eoff % 2 == 1 {
            eoff += 1;
        }
        if d.len() >= eoff + 10 {
            let eb = u16le(d, eoff).unwrap() as usize;
            let en = u16le(d, eoff + 2).unwrap() as usize;
            let es = u16le(d, eoff + 4).unwrap() as usize;
            let _str_count = u16le(d, eoff + 6).unwrap() as usize;
            let _str_size = u16le(d, eoff + 8).unwrap() as usize;
            let mut p = eoff + 10;
            // Extended boolean values + pad to even.
            let ebools: Vec<bool> = (0..eb)
                .map(|k| d.get(p + k).copied().unwrap_or(0) == 1)
                .collect();
            p += eb;
            if eb % 2 == 1 {
                p += 1;
            }
            // Extended numeric values.
            let enums: Vec<i32> = (0..en)
                .map(|k| {
                    let at = p + k * num_width;
                    if num_width == 2 {
                        i16le(d, at).map(i32::from)
                    } else {
                        i32le(d, at)
                    }
                    .unwrap_or(-1)
                })
                .collect();
            p += en * num_width;
            let n_offsets = 2 * es + eb + en; // value offsets (es) + name offsets (eb+en+es)
            if p + n_offsets * 2 <= d.len() {
                let offs: Vec<i16> = (0..n_offsets)
                    .map(|k| i16le(d, p + k * 2).unwrap_or(-1))
                    .collect();
                let stab = &d[p + n_offsets * 2..];
                let read = |abs: usize| -> Option<Vec<u8>> {
                    stab.get(abs..).map(|rest| {
                        let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
                        rest[..end].to_vec()
                    })
                };
                // The value region (present value strings) precedes the names sub-table; its end is
                // the base for the name offsets.
                let mut vend = 0usize;
                for i in 0..es {
                    let o = offs.get(i).copied().unwrap_or(-1);
                    if o >= 0 {
                        if let Some(s) = read(o as usize) {
                            vend = vend.max(o as usize + s.len() + 1);
                        }
                    }
                }
                // Name offsets follow the `es` value offsets, grouped: bool names, num names, then
                // string names (one per cap, including cancelled ones).
                let name_at = |idx: usize| -> Option<String> {
                    let no = offs.get(idx).copied().unwrap_or(-1);
                    (no >= 0)
                        .then(|| read(vend + no as usize))
                        .flatten()
                        .map(|n| String::from_utf8_lossy(&n).into_owned())
                };
                for k in 0..eb {
                    if let Some(name) = name_at(es + k) {
                        ext_bools.push((name, ebools.get(k).copied().unwrap_or(false)));
                    }
                }
                for k in 0..en {
                    if let Some(name) = name_at(es + eb + k) {
                        ext_nums.push((name, enums.get(k).copied().unwrap_or(-1)));
                    }
                }
                let str_name_base = es + eb + en;
                for i in 0..es {
                    let val_off = offs.get(i).copied().unwrap_or(-1);
                    let Some(name) = name_at(str_name_base + i) else {
                        continue;
                    };
                    if val_off == -2 {
                        ext_strings.insert(name, None); // explicitly cancelled (`name@`)
                    } else if val_off >= 0 {
                        if let Some(v) = read(val_off as usize) {
                            ext_strings.insert(name, Some(v));
                        }
                    }
                }
            }
        }

        Ok(Terminfo {
            names,
            booleans,
            numbers,
            strings,
            str_cancelled,
            ext_bools,
            ext_nums,
            ext_strings,
        })
    }

    /// Look up an extended (user-defined) string capability by name (e.g. `"E3"`), as `tic -x`
    /// records it. Returns `None` when absent or cancelled.
    pub fn ext_string(&self, name: &str) -> Option<&[u8]> {
        self.ext_strings.get(name).and_then(|v| v.as_deref())
    }

    /// All extended (user-defined) boolean caps as `(name, value)` pairs, in storage order
    /// (for `infocmp -x`).
    pub fn ext_bool_names(&self) -> &[(String, bool)] {
        &self.ext_bools
    }

    /// All extended (user-defined) numeric caps as `(name, value)` pairs, in storage order
    /// (for `infocmp -x`); a value of `-2` marks an explicitly cancelled extended numeric.
    pub fn ext_num_names(&self) -> &[(String, i32)] {
        &self.ext_nums
    }

    /// All extended (user-defined) string caps as `(name, Some(value))`, or `(name, None)` for an
    /// explicitly cancelled one (`name@`), sorted by name (for `infocmp -x`).
    pub fn ext_string_caps(&self) -> Vec<(String, Option<Vec<u8>>)> {
        self.ext_strings
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Look up a boolean capability by terminfo short-name (e.g. `"am"`).
    pub fn tigetflag(&self, name: &str) -> i32 {
        match caps::BOOL_NAMES.iter().position(|&n| n == name) {
            None => -1, // not a boolean capability name
            Some(i) => i32::from(self.booleans.get(i).copied().unwrap_or(false)),
        }
    }

    /// Look up a numeric capability by terminfo short-name (e.g. `"cols"`).
    pub fn tigetnum(&self, name: &str) -> i32 {
        match caps::NUM_NAMES.iter().position(|&n| n == name) {
            None => -2, // not a numeric capability name
            Some(i) => match self.numbers.get(i).copied().unwrap_or(-1) {
                v if v >= 0 => v,
                _ => -1, // absent or cancelled
            },
        }
    }

    /// Look up a string capability by terminfo short-name (e.g. `"clear"`).
    pub fn tigetstr(&self, name: &str) -> Tigetstr<'_> {
        match caps::STR_NAMES.iter().position(|&n| n == name) {
            None => Tigetstr::NotString,
            Some(i) => match self.strings.get(i) {
                Some(Some(bytes)) => Tigetstr::Value(bytes),
                _ => Tigetstr::Absent,
            },
        }
    }

    /// Whether a numeric capability was explicitly *cancelled* (stored as -2, distinct from
    /// absent). `infocmp` renders these as `name@`.
    pub fn num_cancelled(&self, name: &str) -> bool {
        caps::NUM_NAMES
            .iter()
            .position(|&n| n == name)
            .is_some_and(|i| self.numbers.get(i).copied() == Some(-2))
    }

    /// Whether a string capability was explicitly *cancelled* (offset -2, distinct from absent).
    /// `infocmp` renders these as `name@`.
    pub fn str_cancelled(&self, name: &str) -> bool {
        caps::STR_NAMES
            .iter()
            .position(|&n| n == name)
            .is_some_and(|i| self.str_cancelled.get(i).copied().unwrap_or(false))
    }

    /// Convenience: the bytes of a present string capability, or `None`.
    pub fn string(&self, name: &str) -> Option<&[u8]> {
        match self.tigetstr(name) {
            Tigetstr::Value(b) => Some(b),
            _ => None,
        }
    }

    /// `longname` -- the terminal's verbose description (the last `|`-separated
    /// name in the entry, e.g. `"xterm terminal emulator (X Window System)"`).
    pub fn longname(&self) -> &str {
        self.names.last().map(String::as_str).unwrap_or("")
    }

    /// `termname` -- the terminal's primary name (the first alias). ncurses
    /// truncates this to 14 bytes; this matches that.
    pub fn termname(&self) -> &str {
        let first = self.names.first().map(String::as_str).unwrap_or("");
        &first[..first.len().min(14)]
    }

    /// `has_ic` -- whether the terminal can insert characters: an insert
    /// capability (`ich1`/`ich`, or `smir`+`rmir`) together with a delete one
    /// (`dch1`/`dch`). Derived from terminfo and pinned by NCURSES.TERMATTRS.
    pub fn has_ic(&self) -> bool {
        let insert = self.string("ich1").is_some()
            || self.string("ich").is_some()
            || (self.string("smir").is_some() && self.string("rmir").is_some());
        let delete = self.string("dch1").is_some() || self.string("dch").is_some();
        insert && delete
    }

    /// `has_il` -- whether the terminal can insert/delete lines: an insert-line
    /// capability (`il1`/`il`) together with a delete-line one (`dl1`/`dl`).
    pub fn has_il(&self) -> bool {
        let insert = self.string("il1").is_some() || self.string("il").is_some();
        let delete = self.string("dl1").is_some() || self.string("dl").is_some();
        insert && delete
    }

    /// `has_colors` -- whether the terminal supports colour (`max_colors > 0`).
    pub fn has_colors(&self) -> bool {
        self.tigetnum("colors") > 0
    }

    /// `can_change_color` -- whether the terminal can redefine colours (the `ccc`
    /// boolean capability).
    pub fn can_change_color(&self) -> bool {
        self.tigetflag("ccc") == 1
    }

    /// `curs_set` -- the terminal bytes that select cursor visibility: `0` invisible
    /// (`civis`), `1` normal (`cnorm`), `2` very visible (`cvvis`). Returns the
    /// padding-processed cap bytes, or `None` if the level's capability is absent.
    /// Pinned to the terminfo entry by court NCURSES.CURS_SET.
    pub fn curs_set(&self, visibility: i32) -> Option<Vec<u8>> {
        let cap = match visibility {
            0 => "civis",
            1 => "cnorm",
            2 => "cvvis",
            _ => return None,
        };
        self.string(cap).map(tputs)
    }

    /// `tgetflag` -- look up a boolean capability by its termcap two-letter code
    /// (e.g. `"am"`). Returns `1`/`0`; an unknown code is `0` (ncurses' termcap
    /// semantics, which differ from `tigetflag`'s `-1`).
    pub fn tgetflag(&self, code: &str) -> i32 {
        match caps::BOOL_CODES
            .iter()
            .position(|&c| c == code && !c.is_empty())
        {
            Some(i) => i32::from(self.booleans.get(i).copied().unwrap_or(false)),
            None => 0,
        }
    }

    /// `tgetnum` -- look up a numeric capability by termcap code (e.g. `"co"`).
    /// Returns the value, or `-1` if absent/unknown.
    pub fn tgetnum(&self, code: &str) -> i32 {
        match caps::NUM_CODES
            .iter()
            .position(|&c| c == code && !c.is_empty())
        {
            Some(i) => match self.numbers.get(i).copied().unwrap_or(-1) {
                v if v >= 0 => v,
                _ => -1,
            },
            None => -1,
        }
    }

    /// `tgetstr` -- look up a string capability by termcap code (e.g. `"cm"`).
    /// ncurses 6.x returns the terminfo string unchanged (no termcap
    /// down-conversion), so this returns the same bytes as [`Terminfo::string`].
    pub fn tgetstr(&self, code: &str) -> Option<&[u8]> {
        let i = caps::STR_CODES
            .iter()
            .position(|&c| c == code && !c.is_empty())?;
        match self.strings.get(i) {
            Some(Some(bytes)) => Some(bytes),
            _ => None,
        }
    }

    /// Load and parse the compiled entry for `term` from the standard terminfo
    /// database locations (the `setupterm` / `tgetent` load path). See
    /// [`search_dirs`].
    pub fn load(term: &str) -> Result<Terminfo, LoadError> {
        Self::load_from(term, &search_dirs())
    }

    /// Load and parse the entry for `term`, searching `dirs` in order. The entry
    /// lives at `<dir>/<first-char>/<term>` (the Linux letter-subdir layout), with
    /// the macOS hashed `<dir>/<hex>/<term>` form tried as a fallback.
    pub fn load_from(term: &str, dirs: &[std::path::PathBuf]) -> Result<Terminfo, LoadError> {
        // Reject names that could escape the database directory.
        if term.is_empty() || term.contains('/') || term.contains('\0') {
            return Err(LoadError::BadName);
        }
        let path = find_entry(term, dirs).ok_or(LoadError::NotFound)?;
        let bytes = std::fs::read(&path).map_err(|_| LoadError::NotFound)?;
        Terminfo::parse(&bytes).map_err(LoadError::Parse)
    }
}

/// Why [`Terminfo::load`] failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// The term name is empty or contains `/` or NUL (could escape the database).
    BadName,
    /// No entry for the name was found in any search directory.
    NotFound,
    /// The entry was found but did not parse.
    Parse(ParseError),
}

/// The terminfo database search path, matching ncurses' precedence:
/// `$TERMINFO`, then each `$TERMINFO_DIRS` element (an empty element means the
/// system defaults), then `$HOME/.terminfo`, then `/etc/terminfo`,
/// `/lib/terminfo`, `/usr/share/terminfo`.
pub fn search_dirs() -> Vec<std::path::PathBuf> {
    use std::path::PathBuf;
    let defaults = ["/etc/terminfo", "/lib/terminfo", "/usr/share/terminfo"];
    let mut dirs: Vec<PathBuf> = Vec::new();
    let push = |p: PathBuf, dirs: &mut Vec<PathBuf>| {
        if !p.as_os_str().is_empty() && !dirs.contains(&p) {
            dirs.push(p);
        }
    };
    if let Ok(ti) = std::env::var("TERMINFO") {
        push(PathBuf::from(ti), &mut dirs);
    }
    if let Ok(tid) = std::env::var("TERMINFO_DIRS") {
        for part in tid.split(':') {
            if part.is_empty() {
                for d in defaults {
                    push(PathBuf::from(d), &mut dirs);
                }
            } else {
                push(PathBuf::from(part), &mut dirs);
            }
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        push(PathBuf::from(home).join(".terminfo"), &mut dirs);
    }
    for d in defaults {
        push(PathBuf::from(d), &mut dirs);
    }
    dirs
}

/// Find the compiled-entry path for `term` within `dirs`, or `None`.
pub fn find_entry(term: &str, dirs: &[std::path::PathBuf]) -> Option<std::path::PathBuf> {
    let first = term.chars().next()?;
    let letter = first.to_string();
    let hashed = format!("{:02x}", first as u32 & 0xff);
    for dir in dirs {
        for sub in [&letter, &hashed] {
            let p = dir.join(sub).join(term);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// tputs / putp: padding processing and output
// ---------------------------------------------------------------------------

/// Process a terminfo string's padding and return the bytes a terminal receives,
/// reproducing ncurses `tputs` for the admitted environment.
///
/// `tputs` strips `$<delay>` padding specs (`delay` is `[0-9.]+` optionally
/// followed by `*` proportional and/or `/` forced) and emits the remaining
/// bytes. On the admitted xterm under a normal-baud pty, the delays are realized
/// as real-time pauses (or skipped under flow control), so **no padding bytes**
/// are emitted -- captured and pinned by court NCURSES.TPUTS. A `$` not starting
/// a valid `$<digit-or-dot...>` spec is literal, matching ncurses' parser.
///
/// Non-claim: on a slow terminal with a pad character and no flow control,
/// ncurses would emit literal pad bytes here; that baud/`pad_char`-dependent
/// padding output is not modeled (the admitted environment emits none).
pub fn tputs(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'$'
            && i + 2 < s.len()
            && s[i + 1] == b'<'
            && (s[i + 2].is_ascii_digit() || s[i + 2] == b'.')
        {
            // Padding spec: skip through the closing '>'.
            i += 2;
            while i < s.len() && s[i] != b'>' {
                i += 1;
            }
            if i < s.len() {
                i += 1; // consume '>'
            }
        } else {
            out.push(s[i]);
            i += 1;
        }
    }
    out
}

/// `putp` -- `tputs` to the standard output stream with an affected-line count of
/// one. The emitted bytes are exactly [`tputs`] of the string.
pub fn putp(s: &[u8]) -> Vec<u8> {
    tputs(s)
}

/// `tgoto` -- evaluate a cursor-address capability for `(col, row)`. The termcap
/// argument order is `(cap, col, row)`; ncurses computes this as `tparm(cap,
/// row, col)`, so for the terminfo `cup`/`cm` string `tgoto(cm, 4, 2)` yields the
/// move to row 2, column 4 (1-based after `%i`, i.e. `\e[3;5H`).
pub fn tgoto(cap: &[u8], col: i32, row: i32) -> Vec<u8> {
    tparm_n(cap, &[row, col])
}

// ---------------------------------------------------------------------------
// tparm: the parameterized terminfo string evaluator
// ---------------------------------------------------------------------------

/// A tparm parameter: terminfo caps take up to nine, each an integer or a string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Param {
    Int(i32),
    Str(Vec<u8>),
}

impl From<i32> for Param {
    fn from(v: i32) -> Param {
        Param::Int(v)
    }
}

#[derive(Clone)]
enum Val {
    Int(i32),
    Str(Vec<u8>),
}

impl Val {
    fn as_int(&self) -> i32 {
        match self {
            Val::Int(v) => *v,
            Val::Str(_) => 0,
        }
    }
}

/// Evaluate a parameterized terminfo string with integer params (the common
/// case: `cup`, `setaf`, `hpa`, `vpa`, ...). See [`tparm`] for the general form.
pub fn tparm_n(cap: &[u8], params: &[i32]) -> Vec<u8> {
    let p: Vec<Param> = params.iter().map(|&v| Param::Int(v)).collect();
    tparm(cap, &p)
}

/// Evaluate a parameterized terminfo string, reproducing ncurses `tparm`:
/// the stack machine over `%`-operations (push/pop params, arithmetic, logic,
/// `%i` 1-based bias, `%?/%t/%e/%;` conditionals, and printf-style `%d/%s/%c/...`).
///
/// Padding (`$<...>`) is intentionally left in place -- it is `tputs`' job, not
/// `tparm`'s. Static and dynamic `%P/%g` variables are per-call (ncurses' static
/// variables persist across calls, but no admitted cap relies on that).
pub fn tparm(cap: &[u8], params: &[Param]) -> Vec<u8> {
    let mut p: Vec<Param> = params.to_vec();
    while p.len() < 9 {
        p.push(Param::Int(0));
    }
    let mut stack: Vec<Val> = Vec::new();
    let mut out: Vec<u8> = Vec::new();
    let mut dvars: Vec<Val> = vec![Val::Int(0); 26];
    let mut svars: Vec<Val> = vec![Val::Int(0); 26];
    let b = cap;
    let mut i = 0usize;

    let pop = |s: &mut Vec<Val>| -> Val { s.pop().unwrap_or(Val::Int(0)) };
    let pop_i = |s: &mut Vec<Val>| -> i32 { s.pop().map(|v| v.as_int()).unwrap_or(0) };

    while i < b.len() {
        if b[i] != b'%' {
            out.push(b[i]);
            i += 1;
            continue;
        }
        // A printf conversion? Try to parse a `%[:][flags][width][.prec]conv`.
        if let Some((text, next)) = parse_printf(b, i + 1, &mut stack) {
            out.extend_from_slice(&text);
            i = next;
            continue;
        }
        if i + 1 >= b.len() {
            break;
        }
        let op = b[i + 1];
        i += 2;
        match op {
            b'%' => out.push(b'%'),
            b'i' => {
                if let Param::Int(v) = &mut p[0] {
                    *v += 1;
                }
                if let Param::Int(v) = &mut p[1] {
                    *v += 1;
                }
            }
            b'p' => {
                let n = b.get(i).copied().unwrap_or(b'0');
                i += 1;
                let idx = (n.wrapping_sub(b'1')) as usize;
                let v = match p.get(idx) {
                    Some(Param::Int(v)) => Val::Int(*v),
                    Some(Param::Str(s)) => Val::Str(s.clone()),
                    None => Val::Int(0),
                };
                stack.push(v);
            }
            b'P' => {
                let n = b.get(i).copied().unwrap_or(b'a');
                i += 1;
                let v = pop(&mut stack);
                if n.is_ascii_lowercase() {
                    dvars[(n - b'a') as usize] = v;
                } else if n.is_ascii_uppercase() {
                    svars[(n - b'A') as usize] = v;
                }
            }
            b'g' => {
                let n = b.get(i).copied().unwrap_or(b'a');
                i += 1;
                let v = if n.is_ascii_lowercase() {
                    dvars[(n - b'a') as usize].clone()
                } else if n.is_ascii_uppercase() {
                    svars[(n - b'A') as usize].clone()
                } else {
                    Val::Int(0)
                };
                stack.push(v);
            }
            b'\'' => {
                // %'c' : push the char constant
                let c = b.get(i).copied().unwrap_or(0);
                stack.push(Val::Int(c as i32));
                i += 1;
                if b.get(i) == Some(&b'\'') {
                    i += 1;
                }
            }
            b'{' => {
                // %{nn} : push integer constant
                let mut n = 0i32;
                let mut neg = false;
                if b.get(i) == Some(&b'-') {
                    neg = true;
                    i += 1;
                }
                while let Some(&d) = b.get(i) {
                    if d.is_ascii_digit() {
                        n = n * 10 + (d - b'0') as i32;
                        i += 1;
                    } else {
                        break;
                    }
                }
                if b.get(i) == Some(&b'}') {
                    i += 1;
                }
                stack.push(Val::Int(if neg { -n } else { n }));
            }
            b'l' => {
                let v = pop(&mut stack);
                let len = match v {
                    Val::Str(s) => s.len() as i32,
                    Val::Int(_) => 0,
                };
                stack.push(Val::Int(len));
            }
            b'+' | b'-' | b'*' | b'/' | b'm' | b'&' | b'|' | b'^' => {
                let y = pop_i(&mut stack);
                let x = pop_i(&mut stack);
                let r = match op {
                    b'+' => x.wrapping_add(y),
                    b'-' => x.wrapping_sub(y),
                    b'*' => x.wrapping_mul(y),
                    b'/' => {
                        if y != 0 {
                            x / y
                        } else {
                            0
                        }
                    }
                    b'm' => {
                        if y != 0 {
                            x % y
                        } else {
                            0
                        }
                    }
                    b'&' => x & y,
                    b'|' => x | y,
                    b'^' => x ^ y,
                    _ => 0,
                };
                stack.push(Val::Int(r));
            }
            b'=' | b'>' | b'<' | b'A' | b'O' => {
                let y = pop_i(&mut stack);
                let x = pop_i(&mut stack);
                let r = match op {
                    b'=' => (x == y) as i32,
                    b'>' => (x > y) as i32,
                    b'<' => (x < y) as i32,
                    b'A' => ((x != 0) && (y != 0)) as i32,
                    b'O' => ((x != 0) || (y != 0)) as i32,
                    _ => 0,
                };
                stack.push(Val::Int(r));
            }
            b'!' => {
                let x = pop_i(&mut stack);
                stack.push(Val::Int((x == 0) as i32));
            }
            b'~' => {
                let x = pop_i(&mut stack);
                stack.push(Val::Int(!x));
            }
            b'?' => { /* begin conditional: no-op */ }
            b';' => { /* end conditional: no-op */ }
            b't' => {
                let cond = pop_i(&mut stack);
                if cond == 0 {
                    // skip the then-branch to the matching %e (continue there) or %;
                    i = skip_branch(b, i, true);
                }
            }
            b'e' => {
                // reached after executing a then-branch: skip the else to %;
                i = skip_branch(b, i, false);
            }
            _ => { /* unknown op: ignore */ }
        }
    }
    out
}

/// Skip from `i` over conditional branch content, honouring nested `%?..%;`.
/// If `stop_at_else`, stop just past a top-level `%e` (or `%;`); otherwise stop
/// just past the top-level `%;`. Returns the new index.
fn skip_branch(b: &[u8], mut i: usize, stop_at_else: bool) -> usize {
    let mut depth = 0i32;
    while i < b.len() {
        if b[i] == b'%' && i + 1 < b.len() {
            let c = b[i + 1];
            match c {
                b'?' => {
                    depth += 1;
                    i += 2;
                }
                b';' => {
                    if depth == 0 {
                        return i + 2;
                    }
                    depth -= 1;
                    i += 2;
                }
                b'e' => {
                    if depth == 0 && stop_at_else {
                        return i + 2;
                    }
                    i += 2;
                }
                _ => i += 2,
            }
        } else {
            i += 1;
        }
    }
    i
}

/// Try to parse and apply a printf conversion at `b[i..]` (i points just past
/// `%`). Returns the rendered bytes and the index after the conversion, or
/// `None` if this is not a printf spec (so the caller handles it as an operator).
fn parse_printf(b: &[u8], start: usize, stack: &mut Vec<Val>) -> Option<(Vec<u8>, usize)> {
    let mut i = start;
    let mut flags = Flags::default();
    if b.get(i) == Some(&b':') {
        i += 1;
        // after ':' a leading '-' is a flag, not the subtract operator
    }
    loop {
        match b.get(i) {
            Some(b'-') => flags.left = true,
            Some(b'+') => flags.plus = true,
            Some(b' ') => flags.space = true,
            Some(b'#') => flags.alt = true,
            Some(b'0') => flags.zero = true,
            _ => break,
        }
        i += 1;
    }
    let mut width = 0usize;
    let mut has_width = false;
    while let Some(&d) = b.get(i) {
        if d.is_ascii_digit() {
            width = width * 10 + (d - b'0') as usize;
            has_width = true;
            i += 1;
        } else {
            break;
        }
    }
    let mut prec: Option<usize> = None;
    if b.get(i) == Some(&b'.') {
        i += 1;
        let mut pv = 0usize;
        while let Some(&d) = b.get(i) {
            if d.is_ascii_digit() {
                pv = pv * 10 + (d - b'0') as usize;
                i += 1;
            } else {
                break;
            }
        }
        prec = Some(pv);
    }
    let conv = *b.get(i)?;
    // A bare `%-`/`%+`/etc with no conversion is an operator, not printf.
    if !matches!(conv, b'd' | b'o' | b'x' | b'X' | b's' | b'c') {
        // If we consumed nothing but the conversion is invalid, bail (operator path).
        if !has_width && prec.is_none() && i == start {
            return None;
        }
        // Consumed flags/width but no valid conversion -> not a printf spec.
        if i == start {
            return None;
        }
        // e.g. "%-" : flags.left set, conv is '%' or another op -> operator path.
        return None;
    }
    i += 1;
    let text = match conv {
        // ncurses `save_char`: `%c` of a 0 value emits 0x80, never an embedded NUL (which `tputs`
        // would treat as the string terminator / padding). Binary cursor-address caps (e.g.
        // `\x1f%p1%c%p2%c`) rely on this for row/column 0.
        b'c' => {
            let v = pop_int(stack) as u8;
            vec![if v == 0 { 0o200 } else { v }]
        }
        b's' => {
            let s = match stack.pop().unwrap_or(Val::Int(0)) {
                Val::Str(s) => s,
                Val::Int(_) => Vec::new(),
            };
            format_str(&s, &flags, has_width.then_some(width), prec)
        }
        _ => format_int(
            pop_int(stack),
            conv,
            &flags,
            has_width.then_some(width),
            prec,
        ),
    };
    Some((text, i))
}

fn pop_int(stack: &mut Vec<Val>) -> i32 {
    stack.pop().map(|v| v.as_int()).unwrap_or(0)
}

#[derive(Default)]
struct Flags {
    left: bool,
    plus: bool,
    space: bool,
    alt: bool,
    zero: bool,
}

fn format_str(s: &[u8], flags: &Flags, width: Option<usize>, prec: Option<usize>) -> Vec<u8> {
    let mut body: Vec<u8> = match prec {
        Some(p) if p < s.len() => s[..p].to_vec(),
        _ => s.to_vec(),
    };
    if let Some(w) = width {
        if body.len() < w {
            let pad = w - body.len();
            if flags.left {
                body.extend(std::iter::repeat(b' ').take(pad));
            } else {
                let mut p = vec![b' '; pad];
                p.extend_from_slice(&body);
                body = p;
            }
        }
    }
    body
}

fn format_int(
    v: i32,
    conv: u8,
    flags: &Flags,
    width: Option<usize>,
    prec: Option<usize>,
) -> Vec<u8> {
    let (digits, prefix): (String, String) = match conv {
        b'd' => {
            let neg = v < 0;
            let mag = (v as i64).unsigned_abs().to_string();
            let sign = if neg {
                "-".to_string()
            } else if flags.plus {
                "+".to_string()
            } else if flags.space {
                " ".to_string()
            } else {
                String::new()
            };
            (mag, sign)
        }
        b'o' => {
            let s = format!("{:o}", v as u32);
            let pre = if flags.alt && !s.starts_with('0') {
                "0".to_string()
            } else {
                String::new()
            };
            (s, pre)
        }
        b'x' => {
            let s = format!("{:x}", v as u32);
            let pre = if flags.alt && v != 0 {
                "0x".to_string()
            } else {
                String::new()
            };
            (s, pre)
        }
        b'X' => {
            let s = format!("{:X}", v as u32);
            let pre = if flags.alt && v != 0 {
                "0X".to_string()
            } else {
                String::new()
            };
            (s, pre)
        }
        _ => (v.to_string(), String::new()),
    };
    // precision = minimum number of digits (zero-pad the number part)
    let mut num = digits;
    if let Some(p) = prec {
        while num.len() < p {
            num.insert(0, '0');
        }
    }
    let mut s = format!("{prefix}{num}");
    if let Some(w) = width {
        if s.len() < w {
            let pad = w - s.len();
            if flags.left {
                s.push_str(&" ".repeat(pad));
            } else if flags.zero && prec.is_none() {
                // zero-pad after the sign/prefix
                let mut z = String::new();
                z.push_str(&prefix);
                z.push_str(&"0".repeat(pad));
                z.push_str(&num);
                s = z;
            } else {
                s = format!("{}{}", " ".repeat(pad), s);
            }
        }
    }
    s.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    // The admitted xterm compiled entry (sha256 83fc9790...), committed as a
    // terminfo binary fixture so these tests are hermetic.
    const XTERM: &[u8] = include_bytes!("../../tests/terminfo/xterm");

    fn xterm() -> Terminfo {
        Terminfo::parse(XTERM).expect("parse xterm terminfo")
    }

    #[test]
    fn parses_names() {
        let t = xterm();
        assert_eq!(t.names.first().map(String::as_str), Some("xterm"));
        assert!(t.names.iter().any(|n| n.contains("X Window System")));
    }

    #[test]
    fn string_caps_derive_seed_sequences() {
        let t = xterm();
        // The hardcoded byte courts are exactly these terminfo string caps.
        assert_eq!(t.string("clear"), Some(&b"\x1b[H\x1b[2J"[..]));
        assert_eq!(t.string("el"), Some(&b"\x1b[K"[..]));
        assert_eq!(t.string("ed"), Some(&b"\x1b[J"[..]));
        assert_eq!(t.string("el1"), Some(&b"\x1b[1K"[..]));
        assert_eq!(t.string("cuu1"), Some(&b"\x1b[A"[..]));
        assert_eq!(t.string("home"), Some(&b"\x1b[H"[..]));
        assert_eq!(t.string("bel"), Some(&b"\x07"[..]));
        assert_eq!(t.string("smcup"), Some(&b"\x1b[?1049h\x1b[22;0;0t"[..]));
        assert_eq!(t.string("rmcup"), Some(&b"\x1b[?1049l\x1b[23;0;0t"[..]));
        assert_eq!(t.string("op"), Some(&b"\x1b[39;49m"[..]));
        assert_eq!(t.string("sgr0"), Some(&b"\x1b(B\x1b[m"[..]));
    }

    #[test]
    fn numeric_and_boolean_caps() {
        let t = xterm();
        assert_eq!(t.tigetnum("cols"), 80);
        assert_eq!(t.tigetnum("lines"), 24);
        assert_eq!(t.tigetnum("colors"), 8);
        assert_eq!(t.tigetflag("am"), 1); // auto-right-margin
        assert_eq!(t.tigetflag("bce"), 1); // back-color-erase
    }

    #[test]
    fn ncurses_return_semantics() {
        let t = xterm();
        // absent vs not-a-capability distinctions match ncurses.
        assert_eq!(t.tigetnum("nosuchcap"), -2); // not a numeric cap name
        assert_eq!(t.tigetflag("nosuchcap"), -1); // not a boolean cap name
        assert_eq!(t.tigetstr("nosuchcap"), Tigetstr::NotString);
    }

    #[test]
    fn tputs_strips_padding() {
        // Pinned against ncurses under a pty by NCURSES.TPUTS.
        assert_eq!(tputs(b"X$<5>Y"), b"XY");
        assert_eq!(tputs(b"A$<10*>B"), b"AB");
        assert_eq!(tputs(b"P$<100/>Q"), b"PQ");
        assert_eq!(tputs(b"Z$<3.5>W"), b"ZW");
        assert_eq!(tputs(b"\x1b[?5h$<100/>\x1b[?5l"), b"\x1b[?5h\x1b[?5l");
        assert_eq!(tputs(b"nodelay"), b"nodelay");
        // A '$' that does not start a padding spec is literal.
        assert_eq!(tputs(b"cost$5"), b"cost$5");
        assert_eq!(tputs(b"a$<b"), b"a$<b");
        assert_eq!(putp(b"x$<2>y"), b"xy");
    }

    #[test]
    fn tputs_derives_flash() {
        // The bell/flash byte court is tputs(flash terminfo cap).
        let t = xterm();
        assert_eq!(tputs(t.string("flash").unwrap()), b"\x1b[?5h\x1b[?5l");
    }

    #[test]
    fn tparm_simple_params() {
        let t = xterm();
        let cup = t.string("cup").unwrap().to_vec();
        // %i biases 0-based (2,4) to the 1-based CUP \e[3;5H.
        assert_eq!(tparm_n(&cup, &[2, 4]), b"\x1b[3;5H");
        assert_eq!(tparm_n(t.string("hpa").unwrap(), &[7]), b"\x1b[8G");
        assert_eq!(tparm_n(t.string("vpa").unwrap(), &[1]), b"\x1b[2d");
        assert_eq!(tparm_n(t.string("setaf").unwrap(), &[2]), b"\x1b[32m");
        assert_eq!(tparm_n(t.string("setab").unwrap(), &[4]), b"\x1b[44m");
        assert_eq!(tparm_n(t.string("cuf").unwrap(), &[3]), b"\x1b[3C");
    }

    #[test]
    fn tparm_conditionals_via_setf() {
        let t = xterm();
        let setf = t.string("setf").unwrap().to_vec();
        // setf has a %?..%t..%e.. chain: p1==1 -> "4", p1==3 -> "6", else %p1%d.
        assert_eq!(tparm_n(&setf, &[1]), b"\x1b[34m");
        assert_eq!(tparm_n(&setf, &[0]), b"\x1b[30m");
        assert_eq!(tparm_n(&setf, &[2]), b"\x1b[32m");
    }

    #[test]
    fn tparm_printf_specs() {
        // Synthetic format strings exercise the printf path (verified vs ncurses
        // by the NCURSES.TPARM court).
        assert_eq!(tparm_n(b"%p1%d", &[42]), b"42");
        assert_eq!(tparm_n(b"%p1%03d", &[7]), b"007");
        assert_eq!(tparm_n(b"%p1%2d", &[7]), b" 7");
        assert_eq!(tparm_n(b"%p1%x", &[255]), b"ff");
        assert_eq!(tparm_n(b"%p1%c", &[b'Z' as i32]), b"Z");
        // ncurses `save_char`: `%c` of 0 emits 0x80, not a NUL (binary cursor-address caps rely on
        // this for row/column 0, e.g. `\x1f%p1%c%p2%c` at (0,5) -> 1f 80 05).
        assert_eq!(tparm_n(b"%p1%c", &[0]), &[0o200u8]);
        assert_eq!(tparm_n(b"\x1f%p1%c%p2%c", &[0, 5]), &[0x1f, 0o200, 0x05]);
        assert_eq!(tparm(b"%p1%s", &[Param::Str(b"hi".to_vec())]), b"hi");
        assert_eq!(tparm_n(b"%{5}%{3}%-%d", &[]), b"2"); // 5-3
        assert_eq!(tparm_n(b"%%", &[]), b"%");
    }

    #[test]
    fn rejects_bad_magic() {
        assert_eq!(
            Terminfo::parse(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            Err(ParseError::BadMagic(0))
        );
        assert_eq!(Terminfo::parse(&[1, 2, 3]), Err(ParseError::ShortHeader));
    }

    #[test]
    fn color_caps() {
        let t = xterm();
        assert!(t.has_colors()); // colors=8
        assert!(!t.can_change_color()); // no ccc
    }

    #[test]
    fn curs_set_levels() {
        let t = xterm();
        assert_eq!(t.curs_set(0).as_deref(), Some(&b"\x1b[?25l"[..])); // civis
        assert_eq!(t.curs_set(1).as_deref(), Some(&b"\x1b[?12l\x1b[?25h"[..])); // cnorm
        assert_eq!(t.curs_set(2).as_deref(), Some(&b"\x1b[?12;25h"[..])); // cvvis
        assert_eq!(t.curs_set(3), None);
    }

    #[test]
    fn terminal_queries() {
        let t = xterm();
        assert_eq!(t.longname(), "xterm terminal emulator (X Window System)");
        assert_eq!(t.termname(), "xterm");
        assert!(t.has_ic()); // xterm: parm_ich + dch
        assert!(t.has_il()); // xterm: il1 + dl
    }

    #[test]
    fn termcap_layer() {
        let t = xterm();
        // tgetflag/tgetnum by termcap code (pinned vs ncurses by NCURSES.TERMCAP).
        assert_eq!(t.tgetflag("am"), 1);
        assert_eq!(t.tgetflag("ZZ"), 0); // unknown code -> 0 (not -1)
        assert_eq!(t.tgetnum("co"), 80);
        assert_eq!(t.tgetnum("li"), 24);
        assert_eq!(t.tgetnum("ZZ"), -1);
        // tgetstr returns the terminfo string unchanged.
        assert_eq!(t.tgetstr("cl"), Some(&b"\x1b[H\x1b[2J"[..]));
        assert_eq!(t.tgetstr("ce"), Some(&b"\x1b[K"[..]));
        assert_eq!(t.tgetstr("cm"), Some(&b"\x1b[%i%p1%d;%p2%dH"[..]));
        assert_eq!(t.tgetstr("ZZ"), None);
        // tgoto(cap, col, row) == tparm(cap, row, col).
        let cm = t.tgetstr("cm").unwrap().to_vec();
        assert_eq!(tgoto(&cm, 4, 2), b"\x1b[3;5H");
    }

    #[test]
    fn loader_finds_and_parses_entry() {
        // Build a private terminfo tree containing the xterm fixture and load it.
        let mut dir = std::env::temp_dir();
        dir.push(format!("ncn-ti-{}", std::process::id()));
        let xd = dir.join("x");
        std::fs::create_dir_all(&xd).unwrap();
        std::fs::write(xd.join("xterm"), XTERM).unwrap();

        let dirs = vec![dir.clone()];
        let found = find_entry("xterm", &dirs).expect("entry found");
        assert!(found.ends_with("x/xterm"));

        let t = Terminfo::load_from("xterm", &dirs).expect("loads");
        assert_eq!(t.string("clear"), Some(&b"\x1b[H\x1b[2J"[..]));
        assert_eq!(t.tigetnum("cols"), 80);

        // Missing entry and path-escaping names are rejected.
        assert_eq!(
            Terminfo::load_from("nosuchterm", &dirs),
            Err(LoadError::NotFound)
        );
        assert_eq!(
            Terminfo::load_from("../x/xterm", &dirs),
            Err(LoadError::BadName)
        );
        assert_eq!(Terminfo::load_from("", &dirs), Err(LoadError::BadName));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn search_dirs_includes_system_default() {
        let dirs = search_dirs();
        assert!(dirs.iter().any(|d| d.ends_with("usr/share/terminfo")));
    }
}
