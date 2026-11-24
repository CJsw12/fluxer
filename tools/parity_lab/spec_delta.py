"""Diffs the official-app spec (spec_official.mjs) against the Rust probe's declared style and rects.

    python -I tools/parity_lab/spec_delta.py <spec.json> [--rust <inspect.json> | --live]
        [--scope <flx-prefix>]... [--out-md f] [--out-json f] [--max N]
    python -I tools/parity_lab/spec_delta.py --scenes <scenes.json> --spec-dir <dir> --live [--out-dir <dir>]
        (opened states: per scene drives the Rust client via rust_probe, diffs against
         <spec-dir>/<slug>.spec.json from `spec_official.mjs --scenes`, writes <slug>.md/.json + summary.md)

Join key is (flx, occurrence index). A property is compared only when BOTH sides declare it:
official = a CSS-module/inline rule sets it (rect is always a box metric); Rust = key present in
the probe's style. Exit code 1 when the delta count exceeds --max (default: never fails).
`spec_delta(spec, rust_elements, scope=None)` is importable (replay.py).
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

REM = 16.0
SIDES = ("top", "right", "bottom", "left")
CORNERS = {
    "top_left": "borderTopLeftRadius",
    "top_right": "borderTopRightRadius",
    "bottom_right": "borderBottomRightRadius",
    "bottom_left": "borderBottomLeftRadius",
}
COLOR_TOL = 6
RECT_TOL = 1.0


# ---------------------------------------------------------------- parsers

def parse_len(v) -> float | None:
    """'12px' / '1.5rem' / '0' / 12 / {'Definite': ..} -> px, None when not a plain length."""
    if v is None or isinstance(v, bool):
        return None
    if isinstance(v, (int, float)):
        return float(v)
    if isinstance(v, dict):
        for k in ("Definite", "Pixels", "Px", "px"):
            if k in v:
                return parse_len(v[k])
        if "Rems" in v or "rem" in v:
            n = parse_len(v.get("Rems", v.get("rem")))
            return None if n is None else n * REM
        return None
    m = re.fullmatch(r"\s*(-?\d*\.?\d+)\s*(px|rem|em)?\s*", str(v))
    if not m:
        return None
    n = float(m.group(1))
    return n * REM if m.group(2) in ("rem", "em") else n


def _chan(tok: str, scale: float = 1.0) -> float:
    tok = tok.strip()
    if tok.endswith("%"):
        return float(tok[:-1]) / 100.0 * scale
    return float(tok)


def parse_color(v) -> tuple[float, float, float, float] | None:
    """-> (r, g, b, a) with all channels 0..255 (alpha scaled), None when unparseable."""
    if not isinstance(v, str):
        return None
    s = v.strip().lower()
    if s == "transparent":
        return (0.0, 0.0, 0.0, 0.0)
    if s.startswith("#"):
        h = s[1:]
        if len(h) in (3, 4):
            h = "".join(c * 2 for c in h)
        if len(h) == 6:
            h += "ff"
        if len(h) != 8 or not re.fullmatch(r"[0-9a-f]{8}", h):
            return None
        return tuple(float(int(h[i : i + 2], 16)) for i in (0, 2, 4, 6))  # type: ignore[return-value]
    m = re.fullmatch(r"rgba?\((.*)\)", s)
    if m:
        parts = [p for p in re.split(r"[\s,/]+", m.group(1).strip()) if p]
        if len(parts) < 3:
            return None
        try:
            r, g, b = (_chan(p, 255.0) for p in parts[:3])
            a = _chan(parts[3], 1.0) * 255.0 if len(parts) > 3 else 255.0
            if len(parts) > 3 and not parts[3].endswith("%"):
                a = float(parts[3]) * 255.0
        except ValueError:
            return None
        return (r, g, b, a)
    m = re.fullmatch(r"color\(srgb\s+(.*)\)", s)
    if m:
        parts = [p for p in re.split(r"[\s/]+", m.group(1).strip()) if p]
        if len(parts) < 3:
            return None
        try:
            r, g, b = (float(p) * 255.0 for p in parts[:3])
            a = float(parts[3]) * 255.0 if len(parts) > 3 else 255.0
        except ValueError:
            return None
        return (r, g, b, a)
    return None


def colors_differ(a, b) -> bool:
    if a is None or b is None:
        return False
    if a[3] <= COLOR_TOL and b[3] <= COLOR_TOL:
        return False
    return any(abs(x - y) > COLOR_TOL for x, y in zip(a, b))


def fmt_color(c) -> str:
    return "#%02x%02x%02x%02x" % tuple(int(round(max(0.0, min(255.0, x)))) for x in c)


def rust_solid(bg) -> str | None:
    """Finds the solid colour inside the verbose gpui Background struct."""
    if isinstance(bg, dict):
        for v in bg.values():
            if isinstance(v, dict) and v.get("tag") == "Solid":
                return v.get("solid")
        if bg.get("tag") == "Solid":
            return bg.get("solid")
    return None


def num(x: float) -> str:
    return str(int(x)) if abs(x - round(x)) < 0.005 else f"{x:.2f}".rstrip("0").rstrip(".")


# ---------------------------------------------------------------- declared-ness

def declared_props(el: dict) -> set[str]:
    """Normalised computed-style names an element's CSS-module/inline rules set (state rules excluded)."""
    out: set[str] = set()
    for rule in el.get("css_rules") or []:
        if rule.get("state"):
            continue
        for name in rule.get("props", {}):
            n = name.lower()
            if n == "padding":
                out |= {f"padding-{s}" for s in SIDES}
            elif n in ("padding-block", "padding-inline"):
                out |= {f"padding-{s}" for s in (("top", "bottom") if "block" in n else ("left", "right"))}
            elif n in ("gap", "row-gap", "column-gap"):
                out |= {"row-gap", "column-gap"} if n == "gap" else {n}
            elif n == "border-radius":
                out |= {f"radius-{c}" for c in CORNERS}
            elif m := re.fullmatch(r"border-(top|bottom)-(left|right)-radius", n):
                out.add(f"radius-{m.group(1)}_{m.group(2)}")
            elif n in ("border", "border-width", "border-color", "border-style"):
                out |= {f"bw-{s}" for s in SIDES} | {f"bc-{s}" for s in SIDES}
            elif m := re.fullmatch(r"border-(top|right|bottom|left)(-(width|color|style))?", n):
                out |= {f"bw-{m.group(1)}", f"bc-{m.group(1)}"}
            elif n in ("background", "background-color"):
                out.add("background-color")
            elif n == "font":
                out |= {"font-size", "font-weight", "line-height"}
            else:
                out.add(n)
    return out


def rule_label(el: dict, *names: str) -> str:
    """'[File.module.css .class]' for the first rule that sets any of the CSS names, else '[computed]'."""
    for rule in el.get("css_rules") or []:
        if rule.get("state"):
            continue
        props = {k.lower() for k in rule.get("props", {})}
        if any(n in props or any(p.startswith(n + "-") for p in props if n in ("border", "padding", "gap", "background", "font")) for n in names):
            cls = f" .{rule['class']}" if rule.get("class") else ""
            return f"[{Path(rule['file']).name}{cls}]"
    return "[computed]"


# ---------------------------------------------------------------- core

def _rust_src(src: str | None) -> str:
    return "fluxer_client/" + src if src else "(no src)"


def _find_rust_parent(parent_flx: str | None, rect, by_flx):
    for cand in by_flx.get(parent_flx or "", []):
        x, y, w, h = cand["rect"]
        if x - 1 <= rect[0] and y - 1 <= rect[1] and x + w + 1 >= rect[0] + rect[2] and y + h + 1 >= rect[1] + rect[3]:
            return cand
    return None


def spec_delta(spec: dict, rust_elements: list[dict], scope: list[str] | None = None) -> dict:
    scope = [s for s in (scope or []) if s]
    in_scope = (lambda f: any(f.startswith(p) for p in scope)) if scope else (lambda f: True)

    rust_by_flx: dict[str, list[dict]] = {}
    for e in rust_elements:
        rust_by_flx.setdefault(e["flx"], []).append(e)

    deltas: list[dict] = []
    joined = 0
    unknown = {"floating": 0, "no_style": 0}
    missing: dict[str, int] = {}

    for el in spec.get("elements", []):
        flx, idx = el["flx"], el["idx"]
        if not in_scope(flx):
            continue
        cands = rust_by_flx.get(flx, [])
        if idx >= len(cands):
            if el.get("in_viewport", True) and el["rect"][2] > 0 and el["rect"][3] > 0:
                missing[flx] = missing.get(flx, 0) + 1
            continue
        rs = cands[idx]
        joined += 1
        style = rs.get("style")
        decl = declared_props(el)
        cs = el.get("computed", {})
        src = _rust_src(rs.get("src"))

        def add(prop, off, rust, *css_names):
            deltas.append(
                {
                    "flx": flx,
                    "idx": idx,
                    "prop": prop,
                    "official": off,
                    "rust": rust,
                    "rule": rule_label(el, *css_names) if css_names else "[box]",
                    "src": src,
                }
            )

        # rect (box metric, always declared)
        orect, rrect = el["rect"], rs.get("rect")
        if rrect:
            for name, i in (("width", 2), ("height", 3)):
                if abs(orect[i] - rrect[i]) > RECT_TOL:
                    add(name, num(orect[i]), num(rrect[i]), name)
            par = _find_rust_parent(el.get("parent_flx"), rrect, rust_by_flx)
            if par is not None and el.get("rel"):
                rel = (rrect[0] - par["rect"][0], rrect[1] - par["rect"][1])
                for name, i in (("x-in-parent", 0), ("y-in-parent", 1)):
                    if abs(el["rel"][i] - rel[i]) > RECT_TOL:
                        add(name, num(el["rel"][i]), num(rel[i]))
            else:
                for name, i in (("x", 0), ("y", 1)):
                    if abs(orect[i] - rrect[i]) > RECT_TOL:
                        add(name, num(orect[i]), num(rrect[i]))

        # floating
        if style is None:
            unknown["no_style"] += 1
            unknown["floating"] += 1
        else:
            r_float = str(style.get("position", "")).lower() == "absolute"
            if bool(el.get("floating")) != r_float:
                add("floating", str(bool(el.get("floating"))).lower(), str(r_float).lower(), "position")
        if style is None:
            continue

        # padding
        pad = style.get("padding") or {}
        for s in SIDES:
            r = parse_len(pad.get(s))
            o = parse_len(cs.get(f"padding{s.capitalize()}"))
            if r is not None and o is not None and f"padding-{s}" in decl and abs(r - o) > 0.5:
                add(f"padding-{s}", num(o), num(r), "padding")

        # gap
        gap = style.get("gap") or {}
        for key, css in (("height", "rowGap"), ("width", "columnGap")):
            r = parse_len(gap.get(key))
            o = parse_len(cs.get(css))
            name = "row-gap" if key == "height" else "column-gap"
            if r is not None and o is not None and name in decl and abs(r - o) > 0.5:
                add("gap-" + ("y" if key == "height" else "x"), num(o), num(r), "gap")

        # radii
        radii = style.get("corner_radii") or {}
        # A radius at or above half the shorter side renders identically (pill/circle), so
        # clamp both sides to that cap before comparing (official 9999px vs Rust 16px on 32px).
        rect_o = el.get("rect") or [0, 0, 0, 0]
        cap = min(rect_o[2], rect_o[3]) / 2 if len(rect_o) == 4 and min(rect_o[2], rect_o[3]) > 0 else None
        for key, css in CORNERS.items():
            r = parse_len(radii.get(key))
            o = parse_len(cs.get(css))
            if cap is not None and r is not None and o is not None:
                r, o = min(r, cap), min(o, cap)
            if r is not None and o is not None and f"radius-{key}" in decl and abs(r - o) > 0.5:
                add("radius-" + key.replace("_", "-"), num(o), num(r), "border-radius")

        # border widths and colour
        bws = style.get("border_widths") or {}
        rb_color = parse_color(style.get("border_color")) if isinstance(style.get("border_color"), str) else None
        for s in SIDES:
            r = parse_len(bws.get(s))
            o = parse_len(cs.get(f"border{s.capitalize()}Width"))
            if r is not None and o is not None and f"bw-{s}" in decl and abs(r - o) > 0.5:
                add(f"border-{s}", num(o), num(r), "border")
            if r and o and f"bc-{s}" in decl and rb_color is not None:
                oc = parse_color(cs.get(f"border{s.capitalize()}Color"))
                if colors_differ(oc, rb_color):
                    add(f"border-{s}-color", fmt_color(oc), fmt_color(rb_color), "border")

        # background
        if "background-color" in decl and style.get("background") is not None:
            rc = parse_color(rust_solid(style["background"]))
            oc = parse_color(cs.get("backgroundColor"))
            if colors_differ(oc, rc):
                add("background", fmt_color(oc), fmt_color(rc), "background")

        # text
        text = style.get("text") or {}
        if "color" in decl and text.get("color"):
            rc, oc = parse_color(text["color"]), parse_color(cs.get("color"))
            if colors_differ(oc, rc):
                add("color", fmt_color(oc), fmt_color(rc), "color")
        r = parse_len(text.get("font_size"))
        o = parse_len(cs.get("fontSize"))
        if r is not None and o is not None and "font-size" in decl and abs(r - o) > 0.5:
            add("font-size", num(o), num(r), "font-size", "font")
        if text.get("font_weight") is not None and "font-weight" in decl:
            try:
                r, o = float(text["font_weight"]), float(cs.get("fontWeight"))
                if abs(r - o) > 0.5:
                    add("font-weight", num(o), num(r), "font-weight", "font")
            except (TypeError, ValueError):
                pass
        r = parse_len(text.get("line_height"))
        o = parse_len(cs.get("lineHeight"))
        if r is not None and o is not None and "line-height" in decl and abs(r - o) > 0.5:
            add("line-height", num(o), num(r), "line-height", "font")

    by_prop: dict[str, int] = {}
    for d in deltas:
        by_prop[d["prop"]] = by_prop.get(d["prop"], 0) + 1
    official_flx = {e["flx"] for e in spec.get("elements", []) if in_scope(e["flx"])}
    rust_only = sorted(f for f in rust_by_flx if in_scope(f) and f not in official_flx)
    return {
        "joined": joined,
        "delta_count": len(deltas),
        "by_property": dict(sorted(by_prop.items(), key=lambda kv: -kv[1])),
        "unknown": unknown,
        "missing_in_rust": dict(sorted(missing.items())),
        "rust_only": rust_only,
        "deltas": deltas,
    }


# ---------------------------------------------------------------- output

def delta_line(d: dict) -> str:
    return f"{d['flx']}  {d['prop']} {d['official']}≠{d['rust']}  {d['rule']}  {d['src']}"


def render_md(res: dict, spec: dict) -> str:
    out = [
        "# Spec delta (official vs Rust probe)",
        "",
        f"- official: {spec.get('client_info')} | theme: {spec.get('theme_class')} | viewport: {spec.get('viewport')}",
        f"- joined {res['joined']} | deltas {res['delta_count']} | missing in Rust {len(res['missing_in_rust'])} flx | "
        f"unknown (style missing) {res['unknown']['no_style']} | rust-only {len(res['rust_only'])}",
        f"- by property: {res['by_property']}",
        "",
    ]
    groups: dict[str, list[dict]] = {}
    for d in res["deltas"]:
        groups.setdefault(d["src"].rsplit(":", 1)[0], []).append(d)
    for src, items in sorted(groups.items(), key=lambda kv: -len(kv[1])):
        out += [f"## {src} ({len(items)})", ""]
        out += [f"- {delta_line(d)}" for d in items]
        out.append("")
    out += ["## Missing in Rust (coverage)", ""]
    out += [f"- {f} x{n}" for f, n in res["missing_in_rust"].items()] or ["- none"]
    return "\n".join(out) + "\n"


def _load_rust(args: list[str]) -> list[dict]:
    if "--live" in args:
        sys.path.insert(0, str(Path(__file__).resolve().parent))
        import rust_probe as rp  # noqa: PLC0415

        data = rp.send("probe_inspect")
    else:
        data = json.loads(Path(args[args.index("--rust") + 1]).read_text(encoding="utf-8"))
    return data["elements"] if isinstance(data, dict) else data


BASE_LINK = "fluxer://channels/1572284491404148736/1572284491404148743"


def _flx_set(elements: list[dict]) -> set[str]:
    return {e["flx"] for e in elements}


def run_scenes(argv: list[str]) -> int:
    """Drives the Rust client through each scene's `rust` actions and diffs the open state."""
    import time  # noqa: PLC0415

    sys.path.insert(0, str(Path(__file__).resolve().parent))
    import rust_probe as rp  # noqa: PLC0415

    opt = lambda k: argv[argv.index(k) + 1] if k in argv else None  # noqa: E731
    scenes = json.loads(Path(opt("--scenes")).read_text(encoding="utf-8"))
    spec_dir = Path(opt("--spec-dir"))
    out_dir = Path(opt("--out-dir") or spec_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    def inspect() -> list[dict]:
        data = rp.send("probe_inspect")
        return data["elements"] if isinstance(data, dict) else data

    def reset() -> None:
        rp.deep_link(BASE_LINK)
        time.sleep(0.6)
        rp.send("probe_key", keystroke="escape")
        time.sleep(0.4)

    rows = []
    for sc in scenes:
        slug, expect = sc["slug"], sc.get("expect_flx")
        row = {"scene": slug, "joined": 0, "deltas": 0, "missing": 0, "status": "ok", "error": None, "top": []}
        spec_file = spec_dir / f"{slug}.spec.json"
        try:
            if not spec_file.exists():
                raise RuntimeError("no official spec (scene failed or not captured)")
            spec = json.loads(spec_file.read_text(encoding="utf-8"))
            reset()
            for act in sc.get("rust", []):
                if "click" in act:
                    r = rp.send("probe_click", flx=act["click"], **({"index": act["index"]} if "index" in act else {}))
                    if not r.get("ok", False):
                        raise RuntimeError(f"rust click failed: {act['click']}: {r}")
                elif "key" in act:
                    rp.send("probe_key", keystroke=act["key"])
                elif "link" in act:
                    rp.deep_link(act["link"])
                elif "wait_ms" in act:
                    time.sleep(act["wait_ms"] / 1000)
                else:
                    raise RuntimeError(f"unknown rust action {act}")
                time.sleep(0.3)
            elements: list[dict] = []
            for _ in range(20):
                elements = inspect()
                if not expect or expect in _flx_set(elements):
                    break
                time.sleep(0.25)
            else:
                raise RuntimeError(f"expect_flx not shown in Rust: {expect}")
            res = spec_delta(spec, elements, sc.get("scope"))
            row.update(joined=res["joined"], deltas=res["delta_count"], missing=len(res["missing_in_rust"]))
            row["top"] = [delta_line(d) for d in res["deltas"][:3]]
            (out_dir / f"{slug}.md").write_text(render_md(res, spec), encoding="utf-8")
            (out_dir / f"{slug}.json").write_text(json.dumps(res, ensure_ascii=False, indent=1), encoding="utf-8")
        except Exception as e:  # noqa: BLE001 - one broken scene must not stop the run
            row.update(status="failed", error=str(e))
        finally:
            try:
                rp.send("probe_key", keystroke="escape")
                time.sleep(0.3)
                # Escape does not close every Rust layer (user-area popout): toggle the trigger instead.
                first = next((a for a in sc.get("rust", []) if "click" in a), None)
                if expect and first and expect in _flx_set(inspect()):
                    row["escape_closes"] = False
                    rp.send("probe_click", flx=first["click"], **({"index": first["index"]} if "index" in first else {}))
                    time.sleep(0.4)
            except Exception:  # noqa: BLE001
                pass
        rows.append(row)
        print(f"{row['status']:6} {slug}: joined={row['joined']} deltas={row['deltas']} missing={row['missing']} {row['error'] or ''}")
    try:
        reset()
    except Exception:  # noqa: BLE001
        pass
    lines = ["# Scene spec delta summary", "", "| scene | joined | deltas | missing-in-Rust | status |", "|---|---|---|---|---|"]
    for r in rows:
        lines.append(f"| {r['scene']} | {r['joined']} | {r['deltas']} | {r['missing']} | {r['status']}{': ' + r['error'] if r['error'] else ''} |")
    (out_dir / "summary.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
    (out_dir / "summary.json").write_text(json.dumps(rows, ensure_ascii=False, indent=1), encoding="utf-8")
    return 0 if all(r["status"] == "ok" for r in rows) else 1


def main(argv: list[str]) -> int:
    sys.stdout.reconfigure(encoding="utf-8")
    if "--scenes" in argv and "--live" in argv and "--spec-dir" in argv:
        return run_scenes(argv)
    if not argv or argv[0].startswith("--") or ("--live" not in argv and "--rust" not in argv):
        print(__doc__)
        return 2
    spec = json.loads(Path(argv[0]).read_text(encoding="utf-8"))
    scope = [argv[i + 1] for i, a in enumerate(argv) if a == "--scope" and i + 1 < len(argv)]
    opt = lambda k: argv[argv.index(k) + 1] if k in argv else None  # noqa: E731
    res = spec_delta(spec, _load_rust(argv), scope)
    if opt("--out-md"):
        Path(opt("--out-md")).write_text(render_md(res, spec), encoding="utf-8")
    if opt("--out-json"):
        Path(opt("--out-json")).write_text(json.dumps(res, ensure_ascii=False, indent=1), encoding="utf-8")
    print(
        f"joined={res['joined']} deltas={res['delta_count']} by_property={res['by_property']} "
        f"missing_in_rust={len(res['missing_in_rust'])} unknown={res['unknown']} rust_only={len(res['rust_only'])}"
    )
    for d in res["deltas"][:40]:
        print(delta_line(d))
    mx = opt("--max")
    return 1 if mx is not None and res["delta_count"] > int(mx) else 0


def selftest() -> None:
    assert parse_len("12px") == 12 and parse_len("1.5rem") == 24 and parse_len({"Definite": 4.0}) == 4
    assert parse_len("100%") is None and parse_len(None) is None and parse_len("0") == 0
    assert parse_color("#7a738c33") == (122, 115, 140, 51)
    assert parse_color("#fff") == (255, 255, 255, 255)
    assert parse_color("rgb(244, 244, 246)") == (244, 244, 246, 255)
    assert parse_color("rgba(0, 0, 0, 0.5)") == (0, 0, 0, 127.5)
    assert parse_color("color(srgb 1 0 0 / 0.5)") == (255, 0, 0, 127.5)
    assert parse_color("nope") is None
    assert not colors_differ((10, 10, 10, 255), (16, 10, 10, 255)) and colors_differ((10, 10, 10, 255), (17, 10, 10, 255))
    assert not colors_differ((1, 2, 3, 0), (9, 9, 9, 0))
    assert rust_solid({"Color": {"tag": "Solid", "solid": "#112233ff"}}) == "#112233ff"
    el = {"flx": "a", "idx": 0, "rect": [0, 0, 10, 56], "computed": {"paddingLeft": "16px", "paddingTop": "0px"},
          "css_rules": [{"file": "x/A.module.css", "class": "c", "state": None, "props": {"padding": "0 1rem"}}]}
    r = spec_delta({"elements": [el]}, [{"flx": "a", "rect": [0, 0, 10, 54], "src": "app/s.rs:1",
                                          "style": {"padding": {"left": "1rem", "top": "4px"}}}])
    props = [d["prop"] for d in r["deltas"]]
    assert props == ["height", "padding-top"], props
    assert "A.module.css .c" in r["deltas"][1]["rule"]
    print("selftest ok")


if __name__ == "__main__":
    if "--selftest" in sys.argv:
        selftest()
    else:
        raise SystemExit(main(sys.argv[1:]))
