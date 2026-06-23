//! `tic` -- compile terminfo *source* into the binary form `infocmp`/ncurses read, byte-identically
//! to ncurses `tic(1)` for the non-extended case.
//!
//! `tic [-x] [-o dir] [-1] [-v] <file|->`: parse each entry (the `name|...,` line followed by
//! comma-separated `cap` / `cap@` / `cap#num` / `cap=string` capabilities), un-escape string values
//! (the inverse of `infocmp`'s `_nc_tic_expand`), and write `<dir>/<c>/<primary-name>` -- the same
//! compiled layout this workspace's terminfo *reader* parses.
//!
//! This is the inverse of the native `infocmp`, so `infocmp -1 <t> | tic -o out -` reproduces the
//! system `tic`'s compiled bytes. Extended (`-x`, user-defined) caps are not yet compiled.

use std::path::PathBuf;
use std::process::exit;

use ncurses_native::terminfo::{caps, MAGIC_EXTENDED_NUMBERS, MAGIC_LEGACY};

/// Un-escape a terminfo source string value into its stored bytes -- the exact inverse of the
/// escaping `infocmp`/`tic` apply, as verified against real `tic`:
///   * `\E`/`\e` -> ESC, `\n`/`\l` -> LF, `\r` -> CR, `\t` -> TAB, `\b` -> BS, `\f` -> FF, `\a` -> BEL;
///   * `\s` -> space, `\0` -> 0x80 (NUL can't be stored, so ncurses uses 0x80), `\nnn` -> octal byte
///     (a zero value also becomes 0x80); `\,`/`\^`/`\\`/`\:` -> that literal byte; any other `\c` -> c;
///   * `^X` -> control byte `X ^ 0x40` (so `^?` -> 0x7f, `^@` -> 0x80);
///   * `%` takes the following byte verbatim (so `%^` keeps a literal caret) unless that byte begins
///     a `\` escape, which is still expanded.
fn unescape(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'%' => {
                out.push(b'%');
                i += 1;
                // The byte after `%` is literal (caret notation suppressed) unless it is an escape.
                if i < s.len() && s[i] != b'\\' {
                    out.push(s[i]);
                    i += 1;
                }
            }
            b'\\' => {
                i += 1;
                if i >= s.len() {
                    break;
                }
                let c = s[i];
                if c.is_ascii_digit() && (b'0'..=b'7').contains(&c) {
                    // Up to three octal digits.
                    let mut val: u32 = 0;
                    let mut n = 0;
                    while n < 3 && i < s.len() && (b'0'..=b'7').contains(&s[i]) {
                        val = val * 8 + u32::from(s[i] - b'0');
                        i += 1;
                        n += 1;
                    }
                    let b = (val & 0xff) as u8;
                    out.push(if b == 0 { 0x80 } else { b });
                } else {
                    out.push(match c {
                        b'E' | b'e' => 0x1b,
                        b'n' | b'l' => 0x0a,
                        b'r' => 0x0d,
                        b't' => 0x09,
                        b'b' => 0x08,
                        b'f' => 0x0c,
                        b'a' => 0x07,
                        b's' => b' ',
                        b'0' => 0x80,
                        other => other,
                    });
                    i += 1;
                }
            }
            b'^' => {
                i += 1;
                if i < s.len() {
                    let v = s[i] ^ 0x40;
                    out.push(if v == 0 { 0x80 } else { v });
                    i += 1;
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// One parsed *predefined* capability from source.
enum Cap {
    Bool(usize),
    Num(usize, i32),
    Str(usize, Vec<u8>),
    /// A cancelled (`name@`) numeric or string capability, by (is_string, index).
    Cancel { is_string: bool, index: usize },
}

/// An *extended* (user-defined) capability -- a name not in the predefined tables. A bare `name@`
/// is, like ncurses, a cancelled extended *string*.
enum ExtCap {
    Bool(String),
    Num(String, i32),
    /// `Some(bytes)` present value, `None` cancelled (`name@`).
    Str(String, Option<Vec<u8>>),
}

/// A parsed terminfo entry ready to compile.
struct Entry {
    names: String,
    caps: Vec<Cap>,
    ext: Vec<ExtCap>,
}

/// Split an entry body into comma-separated fields. A `\X` escape and a `^X` caret pair are each
/// consumed as a two-byte unit, so a comma is only a separator when it is neither escaped (`\,`)
/// nor the target of a caret (as in `cuf1=^\,`, where the `\` belongs to `^` and the comma ends
/// the capability).
fn split_fields(body: &str) -> Vec<String> {
    let b = body.as_bytes();
    let mut fields = Vec::new();
    let mut cur = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' | b'^' if i + 1 < b.len() => {
                cur.push(b[i]);
                cur.push(b[i + 1]);
                i += 2;
            }
            b',' => {
                fields.push(String::from_utf8_lossy(&cur).into_owned());
                cur.clear();
                i += 1;
            }
            c => {
                cur.push(c);
                i += 1;
            }
        }
    }
    if !cur.is_empty() {
        fields.push(String::from_utf8_lossy(&cur).into_owned());
    }
    fields
}

fn bool_index(name: &str) -> Option<usize> {
    caps::BOOL_NAMES.iter().position(|&n| n == name)
}
fn num_index(name: &str) -> Option<usize> {
    caps::NUM_NAMES.iter().position(|&n| n == name)
}
fn str_index(name: &str) -> Option<usize> {
    caps::STR_NAMES.iter().position(|&n| n == name)
}

/// Parse a numeric value (decimal, `0x`/`0X` hex, or leading-zero octal), as `tic`/`infocmp` write.
fn parse_num(v: &str) -> Option<i32> {
    let v = v.trim();
    if let Some(h) = v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")) {
        i64::from_str_radix(h, 16).ok().map(|n| n as i32)
    } else if v.len() > 1 && v.starts_with('0') && v.bytes().all(|c| (b'0'..=b'7').contains(&c)) {
        i64::from_str_radix(v, 8).ok().map(|n| n as i32)
    } else {
        v.parse::<i64>().ok().map(|n| n as i32)
    }
}

/// Parse one source entry (comments already stripped) into [`Entry`], or `None` if it has no name.
fn parse_entry(text: &str) -> Option<Entry> {
    let fields = split_fields(text);
    let mut it = fields.into_iter();
    let names = it.next()?.trim().to_string();
    if names.is_empty() {
        return None;
    }
    let mut caps_out = Vec::new();
    let mut ext_out = Vec::new();
    for f in it {
        let f = f.trim();
        if f.is_empty() || f.starts_with("use=") {
            continue; // `use=` flattening is not needed for resolved (`infocmp -1`) source.
        }
        if let Some(eq) = f.find('=') {
            let (name, val) = (&f[..eq], &f[eq + 1..]);
            match str_index(name) {
                Some(ix) => caps_out.push(Cap::Str(ix, unescape(val.as_bytes()))),
                None => ext_out.push(ExtCap::Str(name.to_string(), Some(unescape(val.as_bytes())))),
            }
        } else if let Some(name) = f.strip_suffix('@') {
            if let Some(ix) = str_index(name) {
                caps_out.push(Cap::Cancel { is_string: true, index: ix });
            } else if let Some(ix) = num_index(name) {
                caps_out.push(Cap::Cancel { is_string: false, index: ix });
            } else {
                // An unknown cancelled cap is, like ncurses, a cancelled extended string.
                ext_out.push(ExtCap::Str(name.to_string(), None));
            }
        } else if let Some(h) = f.find('#') {
            let (name, val) = (&f[..h], &f[h + 1..]);
            if let Some(n) = parse_num(val) {
                match num_index(name) {
                    Some(ix) => caps_out.push(Cap::Num(ix, n)),
                    None => ext_out.push(ExtCap::Num(name.to_string(), n)),
                }
            }
        } else {
            match bool_index(f) {
                Some(ix) => caps_out.push(Cap::Bool(ix)),
                None => ext_out.push(ExtCap::Bool(f.to_string())),
            }
        }
    }
    Some(Entry { names, caps: caps_out, ext: ext_out })
}

/// SVr4 capability counts: without `-x`, `tic` stores only caps below these indices. The
/// ncurses-extension predefined caps above them (the `OT*` termcap-compat set, `meml`/`memu`, ...)
/// are dropped, exactly as ncurses' own `tic` does in the non-extended mode.
const SVR4_BOOL: usize = 37;
const SVR4_NUM: usize = 33;
const SVR4_STR: usize = 394;

/// Compile a parsed entry to the binary terminfo layout (the inverse of this crate's reader).
/// With `ext`, the predefined-cap cutoff is lifted (the full tables are stored) and an extended
/// (user-defined) section is appended; without it, the SVr4 cutoff applies and extensions drop.
fn compile(entry: &Entry, ext: bool) -> Vec<u8> {
    let (cut_b, cut_n, cut_s) = if ext {
        (caps::BOOL_NAMES.len(), caps::NUM_NAMES.len(), caps::STR_NAMES.len())
    } else {
        (SVR4_BOOL, SVR4_NUM, SVR4_STR)
    };

    // Lay capabilities out by index; absent slots are -1, cancelled are -2.
    let mut bool_vals: Vec<i8> = Vec::new(); // 0 absent, 1 true
    let mut num_vals: Vec<i32> = Vec::new(); // -1 absent, -2 cancelled, else value
    let mut str_vals: Vec<Option<Vec<u8>>> = Vec::new();
    let mut str_cancel: Vec<bool> = Vec::new();

    let ensure_b = |v: &mut Vec<i8>, ix: usize| {
        if v.len() <= ix {
            v.resize(ix + 1, 0);
        }
    };
    let ensure_n = |v: &mut Vec<i32>, ix: usize| {
        if v.len() <= ix {
            v.resize(ix + 1, -1);
        }
    };
    let ensure_s = |v: &mut Vec<Option<Vec<u8>>>, c: &mut Vec<bool>, ix: usize| {
        if v.len() <= ix {
            v.resize(ix + 1, None);
            c.resize(ix + 1, false);
        }
    };

    for cap in &entry.caps {
        match cap {
            Cap::Bool(ix) if *ix < cut_b => {
                ensure_b(&mut bool_vals, *ix);
                bool_vals[*ix] = 1;
            }
            Cap::Num(ix, n) if *ix < cut_n => {
                ensure_n(&mut num_vals, *ix);
                num_vals[*ix] = *n;
            }
            Cap::Str(ix, bytes) if *ix < cut_s => {
                ensure_s(&mut str_vals, &mut str_cancel, *ix);
                str_vals[*ix] = Some(bytes.clone());
            }
            Cap::Cancel { is_string: true, index } if *index < cut_s => {
                ensure_s(&mut str_vals, &mut str_cancel, *index);
                str_cancel[*index] = true;
            }
            Cap::Cancel { is_string: false, index } if *index < cut_n => {
                ensure_n(&mut num_vals, *index);
                num_vals[*index] = -2;
            }
            // Caps at/above the cutoff are ncurses extensions, stored only under `-x`.
            _ => {}
        }
    }

    let nbool = bool_vals.len();
    let nnum = num_vals.len();
    let nstr = str_vals.len();

    // 32-bit numbers only when a value does not fit a signed 16-bit (ncurses' magic switch);
    // extended numerics count too.
    let ext_num_max = entry.ext.iter().filter_map(|e| match e {
        ExtCap::Num(_, n) => Some(*n),
        _ => None,
    });
    let need_32 = num_vals.iter().copied().chain(ext_num_max).any(|n| n > 32767);
    let (magic, num_width) = if need_32 {
        (MAGIC_EXTENDED_NUMBERS, 4usize)
    } else {
        (MAGIC_LEGACY, 2usize)
    };

    // Names section (the `|`-joined alias list) plus a terminating NUL.
    let mut names_bytes = entry.names.clone().into_bytes();
    names_bytes.push(0);
    let names_sz = names_bytes.len();

    // String table + offsets, in index order; cancelled -> -2, absent -> -1.
    let mut strtab: Vec<u8> = Vec::new();
    let mut offsets: Vec<i16> = Vec::with_capacity(nstr);
    for i in 0..nstr {
        if let Some(bytes) = &str_vals[i] {
            let off = strtab.len() as i16;
            offsets.push(off);
            strtab.extend_from_slice(bytes);
            strtab.push(0);
        } else if str_cancel[i] {
            offsets.push(-2);
        } else {
            offsets.push(-1);
        }
    }

    let mut out = Vec::new();
    let push16 = |o: &mut Vec<u8>, v: u16| o.extend_from_slice(&v.to_le_bytes());
    push16(&mut out, magic);
    push16(&mut out, names_sz as u16);
    push16(&mut out, nbool as u16);
    push16(&mut out, nnum as u16);
    push16(&mut out, nstr as u16);
    push16(&mut out, strtab.len() as u16);

    out.extend_from_slice(&names_bytes);
    for &b in &bool_vals {
        out.push(b as u8);
    }
    // Numbers begin on an even offset.
    if (names_sz + nbool) % 2 == 1 {
        out.push(0);
    }
    for &n in &num_vals {
        if num_width == 2 {
            out.extend_from_slice(&(n as i16).to_le_bytes());
        } else {
            out.extend_from_slice(&n.to_le_bytes());
        }
    }
    for &o in &offsets {
        out.extend_from_slice(&o.to_le_bytes());
    }
    out.extend_from_slice(&strtab);

    if ext && !entry.ext.is_empty() {
        append_extended(&mut out, &entry.ext, num_width);
    }
    out
}

/// Append the extended (user-defined) section, mirroring ncurses' `-x` layout: an even-aligned
/// header `[eb, en, es, str_count, str_size]`, the extended boolean bytes (+pad), the extended
/// numbers, an offset table of `es` value offsets then `eb+en+es` name offsets, and a string table
/// of the present value strings followed by every cap name. Cancelled strings keep a `-2` value
/// offset and no value string.
fn append_extended(out: &mut Vec<u8>, ext: &[ExtCap], num_width: usize) {
    let bools: Vec<&String> = ext
        .iter()
        .filter_map(|e| if let ExtCap::Bool(n) = e { Some(n) } else { None })
        .collect();
    let nums: Vec<(&String, i32)> = ext
        .iter()
        .filter_map(|e| if let ExtCap::Num(n, v) = e { Some((n, *v)) } else { None })
        .collect();
    let strs: Vec<(&String, &Option<Vec<u8>>)> = ext
        .iter()
        .filter_map(|e| if let ExtCap::Str(n, v) = e { Some((n, v)) } else { None })
        .collect();
    let (eb, en, es) = (bools.len(), nums.len(), strs.len());

    // Value strings (present only) then names (every cap). Offsets: values from the table start,
    // names relative to the end of the value region.
    let mut value_tab: Vec<u8> = Vec::new();
    let mut val_offsets: Vec<i16> = Vec::with_capacity(es);
    for (_, v) in &strs {
        match v {
            Some(bytes) => {
                val_offsets.push(value_tab.len() as i16);
                value_tab.extend_from_slice(bytes);
                value_tab.push(0);
            }
            None => val_offsets.push(-2),
        }
    }
    let mut name_tab: Vec<u8> = Vec::new();
    let mut name_offsets: Vec<i16> = Vec::with_capacity(eb + en + es);
    let push_name = |tab: &mut Vec<u8>, offs: &mut Vec<i16>, name: &str| {
        offs.push(tab.len() as i16);
        tab.extend_from_slice(name.as_bytes());
        tab.push(0);
    };
    for n in &bools {
        push_name(&mut name_tab, &mut name_offsets, n);
    }
    for (n, _) in &nums {
        push_name(&mut name_tab, &mut name_offsets, n);
    }
    for (n, _) in &strs {
        push_name(&mut name_tab, &mut name_offsets, n);
    }

    let present_values = val_offsets.iter().filter(|&&o| o >= 0).count();
    let str_count = present_values + eb + en + es;
    let str_size = value_tab.len() + name_tab.len();

    // The extended section starts on an even boundary.
    if out.len() % 2 == 1 {
        out.push(0);
    }
    let push16 = |o: &mut Vec<u8>, v: u16| o.extend_from_slice(&v.to_le_bytes());
    push16(out, eb as u16);
    push16(out, en as u16);
    push16(out, es as u16);
    push16(out, str_count as u16);
    push16(out, str_size as u16);
    for n in &bools {
        let _ = n;
        out.push(1); // extended booleans present in source are true
    }
    if eb % 2 == 1 {
        out.push(0);
    }
    for (_, v) in &nums {
        if num_width == 2 {
            out.extend_from_slice(&(*v as i16).to_le_bytes());
        } else {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    for &o in &val_offsets {
        out.extend_from_slice(&o.to_le_bytes());
    }
    for &o in &name_offsets {
        out.extend_from_slice(&o.to_le_bytes());
    }
    out.extend_from_slice(&value_tab);
    out.extend_from_slice(&name_tab);
}

fn split_entries(text: &str) -> Vec<String> {
    // Strip comment lines, then group: an entry begins at a non-indented, non-empty line.
    let mut entries = Vec::new();
    let mut cur = String::new();
    for line in text.lines() {
        if line.starts_with('#') {
            continue;
        }
        let is_header = !line.is_empty() && !line.starts_with([' ', '\t']);
        if is_header && !cur.trim().is_empty() {
            entries.push(std::mem::take(&mut cur));
        }
        cur.push_str(line);
        cur.push('\n');
    }
    if !cur.trim().is_empty() {
        entries.push(cur);
    }
    entries
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut out_dir: Option<String> = None;
    let mut input: Option<String> = None;
    let mut ext = false;
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "-o" if i + 1 < argv.len() => {
                out_dir = Some(argv[i + 1].clone());
                i += 1;
            }
            a if a.starts_with("-o") && a.len() > 2 => out_dir = Some(a[2..].to_string()),
            "-x" => ext = true,
            // Accept and ignore the common flags that do not change compiled bytes.
            "-1" | "-v" | "-s" | "-c" | "-r" | "-a" | "-g" | "-q" => {}
            a if a.starts_with('-') && a != "-" => {}
            a => input = Some(a.to_string()),
        }
        i += 1;
    }

    let src = match input.as_deref() {
        None | Some("-") => {
            use std::io::Read;
            let mut s = String::new();
            if std::io::stdin().read_to_string(&mut s).is_err() {
                exit(1);
            }
            s
        }
        Some(path) => match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("tic: cannot read {path}: {e}");
                exit(1);
            }
        },
    };

    let dir = out_dir.unwrap_or_else(|| {
        std::env::var("TERMINFO").unwrap_or_else(|_| "/usr/share/terminfo".to_string())
    });

    for body in split_entries(&src) {
        let Some(entry) = parse_entry(&body) else {
            continue;
        };
        let primary = entry.names.split('|').next().unwrap_or("").trim().to_string();
        if primary.is_empty() {
            continue;
        }
        let compiled = compile(&entry, ext);
        let sub = &primary[..1];
        let subdir = PathBuf::from(&dir).join(sub);
        if let Err(e) = std::fs::create_dir_all(&subdir) {
            eprintln!("tic: cannot create {}: {e}", subdir.display());
            exit(1);
        }
        let path = subdir.join(&primary);
        if let Err(e) = std::fs::write(&path, &compiled) {
            eprintln!("tic: cannot write {}: {e}", path.display());
            exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::unescape;

    #[test]
    fn unescape_letter_and_octal() {
        assert_eq!(unescape(b"\\E[H"), vec![0x1b, b'[', b'H']);
        assert_eq!(unescape(b"\\n\\r\\t"), vec![0x0a, 0x0d, 0x09]);
        assert_eq!(unescape(b"\\0"), vec![0x80]); // NUL stored as 0x80
        assert_eq!(unescape(b"\\200"), vec![0x80]);
        assert_eq!(unescape(b"\\201"), vec![0x81]);
        assert_eq!(unescape(b"\\177"), vec![0x7f]);
        assert_eq!(unescape(b"\\s"), vec![b' ']);
    }

    #[test]
    fn unescape_caret_and_specials() {
        assert_eq!(unescape(b"^A"), vec![0x01]);
        assert_eq!(unescape(b"^?"), vec![0x7f]);
        assert_eq!(unescape(b"\\^\\\\\\,"), vec![b'^', b'\\', b',']);
    }

    #[test]
    fn unescape_percent_operator_keeps_caret_literal() {
        // `%^` is the XOR operator: the caret stays a literal byte, not a control char.
        assert_eq!(unescape(b"%^"), vec![b'%', b'^']);
        assert_eq!(unescape(b"%^A"), vec![b'%', b'^', b'A']);
        assert_eq!(unescape(b"%\\,"), vec![b'%', b',']); // escape after % still expands
        assert_eq!(unescape(b"a%^b"), vec![b'a', b'%', b'^', b'b']);
    }
}
