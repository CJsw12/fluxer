#!/usr/bin/env python3
"""External stateless batch runner for Fluxer parity coordinator."""

from __future__ import annotations

import argparse
import contextlib
import dataclasses
import datetime as dt
import json
import os
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from typing import Any, Callable

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8")
if hasattr(sys.stderr, "reconfigure"):
    sys.stderr.reconfigure(encoding="utf-8")

ALLOWED_MODELS = frozenset({"gemini-4-neon"})
ALLOWED_EFFORTS = frozenset({"medium", "high"})
VALID_RESULT_STATUSES = frozenset(
    {
        "done",
        "continue",
        "stop_t3",
        "stop_backlog_empty",
        "stop_blocked",
        "stop_gate_failed",
    }
)
CONTINUE_STATUSES = frozenset({"done", "continue"})
STOP_STATUSES = VALID_RESULT_STATUSES - CONTINUE_STATUSES
REQUIRED_RESULT_KEYS = frozenset(
    {
        "status",
        "summary",
        "next_task",
        "failed_gate",
        "failed_feature",
        "friction",
        "decisions",
    }
)
ENV_STRIP_PREFIXES = ("ORCA_AGENT_", "ORCA_STRUCTURED_", "ANTIGRAVITY_")

DEFAULT_STATE_PATH = Path("Logs/handoff/runner_state.json")
DEFAULT_LOCK_PATH = Path("Logs/handoff/coordinator_runner.lock")
DEFAULT_OPERATIONS_PATH = Path("Logs/handoff/OPERATIONS.md")
DEFAULT_RETRO_PATH = Path("Logs/handoff/worker_retro.md")
DEFAULT_RUNNER_DIR = Path("target/parity-tmp/runner")

EXIT_OK = 0
EXIT_ERROR = 1
EXIT_LOCK_CONFLICT = 2
EXIT_ACTIVE_DISPATCH = 3
EXIT_GATE_FAILED = 4
EXIT_STOPPED = 5


class RunnerError(Exception):
    """Domain error with an associated process exit code."""

    def __init__(self, message: str, exit_code: int = EXIT_ERROR) -> None:
        super().__init__(message)
        self.exit_code = exit_code


def utc_now_iso() -> str:
    return dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat()


def clean_subprocess_env(base_env: dict[str, str] | None = None) -> dict[str, str]:
    """Return a copy of environment variables without agent/coordinator contamination."""
    src = os.environ if base_env is None else base_env
    return {
        k: v
        for k, v in src.items()
        if not any(k.startswith(prefix) for prefix in ENV_STRIP_PREFIXES)
    }


def validate_model_and_effort(model: str, effort: str) -> None:
    """Reject disallowed model or effort before launching any batch."""
    if model not in ALLOWED_MODELS:
        raise RunnerError(
            f"허용되지 않은 모델입니다: {model!r} (허용: {sorted(ALLOWED_MODELS)})",
            EXIT_ERROR,
        )
    if effort not in ALLOWED_EFFORTS:
        raise RunnerError(
            f"허용되지 않은 effort입니다: {effort!r} (허용: {sorted(ALLOWED_EFFORTS)})",
            EXIT_ERROR,
        )


def default_state() -> dict[str, Any]:
    return {
        "generation": 0,
        "consecutive_no_progress": 0,
        "gate_failure_streak": {},
        "feature_failures": {},
        "active_dispatch": None,
        "history": [],
    }


def load_state(state_path: Path) -> dict[str, Any]:
    """Load runner ledger or return initialized defaults if missing."""
    if not state_path.exists():
        return default_state()
    try:
        raw = json.loads(state_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise RunnerError(f"상태 파일 읽기 실패 ({state_path}): {exc}", EXIT_ERROR) from exc
    if not isinstance(raw, dict):
        raise RunnerError(f"상태 파일 형식이 올바르지 않습니다: {state_path}", EXIT_ERROR)

    state = default_state()
    state["generation"] = int(raw.get("generation", 0))
    state["consecutive_no_progress"] = int(raw.get("consecutive_no_progress", 0))
    state["gate_failure_streak"] = dict(raw.get("gate_failure_streak") or {})
    state["feature_failures"] = dict(raw.get("feature_failures") or {})
    state["active_dispatch"] = raw.get("active_dispatch")
    history = list(raw.get("history") or [])
    state["history"] = history[-30:]
    return state


def save_state_atomic(state_path: Path, state: dict[str, Any]) -> None:
    """Atomically persist state via temporary file in the same directory + os.replace."""
    state_path.parent.mkdir(parents=True, exist_ok=True)
    normalized = dict(state)
    normalized["history"] = list(normalized.get("history") or [])[-30:]
    payload = json.dumps(normalized, ensure_ascii=False, indent=2) + "\n"

    fd, tmp_name = tempfile.mkstemp(
        prefix=f".{state_path.name}.",
        suffix=".tmp",
        dir=str(state_path.parent),
    )
    tmp_path = Path(tmp_name)
    try:
        with os.fdopen(fd, "w", encoding="utf-8", newline="\n") as fh:
            fh.write(payload)
            fh.flush()
            os.fsync(fh.fileno())
        os.replace(tmp_path, state_path)
    except Exception:
        with contextlib.suppress(OSError):
            tmp_path.unlink()
        raise


class ExclusiveFileLock:
    """Cross-platform non-blocking exclusive OS file lock (msvcrt on Windows, fcntl on Unix)."""

    def __init__(self, lock_path: Path) -> None:
        self.lock_path = lock_path
        self._fd: int | None = None

    def acquire(self) -> None:
        self.lock_path.parent.mkdir(parents=True, exist_ok=True)
        fd = os.open(str(self.lock_path), os.O_CREAT | os.O_RDWR, 0o666)
        try:
            if os.name == "nt":
                import msvcrt

                os.lseek(fd, 0, os.SEEK_SET)
                msvcrt.locking(fd, msvcrt.LK_NBLCK, 1)
            else:
                import fcntl

                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError as exc:
            os.close(fd)
            raise RunnerError(
                f"이미 다른 coordinator_runner 인스턴스가 실행 중입니다 ({self.lock_path}).",
                EXIT_LOCK_CONFLICT,
            ) from exc

        self._fd = fd
        with contextlib.suppress(OSError):
            os.lseek(fd, 0, os.SEEK_SET)
            os.ftruncate(fd, 0)
            os.write(fd, f"pid={os.getpid()} started_at={utc_now_iso()}\n".encode("utf-8"))

    def release(self) -> None:
        if self._fd is None:
            return
        fd = self._fd
        self._fd = None
        try:
            if os.name == "nt":
                import msvcrt

                with contextlib.suppress(OSError):
                    os.lseek(fd, 0, os.SEEK_SET)
                    msvcrt.locking(fd, msvcrt.LK_UNLCK, 1)
            else:
                import fcntl

                with contextlib.suppress(OSError):
                    fcntl.flock(fd, fcntl.LOCK_UN)
        finally:
            with contextlib.suppress(OSError):
                os.close(fd)

    def __enter__(self) -> "ExclusiveFileLock":
        self.acquire()
        return self

    def __exit__(self, exc_type: Any, exc: Any, tb: Any) -> None:
        self.release()


@dataclasses.dataclass
class GitSnapshot:
    head: str
    branch: str
    dirty_lines: list[str]
    unpushed_count: int


class SystemAdapter:
    """Subprocess boundary for Git and Orca CLI calls (mockable in --selftest)."""

    def run_cmd(
        self,
        args: list[str],
        *,
        timeout_sec: int = 60,
        cwd: Path | None = None,
    ) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            args,
            cwd=str(cwd) if cwd else None,
            env=clean_subprocess_env(),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout_sec,
            check=False,
        )

    def _git_stdout(self, args: list[str]) -> str:
        proc = self.run_cmd(["git", *args], timeout_sec=30)
        if proc.returncode != 0:
            detail = (proc.stderr or proc.stdout or "").strip()
            raise RunnerError(f"git {' '.join(args)} 실패: {detail}", EXIT_GATE_FAILED)
        return proc.stdout

    def git_snapshot(self) -> GitSnapshot:
        head = self._git_stdout(["rev-parse", "HEAD"]).strip()
        branch = self._git_stdout(["rev-parse", "--abbrev-ref", "HEAD"]).strip()
        porcelain = self._git_stdout(["status", "--porcelain"])
        dirty_lines = [line for line in porcelain.splitlines() if line.strip()]
        unpushed_raw = self._git_stdout(["rev-list", "--count", "@{u}..HEAD"]).strip()
        try:
            unpushed_count = int(unpushed_raw)
        except ValueError as exc:
            raise RunnerError(
                f"미푸시 커밋 수 파싱 실패: {unpushed_raw!r}", EXIT_GATE_FAILED
            ) from exc
        return GitSnapshot(
            head=head,
            branch=branch,
            dirty_lines=dirty_lines,
            unpushed_count=unpushed_count,
        )

    def _run_orca_json(self, args: list[str], *, timeout_sec: int) -> dict[str, Any]:
        proc = self.run_cmd(["orca", *args], timeout_sec=timeout_sec)
        stdout = (proc.stdout or "").strip()
        stderr = (proc.stderr or "").strip()
        if proc.returncode != 0:
            raise RunnerError(
                f"orca {' '.join(args[:3])} 실패 (exit={proc.returncode}): {stderr or stdout}",
                EXIT_ERROR,
            )
        try:
            data = json.loads(stdout)
        except json.JSONDecodeError as exc:
            raise RunnerError(
                f"orca JSON 파싱 실패 ({' '.join(args[:3])}): {stdout[:300]}",
                EXIT_ERROR,
            ) from exc
        if not isinstance(data, dict) or not data.get("ok"):
            raise RunnerError(
                f"orca 응답 ok!=true ({' '.join(args[:3])}): {data}",
                EXIT_ERROR,
            )
        return data

    def create_terminal(self) -> str:
        data = self._run_orca_json(
            ["terminal", "create", "--worktree", "current", "--json"],
            timeout_sec=30,
        )
        handle = ((data.get("result") or {}).get("terminal") or {}).get("handle")
        if not handle or not isinstance(handle, str):
            raise RunnerError(f"orca terminal handle 누락: {data}", EXIT_ERROR)
        return handle

    def close_terminal(self, handle: str) -> None:
        self._run_orca_json(
            ["terminal", "close", "--terminal", handle, "--tab", "--json"],
            timeout_sec=30,
        )

    def worker_start(
        self,
        *,
        from_handle: str,
        model: str,
        effort: str,
        task_title: str,
        spec_file: Path,
    ) -> dict[str, Any]:
        data = self._run_orca_json(
            [
                "orchestration",
                "worker-start",
                "--environment",
                "local",
                "--from",
                from_handle,
                "--worktree",
                "current",
                "--agent",
                "antigravity",
                "--model",
                model,
                "--effort",
                effort,
                "--task-title",
                task_title,
                "--spec-file",
                str(spec_file),
                "--json",
            ],
            timeout_sec=60,
        )
        result = data.get("result")
        if not isinstance(result, dict):
            raise RunnerError(f"worker-start result 누락: {data}", EXIT_ERROR)
        actual_model = result.get("model")
        actual_effort = result.get("effort")
        if actual_model != model or actual_effort != effort:
            raise RunnerError(
                f"worker-start 응답 모델/effort 불일치: 요청=({model}, {effort}), 실제=({actual_model}, {actual_effort})",
                EXIT_ERROR,
            )
        for key in ("dispatchId", "taskId", "runId"):
            if not result.get(key):
                raise RunnerError(f"worker-start 응답에 {key} 누락: {result}", EXIT_ERROR)
        return result

    def check_wait(
        self,
        *,
        terminal_handle: str,
        run_id: str,
        timeout_ms: int,
    ) -> tuple[list[dict[str, Any]], str | None]:
        timeout_sec = max(30, (timeout_ms // 1000) + 30)
        data = self._run_orca_json(
            [
                "orchestration",
                "check",
                "--environment",
                "local",
                "--terminal",
                terminal_handle,
                "--run",
                run_id,
                "--wait",
                "--timeout-ms",
                str(timeout_ms),
                "--json",
            ],
            timeout_sec=timeout_sec,
        )
        result = data.get("result") or {}
        messages = result.get("messages")
        delivery_id = result.get("deliveryId")
        if delivery_id is not None:
            delivery_id = str(delivery_id)
        if messages is None:
            return [], delivery_id
        if not isinstance(messages, list):
            raise RunnerError(f"check 응답 messages 형식 오류: {data}", EXIT_ERROR)
        return [m for m in messages if isinstance(m, dict)], delivery_id

    def ack_message(
        self,
        *,
        terminal_handle: str,
        run_id: str,
        msg_id: str,
    ) -> None:
        self._run_orca_json(
            [
                "orchestration",
                "check",
                "--environment",
                "local",
                "--terminal",
                terminal_handle,
                "--run",
                run_id,
                "--ack",
                msg_id,
                "--json",
            ],
            timeout_sec=30,
        )

    def audit_since(self, started_at: str) -> dict[str, Any] | None:
        """Run agy_audit.py --since <started_at> --json; return parsed dict or None."""
        audit_script = Path("tools/agents/agy_audit.py")
        if not audit_script.is_file():
            return None
        try:
            proc = self.run_cmd(
                [sys.executable, "-I", str(audit_script), "--since", started_at, "--json"],
                timeout_sec=30,
            )
        except Exception:
            return None
        if proc.returncode != 0:
            return None
        try:
            data = json.loads((proc.stdout or "").strip())
        except json.JSONDecodeError:
            return None
        return data if isinstance(data, dict) else None


def check_pre_batch_gate(
    *,
    state: dict[str, Any],
    snapshot: GitSnapshot,
    allow_dirty: bool,
    operations_path: Path = DEFAULT_OPERATIONS_PATH,
) -> list[str]:
    """Return list of Pre-Batch Gate failure reasons (empty means pass)."""
    failures: list[str] = []
    if not allow_dirty and snapshot.dirty_lines:
        failures.append(
            f"작업 트리가 깨끗하지 않습니다 (git status --porcelain: {len(snapshot.dirty_lines)}건)."
        )
    if snapshot.unpushed_count > 0:
        failures.append(
            f"미푸시 커밋이 {snapshot.unpushed_count}건 있습니다 (git rev-list --count @{{u}}..HEAD)."
        )
    if not operations_path.is_file():
        failures.append(f"운영 상태 파일이 없습니다: {operations_path}")
    if int(state.get("consecutive_no_progress", 0)) >= 2:
        failures.append(
            f"연속 무진전 Circuit Breaker 발동 (consecutive_no_progress={state.get('consecutive_no_progress')})."
        )
    blocked_gates = [
        f"{gate}={count}"
        for gate, count in sorted((state.get("gate_failure_streak") or {}).items())
        if int(count) >= 2
    ]
    if blocked_gates:
        failures.append(
            f"동일 게이트 2회 이상 연속 실패로 차단됨: {', '.join(blocked_gates)}"
        )
    return failures


def validate_batch_result_schema(data: Any) -> dict[str, Any]:
    """Validate batch_result_gen_<N>.json schema and return normalized dict."""
    if not isinstance(data, dict):
        raise RunnerError("batch_result JSON 루트는 객체여야 합니다.", EXIT_ERROR)

    missing = sorted(REQUIRED_RESULT_KEYS - set(data.keys()))
    if missing:
        raise RunnerError(
            f"batch_result 필수 키가 누락되었습니다: {', '.join(missing)}",
            EXIT_ERROR,
        )

    status = data.get("status")
    if not isinstance(status, str) or status not in VALID_RESULT_STATUSES:
        raise RunnerError(
            f"batch_result status 값이 올바르지 않습니다: {status!r} (허용: {sorted(VALID_RESULT_STATUSES)})",
            EXIT_ERROR,
        )

    summary = data.get("summary")
    if not isinstance(summary, str) or not summary.strip():
        raise RunnerError("batch_result summary는 비어 있지 않은 문자열이어야 합니다.", EXIT_ERROR)

    for nullable_str_key in ("next_task", "failed_gate", "failed_feature"):
        val = data.get(nullable_str_key)
        if val is not None and not isinstance(val, str):
            raise RunnerError(
                f"batch_result {nullable_str_key}는 문자열 또는 null이어야 합니다.",
                EXIT_ERROR,
            )

    friction = data.get("friction")
    if friction is not None and not isinstance(friction, (str, list)):
        raise RunnerError(
            "batch_result friction은 문자열, 문자열 목록 또는 null이어야 합니다.",
            EXIT_ERROR,
        )
    if isinstance(friction, list) and not all(isinstance(item, str) for item in friction):
        raise RunnerError(
            "batch_result friction 목록의 모든 항목은 문자열이어야 합니다.",
            EXIT_ERROR,
        )

    decisions = data.get("decisions")
    if decisions is not None and not (
        isinstance(decisions, list) and all(isinstance(item, str) for item in decisions)
    ):
        raise RunnerError(
            "batch_result decisions는 문자열 목록 또는 null이어야 합니다.",
            EXIT_ERROR,
        )

    return data


def load_batch_result(result_path: Path) -> dict[str, Any]:
    if not result_path.is_file():
        raise RunnerError(f"배치 결과 파일이 생성되지 않았습니다: {result_path}", EXIT_ERROR)
    try:
        raw = json.loads(result_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise RunnerError(f"배치 결과 JSON 읽기 실패 ({result_path}): {exc}", EXIT_ERROR) from exc
    return validate_batch_result_schema(raw)


def generate_batch_brief(
    *,
    generation: int,
    state: dict[str, Any],
    snapshot: GitSnapshot,
    brief_path: Path,
    result_path: Path,
    smoke_only: bool,
) -> str:
    """Create deterministic batch brief markdown and write it to brief_path."""
    brief_path.parent.mkdir(parents=True, exist_ok=True)
    result_path.parent.mkdir(parents=True, exist_ok=True)
    if result_path.exists():
        result_path.unlink()

    history = state.get("history") or []
    prev_summary = "없음 (첫 배치)"
    prev_next_task = "없음"
    if history:
        last = history[-1]
        prev_summary = (
            f"gen={last.get('generation')}, status={last.get('status')}, "
            f"committed={last.get('committed')}, summary={last.get('summary')}"
        )
        if last.get("next_task"):
            prev_next_task = str(last.get("next_task"))

    recent_frictions: list[str] = []
    for entry in reversed(history):
        fr = normalize_friction(entry.get("friction"))
        if fr:
            recent_frictions.append(f"- gen={entry.get('generation')}: {fr}")
            if len(recent_frictions) >= 3:
                break
    recent_frictions.reverse()
    friction_block = "\n".join(recent_frictions) if recent_frictions else "- 없음"

    feature_failures: dict[str, int] = dict(state.get("feature_failures") or {})
    skip_features = [k for k, v in sorted(feature_failures.items()) if int(v) >= 2]
    warn_features = [f"{k}({v}회)" for k, v in sorted(feature_failures.items()) if int(v) == 1]

    gate_streaks: dict[str, int] = dict(state.get("gate_failure_streak") or {})
    gate_lines = [f"{k}: {v}회" for k, v in sorted(gate_streaks.items()) if int(v) > 0]

    result_rel = result_path.as_posix()

    if smoke_only:
        content = f"""# Smoke Batch Gen {generation}

이 배치는 E2E 스모크 테스트 전용입니다. 코드·설정·Git 상태를 변경하지 마십시오.

## 현재 컨텍스트
- Generation: {generation}
- Branch: `{snapshot.branch}`
- HEAD: `{snapshot.head}`

## 수행 작업
1. 아래 경로에 스모크 결과 JSON을 작성하십시오: `{result_rel}`
2. JSON 스키마:
```json
{{
  "status": "done",
  "summary": "smoke-only E2E check succeeded",
  "next_task": null,
  "failed_gate": null,
  "failed_feature": null,
  "friction": null,
  "decisions": ["smoke-only batch completed without repo edits"]
}}
```
3. 파일 작성 후 즉시 `worker_done`을 전송하고 종료하십시오.
"""
    else:
        skip_text = ", ".join(skip_features) if skip_features else "없음"
        warn_text = ", ".join(warn_features) if warn_features else "없음"
        gate_text = ", ".join(gate_lines) if gate_lines else "없음"
        content = f"""# Parity Coordinator Batch Gen {generation}

## 현재 컨텍스트
- Generation: {generation}
- Branch: `{snapshot.branch}`
- HEAD: `{snapshot.head}`
- 직전 세대 요약: {prev_summary}
- 직전 권장 다음 작업: {prev_next_task}
- 연속 무진전 횟수: {state.get('consecutive_no_progress', 0)} / 2
- 건너뛰기 목록 (2회 이상 실패): {skip_text}
- 1회 실패 기능 주의: {warn_text}
- 게이트 연속 실패 현황: {gate_text}

## 직전 마찰·개선 포인트 (재귀 개선, 2회 반복 시 도구·규칙 우선 보강)
{friction_block}

## 내부 위임 원칙
- T0 탐색·로그 요약은 DeepSeek(`opencode run -m deepseek/deepseek-flash`)을 허용합니다.
- T1 화면 수치 맞추기·단순 구현 위임은 `swarm`에 `gemini-4-hydrogen-medium`을 지정해 맡기십시오.
- **T3 고위험(삭제·밴·권한·인증·저장 형식):** 비T3 백로그를 우선 소진하되, T3 항목에 도달하면 멈추지 말고 `swarm`에 `gemini-4-neon-high`로 구현한 뒤 **다른 계열 독립 검토**(`codex exec -m gpt-6.2-sol -s read-only`)를 거쳐 치명적 지적이 없을 때만 반영하십시오. 실기 테스트 서버에서 실제 삭제·밴·길드 탈퇴는 실행하지 않고 확인 모달·단위 테스트(롤백 포함)까지만 검증합니다.
- **중첩 Orca 워커(`orca orchestration worker-start`) 호출은 금지합니다.**

## 멈춤 조건 (해당 시 즉시 작업 중단 후 아래 결과 JSON에 사유 기록)
- T3 교차 검토 불가(예: Codex 한도 소진 또는 해소 불가 보안·데이터 손실 위험) → `status: "stop_t3"`
- 동일 게이트 2회 연속 실패 → `status: "stop_gate_failed"`
- 백로그 소진 → `status: "stop_backlog_empty"`
- 외부 승인·해결 불가 차단 → `status: "stop_blocked"`

## 완료 시 필수 산출물
배치 종료 전 반드시 `{result_rel}` 파일을 작성하십시오.
모든 키가 필수입니다:
```json
{{
  "status": "done | continue | stop_t3 | stop_backlog_empty | stop_blocked | stop_gate_failed",
  "summary": "이번 배치 수행 요약",
  "next_task": "다음 배치 권장 작업 또는 null",
  "failed_gate": "실패한 게이트 이름 또는 null",
  "failed_feature": "실패한 기능 이름 또는 null",
  "friction": "마찰/낭비 지점 요약 또는 null",
  "decisions": ["주요 결정 사항"]
}}
```
"""

    brief_path.write_text(content, encoding="utf-8", newline="\n")
    return content


def normalize_friction(value: Any) -> str | None:
    """Normalize friction field (str, list[str], or None) into a single-line string."""
    if value is None:
        return None
    if isinstance(value, list):
        parts = [" ".join(str(x).split()) for x in value if str(x).strip()]
        joined = "; ".join(parts).strip()
        return joined or None
    text = " ".join(str(value).split()).strip()
    if not text or text.lower() in {"none", "null", "없음", "n/a"}:
        return None
    return text


def _fmt_duration_short(seconds: int | float) -> str:
    sec = max(0, int(seconds))
    m, s = divmod(sec, 60)
    return f"{m}m{s:02d}s" if m else f"{s}s"


def _fmt_tokens_short(n: int) -> str:
    if n >= 1_000_000:
        return f"{n / 1_000_000:.1f}M"
    if n >= 1_000:
        return f"{n / 1_000:.1f}k"
    return str(n)


def _escape_md_cell(text: str, limit: int = 240) -> str:
    cleaned = " ".join(text.replace("|", "/").split())
    if len(cleaned) > limit:
        return cleaned[: limit - 1] + "…"
    return cleaned or "-"


def append_retro_entry(
    *,
    retro_path: Path,
    generation: int,
    model: str,
    effort: str,
    batch_result: dict[str, Any],
    committed: bool,
    audit_data: dict[str, Any] | None,
) -> None:
    """Append one batch retrospective row (subjective friction + objective agy_audit KPIs)."""
    retro_path.parent.mkdir(parents=True, exist_ok=True)
    if not retro_path.exists():
        header = (
            "# 워커 재귀 개선 회고 로그 (로컬 전용, Git 제외)\n\n"
            "| 날짜 | 작업 | brain / 모델 | 시간 · 스텝 · 토큰 | KPI (`rep` / `>8KB` / `err` / `rework`) | 결과 | Friction (주관+객관) | 반영한 개선 |\n"
            "|---|---|---|---|---|---|---|---|\n"
        )
        retro_path.write_text(header, encoding="utf-8", newline="\n")

    today = dt.datetime.now().strftime("%Y-%m-%d")
    brain_cell = f"- / `{model}-{effort}`"
    time_cell = "-"
    kpi_cell = "- / - / - / -"
    audit_flags_text = ""

    if isinstance(audit_data, dict):
        sessions = audit_data.get("sessions") or []
        if sessions and isinstance(sessions[0], dict):
            s0 = sessions[0]
            sid = str(s0.get("id") or "")[:8] or "-"
            brain_cell = f"`{sid}` / `{model}-{effort}`"
            dur = _fmt_duration_short(s0.get("duration_seconds") or 0)
            tools_map = s0.get("tools") or {}
            steps = sum(int(v) for v in tools_map.values()) if isinstance(tools_map, dict) else 0
            models_map = s0.get("models") or {}
            toks = 0
            if isinstance(models_map, dict):
                for vals in models_map.values():
                    if isinstance(vals, list) and len(vals) >= 2:
                        toks += sum(int(x) for x in vals[1:])
            time_cell = f"{dur} · {steps}s · {_fmt_tokens_short(toks)}"
            rep_cnt = len(s0.get("repeated_reads") or [])
            big_cnt = sum(
                1 for r in (s0.get("top_results") or []) if isinstance(r, dict) and int(r.get("bytes") or 0) > 8192
            )
            err_cnt = int(s0.get("errors") or 0)
            rework_cnt = 1 if (batch_result.get("failed_gate") or batch_result.get("failed_feature")) else 0
            kpi_cell = f"{rep_cnt} / {big_cnt} / {err_cnt} / {rework_cnt}"
            flags = s0.get("flags") or {}
            if isinstance(flags, dict) and flags:
                audit_flags_text = f" [audit flags: {', '.join(sorted(flags.keys()))}]"

    friction_str = normalize_friction(batch_result.get("friction")) or "없음"
    friction_cell = _escape_md_cell(friction_str + audit_flags_text, limit=260)
    result_cell = _escape_md_cell(
        f"status={batch_result.get('status')}, committed={committed}", limit=80
    )
    next_cell = _escape_md_cell(str(batch_result.get("next_task") or "-"), limit=140)
    row = (
        f"| {today} | `Gen {generation}` | {brain_cell} | {time_cell} | "
        f"{kpi_cell} | {result_cell} | {friction_cell} | {next_cell} |\n"
    )
    with retro_path.open("a", encoding="utf-8", newline="\n") as fh:
        fh.write(row)


def evaluate_post_batch(
    *,
    generation: int,
    state: dict[str, Any],
    head_before: str,
    post_snapshot: GitSnapshot,
    batch_result: dict[str, Any],
    allow_dirty: bool,
    smoke_only: bool,
) -> tuple[bool, str]:
    """Update state from batch result and git state; return (should_continue, reason)."""
    status = batch_result["status"]
    committed = post_snapshot.head != head_before

    if smoke_only:
        state["history"].append(
            {
                "generation": generation,
                "finished_at": utc_now_iso(),
                "status": status,
                "committed": committed,
                "head_before": head_before,
                "head_after": post_snapshot.head,
                "summary": batch_result["summary"],
            }
        )
        state["history"] = state["history"][-30:]
        return False, "smoke-only 배치 완료"

    if committed:
        state["consecutive_no_progress"] = 0
    else:
        state["consecutive_no_progress"] = int(state.get("consecutive_no_progress", 0)) + 1

    failed_gate = batch_result.get("failed_gate")
    gate_streaks: dict[str, int] = dict(state.get("gate_failure_streak") or {})
    if failed_gate:
        gate_streaks[failed_gate] = int(gate_streaks.get(failed_gate, 0)) + 1
    elif status in CONTINUE_STATUSES:
        gate_streaks = {}
    state["gate_failure_streak"] = gate_streaks

    failed_feature = batch_result.get("failed_feature")
    feature_failures: dict[str, int] = dict(state.get("feature_failures") or {})
    if failed_feature:
        feature_failures[failed_feature] = int(feature_failures.get(failed_feature, 0)) + 1
    state["feature_failures"] = feature_failures

    state["history"].append(
        {
            "generation": generation,
            "finished_at": utc_now_iso(),
            "status": status,
            "committed": committed,
            "head_before": head_before,
            "head_after": post_snapshot.head,
            "failed_gate": failed_gate,
            "failed_feature": failed_feature,
            "friction": normalize_friction(batch_result.get("friction")),
            "next_task": batch_result.get("next_task"),
            "summary": batch_result["summary"],
        }
    )
    state["history"] = state["history"][-30:]

    if status in STOP_STATUSES:
        return False, f"배치 결과 상태가 즉시 정지 조건입니다: {status}"
    if int(state["consecutive_no_progress"]) >= 2:
        return False, "연속 2회 무진전(커밋 없음)으로 Circuit Breaker가 발동했습니다."
    blocked_gates = [f"{k}={v}" for k, v in sorted(gate_streaks.items()) if int(v) >= 2]
    if blocked_gates:
        return False, f"동일 게이트 2회 연속 실패로 정지합니다: {', '.join(blocked_gates)}"
    if not allow_dirty and post_snapshot.dirty_lines:
        return (
            False,
            f"배치 종료 후 미커밋 변경이 남아 있어 다음 배치를 중단합니다 ({len(post_snapshot.dirty_lines)}건).",
        )
    if post_snapshot.unpushed_count > 0:
        return (
            False,
            f"배치 종료 후 미푸시 커밋이 남아 있어 다음 배치를 중단합니다 ({post_snapshot.unpushed_count}건).",
        )
    if status == "done":
        return False, "배치가 완료(done)를 보고했습니다."
    return True, "다음 배치 진행 가능"


def _is_nonfatal_progress_notice(msg: dict[str, Any]) -> bool:
    """Return True for Orca's informational stall-watchdog escalation notices."""
    if msg.get("type") != "escalation":
        return False
    subject = str(msg.get("subject") or "").strip().lower()
    body = str(msg.get("body") or "").strip().lower()
    return subject == "worker has made no progress" or "nothing was stopped" in body


TRANSIENT_QUOTA_MARKERS = (
    "quota",
    "rate limit",
    "rate_limit",
    "429",
    "resource_exhausted",
    "capacity",
    "overloaded",
    "temporarily unavailable",
    "too many requests",
    "connection reset",
    "timed out",
)


def _is_transient_or_quota_error(exc: Exception) -> bool:
    """Return True for quota/rate-limit/transient errors that should be retried after backoff."""
    text = str(exc).lower()
    return any(marker in text for marker in TRANSIENT_QUOTA_MARKERS)


def recover_state(state_path: Path) -> int:
    """Inspect and clear residual active_dispatch so the runner can resume."""
    state = load_state(state_path)
    active = state.get("active_dispatch")
    if active is None:
        print(f"[recover] active_dispatch가 없습니다. 상태가 이미 정상입니다 ({state_path}).")
        return EXIT_OK

    print(f"[recover] 잔류 active_dispatch 발견: {json.dumps(active, ensure_ascii=False)}")
    state["active_dispatch"] = None
    state["history"].append(
        {
            "generation": active.get("generation", state.get("generation", 0)),
            "finished_at": utc_now_iso(),
            "status": "recovered",
            "committed": False,
            "summary": "manual --recover cleared residual active_dispatch",
        }
    )
    save_state_atomic(state_path, state)
    print(f"[recover] active_dispatch를 초기화했습니다 ({state_path}).")
    return EXIT_OK


def run_batches(
    args: argparse.Namespace,
    sys_adapter: SystemAdapter | None = None,
    operations_path: Path = DEFAULT_OPERATIONS_PATH,
    runner_dir: Path = DEFAULT_RUNNER_DIR,
    retro_path: Path | None = DEFAULT_RETRO_PATH,
    sleep_fn: Callable[[float], None] = time.sleep,
) -> int:
    """Execute --recover, --dry-run, or live batch loop."""
    adapter = sys_adapter or SystemAdapter()
    state_path = Path(args.state_path)
    lock_path = Path(args.lock_path)
    effective_retro_path = (
        None
        if isinstance(adapter, _FakeAdapter) and retro_path == DEFAULT_RETRO_PATH
        else retro_path
    )

    with ExclusiveFileLock(lock_path):
        if args.recover:
            return recover_state(state_path)

        validate_model_and_effort(args.model, args.effort)
        state = load_state(state_path)
        if state.get("active_dispatch") is not None:
            raise RunnerError(
                "이전 배치의 active_dispatch가 남아 있어 재발사를 차단합니다. "
                "`--recover`로 상태를 확인·초기화한 뒤 다시 실행하십시오.",
                EXIT_ACTIVE_DISPATCH,
            )

        if args.dry_run:
            snapshot = adapter.git_snapshot()
            gate_failures = check_pre_batch_gate(
                state=state,
                snapshot=snapshot,
                allow_dirty=args.allow_dirty,
                operations_path=operations_path,
            )
            if gate_failures:
                raise RunnerError(
                    "Pre-Batch Gate 실패:\n- " + "\n- ".join(gate_failures),
                    EXIT_GATE_FAILED,
                )
            next_gen = int(state.get("generation", 0)) + 1
            brief_path = runner_dir / f"batch_gen_{next_gen}.md"
            result_path = runner_dir / f"batch_result_gen_{next_gen}.json"
            generate_batch_brief(
                generation=next_gen,
                state=state,
                snapshot=snapshot,
                brief_path=brief_path,
                result_path=result_path,
                smoke_only=args.smoke_only,
            )
            print(f"[dry-run] Pre-Batch Gate 통과 (branch={snapshot.branch}, head={snapshot.head[:10]})")
            print(f"[dry-run] 생성된 배치 브리프: {brief_path}")
            print(f"[dry-run] 예상 결과 JSON 경로: {result_path}")
            return EXIT_OK

        raw_max_batches = int(args.max_batches)
        max_batches: int | None = 1 if args.smoke_only else (None if raw_max_batches <= 0 else raw_max_batches)
        max_quota_retries = max(0, int(getattr(args, "max_retries", 6)))
        quota_wait_sec = max(1, int(getattr(args, "retry_quota_wait_sec", 300)))
        env_handle = os.environ.get("ORCA_TERMINAL_HANDLE", "").strip()
        created_handle = False
        terminal_handle = env_handle

        try:
            if not terminal_handle:
                terminal_handle = adapter.create_terminal()
                created_handle = True

            batches_done = 0
            transient_retries = 0
            while max_batches is None or batches_done < max_batches:
                snapshot = adapter.git_snapshot()
                gate_failures = check_pre_batch_gate(
                    state=state,
                    snapshot=snapshot,
                    allow_dirty=args.allow_dirty,
                    operations_path=operations_path,
                )
                if gate_failures:
                    raise RunnerError(
                        "Pre-Batch Gate 실패:\n- " + "\n- ".join(gate_failures),
                        EXIT_GATE_FAILED,
                    )

                generation = int(state.get("generation", 0)) + 1
                brief_path = runner_dir / f"batch_gen_{generation}.md"
                result_path = runner_dir / f"batch_result_gen_{generation}.json"
                generate_batch_brief(
                    generation=generation,
                    state=state,
                    snapshot=snapshot,
                    brief_path=brief_path,
                    result_path=result_path,
                    smoke_only=args.smoke_only,
                )

                task_title = (
                    f"smoke-batch-gen-{generation}"
                    if args.smoke_only
                    else f"parity-batch-gen-{generation}"
                )
                state["generation"] = generation
                state["active_dispatch"] = {
                    "generation": generation,
                    "task_title": task_title,
                    "spec_path": str(brief_path),
                    "result_path": str(result_path),
                    "head_before": snapshot.head,
                    "model": args.model,
                    "effort": args.effort,
                    "started_at": utc_now_iso(),
                }
                save_state_atomic(state_path, state)

                try:
                    dispatch = adapter.worker_start(
                        from_handle=terminal_handle,
                        model=args.model,
                        effort=args.effort,
                        task_title=task_title,
                        spec_file=brief_path,
                    )
                    run_id = dispatch["runId"]
                    state["active_dispatch"]["dispatchId"] = dispatch["dispatchId"]
                    state["active_dispatch"]["taskId"] = dispatch["taskId"]
                    state["active_dispatch"]["runId"] = run_id
                    save_state_atomic(state_path, state)

                    deadline = time.monotonic() + max(1, int(args.timeout_sec))
                    worker_done_msg: dict[str, Any] | None = None
                    while worker_done_msg is None:
                        remaining_sec = deadline - time.monotonic()
                        if remaining_sec <= 0:
                            raise RunnerError(
                                f"배치 gen={generation} 대기 시간 초과 ({args.timeout_sec}s, runId={run_id}).",
                                EXIT_ERROR,
                            )
                        timeout_ms = max(1000, int(remaining_sec * 1000))
                        messages, delivery_id = adapter.check_wait(
                            terminal_handle=terminal_handle,
                            run_id=run_id,
                            timeout_ms=timeout_ms,
                        )
                        if not messages:
                            raise RunnerError(
                                f"배치 gen={generation} 대기 중 메시지가 비어 있습니다 (runId={run_id}).",
                                EXIT_ERROR,
                            )
                        ask_question: str | None = None
                        unexpected_msg: dict[str, Any] | None = None
                        ignored_notices: list[str] = []
                        for msg in messages:
                            msg_type = msg.get("type")
                            if msg_type == "ask":
                                ask_question = (msg.get("body") or msg.get("subject") or "").strip()
                                break
                            if msg_type == "worker_done":
                                worker_done_msg = msg
                                break
                            if _is_nonfatal_progress_notice(msg):
                                ignored_notices.append(str(msg.get("subject") or msg.get("id") or "no-progress"))
                                continue
                            unexpected_msg = msg
                            break
                        if delivery_id:
                            adapter.ack_message(
                                terminal_handle=terminal_handle,
                                run_id=run_id,
                                msg_id=delivery_id,
                            )
                        for notice in ignored_notices:
                            print(f"[batch gen={generation}] [warn] Orca 무진전 알림 무시 후 계속 대기: {notice}")
                        if ask_question is not None:
                            raise RunnerError(
                                f"코디네이터가 질문을 보냈습니다 (안전 정지, 추측 응답 금지):\n{ask_question}",
                                EXIT_STOPPED,
                            )
                        if unexpected_msg is not None:
                            raise RunnerError(
                                f"예상하지 못한 메시지 유형 수신 ({unexpected_msg.get('type')}): {unexpected_msg.get('subject')}",
                                EXIT_ERROR,
                            )

                    payload = worker_done_msg.get("payload") or {}
                    outcome = payload.get("outcome")
                    if outcome != "succeeded":
                        raise RunnerError(
                            f"배치 gen={generation} worker_done outcome={outcome!r}: "
                            f"{worker_done_msg.get('body')}",
                            EXIT_ERROR,
                        )
                except RunnerError as exc:
                    if _is_transient_or_quota_error(exc) and transient_retries < max_quota_retries:
                        transient_retries += 1
                        state["active_dispatch"] = None
                        state["generation"] = generation - 1
                        save_state_atomic(state_path, state)
                        print(
                            f"[batch gen={generation}] [warn] 일시적/쿼터 오류 감지 — "
                            f"{quota_wait_sec}s 대기 후 재시도 ({transient_retries}/{max_quota_retries}): {exc}",
                            file=sys.stderr,
                        )
                        sleep_fn(quota_wait_sec)
                        continue
                    raise

                transient_retries = 0
                batches_done += 1

                batch_result = load_batch_result(result_path)
                post_snapshot = adapter.git_snapshot()
                should_continue, reason = evaluate_post_batch(
                    generation=generation,
                    state=state,
                    head_before=snapshot.head,
                    post_snapshot=post_snapshot,
                    batch_result=batch_result,
                    allow_dirty=args.allow_dirty,
                    smoke_only=args.smoke_only,
                )
                started_at = str((state.get("active_dispatch") or {}).get("started_at") or "")
                state["active_dispatch"] = None
                save_state_atomic(state_path, state)
                if effective_retro_path is not None and not args.smoke_only:
                    try:
                        audit_data = adapter.audit_since(started_at) if started_at else None
                        append_retro_entry(
                            retro_path=effective_retro_path,
                            generation=generation,
                            model=args.model,
                            effort=args.effort,
                            batch_result=batch_result,
                            committed=post_snapshot.head != snapshot.head,
                            audit_data=audit_data,
                        )
                    except Exception as retro_exc:
                        print(
                            f"[batch gen={generation}] [warn] worker_retro 기록 실패(무시): {retro_exc}",
                            file=sys.stderr,
                        )
                print(f"[batch gen={generation}] {reason}")

                if not should_continue:
                    if batch_result["status"] in STOP_STATUSES or int(
                        state.get("consecutive_no_progress", 0)
                    ) >= 2:
                        return EXIT_STOPPED
                    break

            return EXIT_OK
        finally:
            if created_handle and terminal_handle:
                with contextlib.suppress(Exception):
                    adapter.close_terminal(terminal_handle)


# ---------------------------------------------------------------------------
# Self-test suite (no external Orca/Git subprocesses)
# ---------------------------------------------------------------------------


class _FakeAdapter(SystemAdapter):
    """In-memory mock adapter for --selftest."""

    def __init__(
        self,
        *,
        head_seq: list[str] | None = None,
        dirty_lines: list[str] | None = None,
        unpushed_count: int = 0,
        worker_model: str | None = None,
        worker_effort: str | None = None,
        on_worker_start: Callable[[int, Path], None] | None = None,
    ) -> None:
        self.head_seq = list(head_seq or ["aaa1111", "bbb2222"])
        self.dirty_lines = list(dirty_lines or [])
        self.unpushed_count = unpushed_count
        self.worker_model = worker_model
        self.worker_effort = worker_effort
        self.on_worker_start = on_worker_start
        self.snapshot_calls = 0
        self.batch_calls = 0
        self.terminal_created = False
        self.terminal_closed = False

    def git_snapshot(self) -> GitSnapshot:
        idx = min(self.snapshot_calls, len(self.head_seq) - 1)
        self.snapshot_calls += 1
        return GitSnapshot(
            head=self.head_seq[idx],
            branch="desktop-rs",
            dirty_lines=list(self.dirty_lines),
            unpushed_count=self.unpushed_count,
        )

    def create_terminal(self) -> str:
        self.terminal_created = True
        return "term_fake_1"

    def close_terminal(self, handle: str) -> None:
        self.terminal_closed = True

    def audit_since(self, started_at: str) -> dict[str, Any] | None:
        return None

    def worker_start(
        self,
        *,
        from_handle: str,
        model: str,
        effort: str,
        task_title: str,
        spec_file: Path,
    ) -> dict[str, Any]:
        self.batch_calls += 1
        if self.on_worker_start is not None:
            self.on_worker_start(self.batch_calls, spec_file)
        return {
            "dispatchId": f"disp_{self.batch_calls}",
            "taskId": f"task_{self.batch_calls}",
            "runId": f"run_{self.batch_calls}",
            "state": "ready",
            "model": self.worker_model or model,
            "effort": self.worker_effort or effort,
            "mode": "structured",
            "terminal": from_handle,
        }

    def check_wait(
        self,
        *,
        terminal_handle: str,
        run_id: str,
        timeout_ms: int,
    ) -> tuple[list[dict[str, Any]], str | None]:
        return (
            [
                {
                    "id": f"msg_{self.batch_calls}",
                    "type": "worker_done",
                    "subject": "done",
                    "body": "ok",
                    "payload": {"outcome": "succeeded"},
                }
            ],
            f"deliv_{self.batch_calls}",
        )

    def ack_message(self, *, terminal_handle: str, run_id: str, msg_id: str) -> None:
        return None


def _make_args(tmp: Path, **overrides: Any) -> argparse.Namespace:
    base = {
        "max_batches": 3,
        "model": "gemini-4-neon",
        "effort": "medium",
        "timeout_sec": 60,
        "state_path": str(tmp / "runner_state.json"),
        "lock_path": str(tmp / "runner.lock"),
        "dry_run": False,
        "recover": False,
        "allow_dirty": False,
        "smoke_only": False,
        "selftest": False,
    }
    base.update(overrides)
    return argparse.Namespace(**base)


class CoordinatorRunnerSelfTests(unittest.TestCase):
    def test_01_exclusive_lock_conflict(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            lock_path = Path(td) / "runner.lock"
            with ExclusiveFileLock(lock_path):
                second = ExclusiveFileLock(lock_path)
                with self.assertRaises(RunnerError) as ctx:
                    second.acquire()
                self.assertEqual(ctx.exception.exit_code, EXIT_LOCK_CONFLICT)

    def test_02_atomic_state_save_and_history_cap(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            state_path = Path(td) / "nested" / "state.json"
            state = default_state()
            state["generation"] = 5
            state["history"] = [{"generation": i} for i in range(40)]
            save_state_atomic(state_path, state)
            loaded = load_state(state_path)
            self.assertEqual(loaded["generation"], 5)
            self.assertEqual(len(loaded["history"]), 30)
            self.assertEqual(loaded["history"][0]["generation"], 10)

    def test_03_disallowed_model_and_effort_rejected(self) -> None:
        with self.assertRaises(RunnerError):
            validate_model_and_effort("gemini-3.8-flash", "medium")
        with self.assertRaises(RunnerError):
            validate_model_and_effort("gemini-4-neon", "xhigh")

    def test_04_dirty_and_unpushed_blocked(self) -> None:
        state = default_state()
        with tempfile.TemporaryDirectory() as td:
            ops = Path(td) / "OPERATIONS.md"
            ops.write_text("ok", encoding="utf-8")

            dirty_snap = GitSnapshot("aaa", "desktop-rs", [" M foo.rs"], 0)
            failures = check_pre_batch_gate(
                state=state, snapshot=dirty_snap, allow_dirty=False, operations_path=ops
            )
            self.assertTrue(any("깨끗하지 않습니다" in f for f in failures))

            unpushed_snap = GitSnapshot("aaa", "desktop-rs", [], 2)
            failures2 = check_pre_batch_gate(
                state=state, snapshot=unpushed_snap, allow_dirty=False, operations_path=ops
            )
            self.assertTrue(any("미푸시 커밋" in f for f in failures2))

    def test_05_active_dispatch_blocks_relaunch(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            tmp = Path(td)
            state_path = tmp / "state.json"
            state = default_state()
            state["active_dispatch"] = {"generation": 1, "runId": "run_old"}
            save_state_atomic(state_path, state)

            args = _make_args(tmp, state_path=str(state_path), dry_run=True)
            with self.assertRaises(RunnerError) as ctx:
                run_batches(args, sys_adapter=_FakeAdapter())
            self.assertEqual(ctx.exception.exit_code, EXIT_ACTIVE_DISPATCH)

    def test_06_recover_clears_active_dispatch(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            tmp = Path(td)
            state_path = tmp / "state.json"
            state = default_state()
            state["active_dispatch"] = {"generation": 2, "runId": "run_stuck"}
            save_state_atomic(state_path, state)

            args = _make_args(tmp, state_path=str(state_path), recover=True)
            rc = run_batches(args, sys_adapter=_FakeAdapter())
            self.assertEqual(rc, EXIT_OK)
            reloaded = load_state(state_path)
            self.assertIsNone(reloaded["active_dispatch"])
            self.assertEqual(reloaded["history"][-1]["status"], "recovered")

    def test_07_consecutive_no_progress_circuit_breaker(self) -> None:
        state = default_state()
        snap = GitSnapshot("same_head", "desktop-rs", [], 0)
        res = {
            "status": "continue",
            "summary": "no commit",
            "next_task": "retry",
            "failed_gate": None,
            "failed_feature": None,
            "friction": None,
            "decisions": [],
        }
        cont1, _ = evaluate_post_batch(
            generation=1,
            state=state,
            head_before="same_head",
            post_snapshot=snap,
            batch_result=res,
            allow_dirty=False,
            smoke_only=False,
        )
        self.assertTrue(cont1)
        self.assertEqual(state["consecutive_no_progress"], 1)

        cont2, reason2 = evaluate_post_batch(
            generation=2,
            state=state,
            head_before="same_head",
            post_snapshot=snap,
            batch_result=res,
            allow_dirty=False,
            smoke_only=False,
        )
        self.assertFalse(cont2)
        self.assertEqual(state["consecutive_no_progress"], 2)
        self.assertIn("연속 2회 무진전", reason2)

    def test_08_same_gate_two_failures_blocks(self) -> None:
        state = default_state()
        snap = GitSnapshot("h2", "desktop-rs", [], 0)
        res = {
            "status": "continue",
            "summary": "gate failed",
            "next_task": "fix",
            "failed_gate": "spec_gate",
            "failed_feature": None,
            "friction": None,
            "decisions": [],
        }
        cont1, _ = evaluate_post_batch(
            generation=1,
            state=state,
            head_before="h1",
            post_snapshot=snap,
            batch_result=res,
            allow_dirty=False,
            smoke_only=False,
        )
        self.assertTrue(cont1)
        cont2, reason2 = evaluate_post_batch(
            generation=2,
            state=state,
            head_before="h2",
            post_snapshot=GitSnapshot("h3", "desktop-rs", [], 0),
            batch_result=res,
            allow_dirty=False,
            smoke_only=False,
        )
        self.assertFalse(cont2)
        self.assertIn("spec_gate=2", reason2)

    def test_09_stop_t3_and_stop_backlog_empty_immediate_stop(self) -> None:
        for stop_status in ("stop_t3", "stop_backlog_empty"):
            state = default_state()
            snap = GitSnapshot("h2", "desktop-rs", [], 0)
            res = {
                "status": stop_status,
                "summary": f"stopped on {stop_status}",
                "next_task": None,
                "failed_gate": None,
                "failed_feature": None,
                "friction": None,
                "decisions": [],
            }
            cont, reason = evaluate_post_batch(
                generation=1,
                state=state,
                head_before="h1",
                post_snapshot=snap,
                batch_result=res,
                allow_dirty=False,
                smoke_only=False,
            )
            self.assertFalse(cont)
            self.assertIn(stop_status, reason)

    def test_10_batch_result_schema_validation(self) -> None:
        valid = {
            "status": "done",
            "summary": "completed",
            "next_task": None,
            "failed_gate": None,
            "failed_feature": None,
            "friction": "없음",
            "decisions": ["kept minimal diff"],
        }
        self.assertEqual(validate_batch_result_schema(valid)["status"], "done")

        invalid_missing = {"status": "done", "summary": "missing keys"}
        with self.assertRaises(RunnerError):
            validate_batch_result_schema(invalid_missing)

        invalid_status = dict(valid, status="unknown_status")
        with self.assertRaises(RunnerError):
            validate_batch_result_schema(invalid_status)

    def test_11_ignores_orca_nonfatal_no_progress_escalation(self) -> None:
        class _NoticeThenDoneAdapter(_FakeAdapter):
            def __init__(self, first_msg: dict[str, Any], result_path: Path) -> None:
                super().__init__(
                    head_seq=["h1", "h2"],
                    on_worker_start=lambda _g, _p: result_path.write_text(
                        json.dumps(
                            {
                                "status": "done",
                                "summary": "ok after notice",
                                "next_task": None,
                                "failed_gate": None,
                                "failed_feature": None,
                                "friction": None,
                                "decisions": [],
                            }
                        ),
                        encoding="utf-8",
                    ),
                )
                self._first_msg = first_msg
                self._checks = 0

            def check_wait(
                self,
                *,
                terminal_handle: str,
                run_id: str,
                timeout_ms: int,
            ) -> tuple[list[dict[str, Any]], str | None]:
                self._checks += 1
                if self._checks == 1:
                    return [self._first_msg], "deliv_notice"
                return super().check_wait(
                    terminal_handle=terminal_handle,
                    run_id=run_id,
                    timeout_ms=timeout_ms,
                )

        with tempfile.TemporaryDirectory() as td:
            tmp = Path(td)
            res_path = DEFAULT_RUNNER_DIR / "batch_result_gen_1.json"
            backup = res_path.read_text(encoding="utf-8") if res_path.exists() else None
            try:
                notice = {
                    "id": "msg_stop_123",
                    "type": "escalation",
                    "subject": "Worker has made no progress",
                    "body": "Worker has made no progress for 10 min. It is still running; nothing was stopped.",
                }
                adapter = _NoticeThenDoneAdapter(notice, res_path)
                args = _make_args(tmp, max_batches=1, timeout_sec=30)
                rc = run_batches(args, sys_adapter=adapter)
                self.assertEqual(rc, EXIT_OK)
                self.assertEqual(adapter._checks, 2)

                real_esc = {
                    "id": "msg_real_123",
                    "type": "escalation",
                    "subject": "Blocked: external approval required",
                    "body": "Need user approval.",
                }
                adapter2 = _NoticeThenDoneAdapter(real_esc, res_path)
                args2 = _make_args(tmp, state_path=str(tmp / "state2.json"), max_batches=1, timeout_sec=30)
                with self.assertRaises(RunnerError) as ctx:
                    run_batches(args2, sys_adapter=adapter2)
                self.assertIn("escalation", str(ctx.exception))
            finally:
                if backup is not None:
                    res_path.write_text(backup, encoding="utf-8")
                elif res_path.exists():
                    res_path.unlink()

    def test_12_friction_propagates_to_next_brief_and_retro_log(self) -> None:
        class _AuditAdapter(_FakeAdapter):
            def audit_since(self, started_at: str) -> dict[str, Any] | None:
                return {
                    "sessions": [
                        {
                            "id": "abcd1234-5678-90ef",
                            "duration_seconds": 125,
                            "tools": {"read_file": 4, "run_command": 3},
                            "models": {"gemini-4-neon": [7, 10000, 2000]},
                            "repeated_reads": [{"path": "foo.rs", "count": 3}],
                            "top_results": [{"bytes": 12000}, {"bytes": 500}],
                            "errors": 1,
                            "flags": {},
                        }
                    ]
                }

        with tempfile.TemporaryDirectory() as td:
            tmp = Path(td)
            runner_dir = tmp / "runner"
            retro_path = tmp / "worker_retro.md"
            briefs: list[str] = []

            def _on_start(gen: int, spec_file: Path) -> None:
                briefs.append(spec_file.read_text(encoding="utf-8"))
                res_file = runner_dir / f"batch_result_gen_{gen}.json"
                res_file.parent.mkdir(parents=True, exist_ok=True)
                res_file.write_text(
                    json.dumps(
                        {
                            "status": "continue" if gen == 1 else "done",
                            "summary": f"gen {gen} summary",
                            "next_task": f"next after {gen}" if gen == 1 else None,
                            "failed_gate": None,
                            "failed_feature": None,
                            "friction": f"friction from gen {gen}",
                            "decisions": [f"dec {gen}"],
                        },
                        ensure_ascii=False,
                    ),
                    encoding="utf-8",
                )

            adapter = _AuditAdapter(
                head_seq=["h0", "h1", "h1", "h2"],
                on_worker_start=_on_start,
            )
            args = _make_args(tmp, max_batches=2)
            rc = run_batches(
                args,
                sys_adapter=adapter,
                runner_dir=runner_dir,
                retro_path=retro_path,
            )
            self.assertEqual(rc, EXIT_OK)
            self.assertEqual(len(briefs), 2)
            self.assertIn("friction from gen 1", briefs[1])
            self.assertIn("next after 1", briefs[1])

            retro_text = retro_path.read_text(encoding="utf-8")
            self.assertIn("`Gen 1`", retro_text)
            self.assertIn("`Gen 2`", retro_text)
            self.assertIn("`abcd1234`", retro_text)
            self.assertIn("1 / 1 / 1 / 0", retro_text)
            self.assertIn("friction from gen 1", retro_text)

    def test_13_unlimited_max_batches_zero_runs_until_stop_backlog_empty(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            tmp = Path(td)
            runner_dir = tmp / "runner"
            briefs: list[str] = []

            def _on_start(gen: int, spec_file: Path) -> None:
                briefs.append(spec_file.read_text(encoding="utf-8"))
                res_file = runner_dir / f"batch_result_gen_{gen}.json"
                res_file.parent.mkdir(parents=True, exist_ok=True)
                res_file.write_text(
                    json.dumps(
                        {
                            "status": "stop_backlog_empty" if gen == 3 else "continue",
                            "summary": f"gen {gen} done",
                            "next_task": None,
                            "failed_gate": None,
                            "failed_feature": None,
                            "friction": None,
                            "decisions": [],
                        },
                        ensure_ascii=False,
                    ),
                    encoding="utf-8",
                )

            adapter = _FakeAdapter(
                head_seq=["h0", "h1", "h1", "h2", "h2", "h3"],
                on_worker_start=_on_start,
            )
            args = _make_args(tmp, max_batches=0)
            rc = run_batches(args, sys_adapter=adapter, runner_dir=runner_dir)
            self.assertEqual(rc, EXIT_STOPPED)
            self.assertEqual(len(briefs), 3)
            self.assertIn("gemini-4-neon-high", briefs[0])
            self.assertIn("codex exec -m gpt-6.2-sol -s read-only", briefs[0])

    def test_14_quota_error_retries_with_backoff_then_succeeds(self) -> None:
        class _QuotaOnceAdapter(_FakeAdapter):
            def __init__(self, runner_dir: Path) -> None:
                super().__init__(head_seq=["h0", "h1"])
                self._runner_dir = runner_dir
                self._attempts = 0

            def worker_start(self, **kwargs: Any) -> dict[str, Any]:
                self._attempts += 1
                if self._attempts == 1:
                    raise RunnerError("429 RESOURCE_EXHAUSTED: quota exceeded", EXIT_ERROR)
                res_file = self._runner_dir / "batch_result_gen_1.json"
                res_file.parent.mkdir(parents=True, exist_ok=True)
                res_file.write_text(
                    json.dumps(
                        {
                            "status": "done",
                            "summary": "succeeded after quota retry",
                            "next_task": None,
                            "failed_gate": None,
                            "failed_feature": None,
                            "friction": None,
                            "decisions": [],
                        },
                        ensure_ascii=False,
                    ),
                    encoding="utf-8",
                )
                return super().worker_start(**kwargs)

        with tempfile.TemporaryDirectory() as td:
            tmp = Path(td)
            runner_dir = tmp / "runner"
            sleeps: list[float] = []
            adapter = _QuotaOnceAdapter(runner_dir)
            args = _make_args(tmp, max_batches=1)
            args.max_retries = 3
            args.retry_quota_wait_sec = 42
            rc = run_batches(
                args,
                sys_adapter=adapter,
                runner_dir=runner_dir,
                sleep_fn=sleeps.append,
            )
            self.assertEqual(rc, EXIT_OK)
            self.assertEqual(adapter._attempts, 2)
            self.assertEqual(sleeps, [42])


def run_selftest() -> int:
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(CoordinatorRunnerSelfTests)
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    return EXIT_OK if result.wasSuccessful() else EXIT_ERROR


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="External stateless batch runner for Fluxer parity coordinator."
    )
    parser.add_argument("--max-batches", type=int, default=3, help="최대 연속 배치 수 (0=무제한, 기본: 3)")
    parser.add_argument("--model", default="gemini-4-neon", help="코디네이터 모델 (기본: gemini-4-neon)")
    parser.add_argument("--effort", default="medium", help="추론 강도 (medium|high, 기본: medium)")
    parser.add_argument("--timeout-sec", type=int, default=3600, help="배치 1회 타임아웃(초) (기본: 3600)")
    parser.add_argument("--max-retries", type=int, default=6, help="쿼터/일시적 오류 최대 재시도 횟수 (기본: 6)")
    parser.add_argument("--retry-quota-wait-sec", type=int, default=300, help="쿼터/일시적 오류 재시도 대기 시간(초, 기본: 300)")
    parser.add_argument("--state-path", default=str(DEFAULT_STATE_PATH), help="원장 JSON 경로")
    parser.add_argument("--lock-path", default=str(DEFAULT_LOCK_PATH), help="배타적 락 파일 경로")
    parser.add_argument("--dry-run", action="store_true", help="Pre-Batch Gate 검사 및 브리프 생성만 수행")
    parser.add_argument("--recover", action="store_true", help="잔류 active_dispatch 상태 확인 및 초기화")
    parser.add_argument("--allow-dirty", action="store_true", help="dirty 작업 트리 허용 (개발 테스트용)")
    parser.add_argument("--smoke-only", action="store_true", help="1배치 E2E 스모크 브리프 실행")
    parser.add_argument("--selftest", action="store_true", help="외부 호출 없는 자체 단위 테스트 실행")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    if args.selftest:
        return run_selftest()
    try:
        return run_batches(args)
    except RunnerError as exc:
        print(f"[error] {exc}", file=sys.stderr)
        return exc.exit_code


if __name__ == "__main__":
    raise SystemExit(main())
