#!/usr/bin/env python3
"""Extract the ncurses public C API function inventory from installed headers
via the clang AST. Authoritative C inventory for the port-parity matrices.

Output: a normalized JSON model (stable sort) plus provenance (header sha256,
clang version). This script is the reproducible provenance step; its output is
committed and consumed by `cargo xtask gen`. It does NOT read ncurses C source
(there is none in this repo) -- it reads the public declarations the headers
expose, which is the surface a curses-compatible library must implement.
"""
import json, hashlib, subprocess, sys, os

HEADERS = ["/usr/include/curses.h", "/usr/include/term.h"]

def sha256(p):
    return hashlib.sha256(open(p, "rb").read()).hexdigest()

def clang_version():
    return subprocess.run(["clang", "--version"], capture_output=True, text=True).stdout.splitlines()[0].strip()

def _file_of(node, state):
    """clang -ast-dump=json only emits loc.file when it changes from the prior
    node (source order). Track the running 'current file' so every decl is
    attributed to the header it is actually declared in."""
    for key in ("loc", "range"):
        sub = node.get(key)
        if isinstance(sub, dict):
            f = sub.get("file")
            if not f and isinstance(sub.get("begin"), dict):
                f = sub["begin"].get("file")
            if f:
                state["cur"] = f
    return state["cur"]

def walk(node, header, out, state):
    if isinstance(node, dict):
        f = _file_of(node, state)
        if node.get("kind") == "FunctionDecl":
            name = node.get("name")
            if name and f and os.path.realpath(f) == os.path.realpath(header):
                out.add(name)
        for v in node.get("inner", []):
            walk(v, header, out, state)

def main():
    funcs = {}
    prov = []
    for h in HEADERS:
        if not os.path.exists(h):
            continue
        names = set()
        ast = subprocess.run(
            ["clang", "-Xclang", "-ast-dump=json", "-fsyntax-only", h],
            capture_output=True, text=True)
        try:
            root = json.loads(ast.stdout)
        except json.JSONDecodeError:
            print("clang json parse failed for", h, file=sys.stderr); sys.exit(2)
        walk(root, h, names, {"cur": None})
        for n in names:
            funcs[n] = os.path.basename(h)
        prov.append({"header": h, "sha256": sha256(h), "functions": len(names)})
    model = {
        "provenance": {
            "tool": clang_version(),
            "method": "clang -Xclang -ast-dump=json on installed ncursesw headers",
            "headers": prov,
            "note": "Public API FunctionDecls declared directly in the named header. "
                    "Preprocessor macro aliases (e.g. addch -> waddch) are not FunctionDecls "
                    "and are not counted here; the real w*/*_sp functions are.",
        },
        "functions": sorted(funcs.keys()),
        "function_header": funcs,
        "count": len(funcs),
    }
    json.dump(model, sys.stdout, indent=2, sort_keys=True)
    print()

if __name__ == "__main__":
    main()
