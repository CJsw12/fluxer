"""One-command dev loop: test -> build -> relaunch (probe on) -> wait Online -> optional scene check.

    python -I tools/parity_lab/cycle.py [--release] [--worker] [--skip-tests] [--force]
        [--check OFFICIAL.dom.json OFFICIAL.png fluxer://... OUT_DIR]
    python -I tools/parity_lab/cycle.py --selftest
    python -I tools/parity_lab/cycle.py --help

Default builds the fast incremental debug binary (target/fc-root/debug/fluxer_client.exe);
--release packages dist/ (add --worker when voice_worker changed). The debug binary has no
voice_worker next to it, so voice/screen-share scenes need --release. The client runs detached
with FLUXER_PARITY_PROBE=1 and the test profile from env PARITY_PROFILE (LOCALAPPDATA override).

--release skips test/build/package when dist/fluxer_client/BUILD_STAMP.json matches the current
git state (HEAD + dirty/untracked tree) and dist/fluxer_client/fluxer_client.exe exists; --force
rebuilds anyway. --selftest only exercises argparse and the stamp match logic.
"""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
import subprocess
import sys
import time
from contextlib import redirect_stderr, redirect_stdout
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))  # `python -I` drops the script dir
import rust_probe as rp  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
TARGET = REPO / "target" / "fc-root"
RESTART_TIMEOUT = 240
PWSH = Path(os.environ.get("LOCALAPPDATA", "")) / "Programs" / "PowerShell" / "7" / "pwsh.exe"
DIST = REPO / "dist" / "fluxer_client"
DIST_EXE = DIST / "fluxer_client.exe"
STAMP = DIST / "BUILD_STAMP.json"
STAMP_SKIP_LINE = "cycle: dist matches HEAD (stamp), skipping build/package"


def run(cmd: list[str], cwd: Path = REPO, env: dict | None = None, tail: int = 6) -> int:
    t = time.time()
    r = subprocess.run(cmd, cwd=cwd, env=env, capture_output=True, text=True, encoding="utf-8", errors="replace")
    lines = [l for l in (r.stdout + r.stderr).splitlines() if l.strip()]
    keep = [l for l in lines if any(k in l for k in ("test result", "FAILED", "panicked", "error", "warning: unused", "SHA256"))]
    print(f"[{time.time() - t:5.0f}s] {' '.join(cmd[:4])} -> {r.returncode}")
    for l in (keep or lines)[-tail:]:
        print("   ", l[:200])
    return r.returncode


def parse_args(argv: list[str]) -> argparse.Namespace:
    p = argparse.ArgumentParser(
        prog="cycle.py",
        description="Test, build and relaunch the Fluxer parity client (probe on).",
        epilog=(
            "Debug build: target/fc-root/debug/fluxer_client.exe. --release packages dist/ and is "
            "skipped when dist/fluxer_client/BUILD_STAMP.json matches the current git state."
        ),
    )
    p.add_argument("--release", action="store_true", help="package dist/ instead of an incremental debug build")
    p.add_argument("--worker", action="store_true", help="include fluxer_voice_worker in the release package")
    p.add_argument("--skip-tests", action="store_true", help="skip cargo test before building")
    p.add_argument("--force", action="store_true", help="rebuild even when the release build stamp matches")
    p.add_argument("--selftest", action="store_true", help="unit-test argparse and the stamp match logic, then exit")
    p.add_argument(
        "--check",
        nargs=4,
        default=None,
        metavar=("DOM_JSON", "PNG", "DEEPLINK", "OUT_DIR"),
        help="after relaunch, run check_scene.py with these four arguments",
    )
    return p.parse_args(argv)


def _git(*args: str) -> bytes:
    try:
        r = subprocess.run(["git", *args], cwd=REPO, capture_output=True)
    except OSError:
        return b""
    return r.stdout if r.returncode == 0 else b""


def git_head() -> str:
    return _git("rev-parse", "HEAD").decode("utf-8", "replace").strip()


def git_tree() -> str:
    """sha256 over `git diff HEAD` plus the untracked list under fluxer_client/."""
    h = hashlib.sha256()
    h.update(_git("diff", "HEAD"))
    h.update(b"\0")
    untracked = _git("ls-files", "--others", "--exclude-standard", "fluxer_client")
    h.update(untracked)
    for name in untracked.decode("utf-8", "replace").splitlines():
        try:
            h.update(b"\0" + (REPO / name).read_bytes())
        except OSError:
            h.update(b"\0<unreadable>")
    return h.hexdigest()


def current_state() -> dict:
    return {"head": git_head(), "tree": git_tree()}


def read_stamp() -> dict | None:
    try:
        data = json.loads(STAMP.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None
    return data if isinstance(data, dict) else None


def write_stamp(state: dict, worker: bool) -> None:
    DIST.mkdir(parents=True, exist_ok=True)
    data = {
        "head": state.get("head", ""),
        "tree": state.get("tree", ""),
        "worker": bool(worker),
        "created": datetime.now(timezone.utc).isoformat(timespec="seconds"),
    }
    STAMP.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")


def stamp_matches(stamp: dict | None, current: dict, worker: bool = False) -> bool:
    """Pure check: does a build stamp describe the current HEAD/tree (and satisfy --worker)?"""
    if not isinstance(stamp, dict) or not isinstance(current, dict):
        return False
    if not stamp.get("head") or not stamp.get("tree"):
        return False
    if stamp.get("head") != current.get("head") or stamp.get("tree") != current.get("tree"):
        return False
    return not (worker and not stamp.get("worker"))


def run_cycle(opts: argparse.Namespace) -> int:
    env = dict(os.environ, CARGO_TARGET_DIR=str(TARGET))
    do_build = True
    # Snapshot the source state before building so the stamp describes the
    # sources the binary was built from, even if files change mid-build.
    state = current_state() if opts.release else {}
    if opts.release and not opts.force and DIST_EXE.is_file():
        if stamp_matches(read_stamp(), state, opts.worker):
            do_build = False
            print(STAMP_SKIP_LINE)
    if do_build and not opts.skip_tests:
        if run(["cargo", "test", "-p", "fluxer_client_app", "-j", "4", "--", "--test-threads=2"], REPO / "fluxer_client", env):
            return 1
    for im in ("fluxer_client.exe", "fluxer_voice_worker.exe"):
        subprocess.run(["taskkill", "/F", "/IM", im], capture_output=True)
    if do_build:
        if opts.release:
            args = [str(PWSH), "-NoProfile", "-File", "scripts/package_fluxer_client.ps1"] + (["-IncludeVoiceWorker"] if opts.worker else [])
            if run(args):
                return 1
            try:
                write_stamp(state, opts.worker)
            except OSError as e:
                print(f"cycle: could not write build stamp: {e}")
            exe = DIST_EXE
        else:
            if run(["cargo", "build", "-p", "fluxer_client_app", "-j", "4"], REPO / "fluxer_client", env):
                return 1
            exe = TARGET / "debug" / "fluxer_client.exe"
    else:
        exe = DIST_EXE
    restart = [sys.executable, "-I", str(REPO / "tools" / "parity_lab" / "env.py"), "restart-client"] + ([] if opts.release else ["--dev"])
    try:
        r = subprocess.run(restart, capture_output=True, text=True, encoding="utf-8", errors="replace",
                           timeout=RESTART_TIMEOUT, cwd=str(REPO), stdin=subprocess.DEVNULL)
        code, out = r.returncode, (r.stdout or "") + (r.stderr or "")
    except subprocess.TimeoutExpired:
        code, out = 124, "env.py restart-client timed out"
    print(chr(10).join(out.splitlines()[-6:]))
    if code != 0:
        print("client: NOT online (check tunnel)")
        return 1
    print("client: Online")
    rp.wake_a11y()
    if opts.check:
        return run([sys.executable, "-I", str(REPO / "tools" / "parity_lab" / "check_scene.py"), *opts.check], tail=40)
    return 0


def selftest() -> int:
    checks: list[str] = []
    failures: list[str] = []

    def check(name: str, cond: bool) -> None:
        checks.append(name)
        if not cond:
            failures.append(name)

    def fake_runner(*_a, **_k) -> int:
        raise AssertionError("runner must not run during --help / --selftest parsing")

    def drive(argv: list[str]) -> tuple[int | None, str, str]:
        io_out.seek(0)
        io_out.truncate(0)
        io_err.seek(0)
        io_err.truncate(0)
        code: int | None = None
        with redirect_stdout(io_out), redirect_stderr(io_err):
            try:
                code = main(argv, runner=fake_runner)
            except SystemExit as e:
                code = 0 if e.code is None else e.code if isinstance(e.code, int) else 1
        return code, io_out.getvalue(), io_err.getvalue()

    io_out, io_err = io.StringIO(), io.StringIO()
    code, out, _err = drive(["--help"])
    check("help exits 0", code == 0)
    check("help prints usage", "usage: cycle.py" in out and "--release" in out)
    code, _out, err = drive(["--bogus-flag"])
    check("unknown flag exits 2", code == 2)
    check("unknown flag reports error", "unrecognized arguments" in err)
    check("--selftest parses", parse_args(["--selftest"]).selftest)

    opts = parse_args(["--release", "--worker", "--skip-tests", "--force"])
    check("flags parse", opts.release and opts.worker and opts.skip_tests and opts.force)
    check("--check parses four args", parse_args(["--check", "a.json", "a.png", "fluxer://x", "out"]).check == ["a.json", "a.png", "fluxer://x", "out"])
    check("--check defaults to None", parse_args([]).check is None)

    cur = {"head": "h1", "tree": "t1"}
    base = {"head": "h1", "tree": "t1", "worker": False, "created": "now"}
    check("stamp match", stamp_matches(dict(base), dict(cur), False))
    check("worker stamp matches without --worker", stamp_matches(dict(base, worker=True), dict(cur), False))
    check("stamp match with worker stamp", stamp_matches(dict(base, worker=True), dict(cur), True))
    check("worker requested but stamp has none", not stamp_matches(dict(base), dict(cur), True))
    check("head mismatch", not stamp_matches(dict(base, head="h0"), dict(cur), False))
    check("tree mismatch", not stamp_matches(dict(base, tree="t0"), dict(cur), False))
    check("missing stamp", not stamp_matches(None, dict(cur), False))
    check("empty tree", not stamp_matches(dict(base, tree=""), dict(cur), False))
    check("missing current", not stamp_matches(dict(base), {}, False))

    if failures:
        print("selftest FAILED: " + ", ".join(failures))
        return 1
    print(f"selftest ok ({len(checks)} checks)")
    return 0


def main(argv: list[str], runner=None) -> int:
    try:
        sys.stdout.reconfigure(encoding="utf-8")
    except (AttributeError, ValueError):
        pass
    opts = parse_args(argv)
    if opts.selftest:
        return selftest()
    return (runner or run_cycle)(opts)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
