#!/usr/bin/env python3
"""Offline visual gallery capture & regional parity diff tool for Fluxer GPUI.

Usage:
  # 1. Capture a single top-level Win32 window by PID (called by --visual-gallery):
  python -I scripts/visual_parity_diff.py --capture-pid 1234 --out Logs/visual-gallery/01_general.png

  # 2. Run the full 12-scenario offline visual gallery in one command:
  python -I scripts/visual_parity_diff.py --run-gallery --out Logs/visual-gallery

  # 3. Compare baseline vs candidate screenshots with regional diff metrics:
  python -I scripts/visual_parity_diff.py --diff --baseline <dir_or_png> --candidate <dir_or_png> --out Logs/visual-diff
"""

from __future__ import annotations

import argparse
import ctypes
import ctypes.wintypes as wt
import json
import math
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

from PIL import Image, ImageChops

sys.stdout.reconfigure(encoding="utf-8")

PW_RENDERFULLCONTENT = 0x00000002
DIB_RGB_COLORS = 0
BI_RGB = 0
SRCCOPY = 0x00CC0020


class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [
        ("biSize", wt.DWORD),
        ("biWidth", wt.LONG),
        ("biHeight", wt.LONG),
        ("biPlanes", wt.WORD),
        ("biBitCount", wt.WORD),
        ("biCompression", wt.DWORD),
        ("biSizeImage", wt.DWORD),
        ("biXPelsPerMeter", wt.LONG),
        ("biYPelsPerMeter", wt.LONG),
        ("biClrUsed", wt.DWORD),
        ("biClrImportant", wt.DWORD),
    ]


class BITMAPINFO(ctypes.Structure):
    _fields_ = [("bmiHeader", BITMAPINFOHEADER), ("bmiColors", wt.DWORD * 3)]


def _find_window_for_pid(target_pid: int, timeout_s: float = 5.0) -> int:
    user32 = ctypes.windll.user32
    deadline = time.monotonic() + timeout_s
    while time.monotonic() < deadline:
        found: list[tuple[int, int]] = []

        @ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
        def _enum_cb(hwnd: int, _lparam: int) -> bool:
            if not user32.IsWindowVisible(hwnd):
                return True
            pid = wt.DWORD(0)
            user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
            if pid.value == target_pid:
                rect = wt.RECT()
                if user32.GetWindowRect(hwnd, ctypes.byref(rect)):
                    area = max(0, rect.right - rect.left) * max(0, rect.bottom - rect.top)
                    if area > 10_000:
                        found.append((area, hwnd))
            return True

        user32.EnumWindows(_enum_cb, 0)
        if found:
            found.sort(reverse=True)
            return found[0][1]
        time.sleep(0.05)
    raise RuntimeError(f"No visible top-level window found for PID {target_pid}")


def capture_hwnd_to_png(hwnd: int, out_path: Path) -> tuple[int, int]:
    user32 = ctypes.windll.user32
    gdi32 = ctypes.windll.gdi32
    try:
        user32.SetProcessDPIAware()
    except Exception:
        pass

    rect = wt.RECT()
    if not user32.GetWindowRect(hwnd, ctypes.byref(rect)):
        raise RuntimeError(f"GetWindowRect failed for HWND {hwnd}")
    width = rect.right - rect.left
    height = rect.bottom - rect.top
    if width <= 0 or height <= 0:
        raise RuntimeError(f"Invalid window rect {width}x{height} for HWND {hwnd}")

    hwnd_dc = user32.GetWindowDC(hwnd)
    mem_dc = gdi32.CreateCompatibleDC(hwnd_dc)
    bmp = gdi32.CreateCompatibleBitmap(hwnd_dc, width, height)
    old_bmp = gdi32.SelectObject(mem_dc, bmp)
    try:
        ok = user32.PrintWindow(hwnd, mem_dc, PW_RENDERFULLCONTENT)
        if not ok:
            gdi32.BitBlt(mem_dc, 0, 0, width, height, hwnd_dc, 0, 0, SRCCOPY)

        bmi = BITMAPINFO()
        bmi.bmiHeader.biSize = ctypes.sizeof(BITMAPINFOHEADER)
        bmi.bmiHeader.biWidth = width
        bmi.bmiHeader.biHeight = -height  # top-down DIB
        bmi.bmiHeader.biPlanes = 1
        bmi.bmiHeader.biBitCount = 32
        bmi.bmiHeader.biCompression = BI_RGB

        buf = ctypes.create_string_buffer(width * height * 4)
        rows = gdi32.GetDIBits(mem_dc, bmp, 0, height, buf, ctypes.byref(bmi), DIB_RGB_COLORS)
        if rows != height:
            raise RuntimeError(f"GetDIBits returned {rows} rows, expected {height}")

        img = Image.frombuffer("RGBA", (width, height), buf.raw, "raw", "BGRA", 0, 1).convert("RGB")
        # Crop Win11 invisible DWM resize borders to the exact client area so
        # captures match logical window dimensions (e.g. 1280x800, 1920x1040).
        client_rect = wt.RECT()
        pt = wt.POINT(0, 0)
        if user32.GetClientRect(hwnd, ctypes.byref(client_rect)) and user32.ClientToScreen(
            hwnd, ctypes.byref(pt)
        ):
            cl = max(0, pt.x - rect.left)
            ct = max(0, pt.y - rect.top)
            cr = min(width, cl + (client_rect.right - client_rect.left))
            cb = min(height, ct + (client_rect.bottom - client_rect.top))
            if cr > cl and cb > ct:
                img = img.crop((cl, ct, cr, cb))
                width, height = img.size
        out_path.parent.mkdir(parents=True, exist_ok=True)
        img.save(out_path, format="PNG")
        return width, height
    finally:
        gdi32.SelectObject(mem_dc, old_bmp)
        gdi32.DeleteObject(bmp)
        gdi32.DeleteDC(mem_dc)
        user32.ReleaseDC(hwnd, hwnd_dc)


def official_regions(width: int, height: int) -> dict[str, tuple[int, int, int, int]]:
    """Returns official Fluxer layout bounding boxes for regional parity diffing."""
    tb = min(32, height)
    rail = min(72, width)
    sidebar_end = min(rail + 320, width)
    header_end = min(tb + 56, height)
    composer_top = max(header_end, height - 74)
    right_start = max(sidebar_end, width - 264)
    return {
        "titlebar": (0, 0, width, tb),
        "guild_rail": (0, tb, rail, height),
        "channel_sidebar": (rail, tb, sidebar_end, height),
        "header": (sidebar_end, tb, width, header_end),
        "main_content": (sidebar_end, header_end, right_start, composer_top),
        "composer": (sidebar_end, composer_top, right_start, height),
        "right_panel": (right_start, header_end, width, height),
    }


def _image_metrics(a: Image.Image, b: Image.Image, threshold: int = 12) -> dict[str, float]:
    diff = ImageChops.difference(a, b)
    raw = diff.tobytes()
    n_pixels = max(1, a.width * a.height)
    total_abs = 0
    total_sq = 0
    changed_pixels = 0
    for i in range(0, len(raw), 3):
        dr, dg, db = raw[i], raw[i + 1], raw[i + 2]
        total_abs += dr + dg + db
        total_sq += dr * dr + dg * dg + db * db
        if max(dr, dg, db) > threshold:
            changed_pixels += 1
    channels = n_pixels * 3
    return {
        "mae": round(total_abs / channels, 3),
        "rmse": round(math.sqrt(total_sq / channels), 3),
        "changed_pct": round((changed_pixels / n_pixels) * 100.0, 3),
    }


def diff_pair(baseline_path: Path, candidate_path: Path, out_dir: Path) -> dict[str, Any]:
    base = Image.open(baseline_path).convert("RGB")
    cand = Image.open(candidate_path).convert("RGB")
    resized = False
    if base.size != cand.size:
        cand = cand.resize(base.size, Image.Resampling.LANCZOS)
        resized = True

    overall = _image_metrics(base, cand)
    regions: dict[str, Any] = {}
    for name, box in official_regions(base.width, base.height).items():
        if box[2] > box[0] and box[3] > box[1]:
            regions[name] = {
                "box": list(box),
                **_image_metrics(base.crop(box), cand.crop(box)),
            }

    diff_img = ImageChops.difference(base, cand)
    heatmap = diff_img.point(lambda p: min(255, p * 4))
    strip = Image.new("RGB", (base.width * 3, base.height))
    strip.paste(base, (0, 0))
    strip.paste(cand, (base.width, 0))
    strip.paste(heatmap, (base.width * 2, 0))
    out_dir.mkdir(parents=True, exist_ok=True)
    strip_name = f"diff_{baseline_path.stem}.png"
    strip.save(out_dir / strip_name, format="PNG")

    return {
        "name": baseline_path.stem,
        "baseline": str(baseline_path),
        "candidate": str(candidate_path),
        "size": [base.width, base.height],
        "resized_candidate": resized,
        "overall": overall,
        "regions": regions,
        "comparison_image": strip_name,
    }


def write_gallery_html(out_dir: Path) -> None:
    manifest_path = out_dir / "manifest.json"
    scenarios = []
    if manifest_path.is_file():
        data = json.loads(manifest_path.read_text(encoding="utf-8"))
        scenarios = data.get("scenarios", [])
    cards = []
    for s in scenarios:
        cards.append(
            f'<figure style="margin:0;background:#16151a;border:1px solid #2b2935;border-radius:8px;padding:10px">'
            f'<figcaption style="font:600 13px sans-serif;color:#eee;margin-bottom:8px">'
            f'{s["index"]:02d}. {s["title"]} ({int(s["width"])}x{int(s["height"])})</figcaption>'
            f'<img src="{s["file"]}" style="width:100%;border-radius:6px;display:block"/>'
            f"</figure>"
        )
    html = (
        "<!doctype html><meta charset='utf-8'><title>Fluxer Visual Gallery</title>"
        "<body style='background:#0d0c10;color:#fff;padding:20px;font-family:sans-serif'>"
        "<h1>Fluxer GPUI 12-Scenario Offline Visual Gallery</h1>"
        "<div style='display:grid;grid-template-columns:repeat(auto-fill,minmax(560px,1fr));gap:16px'>"
        + "\n".join(cards)
        + "</div></body>"
    )
    (out_dir / "index.html").write_text(html, encoding="utf-8")


def default_client_binary() -> Path:
    candidates = [
        Path("target/fc-root/debug/fluxer_client.exe"),
        Path("target/fc-root/release/fluxer_client.exe"),
        Path("dist/fluxer_client/fluxer_client.exe"),
    ]
    for c in candidates:
        if c.is_file():
            return c
    return candidates[0]


def main() -> int:
    parser = argparse.ArgumentParser(description="Fluxer GPUI offline visual gallery & parity diff tool")
    parser.add_argument("--capture-pid", type=int, help="Capture top-level visible window of PID")
    parser.add_argument("--run-gallery", action="store_true", help="Run 12-scenario offline visual gallery")
    parser.add_argument("--bin", type=Path, help="Path to fluxer_client binary for --run-gallery")
    parser.add_argument("--diff", action="store_true", help="Compare baseline vs candidate PNGs")
    parser.add_argument("--baseline", type=Path, help="Baseline PNG or directory for --diff")
    parser.add_argument("--candidate", type=Path, help="Candidate PNG or directory for --diff")
    parser.add_argument("--out", type=Path, required=True, help="Output PNG or directory")
    args = parser.parse_args()

    if args.capture_pid is not None:
        hwnd = _find_window_for_pid(args.capture_pid)
        w, h = capture_hwnd_to_png(hwnd, args.out)
        print(f"Captured PID {args.capture_pid} ({w}x{h}) -> {args.out}")
        return 0

    if args.run_gallery:
        exe = args.bin or default_client_binary()
        if not exe.is_file():
            print(f"Error: client binary not found at {exe}", file=sys.stderr)
            return 2
        args.out.mkdir(parents=True, exist_ok=True)
        proc = subprocess.run(
            [str(exe), "--visual-gallery", str(args.out), "--no-os-integration"],
            check=False,
        )
        if proc.returncode != 0:
            print(f"Visual gallery exited with code {proc.returncode}", file=sys.stderr)
            return proc.returncode
        pngs = sorted(args.out.glob("*.png"))
        write_gallery_html(args.out)
        print(f"Generated {len(pngs)} visual gallery screenshots in {args.out}")
        return 0 if len(pngs) == 12 else 1

    if args.diff:
        if not args.baseline or not args.candidate:
            print("--diff requires --baseline and --candidate", file=sys.stderr)
            return 2
        results = []
        if args.baseline.is_file() and args.candidate.is_file():
            results.append(diff_pair(args.baseline, args.candidate, args.out))
        elif args.baseline.is_dir() and args.candidate.is_dir():
            for base_png in sorted(args.baseline.glob("*.png")):
                cand_png = args.candidate / base_png.name
                if cand_png.is_file():
                    results.append(diff_pair(base_png, cand_png, args.out))
        else:
            print("--baseline and --candidate must both be files or both be directories", file=sys.stderr)
            return 2
        report = {"count": len(results), "results": results}
        args.out.mkdir(parents=True, exist_ok=True)
        (args.out / "diff_report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
        print(json.dumps(report, indent=2))
        return 0

    parser.print_help()
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
