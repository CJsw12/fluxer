"""Join official DOM capture (`*.dom.json` from web_capture/crawl) with a Rust probe inspect dump
by `data-flx` id and report geometry/text deltas plus an overlay image.

    python -I tools/parity_lab/compare.py OFFICIAL.dom.json RUST_inspect.json OUT_DIR
        [--official-png A.png --rust-png B.png] [--tol 2]

Matching: exact `data-flx` literal first (the official `--N` suffix marks a distinct source site),
then the base id without the suffix. Repeated ids are paired by
identical text first, then by reading order (top-to-bottom, left-to-right).
Only ids the Rust side has tagged are scored for geometry; `coverage` lists every official id in
the scene so untagged/missing UI is visible as a number, not a guess.
"""

from __future__ import annotations

import json
import re
import sys
from collections import defaultdict
from pathlib import Path

SUFFIX = re.compile(r"--\d+$")


def base(flx: str) -> str:
    return SUFFIX.sub("", flx)


def load_official(path: Path, offset: tuple[int, int] = (0, 0)) -> dict[str, list[dict]]:
    """`offset` shifts web captures into desktop coordinates (web has no 32px native title bar)."""
    out: dict[str, list[dict]] = defaultdict(list)
    for e in json.loads(path.read_text(encoding="utf-8")):
        x, y, w, h = e["rect"]
        out[e["flx"]].append({"rect": [x + offset[0], y + offset[1], w, h], "text": (e.get("text") or e.get("label") or "").strip(), "style": e.get("style", {})})
    return out


def load_rust(path: Path) -> dict[str, list[dict]]:
    out: dict[str, list[dict]] = defaultdict(list)
    data = json.loads(path.read_text(encoding="utf-8"))
    for e in data["elements"]:
        text = (e.get("text") or "").strip()
        out[e["flx"]].append({"rect": [round(v) for v in e["rect"]], "text": "" if text == "derived" else text, "derived": text == "derived"})
    return out


def pair(off: list[dict], rs: list[dict]) -> list[tuple[dict | None, dict | None]]:
    order = lambda e: (e["rect"][1], e["rect"][0])  # noqa: E731
    off, rs = sorted(off, key=order), sorted(rs, key=order)
    pairs, used = [], set()
    for o in off:
        hit = next((i for i, r in enumerate(rs) if i not in used and o["text"] and r["text"] == o["text"]), None)
        if hit is not None:
            used.add(hit)
            pairs.append((o, rs[hit]))
        else:
            pairs.append((o, None))
    rest = [r for i, r in enumerate(rs) if i not in used]
    # Second pass: unmatched by order.
    final = []
    for o, r in pairs:
        if r is None and rest:
            r = rest.pop(0)
        final.append((o, r))
    final.extend((None, r) for r in rest)
    return final


def compare(official: dict, rust: dict, tol: int) -> dict:
    rows, scored = [], 0
    ok = 0
    for flx in sorted(rust):
        # `--N` marks a distinct source site in the official codemod, so exact literals win; a Rust
        # tag without the suffix falls back to every official site sharing the base id.
        off = official.get(flx) or [e for k, v in official.items() if base(k) == flx for e in v]
        for o, r in pair(off, rust[flx]):
            if o is None:
                rows.append({"flx": flx, "status": "extra_in_rust", "rust": r["rect"], "text": r["text"]})
                continue
            if r is None:
                rows.append({"flx": flx, "status": "missing_in_rust", "official": o["rect"], "text": o["text"]})
                continue
            d = [r["rect"][i] - o["rect"][i] for i in range(4)]
            text_ok = not o["text"] or not r["text"] or o["text"] == r["text"]
            good = all(abs(v) <= tol for v in d) and text_ok
            scored += 1
            ok += good
            rows.append({
                "flx": flx, "status": "ok" if good else "mismatch", "official": o["rect"], "rust": r["rect"],
                "delta": d, "text_official": o["text"], "text_rust": r["text"], "derived": r.get("derived", False),
                "official_style": {k: o["style"].get(k) for k in ("fontSize", "fontWeight", "color", "backgroundColor", "borderRadius") if o["style"].get(k)},
            })
    coverage = {
        "official_ids": len(official),
        "rust_tagged_ids": len(rust),
        "tagged_and_present": len([f for f in rust if f in official or any(base(k) == f for k in official)]),
    }
    return {"score": {"pairs": scored, "within_tol": ok, "tol_px": tol}, "coverage": coverage, "rows": rows}


def sample_colors(report: dict, official_png: Path, rust_png: Path, max_delta: float) -> None:
    """Median colour of each paired box's inner area in both screenshots (no Rust style export needed).

    Uses the inset centre band so borders/antialiased text edges weigh less; large deltas flag a
    background/foreground colour mismatch the geometry check cannot see.
    """
    from PIL import Image  # noqa: PLC0415

    a = Image.open(official_png).convert("RGB")
    b = Image.open(rust_png).convert("RGB")
    ox, oy = report.get("official_offset", [0, 0])
    if ox or oy:  # place the web capture where the desktop content area starts
        shifted = Image.new("RGB", (a.width + ox, a.height + oy), (0, 0, 0))
        shifted.paste(a, (ox, oy))
        a = shifted

    def median(img, rect):
        x, y, w, h = rect
        if w < 4 or h < 4:
            return None
        box = (max(0, x + w // 4), max(0, y + h // 4), min(img.width, x + w - w // 4), min(img.height, y + h - h // 4))
        if box[2] <= box[0] or box[3] <= box[1]:
            return None
        crop = img.crop(box)
        px = sorted(crop.get_flattened_data() if hasattr(crop, "get_flattened_data") else crop.getdata(), key=sum)
        return px[len(px) // 2]

    for row in report["rows"]:
        if "official" not in row or "rust" not in row:
            continue
        ca, cb = median(a, row["official"]), median(b, row["rust"])
        if ca is None or cb is None:
            continue
        delta = sum((ca[i] - cb[i]) ** 2 for i in range(3)) ** 0.5
        row["color_official"], row["color_rust"], row["color_delta"] = list(ca), list(cb), round(delta, 1)
        if delta > max_delta and row["status"] == "ok":
            row["status"] = "mismatch"


def overlay(report: dict, official_png: Path, rust_png: Path, out: Path) -> None:
    from PIL import Image, ImageDraw  # noqa: PLC0415

    a = Image.open(official_png).convert("RGB")
    b = Image.open(rust_png).convert("RGB")
    w, h = a.size
    canvas = Image.new("RGB", (w * 2, max(h, b.size[1])), (0, 0, 0))
    canvas.paste(a, (0, 0))
    canvas.paste(b, (w, 0))
    dr = ImageDraw.Draw(canvas)
    for row in report["rows"]:
        if row["status"] == "ok":
            continue
        color = (255, 60, 60) if row["status"] == "mismatch" else (255, 200, 0)
        if "official" in row:
            x, y, ww, hh = row["official"]
            dr.rectangle([x, y, x + ww, y + hh], outline=(0, 255, 120))
            if row["status"] == "mismatch":
                rx, ry, rw, rh = row["rust"]
                dr.rectangle([rx, ry, rx + rw, ry + rh], outline=color)  # rust box drawn on official half
        if "rust" in row:
            x, y, ww, hh = row["rust"]
            dr.rectangle([w + x, y, w + x + ww, y + hh], outline=color)
    canvas.save(out)


def main(argv: list[str]) -> int:
    sys.stdout.reconfigure(encoding="utf-8")
    if len(argv) < 3:
        print(__doc__)
        return 2
    opts = dict(zip(argv[3::2], argv[4::2]))
    out = Path(argv[2])
    out.mkdir(parents=True, exist_ok=True)
    off = tuple(int(v) for v in opts.get("--official-offset", "0,0").split(","))
    report = compare(load_official(Path(argv[0]), off), load_rust(Path(argv[1])), int(opts.get("--tol", 2)))
    report["official_offset"] = list(off)
    (out / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=1), encoding="utf-8")
    if "--official-png" in opts and "--rust-png" in opts:
        sample_colors(report, Path(opts["--official-png"]), Path(opts["--rust-png"]), float(opts.get("--color-tol", 18)))
        (out / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=1), encoding="utf-8")
        overlay(report, Path(opts["--official-png"]), Path(opts["--rust-png"]), out / "overlay.png")
    s, c = report["score"], report["coverage"]
    print(f"geometry: {s['within_tol']}/{s['pairs']} within {s['tol_px']}px | coverage: rust tags {c['rust_tagged_ids']} ids, official scene {c['official_ids']} ids")
    for row in report["rows"]:
        if row["status"] == "ok":
            continue
        if row["status"] == "mismatch":
            col = f" colorΔ={row['color_delta']} {row['color_official']}→{row['color_rust']}" if row.get("color_delta", 0) > 18 else ""
            print(f"  MISMATCH {row['flx'][:70]:70} off={row['official']} rust={row['rust']} d={row['delta']}{col}")
        else:
            print(f"  {row['status'].upper():16} {row['flx'][:70]:70} {row.get('official') or row.get('rust')} {row.get('text','')[:20]}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
