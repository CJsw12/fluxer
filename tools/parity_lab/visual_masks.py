"""Build a shared, pixel-aligned mask for dynamic UI."""

import re
from math import ceil, floor

from PIL import Image, ImageChops

FLX = re.compile(
    r"(?:^|\.)(?:timestamp|message-timestamp|divider-timestamp|relative-time)$"
    r"|presence|status-indicator|status-dot|online-count|member-count|activity-text"
    r"|avatar|guild-icon|server-icon|unread-badge|unread-count|mention-badge|notification-badge"
    r"|typing-indicator|typing-users|toast|update-banner|update-icon"
)
TEXT = re.compile(r"tester[12](?:#\d+)?|Online(?:—\d+)?|\d{1,2}:\d{2}(?: AM| PM)?")


def bounds(rect, size):
    x, y, w, h = rect
    return max(0, floor(x)), max(0, floor(y)), min(size[0], ceil(x + w)), min(size[1], ceil(y + h))


def build(scene, official, rust, size, offset=(0, 0)):
    mask = Image.new("1", size)
    rects = list(scene.get("masks", []))
    for is_rust, rows in ((False, official), (True, rust)):
        for row in rows:
            x, y, w, h = row["rect"]
            if (FLX.search(row["flx"]) or (row.get("tag") == "img" and w * h <= 4096)
                    or (is_rust and TEXT.fullmatch((row.get("text") or "").strip()))):
                dx, dy = (0, 0) if is_rust else offset
                rects.append((x + dx, y + dy, w, h))
    for rect in rects:
        x0, y0, x1, y1 = bounds(rect, size)
        if x1 > x0 and y1 > y0:
            mask.paste(1, (x0, y0, x1, y1))
    return mask


def crop_panel(a, b, box, path):
    a, b = a.crop(box), b.crop(box)
    diff = ImageChops.difference(a, b).point(lambda v: min(255, v * 3))
    panel = Image.new("RGB", (a.width * 3, a.height))
    for index, image in enumerate((a, b, diff)):
        panel.paste(image, (index * a.width, 0))
    path.parent.mkdir(parents=True, exist_ok=True)
    panel.save(path)
