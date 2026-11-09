# PROGRESS

## 워크트리 및 Git 형상 (Baseline)
- **작업 경로**: `C:\Users\k3832\orca\projects\fluxer`
- **현재 브랜치**: `desktop-rs` (최신 커밋 직전: `356bd77f`, 마일스톤 4 완수 커밋 반영 예정)
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
1. **마일스톤 4: 실시간 미디어 및 최종 검증 (Real-Time Media & Final Verification) 완료**:
   - **`win-process-loopback` 크레이트 N-API 완전 디커플링 ([`win-process-loopback`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop/native/win-process-loopback))**:
     - `node` feature(`default = ["node"]`)를 신설하고 `napi`, `napi-derive`, `napi-build`를 optional 의존성으로 전환하여 기존 Electron 빌드 100% 호환 보존.
     - `Inner` 및 `platform` 모듈 내부에서 N-API 의존성을 완전히 제거하고, 독립 Safe Rust `LoopbackError` 및 `LoopbackResult` 타입 도입.
     - `CallbackBackend::Rust` 및 순수 Rust Safe 인터페이스(`ProcessLoopback::new_with_sink`, `start`, `stop`, `dispose`, `set_endpoints`, `list_playback_devices_sync`) 노출.
     - Node feature On/Off 양방향 컴파일 0 오류 달성.
   - **`sample-pool` 제로카피 안전 슬라이스 노출 ([`sample-pool`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop/native/sample-pool))**:
     - `PooledSamples`에 불필요한 복사 없이 안전하게 메모리를 읽을 수 있는 `as_slice(&self) -> &[T]` 및 `as_mut_slice(&mut self) -> &mut [T]` API 추가.
   - **WASAPI 루프백 및 오디오 서브시스템 실체 직결 ([`audio.rs`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/src/audio.rs), [`ipc.rs`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/src/ipc.rs))**:
     - `AudioManager` 신설 및 `nativeAudio` 7종 IPC API(`getAvailability`, `listAudibleApplications`, `resolveAudioRootPid`, `listPlaybackDevices`, `start`, `stop`, `setWinEndpoints`) 완전 실체 직결.
     - 시스템 전체 오디오(session-mixer/process-exclude) 및 개별 프로세스 오디오 캡처 수명주기 완벽 연동.
     - 실제 Windows 오디오 COM 엔드포인트를 열거하는 `list_playback_devices_sync` 실기 연동 검증.
   - **프론트엔드 WebRTC/LiveKit 오디오 입력 파이프라인 직결 ([`bridge.rs`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/src/bridge.rs), [`main.rs`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/src/main.rs))**:
     - 기존 더미 빈 함수였던 `nativeAudio.onFrame`과 `nativeAudio.onEnd`를 실시간 이벤트 리스너 파이프라인으로 구현.
     - Rust 호스트의 WASAPI 실시간 PCM 프레임을 Base64 인코딩 후 `window.__fluxerNativeAudioFrame`을 통해 브라우저 `ArrayBuffer`로 즉각 변환하여 LiveKit/WebRTC 클라이언트에 공급.
     - `window.__fluxerNativeAudioEnd`를 통해 세션 종료/에러 사유 전달.
   - **`ts-rs` 5종 신규 계약 정의 추가 (총 26종 계약 자동 추출 완료)**:
     - `NativeAudioCapabilities.ts`, `NativeAudioAvailability.ts`, `NativeAudioStartOptions.ts`, `NativeAudioStartResult.ts`, `NativeAudioEndMessage.ts` 추가.
   - **회귀 검증 및 프로덕션 릴리스 바이너리 빌드**:
     - 단위 테스트 74개 전수 통과 (74 passed, 0 failed, Exit 0).
     - `cargo clippy -p fluxer_desktop_rs -- -D warnings` 경고 0건 통과.
     - `target/release/fluxer.exe` 릴리스 바이너리 빌드 완료 (14.8 MB / 14,804,480 bytes, Exit 0).

2. **마일스톤 3: 저장 및 입력 (Storage & Input) 완료**:
   - WebView2 V8 Fatal Out of Memory 크래시 원천 퇴치 (`--max-old-space-size=2048` 정상화 및 버퍼링 폭증 플래그 배제).
   - `windows-input-hook` 크레이트 N-API 완전 디커플링 및 저수준 PTT/전역 단축키 이벤트 매니저 연동.
   - 번들 `rusqlite` RowStore 및 AccountEngine 8종 수명주기 메서드 직결.

3. **마일스톤 2: 저위험 직결과 시스템 서비스 완료**:
   - `win-shell` N-API 디커플링 및 자동 시작/바로가기 동기화.
   - `ITaskbarList3` 작업표시줄 배지 및 `flashFrame` 연동.
   - `notify` 기반 150ms 디바운스 테마 파일 감시자 연동.

4. **마일스톤 1: 기준선과 계약 완료**:
   - `IPC_CONTRACT_BASELINE.md` 전수 감사 및 `ts-rs` 계약 추출 파이프라인 확립.

## 핵심 결정 사항 (Decisions)
- **WASAPI 루프백 PCM 실시간 디스패치 아키텍처**:
  - `win-process-loopback`의 실시간 스레드로부터 발생한 `&[f32]` 슬라이스를 `size_of_val` 기반 제로카피 바이트 슬라이스로 참조 후 Base64 변환하여 WebView2의 `ArrayBuffer`로 복원, WebRTC/LiveKit의 규격과 100% 일치하도록 보장함.
- **Node/Rust 듀얼 호환 Feature Gating 패턴**:
  - `win-shell`, `windows-input-hook`, `win-process-loopback` 3대 네이티브 크레이트 모두 `default = ["node"]` 구조를 채택하여 원본 Electron 빌드를 단 1줄의 손상 없이 보존하고, 순수 Rust 호스트에서는 `default-features = false`로 무설치 네이티브 직결을 달성함.

## 결함 및 부채 (Blockers / Tech Debt)
- 없음. 마일스톤 1~4 전 단계 100% 완수 및 회귀 테스트 0 결함 달성.

## 다음 작업 (Next Steps)
- 마일스톤 4 완료 커밋 생성 및 `origin/desktop-rs` 리모트 푸시.
- 사용자의 실기 구동 및 화면 공유/음성 통화 통합 사용 모니터링 지원.
