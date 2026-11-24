"""Classifies the flx elements the official app has but the Rust client is missing.

    python -I tools/parity_lab/coverage.py <spec.json> <delta.json> [--out-md f] [--out-json f]
    python -I tools/parity_lab/coverage.py --scenes-dir <dir> [--out-md f] [--out-json f]
        (pairs every <slug>.spec.json with <slug>.json in <dir> and prints a summary table)

Input: a spec from spec_official.mjs ({viewport, elements:[{flx, idx, tag, rect, in_viewport,
computed, css_rules, icon, ...}]}) and a delta from spec_delta.py (with 'missing_in_rust': {flx: n}).
`n` is the number of that flx's occurrences the Rust probe did not report; spec_delta counts only
elements that are in the viewport with a non-zero rect, in document order, so the missing ones are
the tail of that flx's eligible occurrences (see missing_elements()).

Each missing element is classified as:
  hidden      rect width/height 0, opacity 0, visibility hidden, or rect fully outside the viewport
  visual      non-transparent background, visible border, direct text, icon/svg, box-shadow,
              clickable, or <img>
  structural  everything else (pure layout wrappers)

Spec fields: computed carries no textContent, role, cursor or visibility (only the style props
listed in spec_official.mjs PROPS). Visual-decision fallbacks for the missing fields are therefore:
  text      spec 'text'/'textContent' when a future spec adds it, else a class token naming a
            text part (text/label/title/placeholder/...) or a text-bearing tag (span/p/h1..h6/
            time/...) that is a leaf in the flx tree or wraps other text-bearing elements;
  cursor    a CSS-module rule declaring `cursor: pointer` (specs do record rule props);
  role      element tag button/a/option/summary or a class token button/btn/tab/link/checkbox/
            radio/switch/toggle;
  img       the element tag.
Every inferred reason is marked in the JSON reason kind and in the markdown reason cell.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

DEFAULT_VIEWPORT = (1280.0, 800.0)

SVG_PAINT_TAGS = {
    "svg", "path", "rect", "circle", "ellipse", "line", "polyline", "polygon", "use", "image",
}
TEXT_TAGS = {
    "span", "p", "h1", "h2", "h3", "h4", "h5", "h6", "time", "strong", "em", "b", "u", "s",
    "code", "pre", "blockquote", "li", "label", "small", "abbr", "mark", "dt", "dd", "legend",
    "caption", "figcaption", "cite", "kbd", "samp", "var", "sub", "sup", "output",
}
TEXT_TOKENS = {
    "text", "label", "title", "heading", "subtitle", "caption", "hint", "placeholder",
    "wordmark", "username", "nickname", "timestamp", "count", "badge", "name",
}
CLICK_TAGS = {"button", "a", "option", "summary"}
CLICK_TOKENS = {
    "button", "btn", "tab", "link", "checkbox", "radio", "switch", "toggle", "clickable",
}
CURSOR_ROLES = ("button", "link", "menuitem", "tab", "checkbox", "radio", "switch", "option")


# ---------------------------------------------------------------- value parsers

def parse_color(v):
    """-> (r, g, b, a) with channels 0..255; None when the value is not a colour."""
    if not isinstance(v, str):
        return None
    s = v.strip().lower()
    if s in ("", "none"):
        return None
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
        return tuple(float(int(h[i : i + 2], 16)) for i in (0, 2, 4, 6))
    m = re.fullmatch(r"rgba?\((.*)\)", s)
    if m:
        parts = [p for p in re.split(r"[\s,/]+", m.group(1).strip()) if p]
        if len(parts) < 3:
            return None
        try:
            chans = []
            for p in parts[:3]:
                chans.append(float(p[:-1]) / 100.0 * 255.0 if p.endswith("%") else float(p))
            if len(parts) > 3:
                a = parts[3]
                alpha = float(a[:-1]) / 100.0 if a.endswith("%") else float(a)
            else:
                alpha = 1.0
        except ValueError:
            return None
        return (chans[0], chans[1], chans[2], alpha * 255.0)
    m = re.fullmatch(r"color\(srgb\s+(.*)\)", s)
    if m:
        parts = [p for p in re.split(r"[\s/]+", m.group(1).strip()) if p]
        if len(parts) < 3:
            return None
        try:
            chans = [float(p) * 255.0 for p in parts[:3]]
            alpha = float(parts[3]) if len(parts) > 3 else 1.0
        except ValueError:
            return None
        return (chans[0], chans[1], chans[2], alpha * 255.0)
    return None


def visible_color(v) -> bool:
    c = parse_color(v)
    return c is not None and c[3] > 0.5


def px_len(v):
    """'12px' / '0' / 12 -> float, None when not a plain length."""
    if v is None or isinstance(v, bool):
        return None
    if isinstance(v, (int, float)):
        return float(v)
    m = re.fullmatch(r"\s*(-?\d*\.?\d+)\s*(px)?\s*", str(v))
    return float(m.group(1)) if m else None


def num(x: float) -> str:
    return str(int(x)) if abs(x - round(x)) < 0.005 else f"{x:.2f}".rstrip("0").rstrip(".")


def rect_str(rect) -> str:
    if not isinstance(rect, list) or len(rect) != 4:
        return "?"
    x, y, w, h = rect
    return f"{num(x)},{num(y)} {num(w)}x{num(h)}"


# ---------------------------------------------------------------- lookups

def tokens(s) -> list[str]:
    """CSS module class -> lowercase word tokens (handles camelCase and kebab-case)."""
    if not isinstance(s, str) or not s:
        return []
    out: list[str] = []
    for part in re.split(r"[^A-Za-z0-9]+", s):
        out += re.findall(r"[A-Z]+(?=[A-Z][a-z])|[A-Z]?[a-z]+|[A-Z]+|\d+", part)
    return [t.lower() for t in out]


def rules_of(el) -> list[dict]:
    return [r for r in (el.get("css_rules") or []) if isinstance(r, dict)]


def base_rules(el) -> list[dict]:
    return [r for r in rules_of(el) if not r.get("state")]


def rule_label(rule: dict | None) -> str:
    if not rule:
        return ""
    state = ":" + rule["state"] if rule.get("state") else ""
    cls = f" .{rule['class']}{state}" if rule.get("class") else ""
    return f"[{Path(rule.get('file') or '?').name}{cls}]"


def rule_for_prop(el, names: tuple[str, ...], prefix: bool = False) -> dict | None:
    for rule in base_rules(el):
        for name in rule.get("props") or {}:
            n = name.lower()
            if n in names or (prefix and any(n.startswith(p) for p in names)):
                return rule
    return None


def module_of(el) -> tuple[str, str | None]:
    """(file, class) of the element's first matched CSS-module rule; inline-only falls back to it."""
    rules = rules_of(el)
    for rule in rules:
        if rule.get("file") and rule["file"] != "(inline)":
            return rule["file"], rule.get("class")
    if rules:
        return rules[0].get("file") or "(no rule)", rules[0].get("class")
    return "(no rule)", None


def component_of(flx: str) -> str:
    parts = [p for p in (flx or "").split(".") if p]
    return ".".join(parts[:2]) if parts else "(none)"


def text_source_token(el):
    """'class:<name>' / 'flx:<seg>' when a class or the flx's last segment names a text part."""
    if el.get("icon") or (el.get("tag") or "").lower() in SVG_PAINT_TAGS:
        return None  # icon elements (and flx names like text-channel-icon) are not text
    for rule in base_rules(el):
        if TEXT_TOKENS & set(tokens(rule.get("class"))):
            return "class:" + rule["class"], "~" + rule["class"]
    seg = (el.get("flx") or "").rsplit(".", 1)[-1]
    if TEXT_TOKENS & set(tokens(seg)):
        return "flx:" + seg, "~" + seg
    return None


def text_hint(el, has_text_below: bool):
    """-> (snippet, source) or None. Uses spec text when present, else tag/class inference.

    The spec records no textContent, so 'direct text' falls back to a class/flx segment that names
    a text part, or a text-bearing tag (span/p/h1..h6/time/...) that is a leaf in the flx tree or
    wraps other text-bearing elements.
    """
    for key in ("text", "textContent", "innerText"):
        if isinstance(el.get(key), str):
            s = " ".join(el[key].split())
            return (s, "spec:" + key) if s else None
    tok = text_source_token(el)
    if tok:
        return tok[1], tok[0]
    tag = (el.get("tag") or "").lower()
    if tag in TEXT_TAGS and has_text_below:
        return (f"~<{tag}>", "tag:" + tag)
    return None


def click_hint(el):
    """-> (label, rule) for clickable signals that exist in the spec, else None."""
    tag = (el.get("tag") or "").lower()
    if tag in CLICK_TAGS:
        return f"tag:{tag}", None
    for rule in base_rules(el):
        if str((rule.get("props") or {}).get("cursor", "")).strip().lower() == "pointer":
            return f"cursor(cursor:pointer)", rule
        hit = CLICK_TOKENS & set(tokens(rule.get("class")))
        if hit:
            return f"class:{rule['class']}", rule
        sel = rule.get("selector") or ""
        m = re.search(r"role\s*=\s*[\"']([a-z-]+)[\"']", sel)
        if m and m.group(1) in CURSOR_ROLES:
            return f"role:{m.group(1)}", rule
    for key in ("role", "cursor"):
        v = el.get(key)
        if key == "role" and isinstance(v, str) and v in CURSOR_ROLES:
            return f"role:{v}", None
        if key == "cursor" and isinstance(v, str) and v == "pointer":
            return "cursor:pointer", None
    return None


# ---------------------------------------------------------------- classification

def hidden_reasons(el, viewport) -> list[dict]:
    out: list[dict] = []
    rect = el.get("rect")
    if isinstance(rect, list) and len(rect) == 4:
        x, y, w, h = (float(v) for v in rect)
        if w <= 0 or h <= 0:
            out.append({"kind": "rect", "detail": f"rect {num(w)}x{num(h)}"})
        vw, vh = viewport
        if x + w <= 0 or y + h <= 0 or x >= vw or y >= vh:
            out.append({"kind": "offscreen", "detail": "outside viewport"})
    op = px_len((el.get("computed") or {}).get("opacity"))
    if op is not None and op <= 0:
        out.append({"kind": "opacity", "detail": "opacity 0"})
    vis = (el.get("computed") or {}).get("visibility")
    if isinstance(vis, str) and vis.lower() in ("hidden", "collapse"):
        out.append({"kind": "visibility", "detail": f"visibility {vis}"})
    return out


def visual_reasons(el, th) -> list[dict]:
    cs = el.get("computed") or {}
    out: list[dict] = []

    tag = (el.get("tag") or "").lower()
    if tag == "img":
        out.append({"kind": "img", "detail": "<img>"})
    if el.get("icon"):
        vb = (el.get("icon") or {}).get("viewBox")
        out.append({"kind": "icon", "detail": f"icon(svg {vb})" if vb else "icon(svg)"})
    elif tag in SVG_PAINT_TAGS:
        out.append({"kind": "svg", "detail": f"<{tag}>"})

    if th:
        out.append({"kind": "text", "detail": "text(" + th[1] + ")", "text": th[0], "text_source": th[1]})

    bg = cs.get("backgroundColor")
    if visible_color(bg):
        rule = rule_for_prop(el, ("background", "background-color"))
        out.append({"kind": "background-color", "detail": f"background-color {bg}", "rule": rule})

    for side in ("Top", "Right", "Bottom", "Left"):
        bw = px_len(cs.get("border" + side + "Width"))
        bc = cs.get("border" + side + "Color")
        if bw and bw > 0 and visible_color(bc):
            rule = rule_for_prop(el, ("border",), prefix=True)
            out.append({"kind": "border", "detail": f"border-{side.lower()} {num(bw)}px {bc}", "rule": rule})

    shadow = cs.get("boxShadow")
    if isinstance(shadow, str) and shadow.strip() and shadow.strip().lower() != "none":
        out.append({"kind": "box-shadow", "detail": "box-shadow " + shadow.strip()[:28],
                    "rule": rule_for_prop(el, ("box-shadow",))})

    ch = click_hint(el)
    if ch:
        out.append({"kind": "clickable", "detail": "clickable(" + ch[0] + ")", "rule": ch[1]})
    return out


def classify(el, viewport, th) -> dict:
    hidden = hidden_reasons(el, viewport)
    if hidden:
        return {"kind": "hidden", "reasons": hidden}
    tag = (el.get("tag") or "").lower()
    visual = visual_reasons(el, th)
    if visual:
        return {"kind": "visual", "reasons": visual}
    return {"kind": "structural", "reasons": [{"kind": "layout", "detail": f"<{tag}> layout wrapper"}]}


def reason_text(reasons: list[dict]) -> str:
    if not reasons:
        return ""
    head = reasons[0]
    s = head["detail"]
    if head.get("rule"):
        s += " " + rule_label(head["rule"])
    if len(reasons) > 1:
        s += f" (+{len(reasons) - 1})"
    return s


def eligible(el) -> bool:
    rect = el.get("rect")
    if not (isinstance(rect, list) and len(rect) == 4):
        return False
    return bool(el.get("in_viewport", True)) and rect[2] > 0 and rect[3] > 0


def missing_elements(spec: dict, missing: dict) -> tuple[list[dict], list[dict]]:
    """Maps {flx: n} to the spec elements. n counts the tail of eligible occurrences (spec_delta
    skips zero-rect/off-viewport elements and matches Rust candidates in document order)."""
    by_flx: dict[str, list[dict]] = {}
    for el in spec.get("elements", []):
        by_flx.setdefault(el.get("flx") or "", []).append(el)
    picked: list[dict] = []
    unmapped: list[dict] = []
    for flx in sorted(missing):
        n = int(missing[flx])
        if n <= 0:
            continue
        els = [e for e in by_flx.get(flx, []) if eligible(e)]
        take = els[-n:] if n <= len(els) else els
        picked += take
        if len(take) < n:
            unmapped.append({"flx": flx, "missing": n, "matched": len(take)})
    return picked, unmapped


def snippet(s: str, limit: int = 40) -> str:
    s = " ".join((s or "").split())
    return s if len(s) <= limit else s[: limit - 3] + "..."


def md_cell(s) -> str:
    return str(s).replace("|", "\\|").replace("\n", " ")


def scene_coverage(spec: dict, delta: dict, slug: str, spec_path: str = "", delta_path: str = "") -> dict:
    viewport = tuple(float(v) for v in (spec.get("viewport") or DEFAULT_VIEWPORT)[:2])
    missing = delta.get("missing_in_rust") or {}
    picked, unmapped = missing_elements(spec, missing)

    parents = {el.get("parent_flx") for el in spec.get("elements", []) if el.get("parent_flx")}
    leaf_flx = {el.get("flx") for el in spec.get("elements", []) if el.get("flx") not in parents}
    children: dict[str, set[str]] = {}
    for el in spec.get("elements", []):
        if el.get("parent_flx") and el.get("flx"):
            children.setdefault(el["parent_flx"], set()).add(el["flx"])

    # Text proxy: a class token naming a text part, or a text-bearing tag that is a leaf.
    source_flx = set()
    for el in spec.get("elements", []):
        tag = (el.get("tag") or "").lower()
        if (tag in TEXT_TAGS and el.get("flx") in leaf_flx) or text_source_token(el):
            source_flx.add(el.get("flx"))
    text_below: dict[str, bool] = {}

    def has_text_below(flx: str, seen: frozenset = frozenset()) -> bool:
        if flx in text_below:
            return text_below[flx]
        if not flx or flx in seen:
            return False
        seen = seen | {flx}
        result = flx in source_flx or any(has_text_below(c, seen) for c in children.get(flx, ()))
        text_below[flx] = result
        return result

    items, counts = [], {"visual": 0, "structural": 0, "hidden": 0}
    by_component: dict[str, dict[str, int]] = {}
    by_module: dict[str, dict[str, int]] = {}
    for el in picked:
        flx = el.get("flx") or ""
        th = text_hint(el, flx in source_flx or any(has_text_below(c) for c in children.get(flx, ())))
        c = classify(el, viewport, th)
        counts[c["kind"]] += 1
        mod_file, mod_class = module_of(el)
        item = {
            "flx": el.get("flx"),
            "idx": el.get("idx"),
            "tag": el.get("tag"),
            "rect": el.get("rect"),
            "kind": c["kind"],
            "component": component_of(el.get("flx") or ""),
            "module": mod_file,
            "class": mod_class,
            "reason": reason_text(c["reasons"]),
            "reasons": c["reasons"],
            "text": th[0] if th else "",
            "text_source": th[1] if th else None,
        }
        items.append(item)
        for bucket, key in ((by_component, item["component"]), (by_module, mod_file)):
            row = bucket.setdefault(key, {"visual": 0, "structural": 0, "hidden": 0})
            row[c["kind"]] += 1

    visual = [it for it in items if it["kind"] == "visual"]
    visual_by_flx: dict[str, int] = {}
    for it in visual:
        visual_by_flx[it["flx"]] = visual_by_flx.get(it["flx"], 0) + 1
    top_visual = sorted(visual_by_flx.items(), key=lambda kv: (-kv[1], kv[0]))[:10]

    return {
        "scene": slug,
        "spec": spec_path,
        "delta": delta_path,
        "viewport": [int(v) if float(v).is_integer() else v for v in viewport],
        "missing_flx": len(missing),
        "missing_elements": len(picked),
        "counts": counts,
        "by_component": dict(sorted(by_component.items(), key=lambda kv: (-sum(kv[1].values()), kv[0]))),
        "by_module": dict(sorted(by_module.items(), key=lambda kv: (-sum(kv[1].values()), kv[0]))),
        "top_visual_flx": [{"flx": f, "visual": n} for f, n in top_visual],
        "unmapped": unmapped,
        "items": sorted(items, key=lambda it: (it["component"], it["flx"] or "", it["idx"] if it["idx"] is not None else -1)),
    }


# ---------------------------------------------------------------- markdown

SCENE_NOTES = (
    "spec has no text/role/cursor/visibility fields; text, clickability and role were inferred from "
    "element tag, CSS-module class tokens and rule props (reasons show the source)"
)


def render_table(rows: dict[str, dict[str, int]], first_col: str) -> list[str]:
    if not rows:
        return [f"_{first_col}: none_", ""]
    out = [f"| {first_col} | visual | structural | hidden | total |", "|---|---|---|---|---|"]
    for key, c in rows.items():
        out.append(f"| {md_cell(key)} | {c['visual']} | {c['structural']} | {c['hidden']} | {sum(c.values())} |")
    out.append("")
    return out


def render_scene_md(res: dict) -> list[str]:
    c = res["counts"]
    out = [
        f"## {res['scene']}",
        "",
        f"- spec: `{res['spec'] or '?'}` | delta: `{res['delta'] or '?'}` | viewport: {res['viewport']}",
        f"- missing: {res['missing_flx']} flx / {res['missing_elements']} elements | "
        f"visual {c['visual']} / structural {c['structural']} / hidden {c['hidden']}",
        f"- note: {SCENE_NOTES}",
        "",
        "| kind | count |",
        "|---|---|",
        f"| visual | {c['visual']} |",
        f"| structural | {c['structural']} |",
        f"| hidden | {c['hidden']} |",
        "",
    ]
    out.append("### By component")
    out += render_table(res["by_component"], "component")
    out.append("### By CSS module")
    out += render_table(res["by_module"], "module")
    out.append(f"### Visual missing ({len([i for i in res['items'] if i['kind'] == 'visual'])})")
    visual = [it for it in res["items"] if it["kind"] == "visual"]
    if not visual:
        out += ["_none_", ""]
    else:
        out += ["| flx | idx | rect | reason | module / class | text |", "|---|---|---|---|---|---|"]
        for it in visual:
            cls = f"{Path(it['module']).name} .{it['class']}" if it["class"] else Path(it["module"]).name
            out.append(
                f"| {md_cell(it['flx'])} | {it['idx']} | {rect_str(it['rect'])} | {md_cell(it['reason'])} | "
                f"{md_cell(cls)} | {md_cell(snippet(it['text']))} |"
            )
        out.append("")
    if res["unmapped"]:
        out += ["### Unmapped delta entries", ""]
        for u in res["unmapped"]:
            out.append(f"- {u['flx']}: missing {u['missing']}, matched {u['matched']} spec elements")
        out.append("")
    return out


def render_md(results: list[dict]) -> str:
    if len(results) == 1:
        res = results[0]
        out = [f"# Missing in Rust coverage: {res['scene']}", ""]
        return "\n".join(out + render_scene_md(res)) + "\n"
    out = ["# Missing in Rust coverage (scenes)", "",
           f"- note: {SCENE_NOTES}", "",
           "| scene | missing flx | missing elements | visual | structural | hidden |",
           "|---|---|---|---|---|---|"]
    for res in results:
        c = res["counts"]
        out.append(
            f"| {md_cell(res['scene'])} | {res['missing_flx']} | {res['missing_elements']} | "
            f"{c['visual']} | {c['structural']} | {c['hidden']} |"
        )
    out.append("")
    for res in results:
        out += render_scene_md(res)
    return "\n".join(out) + "\n"


# ---------------------------------------------------------------- CLI

def usage(code: int = 2) -> int:
    print(__doc__.strip())
    return code


def take(argv: list[str], name: str) -> str | None:
    return argv[argv.index(name) + 1] if name in argv and argv.index(name) + 1 < len(argv) else None


def load_json(path: str) -> dict:
    data = json.loads(Path(path).read_text(encoding="utf-8"))
    if isinstance(data, list):
        raise ValueError(f"{path}: expected an object, got a list")
    return data


def print_summary(results: list[dict]) -> None:
    print(f"{'scene':36} {'flx':>4} {'elems':>5} {'visual':>6} {'struct':>6} {'hidden':>6}")
    for res in results:
        c = res["counts"]
        print(f"{res['scene']:36} {res['missing_flx']:4} {res['missing_elements']:5} "
              f"{c['visual']:6} {c['structural']:6} {c['hidden']:6}")


def main(argv: list[str]) -> int:
    sys.stdout.reconfigure(encoding="utf-8")
    if not argv or "-h" in argv or "--help" in argv:
        return usage(0 if argv else 2)

    scenes_dir = take(argv, "--scenes-dir")
    out_md = take(argv, "--out-md")
    out_json = take(argv, "--out-json")
    known = {"--scenes-dir", "--out-md", "--out-json"}
    positional, i = [], 0
    while i < len(argv):
        a = argv[i]
        if a in known:
            if i + 1 >= len(argv):
                print(f"error: {a} needs a value", file=sys.stderr)
                return 2
            i += 2
            continue
        if a.startswith("-"):
            print(f"error: unknown option {a}", file=sys.stderr)
            return 2
        positional.append(a)
        i += 1

    try:
        if scenes_dir:
            if positional:
                print("error: --scenes-dir takes no positional arguments", file=sys.stderr)
                return 2
            d = Path(scenes_dir)
            if not d.is_dir():
                print(f"error: not a directory: {d}", file=sys.stderr)
                return 2
            results, skipped = [], []
            for spec_path in sorted(d.glob("*.spec.json")):
                slug = spec_path.name[: -len(".spec.json")]
                delta_path = d / f"{slug}.json"
                if not delta_path.exists():
                    skipped.append(slug)
                    continue
                try:
                    results.append(scene_coverage(load_json(str(spec_path)), load_json(str(delta_path)),
                                                  slug, str(spec_path), str(delta_path)))
                except Exception as e:  # noqa: BLE001 - one broken scene must not stop the run
                    skipped.append(f"{slug} ({e})")
            if skipped:
                print("skipped: " + ", ".join(skipped), file=sys.stderr)
            if not results:
                print(f"error: no <slug>.spec.json + <slug>.json pair in {d}", file=sys.stderr)
                return 1
            print_summary(results)
            payload = {"viewport": results[0]["viewport"], "scenes": results}
            md = render_md(results)
        else:
            if len(positional) != 2:
                return usage()
            spec_path, delta_path = positional
            spec = load_json(spec_path)
            delta = load_json(delta_path)
            if "missing_in_rust" not in delta:
                print(f"error: {delta_path} has no 'missing_in_rust' field", file=sys.stderr)
                return 1
            slug = Path(spec_path).name
            slug = slug[: -len(".spec.json")] if slug.endswith(".spec.json") else Path(spec_path).stem
            res = scene_coverage(spec, delta, slug, spec_path, delta_path)
            results = [res]
            print_summary(results)
            payload = res
            md = render_md(results)

        if out_md:
            Path(out_md).write_text(md, encoding="utf-8")
        if out_json:
            Path(out_json).write_text(json.dumps(payload, ensure_ascii=False, indent=1), encoding="utf-8")
    except (OSError, ValueError) as e:
        print(f"error: {e}", file=sys.stderr)
        return 1
    return 0


def selftest() -> None:
    el = {
        "flx": "a.b.card", "idx": 0, "tag": "div", "rect": [0, 0, 10, 10], "in_viewport": True,
        "computed": {"backgroundColor": "rgba(0, 0, 0, 0)", "opacity": "1", "boxShadow": "none",
                     "borderTopWidth": "1px", "borderTopColor": "rgb(1, 2, 3)"},
        "css_rules": [{"file": "x/Card.module.css", "class": "cardButton", "state": None,
                       "props": {"cursor": "pointer"}}],
        "icon": None,
    }
    spec = {"viewport": [1280, 800], "elements": [el]}
    res = scene_coverage(spec, {"missing_in_rust": {"a.b.card": 1}}, "s")
    assert res["counts"] == {"visual": 1, "structural": 0, "hidden": 0}, res["counts"]
    assert "clickable" in [r["kind"] for r in res["items"][0]["reasons"]]
    assert component_of("user.profile.card") == "user.profile" and component_of("x") == "x"
    assert parse_color("rgba(0, 0, 0, 0)")[3] == 0 and parse_color("rgb(244, 244, 246)")[3] == 255
    assert tokens("itemLabelText") == ["item", "label", "text"]
    hidden = dict(el, rect=[0, 0, 0, 10])
    assert hidden_reasons(hidden, (1280, 800))[0]["kind"] == "rect"
    assert hidden_reasons(dict(el, rect=[2000, 0, 10, 10]), (1280, 800))[0]["kind"] == "offscreen"
    print("selftest ok")


if __name__ == "__main__":
    if "--selftest" in sys.argv:
        selftest()
    else:
        raise SystemExit(main(sys.argv[1:]))
