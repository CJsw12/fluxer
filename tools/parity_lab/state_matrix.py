"""M9.2 state-matrix capture & diff runner: official (CDP) vs Rust (parity probe).

    python -I tools/parity_lab/state_matrix.py --out DIR [--voice] [--launch-rust EXE]
        [--official-dir DIR] [--attach 9334]

Captures baseline + 14 hover states (+ optional voice/mute/deafen states with mic ALWAYS muted)
on both apps at 1280x800, runs geometry+color comparison per state, diffs newly-appeared elements
(tooltips, action bars, voice badges, voice panel) and hover target pixel/color deltas.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import compare  # noqa: E402
import rust_probe as rp  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
GUILD = "1572284491404148736"
CHANNEL = "1572284491404148743"
VOICE_CHANNEL = "1572284491404148740"
MUTE_FLX = "app.user-area.user-area-inner.control-button.mute"
DEAFEN_FLX = "app.user-area.user-area-inner.control-button.deafen"


def box_stats(png: Path, rect: list[int]) -> tuple[list[int], float] | None:
    from PIL import Image, ImageStat  # noqa: PLC0415

    im = Image.open(png).convert("RGB")
    x, y, w, h = rect
    crop = im.crop((max(0, x), max(0, y), min(im.width, x + w), min(im.height, y + h)))
    if crop.width < 1 or crop.height < 1:
        return None
    stat = ImageStat.Stat(crop)
    return [round(v, 1) for v in stat.mean], round(sum(stat.stddev) / 3.0, 1)


def box_l1_diff(a_png: Path, b_png: Path, rect: list[int]) -> float | None:
    from PIL import Image, ImageChops, ImageStat  # noqa: PLC0415

    a = Image.open(a_png).convert("RGB")
    b = Image.open(b_png).convert("RGB")
    x, y, w, h = rect
    box = (max(0, x), max(0, y), min(a.width, b.width, x + w), min(a.height, b.height, y + h))
    if box[2] <= box[0] or box[3] <= box[1]:
        return None
    diff = ImageChops.difference(a.crop(box), b.crop(box))
    return round(sum(ImageStat.Stat(diff).mean) / 3.0, 2)


def box_changed_pixels(a_png: Path, b_png: Path, rect: list[int], thresh: int = 28) -> int:
    """Counts pixels inside `rect` whose RGB L1 delta exceeds `thresh` (ignores subtle bg tint)."""
    from PIL import Image, ImageChops  # noqa: PLC0415

    a = Image.open(a_png).convert("RGB")
    b = Image.open(b_png).convert("RGB")
    x, y, w, h = rect
    box = (max(0, x), max(0, y), min(a.width, b.width, x + w), min(a.height, b.height, y + h))
    if box[2] <= box[0] or box[3] <= box[1]:
        return 0
    diff = ImageChops.difference(a.crop(box), b.crop(box))
    pxs = diff.get_flattened_data() if hasattr(diff, "get_flattened_data") else diff.getdata()
    return sum(1 for r, g, bl in pxs if (r + g + bl) / 3.0 > thresh)


def is_red_active(png: Path, rect: list[int]) -> bool:
    stats = box_stats(png, rect)
    if not stats:
        return False
    r, g, b = stats[0]
    return r > g + 12 and r > b + 10


def find_flx_rect(inspect_data: dict, flx: str, index: int = 0) -> list[int] | None:
    hits = [e for e in inspect_data.get("elements", []) if e.get("flx") == flx]
    if 0 <= index < len(hits):
        return [round(v) for v in hits[index]["rect"]]
    return None


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


def rust_park_and_inspect(slug: str, out_dir: Path, settle: float = 0.5) -> tuple[dict, Path]:
    time.sleep(settle)
    data = rp.send("probe_inspect")
    (out_dir / f"{slug}.json").write_text(json.dumps(data, ensure_ascii=False), encoding="utf-8")
    png = rp.screenshot(out_dir / f"{slug}.png")
    return data, png


def ensure_rust_muted(out_dir: Path) -> str:
    data = rp.send("probe_inspect")
    rect = find_flx_rect(data, MUTE_FLX)
    if not rect:
        raise RuntimeError("mute button rect not found in Rust inspect")
    pre = rp.screenshot(out_dir / "_mic_check0.png")
    if is_red_active(pre, rect):
        return "already_muted"
    rp.send("probe_click", flx=MUTE_FLX)
    time.sleep(0.4)
    post = rp.screenshot(out_dir / "_mic_check1.png")
    if not is_red_active(post, rect):
        raise RuntimeError(f"failed to confirm Rust mic mute (box mean={box_stats(post, rect)})")
    return "muted_now"


def main(argv: list[str]) -> int:
    sys.stdout.reconfigure(encoding="utf-8")
    voice = "--voice" in argv
    strict = "--strict" in argv
    clean_args = [a for a in argv if a not in ("--voice", "--strict")]
    opts = dict(zip(clean_args[0::2], clean_args[1::2]))
    out = Path(opts.get("--out", "Logs/parity/m92_baseline"))
    attach = int(opts.get("--attach", 9334))
    launch_exe = opts.get("--launch-rust")
    off_dir = Path(opts["--official-dir"]) if "--official-dir" in opts else out / "official"
    rust_dir = out / "rust"
    cmp_dir = out / "compare"
    for d in (out, off_dir, rust_dir, cmp_dir):
        d.mkdir(parents=True, exist_ok=True)

    if "--official-dir" not in opts:
        cmd = ["node", str(REPO / "tools" / "parity_lab" / "state_official.mjs"), "--attach", str(attach), "--origin", "fluxer-app://app", "--out", str(off_dir)]
        if voice:
            cmd.append("--voice")
        r = subprocess.run(cmd, cwd=REPO, capture_output=False, text=True, encoding="utf-8")
        if r.returncode != 0:
            print("official capture failed")
            return 1

    manifest = json.loads((off_dir / "manifest.json").read_text(encoding="utf-8"))
    proc = launch_rust_same_desktop(Path(launch_exe)) if launch_exe else None
    rust_states: dict[str, dict] = {}
    joined_voice = False

    try:
        rp.send("probe_resize", w=1280.0, h=800.0)
        rp.deep_link(f"fluxer://channels/{GUILD}/{CHANNEL}")
        time.sleep(1.5)
        for _ in range(2):
            rp.send("probe_key", keystroke="escape")
        rp.send("probe_hover", x=1240.0, y=760.0)
        rest_data, rest_png = rust_park_and_inspect("rest", rust_dir, settle=0.8)
        rust_states["rest"] = {"data": rest_data, "png": rest_png}

        # Hover states from official manifest
        for st in manifest["states"]:
            slug = st["slug"]
            if not slug.startswith("hover_") or st.get("missing"):
                continue
            target = st["target"]
            flx, idx = target["flx"], target["idx"]
            rp.send("probe_hover", x=1240.0, y=760.0)
            time.sleep(0.35)
            hits = [e for e in rest_data.get("elements", []) if e.get("flx") == flx]
            if idx < len(hits):
                if target.get("offset"):
                    r0 = hits[idx]["rect"]
                    ox, oy = target["offset"]
                    r_hover = rp.send("probe_hover", x=float(r0[0] + ox), y=float(r0[1] + oy))
                    method = "flx_offset"
                else:
                    r_hover = rp.send("probe_hover", flx=flx, index=idx)
                    method = "flx"
            else:
                cx, cy = target["center"]
                r_hover = rp.send("probe_hover", x=float(cx), y=float(cy))
                method = "coord_fallback"
            data, png = rust_park_and_inspect(slug, rust_dir, settle=0.55)
            rust_states[slug] = {"data": data, "png": png, "hover_method": method, "hover_reply": r_hover}

        if voice:
            mic_status = ensure_rust_muted(rust_dir)
            rp.deep_link(f"fluxer://scene/voice/{GUILD}/{VOICE_CHANNEL}")
            joined_voice = True
            time.sleep(5.0)
            rp.send("probe_hover", x=1240.0, y=760.0)
            d, p = rust_park_and_inspect("voice_stage", rust_dir, settle=0.8)
            rust_states["voice_stage"] = {"data": d, "png": p, "mic_status": mic_status}

            rp.deep_link(f"fluxer://scene/voice-split/{GUILD}/{VOICE_CHANNEL}")
            time.sleep(1.5)
            rp.send("probe_hover", x=1240.0, y=760.0)
            d, p = rust_park_and_inspect("voice_stage_split", rust_dir, settle=0.6)
            rust_states["voice_stage_split"] = {"data": d, "png": p}

            rp.deep_link(f"fluxer://channels/{GUILD}/{CHANNEL}")
            time.sleep(1.5)
            rp.send("probe_hover", x=1240.0, y=760.0)
            d, p = rust_park_and_inspect("voice_in_markdown", rust_dir, settle=0.6)
            rust_states["voice_in_markdown"] = {"data": d, "png": p}

            if any(st["slug"] == "voice_deafened" for st in manifest["states"]):
                rp.send("probe_click", flx=DEAFEN_FLX)
                time.sleep(0.6)
                rp.send("probe_hover", x=1240.0, y=760.0)
                d, p = rust_park_and_inspect("voice_deafened", rust_dir, settle=0.5)
                deaf_rect = find_flx_rect(d, DEAFEN_FLX)
                rust_states["voice_deafened"] = {
                    "data": d,
                    "png": p,
                    "deaf_red_active": bool(deaf_rect and is_red_active(p, deaf_rect)),
                }
                rp.send("probe_click", flx=DEAFEN_FLX)
                time.sleep(0.4)
    finally:
        if joined_voice:
            try:
                rp.deep_link("fluxer://scene/voice-leave")
                time.sleep(0.8)
                rp.deep_link(f"fluxer://channels/{GUILD}/{CHANNEL}")
            except Exception:
                pass
        if proc is not None:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()

    off_rest_dom = json.loads((off_dir / "rest.dom.json").read_text(encoding="utf-8"))
    off_rest_ids = {compare.base(e["flx"]) for e in off_rest_dom}
    rust_rest_ids = {compare.base(e["flx"]) for e in rust_states["rest"]["data"].get("elements", [])}

    summary_rows = []
    for st in manifest["states"]:
        slug = st["slug"]
        if st.get("missing") or slug not in rust_states:
            summary_rows.append({"slug": slug, "status": "skipped", "official_missing": st.get("missing", False)})
            continue
        off_dom_path = off_dir / f"{slug}.dom.json"
        off_png_path = off_dir / f"{slug}.png"
        rs_json_path = rust_dir / f"{slug}.json"
        rs_png_path = rust_dir / f"{slug}.png"
        s_cmp_dir = cmp_dir / slug
        s_cmp_dir.mkdir(parents=True, exist_ok=True)

        rep = compare.compare(compare.load_official(off_dom_path), compare.load_rust(rs_json_path), tol=2)
        compare.sample_colors(rep, off_png_path, rs_png_path, max_delta=18.0)
        compare.sample_visual_anomalies(rep, off_png_path, rs_png_path, s_cmp_dir / "diff_heatmap.png" if slug == "rest" else None)
        compare.overlay(rep, off_png_path, rs_png_path, s_cmp_dir / "overlay.png")
        (s_cmp_dir / "report.json").write_text(json.dumps(rep, ensure_ascii=False, indent=1), encoding="utf-8")

        off_dom = json.loads(off_dom_path.read_text(encoding="utf-8"))
        off_added = sorted({compare.base(e["flx"]) for e in off_dom} - off_rest_ids)
        rs_added = sorted({compare.base(e["flx"]) for e in rust_states[slug]["data"].get("elements", [])} - rust_rest_ids)

        extra: dict = {}
        strict_failures: list[str] = []
        if slug.startswith("hover_") and "target" in st:
            t_rect = st["target"]["rect"]
            rs_rect = find_flx_rect(rust_states["rest"]["data"], st["target"]["flx"], st["target"]["idx"]) or t_rect
            extra["hover_method"] = rust_states[slug].get("hover_method")
            extra["official_style_diff"] = st.get("style_diff", {})
            extra["official_tooltips"] = [
                {"text": tt["text"], "rect": tt["rect"], "bg": tt["style"].get("backgroundColor")}
                for tt in st.get("tooltips", [])
            ]
            extra["official_box_delta"] = box_l1_diff(off_dir / "rest.png", off_png_path, t_rect)
            extra["rust_box_delta"] = box_l1_diff(rust_states["rest"]["png"], rs_png_path, rs_rect)
            extra["official_box_mean"] = [box_stats(off_dir / "rest.png", t_rect), box_stats(off_png_path, t_rect)]
            extra["rust_box_mean"] = [box_stats(rust_states["rest"]["png"], rs_rect), box_stats(rs_png_path, rs_rect)]
            cflx = st.get("container_flx")
            if cflx:
                off_cr = st.get("container_rect")
                rs_cr = find_flx_rect(rust_states["rest"]["data"], cflx, 0) or off_cr
                if off_cr and rs_cr:
                    off_chg = box_changed_pixels(off_dir / "rest.png", off_png_path, off_cr, thresh=28)
                    rs_chg = box_changed_pixels(rust_states["rest"]["png"], rs_png_path, rs_cr, thresh=28)
                    phantom = rs_chg > max(off_chg * 2.5 + 220, 350)
                    extra["container_flx"] = cflx
                    extra["container_bright_changed_off"] = off_chg
                    extra["container_bright_changed_rust"] = rs_chg
                    extra["phantom_hover_ui"] = phantom
                    if phantom:
                        strict_failures.append(f"{slug}: phantom_hover_ui (bright px rust={rs_chg} off={off_chg} in {cflx})")
            if extra["official_tooltips"] and not rs_added:
                strict_failures.append(f"{slug}: missing rust tooltip")
        elif slug.startswith("voice_"):
            for rrow in rep["rows"]:
                if rrow["flx"] == "app.user-area.user-area-inner.section" and any(abs(v) > 4 for v in rrow.get("delta", [])):
                    strict_failures.append(f"{slug}: user-area delta {rrow['delta']}")

        ok_cnt = rep["score"]["pairs"] - sum(1 for r in rep["rows"] if r["status"] == "mismatch")
        row = {
            "slug": slug,
            "pairs_ok": f"{ok_cnt}/{rep['score']['pairs']}",
            "official_added": off_added,
            "rust_added": rs_added,
            "strict_failures": strict_failures,
            "mismatches": [
                {"flx": r["flx"], "delta": r.get("delta"), "color_delta": r.get("color_delta"), "edge_p95": r.get("edge_p95")}
                for r in rep["rows"] if r["status"] == "mismatch"
            ][:15],
            **extra,
        }
        summary_rows.append(row)

    all_strict_failures = [msg for r in summary_rows for msg in r.get("strict_failures", [])]
    report = {
        "official_client": manifest.get("client"),
        "channels": manifest.get("channels", []),
        "initial_audio": manifest.get("initial_audio"),
        "voice_states": manifest.get("voice_states"),
        "blocked_writes": manifest.get("blocked_writes", []),
        "strict_failures": all_strict_failures,
        "states": summary_rows,
    }
    (out / "matrix_report.json").write_text(json.dumps(report, ensure_ascii=False, indent=1), encoding="utf-8")

    print("\n=== M9.2 STATE MATRIX SUMMARY ===")
    for r in summary_rows:
        if r.get("status") == "skipped":
            print(f"{r['slug']:22} SKIPPED")
            continue
        extra_str = ""
        if "official_box_delta" in r:
            phantom_str = f" cpx={r['container_bright_changed_off']}->{r['container_bright_changed_rust']}" if "container_bright_changed_rust" in r else ""
            if r.get("phantom_hover_ui"):
                phantom_str += " [PHANTOM!]"
            extra_str = f" | boxΔ off={r['official_box_delta']} rust={r['rust_box_delta']} tips={len(r['official_tooltips'])}{phantom_str} styleΔ={list(r['official_style_diff'])}"
        print(f"{r['slug']:22} {r['pairs_ok']:8} +off={len(r['official_added']):2} +rust={len(r['rust_added']):2}{extra_str}")
        if r["official_added"] != r["rust_added"]:
            print(f"    +off : {r['official_added'][:8]}")
            print(f"    +rust: {r['rust_added'][:8]}")
    if all_strict_failures:
        print("\nSTRICT FAILURES:")
        for msg in all_strict_failures:
            print(f"  - {msg}")
    return 1 if (strict and all_strict_failures) else 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
