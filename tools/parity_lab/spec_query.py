"""Read-only inspector for parity spec and spec_delta JSON.

Usage (from the repo root, with ``python -I``):
  python -I tools/parity_lab/spec_query.py baseline [SCENE_SUBSTR]
  python -I tools/parity_lab/spec_query.py spec <spec.json> [--flx RE] [--text RE]
      [--fields font-size,color,...] [--limit 40]
  python -I tools/parity_lab/spec_query.py delta <delta.json|dir> [--scene SLUG]
      [--flx RE] [--limit 40]
  python -I tools/parity_lab/spec_query.py --selftest

``baseline`` prints spec_baseline.json maxima, one line per scene (``markdown``
for the current-screen entry, ``<set>/<slug>`` for opened-state sets), sorted by
deltas. ``spec`` lists official spec elements with rect, text and computed
styles; filters are case-insensitive regexes. ``delta`` summarises a spec_delta
JSON output (deltas, by_property, missing_in_rust, rust_only) and lists row
values with source locations. A directory picks ``<slug>.delta.json``,
``<slug>.json`` (with ``--scene``), ``*delta*.json`` or any delta-shaped JSON.

Official specs currently record no textContent, so the text column is '-' unless
a spec adds ``text``/``textContent``/``innerText``.
"""

import argparse
import contextlib
import io
import json
import re
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
DEFAULT_FIELDS = ["font-size", "font-weight", "line-height", "color", "background-color",
                  "padding", "margin", "gap", "border-radius", "border-width"]
SIMPLE_STYLES = {"font-size": "fontSize", "font-weight": "fontWeight",
                 "line-height": "lineHeight", "color": "color",
                 "background-color": "backgroundColor"}
SIDE_STYLES = {
    "padding": ["paddingTop", "paddingRight", "paddingBottom", "paddingLeft"],
    "margin": ["marginTop", "marginRight", "marginBottom", "marginLeft"],
    "border-width": ["borderTopWidth", "borderRightWidth", "borderBottomWidth", "borderLeftWidth"],
    "border-radius": ["borderTopLeftRadius", "borderTopRightRadius",
                      "borderBottomRightRadius", "borderBottomLeftRadius"],
    "gap": ["rowGap", "columnGap"],
}
def die(msg: str):
    print(f"spec_query: {msg}", file=sys.stderr)
    raise SystemExit(2)
def load_json(path: Path):
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        die(f"not found: {path}")
    except (OSError, json.JSONDecodeError) as exc:
        die(f"cannot read {path}: {exc}")
def rel(path: Path) -> str:
    try:
        return path.resolve().relative_to(Path.cwd()).as_posix()
    except ValueError:
        return path.as_posix()
def as_int(value, default: int = 0) -> int:
    try:
        return int(value)
    except (TypeError, ValueError):
        return default
def round_num(value) -> str:
    try:
        num = float(value)
    except (TypeError, ValueError):
        return str(value)
    if abs(num - round(num)) < 1e-6:
        return str(int(round(num)))
    return f"{num:.1f}".rstrip("0").rstrip(".")
def rect_str(el: dict) -> str:
    rect = el.get("rect")
    if not isinstance(rect, (list, tuple)) or len(rect) < 4:
        return "-"
    return ",".join(round_num(v) for v in rect[:4])
def clip(text, limit: int = 40) -> str:
    text = " ".join(str(text or "").split())
    return text if len(text) <= limit else text[: limit - 1] + "…"
def element_text(el: dict) -> str:
    for key in ("text", "textContent", "innerText"):
        if el.get(key):
            return str(el[key])
    return ""
def style_value(el: dict, field: str) -> str:
    computed = el.get("computed") or {}
    if field in SIDE_STYLES:
        values = [computed.get(key) for key in SIDE_STYLES[field]]
        if all(value is None for value in values):
            return "-"
        if len({str(v) for v in values}) == 1:
            return str(values[0])
        return ",".join("-" if v is None else str(v) for v in values)
    key = SIMPLE_STYLES.get(field, field)
    value = computed.get(key, el.get(key))
    return "-" if value is None or value == "" else str(value)
def compile_re(pattern, flag: str):
    if not pattern:
        return None
    try:
        return re.compile(pattern, re.IGNORECASE)
    except re.error as exc:
        die(f"invalid {flag} regex {pattern!r}: {exc}")
def baseline_rows(data: dict):
    rows = [(str(name), as_int(scene.get("max_deltas")), as_int(scene.get("max_missing")))
            for name, scene in (data.get("scenes") or {}).items()]
    for set_name, cfg in (data.get("scene_sets") or {}).items():
        for slug, maximum in (cfg.get("max") or {}).items():
            maximum = maximum or {}
            rows.append((f"{set_name}/{slug}", as_int(maximum.get("deltas")),
                         as_int(maximum.get("missing"))))
    rows.sort(key=lambda row: (-row[1], row[0]))
    return rows
def cmd_baseline(filter_text: str = "") -> int:
    needle = filter_text.lower()
    rows = [row for row in baseline_rows(load_json(HERE / "spec_baseline.json"))
            if needle in row[0].lower()]
    for label, deltas, missing in rows:
        print(f"{label}  deltas={deltas} missing={missing}")
    if not rows:
        print("no scenes matched")
    return len(rows)
def spec_lines(spec_path: Path, flx_re, text_re, fields, limit: int):
    data = load_json(spec_path)
    elements = data.get("elements") or []
    matches = [(el, str(el.get("flx") or ""), element_text(el)) for el in elements]
    matches = [m for m in matches
               if (not flx_re or flx_re.search(m[1])) and (not text_re or text_re.search(m[2]))]
    shown = matches[: max(0, limit)]
    lines = [f"{rel(spec_path)}: elements={len(elements)} matches={len(matches)} "
             f"shown={len(shown)}"]
    for el, flx, text in shown:
        parts = [flx] + ([f"idx={el['idx']}"] if el.get("idx") else [])
        parts += [f"rect={rect_str(el)}", f"text={clip(text) or '-'}"]
        parts += [f"{field}={style_value(el, field)}" for field in fields]
        lines.append(" ".join(parts))
    if not matches:
        lines.append("no elements matched")
    return lines, len(matches)
def cmd_spec(args) -> int:
    fields = DEFAULT_FIELDS
    if args.fields:
        fields = [part.strip() for part in args.fields.split(",") if part.strip()]
        if not fields:
            die("--fields needs at least one field name")
    lines, count = spec_lines(Path(args.json), compile_re(args.flx, "--flx"),
                              compile_re(args.text, "--text"), fields, args.limit)
    print("\n".join(lines))
    return count
def is_delta(data) -> bool:
    return isinstance(data, dict) and isinstance(data.get("deltas"), list)
def delta_files(path: Path, scene: str | None) -> list[Path]:
    if path.is_file():
        return [path]
    if not path.is_dir():
        die(f"not a file or directory: {path}")
    candidates: list[Path] = []
    if scene:
        candidates = [path / name for name in (f"{scene}.delta.json", f"{scene}.json")
                      if (path / name).is_file()]
        if not candidates:
            candidates = sorted(path.rglob(f"*{scene}*delta*.json"))
    if not candidates:
        candidates = sorted(path.rglob("*delta*.json")) or sorted(path.glob("*.json"))
    usable = []
    for candidate in candidates:
        try:
            data = json.loads(candidate.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            continue
        if is_delta(data):
            usable.append(candidate)
    if not usable:
        die(f"no spec_delta JSON found under {path}")
    return usable
def delta_summary(path: Path, data: dict, flx_re, limit: int):
    deltas = data.get("deltas") or []
    missing = data.get("missing_in_rust") or {}
    rust_only = data.get("rust_only") or []
    missing_count = len(missing) if isinstance(missing, dict) else 0
    missing_refs = sum(as_int(v) for v in missing.values()) if isinstance(missing, dict) else 0
    by_prop = data.get("by_property") or {}
    prop_text = " ".join(f"{key}={value}" for key, value in
                         sorted(by_prop.items(), key=lambda kv: (-as_int(kv[1]), kv[0])))
    lines = [f"{rel(path)}: deltas={as_int(data.get('delta_count'), len(deltas))} "
             f"joined={as_int(data.get('joined'))} missing_in_rust={missing_count} "
             f"(refs={missing_refs}) rust_only={len(rust_only)}"]
    if prop_text:
        lines.append(f"by property: {prop_text}")
    unknown = data.get("unknown") or {}
    if isinstance(unknown, dict) and unknown:
        lines.append("unknown: " + " ".join(f"{k}={v}" for k, v in sorted(unknown.items())))
    matched = [row for row in deltas
               if not flx_re or flx_re.search(str(row.get("flx") or ""))]
    shown = matched[: max(0, limit)]
    lines.append(f"rows: matches={len(matched)} shown={len(shown)}")
    for row in shown:
        parts = [str(row.get("flx") or "?")] + ([f"idx={row['idx']}"] if row.get("idx") else [])
        parts += [str(row.get("prop") or "?"),
                  f"official={row.get('official', '-')} rust={row.get('rust', '-')}"]
        parts += [f"{key}={row[key]}" for key in ("rule", "src") if row.get(key)]
        lines.append(" ".join(parts))
    if not matched:
        lines.append("no delta rows matched")
    return lines, len(matched)
def cmd_delta(args) -> int:
    flx_re = compile_re(args.flx, "--flx")
    total = 0
    for index, path in enumerate(delta_files(Path(args.path), args.scene)):
        data = load_json(path)
        if not is_delta(data):
            die(f"not a spec_delta JSON (missing 'deltas'): {path}")
        if index:
            print()
        lines, count = delta_summary(path, data, flx_re, args.limit)
        total += count
        print("\n".join(lines))
    return total
def selftest() -> int:
    with tempfile.TemporaryDirectory(prefix="spec_query_selftest_") as tmp:
        tmp_path = Path(tmp)
        baseline = {"scenes": {"markdown": {"max_deltas": 4, "max_missing": 7}},
                    "scene_sets": {"more": {"max": {
                        "settings-appearance": {"deltas": 0, "missing": 3},
                        "dm-home": {"deltas": 9, "missing": 1}}}}}
        cs = {"fontSize": "16px", "fontWeight": "400", "lineHeight": "24px",
              "color": "rgb(1, 2, 3)", "backgroundColor": "rgb(4, 5, 6)"}
        for side in ("Top", "Right", "Bottom", "Left"):
            cs[f"padding{side}"], cs[f"margin{side}"], cs[f"border{side}Width"] = "8px", "0px", "1px"
        for corner in ("TopLeft", "TopRight", "BottomRight", "BottomLeft"):
            cs[f"border{corner}Radius"] = "6px"
        cs["rowGap"] = cs["columnGap"] = "4px"
        spec = {"elements": [
            {"flx": "sidebar.root", "idx": 0, "rect": [0, 0, 240, 800],
             "text": "Direct messages", "computed": cs},
            {"flx": "sidebar.item", "idx": 2, "rect": [4.5, 8, 100.25, 20],
             "computed": {"color": "red"}}]}
        delta = {"joined": 1, "delta_count": 1, "by_property": {"width": 1},
                 "unknown": {}, "missing_in_rust": {"sidebar.item": 2}, "rust_only": [],
                 "deltas": [{"flx": "sidebar.root", "idx": 0, "prop": "width",
                             "official": "240", "rust": "200", "rule": "[computed]",
                             "src": "src/ui/sidebar.rs:10"}]}
        for name, payload in (("baseline", baseline), ("spec", spec), ("delta", delta)):
            (tmp_path / f"{name}.json").write_text(json.dumps(payload), encoding="utf-8")
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            rows = baseline_rows(baseline)
            for label, deltas, missing in rows:
                print(f"{label}  deltas={deltas} missing={missing}")
            spec_output, spec_count = spec_lines(tmp_path / "spec.json", re.compile("sidebar"),
                                                 None, DEFAULT_FIELDS, 40)
            print("\n".join(spec_output))
            delta_output, delta_count = delta_summary(tmp_path / "delta.json", delta,
                                                      re.compile("sidebar"), 40)
            print("\n".join(delta_output))
        text = out.getvalue()
        assert len(rows) == 3, len(rows)
        assert spec_count == 2, spec_count
        assert delta_count == 1, delta_count
        assert "dm-home  deltas=9 missing=1" in text
        assert "sidebar.root rect=0,0,240,800 text=Direct messages" in text
        assert "padding=8px" in text and "margin=0px" in text
        assert "official=240 rust=200" in text and "src=src/ui/sidebar.rs:10" in text
        print("selftest ok (baseline rows=3, spec matches=2, delta rows=1, temp dir cleaned)")
    return 0
def main(argv=None) -> int:
    try:
        sys.stdout.reconfigure(encoding="utf-8")
    except (AttributeError, ValueError):
        pass
    parser = argparse.ArgumentParser(prog="spec_query.py",
                                     description="Read-only parity spec/delta query tool")
    parser.add_argument("--selftest", action="store_true", help="run synthetic checks")
    sub = parser.add_subparsers(dest="cmd")
    base = sub.add_parser("baseline", help="print spec_baseline.json maxima")
    base.add_argument("scene", nargs="?", default="")
    spec = sub.add_parser("spec", help="list official spec elements")
    spec.add_argument("json"); spec.add_argument("--flx"); spec.add_argument("--text")
    spec.add_argument("--fields"); spec.add_argument("--limit", type=int, default=40)
    delta = sub.add_parser("delta", help="summarise a spec_delta JSON output")
    delta.add_argument("path"); delta.add_argument("--scene"); delta.add_argument("--flx")
    delta.add_argument("--limit", type=int, default=40)
    args = parser.parse_args(argv)
    if args.selftest:
        return selftest()
    handlers = {"baseline": lambda: cmd_baseline(args.scene), "spec": lambda: cmd_spec(args),
                "delta": lambda: cmd_delta(args)}
    if args.cmd in handlers:
        handlers[args.cmd]()
        return 0
    parser.print_help()
    return 1

if __name__ == "__main__":
    raise SystemExit(main())
