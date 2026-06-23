#!/usr/bin/env python3
"""Extract the key tables ncurses uses for keyname/key_defined/has_key:

  * code_names: KEY_* code -> name (from `keyname` over the key-code range), and
  * cap_codes:  terminfo key-cap name -> KEY_* code (from `key_defined` applied to
    each present key cap, unioned across several terminals -- the mapping is
    terminal-independent).

Output: docs-src/models/keys.json. Provenance step (reads ncurses, not source).
"""
import json, subprocess, sys, tempfile, os

C = r"""
#include <curses.h>
#include <term.h>
#include <stdio.h>
#include <string.h>
extern const char *const strnames[];
int main(void){
  initscr(); keypad(stdscr, TRUE);
  /* code -> name over the key-code range */
  for(int code=256; code<=511; code++){
    const char *n = keyname(code);
    /* Only the canonical KEY_* names; dynamic extended-key names (e.g. kDC5) get
       run/terminal-specific codes and are not stable. */
    if(n && !strncmp(n,"KEY_",4)) fprintf(stderr,"N\t%d\t%s\n", code, n);
  }
  /* cap -> code: for each present *key* cap (k-prefixed), what key does it define? */
  for(int i=0; strnames[i]; i++){
    if(strnames[i][0] != 'k') continue;
    char *v = tigetstr((char*)strnames[i]);
    if(v && v != (char*)-1){
      int k = key_defined(v);
      if(k > 0) fprintf(stderr,"C\t%s\t%d\n", strnames[i], k);
    }
  }
  endwin();
  return 0;
}
"""

TERMS = ["xterm", "xterm-256color", "linux", "screen", "tmux", "vt100", "ansi", "rxvt"]


def ncurses_version():
    r = subprocess.run(["infocmp", "-V"], capture_output=True, text=True)
    return (r.stdout or r.stderr).strip()


def run_under_pty(binary, term):
    import pty, struct, fcntl, termios, select
    err_r, err_w = os.pipe()
    pid, fd = pty.fork()
    if pid == 0:
        os.close(err_r); os.dup2(err_w, 2); os.close(err_w)
        os.environ["TERM"] = term
        os.environ["TERMINFO"] = "/usr/share/terminfo"
        fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
        os.execvp(binary, [binary])
    os.close(err_w)
    err = b""
    fds = [fd, err_r]
    while fds:
        r, _, _ = select.select(fds, [], [])
        for s in r:
            try:
                d = os.read(s, 65536)
            except OSError:
                d = b""
            if not d:
                fds.remove(s); continue
            if s == err_r:
                err += d
    os.waitpid(pid, 0)
    return err.decode("latin-1")


def main():
    d = tempfile.mkdtemp()
    cpath = os.path.join(d, "k.c"); bpath = os.path.join(d, "k")
    open(cpath, "w").write(C)
    cc = subprocess.run(["cc", cpath, "-o", bpath, "-lncursesw"], capture_output=True, text=True)
    if cc.returncode != 0:
        sys.stderr.write(cc.stderr); sys.exit(2)
    code_names = {}
    cap_codes = {}
    for term in TERMS:
        f = os.path.join("/usr/share/terminfo", term[0], term)
        if not os.path.exists(f):
            continue
        for line in run_under_pty(bpath, term).splitlines():
            p = line.split("\t")
            if p[0] == "N" and len(p) == 3:
                code_names.setdefault(int(p[1]), p[2])
            elif p[0] == "C" and len(p) == 3:
                cap_codes.setdefault(p[1], int(p[2]))
    model = {
        "provenance": {"source": ncurses_version(),
                       "method": "ncurses keyname (code->name) and key_defined (cap->code) across terminals"},
        "code_names": {str(k): v for k, v in sorted(code_names.items())},
        "cap_codes": dict(sorted(cap_codes.items())),
        "counts": {"code_names": len(code_names), "cap_codes": len(cap_codes)},
    }
    json.dump(model, sys.stdout, indent=2)
    print()


if __name__ == "__main__":
    main()
