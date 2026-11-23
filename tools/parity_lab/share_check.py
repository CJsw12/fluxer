"""Automated two-way screen-share check between the Rust client (parity probe on) and the official
desktop app (CDP). No human needed; both apps must be logged in to the same test server.

    python -I tools/parity_lab/share_check.py --guild G --channel C --out DIR
        [--attach 9334] [--share-button X,Y] [--watch-button X,Y]

Steps: both join the voice channel with mics muted → Rust shares its primary screen → official
watches and must receive moving video → Rust stops → official shares the Rust client window →
Rust watches and must render moving video without a colour cast → official stops.
Verdicts land in DIR/share_check.json. Screen contents leave the machine only to the test server.
The share/watch button coordinates default to the 1280×800 voice stage layout; pass them when the
layout changes (tag the buttons with `.flx` to remove this dependency).
"""

from __future__ import annotations

import json
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import rust_probe as rp  # noqa: E402

HERE = Path(__file__).resolve().parent
STAGE = (405, 125, 1002, 508)  # watched-stream area in the Rust voice stage at 1280×800


def official(attach: int, *args: str) -> dict:
    r = subprocess.run(["node", str(HERE / "voice_official.mjs"), "--attach", str(attach), *args],
                       capture_output=True, text=True, encoding="utf-8", timeout=120)
    line = (r.stdout.strip().splitlines() or ["{}"])[-1]
    try:
        return json.loads(line)
    except json.JSONDecodeError:
        return {"error": (r.stderr or r.stdout)[-300:]}


def pixels(path: Path):
    from PIL import Image  # noqa: PLC0415

    im = Image.open(path).convert("RGB").crop(STAGE)
    return im, (list(im.get_flattened_data()) if hasattr(im, "get_flattened_data") else list(im.getdata()))


def red_cast(path: Path) -> float:
    """Share of strongly red-dominant pixels; dark UI video should be close to 0."""
    _, px = pixels(path)
    return round(sum(1 for r, g, b in px if r > g + 40 and r > b + 40) / len(px), 3)


def changed(a: Path, b: Path) -> float:
    from PIL import ImageChops, ImageStat  # noqa: PLC0415

    ia, _ = pixels(a)
    ib, _ = pixels(b)
    return round(sum(ImageStat.Stat(ImageChops.difference(ia, ib)).mean), 3)


def ensure_rust_mic_muted() -> str:
    """The mute control toggles, so read its accessible label first (UIA via Orca computer use)."""
    r = subprocess.run(["orca", "computer", "get-app-state", "--app", "fluxer_client", "--no-screenshot",
                        "--query", "microphone"], capture_output=True, text=True, encoding="utf-8", timeout=60)
    if "Unmute microphone" in r.stdout:
        return "already muted"
    if "Mute microphone" in r.stdout:
        rp.send("probe_click", flx="app.user-area.user-area-inner.control-button.mute")
        return "muted now"
    return "unknown (mic state not found)"


def xy(text: str) -> tuple[float, float]:
    x, y = text.split(",")
    return float(x), float(y)


def main(argv: list[str]) -> int:
    sys.stdout.reconfigure(encoding="utf-8")
    opts = dict(zip(argv[0::2], argv[1::2]))
    guild, channel, out = opts["--guild"], opts["--channel"], Path(opts["--out"])
    attach = int(opts.get("--attach", 9334))
    share_btn = xy(opts.get("--share-button", "725,759"))
    watch_btn = xy(opts.get("--watch-button", "703,303"))
    out.mkdir(parents=True, exist_ok=True)
    report: dict = {}

    rp.send("probe_resize", w=1280.0, h=800.0)
    mute = ensure_rust_mic_muted()
    report["rust_join"] = rp.deep_link(f"fluxer://scene/voice/{guild}/{channel}")
    time.sleep(7)
    report["official_join"] = official(attach, "join", "--guild", guild, "--channel", channel)

    # Rust -> official
    report["rust_share_click"] = rp.send("probe_click", x=share_btn[0], y=share_btn[1])
    time.sleep(6)
    rp.screenshot(out / "rust_sharing.png")
    watch = official(attach, "watch")
    vids = watch.get("videos") or []
    report["rust_to_official"] = {
        **watch,
        "pass": bool(vids) and vids[0].get("width", 0) > 0 and vids[0].get("frames_2s", 0) > 0,
    }
    rp.send("probe_click", x=share_btn[0], y=share_btn[1])  # stop Rust share
    time.sleep(2)

    # official -> Rust (shares the Rust client window, so nothing private leaves the machine)
    report["official_share"] = official(attach, "share", "--source", opts.get("--source", "Fluxer - #"))
    time.sleep(4)
    rp.send("probe_click", x=watch_btn[0], y=watch_btn[1])
    time.sleep(8)
    a, b = out / "rust_watch_a.png", out / "rust_watch_b.png"
    rp.screenshot(a)
    rp.send("probe_hover", x=200.0, y=149.0)  # change the shared window so the video must move
    time.sleep(3)
    rp.screenshot(b)
    rp.send("probe_hover", x=1240.0, y=760.0)
    cast, delta = red_cast(a), changed(a, b)
    report["official_to_rust"] = {"red_cast": cast, "frame_change": delta, "pass": cast < 0.02 and delta > 0.05}
    report["official_stop"] = official(attach, "stop")
    # Leave both apps idle again (no open voice session on the test server).
    report["official_leave"] = official(attach, "leave")
    report["rust_leave"] = rp.deep_link("fluxer://scene/voice-leave")
    report["rust_mic"] = mute

    (out / "share_check.json").write_text(json.dumps(report, ensure_ascii=False, indent=1), encoding="utf-8")
    ok = report["rust_to_official"]["pass"] and report["official_to_rust"]["pass"]
    print(json.dumps({"rust_to_official": report["rust_to_official"], "official_to_rust": report["official_to_rust"]}, ensure_ascii=False))
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
