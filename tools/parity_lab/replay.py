"""Replay an official crawl graph (`crawl.mjs` → graph.json) on the Rust app through the parity probe
and score, per state, whether the same trigger exists and how much of the opened UI is present.

    python -I tools/parity_lab/replay.py GRAPH_DIR OUT_DIR --link fluxer://channels/<g>/<c>

For every state: resize 1280×800 → deep link to the route → Escape twice → click each trail id
(`probe_click flx index`) → inspect + screenshot → compare.py against the official state DOM.
Result `replay.json` is the "every window a button opens" checklist with a status per state:
  trigger_missing   the Rust app has no element tagged with the official trigger id
  opened_partial    trigger clicked; share of the official state's new ids present in Rust (< 100%)
  opened_full       every official new id is present (geometry is then judged by compare.py)
"""

from __future__ import annotations

import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import compare  # noqa: E402
import rust_probe as rp  # noqa: E402


def run(graph_dir: Path, out: Path, link: str, settle: float = 0.9) -> dict:
    graph = json.loads((graph_dir / "graph.json").read_text(encoding="utf-8"))
    out.mkdir(parents=True, exist_ok=True)
    results = []
    for st in graph["states"]:
        rp.send("probe_resize", w=float(graph["width"]), h=float(graph["height"]))
        rp.deep_link(link)
        time.sleep(1.2)
        for _ in range(2):
            rp.send("probe_key", keystroke="escape")
        time.sleep(0.3)
        status, missing_trigger = "opened_full", None
        for step in st["trail"]:
            r = rp.send("probe_click", flx=step["flx"], index=int(step.get("idx", 0)))
            if not r.get("ok") and compare.base(step["flx"]) != step["flx"]:
                r = rp.send("probe_click", flx=compare.base(step["flx"]), index=int(step.get("idx", 0)))
            if not r.get("ok"):
                status, missing_trigger = "trigger_missing", step["flx"]
                break
            time.sleep(settle)
        row = {"id": st["id"], "trail": [s.get("label") or s.get("text") or s["flx"] for s in st["trail"]], "status": status}
        if status == "trigger_missing":
            row["missing_trigger"] = missing_trigger
            results.append(row)
            print(f"{st['id']} {' > '.join(row['trail'])[:60]:60} TRIGGER MISSING ({missing_trigger})")
            continue
        inspect = rp.send("probe_inspect")
        rust_json = out / f"{st['id']}.rust.json"
        rust_json.write_text(json.dumps(inspect, ensure_ascii=False), encoding="utf-8")
        rust_png = rp.screenshot(out / f"{st['id']}.rust.png")
        rust_ids = {e["flx"] for e in inspect.get("elements", [])}
        added = list(st["added"])  # crawl.mjs records base ids
        present = [a for a in added if a in rust_ids or any(compare.base(r) == a for r in rust_ids)]
        share = len(present) / len(added) if added else 1.0
        row.update({"added": len(added), "present": len(present), "share": round(share, 3)})
        if share < 1.0:
            row["status"] = "opened_partial"
        official_dom = graph_dir / "states" / f"{st['id']}.dom.json"
        report = compare.compare(compare.load_official(official_dom), compare.load_rust(rust_json), 2)
        compare.sample_colors(report, graph_dir / "states" / f"{st['id']}.png", rust_png, 18)
        row["geometry"] = report["score"]
        (out / f"{st['id']}.report.json").write_text(json.dumps(report, ensure_ascii=False, indent=1), encoding="utf-8")
        results.append(row)
        print(f"{st['id']} {' > '.join(row['trail'])[:60]:60} {row['status']} ids {len(present)}/{len(added)}")
    summary = {
        "states": len(results),
        "trigger_missing": sum(r["status"] == "trigger_missing" for r in results),
        "opened_partial": sum(r["status"] == "opened_partial" for r in results),
        "opened_full": sum(r["status"] == "opened_full" for r in results),
    }
    payload = {"route_link": link, "official_client": graph.get("client"), "summary": summary, "states": results}
    (out / "replay.json").write_text(json.dumps(payload, ensure_ascii=False, indent=1), encoding="utf-8")
    rp.send("probe_key", keystroke="escape")
    return payload


def main(argv: list[str]) -> int:
    sys.stdout.reconfigure(encoding="utf-8")
    if len(argv) < 2:
        print(__doc__)
        return 2
    opts = dict(zip(argv[2::2], argv[3::2]))
    res = run(Path(argv[0]), Path(argv[1]), opts["--link"])
    print(json.dumps(res["summary"]))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
