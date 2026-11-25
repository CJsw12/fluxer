#!/usr/bin/env python3
"""Generate a Rust module of theme token constants from a theme_tokens.mjs capture.

Stdlib only, deterministic (no timestamps or host paths in the output).
Usage:
  python -I tools/parity_lab/theme_tokens_rs.py <tokens.json> <out.rs>
  python -I tools/parity_lab/theme_tokens_rs.py --multi dark=<dark.json> light=<light.json> --out <out.rs>
"""

import json
import math
import re
import sys
from pathlib import Path

HEX_RE = re.compile(r"^#([0-9a-fA-F]{3}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$")
FUNC_RE = re.compile(r"^(rgba?|hsla?)\((.*)\)$", re.IGNORECASE)
COLOR_SRGB_RE = re.compile(r"^color\(\s*srgb\s+([^()]+)\)$", re.IGNORECASE)
MOD_NAME_RE = re.compile(r"^[a-zA-Z_][a-zA-Z0-9_]*$")
NON_ALNUM_RE = re.compile(r"[^0-9A-Za-z]+")
UNDERSCORES_RE = re.compile(r"_+")


def const_name(raw):
    name = raw[2:] if raw.startswith("--") else raw
    name = NON_ALNUM_RE.sub("_", name).strip("_")
    name = UNDERSCORES_RE.sub("_", name).upper() or "TOKEN"
    if name[0].isdigit():
        name = f"_{name}"
    return name


def rust_str(value):
    out = []
    for ch in value:
        if ch == "\\":
            out.append("\\\\")
        elif ch == '"':
            out.append('\\"')
        elif ch == "\n":
            out.append("\\n")
        elif ch == "\r":
            out.append("\\r")
        elif ch == "\t":
            out.append("\\t")
        elif ord(ch) < 0x20 or ord(ch) == 0x7F:
            out.append("\\u{%x}" % ord(ch))
        else:
            out.append(ch)
    return "".join(out)


def byte(value):
    return max(0, min(255, int(math.floor(value + 0.5))))


def _split_args(inner):
    alpha = None
    if "/" in inner:
        inner, alpha = inner.split("/", 1)
        alpha = alpha.strip()
    parts = [p.strip() for p in (inner.split(",") if "," in inner else inner.split())]
    if alpha is None and len(parts) == 4:
        alpha = parts.pop()
    return parts, alpha


def _float(text):
    text = text.strip().lower()
    if text.endswith("deg"):
        text = text[:-3]
    return float(text)


def _percent(text, scale):
    text = text.strip()
    if text.endswith("%"):
        return _float(text[:-1]) / 100.0 * scale
    return _float(text)


def _hsl_to_rgb(hue, sat, light):
    hue = math.fmod(_float(hue), 360.0)
    if hue < 0:
        hue += 360.0
    sat = max(0.0, min(1.0, _percent(sat, 1.0)))
    light = max(0.0, min(1.0, _percent(light, 1.0)))
    chroma = (1.0 - abs(2.0 * light - 1.0)) * sat
    hp = hue / 60.0
    x = chroma * (1.0 - abs(math.fmod(hp, 2.0) - 1.0))
    m = light - chroma / 2.0
    sector = int(hp) % 6
    rgb = [
        (chroma, x, 0.0),
        (x, chroma, 0.0),
        (0.0, chroma, x),
        (0.0, x, chroma),
        (x, 0.0, chroma),
        (chroma, 0.0, x),
    ][sector]
    return tuple((c + m) * 255.0 for c in rgb)


def color_rgba(value):
    """Return 0xRRGGBBAA for a plain color value, else None."""
    text = value.strip()
    hex_match = HEX_RE.match(text)
    if hex_match:
        digits = hex_match.group(1)
        if len(digits) == 3:
            digits = "".join(ch * 2 for ch in digits)
        if len(digits) == 6:
            digits += "FF"
        return int(digits, 16)
    srgb = COLOR_SRGB_RE.match(text)
    if srgb:
        parts, alpha = _split_args(srgb.group(1))
        try:
            if len(parts) != 3:
                return None
            channels = [_percent(p, 1.0) * 255.0 for p in parts]
            a = 1.0 if alpha is None else _percent(alpha, 1.0)
        except (ValueError, IndexError):
            return None
        r, g, b = (byte(c) for c in channels)
        return (r << 24) | (g << 16) | (b << 8) | byte(a * 255.0)
    func = FUNC_RE.match(text)
    if not func:
        return None
    kind = func.group(1).lower()
    inner = func.group(2)
    if "(" in inner:
        return None
    parts, alpha = _split_args(inner)
    try:
        if kind.startswith("rgb") and len(parts) == 3:
            channels = [_percent(p, 255.0) for p in parts]
        elif kind.startswith("hsl") and len(parts) == 3:
            channels = list(_hsl_to_rgb(parts[0], parts[1], parts[2]))
        else:
            return None
        a = 1.0 if alpha is None else _percent(alpha, 1.0)
    except (ValueError, IndexError):
        return None
    r, g, b = (byte(c) for c in channels)
    return (r << 24) | (g << 16) | (b << 8) | byte(a * 255.0)


def unique_name(base, used):
    candidate = base
    n = 2
    while candidate in used or f"{candidate}_RGBA" in used:
        candidate = f"{base}_{n}"
        n += 1
    used.add(candidate)
    return candidate


def load_payload(src):
    try:
        payload = json.loads(src.read_text(encoding="utf-8-sig"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ValueError(f"cannot read {src}: {exc}") from exc
    tokens = payload.get("tokens") if isinstance(payload, dict) else None
    if not isinstance(tokens, dict):
        raise ValueError(f"input JSON {src} has no 'tokens' object")
    resolved = payload.get("resolved") if isinstance(payload.get("resolved"), dict) else {}
    client = payload.get("client")
    build = client.strip() if isinstance(client, str) and client.strip() else "unknown build"
    return tokens, resolved, build


def emit_token_consts(tokens, resolved, indent=""):
    lines = []
    used = set()
    rgba_count = 0
    for raw in sorted(tokens):
        value = tokens[raw]
        if not isinstance(value, str):
            raise ValueError(f"token {raw} has a non-string value")
        name = unique_name(const_name(raw), used)
        lines.append(f'{indent}pub const {name}: &str = "{rust_str(value)}";')
        # Prefer the engine-resolved colour (rgb()/rgba()/color(srgb ...)) over the raw expression.
        resolved_val = resolved.get(raw, value)
        rgba = color_rgba(resolved_val) if isinstance(resolved_val, str) else None
        if rgba is not None:
            lines.append(f"{indent}pub const {name}_RGBA: u32 = 0x{rgba:08X};")
            rgba_count += 1
        lines.append("")
    return lines, rgba_count


def run_multi(args):
    entries = {}
    out_path = None
    i = 0
    while i < len(args):
        arg = args[i]
        if arg == "--out":
            if i + 1 >= len(args):
                print("error: --out requires a file path", file=sys.stderr)
                return 2
            out_path = Path(args[i + 1])
            i += 2
            continue
        if "=" not in arg:
            print(f"error: expected <mod>=<path.json>, got {arg!r}", file=sys.stderr)
            return 2
        mod_name, path_str = arg.split("=", 1)
        if not MOD_NAME_RE.match(mod_name):
            print(f"error: invalid Rust module name {mod_name!r}", file=sys.stderr)
            return 2
        if mod_name in entries:
            print(f"error: duplicate module name {mod_name!r}", file=sys.stderr)
            return 2
        entries[mod_name] = Path(path_str)
        i += 1

    if not entries or out_path is None:
        print(
            "usage: python -I theme_tokens_rs.py --multi <mod>=<tokens.json> [...] --out <out.rs>",
            file=sys.stderr,
        )
        return 2

    loaded = []
    builds = []
    for mod_name in sorted(entries):
        src = entries[mod_name]
        try:
            tokens, resolved, build = load_payload(src)
        except ValueError as exc:
            print(f"error: {exc}", file=sys.stderr)
            return 1
        loaded.append((mod_name, tokens, resolved))
        if build != "unknown build" and build not in builds:
            builds.append(build)

    source_build = "; ".join(builds) if builds else "unknown build"
    summary = ", ".join(f"{mod_name} ({len(tokens)} tokens)" for mod_name, tokens, _ in loaded)
    lines = [
        "// Generated by tools/parity_lab/theme_tokens_rs.py. Do not edit by hand.",
        f"// Source build: {source_build}",
        f"// Modules: {summary}",
        "#![allow(dead_code)]",
        "",
        f'pub const SOURCE_BUILD: &str = "{rust_str(source_build)}";',
        "",
    ]

    total_tokens = 0
    total_rgba = 0
    for mod_name, tokens, resolved in loaded:
        try:
            mod_lines, rgba_count = emit_token_consts(tokens, resolved, indent="    ")
        except ValueError as exc:
            print(f"error: {exc}", file=sys.stderr)
            return 1
        total_tokens += len(tokens)
        total_rgba += rgba_count
        lines.append(f"pub mod {mod_name} {{")
        lines.extend(mod_lines)
        while lines and lines[-1] == "":
            lines.pop()
        lines.append("}")
        lines.append("")

    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_bytes(("\n".join(lines).rstrip("\n") + "\n").encode("utf-8"))
    print(f"wrote {total_tokens} tokens ({total_rgba} rgba) across {len(loaded)} modules to {out_path}")
    return 0


def main(argv):
    if len(argv) >= 2 and argv[1] == "--multi":
        return run_multi(argv[2:])
    if len(argv) != 3:
        print(
            "usage: python -I theme_tokens_rs.py <tokens.json> <out.rs>\n"
            "   or: python -I theme_tokens_rs.py --multi <mod>=<tokens.json> [...] --out <out.rs>",
            file=sys.stderr,
        )
        return 2
    src, dst = Path(argv[1]), Path(argv[2])
    try:
        tokens, resolved, build = load_payload(src)
        token_lines, _ = emit_token_consts(tokens, resolved)
    except ValueError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1

    lines = [
        "// Generated by tools/parity_lab/theme_tokens_rs.py. Do not edit by hand.",
        f"// Source build: {build}",
        f"// Token count: {len(tokens)}",
        "",
    ]
    lines.extend(token_lines)

    dst.parent.mkdir(parents=True, exist_ok=True)
    dst.write_bytes(("\n".join(lines).rstrip("\n") + "\n").encode("utf-8"))
    print(f"wrote {len(tokens)} tokens to {dst}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
