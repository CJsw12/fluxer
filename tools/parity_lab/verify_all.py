#!/usr/bin/env python3
"""Run every pre-commit parity gate and print a short summary (one row per step).

    python -I tools/parity_lab/verify_all.py [--build] [--worker] [--skip-tests]
        [--base REF] [--out DIR] [--only step,step] [--no-heal]

Steps, in order:
  build    only with --build: cycle.py --release --fast [--worker] [--skip-tests] (tests + SHA256)
  precheck Rust Online, member list "Online ... 2", Rust mic on, official CDP mic/speaker on
           (on FAIL: env.py heal once, 240 s, OUT/heal.log, then precheck again)
  icons    check_icons.py (errors -> FAIL, moved -> WARN)
  colors   check_colors.py --base REF
  ratchet  spec_gate.py --live --all (counts OK/FAIL, names only the failing scenes)
  resize   check_resize.py --out OUT/resize
  surface  check_surface_parity.py --out OUT/surface
  state    state_matrix.py --strict --out OUT/state, then matrix_report.json strict_failures

Every step's stdout+stderr goes to OUT/<step>.log (OUT defaults to
target/parity-tmp/verify/<timestamp>) and is never echoed; only the final table is printed.
Exit 0 only when no step FAILs. --only runs a subset and always adds precheck unless the list
excludes it explicitly with a leading `-` or `!` (for example `--only colors,-precheck`).

The always-on client is used: --launch-rust is never passed. Python children run with
sys.executable -I and cwd=repo root. A failing precheck is healed once with env.py heal unless
--no-heal is given.
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import sys
import time
from collections.abc import Callable
from concurrent.futures import Future, ThreadPoolExecutor
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
PRECHECK_GUILD = "1572284491404148736"
PRECHECK_CHANNEL = "1572284491404148743"
PRECHECK_POLL_TIMEOUT_S = 18.0
PRECHECK_POLL_INTERVAL_S = 0.35
ORCA_WAKE_TIMEOUT_S = 10
STEPS = ("build", "precheck", "icons", "colors", "ratchet", "resize", "surface", "state", "user-area")
LIVE_STEPS = ("ratchet", "resize", "surface", "state", "user-area")
TIMEOUTS = {
    "build": 2400,
    "precheck": 120,
    "heal": 240,
    "icons": 300,
    "colors": 180,
    "ratchet": 3600,
    "resize": 900,
    "surface": 900,
    "state": 1200,
    "user-area": 60,
}
DETAIL_MAX = 96


# ---------------------------------------------------------------- plumbing

@dataclass
class Result:
    status: str  # PASS | FAIL | WARN | SKIP
    detail: str
    log: Path | None = None


def clip(text: object, limit: int = DETAIL_MAX) -> str:
    flat = " ".join(str(text).split())
    return flat if len(flat) <= limit else flat[: limit - 1] + "…"


def log_rel(path: Path) -> str:
    try:
        return path.resolve().relative_to(REPO).as_posix()
    except ValueError:
        return path.resolve().as_posix()


def _as_text(stream: object) -> str:
    if stream is None:
        return ""
    if isinstance(stream, bytes):
        return stream.decode("utf-8", "replace")
    return str(stream)


def capture(cmd: list[str], timeout: int) -> tuple[int | None, str, bool]:
    """Runs a command, returning (exit code, combined output, timed_out). Never prints."""
    try:
        r = subprocess.run(
            cmd,
            cwd=REPO,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
        )
        return r.returncode, (r.stdout or "") + (r.stderr or ""), False
    except subprocess.TimeoutExpired as exc:
        text = _as_text(exc.stdout) + _as_text(exc.stderr)
        return None, text + f"\n[verify_all] timeout after {timeout}s\n", True
    except OSError as exc:
        return None, f"[verify_all] could not start {cmd[0]}: {exc}\n", False


BROKER_GATE_STEPS = ("surface", "state", "user-area")
BROKER_LOGS: dict[str, int] = {}  # step -> broker exit code, for steps whose log was written by the broker
STATE_OFFICIAL_CACHE = REPO / "target" / "parity-tmp" / "state_official_cache"


def cached_official_state_dir(cache_dir: Path = STATE_OFFICIAL_CACHE) -> Path | None:
    """Return cache_dir when it holds a usable official state_matrix capture."""
    manifest = cache_dir / "manifest.json"
    if not (manifest.is_file() and (cache_dir / "rest.dom.json").is_file() and (cache_dir / "rest.png").is_file()):
        return None
    try:
        data = json.loads(manifest.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None
    states = data.get("states") if isinstance(data, dict) else None
    if not isinstance(states, list) or len(states) < 5:
        return None
    return cache_dir


def save_official_state_cache(src_dir: Path, cache_dir: Path = STATE_OFFICIAL_CACHE) -> bool:
    """Persist a verified official state capture into cache_dir for reuse across runs."""
    if cached_official_state_dir(src_dir) is None:
        return False
    try:
        cache_dir.parent.mkdir(parents=True, exist_ok=True)
        shutil.copytree(src_dir, cache_dir, dirs_exist_ok=True)
        return True
    except OSError:
        return False


def should_skip_after_failure(name: str, results: dict[str, Result], fail_fast: bool) -> str | None:
    """Return skip reason when an earlier failure makes running `name` wasteful, else None."""
    if name in LIVE_STEPS:
        build_res = results.get("build")
        if build_res and build_res.status == "FAIL" and "sha n/a" in build_res.detail:
            return "build failed before producing a new binary"
        pre_res = results.get("precheck")
        if pre_res and pre_res.status == "FAIL":
            return "precheck failed - fix the environment first"
    if fail_fast and name in BROKER_GATE_STEPS:
        failed = [
            step for step in ("icons", "colors", "ratchet", "resize")
            if results.get(step) and results[step].status == "FAIL"
        ]
        if failed:
            return f"skipped after earlier FAIL ({', '.join(failed)}; pass --no-fail-fast to run)"
    return None


def load_env_module():
    """env.py loaded by path (isolated mode has no script dir on sys.path)."""
    import importlib.util
    spec = importlib.util.spec_from_file_location("fluxer_env_mod", HERE / "env.py")
    if spec is None or spec.loader is None:
        raise ImportError("cannot load env.py")
    mod = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = mod
    spec.loader.exec_module(mod)
    return mod


def use_broker(environ: dict | None = None, interactive: bool | None = None) -> bool:
    import os
    environ = os.environ if environ is None else environ
    if environ.get("FLUXER_ENV_FORCE_BROKER") == "1":
        return True
    if interactive is None:
        try:
            interactive = load_env_module().on_interactive_desktop()
        except (ImportError, OSError, SyntaxError):
            interactive = True
    return not interactive


def run_gate_via_broker(names: list[str], out_dir: Path, submit=None) -> tuple[int, str]:
    """One broker `gate` request for all window-capturing steps, writing into out_dir."""
    try:
        out_dir.resolve().relative_to((REPO / "target" / "parity-tmp").resolve())
    except ValueError:
        return 2, "--out must be inside target/parity-tmp to run window steps via the broker"
    if submit is None:
        try:
            submit = load_env_module().submit_broker_request
        except (ImportError, OSError, SyntaxError) as exc:
            return 2, f"cannot load env.py: {exc}"
    return submit("gate", ["--only", ",".join(names), "--out", str(out_dir)])


def run_logged(name: str, cmd: list[str], out_dir: Path, timeout: int) -> tuple[int | None, str, bool, Path]:
    if name in BROKER_LOGS:
        log = out_dir / f"{name}.log"
        try:
            return 0, log.read_text(encoding="utf-8", errors="replace"), False, log
        except OSError:
            return 1, f"broker produced no {name}.log", False, log
    code, text, timed_out = capture(cmd, timeout)
    log = out_dir / f"{name}.log"
    log.write_text(text, encoding="utf-8", errors="replace")
    return code, text, timed_out, log


def first_error(text: str, limit: int = 70) -> str:
    """One short line that explains a failing gate, without echoing whole logs."""
    lines = [line.strip() for line in text.splitlines() if line.strip()]
    strong = ("error[", "error:", "panicked", "exception", "traceback", "runtimeerror")
    for line in lines:
        if any(k in line.lower() for k in strong):
            return clip(line, limit)
    for line in lines:
        low = line.lower()
        zero = re.search(r"\b0 failed\b", low)
        if zero:
            continue
        if any(k in low for k in ("failed", "not found", "missing", "no such")):
            return clip(line, limit)
    return clip(lines[-1], limit) if lines else "no output"


# ---------------------------------------------------------------- summary parsers

TEST_RESULT_RE = re.compile(r"test result: \w+\. (\d+) passed; (\d+) failed;")
SHA_RE = re.compile(r"SHA256:\s*([0-9a-fA-F]{8,64})")
ICONS_RE = re.compile(
    r"^(\d+) sites, (\d+) assets checked, (\d+) errors, new=(\d+) gone=(\d+) moved=(\d+) unverified=(\d+)$",
    re.MULTILINE,
)
COLORS_RE = re.compile(r"^colors: (\d+) new hard-coded literal\(s\)$", re.MULTILINE)
GATE_LINE_RE = re.compile(r"^(\S+) deltas=\d+/\d+ missing=\d+/\d+ (OK|FAIL)$")
SUMMARY_RE = re.compile(r"SUMMARY: (\d+)/(\d+) passed")
FIRST_COLOR_RE = re.compile(r"^(\S+:\d+)\s+\S[^\n]*?\| ", re.MULTILINE)


def parse_build(text: str) -> tuple[int, int, str | None]:
    passed = failed = 0
    for p, f in TEST_RESULT_RE.findall(text):
        passed += int(p)
        failed += int(f)
    shas = SHA_RE.findall(text)
    return passed, failed, (shas[-1] if shas else None)


def parse_icons(text: str) -> dict[str, int] | None:
    rows = ICONS_RE.findall(text)
    if not rows:
        return None
    keys = ("sites", "assets", "errors", "new", "gone", "moved", "unverified")
    return dict(zip(keys, (int(v) for v in rows[-1])))


def parse_colors(text: str) -> int | None:
    rows = COLORS_RE.findall(text)
    return int(rows[-1]) if rows else None


def parse_spec_gate(text: str) -> tuple[int, list[str]]:
    ok = 0
    failures: list[str] = []
    for line in text.splitlines():
        m = GATE_LINE_RE.match(line.strip())
        if not m:
            continue
        if m.group(2) == "OK":
            ok += 1
        else:
            failures.append(m.group(1))
    return ok, failures


def parse_gate_summary(text: str) -> tuple[int, int] | None:
    rows = SUMMARY_RE.findall(text)
    if not rows:
        return None
    return int(rows[-1][0]), int(rows[-1][1])


def parse_matrix_report(path: Path) -> tuple[dict | None, list[str]]:
    if not path.is_file():
        return None, []
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None, []
    if not isinstance(data, dict):
        return None, []
    failures = data.get("strict_failures") or []
    if not isinstance(failures, list):
        failures = []
    return data, [str(f) for f in failures]


# ---------------------------------------------------------------- steps

def step_build(args: argparse.Namespace, out_dir: Path) -> Result:
    if not args.build:
        return Result("SKIP", "not requested (pass --build)")
    cmd = [sys.executable, "-I", str(HERE / "cycle.py"), "--release", "--fast"]
    if args.worker:
        cmd.append("--worker")
    if args.skip_tests:
        cmd.append("--skip-tests")
    code, text, timed_out, log = run_logged("build", cmd, out_dir, TIMEOUTS["build"])
    passed, failed, sha = parse_build(text)
    if args.skip_tests:
        tests = "tests skipped"
    elif passed or failed:
        tests = f"tests {passed} passed/{failed} failed"
    else:
        tests = "no test result lines"
    detail = f"{tests}, sha {sha[:8] if sha else 'n/a'}"
    if timed_out:
        return Result("FAIL", clip(f"timeout after {TIMEOUTS['build']}s, {detail}"), log)
    if code != 0:
        return Result("FAIL", clip(f"exit {code}, {detail} ({first_error(text)})"), log)
    if (not args.skip_tests and (not passed or failed)) or not sha:
        return Result("FAIL", detail, log)
    return Result("PASS", detail, log)


def _evaluate_rust_inspect(inspect: dict) -> tuple[list, str | None, str | None, bool]:
    elements = inspect.get("elements") if isinstance(inspect.get("elements"), list) else []
    status = next(
        (e.get("text") for e in elements if e.get("flx") == "app.user-area.user-area-inner.user-status"),
        None,
    )
    header = next(
        (
            str(e.get("text"))
            for e in elements
            if e.get("flx") == "channel.member-list-group-header"
            and str(e.get("text") or "").strip().startswith("Online")
            and "2" in str(e.get("text") or "")
        ),
        None,
    )
    a11y_dump = json.dumps(inspect.get("a11y") or {}, ensure_ascii=False)
    mic_ok = "Mute microphone" in a11y_dump
    return elements, status, header, mic_ok


def _poll_rust_precheck(
    probe_fn: Callable[[], dict],
    wake_a11y_fn: Callable[[], object],
    deep_link_fn: Callable[[str], object],
    escape_fn: Callable[[], object],
    *,
    timeout_s: float = PRECHECK_POLL_TIMEOUT_S,
    interval_s: float = PRECHECK_POLL_INTERVAL_S,
    sleep_fn: Callable[[float], None] = time.sleep,
    monotonic_fn: Callable[[], float] = time.monotonic,
) -> tuple[dict, list[str], int]:
    """Poll rust_probe until Online, member list 2, and a11y mic are ready or timeout expires."""
    deadline = monotonic_fn() + max(0.0, timeout_s)
    deep_linked = False
    woke_a11y = False
    attempts = 0
    inspect: dict = {}
    last_error = ""
    elements: list = []
    status: str | None = None
    header: str | None = None
    mic_ok = False

    while True:
        attempts += 1
        try:
            reply = probe_fn()
        except Exception as exc:  # noqa: BLE001  (report any probe transport failure)
            inspect = {}
            last_error = f"rust probe unreachable: {type(exc).__name__}: {exc}"
        else:
            if isinstance(reply, dict) and reply.get("ok") is False:
                inspect = reply
                last_error = f"rust probe_inspect failed: {reply.get('error', 'unknown error')}"
            elif isinstance(reply, dict):
                inspect = reply
                last_error = ""
            else:
                inspect = {}
                last_error = ""

        elements, status, header, mic_ok = _evaluate_rust_inspect(inspect)
        if elements and status == "Online" and header and mic_ok:
            return inspect, [], attempts

        now = monotonic_fn()
        if now >= deadline:
            break

        if elements and status == "Online":
            if not mic_ok and not woke_a11y:
                try:
                    woke_a11y = wake_a11y_fn() is not False
                except Exception:  # noqa: BLE001
                    pass
            if not header and not deep_linked:
                try:
                    deep_link_fn(f"fluxer://channels/{PRECHECK_GUILD}/{PRECHECK_CHANNEL}")
                    escape_fn()
                except Exception:  # noqa: BLE001
                    pass
                deep_linked = True

        remaining = deadline - monotonic_fn()
        if remaining <= 0:
            break
        sleep_fn(min(interval_s, remaining) if interval_s > 0 else 0.0)

    fails: list[str] = []
    if last_error:
        fails.append(last_error)
    if not elements:
        if not fails:
            fails.append("rust inspect returned no elements")
    else:
        if status != "Online":
            fails.append(f"rust status {status!r} != 'Online'")
        if not header:
            fails.append("no member-list-group-header text starting with 'Online' and containing '2'")
        if not mic_ok:
            fails.append("rust mic not on ('Mute microphone' missing from a11y)")
    return inspect, fails, attempts


def wake_a11y_for_precheck(
    local_wake_fn: Callable[[], bool],
    *,
    use_broker_fn: Callable[[], bool] = use_broker,
    which_fn: Callable[[str], str | None] = shutil.which,
    capture_fn: Callable[[list[str], int], tuple[int | None, str, bool]] = capture,
) -> bool:
    """Wake fluxer_client's UIA tree locally, or via Orca on hidden desktops."""
    try:
        if local_wake_fn():
            return True
    except Exception:  # noqa: BLE001
        pass
    if not use_broker_fn():
        return False
    orca_bin = which_fn("orca")
    if not orca_bin:
        return False
    code, _text, timed_out = capture_fn(
        [
            orca_bin,
            "computer",
            "get-app-state",
            "--app",
            "fluxer_client",
            "--no-screenshot",
            "--query",
            "Status",
        ],
        ORCA_WAKE_TIMEOUT_S,
    )
    return not timed_out and code == 0


def _precheck_once(args: argparse.Namespace, out_dir: Path) -> Result:
    log = out_dir / "precheck.log"
    lines: list[str] = []

    sys.path.insert(0, str(HERE))
    import rust_probe  # noqa: PLC0415  (module reads the IPC file only when called)

    inspect, fails, attempts = _poll_rust_precheck(
        lambda: rust_probe.send("probe_inspect", timeout=2.0),
        lambda: wake_a11y_for_precheck(rust_probe.wake_a11y),
        rust_probe.deep_link,
        lambda: rust_probe.send("probe_key", keystroke="escape"),
    )
    if attempts > 1:
        lines.append(f"rust: precheck polled {attempts} attempt(s)")

    elements, status, header, mic_ok = _evaluate_rust_inspect(inspect)
    if elements:
        if status == "Online":
            lines.append("rust: status 'Online' OK")
        if header:
            lines.append(f"rust: member-list-group-header {header!r} OK")
        if mic_ok:
            lines.append("rust: 'Mute microphone' present in a11y OK (mic on)")

    port = 9334
    code, text, timed_out = capture(["node", str(HERE / "mic_state.mjs"), "--attach", str(port)], 30)
    lines.append(f"official mic_state ({port}): {clip(text, 300) or '<no output>'}")
    state = None
    if not timed_out and code == 0:
        for raw in reversed(text.splitlines()):
            raw = raw.strip()
            if raw.startswith("{"):
                try:
                    state = json.loads(raw)
                    break
                except json.JSONDecodeError:
                    continue
    if timed_out:
        fails.append(f"official CDP unreachable (mic_state.mjs timed out on port {port})")
    elif code == 2:
        fails.append(f"official CDP unreachable (mic_state.mjs exit 2, port {port})")
    elif code != 0:
        fails.append(f"mic_state.mjs exit {code}")
    elif not isinstance(state, dict):
        fails.append("mic_state.mjs printed no JSON")
    else:
        if state.get("mic_on"):
            lines.append("official: mic on OK")
        else:
            fails.append(f"official mic off (official={state.get('official')})")
        if state.get("speaker_on"):
            lines.append("official: speaker on OK")
        else:
            fails.append(f"official speaker off (official={state.get('official')})")

    lines.extend(f"FAIL {msg}" for msg in fails)
    log.write_text("\n".join(lines) + "\n", encoding="utf-8", errors="replace")
    if fails:
        return Result("FAIL", clip("; ".join(fails)), log)
    return Result("PASS", "rust Online + member list 2, rust mic on, official mic+speaker on", log)


def heal_command() -> list[str]:
    return [sys.executable, "-I", str(HERE / "env.py"), "heal"]


def should_heal(args: argparse.Namespace, result: Result) -> bool:
    """A failing precheck is healed once, unless --no-heal is given."""
    return result.status == "FAIL" and not getattr(args, "no_heal", False)


def run_heal(out_dir: Path) -> tuple[int | None, bool, Path]:
    code, text, timed_out = capture(heal_command(), TIMEOUTS["heal"])
    log = out_dir / "heal.log"
    log.write_text(
        "$ " + " ".join(heal_command()) + "\n" + text + f"\n[verify_all] exit={code} timeout={timed_out}\n",
        encoding="utf-8",
        errors="replace",
    )
    return code, timed_out, log


def step_precheck(args: argparse.Namespace, out_dir: Path) -> Result:
    result = _precheck_once(args, out_dir)
    if not should_heal(args, result):
        return result
    code, timed_out, _ = run_heal(out_dir)
    status = "heal timeout" if timed_out else f"heal exit={code}"
    first_attempt = result.log.read_text(encoding="utf-8") if result.log else ""
    retry = _precheck_once(args, out_dir)
    note = f"[verify_all] {status}; precheck after heal: {retry.status}\n"
    if retry.log and retry.log.is_file():
        with retry.log.open("a", encoding="utf-8", errors="replace") as handle:
            handle.write("[verify_all] precheck before heal:\n" + first_attempt)
            handle.write(note)
    if retry.status == "PASS":
        retry.detail = clip(f"{retry.detail} (healed; {status})")
        return retry
    retry.detail = clip(f"heal did not fix: {retry.detail} ({status})")
    return retry


def step_icons(args: argparse.Namespace, out_dir: Path) -> Result:
    code, text, timed_out, log = run_logged(
        "icons", [sys.executable, "-I", str(HERE / "check_icons.py")], out_dir, TIMEOUTS["icons"]
    )
    if timed_out:
        return Result("FAIL", f"timeout after {TIMEOUTS['icons']}s", log)
    st = parse_icons(text)
    if st is None:
        return Result("FAIL", clip(f"exit {code}, no summary line ({first_error(text)})"), log)
    detail = (
        f"sites={st['sites']} errors={st['errors']} new={st['new']} gone={st['gone']} "
        f"moved={st['moved']} unverified={st['unverified']}"
    )
    if st["errors"] > 0:
        return Result("FAIL", clip(f"{detail} ({first_error(text)})"), log)
    if code != 0:
        return Result("FAIL", clip(f"exit {code}, {detail}"), log)
    if st["moved"] > 0:
        return Result("WARN", clip(f"{detail} (run --sync to fold line drift)"), log)
    return Result("PASS", detail, log)


def step_colors(args: argparse.Namespace, out_dir: Path) -> Result:
    cmd = [sys.executable, "-I", str(HERE / "check_colors.py"), "--base", args.base]
    code, text, timed_out, log = run_logged("colors", cmd, out_dir, TIMEOUTS["colors"])
    if timed_out:
        return Result("FAIL", f"timeout after {TIMEOUTS['colors']}s", log)
    n = parse_colors(text)
    if n is None:
        return Result("FAIL", clip(f"exit {code}, no summary line ({first_error(text)})"), log)
    detail = f"{n} new literal(s)"
    first = FIRST_COLOR_RE.search(text)
    if first and n:
        detail += f", first {first.group(1)}"
    if n > 0:
        return Result("FAIL", detail, log)
    if code != 0:
        return Result("FAIL", clip(f"exit {code}, {detail}"), log)
    return Result("PASS", detail, log)


def step_ratchet(args: argparse.Namespace, out_dir: Path) -> Result:
    cmd = [sys.executable, "-I", str(HERE / "spec_gate.py"), "--live", "--all"]
    code, text, timed_out, log = run_logged("ratchet", cmd, out_dir, TIMEOUTS["ratchet"])
    if timed_out:
        return Result("FAIL", f"timeout after {TIMEOUTS['ratchet']}s", log)
    ok, failures = parse_spec_gate(text)
    detail = f"{ok} OK, {len(failures)} FAIL"
    if failures:
        extra = ", ".join(failures[:4]) + (f" +{len(failures) - 4}" if len(failures) > 4 else "")
        detail += f" ({extra})"
    if failures:
        return Result("FAIL", clip(detail), log)
    if code != 0:
        return Result("FAIL", clip(f"exit {code}, {detail} ({first_error(text)})"), log)
    return Result("PASS", detail, log)


def _step_passed_gate(name: str, cmd: list[str], out_dir: Path) -> Result:
    code, text, timed_out, log = run_logged(name, cmd, out_dir, TIMEOUTS[name])
    if timed_out:
        return Result("FAIL", f"timeout after {TIMEOUTS[name]}s", log)
    pair = parse_gate_summary(text)
    if pair is None:
        return Result("FAIL", clip(f"exit {code}, no SUMMARY line ({first_error(text)})"), log)
    passed, total = pair
    detail = f"{passed}/{total} checks passed"
    if passed != total or code != 0:
        return Result("FAIL", clip(f"{detail} ({first_error(text)})"), log)
    return Result("PASS", detail, log)


def step_resize(args: argparse.Namespace, out_dir: Path) -> Result:
    return _step_passed_gate(
        "resize",
        [sys.executable, "-I", str(HERE / "check_resize.py"), "--out", str(out_dir / "resize")],
        out_dir,
    )


def step_surface(args: argparse.Namespace, out_dir: Path) -> Result:
    return _step_passed_gate(
        "surface",
        [sys.executable, "-I", str(HERE / "check_surface_parity.py"), "--out", str(out_dir / "surface")],
        out_dir,
    )


def step_state(args: argparse.Namespace, out_dir: Path) -> Result:
    state_dir = out_dir / "state"
    cmd = [sys.executable, "-I", str(HERE / "state_matrix.py"), "--strict", "--out", str(state_dir)]
    refresh = getattr(args, "refresh_official", False)
    cache_dir = None if refresh else cached_official_state_dir()
    if cache_dir is not None:
        cmd.extend(["--official-dir", str(cache_dir)])
    code, text, timed_out, log = run_logged("state", cmd, out_dir, TIMEOUTS["state"])
    report_path = state_dir / "matrix_report.json"
    report, failures = parse_matrix_report(report_path)
    if timed_out:
        return Result("FAIL", f"timeout after {TIMEOUTS['state']}s", log)
    if report is None:
        return Result("FAIL", clip(f"no usable matrix_report.json ({first_error(text)})"), log)
    if cache_dir is None and code == 0:
        save_official_state_cache(state_dir / "official")
    detail = f"strict_failures={len(failures)}"
    if cache_dir is not None:
        detail += " (cached official)"
    if failures:
        detail += f" ({failures[0]})"
    if failures:
        return Result("FAIL", clip(detail), log)
    if code != 0:
        return Result("FAIL", clip(f"exit {code}, {detail} ({first_error(text)})"), log)
    return Result("PASS", detail, log)


def step_user_area(args: argparse.Namespace, out_dir: Path) -> Result:
    return _step_passed_gate(
        "user-area",
        [sys.executable, "-I", str(HERE / "check_user_area.py"), str(out_dir / "user-area")],
        out_dir,
    )


STEP_FUNCS = {
    "build": step_build,
    "precheck": step_precheck,
    "icons": step_icons,
    "colors": step_colors,
    "ratchet": step_ratchet,
    "resize": step_resize,
    "surface": step_surface,
    "state": step_state,
    "user-area": step_user_area,
}


# ---------------------------------------------------------------- selection / table

def select_steps(only: str | None, want_build: bool) -> list[str]:
    """Steps to execute, in canonical order. Raises ValueError on an unknown name."""
    if not only:
        keep = set(STEPS)
    else:
        include: set[str] = set()
        exclude: set[str] = set()
        for token in only.split(","):
            token = token.strip()
            if not token:
                continue
            negated = token[0] in "-!"
            name = token[1:] if negated else token
            if name not in STEPS:
                raise ValueError(f"unknown step: {name!r} (choose from {', '.join(STEPS)})")
            (exclude if negated else include).add(name)
        keep = include if include else set(STEPS)
        if "precheck" not in exclude:
            keep.add("precheck")
        keep -= exclude
    if not want_build:
        keep.discard("build")
    return [step for step in STEPS if step in keep]


def print_table(results: dict[str, Result]) -> int:
    print(f"{'step':<8}  {'result':<5}  details  (log: relative path)")
    for name in STEPS + (("client",) if "client" in results else ()):
        result = results.get(name) or Result("SKIP", "not selected")
        log = f"  (log: {log_rel(result.log)})" if result.log else ""
        print(f"{name:<8}  {result.status:<5}  {clip(result.detail)}{log}")
    counts = {status: sum(1 for r in results.values() if r.status == status) for status in ("PASS", "WARN", "FAIL", "SKIP")}
    failed = counts["FAIL"] > 0
    print(
        f"verify: {counts['PASS']} PASS, {counts['WARN']} WARN, "
        f"{counts['FAIL']} FAIL, {counts['SKIP']} SKIP -> exit {1 if failed else 0}"
    )
    return 1 if failed else 0


def client_running() -> bool:
    """True when the Rust client answers the parity probe on its IPC port."""
    sys.path.insert(0, str(HERE))
    try:
        import rust_probe  # noqa: PLC0415
        return bool(rust_probe.send("probe_inspect", timeout=5.0).get("ok"))
    except Exception:  # noqa: BLE001 - any failure means not reachable
        return False


def restart_client() -> tuple[int | None, str]:
    code, text, timed_out = capture([sys.executable, "-I", str(HERE / "env.py"), "restart-client"], TIMEOUTS["heal"])
    return (None if timed_out else code), text


def restore_client(probe=client_running, restart=restart_client) -> Result:
    """Leaves the always-on client running after gates that may have closed it."""
    if probe():
        return Result("PASS", "running")
    code, text = restart()
    if code == 0 and probe():
        return Result("PASS", "restarted")
    return Result("FAIL", clip(f"restart-client exit {code} ({first_error(text)})"))


# ---------------------------------------------------------------- selftest

BUILD_SAMPLE = """[   12s] cargo test -p fluxer_client_app -j 4 -> 0
    test result: ok. 480 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 12.3s
    test result: ok. 32 passed; 0 failed; 3 ignored; 0 measured; 0 filtered out; finished in 4.1s
[   45s] pwsh -NoProfile -File scripts/package_fluxer_client.ps1 -> 0
    SHA256: 9f2c1c0d4b3a2e5f6a7b8c9d0e1f2a3b4c5d6e7f
"""

ICONS_SAMPLE = """ERROR NEW call site fluxer_client/app/src/ui/x.rs:12 Chat
MOVED fluxer_client/app/src/ui/x.rs:Chat 10 -> 11
512 sites, 96 assets checked, 0 errors, new=0 gone=0 moved=3 unverified=0
"""

COLORS_SAMPLE = """fluxer_client/app/src/ui/x.rs:12  rgb(0x112233)  | let a = rgb(0x112233);
colors: 1 new hard-coded literal(s)
"""

GATE_SAMPLE = """markdown deltas=0/0 missing=0/0 OK
popouts/menu deltas=2/2 missing=1/1 OK
popouts/emoji deltas=3/2 missing=0/0 FAIL
  scene status=failed: expect_flx not shown
"""

RESIZE_SAMPLE = """  [PASS] sidebar.default.nav_w  got=240 want=240

SUMMARY: 7/8 passed -> target/parity-tmp/verify/x/resize/resize_report.json
"""


def selftest() -> None:
    assert parse_build(BUILD_SAMPLE) == (512, 0, "9f2c1c0d4b3a2e5f6a7b8c9d0e1f2a3b4c5d6e7f")
    assert parse_build("test result: FAILED. 4 passed; 1 failed; 0 ignored")[:2] == (4, 1)
    assert parse_build("no tests here") == (0, 0, None)

    icons = parse_icons(ICONS_SAMPLE)
    assert icons is not None and icons["errors"] == 0 and icons["moved"] == 3 and icons["sites"] == 512, icons
    assert parse_icons("no summary") is None
    assert parse_colors(COLORS_SAMPLE) == 1
    assert parse_colors("nothing") is None
    assert FIRST_COLOR_RE.search(COLORS_SAMPLE).group(1) == "fluxer_client/app/src/ui/x.rs:12"

    ok, failures = parse_spec_gate(GATE_SAMPLE)
    assert ok == 2 and failures == ["popouts/emoji"], (ok, failures)
    assert parse_spec_gate("markdown deltas=5 max 1->2 missing=0 max 0->0 lowered") == (0, [])

    assert parse_gate_summary(RESIZE_SAMPLE) == (7, 8)
    assert parse_gate_summary("no summary") is None

    assert select_steps(None, False) == ["precheck", "icons", "colors", "ratchet", "resize", "surface", "state", "user-area"]
    assert select_steps(None, True)[0] == "build"
    assert select_steps("colors", False) == ["precheck", "colors"]
    assert select_steps("colors,-precheck", False) == ["colors"]
    assert select_steps("!precheck,icons", False) == ["icons"]
    assert select_steps("build", True) == ["build", "precheck"]
    assert select_steps("-resize", False) == ["precheck", "icons", "colors", "ratchet", "surface", "state", "user-area"]
    for bad in ("nope", "colors,nope"):
        try:
            select_steps(bad, False)
        except ValueError:
            pass
        else:
            raise AssertionError(f"select_steps({bad!r}) must raise")

    assert clip("a" * 200) == "a" * (DETAIL_MAX - 1) + "…"
    assert clip("a  b\n c") == "a b c"

    class _Args:
        no_heal = False

    ok_result = Result("PASS", "fine")
    fail_result = Result("FAIL", "offline")
    assert should_heal(_Args(), fail_result)
    assert not should_heal(_Args(), ok_result)
    _Args.no_heal = True
    assert not should_heal(_Args(), fail_result)
    assert heal_command() == [sys.executable, "-I", str(HERE / "env.py"), "heal"]
    assert TIMEOUTS["heal"] == 240
    healed = Result("PASS", "rust Online (healed; heal exit=0)")
    assert "(healed" in healed.detail
    assert restore_client(lambda: True, lambda: (_ for _ in ()).throw(AssertionError("no restart"))).detail == "running"
    states = iter([False, True])
    assert restore_client(lambda: next(states), lambda: (0, "")).detail == "restarted"
    assert restore_client(lambda: False, lambda: (1, "boom")).status == "FAIL"
    assert restore_client(lambda: False, lambda: (None, "")).status == "FAIL"
    # _poll_rust_precheck: transient not-ready -> retry success, and timeout failure
    clock = [0.0]

    def _fake_sleep(dt: float) -> None:
        clock[0] += dt

    ready_payload = {
        "ok": True,
        "elements": [
            {"flx": "app.user-area.user-area-inner.user-status", "text": "Online"},
            {"flx": "channel.member-list-group-header", "text": "Online—2"},
        ],
        "a11y": {"name": "Mute microphone"},
    }
    probe_seq = iter([
        OSError("starting"),
        {"ok": True, "elements": [{"flx": "app.user-area.user-area-inner.user-status", "text": "Online"}], "a11y": {}},
        ready_payload,
    ])
    wakes: list[int] = []
    links: list[str] = []
    escapes: list[int] = []

    def _next_probe() -> dict:
        item = next(probe_seq)
        if isinstance(item, Exception):
            raise item
        return item

    _, poll_fails, poll_attempts = _poll_rust_precheck(
        _next_probe,
        lambda: wakes.append(1),
        lambda url: links.append(url),
        lambda: escapes.append(1),
        timeout_s=10.0,
        interval_s=1.0,
        sleep_fn=_fake_sleep,
        monotonic_fn=lambda: clock[0],
    )
    assert poll_fails == [] and poll_attempts == 3, (poll_fails, poll_attempts)
    assert len(wakes) == 1 and len(links) == 1 and len(escapes) == 1

    clock[0] = 0.0
    _, timeout_fails, timeout_attempts = _poll_rust_precheck(
        lambda: {"ok": True, "elements": [{"flx": "app.user-area.user-area-inner.user-status", "text": "Online"}], "a11y": {}},
        lambda: None,
        lambda _url: None,
        lambda: None,
        timeout_s=3.0,
        interval_s=1.5,
        sleep_fn=_fake_sleep,
        monotonic_fn=lambda: clock[0],
    )
    assert timeout_attempts == 3 and len(timeout_fails) == 2, (timeout_attempts, timeout_fails)

    # wake_a11y_for_precheck: local success skips orca; hidden desktop falls back to orca
    orca_calls: list[tuple[list[str], int]] = []
    assert wake_a11y_for_precheck(
        lambda: True,
        use_broker_fn=lambda: (_ for _ in ()).throw(AssertionError("should not check broker")),
        which_fn=lambda _n: (_ for _ in ()).throw(AssertionError("should not check orca")),
        capture_fn=lambda cmd, t: (orca_calls.append((cmd, t)) or (0, "", False)),
    )
    assert not wake_a11y_for_precheck(
        lambda: False,
        use_broker_fn=lambda: False,
        which_fn=lambda _n: (_ for _ in ()).throw(AssertionError("should not check orca")),
        capture_fn=lambda cmd, t: (orca_calls.append((cmd, t)) or (0, "", False)),
    )
    assert wake_a11y_for_precheck(
        lambda: (_ for _ in ()).throw(OSError("no hwnd")),
        use_broker_fn=lambda: True,
        which_fn=lambda n: "C:/orca/orca.cmd" if n == "orca" else None,
        capture_fn=lambda cmd, t: (orca_calls.append((cmd, t)) or (0, "ok", False)),
    )
    assert orca_calls == [(
        ["C:/orca/orca.cmd", "computer", "get-app-state", "--app", "fluxer_client", "--no-screenshot", "--query", "Status"],
        ORCA_WAKE_TIMEOUT_S,
    )]
    assert not wake_a11y_for_precheck(
        lambda: False,
        use_broker_fn=lambda: True,
        which_fn=lambda _n: None,
        capture_fn=lambda _cmd, _t: (_ for _ in ()).throw(AssertionError("should not run capture")),
    )
    assert not wake_a11y_for_precheck(
        lambda: False,
        use_broker_fn=lambda: True,
        which_fn=lambda _n: "orca",
        capture_fn=lambda _cmd, _t: (1, "err", False),
    )
    assert not wake_a11y_for_precheck(
        lambda: False,
        use_broker_fn=lambda: True,
        which_fn=lambda _n: "orca",
        capture_fn=lambda _cmd, _t: (None, "timeout", True),
    )

    assert build_parser().parse_args(["--no-restore-client"]).no_restore_client
    assert build_parser().parse_args(["--no-fail-fast"]).no_fail_fast
    assert build_parser().parse_args(["--refresh-official"]).refresh_official
    assert should_skip_after_failure("ratchet", {"build": Result("FAIL", "exit 1, no tests, sha n/a")}, True) is not None
    assert should_skip_after_failure("ratchet", {"build": Result("FAIL", "exit 1, sha 8c6bf354 (client offline)")}, True) is None
    assert should_skip_after_failure("surface", {"ratchet": Result("FAIL", "21 OK, 1 FAIL")}, True) is not None
    assert should_skip_after_failure("surface", {"ratchet": Result("FAIL", "21 OK, 1 FAIL")}, False) is None
    assert should_skip_after_failure("resize", {"ratchet": Result("FAIL", "21 OK, 1 FAIL")}, True) is None
    assert callable(load_env_module().submit_broker_request)
    assert use_broker({}, interactive=False) and not use_broker({}, interactive=True)
    assert use_broker({"FLUXER_ENV_FORCE_BROKER": "1"}, interactive=True)
    import tempfile
    with tempfile.TemporaryDirectory(dir=REPO / "target" / "parity-tmp" if (REPO / "target" / "parity-tmp").is_dir() else None) as td:
        gd = Path(td)
        fake_off = gd / "off"
        fake_off.mkdir()
        assert cached_official_state_dir(fake_off) is None
        (fake_off / "manifest.json").write_text(json.dumps({"states": [{"slug": f"s{i}"} for i in range(6)]}), encoding="utf-8")
        (fake_off / "rest.dom.json").write_text("[]", encoding="utf-8")
        (fake_off / "rest.png").write_bytes(b"png")
        assert cached_official_state_dir(fake_off) == fake_off
        fake_cache = gd / "cache"
        assert save_official_state_cache(fake_off, fake_cache)
        assert cached_official_state_dir(fake_cache) == fake_cache
        calls: list = []
        assert run_gate_via_broker(["surface"], Path(tempfile.gettempdir()) / "nope", lambda *a: (0, ""))[0] == 2
        if gd.resolve().is_relative_to((REPO / "target" / "parity-tmp").resolve()):
            assert run_gate_via_broker(["surface", "state"], gd, lambda a, e: (calls.append((a, e)) or (0, "ok")))[0] == 0
            assert calls == [("gate", ["--only", "surface,state", "--out", str(gd)])]
            (gd / "surface.log").write_text("noise" + chr(10) + "SUMMARY: 5/5 passed" + chr(10), encoding="utf-8")
            BROKER_LOGS["surface"] = 0
            try:
                r = step_surface(argparse.Namespace(), gd)
                assert r.status in ("PASS", "FAIL") and r.log == gd / "surface.log"
                BROKER_LOGS["state"] = 0
                assert step_state(argparse.Namespace(), gd).status == "FAIL"  # no matrix_report.json
            finally:
                BROKER_LOGS.clear()
    print("selftest ok (build/icons/colors/spec_gate/resize parsers + step selection + heal gating + client restore + fail-fast + official cache)")


# ---------------------------------------------------------------- CLI

def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="verify_all.py",
        description="Run every pre-commit parity gate, log each to OUT/<step>.log and print a short summary.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=(
            "examples:\n"
            "  python -I tools/parity_lab/verify_all.py\n"
            "  python -I tools/parity_lab/verify_all.py --build --skip-tests\n"
            "  python -I tools/parity_lab/verify_all.py --only colors,icons --base origin/desktop-rs\n"
            "  python -I tools/parity_lab/verify_all.py --only colors,-precheck\n"
            "  python -I tools/parity_lab/verify_all.py --only precheck --no-heal\n"
            "  python -I tools/parity_lab/verify_all.py --selftest"
        ),
    )
    p.add_argument("--build", action="store_true", help="also run cycle.py --release --fast (tests + parity package)")
    p.add_argument("--worker", action="store_true", help="with --build: pass --worker to cycle.py")
    p.add_argument("--skip-tests", action="store_true", help="with --build: pass --skip-tests to cycle.py")
    p.add_argument("--base", default="HEAD", metavar="REF", help="git ref for check_colors.py (default: HEAD)")
    p.add_argument("--out", metavar="DIR", help="log/output dir (default: target/parity-tmp/verify/<timestamp>)")
    p.add_argument(
        "--only",
        metavar="STEP,STEP",
        help="subset of: " + ", ".join(STEPS) + " (precheck is added unless excluded with -step)",
    )
    p.add_argument("--selftest", action="store_true", help="unit-test the output parsers and step selection")
    p.add_argument(
        "--no-heal",
        action="store_true",
        help="do not run env.py heal when precheck fails (default: heal once, then precheck again)",
    )
    p.add_argument(
        "--no-restore-client",
        action="store_true",
        help="do not restart the Rust client at the end when it is not running (default: restore after build/live steps)",
    )
    p.add_argument(
        "--no-fail-fast",
        action="store_true",
        help="run slow surface/state gates even when icons/colors/ratchet/resize already failed",
    )
    p.add_argument(
        "--refresh-official",
        action="store_true",
        help="re-capture official state_matrix instead of reusing target/parity-tmp/state_official_cache",
    )
    return p


def main(argv: list[str] | None = None) -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    parser = build_parser()
    args = parser.parse_args(argv)
    if args.selftest:
        selftest()
        return 0
    try:
        steps = select_steps(args.only, args.build)
    except ValueError as exc:
        parser.error(str(exc))

    out_dir = Path(args.out).expanduser() if args.out else REPO / "target" / "parity-tmp" / "verify" / datetime.now().strftime("%Y%m%d-%H%M%S")
    if not out_dir.is_absolute():
        out_dir = REPO / out_dir
    out_dir.mkdir(parents=True, exist_ok=True)

    started = time.monotonic()
    with ThreadPoolExecutor(max_workers=2) as executor:
        static = {
            name: executor.submit(timed_step, name, args, out_dir)
            for name in ("icons", "colors") if "build" in steps and name in steps
        }
        results, durations = run_steps(args, steps, out_dir, static)
    rows = [
        {"name": name, "status": result.status, "sec": round(durations.get(name, 0), 3)}
        for name, result in results.items()
    ]
    longest = max(rows, key=lambda row: row["sec"])
    summary = {"total_sec": round(time.monotonic() - started, 3), "longest_step": longest["name"], "steps": rows}
    (out_dir / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    return print_table(results)


def timed_step(name: str, args: argparse.Namespace, out_dir: Path) -> tuple[Result, float]:
    started = time.monotonic()
    result = STEP_FUNCS[name](args, out_dir)
    return result, time.monotonic() - started


def run_steps(
    args: argparse.Namespace, steps: list[str], out_dir: Path, static: dict[str, Future[tuple[Result, float]]],
) -> tuple[dict[str, Result], dict[str, float]]:
    results: dict[str, Result] = {}
    durations: dict[str, float] = {}
    broker_decision = any(n in BROKER_GATE_STEPS for n in steps) and use_broker()
    broker_rows = {}
    gate_failure = ""
    for name in STEPS:
        if name not in steps:
            detail = "not requested (pass --build)" if name == "build" and not args.build else "not selected"
            results[name] = Result("SKIP", detail)
            continue
        skip_reason = should_skip_after_failure(name, results, fail_fast=not args.no_fail_fast)
        if skip_reason is not None:
            results[name] = Result("SKIP", skip_reason)
            continue
        started = time.monotonic()
        via_broker = False
        if name in BROKER_GATE_STEPS and broker_decision:
            if not BROKER_LOGS and not gate_failure:
                names = [n for n in BROKER_GATE_STEPS if n in steps]
                summary_file = out_dir / "summary.json"
                summary_file.unlink(missing_ok=True)
                code, tail = run_gate_via_broker(names, out_dir)
                if code in (2, 3, 4):
                    gate_failure = tail
                elif summary_file.is_file():
                    broker_rows = {row["name"]: row for row in json.loads(summary_file.read_text(encoding="utf-8"))["steps"]}
                    for n in names:
                        BROKER_LOGS[n] = code
                else:
                    gate_failure = "no verification summary from broker"
            via_broker = True
            if gate_failure:
                results[name] = Result("FAIL", clip(f"broker: {gate_failure}"))
                durations[name] = time.monotonic() - started
                continue
        results[name], durations[name] = static[name].result() if name in static else timed_step(name, args, out_dir)
        if via_broker:
            row = broker_rows[name]
            durations[name] = row["sec"]
            if results[name].status == "PASS" and row["status"] != "PASS":
                results[name] = Result("FAIL", f"broker reported {row['status']}", results[name].log)
            results[name].detail += " (via broker)"
        if results[name].status != "SKIP":
            results[name].detail += f" [{durations[name]:.0f}s]"
    BROKER_LOGS.clear()
    if not args.no_restore_client and (args.build or any(n in LIVE_STEPS for n in steps)):
        started = time.monotonic()
        results["client"] = restore_client()
        durations["client"] = time.monotonic() - started
    return results, durations


if __name__ == "__main__":
    raise SystemExit(main())
