"""One-command dev loop: test -> build -> relaunch (probe on) -> wait Online -> optional scene check.

    python -I tools/parity_lab/cycle.py [--release] [--worker] [--skip-tests]
        [--check OFFICIAL.dom.json OFFICIAL.png fluxer://... OUT_DIR]

Default builds the fast incremental debug binary (target/fc-root/debug/fluxer_client.exe);
--release packages dist/ (add --worker when voice_worker changed). The debug binary has no
voice_worker next to it, so voice/screen-share scenes need --release. The client runs detached
with FLUXER_PARITY_PROBE=1 and the test profile from env PARITY_PROFILE (LOCALAPPDATA override).
"""

from __future__ import annotations

import os
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))  # `python -I` drops the script dir
import rust_probe as rp  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
TARGET = REPO / "target" / "fc-root"
PWSH = Path(os.environ.get("LOCALAPPDATA", "")) / "Programs" / "PowerShell" / "7" / "pwsh.exe"


def run(cmd: list[str], cwd: Path = REPO, env: dict | None = None, tail: int = 6) -> int:
    t = time.time()
    r = subprocess.run(cmd, cwd=cwd, env=env, capture_output=True, text=True, encoding="utf-8", errors="replace")
    lines = [l for l in (r.stdout + r.stderr).splitlines() if l.strip()]
    keep = [l for l in lines if any(k in l for k in ("test result", "FAILED", "panicked", "error", "warning: unused", "SHA256"))]
    print(f"[{time.time() - t:5.0f}s] {' '.join(cmd[:4])} -> {r.returncode}")
    for l in (keep or lines)[-tail:]:
        print("   ", l[:200])
    return r.returncode


def main(argv: list[str]) -> int:
    sys.stdout.reconfigure(encoding="utf-8")
    release, worker, skip = "--release" in argv, "--worker" in argv, "--skip-tests" in argv
    env = dict(os.environ, CARGO_TARGET_DIR=str(TARGET))
    if not skip and run(["cargo", "test", "-p", "fluxer_client_app", "-j", "4", "--", "--test-threads=2"], REPO / "fluxer_client", env):
        return 1
    for im in ("fluxer_client.exe", "fluxer_voice_worker.exe"):
        subprocess.run(["taskkill", "/F", "/IM", im], capture_output=True)
    if release:
        args = [str(PWSH), "-NoProfile", "-File", "scripts/package_fluxer_client.ps1"] + (["-IncludeVoiceWorker"] if worker else [])
        if run(args):
            return 1
        exe = REPO / "dist" / "fluxer_client" / "fluxer_client.exe"
    else:
        if run(["cargo", "build", "-p", "fluxer_client_app", "-j", "4"], REPO / "fluxer_client", env):
            return 1
        exe = TARGET / "debug" / "fluxer_client.exe"
    profile = os.environ.get("PARITY_PROFILE", str(REPO.parent / "Logs" / "fluxer_test_profile"))
    launch = (
        "Remove-Item Env:SSL_CERT_FILE -ErrorAction SilentlyContinue; "
        f"$env:FLUXER_PARITY_PROBE='1'; $env:LOCALAPPDATA='{profile}'; "
        f"Start-Process -FilePath '{exe}' -ArgumentList '--no-os-integration' -WorkingDirectory '{exe.parent}'"
    )
    subprocess.run([str(PWSH), "-NoProfile", "-Command", launch], check=False)
    for _ in range(30):
        time.sleep(1.5)
        r = subprocess.run(["orca", "computer", "get-app-state", "--app", "fluxer_client", "--no-screenshot", "--query", "Status"],
                           capture_output=True, text=True, encoding="utf-8", errors="replace")
        if "Status: Online" in r.stdout:
            print("client: Online")
            break
        try:
            pr = rp.send("probe_inspect")
            if pr.get("ok") and any(
                e.get("flx") == "app.user-area.user-area-inner.user-status" and e.get("text") == "Online"
                for e in pr.get("elements", [])
            ):
                print("client: Online (via probe)")
                break
        except Exception:
            pass
    else:
        print("client: NOT online (check tunnel)")
        return 1
    rp.wake_a11y()
    if "--check" in argv:
        i = argv.index("--check")
        return run([sys.executable, "-I", str(REPO / "tools" / "parity_lab" / "check_scene.py"), *argv[i + 1:i + 5]], tail=40)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
