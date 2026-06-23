#!/usr/bin/env python3
"""Whole-database byte-parity coverage sweep for the two terminal-general engines.

For every terminal in a compiled terminfo database this compares the crate's
output to *real ncurses* under an 80x24 pty:

  * mvcur  -- a sampled (from -> to) move grid (examples/mvcur_db.rs vs the
              mvcur_oracle binary);
  * doupdate -- the fixed first-paint scenario (examples/doupdate_db.rs vs the
              doupdate_oracle binary);
  * incremental -- a scene-A -> scene-B diff exercising TransformLine / clr_eol /
              rep / ich-dch (examples/doupdate_db2.rs vs the incremental_oracle);
  * attr -- an attribute scenario exercising the SGR engine (set_attributes vs
              individual mode caps, padding, msgr) (examples/attr_db.rs vs attr_oracle);
  * color -- a two-pair color scenario exercising _nc_do_color (setaf/setab or
              setf/setb, orig_pair, color-init, color+no-bce blank-fill)
              (examples/color_db.rs vs color_oracle).

It is the reproducible form of the breadth numbers quoted in README.md /
docs/gap-ledger.md. The terminfo database is ephemeral on this host (tic-compiled
to a temp dir), so this sweep is tooling, not a committed court; the specific
systematic behaviours it found are pinned offline by the committed fixture tests
(tests/mvcur_terminfo.rs, tests/doupdate_terminfo.rs).

Usage:
    python3 tools/db-coverage/sweep.py <tinfo_dir> [mvcur|doupdate|incremental|attr|color|all] [limit]

<tinfo_dir> is a directory tic has compiled a terminfo database into (the same
TERMINFO both the crate and real ncurses resolve). The oracle binaries are built
on first use next to this script.
"""
import sys, os, glob, subprocess, tempfile, struct

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
sys.path.insert(0, os.path.join(ROOT, "tools", "oracle-runner"))
from pty_capture import capture  # noqa: E402

ROWS = [0, 2, 11, 23]
COLS = [0, 1, 5, 40, 79]
QUADS = [(fy, fx, ty, tx) for fy in ROWS for fx in COLS for ty in ROWS for tx in COLS]
SENT = b"\xff\xfe\xfd"


def build(name, libs):
    src = os.path.join(HERE, name + ".c")
    out = os.path.join(HERE, name)
    if not os.path.exists(out) or os.path.getmtime(src) > os.path.getmtime(out):
        r = subprocess.run(["cc", src, "-o", out] + libs, capture_output=True)
        if r.returncode != 0:
            raise SystemExit(f"build {name} failed:\n{r.stderr.decode()}")
    return out


def terminals(tinfo):
    return sorted(set(
        os.path.basename(f)
        for f in glob.glob(os.path.join(tinfo, "*", "*"))
        if "+" not in os.path.basename(f)
    ))


def crate(example, tinfo, names):
    tl = tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False)
    tl.write(" ".join(names))
    tl.close()
    out = subprocess.run(
        ["cargo", "run", "--quiet", "--example", example, "--", tinfo, tl.name],
        capture_output=True, cwd=ROOT,
    ).stdout.decode("latin-1")
    os.unlink(tl.name)
    res, cur = {}, None
    for line in out.splitlines():
        if line.startswith("T "):
            p = line.split()
            cur = p[1]
            res[cur] = None if len(p) > 2 else ([] if example == "mvcur_db" else "")
        elif cur is not None and res.get(cur) is not None and "=" in line:
            k, _, v = line.partition("=")
            res[cur] = res.get(cur) or {}
            res[cur][k] = v
        elif cur is not None and res.get(cur) == "" and line.strip():
            res[cur] = line.strip()
    return res


def mvcur_oracle(binary, tinfo, term, qf):
    out, _e, _c = capture([binary, qf], term=term, env_extra={"TERMINFO": tinfo})
    marks, p = [], out.find(SENT)
    while p != -1:
        marks.append((p, struct.unpack(">I", out[p + 3:p + 7])[0]))
        p = out.find(SENT, p + 7)
    if len(marks) < len(QUADS) + 1:
        return None
    segs = {}
    for j in range(len(marks) - 1):
        pp, idx = marks[j]
        if idx < len(QUADS):
            q = QUADS[idx]
            segs[f"{q[0]},{q[1]},{q[2]},{q[3]}"] = out[pp + 7:marks[j + 1][0]]
    return segs


def doupdate_oracle(binary, tinfo, term):
    out, _e, _c = capture([binary], term=term, env_extra={"TERMINFO": tinfo})
    a = out.find(SENT)
    b = out.find(SENT, a + 3) if a >= 0 else -1
    return out[a + 3:b] if (a >= 0 and b >= 0) else None


def run(tinfo, which, limit):
    names = terminals(tinfo)
    if limit:
        names = names[:limit]
    results = {}
    if which in ("mvcur", "all"):
        binary = build("mvcur_oracle", ["-lncurses"])
        cr = crate("mvcur_db", tinfo, names)
        qf = tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False)
        for q in QUADS:
            qf.write("%d %d %d %d\n" % q)
        qf.close()
        ok = bad = 0
        fails = []
        for t in names:
            if not cr.get(t):
                continue
            orc = mvcur_oracle(binary, tinfo, t, qf.name)
            if orc is None:
                continue
            if all(bytes.fromhex(cr[t][k]) == orc.get(k, b"") for k in cr[t]):
                ok += 1
            else:
                bad += 1
                fails.append(t)
        os.unlink(qf.name)
        results["mvcur"] = (ok, bad, fails)
    # doupdate first-paint, incremental, and attribute all share the single-segment oracle shape.
    for engine, example, oracle_src in (
        ("doupdate", "doupdate_db", "doupdate_oracle"),
        ("incremental", "doupdate_db2", "incremental_oracle"),
        ("attr", "attr_db", "attr_oracle"),
        ("color", "color_db", "color_oracle"),
    ):
        if which not in (engine, "all"):
            continue
        binary = build(oracle_src, ["-lncursesw"])
        cr = crate(example, tinfo, names)
        ok = bad = 0
        fails = []
        for t in names:
            if not cr.get(t):
                continue
            orc = doupdate_oracle(binary, tinfo, t)
            if orc is None:
                continue
            if bytes.fromhex(cr[t]) == orc:
                ok += 1
            else:
                bad += 1
                fails.append(t)
        results[engine] = (ok, bad, fails)
    for engine, (ok, bad, fails) in results.items():
        tot = ok + bad
        pct = 100.0 * ok / tot if tot else 0.0
        print(f"{engine}: {ok}/{tot} byte-exact ({pct:.2f}%)  fails={fails}")


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print(__doc__)
        raise SystemExit(2)
    tinfo = sys.argv[1]
    which = sys.argv[2] if len(sys.argv) > 2 else "all"
    limit = int(sys.argv[3]) if len(sys.argv) > 3 else 0
    run(tinfo, which, limit)
