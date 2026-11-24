"""Ratchet gate for the official-app spec diff (spec_delta.py).

Every scene in spec_baseline.json records a spec path plus the maximum number of deltas and
missing elements it may have. `check` runs spec_delta against one Rust inspect dump and fails
when a scene drifts above those maxima.

    python -I tools/parity_lab/spec_gate.py --selftest
    python -I tools/parity_lab/spec_gate.py [--rust <inspect.json> | --live]
    python -I tools/parity_lab/spec_gate.py --update [--allow-raise] [--rust ... | --live]
    python -I tools/parity_lab/spec_gate.py --init <name> --spec <spec.json> [--rust ... | --live]

check (default) exits 1 when the deltas left after `ignore` exceed max_deltas, or when the
number of missing official flx exceeds max_missing. `missing` counts distinct flx that have no
joined Rust element (the keys of spec_delta's missing_in_rust), not raw element occurrences.

update rewrites max_deltas/max_missing to the current counts only when they went down (ratchet),
and only stamps `updated` when a maximum changed. --allow-raise sets both to the current counts
even when they grew. init adds a scene with the current counts as its initial maxima.
"""

from __future__ import annotations

import argparse
import json
import sys
from datetime import date
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO_ROOT = HERE.parents[1]
DEFAULT_BASELINE = HERE / "spec_baseline.json"

sys.path.insert(0, str(HERE))
import spec_delta  # noqa: E402


# ---------------------------------------------------------------- helpers

def die(msg: str, code: int = 2) -> None:
    print(msg, file=sys.stderr)
    raise SystemExit(code)


def resolve_path(raw: str) -> Path:
    """Accepts absolute paths and paths relative to the cwd, the repo root or this script."""
    path = Path(raw)
    if path.is_absolute():
        return path
    for base in (Path.cwd(), REPO_ROOT, HERE):
        cand = base / path
        if cand.exists():
            return cand
    return Path.cwd() / path


def load_json(path: Path, what: str) -> dict:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except OSError as exc:
        die(f"cannot read {what}: {path} ({exc})")
    except json.JSONDecodeError as exc:
        die(f"invalid JSON in {what}: {path} ({exc})")


def load_baseline(path: Path, required: bool) -> dict:
    if not path.exists():
        if required:
            die(f"baseline not found: {path} (create it with --init <name> --spec <path>)")
        return {"version": 1, "scenes": {}}
    data = load_json(path, "baseline")
    if not isinstance(data, dict) or not isinstance(data.get("scenes"), dict):
        die(f"baseline has no 'scenes' object: {path}")
    return data


def write_baseline(path: Path, data: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def load_rust(rust_path: str | None, live: bool) -> list[dict]:
    """Loads the probe inspect dump, or asks the running app in --live mode only."""
    if live:
        import rust_probe  # noqa: PLC0415  (live mode only: never import it for file mode)

        data = rust_probe.send("probe_inspect")
    else:
        data = load_json(resolve_path(rust_path or ""), "Rust inspect dump")
    if isinstance(data, list):
        elements = data
    elif isinstance(data, dict):
        if data.get("ok") is False:
            die(f"probe_inspect failed: {data.get('error', 'unknown error')}")
        elements = data.get("elements")
    else:
        elements = None
    if not isinstance(elements, list):
        die("Rust inspect dump has no 'elements' list")
    return elements


def load_spec(raw: str) -> dict:
    path = resolve_path(raw)
    if not path.exists():
        die(f"spec not found: {raw} (resolved to {path})")
    spec = load_json(path, "official spec")
    if not isinstance(spec.get("elements"), list):
        die(f"official spec has no 'elements' list: {path}")
    return spec


# ---------------------------------------------------------------- core

def evaluate(scene: dict, spec: dict, rust_elements: list[dict]) -> dict:
    """Runs spec_delta and drops the scene's ignored '<flx>|<prop>' deltas."""
    result = spec_delta.spec_delta(spec, rust_elements)
    ignore = set(scene.get("ignore") or [])
    kept = [d for d in result["deltas"] if f"{d['flx']}|{d['prop']}" not in ignore]
    return {
        "joined": result["joined"],
        "deltas": kept,
        "delta_count": len(kept),
        "ignored": len(result["deltas"]) - len(kept),
        "missing": result["missing_in_rust"],
        "missing_count": len(result["missing_in_rust"]),
    }


def gate_ok(cur: dict, scene: dict) -> bool:
    return cur["delta_count"] <= int(scene["max_deltas"]) and cur["missing_count"] <= int(scene["max_missing"])


def status_line(name: str, cur: dict, scene: dict, ok: bool) -> str:
    return (
        f"{name} deltas={cur['delta_count']}/{scene['max_deltas']} "
        f"missing={cur['missing_count']}/{scene['max_missing']} {'OK' if ok else 'FAIL'}"
    )


def delta_line(d: dict) -> str:
    return f"{d['flx']}  {d['prop']} {d['official']}\u2260{d['rust']}  {d['rule']}  {d['src']}"


def ratchet(scene: dict, delta_count: int, missing_count: int, allow_raise: bool, day: str) -> dict:
    """Returns the updated scene config. Maxima only fall unless allow_raise is set."""
    old_d, old_m = int(scene["max_deltas"]), int(scene["max_missing"])
    new_d = delta_count if allow_raise or delta_count < old_d else old_d
    new_m = missing_count if allow_raise or missing_count < old_m else old_m
    out = dict(scene)
    out["max_deltas"], out["max_missing"] = new_d, new_m
    changed = (new_d, new_m) != (old_d, old_m)
    if changed:
        out["updated"] = day
    return {
        "scene": out,
        "changed": changed,
        "old": (old_d, old_m),
        "new": (new_d, new_m),
        "blocked": not allow_raise and (delta_count > old_d or missing_count > old_m),
    }


# ---------------------------------------------------------------- modes

def run_check(baseline: dict, rust_elements: list[dict]) -> int:
    scenes = baseline.get("scenes") or {}
    if not scenes:
        die("baseline has no scenes; add one with --init <name> --spec <path>")
    failed = 0
    for name, scene in scenes.items():
        cur = evaluate(scene, load_spec(scene["spec"]), rust_elements)
        ok = gate_ok(cur, scene)
        print(status_line(name, cur, scene, ok))
        if ok:
            continue
        failed += 1
        if cur["delta_count"] > int(scene["max_deltas"]):
            for d in cur["deltas"]:
                print("  " + delta_line(d))
        if cur["missing_count"] > int(scene["max_missing"]):
            over = ", ".join(f"{f} x{n}" for f, n in sorted(cur["missing"].items()))
            print(f"  missing_in_rust {cur['missing_count']} > {scene['max_missing']}: {over}")
    return 1 if failed else 0


def run_update(baseline: dict, baseline_path: Path, rust_elements: list[dict], allow_raise: bool) -> int:
    scenes = baseline.get("scenes") or {}
    if not scenes:
        die("baseline has no scenes; add one with --init <name> --spec <path>")
    day = date.today().isoformat()
    changed_any = False
    for name, scene in scenes.items():
        cur = evaluate(scene, load_spec(scene["spec"]), rust_elements)
        res = ratchet(scene, cur["delta_count"], cur["missing_count"], allow_raise, day)
        (old_d, old_m), (new_d, new_m) = res["old"], res["new"]
        state = "lowered" if res["changed"] else ("blocked" if res["blocked"] else "unchanged")
        print(
            f"{name} deltas={cur['delta_count']} max {old_d}->{new_d} "
            f"missing={cur['missing_count']} max {old_m}->{new_m} {state}"
        )
        if res["changed"]:
            scenes[name] = res["scene"]
            changed_any = True
    if changed_any:
        write_baseline(baseline_path, baseline)
        print(f"wrote {baseline_path}")
    else:
        print("baseline unchanged (ratchet does not raise maxima; pass --allow-raise to do so)")
    return 0


def run_init(baseline: dict, baseline_path: Path, name: str, spec_path: str, ignore: list[str],
             rust_elements: list[dict]) -> int:
    scenes = baseline.setdefault("scenes", {})
    if name in scenes:
        die(f"scene already exists: {name} (edit spec_baseline.json or use --update)")
    scene = {
        "spec": Path(spec_path).as_posix(),
        "max_deltas": 0,
        "max_missing": 0,
        "ignore": list(ignore),
        "updated": date.today().isoformat(),
    }
    cur = evaluate(scene, load_spec(spec_path), rust_elements)
    scene["max_deltas"], scene["max_missing"] = cur["delta_count"], cur["missing_count"]
    scenes[name] = scene
    write_baseline(baseline_path, baseline)
    print(
        f"added {name}: deltas={cur['delta_count']} missing={cur['missing_count']} "
        f"ignored={cur['ignored']} -> {baseline_path}"
    )
    return 0


# ---------------------------------------------------------------- selftest

def _synthetic_pair() -> tuple[dict, list[dict]]:
    """3 aligned elements; gate.b is 4px taller in Rust. All styles empty, so rects only."""
    boxes = [("gate.a", 0), ("gate.b", 20), ("gate.c", 40)]
    spec = {
        "elements": [
            {
                "flx": flx,
                "idx": 0,
                "rect": [0, top, 40, 20],
                "computed": {},
                "css_rules": [],
                "floating": False,
                "parent_flx": None,
                "rel": None,
                "in_viewport": True,
            }
            for flx, top in boxes
        ]
    }
    rust = [
        {
            "flx": flx,
            "rect": [0, top, 40, 24 if flx == "gate.b" else 20],
            "src": f"synthetic.rs:{i + 1}",
            "style": {},
        }
        for i, (flx, top) in enumerate(boxes)
    ]
    return spec, rust


def selftest() -> None:
    spec, rust = _synthetic_pair()
    scene = {"spec": "synthetic", "max_deltas": 1, "max_missing": 0, "ignore": [], "updated": "2026-01-01"}

    cur = evaluate(scene, spec, rust)
    assert cur["delta_count"] == 1 and cur["missing_count"] == 0 and cur["joined"] == 3, cur
    assert gate_ok(cur, scene)  # pass: at the limit
    assert status_line("gate", cur, scene, True) == "gate deltas=1/1 missing=0/0 OK"
    dline = delta_line(cur["deltas"][0])
    assert dline == "gate.b  height 20\u226024  [computed]  fluxer_client/synthetic.rs:2", dline

    strict = dict(scene, max_deltas=0)
    assert not gate_ok(cur, strict)  # fail: over the delta limit
    assert status_line("gate", cur, strict, False) == "gate deltas=1/0 missing=0/0 FAIL"

    ignored = dict(scene, max_deltas=0, ignore=["gate.b|height"])
    cur_ignore = evaluate(ignored, spec, rust)
    assert cur_ignore["delta_count"] == 0 and cur_ignore["ignored"] == 1, cur_ignore
    assert gate_ok(cur_ignore, ignored)
    for near in ("gate.b|width", "gate.b|heigh", "gate.c|height", "gate.b|height "):
        assert evaluate(dict(scene, ignore=[near]), spec, rust)["delta_count"] == 1, near

    spec_missing = {"elements": spec["elements"] + [dict(spec["elements"][0], flx="gate.d", rect=[0, 60, 40, 20])]}
    cur_missing = evaluate(scene, spec_missing, rust)
    assert cur_missing["missing_count"] == 1 and cur_missing["missing"] == {"gate.d": 1}, cur_missing
    assert not gate_ok(cur_missing, scene)  # fail: max_missing is 0

    base = {"max_deltas": 5, "max_missing": 3, "updated": "2026-01-01"}
    down = ratchet(base, delta_count=2, missing_count=1, allow_raise=False, day="2026-11-25")
    assert (down["new"], down["changed"]) == ((2, 1), True), down
    assert down["scene"]["updated"] == "2026-11-25"
    blocked = ratchet(base, delta_count=7, missing_count=9, allow_raise=False, day="2026-11-25")
    assert (blocked["new"], blocked["changed"], blocked["blocked"]) == ((5, 3), False, True), blocked
    assert blocked["scene"]["updated"] == "2026-01-01"
    raised = ratchet(base, delta_count=7, missing_count=0, allow_raise=True, day="2026-11-25")
    assert (raised["new"], raised["changed"]) == ((7, 0), True), raised
    equal = ratchet(base, delta_count=5, missing_count=3, allow_raise=False, day="2026-11-25")
    assert (equal["new"], equal["changed"]) == ((5, 3), False), equal
    assert equal["scene"]["updated"] == "2026-01-01"
    print("selftest ok (pass/fail/ignore/ratchet)")


# ---------------------------------------------------------------- CLI

def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="spec_gate.py",
        description="Ratchet gate for official-spec vs Rust probe deltas (tools/parity_lab/spec_baseline.json).",
        epilog=(
            "examples:\n"
            "  python -I tools/parity_lab/spec_gate.py --rust target/parity-tmp/rs.json\n"
            "  python -I tools/parity_lab/spec_gate.py --live\n"
            "  python -I tools/parity_lab/spec_gate.py --update --rust target/parity-tmp/rs.json\n"
            "  python -I tools/parity_lab/spec_gate.py --init markdown --spec target/parity-tmp/md.1280.spec.json --rust target/parity-tmp/rs.json\n"
            "  python -I tools/parity_lab/spec_gate.py --selftest"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p.add_argument("--baseline", default=str(DEFAULT_BASELINE), help="baseline JSON (default: tools/parity_lab/spec_baseline.json)")
    p.add_argument("--rust", metavar="INSPECT_JSON", help="Rust probe inspect dump {elements:[...]}")
    p.add_argument("--live", action="store_true", help="ask the running app via rust_probe.send('probe_inspect')")
    p.add_argument("--update", action="store_true", help="lower recorded maxima to the current counts (ratchet)")
    p.add_argument("--allow-raise", action="store_true", help="with --update: also raise maxima to the current counts")
    p.add_argument("--init", metavar="NAME", help="add a scene with the current counts as maxima")
    p.add_argument("--spec", metavar="SPEC_JSON", help="official spec JSON path (required with --init)")
    p.add_argument("--ignore", action="append", default=[], metavar="FLX|PROP", help="with --init: ignore entry (repeatable)")
    p.add_argument("--selftest", action="store_true", help="run the synthetic pass/fail/ignore/ratchet asserts")
    return p


def main(argv: list[str] | None = None) -> int:
    sys.stdout.reconfigure(encoding="utf-8")
    parser = build_parser()
    args = parser.parse_args(argv)

    if args.selftest:
        selftest()
        return 0
    if args.update and args.init:
        parser.error("--update and --init are mutually exclusive")
    if args.allow_raise and not args.update:
        parser.error("--allow-raise requires --update")
    if args.rust and args.live:
        parser.error("--rust and --live are mutually exclusive")
    if not (args.rust or args.live):
        parser.error("a Rust dump is required: pass --rust <inspect.json> or --live")
    if args.init and not args.spec:
        parser.error("--init requires --spec <spec.json>")
    if args.spec and not args.init:
        parser.error("--spec is only used with --init")

    baseline_path = Path(args.baseline)
    baseline = load_baseline(baseline_path, required=not args.init)
    rust_elements = load_rust(args.rust, args.live)

    if args.init:
        return run_init(baseline, baseline_path, args.init, args.spec, args.ignore, rust_elements)
    if args.update:
        return run_update(baseline, baseline_path, rust_elements, args.allow_raise)
    return run_check(baseline, rust_elements)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
