"""One-shot scene check against the running Rust app (parity probe on).

    python -I tools/parity_lab/check_scene.py OFFICIAL.dom.json OFFICIAL.png fluxer://... OUT_DIR
        [--official-offset 0,32] [--tol 2]

resize 1280×800 → deep link → Escape ×2 → settle → inspect + screenshot → compare.py report and
overlay. Exit code 0 only when every scored pair is within tolerance.
"""

from __future__ import annotations

import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import compare  # noqa: E402
import rust_probe as rp  # noqa: E402


def main(argv: list[str]) -> int:
    sys.stdout.reconfigure(encoding="utf-8")
    if len(argv) < 4:
        print(__doc__)
        return 2
    official_dom, official_png, link, out = Path(argv[0]), Path(argv[1]), argv[2], Path(argv[3])
    opts = dict(zip(argv[4::2], argv[5::2]))
    out.mkdir(parents=True, exist_ok=True)
    rp.send("probe_resize", w=1280.0, h=800.0)
    rp.deep_link(link)
    time.sleep(1.5)
    for _ in range(2):
        rp.send("probe_key", keystroke="escape")
    rp.send("probe_hover", x=1240.0, y=760.0)  # park the pointer like the official capture
    time.sleep(0.8)
    inspect = rp.send("probe_inspect")
    (out / "rust.json").write_text(json.dumps(inspect, ensure_ascii=False), encoding="utf-8")
    rust_png = rp.screenshot(out / "rust.png")
    off = tuple(int(v) for v in opts.get("--official-offset", "0,0").split(","))
    report = compare.compare(compare.load_official(official_dom, off), compare.load_rust(out / "rust.json"), int(opts.get("--tol", 2)))
    report["official_offset"] = list(off)
    compare.sample_colors(report, official_png, rust_png, float(opts.get("--color-tol", 18)))
    compare.overlay(report, official_png, rust_png, out / "overlay.png") if not any(off) else None
    (out / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=1), encoding="utf-8")
    s, c = report["score"], report["coverage"]
    bad = [r for r in report["rows"] if r["status"] != "ok"]
    print(f"geometry+color: {s['pairs'] - sum(r['status'] == 'mismatch' for r in report['rows'])}/{s['pairs']} ok | rust tags {c['rust_tagged_ids']} | official ids {c['official_ids']}")
    for r in bad:
        extra = f" colorΔ={r['color_delta']}" if r.get("color_delta", 0) > 18 else ""
        print(f"  {r['status']:15} {r['flx'][:72]:72} off={r.get('official')} rust={r.get('rust')} d={r.get('delta')}{extra}")
    return 0 if not bad else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
