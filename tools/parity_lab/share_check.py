"""Automated two-way screen-share and voice-audio check between the Rust client (parity probe on)
and the official desktop app (CDP). No human needed; both apps must be logged in to the same test server.

    python -I tools/parity_lab/share_check.py --guild G --channel C --out DIR
        [--attach 9334] [--launch-rust PATH_TO_EXE] [--restore-channel CH] [--source REGEX]

Steps: both join the voice channel with mics muted -> Rust opens screen-share picker via .flx,
selects source/quality and clicks Go Live -> official watches and must receive moving video ->
Rust stops -> official shares the Rust client window -> Rust watches via .flx and must render
moving video without colour cast and with "<name>'s screen" header (no raw "user_") ->
voice audio/speaking structures are inspected on both sides -> finally cleans up both apps.
Verdicts land in DIR/share_check.json.
"""

from __future__ import annotations

import json
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import check_surface_parity as csp  # noqa: E402
import rust_probe as rp  # noqa: E402

HERE = Path(__file__).resolve().parent
STAGE = (405, 125, 1002, 508)  # fallback watched-stream area in the Rust voice stage at 1280x800

FLX_CONTROL_BAR_BTN = "voice.voice-control-bar.voice-control-bar-inner.button.undefined"
FLX_PANEL_MEDIA_BTN = "voice.voice-connection-status.local-participant-controls.media-button.undefined"
FLX_PICKER_MODAL = "voice.screen-share-modal.container"
FLX_PICKER_SOURCE = "voice.screen-share-modal.source-item"
FLX_PICKER_RES_1080P = "voice.screen-share-modal.quality.1080p"
FLX_PICKER_FPS_30 = "voice.screen-share-modal.fps.30"
FLX_PICKER_GO_LIVE = "voice.screen-share-modal.go-live"
FLX_WATCH_STREAM = "voice.voice-stage.watch-stream"
FLX_VIEWER_HEADER = "voice.video-viewer.header"
FLX_VIEWER_STAGE = "voice.video-viewer.stage"
FLX_SPEAKING_STATE = "voice.voice-stage.speaking-state"
FLX_PARTICIPANT_SPEAKING = "voice.voice-stage.participant-speaking"


def official(attach: int, *args: str) -> dict:
    r = subprocess.run(
        ["node", str(HERE / "voice_official.mjs"), "--attach", str(attach), *args],
        capture_output=True,
        text=True,
        encoding="utf-8",
        timeout=120,
    )
    line = (r.stdout.strip().splitlines() or ["{}"])[-1]
    try:
        return json.loads(line)
    except json.JSONDecodeError:
        return {"error": (r.stderr or r.stdout)[-300:]}


def inspect_elements() -> list[dict]:
    res = rp.send("probe_inspect")
    if not res.get("ok"):
        return []
    return res.get("elements") or []


def find_flx(elements: list[dict], flx_id: str) -> list[dict]:
    return [e for e in elements if e.get("flx") == flx_id]


def click_share_button() -> dict:
    """Clicks the Rust screen-share toggle via .flx (control bar index 3, fallback panel index 1)."""
    res = rp.send("probe_click", flx=FLX_CONTROL_BAR_BTN, index=3)
    if res.get("ok"):
        return res
    return rp.send("probe_click", flx=FLX_PANEL_MEDIA_BTN, index=1)


def stage_crop_box() -> tuple[int, int, int, int]:
    """Resolves the watched video viewer box via .flx, falling back to STAGE."""
    matches = find_flx(inspect_elements(), FLX_VIEWER_STAGE)
    if matches:
        x, y, w, h = matches[0].get("rect", [0, 0, 0, 0])
        if w > 80 and h > 80:
            return (int(x + 12), int(y + 40), int(x + w - 12), int(y + h - 12))
    return STAGE


def pixels(path: Path, box: tuple[int, int, int, int]):
    from PIL import Image  # noqa: PLC0415

    im = Image.open(path).convert("RGB").crop(box)
    return im, (list(im.get_flattened_data()) if hasattr(im, "get_flattened_data") else list(im.getdata()))


def red_cast(path: Path, box: tuple[int, int, int, int]) -> float:
    """Share of strongly red-dominant pixels; dark UI video should be close to 0."""
    _, px = pixels(path, box)
    return round(sum(1 for r, g, b in px if r > g + 40 and r > b + 40) / max(len(px), 1), 3)


def changed(a: Path, b: Path, box: tuple[int, int, int, int]) -> float:
    from PIL import ImageChops, ImageStat  # noqa: PLC0415

    ia, _ = pixels(a, box)
    ib, _ = pixels(b, box)
    return round(sum(ImageStat.Stat(ImageChops.difference(ia, ib)).mean), 3)


def ensure_rust_mic_muted() -> str:
    """The mute control toggles, so read its accessible label first (UIA via Orca computer use)."""
    r = subprocess.run(
        ["orca", "computer", "get-app-state", "--app", "fluxer_client", "--no-screenshot", "--query", "microphone"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        timeout=60,
    )
    if "Unmute microphone" in r.stdout:
        return "already muted"
    if "Mute microphone" in r.stdout:
        rp.send("probe_click", flx="app.user-area.user-area-inner.control-button.mute")
        return "muted now"
    return "unknown (mic state not found)"


def parse_args(argv: list[str]) -> dict[str, str]:
    opts: dict[str, str] = {}
    i = 0
    while i < len(argv):
        key = argv[i]
        if key.startswith("--") and i + 1 < len(argv):
            opts[key] = argv[i + 1]
            i += 2
        else:
            i += 1
    return opts


def main(argv: list[str]) -> int:
    sys.stdout.reconfigure(encoding="utf-8")
    opts = parse_args(argv)
    guild, channel, out = opts["--guild"], opts["--channel"], Path(opts["--out"])
    attach = int(opts.get("--attach", "9334"))
    restore_channel = opts.get("--restore-channel", csp.CHANNEL)
    launch_rust = opts.get("--launch-rust")
    out.mkdir(parents=True, exist_ok=True)
    report: dict = {}
    rust_proc: subprocess.Popen | None = None

    try:
        if launch_rust:
            rust_proc = csp.launch_rust_same_desktop(Path(launch_rust))
            report["rust_launched_pid"] = rust_proc.pid

        rp.send("probe_resize", w=1280.0, h=800.0)
        mute = ensure_rust_mic_muted()
        report["rust_mic"] = mute
        report["rust_join"] = rp.deep_link(f"fluxer://scene/voice/{guild}/{channel}")
        time.sleep(7)
        report["official_join"] = official(attach, "join", "--guild", guild, "--channel", channel)

        # Rust -> official via Screen Share Picker modal (.flx driven)
        report["rust_share_open"] = click_share_button()
        time.sleep(1.5)
        picker_els = inspect_elements()
        picker_open = bool(find_flx(picker_els, FLX_PICKER_MODAL))
        source_items = find_flx(picker_els, FLX_PICKER_SOURCE)
        picker_steps = {
            "picker_open": picker_open,
            "source_count": len(source_items),
            "select_1080p": rp.send("probe_click", flx=FLX_PICKER_RES_1080P),
            "select_30fps": rp.send("probe_click", flx=FLX_PICKER_FPS_30),
        }
        if source_items:
            picker_steps["select_source"] = rp.send("probe_click", flx=FLX_PICKER_SOURCE, index=0)
        picker_steps["go_live"] = rp.send("probe_click", flx=FLX_PICKER_GO_LIVE)
        report["rust_picker"] = picker_steps
        time.sleep(6)
        rp.screenshot(out / "rust_sharing.png")
        watch = official(attach, "watch")
        vids = watch.get("videos") or []
        report["rust_to_official"] = {
            **watch,
            "picker_ok": picker_open and bool(picker_steps["go_live"].get("ok")),
            "pass": (
                picker_open
                and bool(picker_steps["go_live"].get("ok"))
                and bool(vids)
                and vids[0].get("width", 0) > 0
                and vids[0].get("frames_2s", 0) > 0
            ),
        }
        report["rust_share_stop"] = click_share_button()
        time.sleep(2)

        # official -> Rust (shares the Rust client window, so nothing private leaves the machine)
        report["official_share"] = official(attach, "share", "--source", opts.get("--source", "Fluxer - #"))
        time.sleep(4)
        report["rust_watch_click"] = rp.send("probe_click", flx=FLX_WATCH_STREAM, index=0)
        time.sleep(8)

        els_watching = inspect_elements()
        header_matches = find_flx(els_watching, FLX_VIEWER_HEADER)
        header_text = (header_matches[0].get("text") or "") if header_matches else ""
        header_ok = ("'s screen" in header_text) and ("user_" not in header_text)
        box = stage_crop_box()

        a, b = out / "rust_watch_a.png", out / "rust_watch_b.png"
        rp.screenshot(a)
        rp.send("probe_hover", x=200.0, y=149.0)  # change the shared window so the video must move
        time.sleep(3)
        rp.screenshot(b)
        rp.send("probe_hover", x=1240.0, y=760.0)
        cast, delta = red_cast(a, box), changed(a, b, box)
        report["official_to_rust"] = {
            "header_text": header_text,
            "header_ok": header_ok,
            "crop_box": list(box),
            "red_cast": cast,
            "frame_change": delta,
            "pass": header_ok and cast < 0.02 and delta > 0.05,
        }

        # Voice audio & speaking structure check (valid even while microphones are muted)
        official_audio = official(attach, "audio")
        speaking_els = find_flx(els_watching, FLX_SPEAKING_STATE)
        participant_speaking = [e.get("text") for e in find_flx(els_watching, FLX_PARTICIPANT_SPEAKING)]
        rust_speaking_text = (speaking_els[0].get("text") or "") if speaking_els else ""
        rust_audio_ok = rust_speaking_text.startswith("active=") and bool(participant_speaking)
        official_audio_ok = bool(official_audio.get("in_voice")) and isinstance(
            official_audio.get("audio_elements"), int
        )
        report["voice_audio"] = {
            "official": official_audio,
            "rust_speaking_state": rust_speaking_text,
            "rust_participants": participant_speaking,
            "pass_criteria": "official.in_voice && numeric audio_elements && rust speaking_state & participant_speaking present",
            "pass": official_audio_ok and rust_audio_ok,
        }
    finally:
        # Always stop sharing, leave voice in both apps, restore official #markdown, and stop launched Rust.
        try:
            report["official_stop"] = official(attach, "stop")
        except Exception as exc:
            report["official_stop_error"] = str(exc)
        try:
            report["official_leave"] = official(attach, "leave")
        except Exception as exc:
            report["official_leave_error"] = str(exc)
        try:
            report["official_restore"] = official(
                attach, "restore", "--guild", guild, "--channel", restore_channel
            )
        except Exception as exc:
            report["official_restore_error"] = str(exc)
        try:
            report["rust_leave"] = rp.deep_link("fluxer://scene/voice-leave")
        except Exception as exc:
            report["rust_leave_error"] = str(exc)
        if rust_proc is not None:
            try:
                rust_proc.terminate()
                rust_proc.wait(timeout=10)
            except Exception:
                try:
                    rust_proc.kill()
                except Exception:
                    pass

    (out / "share_check.json").write_text(json.dumps(report, ensure_ascii=False, indent=1), encoding="utf-8")
    r2o = report.get("rust_to_official", {}).get("pass", False)
    o2r = report.get("official_to_rust", {}).get("pass", False)
    va = report.get("voice_audio", {}).get("pass", False)
    ok = bool(r2o and o2r and va)
    print(
        json.dumps(
            {
                "rust_to_official": report.get("rust_to_official"),
                "official_to_rust": report.get("official_to_rust"),
                "voice_audio": report.get("voice_audio"),
            },
            ensure_ascii=False,
        )
    )
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
