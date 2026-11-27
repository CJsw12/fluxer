#!/usr/bin/env python3
"""Manage Fluxer parity batch sessions and audit completed batches."""
from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8")

REPO = Path(__file__).resolve().parents[2]
STATE_PATH = REPO / "Logs" / "handoff" / "DIRECTOR_STATE.json"
RETRO_PATH = REPO / "Logs" / "handoff" / "worker_retro.md"
VERIFY_ROOT = REPO / "target" / "parity-tmp" / "verify"
USAGE_SCRIPT = Path.home() / ".agent-rules" / "usage.py"
ORCA_BIN = os.environ.get("ORCA_CLI_COMMAND") or shutil.which("orca") or "orca"
ROTATION_PROMPT = STATE_PATH.with_name("DIRECTOR_ROTATE_PROMPT.txt")


def now_utc() -> dt.datetime:
    return dt.datetime.now(dt.timezone.utc)


def iso_utc(ts: dt.datetime | None = None) -> str:
    return (ts or now_utc()).strftime("%Y-%m-%dT%H:%M:%SZ")


def load_state() -> dict:
    if not STATE_PATH.exists():
        raise SystemExit(f"ERROR: missing state file {STATE_PATH}")
    return json.loads(STATE_PATH.read_text(encoding="utf-8"))


def save_state(state: dict) -> None:
    state["updated_utc"] = iso_utc()
    STATE_PATH.write_text(
        json.dumps(state, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )


def run_cmd(cmd: list[str], timeout: int = 30) -> tuple[int, str]:
    try:
        p = subprocess.run(
            cmd,
            cwd=str(REPO),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
        )
        return p.returncode, (p.stdout + ("\n" + p.stderr if p.stderr else "")).strip()
    except Exception as exc:
        return 1, f"EXCEPTION: {exc}"


def parse_usage() -> tuple[str, dict]:
    if not USAGE_SCRIPT.exists():
        return "usage.py not found", {}
    rc, out = run_cmd([sys.executable, "-I", str(USAGE_SCRIPT)], timeout=25)
    info: dict[str, int] = {}
    m_cw = re.search(r"Codex\s*:.*?week\s+(\d+)%\s+left", out)
    m_c5 = re.search(r"Codex\s*:.*?5h\s+(\d+)%\s+left", out)
    m_gw = re.search(r"Gemini Models Weekly Limit Remaining\s+(\d+)%", out)
    m_g5 = re.search(r"Gemini Models Five Hour Limit Remaining\s+(\d+)%", out)
    m_clw = re.search(r"Claude\s*:.*?week\s+(\d+)%\s+left", out)
    m_cl5 = re.search(r"Claude\s*:.*?5h\s+(\d+)%\s+left", out)
    if m_cw:
        info["codex_week"] = int(m_cw.group(1))
    if m_c5:
        info["codex_5h"] = int(m_c5.group(1))
    if m_gw:
        info["gemini_week"] = int(m_gw.group(1))
    if m_g5:
        info["gemini_5h"] = int(m_g5.group(1))
    if m_clw:
        info["claude_week"] = int(m_clw.group(1))
    if m_cl5:
        info["claude_5h"] = int(m_cl5.group(1))
    return out, info


def load_blocked_providers() -> dict:
    """Providers known to be rate-limited (DIRECTOR_STATE.json provider_blocked_until)."""
    blocked = {}
    try:
        for ag_name, until in (load_state().get("provider_blocked_until") or {}).items():
            if now_utc() < dt.datetime.fromisoformat(until.replace("Z", "+00:00")):
                blocked[ag_name] = until
    except Exception:
        return {}
    return blocked


def pick_best_director(u: dict, blocked: dict | None = None) -> tuple[str, str, str, str]:
    """Return (agent, model, effort, reason) picking the provider with most headroom."""
    codex_score = min(u.get("codex_week", -1), u.get("codex_5h", -1))
    gemini_score = min(u.get("gemini_week", -1), u.get("gemini_5h", -1))
    claude_score = min(u.get("claude_week", -1), u.get("claude_5h", -1))
    # usage.py can report stale headroom after a provider hit its session limit;
    # DIRECTOR_STATE.json "provider_blocked_until" {agent: ISO-UTC} overrides it.
    if blocked is None:
        blocked = load_blocked_providers()
    if "codex" in blocked:
        codex_score = -1
    if "antigravity" in blocked:
        gemini_score = -1
    candidates = []
    if codex_score > 15:
        candidates.append((codex_score, 3, "codex", "gpt-6.2-sol", "medium", f"Codex min={codex_score}%"))
    if gemini_score > 15:
        candidates.append((gemini_score, 2, "antigravity", "gemini-4-neon", "medium", f"Gemini min={gemini_score}%"))
    if claude_score > 15:
        candidates.append((claude_score, 1, "claude", "claude-opus-5-5", "medium", f"Claude min={claude_score}%"))
    if candidates:
        candidates.sort(key=lambda x: (x[0], x[1]), reverse=True)
        _, _, ag, mod, eff, why = candidates[0]
        return ag, mod, eff, why
    # Fallback when Claude statusline has no rate_limits printed and both Codex/Gemini < 15%
    return "claude", "claude-opus-5-5", "medium", f"Codex({codex_score}%) & Gemini({gemini_score}%) low -> Claude fallback"


def should_rotate(state: dict, u: dict) -> tuple[bool, list[str]]:
    reasons = []
    b_cnt = state.get("batches_in_current_session", 0)
    b_max = state.get("rotate_session_after_batches", 4)
    if b_cnt >= b_max:
        reasons.append(f"session_batches {b_cnt}/{b_max}")
    cur_ag = (state.get("current_director") or {}).get("agent", "antigravity")
    if cur_ag == "antigravity":
        gmin = min(u.get("gemini_5h", 100), u.get("gemini_week", 100))
        if gmin <= 15:
            reasons.append(f"Gemini credit low ({gmin}%)")
    elif cur_ag == "codex":
        cmin = min(u.get("codex_5h", 100), u.get("codex_week", 100))
        if cmin <= 15:
            reasons.append(f"Codex credit low ({cmin}%)")
    elif cur_ag == "claude":
        clmin = min(u.get("claude_5h", 100), u.get("claude_week", 100))
        if clmin <= 15:
            reasons.append(f"Claude credit low ({clmin}%)")
    return bool(reasons), reasons


def recommend_agent(u: dict) -> str:
    ag, mod, eff, why = pick_best_director(u)
    cw, c5 = u.get("codex_week", -1), u.get("codex_5h", -1)
    gw, g5 = u.get("gemini_week", -1), u.get("gemini_5h", -1)
    return f"RECOMMEND: {ag} / {mod} {eff} ({why}; Codex 5h={c5}% wk={cw}%, Gemini 5h={g5}% wk={gw}%)"


def check_active_worker(state: dict) -> str:
    aw = state.get("active_worker")
    if not aw:
        return "active_worker: NONE (ready to dispatch next batch)"
    did = aw.get("dispatch_id")
    rc, out = run_cmd(
        [ORCA_BIN, "orchestration", "worker-show", "--dispatch", did, "--json"],
        timeout=15,
    )
    live_info = "unknown"
    if rc == 0:
        try:
            res = json.loads(out).get("result", {})
            live_info = (
                f"state={res.get('state')} liveness={res.get('liveness')} "
                f"activity={res.get('activity')} last={res.get('lastActivity')}"
            )
        except Exception:
            live_info = out[:160]
    elapsed_m = 0.0
    try:
        t0 = dt.datetime.fromisoformat(aw["started_utc"].replace("Z", "+00:00"))
        elapsed_m = (now_utc() - t0).total_seconds() / 60.0
    except Exception:
        pass
    return (
        f"active_worker: {aw.get('batch_id')} ({aw.get('agent')}/{aw.get('model')} {aw.get('effort')})\n"
        f"  dispatch={did} task={aw.get('task_id')} started={aw.get('started_utc')} ({elapsed_m:.1f}m ago)\n"
        f"  live: {live_info}\n"
        f"  spec={aw.get('spec_file')} result={aw.get('result_file')}"
    )


def cmd_boot(_args: argparse.Namespace) -> int:
    state = load_state()
    print("=== DIRECTOR BOOT DASHBOARD (compact <4KB) ===")
    print(f"ORCA RUN: {state.get('orca_run_id')}")
    print(
        f"IF NEW SESSION, RUN FIRST: orca orchestration run-use --environment local --id {state.get('orca_run_id')} --json"
    )
    print(check_active_worker(state))

    _, u = parse_usage()
    print(f"\n=== CREDITS ===\n{recommend_agent(u)}")

    pp = state.get("pre_pipelined") or {}
    if pp:
        sp = REPO / pp.get("spec_file", "")
        cp = REPO / pp.get("coords_file", "")
        print(
            f"\n=== PRE-PIPELINED NEXT BATCH ({pp.get('batch_id')}) ===\n"
            f"  spec: {pp.get('spec_file')} (exists={sp.exists()}, {sp.stat().st_size if sp.exists() else 0}B)\n"
            f"  coords: {pp.get('coords_file')} (exists={cp.exists()}, {cp.stat().st_size if cp.exists() else 0}B)\n"
            f"  summary: {pp.get('summary')}"
        )

    b_cnt = state.get("batches_in_current_session", 0)
    b_max = state.get("rotate_session_after_batches", 4)
    p_cnt = state.get("batches_since_progress_sync", 0)
    p_max = state.get("progress_sync_every_n_batches", 2)
    print("\n=== HYGIENE FLAGS ===")
    print(f"  session_batches: {b_cnt}/{b_max}" + (" -> ROTATE / /compact RECOMMENDED!" if b_cnt >= b_max else ""))
    print(f"  since_progress_sync: {p_cnt}/{p_max}" + (" -> SYNC progress.json + progress_svg.py DUE!" if p_cnt >= p_max else ""))

    if RETRO_PATH.exists():
        lines = [ln for ln in RETRO_PATH.read_text(encoding="utf-8").splitlines() if ln.startswith("| 202")]
        print("\n=== LAST 3 RETRO ROWS ===")
        for ln in lines[-3:]:
            print(" ", ln[:220])

    return 0


def cmd_credits(_args: argparse.Namespace) -> int:
    raw, u = parse_usage()
    print(raw)
    print("\n" + recommend_agent(u))
    return 0


def cmd_start(args: argparse.Namespace) -> int:
    state = load_state()
    if state.get("active_worker"):
        print("ERROR: active_worker already set in DIRECTOR_STATE.json; run finish or clear first.")
        return 2
    rc_git, git_out = run_cmd(["git", "status", "--short"], timeout=10)
    if rc_git == 0 and git_out.strip():
        print("ERROR: working tree is dirty before worker-start:\n" + git_out)
        return 2
    spec_path = REPO / args.spec
    if not spec_path.exists():
        print(f"ERROR: spec file missing: {spec_path}")
        return 2

    _, u = parse_usage()
    print(recommend_agent(u))

    started = iso_utc()
    cmd = [
        ORCA_BIN, "orchestration", "worker-start",
        "--environment", "local",
        "--worktree", "current",
        "--agent", args.agent,
        "--model", args.model,
        "--effort", args.effort,
        "--task-title", args.title,
        "--spec-file", args.spec,
        "--json",
    ]
    if state.get("orca_run_id"):
        cmd.extend(["--run", state["orca_run_id"]])
    rc, out = run_cmd(cmd, timeout=45)
    if rc != 0:
        print(f"ERROR: worker-start failed (rc={rc}):\n{out[:1000]}")
        return rc
    data = json.loads(out)
    res = data.get("result", {})
    if res.get("model") != args.model or res.get("effort") != args.effort:
        print(f"WARN: model/effort mismatch! expected={args.model}/{args.effort} got={res.get('model')}/{res.get('effort')}")

    state["orca_run_id"] = res.get("runId") or state.get("orca_run_id")
    state["active_worker"] = {
        "batch_id": args.id,
        "title": args.title,
        "dispatch_id": res.get("dispatchId"),
        "task_id": res.get("taskId"),
        "agent": args.agent,
        "model": res.get("model"),
        "effort": res.get("effort"),
        "started_utc": started,
        "spec_file": args.spec,
        "result_file": args.result,
    }
    state["batches_in_current_session"] = state.get("batches_in_current_session", 0) + 1
    save_state(state)
    print(f"STARTED: {args.id} dispatch={res.get('dispatchId')} run={state['orca_run_id']} utc={started}")
    return 0


def latest_verify_summary(since_dt: dt.datetime) -> dict | None:
    if not VERIFY_ROOT.exists():
        return None
    dirs = sorted(VERIFY_ROOT.iterdir(), key=lambda p: p.name, reverse=True)
    for d in dirs[:5]:
        sj = d / "summary.json"
        if sj.exists():
            try:
                mtime = dt.datetime.fromtimestamp(sj.stat().st_mtime, dt.timezone.utc)
                if mtime >= since_dt - dt.timedelta(minutes=2):
                    return json.loads(sj.read_text(encoding="utf-8"))
            except Exception:
                pass
    return None


def cmd_finish(args: argparse.Namespace) -> int:
    state = load_state()
    aw = state.get("active_worker")
    if not aw:
        print("No active_worker in DIRECTOR_STATE.json.")
        return 1

    started_str = aw["started_utc"]
    started_dt = dt.datetime.fromisoformat(started_str.replace("Z", "+00:00"))
    elapsed_min = (now_utc() - started_dt).total_seconds() / 60.0

    res_path = REPO / (args.result or aw.get("result_file", ""))
    res_data = {}
    if res_path.is_file():
        res_data = json.loads(res_path.read_text(encoding="utf-8"))

    timing = res_data.get("verify_timing_sec")
    vsum = None if timing else latest_verify_summary(started_dt)

    # Audit KPIs (agy_audit.py --since, plus --codex if supported)
    audit_cmd = [sys.executable, "-I", str(REPO / "tools/agents/agy_audit.py"), "--since", started_str, "--json"]
    if aw.get("agent") == "codex":
        rc_h, help_out = run_cmd([sys.executable, "-I", str(REPO / "tools/agents/agy_audit.py"), "--help"], timeout=10)
        if "--codex" in help_out:
            audit_cmd.append("--codex")
    rc_a, out_a = run_cmd(audit_cmd, timeout=25)
    kpi_str = "- / - / - / -"
    if rc_a == 0:
        try:
            ad = json.loads(out_a)
            rep = sum(len(s.get("repeated_reads", [])) for s in ad.get("sessions", []))
            gt8 = sum(
                sum(1 for r in s.get("top_results", []) if r.get("bytes", 0) > 8192)
                for s in ad.get("sessions", [])
            )
            err = sum(s.get("errors", 0) for s in ad.get("sessions", []))
            flags = list(ad.get("flags", {}).keys())
            kpi_str = f"rep={rep} >8KB={gt8} err={err} flags={flags}"
        except Exception:
            kpi_str = "audit-parse-err"

    print(f"=== FINISH AUDIT: {aw.get('batch_id')} ===")
    print(f"Elapsed: {elapsed_min:.1f} min (threshold 25.0m -> {'EXCEEDED!' if elapsed_min > 25 else 'OK'})")
    print(f"Commit: {res_data.get('commit')} | Gates: {res_data.get('gates')} | Status: {res_data.get('status')}")
    print(f"Audit KPI: {kpi_str}")
    if timing:
        steps = {
            step: seconds for step, seconds in timing.items()
            if step != "total" and type(seconds) in (int, float)
        }
        longest = max(steps, key=steps.get, default=None)
        print(f"Verify timing: total={timing.get('total')}s longest={longest} ({steps.get(longest)}s)")
    elif vsum:
        print(f"Verify summary.json: total={vsum.get('total_sec')}s longest={vsum.get('longest_step')}")
    print(f"Friction: {res_data.get('friction')}")

    alerts = []
    if elapsed_min > 25:
        alerts.append("TIME >25m: evaluate bottleneck automation promotion")
    if res_data.get("failed_gate"):
        alerts.append(f"GATE FAILED ({res_data.get('failed_gate')}): check 2x stop rule")
    if state.get("batches_since_progress_sync", 0) + 1 >= state.get("progress_sync_every_n_batches", 2):
        alerts.append("PROGRESS SYNC DUE: update docs/progress/progress.json + scripts/progress_svg.py")
    if state.get("batches_in_current_session", 0) >= state.get("rotate_session_after_batches", 4):
        alerts.append("SESSION ROTATION DUE: run `python -I tools/agents/director_ctl.py rotate` to auto-spawn next Director on richest-credit provider!")
    _, u = parse_usage()
    rot, rot_why = should_rotate(state, u)
    if rot:
        alerts.append(f"AUTO-ROTATE TRIGGERED ({', '.join(rot_why)}) -> run `python -I tools/agents/director_ctl.py rotate` and stop this session!")
    if alerts:
        print("ALERTS:")
        for a in alerts:
            print(" -", a)

    if not args.dry_run:
        state.setdefault("completed_batches", []).append({
            "batch_id": aw.get("batch_id"),
            "commit": res_data.get("commit"),
            "agent": f"{aw.get('agent')}/{aw.get('model')}-{aw.get('effort')}",
            "duration_min": round(elapsed_min, 1),
            "result": (res_data.get("summary") or "")[:160],
        })
        state["active_worker"] = None
        state["batches_since_progress_sync"] = state.get("batches_since_progress_sync", 0) + 1
        save_state(state)
        print("State updated (active_worker cleared). Append 1 row to Logs/handoff/worker_retro.md now.")
    return 0


def cmd_rotate(args: argparse.Namespace) -> int:
    state = load_state()
    last_rot = state.get("last_rotation_utc")
    if last_rot and not args.force:
        try:
            t_last = dt.datetime.fromisoformat(last_rot.replace("Z", "+00:00"))
            age_s = (now_utc() - t_last).total_seconds()
            if age_s < 300:
                print(f"GUARD: rotated {age_s:.0f}s ago (<300s). Pass --force to override.")
                return 2
        except Exception:
            pass

    _, u = parse_usage()
    ag, mod, eff, why = pick_best_director(u)
    run_id = state.get("orca_run_id", "")
    worktree = state.get("worktree_id") or "current"
    if not ROTATION_PROMPT.is_file():
        print(f"ERROR: missing rotation prompt template {ROTATION_PROMPT}")
        return 2
    prompt = ROTATION_PROMPT.read_text(encoding="utf-8").strip().replace("{run_id}", run_id)
    if not prompt:
        print("ERROR: rotation prompt template is empty")
        return 2
    cmd = [
        ORCA_BIN, "agent", "launch",
        "--environment", "local",
        "--worktree", worktree,
        "--agent", ag,
        "--model", mod,
        "--effort", eff,
        "--prompt", prompt,
        "--json",
    ]
    print(f"ROTATE TARGET: {ag} / {mod} {eff} ({why})")
    if args.dry_run:
        print("DRY-RUN CMD:", " ".join(cmd[:-2]), "--prompt <...> --json")
        return 0

    rc, out = run_cmd(cmd, timeout=45)
    if rc != 0:
        print(f"ERROR: agent launch failed (rc={rc}):\n{out[:1000]}")
        return rc
    res = json.loads(out).get("result", {})
    launch = res.get("launch") or res
    outcome = launch.get("outcome") or {}
    p_out = (launch.get("prompt") or {}).get("outcome")
    handle = (
        outcome.get("handle")
        or res.get("handle")
        or res.get("orcaSessionId")
        or res.get("agentTerminalHandle")
    )
    kind = outcome.get("kind") or res.get("kind")

    state["batches_in_current_session"] = 0
    state["last_rotation_utc"] = iso_utc()
    state["current_director"] = {
        "agent": ag,
        "model": mod,
        "effort": eff,
        "handle": handle,
        "kind": kind,
        "rotated_utc": state["last_rotation_utc"],
        "reason": why,
    }
    save_state(state)
    print(f"ROTATED OK: agent={ag} model={mod} effort={eff} kind={kind} handle={handle} prompt={p_out}")
    print("HANDOFF COMPLETE — STOP THIS SESSION NOW (do not wait for the new Director).")
    return 0


def cmd_selftest(_args: argparse.Namespace) -> int:
    from contextlib import redirect_stdout
    from io import StringIO
    from tempfile import TemporaryDirectory
    from unittest.mock import patch

    with TemporaryDirectory() as directory:
        root = Path(directory)
        summary = root / "latest" / "summary.json"
        summary.parent.mkdir()
        summary.write_text('{"total_sec":0.096,"longest_step":"precheck"}', encoding="utf-8")
        result = root / "batch.json"
        result.write_text(
            '{"verify_timing_sec":{"total":110.262,"longest_step":"build","build":59.76}}',
            encoding="utf-8",
        )
        state = {"active_worker": {"started_utc": iso_utc(), "result_file": str(result)}}
        output = StringIO()
        with (
            patch.object(sys.modules[__name__], "VERIFY_ROOT", root),
            patch.object(sys.modules[__name__], "load_state", return_value=state),
            patch.object(sys.modules[__name__], "run_cmd", return_value=(1, "")),
            patch.object(sys.modules[__name__], "parse_usage", return_value=("", {})),
            redirect_stdout(output),
        ):
            assert cmd_finish(argparse.Namespace(result="", dry_run=True)) == 0
            result.unlink()
            assert cmd_finish(argparse.Namespace(result="", dry_run=True)) == 0
        verification = [line for line in output.getvalue().splitlines() if line.startswith("Verify ")]
        assert verification == [
            "Verify timing: total=110.262s longest=build (59.76s)",
            "Verify summary.json: total=0.096s longest=precheck",
        ], verification
    assert pick_best_director({"codex_week": 94, "codex_5h": 86, "gemini_week": 19, "gemini_5h": 20}, {})[0] == "codex"
    assert pick_best_director({"codex_week": 94, "codex_5h": 86, "gemini_week": 19, "gemini_5h": 20}, {"codex": "x"})[0] == "antigravity"
    assert pick_best_director({"codex_week": 10, "codex_5h": 10, "gemini_week": 80, "gemini_5h": 80}, {})[0] == "antigravity"
    assert pick_best_director({"codex_week": 5, "codex_5h": 5, "gemini_week": 10, "gemini_5h": 10}, {})[0] == "claude"
    rot, _ = should_rotate({"batches_in_current_session": 4, "rotate_session_after_batches": 4, "current_director": {"agent": "antigravity"}}, {"gemini_5h": 50, "gemini_week": 50})
    assert rot is True
    rot2, _ = should_rotate({"batches_in_current_session": 1, "rotate_session_after_batches": 4, "current_director": {"agent": "antigravity"}}, {"gemini_5h": 10, "gemini_week": 16})
    assert rot2 is True
    print("SELFTEST PASS (batch timing + credit router + rotate guard verified)")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description="Fluxer Director Session Controller")
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("boot", help="Print compact startup dashboard for a new Director session")
    sub.add_parser("credits", help="Run full usage.py and recommend agent/model")
    p_st = sub.add_parser("start", help="Start next worker and persist DIRECTOR_STATE.json")
    p_st.add_argument("--id", required=True)
    p_st.add_argument("--title", required=True)
    p_st.add_argument("--agent", required=True, choices=["codex", "antigravity", "claude"])
    p_st.add_argument("--model", required=True)
    p_st.add_argument("--effort", default="medium")
    p_st.add_argument("--spec", required=True)
    p_st.add_argument("--result", required=True)
    p_fn = sub.add_parser("finish", help="Audit completed worker and output retro/alerts")
    p_fn.add_argument("--result", default="")
    p_fn.add_argument("--dry-run", action="store_true")
    p_rt = sub.add_parser("rotate", help="Auto-launch new Director session on richest-credit provider")
    p_rt.add_argument("--dry-run", action="store_true")
    p_rt.add_argument("--force", action="store_true")
    sub.add_parser("selftest", help="Run self-test")
    args = ap.parse_args()
    return {
        "boot": cmd_boot,
        "credits": cmd_credits,
        "start": cmd_start,
        "finish": cmd_finish,
        "rotate": cmd_rotate,
        "selftest": cmd_selftest,
    }[args.cmd](args)


if __name__ == "__main__":
    raise SystemExit(main())
