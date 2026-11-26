# Parity Lab — 공식 Fluxer와 Rust 앱의 1:1 자동 비교·탐색 체계

> 참고서다. 에이전트 규칙·절차·도구 한 줄 사용법은 `AGENTS.md`("UI 1:1 작업" 절)에 있고, 지난 기록(기존 한계·첫 비교·남은 과제·외부 사례)은 로컬 보관본 `docs/archive/PARITY_LAB-history.local.md`로 옮겼다.

> 작성 2026-11-23. 목표는 Rust GPUI 앱(`fluxer_client/`)의 모든 화면과 모든 버튼으로 열리는 창을 공식 Fluxer와 기능·모양 모두 같게 만드는 것이다. 이 문서는 기존 검증 방법의 한계, 새 방법의 설계와 실측 결과, 남은 과제를 정리한다.

## 1. 결론

- 기준값은 사람이 고른 스크린샷이 아니라 **공식 앱 자체의 DOM**이다. 공식 앱은 웹 기술(React)로 만들어져 있고 소스에 `data-flx` 식별자가 약 18,700개 박혀 있다. 이 값은 프로덕션 빌드에도 남는다. Chrome DevTools Protocol(CDP)로 공식 앱을 구동하면 요소별 위치·크기·계산된 CSS·텍스트·접근성 트리를 정확한 숫자로 얻을 수 있다.
- Rust 앱의 요소에 **같은 `data-flx` 문자열**을 붙이고, 앱이 그 요소들의 경계를 IPC로 내보내게 하면 두 앱을 요소 단위로 조인해 차이를 픽셀 수치로 계산할 수 있다.
- 공식 앱의 모든 클릭 가능한 요소를 자동으로 누르는 **크롤러**가 "버튼을 누르면 뜨는 창" 목록(상태 그래프)을 만든다. Rust 앱은 같은 ID를 같은 순서로 눌러 같은 상태가 나오는지 대조한다.
- 이 세 가지가 맞물리면 사람이 장면을 고르거나 눈으로 비교하지 않아도 "무엇이 몇 px 다르다/무엇이 없다"가 기계적으로 나오고, 에이전트가 그 보고서를 입력으로 수정·재검증을 반복할 수 있다.

## 3. 구성 요소 (`tools/parity_lab/`)

### 3.1 공식 기준값 추출 — `web_capture.mjs`, `lib.mjs`
- 의존성 없는 Node(≥22) CDP 클라이언트다. 두 모드가 있다.
  - **데스크톱(권장)**: 공식 Electron 앱을 `--remote-debugging-port=<port>`로 실행하고 `--attach <port> --origin fluxer-app://app`으로 붙는다. 네이티브 타이틀바·데스크톱 전용 요소까지 포함된 진짜 데스크톱 화면이다.
  - **웹**: 전용 프로필의 headless Chrome으로 시험 서버 웹 앱을 연다. 세션이 없으면 실제 로그인 폼으로 한 번 로그인한다(앱이 계정 카탈로그로 세션을 복원하므로 토큰 주입만으로는 안 된다. 부팅 뒤에는 `window.localStorage`도 제거된다).
- 뷰포트·DPR을 1280×800/1로 고정하고 `prefers-reduced-motion: reduce`를 에뮬레이션한다. 웹 전용 nag bar는 실제 입력 이벤트로 닫는다(React 핸들러는 합성 `.click()`을 무시하는 경우가 있다).
- 출력: `<slug>.png`, `<slug>.dom.json`(보이는 `[data-flx]` 요소의 사각형·주요 계산 스타일·고유 텍스트), `<slug>.ax.json`(역할·이름·경계), `manifest.json`(공식 빌드 문자열 포함).

### 3.2 Rust 패리티 프로브 — `fluxer_client/app/src/parity_probe.rs`, `tools/parity_lab/rust_probe.py`
- `FLUXER_PARITY_PROBE=1`로 실행할 때만 켜진다. 꺼져 있으면 요소를 추가하지 않는다.
- `.flx("<공식 data-flx>")`가 붙은 요소는 절대 위치 `canvas` 자식의 prepaint에서 경계를 기록한다(릴리스 빌드에서도 동작, 렌더 중 상태 변경·`notify` 없음).
- 단일 인스턴스 IPC(토큰 인증)에 `probe_inspect`, `probe_resize`, `probe_click`, `probe_hover`, `probe_key`를 추가했다. 클릭·키 입력은 `Window::dispatch_event`로 창 내부에 합성 이벤트를 넣으므로 사용자의 실제 마우스·포커스를 빼앗지 않는다. `probe_resize`로 사용자가 창 크기를 바꿔도 캡처 직전에 1280×800 콘텐츠 크기로 되돌린다.

### 3.3 조인 비교기 — `compare.py`
- 공식 `dom.json`과 Rust `inspect` JSON을 ID(`--N` 접미사 제외)로 조인한다. 반복 요소는 같은 텍스트끼리, 그다음 읽기 순서로 짝짓는다.
- 사각형 차이(±tol px), 텍스트 차이, 두 스크린샷에서 같은 박스 안쪽의 중앙값 색 차이를 계산한다. 결과는 `report.json`과 나란히 놓은 `overlay.png`(공식=초록, Rust=빨강/노랑)다.
- `coverage`는 장면의 공식 ID 수와 Rust가 태그한 ID 수를 함께 보여 준다. 태그되지 않은 UI가 숫자로 드러난다.
- 웹 캡처를 데스크톱 좌표로 옮길 때는 `--official-offset 0,32`(타이틀바 높이)를 준다.

### 3.4 클릭 크롤러 — `crawl.mjs`
- 기준 화면에서 보이는 클릭 가능한 `[data-flx]` 요소를 모두 찾아 하나씩 실제 입력으로 누르고, 새로 나타난 ID 집합·경로 변화로 상태를 구분해 스크린샷·DOM과 함께 `graph.json`에 기록한다. 같은 창을 여는 다른 버튼은 `also_opened_by`로 묶는다.
- `--depth 2`면 열린 레이어(팝아웃·모달·메뉴) 안의 컨트롤도 같은 방식으로 탐색한다(설정 모달의 탭, 메뉴의 하위 메뉴 등).
- 안전장치: 창 제어, 채널·길드·DM 이동, 삭제·나가기·추방·차단·신고·로그아웃·전송·통화, 반응·메시지 액션 바, 파일 업로드·음성 메시지·카메라·화면 공유처럼 파괴적이거나 OS 대화상자를 여는 동작은 누르지 않는다. 레이어 없이 상태만 바꾸는 토글(즐겨찾기, 음소거)은 기록 직후 다시 눌러 되돌린다. 매 클릭 전 기준 화면과 ID 집합이 하나라도 다르면 경로를 다시 불러온다.
- **쓰기 차단 가드(필수)**: 2026-11-24 깊이 2 크롤이 메시지 위 빠른 반응 버튼과 채널 알림 메뉴를 눌러 시험 서버 데이터를 바꿨다(반응 4개 추가, #markdown 채널 알림 덮어쓰기 `muted: true`·`message_notifications: 2`). 거부 목록만으로는 막을 수 없어 크롤러는 이제 모든 문서에 `fetch`·`XMLHttpRequest`·`sendBeacon`을 감싸는 스크립트를 주입해 API 경로의 GET이 아닌 요청을 전송 전에 실패시킨다. 데스크톱 앱은 API를 사용자 정의 스킴(`fluxer-app://app/api/<인코딩된 URL>`)으로 프록시하므로 CDP `Fetch` 가로채기로는 막을 수 없다(실험으로 확인). 차단된 요청은 `graph.json`의 `blocked_writes`에 남는다. 게이트웨이(WebSocket) 명령은 막지 않으므로 음성 참가·상태 변경 컨트롤은 거부 목록으로 막는다.
- 크롤 결과(`graph.json`)는 상태를 찾을 때마다 저장되므로 중간에 멈춰도 그때까지의 기록이 남는다.
- 실측: 공식 데스크톱 #markdown에서 30번 클릭으로 서로 다른 상태 25개(커뮤니티 메뉴, 알림 설정, 스레드, 고정 메시지, 검색 범위, 받은 편지함, 사용자 프로필 팝아웃, 멘션 팝아웃, 미디어·스티커·이모지 피커, 사용자 메뉴, 사용자 설정 등)를 기록했다.

### 3.5 Rust 재생 러너 — `replay.py`
- 크롤러의 `graph.json`을 받아 상태마다 `resize` → 딥링크 → Escape → `trail`의 각 ID를 `probe_click`(정확한 리터럴 먼저, 없으면 `--N` 없는 기본 ID) → `inspect`·스크린샷 → `compare.py`를 실행한다.
- 상태별 판정: `trigger_missing`(Rust에 트리거 요소가 없음), `opened_partial`(공식 상태에 새로 생긴 ID 중 일부만 Rust에 있음), `opened_full`(모두 있음, 이후 기하·색 비교). 결과 `replay.json`이 "버튼으로 열리는 모든 창" 체크리스트다.
- 실측(#markdown, 공식 상태 25개): `trigger_missing` 15, `opened_partial` 10(공식 레이어 ID 존재율 모두 0%), `opened_full` 0. 즉 레일 버튼, 커뮤니티 메뉴, 검색 범위, 스포일러, 코드 복사, 멘션·이모지 팝아웃, 미디어·스티커·이모지 피커, 사용자 메뉴의 트리거가 아직 태그되지 않았고, 열리는 창들은 공식 구조로 이식·태그되지 않았다. 예: 공식 커뮤니티 메뉴는 떠 있는 메뉴 7항목(Invite members, Notification settings, Privacy settings, Edit community profile, Hide muted channels, Delete my messages, Leave community)인데 Rust는 사이드바 안쪽 카드 3항목(Create channel, Community Settings, Bans)이다.

## 4. Orca computer use 평가

- **가능한 것**: GPUI 창의 UIA 접근성 트리(역할·이름)를 백그라운드에서 읽는다. AccessKit 덕분에 릴리스 빌드에서도 `Status: Online`, 채널·버튼 이름이 보인다. 창 스크린샷도 얻는다. 연결 상태 확인(`Status: Online`)처럼 앱 수정 없이 하는 점검에 유용하다.
- **한계**: JSON 출력에 요소별 좌표가 없고, 색·글꼴·여백 같은 스타일 정보가 없다. 입력은 `background=false`라 사용자 창 포커스를 침범한다. 역할이 없는 `div`는 트리에 나오지 않는다.
- **판단**: 주 비교 경로로는 부족하다. 보조 수단으로 쓴다(접근성 이름·역할 대조, 연결 상태 점검, CDP를 쓸 수 없는 네이티브 대화상자 확인). 앱 구동은 Rust IPC 프로브, 공식 앱은 CDP가 더 정확하고 사용자 입력을 방해하지 않는다.

## 5. 결정성·버전 고정

- **같은 계정·같은 데이터·같은 테마**: 두 앱을 같은 시험 계정으로 로그인시킨다. 다른 계정이면 본인 멘션 강조, 사용자 영역, 계정 테마(서버 동기화)가 달라져 색 비교가 무의미해진다.
- **결정적 시드**: 공식 API는 `test_mode`에서 `/test/messages/seed`(고정 ID·타임스탬프 메시지), 사용자 플래그·프리미엄, 길드 기능 패치 경로를 제공한다(`fluxer_api/src/api/test/TestHarnessController.ts`). 사용자·길드는 일반 REST로 만든 뒤 이 경로로 상태를 고정한다.
- **기준 버전 고정**: 모든 캡처에 공식 빌드 문자열(`Stable Desktop …, Web …`)을 기록한다. 포터블 데스크톱은 `FLUXER_DISABLE_MODULE_UPDATES`로 자동 업데이트를 막고, 저장소 `main`(= `upstream/main`)을 그 빌드에 맞는 커밋으로 맞춘다. 이상적으로는 저장소의 `fluxer_app`을 직접 빌드해 기준값으로 쓴다(`pnpm`·`rspack` 빌드, API는 같은 출처 `/api`).

## 6. 자동 개발 루프

1. 장면 목록 = 크롤러 그래프(공식 앱) + 경로 목록(채널·DM·친구·설정 탭·음성). 각 상태마다 공식 `dom.json`/`png`와 진입 경로(`trail`)가 있다.
2. Rust 재생: `rust_probe.py`로 `resize 1280 800` → 딥링크 → `trail`의 각 ID를 `probe_click` → `inspect` + `shot`. 장면 성공 판정은 공식 상태의 핵심 ID가 Rust 쪽에 나타났는지로 한다(없으면 "미구현 창"으로 보고).
3. `compare.py` 보고서를 영역별로 묶는다(레일, 사이드바, 헤더, 메시지, 컴포저, 멤버 목록, 각 팝아웃·모달).
4. 위임: 공식 TSX↔Rust 파일 대응과 `.flx` 태깅처럼 기계적인 일은 DeepSeek flash 또는 Haiku, 영역별 수정은 파일 소유권을 나눈 Sonnet(`fluxer-porter`), GPUI 구조 변경·난해한 버그·최종 검토만 Opus.
5. 재빌드 후 같은 장면을 다시 비교해 기준(기하 ±2px, 색 Δ≤18, 누락 ID 0)을 넘을 때까지 반복한다. 보고서 수치가 회귀하면 커밋하지 않는다.

## 10. 운영 가이드 (인계용)

### 10.1 상시 실행 구성
세 프로세스를 띄워 둔 채로 개발한다. 접속 정보·키·계정은 저장소 밖 비공개 위치에 있다(경로는 에이전트 메모리와 비공개 메모 참고).
1. **시험 서버 터널**: 로컬 443을 시험 서버로 포워딩하는 SSH 터널. hosts에 시험 도메인이 127.0.0.1로 등록돼 있고 서버 루트 인증서는 Windows 신뢰 저장소에 있다.
2. **Rust 클라이언트**: `dist/fluxer_client/fluxer_client.exe --no-os-integration`을 `FLUXER_PARITY_PROBE=1`과 시험 프로필용 `LOCALAPPDATA`로 실행한다. `SSL_CERT_FILE`은 설정하지 않는다(없는 파일을 가리키면 TLS가 실패해 `Reconnecting…`에서 멈춘다). 연결 확인은 `orca computer get-app-state --app fluxer_client --no-screenshot --query Status` → `Status: Online`.
3. **공식 비교 앱**: 포터블 공식 데스크톱 사본을 `--remote-debugging-port=9334`로 실행한다(다크 테마, tester2). 자동 업데이트로 기준 빌드가 바뀌지 않도록 실행 전 환경변수 `FLUXER_DISABLE_MODULE_UPDATES=1`(PowerShell: `$env:FLUXER_DISABLE_MODULE_UPDATES = "1"`)을 설정한다. 사용자가 평소 쓰는 공식 앱(`%LOCALAPPDATA%\fluxer_desktop`)과 같은 이름(`Fluxer.exe`)이므로 경로로 구분하고, 개인 앱에는 절대 붙지 않는다.

- 세 프로세스는 에이전트 도구의 백그라운드 실행(최대 2시간)으로 띄우면 시간이 지나 강제 종료된다. `pwsh`의 `Start-Process`로 분리 실행한다. 클라이언트에 줄 `FLUXER_PARITY_PROBE`·`LOCALAPPDATA`는 같은 `pwsh` 세션에서 `$env:`로 설정한 뒤 실행하면 상속된다. SSH 키 경로에 공백이 있으면 인자에 따옴표를 넣어야 하며, 그렇지 않으면 터널이 조용히 바로 종료된다.

### 10.3 위임 방식
규칙은 `AGENTS.md`의 "UI 1:1 작업" 2절(규칙)로 옮겼다.

### 10.4 자주 겪은 함정
- `MSYS_NO_PATHCONV=1` 없이 Git Bash에서 `/channels/...`를 넘기면 `C:/Program Files/Git/channels/...`로 바뀐다. 스크립트는 저장소 루트에서 실행한다(상대 경로).
- 공식 앱은 부팅 후 `window.localStorage`를 지우고, React 핸들러가 합성 `.click()`을 무시하는 경우가 있어 CDP 실제 입력을 쓴다. 웹 앱은 토큰 주입으로 로그인되지 않으므로 실제 로그인 폼을 한 번 거친다.
- **voice_worker 재빌드**: `scripts/package_fluxer_client.ps1`은 `-IncludeVoiceWorker`를 줄 때만 워커를 다시 빌드해 `dist`에 복사한다. 워커(`fluxer_client/voice_worker`)를 고쳤다면 반드시 이 스위치로 패키징한다. 2026-11-24까지 `dist`의 워커는 11/22 빌드에 머물러 있었다.
- 공식 데스크톱 앱에서 음성 채널 경로를 열면 "Join voice channel" 버튼이 있는 안내 화면이 나온다. 앱 재시작 뒤에는 "Reconnect / Not now" 알림 막대도 뜬다.
- 시험 서버 로그인은 짧은 시간에 반복하면 429(`retry_after` 약 20초)가 난다. 토큰을 한 번 받아 재사용한다. Node에서 시험 서버에 직접 붙을 때는 `NODE_EXTRA_CA_CERTS`에 서버 루트 인증서를 준다.
- 사용자 길드 알림 설정은 REST GET이 없다. 게이트웨이 `READY`의 `user_guild_settings`로 읽는다.
- 같은 계정이 아니면 본인 멘션 강조·사용자 영역·계정 테마가 달라 색 비교가 어긋난다. 색 비교 결과에서 이런 항목은 버그로 세지 않는다.

### 10.5 시험 서버 상태 복구 점검
크롤이나 수동 시험 뒤에는 다음을 확인한다: #markdown 메시지 4개의 반응 없음, 고정 메시지 없음, tester2 `user_guild_settings`의 채널 덮어쓰기 없음(`muted: false`), 포터블 앱의 즐겨찾기 해제·멤버 목록 표시·마이크/스피커 켜짐·다크 테마.

### 10.6 자주 한 실수와 예방법
- **성공 판정 없는 캡처를 검증으로 보고함** → 32장이 모두 연결 실패 화면이었는데 통과로 보고. 예방: 캡처마다 장면 성공(연결 상태, 기대 요소 존재)을 확인하고, 아니면 실패로 보고.
- **`SSL_CERT_FILE`에 없는 파일 지정** → 클라이언트가 `Reconnecting…`에 멈춤. 예방: 이 변수는 설정하지 않는다.
- **정적 감사(정규식) 통과를 1:1 일치로 착각** → 실측은 4/53만 일치. 예방: 판정은 공식 앱 실측 비교(`check_scene.py`)로만 한다.
- **아이콘을 이름·소스로 짐작하거나 기억으로 SVG 작성** → 굵기(fill/regular) 오류. 예방: 실행 중인 공식 앱에서 뽑은 원본(`harvest_icons.mjs`, `official_icons.json`)만 쓴다.
- **크롤러가 서버 데이터를 바꿈**(반응 추가, 알림 음소거) → 예방: 쓰기 차단 가드가 켜진 `crawl.mjs`만 쓰고, 크롤 뒤 상태 점검(PARITY_LAB 10.5).
- **한 가지 상태만 비교**(호버·음성·음소거 미확인) → 호버 툴팁, 음성 배지, 화면 공유 결함을 놓침. 예방: 상태별(호버, 음성 참가, 음소거, 공유 중) 캡처를 함께 비교한다.
- **"구현됨"을 동작 확인 없이 집계** → 화면 공유 미동작을 놓침. 예방: 기능은 자동 기능 시험(`share_check.py` 등)을 통과해야 완료로 센다.
- **워커를 고치고 일반 패키징만 함** → `dist`에 옛 워커가 남음. 예방: `voice_worker`를 바꾸면 `-IncludeVoiceWorker`로 패키징.
- **토글 버튼(음소거 등)을 상태 확인 없이 클릭** → 마이크가 켜질 수 있음. 예방: 접근성 이름(`Unmute microphone` 여부)을 먼저 읽고 필요할 때만 클릭.
- **다른 계정·테마·버전으로 비교** → 색·본인 표시 차이를 결함으로 오인. 예방: 같은 테마·버전 기준, 본인 시점 표시는 제외하고 판단.
- **비교 앱 상태를 바꿔 놓고 원복 안 함**(멤버 목록 숨김, 즐겨찾기, 음성 참가·공유) → 다음 측정이 어긋남. 예방: 작업 후 멤버 목록 표시·즐겨찾기 해제·음성 나가기·공유 중지 확인.
- **에이전트 보고를 그대로 믿음** → 미검증 수치·추측이 섞임. 예방: 루트가 빌드 후 실측으로 다시 판정한다.
- **Git Bash 경로 변환·키 경로 공백·백그라운드 2시간 제한** → 도구 실패. 예방: 위 "Tool quick reference"와 상시 실행 구성 항목 참고.
- **게이트에 `--launch-rust`를 줬는데 클라이언트가 이미 실행 중** → 단일 인스턴스라 새 프로세스가 곧바로 종료(exit 0)되어 게이트 3개가 시작도 못 함(2026-11-24). 예방: 상시 실행 중이면 `--launch-rust` 없이 실행한다.
- **세션 도중 터널과 공식 비교 앱이 함께 꺼짐**(2026-11-24, 원인 미확인) → Rust는 `Reconnecting…`, CDP 9334 무응답. 예방: 측정 전에 `Status: Online`과 `curl 127.0.0.1:9334/json/version`을 함께 확인하고, 꺼졌으면 10.1대로 다시 띄운다.
- **공식 앱 게이트웨이가 조용히 끊김**(2026-11-25) → 공식 화면은 예전 상태(둘 다 온라인)를 보여 주지만 서버에서는 tester2가 오프라인이라, 막 재시작한 Rust의 멤버 목록이 `Online—1 / Offline—1`로 바뀌어 멤버 행 y 차이 6건이 새로 생김. 예방: 측정 전에 Rust 멤버 목록이 `Online—2`인지 확인하고, 아니면 공식 앱을 CDP `Page.reload`로 다시 연결한다.
- **측정 태그를 테두리 설정보다 앞에 붙임** → `.flx()`가 테두리를 모르면 상자가 2px 작게 잡힘. 예방: `.flx()`는 `.border_*()` 뒤에 둔다.
- **게이트가 마이크를 음소거로 남김**(2026-11-25) → `check_resize.py`가 음성 단계 전에 강제 음소거하고 원복하지 않아, 실행할 때마다 Rust 마이크가 꺼진 채로 남았다(수정 `e41e87941`). 예방: 음성을 건드리는 스크립트는 시작 상태를 접근성 트리(`Mute`/`Unmute microphone`)로 기록하고 끝에 되돌린다. 측정 전 점검에 양쪽 마이크 상태를 넣는다.
- **서버가 Rust 사용자를 오프라인으로 붙잡음**(2026-11-26) → Rust 화면은 `Online`·멤버 목록 `Online—2`인데 공식 앱에서는 tester1이 `Offline`이고, 공식 앱 새로고침으로도 풀리지 않았다. 시험 서버 게이트웨이 컨테이너를 재시작한 뒤 Rust를 다시 접속시키자 회복됐다(서버 presence 캐시가 비어 길드 대조가 offline으로 바꾸는 경로로 추정). 예방: 측정 전 `env.py status`로 공식 쪽 `Online—2`까지 본다. `env.py heal`은 새로고침 → Rust 재기동까지 자동으로 하고, 그래도 안 되면 게이트웨이 재시작을 안내한다.
- **Antigravity 명령 샌드박스의 숨은 데스크톱**(2026-11-25) → Antigravity의 명령은 별도 데스크톱 샌드박스에서 돌아서, 거기서 띄운 Rust 클라이언트는 사용자 화면에 창이 없다(창 핸들 0, `orca computer`가 `appNotFound`). 측정 IPC는 되므로 작업 중에는 문제없지만 상태 확인은 `rust_probe.py inspect`의 `user-status`로 하고, 스크린샷이 필요한 게이트는 `--launch-rust`로 같은 데스크톱에서 띄운다. `Start-Process ... -Wait`를 백그라운드 작업으로 돌리면 그 작업 수명에 묶인다. 세션을 끝낼 때는 사용자 데스크톱의 일반 터미널(또는 Claude 세션)에서 10.1대로 다시 띄워야 상시 실행이 보인다.
- **공식 앱 자동 업데이트로 기준 버전이 바뀜**(2026-11-26) → 1120 기준 명세와 1124 화면이 섞여 판정이 어긋나고, 1120 캡처에 우연히 떠 있던 "업데이트 있음" 아이콘을 흉내 낸 40px 여백까지 Rust에 들어갔다. 예방: 공식 앱은 항상 `FLUXER_DISABLE_MODULE_UPDATES=1`로 띄우고, 캡처마다 기록되는 빌드 문자열(`version_warning`)이 기준과 다르면 전체 명세를 다시 캡처한다. 일시적 UI(업데이트 아이콘, 알림 막대, 연결 배너)는 흉내 내지 말고 조건부로만 구현한다.
- **장면 캡처가 일부 실패하면 그 장면 명세는 옛 파일로 남음** → 재캡처 뒤에도 스레드 명세 하나가 1120 시점 파일이라 래칫이 엉뚱하게 실패했다. 예방: 재캡처 후 명세 파일 수정 시각을 확인하고, 실패한 장면만 따로 다시 캡처한다.
- **터널이 끊기면 화면 상태가 오류로 남음**(2026-11-26) → 터널이 몇 분 끊긴 사이 Rust 탐색 페이지가 "다시 시도" 오류 상태로 남아, 연결이 돌아온 뒤에도 래칫이 12→22로 실패했다. 예방: `verify_all.py`의 측정 전 점검이 실패하면 실기 단계는 자동으로 건너뛴다. 터널을 복구한 뒤에는 클라이언트를 다시 띄우거나 오류 상태 화면(탐색 등)에서 다시 시도한 다음 측정한다.
- **개인 정보 커밋 위험** → 예방: 커밋 전 IP·키 경로·사용자 경로·계정 문자열을 검색하고, `Logs/`와 `*.local.md`는 커밋하지 않는다.
- **전체 박스·모서리·미태그 모달·컨테이너 호버·의미 차이 누락(2026-11-24 추가 발견 6건)** → ① `sample_colors`가 박스 중앙 50%만 봐서 1px 관통 선(하단 프로필 위 외곽선)과 모서리 반경(멤버 목록 14px 둥근 모서리)을 놓침 ② `.flx` 태그 없는 모달(설정 창)은 `coverage` 숫자만 찍히고 게이트에 걸리지 않음 ③ `state_matrix.py`가 호버 시 대상 작은 박스(32×32) 안쪽만 비교하고 주변 컨테이너 전체를 보지 않았으며 exit 0만 반환해 헤더·사용자 영역 숨은 호버 버튼(채널 설정 기어·길드 액션·계정 전환 버튼)을 놓침 ④ `replay.py`·`composer.rs`가 버튼 클릭 뒤 "서로 다른 화면이 열리는지"를 검사하지 않아 스티커 버튼이 이모지 피커를 여는 결함을 놓침. **예방:** `compare.py`에 전체 박스 `edge_p95`·모서리 16×16 패치·`diff_heatmap.png`·`top_untagged_official`를 넣고, `state_matrix.py --strict`가 컨테이너 밝은 픽셀 변화(`phantom_hover_ui`)와 툴팁 누락 시 실패하도록 바꾸며, `check_surface_parity.py`가 미디어/스티커/이모지 분리·숨은 호버 부재·프레임 이음매·설정 명암을 검증한다.

### 10.7 도구 빠른 참조 (PowerShell·Git Bash 공통)
Run everything from the repo root. In **PowerShell** there is no MSYS path mangling (ignore `MSYS_NO_PATHCONV`); in **Git Bash** prefix commands that pass `/channels/...` with `MSYS_NO_PATHCONV=1`. Python always `python -I`. Long jobs (builds, crawls) as background tasks with completion notification; never poll with sleep loops.

**Orca computer use** (`orca computer ...`, Windows UIA via Orca; read-mostly for us)
- `orca computer list-apps` — running apps with pid. Our Rust app = `fluxer_client`; the user's personal official app is also named `Fluxer` → never target it; the comparison copy is identified by its executable path (see the private ops note; use `--app pid:N`).
- `orca computer get-app-state --app fluxer_client --no-screenshot [--query Status] [--max-elements 120]` — accessibility tree (roles + names, no coordinates). Main uses: connection check (`Status: Online`), whether the mic is muted (`Unmute microphone` present = muted), which buttons exist.
- `get-app-state` without `--no-screenshot` returns a screenshot path in JSON (`--json`).
- `click/type-text/press-key/drag` exist but input is NOT background-capable (`capabilities`: background=false) — it takes the user's foreground/mouse. Do not use them for routine automation; drive the Rust app with the probe IPC and the official app with CDP instead. Never bypass input blocked by occluded windows.
**Rust app (probe IPC, needs FLUXER_PARITY_PROBE=1)** — `python -I tools/parity_lab/rust_probe.py <cmd>`
- `resize 1280 800` · `link fluxer://channels/<g>/<c>` (also `fluxer://scene/voice/<g>/<c>`, `scene/voice-leave`, `scene/settings/<tab>`, `scene/user-area-popout`, ...; list in `fluxer_client/app/src/deep_links.rs`) · `click <data-flx> [index]` · `hover <data-flx>` · `key escape` · `inspect` (JSON of tagged element rects) · `shot out.png`. From Python: `rp.send('probe_click', x=.., y=..)`.
**Official app (CDP on port 9334)**
- Capture a scene: `node tools/parity_lab/web_capture.mjs --attach 9334 --origin fluxer-app://app --out OUT --scenes scenes.json`.
- Crawl all buttons (write guard on): `node tools/parity_lab/crawl.mjs --attach 9334 --origin fluxer-app://app --route /channels/<g>/<c> --out OUT --depth 2 --max 220`; then run the state check (docs/PARITY_LAB.md 10.5).
- Icons on screen → Phosphor ids: `node tools/parity_lab/harvest_icons.mjs --attach 9334`.
- Voice helpers: `node tools/parity_lab/voice_official.mjs --attach 9334 join|watch|share|stop|leave [--guild G --channel C]`.
**Loops and checks**
- Dev loop: `python -I tools/parity_lab/cycle.py [--release] [--worker] [--skip-tests] [--check OFFICIAL.dom.json OFFICIAL.png <link> OUT]`.
- Scene diff: `python -I tools/parity_lab/check_scene.py OFFICIAL.dom.json OFFICIAL.png <link> OUT [--official-offset 0,32]`; generic join: `compare.py` (writes `diff_heatmap.png`); replay crawl graph on Rust: `replay.py GRAPH_DIR OUT --link <link> [--launch-rust dist/fluxer_client/fluxer_client.exe] [--strict-no-duplicates]` (generates `backlog.md`/`backlog.json` and detects copy-paste duplicate screens).
- State & surface gates (normally via `verify_all.py`; with the always-on client do NOT pass `--launch-rust`): `python -I tools/parity_lab/state_matrix.py --strict --out OUT` (blocks `phantom_hover_ui` spikes and missing tooltips); `python -I tools/parity_lab/check_surface_parity.py --out OUT` (seam, member corner, hover phantom UI, Media/Stickers/Emojis split, settings contrast).
- Screen share both ways: `python -I tools/parity_lab/share_check.py --guild G --channel C --out OUT` (needs release client with worker).
- Icons: `python -I tools/parity_lab/check_icons.py` (manifest + originals + call-site sync, must be 0 errors); after moving or adding `AppIcon::` call sites run `--sync` and mark sites verified only with same-size official evidence; ground truth list `tools/parity_lab/official_icons.json`.
- Spec loop (preferred over eyeballing): `node tools/parity_lab/spec_official.mjs --attach 9334 --viewport 1280x800 --out SPEC.json [--hover <flx>]` (read-only CDP: computed box/paint props, matched CSS-module rules, floating) → `python -I tools/parity_lab/spec_delta.py SPEC.json --live [--scope <flx-prefix>]... --out-md OUT.md [--max N]` (one line per delta with CSS rule and Rust `file:line` from the probe's `src`/`style`). Hand the delta lines to the implementer as the acceptance list.
- Theme tokens: `node tools/parity_lab/theme_tokens.mjs --attach 9334 --out tokens.json` (raw + engine-resolved CSS variables) → `python -I tools/parity_lab/theme_tokens_rs.py tokens.json out.rs`.
- Same-account official reference (tester1 = Rust account, owner menus): `../Logs/parity/run_web.py tester1 scenes.json OUT --height 800`; scene actions support `click`, `context` (right click), `hover`, `click_text`, `key`; each scene also writes `.svgs.json`. Web has no titlebar: add 32 to y for desktop coordinates.
- Tests: `cd fluxer_client; $env:CARGO_TARGET_DIR="<repo>\target\fc-root"; cargo test --workspace -j 4 -- --test-threads=2`; voice_worker: `cd fluxer_client\voice_worker; $env:CARGO_TARGET_DIR="<repo>\target\vw"; cargo test --release -j 4`.

### 10.8 명세 루프 표준 절차
`AGENTS.md`의 "UI 1:1 작업" 5절(작업 흐름)로 옮겼다.

### 10.9 자동 진행 모드
`AGENTS.md`의 "UI 1:1 작업" 5절(작업 흐름)로 옮겼다.

