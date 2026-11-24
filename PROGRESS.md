# PROGRESS

현재 상태, 인계, 다음 작업 순서를 관리한다. 마일스톤을 마칠 때 이 문서를 갱신하며, 로드맵은 목표나 수락 기준이 바뀔 때 갱신한다. 이전 기록은 `docs/archive/*.local.md`(Git 제외)에 보관한다.

## 새 세션 시작 순서 (모든 도구 공통)

1. 도구별 지침(Claude: `CLAUDE.md`, Antigravity: 전역 `~/.gemini/config/GEMINI.md`) → `AGENTS.md`(공통 규칙) → 이 문서 → 로컬 운영 메모 `Logs/handoff/OPERATIONS.md`(Git 제외: 시험 환경·상시 실행·상태 스냅샷)를 읽는다. 명령어 모음은 `README.md`.
2. UI 작업이면 `docs/PARITY_LAB.md` 10장을 읽는다. 특히 10.8(명세 루프 표준 절차·워커 브리프 양식), 10.7(도구 빠른 참조), 10.6(자주 한 실수).
3. 상시 실행 구성(시험 서버 터널, 측정 기능을 켠 Rust 클라이언트, 비교용 공식 앱)을 `docs/PARITY_LAB.md` 10.1대로 확인·기동한다. 측정 전 점검 4가지: Rust `Status: Online`, Rust 멤버 목록 `Online—2`, CDP 9334 응답, 양쪽 마이크·스피커 켜짐(10.8 1단계).
4. 아래 Next Steps 1번부터 진행한다.

## 현재 상태 (2026-11-25)

- **Baseline:** 브랜치 `desktop-rs`(작업, 최신 push `e41e87941` 이후 문서 커밋), `main` = `upstream/main`. 주력은 `fluxer_client/`(Rust GPUI + gpui-kit). 음성·화면 공유는 헬퍼 `fluxer_client/voice_worker`(LiveKit, 정적 CRT). `fluxer_desktop_rs/`는 WebView2 전환기 셸. 미커밋 변경 없음.
- **Done(2026-11-24~25):**
  - 검증 도구: 공식 CSS 명세 추출 `spec_official.mjs`(단일 화면·장면 모드, 쓰기 가드), 명세↔Rust 수치 차이 `spec_delta.py`(줄마다 CSS 규칙·Rust `파일:줄`), 프로브의 선언 스타일·소스 위치 출력과 테두리 상자 측정, 누락 분류 `coverage.py`, 래칫 게이트 `spec_gate.py`(#markdown + 장면 셋 `popouts`·`more`), 테마 토큰 추출 `theme_tokens.mjs`, 아이콘 호출 위치 동기화 `check_icons.py --sync`.
  - 수렴 결과: #markdown 차이 61→실제 결함 0(래칫 33/155, 남은 것은 글꼴 메트릭·상속 줄높이·가상 목록 그룹 상자), 팝오버·모달 8장면 차이 163→11, 태그 커버리지 결합 96→176, DM(첨부 모자이크·통화 버튼·스포일러·반응·만료 문구·사이드바·헤더)·설정(사이드바·레이아웃·태그)·메시지 마우스 올림 바와 더보기 메뉴 공식화, 길드 헤더 팝오버 공식화, 팝오버 Escape·버튼 기준 정렬.
  - 이전: M9.2 상태 행렬(`8f49ada84`), M9.3 크기 조절(`fc6c1d245`), M9.4 팝오버 작업 목록(`0e1561775`), M9.5 화면 공유 양방향 자동 시험 1차(`ac638a6a3`), 업스트림 병합(`3cc9d8ffb`).
- **Decisions:**
  - 1:1 판정은 공식 데스크톱 앱 실측 수치가 기준이다. 스크린샷 육안 비교는 마지막 확인용, 정규식 감사·합성 갤러리는 보조다.
  - 비교 기준 앱은 포터블 공식 데스크톱 사본(Web 2026.1120, 다크, tester2)이다. Rust는 tester1이므로 본인 시점·상대 이름·색 차이는 결함이 아니다. 사용자 개인 공식 앱은 조작하지 않는다.
  - 색은 하드코딩하지 않고 공식 `ThemeVariableManifest.ts`·`ColorSystemPalette.ts` 규칙에 맞춘 `Theme` 필드로 넣는다(다크·라이트 모두).
  - 공식에 있지만 Rust 기능이 없는 메뉴 항목은 보이게 두고 각자 고유 핸들러(메뉴 닫기만)와 `TODO(backlog)`를 단다. 핸들러 복붙·숨은 호버 버튼은 금지.
  - Windows MSVC에서 `libwebrtc`가 정적 CRT를 요구해 LiveKit·오디오는 헬퍼 프로세스로 분리한다(비공개 JSON-lines IPC, 토큰 argv·로그 금지).
  - 모델·위임은 도구별 지침을 따르되, 난이도에 맞게 배정하고 루트는 실측 판정·통합·커밋만 한다(`docs/PARITY_LAB.md` 10.3).
- **Blockers:**
  - 진행도의 "93/93 구현"은 동작 확인 없이 센 값이라 신뢰하지 않는다(M9.6에서 집계 기준 변경).
  - 기준 버전: 저장소는 업스트림 2026-11-23(`3608a387c`), 비교 앱은 Web 2026.1120(M9.10).
  - 마크다운 인라인 요소(굵게·인용·목록·코드 등)는 gpui-component `TextView`가 그려 측정 태그를 붙일 수 없다. 비교 방법이 따로 필요하다.
  - 사람·하드웨어가 필요한 항목: 한글 IME 조합, OS 알림·소리, Windows Hello·FIDO2 실기, 장시간 부하, DM 통화 실제 음성.

## Next Steps

1. **기능 백로그(공식에 있고 Rust에 없음, `TODO(backlog)` 검색).**
   - 메시지 더보기 메뉴: Forward, Create thread, Translate, Pin, Bookmark, Mark unread, Copy link, Speak. 메시지 우클릭 메뉴 배선(프로브에 우클릭 없음).
   - 길드 헤더 팝오버: Invite members, Create category, Notification settings, Privacy settings, Edit community profile, Delete my messages, Hide muted channels 필터.
   - 탐색 페이지(나침반이 지금은 Join 모달을 엶), 설정 탭 목록(Connections·Windows app·Applications·Privacy), 키 설정 편집 행, 친구 검색, 업데이트 버튼, 검색 범위 버튼, 묶인 메시지 마우스 올림 시간, DM 영상 통화 카메라·호출, Personal notes 정리·메뉴.
2. **명세 루프 확장(`docs/PARITY_LAB.md` 10.8).** 음성 화면(릴리스+워커 빌드), 친구 목록·스레드 열린 화면, 라이트 테마 장면 추가. 장면을 추가하면 `spec_gate.py --live --all --update --allow-raise`로 기준을 넣는다.
3. **남은 수치 차이.** #markdown 메시지 줄높이 22≠24(상속), 연속 메시지 `message-group` 묶음 구조, 사용자 팝오버·스레드 글꼴 메트릭, DM 대화 77건(대부분 계정 방향·글꼴).
4. **M9 잔여.** 미검증 아이콘 84곳 수확(M9.9), M9.11 테마 토큰을 Rust `Theme` 생성에 연결(추출기는 완료), M9.6·M9.10·M9.12·M9.13.
5. **사람 실기 검증.** 위 Blockers의 하드웨어 항목을 확인한 뒤에만 `docs/progress/progress.json`을 `done`으로 올리고 `python -I scripts/progress_svg.py`로 배지를 갱신한다.

## 미디어 검증 경계

- H.265는 로컬 LiveKit에서 Chrome·Rust 수신, 재구독 복구를 확인했다. 실제 Fluxer 채널·별도 기기·Rust UI 영상 표시·화면 오디오·E2EE·장시간 성능은 미검증이다.
- 화면 공유는 M9.5 1차 자동 시험(`share_check.py`)에서 양방향 통과(Rust→공식 2560×1440 약 14fps, 공식→Rust 색 결함 수정 후 통과). 품질 설정(해상도·FPS·코덱) 전달과 공유 대상 선택 창은 미구현이다.

## 문서 지도

- 활성: `README.md`(개요·명령), `PROGRESS.md`(이 문서), `docs/UI_PARITY_ROADMAP.md`(실행 계획), `docs/PARITY_LAB.md`(검증 방법·운영 가이드·외부 사례), `docs/rust_client_core/{GATEWAY_PROTOCOL,REST_AND_AUTH,STATE_MODEL}.md`(코어 레퍼런스), `docs/progress/progress.json`(기능 추적).
- 로컬 보관(Git 제외, `docs/archive/*.local.md`): 이전 진행 기록, 끝난 M1~M8 상세, 이전 단계 계획서, 비공개 조사. 작업 기준으로 삼지 않는다.
