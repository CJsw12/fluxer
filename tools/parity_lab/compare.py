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
    untapped = sorted(
        (
            {"flx": f, "rect": items[0]["rect"], "area": items[0]["rect"][2] * items[0]["rect"][3]}
            for f, items in official.items()
            if f not in rust and base(f) not in rust and items and items[0]["rect"][2] * items[0]["rect"][3] >= 20000
        ),
        key=lambda x: -x["area"],
    )[:10]
    coverage = {
        "official_ids": len(official),
        "rust_tagged_ids": len(rust),
        "tagged_and_present": len([f for f in rust if f in official or any(base(k) == f for k in official)]),
        "top_untagged_official": untapped,
    }
    return {"score": {"pairs": scored, "within_tol": ok, "tol_px": tol}, "coverage": coverage, "rows": rows}


def sample_visual_anomalies(
    report: dict,
    official_png: Path,
    rust_png: Path,
    out_heatmap: Path | None = None,
    diff_thresh: int = 12,
) -> dict:
    """Computes full-box edge P95 and 16x16 corner pixel diffs (catching crossing lines & border radii)
    and optionally renders `diff_heatmap.png` highlighting every shifted pixel in bright magenta.
    """
    from PIL import Image, ImageChops  # noqa: PLC0415

    a = Image.open(official_png).convert("RGB")
    b = Image.open(rust_png).convert("RGB")
    ox, oy = report.get("official_offset", [0, 0])
    if ox or oy:
        shifted = Image.new("RGB", (a.width + ox, a.height + oy), (0, 0, 0))
        shifted.paste(a, (ox, oy))
        a = shifted

    cw, ch = min(a.width, b.width), min(a.height, b.height)
    a_c, b_c = a.crop((0, 0, cw, ch)), b.crop((0, 0, cw, ch))
    full_diff = ImageChops.difference(a_c, b_c)

    flagged = []
    for row in report["rows"]:
        if "official" not in row or "rust" not in row:
            continue
        x, y, w, h = row["rust"]
        x0, y0 = max(0, x), max(0, y)
        x1, y1 = min(cw, x + w), min(ch, y + h)
        if x1 - x0 < 4 or y1 - y0 < 4:
            continue
        crop = full_diff.crop((x0, y0, x1, y1))
        pxs = sorted(sum(rgb) / 3.0 for rgb in (crop.get_flattened_data() if hasattr(crop, "get_flattened_data") else crop.getdata()))
        edge_p95 = round(pxs[int(0.95 * (len(pxs) - 1))], 1)
        row["edge_p95"] = edge_p95
        if (x1 - x0) >= 48 and (y1 - y0) >= 48:
            patches = [
                (x0, y0, min(x1, x0 + 16), min(y1, y0 + 16)),
                (max(x0, x1 - 16), y0, x1, min(y1, y0 + 16)),
                (x0, max(y0, y1 - 16), min(x1, x0 + 16), y1),
                (max(x0, x1 - 16), max(y0, y1 - 16), x1, y1),
            ]
            c_vals = []
            for pbox in patches:
                pc = full_diff.crop(pbox)
                ppx = sorted(sum(rgb) / 3.0 for rgb in (pc.get_flattened_data() if hasattr(pc, "get_flattened_data") else pc.getdata()))
                c_vals.append(round(ppx[int(0.95 * (len(ppx) - 1))], 1))
            row["corner_tl_p95"] = c_vals[0]
            row["corner_max_p95"] = max(c_vals)
        if edge_p95 > 24.0:
            flagged.append({"flx": row["flx"], "edge_p95": edge_p95, "corner_tl_p95": row.get("corner_tl_p95")})

    if out_heatmap is not None:
        base_gray = b_c.convert("L").convert("RGB")
        # Dim Rust image and paint pixels with L1 > diff_thresh bright magenta
        b_data = list(base_gray.get_flattened_data() if hasattr(base_gray, "get_flattened_data") else base_gray.getdata())
        d_data = list(full_diff.get_flattened_data() if hasattr(full_diff, "get_flattened_data") else full_diff.getdata())
        heat_data = [
            (255, 40, 210) if (dr + dg + db) / 3.0 > diff_thresh else (r // 2, g // 2, bl // 2)
            for (r, g, bl), (dr, dg, db) in zip(b_data, d_data)
        ]
        heat = Image.new("RGB", (cw, ch))
        heat.putdata(heat_data)
        panel = Image.new("RGB", (cw * 3, ch), (0, 0, 0))
        panel.paste(a_c, (0, 0))
        panel.paste(b_c, (cw, 0))
        panel.paste(heat, (cw * 2, 0))
        panel.save(out_heatmap)

    anomalies = {"flagged_edge_boxes": sorted(flagged, key=lambda r: -r["edge_p95"])[:12]}
    report["visual_anomalies"] = anomalies
    return anomalies


def _shift_official(official_png: Path, rust_png: Path, report: dict):
    """Loads both PNGs as RGB and shifts the official screenshot by `official_offset`."""
    from PIL import Image  # noqa: PLC0415

    a = Image.open(official_png).convert("RGB")
    b = Image.open(rust_png).convert("RGB")
    ox, oy = report.get("official_offset", [0, 0])
    if ox or oy:
        shifted = Image.new("RGB", (max(1, a.width + ox), max(1, a.height + oy)), (0, 0, 0))
        shifted.paste(a, (ox, oy))
        a = shifted
    return a, b


def _crop_metrics(crop_a, crop_b) -> tuple[float, float]:
    """Computes RGB pixel_mae and luminance SSIM (C1=(0.01*255)^2, C2=(0.03*255)^2) via Pillow."""
    from PIL import ImageChops, ImageStat  # noqa: PLC0415

    diff_rgb = ImageChops.difference(crop_a, crop_b)
    mae = sum(ImageStat.Stat(diff_rgb).mean) / 3.0
    if mae == 0.0:
        return 0.0, 1.0

    c1 = (0.01 * 255.0) ** 2
    c2 = (0.03 * 255.0) ** 2
    la = crop_a.convert("L")
    lb = crop_b.convert("L")
    diff_l = ImageChops.difference(la, lb)
    n = la.width * la.height
    sa = ImageStat.Stat(la)
    sb = ImageStat.Stat(lb)
    mu_x, mu_y = sa.mean[0], sb.mean[0]
    var_x, var_y = sa.var[0], sb.var[0]
    mean_sq_diff = ImageStat.Stat(diff_l).sum2[0] / n
    two_cov = var_x + var_y + (mu_x - mu_y) ** 2 - mean_sq_diff
    num = (2.0 * mu_x * mu_y + c1) * (two_cov + c2)
    den = (mu_x * mu_x + mu_y * mu_y + c1) * (var_x + var_y + c2)
    ssim = 1.0 if den == 0.0 else max(-1.0, min(1.0, num / den))
    mae_out = round(mae, 4) or round(mae, 6)
    ssim_out = round(ssim, 6)
    if ssim < 1.0 and ssim_out >= 1.0:
        ssim_out = 0.999999
    return mae_out, ssim_out


def _safe_flx_name(flx: str) -> str:
    return re.sub(r"[^A-Za-z0-9._-]+", "_", flx).strip("._")[:80] or "element"


def sample_element_pixels(
    report: dict,
    official_png: Path,
    rust_png: Path,
    out_dir: Path | None = None,
    top_n: int = 10,
) -> dict:
    """Computes per-element RGB `pixel_mae` and luminance `ssim` for every paired >=4x4 box,
    saves side-by-side (official | rust | 3x diff) crops for the worst `top_n` elements into
    `<out_dir>/worst_crops/`, and populates `report["element_pixel_summary"]`.
    """
    from PIL import Image, ImageChops  # noqa: PLC0415

    a, b = _shift_official(official_png, rust_png, report)
    cw, ch = min(a.width, b.width), min(a.height, b.height)
    a_c, b_c = a.crop((0, 0, cw, ch)), b.crop((0, 0, cw, ch))

    scored: list[tuple[dict, tuple[int, int, int, int]]] = []
    for row in report["rows"]:
        if "official" not in row or "rust" not in row:
            continue
        x, y, w, h = row["rust"]
        x0, y0 = max(0, x), max(0, y)
        x1, y1 = min(cw, x + w), min(ch, y + h)
        if x1 - x0 < 4 or y1 - y0 < 4:
            continue
        box = (x0, y0, x1, y1)
        mae, ssim = _crop_metrics(a_c.crop(box), b_c.crop(box))
        row["pixel_mae"] = mae
        row["ssim"] = ssim
        scored.append((row, box))

    diff_items = sorted(
        ((row, box) for row, box in scored if row["pixel_mae"] > 0.0 or row["ssim"] < 1.0),
        key=lambda item: (item[0]["ssim"], -item[0]["pixel_mae"], item[0]["flx"]),
    )[:top_n]

    worst_crops = []
    if out_dir is not None:
        worst_dir = out_dir / "worst_crops"
        worst_dir.mkdir(parents=True, exist_ok=True)
        for rank, (row, box) in enumerate(diff_items, 1):
            ac = a_c.crop(box)
            bc = b_c.crop(box)
            dc = ImageChops.difference(ac, bc).point(lambda v: min(255, v * 3))
            bw, bh = ac.size
            panel = Image.new("RGB", (bw * 3, bh), (0, 0, 0))
            panel.paste(ac, (0, 0))
            panel.paste(bc, (bw, 0))
            panel.paste(dc, (bw * 2, 0))
            fname = f"{rank:02d}_{_safe_flx_name(row['flx'])}.png"
            panel.save(worst_dir / fname)
            rel_path = f"worst_crops/{fname}"
            row["crop_path"] = rel_path
            worst_crops.append({
                "rank": rank,
                "flx": row["flx"],
                "official": row["official"],
                "rust": row["rust"],
                "pixel_mae": row["pixel_mae"],
                "ssim": row["ssim"],
                "crop_path": rel_path,
            })

    maes = [row["pixel_mae"] for row, _ in scored]
    ssims = [row["ssim"] for row, _ in scored]
    summary = {
        "scored_elements": len(scored),
        "mean_pixel_mae": round(sum(maes) / len(maes), 4) if maes else 0.0,
        "max_pixel_mae": max(maes) if maes else 0.0,
        "mean_ssim": round(sum(ssims) / len(ssims), 6) if ssims else 1.0,
        "min_ssim": min(ssims) if ssims else 1.0,
        "worst_crops": worst_crops,
    }
    report["element_pixel_summary"] = summary
    return summary


_STYLE_AUDIT_SPECS = (
    {
        "id": "main_content_frame",
        "label": "메인 콘텐츠 프레임 (1px 테두리·14px 좌상단 반경)",
        "flx_candidates": ("app.outline-frame.frame",),
        "regions": ("top_left_corner", "top_edge", "left_edge"),
        "corner_size": 16,
        "edge_thickness": 2,
        "all_instances": False,
    },
    {
        "id": "sidebar_frame",
        "label": "사이드바 프레임 반경",
        "flx_candidates": ("app.guild-sidebar.guild-navbar",),
        "regions": ("top_left_corner", "bottom_left_corner"),
        "corner_size": 16,
        "edge_thickness": 2,
        "all_instances": False,
    },
    {
        "id": "member_list_frame",
        "label": "멤버 목록 프레임 반경",
        "flx_candidates": ("channel.member-list-container.member-list-container",),
        "regions": ("top_left_corner", "top_right_corner"),
        "corner_size": 16,
        "edge_thickness": 2,
        "all_instances": False,
    },
    {
        "id": "user_area_card",
        "label": "사용자 영역 카드",
        "flx_candidates": (
            "app.user-area.user-area-inner.section",
            "app.user-area.user-area-inner.user-area-container",
        ),
        "regions": ("box", "top_left_corner", "top_edge", "bottom_edge", "left_edge", "right_edge"),
        "corner_size": 12,
        "edge_thickness": 2,
        "all_instances": False,
    },
    {
        "id": "server_initials",
        "label": "서버 이니셜",
        "flx_candidates": (
            "guild.guild-acronym.text",
            "app.sidebar-nav.guild-list-item-presentation.guild-icon-acronym",
            "app.sidebar-nav.guild-list-item-presentation.guild-icon",
        ),
        "regions": ("box", "center_glyph"),
        "corner_size": 12,
        "edge_thickness": 2,
        "all_instances": True,
    },
)


def _region_box(rect: tuple[int, int, int, int], region: str, corner: int, edge: int) -> tuple[int, int, int, int] | None:
    x0, y0, x1, y1 = rect
    w, h = x1 - x0, y1 - y0
    if w < 2 or h < 2:
        return None
    cs = max(2, min(corner, w, h))
    et = max(1, min(edge, w, h))
    if region == "box":
        box = (x0, y0, x1, y1)
    elif region == "top_left_corner":
        box = (x0, y0, min(x1, x0 + cs), min(y1, y0 + cs))
    elif region == "top_right_corner":
        box = (max(x0, x1 - cs), y0, x1, min(y1, y0 + cs))
    elif region == "bottom_left_corner":
        box = (x0, max(y0, y1 - cs), min(x1, x0 + cs), y1)
    elif region == "top_edge":
        ix0, ix1 = (x0 + cs, x1 - cs) if w > cs * 2 + 4 else (x0, x1)
        box = (ix0, y0, ix1, min(y1, y0 + et))
    elif region == "bottom_edge":
        ix0, ix1 = (x0 + cs, x1 - cs) if w > cs * 2 + 4 else (x0, x1)
        box = (ix0, max(y0, y1 - et), ix1, y1)
    elif region == "left_edge":
        iy0, iy1 = (y0 + cs, y1 - cs) if h > cs * 2 + 4 else (y0, y1)
        box = (x0, iy0, min(x1, x0 + et), iy1)
    elif region == "right_edge":
        iy0, iy1 = (y0 + cs, y1 - cs) if h > cs * 2 + 4 else (y0, y1)
        box = (max(x0, x1 - et), iy0, x1, iy1)
    elif region == "center_glyph":
        mx, my = max(0, w // 6), max(0, h // 6)
        box = (x0 + mx, y0 + my, x1 - mx, y1 - my)
    else:
        return None
    return box if (box[2] - box[0] >= 2 and box[3] - box[1] >= 2) else None


def audit_style_pixels(report: dict, official_png: Path, rust_png: Path) -> dict:
    """Pixel-based edge/corner/glyph audit for the M9.8 priority targets."""
    a, b = _shift_official(official_png, rust_png, report)
    cw, ch = min(a.width, b.width), min(a.height, b.height)
    a_c, b_c = a.crop((0, 0, cw, ch)), b.crop((0, 0, cw, ch))

    paired_by_flx: dict[str, list[dict]] = defaultdict(list)
    for row in report["rows"]:
        if "official" in row and "rust" in row:
            paired_by_flx[row["flx"]].append(row)

    targets = []
    present = 0
    diff_count = 0
    missing = 0
    for spec in _STYLE_AUDIT_SPECS:
        matched_flx = next((f for f in spec["flx_candidates"] if paired_by_flx.get(f)), None)
        if matched_flx is None:
            missing += 1
            targets.append({
                "id": spec["id"],
                "label": spec["label"],
                "flx": None,
                "status": "missing",
                "instances": [],
            })
            continue

        present += 1
        rows_for = paired_by_flx[matched_flx] if spec["all_instances"] else paired_by_flx[matched_flx][:1]
        instances = []
        for row in rows_for:
            x, y, w, h = row["rust"]
            clipped = (max(0, x), max(0, y), min(cw, x + w), min(ch, y + h))
            regions_out = {}
            for reg in spec["regions"]:
                rbox = _region_box(clipped, reg, spec["corner_size"], spec["edge_thickness"])
                if rbox is None:
                    continue
                mae, ssim = _crop_metrics(a_c.crop(rbox), b_c.crop(rbox))
                regions_out[reg] = {"box": list(rbox), "pixel_mae": mae, "ssim": ssim}
            inst_mae = max((v["pixel_mae"] for v in regions_out.values()), default=0.0)
            inst_ssim = min((v["ssim"] for v in regions_out.values()), default=1.0)
            inst_status = "ok" if (inst_mae <= 2.0 and inst_ssim >= 0.98) else "diff"
            instances.append({
                "rect": row["rust"],
                "max_pixel_mae": inst_mae,
                "min_ssim": inst_ssim,
                "status": inst_status,
                "regions": regions_out,
            })

        target_mae = max((i["max_pixel_mae"] for i in instances), default=0.0)
        target_ssim = min((i["min_ssim"] for i in instances), default=1.0)
        target_status = "ok" if all(i["status"] == "ok" for i in instances) else "diff"
        if target_status == "diff":
            diff_count += 1
        targets.append({
            "id": spec["id"],
            "label": spec["label"],
            "flx": matched_flx,
            "status": target_status,
            "max_pixel_mae": target_mae,
            "min_ssim": target_ssim,
            "instances": instances,
        })

    audit = {
        "targets": targets,
        "present_targets": present,
        "diff_targets": diff_count,
        "missing_targets": missing,
    }
    report["style_pixel_audit"] = audit
    return audit


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


def selftest() -> None:
    import shutil  # noqa: PLC0415
    from PIL import Image, ImageDraw  # noqa: PLC0415

    tmp = Path(__file__).resolve().parents[2] / "target" / "parity-tmp" / "selftest_compare"
    shutil.rmtree(tmp, ignore_errors=True)
    tmp.mkdir(parents=True, exist_ok=True)
    try:
        off_dom = tmp / "official.dom.json"
        rs_json = tmp / "rust.json"
        off_png = tmp / "official.png"
        rs_same_png = tmp / "rust_same.png"
        rs_diff_png = tmp / "rust_diff.png"

        # Web capture is 200x160; desktop Rust adds 20px top offset (official_offset=[0, 20]).
        dom_rows = [
            {"flx": "app.outline-frame.frame", "rect": [10, 0, 180, 140], "text": ""},
            {"flx": "app.guild-sidebar.guild-navbar", "rect": [10, 0, 50, 140], "text": ""},
            {"flx": "channel.member-list-container.member-list-container", "rect": [140, 0, 50, 140], "text": ""},
            {"flx": "app.user-area.user-area-inner.section", "rect": [4, 110, 56, 26], "text": ""},
            {"flx": "guild.guild-acronym.text", "rect": [2, 10, 24, 24], "text": "FX"},
            {"flx": "tiny.skip", "rect": [0, 0, 3, 3], "text": ""},
        ]
        rust_rows = {
            "elements": [
                {"flx": "app.outline-frame.frame", "rect": [10, 20, 180, 140], "text": ""},
                {"flx": "app.guild-sidebar.guild-navbar", "rect": [10, 20, 50, 140], "text": ""},
                {"flx": "channel.member-list-container.member-list-container", "rect": [140, 20, 50, 140], "text": ""},
                {"flx": "app.user-area.user-area-inner.section", "rect": [4, 130, 56, 26], "text": ""},
                {"flx": "guild.guild-acronym.text", "rect": [2, 30, 24, 24], "text": "FX"},
                {"flx": "tiny.skip", "rect": [0, 20, 3, 3], "text": ""},
            ]
        }
        off_dom.write_text(json.dumps(dom_rows), encoding="utf-8")
        rs_json.write_text(json.dumps(rust_rows), encoding="utf-8")

        im_off = Image.new("RGB", (200, 160), (24, 24, 30))
        dr = ImageDraw.Draw(im_off)
        dr.rectangle([10, 0, 189, 139], outline=(80, 80, 100))
        dr.rectangle([4, 110, 59, 135], fill=(32, 30, 36), outline=(70, 70, 85))
        dr.rectangle([4, 12, 23, 31], fill=(210, 210, 230))
        im_off.save(off_png)

        im_same = Image.new("RGB", (200, 180), (0, 0, 0))
        im_same.paste(im_off, (0, 20))
        im_same.save(rs_same_png)

        im_diff = im_same.copy()
        dr_d = ImageDraw.Draw(im_diff)
        dr_d.rectangle([10, 20, 26, 36], fill=(255, 60, 60))
        dr_d.rectangle([6, 134, 55, 152], fill=(90, 40, 120))
        dr_d.rectangle([4, 32, 23, 51], fill=(40, 200, 90))
        im_diff.save(rs_diff_png)

        # 1. Identical images (with official_offset=[0, 20])
        out_same = tmp / "out_same"
        assert main([str(off_dom), str(rs_json), str(out_same), "--official-offset", "0,20", "--official-png", str(off_png), "--rust-png", str(rs_same_png)]) == 0
        rep_same = json.loads((out_same / "report.json").read_text(encoding="utf-8"))
        scored_same = [r for r in rep_same["rows"] if "pixel_mae" in r]
        assert len(scored_same) == 5, scored_same
        assert all(r["pixel_mae"] == 0.0 and r["ssim"] == 1.0 for r in scored_same), scored_same
        assert all("pixel_mae" not in r for r in rep_same["rows"] if r["flx"] == "tiny.skip")
        assert rep_same["element_pixel_summary"]["min_ssim"] == 1.0
        assert rep_same["element_pixel_summary"]["max_pixel_mae"] == 0.0
        assert (out_same / "worst_crops").is_dir()
        assert rep_same["style_pixel_audit"]["present_targets"] == 5
        assert rep_same["style_pixel_audit"]["diff_targets"] == 0

        # 2. Modified image
        out_diff = tmp / "out_diff"
        assert main([str(off_dom), str(rs_json), str(out_diff), "--official-offset", "0,20", "--official-png", str(off_png), "--rust-png", str(rs_diff_png)]) == 0
        rep_diff = json.loads((out_diff / "report.json").read_text(encoding="utf-8"))
        scored_diff = [r for r in rep_diff["rows"] if "pixel_mae" in r]
        assert any(r["pixel_mae"] > 0.0 and r["ssim"] < 1.0 for r in scored_diff), scored_diff
        worst = rep_diff["element_pixel_summary"]["worst_crops"]
        assert worst, rep_diff["element_pixel_summary"]
        for item in worst:
            crop_file = out_diff / item["crop_path"]
            assert crop_file.exists(), crop_file
            with Image.open(crop_file) as panel:
                assert panel.width == item["rust"][2] * 3 and panel.height == item["rust"][3]
        assert rep_diff["style_pixel_audit"]["diff_targets"] >= 3, rep_diff["style_pixel_audit"]
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
    print("selftest ok")


def main(argv: list[str]) -> int:
    sys.stdout.reconfigure(encoding="utf-8")
    if "--selftest" in argv:
        selftest()
        return 0
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
        official_png = Path(opts["--official-png"])
        rust_png = Path(opts["--rust-png"])
        sample_colors(report, official_png, rust_png, float(opts.get("--color-tol", 18)))
        sample_visual_anomalies(report, official_png, rust_png, out / "diff_heatmap.png")
        sample_element_pixels(report, official_png, rust_png, out)
        audit_style_pixels(report, official_png, rust_png)
        (out / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=1), encoding="utf-8")
        overlay(report, official_png, rust_png, out / "overlay.png")
    s, c = report["score"], report["coverage"]
    print(f"geometry: {s['within_tol']}/{s['pairs']} within {s['tol_px']}px | coverage: rust tags {c['rust_tagged_ids']} ids, official scene {c['official_ids']} ids")
    if c.get("top_untagged_official"):
        print(f"untagged official large surfaces: {[u['flx'] for u in c['top_untagged_official'][:5]]}")
    if "element_pixel_summary" in report:
        eps = report["element_pixel_summary"]
        print(f"element pixels: scored={eps['scored_elements']} mean_mae={eps['mean_pixel_mae']} max_mae={eps['max_pixel_mae']} min_ssim={eps['min_ssim']} worst_crops={len(eps['worst_crops'])}")
    if "style_pixel_audit" in report:
        spa = report["style_pixel_audit"]
        print(f"style pixel audit: present={spa['present_targets']} diff={spa['diff_targets']} missing={spa['missing_targets']}")
    for anom in report.get("visual_anomalies", {}).get("flagged_edge_boxes", [])[:5]:
        print(f"  EDGE_ANOMALY {anom['flx'][:60]:60} edge_p95={anom['edge_p95']} corner_tl={anom.get('corner_tl_p95')}")
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
