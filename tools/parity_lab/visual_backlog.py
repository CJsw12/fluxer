"""Rank captured parity scenes by the visible area affected by a difference."""

import argparse
from collections import Counter, defaultdict
from datetime import datetime, timezone
from hashlib import sha256
import json
from pathlib import Path
import subprocess
import sys
from tempfile import TemporaryDirectory

from PIL import Image

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
PARITY = ROOT / "target/parity-tmp"
sys.path.insert(0, str(HERE))
from compare import (
    _crop_metrics, _extract_web_build, _safe_flx_name, _shift_official, base, compare,
    load_official, load_rust, sample_element_pixels,
)
from visual_masks import FLX, TEXT, bounds, build, crop_panel

SETS = {"popouts": "", "more": "_more", "extra": "_extra", "msg": "_msg", "auto": "_auto"}
UNTAGGED_TABS = {"settings-" + tab for tab in ("themes", "chat", "accessibility", "windows", "advanced")}
STATE_CHANGING_SCENES = {"voice-channel", "light-theme"}


def read(path):
    return json.loads(path.read_text(encoding="utf-8"))


def write(path, obj):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(obj, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")


def artifacts(scene_set, slug, official_dir, rust_dir):
    folder = official_dir / scene_set / slug
    spec = folder / f"{slug}.spec.json"
    png = folder / f"{slug}.png"
    meta = folder / f"{slug}.meta.json"
    for old in (PARITY / scene_set / "spec", PARITY / "scenes", PARITY / "keybinds"):
        if not spec.is_file() and (old / spec.name).is_file():
            spec = old / spec.name
        if not png.is_file() and (old / png.name).is_file():
            png = old / png.name
            meta = old / meta.name
    inspect = rust_dir / f"{slug}.inspect.json"
    rust_png = rust_dir / f"{slug}.png"
    if not (inspect.is_file() or rust_png.is_file()):
        inspect = PARITY / "keybinds" / f"{slug}.json"
        rust_png = PARITY / "keybinds" / f"{slug}-rust.png"
    return spec, png, meta, inspect, rust_png


def score(scene, official_rows, inspect, a, b, offset, min_area, tol, tmp):
    rust_rows = read(inspect)["elements"]
    mask = build(scene, official_rows, rust_rows, a.size, offset)
    for im in (a, b):
        im.paste((0, 0, 0), (0, 0, *im.size), mask)
    scope = tuple(scene.get("scope", []))
    adapted = tmp / "adapted.json"
    write(adapted, [{"flx": e["flx"], "rect": e["rect"], "text": e.get("text", ""),
                     "style": e.get("computed", {})} for e in official_rows if e.get("in_viewport", True)])
    official, rust = load_official(adapted, offset), load_rust(inspect)
    if scope:
        official = {k: v for k, v in official.items() if k.startswith(scope)}
        rust = {k: v for k, v in rust.items() if k.startswith(scope)}
    report = compare(official, rust, tol)
    for path, im in ((tmp / "a.png", a), (tmp / "b.png", b)):
        im.save(path)
    samples = sample_element_pixels(report, tmp / "a.png", tmp / "b.png", top_n=0)
    for flx, entries in official.items():
        if flx not in rust and base(flx) not in rust:
            report["rows"].extend({"flx": flx, "official": e["rect"], "status": "missing_in_rust"} for e in entries)
    items, occurrences = [], Counter()
    for row in report["rows"]:
        flx = row["flx"]
        idx = occurrences[flx]
        occurrences[flx] += 1
        rect = row.get("rust", row.get("official"))
        box = bounds(rect, a.size)
        if box[2] <= box[0] or box[3] <= box[1]:
            continue
        area = (box[2] - box[0]) * (box[3] - box[1])
        effective = area - mask.crop(box).histogram()[255]
        if effective < min_area or (effective < 0.4 * area and area < 20000):
            continue
        ssim, mae = row.get("ssim"), row.get("pixel_mae")
        movement = max(map(abs, row.get("delta", [0])))
        untagged = scene["slug"] in UNTAGGED_TABS and area >= 20000 and (
            "settings-tab-content" in flx or "settings-tab-container" in flx or flx.endswith(".content")
        )
        if ssim is not None:
            values = [("untagged_region" if untagged else "pixel", (1 - ssim) * effective),
                      ("geometry", 0.25 * effective * min(1, movement / 16) if movement > tol else 0)]
        elif row["status"] == "missing_in_rust":
            if untagged:
                mae, ssim = _crop_metrics(a.crop(box), b.crop(box))
                values = [("untagged_region", (1 - ssim) * effective)]
            else:
                values = [("missing", 0.75 * effective)]
        elif row["status"] == "extra_in_rust":
            values = [("rust_only", 0.35 * effective)]
        else:
            continue
        items.extend(dict(flx=flx, idx=idx, kind=kind, rect=rect, box=box, area=area,
                          effective_area=effective, ssim=ssim, pixel_mae=mae, score=value)
                     for kind, value in values if value > 0)
    return items, samples


def selftest():
    with TemporaryDirectory() as tmp_dir:
        tmp = Path(tmp_dir)
        row = {"flx": "panel", "rect": [0, 0, 40, 40]}
        scene = {"slug": "test"}
        a = Image.new("RGB", (40, 40))
        b = Image.new("RGB", a.size, "white")
        inspect = tmp / "inspect.json"
        write(inspect, {"elements": [row]})
        assert not score(scene, [row], inspect, a.copy(), a.copy(), (0, 0), 1, 2, tmp)[0]
        assert score(scene, [row], inspect, a.copy(), b.copy(), (0, 0), 1, 2, tmp)[0][0]["score"] > 1000
        assert not score({**scene, "masks": [row["rect"]]}, [row], inspect, a.copy(), b.copy(), (0, 0), 1, 2, tmp)[0]
        write(inspect, {"elements": [{**row, "text": "tester1#12"}]})
        assert not score(scene, [row], inspect, a.copy(), b.copy(), (0, 0), 1, 2, tmp)[0]
        write(inspect, {"elements": [{**row, "rect": [4, 0, 36, 40]}]})
        shifted = score(scene, [row], inspect, a.copy(), a.copy(), (0, 0), 1, 2, tmp)[0]
        assert shifted[0]["kind"] == "geometry" and shifted[0]["score"] == 90
        write(inspect, {"elements": []})
        assert score(scene, [row], inspect, a, b, (0, 0), 1, 2, tmp)[0][0]["kind"] == "missing"
    print("visual_backlog selftest PASS")


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--scenes", default="all", help="all or comma-separated scene set names")
    p.add_argument("--official-dir", type=Path, default=PARITY / "official/1124")
    p.add_argument("--rust-dir", type=Path, default=PARITY / "visual/latest")
    p.add_argument("--out", type=Path, default=PARITY / "visual_backlog.json")
    p.add_argument("--crops", type=Path, default=PARITY / "visual_crops")
    p.add_argument("--only", help="comma-separated slugs")
    p.add_argument("--top", type=int, default=60)
    p.add_argument("--tol", type=int, default=2)
    p.add_argument("--min-area", type=int, default=400)
    p.add_argument("--expected-web-build", default="Web 2026.1124")
    p.add_argument("--capture-official", action="store_true")
    p.add_argument("--selftest", action="store_true")
    args = p.parse_args()
    if args.selftest:
        selftest()
        return 0
    selected = SETS if args.scenes == "all" else args.scenes.split(",")
    if unknown := set(selected) - SETS.keys():
        p.error(f"unknown scene sets: {', '.join(sorted(unknown))}")
    only = set(args.only.split(",")) if args.only else None
    scenes, items, needed, images = [], [], [], {}
    for name in selected:
        definition = HERE / f"spec_scenes{SETS[name]}.json"
        if not definition.is_file():
            continue
        if args.capture_official:
            cmd = ["node", str(HERE / "spec_official.mjs"), "--attach", "9334", "--scenes", str(definition),
                   "--out-dir", str(args.official_dir / name), "--viewport", "1280x800", "--scene-bundles",
                   "--skip-captured", "--expected-web-build", args.expected_web_build]
            slugs = [d["slug"] for d in read(definition)
                     if (not only or d["slug"] in only) and d["slug"] not in STATE_CHANGING_SCENES]
            if slugs and subprocess.run([*cmd, "--only", ",".join(slugs)], timeout=840).returncode > 1:
                return 2
        for scene in read(definition):
            slug = scene["slug"]
            if only and slug not in only:
                continue
            rec = {"scene_set": name, "slug": slug, "status": "ok"}
            scenes.append(rec)
            spec, png, meta, inspect, rust_png = artifacts(name, slug, args.official_dir, args.rust_dir)
            if not png.is_file() or not spec.is_file() or not meta.is_file():
                rec["status"] = ("no_official_png" if not png.is_file() else
                                 "no_official_spec" if not spec.is_file() else "provenance_inferred")
                rec["provenance"] = "inferred" if not meta.is_file() and png.is_file() else None
                needed.append(f"{name}/{slug}")
                continue
            try:
                m = read(meta)
                build_version = m.get("build", "")
                if build_version != args.expected_web_build and not build_version.startswith(args.expected_web_build + "."):
                    rec["status"] = "wrong_official_build"
                    needed.append(f"{name}/{slug}")
                    continue
                if not (inspect.is_file() and rust_png.is_file()):
                    rec["status"] = "no_rust_shot"
                    continue
                spec_data = read(spec)
                scene_hash = sha256(json.dumps(scene, ensure_ascii=False, separators=(",", ":")).encode()).hexdigest()
                spec_build = _extract_web_build(spec_data.get("client_info"))
                if (spec.parent != png.parent or (spec_build and spec_build != build_version)
                        or m.get("scene_hash") != scene_hash or m.get("viewport") != spec_data.get("viewport")
                        or m.get("theme") not in spec_data.get("theme_class", "").split()):
                    rec["status"] = "stale_official_capture"
                    needed.append(f"{name}/{slug}")
                    continue
                offset = m["offset"]
                a, b = _shift_official(png, rust_png, {"official_offset": offset})
                if a.size != b.size:
                    rec["status"] = "size_mismatch"
                    continue
                with TemporaryDirectory() as directory:
                    ranked, samples = score(scene, spec_data["elements"], inspect, a, b,
                                            offset, args.min_area, args.tol, Path(directory))
                items.extend(dict(scene_set=name, slug=slug, **i) for i in ranked)
                images[name, slug] = (png, rust_png, offset)
                rec.update(official_png=str(png), rust_png=str(rust_png), provenance=m["provenance"],
                           mean_ssim=samples["mean_ssim"], paired=samples["scored_elements"],
                           total_score=sum(i["score"] for i in ranked))
            except (KeyError, OSError, ValueError) as error:
                rec.update(status="invalid_capture", error=str(error))
    items.sort(key=lambda i: (-i["score"], i["scene_set"], i["slug"], i["flx"], i["idx"]))
    grouped = defaultdict(list)
    for i in items:
        grouped[i["flx"]].append(i)
    by_flx = [dict(flx=flx, score_sum=sum(i["score"] for i in rows),
                   scene_count=len({(i["scene_set"], i["slug"]) for i in rows}), worst_scene=rows[0]["slug"],
                   worst_score=rows[0]["score"]) for flx, rows in grouped.items()]
    summary = ["| Rank | Scene | Element | Kind | Score |", "| ---: | --- | --- | --- | ---: |"]
    for rank, item in enumerate(items[:max(args.top, 30)], 1):
        name, slug, flx = item["scene_set"], item["slug"], item["flx"]
        png, rust_png, offset = images[name, slug]
        a, b = _shift_official(png, rust_png, {"official_offset": offset})
        crop = args.crops / name / slug / f"{rank:02d}_{_safe_flx_name(flx)}.png"
        crop_panel(a, b, item["box"], crop)
        item.update(rank=rank, crop_path=str(crop))
        if rank <= 30:
            summary.append(f"| {rank} | {slug} | {flx} | {item['kind']} | {item['score']:.2f} |")
    args.crops.mkdir(parents=True, exist_ok=True)
    (args.crops / "summary.md").write_text("\n".join(summary) + "\n", encoding="utf-8")
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True, timeout=10).strip()
    write(args.out, dict(version=1, generated_at=datetime.now(timezone.utc).isoformat(), git_head=head,
                         mask_rules_hash=sha256((FLX.pattern + TEXT.pattern).encode()).hexdigest(),
                         scenes=scenes, items=items[:max(args.top, 30)], capture_needed=needed, by_flx=by_flx))
    print(f"scenes={len(scenes)} ok={sum(s['status'] == 'ok' for s in scenes)} capture_needed={len(needed)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
