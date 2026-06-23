/*
 * curses.h -- C header for the native-Rust curses C-ABI (ncurses-cabi).
 *
 * Provides source-level compatibility for curses programs: the typedefs, the OK/ERR + A_* + COLOR_*
 * + KEY_* + ACS_* constants, the CPP macro API that has no symbol (getyx/getmaxyx/getbegyx,
 * COLOR_PAIR/PAIR_NUMBER), and prototypes for the exported functions. Verified by the
 * NCURSES.CURSES.HEADER oracle court: a program using this macro API compiles + runs against
 * libncurses_cabi byte-identically to the same program against the system <curses.h>/libncursesw.
 *
 * This is a *subset* mirroring the wired C-ABI surface; it grows with the library. The WINDOW
 * struct is opaque (the getyx-family macros use accessor functions, not field access).
 */
#ifndef NCURSES_NATIVE_CURSES_H
#define NCURSES_NATIVE_CURSES_H

#include "tinfo.h" /* OK/ERR/TRUE/FALSE + the terminfo layer */
#include <wchar.h> /* wchar_t / wint_t for the wide-character (ncursesw) API */

#ifdef __cplusplus
extern "C" {
#endif

typedef unsigned int chtype;
typedef unsigned int attr_t;
typedef struct _win_st WINDOW;

/* Complex character (ncursesw): rendition + up to CCHARW_MAX wide chars (spacing char first).
 * This is the non-ext-color layout; the color pair is packed into `attr` (the A_COLOR bits). */
#define CCHARW_MAX 5
typedef struct {
    attr_t attr;
    wchar_t chars[CCHARW_MAX];
} cchar_t;

/* Mouse (NCURSES_MOUSE_VERSION 2): 5 bits per button; the event mask + MEVENT struct. */
typedef unsigned int mmask_t;
typedef struct {
    short id;
    int x, y, z;
    mmask_t bstate;
} MEVENT;
#define NCURSES_MOUSE_MASK(b, m) ((mmask_t)(m) << (((b) - 1) * 5))
#define BUTTON1_RELEASED       NCURSES_MOUSE_MASK(1, 001)
#define BUTTON1_PRESSED        NCURSES_MOUSE_MASK(1, 002)
#define BUTTON1_CLICKED        NCURSES_MOUSE_MASK(1, 004)
#define BUTTON1_DOUBLE_CLICKED NCURSES_MOUSE_MASK(1, 010)
#define BUTTON1_TRIPLE_CLICKED NCURSES_MOUSE_MASK(1, 020)
#define BUTTON2_RELEASED       NCURSES_MOUSE_MASK(2, 001)
#define BUTTON2_PRESSED        NCURSES_MOUSE_MASK(2, 002)
#define BUTTON2_CLICKED        NCURSES_MOUSE_MASK(2, 004)
#define BUTTON3_RELEASED       NCURSES_MOUSE_MASK(3, 001)
#define BUTTON3_PRESSED        NCURSES_MOUSE_MASK(3, 002)
#define BUTTON3_CLICKED        NCURSES_MOUSE_MASK(3, 004)
#define BUTTON4_PRESSED        NCURSES_MOUSE_MASK(4, 002)
#define BUTTON5_PRESSED        NCURSES_MOUSE_MASK(5, 002)
#define BUTTON_CTRL            NCURSES_MOUSE_MASK(6, 001)
#define BUTTON_SHIFT           NCURSES_MOUSE_MASK(6, 002)
#define BUTTON_ALT             NCURSES_MOUSE_MASK(6, 004)
#define REPORT_MOUSE_POSITION  NCURSES_MOUSE_MASK(6, 010)
#define ALL_MOUSE_EVENTS       (REPORT_MOUSE_POSITION - 1)
extern mmask_t mousemask(mmask_t, mmask_t *);
extern int getmouse(MEVENT *);

/* Attribute bits (NCURSES_BITS layout: shift 8). */
#define NCURSES_ATTR_SHIFT 8
#define NCURSES_BITS(mask, shift) ((chtype)(mask) << ((shift) + NCURSES_ATTR_SHIFT))
#define A_NORMAL     0U
#define A_CHARTEXT   (NCURSES_BITS(1U, 0) - 1U)
#define A_COLOR      NCURSES_BITS(((1U) << 8) - 1U, 0)
#define A_ATTRIBUTES NCURSES_BITS(~(1U - 1U), 0)
#define A_STANDOUT   NCURSES_BITS(1U, 8)
#define A_UNDERLINE  NCURSES_BITS(1U, 9)
#define A_REVERSE    NCURSES_BITS(1U, 10)
#define A_BLINK      NCURSES_BITS(1U, 11)
#define A_DIM        NCURSES_BITS(1U, 12)
#define A_BOLD       NCURSES_BITS(1U, 13)
#define A_ALTCHARSET NCURSES_BITS(1U, 14)
#define A_INVIS      NCURSES_BITS(1U, 15)
#define A_PROTECT    NCURSES_BITS(1U, 16)

/* Colors. */
#define COLOR_BLACK   0
#define COLOR_RED     1
#define COLOR_GREEN   2
#define COLOR_YELLOW  3
#define COLOR_BLUE    4
#define COLOR_MAGENTA 5
#define COLOR_CYAN    6
#define COLOR_WHITE   7
#define COLOR_PAIR(n)    (NCURSES_BITS((n), 0) & A_COLOR)
#define PAIR_NUMBER(a)   (((a) & A_COLOR) >> NCURSES_ATTR_SHIFT)

/* Line-drawing (ACS): NCURSES_ACS(c) indexes the runtime acs_map[] global (the real ncurses
 * mechanism). acs_map is the identity map with A_ALTCHARSET set; the per-terminal acsc glyph
 * translation happens at output time, so acs_map[c] == (A_ALTCHARSET | c). */
extern chtype acs_map[];
#define NCURSES_ACS(c) (acs_map[(unsigned char)(c)])
#define ACS_ULCORNER NCURSES_ACS('l')
#define ACS_LLCORNER NCURSES_ACS('m')
#define ACS_URCORNER NCURSES_ACS('k')
#define ACS_LRCORNER NCURSES_ACS('j')
#define ACS_LTEE     NCURSES_ACS('t')
#define ACS_RTEE     NCURSES_ACS('u')
#define ACS_BTEE     NCURSES_ACS('v')
#define ACS_TTEE     NCURSES_ACS('w')
#define ACS_HLINE    NCURSES_ACS('q')
#define ACS_VLINE    NCURSES_ACS('x')
#define ACS_PLUS     NCURSES_ACS('n')
#define ACS_DIAMOND  NCURSES_ACS('`')
#define ACS_CKBOARD  NCURSES_ACS('a')
#define ACS_DEGREE   NCURSES_ACS('f')
#define ACS_BULLET   NCURSES_ACS('~')

/* Key codes (a representative subset of the terminfo-derived KEY_* set). */
#define KEY_CODE_YES 0400
#define KEY_DOWN     0402
#define KEY_UP       0403
#define KEY_LEFT     0404
#define KEY_RIGHT    0405
#define KEY_HOME     0406
#define KEY_BACKSPACE 0407
#define KEY_F0       0410
#define KEY_F(n)     (KEY_F0 + (n))
#define KEY_DC       0512
#define KEY_IC       0513
#define KEY_NPAGE    0522
#define KEY_PPAGE    0523
#define KEY_ENTER    0527
#define KEY_END      0550
#define KEY_RESIZE   0632
#define KEY_MOUSE    0631

/* The stdscr global (set by initscr). */
extern WINDOW *stdscr;

/* The getyx-family macros use accessor functions (the WINDOW struct is opaque). */
extern int getcury(const WINDOW *);
extern int getcurx(const WINDOW *);
extern int getbegy(const WINDOW *);
extern int getbegx(const WINDOW *);
extern int getmaxy(const WINDOW *);
extern int getmaxx(const WINDOW *);
#define getyx(win, y, x)    ((y) = getcury(win), (x) = getcurx(win))
#define getbegyx(win, y, x) ((y) = getbegy(win), (x) = getbegx(win))
#define getmaxyx(win, y, x) ((y) = getmaxy(win), (x) = getmaxx(win))

/* Lifecycle, input, color. */
extern WINDOW *initscr(void);
extern int endwin(void);
extern int cbreak(void);
extern int raw(void);
extern int noecho(void);
extern int echo(void);
extern int getnstr(char *, int);
extern int getstr(char *);
extern int wgetnstr(WINDOW *, char *, int);
extern int wgetstr(WINDOW *, char *);
extern int keypad(WINDOW *, int);
extern int getch(void);
extern int wgetch(WINDOW *);
extern int ungetch(int);
extern int flushinp(void);
extern int nodelay(WINDOW *, int);
extern void timeout(int);
extern void wtimeout(WINDOW *, int);
extern int halfdelay(int);
extern int notimeout(WINDOW *, int);
extern int nl(void);
extern int nonl(void);
extern int intrflush(WINDOW *, int);
extern int idlok(WINDOW *, int);
extern void immedok(WINDOW *, int);
extern int idcok(WINDOW *, int);
extern int start_color(void);
extern int init_pair(short, short, short);
extern int has_colors(void);
extern int can_change_color(void);
extern int COLORS;
extern int COLOR_PAIRS;
extern int curs_set(int);
extern int beep(void);
extern int flash(void);
extern int napms(int);

/* stdscr drawing. */
extern int refresh(void);
extern int doupdate(void);
extern int move(int, int);
extern int addch(chtype);
extern int addstr(const char *);
extern int mvaddch(int, int, chtype);
extern int mvwaddch(WINDOW *, int, int, chtype);
extern int mvaddstr(int, int, const char *);
extern int insch(chtype);
extern int delch(void);
extern int insstr(const char *);
extern int winsstr(WINDOW *, const char *);
extern int insertln(void);
extern int deleteln(void);
extern int hline(chtype, int);
extern int vline(chtype, int);
extern int mvhline(int, int, chtype, int);
extern int mvvline(int, int, chtype, int);
extern int mvwhline(WINDOW *, int, int, chtype, int);
extern int mvwvline(WINDOW *, int, int, chtype, int);
extern int border(chtype, chtype, chtype, chtype, chtype, chtype, chtype, chtype);
extern int attron(int);
extern int attroff(int);
extern int attrset(int);
extern int attr_on(attr_t, void *);
extern int attr_off(attr_t, void *);
extern int attr_set(attr_t, short, void *);
extern int wattr_on(WINDOW *, attr_t, void *);
extern int wattr_off(WINDOW *, attr_t, void *);
extern int wattr_set(WINDOW *, attr_t, short, void *);
extern int color_set(short, void *);
extern int wcolor_set(WINDOW *, short, void *);
extern int standout(void);
extern int standend(void);
extern int wstandout(WINDOW *);
extern int wstandend(WINDOW *);
extern int erase(void);
extern int clear(void);
extern int wclear(WINDOW *);
extern int clearok(WINDOW *, int);
extern int leaveok(WINDOW *, int);
extern int clrtoeol(void);
extern int clrtobot(void);
extern int bkgd(chtype);
extern chtype getbkgd(WINDOW *);
extern chtype inch(void);
extern chtype winch(WINDOW *);
extern chtype mvinch(int, int);
extern chtype mvwinch(WINDOW *, int, int);
extern int chgat(int, attr_t, short, const void *);
extern int wchgat(WINDOW *, int, attr_t, short, const void *);
extern int mvchgat(int, int, int, attr_t, short, const void *);
extern int mvwchgat(WINDOW *, int, int, int, attr_t, short, const void *);
extern int attr_get(attr_t *, short *, void *);
extern int wattr_get(WINDOW *, attr_t *, short *, void *);
extern int instr(char *);
extern int innstr(char *, int);
extern int winstr(WINDOW *, char *);
extern int winnstr(WINDOW *, char *, int);
extern int mvinnstr(int, int, char *, int);
extern int mvwinnstr(WINDOW *, int, int, char *, int);

/* Windows. */
extern WINDOW *newwin(int, int, int, int);
extern int delwin(WINDOW *);
extern int wmove(WINDOW *, int, int);
extern int waddch(WINDOW *, chtype);
extern int waddstr(WINDOW *, const char *);
extern int mvwaddstr(WINDOW *, int, int, const char *);
extern int wattron(WINDOW *, int);
extern int wattroff(WINDOW *, int);
extern int wattrset(WINDOW *, int);
extern int werase(WINDOW *);
extern int wclrtoeol(WINDOW *);
extern int wclrtobot(WINDOW *);
extern int winsch(WINDOW *, chtype);
extern int wdelch(WINDOW *);
extern int winsertln(WINDOW *);
extern int wdeleteln(WINDOW *);
extern int mvwin(WINDOW *, int, int);
extern int wbkgd(WINDOW *, chtype);
extern int wborder(WINDOW *, chtype, chtype, chtype, chtype, chtype, chtype, chtype, chtype);
extern int box(WINDOW *, chtype, chtype);
extern int whline(WINDOW *, chtype, int);
extern int wvline(WINDOW *, chtype, int);
extern int wnoutrefresh(WINDOW *);
extern int wrefresh(WINDOW *);
extern int scrollok(WINDOW *, int);
extern int setscrreg(int, int);
extern int wsetscrreg(WINDOW *, int, int);
extern int wscrl(WINDOW *, int);
extern int scroll(WINDOW *);
extern int scrl(int);

/* Wide characters (ncursesw): cchar_t builders + the add_wch / addwstr families. */
extern int setcchar(cchar_t *, const wchar_t *, const attr_t, short, const void *);
extern int getcchar(const cchar_t *, wchar_t *, attr_t *, short *, void *);
extern int add_wch(const cchar_t *);
extern int wadd_wch(WINDOW *, const cchar_t *);
extern int mvadd_wch(int, int, const cchar_t *);
extern int mvwadd_wch(WINDOW *, int, int, const cchar_t *);
extern int addwstr(const wchar_t *);
extern int addnwstr(const wchar_t *, int);
extern int waddwstr(WINDOW *, const wchar_t *);
extern int waddnwstr(WINDOW *, const wchar_t *, int);
extern int mvaddwstr(int, int, const wchar_t *);
extern int mvwaddwstr(WINDOW *, int, int, const wchar_t *);
/* Wide input: wget_wch returns KEY_CODE_YES for a function key, OK for a character (wint_t from
 * <wchar.h>). */
extern int get_wch(wint_t *);
extern int wget_wch(WINDOW *, wint_t *);
/* Wide read-back: extract the complex character at a cell. */
extern int in_wch(cchar_t *);
extern int win_wch(WINDOW *, cchar_t *);
extern int mvin_wch(int, int, cchar_t *);
extern int mvwin_wch(WINDOW *, int, int, cchar_t *);

/* Window overlay / copy. */
extern int overlay(const WINDOW *, WINDOW *);
extern int overwrite(const WINDOW *, WINDOW *);
extern int copywin(const WINDOW *, WINDOW *, int, int, int, int, int, int, int);

/* Pads. */
extern WINDOW *newpad(int, int);
extern int pnoutrefresh(WINDOW *, int, int, int, int, int, int);
extern int prefresh(WINDOW *, int, int, int, int, int, int);

#ifdef __cplusplus
}
#endif

#endif /* NCURSES_NATIVE_CURSES_H */
