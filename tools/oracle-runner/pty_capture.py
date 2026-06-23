#!/usr/bin/env python3
"""Capture the exact terminal byte stream a command writes to a pty.

ncurses only emits its optimized output stream when stdout is a terminal of a
known size (it reads TIOCGWINSZ, not $LINES/$COLUMNS). This harness allocates a
pty, forces an 80x24 window, exports a controlled environment, runs the command
with stdout connected to the pty slave (stderr to a pipe so diagnostics never
pollute the captured terminal bytes), and returns the raw bytes plus exit code.

It is the byte-parity capture method named in every oracle receipt.
"""
import os, pty, struct, fcntl, termios, select, sys


def capture(argv, term="xterm", cols=80, rows=24, lang="C.UTF-8", env_extra=None):
    """Run `argv` under an 80x24 pty; return (stdout_bytes, stderr_bytes, exit_code)."""
    err_r, err_w = os.pipe()
    pid, fd = pty.fork()
    if pid == 0:  # child
        os.close(err_r)
        os.dup2(err_w, 2)  # stderr -> pipe (kept off the terminal stream)
        os.close(err_w)
        fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        env = dict(os.environ)
        env["TERM"] = term
        env["LANG"] = lang
        env["LC_ALL"] = lang
        env.pop("COLUMNS", None)
        env.pop("LINES", None)
        if env_extra:
            env.update(env_extra)
        try:
            os.execvpe(argv[0], argv, env)
        except OSError as e:
            sys.stderr.write(f"exec failed: {e}\n")
            os._exit(127)
    # parent
    os.close(err_w)
    out = bytearray()
    err = bytearray()
    fds = [fd, err_r]
    while fds:
        r, _, _ = select.select(fds, [], [])
        for s in r:
            try:
                d = os.read(s, 65536)
            except OSError:
                d = b""
            if not d:
                fds.remove(s)
                continue
            (out if s == fd else err).extend(d)
    _, status = os.waitpid(pid, 0)
    os.close(err_r)
    os.close(fd)  # close the pty master, else a many-terminal sweep leaks fds past the select() limit
    code = os.waitstatus_to_exitcode(status)
    return bytes(out), bytes(err), code


if __name__ == "__main__":
    o, e, c = capture(sys.argv[1:])
    sys.stderr.write(f"exit={c} stdout={len(o)}B stderr={len(e)}B\n")
    sys.stdout.buffer.write(o)
