/* No-endwin attribute oracle: clearok-forced first paint of an attribute scenario, each row a
   distinct attribute (bold/underline/reverse/standout/blink/dim, then normal). Exercises the SGR
   engine -- set_attributes (sgr) vs the individual mode caps -- plus the msgr move-reset. Sentinel
   via write(1) after refresh flushes. Mirrors examples/attr_db.rs.

   Build: cc attr_oracle.c -o attr_oracle -lncursesw
   Run under an 80x24 pty with TERMINFO pointing at the compiled database. */
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
    attrset(A_BOLD);      mvaddstr(1, 0, "bold");
    attrset(A_UNDERLINE); mvaddstr(2, 0, "undl");
    attrset(A_REVERSE);   mvaddstr(3, 0, "revs");
    attrset(A_STANDOUT);  mvaddstr(4, 0, "stnd");
    attrset(A_BLINK);     mvaddstr(5, 0, "blnk");
    attrset(A_DIM);       mvaddstr(6, 0, "dimm");
    attrset(A_NORMAL);    mvaddstr(7, 0, "norm");
    move(8, 0);
    refresh();
    mark();
    return 0;
}
