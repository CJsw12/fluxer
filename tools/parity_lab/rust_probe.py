"""Client for the Rust app's parity probe (single-instance IPC, `FLUXER_PARITY_PROBE=1` only).

    python -I tools/parity_lab/rust_probe.py inspect > out.json
    python -I tools/parity_lab/rust_probe.py resize 1280 800
    python -I tools/parity_lab/rust_probe.py click <flx> [index]
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


def pid(ipc_path: Path = DEFAULT_IPC) -> int:
    return int(_ipc(ipc_path)["pid"])


def screenshot(out: Path, ipc_path: Path = DEFAULT_IPC) -> Path:
    """Captures the Rust window client area via PrintWindow (works while occluded)."""
    sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
    import visual_parity_diff as vpd  # noqa: PLC0415

    hwnd = vpd._find_window_for_pid(pid(ipc_path), timeout_s=5.0)
    vpd.capture_hwnd_to_png(hwnd, out)
    return out


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
    elif cmd == "hover":
        r = send("probe_hover", flx=rest[0], **({"index": int(rest[1])} if len(rest) > 1 else {}))
    elif cmd == "key":
        r = send("probe_key", keystroke=rest[0])
    elif cmd == "link":
        r = deep_link(rest[0])
    elif cmd == "shot":
        r = {"ok": True, "file": str(screenshot(Path(rest[0])))}
    else:
        print(__doc__)
        return 2
    print(json.dumps(r, ensure_ascii=False, indent=1))
    return 0 if r.get("ok") else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
