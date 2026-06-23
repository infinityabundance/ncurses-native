//! # ncurses-terminal
//!
//! The **isolated unsafe tty boundary** for `ncurses-native`.
//!
//! `ncurses-native` is `#![forbid(unsafe_code)]` and dependency-free: it reconstructs every
//! *byte-output and state* surface of ncurses as pure functions (terminfo, tparm/tputs, the
//! window/cell model, the cursor optimizer, color, attributes, key decoding). A handful of ncurses
//! surfaces require raw operating-system terminal syscalls and cannot be reconstructed in safe
//! Rust; ALL such code is quarantined here -- the single crate permitted `unsafe` and OS FFI -- so
//! the core's safety claim is preserved and the trusted-unsafe surface is small and auditable.
//!
//! Implemented: raw-mode entry/exit ([`RawMode`], `termios`/`tcsetattr`) and an interactive key
//! reader ([`Keys`]) that turns real tty input into `KEY_*` codes by driving the byte-exact
//! [`ncurses_native::KeyMap`] decoder. The OS interface (`termios`, `tcgetattr`/`tcsetattr`,
//! `read`, `poll`) is hand-declared for Linux so the workspace stays dependency-free.
//!
//! Still quarantined-but-unbuilt: live `initscr`/`endwin` to a file descriptor, `napms`,
//! `SIGWINCH`/resize, and crash-cleanup (gap ledger IO-*/SIG-*/TIM-*).

use std::collections::VecDeque;
use std::os::raw::{c_int, c_void};

use ncurses_native::KeyMap;

// --- Linux termios FFI (hand-declared; no libc dependency) ------------------

const NCCS: usize = 32;

#[repr(C)]
#[derive(Clone, Copy)]
struct Termios {
    c_iflag: u32,
    c_oflag: u32,
    c_cflag: u32,
    c_lflag: u32,
    c_line: u8,
    c_cc: [u8; NCCS],
    c_ispeed: u32,
    c_ospeed: u32,
}

impl Termios {
    fn zeroed() -> Termios {
        // SAFETY: Termios is a plain-old-data struct; all-zero is a valid bit pattern.
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
struct PollFd {
    fd: c_int,
    events: i16,
    revents: i16,
}

extern "C" {
    fn tcgetattr(fd: c_int, termios_p: *mut Termios) -> c_int;
    fn tcsetattr(fd: c_int, optional_actions: c_int, termios_p: *const Termios) -> c_int;
    fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize;
    fn poll(fds: *mut PollFd, nfds: u64, timeout: c_int) -> c_int;
}

// termios flag bits (Linux).
const IGNBRK: u32 = 0o000001;
const BRKINT: u32 = 0o000002;
const PARMRK: u32 = 0o000010;
const ISTRIP: u32 = 0o000040;
const INLCR: u32 = 0o000100;
const IGNCR: u32 = 0o000200;
const ICRNL: u32 = 0o000400;
const IXON: u32 = 0o002000;
const OPOST: u32 = 0o000001;
const ECHO: u32 = 0o000010;
const ECHONL: u32 = 0o000100;
const ICANON: u32 = 0o000002;
const ISIG: u32 = 0o000001;
const IEXTEN: u32 = 0o100000;
const CSIZE: u32 = 0o000060;
const PARENB: u32 = 0o000400;
const CS8: u32 = 0o000060;
const VMIN: usize = 6;
const VTIME: usize = 5;
const TCSAFLUSH: c_int = 2;
const POLLIN: i16 = 0x001;

/// A raw-mode guard for a terminal file descriptor: enters cfmakeraw-equivalent mode on
/// construction and **restores the original `termios` on drop** (so the tty is left clean even on
/// panic/early return). Returns `None` if the fd is not a terminal.
pub struct RawMode {
    fd: c_int,
    saved: Termios,
}

impl RawMode {
    /// Put `fd` into raw mode (no canonical line editing, no echo, no signal/flow/CR-NL
    /// translation; 8-bit; blocking read of >= 1 byte). Returns `None` if `tcgetattr` fails.
    pub fn enter(fd: c_int) -> Option<RawMode> {
        let mut t = Termios::zeroed();
        if unsafe { tcgetattr(fd, &mut t) } != 0 {
            return None;
        }
        let saved = t;
        t.c_iflag &= !(IGNBRK | BRKINT | PARMRK | ISTRIP | INLCR | IGNCR | ICRNL | IXON);
        t.c_oflag &= !OPOST;
        t.c_lflag &= !(ECHO | ECHONL | ICANON | ISIG | IEXTEN);
        t.c_cflag &= !(CSIZE | PARENB);
        t.c_cflag |= CS8;
        t.c_cc[VMIN] = 1; // a blocking read returns at least one byte
        t.c_cc[VTIME] = 0;
        if unsafe { tcsetattr(fd, TCSAFLUSH, &t) } != 0 {
            return None;
        }
        Some(RawMode { fd, saved })
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        unsafe { tcsetattr(self.fd, TCSAFLUSH, &self.saved) };
    }
}

/// An interactive key reader over a terminal fd: it reads raw bytes and returns the `KEY_*` /
/// literal-byte codes ncurses' `wgetch` would, by driving the byte-exact [`KeyMap`] decoder.
///
/// Each [`next_code`](Keys::next_code) blocks for the first byte, then drains any bytes that arrive
/// within `inter_byte_ms` (the typeahead / escape-sequence window, analogous to `ESCDELAY`), decodes
/// the whole batch with the trie, and returns codes one at a time. With a fully-typed sequence the
/// batch holds the complete key, so the result matches ncurses.
pub struct Keys {
    fd: c_int,
    km: KeyMap,
    queue: VecDeque<i32>,
    inter_byte_ms: c_int,
    mouse_q: VecDeque<(u32, i32, i32, bool)>,
}

/// `KEY_MOUSE` -- returned by the reader when an SGR mouse report is decoded; the event is then
/// retrieved with [`Keys::take_mouse`].
pub const KEY_MOUSE: i32 = 0o631;

/// Parse a leading SGR mouse report `\e[<b;x;yM`/`m`. Returns `((button, x0, y0, press), len)` where
/// x0/y0 are 0-based and `press` is true for `M`. None if not a complete report.
fn parse_sgr_mouse(buf: &[u8]) -> Option<((u32, i32, i32, bool), usize)> {
    // buf begins with b"\x1b[<"; read three ';'-separated decimals, then 'M' or 'm'.
    let mut i = 3;
    let mut nums = [0i64; 3];
    for slot in nums.iter_mut() {
        let mut any = false;
        let mut v = 0i64;
        while i < buf.len() && buf[i].is_ascii_digit() {
            v = v * 10 + (buf[i] - b'0') as i64;
            i += 1;
            any = true;
        }
        if !any {
            return None;
        }
        *slot = v;
        if i < buf.len() && buf[i] == b';' {
            i += 1;
        }
    }
    if i >= buf.len() {
        return None;
    }
    let press = match buf[i] {
        b'M' => true,
        b'm' => false,
        _ => return None,
    };
    i += 1;
    let (b, x, y) = (nums[0] as u32, nums[1] as i32, nums[2] as i32);
    Some(((b, x - 1, y - 1, press), i))
}

impl Keys {
    /// A reader on `fd` using the given key map. `inter_byte_ms` is how long to wait for more bytes
    /// of a multi-byte key after the first (analogous to ncurses' `ESCDELAY`; ~50 ms suffices for
    /// buffered/typed input).
    pub fn new(fd: c_int, km: KeyMap, inter_byte_ms: i32) -> Keys {
        Keys {
            fd,
            km,
            queue: VecDeque::new(),
            inter_byte_ms: inter_byte_ms as c_int,
            mouse_q: VecDeque::new(),
        }
    }

    fn readable(&self, timeout_ms: c_int) -> bool {
        let mut pfd = PollFd {
            fd: self.fd,
            events: POLLIN,
            revents: 0,
        };
        let n = unsafe { poll(&mut pfd, 1, timeout_ms) };
        n > 0 && (pfd.revents & POLLIN) != 0
    }

    fn read_some(&self, buf: &mut Vec<u8>) -> usize {
        let mut chunk = [0u8; 64];
        let n = unsafe { read(self.fd, chunk.as_mut_ptr() as *mut c_void, chunk.len()) };
        if n > 0 {
            buf.extend_from_slice(&chunk[..n as usize]);
            n as usize
        } else {
            0
        }
    }

    /// The next key code, or `None` on end of input. Literal bytes come back as `0..=255`; recognized
    /// key sequences come back as their `KEY_*` code. Blocks for the first byte.
    pub fn next_code(&mut self) -> Option<i32> {
        self.next_code_timed(-1)
    }

    /// As [`next_code`](Keys::next_code) but with a read timeout: `timeout_ms < 0` blocks,
    /// `== 0` is non-blocking (returns `None` immediately if no input -- `nodelay`), `> 0` waits up
    /// to that many milliseconds (`timeout`/`wtimeout`/`halfdelay`).
    pub fn next_code_timed(&mut self, timeout_ms: c_int) -> Option<i32> {
        if let Some(c) = self.queue.pop_front() {
            return Some(c);
        }
        // Wait for the first byte up to the timeout (poll only when a timeout is requested).
        if timeout_ms >= 0 && !self.readable(timeout_ms) {
            return None;
        }
        let mut buf = Vec::new();
        // Block (or, post-poll, read) the first byte.
        if self.read_some(&mut buf) == 0 {
            return None;
        }
        // Drain the rest of a multi-byte key / typeahead within the inter-byte window.
        while self.readable(self.inter_byte_ms) {
            if self.read_some(&mut buf) == 0 {
                break;
            }
        }
        // SGR mouse report `\e[<b;x;yM` (press) / `m` (release): decode to KEY_MOUSE + a stashed
        // event, ahead of the key trie (the trie does not model mouse). Any trailing bytes decode
        // normally.
        let mut pos = 0;
        while buf[pos..].starts_with(b"\x1b[<") {
            match parse_sgr_mouse(&buf[pos..]) {
                Some((ev, consumed)) => {
                    self.mouse_q.push_back(ev);
                    self.queue.push_back(KEY_MOUSE);
                    pos += consumed;
                }
                None => break,
            }
        }
        for code in self.km.decode(&buf[pos..]) {
            self.queue.push_back(code);
        }
        self.queue.pop_front()
    }

    /// The next pending mouse event `(sgr_button, x0, y0, press)`, consumed by `getmouse`.
    pub fn take_mouse(&mut self) -> Option<(u32, i32, i32, bool)> {
        self.mouse_q.pop_front()
    }

    /// Push `code` back so the next [`next_code`](Keys::next_code) returns it (`ungetch`). Repeated
    /// calls are LIFO: the most-recently pushed code comes out first.
    pub fn unget(&mut self, code: i32) {
        self.queue.push_front(code);
    }

    /// Discard any queued (typed-ahead / pushed-back) input (`flushinp`).
    pub fn flush(&mut self) {
        self.queue.clear();
    }

    /// The next *wide* input event -- the `wget_wch` model. A function/special key comes back as
    /// [`WideKey::Code`] (ncurses returns `KEY_CODE_YES`); a regular character is assembled from its
    /// UTF-8 byte sequence and comes back as [`WideKey::Char`] (ncurses returns `OK`). Function-key
    /// escape sequences (`ESC`-led, codes >= 256) and UTF-8 lead bytes (0x80..) never overlap, so the
    /// disambiguation is exact.
    pub fn next_wide(&mut self) -> Option<WideKey> {
        let first = self.next_code()?;
        if first >= 256 {
            return Some(WideKey::Code(first));
        }
        if first < 0x80 {
            return Some(WideKey::Char(first as u32));
        }
        let lead = first as u8;
        let (len, init) = if lead >> 5 == 0b110 {
            (2, (lead & 0x1f) as u32)
        } else if lead >> 4 == 0b1110 {
            (3, (lead & 0x0f) as u32)
        } else if lead >> 3 == 0b1_1110 {
            (4, (lead & 0x07) as u32)
        } else {
            return Some(WideKey::Char(lead as u32)); // stray byte: pass through
        };
        let mut cp = init;
        for _ in 1..len {
            match self.next_code() {
                Some(b) if (0x80..0xC0).contains(&b) => {
                    cp = (cp << 6) | ((b as u32) & 0x3f);
                }
                Some(other) => {
                    // Not a continuation byte: push it back and yield the lead as-is.
                    self.queue.push_front(other);
                    return Some(WideKey::Char(lead as u32));
                }
                None => return Some(WideKey::Char(lead as u32)),
            }
        }
        Some(WideKey::Char(cp))
    }
}

/// A wide input event from [`Keys::next_wide`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WideKey {
    /// A function/special key (a `KEY_*` code) -- `wget_wch` returns `KEY_CODE_YES`.
    Code(i32),
    /// A character codepoint -- `wget_wch` returns `OK`.
    Char(u32),
}
