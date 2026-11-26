#!/usr/bin/env python3
"""Env broker: runs env.py actions on the interactive user desktop on behalf of sandboxed agents.

Agents running on a hidden desktop write a request JSON into target/parity-tmp/env-broker/requests/;
this broker (started at logon on the user desktop) runs `env.py <action>` itself, so GUI processes
appear on the user's visible desktop, and writes responses/<id>.json.

    python -I tools/parity_lab/env_broker.py            # loop (poll ~1 s)
    python -I tools/parity_lab/env_broker.py --once     # process pending requests and exit
    python -I tools/parity_lab/env_broker.py --install [--dry-run] | --uninstall | --status
    python -I tools/parity_lab/env_broker.py --selftest

Only the actions in ALLOWLIST run; args are limited to --json and --config <path>.
"""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
BROKER_DIR = REPO / "target" / "parity-tmp" / "env-broker"
ENV_PY = HERE / "env.py"
ALLOWLIST = ("restart-client", "up", "heal", "shot", "gate")
GATE_STEPS = ("surface", "state")
GATE_TIMEOUT = 900
VERIFY_PY = HERE / "verify_all.py"
SHOT_ROOT = REPO / "target" / "parity-tmp"
PROBE_PY = HERE / "rust_probe.py"
MAX_AGE = 600.0
RUN_TIMEOUT = 170
POLL = 1.0
LOG_LIMIT = 1_000_000
TASK_NAME = "FluxerEnvBroker"
HEARTBEAT_STALE = 5.0
WATCHDOG_INTERVAL = 30.0
WATCHDOG_BACKOFF = 120.0
WATCHDOG_FAILS = 2


def log(broker_dir: Path, msg: str) -> None:
    try:
        broker_dir.mkdir(parents=True, exist_ok=True)
        path = broker_dir / "broker.log"
        if path.exists() and path.stat().st_size > LOG_LIMIT:
            data = path.read_bytes()[-LOG_LIMIT // 2:]
            path.write_bytes(data)
        with path.open("a", encoding="utf-8") as fh:
            fh.write(f"{time.strftime('%Y-%m-%d %H:%M:%S')} {msg}\n")
    except OSError:
        pass


def shot_path(raw: str) -> str | None:
    """Resolved output path when raw is a .png inside <repo>/target/parity-tmp, else None."""
    if not raw or raw.startswith("-") or "\x00" in raw or ".." in Path(raw).parts or not raw.lower().endswith(".png"):
        return None
    path = Path(raw)
    try:
        resolved = (path if path.is_absolute() else REPO / path).resolve()
        resolved.relative_to(SHOT_ROOT.resolve())
    except (OSError, ValueError):
        return None
    return str(resolved)


def gate_args(raw: list[str]) -> list[str] | None:
    """Validated verify_all args for `--only surface,state --out <dir under target/parity-tmp>`, else None."""
    if len(raw) != 4 or raw[0] != "--only" or raw[2] != "--out":
        return None
    items = raw[1].split(",")
    if not items or any(i not in GATE_STEPS for i in items) or len(set(items)) != len(items):
        return None
    out = raw[3]
    if not out or out.startswith("-") or "\x00" in out or ".." in Path(out).parts:
        return None
    try:
        resolved = (Path(out) if Path(out).is_absolute() else REPO / out).resolve()
        resolved.relative_to(SHOT_ROOT.resolve())
    except (OSError, ValueError):
        return None
    if resolved == SHOT_ROOT.resolve():
        return None
    return ["--only", ",".join(items), "--out", str(resolved)]


def validate(req: object, now: float) -> tuple[str | None, list[str], str | None]:
    """Returns (action, extra_args, error)."""
    if not isinstance(req, dict):
        return None, [], "request is not an object"
    rid = req.get("id")
    if not isinstance(rid, str) or not rid.isalnum():
        return None, [], "bad id"
    action = req.get("action")
    if action not in ALLOWLIST:
        return None, [], f"action not allowed: {str(action)[:40]}"
    created = req.get("created")
    if not isinstance(created, (int, float)) or now - created > MAX_AGE:
        return None, [], "request expired"
    raw = req.get("args", [])
    if not isinstance(raw, list) or not all(isinstance(a, str) for a in raw):
        return None, [], "bad args"
    extra: list[str] = []
    if action == "shot":
        target = shot_path(raw[0]) if len(raw) == 1 else None
        return (action, [target], None) if target else (None, [], "shot path not allowed")
    if action == "gate":
        g = gate_args(raw)
        return (action, g, None) if g else (None, [], "gate args not allowed")
    i = 0
    while i < len(raw):
        if raw[i] == "--json":
            extra.append("--json")
            i += 1
        elif raw[i] == "--dev" and action == "restart-client":
            extra.append("--dev")
            i += 1
        elif raw[i] == "--config" and i + 1 < len(raw) and not raw[i + 1].startswith("-"):
            extra += ["--config", raw[i + 1]]
            i += 2
        else:
            return None, [], "arg not allowed"
    return action, extra, None


def default_runner(action: str, extra: list[str]) -> tuple[int, str]:
    env = {k: v for k, v in os.environ.items() if k != "FLUXER_ENV_FORCE_BROKER"}
    timeout = RUN_TIMEOUT
    if action == "shot":
        cmd = [sys.executable, "-I", str(PROBE_PY), "shot", *extra]
    elif action == "gate":
        cmd = [sys.executable, "-I", str(VERIFY_PY), *extra, "--no-heal", "--no-restore-client"]
        timeout = GATE_TIMEOUT
    else:
        cmd = [sys.executable, "-I", str(ENV_PY), action, *extra]
    try:
        r = subprocess.run(cmd, capture_output=True, text=True,
                           encoding="utf-8", errors="replace", timeout=timeout, env=env, cwd=str(REPO),
                           stdin=subprocess.DEVNULL)
        return r.returncode, (r.stdout or "") + (r.stderr or "")
    except subprocess.TimeoutExpired:
        return 124, f"{action} timed out in broker"
    except OSError as exc:
        return 125, f"could not start env.py: {exc}"


def write_json(path: Path, data: dict) -> None:
    tmp = path.with_suffix(".tmp")
    tmp.write_text(json.dumps(data, ensure_ascii=False), encoding="utf-8")
    os.replace(tmp, path)


def process_pending(broker_dir: Path, runner=default_runner, now: float | None = None) -> int:
    req_dir = broker_dir / "requests"
    resp_dir = broker_dir / "responses"
    req_dir.mkdir(parents=True, exist_ok=True)
    resp_dir.mkdir(parents=True, exist_ok=True)
    done = 0
    for path in sorted(req_dir.glob("*.json"), key=lambda p: p.stat().st_mtime):
        now_ts = time.time() if now is None else now
        try:
            req = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            req = None
        try:
            path.unlink()
        except OSError:
            continue
        action, extra, err = validate(req, now_ts)
        rid = req.get("id") if isinstance(req, dict) and isinstance(req.get("id"), str) and req["id"].isalnum() else None
        if err:
            log(broker_dir, f"rejected {path.name}: {err}")
            if rid and err != "request expired":
                write_json(resp_dir / f"{rid}.json", {"id": rid, "exit_code": 2, "output_tail": f"rejected: {err}",
                                                      "finished": time.time()})
            continue
        log(broker_dir, f"run {action} {extra} id={rid}")
        code, out = runner(action or "", extra)
        tail = "\n".join(out.splitlines()[-40:])
        write_json(resp_dir / f"{rid}.json", {"id": rid, "exit_code": code, "output_tail": tail, "finished": time.time()})
        log(broker_dir, f"done {action} exit={code}")
        done += 1
    return done


class Lock:
    """Single-instance lock via an exclusive byte lock on a file."""

    def __init__(self, path: Path):
        self.path = path
        self.fh = None

    def acquire(self) -> bool:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        try:
            self.fh = open(self.path, "a+b")
            self.fh.seek(0)
            if os.name == "nt":
                import msvcrt
                msvcrt.locking(self.fh.fileno(), msvcrt.LK_NBLCK, 1)
            else:
                import fcntl
                fcntl.flock(self.fh.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
            return True
        except OSError:
            if self.fh:
                self.fh.close()
                self.fh = None
            return False

    def release(self) -> None:
        if self.fh:
            self.fh.close()
            self.fh = None


def heartbeat(broker_dir: Path) -> None:
    try:
        (broker_dir / "heartbeat").write_text(str(time.time()), encoding="utf-8")
    except OSError:
        pass


def broker_alive(broker_dir: Path = BROKER_DIR) -> bool:
    try:
        return time.time() - float((broker_dir / "heartbeat").read_text(encoding="utf-8")) < HEARTBEAT_STALE
    except (OSError, ValueError):
        return False


class Watchdog:
    """Restarts the tunnel (env.py up) after consecutive failed TCP checks of its local port."""

    def __init__(self, check, up, logger, interval=WATCHDOG_INTERVAL, backoff=WATCHDOG_BACKOFF, fails=WATCHDOG_FAILS):
        self.check, self.up, self.logger = check, up, logger
        self.interval, self.backoff, self.fails = interval, backoff, fails
        self.next_check = 0.0
        self.failures = 0
        self.backoff_until = 0.0
        self.backoff_logged = False

    def tick(self, now: float, busy: bool = False) -> str:
        """Returns one of: idle, ok, fail, busy, backoff, restarted, restart-failed."""
        if now < self.next_check:
            return "idle"
        if busy:
            return "busy"
        self.next_check = now + self.interval
        if self.check():
            self.failures = 0
            self.backoff_logged = False
            return "ok"
        self.failures += 1
        if self.failures < self.fails:
            return "fail"
        if now < self.backoff_until:
            if not self.backoff_logged:
                self.logger("watchdog: tunnel still down, backing off")
                self.backoff_logged = True
            return "backoff"
        self.logger("watchdog: tunnel down, running env.py up")
        ok = self.up()
        self.failures = 0
        if ok:
            self.logger("watchdog: env.py up finished")
            return "restarted"
        self.backoff_until = now + self.backoff
        self.backoff_logged = False
        self.logger(f"watchdog: env.py up failed, backoff {int(self.backoff)}s")
        return "restart-failed"


def make_watchdog(broker_dir: Path) -> "Watchdog | None":
    import importlib.util
    try:
        spec = importlib.util.spec_from_file_location("fluxer_env_cfg", ENV_PY)
        mod = importlib.util.module_from_spec(spec)
        sys.modules[spec.name] = mod
        spec.loader.exec_module(mod)
        cfg = mod.load_config(None)
        host, port = mod.split_host_port(cfg.tunnel_local)
    except Exception as exc:  # noqa: BLE001
        log(broker_dir, f"watchdog disabled: config not loadable ({type(exc).__name__})")
        return None

    def check() -> bool:
        return mod.tcp_ok(cfg.tunnel_local, 3.0)

    def up() -> bool:
        code, _ = default_runner("up", [])
        return code == 0

    return Watchdog(check, up, lambda m: log(broker_dir, m))


def run_loop(broker_dir: Path, once: bool, watchdog: bool = True) -> int:
    lock = Lock(broker_dir / "broker.lock")
    if not lock.acquire():
        print("env_broker: another broker is already running")
        return 1
    try:
        log(broker_dir, f"broker start pid={os.getpid()} once={once}")
        wd = make_watchdog(broker_dir) if watchdog and not once else None
        while True:
            heartbeat(broker_dir)
            busy = process_pending(broker_dir) > 0
            if once:
                return 0
            if wd is not None:
                wd.tick(time.time(), busy)
                heartbeat(broker_dir)
            time.sleep(POLL)
    finally:
        lock.release()


def broker_interpreter() -> str:
    exe = Path(sys.executable)
    pyw = exe.with_name("pythonw.exe")
    return str(pyw if pyw.exists() else exe)


def startup_dir() -> Path:
    """Per-user Startup folder (no admin needed, unlike an ONLOGON scheduled task)."""
    return Path(os.environ.get("APPDATA", "")) / "Microsoft" / "Windows" / "Start Menu" / "Programs" / "Startup"


def startup_file() -> Path:
    return startup_dir() / f"{TASK_NAME}.cmd"


def startup_script() -> str:
    return f'@start "" "{broker_interpreter()}" -I "{Path(__file__).resolve()}"\r\n'


def task_exists() -> bool:
    return startup_file().is_file()


def start_detached() -> None:
    flags = 0x00000008 | 0x00000200 | 0x01000000  # DETACHED_PROCESS | NEW_PROCESS_GROUP | BREAKAWAY_FROM_JOB
    subprocess.Popen([broker_interpreter(), "-I", str(Path(__file__).resolve())], cwd=str(REPO),
                     stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                     creationflags=flags, close_fds=True)


def selftest() -> None:
    import tempfile
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        now = time.time()
        assert validate({"id": "a1", "action": "up", "args": ["--json"], "created": now}, now)[2] is None
        assert validate({"id": "a1", "action": "down", "args": [], "created": now}, now)[2]
        assert validate({"id": "a1", "action": "up", "args": ["calc.exe"], "created": now}, now)[2]
        assert validate({"id": "a1", "action": "up", "args": [], "created": now - 700}, now)[2] == "request expired"
        assert validate({"id": "../x", "action": "up", "args": [], "created": now}, now)[2]
        assert validate({"id": "a1", "action": "up", "args": ["--config", "C:/x.json"], "created": now}, now)[1] == ["--config", "C:/x.json"]
        assert validate({"id": "s1", "action": "shot", "args": ["target/parity-tmp/a/x.png"], "created": now}, now)[1] == [
            str((REPO / "target/parity-tmp/a/x.png").resolve())]
        for bad in ("target/parity-tmp/../x.png", "target/parity-tmp/x.txt", "C:/Windows/x.png", "/tmp/x.png",
                    "target/other/x.png", "-o.png", "target/parity-tmp"):
            assert shot_path(bad) is None, bad
        assert validate({"id": "s1", "action": "shot", "args": [], "created": now}, now)[2]
        assert validate({"id": "s1", "action": "shot", "args": ["target/parity-tmp/a.png", "--json"], "created": now}, now)[2]
        g = lambda a: validate({"id": "g9", "action": "gate", "args": a, "created": now}, now)  # noqa: E731
        okout = "target/parity-tmp/verify/x"
        resolved = str((REPO / okout).resolve())
        assert g(["--only", "surface,state", "--out", okout]) == ("gate", ["--only", "surface,state", "--out", resolved], None)
        assert g(["--only", "state", "--out", resolved])[1] == ["--only", "state", "--out", resolved]
        for bad in (["--only", "build", "--out", okout], ["--only", "surface,colors", "--out", okout],
                    ["--only", "surface", "--out", "target/parity-tmp/../x"], ["--only", "surface", "--out", "C:/Windows"],
                    ["--only", "surface", "--out", "-x"], ["--only", "surface", "--out", "target/parity-tmp"],
                    ["--only", "surface", "--out", "target/other"], ["--only", "surface"], ["--only", "", "--out", okout],
                    ["--only", "surface,surface", "--out", okout], ["--out", okout, "--only", "surface"],
                    ["--only", "surface", "--out", okout, "--build"], []):
            assert g(bad)[2], bad
        assert validate({"id": "d1", "action": "restart-client", "args": ["--dev"], "created": now}, now)[1] == ["--dev"]
        assert validate({"id": "d1", "action": "up", "args": ["--dev"], "created": now}, now)[2]
        calls: list = []

        def stub(action, extra):
            calls.append((action, extra))
            return 0, "\n".join(f"line{i}" for i in range(60))

        req = d / "requests"
        req.mkdir()
        (req / "g1.json").write_text(json.dumps({"id": "g1", "action": "restart-client", "args": [], "created": now}))
        (req / "b1.json").write_text(json.dumps({"id": "b1", "action": "rm", "args": [], "created": now}))
        (req / "o1.json").write_text(json.dumps({"id": "o1", "action": "up", "args": [], "created": now - 9999}))
        (req / "junk.json").write_text("{not json")
        assert process_pending(d, stub) == 1
        assert calls == [("restart-client", [])]
        ok = json.loads((d / "responses" / "g1.json").read_text())
        assert ok["exit_code"] == 0 and len(ok["output_tail"].splitlines()) == 40 and ok["output_tail"].endswith("line59")
        assert json.loads((d / "responses" / "b1.json").read_text())["exit_code"] == 2
        assert not (d / "responses" / "o1.json").exists() and not list(req.glob("*.json"))
        first = Lock(d / "l.lock")
        assert first.acquire()
        other = Lock(d / "l.lock")
        assert not other.acquire()
        first.release()
        assert run_loop(d, True) == 0 and broker_alive(d)
        msgs: list[str] = []
        state = {"up": True, "ups": 0, "result": False}
        def fake_up() -> bool:
            state["ups"] += 1
            return state["result"]
        w = Watchdog(lambda: state["up"], fake_up, msgs.append)
        assert w.tick(0) == "ok" and w.tick(1) == "idle"
        state["up"] = False
        assert w.tick(30, busy=True) == "busy" and state["ups"] == 0
        assert w.tick(31) == "fail" and state["ups"] == 0          # first failure only
        assert w.tick(62) == "restart-failed" and state["ups"] == 1  # second in a row
        assert w.tick(93) == "fail" and w.tick(124) == "backoff" and state["ups"] == 1
        assert w.tick(155) == "backoff" and msgs.count("watchdog: tunnel still down, backing off") == 1
        state["result"] = True
        assert w.tick(200) == "restarted" and state["ups"] == 2 or w.tick(231) == "restarted"
        state["up"] = True
        assert w.tick(500) == "ok" and w.failures == 0
        (d / "broker.log").write_text("x" * (LOG_LIMIT + 10))
        log(d, "trim")
        assert (d / "broker.log").stat().st_size < LOG_LIMIT
        script = startup_script()
        assert script.startswith('@start ""') and "env_broker.py" in script and " -I " in script
        assert startup_file().name == TASK_NAME + ".cmd"
    print("selftest ok (validation allowlist, shot path rules, gate arg rules, once-run with stub, lock, log trim, startup script, watchdog rules)")


def main() -> int:
    ap = argparse.ArgumentParser(description="Runs allowlisted env.py actions on the interactive user desktop.")
    ap.add_argument("--once", action="store_true", help="process pending requests then exit")
    ap.add_argument("--install", action="store_true", help="add a launcher to the user Startup folder and start the broker now")
    ap.add_argument("--uninstall", action="store_true", help="remove the Startup launcher")
    ap.add_argument("--status", action="store_true", help="task registered? broker alive?")
    ap.add_argument("--dry-run", action="store_true", help="with --install: print the launcher only")
    ap.add_argument("--no-watchdog", action="store_true", help="do not auto-restart the tunnel")
    ap.add_argument("--selftest", action="store_true")
    a = ap.parse_args()
    if a.selftest:
        selftest()
        return 0
    if a.install:
        target = startup_file()
        print(f"startup launcher: {target}")
        print(startup_script().strip())
        if a.dry_run:
            return 0
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(startup_script().encode("utf-8"))
        if not broker_alive():
            start_detached()
            print("broker started now (detached)")
        return 0
    if a.uninstall:
        try:
            startup_file().unlink()
            print("startup launcher removed (a running broker keeps running until logoff)")
        except FileNotFoundError:
            print("startup launcher not installed")
        return 0
    if a.status:
        print(f"task {TASK_NAME}: {'startup launcher installed' if task_exists() else 'no startup launcher'}; "
              f"broker: {'alive' if broker_alive() else 'not running'}")
        return 0
    return run_loop(BROKER_DIR, a.once, not a.no_watchdog)


if __name__ == "__main__":
    raise SystemExit(main())
