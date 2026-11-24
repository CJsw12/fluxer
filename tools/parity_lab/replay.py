"""Replay an official crawl graph (`crawl.mjs` -> graph.json) on the Rust app through the parity probe,
flag duplicate screens (P0), generate a prioritized M9.4 backlog (`backlog.json` + `backlog.md`),
and optionally enforce `--strict-no-duplicates`.

    python -I tools/parity_lab/replay.py GRAPH_DIR OUT_DIR --link fluxer://channels/<g>/<c>
        [--launch-rust dist/fluxer_client/fluxer_client.exe] [--only-ids 007,013]
        [--skip-flx id1,id2] [--strict-no-duplicates] [--no-extra-popouts]
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
                            (
                                e.get("text")
                                for e in r.get("elements", [])
                                if e.get("flx") == "app.user-area.user-area-inner.user-status"
                            ),
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


def img_l1_diff(a_png: Path, b_png: Path) -> float:
    from PIL import Image, ImageChops, ImageStat  # noqa: PLC0415

    a = Image.open(a_png).convert("RGB")
    b = Image.open(b_png).convert("RGB")
    cw, ch = min(a.width, b.width), min(a.height, b.height)
    if cw < 1 or ch < 1:
        return 0.0
    diff = ImageChops.difference(a.crop((0, 0, cw, ch)), b.crop((0, 0, cw, ch)))
    return round(sum(ImageStat.Stat(diff).mean) / 3.0, 2)


def capture_baseline(width: float, height: float, link: str, out: Path) -> tuple[dict, Path, set[str]]:
    rp.send("probe_resize", w=width, h=height)
    rp.deep_link(link)
    time.sleep(1.2)
    rp.deep_link("fluxer://scene/close-overlays")
    rp.send("probe_key", keystroke="escape")
    rp.send("probe_hover", x=max(10.0, width - 40.0), y=max(10.0, height - 40.0))
    time.sleep(0.45)
    data = rp.send("probe_inspect")
    (out / "baseline.rust.json").write_text(json.dumps(data, ensure_ascii=False), encoding="utf-8")
    png = rp.screenshot(out / "baseline.rust.png")
    ids = {compare.base(e["flx"]) for e in data.get("elements", [])}
    return data, png, ids


def probe_extra_popouts(width: float, height: float, link: str, out: Path, baseline_ids: set[str]) -> list[dict]:
    """Covers critical surfaces outside capped crawl depth/clicks (user-area popout & settings modal)."""
    specs = [
        (
            "extra_user_area_popout",
            "app.user-area.user-area-inner.focus-ring-wrapper",
            [
                "app.floating.user-area-popout.container",
                "app.floating.user-area-popout.action-button",
                "app.floating.user-area-popout.action-button.open-manage-accounts",
                "app.floating.user-area-popout.action-button.copy-user-id",
                "app.floating.user-area-popout.edit-profile-button.open-edit-profile",
            ],
        ),
        (
            "extra_settings_modal",
            "app.user-area.user-area-inner.control-button.settings-click",
            [
                "user.user-settings-modal.modal-root",
                "app.settings-modal-layout.settings-modal-desktop-sidebar.desktop-sidebar",
            ],
        ),
    ]
    extra_rows = []
    for slug, trigger_flx, expected_ids in specs:
        rp.send("probe_resize", w=width, h=height)
        rp.deep_link(link)
        time.sleep(0.7)
        rp.deep_link("fluxer://scene/close-overlays")
        rp.send("probe_hover", x=max(10.0, width - 40.0), y=max(10.0, height - 40.0))
        time.sleep(0.25)
        r = rp.send("probe_click", flx=trigger_flx, index=0)
        if not r.get("ok"):
            extra_rows.append(
                {
                    "id": slug,
                    "trail": [trigger_flx],
                    "layer": True,
                    "status": "trigger_missing",
                    "missing_trigger": trigger_flx,
                }
            )
            print(f"{slug} {'TRIGGER MISSING':60} ({trigger_flx})")
            continue
        time.sleep(0.4)
        ins = rp.send("probe_inspect")
        rjson = out / f"{slug}.rust.json"
        rjson.write_text(json.dumps(ins, ensure_ascii=False), encoding="utf-8")
        rpng = rp.screenshot(out / f"{slug}.rust.png")
        present_base = {compare.base(e["flx"]) for e in ins.get("elements", [])}
        present = [i for i in expected_ids if compare.base(i) in present_base]
        missing = [i for i in expected_ids if compare.base(i) not in present_base]
        share = round(len(present) / len(expected_ids), 3) if expected_ids else 1.0
        status = "opened_full" if share == 1.0 else "opened_partial"
        row = {
            "id": slug,
            "trail": [trigger_flx],
            "layer": True,
            "status": status,
            "added": len(expected_ids),
            "present": len(present),
            "share": share,
            "missing_ids": missing,
            "rust_added": sorted(present_base - baseline_ids),
            "rust_json": str(rjson),
            "rust_png": str(rpng),
        }
        extra_rows.append(row)
        print(f"{slug:8} {trigger_flx[:60]:60} {status} ids {len(present)}/{len(expected_ids)}")
        rp.send("probe_key", keystroke="escape")
        rp.deep_link("fluxer://scene/close-overlays")
        time.sleep(0.2)
    return extra_rows


def build_backlog(results: list[dict], extra_rows: list[dict]) -> list[dict]:
    prio_rank = {
        "P0_duplicate": 0,
        "P1_trigger_missing": 1,
        "P2_opened_partial": 2,
        "P3_geometry_mismatch": 3,
    }
    items = []
    for row in [*results, *extra_rows]:
        st = row["status"]
        geom = row.get("geometry") or {}
        pairs = int(geom.get("pairs", 0))
        within = int(geom.get("within_tol", 0))
        untagged = row.get("top_untagged_official") or []
        if st == "duplicate_screen":
            prio = "P0_duplicate"
        elif st == "trigger_missing":
            prio = "P1_trigger_missing"
        elif st == "opened_partial":
            prio = "P2_opened_partial"
        elif pairs > 0 and within < pairs or untagged:
            prio = "P3_geometry_mismatch"
        else:
            continue
        items.append(
            {
                "priority": prio,
                "id": row["id"],
                "status": st,
                "trail": " > ".join(row.get("trail", [])),
                "trigger_flx": row.get("trigger_flx"),
                "missing_trigger": row.get("missing_trigger"),
                "duplicate_of": row.get("duplicate_of"),
                "duplicate_reason": row.get("duplicate_reason"),
                "present_over_added": f"{row.get('present', 0)}/{row.get('added', 0)}" if "added" in row else None,
                "geometry_ok": f"{within}/{pairs}" if pairs else None,
                "missing_ids": row.get("missing_ids", [])[:12],
                "top_untagged_official": [u.get("flx") for u in untagged[:5]],
                "official_png": row.get("official_png"),
                "official_dom": row.get("official_dom"),
                "rust_png": row.get("rust_png"),
                "diff_heatmap_png": row.get("diff_heatmap_png"),
            }
        )
    items.sort(key=lambda it: (prio_rank.get(it["priority"], 9), it["id"]))
    return items


def write_backlog_md(out: Path, summary: dict, items: list[dict]) -> None:
    lines = [
        "# M9.4 Replay Backlog",
        "",
        f"- Total states replayed: **{summary['states']}** (+ {summary.get('extra_popouts', 0)} extra popouts)",
        f"- `duplicate_screen` (P0): **{summary['duplicate_screen']}**",
        f"- `trigger_missing` (P1): **{summary['trigger_missing']}**",
        f"- `opened_partial` (P2): **{summary['opened_partial']}**",
        f"- `opened_full`: **{summary['opened_full']}**",
        f"- Actionable backlog items (P0..P3): **{len(items)}**",
        "",
        "| Priority | ID | Status | Trail | IDs / Geometry | Detail |",
        "|---|---|---|---|---|---|",
    ]
    for it in items:
        counts = f"{it['present_over_added'] or '-'} ({it['geometry_ok'] or '-'})"
        if it["status"] == "duplicate_screen":
            detail = f"duplicate of `{it['duplicate_of']}` ({it['duplicate_reason']})"
        elif it["status"] == "trigger_missing":
            detail = f"missing trigger `{it['missing_trigger']}`"
        elif it["missing_ids"]:
            detail = "missing: " + ", ".join(f"`{m}`" for m in it["missing_ids"][:4])
        else:
            detail = "untagged/geom delta: " + ", ".join(f"`{u}`" for u in it["top_untagged_official"][:3])
        lines.append(f"| {it['priority']} | `{it['id']}` | `{it['status']}` | {it['trail']} | {counts} | {detail} |")
    lines.extend(["", "## Artifact Paths", ""])
    for it in items:
        refs = [p for p in (it.get("official_png"), it.get("rust_png"), it.get("diff_heatmap_png")) if p]
        if refs:
            lines.append(f"- `{it['id']}` ({it['priority']}): " + " | ".join(refs))
    (out / "backlog.md").write_text("\n".join(lines) + "\n", encoding="utf-8")


def run(
    graph_dir: Path,
    out: Path,
    link: str,
    settle: float = 0.4,
    launch_rust: Path | None = None,
    only_ids: set[str] | None = None,
    skip_flx: set[str] | None = None,
    extra_popouts: bool = True,
) -> dict:
    graph = json.loads((graph_dir / "graph.json").read_text(encoding="utf-8"))
    out.mkdir(parents=True, exist_ok=True)
    proc = launch_rust_same_desktop(launch_rust) if launch_rust else None
    results = []
    extra_rows = []
    try:
        width = float(graph["width"])
        height = float(graph["height"])
        _, _, baseline_ids = capture_baseline(width, height, link, out)
        states_iter = graph["states"]
        if only_ids:
            states_iter = [s for s in states_iter if s["id"] in only_ids]
        for st in states_iter:
            if skip_flx and st["trail"] and any(
                s["flx"] in skip_flx or compare.base(s["flx"]) in skip_flx for s in st["trail"]
            ):
                continue
            rp.send("probe_resize", w=width, h=height)
            rp.deep_link(link)
            time.sleep(0.7)
            rp.deep_link("fluxer://scene/close-overlays")
            for _ in range(2):
                rp.send("probe_key", keystroke="escape")
            rp.send("probe_hover", x=max(10.0, width - 40.0), y=max(10.0, height - 40.0))
            time.sleep(0.25)

            status, missing_trigger = "opened_full", None
            clicked_ok = True
            for step in st["trail"]:
                r = rp.send("probe_click", flx=step["flx"], index=int(step.get("idx", 0)))
                if not r.get("ok") and compare.base(step["flx"]) != step["flx"]:
                    r = rp.send("probe_click", flx=compare.base(step["flx"]), index=int(step.get("idx", 0)))
                if not r.get("ok"):
                    status, missing_trigger = "trigger_missing", step["flx"]
                    clicked_ok = False
                    break
                time.sleep(settle)

            official_dom = graph_dir / "states" / f"{st['id']}.dom.json"
            official_png = graph_dir / "states" / f"{st['id']}.png"
            trail_labels = [s.get("label") or s.get("text") or s["flx"] for s in st["trail"]]
            row = {
                "id": st["id"],
                "trail": trail_labels,
                "trigger_flx": st["trail"][-1]["flx"] if st["trail"] else None,
                "depth": len(st["trail"]),
                "layer": bool(st.get("layer")),
                "status": status,
                "official_dom": str(official_dom),
                "official_png": str(official_png),
            }
            if not clicked_ok:
                row["missing_trigger"] = missing_trigger
                results.append(row)
                print(f"{st['id']} {' > '.join(trail_labels)[:60]:60} TRIGGER MISSING ({missing_trigger})")
                continue

            inspect = rp.send("probe_inspect")
            rust_json = out / f"{st['id']}.rust.json"
            rust_json.write_text(json.dumps(inspect, ensure_ascii=False), encoding="utf-8")
            rust_png = rp.screenshot(out / f"{st['id']}.rust.png")

            # Undo in-place toggles (e.g. Hide members) so later states start clean.
            if st.get("toggle_reverted") and st["trail"]:
                last = st["trail"][-1]
                r2 = rp.send("probe_click", flx=last["flx"], index=int(last.get("idx", 0)))
                if not r2.get("ok") and compare.base(last["flx"]) != last["flx"]:
                    rp.send("probe_click", flx=compare.base(last["flx"]), index=int(last.get("idx", 0)))
                time.sleep(0.2)

            rust_ids = {e["flx"] for e in inspect.get("elements", [])}
            rust_base_ids = {compare.base(r_id) for r_id in rust_ids}
            rust_added = sorted(rust_base_ids - baseline_ids)
            added = list(st["added"])
            present = [a for a in added if a in rust_ids or a in rust_base_ids]
            missing_ids = [a for a in added if a not in present]
            share = len(present) / len(added) if added else 1.0
            row.update(
                {
                    "added": len(added),
                    "present": len(present),
                    "share": round(share, 3),
                    "missing_ids": missing_ids,
                    "rust_added": rust_added,
                    "rust_json": str(rust_json),
                    "rust_png": str(rust_png),
                }
            )
            if share < 1.0:
                row["status"] = "opened_partial"

            report = compare.compare(compare.load_official(official_dom), compare.load_rust(rust_json), 2)
            compare.sample_colors(report, official_png, rust_png, 18)
            heatmap_path = out / f"{st['id']}.diff_heatmap.png"
            compare.sample_visual_anomalies(report, official_png, rust_png, heatmap_path)
            row["geometry"] = report["score"]
            row["top_untagged_official"] = report.get("coverage", {}).get("top_untagged_official", [])
            row["diff_heatmap_png"] = str(heatmap_path)
            (out / f"{st['id']}.report.json").write_text(json.dumps(report, ensure_ascii=False, indent=1), encoding="utf-8")
            results.append(row)
            print(f"{st['id']} {' > '.join(trail_labels)[:60]:60} {row['status']} ids {len(present)}/{len(added)}")

        # Duplicate-screen detector across distinct depth-1 layers.
        depth1_layers = [
            r for r in results if r.get("depth") == 1 and r.get("layer") and r.get("status") != "trigger_missing"
        ]
        for idx, cur in enumerate(depth1_layers):
            cur_off = Path(cur["official_png"])
            cur_rs = Path(cur["rust_png"])
            for prev in depth1_layers[:idx]:
                if cur.get("trigger_flx") == prev.get("trigger_flx"):
                    continue
                off_diff = img_l1_diff(Path(prev["official_png"]), cur_off)
                rs_diff = img_l1_diff(Path(prev["rust_png"]), cur_rs)
                if off_diff > 6.0 and rs_diff < 2.5 and set(cur.get("rust_added", [])) == set(prev.get("rust_added", [])):
                    cur["status"] = "duplicate_screen"
                    cur["duplicate_of"] = prev["id"]
                    cur["duplicate_reason"] = f"off_L1={off_diff} vs rust_L1={rs_diff} (same rust_added)"
                    print(f"  [DUPLICATE] {cur['id']} duplicates {prev['id']}: {cur['duplicate_reason']}")
                    break

        if extra_popouts:
            extra_rows = probe_extra_popouts(width, height, link, out, baseline_ids)

        rp.deep_link("fluxer://scene/close-overlays")
        rp.send("probe_key", keystroke="escape")
    finally:
        if proc is not None:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()

    summary = {
        "states": len(results),
        "extra_popouts": len(extra_rows),
        "duplicate_screen": sum(r["status"] == "duplicate_screen" for r in results),
        "trigger_missing": sum(r["status"] == "trigger_missing" for r in results),
        "opened_partial": sum(r["status"] == "opened_partial" for r in results),
        "opened_full": sum(r["status"] == "opened_full" for r in results),
    }
    backlog_items = build_backlog(results, extra_rows)
    payload = {
        "route_link": link,
        "official_client": graph.get("client"),
        "summary": summary,
        "states": results,
        "extra_popouts": extra_rows,
        "backlog": backlog_items,
    }
    (out / "replay.json").write_text(json.dumps(payload, ensure_ascii=False, indent=1), encoding="utf-8")
    (out / "backlog.json").write_text(
        json.dumps({"summary": summary, "items": backlog_items}, ensure_ascii=False, indent=1),
        encoding="utf-8",
    )
    write_backlog_md(out, summary, backlog_items)
    return payload


def parse_args(argv: list[str]) -> dict:
    flags = {"--strict-no-duplicates", "--no-extra-popouts"}
    opts: dict[str, object] = {"positional": [], "strict_no_duplicates": False, "extra_popouts": True}
    i = 0
    while i < len(argv):
        a = argv[i]
        if a == "--strict-no-duplicates":
            opts["strict_no_duplicates"] = True
            i += 1
        elif a == "--no-extra-popouts":
            opts["extra_popouts"] = False
            i += 1
        elif a.startswith("--") and i + 1 < len(argv):
            opts[a[2:].replace("-", "_")] = argv[i + 1]
            i += 2
        elif a in flags:
            i += 1
        else:
            opts["positional"].append(a)  # type: ignore[union-attr]
            i += 1
    return opts


def main(argv: list[str]) -> int:
    sys.stdout.reconfigure(encoding="utf-8")
    opts = parse_args(argv)
    pos = opts["positional"]
    if len(pos) < 2 or "link" not in opts:  # type: ignore[arg-type]
        print(__doc__)
        return 2
    only_ids = set(str(opts["only_ids"]).split(",")) if "only_ids" in opts else None
    skip_flx = set(str(opts["skip_flx"]).split(",")) if "skip_flx" in opts else None
    launch_rust = Path(str(opts["launch_rust"])) if "launch_rust" in opts else None
    res = run(
        Path(pos[0]),  # type: ignore[index]
        Path(pos[1]),  # type: ignore[index]
        str(opts["link"]),
        launch_rust=launch_rust,
        only_ids=only_ids,
        skip_flx=skip_flx,
        extra_popouts=bool(opts["extra_popouts"]),
    )
    print(json.dumps(res["summary"]))
    if opts["strict_no_duplicates"] and res["summary"]["duplicate_screen"] > 0:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
