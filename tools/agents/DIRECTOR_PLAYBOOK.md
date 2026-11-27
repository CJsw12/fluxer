# Director Playbook (상주 지시 세션 운영 매뉴얼)

새 지시 세션을 시작할 때 이 파일을 읽고 아래 2+1단 구조로 작업한다.

## 1. 역할 분리 (편향 차단 + 탭 최소화)
- **지시 세션 (상주 1개, 이 세션):** ① 다음 배치 선정·브리프 작성 → ② `orca orchestration worker-start`로 **새 탭 1개** 생성 → ③ 완료 후 결과·감사 검토 및 **재귀 개선** → ④ 다음 배치 지시만 반복한다.
- **배치 워커 (배치당 새 탭 1개):** 배치 시작 전 `python -I ~/.agent-rules/usage.py`로 주간·단기 잔여 한도를 확인하고, 배치 전체를 마칠 여유가 큰 도구를 고른다. Antigravity는 `gemini-4-neon` medium, Codex는 단순 배치에 `gpt-6.1-terra` medium, 통합 배치에 `gpt-6.2-sol` medium을 쓴다. 맡은 배치의 구현·`verify_all.py --build`·육안 확인·커밋·push·결과 JSON 보고까지 마친다. **중첩 `worker-start` 금지.**
- **워커 내부 실행기 (새 탭 없음):** 독립 파일 묶음은 Antigravity `swarm`(`gemini-4-hydrogen-medium`) 또는 Codex 병렬 서브에이전트(`gpt-6.1-terra` 또는 `gpt-6.2-sol`, medium 명시)에 맡긴다. DeepSeek과 Luna는 T0 탐색·분류에만 쓴다.

## 2. 지시 세션 읽기 화이트리스트 (컨텍스트·편향 차단)
- **읽어도 되는 것 (4종만):** `Logs/handoff/OPERATIONS.md`(스냅샷), 워커의 `worker_done` + `target/parity-tmp/runner/batch_result_gen_N.json`, `python -I tools/agents/agy_audit.py --since <UTC시작시각> --json`, `Logs/handoff/worker_retro.md` 최근 5행.
- **절대 금지:** `fluxer_client/`·`fluxer_app/` 소스 읽기/수정, `git diff` 본문 열람, 빌드·테스트·실기 직접 실행, `coordinator_runner.py`와 동시 실행(러너는 밤샘 무인 전용).
- 5~8배치마다 `/compact`하거나 새 지시 세션으로 교체한다.

## 3. 속도 최적화 배치 규칙 (워커 브리프에 반드시 포함)
1. **배치 묶기:** 독립적으로 수정할 수 있는 파일 묶음 2~3개를 한 배치로 묶어 병렬 구현하고 `verify_all.py --build`를 한 번 실행한다.
2. **전처리 1회로 압축:** T0 탐색으로 대상 파일·줄 번호·항목 목록을 30줄 이내로 정리해 각 브리프에 넣는다.
3. **병렬 워커는 `cargo` 생략:** 공유 체크아웃의 `swarm`과 Codex 서브에이전트는 소유 파일을 수정하고 `check_colors.py`와 `check_icons.py` 결과를 배치 워커에게 보고한다. 모든 편집이 끝나면 배치 워커가 단일 통합 빌드·테스트를 실행한다.
4. **중복 테스트 제거:** `fluxer_client/app`만 바뀐 배치는 `cargo test --workspace`를 따로 돌리지 않고 `verify_all.py --build` 안의 `cargo test -p fluxer_client_app` 1회로 통합한다(`core`/`markdown`/`i18n` 변경 시에만 해당 크레이트 추가 실행).

## 4. 워커 기동 및 감사 명령
- **기동(새 탭 1개):**
  `orca orchestration worker-start --environment local --from <내_세션_핸들> --worktree current --agent antigravity --model gemini-4-neon --effort medium --task-title "parity-batch-<N>" --spec-file target/parity-tmp/runner/batch_spec_<N>.md --json`
  - Codex에서는 `--agent codex --model gpt-6.2-sol --effort medium` 또는 `--agent codex --model gpt-6.1-terra --effort medium`으로 바꾼다. 반환 JSON에서 선택한 모델과 추론 수준을 확인한다.
- **완료 후 감사:** `python -I tools/agents/agy_audit.py --since <기동UTC시각> --json`을 실행한다. Codex 배치에는 `--codex`를 붙인다. 감사 결과와 배치 총 소요 시간·최장 구간을 `Logs/handoff/worker_retro.md`에 1행 기록한다.

## 5. 재귀 개선 기준 (품질 + 속도, 과잉 규칙화 방지)
- 1회성 마찰·지연은 `worker_retro.md`에 기록만 한다.
- **승격 조건 (아래 중 하나 해당 시):**
  1. **품질/효율:** 2회 반복 마찰 또는 게이트 실패.
  2. **시간 단축:** 동일 병목 구간(빌드·검증·워커 대기·전처리)이 **2회 연속 배치 시간의 30% 초과**하거나 **배치 총 시간이 25분 초과**.
- **개선 원칙:** **도구·빌드·캐시·병렬화 자동화 우선(별도 소형 워커 위임)** → 불가 시 `tools/agents/WORKER_RULES.md`에 1~2줄 반영(**30줄 하드캡** 유지). **테스트·래칫·육안 확인 등 품질 게이트 생략으로 시간을 줄이는 것은 금지.**
- 승격 후 다음 1배치의 `agy_audit.py` KPI(`repeated_reads`, `>8KB`, `errors`, `rework`) 및 **배치 소요 시간**이 개선되지 않거나 게이트 신뢰도가 떨어지면 즉시 회수한다.

## 6. T3 및 멈춤 조건
- **T3(삭제·인증·권한·저장 형식):** 구현 전 지시 세션에서 다른 모델 계열의 독립 검토를 받는다. 구현은 GPT-6.2 Sol high 또는 Claude Opus high에 맡기고, 구현 뒤에는 지시 세션이 구현에 참여하지 않은 다른 계열(GPT-6.2 Sol high, Claude Opus high 또는 Gemini 4 Neon high)에 검토를 배정한다. 교차 검토와 통합 검증이 통과한 뒤 지시 세션에서 커밋을 진행한다.
- **즉시 중단·보고:** 동일 게이트 2회 연속 실패, 백로그 소진, 외부 승인·차단 발생.
