#!/usr/bin/env python3
"""Read-only audit of Antigravity CLI session transcripts.
Usage: python -I tools/agents/agy_audit.py [BRAIN_ID ...] [--latest] [--since ISO8601] [--json] [--top N] [--selftest]
AGY_BRAIN_DIR overrides the transcript root (used by --selftest)."""

import argparse, json, os, re, sys, tempfile
from collections import Counter, defaultdict
from datetime import datetime, timedelta, timezone
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
BRAIN_DIR = Path(os.environ.get("AGY_BRAIN_DIR") or (Path.home() / ".gemini" / "antigravity-cli" / "brain"))
TRANSCRIPT = Path(".system_generated") / "logs" / "transcript_full.jsonl"
HOME = str(Path.home())
FLAG_SHORT = {
    "parity_lab source read": "parity-src", "gemini flash model": "flash", "flash delegation": "flash-deleg",
    "inherit delegation": "inherit", "direct fluxer exe": "direct-exe", "git push --force": "force-push",
    "LOCALAPPDATA fluxer_desktop": "personal-app", "SSL_CERT_FILE": "ssl-cert", "user question": "question",
}
JWT_RE = re.compile(r"eyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{4,}")
LONG_RE = re.compile(r"(?<![A-Za-z0-9_\-])[A-Za-z0-9_\-]{32,}(?![A-Za-z0-9_\-])")
BEARER_RE = re.compile(r"(?i)(bearer\s+)\S+")
SK_RE = re.compile(r"\bsk-[A-Za-z0-9_\-]{10,}")
HOME_RE = re.compile(r"(?i)\b[a-z]:[\\/]+users[\\/]+[^\\/\s\"']+")
CONFIRM_RE = re.compile(r"(?i)(shall i|should i|do you want|proceed|confirm|approve|계속|진행|확인|할까요|될까요)")
LINE_CACHE = {}

def mask(text):
    text = str(text)
    for h in (HOME, HOME.replace("\\", "/")):
        text = text.replace(h, "~")
    text = text.replace("%USERPROFILE%", "~")
    rel = str(REPO_ROOT).replace(HOME, "~")
    for r in (rel + "\\", rel.replace("\\", "/") + "/"):
        text = text.replace(r, "")
    return HOME_RE.sub("~", JWT_RE.sub("***JWT***", SK_RE.sub("sk-***", BEARER_RE.sub(r"\1***", LONG_RE.sub("***", text)))))

def short(text, limit):
    text = " ".join(mask(text).replace("\r", " ").replace("\n", " ").split())
    return text if len(text) <= limit else "…" + text[-(limit - 1):]

def parse_ts(value):
    if not isinstance(value, str) or not value:
        return None
    try:
        dt = datetime.fromisoformat(value.strip().replace("Z", "+00:00"))
    except ValueError:
        return None
    return dt.replace(tzinfo=timezone.utc) if dt.tzinfo is None else dt

def fmt_dur(seconds):
    h, rem = divmod(max(0, int(seconds)), 3600)
    m, s = divmod(rem, 60)
    return f"{h}h{m:02d}m" if h else (f"{m}m{s:02d}s" if m else f"{s}s")

def fmt_num(n):
    return f"{n / 1_000_000:.1f}M" if n >= 1_000_000 else (f"{n / 1_000:.1f}k" if n >= 1_000 else str(n))

def good_int(value):
    try: return int(value or 0)
    except (TypeError, ValueError): return 0

def total_tokens(session):
    return sum(sum(v[1:]) for v in session["models"].values())

def update_line_cache(path):
    if path not in LINE_CACHE:
        unsafe = bool({p.lower() for p in path.parts} & {"logs", "roaming"}) or path.name.lower() in {".env", "credentials", "id_rsa"} or path.suffix.lower() in {".pem", ".key"}
        try:
            if unsafe: LINE_CACHE[path] = None
            else:
                data = path.read_bytes()
                LINE_CACHE[path] = data.count(b"\n") + (0 if data.endswith(b"\n") else 1)
        except OSError:
            LINE_CACHE[path] = None
    return LINE_CACHE[path]

def resolve_path(raw):
    text = str(raw).strip().strip("\"'`")
    if not text or text.startswith(("$", "%")) or "://" in text:
        return None
    p = Path(os.path.expandvars(text)).expanduser()
    p = p if p.is_absolute() else REPO_ROOT / p
    try:
        return p.resolve()
    except OSError:
        return None

def arg_hint(args):
    if not isinstance(args, dict):
        return ""
    for key in ("Path", "CommandLine", "Command", "path", "file_path", "Target", "Prompt", "brief", "query", "Message", "text"):
        if isinstance(args.get(key), str) and args[key].strip():
            return short(args[key], 80)
    for value in args.values():
        if isinstance(value, str) and value.strip():
            return short(value, 80)
    return ""

def flag_add(session, name, step):
    session["flags"][name].append((session["id"], step))

def check_command(session, cmd, step):
    if not cmd: return
    norm, low = cmd.replace("\\", "/"), cmd.replace("\\", "/").lower()
    if "opencode" in low or "deepseek" in low: session["opencode_cmds"] += 1
    if re.search(r"(?i)git\s+push\b[^\n;&|]*--force", cmd): flag_add(session, "git push --force", step)
    if "SSL_CERT_FILE" in cmd: flag_add(session, "SSL_CERT_FILE", step)
    if "fluxer_desktop" in low and ("localappdata" in low or "appdata/local" in low): flag_add(session, "LOCALAPPDATA fluxer_desktop", step)
    if re.search(r"(?i)(?:cat|type|get-content)\s+[^\n;&|]*tools/parity_lab/[^\s\"']+\.(?:py|mjs)", norm): flag_add(session, "parity_lab source read", step)
    for seg in re.split(r"[\n;]", norm):
        if "env.py" in low: break
        if re.search(r"(?i)(fluxer_client|fluxer)\.exe", seg) and not re.search(r"(?i)taskkill|get-process|stop-process", seg):
            if re.search(r"(?i)start-process|&\s|(?:^|\s)\./|(?:^|\s)[a-z]:/|cmd(?:\.exe)?\s+/c|(?:^|\s)start\s", seg):
                flag_add(session, "direct fluxer exe", step)
                break

def analyze(transcript, brain_dir):
    sid = Path(brain_dir).name
    s = {"id": sid, "first": None, "last": None, "models": {}, "tools": Counter(), "delegation_calls": 0,
         "delegation_models": Counter(), "opencode_cmds": 0, "results": [], "reads": [], "unranged": [],
         "flags": defaultdict(list), "errors": 0, "error_msgs": []}
    pending, pending_step = [], -1
    with open(transcript, encoding="utf-8", errors="replace") as fh:
        for line in fh:
            if not line.strip():
                continue
            try:
                r = json.loads(line)
            except json.JSONDecodeError:
                continue
            step = r.get("step_index", 0)
            ts = parse_ts(r.get("created_at"))
            if ts:
                s["first"] = ts if s["first"] is None or ts < s["first"] else s["first"]
                s["last"] = ts if s["last"] is None or ts > s["last"] else s["last"]
            model = r.get("model") if isinstance(r.get("model"), str) else ""
            if model:
                slot = s["models"].setdefault(model, [0, 0, 0, 0])
                slot[0] += 1; slot[1] += good_int(r.get("input_tokens"))
                slot[2] += good_int(r.get("cache_read_tokens")); slot[3] += good_int(r.get("output_tokens"))
                if "flash" in model.lower(): flag_add(s, "gemini flash model", step)
            tcs = r.get("tool_calls") or []
            if tcs:
                pending, pending_step = [(step, tc) for tc in tcs], step
            elif pending and r.get("source") == "MODEL" and step > pending_step:
                _pstep, tc = pending.pop(0)
                content = r.get("content")
                s["results"].append({"step": step, "session": sid, "tool": tc.get("name") or "?",
                                     "hint": arg_hint(tc.get("args") or {}), "size": len(content) if isinstance(content, str) else 0})
            for tc in tcs:
                name = str(tc.get("name") or "?")
                args = tc.get("args") if isinstance(tc.get("args"), dict) else {}
                s["tools"][name] += 1
                if name in ("read_file", "view_file"):
                    raw = args.get("Path") or args.get("path") or args.get("File")
                    if raw:
                        s["reads"].append((str(raw), step))
                        if not any(args.get(k) not in (None, "", 0, "0") for k in ("StartLine", "EndLine", "start", "end")):
                            p = resolve_path(raw)
                            lines = update_line_cache(p) if p is not None and p.is_file() else None
                            if lines and lines > 300: s["unranged"].append({"path": str(p), "step": step, "lines": lines, "session": sid})
                        nlow = str(raw).replace("\\", "/").lower()
                        if "tools/parity_lab/" in nlow and nlow.endswith((".py", ".mjs")): flag_add(s, "parity_lab source read", step)
                        if "fluxer_desktop" in nlow and ("localappdata" in nlow or "appdata/local" in nlow): flag_add(s, "LOCALAPPDATA fluxer_desktop", step)
                if name == "run_command":
                    check_command(s, str(args.get("CommandLine") or args.get("Command") or ""), step)
                if name in ("invoke_subagent", "swarm"):
                    s["delegation_calls"] += 1
                    requested = args.get("Model") or args.get("model")
                    if isinstance(requested, str) and requested.strip():
                        m = requested.strip(); s["delegation_models"][m] += 1
                        if "flash" in m.lower(): flag_add(s, "flash delegation", step)
                        if m.lower() == "inherit": flag_add(s, "inherit delegation", step)
                if name == "ask_question": flag_add(s, "user question", step)
                if name in ("edit_file", "write_file"):
                    low = str(args.get("Path") or "").replace("\\", "/").lower()
                    if "fluxer_desktop" in low and ("localappdata" in low or "appdata/local" in low):
                        flag_add(s, "LOCALAPPDATA fluxer_desktop", step)
            content = r.get("content")
            if (not tcs and isinstance(content, str) and r.get("source") == "MODEL" and 0 < len(content) < 1500
                    and not content.startswith("Created At:") and content.rstrip().endswith(("?", "？")) and CONFIRM_RE.search(content)):
                flag_add(s, "user question", step)
            if r.get("error") or str(r.get("status") or "").upper() in ("ERROR", "FAILED"):
                s["errors"] += 1
                s["error_msgs"].append((short(r.get("error") or content or r.get("status"), 100), sid, step))
    repeats = Counter(os.path.normcase(os.path.normpath(str(raw).strip().strip("\"'`"))) for raw, _ in s["reads"])
    s["repeats"] = [(raw, n) for raw, n in repeats.items() if n >= 2]
    return s

def discover(brain_dir):
    return [p for p in brain_dir.iterdir() if (p / TRANSCRIPT).is_file()] if brain_dir.is_dir() else []

def first_ts(transcript):
    try:
        with open(transcript, encoding="utf-8", errors="replace") as fh:
            for line in fh:
                if line.strip():
                    return parse_ts(json.loads(line).get("created_at"))
    except (OSError, json.JSONDecodeError):
        return None
    return None

def pick_sessions(brain_dir, ids, since, latest):
    found = discover(brain_dir)
    if ids:
        picked = []
        for wanted in ids:
            hits = [p for p in found if p.name == wanted] or [p for p in found if p.name.startswith(wanted)]
            if len(hits) != 1:
                raise SystemExit(f"brain id not found or ambiguous: {wanted}")
            picked.append(hits[0])
        return picked
    if since is not None and not latest:
        return [p for p in found if (first_ts(p / TRANSCRIPT) or datetime.min.replace(tzinfo=timezone.utc)) >= since]
    return [max(found, key=lambda p: (p / TRANSCRIPT).stat().st_mtime)] if found else []

def select_and_analyze(args):
    since = parse_ts(args.since) if args.since else None
    if args.since and since is None:
        raise SystemExit(f"invalid --since: {args.since}")
    dirs = pick_sessions(Path(args.brain_dir), args.brain_ids, since, args.latest)
    if not dirs:
        raise SystemExit(f"no transcripts found under {args.brain_dir}")
    sessions = [analyze(p / TRANSCRIPT, p) for p in dirs]
    sessions.sort(key=lambda s: s["first"] or datetime.min.replace(tzinfo=timezone.utc))
    if args.brain_ids: root_ids = {s["id"] for s in sessions}
    elif since is not None and not args.latest: root_ids = {sessions[0]["id"]}
    else: root_ids = {dirs[-1].name}
    return sessions, root_ids

def aggregate(sessions):
    grand, tm = [0, 0, 0, 0], defaultdict(lambda: [0, 0, 0, 0])
    for s in sessions:
        for m, v in s["models"].items():
            for i in range(4): tm[m][i] += v[i]; grand[i] += v[i]
    return grand, tm

def render(sessions, root_ids, top_n):
    if not sessions:
        return ["AGY AUDIT: no sessions"]
    grand, tm = aggregate(sessions)
    tok = sum(grand[1:])
    root_tok = sum(total_tokens(s) for s in sessions if s["id"] in root_ids)
    pct = 100 * root_tok / tok if tok else 0
    firsts, lasts = [s["first"] for s in sessions if s["first"]], [s["last"] for s in sessions if s["last"]]
    span = fmt_dur((max(lasts) - min(firsts)).total_seconds()) if firsts and lasts else "?"
    L = [f"AGY AUDIT sessions={len(sessions)} root={','.join(sorted(x[:8] for x in root_ids))} span={span}",
         f"TOTALS steps={grand[0]} tokens={fmt_num(tok)} (in {fmt_num(grand[1])} / cache {fmt_num(grand[2])} / out {fmt_num(grand[3])})  root {pct:.0f}% other {100 - pct:.0f}%",
         "MODEL TOTALS " + " | ".join(f"{m} s={v[0]} in={fmt_num(v[1])} cache={fmt_num(v[2])} out={fmt_num(v[3])}" for m, v in sorted(tm.items(), key=lambda kv: -sum(kv[1][1:]))),
         "SESSIONS"]
    for i, s in enumerate(sessions[:8]):
        L.append(f"[{i + 1}] {s['id'][:8]} {fmt_dur((s['last'] - s['first']).total_seconds()) if s['first'] and s['last'] else '?'} steps={sum(v[0] for v in s['models'].values())} tok={fmt_num(total_tokens(s))} {'ROOT' if s['id'] in root_ids else 'other'}")
        ms = sorted(s["models"].items(), key=lambda kv: -sum(kv[1][1:]))
        L += [f"    {m[:30]:30} steps={v[0]:<4} in={fmt_num(v[1]):>7} cache={fmt_num(v[2]):>7} out={fmt_num(v[3]):>7}" for m, v in ms] if len(sessions) <= 4 else \
            ["    models " + " | ".join(f"{m} s={v[0]} in={fmt_num(v[1])} c={fmt_num(v[2])} o={fmt_num(v[3])}" for m, v in ms)]
        flags = ",".join(f"{FLAG_SHORT.get(k, k)}={len(v)}" for k, v in s["flags"].items() if v)
        L.append("    tools " + " ".join(f"{n}={c}" for n, c in s["tools"].most_common()) + f" | deleg calls={s['delegation_calls']} " + (",".join(f"{m}:{c}" for m, c in s["delegation_models"].most_common()) or "-") + f" oc/deepseek={s['opencode_cmds']}")
        L.append(f"    waste results={len(s['results'])} repeated={len(s['repeats'])} unranged={len(s['unranged'])} | flags {flags or '-'} | errors={s['errors']}")
    if len(sessions) > 8:
        L.append(f"    ... {len(sessions) - 8} more sessions not shown (totals include all)")
    results = sorted((r for s in sessions for r in s["results"]), key=lambda r: -r["size"])[:top_n]
    L.append(f"TOP TOOL RESULTS (top {top_n})")
    L += [f"  {r['size'] / 1024:7.1f}KB step={r['step']:<4} {r['tool'][:14]:14} {r['hint']} [{r['session'][:8]}]" for r in results] or ["  (none)"]
    reps = [(s["id"], raw, n) for s in sessions for raw, n in s["repeats"]]
    L.append(f"REPEATED READS paths={len(reps)}")
    L += [f"  {n}x {short(raw, 70)} [{sid[:8]}]" for sid, raw, n in sorted(reps, key=lambda x: -x[2])[:5]]
    unr = [u for s in sessions for u in s["unranged"]]
    L.append(f"UNRANGED BIG-FILE READS files={len(unr)}")
    L += [f"  {u['lines']} lines {short(u['path'], 65)} [{u['session'][:8]}]" for u in unr[:5]]
    all_flags = defaultdict(list)
    for s in sessions:
        for name, steps in s["flags"].items(): all_flags[name].extend(steps)
    L.append(f"RULE FLAGS total={sum(len(v) for v in all_flags.values())}")
    for name in FLAG_SHORT:
        steps = all_flags.get(name) or []
        if steps: L.append(f"  {name}: {len(steps)} [" + ", ".join(f"{sid[:8]}#{step}" for sid, step in steps[:3]) + "]")
    L.append(f"ERRORS total={sum(s['errors'] for s in sessions)}")
    L += [f"  {short(msg, 100)} [{sid[:8]}#{step}]" for msg, sid, step in [e for s in sessions for e in s["error_msgs"]][:3]]
    if len(L) > 70:
        L = L[:69] + [f"... output truncated ({len(L) - 69} lines)"]
    return L

def to_json(sessions, root_ids, top_n):
    grand, tm = aggregate(sessions)
    tok = sum(grand[1:])
    root_tok = sum(total_tokens(s) for s in sessions if s["id"] in root_ids)
    flags = defaultdict(list)
    for s in sessions:
        for name, steps in s["flags"].items(): flags[name].extend(steps)
    def models(d):
        return {m: {"steps": v[0], "input": v[1], "cache_read": v[2], "output": v[3]} for m, v in d.items()}
    out = [{
        "id": s["id"], "root": s["id"] in root_ids, "first": s["first"].isoformat() if s["first"] else None,
        "last": s["last"].isoformat() if s["last"] else None,
        "duration_seconds": (s["last"] - s["first"]).total_seconds() if s["first"] and s["last"] else None,
        "models": models(s["models"]), "tools": dict(s["tools"]),
        "delegation": {"calls": s["delegation_calls"], "models": dict(s["delegation_models"]), "opencode_deepseek_commands": s["opencode_cmds"]},
        "top_results": [{"step": r["step"], "tool": r["tool"], "hint": mask(r["hint"]), "bytes": r["size"]} for r in sorted(s["results"], key=lambda r: -r["size"])[:top_n]],
        "repeated_reads": [{"path": mask(raw), "count": n} for raw, n in s["repeats"]],
        "unranged_big_reads": [{"path": mask(u["path"]), "lines": u["lines"], "step": u["step"]} for u in s["unranged"]],
        "flags": {name: {"count": len(steps), "steps": [f"{sid}#{step}" for sid, step in steps[:3]]} for name, steps in s["flags"].items()},
        "errors": s["errors"], "error_messages": [mask(m) for m, _, _ in s["error_msgs"][:3]]} for s in sessions]
    results = sorted((r for s in sessions for r in s["results"]), key=lambda r: -r["size"])[:top_n]
    return {"root": sorted(root_ids),
            "totals": {"steps": grand[0], "input": grand[1], "cache_read": grand[2], "output": grand[3], "tokens": tok, "root_share": (root_tok / tok) if tok else 0.0},
            "models": models(tm), "sessions": out,
            "top_results": [{"session": r["session"], "step": r["step"], "tool": r["tool"], "hint": mask(r["hint"]), "bytes": r["size"]} for r in results],
            "flags": {name: {"count": len(steps), "steps": [f"{sid}#{step}" for sid, step in steps[:3]]} for name, steps in flags.items()},
            "errors": [{"message": mask(m), "session": sid, "step": step} for s in sessions for m, sid, step in s["error_msgs"]][:3]}

def selftest():
    bad = []
    def check(cond, label):
        if not cond: bad.append(label)
    base = datetime(2026, 1, 1, tzinfo=timezone.utc)
    def row(step, source, typ, **kw):
        return {"step_index": step, "source": source, "type": typ, "status": kw.pop("status", "DONE"),
                "created_at": (base + timedelta(seconds=step)).isoformat().replace("+00:00", "Z"), **kw}
    with tempfile.TemporaryDirectory() as tmp:
        brain = Path(tmp)
        big = brain / "big_file.txt"
        big.write_text("\n".join("line" for _ in range(301)) + "\n", encoding="utf-8")
        token = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U"
        a, b = brain / "aaaaaaaa-0000-0000-0000-000000000001", brain / "bbbbbbbb-0000-0000-0000-000000000002"
        (a / TRANSCRIPT).parent.mkdir(parents=True)
        rows = [row(0, "USER_EXPLICIT", "USER_INPUT", content="hi"),
                row(1, "MODEL", "PLANNER_RESPONSE", model="neon", input_tokens=100, cache_read_tokens=50, output_tokens=10, tool_calls=[
                    {"name": "read_file", "args": {"Path": str(big)}},
                    {"name": "read_file", "args": {"Path": str(big), "StartLine": 1, "EndLine": 9}},
                    {"name": "run_command", "args": {"CommandLine": "git push --force origin main"}},
                    {"name": "run_command", "args": {"CommandLine": '& "$env:LOCALAPPDATA\\fluxer_desktop\\Fluxer.exe"'}},
                    {"name": "run_command", "args": {"CommandLine": "SSL_CERT_FILE=1 ./fluxer_client.exe"}},
                    {"name": "run_command", "args": {"CommandLine": "Get-Content tools/parity_lab/x.py"}},
                    {"name": "read_file", "args": {"Path": "tools/parity_lab/y.mjs"}},
                    {"name": "invoke_subagent", "args": {"Model": "inherit", "Prompt": "x"}},
                    {"name": "run_command", "args": {"CommandLine": "opencode run hi"}}])]
        rows += [row(step, "MODEL", "GENERIC", content="Created At: x\nCompleted At: y\n" + "z" * (5000 if step == 2 else 100)) for step in range(2, 11)]
        rows += [row(11, "MODEL", "PLANNER_RESPONSE", model="gemini-flash-worker-agy", input_tokens=300, cache_read_tokens=100, output_tokens=20, tool_calls=[
                     {"name": "ask_question", "args": {"questions": []}},
                     {"name": "swarm", "args": {"action": "add", "model": "gemini-flash-x", "brief": "b"}}]),
                 row(12, "MODEL", "GENERIC", content="Created At: x\nok"),
                 row(13, "MODEL", "GENERIC", content="Created At: x\nok"),
                 row(14, "MODEL", "GENERIC", model="neon", input_tokens=20, output_tokens=5, status="ERROR", error="boom " + token, content="")]
        (a / TRANSCRIPT).write_text("\n".join(json.dumps(r) for r in rows) + "\n", encoding="utf-8")
        (b / TRANSCRIPT).parent.mkdir(parents=True)
        rows_b = [row(-3600, "MODEL", "PLANNER_RESPONSE", model="gemini-flash-worker-agy", input_tokens=10, output_tokens=1,
                      tool_calls=[{"name": "run_command", "args": {"CommandLine": "echo b"}}]),
                  row(-3599, "MODEL", "GENERIC", content="Created At: x\nok")]
        (b / TRANSCRIPT).write_text("\n".join(json.dumps(r) for r in rows_b) + "\n", encoding="utf-8")
        sa, sb = analyze(a / TRANSCRIPT, a), analyze(b / TRANSCRIPT, b)
        for key, count in {"read_file": 3, "run_command": 5, "ask_question": 1}.items():
            check(sa["tools"][key] == count, f"tool {key}")
        check(sa["delegation_calls"] == 2 and sa["delegation_models"]["inherit"] == 1 and sa["delegation_models"]["gemini-flash-x"] == 1, "delegation")
        check(len(sa["repeats"]) == 1 and sa["repeats"][0][1] == 2, "repeated reads")
        check(len(sa["unranged"]) == 1 and sa["unranged"][0]["lines"] == 301, "unranged read")
        check(sa["errors"] == 1 and max(r["size"] for r in sa["results"]) >= 5000, "errors/result")
        check(sa["models"]["neon"][0] == 2 and sa["models"]["neon"][1] == 120, "model steps/tokens")
        expected = {"parity_lab source read": 2, "git push --force": 1, "SSL_CERT_FILE": 1, "direct fluxer exe": 2,
                    "LOCALAPPDATA fluxer_desktop": 1, "gemini flash model": 1, "flash delegation": 1, "inherit delegation": 1, "user question": 1}
        for key, count in expected.items(): check(len(sa["flags"].get(key, [])) == count, f"flag {key}")
        check(mask(HOME + "\\private") == "~\\private" and token not in mask(token), "masking")
        check(token not in "\n".join(render([sa, sb], {sa["id"]}, 8)), "render mask")
        check([p.name for p in pick_sessions(brain, [], parse_ts("2025-12-31T23:30:00Z"), False)] == [a.name], "since selection")
        os.utime(a / TRANSCRIPT, (10_000_000_000, 10_000_000_000))
        check(pick_sessions(brain, [], None, True)[0].name == a.name, "latest selection")
        check(pick_sessions(brain, ["bbbb"], None, True)[0].name == b.name, "prefix selection")
        check(len(render([sa, sb], {sa["id"]}, 8)) <= 70 and len(render([sa], {sa["id"]}, 8)) > 5, "render shape")
    if bad:
        raise SystemExit("SELFTEST FAILED: " + ", ".join(bad))
    print("SELFTEST OK (all checks passed)")

def main():
    try:
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    except (AttributeError, OSError):
        pass
    parser = argparse.ArgumentParser(description="Audit Antigravity CLI transcripts (read-only)")
    parser.add_argument("brain_ids", nargs="*", help="brain/session id or unique prefix")
    parser.add_argument("--latest", action="store_true", help="most recently modified brain (default)")
    parser.add_argument("--since", help="ISO8601: include brains whose first created_at >= since")
    parser.add_argument("--json", action="store_true", help="machine-readable output")
    parser.add_argument("--top", type=int, default=8, help="largest tool results to show (default 8)")
    parser.add_argument("--brain-dir", default=str(BRAIN_DIR), help="transcript root")
    parser.add_argument("--selftest", action="store_true", help="run synthetic parser tests")
    args = parser.parse_args()
    if args.top < 1:
        parser.error("--top must be >= 1")
    if args.selftest:
        selftest()
        return
    sessions, root_ids = select_and_analyze(args)
    if args.json:
        print(json.dumps(to_json(sessions, root_ids, args.top), ensure_ascii=True, indent=2))
    else:
        print("\n".join(render(sessions, root_ids, args.top)))


if __name__ == "__main__":
    main()
