#!/usr/bin/env python3
"""Verify tools/parity_lab/icon_manifest.json against the icon assets and call sites.

- every referenced asset/source file exists
- Phosphor-sourced assets have path data identical to target/phosphor-core/assets/<weight>/<name>.svg
- custom icons have path data identical to the `d="..."` of the TSX source
- every `AppIcon::<Variant>` call site under fluxer_client/app/src is listed in the manifest,
  matched by (file, rust_icon, occurrence order) so line drift keeps the evidence fields

`--sync` rewrites the manifest: line numbers are updated, new call sites are appended as
`unverified` (Phosphor original resolved by identical path data) and call sites that no longer
exist are dropped. `--dry-run` with `--sync` prints the outcome without writing the manifest.
Exit code 1 on any mismatch, new call site or gone call site.
"""
import argparse
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PATH_RE = re.compile(r'\sd="([^"]*)"')
MANIFEST_PATH = ROOT / "tools/parity_lab/icon_manifest.json"
APP_SRC = ROOT / "fluxer_client/app/src"
ICONS_RS = APP_SRC / "icons.rs"
PHOSPHOR_ROOT = ROOT / "target/phosphor-core/assets"
RUST_ASSET_PREFIX = "fluxer_client/app/assets/icons/"
ASSET_ARM_RE = re.compile(r'Self::(\w+)\s*=>\s*"icons/phosphor/([^"]+)"')
CALL_SITE_RE = re.compile(r"\bAppIcon::([A-Za-z_][A-Za-z0-9_]*)")
TEST_ATTR_RE = re.compile(r"^#\[cfg\(test\)\]")
MOD_START_RE = re.compile(r"^(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{")
WEIGHT_ORDER = {"bold": 0, "fill": 1, "duotone": 2, "regular": 3, "light": 4, "thin": 5}
NEW_REASON = "new call site, not yet matched to official harvest"


def paths(text: str) -> list[str]:
    return [re.sub(r"\s+", " ", p).strip() for p in PATH_RE.findall(text)]


def code_only(line: str, state: dict) -> str:
    """Return `line` without string literals and comments, carrying `state` across lines."""
    out: list[str] = []
    i = 0
    while i < len(line):
        if state["block"]:
            if line.startswith("*/", i):
                state["block"] -= 1
                i += 2
            elif line.startswith("/*", i):
                state["block"] += 1
                i += 2
            else:
                i += 1
            continue
        if state["string"]:
            if line[i] == "\\":
                i += 2
                continue
            if line[i] == '"':
                state["string"] = False
            i += 1
            continue
        if line.startswith("//", i):
            break
        if line.startswith("/*", i):
            state["block"] = 1
            i += 2
            continue
        if line[i] == '"':
            state["string"] = True
            i += 1
            continue
        out.append(line[i])
        i += 1
    return "".join(out)


def live_lines(text: str):
    """Yield (line number, code) for lines outside `#[cfg(test)]` modules."""
    state = {"block": 0, "string": False}
    skip_depth = 0
    pending_test = False
    for lineno, raw in enumerate(text.splitlines(), 1):
        code = code_only(raw, state)
        stripped = code.strip()
        if skip_depth:
            skip_depth += code.count("{") - code.count("}")
            if skip_depth <= 0:
                skip_depth = 0
            continue
        if TEST_ATTR_RE.match(stripped):
            pending_test = True
            continue
        if pending_test and MOD_START_RE.match(stripped):
            pending_test = False
            skip_depth = code.count("{") - code.count("}")
            if skip_depth <= 0:
                skip_depth = 0
            continue
        if not stripped or stripped.startswith("#[") or stripped.startswith("//"):
            continue
        pending_test = False
        yield lineno, code


def app_icon_assets() -> dict[str, str]:
    """Map `AppIcon` variant to the manifest `rust_asset` path used for it."""
    text = ICONS_RS.read_text(encoding="utf-8")
    mapping: dict[str, str] = {}
    for variant, name in ASSET_ARM_RE.findall(text):
        mapping.setdefault(variant, RUST_ASSET_PREFIX + name.rsplit("/", 1)[-1])
    return mapping


def scan_call_sites(assets: dict[str, str]) -> tuple[list[dict], list[str]]:
    """Collect every `AppIcon::<Variant>` use outside icons.rs and tests."""
    sites: list[dict] = []
    unknown: list[str] = []
    for path in sorted(APP_SRC.rglob("*.rs")):
        if path.name == "icons.rs" or path.name.startswith("tests"):
            continue
        rel = path.relative_to(ROOT).as_posix()
        text = path.read_text(encoding="utf-8")
        for lineno, code in live_lines(text):
            for variant in CALL_SITE_RE.findall(code):
                asset = assets.get(variant)
                if asset is None:
                    unknown.append(f"{rel}:{lineno}: AppIcon::{variant} has no icons.rs asset arm")
                sites.append({"rust_icon": variant, "file": rel, "line": lineno, "rust_asset": asset})
    return sites, unknown


def group_by_site(sites: list[dict]) -> dict[tuple[str, str], list[dict]]:
    grouped: dict[tuple[str, str], list[dict]] = {}
    for site in sites:
        grouped.setdefault((site["file"], site["rust_icon"]), []).append(site)
    return grouped


def compare(manifest_sites: list[dict], source_sites: list[dict]) -> tuple[list, list, list]:
    """Pair manifest and source sites; return (pairs, new, gone)."""
    manifest_groups = group_by_site(manifest_sites)
    source_groups = group_by_site(source_sites)
    pairs: list[tuple[dict, dict]] = []
    new: list[dict] = []
    gone: list[dict] = []
    for key, msites in manifest_groups.items():
        ssites = source_groups.get(key, [])
        for index, msite in enumerate(msites):
            if index < len(ssites):
                pairs.append((msite, ssites[index]))
            else:
                gone.append(msite)
    for key, ssites in source_groups.items():
        already = len(manifest_groups.get(key, []))
        new.extend(ssites[already:])
    new.sort(key=lambda site: (site["file"], site["line"], site["rust_icon"]))
    return pairs, new, gone


class PhosphorIndex:
    """Index target/phosphor-core/assets by normalised path data."""

    def __init__(self) -> None:
        self._index: dict[tuple[str, ...], list[str]] | None = None

    def _build(self) -> None:
        index: dict[tuple[str, ...], list[str]] = {}
        if PHOSPHOR_ROOT.is_dir():
            for svg in sorted(PHOSPHOR_ROOT.rglob("*.svg")):
                key = tuple(paths(svg.read_text(encoding="utf-8")))
                if not key:
                    continue
                index.setdefault(key, []).append(svg.relative_to(PHOSPHOR_ROOT).as_posix())
        self._index = index

    def find(self, asset: Path) -> str | None:
        if self._index is None:
            self._build()
        assert self._index is not None
        matches = self._index.get(tuple(paths(asset.read_text(encoding="utf-8"))))
        if not matches:
            return None
        return min(matches, key=lambda rel: (WEIGHT_ORDER.get(rel.split("/", 1)[0], 99), rel))


def sync_sites(manifest_sites: list[dict], pairs: list, new: list[dict], gone: list[dict]) -> list[dict]:
    """Return the manifest sites with updated lines, appended new sites and gone sites removed."""
    gone_ids = {id(site) for site in gone}
    lines = {id(msite): ssite["line"] for msite, ssite in pairs}
    index = PhosphorIndex()
    sites: list[dict] = []
    for site in manifest_sites:
        if id(site) in gone_ids:
            continue
        entry = dict(site)
        if id(site) in lines:
            entry["line"] = lines[id(site)]
        sites.append(entry)
    for src in new:
        asset = ROOT / src["rust_asset"] if src["rust_asset"] else None
        phosphor = index.find(asset) if asset is not None and asset.is_file() else None
        if phosphor is None:
            print(f"WARN no Phosphor original for new call site {src['file']}:{src['line']} {src['rust_icon']}")
        sites.append(
            {
                "rust_icon": src["rust_icon"],
                "file": src["file"],
                "line": src["line"],
                "rust_asset": src["rust_asset"],
                "kind": "phosphor",
                "phosphor": phosphor,
                "status": "unverified",
                "official_component": None,
                "official_icon": None,
                "weight": None,
                "size": None,
                "source_file": None,
                "reason": NEW_REASON,
            }
        )
    return sites


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--check-sync", action="store_true", help="compare call sites with the manifest (default)")
    parser.add_argument("--sync", action="store_true", help="rewrite icon_manifest.json from the call sites")
    parser.add_argument("--dry-run", action="store_true", help="with --sync, print the summary but do not write")
    args = parser.parse_args(argv)
    if args.dry_run and not args.sync:
        parser.error("--dry-run requires --sync")

    manifest = json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))
    source_sites, unknown = scan_call_sites(app_icon_assets())
    pairs, new, gone = compare(manifest["sites"], source_sites)
    moved = [(msite, ssite) for msite, ssite in pairs if msite["line"] != ssite["line"]]

    errors: list[str] = list(unknown)
    reports: list[str] = []
    if args.sync:
        sites = sync_sites(manifest["sites"], pairs, new, gone)
        effective = dict(manifest)
        effective["sites"] = sites
        for src in new:
            reports.append(f"NEW {src['file']}:{src['line']} {src['rust_icon']}")
        for site in gone:
            reports.append(f"GONE {site['file']}:{site['line']} {site['rust_icon']}")
    else:
        effective = manifest
        for src in new:
            errors.append(f"NEW call site {src['file']}:{src['line']} {src['rust_icon']}")
        for site in gone:
            errors.append(f"GONE call site {site['file']}:{site['line']} {site['rust_icon']}")
    for msite, ssite in moved:
        reports.append(f"MOVED {msite['file']}:{msite['rust_icon']} {msite['line']} -> {ssite['line']}")

    checked: set[str] = set()
    for site in effective["sites"]:
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
    for report in reports:
        print(report)
    if args.sync:
        if args.dry_run:
            print(f"dry run: not writing {MANIFEST_PATH.relative_to(ROOT).as_posix()}")
        else:
            text = json.dumps(effective, indent=1, ensure_ascii=False) + "\n"
            with open(MANIFEST_PATH, "w", encoding="utf-8", newline="\n") as handle:
                handle.write(text)
            print(f"wrote {MANIFEST_PATH.relative_to(ROOT).as_posix()} ({len(effective['sites'])} sites)")
    unverified = sum(1 for site in effective["sites"] if site.get("status") == "unverified")
    print(
        f"{len(source_sites)} sites, {len(checked)} assets checked, {len(errors)} errors, "
        f"new={len(new)} gone={len(gone)} moved={len(moved)} unverified={unverified}"
    )
    return 1 if errors else 0


if __name__ == "__main__":
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")
    sys.exit(main())
