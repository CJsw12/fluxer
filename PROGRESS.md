# PROGRESS

현재 상태, 인계, 다음 작업 순서를 관리한다. 마일스톤을 마칠 때 이 문서를 갱신하며, 로드맵은 목표나 수락 기준이 바뀔 때 갱신한다. 이전 기록은 `docs/archive/*.local.md`(Git 제외)에 보관한다.

## 새 세션 시작 순서

1. `CLAUDE.md`(Claude 전용) → `AGENTS.md`(공통 규칙) → 이 문서 → 로컬 운영 메모 `Logs/handoff/OPERATIONS.md`(Git 제외, 시험 환경·상시 실행·진행 중 작업)를 읽는다. 명령어 모음은 `README.md`.
2. UI 작업이면 `docs/UI_PARITY_ROADMAP.md`의 미완료(`[ ]`) 첫 항목과 `docs/PARITY_LAB.md` 10장(운영 가이드)을 읽는다.
3. 상시 실행 구성(시험 서버 터널, 측정 기능을 켠 Rust 클라이언트, 비교용 공식 앱)을 `docs/PARITY_LAB.md` 10.1대로 띄우고, Rust 클라이언트가 `Status: Online`인지 확인한다. 터널은 에이전트 도구의 백그라운드 시간 제한(2시간)에 걸리므로, 오래 유지하려면 별도 터미널에서 실행한다.
4. 아래 Next Steps 1번부터 진행한다.

## 현재 상태 (2026-11-24)

- **Baseline:** 브랜치 `desktop-rs`(작업), `main` = `upstream/main`. 주력은 `fluxer_client/`(Rust GPUI + gpui-kit). 음성·화면 공유는 별도 헬퍼 `fluxer_client/voice_worker`(LiveKit, 정적 CRT) 프로세스. `fluxer_desktop_rs/`는 WebView2 전환기 셸.
- **Done(최근):** M9.2 상태 행렬·호버·툴팁·음성 영역 1:1 수렴(`8f49ada84`), M9.3 앱 내부 크기 조절·프로브 `probe_drag`/`probe_dblclick` 구현 및 실기 25/25 검증(`fc6c1d245`), 1차 UI 결함 3건 수정(`bdf396dba`, 프로필 선 겹침·멤버 목록 모서리·설정 모달 스타일), Parity Lab 게이트 보강(`compare.py` `diff_heatmap.png`·모서리/미태그 모달 리포트, `state_matrix.py --strict`, `check_surface_parity.py`) 및 2차 UI 결함 3건 수정(채널/길드/계정 전환 숨은 호버 버튼 제거, 작성창 `Media | Stickers | Emojis` 탭 분리), M9.4 팝오버·모달 작업 목록 자동 생성(`replay.py --strict-no-duplicates`, 중복 화면 0건) 및 채널 알림·스레드·고정 메시지·받은 편지함·작성창 `+` 팝오버 구현(`cargo test --workspace` 709개·실기 `replay.py --strict-no-duplicates`·`check_surface_parity` 10/10·`state_matrix --strict`·`check_resize` 25/25 통과), 화면 공유 양방향 자동 시험과 수신 색 결함 수정(`ac638a6a3`, M9.5 1차), 업스트림 2026-11-23 병합(`3cc9d8ffb`). 상세는 `docs/PARITY_LAB.md` 7.1, 커밋 `82bed49a3`..`7ddeff5ed`.
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

> 2026-11-24 완료: 화면 공유 1차(`ac638a6a3`), 업스트림 병합(`3cc9d8ffb`), 프로필 카드·레일 호버(`8a44b5429`), 아이콘 원본 1~3차(`eb81a2d8c`·`e69b04c1c`·`cfc69a1e4`, 125곳 중 68곳 검증), 상태 아이콘 모양·외곽선·이니셜(`8bdf0c860`), 길드 툴팁 인원·음성 배지(`f4a58f66f`), M9.2 상태 행렬(`8f49ada84`), M9.3 크기 조절·`probe_drag`, 개발 루프 `tools/parity_lab/cycle.py`. 다음 순서와 운영 메모는 `Logs/handoff/OPERATIONS.md`.

1. **M9 검증 사각지대 보완(`docs/UI_PARITY_ROADMAP.md` M9).** 사용자가 지적한 7건은 지금까지의 방법으로 잡지 못했다.
   - 남은 핵심 순서: M9.9 아이콘 원본 수확 마무리 → M9.8 테두리·모서리 속성 비교 → M9.7 요소 픽셀 비교 → M9.1 조작 요소 목록 → M9.11·M9.12 → M9.6·M9.10·M9.13.
   - 2026-11-24 M9.9 진행: 길드 헤더 팝오버를 공식 소유자·일반 멤버 실측(떠 있는 288px, 20px fill 아이콘)으로 교체하고 아이콘 굵기 3곳을 고쳤다. `check_icons.py --check-sync`가 소스 호출 위치 누락을 막는다(149곳, 미검증 69곳). 남은 일: 미검증 69곳 수확, 길드 팝오버의 기능 없는 7개 항목(초대·카테고리 생성·알림·개인정보·커뮤니티 프로필·내 메시지 삭제·음소거 채널 숨김), 메시지 마우스 올림 바 구성(공식은 반응·편집·전달·스레드·더보기만).
   - 2026-11-25 팝오버·모달 8개 장면 명세 루프(`0570967ec` 장면 모드·래칫 게이트, `3a5afa837` 프로브 테두리 상자 측정, `b81ad66d4` 정렬·메뉴·피커·설정 모달, `18f03c688` 포커스 무관 Escape·버튼 기준 정렬). 장면 차이 합계 163→11(남은 것은 글꼴 메트릭·계정 차이). 남은 큰 일: 장면별 Rust 누락 요소(합계 104개)와 #markdown 누락 197개.
   - 2026-11-25 화면 확장: 태그 커버리지(`1ca8311d7`), 4차(`30b60574d`), DM·탐색·설정 탭 장면(`f0d040d2d`), 5차 DM 첨부 모자이크·DM 헤더 통화 버튼·설정 레이아웃(`d6f0a36d7`). 남은 DM 항목: 스포일러 상자 미표시, 반응 알약·반응 추가 버튼, 첨부 만료 문구(`Attachment`에 `expires_at` 없음), DM 사이드바 Personal notes·행 크기, DM 헤더 아바타. 기능 백로그: 탐색 페이지, 설정 탭 목록(Connections·Windows app·Applications·Privacy), 키 설정 편집, 친구 검색, DM 영상 통화 카메라·호출.
   - 비용·완성도 개선(Opus 조사 2026-11-24; ①~⑥ 구현 완료: ⑤ `spec_gate.py`, ⑥ 워커 자체 재측정 루프 운영 중): ① Rust 프로브가 `.flx` 요소의 선언 스타일(`StyleRefinement` 직렬화)과 소스 위치를 함께 출력 ② 공식 CDP 명세 추출(`DOMSnapshot`·`CSS.getMatchedStylesForNode`·`forcePseudoState`, 공식 데스크톱에서 동작 확인 필요) ③ 명세↔Rust 자동 차이 목록(`spec_delta.py`) ④ 테마 토큰 자동 생성(M9.11) ⑤ flx·재생·아이콘 래칫 게이트 ⑥ 워커가 차이 목록 0줄을 수용 기준으로 스스로 재측정. 순서: ①·④ → ② → ③ → ⑤ → ⑥.
2. **메시지 영역 마감.**
   - 2026-11-25 `spec_delta` 루프로 #markdown 전체 차이 61→15(`a78c5796e`·`71db04e6a`). 원인 확정: 코드 블록은 `.hljs` 위아래 0.25rem 누락(8px 짧았음), 본문 여분은 `.messageText` 아래 1px 누락. 남은 구조 차이: 공식은 연속 메시지를 한 `message-group` 상자로 묶는다. 제목 줄높이 1.1px 잔차(GPUI 고정 간격).
   - 스포일러 상자, 컬러 이모지, 코드 블록 테두리, 목록 글머리를 맞춘다.
3. **화면 확장.** DM·친구·설정 탭·음성 화면에서 같은 루프를 반복한다. 크롤은 쓰기 가드를 켠 상태로만 실행하고, 끝나면 `docs/PARITY_LAB.md` 10.5 복구 점검을 한다.
4. **사람 실기 검증.** 위 Blockers의 하드웨어 항목을 확인한 뒤에만 `docs/progress/progress.json`을 `done`으로 올리고 `python -I scripts/progress_svg.py`로 배지를 갱신한다.

## 미디어 검증 경계

- H.265는 로컬 LiveKit에서 Chrome·Rust 수신, 재구독 복구를 확인했다. 실제 Fluxer 채널·별도 기기·Rust UI 영상 표시·화면 오디오·E2EE·장시간 성능은 미검증이다.
- 화면 공유는 송신 코드(`e94acf19e`)만 있고 실제 송수신은 동작하지 않는다(사용자 확인, M9.5).

## 문서 지도

- 활성: `README.md`(개요·명령), `PROGRESS.md`(이 문서), `docs/UI_PARITY_ROADMAP.md`(실행 계획), `docs/PARITY_LAB.md`(검증 방법·운영 가이드·외부 사례), `docs/rust_client_core/{GATEWAY_PROTOCOL,REST_AND_AUTH,STATE_MODEL}.md`(코어 레퍼런스), `docs/progress/progress.json`(기능 추적).
- 로컬 보관(Git 제외, `docs/archive/*.local.md`): 이전 진행 기록, 끝난 M1~M8 상세, 이전 단계 계획서(`RUST_*_PLAN`, `NATIVE_MIGRATION_PLAN`, `IPC_CONTRACT_BASELINE`, `FEATURE_GAPS`, `NEXT_BRIEFS`), 비공개 조사(`IPC_M0_FINDINGS`, `RUST_PORT_RESEARCH_*`, `RUST_MIGRATION_REFERENCE_REVIEW`, `model-selection-evidence`). 작업 기준으로 삼지 않는다.
