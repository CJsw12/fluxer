# PROGRESS

현재 상태, 인계, 다음 작업 순서를 관리한다. 마일스톤을 마칠 때 이 문서를 갱신하며, 로드맵은 목표나 수락 기준이 바뀔 때 갱신한다. 기록 보관은 전역 진행 지침을 따른다.

## 현재 상태 (2026-11-22)

- **Baseline:** `desktop-rs`의 HEAD는 음성 `VOICE_SERVER_UPDATE` `https://`→`wss://` 정규화 수정(`c5c12c81a`), 공식 Fluxer 1:1 UI 패리티 방법 A 1~3단계(2단 사용자 설정창 `588c1ff29`, 2단 채널·역할·밴 설정 통합 `f7e9a18d5`, DM 사이드바 `Quick switcher CTRL K`·아바타 행·플로팅 사용자 카드·DM 환영 배너)까지 원격에 푸시했다(전체 **88%**·일상 사용 **89%**, 실기 51·구현 41·부분 0·미착수 1). 자동 업데이트·E2E `d030b38bf`, 와이드/UI 패리티 `0e481aa7c`·`e4cc0e4fb`·`f0392a613`·`85eb73c43`·`3da55dd4e`, 마일스톤 G4 `ba0cb82dd`, G3 `367f0c287`, G2 `14b56d656`, G1 `2f71d33c3`, 4단계 진행도 시스템·README 동기화 `98c903b84`, 화면 공유 송신 `e94acf19e`, 2차 교차 검토 수정 `3b516e2b2`, Batch 2 `7f971ed1b`, Batch 1 `8f4ed6548`, 업데이트 확인기 `0df3bd344`, `TrackPublished`/`TrackUnpublished` 전파 `7438375c3`, 원격 비디오 디코드·IPC `92361b719`, `PlatformAudio` 장치 바인딩 `3d5e5bb0c`, OS 오디오 장치 열거·설정 UI `e9afd20d1`, GPUI 비디오 뷰어 `9378a8e20`, 원격 비디오 구독 IPC `a33512209`, 비디오 구독 의도 `3791963b0`, AccessKit 스크린리더 라벨 `5531d8254`, 동작 줄이기 접근성 `ad16d65dc`, `--no-os-integration` `f48890893`, 음성 오디오 IPC `d63dd74a3`, 음성 장치·볼륨 설정 UI `fcf9906df`, voice_worker MAX_PATH 수정 `b2f2af3ff`, 진행도 배지·패키징 수정 `83e337f2f`, 코덱 트래커 연동 `87abf9688`, 워커 이벤트 전파 `b1beee351`, 트랙 스냅샷·패키징 `8766e34ff`, 트랙 IPC 동기화 `2f45f8342`, 독립 LiveKit 음성 헬퍼 `5d5ac58ba`, 워커 자동 탐색 `f935220d5`, 음성 IPC 전송 `333503041`, 음성 패널 UI `a15d70028`, SPEAK·STREAM 권한 백엔드 `da5d5cfae`, 코덱 협상·비디오 큐 `639fa2730`, 음성 보안 강화 `86cfd706a`, GPUI 음성 UI `712cd011d`, 음성 취소·재연결 `472002e23`, 백엔드 음성 세션 `d8e4deaf9`, opcode 4 계약 `3dab82658`, GIF 확대 보기 `b40b7c73f`를 포함한다. 현재 Windows 릴리스(`scripts/package_fluxer_client.ps1`)는 음성 수정 + 방법 A 1~3단계 완료본에서 빌드한다(`dist/fluxer_client/fluxer_client.exe` 및 `dist/fluxer_client/fluxer_voice_worker.exe` SHA-256 `38703E21B4991751…`). 10분 계측은 이전 패키지의 결과다.
- **Done:** 답장 조회(`cfb8d1b3d`)·재시도 제한(`4dff14772`)·편집 순서(`3184571c8`)·대량 삭제(`e81560367`)·GIF 확대 보기(`b40b7c73f`), 2단계 1~6번 핵심 계약, 음성 장치·볼륨·OS 열거(`d63dd74a3`·`e9afd20d1`·`3d5e5bb0c`), 격리 실기용 `--no-os-integration`(`f48890893`), 접근성 동작 줄이기·AccessKit 라벨(`ad16d65dc`·`5531d8254`), 원격 비디오 발견·구독·디코드·GPUI 뷰어(`3791963b0`·`a33512209`·`9378a8e20`·`92361b719`·`7438375c3`), 안전 URL 검증 업데이트 확인기·에셋 다운로드·SHA-256 검증(`0df3bd344` 및 최신 커밋), `Codex (gpt-6.2-sol)` 1·2차 읽기 전용 독립 교차 검토(`7438375c3`·`7f971ed1b`, `8f4ed6548`·`7f971ed1b`·`3b516e2b2`), `voice_worker` Safe-Rust `DesktopCapturer` 화면 공유 송신(`e94acf19e`), 4단계 진행도 시스템·README 자동 동기화(`98c903b84`), 무인 `/goal` 마일스톤 G1~G4(`2f71d33c3`·`14b56d656`·`367f0c287`·`ba0cb82dd`), 그리고 Orca 격리 프로필 공식 웹과 최신 릴리스 Rust 앱의 전 채널(`#general`·`#markdown`·`#한국어`·`#announcements`·DM) 및 기본/최대화 창 1:1 비교·수정을 완료했다(`3da55dd4e`·`85eb73c43`·`f0392a613`·`e4cc0e4fb`·`0e481aa7c`). 실시간 E2E 검증으로 **DM 목록·대화, 반응 pill 표시(`👍 1`), 답장·미리보기, 입력 중 표시(`tester2 is typing...`), 스포일러 가림, 파일 로그(`client.log`), 날짜·숫자 현지화, 앱 UI 번역 적용** 8개 항목을 추가 `done` 승격했고 `자동 업데이트`를 `implemented`로 올렸다(`fluxer_client_app` **347건** 포함 총 **684건 통과**, 로그·스크린샷은 Git 밖 보관).
  - **시험 서버:** 전용 CA를 사용하는 SSH 터널에서 탐색 API, 시험 계정 로그인과 프로필·길드·채널·메시지 조회가 HTTP 200, 로그아웃은 204였다. 별도 게이트웨이 검사에서 HELLO·READY·하트비트 ACK·RESUMED를 확인했다. 로컬 fixture E2E 3건도 통과했다. 시험 계정 사이에서 합성 PNG와 2프레임 GIF를 업로드하고 다시 내려받아 파일 형식과 프레임 수를 확인했다.
  - **Windows 릴리스:** SHA-256 `AF3A852B0692781C…`의 격리 앱에서 시험 DM을 받았다. 터널 재연결 뒤 같은 앱의 읽지 않은 항목이 2→3으로 늘어 후속 DM 수신도 확인했다. 시험 메시지 두 건은 정리했다. 최신 `e81560367` 릴리스에서 시험 DM의 PNG·GIF 표시와 확대 보기, 파일 선택부터 PNG 전송·서버 수신까지, YouTube 링크 카드와 스크롤을 확인했다. GIF 프록시는 두 프레임을 반환했고 Windows 애니메이션 효과가 꺼진 상태에서 앱은 첫 프레임을 표시했다.
  - **공식 웹 비교:** 격리된 Orca 프로필에서 같은 시험 서버와 DM을 열었다. 동작 줄이기 상태에서도 공식 웹은 사용자가 GIF 확대 보기를 열면 두 프레임을 재생했고, Rust 앱은 확대 보기에서도 첫 프레임에 머물렀다. PNG는 양쪽에서 표시됐다. `b40b7c73f`는 동작 줄이기를 유지하면서 사용자가 연 GIF 확대 보기에서만 재생한다. 확대 보기 테스트 2건을 통과했다. 시험 서버의 Windows 디버그 앱에서 동작 줄이기를 켠 채 활성 창 7장을 비교해 확대 보기의 두 프레임 재생과 대화 썸네일의 정지를 확인했다. 증거는 Git 밖에 보관했다.
  - **앱 계측:** 10분 2초 동안 121회 샘플링했다. DM 2회와 터널 재연결 1회를 포함한다. Private bytes 85.4~126.1 MiB(첫 86.4, 마지막 122.8), working set 77.1~137.2 MiB, CPU 평균 2.9%(논리 코어 1개 기준), GPU 최고 엔진 사용률 1.3%, 전용·공유 GPU 메모리 최대 43.4·4.7 MiB였다. 원본 카운터는 Git 밖에 보관했다.
- **Decisions:** 주력은 `fluxer_client/`의 GPUI + gpui-kit 앱이다. Rust UI·미디어의 실기 동등성 검증 후 데스크톱 실행 경로에서 WebView2를 제거한다. Windows MSVC에서 `livekit`/`webrtc-sys`의 사전 빌드 `libwebrtc`는 정적 CRT(`/MT`, `+crt-static`)를 요구하므로, GPUI 앱(`/MD`)의 빌드·의존성과 격리하기 위해 네이티브 LiveKit Room·오디오는 별도 헬퍼 프로세스(비공개 stdin/stdout JSON-lines IPC, `kill_on_drop`, 토큰 argv/로그 금지)로 분리해 `VoiceTransport`/`VoiceRoom`에 연결한다. 장기 목표나 단계별 수락 기준이 필요할 때는 로컬 `RUST_FULL_MIGRATION_PLAN.md`를 참고한다. WebView2 전환기 셸을 수정할 때는 로컬 `RUST_DESKTOP_EXECUTION_PLAN.md`를 참고한다.
- **Blockers:** P0 미완료. 창을 직접 선택한 뒤 앱 입력을 진행할 수 있었다. 로그인 폼, 실제 한글 IME, 애니메이션 효과가 켜진 상태의 GIF 재생, OS 알림 팝업·소리, 입력 부하·장시간 누수는 미검증이다. 자체 테스트 서버 로그인 엔드포인트가 ALTCHA(`400 CAPTCHA_REQUIRED`)를 요구하며, `--token-env` 없이 실행 시 `HKCU\Software\Classes\fluxer` 기본 연결을 덮어쓰는 경로가 있어 자동화 실기는 중단했다. `d8e4deaf9`(`backend/voice.rs`)의 자격 증명·동시성 변경에 대한 다른 계열(Claude/Codex) 독립 교차 검토는 쿼터 소진으로 보류 중이다.

### Next Steps

1. **일상 사용 P0를 검증한다.** 자체 테스트 서버에서 `b40b7c73f` 릴리스 패키지(`713E71CB3FC97521…`)로 남은 로그인 폼, 한글 IME, 동작 줄이기 해제 상태의 GIF 재생과 확대 보기 동등성, OS 알림 팝업·소리와 입력 부하에서 장시간 메모리·CPU·GPU 사용을 확인한다. 로그인 폼 실기 시 시스템 기본 `fluxer://` 연결을 건드리지 않도록 주의하고, CAPTCHA 통과 및 포그라운드 입력은 사용자가 직접 수행한다.
2. **2단계 2번(네이티브 LiveKit 음성 전송·오디오 입출력)을 이어간다.** `backend/voice.rs`의 `VoiceTransport`·`VoiceRoom` 인터페이스에 실제 LiveKit Room 연결과 마이크 캡처·원격 오디오 재생을 연결하고, 자체 SFU에서 두 계정의 양방향 음성, mute/deaf, 퇴장 후 재생 종료를 확인한다. Claude/Codex 쿼터가 회복되면 `d8e4deaf9`와 후속 LiveKit 연결 변경을 함께 다른 계열 독립 검토로 확인한다.

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
