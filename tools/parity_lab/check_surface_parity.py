"""Live M9.x surface, phantom-hover, seam/corner pixel, and expression-picker semantic parity gate.

    python -I tools/parity_lab/check_surface_parity.py --launch-rust dist/fluxer_client/fluxer_client.exe --out Logs/parity/surface_after
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

FLX_MEDIA_BTN = "channel.textarea.textarea-buttons.textarea-button.expression-picker-toggle--2"
FLX_STICKER_BTN = "channel.textarea.textarea-buttons.textarea-button.expression-picker-toggle--3"
FLX_EMOJI_BTN = "channel.textarea.textarea-buttons.textarea-button.expression-picker-toggle--4"
FLX_PICKER = "expressions.expression-picker-popout.container"
FLX_TAB = "expressions.expression-picker-popout.tab.button"
FLX_MEDIA_EMPTY = "expressions.media-picker.empty"
FLX_STICKER_EMPTY = "expressions.sticker-picker.empty"
FLX_EMOJI_BODY = "expressions.emoji-picker.container"
FLX_GUILD_HEADER = "app.guild-header.header-wrapper"
FLX_CH_HEADER = "channel.channel-header.header-wrapper"
FLX_USER_PROFILE = "app.user-area.user-area-inner.focus-ring-wrapper"
FLX_USER_SECTION = "app.user-area.user-area-inner.section"
FLX_MEMBER_LIST = "channel.member-list-container.member-list-container"
FLX_SETTINGS_BTN = "app.user-area.user-area-inner.control-button.settings-click"
FLX_SETTINGS_MODAL = "user.user-settings-modal.modal-root"
FLX_SETTINGS_SIDEBAR = "app.settings-modal-layout.settings-modal-desktop-sidebar.desktop-sidebar"


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


def find_flx(data: dict, flx: str) -> list[dict]:
    return [e for e in data.get("elements", []) if e.get("flx") == flx]


def bright_diff_count(a_png: Path, b_png: Path, rect: list[int], thresh: int = 28) -> int:
    from PIL import Image, ImageChops  # noqa: PLC0415

    a = Image.open(a_png).convert("RGB")
    b = Image.open(b_png).convert("RGB")
    x, y, w, h = [round(v) for v in rect]
    box = (max(0, x), max(0, y), min(a.width, b.width, x + w), min(a.height, b.height, y + h))
    if box[2] <= box[0] or box[3] <= box[1]:
        return 0
    diff = ImageChops.difference(a.crop(box), b.crop(box))
    pxs = diff.get_flattened_data() if hasattr(diff, "get_flattened_data") else diff.getdata()
    return sum(1 for r, g, bl in pxs if (r + g + bl) / 3.0 > thresh)


def mean_luma(png: Path, box: tuple[int, int, int, int]) -> float:
    from PIL import Image, ImageStat  # noqa: PLC0415

    im = Image.open(png).convert("L")
    x0, y0, x1, y1 = max(0, box[0]), max(0, box[1]), min(im.width, box[2]), min(im.height, box[3])
    if x1 <= x0 or y1 <= y0:
        return 0.0
    return round(ImageStat.Stat(im.crop((x0, y0, x1, y1))).mean[0], 2)


def record(checks: list[dict], name: str, ok: bool, detail) -> None:
    checks.append({"name": name, "ok": bool(ok), "detail": detail})
    print(f"  [{'PASS' if ok else 'FAIL'}] {name:42} {detail}")


def main(argv: list[str]) -> int:
    sys.stdout.reconfigure(encoding="utf-8")
    opts = dict(zip(argv[0::2], argv[1::2]))
    out = Path(opts.get("--out", "Logs/parity/surface_after"))
    exe = opts.get("--launch-rust")
    out.mkdir(parents=True, exist_ok=True)
    proc = launch_rust_same_desktop(Path(exe)) if exe else None
    checks: list[dict] = []

    try:
        rp.send("probe_resize", w=1280.0, h=800.0)
        rp.deep_link(f"fluxer://channels/{GUILD}/{CHANNEL}")
        time.sleep(1.5)
        for _ in range(2):
            rp.send("probe_key", keystroke="escape")
        rp.send("probe_hover", x=1240.0, y=760.0)
        time.sleep(0.6)
        rest = rp.send("probe_inspect")
        rest_png = rp.screenshot(out / "rest.png")

        # 1. Seam over user-area at x=72 & sharp top-left corner on member-list
        print("=== Frame Seam & Member Corner ===")
        us = find_flx(rest, FLX_USER_SECTION)
        if us:
            ur = [round(v) for v in us[0]["rect"]]
            strips = [(ur[1] + 2, ur[1] + 8), (ur[1] + ur[3] - 8, ur[1] + ur[3] - 2)]
            deltas = []
            sample_detail = []
            for y0, y1 in strips:
                l70 = mean_luma(rest_png, (70, y0, 72, y1))
                l72 = mean_luma(rest_png, (72, y0, 73, y1))
                l74 = mean_luma(rest_png, (74, y0, 76, y1))
                deltas.append(round(abs(l72 - (l70 + l74) / 2.0), 2))
                sample_detail.append({"y": [y0, y1], "l70": l70, "l72": l72, "l74": l74})
            seam_delta = max(deltas)
            record(checks, "frame.user_area_no_vertical_seam", seam_delta <= 4.0, {"deltas": deltas, "delta": seam_delta, "strips": sample_detail})
        else:
            record(checks, "frame.user_area_no_vertical_seam", False, "missing user-area section")

        ml = find_flx(rest, FLX_MEMBER_LIST)
        if ml:
            mr = [round(v) for v in ml[0]["rect"]]
            c_tl = mean_luma(rest_png, (mr[0] + 1, mr[1], mr[0] + 4, mr[1] + 3))
            c_in = mean_luma(rest_png, (mr[0] + mr[2] - 40, mr[1] + 10, mr[0] + mr[2] - 10, mr[1] + 30))
            corner_delta = round(abs(c_tl - c_in), 2)
            record(checks, "frame.member_list_square_tl", corner_delta <= 4.0, {"corner": c_tl, "inner": c_in, "delta": corner_delta})
        else:
            record(checks, "frame.member_list_square_tl", False, "missing member list")

        # 2. Zero phantom hover buttons on guild header, channel header, and user profile
        print("=== Phantom Hover UI Guard ===")
        hover_specs = [
            ("hover.guild_header_no_phantom", FLX_GUILD_HEADER, None, FLX_GUILD_HEADER, 220),
            ("hover.channel_header_no_phantom", FLX_CH_HEADER, (120.0, 28.0), FLX_CH_HEADER, 150),
            ("hover.user_profile_no_phantom", FLX_USER_PROFILE, None, FLX_USER_SECTION, 150),
        ]
        for cname, tflx, offset, cflx, max_px in hover_specs:
            thit = find_flx(rest, tflx)
            chit = find_flx(rest, cflx)
            if not thit or not chit:
                record(checks, cname, False, "missing target/container")
                continue
            tr = thit[0]["rect"]
            if offset:
                rp.send("probe_hover", x=float(tr[0] + offset[0]), y=float(tr[1] + offset[1]))
            else:
                rp.send("probe_hover", flx=tflx)
            time.sleep(0.4)
            hpng = rp.screenshot(out / f"{cname.replace('.', '_')}.png")
            changed = bright_diff_count(rest_png, hpng, chit[0]["rect"], thresh=28)
            record(checks, cname, changed <= max_px, {"changed_bright_px": changed, "limit": max_px})
            rp.send("probe_hover", x=1240.0, y=760.0)
            time.sleep(0.25)

        # 3. Composer Media / Stickers / Emojis distinctness & toggle
        print("=== Composer Expression Picker ===")
        for label, btn_flx, expected_body, img_name in [
            ("composer.media_tab", FLX_MEDIA_BTN, FLX_MEDIA_EMPTY, "rust_expression_media.png"),
            ("composer.stickers_tab", FLX_STICKER_BTN, FLX_STICKER_EMPTY, "rust_expression_stickers.png"),
            ("composer.emojis_tab", FLX_EMOJI_BTN, FLX_EMOJI_BODY, "rust_expression_emojis.png"),
        ]:
            rp.send("probe_click", flx=btn_flx)
            time.sleep(0.45)
            d = rp.send("probe_inspect")
            rp.screenshot(out / img_name)
            tabs = [e.get("text") for e in find_flx(d, FLX_TAB)]
            body_present = bool(find_flx(d, expected_body))
            other_bodies = sum(bool(find_flx(d, b)) for b in (FLX_MEDIA_EMPTY, FLX_STICKER_EMPTY, FLX_EMOJI_BODY) if b != expected_body)
            record(checks, label, body_present and other_bodies == 0 and tabs == ["Media", "Stickers", "Emojis"], {"tabs": tabs, "body": body_present, "other_bodies": other_bodies})

        # Repeat click on Emojis toggles popout closed
        rp.send("probe_click", flx=FLX_EMOJI_BTN)
        time.sleep(0.3)
        d_closed = rp.send("probe_inspect")
        record(checks, "composer.repeat_click_closes", not find_flx(d_closed, FLX_PICKER), {"picker_present": bool(find_flx(d_closed, FLX_PICKER))})

        # 4. Settings surface contrast & structure
        print("=== Settings Modal Sanity ===")
        rp.send("probe_click", flx=FLX_SETTINGS_BTN)
        time.sleep(0.5)
        ds = rp.send("probe_inspect")
        spng = rp.screenshot(out / "rust_settings_surface.png")
        mhit = find_flx(ds, FLX_SETTINGS_MODAL)
        shit = find_flx(ds, FLX_SETTINGS_SIDEBAR)
        if mhit and shit:
            mr = [round(v) for v in mhit[0]["rect"]]
            sr = [round(v) for v in shit[0]["rect"]]
            sb_luma = mean_luma(spng, (sr[0] + sr[2] - 36, sr[1] + 100, sr[0] + sr[2] - 8, sr[1] + 150))
            ct_luma = mean_luma(spng, (sr[0] + sr[2] + 220, sr[1] + 12, sr[0] + sr[2] + 320, sr[1] + 44))
            record(checks, "settings.sidebar_lighter_than_content", sb_luma > ct_luma + 4.0, {"sidebar_luma": sb_luma, "content_luma": ct_luma, "modal": mr, "sidebar": sr})
        else:
            record(checks, "settings.sidebar_lighter_than_content", False, {"modal": len(mhit), "sidebar": len(shit)})
        rp.send("probe_key", keystroke="escape")
    finally:
        if proc is not None:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()

    passed = sum(1 for c in checks if c["ok"])
    report = {"passed": passed, "total": len(checks), "checks": checks}
    (out / "surface_report.json").write_text(json.dumps(report, indent=1), encoding="utf-8")
    print(f"\nSUMMARY: {passed}/{len(checks)} passed -> {out / 'surface_report.json'}")
    return 0 if passed == len(checks) else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
