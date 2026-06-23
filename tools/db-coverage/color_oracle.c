/* No-endwin color oracle: clearok-forced first paint of a two-pair color scenario (red-on-blue,
   green-on-black, default). Exercises _nc_do_color (setaf/setab or setf/setb with toggled_colors,
   orig_pair), the color-init before the clear, and the color+no-bce blank-fill. Mirrors
   examples/color_db.rs (which does blank refresh -> clearok -> paint).

   Build: cc color_oracle.c -o color_oracle -lncursesw
   Run under an 80x24 pty with TERMINFO pointing at the compiled database. */
#include <curses.h>
#include <unistd.h>

static void mark(void) {
    unsigned char s[3] = {0xff, 0xfe, 0xfd};
    write(1, s, 3);
}

int main(void) {
    initscr();
    start_color();
    init_pair(1, COLOR_RED, COLOR_BLUE);
    init_pair(2, COLOR_GREEN, COLOR_BLACK);
    refresh();
    mark();
    clearok(curscr, TRUE);
    attrset(COLOR_PAIR(1)); mvaddstr(1, 0, "red");
    attrset(COLOR_PAIR(2)); mvaddstr(2, 0, "grn");
    attrset(A_NORMAL);      mvaddstr(3, 0, "def");
    move(4, 0);
    refresh();
    mark();
    return 0;
}
