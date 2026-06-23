//! `libtinfo` -- the standalone low-level terminfo / `tparm` / `tputs` library (soname
//! `libtinfo.so.6`). ncurses' "termlib" split ships these capability-access functions separately
//! from the curses screen API so programs needing only terminfo (bash, less, readline) link a
//! minimal library. This cdylib exports *exactly* that subset and nothing else (`nm -D` matches the
//! real `libtinfo`'s surface); the version script versions the symbols under the tinfo node.
//!
//! The C-ABI bodies mirror the tinfo layer of `ncurses-cabi` (the full `libncursesw`) over the same
//! `ncurses_native` core. It is a dedicated crate rather than a re-export because a cdylib that
//! *depended* on `ncurses-cabi` would re-export its curses symbols too (rustc exports a
//! dependency's `#[no_mangle]` symbols), which would defeat the minimal split.

#![allow(non_camel_case_types, non_upper_case_globals)]
// Every entry point carries the standard C-ABI pointer contract (valid NUL-terminated strings /
// writable out-params), exactly as the C `<term.h>` prototypes require.
#![allow(clippy::missing_safety_doc)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_long};

use ncurses_native::{putp as core_putp, tgoto as core_tgoto, tputs as core_tputs};
use ncurses_native::{tparm_n as core_tparm_n, Terminfo, Tigetstr};

const OK: c_int = 0;
const ERR: c_int = -1;

/// `chtype` -- packed char+attr+color cell (32-bit default ABI).
pub type chtype = u32;

/// ncurses' `acs_map[]` (a TINFO global): `ACS_*` / `NCURSES_ACS(c)` expand to `acs_map[(uchar)c]`.
/// It is the terminal-independent identity map with `A_ALTCHARSET` (0x400000) set on every entry --
/// the per-terminal `acsc` glyph translation happens at output time, not here (verified against real
/// ncurses: e.g. cygwin maps `a`->0xb1 in `acsc`, yet `acs_map['a']` is still `'a'|A_ALTCHARSET`).
const fn make_acs_map() -> [chtype; 128] {
    let mut m = [0u32; 128];
    let mut c = 0;
    while c < 128 {
        m[c] = c as u32 | 0x0040_0000;
        c += 1;
    }
    m
}
#[no_mangle]
pub static mut acs_map: [chtype; 128] = make_acs_map();

static VERSION: &[u8] = b"ncurses-native 0.1.0\0";

/// `curses_version` -- the library identity string (real ncurses symbol).
#[no_mangle]
pub extern "C" fn curses_version() -> *const c_char {
    VERSION.as_ptr() as *const c_char
}

struct TermState {
    ti: Terminfo,
    /// Owns the NUL-terminated cap strings handed out by `tigetstr`, keyed by name, so the returned
    /// pointers remain valid until the next `setupterm`.
    cache: HashMap<String, CString>,
}

thread_local! {
    static CUR_TERM: RefCell<Option<TermState>> = const { RefCell::new(None) };
    static TGOTO_BUF: RefCell<CString> = RefCell::new(CString::default());
    static TPARM_BUF: RefCell<CString> = RefCell::new(CString::default());
}

/// ncurses' `(char *)-1` sentinel ("not a string capability").
fn minus_one() -> *mut c_char {
    (-1isize) as *mut c_char
}

fn cstr_to_string(p: *const c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
}

/// `setupterm(term, fildes, errret)` -- load the terminfo entry for `term` (or `$TERM`) into the
/// thread-local `cur_term`. Returns `OK`/`ERR`; when `errret` is non-NULL it receives ncurses'
/// status code (`1` = OK, `0` = not found).
#[no_mangle]
pub unsafe extern "C" fn setupterm(
    term: *const c_char,
    _fildes: c_int,
    errret: *mut c_int,
) -> c_int {
    let name = cstr_to_string(term).unwrap_or_else(|| std::env::var("TERM").unwrap_or_default());
    let set_err = |v: c_int| {
        if !errret.is_null() {
            unsafe { *errret = v };
        }
    };
    match Terminfo::load(&name) {
        Ok(ti) => {
            CUR_TERM.with(|c| {
                *c.borrow_mut() = Some(TermState { ti, cache: HashMap::new() });
            });
            set_err(1);
            OK
        }
        Err(_) => {
            set_err(0);
            ERR
        }
    }
}

/// `tigetstr(capname)` -- the string capability, or `NULL` if absent / `(char *)-1` if `capname` is
/// not a string capability. The pointer is owned by `cur_term`.
#[no_mangle]
pub unsafe extern "C" fn tigetstr(capname: *const c_char) -> *mut c_char {
    let name = match cstr_to_string(capname) {
        Some(n) => n,
        None => return minus_one(),
    };
    CUR_TERM.with(|c| {
        let mut b = c.borrow_mut();
        let st = match b.as_mut() {
            Some(s) => s,
            None => return minus_one(),
        };
        match st.ti.tigetstr(&name) {
            Tigetstr::Value(v) => match CString::new(v) {
                Ok(cs) => {
                    st.cache.insert(name.clone(), cs);
                    st.cache.get(&name).unwrap().as_ptr() as *mut c_char
                }
                Err(_) => std::ptr::null_mut(),
            },
            Tigetstr::Absent => std::ptr::null_mut(),
            Tigetstr::NotString => minus_one(),
        }
    })
}

/// `tigetnum(capname)` -- the numeric capability (`-1` absent, `-2` cancelled).
#[no_mangle]
pub unsafe extern "C" fn tigetnum(capname: *const c_char) -> c_int {
    let name = match cstr_to_string(capname) {
        Some(n) => n,
        None => return -1,
    };
    CUR_TERM.with(|c| c.borrow().as_ref().map(|st| st.ti.tigetnum(&name)).unwrap_or(-1))
}

/// `tigetflag(capname)` -- the boolean capability (`1` true, `0` absent, `-1` cancelled).
#[no_mangle]
pub unsafe extern "C" fn tigetflag(capname: *const c_char) -> c_int {
    let name = match cstr_to_string(capname) {
        Some(n) => n,
        None => return -1,
    };
    CUR_TERM.with(|c| c.borrow().as_ref().map(|st| st.ti.tigetflag(&name)).unwrap_or(0))
}

/// `tputs(str, affcnt, putc)` -- process padding in `str` and emit each resulting byte through the
/// `putc` callback. Returns `OK`.
#[no_mangle]
pub unsafe extern "C" fn tputs(
    string: *const c_char,
    _affcnt: c_int,
    putc: extern "C" fn(c_int) -> c_int,
) -> c_int {
    if string.is_null() {
        return ERR;
    }
    let bytes = unsafe { CStr::from_ptr(string) }.to_bytes().to_vec();
    for b in core_tputs(&bytes) {
        putc(b as c_int);
    }
    OK
}

/// `putp(str)` -- like `tputs(str, 1, putchar)`: process padding and write the bytes to stdout.
#[no_mangle]
pub unsafe extern "C" fn putp(string: *const c_char) -> c_int {
    if string.is_null() {
        return ERR;
    }
    let bytes = unsafe { CStr::from_ptr(string) }.to_bytes().to_vec();
    use std::io::Write;
    let out = core_putp(&bytes);
    match std::io::stdout().write_all(&out) {
        Ok(()) => OK,
        Err(_) => ERR,
    }
}

/// `tgoto(cap, col, row)` -- instantiate a cursor-addressing capability; returns a pointer into a
/// reused thread-local buffer (ncurses' static-buffer contract).
#[no_mangle]
pub unsafe extern "C" fn tgoto(cap: *const c_char, col: c_int, row: c_int) -> *mut c_char {
    let bytes = match cstr_to_string(cap) {
        Some(s) => s.into_bytes(),
        None => return std::ptr::null_mut(),
    };
    let out = core_tgoto(&bytes, col, row);
    TGOTO_BUF.with(|b| {
        *b.borrow_mut() = CString::new(out).unwrap_or_default();
        b.borrow().as_ptr() as *mut c_char
    })
}

/// `tparm(cap, ...)` -- instantiate a parameterized capability (nine `c_long` params,
/// call-compatible with the variadic ncurses prototype). Numeric parameters only. Returns a pointer
/// into a reused thread-local buffer.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn tparm(
    cap: *const c_char,
    a1: c_long,
    a2: c_long,
    a3: c_long,
    a4: c_long,
    a5: c_long,
    a6: c_long,
    a7: c_long,
    a8: c_long,
    a9: c_long,
) -> *mut c_char {
    let bytes = match cstr_to_string(cap) {
        Some(s) => s.into_bytes(),
        None => return std::ptr::null_mut(),
    };
    let params = [a1, a2, a3, a4, a5, a6, a7, a8, a9].map(|x| x as i32);
    let out = core_tparm_n(&bytes, &params);
    TPARM_BUF.with(|b| {
        *b.borrow_mut() = CString::new(out).unwrap_or_default();
        b.borrow().as_ptr() as *mut c_char
    })
}
