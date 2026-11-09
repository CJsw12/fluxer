# Fluxer IPC 계약 및 네이티브 구현 기준선 (IPC_CONTRACT_BASELINE.md)

기준 커밋: `desktop-rs` 브랜치  
대상 프로젝트: `fluxer_desktop_rs`, `packages/desktop_ipc`, `fluxer_desktop`

---

## 1. 개요 및 목적

본 문서는 [`NATIVE_MIGRATION_PLAN.md`](file:///C:/Users/k3832/orca/projects/fluxer/NATIVE_MIGRATION_PLAN.md)의 **마일스톤 1 (기준선과 계약)** 수행 결과물로서, 다음 세 가지 핵심 목적을 가집니다.

1. **전수 감사(Zero-Mock 검증)**: 기존 셸에서 실제 구현된 기능과 자리표시자(Stub)/빈 구현 상태인 기능을 명확히 격리하여 기록합니다.
2. **타입 안전성 보장 (`ts-rs`)**: Rust DTO 구조체로부터 TypeScript 정의를 자동 추출(`bindings/contracts/`)하여 클라이언트와 호스트 간 계약 일치성을 100% 보증합니다.
3. **단계적 전환의 헌장**: 마일스톤 2(Windows 셸/배지/테마) 및 마일스톤 3(SQLite/입력 훅) 전환 시 준수해야 할 계약 규격을 정의합니다.

---

## 2. IPC 채널 및 브리지 API 전수 감사 매트릭스

| 범주 | IPC 명령 / 채널명 | 호출 경로 (`bridge.rs`) | Rust 구현 상태 | 연동 DTO / 비고 |
| :--- | :--- | :--- | :--- | :--- |
| **시스템 정보** | `getDesktopInfo` | `desktopBridge.getDesktopInfo` | **구현 완료 (Live)** | win32 플랫폼 메타데이터 반환 |
| **시스템 정보** | `getGpuInfo` | `desktopBridge.getGpuInfo` | **구현 완료 (Live)** | D3D11 / WebView2 GPU 가속 메타데이터 반환 |
| **시스템 정보** | `getAppMetrics` | `desktopBridge.getAppMetrics` | **구현 완료 (Live)** | CPU 코어 수 및 프로세스 Working Set 반환 |
| **기능 매니페스트**| `desktop-capability-manifest:get` | `invokeRust(...)` | **구현 완료 (Live)** | [`DesktopCapabilityManifest`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/bindings/contracts/DesktopCapabilityManifest.ts) (`ts-rs`) |
| **윈도우 동작** | `desktopWindowBehaviorGet` | `getDesktopWindowBehaviorSettings` | **구현 완료 (Live)** | [`DesktopWindowBehaviorSettings`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/bindings/contracts/DesktopWindowBehaviorSettings.ts) (`ts-rs`) |
| **윈도우 동작** | `desktopWindowBehaviorSet` | `setDesktopWindowBehaviorSettings` | **구현 완료 (Live)** | 트레이 최소화/닫기 원자적 플래그 갱신 |
| **클립보드** | `clipboardWriteText` / `ReadText`| `clipboardWriteText` / `Read` | **구현 완료 (Live)** | `arboard` 기반 순수 네이티브 클립보드 연동 |
| **URL 오픈** | `openExternal` | `openExternal` | **구현 완료 (Live)** | `open::that` 기반 OS 기본 브라우저 연동 |
| **인스턴스 탐색** | `desktopRuntimeResolve` | `desktopRuntimeConfig.resolve` | **구현 완료 (Live)** | `.well-known/fluxer` 인스턴스 자동 탐색 및 저장 |
| **인스턴스 관리** | `knownInstancesGetAll` / `Upsert` | `knownInstances.getAll` / `upsert` | **구현 완료 (Live)** | [`DesktopKnownInstanceRecord`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/bindings/contracts/DesktopKnownInstanceRecord.ts) (`ts-rs`) |
| **스토리지** | `desktopStorageGetStatus` | `desktopStorage.getStatus` | **구현 완료 (Live)** | [`DesktopStoreStatus`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/bindings/contracts/DesktopStoreStatus.ts) (`ts-rs`) |
| **스토리지** | `desktopStorageLoad` / `Get` / `Set`| `desktopStorage.load` / `get` / `set` | **구현 완료 (Live)** | [`DesktopStorageEntry`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/bindings/contracts/DesktopStorageEntry.ts) (`ts-rs`) |
| **스토리지** | `desktopStorageSetMany` | `desktopStorage.setMany` | **구현 완료 (Live)** | [`DesktopStorageWrite`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/bindings/contracts/DesktopStorageWrite.ts) (`ts-rs`) |
| **계정 저장소** | `desktopAccountsGetAll` / `Upsert` | `desktopAccounts.getAll` / `upsert` | **구현 완료 (Live)** | [`DesktopAccountRecord`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/bindings/contracts/DesktopAccountRecord.ts) (`ts-rs`) |
| **계정 저장소** | `desktopAccountsCompareAndSwap` | `desktopAccounts.compareAndSwap` | **구현 완료 (Live)** | [`DesktopAccountCompareAndSwapRequest`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/bindings/contracts/DesktopAccountCompareAndSwapRequest.ts) (`ts-rs`) |
| **게이트웨이** | `nativeGatewayTransport*` (5종) | `nativeGatewayTransport.*` | **구현 완료 (Live)** | Tokio 웹소켓 기반 비동기 게이트웨이 파이프라인 |
| **프록시 네트워킹**| `fetchProxy` | `fetchProxy` | **구현 완료 (Live)** | WASM/바이너리 및 REST 프록시 전송 |
| **오버레이/스트림**| `voiceStateUpdate` / `popout*` | `voiceStateUpdate` / `popout*` | **구현 완료 (Live)** | [`VoiceOverlayState`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/bindings/contracts/VoiceOverlayState.ts) 및 GPUI 서브프로세스 기동 |
| **AccountEngine** | `accountEngine.request` | `desktopBridge.accountEngine` | **자리표시자 (Stub)** | 현재 무조건 `Error: AccountEngine unavailable` 반환 (마일스톤 3 SQLite 직결 대상) |
| **오디오 스트림** | `nativeAudio.onFrame` / `onEnd` | `desktopBridge.nativeAudio` | **자리표시자 (Stub)** | 장치/프로세스 목록은 반환하나 실시간 PCM 콜백 미연동 (마일스톤 4 대상) |
| **테마 감시** | `watchThemeFolders` / `Change` | `desktopBridge.watchThemeFolders` | **자리표시자 (Stub)** | 빈 Promise / 빈 리스너 반환 (마일스톤 2 `notify` 직결 대상) |
| **작업표시줄 뱃지**| `setBadgeCount` / `setUnreadBadge` | `desktopBridge.setBadgeCount` | **자리표시자 (Stub)** | 빈 함수 (마일스톤 2 `ITaskbarList3` 직결 대상) |
| **입력 단축키** | 전역 단축키 훅 (PTT) | 미등록 | **미구현 (Unsupported)** | 마일스톤 3 `windows-input-hook` 분리 직결 대상 |
| **화면 공유** | DXGI/WGC 네이티브 주입 | `bridge.rs` (크로미움 대화상자) | **브라우저 유지 (Baseline)** | 브라우저 `getDisplayMedia`로 무결성 보존 중 |

---

## 3. `ts-rs` 추출 타입 및 스키마 검증 결과

`fluxer_desktop_rs/src/contracts.rs`로부터 생성된 1차 핵심 계약 TypeScript 파일 11종:

1. [`DesktopCapabilityManifest.ts`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/bindings/contracts/DesktopCapabilityManifest.ts)
2. [`DesktopKnownInstanceRecord.ts`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/bindings/contracts/DesktopKnownInstanceRecord.ts)
3. [`DesktopStoreStatus.ts`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/bindings/contracts/DesktopStoreStatus.ts)
4. [`DesktopStoreQuarantineReason.ts`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/bindings/contracts/DesktopStoreQuarantineReason.ts)
5. [`DesktopStorageEntry.ts`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/bindings/contracts/DesktopStorageEntry.ts)
6. [`DesktopStorageWrite.ts`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/bindings/contracts/DesktopStorageWrite.ts)
7. [`DesktopStoreMarker.ts`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/bindings/contracts/DesktopStoreMarker.ts)
8. [`DesktopAccountRecord.ts`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/bindings/contracts/DesktopAccountRecord.ts)
9. [`DesktopAccountCompareAndSwapRequest.ts`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/bindings/contracts/DesktopAccountCompareAndSwapRequest.ts)
10. [`DesktopWindowBehaviorSettings.ts`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/bindings/contracts/DesktopWindowBehaviorSettings.ts)
11. [`VoiceOverlayState.ts`](file:///C:/Users/k3832/orca/projects/fluxer/fluxer_desktop_rs/bindings/contracts/VoiceOverlayState.ts)

### 검증 핵심 포인트
- **JavaScript 타임스탬프 정밀도 호환**: `u64` 타입에 `#[ts(type = "number")]`를 명시하여 TypeScript의 53비트 정수(IEEE-754 `number`)와 100% 호환되도록 구성.
- **CamelCase 직렬화 보장**: `#[serde(rename_all = "camelCase")]`를 통해 프론트엔드 계약 필드명(`storageKey`, `instanceKey`, `appStore` 등)과 완벽 일치.
- **Fixture 양방향 라운드트립**: 단위 테스트 49개 전수 통과로 역직렬화 정합성 입증 완료.

---

## 4. 마일스톤 2 착수 준비 (다음 단계)

1. **`win-shell` 크레이트 N-API 디커플링**:
   - `fluxer_desktop/native/win-shell/Cargo.toml`에 `node` feature를 신설하고, `default = ["node"]` 설정.
   - `fluxer_desktop_rs`에서 `fluxer_win_shell = { path = "../fluxer_desktop/native/win-shell", default-features = false }`로 순수 Rust 직결.
2. **작업표시줄 배지 카운트 (`ITaskbarList3`) 구현**:
   - `WindowsBadge.ts` 로직을 Win32 COM 인터페이스로 Rust 호스트 내부에 직결.
3. **테마 디렉터리 핫리로드 (`notify`) 구현**:
   - 150ms 디바운스 및 디렉터리 파일 변경 이벤트 파이프라인 가동.
