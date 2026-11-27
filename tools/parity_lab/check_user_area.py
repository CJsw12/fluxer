"""Live regression checks for profile menus, sidebar clipping and dashed rail buttons."""

import json
import math
import re
import subprocess
import sys
from pathlib import Path

from PIL import Image

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import rust_probe as rp  # noqa: E402

USER = "app.user-area.user-area-inner.focus-ring-wrapper"
POPOUT = "app.floating.user-area-popout."
CARD = POPOUT + "container"
STATUS = POPOUT + "status-menu.status-menu"
EXPIRY = POPOUT + "status-menu.expiry-popup"
GUILD = "app.guild-header.header-wrapper"
FRAME = "app.outline-frame.frame"
CHANNEL = "app.channel-item-content.channel-name"
RAIL = "app.sidebar-nav."
MARKDOWN = "fluxer://channels/1572284491404148736/1572284491404148743"


def command(name, **fields):
    result = rp.send("probe_" + name, **fields)
    if not result.get("ok"):
        raise RuntimeError(f"{name}: {result.get('error')}")
    return result


def element(data, flx, text=None):
    return next((el for el in data["elements"]
                 if el["flx"] == flx and (text is None or el.get("text") == text)), None)


def rect(data, flx, text=None):
    found = element(data, flx, text)
    if found is None:
        raise RuntimeError(f"missing {flx} {text or ''}")
    return found["rect"]


def shot(out, name, data):
    path = out / name
    result = rp.shot(path)
    if not result.get("ok"):
        raise RuntimeError(result.get("error"))
    with Image.open(path) as raw:
        return raw.convert("RGB").resize(tuple(map(round, data["viewport"])), Image.Resampling.BOX)


def distance(a, b):
    return max(abs(x - y) for x, y in zip(a, b))


def hover_fill_top(image, x, frame_y, depth=8):
    """First row (from frame_y - 4) whose colour matches the hover fill sampled `depth` px below the frame."""
    fill = image.getpixel((x, frame_y + depth))
    return next((row for row in range(frame_y - 4, frame_y + depth + 1)
                 if distance(image.getpixel((x, row)), fill) <= 3), None)


def count_dashes(image, bounds):
    x, y, w, h = bounds
    cx, cy = x + w / 2 - 0.5, y + h / 2 - 0.5
    background = image.getpixel((round(cx), round(y + 5)))
    pixels = {
        (col, row)
        for col in range(round(x) - 1, round(x + w) + 1)
        for row in range(round(y) - 1, round(y + h) + 1)
        if w / 2 - 4 < math.hypot(col - cx, row - cy) < w / 2 + 1
        and distance(image.getpixel((col, row)), background) > 10
    }
    count = 0
    while pixels:
        component = [pixels.pop()]
        for col, row in component:
            for dx in (-1, 0, 1):
                for dy in (-1, 0, 1):
                    neighbor = col + dx, row + dy
                    if neighbor in pixels:
                        pixels.remove(neighbor)
                        component.append(neighbor)
        count += len(component) >= 3
    return count


def main(out):
    out.mkdir(parents=True, exist_ok=True)
    checks = []

    def check(name, passed, detail=""):
        checks.append(bool(passed))
        print(f"{'PASS' if passed else 'FAIL'} {name} {detail}", flush=True)

    try:
        reference = subprocess.run(["node", str(HERE / "user_area_reference.mjs"), str(out)],
                                   capture_output=True, text=True, timeout=20)
        if reference.returncode:
            raise RuntimeError(reference.stderr.strip())
        official = json.loads(reference.stdout)
        command("resize", w=1280, h=800)
        rp.deep_link(MARKDOWN)
        if not rp.markdown_ready(rp.inspect_until(rp.markdown_ready, 3)):
            raise RuntimeError("markdown channel did not open")
        command("key", keystroke="escape")
        outside = {"x": 990, "y": 220}

        for theme in ("dark", "light"):
            rp.deep_link(f"fluxer://scene/theme/{theme}")
            command("click", flx=USER)
            data = command("inspect")
            x, y, w, h = map(round, rect(data, CARD))
            image = shot(out, f"profile_{theme}.png", data)
            expected = tuple(map(int, re.findall(r"\d+", official[theme])))
            points = [(x + 4, y + 130), (x + w - 5, y + 215),
                      (x + w // 2, y + 217), (x + w // 2, y + h - 5)]
            actual = [image.getpixel(point) for point in points]
            check(f"{theme} card interior and edges", all(distance(p, expected) <= 5 for p in actual),
                  f"expected {expected}, sampled {actual}")
            command("click", **outside)
            check(f"{theme} profile outside click", element(command("inspect"), CARD) is None)

        rp.deep_link("fluxer://scene/theme/dark")
        command("click", flx=USER)
        command("hover", flx=POPOUT + "action-button")
        sx, sy, sw, _ = rect(command("inspect"), STATUS)
        command("hover", x=sx + sw / 2, y=sy + 55)
        data = command("inspect")
        vw, vh = data["viewport"]
        for overlay in (CARD, STATUS, EXPIRY):
            x, y, w, h = rect(data, overlay)
            check(f"{overlay} in viewport", 0 <= x and 0 <= y and x + w <= vw and y + h <= vh,
                  (x, y, w, h))
        _, y, w, h = rect(data, EXPIRY)
        ref = official["expiry"]
        check("Idle expiry flips up", abs(y - ref["y"]) <= 3 and w == ref["width"] and h == ref["height"],
              f"Rust {(y, w, h)} vs official {(ref['y'], ref['width'], ref['height'])}")
        shot(out, "user6_fix_popout.png", data)
        command("click", **outside)
        data = command("inspect")
        check("nested profile outside click", not any(element(data, k) for k in (CARD, STATUS, EXPIRY)))

        for trigger, popup in (
            (GUILD, "guild.guild-header-popout.container"),
            ("channel.channel-header-components.channel-pins-button.channel-header-icon",
             "channel.channel-pins-popout.container"),
            ("threads.threads-button.channel-header-icon", "threads.thread-browser.container"),
        ):
            command("click", flx=trigger)
            opened = rp.inspect_until(lambda d: bool(element(d, popup)), 1)
            command("click", **outside)
            check(f"{popup} outside click", bool(element(opened, popup)) and not element(command("inspect"), popup))

        command("hover", **outside)
        data = command("inspect")
        resting = shot(out, "sidebar_rest.png", data)
        command("hover", flx=GUILD)
        data = command("inspect")
        hovered = shot(out, "user6_fix_hover_rail.png", data)
        x, y, w, _ = map(round, rect(data, GUILD))
        frame_y = round(rect(data, FRAME)[1])
        corner = (x + 1, y + 1)
        official_hover = Image.open(out / "official_hover.png").convert("RGB")
        cx = x + w // 2
        top_rows = {row: (hovered.getpixel((cx, row)), official_hover.getpixel((cx, row)))
                    for row in (frame_y, frame_y + 1)}
        rust_top, official_top = hover_fill_top(hovered, cx, frame_y), hover_fill_top(official_hover, cx, frame_y)
        check("guild hover top rows match official (frame line + fill start)",
              all(distance(r, o) <= 3 for r, o in top_rows.values()) and rust_top == official_top is not None,
              f"rows {top_rows} fill_top Rust {rust_top} official {official_top} header_y {y} frame_y {frame_y}")
        check("guild hover clipped at rounded corner",
              distance(resting.getpixel(corner), hovered.getpixel(corner)) <= 2)
        for button in ("discovery-button", "add-guild-button"):
            flx = RAIL + button + ".div--2"
            official_dashes = count_dashes(official_hover, rect(data, flx))
            dashes = count_dashes(hovered, rect(data, flx))
            check(f"{button} has 13 dashes", dashes == official_dashes == 13, f"Rust {dashes}, official {official_dashes}")

        x, y, _, h = map(round, rect(data, CHANNEL, "general"))
        bg = hovered.getpixel((x - 2, y + h - 2))
        def glyph_rows(image):
            return [sum(distance(image.getpixel((col, row)), bg) > 28 for col in range(x, x + 9))
                    for row in range(y - 1, y + h + 4)]

        ink, reference_ink = glyph_rows(hovered), glyph_rows(official_hover)
        painted = [i for i, count in enumerate(ink) if count]
        check("general descender matches official painted height and lower edge",
              bool(painted) and sum(bool(n) for n in ink) == sum(bool(n) for n in reference_ink)
              and abs(ink[painted[-1]] - next(n for n in reversed(reference_ink) if n)) <= 1
              and painted[-1] < len(ink) - 1, f"Rust {ink}, official {reference_ink}")
    finally:
        command("key", keystroke="escape")
        rp.deep_link(MARKDOWN)
        rp.deep_link("fluxer://scene/theme/dark")

    print(f"SUMMARY: {sum(checks)}/{len(checks)} passed")
    return int(not checks or not all(checks))


if __name__ == "__main__":
    raise SystemExit(main(Path(sys.argv[1])))
