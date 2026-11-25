#!/usr/bin/env python3
"""Extract official UI interactions from fluxer_app/src and compare against fluxer_client/app/src.

Stdlib only, deterministic (relative POSIX paths, no timestamps or host paths).

Categories:
  1. resize_drag   - resize / drag-handle / splitter / resize-pill / useResizable / ResizeHandle
  2. tooltips      - <Tooltip, tooltip props, data-tooltip
  3. context_menus - onContextMenu handlers and *ContextMenu components
  4. keybinds      - KeybindCommands & getDefaultKeybinds definitions
  5. hover_only    - *.module.css :hover rules toggling opacity/visibility/display/pointer-events

Usage:
  python -I tools/parity_lab/extract_interactions.py --selftest
  python -I tools/parity_lab/extract_interactions.py --out tools/parity_lab/interactions.json
"""
from __future__ import annotations

import argparse
import json
import re
import sys
import tempfile
import unittest
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_OFFICIAL_SRC = ROOT / "fluxer_app/src"
DEFAULT_RUST_SRC = ROOT / "fluxer_client/app/src"
DEFAULT_OUT = ROOT / "tools/parity_lab/interactions.json"

CATEGORIES = (
    "resize_drag",
    "tooltips",
    "context_menus",
    "keybinds",
    "hover_only",
)

EXCLUDED_DIR_PARTS = {"locales", "__tests__", "node_modules"}

# 1. resize_drag patterns
RESIZE_DRAG_RE = re.compile(
    r"\b(useResizable\w*|ResizeHandle\w*|FloatingPaneResizeHandles|ResizablePaneHandles|"
    r"SidebarResizeHandle)\b|resize-pill|drag-handle|splitter",
    re.IGNORECASE,
)
RESIZE_NOISE_RE = re.compile(
    r"resize\s*:\s*none|ResizeObserver|window\.addEventListener\(\s*['\"]resize['\"]",
    re.IGNORECASE,
)

# 2. tooltips patterns
TOOLTIP_TAG_RE = re.compile(r"<Tooltip\b([^>]*)>", re.DOTALL)
DATA_TOOLTIP_RE = re.compile(r"\bdata-tooltip\s*=\s*([^\s>}]+)")
TOOLTIP_PROP_RE = re.compile(r"\btooltip\s*=\s*(\{[^}]+\}|\"[^\"]+\"|'[^']+')")
DATA_FLX_RE = re.compile(r'\bdata-flx\s*=\s*"([^"]+)"')

# 3. context_menus patterns
ON_CONTEXT_MENU_RE = re.compile(r"\bonContextMenu\s*=\s*\{?([^}\s>]+)")
CONTEXT_MENU_COMP_RE = re.compile(r"<([A-Z][A-Za-z0-9_]*ContextMenu)\b")
CONTEXT_MENU_EXPORT_RE = re.compile(
    r"export\s+(?:const|function)\s+([A-Z][A-Za-z0-9_]*ContextMenu)\b"
)

# 4. keybinds patterns
KEYBIND_COMMAND_VALUES_RE = re.compile(
    r"KEYBIND_COMMAND_VALUES\s*=\s*\[(.*?)\]\s*as\s+const", re.DOTALL
)
QUOTED_STR_RE = re.compile(r"'([^']+)'|\"([^\"]+)\"")
DEFAULT_KEYBINDS_BLOCK_RE = re.compile(
    r"getDefaultKeybinds\s*=.*?withBrowserReservedDefaultsCleared\(\s*\[(.*?)\]\s*\)",
    re.DOTALL,
)
KEYBIND_ENTRY_RE = re.compile(r"\{\s*action:\s*'([^']+)'(.*?)\n\t\t\}", re.DOTALL)
COMBO_BLOCK_RE = re.compile(r"combo:\s*\{([^}]*)\}")
SECTION_RE = re.compile(r"section:\s*'([^']+)'")

# 5. hover_only patterns
CSS_COMMENT_RE = re.compile(r"/\*.*?\*/", re.DOTALL)
CSS_HOVER_RULE_RE = re.compile(r"([^{}]+:hover[^{}]*)\{([^{}]*)\}", re.DOTALL)
CSS_VISIBILITY_PROP_RE = re.compile(
    r"\b(opacity|visibility|display|pointer-events)\s*:\s*([^;}\n]+)", re.IGNORECASE
)

# Rust inspection patterns
RUST_FLX_RE = re.compile(r'\.flx(?:_text)?\(\s*"([^"]+)"')
RUST_TOOLTIP_RE = re.compile(r"\btooltip\b", re.IGNORECASE)
RUST_CONTEXT_MENU_COMP_RE = re.compile(r"\b([A-Z][A-Za-z0-9_]*ContextMenu)\b")
RUST_RIGHT_CLICK_RE = re.compile(r"MouseButton::Right|context_menu_item")
RUST_BUILTIN_SHORTCUT_RE = re.compile(r'should_run_builtin_shortcut\(\s*"([^"]+)"\s*\)')
RUST_SHORTCUT_SPEC_RE = re.compile(
    r'(?:spec|DefaultKeybindSpec\s*\{[^}]*action:\s*)\(?\s*"([^"]+)"\s*,\s*(?:label:\s*)?"([^"]+)"\s*,\s*(?:combo:\s*)?"([^"]+)"'
)
RUST_ASSIGNABLE_RE = re.compile(r'\("([a-z0-9_]+)"\s*,\s*"([^"]+)"\)')
RUST_RESIZE_ACTIVE_RE = re.compile(r"ActiveResize::([A-Za-z0-9_]+)")
RUST_HOVER_RE = re.compile(r"\.hover\(|_hovered\b|on_hover\b")


def _rel_posix(path: Path, root: Path) -> str:
    return path.resolve().relative_to(root.resolve()).as_posix()


def _should_skip(path: Path) -> bool:
    parts = set(path.parts)
    if parts & EXCLUDED_DIR_PARTS:
        return True
    name = path.name
    if ".test." in name or ".spec." in name or name.endswith(".d.ts"):
        return True
    return False


def _iter_files(base: Path, suffixes: tuple[str, ...]) -> list[Path]:
    if not base.exists():
        return []
    files = [
        p
        for p in base.rglob("*")
        if p.is_file() and p.name.endswith(suffixes) and not _should_skip(p)
    ]
    return sorted(files, key=lambda p: p.as_posix())


def _line_of_offset(text: str, offset: int) -> int:
    return text.count("\n", 0, offset) + 1


def _nearby_flx(text: str, offset: int, window: int = 350) -> str | None:
    start = max(0, offset - window)
    end = min(len(text), offset + window)
    snippet = text[start:end]
    m = DATA_FLX_RE.search(snippet)
    return m.group(1) if m else None


def _camel_to_kebab(name: str) -> str:
    s = re.sub(r"([a-z0-9])([A-Z])", r"\1-\2", name)
    return s.lower()


class RustIndex:
    """Indexed interaction capabilities extracted from fluxer_client/app/src."""

    def __init__(self, rust_src: Path, repo_root: Path) -> None:
        self.rust_src = rust_src
        self.repo_root = repo_root
        self.flx_locations: dict[str, list[str]] = {}
        self.tooltip_files: dict[str, int] = {}
        self.tooltip_flx_tags: set[str] = set()
        self.context_menu_names: dict[str, list[str]] = {}
        self.right_click_files: list[str] = []
        self.active_shortcuts: set[str] = set()
        self.cheatsheet_shortcuts: dict[str, str] = {}
        self.assignable_shortcuts: set[str] = set()
        self.resize_variants: set[str] = set()
        self.resize_files: list[str] = []
        self.hover_files: set[str] = set()
        self._scan()

    def _scan(self) -> None:
        rs_files = [
            p
            for p in _iter_files(self.rust_src, (".rs",))
            if not p.name.startswith("tests") and p.name != "theme_tokens_generated.rs"
        ]
        for p in rs_files:
            rel = _rel_posix(p, self.repo_root)
            text = p.read_text(encoding="utf-8", errors="ignore")

            for m in RUST_FLX_RE.finditer(text):
                tag = m.group(1)
                line = _line_of_offset(text, m.start())
                self.flx_locations.setdefault(tag, []).append(f"{rel}:{line}")
                ctx = text[max(0, m.start() - 250) : min(len(text), m.end() + 250)]
                if RUST_TOOLTIP_RE.search(ctx):
                    self.tooltip_flx_tags.add(tag)

            tt_hits = len(RUST_TOOLTIP_RE.findall(text))
            if tt_hits:
                self.tooltip_files[rel] = tt_hits

            for m in RUST_CONTEXT_MENU_COMP_RE.finditer(text):
                name = m.group(1)
                line = _line_of_offset(text, m.start())
                self.context_menu_names.setdefault(name, []).append(f"{rel}:{line}")

            if RUST_RIGHT_CLICK_RE.search(text):
                self.right_click_files.append(rel)

            if "resize" in p.name.lower() or RUST_RESIZE_ACTIVE_RE.search(text):
                self.resize_files.append(rel)
            for m in RUST_RESIZE_ACTIVE_RE.finditer(text):
                self.resize_variants.add(m.group(1))

            if RUST_HOVER_RE.search(text):
                self.hover_files.add(rel)

            if p.name == "shortcuts.rs":
                for m in RUST_BUILTIN_SHORTCUT_RE.finditer(text):
                    self.active_shortcuts.add(m.group(1))
                for m in RUST_SHORTCUT_SPEC_RE.finditer(text):
                    self.cheatsheet_shortcuts[m.group(1)] = m.group(3)
                for m in RUST_ASSIGNABLE_RE.finditer(text):
                    self.assignable_shortcuts.add(m.group(1))


def extract_resize_drag(official_src: Path, repo_root: Path) -> list[dict]:
    items: list[dict] = []
    seen_keys: set[str] = set()
    for p in _iter_files(official_src, (".tsx", ".ts", ".module.css")):
        rel = _rel_posix(p, repo_root)
        text = p.read_text(encoding="utf-8", errors="ignore")
        for m in RESIZE_DRAG_RE.finditer(text):
            line = _line_of_offset(text, m.start())
            line_text = text.splitlines()[line - 1] if line <= len(text.splitlines()) else ""
            if RESIZE_NOISE_RE.search(line_text):
                continue
            token = m.group(0)
            key = f"{rel}:{token.lower()}"
            if key in seen_keys:
                continue
            seen_keys.add(key)
            flx = _nearby_flx(text, m.start())
            items.append(
                {
                    "id": f"resize_drag:{rel}#{token}",
                    "category": "resize_drag",
                    "name": token,
                    "official": {
                        "file": rel,
                        "line": line,
                        "pattern": token,
                        "flx": flx,
                    },
                }
            )
    return sorted(items, key=lambda x: x["id"])


def extract_tooltips(official_src: Path, repo_root: Path) -> list[dict]:
    items: list[dict] = []
    for p in _iter_files(official_src, (".tsx",)):
        rel = _rel_posix(p, repo_root)
        text = p.read_text(encoding="utf-8", errors="ignore")
        hits: list[tuple[int, str, str, str | None]] = []

        for m in TOOLTIP_TAG_RE.finditer(text):
            line = _line_of_offset(text, m.start())
            attrs = re.sub(r"\s+", " ", m.group(1)).strip()
            flx = _nearby_flx(text, m.start())
            hits.append((line, "Tooltip", attrs[:120], flx))

        for m in DATA_TOOLTIP_RE.finditer(text):
            line = _line_of_offset(text, m.start())
            val = m.group(1).strip()
            flx = _nearby_flx(text, m.start())
            hits.append((line, "data-tooltip", val[:120], flx))

        for m in TOOLTIP_PROP_RE.finditer(text):
            line = _line_of_offset(text, m.start())
            val = re.sub(r"\s+", " ", m.group(1)).strip()
            flx = _nearby_flx(text, m.start())
            hits.append((line, "tooltip-prop", val[:120], flx))

        for idx, (line, kind, detail, flx) in enumerate(sorted(hits), 1):
            items.append(
                {
                    "id": f"tooltips:{rel}:L{line}:{kind}:{idx}",
                    "category": "tooltips",
                    "name": f"{p.stem}:{kind}@L{line}",
                    "official": {
                        "file": rel,
                        "line": line,
                        "kind": kind,
                        "detail": detail,
                        "flx": flx,
                    },
                }
            )
    return sorted(items, key=lambda x: x["id"])


def extract_context_menus(official_src: Path, repo_root: Path) -> list[dict]:
    items: list[dict] = []
    seen: set[str] = set()
    for p in _iter_files(official_src, (".tsx", ".ts")):
        rel = _rel_posix(p, repo_root)
        text = p.read_text(encoding="utf-8", errors="ignore")

        for m in CONTEXT_MENU_EXPORT_RE.finditer(text):
            name = m.group(1)
            line = _line_of_offset(text, m.start())
            key = f"comp:{rel}:{name}"
            if key in seen:
                continue
            seen.add(key)
            flx = _nearby_flx(text, m.start())
            items.append(
                {
                    "id": f"context_menus:component:{name}:{rel}",
                    "category": "context_menus",
                    "name": name,
                    "official": {
                        "file": rel,
                        "line": line,
                        "kind": "component_definition",
                        "flx": flx,
                    },
                }
            )

        for m in CONTEXT_MENU_COMP_RE.finditer(text):
            name = m.group(1)
            line = _line_of_offset(text, m.start())
            key = f"usage:{rel}:{name}:{line}"
            if key in seen:
                continue
            seen.add(key)
            flx = _nearby_flx(text, m.start())
            items.append(
                {
                    "id": f"context_menus:render:{name}:{rel}:L{line}",
                    "category": "context_menus",
                    "name": name,
                    "official": {
                        "file": rel,
                        "line": line,
                        "kind": "component_render",
                        "flx": flx,
                    },
                }
            )

        for m in ON_CONTEXT_MENU_RE.finditer(text):
            handler = m.group(1).strip()
            line = _line_of_offset(text, m.start())
            flx = _nearby_flx(text, m.start())
            items.append(
                {
                    "id": f"context_menus:handler:{rel}:L{line}",
                    "category": "context_menus",
                    "name": f"{p.stem}.onContextMenu",
                    "official": {
                        "file": rel,
                        "line": line,
                        "kind": "onContextMenu",
                        "handler": handler[:80],
                        "flx": flx,
                    },
                }
            )
    return sorted(items, key=lambda x: x["id"])


def _parse_combo(combo_body: str) -> str:
    key_m = re.search(r"key:\s*'([^']*)'", combo_body)
    key = key_m.group(1) if key_m else ""
    parts: list[str] = []
    if re.search(r"\bctrlOrMeta\s*:\s*true\b", combo_body) or re.search(r"\bctrl\s*:\s*true\b", combo_body):
        parts.append("ctrl")
    if re.search(r"\balt\s*:\s*true\b", combo_body):
        parts.append("alt")
    if re.search(r"\bshift\s*:\s*true\b", combo_body):
        parts.append("shift")
    if re.search(r"\bmeta\s*:\s*true\b", combo_body):
        parts.append("meta")
    if key:
        norm_key = key.lower().replace("arrow", "")
        parts.append(norm_key)
    return "-".join(parts)


def extract_keybinds(official_src: Path, repo_root: Path) -> list[dict]:
    commands_path = official_src / "features/input/state/input_keybind/KeybindCommands.ts"
    input_kb_path = official_src / "features/input/state/InputKeybind.ts"

    commands: list[str] = []
    if commands_path.exists():
        cmd_text = commands_path.read_text(encoding="utf-8", errors="ignore")
        m = KEYBIND_COMMAND_VALUES_RE.search(cmd_text)
        if m:
            for q in QUOTED_STR_RE.finditer(m.group(1)):
                commands.append(q.group(1) or q.group(2))

    defaults: dict[str, dict] = {}
    if input_kb_path.exists():
        kb_text = input_kb_path.read_text(encoding="utf-8", errors="ignore")
        block_m = DEFAULT_KEYBINDS_BLOCK_RE.search(kb_text)
        if block_m:
            block = block_m.group(1)
            block_start = block_m.start(1)
            for em in KEYBIND_ENTRY_RE.finditer(block):
                action = em.group(1)
                body = em.group(2)
                line = _line_of_offset(kb_text, block_start + em.start())
                combo_m = COMBO_BLOCK_RE.search(body)
                combo_str = _parse_combo(combo_m.group(1)) if combo_m else ""
                sec_m = SECTION_RE.search(body)
                section = sec_m.group(1) if sec_m else "unknown"
                defaults[action] = {
                    "file": _rel_posix(input_kb_path, repo_root),
                    "line": line,
                    "combo": combo_str,
                    "section": section,
                    "informational_only": "informationalOnly: true" in body,
                }

    all_actions = sorted(set(commands) | set(defaults.keys()))
    cmd_rel = _rel_posix(commands_path, repo_root) if commands_path.exists() else "features/input/state/input_keybind/KeybindCommands.ts"
    items: list[dict] = []
    for action in all_actions:
        info = defaults.get(action)
        items.append(
            {
                "id": f"keybinds:{action}",
                "category": "keybinds",
                "name": action,
                "official": info
                if info
                else {
                    "file": cmd_rel,
                    "line": 1,
                    "combo": "",
                    "section": "custom_or_unbound",
                    "informational_only": False,
                },
            }
        )
    return items


def extract_hover_only(official_src: Path, repo_root: Path) -> list[dict]:
    items: list[dict] = []
    for p in _iter_files(official_src, (".module.css",)):
        rel = _rel_posix(p, repo_root)
        raw = p.read_text(encoding="utf-8", errors="ignore")
        cleaned = CSS_COMMENT_RE.sub("", raw)
        seen_sel: set[str] = set()
        for m in CSS_HOVER_RULE_RE.finditer(cleaned):
            selector = re.sub(r"\s+", " ", m.group(1)).strip()
            body = m.group(2)
            props = [
                f"{pm.group(1).lower()}: {pm.group(2).strip()}"
                for pm in CSS_VISIBILITY_PROP_RE.finditer(body)
            ]
            if not props:
                continue
            key = f"{rel}:{selector}"
            if key in seen_sel:
                continue
            seen_sel.add(key)
            line = _line_of_offset(raw, m.start())
            items.append(
                {
                    "id": f"hover_only:{rel}:{selector}",
                    "category": "hover_only",
                    "name": f"{p.stem}:{selector}",
                    "official": {
                        "file": rel,
                        "line": line,
                        "selector": selector,
                        "properties": props,
                    },
                }
            )
    return sorted(items, key=lambda x: x["id"])


def _classify_resize_drag(item: dict, rust: RustIndex) -> tuple[str, list[str], str]:
    off = item["official"]
    f = off["file"]
    flx = off.get("flx")
    if flx and flx in rust.flx_locations:
        return "present", rust.flx_locations[flx], f"matched data-flx '{flx}'"
    if "SidebarResizeHandle" in f or "GuildsLayout" in f or "OutlineFrame" in f:
        if "Sidebar" in rust.resize_variants:
            return "present", ["fluxer_client/app/src/client_view.rs"], "ActiveResize::Sidebar implemented"
    if "ExpressionPickerPopout" in f:
        if "EmojiPicker" in rust.resize_variants:
            return "present", ["fluxer_client/app/src/client_view.rs"], "ActiveResize::EmojiPicker implemented"
    if "CompactVoiceCall" in f:
        if "VoiceCallHeight" in rust.resize_variants:
            return "present", ["fluxer_client/app/src/client_view.rs"], "ActiveResize::VoiceCallHeight implemented"
    if any(k in f for k in ("ChannelPinsPopout", "InboxPopout")):
        return "differs", ["fluxer_client/app/src/ui/channel_popouts.rs"], "popout exists in Rust without resize handle"
    return "missing", [], "no matching resize/drag handle in fluxer_client/app/src"


def _classify_tooltip(item: dict, rust: RustIndex) -> tuple[str, list[str], str]:
    off = item["official"]
    flx = off.get("flx")
    f = off["file"].lower()
    if flx:
        if flx in rust.tooltip_flx_tags:
            return "present", rust.flx_locations.get(flx, []), f"flx '{flx}' has tooltip in Rust"
        if flx in rust.flx_locations:
            return "differs", rust.flx_locations[flx], f"flx '{flx}' exists in Rust without tooltip call"
    # Semantic area fallback
    area_map = {
        "channelheader": "fluxer_client/app/src/ui/header.rs",
        "voicecontrolbar": "fluxer_client/app/src/ui/user_panel.rs",
        "guildrail": "fluxer_client/app/src/ui/guild_rail.rs",
        "channelmessage": "fluxer_client/app/src/ui/message_actions.rs",
        "textarea": "fluxer_client/app/src/ui/composer.rs",
        "channellist": "fluxer_client/app/src/ui/channel_list.rs",
    }
    for key, rust_file in area_map.items():
        if key in f and rust_file in rust.tooltip_files:
            return "differs", [rust_file], f"area '{key}' has tooltips in Rust, exact element unverified"
    return "missing", [], "no matching tooltip in fluxer_client/app/src"


def _classify_context_menu(item: dict, rust: RustIndex) -> tuple[str, list[str], str]:
    name = item["name"]
    off = item["official"]
    flx = off.get("flx")
    if name in rust.context_menu_names:
        return "present", rust.context_menu_names[name], f"Rust implements {name}"
    if "ChannelMessage" in off["file"] and "MessageContextMenu" in rust.context_menu_names:
        return "present", rust.context_menu_names["MessageContextMenu"], "mapped to MessageContextMenu"
    if flx and flx in rust.flx_locations:
        locs = rust.flx_locations[flx]
        has_rc = any(loc.split(":")[0] in rust.right_click_files for loc in locs)
        if has_rc:
            return "present", locs, f"matched data-flx '{flx}' with right-click/context-menu handler"
        return "differs", locs, f"flx '{flx}' exists in Rust without right-click/context-menu handler"
    if "ChannelList" in off["file"] or "ChannelItem" in off["file"]:
        if "fluxer_client/app/src/ui/channel_list.rs" in rust.right_click_files:
            return "differs", ["fluxer_client/app/src/ui/channel_list.rs"], "partial right-click handler in channel_list.rs"
    return "missing", [], f"no matching context menu for {name}"


def _classify_keybind(item: dict, rust: RustIndex) -> tuple[str, list[str], str]:
    action = item["name"]
    off = item["official"]
    is_active = action in rust.active_shortcuts
    in_cheatsheet = action in rust.cheatsheet_shortcuts
    is_assignable = action in rust.assignable_shortcuts

    if is_active:
        rust_combo = rust.cheatsheet_shortcuts.get(action, "")
        off_combo = off.get("combo", "")
        if rust_combo and off_combo and rust_combo != off_combo:
            return (
                "differs",
                ["fluxer_client/app/src/shortcuts.rs"],
                f"combo differs: official '{off_combo}' vs rust '{rust_combo}'",
            )
        return "present", ["fluxer_client/app/src/shortcuts.rs"], "bound in shortcuts.rs"
    if in_cheatsheet or is_assignable:
        return (
            "differs",
            ["fluxer_client/app/src/shortcuts.rs"],
            "listed in cheatsheet/assignable only, not bound in should_run_builtin_shortcut",
        )
    return "missing", [], "action not present in shortcuts.rs"


def _classify_hover_only(item: dict, rust: RustIndex) -> tuple[str, list[str], str]:
    off = item["official"]
    stem = Path(off["file"]).name.replace(".module.css", "").lower()
    if "message" in stem and "fluxer_client/app/src/ui/message_row.rs" in rust.hover_files:
        return "present", ["fluxer_client/app/src/ui/message_row.rs"], "hover action bar implemented in message_row.rs"
    if "channel" in stem and "fluxer_client/app/src/ui/channel_list.rs" in rust.hover_files:
        return "differs", ["fluxer_client/app/src/ui/channel_list.rs"], "partial hover state in channel_list.rs"
    return "missing", [], f"no hover-only visibility transition matched for {stem}"


CLASSIFIERS = {
    "resize_drag": _classify_resize_drag,
    "tooltips": _classify_tooltip,
    "context_menus": _classify_context_menu,
    "keybinds": _classify_keybind,
    "hover_only": _classify_hover_only,
}


def build_report(official_src: Path, rust_src: Path, repo_root: Path) -> dict:
    rust = RustIndex(rust_src, repo_root)
    raw_items: list[dict] = []
    raw_items.extend(extract_resize_drag(official_src, repo_root))
    raw_items.extend(extract_tooltips(official_src, repo_root))
    raw_items.extend(extract_context_menus(official_src, repo_root))
    raw_items.extend(extract_keybinds(official_src, repo_root))
    raw_items.extend(extract_hover_only(official_src, repo_root))

    items: list[dict] = []
    action_items: list[dict] = []
    by_category: dict[str, Counter] = {c: Counter() for c in CATEGORIES}
    total_counts = Counter()

    for item in raw_items:
        cat = item["category"]
        status, matches, reason = CLASSIFIERS[cat](item, rust)
        enriched = {
            **item,
            "status": status,
            "rust": {
                "status": status,
                "matches": sorted(set(matches)),
                "reason": reason,
            },
        }
        items.append(enriched)
        by_category[cat][status] += 1
        by_category[cat]["total"] += 1
        total_counts[status] += 1
        total_counts["total"] += 1

        if status in ("missing", "differs"):
            action_items.append(
                {
                    "id": f"action:{item['id']}",
                    "item_id": item["id"],
                    "category": cat,
                    "status": status,
                    "title": f"[{cat}/{status}] {item['name']}: {reason}",
                    "official_file": item["official"]["file"],
                    "official_line": item["official"]["line"],
                    "rust_matches": sorted(set(matches)),
                }
            )

    summary = {
        "total": {
            "total": total_counts["total"],
            "present": total_counts["present"],
            "missing": total_counts["missing"],
            "differs": total_counts["differs"],
        },
        "by_category": {
            cat: {
                "total": by_category[cat]["total"],
                "present": by_category[cat]["present"],
                "missing": by_category[cat]["missing"],
                "differs": by_category[cat]["differs"],
            }
            for cat in CATEGORIES
        },
    }

    return {
        "version": 1,
        "source_roots": {
            "official": _rel_posix(official_src, repo_root),
            "rust": _rel_posix(rust_src, repo_root),
        },
        "categories": list(CATEGORIES),
        "summary": summary,
        "items": items,
        "action_items": action_items,
    }


class ExtractInteractionsTests(unittest.TestCase):
    def _build_fixture(self, tmp: Path) -> tuple[Path, Path, Path]:
        off = tmp / "fluxer_app/src"
        rust = tmp / "fluxer_client/app/src"
        (off / "features/app/components/layout").mkdir(parents=True)
        (off / "features/input/state/input_keybind").mkdir(parents=True)
        (off / "features/channel/components").mkdir(parents=True)
        (rust / "ui").mkdir(parents=True)

        (off / "features/app/components/layout/SidebarResizeHandle.tsx").write_text(
            'export const SidebarResizeHandle = () => <div data-flx="app.sidebar-resize" />;\n',
            encoding="utf-8",
        )
        (off / "features/channel/components/Header.tsx").write_text(
            'export const H = () => <Tooltip content="Pin"><button data-flx="hdr.pin" onContextMenu={openMenu} /></Tooltip>;\n',
            encoding="utf-8",
        )
        (off / "features/channel/components/Message.module.css").write_text(
            ".row:hover .actions { opacity: 1; pointer-events: auto; }\n",
            encoding="utf-8",
        )
        (off / "features/input/state/input_keybind/KeybindCommands.ts").write_text(
            "const KEYBIND_COMMAND_VALUES = [\n  'nav_channel_prev',\n  'voice_push_to_talk',\n] as const;\n",
            encoding="utf-8",
        )
        (off / "features/input/state/InputKeybind.ts").write_text(
            "const getDefaultKeybinds = () => withBrowserReservedDefaultsCleared([\n"
            "\t\t{\n"
            "\t\t\taction: 'nav_channel_prev',\n"
            "\t\t\tcombo: {key: 'ArrowUp', alt: true},\n"
            "\t\t\tsection: 'navigation',\n"
            "\t\t},\n"
            "\t\t{\n"
            "\t\t\taction: 'voice_push_to_talk',\n"
            "\t\t\tcombo: {key: 'v'},\n"
            "\t\t\tsection: 'voice_and_video',\n"
            "\t\t},\n"
            "\t]);\n",
            encoding="utf-8",
        )

        (rust / "client_view.rs").write_text(
            'enum ActiveResize { Sidebar }\nfn x() { div().flx("app.sidebar-resize"); }\n',
            encoding="utf-8",
        )
        (rust / "ui/header.rs").write_text(
            'fn pin() { div().flx("hdr.pin").tooltip("Pin"); }\n',
            encoding="utf-8",
        )
        (rust / "ui/message_row.rs").write_text(
            "fn row() { div().hover(|s| s); }\n",
            encoding="utf-8",
        )
        (rust / "shortcuts.rs").write_text(
            'fn run() { if v.should_run_builtin_shortcut("nav_channel_prev") {} }\n',
            encoding="utf-8",
        )
        return off, rust, tmp

    def test_extract_all_categories_and_determinism(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            off, rust, root = self._build_fixture(Path(td))
            r1 = build_report(off, rust, root)
            r2 = build_report(off, rust, root)
            s1 = json.dumps(r1, indent=2, sort_keys=True)
            s2 = json.dumps(r2, indent=2, sort_keys=True)
            self.assertEqual(s1, s2)
            self.assertNotIn(str(root), s1)

            cats = r1["summary"]["by_category"]
            for c in CATEGORIES:
                self.assertGreaterEqual(cats[c]["total"], 1, f"missing category {c}")
            self.assertEqual(cats["keybinds"]["present"], 1)
            self.assertEqual(cats["keybinds"]["missing"], 1)
            self.assertEqual(cats["tooltips"]["present"], 1)
            self.assertEqual(cats["resize_drag"]["present"], 1)
            self.assertEqual(cats["hover_only"]["present"], 1)
            self.assertEqual(cats["context_menus"]["differs"], 1)
            self.assertEqual(
                len(r1["action_items"]),
                r1["summary"]["total"]["missing"] + r1["summary"]["total"]["differs"],
            )


def run_selftest() -> int:
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(ExtractInteractionsTests)
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    return 0 if result.wasSuccessful() else 1


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--selftest", action="store_true", help="Run built-in unit tests")
    parser.add_argument(
        "--out",
        type=Path,
        default=DEFAULT_OUT,
        help="Output JSON path (default: tools/parity_lab/interactions.json)",
    )
    parser.add_argument("--official", type=Path, default=DEFAULT_OFFICIAL_SRC)
    parser.add_argument("--rust", type=Path, default=DEFAULT_RUST_SRC)
    args = parser.parse_args(argv)

    if args.selftest:
        return run_selftest()

    report = build_report(args.official, args.rust, ROOT)
    out_path = args.out if args.out.is_absolute() else (ROOT / args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")

    s = report["summary"]["total"]
    print(
        f"wrote {out_path.relative_to(ROOT).as_posix()}: "
        f"total={s['total']} present={s['present']} missing={s['missing']} differs={s['differs']}"
    )
    for cat in CATEGORIES:
        c = report["summary"]["by_category"][cat]
        print(
            f"  {cat}: total={c['total']} present={c['present']} missing={c['missing']} differs={c['differs']}"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
