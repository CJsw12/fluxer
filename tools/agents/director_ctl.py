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
# User directive 2026-11-28: directors and batch workers run only on Codex or Gemini
# (Antigravity). Claude is not used; DeepSeek stays for T0 helpers.
ALLOWED_AGENTS = ("codex", "antigravity")
AGENT_MODELS = {
    "codex": ("gpt-6.2-sol", "medium", "--model gpt-6.2-sol -c model_reasoning_effort=medium"),
    "antigravity": ("gemini-4-neon", "medium", "--model gemini-4-neon"),
}
CREDIT_FLOOR = 15
WATCHDOG_NAME = "Fluxer director quota resume ({agent})"
WATCHDOG_CRON = "*/10 * * * *"
# A director that hit its limit can die without running `pause`; treat the
# state as abandoned after this long without a save/heartbeat.
STALE_DIRECTOR_MIN = 150
LAUNCH_GUARD_MIN = 20


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
    info: dict = {}
    m_cw = re.search(r"Codex\s*:.*?week\s+(\d+)%\s+left", out)
    m_c5 = re.search(r"Codex\s*:.*?5h\s+(\d+)%\s+left", out)
    m_gw = re.search(r"Gemini Models Weekly Limit Remaining\s+(\d+)%\s*(\S+)?", out)
    m_g5 = re.search(r"Gemini Models Five Hour Limit Remaining\s+(\d+)%\s*(\S+)?", out)
    m_clw = re.search(r"Claude\s*:.*?week\s+(\d+)%\s+left", out)
    m_cl5 = re.search(r"Claude\s*:.*?5h\s+(\d+)%\s+left", out)
    if m_cw:
        info["codex_week"] = int(m_cw.group(1))
    if m_c5:
        info["codex_5h"] = int(m_c5.group(1))
    # usage.py reads the newest Codex session snapshot, which stays at the old
    # (exhausted) value until a new Codex session runs. A window whose reset
    # time has passed is full again.
    for key, label in (("codex_5h", "5h"), ("codex_week", "week")):
        m = re.search(rf"Codex\s*:.*?{label}\s+\d+%\s+left\s+\(reset\s+(\d\d)-(\d\d)\s+(\d\d):(\d\d)\)", out)
        if not m:
            continue
        local_now = dt.datetime.now().astimezone()
        try:
            reset = local_now.replace(month=int(m.group(1)), day=int(m.group(2)),
                                      hour=int(m.group(3)), minute=int(m.group(4)), second=0, microsecond=0)
        except ValueError:
            continue
        if reset - local_now > dt.timedelta(days=200):  # December reset seen in January
            reset = reset.replace(year=reset.year - 1)
        info[f"{key}_reset"] = iso_utc(reset.astimezone(dt.timezone.utc))
        if reset <= local_now and key in info:
            info[key] = 100
    if m_gw:
        info["gemini_week"] = int(m_gw.group(1))
        if m_gw.group(2) and m_gw.group(2)[:4].isdigit():
            info["gemini_week_reset"] = m_gw.group(2)
    if m_g5:
        info["gemini_5h"] = int(m_g5.group(1))
        if m_g5.group(2) and m_g5.group(2)[:4].isdigit():
            info["gemini_5h_reset"] = m_g5.group(2)
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


def parse_iso(value: str | None) -> dt.datetime | None:
    if not value:
        return None
    try:
        t = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None
    return t if t.tzinfo else t.replace(tzinfo=dt.timezone.utc)


def provider_scores(u: dict, blocked: dict | None = None) -> dict[str, int]:
    # usage.py can report stale headroom after a provider hit its session limit;
    # DIRECTOR_STATE.json "provider_blocked_until" {agent: ISO-UTC} overrides it.
    if blocked is None:
        blocked = load_blocked_providers()
    scores = {
        "codex": min(u.get("codex_week", -1), u.get("codex_5h", -1)),
        "antigravity": min(u.get("gemini_week", -1), u.get("gemini_5h", -1)),
    }
    for ag in blocked:
        if ag in scores:
            scores[ag] = -1
    return scores


def pick_best_director(u: dict, blocked: dict | None = None) -> tuple[str, str, str, str]:
    """Return (agent, model, effort, reason); agent is "" when Codex and Gemini are both exhausted."""
    scores = provider_scores(u, blocked)
    candidates = [
        (score, 2 if ag == "codex" else 1, ag)
        for ag, score in scores.items()
        if score > CREDIT_FLOOR
    ]
    if candidates:
        candidates.sort(reverse=True)
        score, _, ag = candidates[0]
        model, effort, _ = AGENT_MODELS[ag]
        return ag, model, effort, f"{ag} min={score}%"
    return "", "", "", (
        f"Codex({scores['codex']}%) & Gemini({scores['antigravity']}%) exhausted -> pause until "
        f"{earliest_reset(u, blocked) or 'unknown'}"
    )


def earliest_reset(u: dict, blocked: dict | None = None) -> str:
    """Earliest UTC time at which Codex or Gemini should have headroom again."""
    if blocked is None:
        blocked = load_blocked_providers()
    times: list[dt.datetime] = []
    t = parse_iso(blocked.get("codex"))
    if t:
        times.append(t)
    elif min(u.get("codex_week", -1), u.get("codex_5h", -1)) <= CREDIT_FLOOR:
        key = "codex_week_reset" if u.get("codex_week", -1) <= CREDIT_FLOOR else "codex_5h_reset"
        times.append(parse_iso(u.get(key)) or now_utc() + dt.timedelta(hours=1))
    t = parse_iso(blocked.get("antigravity"))
    if t:
        times.append(t)
    elif u.get("gemini_week", -1) <= CREDIT_FLOOR:
        t = parse_iso(u.get("gemini_week_reset"))
        if t:
            times.append(t)
    elif u.get("gemini_5h", -1) <= CREDIT_FLOOR:
        t = parse_iso(u.get("gemini_5h_reset"))
        if t:
            times.append(t)
    future = [x for x in times if x > now_utc()]
    return iso_utc(min(future)) if future else ""


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
    else:
        reasons.append(f"director agent {cur_ag!r} is not allowed (Codex/Gemini only)")
    return bool(reasons), reasons


def recommend_agent(u: dict) -> str:
    ag, mod, eff, why = pick_best_director(u)
    if not ag:
        return f"RECOMMEND: NONE ({why}) -> run `director_ctl.py pause` and stop this session"
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
    print(take_over_director(state))
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

    directives = state.get("user_directives") or []
    if directives:
        print("\n=== USER DIRECTIVES (must apply; full text in Logs/handoff/DIRECTOR_HANDOFF.md) ===")
        for d in directives:
            print(f"  - {d}")

    pending = state.get("pending") or []
    if pending:
        print("\n=== PENDING WORK (in-flight T0 / queued batches / tracks) ===")
        for p in pending:
            f = p.get("file")
            ex = f" exists={(REPO / f).exists()}" if f else ""
            print(f"  - [{p.get('track', '-')}] {p.get('id')}: {p.get('note')} ({f or '-'}{ex})")

    if state.get("blocker"):
        print(f"\n=== BLOCKER ===\n  {state['blocker']}")

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
    scores = provider_scores(u)
    if scores.get(args.agent, -1) <= CREDIT_FLOOR:
        print(f"ERROR: {args.agent} has no headroom ({scores.get(args.agent)}%). "
              "Pick the recommended agent, or run `director_ctl.py pause` when none is left.")
        return 3

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
        if aw.get("dispatch_id"):
            rc_rel, rel = run_cmd([ORCA_BIN, "orchestration", "worker-release", "--environment", "local",
                                   "--dispatch", aw["dispatch_id"], "--json"], timeout=30)
            print(f"worker-release {aw['dispatch_id']}: {'ok' if rc_rel == 0 else rel[:160]}")
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
    if not ag:
        print(f"NO PROVIDER: {why}")
        return cmd_pause(argparse.Namespace(reason=why, dry_run=args.dry_run))
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
        "--surface", "terminal",
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
    state["previous_director"] = state.get("current_director")
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
    print("HANDOFF COMPLETE — STOP THIS SESSION NOW (the new Director closes this tab on boot).")
    return 0


def close_director(d: dict | None) -> str:
    if not d:
        return "none"
    handle = d.get("handle") or ""
    if d.get("kind") != "terminal" or not handle:
        return f"cannot auto-close {d.get('kind') or '?'} session {handle or '?'} (close its tab manually)"
    rc, out = run_cmd([ORCA_BIN, "terminal", "close", "--terminal", handle, "--tab", "--json"], timeout=30)
    if rc == 0:
        return f"closed {handle}"
    if "terminal_handle_stale" in out:
        return f"already gone {handle}"
    return f"close failed for {handle}: {out[:160]}"


def sweep_idle_tabs(state: dict, keep_handle: str = "") -> list[str]:
    """Close finished Codex/Gemini terminal tabs in this worktree (old Directors, settled workers).

    Only terminals whose agent session is idle ("done") are closed; working agents,
    plain shells and chat sessions (not closable from the CLI) are left alone.
    """
    wt = (state.get("worktree_id") or "").removeprefix("id:")
    if not wt:
        return []
    rc, out = run_cmd([ORCA_BIN, "agent", "list", "--json"], timeout=30)
    if rc != 0:
        return [f"sweep skipped: agent list failed ({out[:120]})"]
    try:
        res = json.loads(out)["result"]
        items = res if isinstance(res, list) else next(v for v in res.values() if isinstance(v, list))
    except Exception:
        return ["sweep skipped: agent list unreadable"]
    idle_panes = {
        a.get("paneKey") for a in items
        if a.get("worktreeId") == wt and a.get("kind") == "terminal"
        and a.get("agent") in ALLOWED_AGENTS and a.get("state") == "done"
    }
    if not idle_panes:
        return []
    rc, out = run_cmd([ORCA_BIN, "terminal", "list", "--worktree", f"id:{wt}", "--json"], timeout=30)
    if rc != 0:
        return [f"sweep skipped: terminal list failed ({out[:120]})"]
    try:
        res = json.loads(out)["result"]
        terms = res if isinstance(res, list) else next(v for v in res.values() if isinstance(v, list))
    except Exception:
        return ["sweep skipped: terminal list unreadable"]
    msgs = []
    for t in terms:
        handle = t.get("handle") or ""
        if not handle or handle == keep_handle:
            continue
        rc, shown = run_cmd([ORCA_BIN, "terminal", "show", "--terminal", handle, "--json"], timeout=20)
        if rc != 0:
            continue
        try:
            info = json.loads(shown)["result"]
            info = info.get("terminal", info)
        except Exception:
            continue
        if f"{info.get('tabId')}:{info.get('leafId')}" not in idle_panes:
            continue
        rc, closed = run_cmd([ORCA_BIN, "terminal", "close", "--terminal", handle, "--tab", "--json"], timeout=30)
        msgs.append(f"closed idle {info.get('agentIdentity')} tab {handle}" if rc == 0
                    else f"close failed {handle}: {closed[:120]}")
    return msgs


def take_over_director(state: dict) -> str:
    """Called on boot: record this session, close the previous Director tab, clear pause."""
    me_handle = os.environ.get("ORCA_TERMINAL_HANDLE") or ""
    me_session = os.environ.get("ORCA_AGENT_SESSION_ID") or ""
    cur = dict(state.get("current_director") or {})
    prev = state.get("previous_director")
    is_me = bool((me_handle and cur.get("handle") == me_handle) or (me_session and cur.get("session") == me_session))
    if cur.get("booted_utc") and not is_me:
        # The recorded director already booted in another session: it is the old one.
        prev = cur
        cur = {k: v for k, v in cur.items() if k in ("agent", "model", "effort")}
    lines = []
    if prev and not ((me_handle and prev.get("handle") == me_handle) or (me_session and prev.get("session") == me_session)):
        lines.append(f"PREVIOUS DIRECTOR: {close_director(prev)}")
    state["previous_director"] = None
    if me_handle:
        cur["handle"] = me_handle
        cur["kind"] = "terminal"
    if me_session:
        cur["session"] = me_session
    cur["booted_utc"] = iso_utc()
    state["current_director"] = cur
    if state.get("paused"):
        lines.append(f"RESUMED FROM PAUSE: {state['paused'].get('reason')}")
        state["paused"] = None
    save_state(state)
    lines.extend(sweep_idle_tabs(state, keep_handle=me_handle))
    agent = cur.get("agent") or "?"
    if agent not in ALLOWED_AGENTS:
        lines.append(f"WARN: director agent {agent!r} is not Codex/Gemini -> finish the current step, then `rotate`.")
    return "\n".join(lines) if lines else "DIRECTOR: takeover recorded"


def ensure_watchdogs(dry_run: bool = False) -> list[str]:
    """One recurring automation per allowed provider; the precheck only lets the best one fire."""
    rc, out = run_cmd([ORCA_BIN, "automations", "list", "--json"], timeout=30)
    existing = set()
    if rc == 0:
        try:
            existing = {a.get("name") for a in json.loads(out)["result"]["automations"]}
        except Exception:
            pass
    state = load_state()
    prompt = ROTATION_PROMPT.read_text(encoding="utf-8").strip().replace("{run_id}", state.get("orca_run_id", ""))
    worktree = state.get("worktree_id") or "current"
    script = str(Path(__file__).resolve()).replace("\\", "/")
    python = sys.executable.replace("\\", "/")
    msgs = []
    for ag in ALLOWED_AGENTS:
        name = WATCHDOG_NAME.format(agent=ag)
        if name in existing:
            msgs.append(f"watchdog exists: {name}")
            continue
        cmd = [
            ORCA_BIN, "automations", "create",
            "--name", name,
            "--trigger", WATCHDOG_CRON,
            "--provider", ag,
            "--workspace", worktree,
            "--workspace-mode", "existing",
            f"--extra-agent-args={AGENT_MODELS[ag][2]}",
            "--precheck", f'"{python}" -I "{script}" resume-check --agent {ag}',
            "--precheck-timeout", "90",
            "--prompt", prompt,
            "--json",
        ]
        if dry_run:
            msgs.append("DRY-RUN: " + " ".join(cmd[:14]))
            continue
        rc, out = run_cmd(cmd, timeout=45)
        msgs.append(f"watchdog {'created' if rc == 0 else 'FAILED'}: {name}" + ("" if rc == 0 else f" {out[:300]}"))
    return msgs


def cmd_pause(args: argparse.Namespace) -> int:
    state = load_state()
    _, u = parse_usage()
    resume = earliest_reset(u) or iso_utc(now_utc() + dt.timedelta(hours=1))
    state["paused"] = {"since": iso_utc(), "resume_after": resume, "reason": getattr(args, "reason", "") or "quota"}
    if not args.dry_run:
        save_state(state)
    for m in ensure_watchdogs(args.dry_run):
        print(m)
    print(f"PAUSED until >= {resume}. The watchdog automation starts the next Codex/Gemini Director,")
    print("which closes this tab on boot when it is a terminal. STOP THIS SESSION NOW.")
    return 0


def director_alive(state: dict) -> bool:
    cur = state.get("current_director") or {}
    sid = cur.get("session") or ""
    handle = cur.get("handle") or ""
    rc, out = run_cmd([ORCA_BIN, "agent", "list", "--json"], timeout=30)
    if rc != 0:
        return True  # unknown -> do not risk a second director
    try:
        res = json.loads(out)["result"]
        items = res if isinstance(res, list) else next(v for v in res.values() if isinstance(v, list))
    except Exception:
        return True
    for a in items:
        if (sid and a.get("orcaSessionId") == sid) or (handle and handle in str(a.get("paneKey", ""))):
            return a.get("state") in ("working", "waiting", "ready")
    return False


def cmd_resume_check(args: argparse.Namespace) -> int:
    """Automation precheck: exit 0 only when this provider should start the next Director."""
    state = load_state()
    paused = dict(state.get("paused") or {})
    age = (now_utc() - (parse_iso(state.get("updated_utc")) or now_utc())).total_seconds() / 60.0
    if not paused and (age < STALE_DIRECTOR_MIN or director_alive(state)):
        print(f"skip: director active (state age {age:.0f}m)")
        return 1
    if paused:
        for m in sweep_idle_tabs(state):
            print(m)
    launching = parse_iso(paused.get("launching_utc"))
    if launching and (now_utc() - launching).total_seconds() < LAUNCH_GUARD_MIN * 60:
        print("skip: a resume launch is already in progress")
        return 1
    resume_after = parse_iso(paused.get("resume_after"))
    if resume_after and now_utc() < resume_after:
        print(f"skip: waiting for reset {paused.get('resume_after')}")
        return 1
    _, u = parse_usage()
    best = pick_best_director(u)[0]
    if best != args.agent:
        print(f"skip: best provider is {best or 'none'}")
        return 1
    model, effort, _ = AGENT_MODELS[args.agent]
    if not paused:
        paused = {"since": iso_utc(), "reason": f"stale director state ({age:.0f}m)"}
    paused["launching_utc"] = iso_utc()
    state["paused"] = paused
    state["previous_director"] = state.get("current_director")
    state["current_director"] = {"agent": args.agent, "model": model, "effort": effort,
                                 "rotated_utc": iso_utc(), "reason": "quota-resume watchdog"}
    state["batches_in_current_session"] = 0
    save_state(state)
    print(f"go: {args.agent}")
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
    assert pick_best_director({"codex_week": 5, "codex_5h": 5, "gemini_week": 10, "gemini_5h": 10}, {})[0] == ""
    assert pick_best_director({"codex_week": 5, "codex_5h": 5, "gemini_week": 10, "gemini_5h": 10, "claude_week": 90, "claude_5h": 90}, {})[0] == ""
    soon = iso_utc(now_utc() + dt.timedelta(hours=2))
    later = iso_utc(now_utc() + dt.timedelta(days=3))
    assert earliest_reset({"codex_week": 90, "codex_5h": 90, "gemini_week": 10, "gemini_week_reset": later, "gemini_5h": 90}, {"codex": soon}) == soon
    assert earliest_reset({"codex_week": 90, "codex_5h": 90, "gemini_week": 10, "gemini_week_reset": soon, "gemini_5h": 90}, {"codex": later}) == soon
    past = (dt.datetime.now().astimezone() - dt.timedelta(hours=2)).strftime("%m-%d %H:%M")
    fut = (dt.datetime.now().astimezone() + dt.timedelta(days=3)).strftime("%m-%d %H:%M")
    with patch.object(sys.modules[__name__], "run_cmd",
                      return_value=(0, f"Codex   : 5h 2% left (reset {past}), week 40% left (reset {fut})")), \
            patch.object(USAGE_SCRIPT.__class__, "exists", return_value=True):
        _, cu = parse_usage()
    assert cu["codex_5h"] == 100 and cu["codex_week"] == 40, cu
    rot3, _ = should_rotate({"batches_in_current_session": 0, "current_director": {"agent": "claude"}}, {})
    assert rot3 is True
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
    p_st.add_argument("--agent", required=True, choices=list(ALLOWED_AGENTS))
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
    p_pa = sub.add_parser("pause", help="Codex/Gemini exhausted: record pause and ensure quota-resume watchdogs")
    p_pa.add_argument("--reason", default="")
    p_pa.add_argument("--dry-run", action="store_true")
    p_rc = sub.add_parser("resume-check", help="Automation precheck: exit 0 if this provider should start the next Director")
    p_rc.add_argument("--agent", required=True, choices=list(ALLOWED_AGENTS))
    sub.add_parser("watchdogs", help="Create missing quota-resume watchdog automations")
    sub.add_parser("selftest", help="Run self-test")
    args = ap.parse_args()
    return {
        "boot": cmd_boot,
        "credits": cmd_credits,
        "start": cmd_start,
        "finish": cmd_finish,
        "rotate": cmd_rotate,
        "pause": cmd_pause,
        "resume-check": cmd_resume_check,
        "watchdogs": lambda _a: (print("\n".join(ensure_watchdogs())), 0)[1],
        "selftest": cmd_selftest,
    }[args.cmd](args)


if __name__ == "__main__":
    raise SystemExit(main())
