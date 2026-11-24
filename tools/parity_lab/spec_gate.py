"""Ratchet gate for the official-app spec diff (spec_delta.py).

Every scene in spec_baseline.json records a spec path plus the maximum number of deltas and
missing elements it may have. `check` runs spec_delta against one Rust inspect dump and fails
when a scene drifts above those maxima.

`--scenes-gate` gates the opened-state sets instead ("scene_sets" in the baseline): each set
drives spec_delta's scene mode (over the `scenes` json, capturing against `spec_dir`) and records
a maximum per slug in `max`. `--all` gates both the current screen and the scene sets.

    python -I tools/parity_lab/spec_gate.py --selftest
    python -I tools/parity_lab/spec_gate.py [--rust <inspect.json> | --live]
    python -I tools/parity_lab/spec_gate.py --scenes-gate --live
    python -I tools/parity_lab/spec_gate.py --scenes-gate --from-dir <spec_delta scenes out dir>
    python -I tools/parity_lab/spec_gate.py --all --live
    python -I tools/parity_lab/spec_gate.py --update [--allow-raise] [--rust ... | --live | --scenes-gate ...]
    python -I tools/parity_lab/spec_gate.py --init <name> --spec <spec.json> [--rust ... | --live]

check (default) exits 1 when the deltas left after `ignore` exceed max_deltas, or when the
number of missing official flx exceeds max_missing. `missing` counts distinct flx that have no
joined Rust element (the keys of spec_delta's missing_in_rust), not raw element occurrences.

Scene sets are compared per slug after their own `ignore` entries ('<slug>|<flx>|<prop>'):
'<set>/<slug> deltas=a/max missing=b/max OK|FAIL', and a scene whose status is not ok is FAIL.
A slug that is absent from `max` counts as 0/0, so an uninitialised set fails the gate.

update rewrites maxima to the current counts only when they went down (ratchet), and only stamps
`updated` when a maximum changed. --allow-raise sets them to the current counts even when they
grew, and is also how a brand-new scene set (`"max": {}`) or a new slug gets its first maxima.
init adds a markdown scene with the current counts as its initial maxima.
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
TMP_ROOT = REPO_ROOT / "target" / "parity-tmp"

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
        return {"version": 1, "scenes": {}, "scene_sets": {}}
    data = load_json(path, "baseline")
    if not isinstance(data, dict):
        die(f"baseline is not a JSON object: {path}")
    if data.get("scenes") is None and data.get("scene_sets") is None:
        die(f"baseline has neither 'scenes' nor 'scene_sets': {path}")
    for key in ("scenes", "scene_sets"):
        if data.get(key) is not None and not isinstance(data[key], dict):
            die(f"baseline '{key}' is not an object: {path}")
        data.setdefault(key, {})
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


# ---------------------------------------------------------------- scene sets

def validate_scene_set(name: str, cfg: dict) -> None:
    if not isinstance(cfg, dict):
        die(f"scene set '{name}' is not an object")
    if not isinstance(cfg.get("scenes"), str) or not cfg["scenes"]:
        die(f"scene set '{name}' has no 'scenes' path")
    if cfg.get("max") is not None and not isinstance(cfg["max"], dict):
        die(f"scene set '{name}' has a non-object 'max'")


def scene_ignores(name: str, entries: list | None) -> dict[str, set[tuple[str, str]]]:
    """'<slug>|<flx>|<prop>' -> {slug: {(flx, prop)}} for one set."""
    out: dict[str, set[tuple[str, str]]] = {}
    for raw in entries or []:
        parts = [p.strip() for p in str(raw).split("|")]
        if len(parts) != 3 or not all(parts):
            die(f"scene set '{name}': ignore entry must be '<slug>|<flx>|<prop>': {raw!r}")
        out.setdefault(parts[0], set()).add((parts[1], parts[2]))
    return out


def scene_dir_for(name: str, from_dir: Path) -> Path:
    """Offline source dir: <from-dir>/<set>/summary.json when present, else <from-dir> itself."""
    sub = from_dir / name
    return sub if (sub / "summary.json").exists() else from_dir


def read_scene_rows(out_dir: Path, name: str) -> list[dict]:
    path = out_dir / "summary.json"
    if not path.exists():
        die(f"scene set '{name}': no scene output at {out_dir} (expected summary.json)")
    rows = load_json(path, "scene summary")
    if not isinstance(rows, list):
        die(f"scene set '{name}': summary.json is not a list: {path}")
    return rows


def scene_counts(out_dir: Path, slug: str, ignore: set[tuple[str, str]]) -> dict:
    """Reads one <slug>.json from spec_delta's scene mode and drops the set's ignored deltas."""
    path = out_dir / f"{slug}.json"
    if not path.exists():
        die(f"scene '{slug}': no {slug}.json next to summary.json in {out_dir}")
    res = load_json(path, f"scene delta for {slug}")
    all_deltas = res.get("deltas") or []
    kept = [d for d in all_deltas if (d.get("flx"), d.get("prop")) not in ignore]
    missing = res.get("missing_in_rust") or {}
    return {
        "deltas": kept,
        "delta_count": len(kept),
        "ignored": len(all_deltas) - len(kept),
        "missing": missing,
        "missing_count": len(missing),
    }


def drive_scene_set(name: str, cfg: dict) -> Path:
    """Runs spec_delta's scene mode for one set into target/parity-tmp/gate_<name>."""
    scenes = resolve_path(str(cfg["scenes"]))
    if not scenes.exists():
        die(f"scene set '{name}': scenes file not found: {cfg['scenes']} (resolved to {scenes})")
    if not cfg.get("spec_dir"):
        die(f"scene set '{name}': needs 'spec_dir' for --live")
    spec_dir = resolve_path(str(cfg["spec_dir"]))
    out_dir = TMP_ROOT / f"gate_{name}"
    out_dir.mkdir(parents=True, exist_ok=True)
    spec_delta.run_scenes(
        ["--scenes", str(scenes), "--spec-dir", str(spec_dir), "--live", "--out-dir", str(out_dir)]
    )
    return out_dir


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


def ratchet_pair(old: tuple[int, int], cur: tuple[int, int], allow_raise: bool) -> tuple[tuple[int, int], bool, bool]:
    """(new maxima, changed, blocked) for one recorded (deltas, missing) pair."""
    new = (
        cur[0] if allow_raise or cur[0] < old[0] else old[0],
        cur[1] if allow_raise or cur[1] < old[1] else old[1],
    )
    return new, new != old, not allow_raise and (cur[0] > old[0] or cur[1] > old[1])


def ratchet(scene: dict, delta_count: int, missing_count: int, allow_raise: bool, day: str) -> dict:
    """Returns the updated scene config. Maxima only fall unless allow_raise is set."""
    old = (int(scene["max_deltas"]), int(scene["max_missing"]))
    new, changed, blocked = ratchet_pair(old, (delta_count, missing_count), allow_raise)
    out = dict(scene)
    out["max_deltas"], out["max_missing"] = new
    if changed:
        out["updated"] = day
    return {
        "scene": out,
        "changed": changed,
        "old": old,
        "new": new,
        "blocked": blocked,
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


def run_scene_check(baseline: dict, dirs: dict[str, Path]) -> int:
    """Gates every slug of every scene set against its recorded maxima."""
    sets = baseline.get("scene_sets") or {}
    if not sets:
        die("baseline has no scene_sets; add one to spec_baseline.json")
    failed = 0
    for name, cfg in sets.items():
        validate_scene_set(name, cfg)
        out_dir = dirs[name]
        ignore = scene_ignores(name, cfg.get("ignore"))
        maxima = cfg.get("max") or {}
        for row in read_scene_rows(out_dir, name):
            slug = str(row.get("scene") or "?")
            known = row.get("status") == "ok"
            cur = scene_counts(out_dir, slug, ignore.get(slug, set())) if known else {
                "deltas": [], "delta_count": 0, "ignored": 0, "missing": {}, "missing_count": 0,
            }
            mx = maxima.get(slug) or {}
            max_d, max_m = int(mx.get("deltas", 0)), int(mx.get("missing", 0))
            ok = known and cur["delta_count"] <= max_d and cur["missing_count"] <= max_m
            print(
                f"{name}/{slug} deltas={cur['delta_count']}/{max_d} "
                f"missing={cur['missing_count']}/{max_m} {'OK' if ok else 'FAIL'}"
            )
            if ok:
                continue
            failed += 1
            if not known:
                print(f"  scene status={row.get('status')}: {row.get('error') or 'no error reported'}")
                continue
            if slug not in maxima:
                print(f"  no maxima for '{slug}' (untracked; run --update --allow-raise)")
            if cur["delta_count"] > max_d:
                for d in cur["deltas"]:
                    print("  " + delta_line(d))
            if cur["missing_count"] > max_m:
                over = ", ".join(f"{f} x{n}" for f, n in sorted(cur["missing"].items()))
                print(f"  missing_in_rust {cur['missing_count']} > {max_m}: {over}")
    return 1 if failed else 0


def run_scene_update(baseline: dict, baseline_path: Path, dirs: dict[str, Path], allow_raise: bool) -> int:
    """Ratchets per-slug maxima down (or to the current counts with --allow-raise)."""
    sets = baseline.get("scene_sets") or {}
    if not sets:
        die("baseline has no scene_sets; add one to spec_baseline.json")
    day = date.today().isoformat()
    changed_any = False
    for name, cfg in sets.items():
        validate_scene_set(name, cfg)
        out_dir = dirs[name]
        ignore = scene_ignores(name, cfg.get("ignore"))
        maxima = cfg.setdefault("max", {})
        set_changed = False
        for row in read_scene_rows(out_dir, name):
            slug = str(row.get("scene") or "?")
            if row.get("status") != "ok":
                print(f"{name}/{slug} skipped (status={row.get('status')}: {row.get('error') or ''})")
                continue
            cur = scene_counts(out_dir, slug, ignore.get(slug, set()))
            counts = (cur["delta_count"], cur["missing_count"])
            if slug not in maxima:
                maxima[slug] = {"deltas": counts[0], "missing": counts[1]}
                set_changed = True
                print(
                    f"{name}/{slug} deltas={counts[0]} max new->{counts[0]} "
                    f"missing={counts[1]} max new->{counts[1]} added"
                )
                continue
            old = (int(maxima[slug].get("deltas", 0)), int(maxima[slug].get("missing", 0)))
            new, changed, blocked = ratchet_pair(old, counts, allow_raise)
            state = "lowered" if changed else ("blocked" if blocked else "unchanged")
            if changed:
                maxima[slug] = dict(maxima[slug], deltas=new[0], missing=new[1])
                set_changed = True
            print(
                f"{name}/{slug} deltas={counts[0]} max {old[0]}->{new[0]} "
                f"missing={counts[1]} max {old[1]}->{new[1]} {state}"
            )
        if set_changed:
            cfg["max"] = dict(sorted(maxima.items()))
            cfg["updated"] = day
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


def _scene_fixture(root: Path) -> Path:
    """Offline spec_delta scene output: alpha/beta ok, gamma failed (no <slug>.json written)."""
    root.mkdir(parents=True, exist_ok=True)
    rows = [
        {"scene": "alpha", "joined": 3, "deltas": 2, "missing": 1, "status": "ok", "error": None},
        {"scene": "beta", "joined": 2, "deltas": 1, "missing": 0, "status": "ok", "error": None},
        {"scene": "gamma", "joined": 0, "deltas": 0, "missing": 0, "status": "failed", "error": "expect_flx not shown"},
    ]
    (root / "summary.json").write_text(json.dumps(rows), encoding="utf-8")
    alpha = {
        "joined": 3,
        "delta_count": 2,
        "deltas": [
            {"flx": "s.alpha", "idx": 0, "prop": "height", "official": "20", "rust": "24",
             "rule": "[computed]", "src": "fluxer_client/synthetic.rs:1"},
            {"flx": "s.alpha", "idx": 1, "prop": "background", "official": "#000000ff", "rust": "#ffffffff",
             "rule": "[computed]", "src": "fluxer_client/synthetic.rs:2"},
        ],
        "missing_in_rust": {"s.alpha.ghost": 1},
    }
    beta = {
        "joined": 2,
        "delta_count": 1,
        "deltas": [
            {"flx": "s.beta", "idx": 0, "prop": "width", "official": "40", "rust": "41",
             "rule": "[computed]", "src": "fluxer_client/synthetic.rs:3"},
        ],
        "missing_in_rust": {},
    }
    (root / "alpha.json").write_text(json.dumps(alpha), encoding="utf-8")
    (root / "beta.json").write_text(json.dumps(beta), encoding="utf-8")
    return root


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

    # ------------------------------------------------------------ scene sets (offline)
    import contextlib  # noqa: PLC0415
    import io  # noqa: PLC0415
    import shutil  # noqa: PLC0415

    def capture(fn, *a, **kw):
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            code = fn(*a, **kw)
        return code, buf.getvalue().splitlines()

    def scene_bl(maxima: dict, ignore: list[str] | None = None) -> dict:
        cfg = {
            "scenes": "synthetic_scenes.json",
            "spec_dir": "synthetic_spec",
            "max": maxima,
            "ignore": list(ignore or []),
            "updated": "2026-01-01",
        }
        return {"version": 1, "scenes": {}, "scene_sets": {"blue": cfg}}

    at_limit = {"alpha": {"deltas": 2, "missing": 1}, "beta": {"deltas": 1, "missing": 0},
                "gamma": {"deltas": 0, "missing": 0}}
    tmp = TMP_ROOT / "selftest_gate"
    shutil.rmtree(tmp, ignore_errors=True)
    try:
        _scene_fixture(tmp)
        dirs = {"blue": tmp}

        # pass: every ok scene sits at its maximum; a non-ok scene is FAIL
        code, lines = capture(run_scene_check, scene_bl(at_limit), dirs)
        assert code == 1, lines
        assert "blue/alpha deltas=2/2 missing=1/1 OK" in lines, lines
        assert "blue/beta deltas=1/1 missing=0/0 OK" in lines, lines
        assert "blue/gamma deltas=0/0 missing=0/0 FAIL" in lines, lines
        assert any("scene status=failed: expect_flx not shown" in ln for ln in lines), lines

        # fail: a count above the maximum prints the offending deltas
        strict = at_limit | {"alpha": {"deltas": 1, "missing": 0}}
        code, lines = capture(run_scene_check, scene_bl(strict), dirs)
        assert code == 1 and "blue/alpha deltas=2/1 missing=1/0 FAIL" in lines, lines
        assert any("s.alpha  height 20\u226024" in ln for ln in lines), lines
        assert any("missing_in_rust 1 > 0: s.alpha.ghost x1" in ln for ln in lines), lines

        # ignore: '<slug>|<flx>|<prop>' drops only exact matches
        ignored = at_limit | {"alpha": {"deltas": 0, "missing": 1}}
        code, lines = capture(
            run_scene_check,
            scene_bl(ignored, ["alpha|s.alpha|height", "alpha|s.alpha|background"]),
            dirs,
        )
        assert code == 1 and "blue/alpha deltas=0/0 missing=1/1 OK" in lines, lines
        exact = scene_ignores("blue", ["alpha|s.alpha|height"])["alpha"]
        assert scene_counts(tmp, "alpha", exact)["delta_count"] == 1
        near = scene_ignores("blue", ["alpha|s.alpha|heigh"])["alpha"]
        assert scene_counts(tmp, "alpha", near)["delta_count"] == 2
        try:
            with contextlib.redirect_stderr(io.StringIO()):
                scene_ignores("blue", ["alpha|height"])
        except SystemExit as exc:
            assert exc.code == 2, exc.code
        else:
            raise AssertionError("bad ignore entry must die")

        # ratchet-down: lowers maxima and stamps updated; failed scenes are skipped
        down_bl = scene_bl(at_limit | {"alpha": {"deltas": 5, "missing": 4}})
        down_path = tmp / "selftest_baseline.json"
        code, lines = capture(run_scene_update, down_bl, down_path, dirs, False)
        assert code == 0, lines
        assert "blue/alpha deltas=2 max 5->2 missing=1 max 4->1 lowered" in lines, lines
        assert "blue/beta deltas=1 max 1->1 missing=0 max 0->0 unchanged" in lines, lines
        assert any("blue/gamma skipped (status=failed" in ln for ln in lines), lines
        assert down_bl["scene_sets"]["blue"]["max"]["alpha"] == {"deltas": 2, "missing": 1}, down_bl
        assert down_bl["scene_sets"]["blue"]["updated"] == date.today().isoformat()
        assert json.loads(down_path.read_text(encoding="utf-8"))["scene_sets"]["blue"]["max"]["alpha"] == {
            "deltas": 2, "missing": 1,
        }

        # ratchet does not raise: an over-maximum run is blocked and writes nothing
        blocked_bl = scene_bl(at_limit | {"alpha": {"deltas": 1, "missing": 1}})
        blocked_path = tmp / "blocked.json"
        code, lines = capture(run_scene_update, blocked_bl, blocked_path, dirs, False)
        assert code == 0 and not blocked_path.exists(), lines
        assert "blue/alpha deltas=2 max 1->1 missing=1 max 1->1 blocked" in lines, lines

        # untracked slugs count as 0/0 until --update --allow-raise records them
        empty_bl = scene_bl({})
        code, lines = capture(run_scene_check, empty_bl, dirs)
        assert code == 1 and "blue/alpha deltas=2/0 missing=1/0 FAIL" in lines, lines
        assert any("no maxima for 'alpha'" in ln for ln in lines), lines
        raised_path = tmp / "raised.json"
        code, lines = capture(run_scene_update, empty_bl, raised_path, dirs, True)
        assert code == 0, lines
        assert empty_bl["scene_sets"]["blue"]["max"] == {
            "alpha": {"deltas": 2, "missing": 1},
            "beta": {"deltas": 1, "missing": 0},
        }, empty_bl
        code, lines = capture(run_scene_check, empty_bl, dirs)
        assert code == 1 and "blue/alpha deltas=2/2 missing=1/1 OK" in lines, lines
    finally:
        shutil.rmtree(tmp, ignore_errors=True)

    print("selftest ok (pass/fail/ignore/ratchet + scene-set pass/fail/ignore/ratchet-down)")


# ---------------------------------------------------------------- CLI

def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="spec_gate.py",
        description="Ratchet gate for official-spec vs Rust probe deltas (tools/parity_lab/spec_baseline.json).",
        epilog=(
            "examples:\n"
            "  python -I tools/parity_lab/spec_gate.py --rust target/parity-tmp/rs.json\n"
            "  python -I tools/parity_lab/spec_gate.py --live\n"
            "  python -I tools/parity_lab/spec_gate.py --scenes-gate --live\n"
            "  python -I tools/parity_lab/spec_gate.py --scenes-gate --from-dir target/parity-tmp/gate_popouts\n"
            "  python -I tools/parity_lab/spec_gate.py --all --live\n"
            "  python -I tools/parity_lab/spec_gate.py --update --rust target/parity-tmp/rs.json\n"
            "  python -I tools/parity_lab/spec_gate.py --update --allow-raise --scenes-gate --live\n"
            "  python -I tools/parity_lab/spec_gate.py --init markdown --spec target/parity-tmp/md.1280.spec.json --rust target/parity-tmp/rs.json\n"
            "  python -I tools/parity_lab/spec_gate.py --selftest"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p.add_argument("--baseline", default=str(DEFAULT_BASELINE), help="baseline JSON (default: tools/parity_lab/spec_baseline.json)")
    p.add_argument("--rust", metavar="INSPECT_JSON", help="Rust probe inspect dump {elements:[...]}")
    p.add_argument("--live", action="store_true", help="ask the running app via rust_probe.send('probe_inspect')")
    p.add_argument("--scenes-gate", action="store_true", help="gate the baseline 'scene_sets' slugs instead of the current screen")
    p.add_argument("--all", action="store_true", help="gate the current screen and the scene sets (implies --scenes-gate)")
    p.add_argument("--from-dir", metavar="DIR", help="with --scenes-gate: read an existing spec_delta scenes output dir instead of driving the app")
    p.add_argument("--update", action="store_true", help="lower recorded maxima to the current counts (ratchet)")
    p.add_argument("--allow-raise", action="store_true", help="with --update: also raise maxima to the current counts (initialises new sets/slugs)")
    p.add_argument("--init", metavar="NAME", help="add a markdown scene with the current counts as maxima")
    p.add_argument("--spec", metavar="SPEC_JSON", help="official spec JSON path (required with --init)")
    p.add_argument("--ignore", action="append", default=[], metavar="FLX|PROP", help="with --init: ignore entry (repeatable)")
    p.add_argument("--selftest", action="store_true", help="run the synthetic pass/fail/ignore/ratchet asserts")
    return p


def main(argv: list[str] | None = None) -> int:
    sys.stdout.reconfigure(encoding="utf-8")
    parser = build_parser()
    args = parser.parse_args(argv)
    scenes_gate = bool(args.scenes_gate or args.all)

    if args.selftest:
        selftest()
        return 0
    if args.update and args.init:
        parser.error("--update and --init are mutually exclusive")
    if args.allow_raise and not args.update:
        parser.error("--allow-raise requires --update")
    if args.rust and args.live:
        parser.error("--rust and --live are mutually exclusive")
    if args.from_dir and args.live:
        parser.error("--from-dir reads an existing scenes output; it cannot be combined with --live")
    if args.from_dir and not scenes_gate:
        parser.error("--from-dir requires --scenes-gate (or --all)")
    if scenes_gate and not (args.live or args.from_dir):
        parser.error("--scenes-gate requires --live or --from-dir <dir>")
    wants_screen = not scenes_gate or args.all
    if wants_screen and not (args.rust or args.live):
        parser.error("a Rust dump is required: pass --rust <inspect.json> or --live")
    if args.init and scenes_gate:
        parser.error("--init adds markdown scenes; initialise a scene set with --update --allow-raise")
    if args.init and not args.spec:
        parser.error("--init requires --spec <spec.json>")
    if args.spec and not args.init:
        parser.error("--spec is only used with --init")

    baseline_path = Path(args.baseline)
    baseline = load_baseline(baseline_path, required=not args.init)

    scene_dirs: dict[str, Path] = {}
    if scenes_gate:
        sets = baseline.get("scene_sets") or {}
        if not sets:
            die("baseline has no scene_sets; add one to spec_baseline.json")
        for name, cfg in sets.items():
            validate_scene_set(name, cfg)

    status = 0
    if wants_screen:
        rust_elements = load_rust(args.rust, args.live)
        if args.init:
            return run_init(baseline, baseline_path, args.init, args.spec, args.ignore, rust_elements)
        if args.update:
            status |= run_update(baseline, baseline_path, rust_elements, args.allow_raise)
        else:
            status |= run_check(baseline, rust_elements)

    if scenes_gate:
        for name, cfg in baseline["scene_sets"].items():
            if args.live:
                scene_dirs[name] = drive_scene_set(name, cfg)
            else:
                scene_dirs[name] = scene_dir_for(name, Path(args.from_dir))
        if args.update:
            status |= run_scene_update(baseline, baseline_path, scene_dirs, args.allow_raise)
        else:
            status |= run_scene_check(baseline, scene_dirs)

    return 1 if status else 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
