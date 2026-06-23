/* No-endwin doupdate oracle for the whole-database first-paint coverage sweep.
 *
 * refresh() flushes ncurses' output buffer to fd 1; the sentinel is then written
 * with raw write(1,...) so it is correctly ordered with ncurses output (a stdio
 * mark would reorder, since ncurses bypasses stdio). The first refresh emits the
 * smcup/clear framing of a blank screen; clearok(curscr, TRUE) forces the second
 * refresh to emit a full clear + repaint, which matches the crate's first
 * doupdate (clear + paint of the non-blank cells). The bytes between the two
 * sentinels are exactly that clear+paint. No endwin() is called.
 *
 * Scenario mirrors examples/doupdate_db.rs: "hello world" at (2,5), "abc" at
 * (5,0), cursor parked at (5,3).
 *
 * Build: cc doupdate_oracle.c -o doupdate_oracle -lncursesw
 * Run under an 80x24 pty with TERMINFO pointing at the compiled database.
 */
#include <curses.h>
#include <unistd.h>

static void mark(void) {
    unsigned char s[3] = {0xff, 0xfe, 0xfd};
    write(1, s, 3);
}

int main(void) {
    initscr();
    refresh();
    mark();
    clearok(curscr, TRUE);
    mvaddstr(2, 5, "hello world");
    mvaddstr(5, 0, "abc");
    move(5, 3);
    refresh();
    mark();
    return 0; /* no endwin */
}
