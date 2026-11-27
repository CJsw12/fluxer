# Director Playbook (상주 지시 세션 운영 매뉴얼)

새 지시 세션(Claude 또는 Gemini 4 Neon)을 시작할 때 이 파일을 읽고 아래 2+1단 구조로만 작업한다.

## 1. 역할 분리 (편향 차단 + 탭 최소화)
- **지시 세션 (상주 1개, 이 세션):** ① 다음 배치 선정·브리프 작성 → ② `orca orchestration worker-start`로 **새 탭 1개** 생성 → ③ 완료 후 결과·감사 검토 및 **재귀 개선** → ④ 다음 배치 지시만 반복한다.
- **배치 워커 (배치당 새 탭 1개, `Gemini 4 Neon Medium`):** 맡은 1개 배치의 전처리·구현 위임·`verify_all.py --build`·육안 확인·커밋·push·결과 JSON 보고까지 완결한다. **중첩 `worker-start` 금지.**
- **워커 내부 실행기 (새 탭 없음, 서브/백그라운드):** T0 탐색·분류는 DeepSeek(`opencode`, 백그라운드), T1 구현은 `swarm`(`gemini-4-hydrogen-medium`, 비중첩 파일 2~3개 병렬).

## 2. 지시 세션 읽기 화이트리스트 (컨텍스트·편향 차단)
- **읽어도 되는 것 (4종만):** `Logs/handoff/OPERATIONS.md`(스냅샷), 워커의 `worker_done` + `target/parity-tmp/runner/batch_result_gen_N.json`, `python -I tools/agents/agy_audit.py --since <UTC시작시각> --json`, `Logs/handoff/worker_retro.md` 최근 5행.
- **절대 금지:** `fluxer_client/`·`fluxer_app/` 소스 읽기/수정, `git diff` 본문 열람, 빌드·테스트·실기 직접 실행, `coordinator_runner.py`와 동시 실행(러너는 밤샘 무인 전용).
- 5~8배치마다 `/compact`하거나 새 지시 세션으로 교체한다.

## 3. 속도 최적화 배치 규칙 (워커 브리프에 반드시 포함)
1. **배치 묶기:** 고정 7~8분짜리 `verify_all.py --build`를 아끼기 위해 1개 배치에 **비중첩 파일 클러스터 2~3개**(예: 상호작용 30~50건 또는 설정/화면 탭 2~3개)를 묶어 `swarm(gemini-4-hydrogen-medium)`으로 병렬 구현한다.
2. **전처리 1회로 압축:** 워커 시작 직후 DeepSeek으로 대상 파일·줄 번호·정확한 항목 목록을 30줄 이내로 압축해 `swarm` 브리프에 주입한다(워커 반복 탐색 제거).
3. **병렬 `swarm` 중 `cargo` 금지:** `workspace: "inherit"` 병렬 `swarm` 워커들은 편집 + `check_colors.py`/`check_icons.py`만 실행하고 종료 시 `@coordinator`로 보고한다. 배치 워커가 모든 `swarm` 종료 후 **단일 통합 `cargo check`/테스트**를 실행한다(`target/fc-root` 락 경합·형제 미완성 컴파일 오류 제거).
4. **중복 테스트 제거:** `fluxer_client/app`만 바뀐 배치는 `cargo test --workspace`를 따로 돌리지 않고 `verify_all.py --build` 안의 `cargo test -p fluxer_client_app` 1회로 통합한다(`core`/`markdown`/`i18n` 변경 시에만 해당 크레이트 추가 실행).

## 4. 워커 기동 및 감사 명령
- **기동(새 탭 1개):**
  `orca orchestration worker-start --environment local --from <내_세션_핸들> --worktree current --agent antigravity --model gemini-4-neon --effort medium --task-title "parity-batch-<N>" --spec-file target/parity-tmp/runner/batch_spec_<N>.md --json`
  - 반환 JSON의 `model == "gemini-4-neon"`, `effort == "medium"` 일치를 확인한다(T3·2회 실패 복합 버그만 `high` 또는 Claude 예외 승격).
- **완료 후 감사:** `python -I tools/agents/agy_audit.py --since <기동UTC시각> --json` 결과를 `Logs/handoff/worker_retro.md`에 1행 기록한다.

## 5. 재귀 개선 기준 (과잉 규칙화 방지)
- 1회성 마찰은 `worker_retro.md`에 기록만 한다.
- **2회 반복 마찰** 또는 **게이트 실패**만 승격한다: **도구 자동화 우선(별도 소형 워커에 위임)** → 불가 시 `tools/agents/WORKER_RULES.md`에 1~2줄 반영(**30줄 하드캡** 유지).
- 승격 후 다음 1배치의 `agy_audit.py` KPI(`repeated_reads`, `>8KB`, `errors`, `rework`)가 개선되지 않으면 즉시 회수한다.

## 6. T3 및 멈춤 조건
- **T3(삭제·인증·권한·저장 형식):** 배치 워커의 커밋을 금지하고, 구현 완료 후 **다른 계열 교차 검토 워커**를 별도 탭으로 실행해 통과한 뒤에만 커밋한다.
- **즉시 중단·보고:** 동일 게이트 2회 연속 실패, 백로그 소진, 외부 승인·차단 발생.
