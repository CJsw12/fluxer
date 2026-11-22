# PROGRESS

현재 상태, 인계, 다음 작업 순서를 관리한다. 마일스톤을 마칠 때 이 문서를 갱신하며, 로드맵은 목표나 수락 기준이 바뀔 때 갱신한다. 기록 보관은 전역 진행 지침을 따른다.

## 현재 상태 (2026-11-22)

- **Baseline:** `desktop-rs`에서 공식 Fluxer 1:1 UI 패리티 로드맵(`docs/UI_PARITY_ROADMAP.md`) **M1.1**(검색 결과 임베드·이미지·아바타·마크다운 복구, `fluxer_client_app` **350건 통과**)까지 완료했다. 기존 기준선은 음성 `VOICE_SERVER_UPDATE` `https://`→`wss://` 정규화 수정(`c5c12c81a`), 공식 Fluxer 1:1 UI 패리티 방법 A 1~3단계(2단 사용자 설정창 `588c1ff29`, 2단 채널·역할·밴 설정 통합 `f7e9a18d5`, 메인 셸 마감 `e08ba4815`), 자동 업데이트·E2E `d030b38bf`, 마일스톤 G1~G4(`2f71d33c3`~`ba0cb82dd`), 4단계 진행도 시스템(`98c903b84`), 화면 공유 송신(`e94acf19e`), `Codex (gpt-6.2-sol)` 1·2차 교차 검토 수정(`8f4ed6548`·`7f971ed1b`·`3b516e2b2`)을 포함한다(전체 **88%**·일상 사용 **89%**, 실기 51·구현 41·부분 0·미착수 1). 현재 Windows 릴리스(`scripts/package_fluxer_client.ps1`)는 `e08ba4815`에서 빌드했다: `dist/fluxer_client/fluxer_client.exe`(45,046,272바이트, SHA-256 `81AACC6BBC59B081…`, 릴리스 경고 0건) 및 `dist/fluxer_client/fluxer_voice_worker.exe`(30,737,920바이트, SHA-256 `38703E21B4991751…`).
- **Done:** 답장 조회(`cfb8d1b3d`)·재시도 제한(`4dff14772`)·편집 순서(`3184571c8`)·대량 삭제(`e81560367`)·GIF 확대 보기(`b40b7c73f`), 2단계 1~6번 핵심 계약, 음성 장치·볼륨·OS 열거(`d63dd74a3`·`e9afd20d1`·`3d5e5bb0c`), 격리 실기용 `--no-os-integration`(`f48890893`), 접근성 동작 줄이기·AccessKit 라벨(`ad16d65dc`·`5531d8254`), 원격 비디오 발견·구독·디코드·GPUI 뷰어(`3791963b0`·`a33512209`·`9378a8e20`·`92361b719`·`7438375c3`), 안전 URL 검증 업데이트 확인기·에셋 다운로드·SHA-256 검증(`0df3bd344`·`d030b38bf`), `Codex (gpt-6.2-sol)` 1·2차 읽기 전용 독립 교차 검토(`7438375c3`·`7f971ed1b`, `8f4ed6548`·`7f971ed1b`·`3b516e2b2`), `voice_worker` Safe-Rust `DesktopCapturer` 화면 공유 송신(`e94acf19e`), 4단계 진행도 시스템·README 자동 동기화(`98c903b84`), 무인 `/goal` 마일스톤 G1~G4(`2f71d33c3`·`14b56d656`·`367f0c287`·`ba0cb82dd`), Orca 공식 웹↔릴리스 Rust 앱 실시간 E2E 검증(`d030b38bf`, 8개 항목 추가 `done` 승격), 음성 `https://`→`wss://` 엔드포인트 정규화(`c5c12c81a`), 공용 2단 설정 셸·사용자/채널/역할/밴 설정 2단 개편·메인 셸 1차 마감(`588c1ff29`·`f7e9a18d5`·`e08ba4815`), **M1.1** 검색 결과 `SearchHit`의 임베드·이미지 첨부·아바타·마크다운 보존 및 `ui::embed` 재사용 + `tests_search.rs`·`search_panel::tests` 검증(`fluxer_client_app` **350건 통과**)을 완료했다.
  - **시험 서버:** 전용 CA를 사용하는 SSH 터널에서 탐색 API, 시험 계정 로그인과 프로필·길드·채널·메시지 조회가 HTTP 200, 로그아웃은 204였다. 별도 게이트웨이 검사에서 HELLO·READY·하트비트 ACK·RESUMED를 확인했다. 로컬 fixture E2E 3건도 통과했다. 시험 계정 사이에서 합성 PNG와 2프레임 GIF를 업로드하고 다시 내려받아 파일 형식과 프레임 수를 확인했다.
  - **Windows 릴리스:** SHA-256 `AF3A852B0692781C…`의 격리 앱에서 시험 DM을 받았다. 터널 재연결 뒤 같은 앱의 읽지 않은 항목이 2→3으로 늘어 후속 DM 수신도 확인했다. 시험 메시지 두 건은 정리했다. 최신 `e81560367` 릴리스에서 시험 DM의 PNG·GIF 표시와 확대 보기, 파일 선택부터 PNG 전송·서버 수신까지, YouTube 링크 카드와 스크롤을 확인했다. GIF 프록시는 두 프레임을 반환했고 Windows 애니메이션 효과가 꺼진 상태에서 앱은 첫 프레임을 표시했다.
  - **공식 웹 비교:** 격리된 Orca 프로필에서 같은 시험 서버와 DM을 열었다. 동작 줄이기 상태에서도 공식 웹은 사용자가 GIF 확대 보기를 열면 두 프레임을 재생했고, Rust 앱은 확대 보기에서도 첫 프레임에 머물렀다. PNG는 양쪽에서 표시됐다. `b40b7c73f`는 동작 줄이기를 유지하면서 사용자가 연 GIF 확대 보기에서만 재생한다. 확대 보기 테스트 2건을 통과했다. 시험 서버의 Windows 디버그 앱에서 동작 줄이기를 켠 채 활성 창 7장을 비교해 확대 보기의 두 프레임 재생과 대화 썸네일의 정지를 확인했다. 증거는 Git 밖에 보관했다.
  - **앱 계측:** 10분 2초 동안 121회 샘플링했다. DM 2회와 터널 재연결 1회를 포함한다. Private bytes 85.4~126.1 MiB(첫 86.4, 마지막 122.8), working set 77.1~137.2 MiB, CPU 평균 2.9%(논리 코어 1개 기준), GPU 최고 엔진 사용률 1.3%, 전용·공유 GPU 메모리 최대 43.4·4.7 MiB였다. 원본 카운터는 Git 밖에 보관했다.
- **Decisions:** 주력은 `fluxer_client/`의 GPUI + gpui-kit 앱이다. Rust UI·미디어의 실기 동등성 검증 후 데스크톱 실행 경로에서 WebView2를 제거한다. Windows MSVC에서 `livekit`/`webrtc-sys`의 사전 빌드 `libwebrtc`는 정적 CRT(`/MT`, `+crt-static`)를 요구하므로, GPUI 앱(`/MD`)의 빌드·의존성과 격리하기 위해 네이티브 LiveKit Room·오디오는 별도 헬퍼 프로세스(비공개 stdin/stdout JSON-lines IPC, `kill_on_drop`, 토큰 argv/로그 금지)로 분리해 `VoiceTransport`/`VoiceRoom`에 연결한다. **현재 모든 에이전트(Codex·Claude·Antigravity)의 유일한 활성 UI·검증 계획서는 `docs/UI_PARITY_ROADMAP.md`이며, 기존 `RUST_FULL_MIGRATION_PLAN.md`·`RUST_DESKTOP_EXECUTION_PLAN.md`는 `docs/archive/`로 보관했다.**
- **Blockers / Known UI Gaps:**
  - **UI 패리티 구조 결함 (`docs/UI_PARITY_ROADMAP.md`로 일괄 해결 예정):** `Search messages`(`ui/search_panel.rs`) 결과에서 임베드·링크 미리보기·아바타 누락, DM·설정·음성·검색 곳곳의 프로필 사진 누락(공용 `ui::avatar` 부재), `Lucide`→공식 `Phosphor Icons` 불일치, 음성 채널 접속 시 중앙 스테이지(`VoiceCallView`) 미구현, 기본/최대화 창 비율 차이.
  - **사람 개입 필요 실기 항목:** ALTCHA 로그인 폼, 실제 한글 IME 조합 입력, 실기기 양방향 음성·화면 공유 체감 확인(`c5c12c81a` 수정본 실기 재확인 필요), OS 알림 팝업·소리, 장시간 입력 부하 계측, 하드웨어 보안 키 기반 `패스키·보안 키`.

### Next Steps (새 세션 진입 시 순서대로 실행)

> **활성 로드맵:** `docs/UI_PARITY_ROADMAP.md` (구 `RUST_*_PLAN.md`·`NATIVE_MIGRATION_PLAN.md`·`NEXT_BRIEFS.md`는 보관본이므로 따르지 않는다.)

1. **M1 — 공용 렌더러 강제 재사용 & 에셋 통일 (`docs/UI_PARITY_ROADMAP.md#m1`):**
   - `M1.1` 완료: `ui/search_panel.rs`의 `SearchHit`가 `embeds`·`images`·`avatar_url`·`user_id`·`document`를 보존하고 `ui::embed`·이미지 미리보기·아바타를 렌더링하도록 수정 + `tests_search.rs` 테스트 통과.
   - `M1.2`: 공식 `StatusAwareAvatar.tsx` 기준 공용 `ui::avatar` 신설 후 메시지·멤버·DM 행·DM 배너·사용자 패널·설정·검색·음성·프로필 팝아웃 9곳 일괄 교체.
   - `M1.3`: `@phosphor-icons/core` 원본 SVG를 `fluxer_client/app/assets/icons/`에 임베드하고 `icons.rs`를 Phosphor 아이콘으로 교체.
2. **M2 — 공식 치수 동기화 & 미구현 핵심 화면 (`docs/UI_PARITY_ROADMAP.md#m2`):**
   - `M2.1`: `fluxer_app` `MessageLayoutSpec.ts`·`*.module.css` 실측치를 `theme.rs` `ThemeSizes`에 1:1 매핑.
   - `M2.2`: 음성 채널 접속 시 중앙 스테이지(`ui/voice_stage.rs`, 참가자 타일 그리드·발화 링·화면 공유 뷰어·하단 통화 컨트롤) 구현 및 `tests_voice.rs` 검증.
   - `M2.3`: 설정창 테마 스와치·슬라이더 및 타이틀바 정합성 마감.
3. **M3 — 오프라인 12-시나리오 시각 갤러리 & 네이티브 창 자동 대조 (`docs/UI_PARITY_ROADMAP.md#m3`):**
   - `M3.1`: 로그인·CAPTCHA 없이 12개 화면 상태를 기본(`1280×800`)/최대화(`1920×1040`) PNG로 일괄 출력하는 오프라인 시각 갤러리 하네스 구축.
   - `M3.2`: 공식 네이티브 창(`fluxer_desktop_rs` 또는 격리 `fluxer_desktop`)과 동일 해상도 자동 diff로 잔여 UI 차이를 일괄 정리하고 실기 검증 완료 항목을 `docs/progress/progress.json` `done`으로 승격.

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
