#!/usr/bin/env python3
"""Drive the composer through a pty and read the rendered screen back.

Used as a smoke test: it starts the binary in a temporary repository, types a
name into the first-start dialog, fills the subject and submits with Ctrl+S,
then reports what the terminal actually showed.
"""

import fcntl
import os
import pty
import re
import select
import shutil
import struct
import subprocess
import sys
import tempfile
import termios
import time

BIN = sys.argv[1] if len(sys.argv) > 1 else "target/release/commitsmith"
# A normal Windows console and a normal terminal both fit this.
COLS, ROWS = 100, 30
ANSI = re.compile(rb"\x1b\[[0-9;?]*[a-zA-Z]|\x1b\][^\x07]*\x07")


def run_session(script, commit=False):
    cwd = tempfile.mkdtemp(prefix="commitsmith-smoke-")
    subprocess.run(["git", "init", "-q", "-b", "main"], cwd=cwd, check=True)
    subprocess.run(["git", "config", "user.email", "smoke@example.com"], cwd=cwd, check=True)
    subprocess.run(["git", "config", "user.name", "Smoke Test"], cwd=cwd, check=True)
    with open(os.path.join(cwd, "a.txt"), "w") as handle:
        handle.write("hello\n")
    subprocess.run(["git", "add", "a.txt"], cwd=cwd, check=True)

    config_home = tempfile.mkdtemp(prefix="commitsmith-cfg-")
    env = dict(os.environ, XDG_CONFIG_HOME=config_home, TERM="xterm-256color")

    primary, secondary = pty.openpty()
    # Without a size the terminal is 0x0, the composer falls back to its compact
    # layout and the full form is never drawn. That gap let a startup panic slip
    # into 0.3.1, so the size is part of the test.
    fcntl.ioctl(secondary, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))
    argv = [os.path.abspath(BIN)]
    if commit:
        argv.append("--commit")
    process = subprocess.Popen(
        argv,
        cwd=cwd,
        env=env,
        stdin=secondary,
        stdout=secondary,
        stderr=secondary,
        start_new_session=True,
    )
    os.close(secondary)

    chunks = []

    def pump(seconds):
        end = time.time() + seconds
        while time.time() < end:
            ready, _, _ = select.select([primary], [], [], 0.05)
            if ready:
                try:
                    chunk = os.read(primary, 65536)
                except OSError:
                    return
                if not chunk:
                    return
                chunks.append(chunk)

    pump(1.2)
    for step in script:
        os.write(primary, step.encode())
        pump(0.7)
    pump(2.0)

    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
    os.close(primary)

    screen = ANSI.sub(b"", b"".join(chunks)).decode("utf-8", "replace")
    head = subprocess.run(
        ["git", "log", "--format=%s"], cwd=cwd, capture_output=True, text=True
    ).stdout.strip()
    repo = cwd
    config_path = os.path.join(config_home, "commitsmith", "config.toml")
    config = ""
    if os.path.exists(config_path):
        with open(config_path) as handle:
            config = handle.read()
    shutil.rmtree(config_home, ignore_errors=True)
    return screen, head, config, cwd


def main():
    # --commit: the same path a user takes when the message goes straight to git.
    screen, head, config, repo = run_session(
        ["Marc Hinte", "\r", "\t", "\t", "add greeting file", "\x13"], commit=True
    )
    failures = []
    if 'user = "Marc Hinte"' not in config:
        failures.append("the first-start dialog did not write the name")
    if not head.startswith("feat: add greeting file"):
        failures.append(f"commit message wrong: {head!r}")
    # git echoes the message it stored; the handle has to be part of it.
    stored = subprocess.run(["git", "log", "-1", "--format=%B"], cwd=repo, capture_output=True, text=True).stdout
    if "Co-authored-by: himar" not in stored:
        failures.append(f"the handle did not reach the commit message: {stored!r}")
    # Ratatui redraws in place, so the dialog text stays in the byte stream of
    # earlier frames. Only the tail matters: if the dialog were still showing, it
    # would be in the last frame.
    tail = screen[-2000:]
    if "What is your full name" in tail:
        failures.append("the setup dialog is still on the final screen")
    if "panicked at" in screen:
        panic = [line for line in screen.splitlines() if "panicked at" in line]
        failures.append("the program panicked: " + " / ".join(panic))
    print("commit:", head or "<none>")
    print("config:", config.strip().splitlines()[-2:] or "<none>")
    shutil.rmtree(repo, ignore_errors=True)
    for failure in failures:
        print("FAIL:", failure, file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
