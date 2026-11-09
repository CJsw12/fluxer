# Fluxer Rust 네이티브 전환 계획

기준: `C:\Users\k3832\orca\projects\fluxer`, `desktop-rs`, `9d9814a764e293c99fdc36a4522ec1796cb607ab`. 본 문서는 소스 조사에 근거한 계획이며, 이 조사에서 빌드나 실제 앱 실행은 수행하지 않았다. 기존 33개 테스트와 메모리 수치는 인수인계상 과거 주장으로만 취급한다.

## 먼저 바로잡을 전제

- `fluxer_desktop/native/win-game-capture/src/lib.rs`의 공개 API는 GPU 스케줄링 우선순위와 `GraphicsCaptureItem` 재사용 후킹 및 계측이다. 이 크레이트 자체가 프레임을 생성하는 DXGI/WGC 캡처 엔진이라고 확인되지 않았다. `fluxer_desktop_rs/src/bridge.rs:484`는 브라우저 `getDisplayMedia`를 감싸므로, 현재 브라우저 캡처를 보존하면서 네이티브 프레임을 WebRTC에 공급하는 경로가 실제로 가능한지 별도 검증해야 한다. 후킹 코드는 프로세스 및 런타임 버전에 민감하며 WebView2 렌더러에 적용된다고 가정하면 안 된다.
- `fluxer_desktop_rs/src/audio.rs:131`은 실제 오디오 캡처가 아니라 ID를 등록하고 48kHz/2채널을 반환한다. `src/bridge.rs:1039`의 오디오 프레임 및 종료 리스너, `:907`의 테마 기능, `:1352`의 배지 기능은 빈 구현이다. AccountEngine도 `:1339`에서 unavailable이다. 따라서 33개 테스트가 통과했다는 과거 기록으로 오디오, AccountEngine, 테마, 배지, 전체 기능이 검증된 것으로 간주할 수 없다.
- Windows 호스트는 WebView2이며 브라우저/V8과 별도 프로세스가 남는다. Rust 직결은 Electron과 Node를 요구하지 않지만 UI까지 순수 Rust가 되거나 프로세스 전체 RAM이 자동으로 줄어든다는 뜻은 아니다.

## 크레이트 분리와 직접 연결

네 크레이트는 현재 독립 `[workspace]`, `cdylib`와 `rlib`를 가지고 있지만 `napi`, `napi-derive`, `napi-build`가 필수이고 `build.rs`에서 setup을 실행한다. 따라서 현 상태의 `rlib`를 path dependency로 넣는 것만으로 Node 의존이 제거되지는 않는다. 각 크레이트에 기본 `node` feature를 두어 기존 Electron 빌드를 유지하고, N-API 의존성과 build setup 및 래퍼를 feature로 제한한다. Rust 호스트는 Windows 전용 target 의존성에서 `default-features = false`를 사용한다. 중첩 독립 workspace와 `sample-pool`, `rt-thread` path 관계는 `cargo metadata`, lockfile, Electron 빌드 양쪽으로 먼저 확인한다. 필요하면 독립 코어 크레이트를 분리하며 workspace를 강제로 합치지 않는다.

- `win-shell/src/lib.rs`: platform 내부도 `napi::Result`를 쓰고 공개 함수는 `AsyncTask`를 반환한다. 단축키/레지스트리/알림 상태를 일반 Rust 옵션과 명시적 오류를 반환하는 동기 코어로 추출하고, Electron Task는 변환 어댑터로 유지한다. Rust 호스트에서는 UI 스레드를 막지 않도록 별도 작업자로 호출한다. 현재 win-shell이 작업표시줄 배지를 제공한다는 근거는 없으며 배지는 별도 구현이다.
- `windows-input-hook/src/lib.rs:208`: Windows 훅 내부가 `ThreadsafeFunction`을 직접 보유한다. `QueuedEvent`와 훅 메시지 루프를 Node 독립형으로 분리하고 bounded nonblocking 이벤트 전달, 명확한 overflow 정책, start/stop/join, 중복 등록 방지, 키 업 유실 시 상태 복구를 설계한다. Electron 래퍼에서만 TSFN을 이용한다. 포커스/권한/개인정보 범위와 게임·관리자 권한 조합을 실기 검증한다.
- `win-process-loopback/src/lib.rs:306`: PCM, 오류와 lifecycle 콜백에 TSFN, `Float32Array`, N-API 오류가 깊게 묶여 있고 platform도 N-API 오류를 사용한다. `sample-pool`과 `rt-thread`를 유지한 채 오디오 코어의 명시적 오류와 소유권 있는 프레임/콜백 인터페이스를 분리하고 Electron 어댑터가 변환한다. 고정 크기 bounded 큐, 드롭 계측, 종료·장치 변경·프로세스 종료 및 COM/thread 수명 규칙을 보장한다. 실시간 스레드에서 JSON/base64 또는 무제한 할당을 피한다.
- `win-game-capture`: 우선순위와 재사용 기능을 일반 Rust API로 노출하되 후킹 구현을 임의로 확장하지 않는다. 브라우저의 기존 화면 공유 경로를 기준선으로 둔다. 프레임을 WebView2의 MediaStream/LiveKit으로 실제로 전달하는 지원 경로와 보안 모델을 먼저 입증하고, 불가능하면 별도 설계를 결정한다. Rust에서 캡처한 프레임이나 PCM이 자동으로 브라우저 WebRTC 트랙이 되지는 않는다.

예상 의존성 형태는 `[target.'cfg(windows)'.dependencies]` 아래 `fluxer_win_shell = { path = "../fluxer_desktop/native/win-shell", default-features = false }`이며 다른 크레이트에도 같은 패턴을 적용한다. 위 feature 구조가 구현된 뒤에만 유효하다. 전환 중 각 모듈의 Electron 기본 빌드와 Rust no-default-features 빌드를 모두 검사한다.

## IPC 계약과 데이터 경계

`packages/desktop_ipc/src/DesktopBridgeTypes.ts`가 현재 원본 계약이며, 인터페이스 전체가 이 파일 하나에 있지 않으므로 preload와 AccountEngine 계약도 함께 목록화한다. `ts-rs`는 Rust에서 TypeScript를 생성할 뿐 기존 TypeScript에서 Rust 타입을 자동 역생성하지 않는다. 각 기능을 이관할 때 Rust DTO에 `serde` 및 `TS`를 적용하여 별도 생성 디렉터리에 출력하고 기존 TypeScript 계약과 양방향 assignability 검사 및 실 JSON fixture 비교를 CI에 추가한다. 생성물이 원본을 덮어쓰지 않게 하며, 검증 후 해당 계약의 소유권 이전 여부를 결정한다. UI 소스는 임의 수정하지 않는다.

camelCase, optional과 null의 차이, tagged union, 숫자 범위와 JS 정밀도, 빈 배열/객체, 미지 필드, 오류·취소, 이벤트와 unsubscribe를 명시한다. 예를 들어 `ThemeFolderChange`와 `NativeAudioFrameMessage`는 실제 JSON 및 프레임 전송 형태를 각각 검증해야 하며, `Float32Array`와 바이너리를 JSON 배열로 무조건 바꾸지 않는다. `ts-rs`의 타입 생성만으로 런타임 프로토콜·바이트 형식·이벤트 전달이 보장되지 않는다. [ts-rs 공식 derive 문서](https://github.com/aleph-alpha/ts-rs/blob/main/_autodocs/api-reference/TS-derive.md)를 참조한다.

## Electron 잔여 서비스

- SQLite: `SqliteRowStore.ts`뿐 아니라 `CacheDatabase.ts`와 AccountEngine 호출 체인을 함께 이관한다. 기존 schema의 `meta`, `rows`, `cursors`, `summaries`, `mentions`, epoch, WAL, account 격리 및 prefix 검색 의미를 보존하고 `rusqlite`는 전용 소유 스레드에서 사용한다. 256 KiB/값, 계정별 200만 행·1 GiB 제한과 중복 키 검사를 fixture로 비교한다. `replaceAccount`는 데이터와 cursor를 원자적으로 커밋하도록 트랜잭션을 적용하며 원본과 차이가 생기는 실패 의미를 명시한다. 파일 권한, 복구, 충돌, 스키마 버전, 기존 DB 백업/롤백 및 캐시 재생성을 검증한다. 저장소만 추가해도 현재 unavailable인 AccountEngine이 자동 복구되지는 않는다.
- 테마: `ThemeLocalFiles.ts`의 사용자 선택과 허용 경로·실제 경로 검증, 파일 크기 및 개수 제한, 구독 해제를 포함해 `notify`와 디렉터리 스냅샷 비교를 연결한다. 원본은 150ms debounce, added/changed/removed/renamed, missing/live 및 truncated를 사용한다. Windows 감시 종료, rename, overflow/rescan, 원자적 교체, 폴더 재생성 및 링크 우회를 시험한다. 이벤트는 유실될 수 있으므로 스냅샷 재검색으로 복구한다. [notify 이벤트 가이드](https://github.com/notify-rs/notify/wiki/The-Event-Guide).
- 배지: UI 스레드의 유효한 HWND에서 COM 초기화와 `ITaskbarList3` 수명 및 `SetOverlayIcon`을 관리한다. 기존 `WindowsBadge.ts`의 0/점/1~10 이상 규칙, 설명 문자열, dark 설정 및 DPI별 기존 아이콘을 재사용하고 HICON을 해제한다. 탐색기 재시작, 창 재생성, DPI 변경, 0으로 지우기를 검증한다. `setUnreadBadge` 연결과 태스크바 실제 표시를 함께 시험한다.

## 4개 이행 마일스톤과 완료 조건

1. **기준선과 계약**: 현재 기능별 실제 구현/자리표시 목록, IPC 메서드와 이벤트, Electron 응답 fixture를 기록한다. WebView2 실제 화면 공유·DM·통화, 테스트 및 프로세스 트리 메모리를 같은 환경에서 측정한다. 한 계약군부터 ts-rs 생성, TS 양방향 타입 검사 및 JSON 사례를 통과한다. 기능별 지원 여부를 실제 상태에 맞게 노출한다.
2. **저위험 직결과 시스템 서비스**: win-shell을 분리하고 기본 Electron 빌드 및 Node 없는 Rust 링크를 각각 확인한다. 배지와 테마를 실제 IPC 및 구독으로 연결한다. 창 재생성, DPI, 경로 권한, 디렉터리 삭제·복구와 watcher 정리 통과가 완료 조건이다.
3. **저장 및 입력**: SQLite와 전체 AccountEngine 의존 경로를 기능 단위로 연결하고, 데이터 보존·롤백·crash recovery·account 격리와 기존 DB 호환을 검사한다. input-hook을 독립 코어와 bounded 이벤트 경로로 분리하여 키 다운/업, 반복 등록, stop, 포커스 및 게임 중 PTT를 실기 검증한다. 사용자의 기존 DB는 백업 후 마이그레이션한다.
4. **실시간 미디어 및 최종 검증**: loopback을 코어/Node 어댑터로 분리하고 실제 PCM이 브라우저 WebRTC/LiveKit 트랙에 도달하는 경로를 구현한다. 프로세스 선택·종료, 장치 변경, backpressure, 음성 싱크와 반복 시작/중지, CPU 및 드롭을 검사한다. 게임 캡처 후킹은 대상 프로세스와 지원을 입증한 경우에만 도입한다. 화면공유는 현재 `getDisplayMedia` 경로를 유지하며 수신자에게 실제 프레임과 오디오가 전달되는지 별도 기기에서 검증한다. Electron 빌드, Windows Rust 빌드, 계약/단위/통합 테스트, 실 앱 회귀와 최종 릴리스 빌드를 마친다.

각 단계에서 `cargo check -p fluxer_desktop_rs`, 관련 크레이트의 Node on/off 빌드 및 실질적 `cargo test`, 필요 시 `cargo clippy`를 실행하고 첫 실패를 보존한다. 메모리는 동일 WebView2 버전·기기·프로필·시간 조건에서 유휴와 통화/공유 각각의 전체 프로세스 트리 working set 및 private bytes를 반복 측정한다. working-set trimming은 private bytes를 해제했다는 증거가 아니며 80~120 MB는 검증 목표이지 보장치가 아니다. 성능 악화, 무손실 데이터 및 기능 회귀가 발생하면 해당 기능 전환을 중단하고 이전 경로로 되돌린다. 각 완료 단계에서만 주 에이전트가 PROGRESS.md를 갱신한다.
