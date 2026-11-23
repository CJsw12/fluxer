# PROGRESS

현재 상태, 인계, 다음 작업 순서를 관리한다. 마일스톤을 마칠 때 이 문서를 갱신하며, 로드맵은 목표나 수락 기준이 바뀔 때 갱신한다. 기록 보관은 전역 진행 지침을 따른다.

## 현재 상태 (2026-11-24)

- **Baseline:** `desktop-rs`에서 공식 Fluxer 1:1 UI 패리티 로드맵(`docs/UI_PARITY_ROADMAP.md`) **M1~M3 전체(`324718aa8`..`b512e911c`)**, **패스키·보안 키(WebAuthn / FIDO2, `4b4138821`)**, **M4 전체(`80b09962a`)**, **M5 전체(`16d75565f`)**, **M6 전체(`393e32888`)**, 그리고 **M7 전체(M7.1 포팅 검증 도구 `scripts/audit_ui_parity.py` v2 고도화 — 정적 `info_card_row` 배지 치환 차단 및 호버/인디케이터 상태 머신·인터랙티브 컨트롤 155개 항목 전수 감사 · M7.2 좌측 길드 레일 `Direct messages`/`Favorites`/`Guild`/`Discover`/`Join` 1:1 호버 모핑 및 좌측 흰색 인디케이터 바 `40px`/`20px`/`8px` 상태 머신 이식 · M7.3 설정 `Voice & Video` 탭의 화면 공유 해상도/프레임레이트/비디오 코덱/비트레이트/하드웨어 가속/카메라 해상도·미러링 및 음성 처리 프로필/노이즈 억제 백엔드/에코 캔슬레이션/자동 게인/VAD 감도/PTT 해제 지연/인터랙티브 마이크 테스트 전수 이식 · M7.4 `Privacy & Safety`/`Messages & Media`/`Notifications`/`Language` 탭 정적 배지를 인터랙티브 컨트롤로 전수 전환)**까지 완료했다(`docs/progress/progress.json` 기준 추적 대상 **93/93 기능 100% 구현**, 전체 **90%**·일상 사용 **92%**). 검증 기준선은 `scripts/audit_ui_parity.py` **155/155 passed**, `fluxer_client` 전체 워크스페이스 **693 passed**(앱 364·코어 280·마크다운 36·i18n 10·devserver E2E 3, 0 failed) + `voice_worker` **10 passed**(0 failed, 총 **703 passed**), 오프라인 12-시나리오(`1280×800`·`1920×1040`, Light/Dark) 릴리스 PNG 캡처, 로컬 `fluxer_devserver` 릴리스 E2E, 전용 테스트 서버 로그인 상태 32-시나리오 전수 실기 캡처·공식 앱 대조를 포함한다.
- **Done:** 답장 조회(`cfb8d1b3d`)·재시도 제한(`4dff14772`)·편집 순서(`3184571c8`)·대량 삭제(`e81560367`)·GIF 확대 보기(`b40b7c73f`), 2단계 1~6번 핵심 계약, 음성 장치·볼륨·OS 열거(`d63dd74a3`·`e9afd20d1`·`3d5e5bb0c`), 격리 실기용 `--no-os-integration`(`f48890893`), 접근성 동작 줄이기·AccessKit 라벨(`ad16d65dc`·`5531d8254`), 원격 비디오 발견·구독·디코드·GPUI 뷰어(`3791963b0`·`a33512209`·`9378a8e20`·`92361b719`·`7438375c3`), 안전 URL 검증 업데이트 확인기·에셋 다운로드·SHA-256 검증(`0df3bd344`·`d030b38bf`), `Codex (gpt-6.2-sol)` 1·2·3차 읽기 전용 독립 교차 검토(`7438375c3`·`7f971ed1b`·`8f4ed6548`·`3b516e2b2` 및 WebAuthn T3 교차 검토), `voice_worker` Safe-Rust `DesktopCapturer` 화면 공유 송신(`e94acf19e`), 4단계 진행도 시스템·README 자동 동기화(`98c903b84`), 무인 `/goal` 마일스톤 G1~G4(`2f71d33c3`·`14b56d656`·`367f0c287`·`ba0cb82dd`), Orca 공식 웹↔릴리스 Rust 앱 실시간 E2E 검증(`d030b38bf`, 8개 항목 추가 `done` 승격), 음성 `https://`→`wss://` 엔드포인트 정규화(`c5c12c81a`), 공용 2단 설정 셸·사용자/채널/역할/밴 설정 2단 개편·메인 셸 1차 마감(`588c1ff29`·`f7e9a18d5`·`e08ba4815`), **M1.1** 검색 결과 복구(`324718aa8`), **M1.2** 공용 `ui::avatar` 일괄 교체(`108d53231`), **M1.3** Phosphor SVG 42종 교체(`1b9d2167d`), **M2.1** 공식 `ThemeSizes` 1:1 동기화(`301fade44`), **M2.2** 중앙 음성/비디오 스테이지(`1725a44fa`), **M2.3** 커스텀 타이틀바 및 설정 `Appearance` 테마 스와치·슬라이더(`52afe5f9a`), **M3.1** 오프라인 12-시나리오 시각 갤러리 하네스(`47c1ced3a`), **M3.2** 시각 갤러리 영역별 diff 결함 수렴 및 공식 앱·전용 테스트 서버 실기 대조(`2e9df9f26`·`55cf2c879`·`b512e911c`, 4건 `done` 승격), **패스키·보안 키(WebAuthn / FIDO2)** 전체 구현(`4b4138821`), **M4.1~M4.5** 공식 `fluxer_app/src` 기반 자동 패리티 감사·`LocalVoiceStateMachine` 데픈↔뮤트 결합·통합 `UserArea`+`VoiceConnectionStatus`·상하 분할 및 오디오 아바타 리스트+분할 `VoiceControlBar`·설정 모달 공식 12개 탭 및 하위 섹션 트리 전수 구현(`80b09962a`), **M5.1~M5.6** 세부 인터랙션 팝아웃·길드 레일 홈 브랜드 아이콘 및 서버 이미지·채널/서버 설정 2단 모달 탭 통합·사이드바 음성 행 우측 아이콘/LIVE 배지·설정 신규 섹션 `Prefs` 실시간 스위치 연동(`16d75565f`), **M6.1~M6.4** 로그인 상태 전 시나리오 제어(`fluxer://scene/*`)·실제 서버 YouTube 임베드 중복 썸네일 수정·본인 멘션 행 전체 하이라이트·길드 레일/서버 헤더 드롭다운/채널 헤더/입력창 우측 아이콘 바 수렴 및 전용 테스트 서버 로그인 32-시나리오 전수 실기 캡처·대조(`393e32888`), **M7.1~M7.5** 포팅 검증 도구 v2(정적 배지 치환 금지·155/155 계약 감사)·길드 레일 호버 모핑 및 좌측 흰색 인디케이터 바(`40px`/`20px`/`8px`) 상태 머신·비디오 코덱/해상도/FPS/비트레이트 및 음성 처리/마이크 테스트 등 전체 설정 인터랙티브 컨트롤 100% 이식을 완료했다.
  - **시험 서버:** 전용 CA를 사용하는 SSH 터널에서 탐색 API, 시험 계정 로그인과 프로필·길드·채널·메시지 조회가 HTTP 200, 로그아웃은 204였다. 별도 게이트웨이 검사에서 HELLO·READY·하트비트 ACK·RESUMED를 확인했다. 로컬 fixture E2E 3건도 통과했다. 시험 계정 사이에서 합성 PNG와 2프레임 GIF를 업로드하고 다시 내려받아 파일 형식과 프레임 수를 확인했다.
  - **Windows 릴리스:** SHA-256 `AF3A852B0692781C…`의 격리 앱에서 시험 DM을 받았다. 터널 재연결 뒤 같은 앱의 읽지 않은 항목이 2→3으로 늘어 후속 DM 수신도 확인했다. 시험 메시지 두 건은 정리했다. 최신 `e81560367` 릴리스에서 시험 DM의 PNG·GIF 표시와 확대 보기, 파일 선택부터 PNG 전송·서버 수신까지, YouTube 링크 카드와 스크롤을 확인했다. GIF 프록시는 두 프레임을 반환했고 Windows 애니메이션 효과가 꺼진 상태에서 앱은 첫 프레임을 표시했다.
  - **공식 웹 비교:** 격리된 Orca 프로필에서 같은 시험 서버와 DM을 열었다. 동작 줄이기 상태에서도 공식 웹은 사용자가 GIF 확대 보기를 열면 두 프레임을 재생했고, Rust 앱은 확대 보기에서도 첫 프레임에 머물렀다. PNG는 양쪽에서 표시됐다. `b40b7c73f`는 동작 줄이기를 유지하면서 사용자가 연 GIF 확대 보기에서만 재생한다. 확대 보기 테스트 2건을 통과했다. 시험 서버의 Windows 디버그 앱에서 동작 줄이기를 켠 채 활성 창 7장을 비교해 확대 보기의 두 프레임 재생과 대화 썸네일의 정지를 확인했다. 증거는 Git 밖에 보관했다.
  - **앱 계측:** 10분 2초 동안 121회 샘플링했다. DM 2회와 터널 재연결 1회를 포함한다. Private bytes 85.4~126.1 MiB(첫 86.4, 마지막 122.8), working set 77.1~137.2 MiB, CPU 평균 2.9%(논리 코어 1개 기준), GPU 최고 엔진 사용률 1.3%, 전용·공유 GPU 메모리 최대 43.4·4.7 MiB였다. 원본 카운터는 Git 밖에 보관했다.
- **Decisions:** 주력은 `fluxer_client/`의 GPUI + gpui-kit 앱이다. Rust UI·미디어의 실기 동등성 검증 후 데스크톱 실행 경로에서 WebView2를 제거한다. Windows MSVC에서 `livekit`/`webrtc-sys`의 사전 빌드 `libwebrtc`는 정적 CRT(`/MT`, `+crt-static`)를 요구하므로, GPUI 앱(`/MD`)의 빌드·의존성과 격리하기 위해 네이티브 LiveKit Room·오디오는 별도 헬퍼 프로세스(비공개 stdin/stdout JSON-lines IPC, `kill_on_drop`, 토큰 argv/로그 금지)로 분리해 `VoiceTransport`/`VoiceRoom`에 연결한다. **현재 모든 에이전트(Codex·Claude·Antigravity)의 유일한 활성 UI·검증 계획서는 `docs/UI_PARITY_ROADMAP.md`이며, 기존 `RUST_FULL_MIGRATION_PLAN.md`·`RUST_DESKTOP_EXECUTION_PLAN.md`는 `docs/archive/`로 보관했다.**
- **Blockers / Known UI Gaps:**
  - **기능 구현 완료(93/93, `todo=0, partial=0`), 사람·하드웨어 개입 필요 실기 항목만 남음:** 실제 한글 IME 조합 입력, 실기기 양방향 음성·화면 공유 체감 확인(`c5c12c81a`·`1725a44fa` 수정본 실기 재확인 필요), OS 알림 팝업·소리, 장시간 입력 부하 계측, 실제 Windows Hello·FIDO2 하드웨어 보안 키 터치를 거치는 `패스키·보안 키` 실기 승인(`implemented`→`done` 승격 대기).

### Next Steps (새 세션 진입 시 순서대로 실행)

> **주 검증 방법(2026-11-24~):** `docs/PARITY_LAB.md`(공식 앱 DOM 실측 ↔ Rust 프로브 조인 비교). 정규식 감사(`scripts/audit_ui_parity.py`)와 합성 갤러리는 보조로만 쓴다.
>
> **활성 로드맵:** `docs/UI_PARITY_ROADMAP.md` (M1.1~M8.4 + 패스키·보안 키까지 전체 93/93 기능 구현·정적 AST/상태머신/인터랙티브 컨트롤 184/184 자동 감사·오프라인 시각 검증·전용 테스트 서버 실기 대조·릴리스 패키징 완료; 구 `RUST_*_PLAN.md`·`NATIVE_MIGRATION_PLAN.md`·`NEXT_BRIEFS.md`는 보관본이므로 따르지 않는다.)

1. **M1 — 공용 렌더러 강제 재사용 & 에셋 통일 (`docs/UI_PARITY_ROADMAP.md#m1`) [완료]:**
   - `M1.1` 완료(`324718aa8`), `M1.2` 완료(`108d53231`), `M1.3` 완료(`1b9d2167d`).
2. **M2 — 공식 치수 동기화 & 미구현 핵심 화면 (`docs/UI_PARITY_ROADMAP.md#m2`) [완료]:**
   - `M2.1` 완료(`301fade44`), `M2.2` 완료(`1725a44fa`), `M2.3` 완료(`52afe5f9a`).
3. **M3 — 오프라인 12-시나리오 시각 갤러리 & 네이티브 창 자동 대조 (`docs/UI_PARITY_ROADMAP.md#m3`) [완료]:**
   - `M3.1` 완료(`47c1ced3a`), `M3.2` 완료(`2e9df9f26`·`55cf2c879`·`b512e911c`, 전용 테스트 서버 포터블 공식 앱↔릴리스 앱 실기 대조 및 4건 `done` 승격 완료).
4. **패스키·보안 키(WebAuthn / FIDO2) 구현 및 T3 독립 교차 검토 [완료]:**
   - REST 8개 메서드, Windows 네이티브 `webauthn.dll` 브리지, 로그인/MFA/설정 Account UI 구현 및 `Codex (gpt-6.2-sol high)` T3 교차 검토 지적 7건 수정 완료(`4b4138821`).
5. **M4 — 공식 원본(`fluxer_app/src`) 기반 전수 자동 감사 & 음성·하단 패널·설정창 1:1 수렴 (`docs/UI_PARITY_ROADMAP.md#m4`) [완료]:**
   - `M4.1`(`scripts/audit_ui_parity.py`), `M4.2`(`LocalVoiceStateMachine` 데픈↔뮤트 결합 및 `should_unmute_on_undeafen`), `M4.3`(통합 `UserArea` + `VoiceConnectionStatus` + 2열 미디어 컨트롤), `M4.4`(`CompactVoiceCallView` 상하 분할 + 오디오 원형 아바타 리스트 + 드롭다운 분할 `VoiceControlBar`), `M4.5`(`UserSettingsModal` 공식 12개 탭 + 좌측 하위 섹션 트리 + 내부 섹션 카드) 구현·검증 완료(`80b09962a`).
6. **M5 — 세부 인터랙션 팝아웃 · 길드 레일 아이콘 · 모달 탭 통합 · 사이드바 음성 행 1:1 마감 (`docs/UI_PARITY_ROADMAP.md#m5`) [완료]:**
   - `M5.1`~`M5.6` 구현·검증 완료(`16d75565f`).
7. **M6 — 로그인 실기 전 시나리오 자동 제어(`fluxer://scene/*`) · 공식 앱 실기 대조 격차 수렴 (`docs/UI_PARITY_ROADMAP.md#m6`) [완료]:**
   - `M6.1`~`M6.4` 완료(`393e32888`).
8. **M7 — 포팅 검증 방법론 고도화(정적 배지 치환 금지 · 호버/인디케이터 상태 머신 감사) 및 길드 레일 호버 · 음성/비디오/전체 설정 인터랙티브 컨트롤 100% 이식 (`docs/UI_PARITY_ROADMAP.md#m7`) [완료]:**
   - `M7.1`(`scripts/audit_ui_parity.py` v2 155/155 통과 및 정적 배지 치환 금지), `M7.2`(길드 레일 `Direct messages`/`Favorites`/`Guild` 호버 모핑 및 좌측 흰색 인디케이터 바 `40px`/`20px`/`8px` 상태 머신), `M7.3`(`Voice & Video` 탭 카메라·화면 공유 해상도/프레임레이트/코덱/비트레이트/하드웨어 가속 및 음성 처리 프로필/노이즈 억제 백엔드/에코 캔슬레이션/자동 게인/VAD 감도/PTT 해제 지연/마이크 테스트 전수 이식), `M7.4`(`Privacy & Safety`/`Messages & Media`/`Notifications`/`Language` 인터랙티브 컨트롤 전수 전환), `M7.5`(테스트 서버 실기 검증 및 릴리스 패키징) 완료.
9. **M8 — 화면 단위 픽셀 · CSS 레이아웃 · 아이콘 1:1 수렴 (`docs/UI_PARITY_ROADMAP.md#m8`) [완료]:**
   - 좌측 열 통합·`56px` 경계 정렬, 공식 Phosphor 아이콘 5종, 호버 전용 관리·계정 버튼, 친구 탭 스타일, 검은 통화 스테이지·`17px` 코너 배지, 설정 프로필 배너 헤더를 반영했다. 감사 184/184, 워크스페이스 693 + `voice_worker` 10 통과, 릴리스 패키징 경고 0건.
   - **검증 방법 결함 발견:** M8 로그인 실기 재캡처는 하네스가 존재하지 않는 CA 파일을 `SSL_CERT_FILE`로 넘겨 32장 모두 `Reconnecting…` 상태의 같은 화면이었는데도 종료 코드 0으로 끝났다. 장면 성공 여부를 판정하지 않는 캡처는 검증으로 인정하지 않는다. 후속은 10번의 Parity Lab이다.
10. **Parity Lab — 공식 앱 DOM 기준 자동 비교·탐색 체계 (`docs/PARITY_LAB.md`) [도구 완성, 메인 화면 수렴 중 — 다음 세션은 여기서 시작]:**
   - **도구**(`82bed49a3`·`9acf3254c`): CDP 기준값 추출(`web_capture.mjs`), 클릭 크롤러(`crawl.mjs`, 쓰기 차단 가드 포함), 조인 비교기(`compare.py`), 장면 점검(`check_scene.py`), Rust 패리티 프로브(`FLUXER_PARITY_PROBE=1`, IPC `probe_inspect/resize/click/hover/key`), 재생 러너(`replay.py`). 사용법·상시 실행 구성·위임 방식·함정·복구 점검은 `docs/PARITY_LAB.md` 10장.
   - **수렴**(`00dbe2159`·`df21a8585` + 이번 인계 커밋): #markdown 기하 일치 4/53 → 셸 대부분 0~1px(다크 데스크톱 기준). 헤더·멤버 목록 배치, 아이콘 버튼, 줄높이, 하단 사용자 영역 겹침, 테마 토큰(`background_secondary_lighter`, `text_chat/secondary/tertiary`), Segoe UI Variable Text, 타임스탬프, 타이틀바, 레일 fill 아이콘을 맞췄다.
   - **사고와 조치(2026-11-24):** 깊이 2 크롤이 시험 서버에 반응 4개와 #markdown 채널 알림 덮어쓰기(tester2)를 남겼다. 둘 다 API로 원복·확인했고(반응 0개, 덮어쓰기 `muted: false`·`message_notifications: 3`), 크롤러에 GET 외 API 요청을 전송 전에 막는 페이지 내 가드를 넣었다(CDP `Fetch`는 데스크톱 앱의 사용자 정의 스킴을 가로채지 못함).
   - **다음 순서:** ① 메시지 영역 세로 치수(바닥 여백 25 → 메시지 높이: ①·③ 메시지 본문 아래 28·24px 원인 미확정, 코드 블록 메시지 9px 작음) ② 인라인 스포일러 박스·컬러 이모지·코드 블록 테두리·목록 글머리 ③ 가드가 들어간 크롤러로 다크 깊이 2 크롤 재실행(`crawl-md-dark`는 52개 상태에서 중단) → `replay.py` 체크리스트 → 버튼으로 열리는 창(커뮤니티 메뉴, 알림·검색 범위 메뉴, 스레드·고정·받은 편지함 팝오버, 미디어/스티커/이모지 피커, 사용자 메뉴, 사용자 설정 모달) 공식 구조 이식과 `.flx` 태깅 ④ DM·친구·설정 탭·음성 화면으로 확장.
   - **검증 사각지대(최우선 보완, `docs/UI_PARITY_ROADMAP.md` M9):** 사용자가 지적한 4건을 지금까지의 방법이 잡지 못했다.
     1. 앱 내부 크기 조절 미구현
     2. 화면 공유 실제 송수신 미동작
     3. 레일 호버 동작 차이(툴팁 없음, 탐색 버튼 호버 모양, 음성 활동 배지 없음)
     4. 왼쪽 아래 프로필 클릭 결과가 공식 프로필 카드가 아님
     - 원인: 한 가지 상태만 비교, 조작·기능 미판정, 측정 결과를 작업 목록으로 옮기는 절차 없음, 동작 확인 없는 "구현" 집계.
     - 다음 세션은 M9.1~M9.6을 메시지 영역 마감보다 먼저 한다. 진행도의 "93/93 구현"은 신뢰하지 않는다.
   - **미해결:** 공식 비교 사본(tester2)과 Rust(tester1)의 계정이 달라 본인 시점 표시(멘션 강조·사용자 영역)는 비교에서 제외한다. 저장소 `fluxer_app` 직접 빌드(기준 버전 고정)는 미실행.
11. **후속 하드웨어 실기 검증(사람 개입 필요):**
   - 자체 시험 서버에서 최신 릴리스(`dist/fluxer_client/fluxer_client.exe`)로 실제 Windows Hello/FIDO2 패스키 등록·로그인, 실기기 양방향 음성·화면 공유, 한글 IME 조합, OS 알림·소리, 장시간 부하를 확인한 뒤 확인된 항목만 `docs/progress/progress.json` `done`으로 승격하고 `python -I scripts/progress_svg.py`로 배지·README를 갱신한다.

## 에이전트 지침 정리 (2026-11-21)

- **Baseline:** `desktop-rs` `1647b3c59`, 기존 작업 트리 변경을 보존했다.
- **Done:** 전역과 Fluxer 시작 지침의 반복을 제거했다. 셸 명령을 로컬 실행 계획으로 옮기고 Claude 구현 워커의 빌드 경로를 공통 규칙에 맞췄다.
- **Decisions:** 공통 검증·부하·데이터 보존 규칙은 `AGENTS.md`, 기존 Claude 호출 방식은 `CLAUDE.md`에 둔다. 명시적 사용자·상위 세션 지시가 우선한다.
- **Blockers:** 변경된 지침의 새 Claude 세션 자동 로딩과 실제 위임은 미검증이다.
- 실제 위임 시 해당 세션의 지침과 사용 가능한 도구를 확인한다.

## 미디어 검증 경계

- H.265는 로컬 LiveKit에서 Chrome·Rust 수신, Rust 재구독 복구, fixture Room 반복 종료를 확인했다. 실제 Fluxer 채널·별도 기기·Rust UI 영상 표시·화면 오디오·E2EE와 장시간 성능은 미검증이다.
- 창 버튼 표시와 클릭을 구분해 기록한다. WebView2 OOM 재발 방지는 검증되지 않았다.

## 작업 재개 시 확인

- 프로젝트 지침에 GPUI 렌더 재발 방지, 테스트 부하 상한, 전용 테스트 환경과 미공개 유지 규칙을 기록했다. 기능별 실기 미확인 목록은 보관본의 마지막 인계 항목에서 확인한다.

## 완료 기록 보관

- `docs/archive/PROGRESS-2026-11-10_to_2026-11-21.local.md`에 기존 기록 전체를 원본 그대로 보관했다. 초기 셸 이주, H.265 파일럿, GPUI 마일스톤 1~19와 지침 변경 기록을 포함한다.
- 보관본은 이 작업 공간에 저장되며 Git에서 제외된다. 개발 세션에서는 현재 상태에 필요한 기록만 읽는다.
