#!/usr/bin/env python3
"""Flag newly added hard-coded colours under fluxer_client/app/src (theme-token guard).

    python -I tools/parity_lab/check_colors.py [--base REF] [--all] [--selftest]

Default mode inspects the ADDED lines of `git diff REF` (REF defaults to HEAD, so staged and
unstaged changes are both covered) and flags colour literals that bypass the theme:
`rgb(0x..)`, `rgba(0x..)`, `hsla(<number>, ...)`, `hsl(<number>, ...)` and their `gpui_kit::`
forms. Files named tests*.rs, theme.rs and theme_tokens_generated.rs are skipped, as are
comment lines and lines containing `assert`. Fully transparent literals (`hsla(0., 0., 0., 0.)`,
8-digit hex ending in `00`) and `transparent_black()`/`transparent_white()` are ignored.

`tools/parity_lab/color_allowlist.json` carries reviewed exceptions: each entry is
{"file": "<repo-relative path>", "contains": "<exact substring>", "reason": "..."} and an entry
without a non-empty reason is ignored (warned on stderr). A matching entry suppresses every
literal on that added line.

Output: one line per finding, `file:line  literal  | snippet`, then
`colors: N new hard-coded literal(s)`; exit 1 when N > 0. --all scans the whole tree and only
prints per-file counts (informational, exit 0).
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
APP_SRC_REL = "fluxer_client/app/src"
ALLOWLIST_PATH = HERE / "color_allowlist.json"
EXCLUDED_NAMES = {"theme.rs", "theme_tokens_generated.rs"}
SCOPE_RE = re.compile(r"^fluxer_client/app/src/.+\.rs$")
CALL_RE = re.compile(r"(?<![A-Za-z0-9_])(?P<prefix>gpui_kit::)?(?P<func>rgba?|hsla?)\(")
NUM_START_RE = re.compile(r"[-+0-9.]")
HEX_RE = re.compile(r"^[0-9a-fA-F]+$")
HEX_CONST_RE = re.compile(r"\bconst\s+\w+\s*(?::[^=;]+)?=\s*(0x[\da-fA-F_]+)(?:u32)?\s*;")
SNIPPET_MAX = 80


def die(msg: str, code: int = 2) -> None:
    print(msg, file=sys.stderr)
    raise SystemExit(code)


# ---------------------------------------------------------------- scanning

def in_scope(path: str) -> bool:
    if not SCOPE_RE.match(path):
        return False
    name = path.rsplit("/", 1)[-1]
    if name in EXCLUDED_NAMES:
        return False
    return not (name.startswith("tests") and name.endswith(".rs"))


def strip_comment(line: str) -> str:
    """Drops a trailing `//` comment that is outside string literals."""
    in_str = False
    i = 0
    while i < len(line):
        ch = line[i]
        if in_str:
            if ch == "\\":
                i += 2
                continue
            if ch == '"':
                in_str = False
            i += 1
            continue
        if ch == '"':
            in_str = True
            i += 1
            continue
        if line.startswith("//", i):
            return line[:i]
        i += 1
    return line


def _call_end(line: str, open_index: int) -> int:
    depth = 0
    for i in range(open_index, len(line)):
        if line[i] == "(":
            depth += 1
        elif line[i] == ")":
            depth -= 1
            if depth == 0:
                return i
    return -1


def _alpha_zero(func: str, args: str) -> bool:
    if func == "hsla":
        parts = [p.strip() for p in args.split(",")]
        if len(parts) >= 4:
            try:
                return float(parts[-1]) == 0.0
            except ValueError:
                return False
    return False


def find_literals(code: str) -> list[str]:
    """Colour literals in one line of code (comments already stripped)."""
    out: list[str] = []
    for m in CALL_RE.finditer(code):
        end = _call_end(code, m.end() - 1)
        if end < 0:
            continue
        args = code[m.end():end]
        func = m.group("func")
        literal = code[m.start():end + 1]
        if func in ("rgb", "rgba"):
            raw = args.strip()
            if not raw.lower().startswith("0x"):
                continue
            digits = raw[2:].removesuffix("u32").replace("_", "")
            if not HEX_RE.match(digits) or len(digits) not in (3, 4, 6, 8):
                continue
            if len(digits) in (4, 8) and digits.lower().endswith("00"):
                continue  # fully transparent
        else:
            if not NUM_START_RE.match(args.lstrip()):
                continue
            if _alpha_zero(func, args):
                continue
        out.append(literal)
    for m in HEX_CONST_RE.finditer(code):
        digits = m[1][2:].replace("_", "")
        if len(digits) in (6, 8):
            out.append(m[1])
    return out


def snippet(text: str) -> str:
    flat = " ".join(text.split())
    return flat if len(flat) <= SNIPPET_MAX else flat[: SNIPPET_MAX - 1] + "…"


def added_lines(diff_text: str) -> list[tuple[str, int, str]]:
    """(path, new line number, text) for every added line in a unified diff."""
    rows: list[tuple[str, int, str]] = []
    path: str | None = None
    new_no: int | None = None
    for raw in diff_text.splitlines():
        if raw.startswith("+++ "):
            target = raw[4:].strip()
            if target == "/dev/null":
                path = None
            else:
                path = target[2:] if target.startswith("b/") else target
        elif raw.startswith("@@"):
            m = re.search(r"\+(\d+)(?:,\d+)?", raw)
            new_no = int(m.group(1)) if m else None
        elif raw.startswith("+"):
            if path is not None and new_no is not None:
                rows.append((path, new_no, raw[1:]))
            if new_no is not None:
                new_no += 1
        elif raw.startswith(" ") and new_no is not None:
            new_no += 1
    return rows


def line_findings(path: str, text: str, allow: list[dict], bad_allow: list[dict]) -> tuple[list[str], int]:
    """Literals on one source line and how many were allowlisted; skipped lines yield none."""
    stripped = text.strip()
    if not stripped or stripped.startswith(("//", "/*", "*")):
        return [], 0
    if "assert" in text:
        return [], 0
    kept: list[str] = []
    allowed = 0
    for literal in find_literals(strip_comment(text)):
        if is_allowed(path, text, allow, bad_allow):
            allowed += 1
        else:
            kept.append(literal)
    return kept, allowed


def is_allowed(path: str, text: str, allow: list[dict], bad_allow: list[dict]) -> bool:
    for entry in allow:
        if not isinstance(entry, dict) or entry.get("file") != path:
            continue
        contains = entry.get("contains")
        if not isinstance(contains, str) or not contains:
            continue
        if contains not in text:
            continue
        reason = entry.get("reason")
        if isinstance(reason, str) and reason.strip():
            return True
        if entry not in bad_allow:
            bad_allow.append(entry)
    return False


def load_allowlist() -> list[dict]:
    if not ALLOWLIST_PATH.is_file():
        return []
    try:
        data = json.loads(ALLOWLIST_PATH.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        die(f"invalid JSON in {ALLOWLIST_PATH.relative_to(REPO).as_posix()}: {exc}")
    entries = data.get("allow") if isinstance(data, dict) else None
    if entries is None:
        return []
    if not isinstance(entries, list):
        die(f"allowlist 'allow' is not a list: {ALLOWLIST_PATH.relative_to(REPO).as_posix()}")
    return [e for e in entries if isinstance(e, dict)]


# ---------------------------------------------------------------- modes

def scan_diff(diff_text: str, allow: list[dict]) -> tuple[list[dict], int, list[dict]]:
    findings: list[dict] = []
    allowed = 0
    bad_allow: list[dict] = []
    for path, lineno, text in added_lines(diff_text):
        if not in_scope(path):
            continue
        literals, allowed_here = line_findings(path, text, allow, bad_allow)
        allowed += allowed_here
        for literal in literals:
            findings.append({"file": path, "line": lineno, "literal": literal, "snippet": snippet(text)})
    return findings, allowed, bad_allow


def scan_tree() -> dict[str, int]:
    counts: dict[str, int] = {}
    root = REPO / APP_SRC_REL
    for path in sorted(root.rglob("*.rs")):
        rel = path.relative_to(REPO).as_posix()
        if not in_scope(rel):
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except OSError as exc:
            die(f"cannot read {rel}: {exc}")
        n = 0
        for line in text.splitlines():
            stripped = line.strip()
            if not stripped or stripped.startswith(("//", "/*", "*")) or "assert" in line:
                continue
            n += len(find_literals(strip_comment(line)))
        if n:
            counts[rel] = n
    return counts


def git_diff(ref: str) -> str:
    r = subprocess.run(
        ["git", "diff", "--no-color", "-U0", ref, "--", APP_SRC_REL],
        cwd=REPO,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    if r.returncode != 0:
        die(f"git diff {ref} failed: {(r.stderr or r.stdout).strip()[:300] or f'exit {r.returncode}'}")
    return r.stdout


# ---------------------------------------------------------------- selftest

SAMPLE_DIFF = """diff --git a/fluxer_client/app/src/ui/x.rs b/fluxer_client/app/src/ui/x.rs
--- a/fluxer_client/app/src/ui/x.rs
+++ b/fluxer_client/app/src/ui/x.rs
@@ -10,0 +11,6 @@ fn f() {
+    let a = rgb(0x112233);
+    let b = gpui_kit::hsla(0., 0., 0., 0.);
+    let c = gpui_kit::rgba(0xffffff12).into();
+    let d = theme.background;
+    assert_eq!(h, rgb(0xff0000));
+    // rgb(0xdeadbe) in a comment
diff --git a/fluxer_client/app/src/ui/theme.rs b/fluxer_client/app/src/ui/theme.rs
--- a/fluxer_client/app/src/ui/theme.rs
+++ b/fluxer_client/app/src/ui/theme.rs
@@ -1,0 +2,1 @@
+const T: u32 = 0x2a2730;
diff --git a/fluxer_client/app/src/ui/tests_x.rs b/fluxer_client/app/src/ui/tests_x.rs
--- a/fluxer_client/app/src/ui/tests_x.rs
+++ b/fluxer_client/app/src/ui/tests_x.rs
@@ -1,0 +2,1 @@
+    let t = rgb(0x010203);
diff --git a/scripts/x.py b/scripts/x.py
--- a/scripts/x.py
+++ b/scripts/x.py
@@ -1,0 +2,1 @@
+COLOR = "rgb(0xabcdef)"
diff --git a/fluxer_client/app/src/ui/y.rs b/fluxer_client/app/src/ui/y.rs
--- a/fluxer_client/app/src/ui/y.rs
+++ b/fluxer_client/app/src/ui/y.rs
@@ -20,0 +21,2 @@
+    let e = gpui_kit::hsla(252. / 360., 0.17, 0.80, 0.15);
+    let g = crate::theme::rgb_alpha(0.5);
"""


def selftest() -> None:
    rows = added_lines(SAMPLE_DIFF)
    assert len(rows) == 11, rows
    assert rows[0] == ("fluxer_client/app/src/ui/x.rs", 11, "    let a = rgb(0x112233);"), rows[0]
    assert find_literals("let a = rgb(0x112233);") == ["rgb(0x112233)"]
    assert find_literals("let b = gpui_kit::rgba(0xffffff12).into();") == ["gpui_kit::rgba(0xffffff12)"]
    assert find_literals("hsla(0., 0., 0., 0.)") == []
    assert find_literals("rgba(0x00000000)") == []
    assert find_literals("let x = theme.background;") == []
    assert find_literals("const CARD_BG: u32 = 0x0c_0b_0e;") == ["0x0c_0b_0e"]
    assert find_literals("rgb(0x0c_0b_0e_u32)") == ["rgb(0x0c_0b_0e_u32)"]
    assert find_literals("const FLAG: u32 = 0x0800;") == []
    assert find_literals("hsla(198, calc(92% * 1), 70%, .45)") == ["hsla(198, calc(92% * 1), 70%, .45)"]
    assert find_literals('s = "rgb(0x123456)"') == ["rgb(0x123456)"]
    assert strip_comment('a = "http://x"; b = 1; // rgb(0x1)') == 'a = "http://x"; b = 1; '

    findings, allowed, bad = scan_diff(SAMPLE_DIFF, [])
    got = [(f["file"], f["line"], f["literal"]) for f in findings]
    assert got == [
        ("fluxer_client/app/src/ui/x.rs", 11, "rgb(0x112233)"),
        ("fluxer_client/app/src/ui/x.rs", 13, "gpui_kit::rgba(0xffffff12)"),
        ("fluxer_client/app/src/ui/y.rs", 21, "gpui_kit::hsla(252. / 360., 0.17, 0.80, 0.15)"),
    ], got
    assert allowed == 0 and not bad, (allowed, bad)
    assert findings[0]["snippet"] == "let a = rgb(0x112233);"

    allow = [{"file": "fluxer_client/app/src/ui/x.rs", "contains": "rgba(0xffffff12)", "reason": "official literal"}]
    findings, allowed, bad = scan_diff(SAMPLE_DIFF, allow)
    assert len(findings) == 2 and allowed == 1, (findings, allowed)
    assert find_literals("transparent_black()") == []

    bad_entry = {"file": "fluxer_client/app/src/ui/x.rs", "contains": "rgb(0x112233)", "reason": "  "}
    findings, allowed, bad = scan_diff(SAMPLE_DIFF, [bad_entry])
    assert len(findings) == 3 and allowed == 0 and bad == [bad_entry], (findings, allowed, bad)

    assert in_scope("fluxer_client/app/src/ui/x.rs")
    assert not in_scope("fluxer_client/app/src/ui/theme.rs")
    assert not in_scope("fluxer_client/app/src/ui/theme_tokens_generated.rs")
    assert not in_scope("fluxer_client/app/src/ui/tests_x.rs")
    assert not in_scope("fluxer_client/app/src/ui/x.rs.bak")
    assert not in_scope("fluxer_client/app/tests/x.rs")
    assert snippet("a  b\n c") == "a b c"
    assert len(snippet("x" * 200)) == SNIPPET_MAX
    print("selftest ok (diff lines, literals, transparent/comment/test skips, allowlist)")


# ---------------------------------------------------------------- CLI

def main(argv: list[str] | None = None) -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    parser = argparse.ArgumentParser(
        prog="check_colors.py",
        description="Flag added hard-coded colour literals under fluxer_client/app/src.",
    )
    parser.add_argument("--base", default="HEAD", metavar="REF", help="git ref to diff against (default: HEAD)")
    parser.add_argument("--all", action="store_true", help="scan the whole tree, print per-file counts (exit 0)")
    parser.add_argument("--selftest", action="store_true", help="assert on an in-memory sample diff")
    args = parser.parse_args(argv)

    if args.selftest:
        selftest()
        return 0

    if args.all:
        counts = scan_tree()
        for rel, n in sorted(counts.items()):
            print(f"{rel}  {n}")
        print(f"colors(all): {sum(counts.values())} hard-coded literal(s) in {len(counts)} file(s)")
        return 0

    allow = load_allowlist()
    findings, allowed, bad_allow = scan_diff(git_diff(args.base), allow)
    for entry in bad_allow:
        print(
            f"colors: allowlist entry without reason ignored: {entry.get('file')} / {entry.get('contains')!r}",
            file=sys.stderr,
        )
    for f in findings:
        print(f"{f['file']}:{f['line']}  {f['literal']}  | {f['snippet']}")
    print(f"colors: {len(findings)} new hard-coded literal(s)")
    return 1 if findings else 0


if __name__ == "__main__":
    raise SystemExit(main())
