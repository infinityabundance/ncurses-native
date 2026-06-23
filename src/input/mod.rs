//! Deterministic input decoding -- the part of the input model that does not
//! depend on read timing: naming a key code ([`keyname`]) and the terminfo-built
//! key map ([`KeyMap`]: [`KeyMap::key_defined`], [`KeyMap::has_key`]).
//!
//! Interactive reading (`getch`/`wgetch`/`getstr`/`scanw`) and the ESC-timeout
//! ambiguity are *not* reconstructed here -- they consume terminal input under a
//! timing model and have no deterministic terminal-output contract. This module
//! is the slice that can be pinned by a court: NCURSES.KEYNAME and
//! NCURSES.KEY.DEFINED compare these against ncurses.

pub mod keys;

/// `keyname` -- the printable name of a key code, matching ncurses:
/// printable bytes as themselves, control bytes as `^X`, `0x7f` as `^?`, meta
/// bytes as `M-<name>`, and `KEY_*` codes via the generated [`keys::CODE_NAMES`]
/// table. Returns `None` where ncurses returns `NULL` (an unknown high code).
pub fn keyname(code: i32) -> Option<String> {
    if code == -1 {
        return Some("-1".to_string());
    }
    if code < 0 {
        return None;
    }
    if code <= 0xff {
        let c = code as u8;
        return Some(match c {
            0x80..=0xff => format!("M-{}", keyname((c & 0x7f) as i32).unwrap_or_default()),
            0x7f => "^?".to_string(),
            0..=0x1f => format!("^{}", (c ^ 0x40) as char),
            _ => (c as char).to_string(),
        });
    }
    keys::CODE_NAMES
        .iter()
        .find(|(k, _)| *k == code)
        .map(|(_, n)| n.to_string())
}

/// A terminal's key map: the sequences its terminfo key capabilities bind to
/// `KEY_*` codes. Built from a parsed [`crate::Terminfo`].
#[derive(Debug, Clone, Default)]
pub struct KeyMap {
    /// (sequence bytes, key code), one per present key capability.
    bindings: Vec<(Vec<u8>, i32)>,
}

impl KeyMap {
    /// Build the key map from a terminfo entry: for every key capability the
    /// entry defines (the `k*` caps in [`keys::CAP_CODES`]), bind its byte
    /// sequence to the corresponding `KEY_*` code.
    pub fn from_terminfo(t: &crate::Terminfo) -> KeyMap {
        let mut bindings = Vec::new();
        for &(cap, code) in keys::CAP_CODES {
            if let Some(seq) = t.string(cap) {
                if !seq.is_empty() {
                    bindings.push((seq.to_vec(), code));
                }
            }
        }
        KeyMap { bindings }
    }

    /// `key_defined` -- the key code an exact sequence is bound to, `-1` if the
    /// sequence is a proper prefix of a bound key (incomplete), or `0` if it is
    /// not the start of any bound key.
    pub fn key_defined(&self, seq: &[u8]) -> i32 {
        if seq.is_empty() {
            return 0;
        }
        if let Some((_, code)) = self.bindings.iter().find(|(s, _)| s == seq) {
            return *code;
        }
        if self.bindings.iter().any(|(s, _)| s.len() > seq.len() && &s[..seq.len()] == seq) {
            return -1;
        }
        0
    }

    /// `has_key` -- whether the terminal defines a key with this code.
    pub fn has_key(&self, code: i32) -> bool {
        self.bindings.iter().any(|(_, c)| *c == code)
    }

    /// Decode a **complete** input byte buffer into the sequence of codes `wgetch` would return:
    /// at each position the longest bound key sequence that is a prefix of the remaining bytes is
    /// consumed and its `KEY_*` code emitted; a byte that starts no key is returned literally
    /// (`0..=255`). This reproduces ncurses' trie traversal for the case where every byte is
    /// already available (no `ESCDELAY` ambiguity); a lone trailing prefix (e.g. a bare `ESC`)
    /// would, in ncurses, depend on the timer and is out of this pure decoder's scope.
    pub fn decode(&self, input: &[u8]) -> Vec<i32> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < input.len() {
            let rest = &input[i..];
            let mut best: Option<(usize, i32)> = None;
            for (seq, code) in &self.bindings {
                if rest.len() >= seq.len()
                    && &rest[..seq.len()] == seq.as_slice()
                    && best.map_or(true, |(n, _)| seq.len() > n)
                {
                    best = Some((seq.len(), *code));
                }
            }
            match best {
                Some((n, code)) => {
                    out.push(code);
                    i += n;
                }
                None => {
                    out.push(input[i] as i32);
                    i += 1;
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyname_formats() {
        assert_eq!(keyname(65).as_deref(), Some("A"));
        assert_eq!(keyname(9).as_deref(), Some("^I"));
        assert_eq!(keyname(1).as_deref(), Some("^A"));
        assert_eq!(keyname(27).as_deref(), Some("^["));
        assert_eq!(keyname(127).as_deref(), Some("^?"));
        assert_eq!(keyname(26).as_deref(), Some("^Z"));
        assert_eq!(keyname(193).as_deref(), Some("M-A")); // 128 + 'A'
        assert_eq!(keyname(259).as_deref(), Some("KEY_UP"));
        assert_eq!(keyname(265).as_deref(), Some("KEY_F(1)"));
        assert_eq!(keyname(-1).as_deref(), Some("-1"));
        assert_eq!(keyname(633), None); // unknown high code
    }

    #[test]
    fn key_map_from_xterm() {
        const XTERM: &[u8] = include_bytes!("../../tests/terminfo/xterm");
        let t = crate::Terminfo::parse(XTERM).unwrap();
        let km = KeyMap::from_terminfo(&t);
        // xterm application-mode arrow + function keys.
        assert_eq!(km.key_defined(b"\x1bOA"), 259); // KEY_UP (kcuu1)
        assert_eq!(km.key_defined(b"\x1bOP"), 265); // KEY_F(1) (kf1)
        assert_eq!(km.key_defined(b"\x1b[3~"), 330); // KEY_DC (kdch1)
        assert_eq!(km.key_defined(b"\x1b"), -1); // proper prefix -> incomplete
        assert_eq!(km.key_defined(b"\x1b[A"), 0); // not bound in xterm app mode
        assert!(km.has_key(259) && km.has_key(265));
        assert!(!km.has_key(99999));
    }

    #[test]
    fn decode_mixed_buffer() {
        const XTERM: &[u8] = include_bytes!("../../tests/terminfo/xterm");
        let t = crate::Terminfo::parse(XTERM).unwrap();
        let km = KeyMap::from_terminfo(&t);
        // plain text, an arrow key, more text, F1, Delete.
        assert_eq!(
            km.decode(b"hi\x1bOAx\x1bOP\x1b[3~"),
            vec![104, 105, 259, 120, 265, 330]
        );
        // a byte that starts no key is literal.
        assert_eq!(km.decode(b"AB"), vec![65, 66]);
    }
}
