/* No-endwin mvcur oracle for the whole-database cursor-motion coverage sweep.
 *
 * Reads "oldrow oldcol newrow newcol" quadruples from argv[1] and runs ncurses'
 * mvcur(3) for each, bracketing every call's byte output with a sentinel so the
 * sweep can split the stream into per-move segments.
 *
 * The sentinel is 0xff 0xfe 0xfd + a 4-byte big-endian index: those high bytes
 * never appear in cursor-motion output (cup/relative sequences are control +
 * low-ASCII; padding is NUL), so -- unlike a low-byte mark -- it cannot collide
 * with a legitimate column/row value byte (e.g. a non-CSI cup emitting \x01 for
 * column 1). The index lets the parser verify alignment. No endwin() is called,
 * so terminal teardown never contaminates the trailing segment.
 *
 * Build: cc mvcur_oracle.c -o mvcur_oracle -lncurses
 * Run under an 80x24 pty with TERMINFO pointing at the compiled database.
 */
#include <curses.h>
#include <term.h>
#include <stdio.h>
#include <stdint.h>

static void mark(uint32_t i) {
    unsigned char s[7] = {0xff, 0xfe, 0xfd,
                          (i >> 24) & 0xff, (i >> 16) & 0xff, (i >> 8) & 0xff, i & 0xff};
    fwrite(s, 1, 7, stdout);
}

int main(int argc, char **argv) {
    initscr();
    FILE *f = fopen(argv[1], "r");
    int a, b, c, d;
    uint32_t i = 0;
    while (fscanf(f, "%d %d %d %d", &a, &b, &c, &d) == 4) {
        mark(i++);
        mvcur(a, b, c, d);
    }
    mark(i);
    fflush(stdout);
    return 0; /* no endwin: no teardown to contaminate segments */
}
