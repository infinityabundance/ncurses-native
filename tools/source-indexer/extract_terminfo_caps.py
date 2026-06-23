#!/usr/bin/env python3
"""Extract the canonical terminfo capability-name tables (boolean/number/string,
in index order) from ncurses' own public `boolnames`/`numnames`/`strnames`
arrays. These define the index each cap occupies in a compiled terminfo entry
and in tigetflag/tigetnum/tigetstr, so they are the authoritative order.

Output: docs-src/models/terminfo-caps.json (consumed by `cargo xtask gen`, which
codegens src/terminfo/caps.rs). This is the provenance step, analogous to the
clang C-API extractor; it reads ncurses, not ncurses C source.
"""
import json, subprocess, sys, tempfile, os, hashlib

C = r"""
#include <curses.h>
#include <term.h>
#include <stdio.h>
extern const char *const boolcodes[], *const numcodes[], *const strcodes[];
static void arr(const char *name, const char *const *a){
  printf("  \"%s\": [", name);
  for(int i=0;a[i];i++) printf("%s\"%s\"", i?",":"", a[i]);
  printf("]");
}
int main(void){
  printf("{\n");
  arr("bool", boolnames); printf(",\n");
  arr("num", numnames); printf(",\n");
  arr("str", strnames); printf(",\n");
  arr("bool_codes", boolcodes); printf(",\n");
  arr("num_codes", numcodes); printf(",\n");
  arr("str_codes", strcodes); printf("\n");
  printf("}\n");
  return 0;
}
"""


def ncurses_version():
    r = subprocess.run(["infocmp", "-V"], capture_output=True, text=True)
    return (r.stdout or r.stderr).strip()


def main():
    d = tempfile.mkdtemp()
    cpath = os.path.join(d, "capnames.c")
    bpath = os.path.join(d, "capnames")
    open(cpath, "w").write(C)
    cc = subprocess.run(["cc", cpath, "-o", bpath, "-lncursesw"],
                        capture_output=True, text=True)
    if cc.returncode != 0:
        sys.stderr.write(cc.stderr)
        sys.exit(2)
    out = subprocess.run([bpath], capture_output=True, text=True).stdout
    tables = json.loads(out)
    model = {
        "provenance": {
            "source": ncurses_version(),
            "method": "ncurses public boolnames/numnames/strnames (terminfo) and boolcodes/numcodes/strcodes (termcap) arrays, index order",
        },
        "bool": tables["bool"],
        "num": tables["num"],
        "str": tables["str"],
        "bool_codes": tables["bool_codes"],
        "num_codes": tables["num_codes"],
        "str_codes": tables["str_codes"],
        "counts": {k: len(tables[k]) for k in ("bool", "num", "str")},
    }
    json.dump(model, sys.stdout, indent=2)
    print()


if __name__ == "__main__":
    main()
