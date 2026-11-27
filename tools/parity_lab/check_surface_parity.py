"""Live M9.x surface, phantom-hover, seam/corner pixel, and expression-picker semantic parity gate.

    python -I tools/parity_lab/check_surface_parity.py --launch-rust dist/fluxer_client/fluxer_client.exe --out Logs/parity/surface_after
    python -I tools/parity_lab/check_surface_parity.py --out DIR [--only-pixels]
    python -I tools/parity_lab/check_surface_parity.py --selftest

Official-reference pixel checks (dark and light, official captured by surface_reference.mjs over
CDP 9334 with a theme-class swap only): user-card outer band pixels (200,729)/(200,795), settings
modal 4-corner 12x12 patches (official modal rect; pixels outside the 12px arc must match, excluding pixels whose
rest captures already differ, and at least half of them must remain), y=32 row mean luma over the channel-list column,
and composer Media/Stickers/Emojis glyph (solid-ink bbox +-1 px and ink mass +-8%). Pixels within +-3.
`--only-pixels` runs just these official-reference checks.
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
HERE = Path(__file__).resolve().parent
PIXEL_TOL = 3.0
GLYPH_TOL = 1
INK_TOL = 0.08  # rust/official ink mass; a 22px vs 20px glyph is ~1.21
BAND_POINTS = ((200, 729), (200, 795))
ROW_Y32 = (73, 32, 393, 33)  # x0, y0, x1, y1: channel-list column, first content row under the titlebar
CORNER = 12
COMPOSER_BUTTONS = (("media", 890, 748), ("stickers", 926, 748), ("emojis", 962, 748))


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


def load_rgb(png: Path):
    from PIL import Image  # noqa: PLC0415

    im = Image.open(png).convert("RGB")
    if im.size != (1280, 800):
        im = im.resize((1280, 800), Image.Resampling.BOX)
    return im


def pixel_delta(a, b) -> int:
    return max(abs(x - y) for x, y in zip(a, b))


def patch_mean_delta(a, b, box: tuple[int, int, int, int]) -> float:
    """Mean per-pixel max-channel |a-b| over box (x0, y0, x1, y1)."""
    from PIL import ImageChops, ImageStat  # noqa: PLC0415

    diff = ImageChops.difference(a.crop(box), b.crop(box))
    return round(max(ImageStat.Stat(diff).mean), 2)


def corner_boxes(rect: list[float], size: int = CORNER) -> dict[str, tuple[int, int, int, int]]:
    x0, y0 = round(rect[0]), round(rect[1])
    x1, y1 = round(rect[0] + rect[2]), round(rect[1] + rect[3])
    return {
        "tl": (x0, y0, x0 + size, y0 + size),
        "tr": (x1 - size, y0, x1, y0 + size),
        "bl": (x0, y1 - size, x0 + size, y1),
        "br": (x1 - size, y1 - size, x1, y1),
    }


MIN_EXTERIOR_SHARE = 0.5  # fewer usable outside-arc pixels than this share -> FAIL (cannot judge)


def corner_exterior_delta(a, b, box: tuple[int, int, int, int], corner: str, radius: int = CORNER,
                          rest_a=None, rest_b=None) -> dict:
    """Mean max-channel |a-b| over the patch pixels outside the rounded corner (arc + 1px AA band).

    With rest_a/rest_b (same-theme captures without the modal), pixels where the app content under
    the backdrop already differs by more than PIXEL_TOL are excluded (they are not modal pixels).
    """
    x0, y0, x1, y1 = box
    cx = x0 + radius if corner in ("tl", "bl") else x1 - radius
    cy = y0 + radius if corner in ("tl", "tr") else y1 - radius
    pts = [(x, y) for y in range(y0, y1) for x in range(x0, x1)
           if ((x + 0.5 - cx) ** 2 + (y + 0.5 - cy) ** 2) ** 0.5 > radius + 1]
    if rest_a is not None and rest_b is not None:
        used = [pt for pt in pts if pixel_delta(rest_a.getpixel(pt), rest_b.getpixel(pt)) <= PIXEL_TOL]
    else:
        used = pts
    vals = [pixel_delta(a.getpixel(pt), b.getpixel(pt)) for pt in used]
    return {
        "delta": round(sum(vals) / len(vals), 2) if vals else None,
        "used": len(used),
        "excluded": len(pts) - len(used),
        "total": len(pts),
    }


def corner_ok(res: dict) -> bool:
    return res["delta"] is not None and res["used"] >= res["total"] * MIN_EXTERIOR_SHARE and res["delta"] <= PIXEL_TOL


def row_luma(im, box: tuple[int, int, int, int]) -> float:
    from PIL import ImageStat  # noqa: PLC0415

    return round(ImageStat.Stat(im.convert("L").crop(box)).mean[0], 2)


def glyph_bbox(im, x: int, y: int, size: int = 32, thresh: int = 80) -> list[int] | None:
    """[w, h] of the solid ink (>thresh from the corner background) inside a size x size button."""
    bg = im.getpixel((x + 1, y + 1))
    xs, ys = [], []
    for row in range(y + 2, y + size - 2):
        for col in range(x + 2, x + size - 2):
            if pixel_delta(im.getpixel((col, row)), bg) > thresh:
                xs.append(col)
                ys.append(row)
    if not xs:
        return None
    return [max(xs) - min(xs) + 1, max(ys) - min(ys) + 1]


def ink_mass(im, x: int, y: int, size: int = 32) -> float:
    """Sum of per-pixel max-channel distance from the corner background, in full-ink pixels."""
    bg = im.getpixel((x + 1, y + 1))
    return round(sum(pixel_delta(im.getpixel((c, r)), bg) for r in range(y, y + size) for c in range(x, x + size)) / 255.0, 1)


def save_zoom(im, box: tuple[int, int, int, int], scale: int, out: Path) -> None:
    from PIL import Image  # noqa: PLC0415

    crop = im.crop(box)
    crop.resize((crop.width * scale, crop.height * scale), Image.Resampling.NEAREST).save(out)


def official_reference(out: Path) -> dict:
    r = subprocess.run(
        ["node", str(HERE / "surface_reference.mjs"), str(out)],
        cwd=REPO, capture_output=True, text=True, encoding="utf-8", timeout=60,
    )
    if r.returncode != 0:
        raise RuntimeError(f"surface_reference.mjs exit {r.returncode}: {r.stderr.strip()[-300:]}")
    return json.loads(r.stdout.strip().splitlines()[-1])


def pixel_checks(checks: list[dict], out: Path) -> None:
    """Official-reference pixel checks in dark and light; leaves Rust dark with no modal open."""
    print("=== Official Reference Pixels (dark/light) ===")
    try:
        ref = official_reference(out)
    except Exception as exc:  # noqa: BLE001
        for theme in ("dark", "light"):
            for name in ("user_card_outer_band", "settings_modal_corners", "header_row_y32"):
                record(checks, f"pixel.{name}_{theme}", False, f"official reference failed: {exc}")
        for name, _, _ in COMPOSER_BUTTONS:
            record(checks, f"composer.glyph_bbox_{name}", False, f"official reference failed: {exc}")
        return
    modal_rect = ref["modal"]
    try:
        for theme in ("dark", "light"):
            rp.deep_link(f"fluxer://scene/theme/{theme}")
            time.sleep(0.5)
            rp.send("probe_hover", x=1240.0, y=760.0)
            time.sleep(0.4)
            rs = load_rgb(rp.screenshot(out / f"rust_rest_{theme}.png"))
            off = load_rgb(out / f"official_rest_{theme}.png")
            samples = [{"pt": pt, "official": off.getpixel(pt), "rust": rs.getpixel(pt)} for pt in BAND_POINTS]
            worst = max(pixel_delta(s_["official"], s_["rust"]) for s_ in samples)
            record(checks, f"pixel.user_card_outer_band_{theme}", worst <= PIXEL_TOL, {"max_delta": worst, "samples": samples})
            l_off, l_rs = row_luma(off, ROW_Y32), row_luma(rs, ROW_Y32)
            record(checks, f"pixel.header_row_y32_{theme}", abs(l_off - l_rs) <= PIXEL_TOL,
                   {"official_luma": l_off, "rust_luma": l_rs, "row": list(ROW_Y32)})
            if theme == "dark":
                for name, bx, by in COMPOSER_BUTTONS:
                    g_off, g_rs = glyph_bbox(off, bx, by), glyph_bbox(rs, bx, by)
                    m_off, m_rs = ink_mass(off, bx, by), ink_mass(rs, bx, by)
                    ratio = round(m_rs / m_off, 3) if m_off else None
                    ok = bool(g_off and g_rs and ratio) and all(abs(a - b) <= GLYPH_TOL for a, b in zip(g_off, g_rs))                         and abs(ratio - 1.0) <= INK_TOL
                    record(checks, f"composer.glyph_bbox_{name}", ok,
                           {"official_wh": g_off, "rust_wh": g_rs, "ink_ratio": ratio, "button": [bx, by, 32, 32]})
            rp.send("probe_click", flx=FLX_SETTINGS_BTN)
            rp.inspect_until(lambda frame: bool(find_flx(frame, FLX_SETTINGS_SIDEBAR)), 1.5)
            rp.send("probe_hover", x=640.0, y=4.0)
            time.sleep(0.8)
            rs_m = load_rgb(rp.screenshot(out / f"rust_settings_{theme}.png"))
            off_m = load_rgb(out / f"official_settings_{theme}.png")
            boxes = corner_boxes(modal_rect)
            outside = {k: corner_exterior_delta(off_m, rs_m, box, k, rest_a=off, rest_b=rs) for k, box in boxes.items()}
            deltas = {k: patch_mean_delta(off_m, rs_m, box) for k, box in boxes.items()}
            record(checks, f"pixel.settings_modal_corners_{theme}", all(corner_ok(v) for v in outside.values()),
                   {"outside_arc_delta": outside, "patch_mean_delta": deltas, "modal": [round(v, 1) for v in modal_rect]})
            rp.send("probe_key", keystroke="escape")
            time.sleep(0.3)
    finally:
        rp.send("probe_key", keystroke="escape")
        rp.deep_link("fluxer://scene/theme/dark")
        time.sleep(0.3)


def selftest() -> None:
    from PIL import Image  # noqa: PLC0415

    assert corner_boxes([77.8, 52, 1124.4, 728]) == {
        "tl": (78, 52, 90, 64), "tr": (1190, 52, 1202, 64), "bl": (78, 768, 90, 780), "br": (1190, 768, 1202, 780),
    }
    a = Image.new("RGB", (1280, 800), (26, 24, 30))
    b = Image.new("RGB", (1280, 800), (12, 11, 14))
    assert patch_mean_delta(a, a, (0, 0, 12, 12)) == 0
    assert patch_mean_delta(a, b, (0, 0, 12, 12)) == 16
    sq = Image.new("RGB", (40, 40), (4, 4, 5))
    sq_rust = sq.copy()
    sq_rust.paste((42, 39, 48), (0, 0, 40, 40))
    sq_res = corner_exterior_delta(sq, sq_rust, (0, 0, 12, 12), "tl")
    assert sq_res["delta"] > 30 and sq_res["excluded"] == 0 and not corner_ok(sq_res)
    assert corner_ok(corner_exterior_delta(sq, sq, (0, 0, 12, 12), "br"))
    # content under the backdrop differs in one rest pixel: excluded, the rest still judged
    rest_a, rest_b = sq.copy(), sq.copy()
    rest_b.putpixel((0, 0), (100, 138, 185))
    bad = sq.copy()
    bad.putpixel((0, 0), (100, 138, 185))
    res = corner_exterior_delta(sq, bad, (0, 0, 12, 12), "tl", rest_a=rest_a, rest_b=rest_b)
    assert res["excluded"] == 1 and res["delta"] == 0 and corner_ok(res), res
    assert not corner_ok(corner_exterior_delta(sq, sq_rust, (0, 0, 12, 12), "tl", rest_a=rest_a, rest_b=rest_b))
    # too many excluded pixels -> cannot judge -> FAIL
    res = corner_exterior_delta(sq, sq, (0, 0, 12, 12), "tl", rest_a=sq, rest_b=sq_rust)
    assert res["used"] == 0 and not corner_ok(res), res
    assert row_luma(a, ROW_Y32) > row_luma(b, ROW_Y32) + 3
    assert pixel_delta((26, 24, 30), (12, 11, 14)) == 16
    for x in range(895, 915):
        for y in range(753, 773):
            a.putpixel((x, y), (200, 200, 200))
    assert glyph_bbox(a, 890, 748) == [20, 20]
    assert glyph_bbox(b, 890, 748) is None
    assert ink_mass(a, 890, 748) > 0 and ink_mass(b, 890, 748) == 0
    print("check_surface_parity selftest: OK")


def record(checks: list[dict], name: str, ok: bool, detail) -> None:
    checks.append({"name": name, "ok": bool(ok), "detail": detail})
    print(f"  [{'PASS' if ok else 'FAIL'}] {name:42} {detail}")


class _PixelsOnlyDone(Exception):
    pass


def main(argv: list[str]) -> int:
    sys.stdout.reconfigure(encoding="utf-8")
    if "--selftest" in argv:
        selftest()
        return 0
    only_pixels = "--only-pixels" in argv
    argv = [a for a in argv if a != "--only-pixels"]
    opts = dict(zip(argv[0::2], argv[1::2]))
    out = Path(opts.get("--out", "Logs/parity/surface_after"))
    exe = opts.get("--launch-rust")
    out.mkdir(parents=True, exist_ok=True)
    proc = launch_rust_same_desktop(Path(exe)) if exe else None
    checks: list[dict] = []

    try:
        rp.send("probe_resize", w=1280.0, h=800.0)
        rp.deep_link(f"fluxer://channels/{GUILD}/{CHANNEL}")
        rp.inspect_until(rp.markdown_ready, 1.5)
        for _ in range(2):
            rp.send("probe_key", keystroke="escape")
        rp.send("probe_hover", x=1240.0, y=760.0)
        time.sleep(0.6)
        rest = rp.send("probe_inspect")
        rest_png = rp.screenshot(out / "rest.png")
        if only_pixels:
            pixel_checks(checks, out)
            raise _PixelsOnlyDone

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
            d = rp.inspect_until(lambda frame: bool(find_flx(frame, expected_body)), 0.45)
            rp.screenshot(out / img_name)
            tabs = [e.get("text") for e in find_flx(d, FLX_TAB)]
            body_present = bool(find_flx(d, expected_body))
            other_bodies = sum(bool(find_flx(d, b)) for b in (FLX_MEDIA_EMPTY, FLX_STICKER_EMPTY, FLX_EMOJI_BODY) if b != expected_body)
            record(checks, label, body_present and other_bodies == 0 and tabs == ["Media", "Stickers", "Emojis"], {"tabs": tabs, "body": body_present, "other_bodies": other_bodies})

        # Repeat click on Emojis toggles popout closed
        rp.send("probe_click", flx=FLX_EMOJI_BTN)
        d_closed = rp.inspect_until(lambda frame: not find_flx(frame, FLX_PICKER), 0.3)
        record(checks, "composer.repeat_click_closes", not find_flx(d_closed, FLX_PICKER), {"picker_present": bool(find_flx(d_closed, FLX_PICKER))})

        # 4. Settings surface contrast & structure
        print("=== Settings Modal Sanity ===")
        rp.send("probe_click", flx=FLX_SETTINGS_BTN)
        ds = rp.inspect_until(lambda frame: bool(find_flx(frame, FLX_SETTINGS_SIDEBAR)), 0.5)
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
        time.sleep(0.3)

        # 5. Official-reference pixels (user-card band, modal corners, y=32 row, composer glyphs)
        pixel_checks(checks, out)
    except _PixelsOnlyDone:
        pass
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
