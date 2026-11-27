"""Client for the Rust app's parity probe (single-instance IPC, `FLUXER_PARITY_PROBE=1` only).

    python -I tools/parity_lab/rust_probe.py inspect > out.json
    python -I tools/parity_lab/rust_probe.py resize 1280 800
    python -I tools/parity_lab/rust_probe.py click <flx> [index]
    python -I tools/parity_lab/rust_probe.py right-click <flx> [index]
    python -I tools/parity_lab/rust_probe.py dblclick <flx> [index]
    python -I tools/parity_lab/rust_probe.py drag <flx> <to_x> <to_y> [index] [steps]
    python -I tools/parity_lab/rust_probe.py key escape
    python -I tools/parity_lab/rust_probe.py link fluxer://channels/<g>/<c>
    python -I tools/parity_lab/rust_probe.py shot out.png

The IPC file (`instance.ipc`: port, token, pid) lives in the client's session directory
(`%APPDATA%/fluxer_client` on Windows unless `--ipc` points elsewhere). The token is read from
that file and never printed.
"""

from __future__ import annotations

import json
import os
import socket
import sys
import time
from collections.abc import Callable
from pathlib import Path

DEFAULT_IPC = Path(os.environ.get("APPDATA", "")) / "fluxer_client" / "instance.ipc"


def _ipc(path: Path = DEFAULT_IPC) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def send(action: str, ipc_path: Path = DEFAULT_IPC, timeout: float = 8.0, **fields) -> dict:
    info = _ipc(ipc_path)
    payload = {"token": info["token"], "action": action, **fields}
    with socket.create_connection(("127.0.0.1", int(info["port"])), timeout=timeout) as s:
        s.sendall((json.dumps(payload) + "\n").encode("utf-8"))
        buf = b""
        while not buf.endswith(b"\n"):
            chunk = s.recv(1 << 20)
            if not chunk:
                break
            buf += chunk
    text = buf.decode("utf-8", "replace").strip()
    if text == "ok":
        return {"ok": True}
    if not text:
        return {"ok": False, "error": "no reply (probe disabled or request rejected)"}
    return json.loads(text)


def deep_link(url: str, ipc_path: Path = DEFAULT_IPC) -> dict:
    return send("deep_link", ipc_path, url=url)


def inspect_until(ready: Callable[[dict], bool], timeout: float) -> dict:
    """Return the first ready frame, or the last observed frame at the deadline."""
    deadline = time.monotonic() + timeout
    while True:
        data = send("probe_inspect")
        remaining = deadline - time.monotonic()
        if (data.get("ok") and ready(data)) or remaining <= 0:
            return data
        time.sleep(min(0.075, remaining))


def markdown_ready(data: dict) -> bool:
    return any(
        e["flx"].startswith("channel.channel-header.channel-name") and e.get("text") == "markdown"
        for e in data.get("elements", [])
    )


def pid(ipc_path: Path = DEFAULT_IPC) -> int:
    return int(_ipc(ipc_path)["pid"])


def screenshot(out: Path, ipc_path: Path = DEFAULT_IPC) -> Path:
    """Captures the Rust window client area via PrintWindow (works while occluded)."""
    sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
    import visual_parity_diff as vpd  # noqa: PLC0415

    hwnd = vpd._find_window_for_pid(pid(ipc_path), timeout_s=5.0)
    vpd.capture_hwnd_to_png(hwnd, out)
    return out


def shot_via_broker(out: Path) -> dict:
    """Off the interactive desktop EnumWindows cannot see the client; ask the user-desktop broker."""
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    import env  # noqa: PLC0415

    out = out if out.is_absolute() else Path.cwd() / out
    code, text = env.submit_broker_request("shot", [str(out)])
    if code == 0 and out.is_file():
        return {"ok": True, "file": str(out)}
    return {"ok": False, "error": f"broker shot failed (exit {code}): {text[-300:]}"}


def shot(out: Path) -> dict:
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    import env  # noqa: PLC0415

    if os.environ.get("FLUXER_ENV_FORCE_BROKER") == "1" or not env.on_interactive_desktop():
        return shot_via_broker(out)
    return {"ok": True, "file": str(screenshot(out))}


def wake_a11y(ipc_path: Path = DEFAULT_IPC) -> bool:
    """Send WM_GETOBJECT (UiaRootObjectId) so AccessKit activates on hidden desktops."""
    if sys.platform != "win32":
        return False
    try:
        import ctypes  # noqa: PLC0415

        sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
        import visual_parity_diff as vpd  # noqa: PLC0415

        hwnd = vpd._find_window_for_pid(pid(ipc_path), timeout_s=3.0)
        res = ctypes.c_size_t(0)
        ok = ctypes.windll.user32.SendMessageTimeoutW(
            ctypes.c_void_p(hwnd),
            0x003D,  # WM_GETOBJECT
            ctypes.c_size_t(0),
            ctypes.c_ssize_t(-25),  # UiaRootObjectId
            2,  # SMTO_ABORTIFHUNG
            2000,
            ctypes.byref(res),
        )
        return bool(ok)
    except Exception:
        return False


def main(argv: list[str]) -> int:
    sys.stdout.reconfigure(encoding="utf-8")
    if not argv:
        print(__doc__)
        return 2
    cmd, rest = argv[0], argv[1:]
    if cmd == "inspect":
        r = send("probe_inspect")
    elif cmd == "resize":
        r = send("probe_resize", w=float(rest[0]), h=float(rest[1]))
    elif cmd == "click":
        r = send("probe_click", flx=rest[0], **({"index": int(rest[1])} if len(rest) > 1 else {}))
    elif cmd in ("right-click", "rclick"):
        r = send("probe_right_click", flx=rest[0], **({"index": int(rest[1])} if len(rest) > 1 else {}))
    elif cmd == "dblclick":
        r = send("probe_dblclick", flx=rest[0], **({"index": int(rest[1])} if len(rest) > 1 else {}))
    elif cmd == "drag":
        r = send(
            "probe_drag",
            flx=rest[0],
            to_x=float(rest[1]),
            to_y=float(rest[2]),
            **({"index": int(rest[3])} if len(rest) > 3 else {}),
            **({"steps": int(rest[4])} if len(rest) > 4 else {}),
        )
    elif cmd == "hover":
        r = send("probe_hover", flx=rest[0], **({"index": int(rest[1])} if len(rest) > 1 else {}))
    elif cmd == "key":
        r = send("probe_key", keystroke=rest[0])
    elif cmd == "link":
        r = deep_link(rest[0])
    elif cmd == "shot":
        r = shot(Path(rest[0]))
    else:
        print(__doc__)
        return 2
    print(json.dumps(r, ensure_ascii=False, indent=1))
    return 0 if r.get("ok") else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
