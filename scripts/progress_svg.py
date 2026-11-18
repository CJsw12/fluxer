#!/usr/bin/env python3
"""Generate progress badges and a progress map from docs/progress/progress.json.

Standard library only. Run from the repository root:

    python scripts/progress_svg.py
"""

from __future__ import annotations

import json
import math
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PROGRESS_DIR = ROOT / "docs" / "progress"
INPUT = PROGRESS_DIR / "progress.json"

FONT_STACK = (
    "Segoe UI, Malgun Gothic, Apple SD Gothic Neo, Noto Sans KR, "
    "Noto Sans CJK KR, sans-serif"
)

COLOR_DONE = "#3fb950"
COLOR_PARTIAL = "#d29922"
COLOR_TODO = "#484f58"
COLOR_BG = "#0d1117"
COLOR_TEXT = "#e6edf3"
COLOR_MUTED = "#8b949e"
COLOR_TRACK = "#21262d"

STATUS_LABEL = {
    "done": "완료",
    "partial": "부분",
    "todo": "미착수",
}
STATUS_COLOR = {
    "done": COLOR_DONE,
    "partial": COLOR_PARTIAL,
    "todo": COLOR_TODO,
}
STATUS_WEIGHT = {"done": 1.0, "partial": 0.5, "todo": 0.0}

# Areas left out of the "daily driver" badge.
DAILY_DRIVER_EXCLUDED = {"voice", "guilds", "search", "i18n"}


def esc(text: str) -> str:
    """Escape text for both XML content and double-quoted attributes."""
    return (
        str(text)
        .replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace('"', "&quot;")
        .replace("'", "&apos;")
    )


def score(items: list[dict]) -> float:
    return sum(STATUS_WEIGHT.get(item["status"], 0.0) for item in items)


def percent(items: list[dict]) -> float:
    """Return the completion percentage, done=1 and partial=0.5."""
    if not items:
        return 0.0
    return 100.0 * score(items) / len(items)


def percent_label(items: list[dict]) -> str:
    return "%d%%" % int(math.floor(percent(items) + 0.5 + 1e-9))


def text_width(text: str, font_size: float = 11.0) -> float:
    """Approximate rendered width; wide CJK glyphs count as a full em."""
    width = 0.0
    for ch in text:
        code = ord(ch)
        if code >= 0x1100 and (
            0x1100 <= code <= 0x11FF
            or 0x2E80 <= code <= 0xA4CF
            or 0xAC00 <= code <= 0xD7A3
            or 0xF900 <= code <= 0xFAFF
            or 0xFE30 <= code <= 0xFE4F
            or 0xFF00 <= code <= 0xFF60
            or 0xFFE0 <= code <= 0xFFE6
        ):
            width += font_size
        elif ch == " ":
            width += 0.34 * font_size
        elif ch in "ilI.,:;'|!`":
            width += 0.3 * font_size
        elif ch in "mwMW%":
            width += 0.85 * font_size
        elif ch.isdigit() or ch.isupper():
            width += 0.64 * font_size
        else:
            width += 0.58 * font_size
    return width


def badge_color(value: float) -> str:
    if value >= 80.0:
        return COLOR_DONE
    if value >= 50.0:
        return COLOR_PARTIAL
    return "#da3633"


def write_badge(path: Path, label: str, value: str, value_percent: float) -> None:
    label_w = int(math.ceil(text_width(label))) + 16
    value_w = int(math.ceil(text_width(value))) + 16
    total_w = label_w + value_w
    color = badge_color(value_percent)
    label_x = label_w / 2.0
    value_x = label_w + value_w / 2.0
    aria = "%s: %s" % (label, value)

    svg = (
        '<svg xmlns="http://www.w3.org/2000/svg" width="%d" height="20" '
        'role="img" aria-label="%s">\n'
        "  <title>%s</title>\n"
        '  <clipPath id="round"><rect width="%d" height="20" rx="3"/></clipPath>\n'
        '  <g clip-path="url(#round)">\n'
        '    <rect width="%d" height="20" fill="#555555"/>\n'
        '    <rect x="%d" width="%d" height="20" fill="%s"/>\n'
        "  </g>\n"
        '  <g fill="#ffffff" font-family="%s" font-size="11" text-anchor="middle">\n'
        '    <text x="%.1f" y="14">%s</text>\n'
        '    <text x="%.1f" y="14" font-weight="600">%s</text>\n'
        "  </g>\n"
        "</svg>\n"
    ) % (
        total_w,
        esc(aria),
        esc(aria),
        total_w,
        label_w,
        label_w,
        value_w,
        color,
        esc(FONT_STACK),
        label_x,
        esc(label),
        value_x,
        esc(value),
    )
    path.write_text(svg, encoding="utf-8")


def write_map(path: Path, data: dict) -> None:
    areas = data["areas"]

    width = 900
    left_name = 24
    bar_x = 270
    bar_w = 190
    percent_x = 474
    grid_x = 510
    pitch = 14
    row_h = 34
    top = 74
    bottom = top + row_h * len(areas)
    legend_y = bottom + 30
    height = legend_y + 40

    out = []
    out.append(
        '<svg xmlns="http://www.w3.org/2000/svg" width="%d" height="%d" '
        'viewBox="0 0 %d %d" role="img" aria-label="%s">\n'
        % (
            width,
            height,
            width,
            height,
            esc("Fluxer Rust 클라이언트 진행 지도"),
        )
    )
    out.append(
        '  <rect width="%d" height="%d" fill="%s"/>\n' % (width, height, COLOR_BG)
    )
    out.append(
        '  <text x="%d" y="38" fill="%s" font-family="%s" font-size="18" '
        'font-weight="600">%s</text>\n'
        % (left_name, COLOR_TEXT, esc(FONT_STACK), esc("Fluxer Rust 클라이언트 진행 지도"))
    )
    out.append(
        '  <text x="%d" y="38" fill="%s" font-family="%s" font-size="12" '
        'text-anchor="end">%s</text>\n'
        % (
            width - left_name,
            COLOR_MUTED,
            esc(FONT_STACK),
            esc("기준일 %s" % data.get("updated", "")),
        )
    )
    out.append(
        '  <line x1="%d" y1="54" x2="%d" y2="54" stroke="%s" stroke-width="1"/>\n'
        % (left_name, width - left_name, COLOR_TRACK)
    )

    for index, area in enumerate(areas):
        items = area["items"]
        row_y = top + index * row_h
        pct = percent(items)
        label = percent_label(items)
        bar_fill = max(0.0, min(1.0, pct / 100.0)) * bar_w

        out.append(
            '  <text x="%d" y="%.1f" fill="%s" font-family="%s" font-size="13">%s</text>\n'
            % (left_name, row_y + 18, COLOR_TEXT, esc(FONT_STACK), esc(area["name"]))
        )
        out.append(
            '  <rect x="%d" y="%.1f" width="%d" height="10" rx="5" fill="%s"/>\n'
            % (bar_x, row_y + 7, bar_w, COLOR_TRACK)
        )
        if bar_fill > 0:
            out.append(
                '  <rect x="%d" y="%.1f" width="%.1f" height="10" rx="5" fill="%s"/>\n'
                % (
                    bar_x,
                    row_y + 7,
                    bar_fill,
                    COLOR_DONE if pct >= 100.0 else "#58a6ff",
                )
            )
        out.append(
            '  <text x="%d" y="%.1f" fill="%s" font-family="%s" font-size="12" '
            'text-anchor="end">%s</text>\n'
            % (percent_x, row_y + 17, COLOR_MUTED, esc(FONT_STACK), esc(label))
        )

        for item_index, item in enumerate(items):
            status = item["status"]
            color = STATUS_COLOR.get(status, COLOR_TODO)
            x = grid_x + item_index * pitch
            tooltip = "%s : %s" % (
                item["name"],
                STATUS_LABEL.get(status, status),
            )
            out.append(
                '  <g><title>%s</title><rect x="%d" y="%.1f" width="10" height="10" '
                'rx="2" fill="%s"/></g>\n'
                % (esc(tooltip), x, row_y + 7, color)
            )

    # Legend.
    out.append(
        '  <text x="%d" y="%.1f" fill="%s" font-family="%s" font-size="12">%s</text>\n'
        % (left_name, legend_y + 10, COLOR_MUTED, esc(FONT_STACK), esc("범례"))
    )
    legend_x = left_name + 48
    for status in ("done", "partial", "todo"):
        out.append(
            '  <rect x="%d" y="%.1f" width="10" height="10" rx="2" fill="%s"/>\n'
            % (legend_x, legend_y + 1, STATUS_COLOR[status])
        )
        out.append(
            '  <text x="%d" y="%.1f" fill="%s" font-family="%s" font-size="12">%s</text>\n'
            % (
                legend_x + 16,
                legend_y + 10,
                COLOR_TEXT,
                esc(FONT_STACK),
                esc(STATUS_LABEL[status]),
            )
        )
        legend_x += 16 + int(text_width(STATUS_LABEL[status], 12)) + 26

    out.append(
        '  <text x="%d" y="%.1f" fill="%s" font-family="%s" font-size="11" '
        'text-anchor="end">%s</text>\n'
        % (
            width - left_name,
            legend_y + 10,
            COLOR_MUTED,
            esc(FONT_STACK),
            esc("완료 1점 · 부분 0.5점"),
        )
    )
    out.append("</svg>\n")
    path.write_text("".join(out), encoding="utf-8")


def main() -> None:
    data = json.loads(INPUT.read_text(encoding="utf-8"))
    areas = data["areas"]

    all_items = [item for area in areas for item in area["items"]]
    overall = percent(all_items)

    daily_areas = [area for area in areas if area["key"] not in DAILY_DRIVER_EXCLUDED]
    daily_items = [item for area in daily_areas for item in area["items"]]
    daily = percent(daily_items)

    write_badge(
        PROGRESS_DIR / "badge-overall.svg",
        "Rust 클라이언트",
        percent_label(all_items),
        overall,
    )
    write_badge(
        PROGRESS_DIR / "badge-daily-driver.svg",
        "일상 사용",
        percent_label(daily_items),
        daily,
    )
    write_map(PROGRESS_DIR / "progress.svg", data)

    print("overall: %s (%d items)" % (percent_label(all_items), len(all_items)))
    print(
        "daily driver: %s (%d items, %d areas)"
        % (percent_label(daily_items), len(daily_items), len(daily_areas))
    )


if __name__ == "__main__":
    main()
