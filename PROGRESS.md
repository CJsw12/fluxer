# PROGRESS

현재 상태, 인계, 다음 작업 순서를 관리한다. 마일스톤을 마칠 때 이 문서를 갱신하며, 로드맵은 목표나 수락 기준이 바뀔 때 갱신한다. 이전 기록은 `docs/archive/*.local.md`(Git 제외)에 보관한다.

## 새 세션 시작 순서 (모든 도구 공통)

1. 도구별 지침(Claude: `CLAUDE.md`, Antigravity: 전역 `~/.gemini/config/GEMINI.md`) → `AGENTS.md`(공통 규칙) → 이 문서 → 로컬 운영 메모 `Logs/handoff/OPERATIONS.md`(Git 제외: 시험 환경·상시 실행·상태 스냅샷)를 읽는다. 명령어 모음은 `README.md`.
2. `docs/AGENT_GUIDE.md`(규칙·컨텍스트 예산·도구 한 줄 사용법·작업 흐름, 약 2천 토큰)를 읽는다. `docs/PARITY_LAB.md`는 참고서라 통째로 읽지 않고, 필요한 절만 찾아 읽는다.
3. 상시 실행 구성(시험 서버 터널, 측정 기능을 켠 Rust 클라이언트, 비교용 공식 앱)을 `docs/PARITY_LAB.md` 10.1대로 확인·기동한다. 측정 전 점검 4가지: Rust `Status: Online`, Rust 멤버 목록 `Online—2`, CDP 9334 응답, 양쪽 마이크·스피커 켜짐(10.8 1단계).
4. 아래 Next Steps 1번부터 **사용자에게 묻지 말고 바로** 진행한다(선택이 필요하면 추천안으로 진행하고 보고에 남긴다). 사용자 선호(자율 진행·커밋·push 상시 승인·빌드 부하 제한)는 `Logs/handoff/OPERATIONS.md` 첫 절에 있다.

## 현재 상태 (2026-11-26)

- **Baseline:** 브랜치 `desktop-rs`(작업), `main` = `upstream/main`(`3608a387c`). 주력은 `fluxer_client/`(Rust GPUI + gpui-kit). 음성·화면 공유는 헬퍼 `fluxer_client/voice_worker`(LiveKit, 정적 CRT). `fluxer_desktop_rs/`는 WebView2 전환기 셸.
- **Done(2026-11-26, Feature 1~10):**
  - `76723c648`: 설정 탭(Windows app 분리·Connections·Applications 조회·Privacy 보강·사이드바 `.flx()`, `750 tests`, `210 icon sites`).
  - `226e8dc36`: Keybinds 편집기·단축키 영속화·런타임 연동(`Theme.background_modifier_accent`, `754 tests`, `225 icon sites`).
  - `19a444ca7`: 명세 루프 확장 4장면(`voice-channel`, `friends-list`, `threads-panel`, `light-theme`, 10.5 복원 가드, `755 tests`).
  - `6e5551ffc`: 남은 수치 차이 수렴(마크다운 22/24px line-height·`message-group`·`-10.78px`·`#635fe3`·DM 각주·`spec_baseline.json` 하향 래칫, `756 tests`).
  - `ba88d064b`: M9.11 테마 토큰 자동 생성(`theme_tokens_generated.rs` dark 654·light 656 / 682 RGBA, `Theme::from_kind`, `757 tests`, 릴리스 SHA256 `1bc86b64`).
  - `06811e7dd`: M9.9 공식 아이콘 수확(`official_icons.json` 244건, `icon_manifest.json` verified 80→113·unverified 145→112, `app_icons_load`, `check_icons.py --selftest`).
  - `a47e88a9d`: M9.1 조작 요소 추출기(`extract_interactions.py`, `interactions.json` 5개 범주 770건: present 53·missing 643·differs 74·후속 717건).
  - `0f3c2a4b3`: M9.7 요소 픽셀 MAE/SSIM·상위 10개 `worst_crops/` + M9.8 확장 스타일·`style_pixel_audit` (22개 장면 `spec_gate` 0회귀).
  - `11d879106`: M9.5 화면 공유 선택(`ScreenSharePicker`)·화질/FPS/코덱/비트레이트·`"<이름>'s screen · <height>p <fps> FPS"`·`ActiveSpeakers`·`share_check.py --launch-rust`/`voice_audio` (`voice_worker` 14 + 워크스페이스 760 = 774 tests, 릴리스 `9a3b3479`, 워커 `9ada6a5a`, `check_resize` 25/25, `check_surface_parity` 10/10, `state_matrix --strict`, `spec_gate` 22개 장면 0회귀).
  - M9.10·M9.12·M9.13·M9.6 문서화 및 도구 반영: `FLUXER_DISABLE_MODULE_UPDATES=1` 문서화, `spec_official.mjs`·`compare.py` 버전 경고(`version_warning`), M9.12(`gpui-pre 0.3.12` 헤드리스 렌더 미지원 결정)·M9.13(웹뷰 혼용 기준), M9.6 `verified` 기준 진행도(`progress.json`, `progress_svg.py`, 배지·지도·`README.md`).
- **Decisions:**
  - 1:1 판정은 공식 데스크톱 앱 실측 수치가 기준이다. 스크린샷 육안 비교는 마지막 확인용, 정규식 감사·합성 갤러리는 보조다.
  - 비교 기준 앱은 포터블 공식 데스크톱 사본(다크, tester2)이며 자동 업데이트 방지를 위해 `FLUXER_DISABLE_MODULE_UPDATES=1`을 사용한다. Rust는 tester1이므로 본인 시점·상대 이름·색 차이는 결함이 아니다. 사용자 개인 공식 앱은 조작하지 않는다.
  - 색은 하드코딩하지 않고 `ThemeVariableManifest.ts`에서 자동 생성한 `theme_tokens_generated.rs`와 `Theme` 필드를 사용한다(다크·라이트 모두).
  - 공식에 있지만 Rust 기능이 없는 메뉴 항목은 보이게 두고 각자 고유 핸들러(메뉴 닫기만)와 `TODO(backlog)`를 단다. 핸들러 복붙·숨은 호버 버튼은 금지.
  - Windows MSVC에서 `libwebrtc`가 정적 CRT를 요구해 LiveKit·오디오는 헬퍼 프로세스로 분리한다(비공개 JSON-lines IPC, 토큰 argv·로그 금지).
  - M9.12: `gpui-pre 0.3.12`의 `TestWindow::render_to_image`가 `HeadlessRenderer` 미설정으로 동작하지 않으므로 구조적 `TestAppContext` + 동일 데스크톱 실기 프로브 스크린샷 체계를 유지한다.
  - M9.13: 핵심 UI(채팅·레일·사이드바·설정·팝오버·음성 스테이지/화면 공유 뷰어)는 네이티브 GPUI를 유지하고, 향후 고비용 리치 임베드 등만 WebView2 혼용 후보로 제한한다.
  - 모델·위임은 도구별 지침을 따르되, 난이도에 맞게 배정하고 루트는 실측 판정·통합·커밋만 한다(`docs/PARITY_LAB.md` 10.3·10.9).
- **Blockers:**
  - 공식 비교 앱(`Web 2026.1124.64728`)이 테스트 서버에서 `GatewayReadyTimeoutError`·`Voice server timeout`을 겪어 M9.5 양방향 `share_check` 최종 완료 판정은 버전 정렬 후 재검증이 필요하다(M9.10).
  - `icon_manifest.json` 미검증 아이콘 112곳(M9.9)과 `interactions.json` 누락·차이 717건(M9.1 후속)은 별도 단계로 이어간다.
  - 마크다운 인라인 요소(굵게·인용·목록·코드 등)는 gpui-component `TextView`가 그려 측정 태그를 붙일 수 없다.
  - 사람·하드웨어가 필요한 항목: 한글 IME 조합, OS 알림·소리, Windows Hello·FIDO2 실기, 장시간 부하, DM 통화 실제 음성.

## Next Steps

1. **M9.5 양방향 화면 공유 재검증 (버전 정렬 후).** 비교 앱 버전을 테스트 서버·저장소와 맞춘 뒤(`FLUXER_DISABLE_MODULE_UPDATES=1`), 실기 `share_check.py --launch-rust`로 `rust_to_official`, `official_to_rust`, `voice_audio` 최종 통과를 확인하고 M9.5를 완료 처리한다.
2. **T3 고위험 백로그 구현 및 교차 계열 검토.** 메시지 삭제·일괄 삭제·길드 탈퇴·밴·인증 등 서버 데이터 변경·삭제·권한 경로(`TODO(backlog)`)는 구현 후 다른 계열 모델의 독립 검토를 거쳐 반영한다.
3. **M9.9 미검증 아이콘 수확 및 M9.1 후속.** `icon_manifest.json` 미검증 112곳을 공식 원본 SVG로 마저 검증하고, `interactions.json` 후속 717건을 우선순위대로 반영한다.
4. **사람·하드웨어 실기 검증.** 위 Blockers의 하드웨어 항목(한글 IME·OS 알림·Windows Hello·장시간 부하·실제 음성)을 확인한 뒤에만 `docs/progress/progress.json`의 `verified`를 `true`로 올리고 `python -I scripts/progress_svg.py`로 배지를 갱신한다.

## 미디어 검증 경계

- H.265는 로컬 LiveKit에서 Chrome·Rust 수신, 재구독 복구를 확인했다. 실제 Fluxer 채널·별도 기기·Rust UI 영상 표시·화면 오디오·E2EE·장시간 성능은 미검증이다.
- 화면 공유는 M9.5 1차 자동 시험(`share_check.py`)에서 양방향 통과(Rust→공식 2560×1440 약 14fps, 공식→Rust 색 결함 수정 후 통과)했고, 2차(`11d879106`)로 공유 대상 선택(`ScreenSharePicker`)·화질/FPS/코덱/비트레이트·뷰어 헤더(`"<이름>'s screen · <height>p <fps> FPS"`)·`ActiveSpeakers` 및 Rust 측 실기 피커/`Go Live`/`LIVE`/`speaking_state`까지 검증했다. 양방향 `share_check` 최종 통과는 공식 비교 앱(`Web 2026.1124.64728`)과 테스트 서버 버전 정렬 후 재검증이 필요하다.

## 문서 지도

- 활성: `README.md`(개요·명령), `PROGRESS.md`(이 문서), `docs/UI_PARITY_ROADMAP.md`(실행 계획), `docs/AGENT_GUIDE.md`(에이전트 시작 안내), `docs/PARITY_LAB.md`(검증 방법·운영 가이드·외부 사례, 참고서), `docs/rust_client_core/{GATEWAY_PROTOCOL,REST_AND_AUTH,STATE_MODEL}.md`(코어 레퍼런스), `docs/progress/progress.json`(기능 추적).
- 로컬 보관(Git 제외, `docs/archive/*.local.md`): 이전 진행 기록, 끝난 M1~M8 상세, 이전 단계 계획서, 비공개 조사. 작업 기준으로 삼지 않는다.
