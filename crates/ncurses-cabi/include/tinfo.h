/*
 * tinfo.h -- C header for the native-Rust ncurses C-ABI (ncurses-cabi).
 *
 * Declares the low-level terminfo ("tinfo") symbols currently exported by
 * libncurses_cabi.{so,a}, which are verified byte-identical to the system
 * libtinfo by the NCURSES.CABI oracle court. This header is intentionally a
 * *subset*: it grows as more of the surface is wired through the C ABI. It is
 * not yet the full curses.h (the window/screen API and the CPP macro API --
 * getyx, COLOR_PAIR, ACS_* -- are not provided here; see gap-ledger STRUCT-02).
 *
 * Link:  cc prog.c -lncurses_cabi          (shared object)
 *        cc prog.c libncurses_cabi.a -lpthread -ldl -lm   (static archive)
 */
#ifndef NCURSES_NATIVE_TINFO_H
#define NCURSES_NATIVE_TINFO_H

#ifdef __cplusplus
extern "C" {
#endif

/* Return codes (curses contract). */
#define OK    0
#define ERR   (-1)
#define TRUE  1
#define FALSE 0

/* The library identity string ("do not free"). */
extern const char *curses_version(void);

/*
 * Low-level terminfo interface. A single thread-local `cur_term` holds the
 * entry loaded by setupterm(); the strings returned by tigetstr()/tgoto()
 * remain valid until the next setupterm() (ncurses' ownership contract).
 */

/* Load the terminfo entry for `term` (or $TERM when NULL). Returns OK/ERR;
 * when `errret` is non-NULL it receives 1 (OK) or 0 (not found). */
extern int setupterm(const char *term, int filedes, int *errret);

/* String capability by name: the bytes, NULL if absent, or (char *)-1 if
 * `capname` is not a string capability. */
extern char *tigetstr(const char *capname);

/* Numeric capability by name: the value, -1 if absent, -2 if cancelled. */
extern int tigetnum(const char *capname);

/* Boolean capability by name: 1 if set, 0 if absent, -1 if cancelled. */
extern int tigetflag(const char *capname);

/* Process padding in `str` and emit each byte through `putc`. Returns OK. */
extern int tputs(const char *str, int affcnt, int (*putc)(int));

/* tputs(str, 1, putchar): process padding and write to stdout. Returns OK. */
extern int putp(const char *str);

/* Instantiate a cursor-addressing cap; returns a reused static buffer. */
extern char *tgoto(const char *cap, int col, int row);

/* Instantiate a parameterized cap (numeric params). Variadic, like ncurses:
 * reads up to nine `long` parameters. Returns a reused static buffer. */
extern char *tparm(const char *cap, ...);

#ifdef __cplusplus
}
#endif

#endif /* NCURSES_NATIVE_TINFO_H */
