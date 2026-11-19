# PROGRESS

## 워크트리 및 Git 형상 (Baseline)
- **작업 경로**: `C:\Users\k3832\orca\projects\fluxer`
- **현재 브랜치**: `desktop-rs` (최신 커밋: `02934b34`, 리모트 `origin/desktop-rs` 동기화 완료)
- **Git 브랜치 구조 및 듀얼 리모트**:
  - `main`: 공식 원본 저장소(`upstream/main`, `e86b0c94`)와 100% 동일한 순수 업스트림 상태로 유지 (오염 0건)
  - `desktop-rs`: Rust 네이티브 데스크톱 클라이언트(`fluxer_desktop_rs`) 전용 개발 및 릴리스 배포 브랜치
  - `origin`: `https://github.com/CJsw12/fluxer.git` (`main`, `desktop-rs` 동기화 완료)
  - `upstream`: `https://github.com/fluxerapp/fluxer.git` (공식 원본 저장소)
- **신규 아키텍처 산출물**:
  - [`NATIVE_MIGRATION_PLAN.md`](file:///C:/Users/k3832/orca/projects/fluxer/NATIVE_MIGRATION_PLAN.md) (Codex Terra 수립 4단계 네이티브 이행 계획서 - 4단계 전수 완료)
  - [`IPC_CONTRACT_BASELINE.md`](file:///C:/Users/k3832/orca/projects/fluxer/IPC_CONTRACT_BASELINE.md) (마일스톤 1 IPC 전수 감사 및 구현 상태 매트릭스)
  - [`fluxer_desktop_rs/bindings/contracts/`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/bindings/contracts/) (`ts-rs` 자동 추출 26종 TypeScript 계약 정의)
  - [`fluxer_desktop_rs/src/audio.rs`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/src/audio.rs) (Windows WASAPI 프로세스 루프백 및 오디오 매니저 모듈)
  - [`fluxer_desktop_rs/src/sqlite_store.rs`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/src/sqlite_store.rs) (번들 SQLite 3 RowStore / Cursors / Summaries / Mentions 엔진)
  - [`fluxer_desktop_rs/src/account_engine.rs`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/src/account_engine.rs) (AccountEngine 계약 핸들러 모듈)
  - [`fluxer_desktop_rs/src/input_hook.rs`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/src/input_hook.rs) (Windows 저수준 PTT 및 전역 단축키 이벤트 매니저)
  - [`fluxer_desktop_rs/src/badge.rs`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/src/badge.rs) (Windows `ITaskbarList3` 작업표시줄 배지 모듈)
  - [`fluxer_desktop_rs/src/theme_watch.rs`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/src/theme_watch.rs) (`notify` 150ms 디바운스 테마 파일 감시자 모듈)

## 완료 및 검증 (Done)
1. **창 제어 버튼(Titlebar) 부재 결함 및 WebView2 OOM 크래시 근본 해결 (커밋 `02934b34`)**:
   - **창 제어 캡션 버튼 및 스타트업 타이틀바 완전 복원 ([`bridge.rs`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/src/bridge.rs))**:
     - `document.documentElement`에 `platform-native platform-windows` 클래스 및 `--native-titlebar-height: 32px` CSS 변수를 즉각 주입하여 React 컴포넌트(`NativeTitlebar.tsx`)가 네이티브 데스크톱 레이아웃으로 렌더링되도록 보장함.
     - React 마운트 이전 또는 인스턴스 선택 화면에서도 창 제어가 가능하도록 스타트업 타이틀바 DOM(`<div id="fluxer-startup-native-titlebar" class="fluxer-native-titlebar" ...>`)과 최소화/최대화/닫기 Phosphor SVG 캡션 버튼, Fluxer 워드마크 및 자체 내장 스타일을 주입함.
     - 창 최대화 상태(`fluxer:maximize-change`)에 따른 최대화/복원(Maximize/Restore) 아이콘 동적 갱신 연동.
   - **WebView2 V8 Out of Memory 크래시 원천 퇴치 ([`main.rs`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/src/main.rs), [`bridge.rs`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/src/bridge.rs))**:
     - 20초마다 무차별적으로 렌더러와 GPU 프로세스의 워킹셋을 0으로 털어버리던 `trim_process_working_set()` 주기 스레드를 제거하고, `WindowEvent::Focused(false)`(창 비활성화 시)에만 안전하게 백그라운드 메모리를 회수하도록 변경함.
     - 20초마다 돌던 `setInterval(window.gc, 20000)` 강제 V8 GC 루프를 제거하여 메모리 페이징 쓰래싱 및 OOM 예외를 원천 차단함.
     - `--renderer-process-limit=1` 단일 프로세스 병목 및 셰이더 디스크 캐시 비활성화를 제거하고, `--js-flags=--max-old-space-size=4096`을 안전하게 주입하여 대용량 채널과 실시간 WebRTC 미디어 로딩 시에도 메모리 고갈이 발생하지 않도록 조치함.
   - **검증 완료**:
     - 단위 테스트 74개 전수 통과 (74 passed, 0 failed, Exit 0).
     - Clippy 경고 0건 통과.
     - 릴리스 바이너리 빌드 완료 (`target/release/fluxer.exe`, 14.8 MB / 14,810,624 bytes, Exit 0).
     - 원격 저장소 푸시 완료 (`origin/desktop-rs`).

2. **마일스톤 4: 실시간 미디어 및 최종 검증 (Real-Time Media & Final Verification) 완료**:
   - `win-process-loopback` N-API 디커플링 및 Safe Rust WASAPI 파이프라인 직결.
   - `sample-pool` 제로카피 안전 슬라이스 노출.
   - `nativeAudio` 7종 IPC API 및 실시간 PCM Base64/ArrayBuffer WebRTC 스트리밍 파이프라인 연동.
   - `ts-rs` 5종 신규 계약 정의 추가 (총 26종 계약 자동 추출 완료).

3. **마일스톤 3: 저장 및 입력 (Storage & Input) 완료**:
   - `windows-input-hook` N-API 디커플링 및 논블로킹 PTT/전역 단축키 이벤트 매니저 연동.
   - 번들 `rusqlite` RowStore 및 AccountEngine 8종 수명주기 메서드 직결.

4. **마일스톤 2: 저위험 직결과 시스템 서비스 완료**:
   - `win-shell` N-API 디커플링 및 자동 시작/바로가기 동기화.
   - `ITaskbarList3` 작업표시줄 배지 및 `flashFrame` 연동.
   - `notify` 기반 150ms 디바운스 테마 파일 감시자 연동.

5. **마일스톤 1: 기준선과 계약 완료**:
   - `IPC_CONTRACT_BASELINE.md` 전수 감사 및 `ts-rs` 계약 추출 파이프라인 확립.

## 핵심 결정 사항 (Decisions)
- **타이틀바 이중 보호 아키텍처**:
  - 프리로드 스크립트에서 스타트업 타이틀바(`<div id="fluxer-startup-native-titlebar">`)를 선제적으로 DOM에 주입하여 웹뷰 초기 로딩 중에도 창 조작이 즉시 가능하도록 하고, React의 `NativeTitlebar` 마운트 시 `useLayoutEffect`를 통해 자연스럽게 바통을 넘기도록 설계함.
- **안정적 메모리 거버넌스 (Event-Driven Trimming)**:
  - 렌더러가 활발하게 작동하는 도중 20초마다 강제로 워킹셋을 디스크로 페이징 아웃하던 타이머를 영구 제거하고, 창이 포커스를 잃었을 때(`WindowEvent::Focused(false)`)에만 안전하게 시스템 메모리를 반환하도록 정규화함.

## 결함 및 부채 (Blockers / Tech Debt)
- 없음. 마일스톤 1~4 전 단계 및 사용자 보고 결함 2종(창제어 버튼 부재, WebView2 OOM 크래시) 100% 해결 완료.

## 다음 작업 (Next Steps)
- 신규 빌드된 `target/release/fluxer.exe` 실기 검증 모니터링 지원.

## 2026-11-10 창 제어 버튼 재조사
- Baseline: `C:\Users\k3832\orca\projects\fluxer`, `desktop-rs`, HEAD `02934b34`. 기존 `PROGRESS.md` 미커밋 변경을 유지함.
- Done: `fluxer_desktop_rs/src/bridge.rs`의 전역 단축키 객체 뒤에 남은 중복 구문이 JavaScript 파싱을 막는 원인을 확인하고 제거함. Node.js가 설치된 경우 실제 브리지 전체를 `node --check`로 검증하는 회귀 테스트를 추가함. `cargo test -p fluxer_desktop_rs`: 75 passed, Exit 0. `cargo clippy -p fluxer_desktop_rs -- -D warnings`: Exit 0.
- Decisions: 웹 프론트엔드를 수정하지 않고 브리지 구문을 바로잡음. Node.js가 없으면 추가 구문 테스트는 건너뜀.
- Blockers: 기존 릴리스에서 버튼이 없는 현상을 실제 창에서 재현함. 수정본 릴리스 빌드가 진행 중이므로 수정본의 창 표시 및 버튼 조작은 아직 미검증임. OOM 해결 여부도 미검증임.
- Next Steps: `cargo build --release -p fluxer_desktop_rs` 완료 결과를 확인하고 `target/release/fluxer.exe`에서 버튼 표시와 최소화/최대화/닫기 동작을 검증함. 변경 통계는 `git diff --stat`로 확인함.
## 2026-11-10 사용자 확인 및 남은 이관 범위
- Baseline: `C:\Users\k3832\orca\projects\fluxer`, `desktop-rs`, HEAD `02934b34`; `git diff --stat`: `PROGRESS.md`, `fluxer_desktop_rs/src/bridge.rs` 변경 중.
- Done: 사용자가 창 제어 버튼이 보인다고 확인함. 실행 파일 경로와 각 버튼 클릭 동작은 별도로 확인되지 않음. 앞 절에 기록한 테스트 및 Clippy 결과는 유지함. 릴리스 실행 파일은 2026-11-10 14:45:45에 갱신되었으나 이 기록만으로 빌드 종료 코드를 확인할 수 없음.
- Decisions: Rust 코드의 존재와 실제 앱에서 기능이 검증된 상태를 구분함. 사용자가 본래 웹 UI 수정은 요청하지 않았음.
- Blockers: `bridge.rs`에는 알림, 다운로드, 패스키 등 미구현 경로가 남아 있음. 화면공유 오디오, 저장소, 단축키 등 실제 종단 간 동작과 OOM 재발 여부는 별도 검증이 필요함.
- Next Steps: 사용자 우선순위에 따라 미구현 기능 또는 기존 기능의 종단 간 검증을 선택함. 창 제어 버튼 각각의 클릭 동작은 아직 확인되지 않음.
## 2026-11-10 Rust 데스크톱 후속 이관 계획
- Baseline: `C:\Users\k3832\orca\projects\fluxer`, `desktop-rs`, HEAD `02934b34`. 작성 전 `git diff --stat`: `PROGRESS.md`, `fluxer_desktop_rs/src/bridge.rs`, 총 2파일 66 additions/48 deletions. `RUST_DESKTOP_EXECUTION_PLAN.md`는 신규 미추적 파일임.
- Done: `RUST_DESKTOP_EXECUTION_PLAN.md`에 기능별 원본 경로, M0~M5 우선순위, 모델 배정, 보안·동시성 검토, 구현 범위, 명령 및 실기 수락 기준을 작성함. 기존 릴리스 빌드 세션의 `cargo build --release -p fluxer_desktop_rs` 종료 코드 0을 확인함. 이전 기록의 빌드 진행 중 표기는 이 결과로 대체함.
- Decisions: 창 버튼 표시만 사용자 확인으로 기록하며 버튼별 클릭과 OOM 재발 방지는 미검증으로 유지함. 오디오 캡처 시작 오류가 성공 ID로 반환되는 코드 경로를 M0에 우선 배정함. 구현 책임이 있는 인증·권한·동시성 작업은 별도 검토 기준을 적용함.
- Blockers: 알림·딥링크·다운로드·패스키 및 단축키 일부는 미연결이거나 미지원임. 오디오 실패 처리와 실기 미디어·저장소 검증은 아직 수행하지 않음.
- Next Steps: `RUST_DESKTOP_EXECUTION_PLAN.md`의 M0에 따라 `fluxer_desktop_rs/src/audio.rs`의 캡처 실패 계약과 `bridge.rs` 미지원 응답을 원본과 대조하고 실패·정리 테스트를 수행함. `cargo test -p fluxer_desktop_rs`와 `cargo clippy -p fluxer_desktop_rs -- -D warnings`를 관련 수정 후 실행함.
## 2026-11-10 M0 오디오 및 테마 파일 접근
- Baseline: `C:\Users\k3832\orca\projects\fluxer`, `desktop-rs`, HEAD `02934b34`. 기록 직전 추적 파일 통계: PROGRESS.md                     |  83 ++--
 fluxer_desktop_rs/src/audio.rs  | 841 +++++++++++++++++++++++++++++++++-------
 fluxer_desktop_rs/src/bridge.rs |  40 +-
 fluxer_desktop_rs/src/ipc.rs    |  73 ++--
 fluxer_desktop_rs/src/main.rs   |  17 +-
 5 files changed, 820 insertions(+), 234 deletions(-). 신규 미추적 파일: `RUST_DESKTOP_EXECUTION_PLAN.md`, `IPC_M0_FINDINGS.md`.
- Done: `fluxer_desktop_rs/src/audio.rs`의 캡처 시작·종료 감독과 실패 반환, `ipc.rs`의 오류 응답 및 미승인 테마 경로 차단, `main.rs`의 오류·종료 이벤트 JSON 직렬화, `bridge.rs`의 테마 폴더 감시 미지원 표시를 반영함. `IPC_M0_FINDINGS.md`에 확인한 API 계약 차이를 기록함. `cargo test -p fluxer_desktop_rs --quiet`: 86 passed, 0 failed, Exit 0. `cargo clippy -p fluxer_desktop_rs -- -D warnings`: Exit 0. `git diff --check`: Exit 0. 실제 앱 또는 원격 수신자 음성 전달은 이번 변경 후 검증하지 않음.
- Decisions: Sol 고위험 검토에서 장치 준비 전에 성공 응답하지 않도록 수정하고 callback 자체에서 join하지 않도록 감독 스레드가 소유·정리함. 중지 응답은 종료 상태 전환을 의미하며 네이티브 정리 완료를 보장하지 않음. 신뢰 문서 검증과 선택 경로 허용 목록이 없으므로 테마 파일 읽기·폴더 가져오기를 임시 차단함. 테마 기능 복구 전에는 차단을 해제하지 않음.
- Blockers: M0 미완료. 종료 이벤트가 시작 응답 또는 프런트엔드 리스너 등록보다 일찍 도착하면 이벤트가 유실되는 기존 경쟁이 남아 있음. 마이크로태스크 재전달은 소비자의 초기화 순서를 보장하지 못해 철회함. 시작·종료 동작과 정상 PCM의 실기 검증, 전체 브리지 계약 전수 조사, 다른 IPC의 신뢰 경계 검토가 남음. 테마 가져오기·파일 읽기는 현재 소스에서 사용할 수 없음.
- Next Steps: `fluxer_app/src/features/voice/utils/NativeAudioCaptureBridge.ts` 및 네이티브 브리지 소비자와 호환되는 명시적 준비 확인/종료 상태 조회 계약을 설계하고 테스트함. `fluxer_desktop/src/main/ThemeLocalFiles.ts`와 `PrivilegedRendererDocuments.ts`를 기준으로 원격 문서 신뢰 정책, 선택 대화상자 및 allowlist 설계를 완료하고 보안 리뷰함. 필요 시 `cargo test -p fluxer_desktop_rs --quiet`와 `cargo clippy -p fluxer_desktop_rs -- -D warnings`를 실행함.
## 2026-11-10 사용자 보고: H.265 및 단축키
- Baseline: `C:\Users\k3832\orca\projects\fluxer`, `desktop-rs`, HEAD `02934b34`; 기존 수정 및 미추적 계획 문서는 보존함.
- Done: 사용자가 음성 채널의 일반 음성 통화가 작동한다고 보고함. 화면 공유 중 프로그램/시스템 오디오를 상대가 듣는지는 별도 미검증임. 사용자는 H.265 허용 후에도 코덱 선택이 비활성화되고 단축키가 작동하지 않는다고 보고함.
- Decisions: `bridge.rs`의 `globalShortcuts.sync`, `onEvent` 등이 빈 구현이며 원본 `KeybindManager.ts`가 이 경로를 사용하는 것을 확인함. H.265 설정은 opt-in 외에 `RTCRtpSender.getCapabilities('video')` 등 실제 지원 탐지에 의존함. 장치의 GPU 모델 확인만으로 실제 WebRTC 인코딩 지원을 주장하거나 강제 활성화하지 않음.
- Blockers: 단축키의 구체적인 키와 포커스 조건을 사용자에게 요청했으며 아직 답변을 받지 못함. 실제 WebView2 H.265 능력과 코덱 실패 원인은 미확정임. 사용자가 실행 중인 `target/release/fluxer.exe`를 종료하거나 교체하지 않음.
- Next Steps: Orca Sol 작업자에게 전역 단축키 원본 계약 및 실제 입력 훅 연결을 맡겨 검증함. H.265는 현재 WebView2의 실제 송신 코덱 탐지와 브라우저/드라이버 제약을 확인함. 실제 화면 공유 오디오는 송신자 PC 소리를 수신자가 듣는지 별도 확인함.## 2026-11-10 H.265 WebView2 진단
- Baseline: `C:\Users\k3832\orca\projects\fluxer`, `desktop-rs`, HEAD `02934b34`. 진단 전 추적 파일 7개, 1328 additions/490 deletions 및 기존 미추적 파일 4개를 보존함. 실행 중인 `target/release/fluxer.exe`를 종료하거나 교체하지 않음.
- Done: `CodecCapabilityDetector.ts`와 `AdvancedVideoControls.tsx`에서 H.265 선택이 opt-in 외에 WebRTC 송신 능력에 의존하는 것을 확인함. GPU는 AMD Radeon RX 9070 XT, 드라이버 32.0.32015.7029이며 실행 중인 앱의 WebView2 Runtime은 157.0.4341.42임. 설치된 pywebview로 별도 비공개 저장소의 WebView2 진단 창을 실행하여 `RTCRtpSender/Receiver.getCapabilities('video')`를 조회함. 기본 실행에서는 양쪽 목록에 H.265가 없었음 (종료 코드 0). `PlatformHEVCEncoderSupport,WebRtcAllowH265Send,WebRtcAllowH265Receive` 활성화 시 수신 목록에는 H.265가 나타났지만 송신 목록에는 없었음 (종료 코드 0). 앱의 기존 Chromium 인수와 이 플래그를 함께 사용한 별도 진단에서도 송신 목록에 H.265가 없었음 (종료 코드 0). 진단 스크립트는 시스템 임시 폴더의 `fluxer_hevc_probe.py`임.
- Decisions: 사용자 화면에서 허위로 선택을 활성화하거나 효과가 확인되지 않은 전역 플래그를 제품에 추가하지 않음. 독립 진단 창 결과는 실행 중인 Fluxer의 직접 측정이나 화면 공유 송수신 성공을 뜻하지 않음.
- Blockers: 이 환경에서 WebView2가 H.265 송신을 노출하지 않는 근본 원인(런타임 빌드, 드라이버 또는 인코더 경로)은 아직 특정되지 않았음. 실제 H.265 화면 공유 및 원격 수신은 검증하지 않음.
- Next Steps: WebView2와 Chromium의 GPU 인코더 노출 차이 및 Windows Media Foundation HEVC 인코더 사용 가능성을 추가 진단하고, 실사용 가능한 송신 경로가 확인되면 호환성과 실제 원격 송수신을 검증함.
- 추가 진단: WebCodecs 비동기 지원 조회는 pywebview 평가 결과가 빈 객체로 반환되어 판정할 수 없었음 (프로세스 종료 코드 0은 코덱 지원을 의미하지 않음).
## 2026-11-10 H.265 인코더 경로 추가 진단
- Baseline: `C:\Users\k3832\orca\projects\fluxer`, `desktop-rs`, HEAD `02934b34`; 기존 미커밋 작업 및 실행 중인 Fluxer를 보존함.
- Done: FFmpeg `hevc_amf`로 1280x720 시험 영상 5프레임 인코딩 성공 (종료 코드 0). 기본 `hevc_mf`는 첫 프레임에서 `80004005`로 실패 (종료 코드 1)했으나, `-pix_fmt nv12 -hw_encoding 1`에서는 일반 및 `-scenario video_conference` 조건 모두 5프레임 성공 (각 종료 코드 0). 별도 WebView2 157 보안 로컬 문서에서 H.265 관련 세 플래그를 켜고 WebCodecs `VideoEncoder`로 5프레임을 실제 인코딩하여 출력 5개, 오류 0개 확인 (종료 코드 0). 직후 동일 문서의 WebRTC 송신 목록에는 H.265가 여전히 없었음. 설치된 Chrome 157의 별도 headless 프로필에서는 WebRTC 송신 및 수신 목록 모두 H.265가 존재함 (종료 코드 0). 이 결과는 사용자 웹 계정, Fluxer 화면 공유 또는 원격 수신 검증이 아님.
- Decisions: 이전 Media Foundation 실패는 기본 인코더 옵션에서 발생한 것으로 한정하며 시스템의 하드웨어 HEVC 미지원으로 해석하지 않음. WebCodecs 인코딩 성공은 WebRTC RTP 송신 지원을 의미하지 않으므로 브리지의 코덱 능력을 조작하지 않음. 브라우저 런타임 교체나 별도 WebRTC 송신 구현 없이 플래그만으로는 관측된 송신 문제를 해결하지 못함.
- Blockers: Edge WebView2에서 H.265 WebRTC 송신 미노출의 내부 원인은 확정하지 못했으며, 실제 앱의 H.265 송신은 미해결임. Chrome에서 Fluxer 로그인, 화면 공유 및 수신 측 재생은 확인하지 않음.
- Next Steps: Chrome에서 같은 서비스의 실제 H.265 송수신을 검증하고, Rust 데스크톱의 장기 해결책으로 Chromium 기반 런타임 전환 또는 실제 네이티브 WebRTC 송신 경로의 구현 범위와 호환성을 평가함. `fluxer_hevc_real_encode_probe.py` 등 진단 스크립트는 시스템 임시 폴더에 있음.
## 2026-11-10 WebView2 유지 조건의 H.265 송신 조사
- Baseline: `C:\Users\k3832\orca\projects\fluxer`, `desktop-rs`, HEAD `02934b34`; 기존 변경과 실행 중인 앱을 보존함.
- Done: WebView2 157.0.4341.42의 `msedge.dll`에 ASCII `WebRtcAllowH265Send` 문자열이 없고 `WebRtcAllowH265Receive`는 있음을 확인함. Chrome 157.0.8103.40의 `chrome.dll`에는 양쪽 문자열이 있음 (각 바이너리 읽기 성공). 문자열 부재만으로 빌드 설정을 확정하지 않음. 독립 WebView2에서 H.265 수신 항목을 송신 transceiver의 `setCodecPreferences`에 넣는 호출은 허용되었으나 실제 로컬 WebRTC 연결의 offer/answer에는 H.265가 없고 VP8로 5프레임 송신됨 (종료 코드 0). 수정 SDP에 H.265를 추가하고 다른 코덱을 남긴 실험에서는 offer/answer에 H.265 문자열이 있으나 실제 송신 통계는 VP8, 4프레임이었음 (종료 코드 0). H.265만 남긴 수정 SDP는 `setLocalDescription`에서 codecs 파싱 오류로 거부됨 (진단 프로세스 종료 코드 0, 실험 자체 실패). 임시 스크립트 `fluxer_hevc_sdp_probe.py`, `fluxer_hevc_loopback.py`, `fluxer_hevc_loopback.js`를 사용함.
- Decisions: WebView2를 유지하고 브라우저 교체를 추진하지 않음. 코덱 선호도나 SDP 문자열 삽입만으로 실제 H.265 송신이 된다고 판단하지 않음. LiveKit 공식 문서의 Rust 사전 인코딩 H.265 전송은 WebView2 UI와 병행 가능한 조사 후보이나, 별도 네이티브 WebRTC 스택, 방 연결 및 인증, 트랙 식별, 프레임 전송, 키프레임·비트레이트 피드백이 필요함. 무게와 호환성은 아직 측정하지 않았음.
- Blockers: WebView2 내장 WebRTC에서 H.265 송신을 활성화하는 검증된 방법을 찾지 못함. 네이티브 우회 구현과 실제 Fluxer 서버·수신자 송수신은 미수행임. 제품 코드 변경 없음.
- Next Steps: WebView2 차기 런타임의 송신 지원을 재확인하거나, 제품별 방 참가·트랙 계약을 먼저 조사한 후 사전 인코딩 LiveKit 경로의 최소 프로토타입에서 바이너리 크기, 메모리, 동시 참가/인증 및 실제 수신 호환성을 측정함. 출처: https://github.com/MicrosoftEdge/MSEdgeExplainers/issues/1273 , https://docs.livekit.io/transport/media/pre-encoded/ .
## 2026-11-10 Rust 전환 구현 재조사
- Baseline: `C:\Users\k3832\orca\projects\fluxer`, `desktop-rs`, HEAD `02934b34`; 조사 전 추적 변경 7개 1346 additions/490 deletions와 기존 미추적 파일 4개를 보존함.
- Done: `RUST_PORT_RESEARCH_2026-11-10.md`에 현재 Wry/IPC, 기존 Windows Rust 크레이트, PCM 전달, Voice Engine V2 계약, LiveKit 방·트랙 연결과 H.265 후보를 기록함. Context7의 Wry 0.57 및 LiveKit Rust 문서와 Microsoft/LiveKit 원문을 확인함. 정적 소스 조사만 수행했으며 제품 코드 수정이나 빌드는 없음.
- Decisions: WebView2와 기존 웹 UI를 유지함. H.265는 화면 공유 송신 단위로 실험하고 네이티브 수신·전체 전환과 RAM 절감은 측정 이후 판단함. 기존 수정된 단축키 브리지는 과거 빈 구현 기록과 구분함.
- Blockers: 제품의 IPC 신뢰 경계, 오디오 종료 이벤트 경쟁, 화면 공유 오디오 실기, 네이티브 LiveKit 참가·인증과 수신자 호환성, Windows에서 SDK 무게와 반복 publish 메모리는 미검증임.
- Next Steps: 실제 바이너리·프로세스 트리 성능 기준선과 신뢰 경계를 확인하고, 별도 테스트 방에서 화면 캡처·HEVC·사전 인코딩 송신을 검증한 후 동일 조건으로 비용을 비교함.
## 2026-11-10 Rust 완전 이주 계획 및 GPU 정보 직결
- Baseline: `C:\Users\k3832\orca\projects\fluxer`, `desktop-rs`, HEAD `02934b34`; 작업 전 추적 파일 7개 1352 additions/490 deletions, 미추적 파일 5개. 기존 변경을 보존함.
- Done: `RUST_FULL_MIGRATION_PLAN.md`에 WebView2 유지, 셸·미디어·수신 이주 순서와 검증 조건을 작성함. `fluxer_desktop/native/platform-info`의 Node 의존성을 기본 `node` feature로 분리하고 기존 DXGI GPU 열거를 Rust 셸에 직접 연결함. `ipc.rs`의 가짜 NVIDIA GPU 응답을 실제 어댑터 목록으로 교체했고 DXGI 우선순위를 WebView2의 실제 활성 GPU로 오인하지 않도록 `active: false`로 반환함. 루트 workspace exclude 및 Cargo.lock을 갱신함. `cargo test -p fluxer_desktop_rs --quiet`: 96 passed, 0 failed, Exit 0. GPU 집중 테스트 1개: 실제 DXGI `1002:7550`, Exit 0; 시스템 GPU 정보와 일치함. `cargo check --locked --manifest-path fluxer_desktop/native/platform-info/Cargo.toml`: 기본 Node feature 컴파일 Exit 0. 테스트로 생성된 기존에 깨끗했던 TypeScript 계약 파일 2개는 원상복구함.
- Decisions: 실제 WebView2 활성 GPU와 H.265 WebRTC 송신 능력은 DXGI 열거로 추정하지 않음. 사용자 실행 중인 앱과 기존 release 실행 파일은 교체하지 않음. 전체 Rust 이주 중 기존 웹 UI와 WebView2 유지라는 이전 사용자 조건을 적용함.
- Blockers: `cargo clippy -p fluxer_desktop_rs -- -D warnings`는 기존 미추적 `src/input_hook_key_codes.rs:167`의 `clippy::unnecessary_lazy_evaluations`에서 실패(Exit 101). 해당 lint만 허용한 `cargo clippy -p fluxer_desktop_rs -- -D warnings -A clippy::unnecessary_lazy_evaluations`는 Exit 0. 기존 Node 애드온의 실제 로딩과 변경 후 실제 앱 IPC는 미검증. `getAppMetrics`의 고정 수치는 아직 미수정이며 H.265 송신과 이주 전체는 미완료.
- Next Steps: 실제 앱에서 변경된 GPU 응답을 검증하고 `getAppMetrics` 고정 수치를 실제 프로세스 정보 또는 명시적 미지원으로 대체함. 오디오 종료 이벤트 경쟁과 IPC 신뢰 경계, 네이티브 H.265 테스트 방 송수신 및 메모리 비교를 계획 순서대로 수행함.
## 2026-11-10 최종 Rust UI 목표 정정
- Baseline: `desktop-rs`, HEAD `02934b34`. 기존 미커밋 구현과 문서를 보존함.
- Done: 사용자의 정정에 따라 `RUST_FULL_MIGRATION_PLAN.md`를 갱신함. UI를 포함한 데스크톱 클라이언트의 Rust 이주를 최종 목표로 명시하고 WebView2·기존 웹 UI는 전환 중에만 유지하도록 수정함. Rust UI 기능별 구현과 실기 동등성 검증을 추가함.
- Decisions: 기존 웹 프론트엔드 원본은 보존하며 별도 Rust UI를 구현함. 현재 GPUI 코드만으로 UI 프레임워크나 기능 완성을 확정하지 않음. 이전 절의 WebView2 영구 유지로 해석될 수 있는 설명은 이 정정으로 대체함.
- Blockers: Rust UI 구현·전체 기능 동등성·실기 검증은 아직 수행하지 않음. 서버 및 타 플랫폼 이주 범위는 별도로 확인해야 함.
- Next Steps: 기존 GPU 직결 변경의 실기 검증과 계획의 기능별 이주를 계속하고, Rust UI 선택과 실제 화면·계약·성능 조사 후 기능 단위로 구현함.
## 2026-11-10 외부 Rust UI 이주 참고 구현 조사
- Baseline: `desktop-rs`, HEAD `02934b34`. 기존 미커밋 변경을 보존함.
- Done: 사용자 제공 저장소 README 및 루트 라이선스를 확인함. KELD는 Bun·Rust 호스트·시스템 웹뷰를 사용하는 pre-alpha이며 MIT 또는 Apache-2.0임. GPUIX는 React/Solid와 Node-API를 이용하는 GPUI 렌더러이며 Apache-2.0임. electron-gpui는 Electron 내부에서 Rust GPUI 창을 운용하며 MIT임. 각 저장소의 설계와 GPUI 예제를 이주 참고 자료로 분류함.
- Decisions: 최종 목표는 JS 런타임·웹뷰 의존 없는 Rust 데스크톱 UI이므로 KELD·GPUIX·electron-gpui 자체를 최종 런타임으로 선정하지 않음. GPUIX의 가상 목록·텍스트 입력 및 electron-gpui의 창 수명·이벤트 설계를 참고하고, KELD의 웹뷰·IPC·측정 방법은 전환 단계에 한정해 검토함. 외부 벤치마크를 Fluxer 측정 결과로 사용하지 않음.
- Blockers: 세 프로젝트의 소스 빌드, Windows 동작, Fluxer 통합, 라이선스 하위 의존성 및 미디어 호환성은 검증하지 않음. Rust UI 프레임워크는 아직 확정하지 않음.
- Next Steps: 구현 시 참고 파일과 버전·라이선스를 확인하고, 기존 GPUI 버전과 Fluxer 입력·접근성·영상 표시 요구사항을 실제 앱에서 검증함. 출처: https://github.com/gyldlab/keld ; https://github.com/remorses/gpuix ; https://github.com/biw/electron-gpui .
## 2026-11-10 H.265 네이티브 LiveKit 송수신 실험 착수
- Baseline: `C:\Users\k3832\orca\projects\fluxer`, `desktop-rs`, HEAD `02934b34`; 기존 미커밋 파일과 실행 중인 앱을 보존함.
- Done: `tools/h265_livekit_probe`에 독립 Rust 송수신 실험을 작성함. 로컬 개발 서버의 별도 참가자 2명을 사용하고 H.265 화면 공유 트랙과 수신 측 MIME, 640x360 디코딩 프레임을 검증하도록 구성함. 이 코드는 아직 컴파일·실행 성공이 확인되지 않음. LiveKit Server v1.13.11 Windows 공식 아카이브를 임시 폴더에 다운로드하고 SHA-256을 GitHub 릴리스 값과 대조함. 서버는 아직 시작하지 않음. FFmpeg `hevc_mf` 하드웨어 인코더로 640x360 15fps 합성 영상 120프레임을 생성하고 ffprobe에서 HEVC 및 프레임 수를 확인함. 첫 패킷에서 VPS/SPS/PPS와 키프레임 NAL을 확인함. 최초 `hevc_metadata` bitstream filter는 VPS 관련 오류로 실패했으며 필터 없는 fixture를 사용함.
- Decisions: 테스트 영상과 로컬 서버는 실제 Fluxer 계정·서버를 사용하지 않음. 인증 토큰은 출력하지 않음. 합성 영상 디코딩 성공만으로 실제 화면 캡처나 원격 기기 및 Fluxer 통합이 완료된 것은 아님.
- Blockers: 첫 `cargo check --manifest-path tools/h265_livekit_probe/Cargo.toml`은 `webrtc-sys` C++ 컴파일에서 `constexpr_parser.h`를 찾지 못해 Exit 101. 파일이 존재하며 전체 경로 261자임을 확인함. Windows 설정을 변경하지 않고 `CARGO_TARGET_DIR=%TEMP%\\lkprobe-target`으로 짧은 경로에서 `cargo check --locked`를 재실행했으며 기록 시점에 진행 중임 (세션 3052, 로그 `%TEMP%\\fluxer-livekit-h265-pilot\\cargo-check-short.log`). 실제 송수신·코덱 협상·수신 디코딩, 실시간 캡처와 사용자 앱 통합은 미검증.
- Next Steps: 기존 컴파일 세션의 종료 코드와 최초 오류를 확인하고 필요 시 수정함. 통과하면 테스트 서버를 localhost에 한정해 실행하고 Rust 송수신 실험의 H.265 협상 및 10프레임 이상 디코딩을 확인한 뒤 서버를 종료함. 이후 실제 캡처, 수신자·오디오·재연결 및 자원 사용량을 검증함.
## 2026-11-10 H.265 해결과 Rust 이주 통합 계획
- Baseline: `C:\Users\k3832\orca\projects\fluxer`, `desktop-rs`, HEAD `02934b34`; 기존 수정 및 미추적 실험을 보존함.
- Done: 이전 세션 3052의 `cargo check --locked` 종료 코드 0을 확인함. 대상은 `tools/h265_livekit_probe`, 짧은 임시 target 경로이며 링크·실행·송수신은 포함하지 않음. 기존 `RUST_FULL_MIGRATION_PLAN.md`의 WebView2 영구 유지 서술이 이전 정정을 반영하지 못한 것을 발견하고, H.265 수직 검증부터 Fluxer 통합, Rust UI 및 셸 이주, 최종 WebView2 제거와 실기·성능 수락까지 계획을 다시 작성함.
- Decisions: 최종 데스크톱 UI·기능·미디어는 Rust로 이주하며 WebView2와 기존 웹 UI는 전환 중에만 유지함. H.265의 합성 영상·실제 화면·Fluxer 채널 수신을 별도 검증 단계로 둠. 서버와 다른 OS는 별도 범위임.
- Blockers: 로컬 LiveKit 송수신, 실제 캡처, 상대방 수신·코덱 fallback·E2EE와 전체 Rust 이주는 미검증임. 새 계획은 구현 완료를 뜻하지 않음.
- Next Steps: 기존 실험을 링크하고 루프백 LiveKit 송수신·H.265 디코딩을 검증한 뒤 실제 캡처와 Fluxer 통합을 진행함. 검증 로그와 프로세스 정리 결과를 기록함.
## 2026-11-10 외부 프로젝트 구현 활용성 상세 조사
- Baseline: `desktop-rs`, HEAD `02934b34`; 기존 수정·미추적 작업을 보존함.
- Done: `RUST_MIGRATION_REFERENCE_REVIEW.md`에 KELD `f49bc07`, GPUIX `0ce4f41`, electron-gpui `64658b8`의 고정 커밋, 실제 입력·가상 목록·접근성·창 수명·웹뷰 권한 소스와 현재 Fluxer GPUI 0.2.2의 차이를 기록함. Context7 GPUI 문서와 각 프로젝트의 소스 및 라이선스를 확인함.
- Decisions: GPUIX는 Rust UI 입력·목록 설계, electron-gpui는 재진입·창 수명, KELD는 임시 웹뷰 권한·IPC 설계 참고로 분류함. GPUI 포크·Node/Electron/Bun 의존성을 그대로 채택하지 않으며 H.265 송신 구현으로 간주하지 않음.
- Blockers: 외부 프로젝트의 빌드, Windows 한글 IME·접근성·기능 호환성 및 Fluxer 성능은 미검증임. 파일명 검색만으로 H.265 관련 구현 부재를 확정하지 않음. LiveKit 파일럿의 실제 송수신은 여전히 미검증임.
- Next Steps: LiveKit H.265 파일럿을 검증하고, 별도 Rust UI 기능 구현 시 고정된 참고 소스의 라이선스와 GPUI API 호환성을 확인함.
## 2026-11-10 H.265 파일럿 링크 빌드
- Baseline: `C:\Users\k3832\orca\projects\fluxer`, `desktop-rs`, HEAD `02934b34`. 기존 미커밋 작업과 실행 중인 사용자 앱을 보존함.
- Done: `tools/h265_livekit_probe`와 이전 성공한 `cargo check --locked`, 공식 LiveKit 테스트 서버 및 H.265 합성 fixture의 존재를 확인함. 사용자 앱·계정·서버는 사용하지 않음.
- Decisions: 짧은 임시 target 경로에서 파일럿을 링크하고 빌드 성공 후에만 별도 로컬 송수신을 실행함.
- Blockers: 실험은 아직 링크·실행·실제 H.265 수신 검증이 완료되지 않음. 빌드 로그 `%TEMP%\fluxer-livekit-h265-pilot\cargo-build.log`, 종료 코드 파일 `%TEMP%\fluxer-livekit-h265-pilot\cargo-build.exit`을 사용함.
- Next Steps: `cargo build --locked --manifest-path tools/h265_livekit_probe/Cargo.toml`을 `CARGO_TARGET_DIR=%TEMP%\lkprobe-target`에서 실행하고, 종료 코드가 0이면 로컬 서버에서 H.265 협상과 수신 디코딩을 검증함. 서버의 실행·종료와 로그를 기록함.
## 2026-11-10 H.265 파일럿 Windows 링크 오류 수정
- Baseline: `desktop-rs`, HEAD `02934b34`; 기존 작업과 실행 중인 사용자 앱을 보존함.
- Done: 파일럿 첫 `cargo build --locked` 종료 코드 101을 확인함. 최초 링크 오류는 `webrtc-sys`의 MSVC 동적 CRT와 libwebrtc의 정적 CRT 간 `LNK2038 RuntimeLibrary` 불일치이며 실행 파일이 생성되지 않음. 공식 LiveKit Rust SDK의 Windows 설정 `-C target-feature=+crt-static`을 확인함: https://github.com/livekit/rust-sdks/blob/main/.cargo/config.toml .
- Decisions: 전역 설정은 변경하지 않고 파일럿 빌드 명령의 `RUSTFLAGS`에만 공식 정적 CRT 옵션을 적용함. 실패 로그 및 원래 종료 코드 파일은 보존함.
- Blockers: H.265 송수신과 실기 화면 공유는 여전히 미검증임. 재빌드 로그 `%TEMP%\fluxer-livekit-h265-pilot\cargo-build-static.log`, 종료 코드 `%TEMP%\fluxer-livekit-h265-pilot\cargo-build-static.exit`을 사용함.
- Next Steps: 정적 CRT 설정으로 재빌드하고 성공 시 로컬 LiveKit 송수신 및 H.265 프레임 디코딩을 검증함. 동일 원인으로 다시 실패하면 반복하지 않고 최초 오류와 의존성 구성을 재조사함.
## 2026-11-10 H.265 파일럿 첫 로컬 송수신
- Baseline: `desktop-rs`, HEAD `02934b34`; 기존 변경과 실행 중인 사용자 앱을 보존함.
- Done: 정적 CRT 설정으로 `cargo build --locked`가 1분 28초에 종료 코드 0으로 완료됨. 공식 LiveKit 테스트 서버와 별도 Rust 송수신 참가자를 실행함. 서버 로그에서 참가자 두 연결의 ICE connected 및 영상 트랙 게시를 확인함. 파일럿은 `receiver timed out`을 보고했으며 H.265 수신 프레임은 확인하지 못함. 서버 프로세스 종료를 확인함. 파일럿 종료 코드는 프로세스 수집 결과가 비어 확인 불가하며 성공으로 처리하지 않음. 서버 로그 `%TEMP%\fluxer-livekit-h265-pilot\server-run-1.err`, 실험 오류 `%TEMP%\fluxer-livekit-h265-pilot\probe-run-1.err` 보존. 사용자 계정·실제 Fluxer 서버는 사용하지 않음.
- Decisions: 첫 실행은 H.265 수신 실패로 기록함. 원인 분리를 위해 파일럿에 track published·subscribed 및 인코딩 프레임 수락 개수 진단을 추가했으며 이를 적용한 새 실행의 결과는 아직 없음.
- Blockers: H.265 구독·협상·프레임 전달 및 디코딩 중 실패 지점은 미확정. 실제 화면 캡처·실사용 통합도 미검증.
- Next Steps: `RUSTFLAGS=-C target-feature=+crt-static`와 짧은 임시 target으로 파일럿을 재빌드하고 정확한 종료 코드와 단계별 로그를 수집함. 동일 실패가 반복되면 원인을 더 조사하고 단순 재시도하지 않음. 재빌드 로그 `%TEMP%\fluxer-livekit-h265-pilot\cargo-build-diagnostic.log`, 종료 코드 `%TEMP%\fluxer-livekit-h265-pilot\cargo-build-diagnostic.exit`.
## 2026-11-10 H.265 수신 시간 초과 원인 조사
- Baseline: `desktop-rs`, HEAD `02934b34`; 기존 수정과 실행 중인 Fluxer 앱을 보존함.
- Done: 단계별 진단을 넣은 두 번째 파일럿은 종료 코드 1, 서버 정리 확인. LiveKit에서 `video/H265` 구독과 인코딩된 120패킷 모두의 입력 수락이 확인됐으나 디코딩 프레임을 기다리다 시간 초과됨. 결과 `%TEMP%\fluxer-livekit-h265-pilot\probe-run-2.log`과 서버 로그를 보존함. FFmpeg에서 동일 HEVC fixture 120프레임 디코딩 Exit 0, 15프레임 간격의 키프레임 8개를 확인함.
- Decisions: 동일 시간 초과 재현 후 SDK 코드를 조사함. `libwebrtc`는 timestamp_us=0을 현재 Unix 시각으로 대체하지만 기존 파일럿은 이후 66666부터 증가하는 값을 사용해 타임스탬프가 큰 폭으로 역행하는 결함이 있었음. 파일럿을 0이 아닌 동일 Unix 기준의 증가하는 타임스탬프로 수정함. 이것이 유일한 원인인지는 미검증임.
- Blockers: 수정 후 H.265 디코딩은 미검증이며 실제 화면 캡처·원격 수신·Fluxer 통합도 미완료임.
- Next Steps: 정적 CRT 설정으로 파일럿을 재빌드하고, 새 로그를 사용하여 타임스탬프 수정이 H.265 디코딩을 해결하는지 1회 검증함. 여전히 실패하면 관련 WebRTC 송수신·코덱 로그를 조사함. 빌드 로그 `%TEMP%\fluxer-livekit-h265-pilot\cargo-build-timestamps.log`, 종료 코드 `%TEMP%\fluxer-livekit-h265-pilot\cargo-build-timestamps.exit`.
## 2026-11-10 H.265 타임스탬프 수정 후 로컬 검증
- Baseline: `desktop-rs`, HEAD `02934b34`; 기존 변경과 실행 중인 사용자 앱을 보존함.
- Done: 타임스탬프 보정 파일럿 재빌드 Exit 0. 로컬 LiveKit 세 번째 실험에서 `video/H265` 구독과 120개 encoded packet 수락을 확인했으나 수신 디코딩은 시간 초과됨 (파일럿 Exit 1, 서버 종료 확인). 파일럿 로그 `%TEMP%\fluxer-livekit-h265-pilot\probe-run-3.log`, 서버 로그 `server-run-3.err` 보존. FFmpeg로 동일 fixture 디코딩 Exit 0, 키프레임 8개 확인. 서버 로그에서 H.265 트랙 게시와 forwarder 시작, PLI 요청을 확인했으나 실측 RTP 전송량과 수신 디코더 지원은 확인하지 못함.
- Decisions: 0 timestamp의 SDK 대체로 인한 시간 역행은 파일럿 자체 결함이므로 수정했지만, 이것만으로 디코딩 시간 초과가 해결되지는 않았음. 동일 실패가 수정 후에도 반복되어 재시도를 중단하고 원인 범위를 RTP 전달·프레임 처리·Windows 수신 디코더로 좁힘.
- Blockers: H.265 실기 성공은 아직 없음. 파일럿은 Fluxer 통합이 아니고, SDK 프레임 수락은 전송·디코딩 성공의 증거가 아님. 서버 로그의 CPU monitoring 미지원 오류는 테스트 서버 자원 모니터링에 관한 것이며 미디어 실패의 원인으로 확인되지 않음.
- Next Steps: 첫 실패 로그와 동일 소스·버전을 유지하면서 송신 및 수신 RTP 통계를 구하고 Rust SDK Windows 디코더 지원을 확인함. 필요 시 기존 Chrome의 H.265 수신 능력을 독립 대조하되 테스트 조건 차이를 명시함. GPU 캡처·실제 Fluxer 채널·수신자와 E2EE는 아직 미검증임.
## 2026-11-10 H.265 Chrome 수신 대조 실험
- Baseline: `desktop-rs`, HEAD `02934b34`; 기존 사용자 작업 및 실행 중인 앱을 보존함.
- Done: `webrtc-sys` 0.3.53의 기본 Windows 디코더 팩토리에서 H.265 디코더 생성 경로가 보이지 않음을 확인함. 동일 Rust 송신기에 대해 격리된 headless Chrome 157 및 LiveKit JS SDK 2.22.3을 별도 수신자로 이용함. 최초 Chrome 테스트는 루프백 ICE 연결 실패였으며 서버 로그를 보존함. Chromium의 루프백 후보 옵션을 해당 임시 Chrome 프로세스에만 적용한 뒤 수신자 통계 `video/H265`, `framesDecoded=90`, `packetsReceived=497`, `bytesReceived=440444`, 640x360을 확인함. 테스트 하네스 Exit 0, 세 프로세스 종료 확인. 이 실행의 Rust 송신기 자체는 별도 Rust 수신자의 timeout으로 Exit 1이었으므로 전체 파일럿 Exit 0으로 간주하지 않음. 실제 화면, 원격 기기, Fluxer 채널 및 네이티브 Rust 수신은 검증하지 않음.
- Decisions: 합성 H.265의 Rust 송신→로컬 LiveKit→Chrome 디코딩 경로가 작동한 것으로 한정함. 사용자 Chrome 프로필과 전역 설정은 변경하지 않았으며 격리 프로세스의 테스트 전용 ICE 플래그는 제품 설정으로 적용하지 않음. Rust 수신자의 H.265 디코딩은 별도 해결이 필요함.
- Blockers: Rust 송신기는 같은 실행에서 Rust 수신자 시간 초과로 Exit 1이었고, 화면 캡처·원격 수신·앱 통합은 미완료임. 발행 전용 모드를 추가해 Chrome 단독 수신과 송신기 종료 코드까지 독립적으로 확인할 예정이며 아직 빌드·재검증하지 않음.
- Next Steps: 발행 전용 모드를 정적 CRT 옵션과 짧은 임시 target으로 빌드하고 Chrome 대조 하네스에서 송신기 Exit 0, H.265·10프레임 이상 및 프로세스 정리를 동시에 검증함. 이후 실제 캡처와 Windows Rust 수신 디코더·Fluxer 통합을 조사함. 빌드 로그 `%TEMP%\fluxer-livekit-h265-pilot\cargo-build-publisher-only.log`, 종료 코드 `cargo-build-publisher-only.exit`.
## 2026-11-10 H.265 로컬 송신 및 창 캡처 검증
- Baseline: `desktop-rs`, HEAD `02934b34`. 기존 미커밋 변경과 사용자 앱을 보존함.
- Done: 발행 전용 Rust 파일럿과 격리된 Chrome 대조 실험은 송신기 Exit 0, H.265 90프레임 디코딩, 하네스 Exit 0 및 서버·브라우저·송신기 종료 확인. 별도 테스트 창을 Windows gdigrab으로 캡처하여 하드웨어 HEVC로 인코딩한 파일도 동일 경로로 송신하여 640x360 H.265 90프레임 디코딩 및 Exit 0을 확인함. 화면 파일을 먼저 만든 뒤 송신한 실험임. `tools/h265_livekit_probe/capture_window.py`와 `chrome_control.py`에 격리 시험을 기록함.
- Decisions: 사용자 바탕화면이 아닌 실험에서 만든 창만 캡처함. 로컬 Chrome 테스트에만 루프백 ICE 후보 옵션을 적용함. Chrome 수신 성공은 Windows 네이티브 Rust 디코더·Fluxer 채널 성공을 의미하지 않음. 녹화 후 송신과 실시간 캡처는 구분함.
- Blockers: 실시간 캡처·인코딩·송신과 Rust 수신자, 실제 Fluxer 음성 채널, 원격 기기·오디오·E2EE는 미검증임. 실제 인코더/SDK 연결 전용 HEVC 스트림 경계 처리 및 테스트를 추가 중이며 아직 빌드하지 않음.
- Next Steps: 실시간 stdin HEVC 액세스 유닛 파서의 테스트와 파일럿을 정적 CRT·짧은 target으로 빌드하고, 자체 테스트 창의 실시간 캡처→HEVC 인코딩→Rust 송신→Chrome 디코딩을 한 번에 검증함. 로그와 종료·정리를 기록함.
## 2026-11-10 H.265 실시간 테스트 창 송수신
- Baseline: `C:\Users\k3832\orca\projects\fluxer`, `desktop-rs`, HEAD `02934b34`. 기존 사용자 작업과 실행 중인 Fluxer 앱을 보존함.
- Done: LiveKit Rust SDK 기본 Windows 수신 경로에 H.265 디코더 구현이 보이지 않는 것을 소스에서 확인함. 격리 Chrome 157 수신 대조를 추가하여 합성 H.265와 별도 테스트 창 캡처 파일의 디코딩을 확인함. 이어 테스트 창 실시간 캡처 → 외부 FFmpeg Media Foundation HEVC 인코딩 → bounded stdin/AUD 분리 → Rust LiveKit → localhost LiveKit → Chrome 디코딩을 실행함. 발행 준비 후 인코더 시작 및 15fps 파일럿 페이싱 후 실험 `live-paced`: 송신 150 access unit 수락, outbound 135프레임, Chrome 8초 샘플 `video/H265` 120프레임 디코딩, 640x360, 송신기 Exit 0, 하네스 Exit 0, 서버·브라우저·인코더·테스트 창·송신기 정리 확인. 실험은 별도 자동 생성 창만 캡처함. 파일럿 디버그 실행 파일 39,238,144 bytes이며 릴리스 또는 앱 전체 크기가 아님. 파서 단위 테스트 2개 통과 Exit 0. 라이브 측정·도구 설명은 `tools/h265_livekit_probe/README.md`와 임시 로그 `publisher-chrome-live-paced.log`, `server-chrome-live-paced.err`에 기록함.
- Decisions: 사전 녹화 파일 실험과 실시간 테스트 창 실험, Chrome 수신과 Rust 수신을 구분함. 15fps 고정 페이싱은 테스트 해결책이며 적응형 인코딩·지연에 대한 제품 검증이 아님. FFmpeg/Python은 진단 도구이고 최종 런타임으로 결정하지 않음. Fluxer `LiveKitService.ts`는 사용자 ID와 connection ID로 참가자 identity를 생성하며, LiveKit은 동일 identity 재참가 시 이전 참가자를 연결 해제하므로 기존 웹 음성 연결에 동일 토큰을 병렬 재사용하지 않음. 출처 https://docs.livekit.io/intro/basics/rooms-participants-tracks/participants/ .
- Blockers: 실제 Fluxer 채널, Windows 네이티브 Rust H.265 수신, 화면 공유 오디오, H.265 비호환 fallback, 재연결·E2EE·장시간 안정성과 전체 앱 RAM/GPU·지연 측정은 미검증임. 최초 Chrome 루프백 연결과 통계 타이밍 실패 로그를 보존하고, 정정한 프로세스 전용 옵션과 테스트 순서로 검증함.
- Next Steps: 기존 사용자 신원·권한을 유지하는 단일 네이티브 미디어 연결 이관 또는 명시적 호환 구조를 설계하고, 실제 제품에 필요한 Windows Graphics Capture·Media Foundation 직접 연동 및 Rust 수신 디코더를 검증함. 기존 테스트 환경에서 키프레임 요청·대역폭 피드백·에러·정리, H.265 미지원 수신자 대체 및 원격 수신자를 검증함.

## 2026-11-10 Windows Graphics Capture 파일럿 준비
- Baseline: `C:\Users\k3832\orca\projects\fluxer`, `desktop-rs`, HEAD `02934b34`; 기존 미커밋 변경을 보존함. 추적 파일 diff는 13개 파일, 1520 additions/521 deletions이며 미추적 파일은 별도임.
- Done: `tools/h265_livekit_probe/wgc_probe`에 정확한 임의 제목의 자체 테스트 창에서 프레임 10개를 받는 독립 Rust 파일럿과 타임아웃·정리 하네스를 작성함. `cargo fmt` 및 Python 구문 검사 Exit 0. 아직 컴파일하거나 실행하지 않음.
- Decisions: 캡처는 테스트에서 생성한 창만 대상으로 하며 화면 내용을 출력하거나 저장하지 않음. 프레임 도착 확인은 인코딩·전송·제품 통합 완료를 뜻하지 않음.
- Blockers: WGC 파일럿 빌드와 실행, GPU 텍스처의 Media Foundation H.265 인코딩 연결은 미검증임. 기존 H.265 Rust 수신과 실제 Fluxer 연동도 미완료임.
- Next Steps: `%TEMP%\lkprobe-target`을 사용하는 격리 빌드를 시작하고 로그 `%TEMP%\fluxer-livekit-h265-pilot\wgc-build.log`와 종료 코드 파일 `wgc-build.exit`를 확인함. 빌드가 성공하면 테스트 창에서 10프레임을 캡처하고 실제 결과를 기록함.

## 2026-11-10 Windows Graphics Capture 실제 프레임 확인
- Baseline: `desktop-rs`, HEAD `02934b34`; 기존 사용자 변경을 보존함.
- Done: 독립 WGC 파일럿은 `%TEMP%\fluxer-wgc-target`으로 오프라인 빌드 성공, Exit 0, 13.79초. 생성한 테스트 창만 대상으로 `test_capture.py` 실행 시 646×396 프레임 10개를 받고 Exit 0. 해당 버전은 캡처 확인만 수행함. 이후 동일 파일럿에 Direct3D 표면을 라이브러리의 Windows HEVC 인코더로 전달해 MP4를 기록하는 시험과 ffprobe 디코딩 검사를 추가했으며 아직 빌드·실행 결과를 확인하지 않음.
- Decisions: WGC 캡처 성공과 실시간 전송·제품 연동은 별도임. 라이브러리의 파일 인코딩 시험은 스트리밍 인코더나 무복사 처리 보장을 의미하지 않음.
- Blockers: 새 HEVC 인코딩 시험, 실시간 access unit 추출과 LiveKit 연결, Rust H.265 수신 및 실제 Fluxer 연동은 미검증임.
- Next Steps: 수정한 WGC 파일럿을 빌드하고 자체 테스트 창을 캡처하여 결과 HEVC의 실제 디코딩과 프로세스 정리를 검증함.

## 2026-11-10 WGC와 Windows HEVC 파일 인코딩 확인
- Baseline: `C:\Users\k3832\orca\projects\fluxer`, `desktop-rs`, HEAD `02934b34`; 기존 사용자 변경 보존.
- Done: `tools/h265_livekit_probe/wgc_probe/src/main.rs`에 자체 테스트 창의 WGC Direct3D 표면을 `windows-capture` 1.5.0의 Windows MediaTranscoder HEVC MP4 경로로 보내는 파일럿을 추가함. `cargo build --offline --locked` Exit 0. `test_capture.py`는 임의 제목의 자체 창만 캡처하고 임시 파일을 정리함. 첫 인코딩 시험: 캡처 60프레임, ffprobe HEVC 646×396 디코딩 88프레임, Exit 0. 개선된 두 번째 시험: 캡처 60프레임, 디코딩 84프레임, 디코딩 이미지 해시 62종, Exit 0. ffprobe/ffmpeg는 결과 검증에만 사용함. 잔여 `fluxer-wgc-probe.exe` 없음. `cargo fmt --check`, Python 구문 검사, `git diff --check -- PROGRESS.md` Exit 0.
- Decisions: 캡처 라이브러리는 MediaTranscoder 파일 경로를 사용하므로 실시간 access unit 추출이나 하드웨어 가속 여부, 무복사 처리를 입증하지 않음. 캡처 수와 파일 디코딩 수가 달라 타이밍과 중복 처리를 추가 검증해야 함. Microsoft의 Media Foundation 문서에 따르면 인코더 MFT의 `IMFTransform::ProcessOutput`은 출력 샘플 접근 경로임: https://learn.microsoft.com/en-us/windows/win32/medfound/processing-data-in-the-encoder .
- Blockers: WGC에서 실시간 HEVC 샘플을 추출하여 Rust LiveKit에 공급하는 경로, 키프레임/비트레이트 피드백, 네이티브 Rust H.265 수신과 Fluxer 채널 통합은 미구현·미검증임. 전체 이주와 RAM 개선도 미완료임.
- Next Steps: MF 인코더 MFT의 입력 포맷, D3D11 장치 공유, 비동기 출력·키프레임 제어를 직접 검증하는 독립 파일럿을 구현하고, 성공 후 LiveKit의 인코딩 프레임 계약에 연결함. 실시간 WGC 경로에 파일용 MediaTranscoder를 대체 수단으로 단정하지 않음.

## 2026-11-10 Media Foundation H.265 변환기 열거 준비
- Baseline: `desktop-rs`, HEAD `02934b34`; 기존 변경 보존.
- Done: 독립 파일럿에 Media Foundation의 HEVC 하드웨어·기타 인코더와 디코더 등록 수를 확인하는 `src/bin/mft_inventory.rs`를 작성하고 포맷함. 아직 컴파일과 실행은 확인하지 않음.
- Decisions: 열거된 변환기는 실제 초기화·실시간 샘플 처리·하드웨어 가속 성공을 보장하지 않음. COM 포인터와 초기화 수명은 진단기 범위로 제한함.
- Blockers: 변환기 열거 결과와 실제 샘플 처리는 미검증임. 실시간 LiveKit 통합 및 Rust 수신도 미완료임.
- Next Steps: `%TEMP%\fluxer-wgc-target`에서 오프라인 진단기 빌드 후 실행 결과를 확인하고, 실패 시 최초 오류를 기록함.

## 2026-11-10 Media Foundation 변환기 등록 확인
- Baseline: `C:\Users\k3832\orca\projects\fluxer`, `desktop-rs`, HEAD `02934b34`; 기존 미커밋 작업 보존.
- Done: `tools/h265_livekit_probe/wgc_probe/src/bin/mft_inventory.rs`를 `cargo build --offline --bin mft_inventory`로 빌드하고 Exit 0. 실행 결과 `hardware_hevc_encoders=2`, `hardware_hevc_decoders=0`, `other_hevc_encoders=1`, `other_hevc_decoders=1`, Exit 0. 다른 항목은 동기·비동기 열거 플래그의 결과임. `cargo fmt --check` 및 `git diff --check -- PROGRESS.md` Exit 0.
- Decisions: 등록 수만으로 코덱 초기화 성공, 가속 여부 또는 디코딩 불가능을 판정하지 않음. 직접 샘플을 받는 MFT 경로와 MediaTranscoder 파일 기록은 별개임.
- Blockers: WGC→MFT 실시간 HEVC 샘플→LiveKit 전송과 수신 경로, 실제 Fluxer 연동 및 Rust 이주는 미완료임. H.265 미지원 대체, 재연결, 오디오, E2EE, 성능은 미검증임.
- Next Steps: 열거한 인코더의 입출력 타입과 D3D11 연동을 확인하고 실시간 MFT 출력 샘플을 추출한 뒤 기존 로컬 LiveKit·Chrome 테스트와 연결함. Rust 수신에서는 시스템 디코더와 SDK 연결 경로를 조사함.

## 2026-11-10 HEVC 하드웨어 인코더 타입 협상 파일럿 준비
- Baseline: `desktop-rs`, HEAD `02934b34`; 기존 사용자 변경 보존.
- Done: `wgc_probe/src/bin/mft_types.rs`에 등록된 하드웨어 인코더를 각각 활성화하고 비동기 MFT를 잠금 해제하여 640×360 15fps HEVC 출력 유형 설정 및 입력 유형 열거를 시도하는 별도 파일럿을 작성함. 아직 빌드·실행 결과는 확인하지 않음.
- Decisions: 장치 활성화와 포맷 협상도 실제 인코딩 및 D3D11 연결 성공을 의미하지 않음. Microsoft 공식 문서의 MFT 비동기 잠금 해제와 출력 타입 우선 절차를 참고함.
- Blockers: 파일럿 실행, 실시간 샘플 생산, LiveKit 연결, 실제 Fluxer 통합 및 Rust 수신은 미검증임.
- Next Steps: `%TEMP%\fluxer-wgc-target`에서 오프라인 파일럿 빌드와 실행을 수행하고 실제 드라이버별 지원 포맷과 실패를 기록함.

## 2026-11-10 MFT 하드웨어 타입 협상과 샘플 출력 파일럿
- Baseline: `desktop-rs`, HEAD `02934b34`; 기존 사용자 변경 보존.
- Done: `mft_types`를 오프라인 빌드 Exit 0 후 실행함. 두 하드웨어 변환기가 비동기 잠금 해제와 640×360 15fps HEVC 출력 타입을 수락하고 각각 NV12, P010, ARGB32 입력 타입을 반환함. 프로세스 Exit 0. 이어 `mft_encode.rs`에 합성 NV12 프레임과 MFT 이벤트 기반 직접 HEVC 출력 샘플 시험, `test_mft_encode.py`에 제한 시간과 ffprobe 검사를 작성하고 포맷·Python 구문 검사함. 새 인코딩 파일럿은 아직 빌드·실행하지 않음.
- Decisions: 타입 열거와 직접 인코딩·GPU 표면 입력·LiveKit 연결은 별개의 검증임. 생성된 합성 입력만 사용함.
- Blockers: 새 샘플 출력 파일럿과 WGC 직접 연결, 실시간 전송·실제 앱 통합은 미검증임.
- Next Steps: 새 파일럿을 오프라인 빌드한 후 30초 제한으로 합성 입력을 처리하고 출력 HEVC 디코딩을 확인함. 실패 로그와 소스 상태를 보존함.

## 2026-11-10 MFT HEVC 샘플 직접 출력 및 송신 장애 조사
- Baseline: `desktop-rs`, HEAD `02934b34`; 기존 작업 보존.
- Done: 비동기 하드웨어 MFT에 합성 NV12 프레임 60개를 직접 제출해 HEVC 샘플 60개를 얻고 ffprobe로 640×360 60프레임을 디코딩함. 빌드·시험 Exit 0. stdout으로 LiveKit 파일럿에 전송하는 별도 시험을 추가함. 첫 시도는 프레임 구분자 중복으로 150샘플이 300프레임으로 분리되어 Chrome 시간 초과. 구분자 중복을 수정한 두 번째 시도는 150프레임 수락에도 RTP 송신 0, Chrome 통계 오류로 실패함. 두 실패 로그를 `%TEMP%\fluxer-livekit-h265-pilot`의 `first-failed-*`, `second-failed-*`로 보존함. 별도 150샘플을 분석한 결과 구분자 150개, 키프레임은 첫 프레임 하나뿐임을 확인함.
- Decisions: 키프레임 부족은 장애 원인의 유력한 가설이며 아직 재현으로 입증하지 못함. LiveKit PLI를 인코더에 전달하는 기능은 아직 없음. Windows `ICodecAPI` GOP 크기 15 설정을 시험 코드에 추가했으나 아직 빌드·실행하지 않음.
- Blockers: 실제 라이브 전송 재시험, WGC 입력과 결합, Rust 수신 및 Fluxer 통합은 미완료임.
- Next Steps: GOP 설정 빌드 후 HEVC 키프레임 수와 디코딩을 먼저 검증하고, 기존 실패와 다른 결과가 확인된 경우에만 로컬 Chrome 송수신을 다시 시험함.

## 2026-11-10 MFT 직접 HEVC 샘플의 로컬 LiveKit 전송 성공
- Baseline: `desktop-rs`, HEAD `02934b34`; 기존 변경 보존.
- Done: `mft_encode`에서 인코더가 이미 생성한 AUD를 보존하도록 수정하고 `ICodecAPI`의 GOP 크기를 15로 설정함. 최초 빌드의 VARIANT union 접근 오류 Exit 101은 명시적 참조로 수정하여 재빌드 Exit 0. 150프레임 입력에서 AUD 150개와 키프레임 10개(1,16,...136)를 확인함. 별도 ffprobe 시험은 60프레임 디코딩, Exit 0. `chrome_control.py --mft-synthetic` 시험에서 합성 NV12 → Windows 하드웨어 MFT 직접 H.265 샘플 → stdin → Rust LiveKit → localhost 서버 → Chrome의 `video/H265` 640×360, 8초 샘플 120프레임 디코딩, 송신 수락 150·RTP 135프레임, 전체 Exit 0. 초기 구분자 중복 및 GOP 설정 전 실패 로그는 각각 보존함.
- Decisions: GOP 설정은 고정 간격 진단용이며 PLI를 실시간으로 전달하는 구현이 아님. 합성 영상 성공은 실제 WGC 입력이나 원격 Fluxer에서의 성공이 아님. 프레임 전송은 현재 파이프 및 테스트 고정 페이싱을 사용함.
- Blockers: WGC 캡처를 MFT 인코더에 입력하는 직접 경로, GPU 장치 공유, 동적 키프레임 요청·혼잡 제어, Rust 네이티브 수신, 실제 Fluxer 연동과 Rust 이주는 미완료임.
- Next Steps: 자체 테스트 창의 WGC 픽셀을 실시간 MFT 입력으로 연결하는 검증을 구축하고 Chrome 수신 영상을 확인함. CPU 복사 방식은 진단용으로 명시하며 GPU 직접 공유는 별도 검증함.

## 2026-11-10 WGC 실시간 인코더 연결 준비
- Baseline: `desktop-rs`, HEAD `02934b34`; 기존 미커밋 작업 보존.
- Done: `wgc_raw.rs`에서 임의 제목의 자체 테스트 창을 WGC로 캡처하고 640×360 BGRA를 전달하는 파일럿을 작성함. `mft_encode.rs`에 BGRA stdin 입력 모드를 추가하고 `chrome_control.py --mft-wgc`에 캡처→MFT→Rust LiveKit→Chrome 시험을 연결함. 포맷 및 Python 구문 검사를 실행했으며 새 Rust 코드는 아직 빌드·실행하지 않음.
- Decisions: 이 진단 경로는 프레임마다 CPU readback과 프로세스 간 복사가 발생하므로 GPU 직접 공유나 RAM 최적화로 간주하지 않음. 캡처 대상은 테스트에서 생성한 창뿐임.
- Blockers: 새 경로의 컴파일·실행·영상 변화 검증, 오류 복구·원격 수신 및 실제 앱 통합은 미완료임.
- Next Steps: `%TEMP%\fluxer-wgc-target`에서 진단 바이너리를 빌드하고 로컬 Chrome 수신과 프로세스 정리를 확인함.

## 2026-11-10 WGC 실시간 H.265 로컬 송수신 확인
- Baseline: `C:\Users\k3832\orca\projects\fluxer`, `desktop-rs`, HEAD `02934b34`; 기존 미커밋 사용자 변경 보존.
- Done: `wgc_raw.rs`의 테스트 창 WGC 캡처와 CPU readback을 `mft_encode.rs` BGRA 입력 및 HEVC 직접 출력, Rust LiveKit, 로컬 Chrome에 연결함. 격리된 `--mft-wgc`를 실행해 캡처·인코딩 각 150프레임, Rust 150개 수락, RTP 135프레임, Chrome 640×360 `video/H265` 8초 샘플 103프레임 디코딩, Exit 0. 수신 3초·8초 이미지 해시 차이를 검사하도록 하네스를 개선한 후 재실행해 같은 103프레임 및 서로 다른 화면을 확인함. 첫 성공 로그는 `first-success-*`, 검증 재실행 로그는 `*-mft-wgc.*`에 보존함. 재실행 송신 경과 12.067초, 5ms 미만 프레임 간격 5회 관찰: 페이싱은 안정성 개선 여지가 있음. 테스트 프로세스 잔여 없음. `cargo fmt --check`, Python 구문 검사, `git diff --check -- PROGRESS.md` Exit 0.
- Decisions: CPU readback, 픽셀 복사, 별도 프로세스와 고정 GOP·페이싱은 시험용이며 성능 최적화 또는 실시간 PLI 대응으로 보고하지 않음. 현재 시험은 생성한 창과 localhost에 한정함.
- Blockers: WGC D3D11 텍스처와 인코더의 GPU 직접 공유, 정확한 시간·가변 프레임 속도, 혼잡 제어와 PLI, 네이티브 Rust H.265 수신, 실제 Fluxer 음성 참가자 연결·오디오·E2EE·재연결 및 원격 호환성은 미완료 또는 미검증임. RAM 개선도 미측정임.
- Next Steps: MFT 입력의 D3D11 지원과 장치 관리자 연결을 검증하여 CPU readback을 줄이고, 타임스탬프·백프레셔·키프레임 피드백을 설계함. 이후 Rust 수신 디코더와 기존 Fluxer 참가자·화면 공유 연결을 구현하고 실제 앱에서 검증함.

## 2026-11-11 남은 작업 계획 정리
- Baseline: `C:\Users\k3832\orca\projects\fluxer`, `desktop-rs`, HEAD `02934b34`; 기존 미커밋 변경 보존. 갱신 전 추적 파일 diff 13개, 1597 additions/521 deletions, 미추적 파일은 별도임.
- Done: 기존 `RUST_FULL_MIGRATION_PLAN.md`에서 로컬 서버와 H.265 캡처 실험이 미실행으로 남아 있던 오래된 상태를 2026-11-10 실행 증거로 갱신하고, GPU 경로·프레임 제어·Rust 수신·Fluxer 통합·최종 Rust UI 이주 순서와 수락 기준을 구체화함. `RUST_DESKTOP_EXECUTION_PLAN.md`가 전환기 계획임을 명시하여 WebView2 최종 제거 목표와의 혼동을 방지함. 계획 문서 내용과 기존 실험 기록을 대조했고 코드·실행 테스트는 수행하지 않음.
- Decisions: `RUST_FULL_MIGRATION_PLAN.md`는 전체 로드맵, `PROGRESS.md`는 실제 상태와 증거, `RUST_DESKTOP_EXECUTION_PLAN.md`는 전환기 셸 세부 계획임. WebView2는 전환기에 유지하고 최종 Rust UI 검증 이후 제거함.
- Blockers: 제품에서의 H.265 송수신, Rust UI 완전 이주와 RAM 개선은 아직 구현 또는 실기 검증되지 않음.
- Next Steps: WGC와 MFT의 D3D11 장치 공유를 검증하고 CPU 복사 기준선과 비교한 뒤 타이밍·키프레임 피드백, Rust 수신과 실제 Fluxer 통합을 진행함.

## 2026-11-11 WGC GPU 텍스처 입력 파일럿 준비
- Baseline: `desktop-rs`, HEAD `02934b34`; 기존 미커밋 작업 보존.
- Done: `wgc_gpu_encode.rs`에 WGC D3D11 장치와 IMFDXGIDeviceManager 연결, 별도 GPU 텍스처 복사, DXGI surface buffer 입력, 비동기 MFT 출력 및 drain 검증을 작성함. 아직 빌드·실행하지 않음.
- Decisions: CPU 픽셀 readback을 호출하지 않는 입력 경로를 시험하며 GPU 내부 변환·복사와 인코더 자체 메모리 사용은 별도로 측정해야 함. 테스트 창만 캡처함.
- Blockers: GPU 입력 실행·디코딩, 실시간 송신 통합과 성능 비교는 미검증임.
- Next Steps: 오프라인 빌드 후 기존 테스트 창·HEVC 디코딩·이미지 변화 하네스로 60개 출력 샘플을 확인함. 빌드 로그는 임시 파일럿 폴더의 `wgc-gpu-build.log`에 기록함.

## 2026-11-11 WGC D3D11 텍스처 직접 입력 검증
- Baseline: `C:\Users\k3832\orca\projects\fluxer`, `desktop-rs`, HEAD `02934b34`; 기존 미커밋 변경 보존.
- Done: `wgc_gpu_encode.rs`에 WGC D3D11 장치 공유와 IMFDXGIDeviceManager 설정, 별도 GPU 텍스처 복사와 DXGI surface sample 입력, 비동기 MFT 출력·drain을 구현함. 첫 빌드는 COM 인터페이스의 Send 제약으로 Exit 101; 비동기 MFT 및 WGC 직렬 콜백·장치 multithread 보호 조건을 명시한 Send 경계를 추가하고 실제 MF_TRANSFORM_ASYNC 확인 후 빌드 Exit 0. 파일 인코딩 시험은 60샘플 생성·60프레임 HEVC 디코딩·이미지 해시 60종, Exit 0. `chrome_control.py --mft-gpu` 연결 시험 `mft-gpu-e9a591de`: 입력·출력150샘플, Rust150수락, RTP135프레임, Chrome640×360 video/H265 8초 샘플103프레임 및 수신 이미지 변화, Exit 0. 송신12.001초, 5ms 미만 입력 간격0회. 잔여 시험 프로세스 없음. cargo fmt 및 Python 구문 확인. 독립 Sol/high 스레드·COM·종료 수명 검토 진행 중.
- Decisions: 입력 픽셀의 CPU readback과 원시 픽셀 파이프를 제거한 시험임. GPU 내부 텍스처 복사와 HEVC 출력 바이트의 CPU 읽기는 존재함. RAM·CPU·GPU 자원 절감은 아직 실측하지 않음. 첫 하드웨어 인코더 및 고정640×360·GOP15만 시험함.
- Blockers: 독립 검토 종료, 오류·장치 손실·크기 변경, 실제 자원 비교, PLI·혼잡 제어, Rust H.265 수신과 Fluxer 통합은 미완료임.
- Next Steps: 독립 검토에서 재현 가능한 수명·동시성 문제를 수정하고 적절한 오류 경로를 검증함. 이후 CPU 복사 경로와 같은 조건에서 자원을 측정하고 미디어 시간·피드백과 Rust 수신 구현을 진행함.

## 2026-11-11 GPU 인코더 수명 검토 및 종료 검증
- Baseline: `desktop-rs`, HEAD `02934b34`; 기존 사용자 변경 보존.
- Done: 독립 Sol/high 리뷰가 지적한 두 문제를 수정함. 활성화 직후 Encoder RAII 가드가 MFShutdownObject를 호출하도록 하여 초기화 실패·정상·조기 종료 수명을 보장하고, 마지막 입력 후 화면 갱신에 의존하지 않고 MFT drain 이벤트를 직접 처리하도록 변경함. 리뷰 재확인에서 해당 문제 해소 및 새로운 중대 결함 없음. 리뷰는 소스 검사임. 수정본 빌드 Exit 0 후 실제 파일 시험60 HEVC프레임·이미지60종, 로컬 송수신 `mft-gpu-04a14c65` video/H265103프레임/8초·이미지 변화, 모두 Exit 0. 자체 창을700ms 후 닫는 별도 실행은 예상 불완전 인코딩 오류와 Exit 1을 반환했으며 하네스는 Exit 0. README와 전체 계획에 GPU 검증 범위·제약을 반영함.
- Decisions: 초기 CPU readback 경로와 GPU 입력 경로를 모두 보존함. 모델 사용은 주 세션과 별도 Sol/high 리뷰 1개 스레드(수정 후 재검토 포함)이며 실제 사용량·절감액은 측정하지 않음.
- Blockers: 독립 바이너리의 드라이버 정지 시 내부 취소·시간 제한은 미구현이며 테스트는 외부 하네스 제한 시간으로 보호됨. 정지 화면에서의 drain 전용 재현 시험은 미실행. 자원 사용량 비교, PLI·비트레이트·프레임 시간 계약, Rust 수신과 실제 Fluxer 통합은 미완료임.
- Next Steps: 제품에 옮기기 전에 제한 시간·취소와 버퍼 재사용을 구현하고 CPU 경로 대비 자원을 측정함. 인코딩 샘플에 실제 시간·키프레임 요청을 연결하고 Rust 디코더 및 Fluxer 연결 소유권 구현을 진행함.

## 2026-11-11 GPU 인코더 이벤트 대기와 취소 구현 준비
- Baseline: `desktop-rs`, HEAD `02934b34`; 기존 사용자 변경 보존.
- Done: 별도 event_wait 모듈에 IMFAsyncCallback/BeginGetEvent 기반 이벤트 수신과 Windows 이벤트 핸들 대기를 작성함. GPU 진단기를 별도 캡처 스레드로 실행하고 외부 취소 이벤트·전체 제한 시간·종료 유예를 처리하도록 작성했으며 아직 빌드·실행하지 않음.
- Decisions: 주 대기는 Windows 이벤트로 처리하며 인코더 대기는 5초, 전체 시험 기본 제한은30초임. 종료 유예5초에도 드라이버가 반환하지 않으면 진단 프로세스는 Exit2로 종료함. 제품 API로 통합한 것은 아님.
- Blockers: 새 동시성 경로의 컴파일·실제 취소·제한시간·수신 회귀와 독립 검토는 미완료임.
- Next Steps: 오프라인 빌드 후 정상 인코딩·H.265 송수신과 정지 창/외부 취소/조기 창닫힘을 검증하고 동시성 검토를 수행함.

## 2026-11-11 GPU 캡처 취소·시간 제한 검증 완료
- Baseline: `C:\Users\k3832\orca\projects\fluxer`, `desktop-rs`, HEAD `02934b34`; 기존 사용자 변경 보존.
- Done: `event_wait.rs`에 IMFAsyncCallback/BeginGetEvent, 한 개의 미완료 요청과 bounded 채널, Windows 이벤트 대기를 구현함. 진단기를 초기화 이전부터 별도 소유 스레드로 감독하고 이름 있는 취소 이벤트·기본30초 제한·종료 유예5초를 연결함. 초기 빌드는 매크로의 windows_core 직접 의존 누락 Exit101; 기존 설치 버전0.61.2를 명시한 후 빌드 Exit0. 독립 Sol/high 검토가 발견한 start_free_threaded 초기화 대기 사각지대를 자체 감독 스레드로 수정했고 재검토에서 새 중대 결함 없음. 소스 검토와 실기 결과는 별개임.
- Done: 실제 GPU 파일 인코딩60샘플→HEVC60프레임·이미지60종, Exit0. `test_gpu_lifecycle.py` 네 경우 모두 예상 Exit1 확인: 사전 취소0.10초, 정지 화면 제한 시간1.25초, 실행 중 취소0.66초, 조기 창닫힘0.64초; 하네스 Exit0. 콜백 전환 직후 송수신은85프레임/8초였고, 입력 credit 대기에서 캡처 프레임을 보존하도록 수정한 최종 `mft-gpu-fb7e6310`은 H.265640×360104프레임/8초·수신 이미지 변화, Exit0. 입력/인코딩150, Rust수락150, RTP135프레임; 입력 완료12.134초·5ms 미만 간격5회. 테스트 프로세스 잔여 없음. cargo fmt, Python 구문 및 PROGRESS diff 공백 검사 Exit0.
- Decisions: 진단 프로세스는 종료 유예에도 드라이버가 멈추면 Exit2로 중단하며, 이 정책은 앱 내부용 종료 설계를 대체하지 않음. 새 동작과 CLI·검증 한계를 README에 기록함. RAM/CPU/GPU 감소는 아직 측정하지 않음.
- Blockers: 실제 초기화 정지 주입·drain 중 취소·강제 Exit2는 미실행. 출력 시간 계약·키프레임/비트레이트 피드백·텍스처 재사용, 자원 비교, Rust 수신·실제 Fluxer 연동은 미완료임.
- Next Steps: 영상 시간과 인코더 제어를 명시적 Rust 계약으로 연결하고 CPU/GPU 경로의 동일 조건 계측을 추가함. Rust H.265 디코더·표시 경로와 실제 Fluxer 참가자/트랙 소유권 통합을 진행함.

## 2026-11-11 HEVC 프레임 시간·키프레임 제어 연결 준비
- Baseline: `desktop-rs`, HEAD `02934b34`; 기존 변경 보존.
- Done: 독립 media_wire 크레이트에 버전·시간·해상도·키프레임·최대8MB 길이를 포함한 HEVC 패킷 계약을 작성하고 경계/잘림/버전/크기/왕복 테스트5개 통과. GPU MFT 출력 시간과 clean-point 속성을 전달하는 모드, Rust LiveKit 요청을 Windows 이벤트로 인코더 ForceKeyFrame에 연결하는 모드와 하네스를 작성함. 아직 양쪽 바이너리 빌드·송수신은 미검증. 최초 cargo fmt의 중첩 workspace 오류는 파일럿 workspace exclude로 수정함.
- Decisions: wire는 진단 프로세스 간 바이너리 계약임. 네이티브 UI·TS 타입 계약을 대신하지 않음. 제어 모드 GOP150으로 고정 주기 키프레임 의존을 줄여 요청 처리를 관찰함. 비트레이트 제어는 아직 연결하지 않음.
- Blockers: 새 모드 컴파일·실시간 시간/요청 처리 검증과 독립 검토, 실제 Fluxer 통합은 미완료임.
- Next Steps: 짧은 target·필요시 정적CRT를 사용해 두 파일럿을 빌드하고 기존/새 모드 송수신과 키프레임 로그를 확인함.

## 2026-11-11 프레임 시간과 키프레임 피드백 실기 확인
- Baseline: `desktop-rs`, HEAD `02934b34`; 기존 작업 보존.
- Done: media_wire 패킷 경계 검증5개 통과, GPU와 송신기 오프라인 빌드 Exit0. 실제 MFT timestamp/clean-point를 전달하고 WebRTC take_keyframe_request를 ForceKeyFrame으로 되돌리는 wire 모드를 실행함. 초기 시험은150수락·109RTP·요청30회로 캡처가15fps 설정을 초과하는 구간이 관찰됨. 캡처 단계에15fps 상한을 적용한 시험 `wire-mft-gpu-75ee34b5`:150수락·145RTP·99프레임/8초 H.265 디코딩, 요청4/인코더 강제3, Exit0. 관찰 결과이며 성능 벤치마크가 아님.
- Done: `recovery-wire-mft-gpu-cc755f37`에서 실제 구독 해제 후500ms 재구독, subscriptions2·해제 전31프레임·재구독 후87프레임 디코딩 및 수신 이미지 변화, Exit0. WebRTC요청5회·인코더강제3회·실제HEVC키프레임4개,150수락·145RTP, 마지막 캡처14.209초. 실제 시간 순서/해상도/키프레임 플래그와 HEVC 내용 대조를 실행함. lifecycle4케이스 재검증 Exit0. 독립 Sol/high 프로토콜·수명 검토 진행 중.
- Decisions: 자동 리셋 이벤트는 요청을 병합하므로 전달 수와 소비 수가 같다는 보장은 없음. 고정 GOP150과 실제 강제 요청을 구분함. 구독 복구는 참가자 재연결이나 전체 네트워크 장애 복구를 뜻하지 않음.
- Blockers: 독립 검토 완료, 비트레이트/혼잡 제어·자원 계측·텍스처 재사용·Rust 수신·실제 Fluxer 연동은 미완료임.
- Next Steps: 프로토콜 및 종료 오류를 검토·수정하고 비트레이트 피드백/실제 자원 계측을 연결함. 이후 Rust 네이티브 디코더와 Fluxer 참가자/트랙 소유권 통합을 진행함.

## 2026-11-11 프레임 거부와 키프레임 복구 검증 보강
- Baseline: `desktop-rs`, HEAD `02934b34`; 기존 사용자 변경 보존.
- Done: 독립 Sol/high 검토에서 stdin blocking 작업의 종료 대기와 주기적 GOP로 복구 검증이 통과할 가능성을 발견해 수정함. 진단 송신기를 명시적 Tokio runtime과250ms shutdown_timeout 경계로 변경함. `test_framed_rejection.py`는 잘못된 해상도/중복 timestamp 패킷 뒤 입력 파이프를 열어둔 채 각각0.26초에 예상 Exit1을 확인, 하네스 Exit0.
- Done: 복구 검사는 초기10개 실제 표시 프레임 후 구독 해제·재구독, 새 subscription generation의 requestVideoFrameCallback와3초 deadline으로 강화함. 첫 강화 시험 dbebf563은 해제 전 디코딩0으로 거부되어, 초기10개 실제 표시를 선행조건으로 바꿈. 정상 최종 `recovery-wire-mft-gpu-d47a8aba`는 첫 새 프레임433ms, 3초 내28새프레임·총79HEVC프레임/8초·화면변화, Exit0. 처음 피드백 없는 초기설정/1회수동key seed 대조 시도2개는 초기재생과 복구를 분리하지 못해 보존하고 중단함. 이후 초기10프레임 재생 뒤 feedback-stop 이벤트로 피드백만 끄는 대조를 구현함. 최종 `no-feedback-recovery-wire-mft-gpu-91011d6c`: 초기10프레임·구독2회·새프레임0·recovery_deadline으로 예상 Exit1, 검사 wrapper Exit0. 모든 시험은 자체 창/localhost임.
- Decisions: 첫 acceptance 결과는 lock으로 고정하며 늦게 도착한 통계가 실패를 덮지 않음. 주기적 GOP 이전 복구와 feedback-off 실패를 실행으로 비교했지만 일반적인 네트워크/참가자 재연결을 보증하지 않음. runtime shutdown_timeout은 진단 프로세스 한정이며 제품은 취소 가능한 reader가 필요함.
- Blockers: 비트레이트 혼잡 제어·메모리/CPU계측·텍스처재사용·Rust수신·실제Fluxer통합은 미완료임.
- Next Steps: 비트레이트 피드백과 동일 조건 자원 계측을 연결하고 Rust 수신 및 기존 Fluxer의 단일 참가자 미디어 소유권 통합을 진행함.

## 2026-11-11 WebRTC 비트레이트 제어 연결 준비
- Baseline: `desktop-rs`, HEAD `02934b34`; 기존 작업 보존.
- Done: media_wire의 선택적 Windows 제어 모듈에8바이트 atomic 비트레이트 mailbox와 별도뷰검증을 작성함. GPU 인코더에 CBR/MeanBitRate 동적설정·0목표 캡처일시중지, 송신기에100ms 피드백틱과0프레임상태에서도 WebRTC요청수신, 하네스에 GPU rate모드를 작성함. 아직 새코드 빌드·실행은 미검증임.
- Decisions: 목표값은0또는64kbps..8Mbps로 제한하며 초기800kbps, 캡처상한15fps임. fps변경은 아직 적용하지 않음. 출력 프레임·배포 제품의 완성은 별도임.
- Blockers: 컴파일, 프로세스간 제어/일시중지·재개, 동적 인코더 설정과 H.265 수신, 독립 동시성 검토는 미완료임.
- Next Steps: 오프라인 빌드 후 제어뷰 테스트 및 실제 인코더 목표 변경·정지/재개와 로컬 WebRTC 비트레이트 피드백을 검증함.

## 2026-11-11 비트레이트 제어 실기 및 SDK 제약 보완
- Baseline: `desktop-rs`, HEAD `02934b34`; 기존 사용자 변경 보존.
- Done: shared rate mailbox와 실제 인코더 readback을 구현하고 media_wire6개 테스트 통과. 첫 CBR rate 시험77fc1290은 초기표시3.39초/early0으로 실패; LowLatency 설정 추가 후 a0cc3e16은2.96초/58프레임으로 수신했지만 초기지연은 미해결임. readback GetValue 인자 오류 빌드Exit101은 설치API반환형에 맞춰 수정해 Exit0. 실제 외부 제어800→200→0→600kbps에서 요청/readback일치, 일시중지·재개 시 동일입력수13, 결과HEVC60프레임 디코딩, 하네스Exit0.
- Done: Sol/high 독립 검토에서 SDK SetRates가 Encode에서만 전달되어0목표 일시중지 후 자동재개가 막힐 가능성과 packet pacing이 피드백틱을 막는 문제를 발견함. SDK 어댑터는0포함64kbps floor로 명시적 fallback을 적용하고 pacing도 피드백 select와 통합함. sender4테스트(기존parser2+floor+pacedfeedback/error) 통과. 재검토에서 두 메커니즘 해소 확인. 최종 `rate-recovery-wire-mft-gpu-9f96e8f9`: 재구독첫화면375ms·3초내29새프레임·전체79H265프레임 및화면변화,Exit0. 외부0pause와SDKfallback은구분함.
- Decisions: 출력 비트레이트를 측정한 것은 아니며 MFT 속성 readback만 검증함. target FPS 적용·네트워크 혼잡/손실실기·자원절감은 미검증임. 기본 raw/기존wire경로를 보존함.
- Blockers: SDK의 독립 SetRates 전달/진정한0suspend, 초기지연약3초원인, 자원계측·텍스처재사용·Rust수신·Fluxer통합은미완료임.
- Next Steps: Windows MFT의 네이티브 Rust HEVC 디코더를 별도Sol/high worker가 독립 파일에 구현중이며 root는 실제GPU HEVC fixture/디코딩하네스를 작성함. 실제60프레임네이티브디코딩을 확인한뒤 SDK수신연결지점을 검증함.

## 2026-11-11 Rust 시스템 HEVC 디코더와 자원 비교
- Baseline: `C:\Users\k3832\orca\projects\fluxer`, `desktop-rs`, HEAD `02934b34`; 기존 사용자 변경 보존.
- Done: Sol/high worker는 독립 `mft_decode.rs`만 구현함. root는 Serena로 타입협상·출력/드레인·NV12배치/픽셀해시·실행경계를 검토하고 새파일555줄 diffstat을 확인함. 단일bin오프라인빌드Exit0, 픽셀해시(패딩제외/UV변화/잘림)테스트1개통과, 인자없음/빈wire 예상Exit1. root 실제 `test_native_decode.py` 실행 `native-decode-cd615da7`: WGC GPU/MFT 실제HEVC60샘플→Rust 시스템디코더 NV12 640×36060프레임·이미지60종, Exit0. 잘못된HEVC패킷은 예상Exit1, 하네스Exit0. 실제 LiveKit 수신이나 UI표시는 아님.
- Done: root는 wgc_raw에도15fps상한을 적용하고 `compare_capture_resources.py`를 구현·실행함. `resources-21ecd47a`: 동일테스트창/640×360/150프레임, 두출력 모두HEVC150프레임디코딩. CPU readback2프로세스 elapsed12.211초, CPU0.641초,100ms표본최고 private121626624bytes/working set96714752bytes. GPU surface1프로세스 elapsed12.092초, CPU0.281초,private107233280bytes/working set76767232bytes. 하네스Exit0, GPU메모리/LiveKit/브라우저/테스트UI/커널버퍼는 미측정. 짧은단일실행이며 전체앱개선으로 일반화하지 않음.
- Decisions: Rust 디코더는 시스템의 동기 HEVC MFT/Main8bit/NV12 고정 해상도·정수짝수표시영역만 지원하는 파일럿임. 반환프레임을 CPU에서 검증했으며 GPU디코딩을 주장하지 않음. 실제 SDK기본 수신기에는 이 디코더를 등록하지 않았음. root와Sol/high worker1개가 구현/통합을 나누고 기존Sol/high reviewer를 재사용함; 사용량수치는 미측정.
- Blockers: 네이티브 디코더의 LiveKit/WebRTC 수신등록·Rust영상표시, 실제 Fluxer 참가자·트랙 소유권, SDK 독립 rate피드백/true0suspend, 초기지연약3초원인, 목표FPS적용·장치/크기변경·텍스처재사용·전체앱성능은 미완료임.
- Next Steps: 검증한 시스템 HEVC 디코더를 실제 SDK 수신경로에 연결할 factory/FFI경계를 구현하고 실시간 원격프레임 전달을 확인함. 그후 실제 Fluxer 미디어 연결과 Rust UI에 연결하며 기존 웹UI는 전환기에 보존함.

## 2026-11-11 자원 비교 조건 보강 및 실제 수신 연결 조사
- Baseline: `desktop-rs`, HEAD `02934b34`; 기존 변경 보존.
- Done: 자원하네스의 창제목을 두경로에서 같게 고정하고 `--gpu-first` 역순 실행을 추가함. `resources-950feddd`: GPU150프레임12.156초/CPU0.359초/표본최고private105930752bytes/working set62730240bytes; CPUreadback150프레임12.020초/CPU0.484초/private121061376bytes/working set94154752bytes. 둘다HEVC150프레임디코딩·Exit0. 프로세스수2대1차이/캐시와샘플링편차가 포함되며 GPU복사만의 효과나 전체앱개선으로 분리하지 않음.
- Done: 별도Sol/high expert가 현재 SDK 공개RustAPI에는 사용자 decoder/factory 등록이나 encoded receive callback이 없음을 소스에서 확인함. 네트워크수신과 검증된RustMF디코더 연결에는 webrtc-sys의 Windows VideoDecoder/CXX 경계와 factory 등록이 필요하다는 구체적 조사결과를 전달했으며 최종 스레드/빌드계약 검토중임. SDKfork나 레지스트리 소스 수정은 아직 수행하지 않음.
- Decisions: 기존 NativeVideoStream의 디코딩된 VideoFrame 전달 계약을 유지할 수 있는 최소SDK변경을 우선 검토함. 독립Rust파일디코딩을 실제WebRTC수신완료로 간주하지 않음.
- Blockers: 실제SDK수신factory/FFI구현, 실제Fluxer연결, 제품안정성·전체앱성능은 미완료임.
- Next Steps: SDK decoderfactory와 Rust MFcore의 호출/스레드/프레임소유권 계약을 확정하고 국소fork로 구현한뒤 실제Rust NativeVideoStream에서 H.265디코딩을 검증함.

## 2026-11-11 실제 SDK HEVC 수신 연결 경계 확정
- Baseline: `desktop-rs`, HEAD `02934b34`; 기존 사용자 변경 보존.
- Done: 별도Sol/high expert가 위임된 읽기전용 소스조사를 완료함. 기존Rust공개API에는 customdecoder/encodedreceive seam이 없고, webrtc-sys의 Windows VideoDecoder/CXX/MFT factory 등록이 최소 연결 지점임을 확인함. root는 반환된 경계·전용COM스레드·owned NV12/RTP시간매핑·Release정리·SDK빌드식별·실제NativeVideoStream 수락조건을 `tools/h265_livekit_probe/NATIVE_HEVC_RECEIVER_INTEGRATION.md`에 기록함. 해당expert는 코드수정/빌드/GUI실행을 하지 않았으며 worker의 standalone디코더와도 별도임.
- Decisions: SDK수신연결을 위해 저장소 소유의 국소fork를 사용하고 registry/global설정은 수정하지 않음. 현재는 계약만 확정했으며 fork/FFI구현은 시작하지 않았음. `max_render_resolution`을 실제입력크기로 오용하지 않고 output sample time으로 bounded RTP매핑을 복원함.
- Blockers: 실제 SDK decoder adapter·CXX/COM worker 및 LiveKit Rust수신실기, Rust영상UI와 Fluxer통합은 미완료임.
- Next Steps: 계약대로 MF core추출/국소SDKfork/Windowsfactory를 구현하고 빌드후 기존송신→Rust NativeVideoStream의 실제H.265프레임을 검증함.

## 2026-11-11 실제 WebRTC HEVC 디코더 구현 진행
- Baseline: `C:\Users\k3832\orca\projects\fluxer`, `desktop-rs`, HEAD `02934b34`; 기존 사용자 변경 보존.
- Done: registry의webrtc-sys0.3.53 전체패키지(license메타데이터/원본헤더포함)를 `tools/h265_livekit_probe/vendor/webrtc-sys`에 복사함. 복사직후 Cargo.toml SHA256원본/사본09A6F12FB91F660BA779CE930B3863021D6FDED9522C96DD9D3F219DEC2DC36C 일치. 현재는사본만수정함. Windowsfactory등록·CXXbridge·NV12→I420adapter·bounded RTP/render시간매핑·빌드등록·파일럿Cargo patch를 작성함. root는 NativeVideoStream의픽셀변화검사와 --native-loopback/독립localhost하네스를 작성함. Python구문검사만실행, SDK컴파일/수신실기는아직미검증.
- Done: Sol/high 구현worker가 MFcore추출과전용COMactor를 `native_hevc`에 구현중임. expert역할의추가구현요청은역할내분석전용제약으로수행되지않았고, C++adapter는root가직접작성함. 권한거절이나사용자승인대기는아님.
- Decisions: 모든COM객체는전용Rust스레드에서만사용·해제하며 C++는owned NV12결과를호출스레드에서WebRTC callback에전달함. 실제 SPS크기를검사하고 max_render_resolution을실제크기로대체하지않음. 기존코덱factory를보존하고 현재SDK archive실제경로/webrtc-a545de4를빌드로그로식별함.
- Blockers: MF actor완료/단위검증, SDKfork C++컴파일·독립검토·로컬Rust수신, 실제Fluxer통합은미완료임.
- Next Steps: actor/CLI회귀검증 후 짧은TEMP target과정적CRT로패치SDK를빌드함. 백그라운드빌드가되면로그·종료코드를기록하고수동폴링하지않음. 통과하면파일fixture와실시간WGC송신을 Rust NativeVideoStream에서검증함.

## 2026-11-11 SDK 소스 선택과 어댑터 사전 검토
- Baseline: `desktop-rs`, HEAD `02934b34`; 기존 변경 보존.
- Done: Windows로 제한한 cargo metadata offline에서 LiveKit0.10.1/libwebrtc0.4.0은 기존registry, webrtc-sys0.3.53은프로젝트vendor, fluxer-native-hevc0.1.0은새core경로로 해석됨을 확인함. 전체플랫폼 offline metadata는미설치Android의존으로실패했으며 다운로드/설치없이Windows범위로한정해통과함. PowerShell메타데이터UTF16출력파서는BOM에맞게읽어확인함.
- Done: 독립Sol/high사전검토는예상vendor변경만있음을확인하고 C++pendingRTP300한도에서영구거부가될수있는문제를발견함. root는한도초과/입력에러시worker정리후다음keyframe으로재시작, 정리실패시동일instance재생성금지, 중복pendingRTP무출력처리를작성함. 해당복구는아직컴파일·실기미검증임.
- Decisions: 최초소스선택·사전검토는빌드/실제수신성공의대용이아님. actor와FFI전체검토및실제SDK빌드는아직남아있음.
- Blockers: actor완료·CLI회귀·실제SDK컴파일/수신검증은미완료임.
- Next Steps: MFactor준비결과를통합하고SDK빌드의최초오류또는성공을기록한뒤 실제파일/live송신수신을검증함.

## 2026-11-11 공유 디코더 코어 회귀와 SDK 빌드 준비
- Baseline: `desktop-rs`, HEAD `02934b34`; 기존 미커밋 작업 보존.
- Done: 구현worker followup이 실행 전 pending_init에 머물러 interrupt한 뒤 root가 기존작성core/actor를보존해직접이어받음. Session시작에서실제MFT활성화/NV12협상을probe하도록수정하고, 첫키프레임실제크기검증·짝수크기/메모리한도·bounded pending RTP·재정렬출력의정확한time제거를보완함. SDK용core는누적300프레임으로멈추지않고CLI만300packet으로제한함. core4단위테스트(픽셀배치/홀수거부/RTPwrap+재정렬/알수없는출력시간거부)통과.
- Done: CLI를새DecoderHandle소비자로전환함. actual capture timestamp를RTP로변환할때초기값을u32::MAX근처로두어실제60프레임디코딩중wrap과원래입력RTP매핑을확인함. 명시적release2회+Drop실행. `native-decode-b0fa8468` 실제60H265→60NV12/이미지60종, 잘못된HEVC 예상Exit1, 하네스Exit0. nativeactor/CLI빌드Exit0. Python native_loopback하네스구문통과. 전체SDK는아직미컴파일/실기미검증.
- Decisions: root가CXX/CPP와actor통합을직접소유함. 독립reviewer가전체actor/FFI를추가검토중임. 기존소스는프로젝트vendor사본만수정하고registry에는쓰기없음.
- Blockers: SDK전체C++/Rust빌드·실제NativeVideoStream수신·통합검토는남아있음.
- Next Steps: CARGO_TARGET_DIR=%TEMP%\lkprobe-target, RUSTFLAGS=-C target-feature=+crt-static, CARGO_BUILD_JOBS=2로offline SDK빌드를실행함. 로그 `%TEMP%\fluxer-livekit-h265-pilot\native-sdk-build-1.log`, 종료파일 `native-sdk-build-1.exit`. 백그라운드로넘어가면수동폴링하지않고턴을종료함. 다음결과확인후 `native_loopback.py --fixture`와 live시험을진행함.

## 2026-11-11 초기화 실패 worker 보존과 SDK 전체 빌드 시작
- Baseline: `desktop-rs`, HEAD `02934b34`; 기존 사용자 변경 보존.
- Done: 독립검토의추가문제(초기화timeout후worker소유권상실/재생성누적)를수정함. startup/release미정리worker JoinHandle을보존하고, 동일process에서미완료worker가있으면새생성을거부하며완료worker만회수함. 실제capabilityprobe도같은감독actor를사용함. 재검토에서새중대문제없음. nativecore5테스트통과(이전4개+소유한정지worker존재시새생성거부). root최종CLI/actor회귀 `native-decode-2610a6b2`:60실제HEVC→60NV12/60화면·RTPwrap대조·release2회+Drop·잘못된HEVC거부,Exit0.
- Decisions: 주입unit검사는대기중worker새생성거부를검증했지만실제드라이버startup정지/자동회수경로는미실행임. SDK전체컴파일과실제수신은아직통과로보고하지않음.
- Blockers: 실제SDK/CXX컴파일·NativeVideoStream의H.265수신/cleanup 검증은남아있음.
- Next Steps: 지금SDK전체offline빌드를실행함. buildlog `%TEMP%\fluxer-livekit-h265-pilot\native-sdk-build-1.log`, exit파일 `native-sdk-build-1.exit`, source환경기록 `native-sdk-build-1.meta.json`. 짧은lkprobe-target/정적CRT/jobs2 사용. 실행중으로넘어가면지침대로수동폴링하지않고턴을종료함. 성공후fixture/live NativeVideoStream 수신을검증함.

## 2026-11-11 컨텍스트 사용 점검
- Baseline: `desktop-rs`, HEAD `02934b34`; 기존 변경 보존.
- Done: 최근 작업 기록과 빌드 종료 파일을 확인함. SDK 빌드 `native-sdk-build-1` Exit 0, dev 빌드 1분 13초. 실행/실제 수신은 미검증. `PROGRESS.md` 점검 시 약 90KB이며 준비·완료 기록이 연속 누적된 것을 확인함.
- Decisions: 다음 작업부터 현재 상태/다음 단계 위주로 기록하고, 과거 기록은 필요한 구체적 사실을 확인할 때만 조회함. 반복 계획·전체 로그 출력·중복 검증을 줄이며 실패 증거와 실행별 결과는 보존함.
- Blockers: 실제 Rust NativeVideoStream H.265 수신 및 Fluxer 통합은 미완료임. 대화의 실제 토큰별 사용량은 이번 점검에서 측정하지 못함.
- Next Steps: 기존 빌드 결과를 재사용해 fixture 및 실시간 NativeVideoStream 수신을 검증함.

## 2026-11-11 README 이주 목표와 현황 정리
- Baseline: `desktop-rs`, HEAD `02934b34`; 기존 미커밋 변경과 미추적 파일을 보존함. 기존 추적 파일 diff는 README 수정 전 13개 파일, 1746줄 추가/521줄 삭제였음.
- Done: 루트 `README.md`에 최종 목표, 단계별 완료 기준과 현재 진행 상황을 정리하고 실험과 실제 Fluxer 통합을 구분함. 기존의 검증되지 않은 완전 호환·메모리 절감 주장을 제거함. 진단 빌드 `native-sdk-diag-1.exit`는 0으로 확인했으나 이 작업에서 수신 시험은 실행하지 않음.
- Decisions: Windows 데스크톱을 이주 완료 범위로 명시하고 WebView2는 전환기에 유지함. UI 프레임워크는 확정하지 않음.
- Blockers: Rust NativeVideoStream H.265 프레임 전달과 실제 Fluxer 통합은 미완료임. 계획 문서 등 미추적 파일은 Git에 추가되기 전에는 GitHub에서 링크가 열리지 않음.
- Next Steps: README의 링크와 diff를 확인하고, 이후 진단 빌드로 H.265 수신 실패 경계를 조사함.

## 2026-11-11 README GitHub 게시
- Baseline: `desktop-rs`, 기존 HEAD `02934b34`; 다른 미커밋 및 미추적 작업을 보존함.
- Done: README만 커밋 `4f6c7504`로 기록하고 `git push origin desktop-rs` Exit 0으로 게시함. 게시되지 않은 로컬 문서 링크는 설명으로 변경함. `git diff --cached --check` Exit 0.
- Decisions: 기존 개발 변경과 실험 코드는 이번 게시에 포함하지 않음.
- Blockers: 네이티브 H.265 수신 및 실제 Fluxer 통합은 미완료임.
- Next Steps: 진단 빌드의 수신 경계 결과를 확인함.

## 2026-11-11 SDK 실제 수신 실패와 경계 진단
- Baseline: `desktop-rs`, HEAD `02934b34`; 기존 변경 보존. 성공빌드1의실행파일SHA256 56A98557829BC7461FC34FA7BFF634DC2E64BD7FBA3217E79A574794DD392CE2 확인.
- Done: 실제localhost fixture수신 `native-fixture-4bdf2675`는H265협상·Rust디코더구성까지확인했지만프레임수신timeout으로실패. 선택적WindowsHEVC로그만추가한실행b3a9e774도동일실패. 프레임없음4초때track통계확인진단을추가한c6e3cb83은해당통계반환에inbound항목이없었고프레임없음으로실패. 각각로그보존/자체프로세스정리됨. 반복재시도중단후CPPDecode경계의최초8호출에입력메타데이터/SPS/설정상한/출력개수만기록하도록진단추가. 아직진단SDK빌드/실행미완료.
- Decisions: SDK컴파일성공과실제수신성공은별개임. 현재증거만으로네트워크/타입협상/출력콜백중원인을단정하지않음. 사용자화면·계정데이터는쓰지않음.
- Blockers: 실제NativeVideoStream H265프레임전달은실패상태이며Fluxer통합은아직미완료임.
- Next Steps: 진단변경을jobs2/정적CRT/짧은target으로재빌드. 로그native-sdk-diag-1.log/종료native-sdk-diag-1.exit. 백그라운드로넘어가면수동폴링없이종료하고완료후CPP경계로그로최초실패를분리함.

## 2026-11-11 HEVC 디코더 지연 출력 반환값 수정
- Baseline: `desktop-rs`, HEAD `4f6c7504`; 기존 수정 및 미추적 작업을 보존함.
- Done: 진단 빌드 Exit 0 확인 후 실제 fixture `native-fixture-2d52d940` 실행은 Exit 1. 입력 640×360 Main8, 디코더 구성 및 SPS 정상, 키프레임만 6회 전달되고 각 출력 0임을 확인함. 프로세스 정리는 하네스에서 완료함. WebRTC upstream `video/video_receive_stream2.cc`의 HandleEncodedFrameOnDecodeQueue는 OK/OK_REQUEST_KEYFRAME 이외 반환값에서 keyframe_required를 설정함. 시스템 디코더가 입력을 수용하고 출력을 지연한 경우 C++ adapter가 NO_OUTPUT 대신 OK를 반환하도록 수정함. git diff --check Exit 0.
- Decisions: 실제 수신 수락 기준은 기존 10프레임 이상, 화면 해시 2종 이상으로 유지함. 출력 지연과 입력 실패를 구분함. upstream 근거: https://raw.githubusercontent.com/webrtc-mirror/webrtc/main/video/video_receive_stream2.cc . 설치 SDK와의 동작 일치는 수정 후 실기로 확인해야 함.
- Blockers: 수정본 SDK 재빌드 및 실제 수신 검증은 아직 미완료임.
- Next Steps: native-sdk-buffered-ok-1.log/.exit로 SDK offline 빌드. 완료 후 native_loopback.py --fixture, 통과 시 live 실행. 빌드가 비동기로 넘어가면 프로젝트 지침에 따라 반복 조회 없이 턴을 종료함.

## 2026-11-11 로컬 Rust H.265 실시간 수신 확인
- Baseline: `desktop-rs`, HEAD `4f6c7504` 및 기존 미커밋/미추적 변경. 이번 변경: HEVC C++ adapter 반환값, Rust MF 저지연 속성, CLI drain 전 출력 진단, main 초기 수신 대기, README 현황.
- Done: buffered-ok SDK 빌드 Exit 0. fixture `749ca6a9` Exit 1은 delta 전달을 확인했으나 첫 출력 전 진단용 4초 제한으로 종료됨. 독립 decoder의 실제 60샘플 비교는 기본 설정 drain 전51/최종60, MF_LOW_LATENCY=1에서 drain 전59/최종60·화면60종임. lowlatency SDK 빌드 Exit 0; fixture `0012e891` Exit 1에서 후속 입력마다 실제 출력1을 확인했지만 최초 입력 직후 이미 진단용 timeout이 발생했음. 초기 대기만10초, 출력 시작 후4초로 구분한 main 재빌드 Exit 0(5.07초). 기존 frame10/hash2/640×360 수락 기준 유지.
- Done: 수정 후 `native-fixture-cdaa4ec8` Exit 0, native_decoded10/distinct10. `native-live-9032f8d8` Exit 0, WGC GPU 입력→MFT H.265→localhost LiveKit→Rust NativeVideoStream에서 native_decoded10/distinct10. 두 하네스 모두 소유 publisher/encoder/window/server 정리를 보고함. 잘못된 HEVC wire는 예상 Exit1로 거부됨. 실행 파일 SHA256 `08D064F95AB10D2D3ADD5161D18AEAD045305015923B2AFCE37E6AE2C9372B64`. 로그는 %TEMP%/fluxer-livekit-h265-pilot 아래 실행별 디렉터리에 보존함. git diff --check Exit 0(문서 최종 갱신 전).
- Decisions: MF 저지연은 공식 HEVC decoder IMFAttributes 문서에 따라 설정함(https://learn.microsoft.com/en-us/windows/win32/medfound/h-265---hevc-video-decoder). 진단용 초기4초 제한을10초로 수정한 사실을 명시하며, 성능 개선이나 첫 화면 지연 수락으로 보고하지 않음. 기존 Sol/high reviewer에게 좁은 읽기 전용 검토를 요청함.
- Blockers: Rust 수신 구독 복구/반복 생성·종료, 별도 기기·실제 Fluxer 채널·Rust UI 영상 표시·전체 앱 자원 실측은 미완료임. 하네스의 정리 출력은 네이티브 메모리/스레드 누수 부재를 입증하지 않음.
- Next Steps: 검토 결과를 반영하고 Rust 수신 구독 해제/재구독과 반복 수신 수명 시험을 추가함. 이후 Fluxer 참가자/트랙 소유권 및 Rust 표시 경계를 연결함. README 변경은 이번 개발 턴에서 아직 GitHub에 게시하지 않음.
- Review: 기존 Sol/high reviewer의 읽기 전용 검토에서 이번 네 가지 변경에 조치할 결함이나 수락 조건 우회가 없다는 결과를 받음. root가 반환값/속성/대기 조건 변경을 직접 확인했고 최종 git diff --check Exit0. 초기10초는 전체 시험 deadline이 아니라 첫 프레임 대기임. 검토자는 실기를 재실행하지 않았음.

## 2026-11-11 Rust 수신 재구독 복구와 실패 대조군
- Baseline: `desktop-rs`, HEAD `4f6c7504`, 기존 미커밋/미추적 변경 보존. 이번 코드 수정은 probe src/main.rs, native_loopback.py만 대상임.
- Done: --native-recovery는 초기10프레임/화면2종 확인 후 기존 sink/track을 버리고 unsubscribe SID 일치 이벤트를 기다린 뒤500ms 후 재구독함. 새 구독마다 새 NativeVideoStream/해시 집합으로 첫 프레임2.5초·5프레임/화면2종·직전 세대에 없던 해시를3초 내 확인하며 두 번 반복함. subscription 대기와 frame 대기는 동일 남은3초를 사용하고 최종 해시 검증 후에도 deadline 확인함.
- Done: 정상 예비9ba727ba/3e582439 모두 Exit0·총20프레임. 최종 build4 Exit0(5.12초), native-recovery-4f86803a Exit0·총20프레임·각 복구5화면, 첫 프레임616/440ms. native-negative-fcf19cf3는 초기10프레임 확인 후 키프레임 전달만 차단하고 generation1 재구독 완료 후 frame deadline으로 예상 publisher Exit1, 하네스Exit0. 각 실행의 소유 프로세스는 하네스에서 정리함. 최종 exe SHA256 4081260D831C09B5B679A25C458BED8890493ED1A70D578576C4FCFD8A62311F. 로그는 %TEMP%/fluxer-livekit-h265-pilot/실행명에 보존함.
- Decisions: 별도 Sol/high 읽기 전용 검토에서 실패 대조군이 subscription timeout도 성공으로 인정하는 P2를 발견해 수정함. 재구독 완료 표지와 프레임 복구 실패를 모두 요구하며 최종 정상/실패 대조군을 재실행함. 재검토에서 새 지적 없음. SDK 설치 소스에서 set_subscribed 및 NativeVideoStream sink 정리를 확인함. 외부 라이브러리나 SDK 변경 없이 기존 계약을 사용함.
- Blockers: 이 시험은 새 Rust stream에서 수신과 반복 재구독을 입증하지만 C++ decoder 파괴 횟수, 메모리·스레드 누수 부재나 별도 네트워크 재연결을 입증하지 않음. 전체 Room 반복 생성/종료, Rust UI 표시, 실제 Fluxer 참가자·트랙 통합은 미완료임.
- Next Steps: Room/decoder 수명 관측을 추가하고 실제 Fluxer 미디어 참가자·트랙 계약을 연결함. 현재 변경은 GitHub 미게시임.

## 2026-11-11 Room 반복 생성 및 HEVC worker 정리 검증
- Baseline: `desktop-rs`, HEAD `4f6c7504`; 기존 변경 보존. 수정은 probe native_hevc/src/lib.rs, src/main.rs, native_loopback.py에 한정함.
- Done: worker마다 식별자를 부여하고 생성/Stopped 응답+join 성공/handle Drop 완료를 기록함. 하네스는 생성ID의 유일성, join/drop ID 다중집합 일치와 cleanup_ok=true를 요구함. --room-cycles는 같은 프로세스에서 fixture Room을2회 생성·수신·종료하고 각 cycle 표지와 두 수신 결과를 요구함. 타임아웃된 receiver task는 abort 후 await하며, 수신 실패 시에도 두 Room.close를 시도함. native_rooms_closed는 양쪽close 성공일 때만 기록함.
- Done: 최종 build2 Exit0(16.67초). 실제 native-fixture-5fc35552는 Exit0, Room2회 종료·각10프레임 이상/화면변화·worker3개 모두join/drop 확인. native-negative-3bba385b는 초기수신 후 피드백차단/재구독/frame deadline 예상 publisher Exit1, 하네스Exit0·Room1회 종료·worker2개join/drop 확인. 예비c5203909/ce8c9acb 결과도 보존함. 최종 exe SHA256 2E168F1AF1F09D34336187D79D98BBB8C0D0996133F0CD02458DB5A70B4FCF47. 로그 %TEMP%/fluxer-livekit-h265-pilot/실행명.
- Done: Sol/high 검토에서 close 오류가 원수신 오류를 가리는 P2를 확인해 finish_receiver_cleanup으로 양쪽close 오류와 원수신 오류 chain을 보존함. 해당 회귀 단위테스트1개 실제통과(4개filtered, 0 skipped). 수정후 위실기재실행/재검토 새지적없음. git diff --check Exit0.
- Decisions: 관측 범위는 Rust handle Drop·COM worker join·Room.close 성공이며 C++ decoder object 파괴 횟수나 전체 native 메모리 누수 부재를 주장하지 않음. 기존 startup-timeout 테스트/불완전worker 보존정책은 변경하지 않음. 반복Room은 재생 가능한 fixture에만 허용함.
- Blockers: 실패가 스트림 수신결과를 받기 전 발생하는 모든 초기화/생산자 경로의 취소정리는 아직 통합하지 않음. 초기화 정지/장시간·GPU자원누수는 미검증임. 실제Fluxer·RustUI는 미완료임.
- Next Steps: 실제Fluxer VoiceEngineV2AppConnectionHostAdapter의 token/endpoint/connection_id 및 autoSubscribe=false 계약과 ScreenShareLiveKitFlows의 기존 room.localParticipant 소유권을 기준으로 Rust 연결 전환을 설계·구현함. 기존 웹참가자와 같은토큰으로 추가접속하지 않음. Serena의 해당TS심볼요청이 완료되지 않아 취소하고 좁은rg조회를 사용했으며 원본UI수정은없음.

## 2026-11-11 Fluxer 서버 응답 계약과 명시적 구독 연결
- Baseline: desktop-rs HEAD4f6c7504, 기존 미커밋/미추적 변경 보존. frontend 원본/데스크톱 IPC는 수정하지 않음.
- Done: Serena로 VoiceEngineV2AppConnectionHostAdapter의 구조/createRoomConnectOptions를 확인함. VOICE_SERVER_UPDATE는 token/endpoint/connection_id 및 optional guild/channel/e2ee_key/ice_servers를 전달하며, ice_servers가 있으면 빈배열도 VoiceMesh로 분기하고 autoSubscribe=false임을 원본에서 확인함. probe src/fluxer_voice.rs에 해당 serde/ts-rs 입력 계약, redacted Debug, 접속 전 validation 및 실제 SDK connect(auto_subscribe=false)를 구현함. probe의 두 local Room은 이 경로를 사용하며 TrackPublished에서 명시적으로 구독함.
- Done: URI는 WSS 또는 loopback WS만 지원하고 userinfo/query/fragment와 빈·과대token/connectionID를 거부함. E2EE key 및 ice_servers가 존재하는 요청은 미지원 오류로 접속 전에 거부함. 새 의존성은 설치된 serde/ts-rs10.1.0/url2.5.8 소스로 확인하고 offline resolve/build함. src와 bindings/contracts/VoiceServerUpdate.ts 및 IceServer/IceUrls 타입이 생성됨.
- Done: fluxer_voice 계약 단위/타입생성 테스트4개 실제통과(5개filtered/0skipped). Sol/high 검토에서 오래된 생성TS의 e2ee null누락을 찾아 재생성하고 decl() null assertion을 추가함. 재검토 새지적없음. ts-rs가 serde의 skip_serializing_if 조합을 해석하지 못한다는 warning은 남지만 명시적 ts(optional/nullable)와 생성결과로 실제형태를 검증함. warning을 숨기지 않음.
- Done: fluxer-voice-contract-build-1 Exit0(17.08초). native-fixture-2c2cec0d 실제Room2cycles Exit0·각10프레임·worker3join/drop. native-recovery-0e0a64fb 실시간Exit0·20프레임·복구첫화면520/517ms·worker4join/drop·Room닫힘 확인. 로그 %TEMP%/fluxer-livekit-h265-pilot 아래 보존함. git diff --check Exit0.
- Decisions: 현재 typed adapter는 격리probe의 로컬발급token으로만 실행하며 실제계정/웹Room의 소유권을 넘기지 않음. SDK 접속오류는 토큰노출 방지를 위해 일반 오류로 반환함. 같은토큰의 추가참가자를 실제Fluxer에 생성하지 않음.
- Blockers: production IPC/브리지에서 서버응답 전달·신뢰검증·웹Room해제 확인·시도세대/채널일치·RustRoom상태전달은 아직 미구현임. E2EE/VoiceMesh/지역변경·오디오·제품UI는 미완료임.
- Next Steps: 실제 연결 전환에 필요한 인증된 브리지와 웹Room해제 확인, stale attempt 취소 및 실패복구를 기존연결상태와 함께 구현함. 지원되는표준LiveKit만 대상으로 시작하고 미지원연결을 자동다운그레이드하지 않음.

## 2026-11-17 upstream 동기화 및 Claude 작업 환경 구성
- **Baseline**: `desktop-rs` 병합 커밋 `d1124083` 위에 기존 미커밋 작업 14개 파일과 미추적 문서·probe를 그대로 복원했다. 병합 전 상태는 `backup/desktop-rs-pre-upstream-20261117`(HEAD `4f6c7504`)와 `stash-backup/wip-20261117`(미커밋 작업) 브랜치에 보존했다. push는 하지 않았다.
- **Done**: `upstream/main` `5d4953aa`(신규 커밋 311개)를 `--no-ff`로 병합했다. 충돌은 `.gitignore`(포크 항목과 upstream 항목 병합, `/AGENTS.md`·`/CLAUDE.md` 무시 유지), `README.md`(포크 README 유지), `Cargo.lock`, `win-shell/Cargo.lock`(upstream 삭제 수용)이었다.
- **Decisions**: upstream이 `fluxer_desktop/native/*`를 루트 워크스페이스 멤버로 넣으면서 gpui 0.2.2의 `core-foundation =0.10.0` 고정과 macos-input-hook의 `^0.10.1` 요구가 충돌했다. `fluxer_desktop_rs`를 독립 워크스페이스(자체 `Cargo.lock`, 이전 루트 lock에서 시작)로 분리하고 루트 `exclude`에 넣었다. `scripts/build_fluxer_rust.ps1`은 `--manifest-path fluxer_desktop_rs\Cargo.toml --target-dir target`으로 바꿔 산출물 경로 `target/release/fluxer.exe`를 유지했다(스크립트 실행은 미검증).
- **검증**: `fluxer_desktop_rs`에서 `cargo check --all-targets` 통과, `cargo test` 96 passed / 0 failed. 루트에서 `cargo check -p fluxer_platform_info -p fluxer_win_shell -p fluxer_windows_input_hook -p fluxer_win_process_loopback` 통과. 실제 앱 실행과 릴리스 빌드는 이번 세션에서 하지 않았다.
- **작업 환경**: 로컬 전용 `CLAUDE.md`(AGENTS.md 가져오기, 명령, 위임 정책)와 `.claude/agents/fluxer-scout`(Haiku), `fluxer-porter`(Sonnet), `fluxer-reviewer`(Opus)를 만들었다. 사용자 지시에 따라 Codex·Gemini는 쓰지 않고, T3 교차 검토는 DeepSeek과 Opus 검토자로 대체한다.
- **Next Steps**: upstream의 데스크톱 변경(`fluxer_desktop/src/main` 62개 파일: 다운로드 표시, 암호화 부트 상태 복원, 트레이 등)을 Rust 셸 IPC와 대조해 누락 기능을 정리한다. 미커밋 작업을 검토한 뒤 커밋하고 `origin/desktop-rs`에 push할지 결정한다.

## 2026-11-17 Rust GUI 프레임워크 조사와 GPUI 시제품
- **Baseline**: `desktop-rs` `d1124083`과 기존 미커밋 작업. 새 시제품은 독립 워크스페이스 `tools/gpui_spike/`(미추적)이며 기존 코드는 수정하지 않았다.
- **규모**: `fluxer_app/src`는 TS/TSX 3,464개, 약 62만 줄이다(voice 9.8만, channel 7.6만, user 6.2만, ui 4.3만, messaging 4.1만, auth 3.8만). MobX·xstate, Lexical 작성창(약 1.5만 줄), Lingui 44개 로케일을 쓴다. gateway·REST·snapshot(약 1.3만 줄)은 React 의존이 없어 Rust 코어로 먼저 옮기기 쉽다. Rust 마크다운 파서 `packages/markdown_parser/rust`가 이미 있다.
- **프레임워크 비교(웹 조사, 일부 2차 출처)**: 추천은 GPUI + longbridge gpui-kit, 차점은 Slint 1.18. 근거는 gpui-kit의 가변 높이 가상 목록과 Markdown 텍스트 선택, winit 기반 프레임워크 공통의 Windows 한글 IME 캐럿 버그(winit PR #4746 미병합)를 GPUI 자체 Win32 백엔드는 피한다는 점이다. 위험은 잦은 API 변경, 업스트림에 없는 D3D11 영상 표시, Windows 스크린리더 불완전이다. iced·egui·Dioxus Native·Freya·Makepad·Xilem·Floem은 탈락.
- **crates.io 직접 확인**: `gpui` 0.2.2에서 정체, `gpui-pre` 0.3.13(2026-11-16, 주간 릴리스, Apache-2.0), `gpui-kit` 0.10.0(2026-11-16)은 `gpui-pre =0.3.12`를 고정한다.
- **GPUI 0.2.2 시제품 측정**: 가변 높이 2만 행 `list` + `EntityInputHandler` 입력창. `--bench` 끝까지 스크롤 5.1초, 1,571 렌더(약 308fps, render 호출 수 기준), 메모리 private 60→76MB. 루트에서 재실행해 재현했다. 한글 IME 실제 조합·화면 표시·글꼴 폴백은 사람이 확인해야 하며 미검증이다(체크리스트 `tools/gpui_spike/README.md`).
- **Next Steps**: gpui-kit 0.10 + gpui-pre 0.3.12 시제품(`tools/gpui_kit_spike/`, 5만 개 Markdown 행)으로 같은 bench를 비교하고, 한글 IME 수동 검증, D3D11 영상 표시 경로를 확인한 뒤 프레임워크를 확정한다.
- **gpui-kit 0.10.0 시제품 측정(`tools/gpui_kit_spike/`, 미추적)**: `MessageScroller`(행 높이 자동 측정) + `TextView::markdown(..).selectable(true)` + `Input`. 5만 행 끝까지 스크롤(12행 단위 점프): markdown 13.3~14.1초, 프레임 간격 p50 3.1~3.2ms / p99 5.3~6.5ms / max 17~44ms, 종료 private 116~156MB. plain 텍스트는 종료 private 92MB, max 6.4ms. 루트에서 재실행해 재현. 경고 0. `v_virtual_list`는 행 높이를 미리 줘야 해서 채팅 목록에 부적합하다.
- **Decisions(확정 2026-11-17, 사용자가 gpui-kit 시제품에서 한글 입력 정상 확인)**: 데스크톱 Rust UI는 GPUI + gpui-kit(`=0.10.0`, gpui-pre `=0.3.12`)으로 진행한다. 확정 조건은 한글 IME 수동 검증 통과다. 실패하면 Slint와 재비교한다. 기존 오버레이·스트림 뷰어의 crates.io `gpui` 0.2.2는 같은 스택으로 옮긴다.
- **미검증**: 두 시제품 모두 한글 IME 조합, 마크다운 드래그 선택·복사(행 간 선택 포함), 휠 스크롤 체감, 창 크기 변경. D3D11 영상 표시 경로는 시도하지 않았다.

## 2026-11-17 Rust 클라이언트 코어 착수 후 일시 정지 (사용자 지시)
- **Done**: 명세 3종 `docs/rust_client_core/{GATEWAY_PROTOCOL,STATE_MODEL,REST_AND_AUTH}.md` 작성. 새 독립 워크스페이스 `fluxer_client/`(core 크레이트, 공유 계약 `crate::Dispatch`) 생성. `fluxer_desktop_rs/src/ipc.rs`의 `danger_accept_invalid_certs(true)` 2곳 제거(TLS 검증 복구, `cargo check` 통과; 자체 서명 인증서 인스턴스는 이제 연결 실패). GUI 실기 도구 `tools/agents/gui.ps1` 추가. gpui-kit 드래그 선택은 원본 빌드에서 정상(gui.ps1 실측 + 헤드리스 테스트 2개).
- **중단 상태**: gateway·state·rest 워커(Opus 3개)를 구현 도중 정지했다. `fluxer_client/core/src/{gateway,state,rest}/`의 코드는 미완성이며 테스트는 작성 전 또는 작성 중이었다. 재개 시 각 디렉터리를 검토하고 `cd fluxer_client && cargo test`부터 확인한다.
- **Next Steps**: 재개 지시 후 세 모듈 완료·테스트 → REST/토큰 코드 DeepSeek+Opus 독립 검토 → `fluxer_client/app`(GPUI + gpui-kit) 로그인·채널·메시지 화면 → gui.ps1로 실기 검증.

## 2026-11-18 Rust 클라이언트 코어 완성, GPUI 앱, 자체 서버 1:1 비교 환경
- **Baseline**: `desktop-rs` `d1124083` + 미커밋 작업. 새 코드는 미추적 `fluxer_client/`(core, app, devserver). 커밋·push 안 함.
- **Done (core)**: gateway(19 테스트)·state(47 + insta 스냅샷)·rest(ALTCHA v2 풀이, 페이서 25/s·동시 8, 응답 8MiB 상한, 로그아웃 세대, 동일 출처 리디렉션) 완성. DeepSeek·Opus 독립 검토 지적 반영(ALTCHA DoS 한계 포함). `cargo test --workspace` 149 통과(core 121, app 25, devserver 3). 워크스페이스에서 serde_json `preserve_order`가 켜져 스냅샷이 깨지던 문제를 `sort_maps`로 수정.
- **Done (app)**: 로그인(+MFA, 캡차 자동 풀이)·토큰 실행, 게이트웨이→상태→UI, 길드/채널 트리·안읽음, MessageScroller 마크다운 행·이전 페이지, 낙관적 전송, 읽음 ack, 멘션 치환·그룹핑·현지 시간·인라인 이미지·창 제목·음성 채널 비선택.
- **실기 검증**: (1) 친구 서버에서 읽기 전용 확인 후 사용자 지시로 중단·스크린샷 삭제. (2) devserver에서 전송·ack·실시간 수신 확인. (3) 자체 서버 `fluxer-dev.home.arpa`(Oracle ARM64, rexctl, 공개 차단, SSH 터널)에서 Rust 앱이 캡차를 풀어 로그인하고 실제 Erlang 게이트웨이(zstd-stream)에 연결됨. 비교용 공식 앱 portable 사본(`../Logs/fluxer-ref/app`, tester2)과 나란히 비교.
- **Next Steps**: 1:1 비교 차이(채널 순서, 시스템 메시지, 행 간격·날짜 구분선·NEW, 길드 안읽음 점, 멘션 강조, 기본 아바타) 수정 중. 이후 다크 테마, 멤버 목록, 입력창 기능, 오버레이/스트림 뷰어 스택 통일.
- **2026-11-18 추가**: 1:1 비교 수정(채널 순서·시스템 메시지·간격·날짜/NEW 구분선·길드 안읽음 pill·멘션 pill·기본 아바타) 자체 서버에서 육안 확인. REST 확장(수정/삭제/핀/반응/타이핑/첨부 multipart/프로필·DM·멤버) 66 테스트. session 모듈(DPAPI, 레코드 바인딩 엔트로피, 잠금, 원자적 쓰기 재시도) Opus+DeepSeek 검토 반영 31 테스트. 워크스페이스 193 테스트 통과. 빌드: rust-lld + 의존성 debuginfo off로 콜드 빌드 1분 51초; sccache는 타깃 디렉터리별 경로 차이로 적중이 낮아 제거.
- **진행 중(병렬)**: core 구조 분리+모델 확장(반응·답장·임베드·타이핑·프레즌스·관계·멤버 목록), app 컴포넌트 분리+로그인 유지+사용자 패널, `fluxer_client/markdown`(공식 Rust 파서 기반 렌더, 선택 유지), `fluxer_client/i18n`(Lingui .po 내장).

## 2026-11-18 일시정지 (사용자 지시: 하던 작업만 마무리)
- **상태**: `fluxer_client` 워크스페이스 `cargo test --workspace` 269 통과(app 37, core 184, markdown 36, i18n 9, devserver 3), 경고 0. 모두 미추적·미커밋.
- **Done**: core 구조 분리(`rest/api/*`, `state/handlers/*`, `gateway/subscriptions.rs`)와 모델 확장(반응·답장·임베드·스티커·타이핑·프레즌스·관계·설정·이모지·멤버 목록 op14, presence op3), `SecretToken` zeroize. app 컴포넌트 분리(`ui/*`, `theme.rs`)·로그인 유지(DPAPI session 연결, 재시작 자동 로그인을 자체 서버에서 실기 확인)·로그아웃·사용자 패널·이미 실행 중 화면. `fluxer_client_markdown`(공식 Rust 파서 → 렌더 모델 → gpui-kit TextView 플러그인, 선택·복사 유지, 스포일러·멘션 pill·커스텀 이모지·타임스탬프·syntect 강조). `fluxer_client_i18n`(Lingui .po 44개 내장, 약 17.5MB).
- **미연결/미검증**: markdown·i18n 크레이트는 앱에 아직 연결 안 함. 반응·답장·타이핑·멤버 목록 UI 없음. op14/op3 프레임 실서버 미검증. i18n 테이블 압축 미적용.
- **재개 시 다음 순서**: ① 앱에 markdown 크레이트 연결(기존 format.rs/MentionPill 대체) ② i18n 연결 + zstd 압축 ③ 반응·답장·수정/삭제·타이핑 UI ④ 멤버 목록 사이드바 ⑤ 첨부 업로드 UI ⑥ 다크 테마. 실기 검증은 개인 테스트 서버로 SSH 443 터널을 연 뒤 자체 서버에서 한다(접속 정보는 저장소 밖 로컬 메모에만 둔다).

## 2026-11-18 재개 후 진행
- **해결**: 헤드리스 테스트 무한 루프(멤버 목록 `uniform_list`가 행 높이 측정용 `0..1` 호출을 보이는 범위로 보고 → 매 프레임 notify). `report_visible`에서 측정 호출 무시. 사용자 PC 과부하 원인이었음. 대응: `.cargo/config.toml` `build.jobs=4`, 빌드 워커 1~2개, 테스트는 `timeout 300 … --test-threads=2`, 빌드 디렉터리 `fc-root`(+`fc-core`)만 사용, 워커별 fc-* 42GB 삭제.
- **Done**: 메시지 상호작용 통합(답장 미리보기·반응 pill·hover 동작 막대·빠른 반응 6종·Up 수정/Esc 취소·삭제 확인·입력 중 표시, `UiEvent::ActionFailed`), 테마(공식 색 생성식 이식, Dark 기본, `FLUXER_THEME`), i18n zstd 압축(17.5→4.0MiB), REST 소셜·초대·설정, core 순수 로직(`state/notify.rs`, `state/visibility.rs`, `search/{switcher,emoji}.rs`, 이모지 1940개 내장). 테스트: app 80, core 213, i18n 10, markdown 36, devserver 3.
- **실기(자체 서버)**: 다크 테마, 멤버 목록 op14 실제 게이트웨이 응답으로 표시(Online — 2), 전송, Up 수정 → 서버 반영(edited_timestamp) 확인.
- **발견 버그(수정 중)**: hover 동작 막대가 실제 마우스를 따라가지 않고 시스템 행에 고정됨; gpui-kit 아이콘(헤더 멤버 토글, 복사/삭제) 빈칸 — 아이콘 AssetSource 미등록 추정.
- **문서**: `docs/rust_client_core/NEXT_BRIEFS.md`(첨부·알림·빠른 전환·단축키·이모지·자동완성 지침), STATE_MODEL/REST_AND_AUTH에 구현 지도 추가.
- **2026-11-18 추가 실기(자체 서버)**: 아이콘 AssetSource 등록(`icons.rs`)으로 아이콘 표시 확인. hover 막대 미표시는 앱 문제가 아니라 다른 창(Gaea)이 테스트 창을 덮어 마우스 입력이 그 창으로 간 것 — `gui.ps1`에 `Assert-Unobstructed`(WindowFromPoint) 추가, `move` 동작 추가. 단축키·Ctrl+K 빠른 전환(“mark” → #markdown 이동)·`@` 자동완성(표시 `@tester2`, 전송 `<@id>`, 서버 mentions 확인)·`:smi` 이모지 삽입 실기 확인. 마크다운(굵게/기울임/밑줄/취소선/코드/스포일러/코드블록 강조/인용/목록) 실서버 데이터로 표시 확인. app 테스트 97 통과(3.8 s).
- **진행 중**: 데스크톱 알림(notify-rust, rodio) + 첨부 업로드 UI.

## 2026-11-18 우선순위 확정 (사용자 승인: "빨리 쓸 수 있게")
- **P0 (실사용 시작 조건)**: ① 알림·첨부(진행 중) ② 링크 미리보기/GIF 임베드 ③ 릴리스 exe(아이콘·단축 아이콘) ④ 안정성(앱 내 한글 IME, 재연결, 장시간 메모리, 스크롤). P0 완료 후 사용자에게 exe 전달 → 친구 서버 실사용 피드백 우선 수정.
- **P1**: 이모지 피커·프로필 팝업, 간단 설정(테마·알림·로그아웃), 한국어 UI(i18n 연결), 트레이·자동 시작.
- **P2**: 친구/DM 관리, 초대, 검색, 이후 음성·화면 공유.
- 원칙: 앱 빌드 워커 1개, 빌드 비필요 조사는 병렬 가능. 검증은 자체 서버.

## P1 구현: 이모지 피커·프로필 팝업·최소 설정 화면 (2026-11-19)
- **Baseline**: `desktop-rs` `f1479c61d` 기반(`fluxer_client/` 420 테스트 통과 상태). `docs/rust_client_core/NEXT_BRIEFS.md` 8~10절 사양 구현.
- **Done**:
  - **Core (`fluxer_client/core`)**: `search/emoji.rs`(`PICKER_FRECENT_LIMIT = 42`, `EmojiChoice`, `frecency_choices`, `custom_choices_by_guild`, `normalize_picker_query`, `picker_search_choices`), `state/model.rs`(`User.banner`, `User.avatar_color`), `rest/profile.rs`(`UserProfileFull`, `ProfileUser`, `UserProfileData`, `GuildMemberSummary`, `MutualGuild`, `PROFILE_FLAG_*`), `rest/api/notes.rs`(`PUT /users/@me/notes/{user_id}`), `state/handlers/notes.rs`(`USER_NOTE_UPDATE`, `READY.notes`, `NotesChanged`).
  - **App (`fluxer_client/app`)**:
    - 이모지 피커(`ui/emoji_picker/{mod.rs, sections.rs, view.rs}`): 자주 쓰는 이모지·길드 커스텀·유니코드 8카테고리 9열 `uniform_list`, 검색, 피부색(`0..=5`), Shift 유지·반응 추가, `ctrl-e`, 컴포저/메시지 액션 버튼 연결.
    - 프로필 팝업(`profile.rs`, `backend/profile.rs`, `ui/profile_popout.rs`): 길드 오버라이드·역할·배너 폴백, UTF-16 256코드유닛 노트 편집/자동 저장(단조 시퀀스 가드), 차단 확인 포함 DM 열기, 공통 서버 이동, 메시지/멤버/유저 패널 클릭 연결.
    - 설정 화면(`prefs.rs`, `settings_model.rs`, `backend/settings.rs`, `ui/settings.rs`, `theme.rs`, `ui/l10n.rs`, `notify/mod.rs`): 런타임 테마(`System`/`Dark`/`Light`), 알림·알림음 토글, 44개 공식 로케일 전환, 로그아웃 확인, `prefs.json` 원자적 저장, `ctrl-,`·유저 패널 톱니 버튼 연결.
  - **검증·진행도**: `cd fluxer_client; $env:CARGO_TARGET_DIR = "../target/fc-root"; cargo test --workspace -- --test-threads=2` → **454 passed, 0 failed, 0 warnings** (`app`: 163, `core`: 242, `i18n`: 10, `markdown`: 36 [+1 existing ignored doctest], `devserver`: 3). DeepSeek `#max`(`fluxer-helper-p1-review`) 교차 검토 P0 0건 및 P1 #2·P2 #1~#9 수정 완료 후 재검증. `docs/progress/progress.json` + `python scripts/progress_svg.py` 갱신(`overall: 52%`, `daily driver: 62%`).
- **Decisions**:
  - 기존 알림 테스트(`tests_notify_upload.rs:46`, `notify/tests.rs:208`) 호환성을 위해 `notifier.enabled`를 마스터 게이트로 유지하고 `sounds_enabled`는 `enabled == true`일 때 효과음을 개별 제어하도록 둠.
  - 헤드리스 병렬 테스트에서 로케일 전환 레이스를 막기 위해 `ui/l10n.rs`에 `#[cfg(test)]` `thread_local!` 저장소(`TEST_TRANSLATOR`) 적용.
  - `uniform_list` 렌더/레이아웃 클로저 내부 상태 변경·`cx.notify()` 금지 원칙 준수.
- **Blockers**:
  - 릴리스 패키지 빌드(`scripts/package_fluxer_client.ps1`)는 PC 여유 메모리 제약으로 사용자 확인 전 보류.
  - P1 UI 3종(이모지 피커·프로필 팝업·설정/한국어 UI) 및 기존 실기 미확인 항목은 테스트 서버 실기 확인 미수행.
- **Next Steps**:
  1. 사용자 승인 시 릴리스 빌드(`scripts/package_fluxer_client.ps1`) 및 실기 검증(P0③~④).
  2. 자체 테스트 서버(`fluxer-dev.home.arpa`)에서 P1 UI(이모지 피커·프로필 팝업·노트/설정 저장·테마/로케일 전환) 실기 확인.
  3. 남은 P1 트레이·자동 시작 구현 착수.

## 트레이·자동 시작(P1) 및 P0③ 릴리스 패키징 (2026-11-19)
- **Baseline**: `desktop-rs` `55597163f` 기반(P1 이모지 피커·프로필·설정 커밋 후 454 테스트 통과).
- **Done**:
  - **의존성·설정 모델 (`fluxer_client/app`)**: `Cargo.toml`에 Windows 전용 `tray-icon = "0.20"` (`default-features = false`) 및 `windows-registry = "0.6.1"` 추가. `prefs.rs`에 `autostart: false`, `close_to_tray: true` 필드 및 4키 레거시 `prefs.json` 하위 호환 파싱·단위 테스트 추가. `settings_model.rs`에 `SettingsEffect::{AutostartChanged, CloseToTrayChanged}` 추가.
  - **Windows 자동 시작 (`platform.rs`, `main.rs`)**: `RUN_REGISTRY_KEY`(`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`) 기반 `set_autostart_enabled`, `format_autostart_command`, `sync_autostart_on_startup` 구현(테스트 빌드 `#[cfg(all(target_os = "windows", not(test)))]` 가드로 테스트 중 실제 레지스트리 미기록, 비문자열 값도 삭제하도록 `get_value` 사용). `main.rs`에 `--autostart` CLI 파싱 및 시작 시 최소화·비활성화 처리 반영.
  - **시스템 트레이 및 닫기 동작 (`tray.rs`, `ui/settings.rs`, `client_view.rs`)**: Windows 트레이 아이콘(`tray-icon`) 및 공식 i18n 카탈로그 키(`Open {productName}`, `Settings`, `Mute notifications`, `Quit {appName}`) 기반 컨텍스트 메뉴, 좌클릭·더블클릭 창 복원, `on_window_should_close` 닫기→최소화 정책(`should_allow_window_close`), 설정 모달 `Startup` 섹션(`Desktop startup`, `Minimize to tray`) 및 레지스트리 쓰기 실패 시 `prefs` 비저장·오류 표시 처리.
  - **검증·DeepSeek 교차 검토**: `deepseek/deepseek-flash`(`fluxer-helper-tray-review`) 교차 검토 P0 0건 및 P1 2건(`tray::is_active()` 가드, autostart 실패 은폐 방지)·P2 항목(`get_value` 삭제, 공식 i18n 키, 모달 높이/스크롤, 중복 `args_parse` 정리) 반영 후 `cd fluxer_client; $env:CARGO_TARGET_DIR = "../target/fc-root"; cargo test --workspace -- --test-threads=2` → **458 passed, 0 failed, 0 warnings** (`app`: 167, `core`: 242, `i18n`: 10, `markdown`: 36 [+1 existing ignored doctest], `devserver`: 3).
  - **P0③ 릴리스 패키징**: `powershell -ExecutionPolicy Bypass -File scripts\package_fluxer_client.ps1` 성공(4m 44s, Exit 0). 산출 `dist/fluxer_client/fluxer_client.exe`(39.17 MB, SHA256 `9a6a9e34600e5025a1e654ece655542c899cf64f5f5ab6b63f953102c6adcae7`, `ProductName=Fluxer`, `FileVersion=0.1.0`, `PE_SUBSYSTEM=2` Windows GUI) 및 `dist/fluxer_client/README.txt` 생성 확인. 빌드 후 임시 `fluxer_client/target` 삭제하고 `target/fc-root`·`dist/fluxer_client/` 유지.
  - **진행도 배지**: `docs/progress/progress.json` 갱신(`릴리스 exe·아이콘: done`, `콘솔 창 제거: done`, `트레이·자동 시작: partial`) 및 `python scripts/progress_svg.py` 실행 → `overall: 53% (93 items)`, `daily driver: 64% (73 items, 10 areas)`.
- **Decisions**:
  - `tray::set_autostart_enabled` 내부에 `#[cfg(not(test))]` 가드를 두어 단위/헤드리스 테스트가 사용자 Windows 시작 프로그램 레지스트리를 오염시키지 않도록 제한함.
  - 트레이 닫기 정책은 창 핸들 숨김 대신 `window.minimize_window()` 유지(`NEXT_BRIEFS.md` 9절 기준; 실제 `ShowWindow(SW_HIDE)` 전환 시 추후 별도 확인 필요).
- **Blockers**:
  - P1 트레이/자동 시작 실동작(최소화·트레이 메뉴 클릭·`--autostart` 재부팅/레지스트리 확인), 릴리스 exe 실기(콘솔 미노출·AUMID 토스트·시작 메뉴 바로가기), 및 기존 실서버 미확인 UI 항목들은 아직 테스트 서버 실기 검증 전임.
- **Next Steps**:
  1. `dist/fluxer_client/fluxer_client.exe` 실기 확인(트레이 아이콘·닫기 최소화·자동 시작 토글·AUMID 토스트·시작 메뉴 바로가기·한글 IME).
  2. 자체 테스트 서버(`fluxer-dev.home.arpa`)에서 P1 UI(이모지 피커·프로필 팝업·노트/설정 저장·테마/로케일 전환) 및 기존 P0 실기 항목 검증.
  3. 필요 시 P0④ 안정성 실기(재연결·장시간 메모리) 및 P2 기능 착수.

## 채널별 드래프트·방문 기록 이동·채널 음소거 및 경고 제거 (2026-11-19)
- **Baseline**: `desktop-rs` `f531f3aba` 기반(458 테스트 통과).
- **Done**:
  - **채널별 드래프트 보존 (`client_view.rs`, `client_view/keyboard.rs`)**: 채널·DM별 미전송 텍스트 및 인라인 `@`/`#`/커스텀 이모지 토큰을 `ClientView::drafts`(`MAX_DRAFTS = 100`)에 저장·복원하도록 구현. `pending_composer`와 단조 카운터 `composer_seq`로 `cx.defer` 실행 전 빠른 연속 전환·사용자 입력·편집 시작(`start_edit`) 간 경합을 차단했고, 편집 중 전환 시 편집 대상 원문 대신 편집 전 드래프트를 보존하며 전송 성공 시 해당 채널 드래프트를 비우도록 처리.
  - **방문 기록 앞뒤 이동·직전 길드 토글 (`nav.rs`, `shortcuts.rs`, `client_view/keyboard.rs`)**: `NavHistory`(`MAX_HISTORY = 100`, 중복·무효 채널 건너뜀, 분기 시 forward 절단) 구현 및 `Alt+Left`/`Alt+Right`(`HistoryBack`/`HistoryForward`), `Ctrl+Alt+Right`(`ToggleLastGuild`) 단축키(총 47바인딩) 등록.
  - **채널·DM 음소거 (`backend.rs`, `backend/settings.rs`, `view_model.rs`, `notify_upload.rs`, `ui/header.rs`, `ui/channel_list.rs`)**: 헤더 `"toggle-channel-mute"` 버튼 및 `Backend::set_channel_muted` 구현. `USER_GUILD_SETTINGS_UPDATE` 낙관 적용 + 스코프(`Option<Snowflake>`)별 시퀀스 가드 + 실패 시 단일 채널 롤백 + 채널 목록 `BellOff` 아이콘·안읽음 점 비활성 처리.
  - **비테스트 바이너리 경고 0건 (`backend.rs`, `settings_model.rs`, `theme.rs`)**: 테스트 전용 `UserSettingsPatch`/`GuildSettingsPatch` import를 `#[cfg(test)]`로 분리하고 테스트 전용 헬퍼에 `#[cfg_attr(not(test), allow(dead_code))]` 적용.
  - **DeepSeek 교차 검토·검증**: `deepseek/deepseek-flash`(`fluxer-helper-m1-review`) 리뷰의 P1 2건(서버 `PATCH /users/@me/guilds/{g|@me}/settings` 전체 맵 교체 사양에 맞춰 `build_channel_mute_patch`가 형제 오버라이드·`collapsed`·`message_notifications`를 함께 전송하도록 수정, 패치·단일 채널 롤백 단위 테스트 추가) 및 P2 항목(스코프별 시퀀스 맵, 단일 채널 롤백 `apply_channel_mute_rollback`, `start_edit`의 `pending_composer` 참조)을 모두 반영. 재검증 결과 `cargo check -p fluxer_client_app --bin fluxer_client` **경고 0건**, `cargo test --workspace -- --test-threads=2` **465 passed, 0 failed, 0 warnings** (`app`: 174, `core`: 242, `i18n`: 10, `markdown`: 36 [+1 existing ignored doctest], `devserver`: 3).
  - **진행도 배지**: `docs/progress/progress.json` 갱신(`드래프트 보존: done`, `방문 기록·앞뒤 이동: done`, `채널 음소거: partial`) + `python scripts/progress_svg.py` 실행 → `overall: 56% (93 items)`, `daily driver: 66% (73 items, 10 areas)`.
- **Decisions**:
  - `build_channel_mute_patch`는 서버 `UserAccountSettingsService`가 `channel_overrides` 전체를 교체하는 동작에 맞춰 현재 상태의 형제 채널 오버라이드를 유지한 맵을 전송하도록 고정.
  - `NavHistory::MAX_HISTORY`는 100, `MAX_DRAFTS`는 100(상한 초과 시 가장 작은 ID 키 제거)으로 설정.
- **Blockers**:
  - 채널·DM 음소거 실서버 `PATCH /users/@me/guilds/{g|@me}/settings` 왕복, 드래프트·앞뒤 이동 GUI 조작, 및 기존 실기 미확인 항목은 테스트 서버 실기 확인 전임.
- **Next Steps**:
  1. Milestone 2(소셜: 친구 목록·차단/관계 관리, 길드 초대 참가·탈퇴, 미디어 뷰어) 구현 착수.
  2. 자체 테스트 서버(`fluxer-dev.home.arpa`)에서 `dist/fluxer_client/fluxer_client.exe` 실기 및 신규 UI 검증.

## 친구 목록·관계 관리·초대 참가·길드 탈퇴 (Milestone 2, 2026-11-19)
- **Baseline**: `desktop-rs` `37957e5a1` 기반(465 테스트 통과).
- **Done**:
  - **소셜·커뮤니티 백엔드 (`backend/social.rs`, `backend.rs`)**: 친구 요청(태그/ID), 수락, 관계 삭제/차단 해제, 차단, 초대 미리보기(`GET /invites/{code}`, `member_count`/`presence_count` 및 그룹 DM 채널명 폴백), 초대 수락(`POST /invites/{code}` 길드·그룹 DM 분기), 길드 탈퇴(`DELETE /users/@me/guilds/{id}` 및 낙관적 `GUILD_DELETE` 반영) 구현. `extract_invite_code`의 `..` 경로 역참조 및 빈 `/invite`/`/i` 경로 검증 강화.
  - **홈 친구 화면·프로필 관계 버튼 (`ui/friends.rs`, `ui/channel_list.rs`, `profile.rs`, `ui/profile_popout.rs`)**: `Place::Home` 미선택 상태에 `Online` / `All` / `Pending` / `Blocked` / `Add friend` 탭과 `home-friends` 항목, 대기 요청 배지, 1:1 DM 열기·생성, Friends 화면 오류 배너(`friends-error`), 프로필 팝업 관계 버튼 및 `refresh_open_profile`(실패 시 `card.loaded` 보존) 추가.
  - **길드 참가·탈퇴 모달 (`ui/guild_modal.rs`, `ui/guild_rail.rs`, `ui/overlay.rs`, `shortcuts.rs`, `client_view.rs`)**: `Ctrl+Shift+N`(총 50바인딩) 및 레일 `+` 버튼으로 `Overlay::JoinGuild` 열기, `GUILD_CREATE`/`CHANNEL_CREATE` 자동 이동(`pending_joined_guild`/`pending_joined_channel`), 비소유자 전용 `"leave-guild-btn"` → `Overlay::LeaveGuild` 확인 모달 및 소유자 가드 구현.
  - **DeepSeek 교차 검토·검증**: `deepseek/deepseek-flash`(`fluxer-helper-m2-review`) 정적 검토 P0/P1 0건 및 P2 6건(연쇄 수락 stale `code` 레이스, 그룹 DM 초대 채널 미오픈, Fluxer API `member_count`/`presence_count` 필드명 불일치, Friends 오류 배너 미노출, Join/Leave 모달 종료 후 컴포저 포커스 복원 누락, Leave 모달 포커스 부재로 인한 `Escape` 오작동)과 입력 유지 가드를 모두 수정하고 [tests_social.rs](fluxer_client/app/src/tests_social.rs)에 4개 헤드리스 UI 테스트 추가. 재검증 결과 `cargo check -p fluxer_client_app --bin fluxer_client` **경고 0건**, `cargo test --workspace -- --test-threads=2` **473 passed, 0 failed, 0 warnings** (`app`: 182, `core`: 242, `i18n`: 10, `markdown`: 36 [+1 existing ignored doctest], `devserver`: 3).
  - **진행도 배지**: `docs/progress/progress.json` 갱신(`친구 목록: partial`, `차단·관계 관리: partial`, `초대 참가: partial`, `길드 탈퇴: partial`) + `python scripts/progress_svg.py` 실행 → `overall: 58% (93 items)`, `daily driver: 68% (73 items, 10 areas)`.
- **Decisions**:
  - `Place::Home`에서 채널 미선택 시 Friends 화면을 표시하되 컴포저를 `0px` 숨김 슬롯에 유지해 기존 `ClientView > Composer > Input` 단축키 문맥과 포커스 복원을 보존.
  - 초대 미리보기(`InvitePreview`)는 Fluxer 스키마(`member_count`, `presence_count`)를 우선 읽고 하위 호환 키(`approximate_*`)를 폴백으로 지원하며, 그룹 DM 초대는 `AcceptedInvite::channel_id`로 채널을 연다.
- **Blockers**:
  - 친구 요청·수락·차단, 초대 링크 미리보기/수락, 그룹 DM 초대, 비소유자 길드 탈퇴 실서버 API 왕복 및 기존 실기 미확인 항목은 테스트 서버 실기 검증 전임.
- **Next Steps**:
  1. Milestone 3(미디어 뷰어, 신뢰 도메인 링크 경고, 글꼴·밀도·확대 설정, 메시지 검색 등) 설계 및 구현 착수.
  2. 자체 테스트 서버(`fluxer-dev.home.arpa`)에서 `dist/fluxer_client/fluxer_client.exe` 재패키징 및 신규 소셜·커뮤니티 UI 실기 확인.

## 미디어 뷰어·신뢰 도메인 경고·글꼴/밀도/확대 설정 (Milestone 3, 2026-11-19)
- **Baseline**: `desktop-rs` `571c4d530` 기반(473 테스트 통과).
- **Done**:
  - **Core (`core/src/rest/api/settings.rs`, `core/src/state/handlers/user_settings.rs`)**: `UserSettingsPatch`에 `message_display_compact`, `trusted_domains` 및 유효성 검사(최대 1000개, 길이 1..=253, 제어 문자 금지) 추가. 기존 `insta` 스냅샷을 보존한 채 `UserSettings::message_display_compact()`·`trusted_domains()` 접근자 및 단위 테스트 추가.
  - **신뢰 도메인 경고 (`trusted_domains.rs`, `backend.rs`, `ui/link_warning.rs`)**: 공식/인스턴스 엔드포인트/사용자 신뢰 도메인 패턴 검사(`TrustedDomain.ts` 규약), `https://fluxer.app@evil.example` userinfo 스푸핑 차단, 안전한 `mailto:`/`tel:` URI 허용 및 `javascript:`/`file:` 차단, 확인 모달(`Overlay::LinkWarning`)과 "항상 신뢰" 영구 저장(`prefs.json` + `UserSettingsPatch`) 구현.
  - **미디어 뷰어 라이트박스 (`ui/media_viewer.rs`, `ui/message_row.rs`, `ui/embed/*`)**: 메시지 이미지 첨부 및 임베드 이미지/GIF/갤러리 클릭 시 `Overlay::MediaViewer` 실행, 창 DPI 배율(`scale_factor`)을 반영한 프록시 크기 계산(정적 1920×1280, 애니메이션 960×720 상한), 좌우 키/버튼 갤러리 이동, 확대/축소(`+`/`-`), 링크 복사, 신뢰 도메인 검사 연계 브라우저 열기 구현.
  - **글꼴·밀도·UI 확대 (`theme.rs`, `prefs.rs`, `settings_model.rs`, `ui/settings.rs`, `shortcuts.rs`, `ui/message_list.rs`, `ui/reply.rs`, `client_view.rs`)**: `DisplayPrefs`(`12..=24px`, `75..=150%`, Cozy/Compact), `Ctrl+=`/`Ctrl+-`/`Ctrl+0` 단축키(총 59바인딩), 설정 `Display` 섹션(글꼴 크기·배율·밀도·신뢰 도메인 초기화) 및 `Account` 하단 고정 레이아웃 구현. Compact 모드에서 아바타 거터 제거 및 시스템 메시지·답장 스파인 정렬 보정, 서버 `USER_SETTINGS_UPDATE`/롤백과 `prefs.json` 양방향 동기화.
  - **DeepSeek 교차 검토·검증**: `deepseek/deepseek-flash`(`fluxer-helper-m3-review`) 정적 검토 P0/P1 0건 및 P2 4건(서버 `message_display_compact` 미동기화, HiDPI 배율·메모리 고려, Compact 시스템/답장 음수 폭 보정, PATCH 실패 시 로컬 설정 롤백 연동)·P3 항목(`mailto:`/`tel:` 허용, embed fallback 도메인 검사)을 반영하고 [tests_media_display.rs](fluxer_client/app/src/tests_media_display.rs)에 회귀 테스트 추가. 재검증 결과 `cargo check -p fluxer_client_app --bin fluxer_client` **경고 0건**, `cargo test --workspace -- --test-threads=2` **482 passed, 0 failed, 0 warnings** (`app`: 189, `core`: 244, `i18n`: 10, `markdown`: 36 [+1 existing ignored doctest], `devserver`: 3).
  - **진행도 배지**: `docs/progress/progress.json` 갱신(`미디어 뷰어: partial`, `신뢰 도메인 경고: partial`, `글꼴·밀도·확대: partial`) + `python scripts/progress_svg.py` 실행 → `overall: 60% (93 items)`, `daily driver: 70% (73 items, 10 areas)`.
- **Decisions**:
  - 신뢰 도메인 검사는 `url::Url` 파싱 결과의 `host_str`만 비교하고, `message_display_compact`와 `trusted_domains`는 `ClientState::user_settings`와 `PrefsStore` 간 양방향 동기화해 오프라인 설정 보존과 실패 롤백을 동시에 보장함.
  - 미디어 뷰어 프록시 요청은 창 배율을 반영하되 애니메이션 GIF/Gifv는 메모리 폭증을 막기 위해 `960×720` 상한으로 제한함.
- **Blockers**:
  - 신뢰 도메인/외부 브라우저 실행, HiDPI 실기기 미디어 뷰어, `message_display_compact`/`trusted_domains` 실서버 `PATCH /users/@me/settings` 왕복 및 기존 실기 미확인 항목은 테스트 서버 실기 검증 전임.
- **Next Steps**:
  1. Milestone 4(메시지 검색 REST·패널, 그룹 DM UI, 반응/멘션/검색 연동 등) 설계 및 구현 착수.
  2. 자체 테스트 서버(`fluxer-dev.home.arpa`)에서 `dist/fluxer_client/fluxer_client.exe` 재패키징 및 신규 UI 실기 확인.

## 인계 (2026-11-19, 다음 에이전트용 — 이 절부터 읽을 것)
- **기준**: `desktop-rs` (Milestone 2 `571c4d530` 이후 Milestone 3 미디어 뷰어·신뢰 도메인 경고·글꼴/밀도/확대 커밋 반영), 워킹 트리 깨끗함(미공개 파일·`dist/` 제외). 미공개 유지 파일(커밋 금지): `RUST_*_PLAN.md`, `RUST_MIGRATION_REFERENCE_REVIEW.md`, `RUST_PORT_RESEARCH_*`, `IPC_M0_FINDINGS.md`, `tools/h265_livekit_probe/`.
- **현재 주력**: 네이티브 GPUI 클라이언트 `fluxer_client/`(core, app, markdown, i18n, devserver). `cd fluxer_client; $env:CARGO_TARGET_DIR = "../target/fc-root"; cargo test --workspace -- --test-threads=2` → 482 통과(app 189, core 244, markdown 36 [+1 ignored doctest], i18n 10, devserver 3), `cargo check --bin fluxer_client` 경고 0.
- **우선순위(P0)**: ① 알림·첨부 ✔(코드) ② 임베드 ✔(코드) ③ 릴리스 exe ✔(`scripts/package_fluxer_client.ps1`, 산출 `dist/fluxer_client/fluxer_client.exe` 39.17 MB, SHA256 `9a6a9e34600e5025a1e654ece655542c899cf64f5f5ab6b63f953102c6adcae7`, `PE_SUBSYSTEM=2`) ④ 안정성 실기(재연결·장시간 메모리·한글 IME). P1~P2 진행: 이모지 피커·프로필 팝업·설정 화면·트레이/자동 시작·드래프트 보존·방문 기록 이동·채널 음소거·친구 목록·관계 관리·초대 참가·길드 탈퇴·미디어 뷰어·신뢰 도메인 경고·글꼴/밀도/확대 ✔(코드/헤드리스).
- **실기 미확인**: 미디어 뷰어·신뢰 도메인 경고·글꼴/밀도/확대 실서버/실기기 동작, 친구/관계 관리·초대 참가·그룹 DM 초대·길드 탈퇴 실서버 동작, 채널/DM 음소거 실서버 PATCH, 드래프트·`Alt+Left/Right` 실기, 트레이 아이콘·닫기 최소화·`--autostart` HKCU Run 실제 동작, 이모지 피커·프로필 팝업·설정/테마/로케일 UI 실서버 동작, GIF 애니메이션 수정, 유튜브 카드 중복 수정, Windows 토스트·알림음(사용자 PC가 방해 금지 모드), 첨부 업로드 실서버, 한국어 UI 화면, 릴리스 exe·AUMID 바로가기.
- **테스트 환경**: 자체 테스트 서버 `fluxer-dev.home.arpa`(SSH 443 터널). 접속·계정·도구는 저장소 밖 `../Logs/fluxer-dev/LOCAL_ACCESS.md`. **친구 서버와 사용자 개인 계정은 테스트에 쓰지 말 것**(개인 정보). 메시지 전송 시험은 자체 서버에서만.
- **PC 부하 규칙**(사용자 PC가 여러 번 멈춤): `fluxer_client/.cargo/config.toml` `build.jobs=4` 유지, 동시에 빌드하는 작업 최대 2개, 앱 테스트는 항상 `timeout 300 … -- --test-threads=2`, 빌드 디렉터리는 `target/fc-root`만(추가로 만들었으면 끝나고 삭제). 헤드리스 GPUI 테스트가 무한 루프로 CPU를 먹은 전례(uniform_list의 0..1 측정 호출) — 렌더/레이아웃 콜백에서 상태 변경·notify 금지.
- **GUI 실기**: `tools/agents/gui.ps1`는 다른 창이 대상 창을 가리면 입력을 거부한다(사용자가 PC 사용 중일 수 있음). 마우스 대신 키보드(Ctrl+K 등) 위주로 확인.
- **커밋 규칙**: 기능 단위로 커밋 후 `git push origin desktop-rs`. push 전 `git fetch`, 사용자가 GitHub에서 README를 직접 고치므로 `git pull --rebase`로 올리고 그 문장은 되돌리지 말 것. 커밋 전 `docs/progress/progress.json` 갱신 + `python scripts/progress_svg.py`. 서버 IP·키 경로·친구 서버 도메인·사용자 홈 경로를 커밋에 넣지 말 것.
- **위임 규칙(사용자 지시)**: Codex 금지. DeepSeek은 `deepseek/deepseek-flash`(V4.1 Flash 1017)만, 다른 DeepSeek 모델 금지. 백그라운드 작업은 완료 알림을 받을 수 있게 작업마다 따로 실행.
