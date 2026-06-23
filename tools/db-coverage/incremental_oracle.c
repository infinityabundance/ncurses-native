/* No-endwin *incremental* doupdate oracle: paint scene A, mark, diff to scene B, mark.
 *
 * erase() blanks newscr without output; the three mvaddstr build scene B; the second refresh emits
 * the A->B diff (TransformLine / clr_eol / rep / ich-dch). The sentinel is written via raw
 * write(1,...) after refresh() flushes ncurses' buffer (a stdio mark would reorder, since ncurses
 * bypasses stdio). The bytes between the two sentinels are exactly the incremental diff. No endwin().
 *
 * Scene A mirrors examples/doupdate_db2.rs: "hello world"@(2,5), "abc"@(5,0); scene B shortens
 * "hello world"->"HELLO" (trailing clear), extends "abc"->"abcdef", and adds "xyz"@(10,20).
 *
 * Build: cc incremental_oracle.c -o incremental_oracle -lncursesw
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
    mvaddstr(2, 5, "hello world");
    mvaddstr(5, 0, "abc");
    move(5, 3);
    refresh();                 /* scene A */
    mark();
    erase();
    mvaddstr(2, 5, "HELLO");
    mvaddstr(5, 0, "abcdef");
    mvaddstr(10, 20, "xyz");
    move(11, 0);
    refresh();                 /* diff to scene B */
    mark();
    return 0;
}
