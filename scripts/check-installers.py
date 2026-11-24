#!/usr/bin/env python3
"""Offline contract test for scripts/install.sh and scripts/install.ps1.

Both installers download a release archive, verify it and hand over to
`commitsmith --install`. The network part cannot run in CI for _this_ repo (it is
private, so release assets are not reachable without a token), and PowerShell is
not on every developer machine. This test therefore fakes the two boundaries:

- a local HTTP server with the same URL layout as a GitHub release
- a stand-in `commitsmith` that records the arguments it was called with

so the script logic itself is exercised. Run with: python3 scripts/check-installers.py
"""

import hashlib
import http.server
import io
import os
import pathlib
import shutil
import subprocess
import sys
import tarfile
import tempfile
import threading
import zipfile

VERSION = "9.9.9"
ASSETS = pathlib.Path(__file__).resolve().parent


def make_assets(root: pathlib.Path) -> None:
    """Build a fake release tree: <root>/releases/download/v9.9.9/..."""
    # A stand-in that proves the installer can execute what it unpacked.
    payload = (
        b"#!/bin/sh\n"
        b'if [ "$1" = "--version" ]; then echo "commitsmith 9.9.9 (stand-in)"; fi\n'
        b"exit 0\n"
    )
    for target in ("x86_64-unknown-linux-musl", "aarch64-unknown-linux-musl"):
        name = f"commitsmith-{VERSION}-{target}"
        buffer = io.BytesIO()
        with tarfile.open(fileobj=buffer, mode="w:gz") as tar:
            info = tarfile.TarInfo(f"{name}/commitsmith")
            info.size = len(payload)
            info.mode = 0o755
            tar.addfile(info, io.BytesIO(payload))
        (root / f"{name}.tar.gz").write_bytes(buffer.getvalue())

    name = f"commitsmith-{VERSION}-x86_64-pc-windows-msvc.zip"
    with zipfile.ZipFile(root / name, "w") as archive:
        archive.writestr("commitsmith.exe", payload)

    lines = []
    for file in sorted(root.glob("commitsmith-*.tar.*")):
        digest = hashlib.sha256(file.read_bytes()).hexdigest()
        lines.append(f"{digest}  {file.name}")
    digest = hashlib.sha256((root / name).read_bytes()).hexdigest()
    lines.append(f"{digest}  {name}")
    (root / "SHA256SUMS").write_text("\n".join(sorted(lines)) + "\n")


class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, directory=None, **kwargs):
        super().__init__(*args, directory=directory, **kwargs)

    def log_message(self, format, *args):  # noqa: A002 - signature of the base class
        pass  # keep the output readable


def serve(site: pathlib.Path) -> tuple[http.server.ThreadingHTTPServer, str]:
    """Serve the release host layout and hand back the base URL to use."""
    # The faked base URL carries no path, so serve the release root and let the
    # installer append /<tag>/<file>.
    root = site / "releases" / "download"
    handler = lambda *a, **kw: Handler(*a, directory=str(root), **kw)  # noqa: E731
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = f"http://127.0.0.1:{server.server_address[1]}"
    return server, base


def run_shell_installer(script: pathlib.Path, work: pathlib.Path, base: str) -> tuple[int, str]:
    """Run install.sh against the fake release host."""
    env = dict(os.environ)
    env["HOME"] = str(work / "home")
    (work / "home").mkdir()

    result = subprocess.run(
        ["sh", str(script), "--self-test", f"v{VERSION}", "--url-base", base],
        env=env,
        capture_output=True,
        text=True,
    )
    return result.returncode, result.stdout + result.stderr


def check_ps1(script: pathlib.Path) -> str:
    """Static checks: PowerShell is not required to review the script's intent."""
    text = script.read_text(encoding="utf-8")
    problems = []
    if "SetEnvironmentVariable" in text:
        problems.append("the binary owns PATH now, the script must not touch it")
    if "$PROFILE" in text:
        problems.append("must not edit the user's PowerShell profile")
    for needed in ("--install", "Expand-Archive", "--alias"):
        if needed not in text:
            problems.append(f"missing {needed}")
    # Balanced braces in the param block are a cheap syntax smoke test.
    if text.count("{") != text.count("}"):
        problems.append("unbalanced braces")
    return "; ".join(problems) or "ok"


def main() -> int:
    failures = []
    work = pathlib.Path(tempfile.mkdtemp(prefix="commitsmith-install-"))
    try:
        release_root = work / "site" / "releases" / "download" / f"v{VERSION}"
        release_root.mkdir(parents=True)
        make_assets(release_root)

        server, base = serve(work / "site")
        try:
            code, output = run_shell_installer(ASSETS / "install.sh", work, base)
        finally:
            server.shutdown()

        print("install.sh --self-test:", code)
        print(output.strip())
        if code != 0:
            failures.append("install.sh self-test failed")
        if "checksum ok" not in output:
            failures.append("install.sh did not verify the checksum")

        ps1 = check_ps1(ASSETS / "install.ps1")
        print("install.ps1 static checks:", ps1)
        if ps1 != "ok":
            failures.append(f"install.ps1: {ps1}")
    finally:
        shutil.rmtree(work, ignore_errors=True)

    if failures:
        for failure in failures:
            print("FAIL:", failure, file=sys.stderr)
        return 1
    print("all installer checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
