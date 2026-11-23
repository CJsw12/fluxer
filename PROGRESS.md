# PROGRESS

현재 상태, 인계, 다음 작업 순서를 관리한다. 마일스톤을 마칠 때 이 문서를 갱신하며, 로드맵은 목표나 수락 기준이 바뀔 때 갱신한다. 이전 기록은 `docs/archive/*.local.md`(Git 제외)에 보관한다.

## 새 세션 시작 순서

1. `CLAUDE.md`(Claude 전용) → `AGENTS.md`(공통 규칙) → 이 문서 → 로컬 운영 메모 `Logs/handoff/OPERATIONS.md`(Git 제외, 시험 환경·상시 실행·진행 중 작업)를 읽는다. 명령어 모음은 `README.md`.
2. UI 작업이면 `docs/UI_PARITY_ROADMAP.md`의 미완료(`[ ]`) 첫 항목과 `docs/PARITY_LAB.md` 10장(운영 가이드)을 읽는다.
3. 상시 실행 구성(시험 서버 터널, 측정 기능을 켠 Rust 클라이언트, 비교용 공식 앱)을 `docs/PARITY_LAB.md` 10.1대로 띄우고, Rust 클라이언트가 `Status: Online`인지 확인한다. 터널은 에이전트 도구의 백그라운드 시간 제한(2시간)에 걸리므로, 오래 유지하려면 별도 터미널에서 실행한다.
4. 아래 Next Steps 1번부터 진행한다.

## 현재 상태 (2026-11-24)

- **Baseline:** 브랜치 `desktop-rs`(작업), `main` = `upstream/main`. 주력은 `fluxer_client/`(Rust GPUI + gpui-kit). 음성·화면 공유는 별도 헬퍼 `fluxer_client/voice_worker`(LiveKit, 정적 CRT) 프로세스. `fluxer_desktop_rs/`는 WebView2 전환기 셸.
- **Done(최근):** 화면 공유 양방향 자동 시험과 수신 색 결함 수정(`ac638a6a3`, M9.5 1차), 업스트림 2026-11-23 병합(`3cc9d8ffb`). Parity Lab 구축(공식 앱 CDP 실측 ↔ Rust 측정 기능 조인 비교, 클릭 크롤러, 재생 러너, 쓰기 차단 가드)과 #markdown 메인 화면 1~5차 수렴. 셸 대부분 0~1px 일치. 상세는 `docs/PARITY_LAB.md` 7.1, 커밋 `82bed49a3`..`7ddeff5ed`.
- **Decisions:**
  - 1:1 판정은 공식 데스크톱 앱 실측이 기준이다. 정규식 감사(`scripts/audit_ui_parity.py`)와 합성 갤러리는 보조다.
  - 비교 기준 앱은 포터블 공식 데스크톱 사본(Web 2026.1120, 다크, tester2)이다. 사용자 개인 공식 앱은 조작하지 않는다.
  - Windows MSVC에서 `libwebrtc`가 정적 CRT를 요구해 LiveKit·오디오는 헬퍼 프로세스로 분리한다(비공개 JSON-lines IPC, 토큰 argv·로그 금지).
  - 모델은 Claude Opus·Sonnet·Haiku 5.5와 DeepSeek flash만 쓴다. 영역별 수정은 Sonnet, 측정·통합·검토는 Opus.
- **Blockers:**
  - 진행도의 "93/93 구현"은 동작 확인 없이 센 값이라 신뢰하지 않는다(M9.6에서 집계 기준 변경).
  - 비교 앱(tester2)과 Rust(tester1)의 계정이 달라 본인 시점 표시는 비교에서 뺀다.
  - 기준 버전: 저장소는 업스트림 2026-11-23(`3608a387c`)으로 갱신했지만 비교 앱은 Web 2026.1120이다(M9.10 남은 일).
  - 사람·하드웨어가 필요한 항목: 한글 IME 조합, OS 알림·소리, Windows Hello·FIDO2 실기, 장시간 부하.

## Next Steps

> 2026-11-24 완료: M9.5 화면 공유 1차(`ac638a6a3`), 업스트림 병합(`3cc9d8ffb`), M9.4 프로필 카드·M9.2 레일 호버(`8a44b5429`). 남은 작은 항목(상태 아이콘 모양, 길드 툴팁 인원 수, 다른 사람 음성 배지 등)은 `Logs/handoff/OPERATIONS.md` 참고.

1. **M9 검증 사각지대 보완(`docs/UI_PARITY_ROADMAP.md` M9).** 사용자가 지적한 7건은 지금까지의 방법으로 잡지 못했다.
   - 대상: 앱 내부 크기 조절, 화면 공유 송수신, 레일 호버, 프로필 카드, 서버 이니셜 글자, 테두리·모서리·사용자 영역 카드, 아이콘 원본.
   - 권장 순서: M9.5 화면 공유 자동 시험 → M9.4 팝오버 내용(첫 대상 프로필 카드) → M9.2 호버·상태 행렬 → M9.9 아이콘 원본 수확 → M9.8 테두리·모서리 → M9.7 요소 픽셀 비교 → M9.3 크기 조절 → M9.1·M9.11·M9.12 → M9.6·M9.10·M9.13.
2. **메시지 영역 마감.**
   - 바닥 여백 → 메시지 높이 순서로 맞춘다. ①·③ 메시지 본문 아래 여분과 코드 블록 9px의 원인이 미확정이다.
   - 스포일러 상자, 컬러 이모지, 코드 블록 테두리, 목록 글머리를 맞춘다.
3. **화면 확장.** DM·친구·설정 탭·음성 화면에서 같은 루프를 반복한다. 크롤은 쓰기 가드를 켠 상태로만 실행하고, 끝나면 `docs/PARITY_LAB.md` 10.5 복구 점검을 한다.
4. **사람 실기 검증.** 위 Blockers의 하드웨어 항목을 확인한 뒤에만 `docs/progress/progress.json`을 `done`으로 올리고 `python -I scripts/progress_svg.py`로 배지를 갱신한다.

## 미디어 검증 경계

- H.265는 로컬 LiveKit에서 Chrome·Rust 수신, 재구독 복구를 확인했다. 실제 Fluxer 채널·별도 기기·Rust UI 영상 표시·화면 오디오·E2EE·장시간 성능은 미검증이다.
- 화면 공유는 송신 코드(`e94acf19e`)만 있고 실제 송수신은 동작하지 않는다(사용자 확인, M9.5).

## 문서 지도

- 활성: `README.md`(개요·명령), `PROGRESS.md`(이 문서), `docs/UI_PARITY_ROADMAP.md`(실행 계획), `docs/PARITY_LAB.md`(검증 방법·운영 가이드·외부 사례), `docs/rust_client_core/{GATEWAY_PROTOCOL,REST_AND_AUTH,STATE_MODEL}.md`(코어 레퍼런스), `docs/progress/progress.json`(기능 추적).
- 로컬 보관(Git 제외, `docs/archive/*.local.md`): 이전 진행 기록, 끝난 M1~M8 상세, 이전 단계 계획서(`RUST_*_PLAN`, `NATIVE_MIGRATION_PLAN`, `IPC_CONTRACT_BASELINE`, `FEATURE_GAPS`, `NEXT_BRIEFS`), 비공개 조사(`IPC_M0_FINDINGS`, `RUST_PORT_RESEARCH_*`, `RUST_MIGRATION_REFERENCE_REVIEW`, `model-selection-evidence`). 작업 기준으로 삼지 않는다.
