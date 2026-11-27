"""Capture Rust settings scenes and compare them to a clean commit's screenshots."""

import argparse
from collections import Counter
from datetime import datetime, timezone
from hashlib import sha256
import json
from pathlib import Path
import subprocess
import sys
import time

from PIL import Image, ImageChops

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
PARITY = ROOT / "target/parity-tmp"
sys.path.insert(0, str(HERE))
from compare import _crop_metrics, _safe_flx_name
import rust_probe as rp
from spec_delta import BASE_LINK, _exec_rust_action, _flx_set
from visual_masks import FLX, TEXT, bounds, build, crop_panel

SIZE = (1280, 800)
MUTE = "app.user-area.user-area-inner.control-button.mute"
SIDEBAR = "app.settings-modal-layout.settings-modal-desktop-sidebar.desktop-sidebar"


def read(path):
    return json.loads(path.read_text(encoding="utf-8"))


def write(path, value):
    path.write_text(json.dumps(value, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT, text=True, timeout=10).strip()


def restore():
    rp.send("probe_key", keystroke="escape")
    rp.deep_link("fluxer://scene/voice-leave")
    rp.deep_link("fluxer://scene/theme/dark")
    rp.deep_link(BASE_LINK)
    frame = rp.inspect_until(rp.markdown_ready, 4)
    if not rp.markdown_ready(frame):
        raise RuntimeError("#markdown did not restore")
    a11y = json.dumps(frame.get("a11y", {}))
    if "Unmute microphone" in a11y:
        rp.send("probe_click", flx=MUTE)
        frame = rp.inspect_until(lambda f: "Mute microphone" in json.dumps(f.get("a11y", {})), 3)
        a11y = json.dumps(frame.get("a11y", {}))
    if "Mute microphone" not in a11y:
        raise RuntimeError("could not confirm Rust microphone restore")


def capture(scene, folder):
    slug = scene["slug"]
    first = rp.send("probe_inspect")
    for act in scene["rust"]:
        _exec_rust_action(rp, act, time)
    if not any("hover" in act for act in scene["rust"]):
        rp.send("probe_hover", x=1240.0, y=760.0)
    time.sleep(scene.get("wait_ms", 600) / 1000)
    expected = scene.get("rust_expect_flx", scene.get("expect_flx", SIDEBAR))
    frame = rp.inspect_until(lambda f: not expected or expected in _flx_set(f["elements"]), 4)
    if expected and expected not in _flx_set(frame["elements"]):
        raise RuntimeError(f"Rust scene did not open: {expected}")
    if not expected and frame["elements"] == first["elements"]:
        raise RuntimeError("Rust interaction did not change the inspected scene")
    png = folder / f"{slug}.png"
    response = rp.shot(png)
    if not response.get("ok"):
        raise RuntimeError(response.get("error", "screenshot failed"))
    with Image.open(png) as image:
        if image.size != SIZE:
            raise RuntimeError(f"unexpected screenshot size: {image.size}")
        if all(low == high for low, high in image.convert("RGB").getextrema()):
            raise RuntimeError("screenshot has no visible content")
    write(folder / f"{slug}.inspect.json", frame)


def scene_delta(scene, before, after, allowed):
    a, prev = before
    b, curr = after
    added_masks = [e["rect"] for rows in (prev, curr) for e in rows if e["flx"] in allowed]
    mask = build({**scene, "masks": [*scene.get("masks", []), *added_masks]}, prev, curr, a.size)
    for im in (a, b):
        im.paste((0, 0, 0), (0, 0, *im.size), mask)
    usable = a.width * a.height - mask.histogram()[255]
    changed = ImageChops.difference(a, b).split()
    added = ImageChops.add(ImageChops.add(changed[0], changed[1]), changed[2])
    ratio = sum(added.histogram()[73:]) / max(usable, 1)
    _, ssim = _crop_metrics(a, b)
    row = dict(slug=scene["slug"], mean_ssim=ssim, ssim_delta=1 - ssim, changed_ratio=ratio, regions=[])
    regions = row["regions"]
    if (1 - ssim >= 0.0015 or ratio >= 0.0015) and "__scene__" not in allowed:
        regions.append(dict(flx="__scene__", kind="scene", severity="FAIL", score=ratio,
                            ssim=ssim, box=(0, 0, *a.size)))

    def keyed(rows):
        seen = Counter()
        keys = {}
        for element in rows:
            flx = element["flx"]
            index = seen[flx]
            seen[flx] += 1
            box = bounds(element["rect"], a.size)
            if box[2] > box[0] and box[3] > box[1]:
                keys[flx, index] = (element["rect"], box)
        return keys

    def area(box):
        return max(0, box[2] - box[0]) * max(0, box[3] - box[1])

    old, new = keyed(prev), keyed(curr)
    for key in sorted(old.keys() | new.keys()):
        flx, _ = key
        if flx in allowed:
            continue
        _, box = new[key] if key in new else old[key]
        effective = area(box) - mask.crop(box).histogram()[255]
        severity = None
        score, element_ssim = 0.0, 1.0
        if key not in new and effective >= 2000:
            kind, severity, score, element_ssim = "missing", "FAIL", 0.75 * effective, 0.0
        elif key not in old and effective >= 4000:
            overlap = any(
                area((max(box[0], prev_box[0]), max(box[1], prev_box[1]),
                      min(box[2], prev_box[2]), min(box[3], prev_box[3]))) >= 0.5 * area(box)
                for _, prev_box in old.values()
            )
            kind, severity, score, element_ssim = "new", "FAIL" if overlap else "WARN", 0.35 * effective, 0.0
        elif key in old and key in new and effective >= 900:
            max_shift = max(abs(x - y) for x, y in zip(old[key][0], new[key][0]))
            _, element_ssim = _crop_metrics(a.crop(box), b.crop(box))
            score = (1 - element_ssim) * effective
            if max_shift >= 8 and effective >= 10000:
                kind, severity = "moved", "FAIL"
            elif score >= 0.30 and element_ssim < 0.97:
                kind, severity = "pixel", "FAIL"
            elif max_shift >= 2 and effective >= 10000:
                kind, severity = "moved", "WARN"
        if severity:
            regions.append(dict(flx=flx, index=key[1], kind=kind, severity=severity,
                                score=score, ssim=element_ssim, box=box))
    return row


def outcome(rows):
    scenes_failed = sum(any(i["severity"] == "FAIL" and i["kind"] == "scene" for i in s["regions"])
                        or s.get("status") == "capture_error" for s in rows)
    elements_failed = sum(i["severity"] == "FAIL" and i["kind"] != "scene" for s in rows for i in s["regions"])
    warned = sum(i["severity"] == "WARN" for s in rows for i in s["regions"])
    result = "FAIL" if scenes_failed or elements_failed >= 2 else "WARN" if warned or elements_failed else "PASS"
    return result, dict(scene_failures=scenes_failed, element_failures=elements_failed, warnings=warned)


def bundle(folder, slug):
    with Image.open(folder / f"{slug}.png") as image:
        rgb = image.convert("RGB")
    if rgb.size != SIZE:
        raise ValueError(f"invalid cached viewport for {slug}")
    return rgb, read(folder / f"{slug}.inspect.json")["elements"]


def selftest():
    row = {"flx": "sidebar", "rect": [0, 0, 40, 40]}
    scene = {"slug": "synthetic"}
    a = Image.new("RGB", (40, 40), "black")
    b = Image.new("RGB", (40, 40), "white")
    assert outcome([scene_delta(scene, (a.copy(), [row]), (a.copy(), [row]), set())])[0] == "PASS"
    assert outcome([scene_delta(scene, (a.copy(), [row]), (b.copy(), [row]), set())])[0] == "FAIL"
    assert outcome([scene_delta(scene, (a.copy(), [row]), (b.copy(), [row]), {"__scene__"})])[0] == "WARN"
    assert outcome([scene_delta({**scene, "masks": [row["rect"]]}, (a, [row]), (b, [row]), set())])[0] == "PASS"
    print("shot_regress selftest PASS")


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--out", type=Path, default=PARITY / "shots")
    p.add_argument("--shots-baseline", default="HEAD")
    p.add_argument("--refresh-shots", action="store_true")
    p.add_argument("--selftest", action="store_true")
    args = p.parse_args()
    if args.selftest:
        selftest()
        return 0
    by_slug = {s["slug"]: s for s in read(HERE / "spec_scenes_more.json")}
    scenes = [{**by_slug[s["slug"]], **s} for s in read(HERE / "shot_scenes.json")]
    manifest = dict(viewport=list(SIZE), theme="dark", version=1,
                    scenes_hash=sha256(json.dumps(scenes, sort_keys=True).encode()).hexdigest(),
                    masks_hash=sha256((FLX.pattern + TEXT.pattern).encode()).hexdigest())
    head = git("rev-parse", "HEAD")
    baseline = git("rev-parse", "--verify", "--end-of-options", args.shots_baseline + "^{commit}")
    dirty = bool(git("status", "--porcelain", "--untracked-files=all"))
    stamp = datetime.now(timezone.utc).strftime("%Y%m%d-%H%M%S-%f")
    cache = PARITY / "shots_cache"
    base = cache / baseline
    output = cache / f"wip-{stamp}"
    output.mkdir(parents=True)
    crop_dir = PARITY / "shots" / stamp / "worst"
    args.out.mkdir(parents=True, exist_ok=True)
    old_manifest = read(base / "manifest.json") if (base / "manifest.json").is_file() else {}
    has_baseline = all(old_manifest.get(k) == v for k, v in manifest.items())
    compare_live = has_baseline and not (args.refresh_shots and not dirty and baseline == head)
    allowed = HERE / "shots_allow.json"
    allow = read(allowed) if allowed.is_file() else []
    rows = []
    try:
        rp.send("probe_resize", w=SIZE[0], h=SIZE[1])
        restore()
        for scene in scenes:
            slug = scene["slug"]
            try:
                capture(scene, output)
                if not compare_live:
                    rows.append(dict(slug=slug, status="SEEDED", regions=[]))
                    continue
                before, after = bundle(base, slug), bundle(output, slug)
                names = {r["flx"] for r in allow if r["sha"] == head and r["slug"] == slug and r["reason"].strip()}
                row = scene_delta(scene, before, after, names)
                row["status"] = outcome([row])[0]
                regions = sorted(row["regions"], key=lambda i: -i["score"])
                for index, item in enumerate(regions, 1):
                    path = crop_dir / slug / f"{index:02d}_{_safe_flx_name(item['flx'])}.png"
                    crop_panel(before[0], after[0], item["box"], path)
                    item["crop"] = str(path)
                    print(f"{item['severity']} [{slug}] {item['flx']} score 0→{item['score']:.4f} "
                          f"ssim 1→{item['ssim']:.6f} crop={path}")
                rows.append(row)
            except (KeyError, OSError, RuntimeError, ValueError) as error:
                rows.append(dict(slug=slug, status="capture_error", regions=[], error=str(error)))
            finally:
                rp.send("probe_key", keystroke="escape")
    except (OSError, RuntimeError, ValueError) as error:
        rows.append(dict(slug="__setup__", status="capture_error", regions=[], error=str(error)))
    finally:
        try:
            restore()
        except (OSError, RuntimeError, ValueError) as error:
            rows.append(dict(slug="__restore__", status="capture_error", regions=[], error=str(error)))
    status, summary = outcome(rows)
    still_clean = not git("status", "--porcelain", "--untracked-files=all") and git("rev-parse", "HEAD") == head
    if status == "PASS" and not dirty and still_clean:
        head_cache = cache / head
        head_cache.mkdir(exist_ok=True)
        for path in output.iterdir():
            path.replace(head_cache / path.name)
        write(head_cache / "manifest.json", {**manifest, "sha": head, "captured_at": stamp})
        output.rmdir()
        output = head_cache
    if status == "PASS" and not compare_live:
        status = "SEEDED"
    summary.update(status=status, seeded=sum(s["status"] == "SEEDED" for s in rows))
    write(args.out / "shots.json", dict(baseline_sha=baseline, current_sha=head, dirty=dirty,
                                      scenes=rows, summary=summary, capture_dir=str(output)))
    detail = f"scenes={len(scenes)} regressions={summary['scene_failures'] + summary['element_failures']}"
    if status == "SEEDED":
        detail += " (WIP captured; seed from clean HEAD)" if dirty else f" (captured to {output.name})"
    print(f"SHOTS {status} {detail}")
    return int(status == "FAIL")


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    raise SystemExit(main())
