#!/usr/bin/env python3
"""Bring up, check and heal the always-on parity test environment (tunnel + official + Rust).

    python -I tools/parity_lab/env.py status|up|heal|down|restart-client
        [--json] [--dry-run] [--config PATH]

    python -I tools/parity_lab/env.py --selftest

Status checks each component and prints PASS/FAIL with a reason:
  tunnel   TCP connect to tunnel_local; ssh.exe processes whose command line contains
           ssh_target, with their parent process name (ssh_key/ssh_target are never printed).
  official process with ExecutablePath == official_exe; CDP /json/version on official_cdp_port;
           location.pathname + member-list-group-header texts (inline node using lib.mjs);
           mic/speaker from mic_state.mjs. Healthy = reachable + header "Online — 2" + both on.
  rust     process with ExecutablePath == rust_exe; visible top-level window (MainWindowHandle);
           probe_inspect user status == "Online". No window => state "hidden-desktop".

`up` starts only missing/unhealthy components in the order tunnel -> official -> rust, each
DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB (retried without
BREAKAWAY when the job object refuses it), stdio to DEVNULL, then waits (30 s / 90 s / 120 s).
A Rust client in state "hidden-desktop" is stopped and relaunched with the probe environment.

`heal` never restarts: a wrong official member header is fixed with lib.mjs navigate()
(Page.reload + reconnect, 110 s), a Rust status other than Online is waited for (60 s), then
the markdown route is opened (deep link + Escape).

`down` stops only our own processes: ssh.exe matching ssh_target, official by exact
official_exe, rust by exact rust_exe, and fluxer_voice_worker.exe in the rust folder. The
user's personal app under %LOCALAPPDATA%\\fluxer_desktop is guarded and never matched.

Config: --config, else env FLUXER_ENV_CONFIG, else <repo>/../Logs/fluxer-dev/env.json.
Keys (defaults in brackets): ssh_exe [ssh.exe], ssh_key, ssh_target, tunnel_local
[127.0.0.1:443], tunnel_remote, official_exe, official_cdp_port [9334], rust_exe
[dist/fluxer_client/fluxer_client.exe, repo-relative], rust_localappdata
[<repo>/../Logs/fluxer_test_profile], markdown_route [<precheck guild>/<precheck channel>].
ssh_key and ssh_target are masked in every output. --dry-run runs only read-only
process/TCP/HTTP checks (no node, probe, launch or kill) and prints the planned commands.
"""

from __future__ import annotations

import argparse
import dataclasses
import json
import os
import re
import shutil
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
MASK = "<masked>"
DEFAULT_CONFIG_REL = Path("..") / "Logs" / "fluxer-dev" / "env.json"
DEFAULT_TUNNEL_LOCAL = "127.0.0.1:443"
DEFAULT_CDP_PORT = 9334
DEFAULT_RUST_EXE = "dist/fluxer_client/fluxer_client.exe"
DEFAULT_ROUTE = "1572284491404148736/1572284491404148743"
RUST_STATUS_FLX = "app.user-area.user-area-inner.user-status"
MEMBER_ONLINE_RE = re.compile(r"Online\s*[—–-]\s*2")

TUNNEL_WAIT = 30
TUNNEL_HEAL_WAIT = 20
OFFICIAL_WAIT = 90
RUST_WAIT = 120
RUST_ONLINE_WAIT = 60
HEAL_NAVIGATE_MS = 110000
PS_TIMEOUT = 30
NODE_TIMEOUT = 30
NAVIGATE_TIMEOUT = 150
PROBE_TIMEOUT = 8.0
TCP_TIMEOUT = 3.0
HTTP_TIMEOUT = 3.0

PS_SNAPSHOT = (
    "[Console]::OutputEncoding=[System.Text.Encoding]::UTF8; "
    "Get-CimInstance Win32_Process | "
    "Select-Object ProcessId,ParentProcessId,Name,ExecutablePath,CommandLine | "
    "ConvertTo-Json -Compress -Depth 4"
)

DOM_SCRIPT = """
import {Cdp, pageWsUrl} from __LIB__;
const port = Number(process.env.PARITY_CDP_PORT || __PORT__);
const race = (promise, ms, what) => Promise.race([
  promise,
  new Promise((_, reject) => {
    const t = setTimeout(() => reject(new Error(what + ' timed out after ' + ms + 'ms')), ms);
    t.unref?.();
  }),
]);
let cdp = null;
try {
  const url = await race(pageWsUrl(port, 12), 10000, 'CDP endpoint on ' + port);
  cdp = new Cdp(url, {timeoutMs: 8000});
  await race(cdp.open(), 8000, 'CDP socket');
  const data = await cdp.eval(`({pathname: location.pathname, headers: [...document.querySelectorAll('[data-flx*=member-list-group-header]')].map((el) => String(el.textContent || '').trim())})`, 8000);
  console.log(JSON.stringify({ok: true, pathname: String((data && data.pathname) || ''), headers: (data && data.headers) || []}));
  try { cdp.ws.close(); } catch {}
} catch (err) {
  console.log(JSON.stringify({ok: false, error: String((err && err.message) || err)}));
  try { if (cdp) cdp.ws.close(); } catch {}
}
"""

NAVIGATE_SCRIPT = """
import {Cdp, pageWsUrl, navigate} from __LIB__;
const port = Number(process.env.PARITY_CDP_PORT || __PORT__);
const race = (promise, ms, what) => Promise.race([
  promise,
  new Promise((_, reject) => {
    const t = setTimeout(() => reject(new Error(what + ' timed out after ' + ms + 'ms')), ms);
    t.unref?.();
  }),
]);
let cdp = null;
try {
  const url = await race(pageWsUrl(port, 30), 15000, 'CDP endpoint on ' + port);
  cdp = new Cdp(url, {timeoutMs: 10000});
  await race(cdp.open(), 10000, 'CDP socket');
  await navigate(cdp, port, undefined, {overallMs: __OVERALL__});
  console.log(JSON.stringify({ok: true}));
  try { cdp.ws.close(); } catch {}
} catch (err) {
  console.log(JSON.stringify({ok: false, error: String((err && err.message) || err)}));
  try { if (cdp) cdp.ws.close(); } catch {}
  process.exitCode = 1;
}
"""


# ---------------------------------------------------------------- config

class ConfigError(RuntimeError):
    pass


def default_config_path() -> Path:
    return (REPO / DEFAULT_CONFIG_REL).resolve()


def resolve_config_path(explicit: str | None, environ: dict | None = None) -> Path:
    environ = os.environ if environ is None else environ
    raw = explicit or environ.get("FLUXER_ENV_CONFIG") or default_config_path()
    path = Path(os.path.expandvars(os.path.expanduser(str(raw))))
    return path if path.is_absolute() else (Path.cwd() / path).resolve()


def _resolve_path(value: object, base: Path) -> Path:
    path = Path(os.path.expandvars(os.path.expanduser(str(value))))
    return path if path.is_absolute() else (base / path).resolve()


def _default_localappdata() -> str:
    return os.environ.get("LOCALAPPDATA") or str(Path.home() / "AppData" / "Local")


PERSONAL_APP_DIR = "fluxer_desktop"


def is_personal_app_path(path: object, localappdata: str | Path | None = None) -> bool:
    """True for anything under %LOCALAPPDATA%\\fluxer_desktop (the user's personal app)."""
    text = str(path)
    if not text:
        return False
    parts = [part.lower() for part in Path(text.replace("/", "\\")).parts]
    if PERSONAL_APP_DIR in parts:
        return True
    root = Path(localappdata if localappdata is not None else _default_localappdata()) / PERSONAL_APP_DIR
    return os.path.normcase(os.path.normpath(text)).startswith(
        os.path.normcase(os.path.normpath(str(root))) + os.sep
    )


@dataclass(frozen=True)
class Config:
    path: Path
    ssh_exe: str
    ssh_key: str
    ssh_target: str
    tunnel_local: str
    tunnel_remote: str
    official_exe: Path
    official_cdp_port: int
    rust_exe: Path
    rust_localappdata: Path
    markdown_route: str

    def masked(self) -> dict:
        return {
            "path": str(self.path),
            "ssh_exe": self.ssh_exe,
            "ssh_key": MASK,
            "ssh_target": MASK,
            "tunnel_local": self.tunnel_local,
            "tunnel_remote": self.tunnel_remote,
            "official_exe": str(self.official_exe),
            "official_cdp_port": self.official_cdp_port,
            "rust_exe": str(self.rust_exe),
            "rust_localappdata": str(self.rust_localappdata),
            "markdown_route": self.markdown_route,
        }


def load_config(explicit: str | None = None, environ: dict | None = None) -> Config:
    path = resolve_config_path(explicit, environ)
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except OSError as exc:
        raise ConfigError(f"cannot read config {path}: {exc}") from exc
    except json.JSONDecodeError as exc:
        raise ConfigError(f"config {path} is not valid JSON: {exc}") from exc
    if not isinstance(raw, dict):
        raise ConfigError(f"config {path} must be a JSON object")

    missing = [key for key in ("ssh_key", "ssh_target", "tunnel_remote", "official_exe") if not raw.get(key)]
    if missing:
        raise ConfigError(f"config {path} is missing required key(s): {', '.join(missing)}")

    try:
        port = int(raw.get("official_cdp_port", DEFAULT_CDP_PORT))
    except (TypeError, ValueError) as exc:
        raise ConfigError("official_cdp_port must be an integer") from exc
    if not 0 < port < 65536:
        raise ConfigError("official_cdp_port must be 1..65535")

    tunnel_local = str(raw.get("tunnel_local") or DEFAULT_TUNNEL_LOCAL)
    try:
        split_host_port(tunnel_local)
    except (ValueError, ConfigError) as exc:
        raise ConfigError(f"tunnel_local must be HOST:PORT: {exc}") from exc

    official_exe = _resolve_path(raw["official_exe"], REPO)
    if is_personal_app_path(official_exe):
        raise ConfigError(
            "official_exe points at the personal app under %LOCALAPPDATA%\\fluxer_desktop; refusing"
        )

    rust_exe = _resolve_path(raw.get("rust_exe") or DEFAULT_RUST_EXE, REPO)
    if is_personal_app_path(rust_exe):
        raise ConfigError("rust_exe points at the personal app; refusing")

    return Config(
        path=path,
        ssh_exe=str(raw.get("ssh_exe") or "ssh.exe"),
        ssh_key=str(raw["ssh_key"]),
        ssh_target=str(raw["ssh_target"]),
        tunnel_local=tunnel_local,
        tunnel_remote=str(raw["tunnel_remote"]),
        official_exe=official_exe,
        official_cdp_port=port,
        rust_exe=rust_exe,
        rust_localappdata=_resolve_path(
            raw.get("rust_localappdata") or REPO.parent / "Logs" / "fluxer_test_profile", REPO
        ),
        markdown_route=str(raw.get("markdown_route") or DEFAULT_ROUTE),
    )


def redact(text: object, cfg: Config) -> str:
    out = str(text)
    for secret in (cfg.ssh_key, cfg.ssh_target):
        if secret:
            out = out.replace(secret, MASK)
    return out


def mask_command(cmd: list[str], cfg: Config) -> str:
    parts = []
    for part in cmd:
        text = redact(part, cfg)
        parts.append(f'"{text}"' if any(ch.isspace() for ch in text) else text)
    return " ".join(parts)


# ---------------------------------------------------------------- commands / env

def tunnel_command(cfg: Config) -> list[str]:
    return [
        cfg.ssh_exe,
        "-i", cfg.ssh_key,
        "-N",
        "-o", "ServerAliveInterval=30",
        "-o", "ServerAliveCountMax=6",
        "-o", "ExitOnForwardFailure=yes",
        "-L", f"{cfg.tunnel_local}:{cfg.tunnel_remote}",
        cfg.ssh_target,
    ]


def official_command(cfg: Config) -> list[str]:
    return [str(cfg.official_exe), f"--remote-debugging-port={cfg.official_cdp_port}"]


def official_env(cfg: Config, base: dict | None = None) -> dict:
    env = dict(os.environ if base is None else base)
    env["FLUXER_DISABLE_MODULE_UPDATES"] = "1"
    return env


def rust_command(cfg: Config) -> list[str]:
    return [str(cfg.rust_exe), "--no-os-integration"]


def rust_env(cfg: Config, base: dict | None = None) -> dict:
    env = dict(os.environ if base is None else base)
    env.pop("SSL_CERT_FILE", None)
    env["FLUXER_PARITY_PROBE"] = "1"
    env["LOCALAPPDATA"] = str(cfg.rust_localappdata)
    return env


def worker_exe(cfg: Config) -> Path:
    return cfg.rust_exe.parent / "fluxer_voice_worker.exe"


def route_url(route: str) -> str:
    route = route.strip()
    if route.startswith("fluxer://"):
        return route
    route = route.lstrip("/")
    if route.startswith("channels/"):
        route = route[len("channels/"):]
    return "fluxer://channels/" + route


def lib_url() -> str:
    return (HERE / "lib.mjs").resolve().as_uri()


def node_script_cmd(script: str, port: int) -> list[str]:
    body = script.replace("__LIB__", json.dumps(lib_url()))
    body = body.replace("__PORT__", str(port)).replace("__OVERALL__", str(HEAL_NAVIGATE_MS))
    return ["node", "--input-type=module", "-e", body]


def navigate_command(port: int) -> list[str]:
    return node_script_cmd(NAVIGATE_SCRIPT, port)


def detach_flags() -> int:
    if os.name != "nt":
        return 0
    flags = getattr(subprocess, "DETACHED_PROCESS", 0)
    flags |= getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0)
    flags |= getattr(subprocess, "CREATE_BREAKAWAY_FROM_JOB", 0)
    return flags


def launch_detached(cmd: list[str], cwd: Path | None = None, env: dict | None = None) -> int:
    """Starts a fully detached process and returns its pid."""
    kwargs = {
        "cwd": str(cwd) if cwd else None,
        "env": env,
        "stdin": subprocess.DEVNULL,
        "stdout": subprocess.DEVNULL,
        "stderr": subprocess.DEVNULL,
        "close_fds": True,
    }
    flags = detach_flags()
    if os.name == "nt":
        kwargs["creationflags"] = flags
        try:
            return subprocess.Popen(cmd, **kwargs).pid
        except OSError:
            kwargs["creationflags"] = flags & ~getattr(subprocess, "CREATE_BREAKAWAY_FROM_JOB", 0)
            return subprocess.Popen(cmd, **kwargs).pid
    kwargs["start_new_session"] = True
    return subprocess.Popen(cmd, **kwargs).pid


# ---------------------------------------------------------------- subprocess plumbing

def run(cmd: list[str], timeout: int, cwd: Path | None = None, env: dict | None = None) -> tuple[int | None, str, bool]:
    try:
        r = subprocess.run(
            cmd,
            cwd=str(cwd) if cwd else None,
            env=env,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
        )
        return r.returncode, (r.stdout or "") + (r.stderr or ""), False
    except subprocess.TimeoutExpired as exc:
        text = exc.stdout if isinstance(exc.stdout, str) else ""
        return None, text or "", True
    except OSError as exc:
        return None, f"could not start {cmd[0]}: {exc}", False


def powershell_exe() -> str:
    for name in ("powershell", "pwsh"):
        found = shutil.which(name)
        if found:
            return found
    return "powershell.exe"


def run_powershell(script: str, timeout: int = PS_TIMEOUT) -> str:
    _, text, _ = run([powershell_exe(), "-NoProfile", "-NonInteractive", "-Command", script], timeout)
    return text


def _load_json_loose(text: object) -> object | None:
    raw = str(text or "").strip()
    if not raw:
        return None
    try:
        return json.loads(raw)
    except json.JSONDecodeError:
        pass
    for index, char in enumerate(raw):
        if char in "{[":
            try:
                return json.loads(raw[index:])
            except json.JSONDecodeError:
                return None
    return None


def parse_process_snapshot(text: object) -> list[dict]:
    data = _load_json_loose(text)
    if isinstance(data, dict):
        data = [data]
    if not isinstance(data, list):
        return []
    out = []
    for item in data:
        if not isinstance(item, dict):
            continue
        pid = _to_int(item.get("ProcessId"))
        ppid = _to_int(item.get("ParentProcessId"))
        if pid is None:
            continue
        out.append({
            "pid": pid,
            "ppid": ppid or 0,
            "name": str(item.get("Name") or ""),
            "exe": str(item.get("ExecutablePath") or ""),
            "cmd": str(item.get("CommandLine") or ""),
        })
    return out


def _to_int(value: object) -> int | None:
    try:
        return int(value)  # type: ignore[arg-type]
    except (TypeError, ValueError):
        return None


def parse_window_handles(text: object) -> dict[int, int]:
    data = _load_json_loose(text)
    if isinstance(data, dict):
        data = [data]
    if not isinstance(data, list):
        return {}
    out: dict[int, int] = {}
    for item in data:
        if not isinstance(item, dict):
            continue
        pid = _to_int(item.get("Id"))
        handle = _to_int(item.get("MainWindowHandle"))
        if pid is not None:
            out[pid] = handle or 0
    return out


def parse_json_line(text: object) -> dict | None:
    for raw in reversed(str(text or "").splitlines()):
        raw = raw.strip()
        if not raw.startswith("{"):
            continue
        try:
            data = json.loads(raw)
        except json.JSONDecodeError:
            continue
        if isinstance(data, dict):
            return data
    return None


def member_header_match(headers: object) -> str | None:
    if not isinstance(headers, list):
        return None
    for header in headers:
        text = str(header or "").strip()
        if MEMBER_ONLINE_RE.search(text):
            return text
    return None


def rust_status_from_inspect(reply: object) -> tuple[str | None, str | None]:
    if not isinstance(reply, dict):
        return None, "no reply"
    if reply.get("ok") is False:
        return None, str(reply.get("error") or "probe_inspect failed")
    for element in reply.get("elements") or []:
        if isinstance(element, dict) and element.get("flx") == RUST_STATUS_FLX:
            return (None if element.get("text") is None else str(element.get("text"))), None
    return None, "user-status element missing from probe_inspect"


def split_host_port(value: str) -> tuple[str, int]:
    host, sep, port = str(value).rpartition(":")
    if not sep or not host:
        raise ConfigError(f"not HOST:PORT: {value!r}")
    try:
        number = int(port)
    except ValueError as exc:
        raise ConfigError(f"bad port in {value!r}") from exc
    return host.strip("[]"), number


# ---------------------------------------------------------------- process matching

def _norm(path: object) -> str:
    # realpath resolves directory junctions (the repo may live on another drive behind
    # a junction), so C:\...\fluxer\x.exe and E:\...\fluxer\x.exe compare equal.
    try:
        resolved = os.path.realpath(str(path))
    except (OSError, ValueError):
        resolved = str(path)
    return os.path.normcase(os.path.normpath(resolved))


def rust_exes(cfg: "Config") -> list[Path]:
    """Configured dist exe plus the dev build that cycle.py (non --release) launches."""
    return [cfg.rust_exe, REPO / "target" / "fc-root" / "debug" / "fluxer_client.exe"]


def matches_rust(proc: dict, cfg: "Config") -> bool:
    return any(matches_exe(proc, exe) for exe in rust_exes(cfg))


def matches_exe(proc: dict, exe: Path) -> bool:
    candidate = str(proc.get("exe") or "")
    if not candidate or is_personal_app_path(candidate):
        return False
    return _norm(candidate) == _norm(exe)


def matches_ssh(proc: dict, ssh_target: str) -> bool:
    if not ssh_target:
        return False
    if not str(proc.get("name") or "").lower().endswith("ssh.exe"):
        return False
    return ssh_target in str(proc.get("cmd") or "")


def find_targets(cfg: Config, procs: list[dict]) -> list[dict]:
    """Our processes only: ssh matching the target, official/rust by exact path, voice worker."""
    out = []
    for proc in procs:
        if not proc.get("pid"):
            continue
        kind = None
        if matches_ssh(proc, cfg.ssh_target):
            kind = "ssh"
        elif matches_exe(proc, cfg.official_exe):
            kind = "official"
        elif matches_rust(proc, cfg):
            kind = "rust"
        elif matches_exe(proc, worker_exe(cfg)):
            kind = "voice-worker"
        if kind:
            out.append({"kind": kind, "pid": proc["pid"], "name": proc.get("name") or ""})
    return out


def process_snapshot() -> list[dict]:
    return parse_process_snapshot(run_powershell(PS_SNAPSHOT))


def window_handles(pids: list[int]) -> dict[int, int]:
    pids = [pid for pid in pids if pid]
    if not pids:
        return {}
    script = (
        "[Console]::OutputEncoding=[System.Text.Encoding]::UTF8; "
        f"Get-Process -Id @({','.join(str(pid) for pid in pids)}) -ErrorAction SilentlyContinue | "
        "Select-Object Id,MainWindowHandle | ConvertTo-Json -Compress"
    )
    return parse_window_handles(run_powershell(script))


def kill_pids(pids: list[int], wait: float = 10.0) -> tuple[list[int], list[int]]:
    """taskkill /F each pid; returns (killed, still_running)."""
    todo = [pid for pid in pids if pid]
    if not todo:
        return [], []
    for pid in todo:
        try:
            subprocess.run(["taskkill", "/F", "/PID", str(pid)], capture_output=True, timeout=20)
        except (OSError, subprocess.TimeoutExpired):
            pass
    deadline = time.time() + wait
    remaining = todo
    while remaining and time.time() < deadline:
        alive = {proc["pid"] for proc in process_snapshot()}
        remaining = [pid for pid in remaining if pid in alive]
        if not remaining:
            break
        time.sleep(0.4)
    return [pid for pid in todo if pid not in remaining], remaining


# ---------------------------------------------------------------- health checks

@dataclass
class Check:
    name: str
    state: str
    ok: bool
    reason: str
    details: dict = field(default_factory=dict)


def clip(text: object, limit: int = 110) -> str:
    flat = " ".join(str(text).split())
    return flat if len(flat) <= limit else flat[: limit - 1] + "…"


def tcp_ok(host_port: str, timeout: float = TCP_TIMEOUT) -> bool:
    host, port = split_host_port(host_port)
    try:
        with socket.create_connection((host, port), timeout):
            return True
    except OSError:
        return False


def cdp_version(port: int, timeout: float = HTTP_TIMEOUT) -> dict | None:
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    try:
        with opener.open(f"http://127.0.0.1:{port}/json/version", timeout=timeout) as response:
            data = json.loads(response.read().decode("utf-8", "replace"))
    except (OSError, urllib.error.URLError, json.JSONDecodeError, ValueError):
        return None
    return data if isinstance(data, dict) else None


def check_tunnel(cfg: Config) -> Check:
    if not tcp_ok(cfg.tunnel_local):
        return Check("tunnel", "down", False, f"TCP {cfg.tunnel_local} unreachable")
    procs = process_snapshot()
    by_pid = {proc["pid"]: proc for proc in procs}
    ssh = [proc for proc in procs if matches_ssh(proc, cfg.ssh_target)]
    details = {
        "ssh": [
            {"pid": proc["pid"], "parent": (by_pid.get(proc["ppid"]) or {}).get("name") or "unknown"}
            for proc in ssh
        ]
    }
    if ssh:
        listed = ", ".join(f"pid={item['pid']} (parent {item['parent']})" for item in details["ssh"])
        return Check("tunnel", "pass", True, f"TCP {cfg.tunnel_local} ok; ssh.exe {listed}", details)
    return Check(
        "tunnel", "pass", True, f"TCP {cfg.tunnel_local} ok; no ssh.exe process matching the target", details
    )


def official_dom(port: int, timeout: int = NODE_TIMEOUT) -> dict:
    env = dict(os.environ, PARITY_CDP_PORT=str(port))
    _, text, timed_out = run(node_script_cmd(DOM_SCRIPT, port), timeout, cwd=REPO, env=env)
    if timed_out:
        return {"ok": False, "error": f"node timed out after {timeout}s"}
    data = parse_json_line(text)
    return data if data is not None else {"ok": False, "error": "node printed no JSON"}


def mic_state(port: int, timeout: int = NODE_TIMEOUT) -> dict | None:
    _, text, timed_out = run(["node", str(HERE / "mic_state.mjs"), "--attach", str(port)], timeout, cwd=REPO)
    if timed_out:
        return None
    return parse_json_line(text)


def check_official(cfg: Config, level: int = 2) -> Check:
    procs = process_snapshot()
    matched = [proc for proc in procs if matches_exe(proc, cfg.official_exe)]
    details: dict = {"pids": [proc["pid"] for proc in matched]}
    if not matched:
        return Check("official", "down", False, "not running (no process with ExecutablePath == official_exe)", details)
    if cdp_version(cfg.official_cdp_port) is None:
        return Check(
            "official", "unreachable", False,
            f"CDP http://127.0.0.1:{cfg.official_cdp_port}/json/version unreachable", details,
        )
    if level < 2:
        return Check("official", "running", True, "process running, CDP reachable (content not checked)", details)

    port = cfg.official_cdp_port
    dom = official_dom(port)
    if not dom.get("ok"):
        return Check("official", "unhealthy", False, f"CDP page read failed: {dom.get('error', 'unknown')}", details)
    headers = dom.get("headers") if isinstance(dom.get("headers"), list) else []
    pathname = str(dom.get("pathname") or "")
    details.update({"pathname": pathname, "member_headers": headers})
    header = member_header_match(headers)
    state = mic_state(port)
    fails: list[str] = []
    if header is None:
        fails.append(f"member-list-group-header not 'Online — 2' (got {headers!r})")
    if state is None:
        fails.append("mic_state.mjs returned no JSON")
    else:
        details["mic_on"] = bool(state.get("mic_on"))
        details["speaker_on"] = bool(state.get("speaker_on"))
        if not state.get("mic_on"):
            fails.append("mic off")
        if not state.get("speaker_on"):
            fails.append("speaker off")
    where = f"pathname {pathname or '/'}"
    if fails:
        return Check("official", "unhealthy", False, f"{where}: " + "; ".join(fails), details)
    return Check(
        "official", "pass", True,
        f"{where}, header {header!r}, mic on, speaker on", details,
    )


def rust_user_status() -> tuple[str | None, str | None]:
    try:
        sys.path.insert(0, str(HERE))
        import rust_probe  # noqa: PLC0415  (reads the IPC file only when called)

        reply = rust_probe.send("probe_inspect", timeout=PROBE_TIMEOUT)
    except Exception as exc:  # noqa: BLE001  (transport failures become the reason)
        return None, f"{type(exc).__name__}: {exc}"
    return rust_status_from_inspect(reply)


def check_rust(cfg: Config, level: int = 2) -> Check:
    procs = process_snapshot()
    matched = [proc for proc in procs if matches_rust(proc, cfg)]
    if not matched:
        return Check("rust", "down", False, "not running (no process with ExecutablePath == rust_exe or the target/fc-root debug exe)")
    pids = [proc["pid"] for proc in matched]
    handles = window_handles(pids)
    visible = [pid for pid in pids if handles.get(pid, 0)]
    details: dict = {"pids": pids, "visible_pids": visible}
    if not visible:
        return Check(
            "rust", "hidden-desktop", False,
            f"pid(s) {pids} have no visible top-level window (MainWindowHandle == 0)", details,
        )
    if level < 1:
        return Check("rust", "running", True, f"pid(s) {visible} visible (probe not checked)", details)
    status, error = rust_user_status()
    details["user_status"] = status
    if error:
        return Check("rust", "probe-unreachable", False, f"parity probe unreachable: {error}", details)
    if status != "Online":
        return Check("rust", "unhealthy", False, f"user status {status!r} != 'Online'", details)
    return Check("rust", "pass", True, f"pid(s) {visible} visible, user status 'Online'", details)


def collect_status(cfg: Config, level: int = 2) -> dict[str, Check]:
    return {
        "tunnel": check_tunnel(cfg),
        "official": check_official(cfg, level),
        "rust": check_rust(cfg, level),
    }


def wait_until(fn, timeout: float, interval: float = 2.0):
    deadline = time.time() + timeout
    while True:
        try:
            check = fn()
        except Exception as exc:  # noqa: BLE001  (keep waiting on transient errors)
            check = Check("wait", "error", False, f"{type(exc).__name__}: {exc}")
        if getattr(check, "ok", False) or time.time() >= deadline:
            return check
        time.sleep(interval)


# ---------------------------------------------------------------- output

def emit(checks: dict[str, Check] | None, actions: list[str], json_mode: bool, cfg: Config | None = None) -> int:
    checks = checks or {}
    failures = [check for check in checks.values() if not check.ok]
    if json_mode:
        payload = {
            "ok": not failures,
            "actions": [redact(action, cfg) if cfg else action for action in actions],
            "components": {
                name: {
                    "state": check.state,
                    "ok": check.ok,
                    "reason": redact(check.reason, cfg) if cfg else check.reason,
                    "details": json.loads(redact(json.dumps(check.details, ensure_ascii=False), cfg))
                    if cfg
                    else check.details,
                }
                for name, check in checks.items()
            },
        }
        print(json.dumps(payload, ensure_ascii=False, indent=1))
        return 1 if failures else 0

    for action in actions:
        print(f"- {redact(action, cfg) if cfg else action}")
    if checks:
        print(f"{'component':<10}  {'result':<5}  reason")
        for name, check in checks.items():
            reason = redact(check.reason, cfg) if cfg else check.reason
            print(f"{name:<10}  {'PASS' if check.ok else 'FAIL':<5}  {clip(reason)}")
        passed = len(checks) - len(failures)
        print(f"env: {passed} PASS, {len(failures)} FAIL -> exit {1 if failures else 0}")
    return 1 if failures else 0


# ---------------------------------------------------------------- commands

def cmd_status(cfg: Config, args: argparse.Namespace) -> int:
    checks = collect_status(cfg, 0 if args.dry_run else 2)
    return emit(checks, [], args.json, cfg)


def _start_rust(cfg: Config, actions: list[str]) -> Check:
    pid = launch_detached(rust_command(cfg), cwd=cfg.rust_exe.parent, env=rust_env(cfg))
    actions.append(f"rust: started pid={pid} (cwd {cfg.rust_exe.parent}, FLUXER_PARITY_PROBE=1)")
    return wait_until(lambda: check_rust(cfg, 1), RUST_WAIT)


def _start_tunnel(cfg: Config, actions: list[str], reason: str, wait: float = TUNNEL_WAIT) -> Check:
    """Start the tunnel ssh (same path as `up`) and wait for the local TCP port."""
    tunnel_cmd = tunnel_command(cfg)
    actions.append(f"tunnel: start ({reason})")
    pid = launch_detached(tunnel_cmd, cwd=REPO)
    actions.append(f"tunnel: pid={pid} {mask_command(tunnel_cmd, cfg)}")
    tunnel = wait_until(lambda: check_tunnel(cfg), wait)
    actions.append(f"tunnel: {'ready' if tunnel.ok else 'NOT ready'} ({tunnel.reason})")
    return tunnel


def cmd_up(cfg: Config, args: argparse.Namespace) -> int:
    dry = args.dry_run
    actions: list[str] = []
    checks = collect_status(cfg, 0)
    if dry:
        actions.append(f"check (read-only): {', '.join(f'{name}={check.state}' for name, check in checks.items())}")

    # tunnel
    tunnel = checks["tunnel"]
    tunnel_cmd = tunnel_command(cfg)
    if tunnel.ok:
        actions.append(f"tunnel: up, no action ({tunnel.reason})")
    elif dry:
        actions.append(f"tunnel: would start ({tunnel.reason}): {mask_command(tunnel_cmd, cfg)}")
    else:
        tunnel = _start_tunnel(cfg, actions, tunnel.reason)

    # official
    official = checks["official"]
    official_cmd = official_command(cfg)
    start = None
    if official.state == "down":
        start = "not running"
    elif official.state == "unreachable":
        start = "CDP unreachable"
        if not dry:
            procs = [p for p in process_snapshot() if matches_exe(p, cfg.official_exe)]
            killed, remaining = kill_pids([p["pid"] for p in procs])
            actions.append(f"official: stopped pid(s) {killed} before relaunch" + (f", still {remaining}" if remaining else ""))
    elif not official.ok:
        actions.append(f"official: running but unhealthy ({official.reason}); heal fixes content, not restarting")
    else:
        actions.append(f"official: healthy, no action ({official.reason})")

    if start:
        if dry:
            actions.append(f"official: would start ({start}): {mask_command(official_cmd, cfg)}")
        else:
            actions.append(f"official: start ({start})")
            pid = launch_detached(official_cmd, cwd=cfg.official_exe.parent, env=official_env(cfg))
            actions.append(f"official: pid={pid} {mask_command(official_cmd, cfg)}")
            official = wait_until(lambda: check_official(cfg, 0), OFFICIAL_WAIT)
            actions.append(f"official: {'ready' if official.ok else 'NOT ready'} ({official.reason})")

    # rust
    rust = checks["rust"]
    rust_cmd = rust_command(cfg)
    if rust.ok:
        actions.append(f"rust: running, no action ({rust.reason})")
    if dry:
        if not rust.ok:
            verb = "relaunch" if rust.state == "hidden-desktop" else "start"
            actions.append(
                f"rust: would {verb} ({rust.reason}): {mask_command(rust_cmd, cfg)} "
                f"(cwd {cfg.rust_exe.parent}, FLUXER_PARITY_PROBE=1, LOCALAPPDATA={cfg.rust_localappdata})"
            )
    else:
        if rust.state == "hidden-desktop":
            procs = [p for p in process_snapshot() if matches_exe(p, cfg.rust_exe)]
            workers = [p for p in process_snapshot() if matches_exe(p, worker_exe(cfg))]
            killed, remaining = kill_pids([p["pid"] for p in procs + workers])
            actions.append(f"rust: stopped hidden-desktop pid(s) {killed}" + (f", still {remaining}" if remaining else ""))
            rust = Check("rust", "down", False, "stopped before relaunch")
        elif rust.state == "unhealthy":
            actions.append(f"rust: visible but not Online ({rust.reason}); waiting up to {RUST_WAIT}s")
            rust = wait_until(lambda: check_rust(cfg, 1), RUST_WAIT)
            actions.append(f"rust: after wait {rust.state} ({rust.reason})")
        if rust.state in ("down", "probe-unreachable"):
            if rust.state == "probe-unreachable":
                procs = [p for p in process_snapshot() if matches_exe(p, cfg.rust_exe)]
                killed, remaining = kill_pids([p["pid"] for p in procs])
                actions.append(
                    f"rust: probe unreachable, stopped pid(s) {killed}"
                    + (f", still {remaining}" if remaining else "")
                )
            rust = _start_rust(cfg, actions)
            actions.append(f"rust: {'Online' if rust.ok else 'NOT Online'} ({rust.reason})")

    final = checks if dry else collect_status(cfg, 2)
    if dry:
        actions.append("dry-run: nothing was started, stopped or reloaded")
    return emit(final, actions, args.json, cfg)


def _rust_deep_link(url: str) -> dict:
    sys.path.insert(0, str(HERE))
    import rust_probe  # noqa: PLC0415

    return rust_probe.deep_link(url)


def _rust_key_escape() -> dict:
    sys.path.insert(0, str(HERE))
    import rust_probe  # noqa: PLC0415

    return rust_probe.send("probe_key", keystroke="escape")


def cmd_heal(cfg: Config, args: argparse.Namespace) -> int:
    dry = args.dry_run
    actions: list[str] = []
    checks = collect_status(cfg, 0 if dry else 2)
    nav_cmd = navigate_command(cfg.official_cdp_port)
    url = route_url(cfg.markdown_route)

    if dry and not checks["tunnel"].ok:
        actions.append(f"tunnel: would restart first ({checks['tunnel'].reason}): "
                       f"{mask_command(tunnel_command(cfg), cfg)}; wait up to {TUNNEL_HEAL_WAIT}s for TCP")
    if not dry and not checks["tunnel"].ok:
        tunnel = _start_tunnel(cfg, actions, checks["tunnel"].reason, TUNNEL_HEAL_WAIT)
        if tunnel.ok:
            # the Rust client needs a few seconds to leave "Reconnecting..."
            actions.append(f"rust: waiting up to {RUST_ONLINE_WAIT}s for Online after tunnel restart")
            wait_until(lambda: check_rust(cfg, 1), RUST_ONLINE_WAIT, interval=3.0)
        checks = collect_status(cfg, 2)

    official = checks["official"]
    rust = checks["rust"]

    if dry:
        actions.append(
            f"official: would ensure header 'Online — 2' ({official.state}: {official.reason}); "
            f"if wrong run {mask_command(nav_cmd, cfg)[:160]}…"
        )
        actions.append(f"rust: would ensure user status 'Online' ({rust.state}: {rust.reason}); then link {url} + key escape")
        actions.append("dry-run: nothing was started, stopped or reloaded")
        return emit(checks, actions, args.json, cfg)

    if official.state == "down":
        actions.append("official: down; cannot heal (run up)")
    elif official.state == "unreachable":
        actions.append("official: CDP unreachable; cannot heal (run up)")
    elif official.ok:
        actions.append("official: header, mic and speaker already healthy")
    else:
        header = member_header_match(official.details.get("member_headers") or [])
        if header is not None:
            actions.append(f"official: header ok but unhealthy ({official.reason}); heal does not click controls")
        else:
            code, text, timed_out = run(nav_cmd, NAVIGATE_TIMEOUT, cwd=REPO, env=dict(os.environ, PARITY_CDP_PORT=str(cfg.official_cdp_port)))
            detail = clip(parse_json_line(text) or {}, 80) if not timed_out else "timeout"
            actions.append(f"official: CDP navigate(Page.reload) exit={code}{' timeout' if timed_out else ''} {detail}")
            official = check_official(cfg, 2)
            actions.append(f"official: after reload {official.state} ({official.reason})")
            if not official.ok and member_header_match(official.details.get("member_headers") or []) is None \
                    and rust.state != "down":
                # The server can hold the Rust user offline (stale presence) while Rust
                # itself looks Online; a fresh Rust gateway session re-publishes it.
                targets = [t for t in find_targets(cfg, process_snapshot()) if t["kind"] in ("rust", "voice-worker")]
                killed, _ = kill_pids([t["pid"] for t in targets])
                actions.append(f"official: still not 'Online — 2'; restarting rust client {killed} to re-publish presence")
                rust = _start_rust(cfg, actions)
                official = wait_until(lambda: check_official(cfg, 2), 30, interval=3.0)
                actions.append(f"official: after rust restart {official.state} ({official.reason})")
                if not official.ok:
                    actions.append("official: still offline; if this persists restart the test server's "
                                   "gateway container (docs/PARITY_LAB.md 10.6) and run heal again")

    if rust.state == "down":
        actions.append("rust: down; cannot heal (run up)")
    else:
        if not rust.ok:
            actions.append(f"rust: waiting up to {RUST_ONLINE_WAIT}s for user status 'Online' ({rust.reason})")
            rust = wait_until(lambda: check_rust(cfg, 1), RUST_ONLINE_WAIT, interval=3.0)
            actions.append(f"rust: after wait {rust.state} ({rust.reason})")
        try:
            reply = _rust_deep_link(url)
            actions.append(f"rust: link {url} -> {reply}")
        except Exception as exc:  # noqa: BLE001
            actions.append(f"rust: link {url} failed: {type(exc).__name__}: {exc}")
        try:
            reply = _rust_key_escape()
            actions.append(f"rust: key escape -> {reply}")
        except Exception as exc:  # noqa: BLE001
            actions.append(f"rust: key escape failed: {type(exc).__name__}: {exc}")
        time.sleep(1.5)
        rust = check_rust(cfg, 2)
        actions.append(f"rust: now {rust.state} ({rust.reason})")

    checks = collect_status(cfg, 2)
    return emit(checks, actions, args.json, cfg)


def cmd_down(cfg: Config, args: argparse.Namespace) -> int:
    actions: list[str] = []
    procs = process_snapshot()
    targets = find_targets(cfg, procs)
    if not targets:
        actions.append("nothing to stop (no matching ssh/official/rust/voice-worker process)")
    for kind in ("ssh", "official", "rust", "voice-worker"):
        pids = [target["pid"] for target in targets if target["kind"] == kind]
        if not pids:
            continue
        label = f"{kind} pid(s) {pids}"
        if args.dry_run:
            actions.append(f"would stop {label}")
            continue
        killed, remaining = kill_pids(pids)
        actions.append(f"stopped {label}" + (f"; still running {remaining}" if remaining else ""))
    if args.dry_run:
        actions.append("dry-run: nothing was stopped")
        return emit({}, actions, args.json, cfg)
    leftover = [target for target in find_targets(cfg, process_snapshot())]
    if leftover:
        names = ", ".join(f"{target['kind']} pid={target['pid']}" for target in leftover)
        actions.append(f"still matching after stop: {names}")
        check = Check("stop", "leftover", False, f"{len(leftover)} process(es) still running")
        return emit({"stop": check}, actions, args.json, cfg)
    return emit({}, actions, args.json, cfg)


def cmd_restart_client(cfg: Config, args: argparse.Namespace) -> int:
    actions: list[str] = []
    kill_cfg = cfg  # stop any running client (dist or debug) before launching the requested one
    if getattr(args, "dev", False):
        cfg = dataclasses.replace(cfg, rust_exe=(REPO / "target" / "fc-root" / "debug" / "fluxer_client.exe").resolve())
    procs = process_snapshot()
    targets = [t for t in find_targets(kill_cfg, procs) if t["kind"] in ("rust", "voice-worker")]
    if args.dry_run:
        pids = [target["pid"] for target in targets]
        actions.append(f"would stop rust client pid(s) {pids}" if pids else "rust client not running")
        actions.append(f"would start: {mask_command(rust_command(cfg), cfg)} (cwd {cfg.rust_exe.parent})")
        actions.append("dry-run: nothing was started or stopped")
        return emit({}, actions, args.json, cfg)
    if targets:
        killed, remaining = kill_pids([target["pid"] for target in targets])
        actions.append(f"stopped rust client pid(s) {killed}" + (f"; still running {remaining}" if remaining else ""))
    else:
        actions.append("rust client was not running")
    rust = _start_rust(cfg, actions)
    checks = {"rust": rust}
    final = checks if rust.ok else {"rust": check_rust(cfg, 2)}
    actions.append(f"rust: {'Online' if final['rust'].ok else 'NOT Online'} ({final['rust'].reason})")
    return emit(final, actions, args.json, cfg)


# ---------------------------------------------------------------- interactive desktop / broker

BROKER_DIR = REPO / "target" / "parity-tmp" / "env-broker"
BROKER_ACTIONS = ("restart-client", "up", "heal")
BROKER_ACK_WAIT = 15.0
BROKER_RESPONSE_WAIT = 180.0
BROKER_RESPONSE_WAITS = {"gate": 930.0}
BROKER_MISSING = ("no env broker on the user desktop; ask the user to run "
                  "`python -I tools/parity_lab/env_broker.py --install` once")


def on_interactive_desktop() -> bool:
    """True when this thread runs on the user desktop (window station WinSta0, desktop Default)."""
    if os.name != "nt":
        return True
    try:
        import ctypes
        from ctypes import wintypes
        user32 = ctypes.WinDLL("user32", use_last_error=True)
        kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        user32.GetThreadDesktop.restype = wintypes.HANDLE
        user32.GetThreadDesktop.argtypes = [wintypes.DWORD]
        user32.GetProcessWindowStation.restype = wintypes.HANDLE
        user32.GetUserObjectInformationW.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p,
                                                     wintypes.DWORD, ctypes.POINTER(wintypes.DWORD)]
        user32.GetUserObjectInformationW.restype = wintypes.BOOL

        def name_of(handle) -> str | None:
            if not handle:
                return None
            buf = ctypes.create_unicode_buffer(256)
            need = wintypes.DWORD(0)
            if not user32.GetUserObjectInformationW(handle, 2, buf, ctypes.sizeof(buf), ctypes.byref(need)):  # UOI_NAME
                return None
            return buf.value

        desktop = name_of(user32.GetThreadDesktop(kernel32.GetCurrentThreadId()))
        station = name_of(user32.GetProcessWindowStation())
        return desktop == "Default" and station == "WinSta0"
    except (OSError, AttributeError, ValueError):
        return False


def needs_broker(command: str | None, dry_run: bool, environ: dict | None = None, interactive: bool | None = None) -> bool:
    if command not in BROKER_ACTIONS or dry_run:
        return False
    environ = os.environ if environ is None else environ
    if environ.get("FLUXER_ENV_FORCE_BROKER") == "1":
        return True
    return not (on_interactive_desktop() if interactive is None else interactive)


def broker_args(args: argparse.Namespace) -> list[str]:
    out: list[str] = []
    if args.json:
        out.append("--json")
    if args.config:
        out += ["--config", str(args.config)]
    if getattr(args, "dev", False):
        out.append("--dev")
    return out


def submit_broker_request(action: str, extra: list[str], broker_dir: Path = BROKER_DIR,
                          ack_wait: float = BROKER_ACK_WAIT, response_wait: float | None = None,
                          poll: float = 0.5) -> tuple[int, str]:
    """Writes a request, waits for the broker; returns (exit_code, output)."""
    import uuid
    if response_wait is None:
        response_wait = BROKER_RESPONSE_WAITS.get(action, BROKER_RESPONSE_WAIT)
    req_dir = broker_dir / "requests"
    resp_dir = broker_dir / "responses"
    req_dir.mkdir(parents=True, exist_ok=True)
    resp_dir.mkdir(parents=True, exist_ok=True)
    rid = uuid.uuid4().hex
    req = req_dir / f"{rid}.json"
    resp = resp_dir / f"{rid}.json"
    tmp = req_dir / f"{rid}.tmp"
    tmp.write_text(json.dumps({"id": rid, "action": action, "args": extra, "created": time.time()}), encoding="utf-8")
    os.replace(tmp, req)
    start = time.time()
    while time.time() - start < response_wait:
        if resp.exists():
            try:
                data = json.loads(resp.read_text(encoding="utf-8"))
            except (OSError, ValueError):
                time.sleep(poll)
                continue
            try:
                resp.unlink()
            except OSError:
                pass
            return int(data.get("exit_code") or 0), str(data.get("output_tail") or "")
        # unclaimed request after ack_wait means nobody is listening (broker deletes it when it picks it up)
        if time.time() - start > ack_wait and req.exists():
            try:
                req.unlink()
            except OSError:
                pass
            return 3, BROKER_MISSING
        time.sleep(poll)
    return 4, "env broker did not finish within the wait limit"


# ---------------------------------------------------------------- selftest

def selftest() -> None:
    import tempfile

    target = "deploy@parity.example.test"
    key = r"C:\Users\dev\.ssh\id_fluxer_ed25519"
    base = {
        "ssh_exe": "ssh.exe",
        "ssh_key": key,
        "ssh_target": target,
        "tunnel_remote": "127.0.0.1:22",
        "official_exe": r"D:\builds\official\Fluxer.exe",
    }
    with tempfile.TemporaryDirectory() as tmp:
        config_path = Path(tmp) / "env.json"
        config_path.write_text(json.dumps(base), encoding="utf-8")
        cfg = load_config(str(config_path), environ={})

        assert cfg.tunnel_local == DEFAULT_TUNNEL_LOCAL
        assert cfg.ssh_exe == "ssh.exe" and cfg.official_cdp_port == DEFAULT_CDP_PORT
        assert cfg.rust_exe == (REPO / DEFAULT_RUST_EXE).resolve()
        assert cfg.markdown_route == DEFAULT_ROUTE
        assert cfg.rust_localappdata == (REPO.parent / "Logs" / "fluxer_test_profile").resolve()

        masked = json.dumps(cfg.masked(), ensure_ascii=False)
        assert key not in masked and target not in masked, masked
        assert cfg.masked()["ssh_key"] == MASK and cfg.masked()["ssh_target"] == MASK
        tunnel_cmd = tunnel_command(cfg)
        rendered = mask_command(tunnel_cmd, cfg)
        assert key not in rendered and target not in rendered and MASK in rendered, rendered
        assert redact(f"key={key} target={target}", cfg) == f"key={MASK} target={MASK}"

        # heal: tunnel down plans/performs a tunnel restart first (stubs, no real processes)
        g = globals()
        saved = {k: g[k] for k in ("collect_status", "emit", "launch_detached", "wait_until", "check_rust", "check_tunnel", "_rust_deep_link", "_rust_key_escape")}
        calls: list[str] = []
        down = Check("tunnel", "down", False, "TCP unreachable")
        okc = Check("x", "pass", True, "ok")
        try:
            g["collect_status"] = lambda c, level=2: {"tunnel": down, "official": okc, "rust": okc}
            g["emit"] = lambda checks, actions, js, c=None: (calls.extend(actions), 0)[1]
            g["launch_detached"] = lambda *a, **k: calls.append("LAUNCH") or 1
            g["wait_until"] = lambda fn, timeout, interval=2.0: okc
            g["check_rust"] = lambda c, level=2: okc
            ns = argparse.Namespace(dry_run=True, json=False)
            cmd_heal(cfg, ns)
            assert any("tunnel: would restart first" in x for x in calls) and "LAUNCH" not in calls, calls
            calls.clear()
            g["_rust_deep_link"] = lambda url: {}
            g["_rust_key_escape"] = lambda: {}
            g["time"] = type("T", (), {"sleep": staticmethod(lambda s: None), "time": time.time})
            cmd_heal(cfg, argparse.Namespace(dry_run=False, json=False))
            assert "LAUNCH" in calls and any(x.startswith("tunnel: start") for x in calls), calls
        finally:
            g.update(saved)
            g["time"] = time
        # config discovery
        assert resolve_config_path(None, {"FLUXER_ENV_CONFIG": str(config_path)}) == config_path.resolve()
        assert resolve_config_path(None, {}) == default_config_path()
        assert default_config_path() == (REPO / ".." / "Logs" / "fluxer-dev" / "env.json").resolve()
        for bad in (
            {**base, "official_exe": r"C:\Users\d\AppData\Local\fluxer_desktop\Fluxer.exe"},
            {k: v for k, v in base.items() if k != "ssh_target"},
            {**base, "official_cdp_port": "nope"},
            {**base, "tunnel_local": "127.0.0.1"},
        ):
            config_path.write_text(json.dumps(bad), encoding="utf-8")
            try:
                load_config(str(config_path), environ={})
            except ConfigError:
                pass
            else:
                raise AssertionError(f"load_config must reject {sorted(bad)}")

        # personal-app guard
        personal = r"C:\Users\d\AppData\Local\fluxer_desktop\Fluxer.exe"
        assert is_personal_app_path(personal)
        assert not is_personal_app_path(r"D:\Fluxer.exe", localappdata=r"C:\Users\d\AppData\Local")
        assert is_personal_app_path(r"C:\Users\d\AppData\Local\fluxer_desktop")
        assert not is_personal_app_path(str(cfg.official_exe))
        assert not matches_exe({"exe": personal, "name": "Fluxer.exe"}, Path(personal))
        assert not matches_exe({"exe": r"D:\builds\official\Fluxer.exe"}, Path(personal))

        # command construction
        assert tunnel_cmd[0] == "ssh.exe"
        assert tunnel_cmd[1:3] == ["-i", key]
        assert tunnel_cmd[-3:] == ["-L", f"{DEFAULT_TUNNEL_LOCAL}:127.0.0.1:22", target], tunnel_cmd
        assert tunnel_cmd.count("-o") == 3 and "ExitOnForwardFailure=yes" in tunnel_cmd
        assert official_command(cfg) == [str(cfg.official_exe), "--remote-debugging-port=9334"]
        assert official_env(cfg, base={"PATH": "p"})["FLUXER_DISABLE_MODULE_UPDATES"] == "1"
        assert rust_command(cfg) == [str(cfg.rust_exe), "--no-os-integration"]
        rust_e = rust_env(cfg, base={"SSL_CERT_FILE": "x", "PATH": "p"})
        assert "SSL_CERT_FILE" not in rust_e
        assert rust_e["FLUXER_PARITY_PROBE"] == "1"
        assert rust_e["LOCALAPPDATA"] == str(cfg.rust_localappdata)
        assert worker_exe(cfg) == cfg.rust_exe.parent / "fluxer_voice_worker.exe"

        # down / hidden-desktop targeting never includes the personal app
        procs = parse_process_snapshot(json.dumps([
            {"ProcessId": 11, "ParentProcessId": 1, "Name": "ssh.exe",
             "ExecutablePath": r"C:\Windows\System32\OpenSSH\ssh.exe", "CommandLine": f"ssh -N {target} -L 127.0.0.1:443:127.0.0.1:22"},
            {"ProcessId": 12, "ParentProcessId": 1, "Name": "ssh.exe",
             "ExecutablePath": r"C:\Windows\System32\OpenSSH\ssh.exe", "CommandLine": "ssh other@elsewhere"},
            {"ProcessId": 13, "ParentProcessId": 1, "Name": "Fluxer.exe",
             "ExecutablePath": str(cfg.official_exe), "CommandLine": "Fluxer.exe --remote-debugging-port=9334"},
            {"ProcessId": 14, "ParentProcessId": 1, "Name": "fluxer_client.exe",
             "ExecutablePath": str(cfg.rust_exe), "CommandLine": "fluxer_client.exe --no-os-integration"},
            {"ProcessId": 15, "ParentProcessId": 14, "Name": "fluxer_voice_worker.exe",
             "ExecutablePath": str(worker_exe(cfg)), "CommandLine": "fluxer_voice_worker.exe"},
            {"ProcessId": 16, "ParentProcessId": 1, "Name": "Fluxer.exe",
             "ExecutablePath": personal, "CommandLine": "Fluxer.exe"},
        ]))
        found = {(item["kind"], item["pid"]) for item in find_targets(cfg, procs)}
        assert found == {("ssh", 11), ("official", 13), ("rust", 14), ("voice-worker", 15)}, found

        # health parsers
        assert member_header_match(["Offline — 0", "Online — 2"]) == "Online — 2"
        assert member_header_match(["Online – 2"]) == "Online – 2"
        assert member_header_match(["Online - 2"]) == "Online - 2"
        assert member_header_match(["Online — 1", ""]) is None
        assert member_header_match(None) is None
        assert parse_json_line('noise\n{"mic_on": true, "speaker_on": false}\n') == {"mic_on": True, "speaker_on": False}
        assert parse_json_line("no json") is None
        assert parse_json_line('{"ok": false, "error": "x"}')["ok"] is False
        assert parse_process_snapshot("") == []
        one = parse_process_snapshot(json.dumps({"ProcessId": 7, "ParentProcessId": 1, "Name": "x.exe"}))
        assert one[0]["pid"] == 7 and one[0]["ppid"] == 1 and one[0]["exe"] == ""
        assert parse_window_handles(json.dumps([{"Id": 3, "MainWindowHandle": 123}])) == {3: 123}
        assert parse_window_handles(json.dumps({"Id": 4, "MainWindowHandle": 0})) == {4: 0}
        assert parse_window_handles("[Console] noise") == {}
        status, error = rust_status_from_inspect({"ok": True, "elements": [{"flx": RUST_STATUS_FLX, "text": "Online"}]})
        assert (status, error) == ("Online", None)
        status, error = rust_status_from_inspect({"ok": False, "error": "no reply"})
        assert status is None and error == "no reply"
        assert split_host_port("127.0.0.1:443") == ("127.0.0.1", 443)
        try:
            split_host_port("127.0.0.1")
        except ConfigError:
            pass
        else:
            raise AssertionError("split_host_port must reject a missing port")

        # routes / navigate command
        assert route_url("1572/1573") == "fluxer://channels/1572/1573"
        assert route_url("/1572/1573") == "fluxer://channels/1572/1573"
        assert route_url("fluxer://channels/1/2") == "fluxer://channels/1/2"
        assert route_url("/channels/1572/1573") == "fluxer://channels/1572/1573"
        nav = navigate_command(9334)
        assert nav[:4] == ["node", "--input-type=module", "-e", nav[3]]
        assert "navigate(cdp, port, undefined, {overallMs: 110000})" in nav[3]
        assert lib_url() in nav[3] and "PARITY_CDP_PORT" in nav[3]

    # interactive-desktop broker routing
    assert needs_broker("up", False, {}, interactive=False)
    assert not needs_broker("up", False, {}, interactive=True)
    assert needs_broker("heal", False, {"FLUXER_ENV_FORCE_BROKER": "1"}, interactive=True)
    assert not needs_broker("status", False, {}, interactive=False)
    assert not needs_broker("up", True, {"FLUXER_ENV_FORCE_BROKER": "1"}, interactive=False)
    assert not needs_broker("down", False, {}, interactive=False)
    assert isinstance(on_interactive_desktop(), bool)
    assert BROKER_RESPONSE_WAITS["gate"] > 900 and "up" not in BROKER_RESPONSE_WAITS
    with tempfile.TemporaryDirectory() as tmp_dir:
        code, out = submit_broker_request("up", [], Path(tmp_dir), ack_wait=0.3, response_wait=2, poll=0.05)
        assert code == 3 and "env_broker.py --install" in out
        assert not list((Path(tmp_dir) / "requests").glob("*.json"))

    print("selftest ok (config/masking, personal-app guard, tunnel/official/rust commands, health parsers)")


# ---------------------------------------------------------------- CLI

def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="env.py",
        description="Bring up, check and heal the always-on parity test environment (tunnel, official, Rust).",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=(
            "examples:\n"
            "  python -I tools/parity_lab/env.py status\n"
            "  python -I tools/parity_lab/env.py up --dry-run --config D:/tmp/env.json\n"
            "  python -I tools/parity_lab/env.py heal --json\n"
            "  python -I tools/parity_lab/env.py down --dry-run\n"
            "  python -I tools/parity_lab/env.py restart-client\n"
            "  python -I tools/parity_lab/env.py --selftest"
        ),
    )
    parser.add_argument("command", nargs="?", choices=("status", "up", "heal", "down", "restart-client"),
                        help="status | up | heal | down | restart-client")
    parser.add_argument("--dev", action="store_true", help="restart-client: launch target/fc-root/debug/fluxer_client.exe")
    parser.add_argument("--json", action="store_true", help="print one JSON object instead of the table")
    parser.add_argument("--dry-run", action="store_true",
                        help="read-only checks only (process/TCP/HTTP); print planned actions, change nothing")
    parser.add_argument("--config", metavar="PATH",
                        help="config JSON (default: $FLUXER_ENV_CONFIG or <repo>/../Logs/fluxer-dev/env.json)")
    parser.add_argument("--selftest", action="store_true", help="unit-test config, guards, commands and parsers")
    return parser


def main(argv: list[str] | None = None) -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    parser = build_parser()
    args = parser.parse_args(argv)
    if args.selftest:
        selftest()
        return 0
    if not args.command:
        parser.print_help()
        return 2
    try:
        cfg = load_config(args.config)
    except ConfigError as exc:
        if args.json:
            print(json.dumps({"ok": False, "error": str(exc)}, ensure_ascii=False))
        else:
            print(f"env: {exc}", file=sys.stderr)
        return 2
    if needs_broker(args.command, args.dry_run):
        code, out = submit_broker_request(args.command, broker_args(args))
        print(out)
        return code
    handlers = {
        "status": cmd_status,
        "up": cmd_up,
        "heal": cmd_heal,
        "down": cmd_down,
        "restart-client": cmd_restart_client,
    }
    return handlers[args.command](cfg, args)


if __name__ == "__main__":
    raise SystemExit(main())
