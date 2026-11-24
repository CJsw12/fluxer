"""Live M9.3 resize parity checker against official CDP measurements (`Logs/parity/m93_official/resize_spec.json`).

    python -I tools/parity_lab/check_resize.py --launch-rust dist/fluxer_client/fluxer_client.exe --out Logs/parity/m93_after
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import rust_probe as rp  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
GUILD = "1572284491404148736"
CHANNEL = "1572284491404148743"
VOICE_CHANNEL = "1572284491404148740"

SB_HANDLE = "app.sidebar-resize-handle"
SB_INDICATOR = "app.sidebar-resize-handle.indicator"
SB_NAV = "app.guild-sidebar.guild-navbar"
USER_AREA = "app.user-area.user-area-inner.section"
HEADER = "channel.channel-header.header-wrapper"
EMOJI_PANEL = "expressions.expression-picker-popout.container"
EMOJI_HANDLE = "ui.resizable-pane.resizable-pane-handles.handle.button"
VOICE_SECTION = "voice.compact-voice-call-view.compact-voice-call-view-inner.section.voice-root-pointer-activity"
VOICE_HANDLE = "voice.compact-voice-call-view.compact-voice-call-view-inner.resize-handle.resize-key-down"


def launch_rust_same_desktop(exe: Path) -> subprocess.Popen:
    env = dict(os.environ)
    env.pop("SSL_CERT_FILE", None)
    env["FLUXER_PARITY_PROBE"] = "1"
    env["LOCALAPPDATA"] = os.environ.get("PARITY_PROFILE", str(REPO.parent / "Logs" / "fluxer_test_profile"))
    ipc = rp.DEFAULT_IPC
    old_mtime = ipc.stat().st_mtime if ipc.exists() else 0.0
    proc = subprocess.Popen([str(exe), "--no-os-integration"], cwd=str(exe.parent), env=env)
    deadline = time.time() + 35.0
    while time.time() < deadline:
        if proc.poll() is not None:
            raise RuntimeError(f"Rust client exited early with code {proc.returncode}")
        if ipc.exists() and ipc.stat().st_mtime > old_mtime:
            try:
                info = json.loads(ipc.read_text(encoding="utf-8"))
                if int(info.get("pid", 0)) == proc.pid:
                    r = rp.send("probe_inspect")
                    if r.get("ok"):
                        status = next(
                            (e.get("text") for e in r.get("elements", []) if e.get("flx") == "app.user-area.user-area-inner.user-status"),
                            None,
                        )
                        if status == "Online":
                            print(f"rust launched pid={proc.pid} status=Online")
                            return proc
            except Exception:
                pass
        time.sleep(0.5)
    proc.terminate()
    raise RuntimeError("Rust client did not reach Status: Online within 35s")


def rect_of(inspect_data: dict, flx: str, index: int = 0) -> list[int] | None:
    hits = [e for e in inspect_data.get("elements", []) if e.get("flx") == flx]
    if 0 <= index < len(hits):
        return [round(v) for v in hits[index]["rect"]]
    return None


def inspect_settled(delay: float = 0.25) -> dict:
    time.sleep(delay)
    return rp.send("probe_inspect")


def check_eq(checks: list[dict], name: str, got, want, tol: int = 2) -> None:
    if got is None:
        ok = False
    elif isinstance(want, (int, float)) and isinstance(got, (int, float)):
        ok = abs(got - want) <= tol
    elif isinstance(want, list) and isinstance(got, list) and len(want) == len(got):
        ok = all(abs(a - b) <= tol for a, b in zip(got, want))
    else:
        ok = got == want
    checks.append({"name": name, "ok": ok, "got": got, "want": want})
    status = "PASS" if ok else "FAIL"
    print(f"  [{status}] {name:38} got={got} want={want}")


def main(argv: list[str]) -> int:
    sys.stdout.reconfigure(encoding="utf-8")
    opts = dict(zip(argv[0::2], argv[1::2]))
    out = Path(opts.get("--out", "Logs/parity/m93_after"))
    spec_path = Path(opts.get("--spec", "Logs/parity/m93_official/resize_spec.json"))
    exe = opts.get("--launch-rust")
    out.mkdir(parents=True, exist_ok=True)
    spec = json.loads(spec_path.read_text(encoding="utf-8"))

    proc = launch_rust_same_desktop(Path(exe)) if exe else None
    checks: list[dict] = []
    joined_voice = False

    try:
        rp.send("probe_resize", w=1280.0, h=800.0)
        rp.deep_link(f"fluxer://channels/{GUILD}/{CHANNEL}")
        time.sleep(1.5)
        for _ in range(2):
            rp.send("probe_key", keystroke="escape")

        # 1. Sidebar width resize
        print("=== Sidebar Resize ===")
        rp.send("probe_dblclick", flx=SB_HANDLE)
        d0 = inspect_settled()
        sb_spec = spec["sidebar"]
        check_eq(checks, "sidebar.default.nav_w", (rect_of(d0, SB_NAV) or [0, 0, 0, 0])[2], sb_spec["default"]["navbar"]["rect"][2])
        check_eq(checks, "sidebar.default.user_area_w", (rect_of(d0, USER_AREA) or [0, 0, 0, 0])[2], sb_spec["default"]["user_area"]["rect"][2])
        check_eq(checks, "sidebar.default.header_x", (rect_of(d0, HEADER) or [0, 0, 0, 0])[0], sb_spec["default"]["channel_header"]["rect"][0])
        check_eq(checks, "sidebar.default.handle_rect", rect_of(d0, SB_HANDLE), sb_spec["default"]["handle"]["rect"], tol=4)

        rp.send("probe_drag", flx=SB_HANDLE, to_x=150.0, to_y=350.0, steps=10)
        dmin = inspect_settled()
        check_eq(checks, "sidebar.min.nav_w", (rect_of(dmin, SB_NAV) or [0, 0, 0, 0])[2], sb_spec["min"]["navbar"]["rect"][2])
        check_eq(checks, "sidebar.min.user_area_w", (rect_of(dmin, USER_AREA) or [0, 0, 0, 0])[2], sb_spec["min"]["user_area"]["rect"][2])
        check_eq(checks, "sidebar.min.header_x", (rect_of(dmin, HEADER) or [0, 0, 0, 0])[0], sb_spec["min"]["channel_header"]["rect"][0])

        rp.send("probe_drag", flx=SB_HANDLE, to_x=650.0, to_y=350.0, steps=10)
        dmax = inspect_settled()
        check_eq(checks, "sidebar.max.nav_w", (rect_of(dmax, SB_NAV) or [0, 0, 0, 0])[2], sb_spec["max"]["navbar"]["rect"][2])
        check_eq(checks, "sidebar.max.user_area_w", (rect_of(dmax, USER_AREA) or [0, 0, 0, 0])[2], sb_spec["max"]["user_area"]["rect"][2])
        check_eq(checks, "sidebar.max.header_x", (rect_of(dmax, HEADER) or [0, 0, 0, 0])[0], sb_spec["max"]["channel_header"]["rect"][0])

        rp.send("probe_dblclick", flx=SB_HANDLE)
        drst = inspect_settled()
        check_eq(checks, "sidebar.dblclick_reset", (rect_of(drst, SB_NAV) or [0, 0, 0, 0])[2], sb_spec["after_double_click"]["navbar"]["rect"][2])

        rp.send("probe_click", flx=SB_HANDLE)
        rp.send("probe_key", keystroke="right")
        check_eq(checks, "sidebar.key.right", (rect_of(inspect_settled(0.15), SB_NAV) or [0, 0, 0, 0])[2], sb_spec["keyboard"]["after_arrow_right"])
        rp.send("probe_key", keystroke="shift-right")
        check_eq(checks, "sidebar.key.shift_right", (rect_of(inspect_settled(0.15), SB_NAV) or [0, 0, 0, 0])[2], sb_spec["keyboard"]["after_shift_arrow_right"])
        rp.send("probe_key", keystroke="home")
        check_eq(checks, "sidebar.key.home", (rect_of(inspect_settled(0.15), SB_NAV) or [0, 0, 0, 0])[2], sb_spec["keyboard"]["after_home"])
        rp.send("probe_key", keystroke="end")
        check_eq(checks, "sidebar.key.end", (rect_of(inspect_settled(0.15), SB_NAV) or [0, 0, 0, 0])[2], sb_spec["keyboard"]["after_end"])
        rp.send("probe_key", keystroke="enter")
        check_eq(checks, "sidebar.key.enter_reset", (rect_of(inspect_settled(0.15), SB_NAV) or [0, 0, 0, 0])[2], sb_spec["keyboard"]["after_enter_reset"])

        # 2. Emoji picker resize
        print("=== Emoji Picker Resize ===")
        rp.deep_link("fluxer://scene/emoji-picker")
        time.sleep(0.6)
        ep0 = inspect_settled()
        ep_spec = spec["expression_picker"]
        check_eq(checks, "emoji.default.rect", rect_of(ep0, EMOJI_PANEL), ep_spec["default_container"]["rect"], tol=4)
        left_rect = rect_of(ep0, EMOJI_HANDLE, index=1)
        top_rect = rect_of(ep0, EMOJI_HANDLE, index=0)
        if left_rect:
            rp.send("probe_drag", flx=EMOJI_HANDLE, index=1, to_x=float(left_rect[0] - 144), to_y=float(left_rect[1] + 50), steps=8)
            ep_wide = inspect_settled()
            check_eq(checks, "emoji.snap_wider.w", (rect_of(ep_wide, EMOJI_PANEL) or [0, 0, 0, 0])[2], 736, tol=48)
            rp.send("probe_drag", flx=EMOJI_HANDLE, index=1, to_x=950.0, to_y=float(left_rect[1] + 50), steps=8)
            ep_min_w = inspect_settled()
            check_eq(checks, "emoji.min_w", (rect_of(ep_min_w, EMOJI_PANEL) or [0, 0, 0, 0])[2], 400)
        if top_rect:
            rp.send("probe_drag", flx=EMOJI_HANDLE, index=0, to_x=800.0, to_y=700.0, steps=8)
            ep_min_h = inspect_settled()
            check_eq(checks, "emoji.min_h", (rect_of(ep_min_h, EMOJI_PANEL) or [0, 0, 0, 0])[3], 360)
        rp.send("probe_dblclick", flx=EMOJI_HANDLE, index=1)
        ep_rst = inspect_settled()
        check_eq(checks, "emoji.dblclick_reset", (rect_of(ep_rst, EMOJI_PANEL) or [0, 0, 0, 0])[2:], ep_spec["default_container"]["rect"][2:], tol=4)
        rp.send("probe_key", keystroke="escape")
        time.sleep(0.3)

        # 3. Compact voice call <-> chat split height
        print("=== Voice Split Resize ===")
        rp.deep_link("fluxer://scene/voice-mute/on")
        time.sleep(0.3)
        rp.deep_link(f"fluxer://scene/voice-split/{GUILD}/{VOICE_CHANNEL}")
        joined_voice = True
        time.sleep(4.5)
        vc0 = inspect_settled()
        vc_spec = spec["voice_compact"]
        check_eq(checks, "voice_split.default.h", (rect_of(vc0, VOICE_SECTION) or [0, 0, 0, 0])[3], vc_spec["default_section"]["rect"][3], tol=4)

        vh_rect = rect_of(vc0, VOICE_HANDLE)
        if vh_rect:
            rp.send("probe_drag", flx=VOICE_HANDLE, to_x=float(vh_rect[0] + vh_rect[2] / 2), to_y=760.0, steps=8)
            vc_max = inspect_settled()
            check_eq(checks, "voice_split.drag_max.h", (rect_of(vc_max, VOICE_SECTION) or [0, 0, 0, 0])[3], vc_spec["drag_max"]["rect"][3], tol=4)

            rp.send("probe_click", flx=VOICE_HANDLE)
            rp.send("probe_key", keystroke="up")
            vc_up = inspect_settled(0.2)
            check_eq(checks, "voice_split.key.up.h", (rect_of(vc_up, VOICE_SECTION) or [0, 0, 0, 0])[3], vc_spec["keyboard"]["after_arrow_up"], tol=4)

            rp.send("probe_drag", flx=VOICE_HANDLE, to_x=float(vh_rect[0] + vh_rect[2] / 2), to_y=120.0, steps=8)
            vc_min = inspect_settled()
            check_eq(checks, "voice_split.drag_min.h", (rect_of(vc_min, VOICE_SECTION) or [0, 0, 0, 0])[3], vc_spec["drag_min"]["rect"][3], tol=4)
        else:
            check_eq(checks, "voice_split.handle_present", None, "handle rect")
    finally:
        if joined_voice:
            try:
                rp.deep_link("fluxer://scene/voice-leave")
                time.sleep(0.8)
                rp.deep_link(f"fluxer://channels/{GUILD}/{CHANNEL}")
                time.sleep(0.5)
            except Exception:
                pass
        if proc is not None:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()

    passed = sum(1 for c in checks if c["ok"])
    report = {"passed": passed, "total": len(checks), "checks": checks}
    (out / "resize_report.json").write_text(json.dumps(report, indent=1), encoding="utf-8")
    print(f"\nSUMMARY: {passed}/{len(checks)} passed -> {out / 'resize_report.json'}")
    return 0 if passed == len(checks) else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
