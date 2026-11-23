#!/usr/bin/env python3
"""Verify tools/parity_lab/icon_manifest.json against the icon assets.

- every referenced asset/source file exists
- Phosphor-sourced assets have path data identical to target/phosphor-core/assets/<weight>/<name>.svg
- custom icons have path data identical to the `d="..."` of the TSX source
Exit code 1 on any mismatch.
"""
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PATH_RE = re.compile(r'\sd="([^"]*)"')


def paths(text: str) -> list[str]:
    return [re.sub(r"\s+", " ", p).strip() for p in PATH_RE.findall(text)]


def main() -> int:
    manifest = json.loads((ROOT / "tools/parity_lab/icon_manifest.json").read_text(encoding="utf-8"))
    errors: list[str] = []
    checked: set[str] = set()
    for site in manifest["sites"]:
        asset = ROOT / site["rust_asset"]
        if not asset.is_file():
            errors.append(f"missing asset {site['rust_asset']}")
            continue
        if not (ROOT / site["file"]).is_file():
            errors.append(f"missing call-site file {site['file']}")
        if site.get("source_file") and not (ROOT / site["source_file"]).is_file():
            errors.append(f"missing official source {site['source_file']}")
        if site["rust_asset"] in checked:
            continue
        checked.add(site["rust_asset"])
        got = paths(asset.read_text(encoding="utf-8"))
        if site["kind"] == "custom":
            ref = ROOT / site["custom_source"]
            if not ref.is_file():
                errors.append(f"missing custom source {site['custom_source']}")
                continue
            want = paths(ref.read_text(encoding="utf-8"))[: len(got)]  # skip clipPath rects
        else:
            name = site.get("phosphor")
            if not name:
                errors.append(f"{site['rust_asset']} has no Phosphor original")
                continue
            ref = ROOT / "target/phosphor-core/assets" / name
            if not ref.is_file():
                errors.append(f"missing Phosphor original {name}")
                continue
            want = paths(ref.read_text(encoding="utf-8"))
        if got != want:
            errors.append(f"path mismatch {site['rust_asset']} vs {ref.relative_to(ROOT)}")
    for e in errors:
        print("ERROR", e)
    print(f"{len(manifest['sites'])} sites, {len(checked)} assets checked, {len(errors)} errors")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
