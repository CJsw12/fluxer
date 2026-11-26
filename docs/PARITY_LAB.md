# Parity Lab — 공식 Fluxer와 Rust 앱의 1:1 자동 비교·탐색 체계

> 작성 2026-11-23. 목표는 Rust GPUI 앱(`fluxer_client/`)의 모든 화면과 모든 버튼으로 열리는 창을 공식 Fluxer와 기능·모양 모두 같게 만드는 것이다. 이 문서는 기존 검증 방법의 한계, 새 방법의 설계와 실측 결과, 남은 과제를 정리한다.

## 1. 결론

- 기준값은 사람이 고른 스크린샷이 아니라 **공식 앱 자체의 DOM**이다. 공식 앱은 웹 기술(React)로 만들어져 있고 소스에 `data-flx` 식별자가 약 18,700개 박혀 있다. 이 값은 프로덕션 빌드에도 남는다. Chrome DevTools Protocol(CDP)로 공식 앱을 구동하면 요소별 위치·크기·계산된 CSS·텍스트·접근성 트리를 정확한 숫자로 얻을 수 있다.
- Rust 앱의 요소에 **같은 `data-flx` 문자열**을 붙이고, 앱이 그 요소들의 경계를 IPC로 내보내게 하면 두 앱을 요소 단위로 조인해 차이를 픽셀 수치로 계산할 수 있다.
- 공식 앱의 모든 클릭 가능한 요소를 자동으로 누르는 **크롤러**가 "버튼을 누르면 뜨는 창" 목록(상태 그래프)을 만든다. Rust 앱은 같은 ID를 같은 순서로 눌러 같은 상태가 나오는지 대조한다.
- 이 세 가지가 맞물리면 사람이 장면을 고르거나 눈으로 비교하지 않아도 "무엇이 몇 px 다르다/무엇이 없다"가 기계적으로 나오고, 에이전트가 그 보고서를 입력으로 수정·재검증을 반복할 수 있다.

## 2. 기존 방법의 한계 (실측 근거)

| 방법 | 한계 | 근거 |
|---|---|---|
| `scripts/audit_ui_parity.py` 정규식 감사 | 셀렉터·문자열 존재만 확인하고 치수·배치·색은 보지 않는다. | 184/184 통과 상태에서 Parity Lab 기하 비교는 #markdown 화면의 태그 요소 53개 중 4개만 ±2px 안이었다. |
| 오프라인 합성 갤러리 | 실제 서버 데이터에서만 생기는 차이를 놓친다. | M6에서 실제 YouTube 임베드 중복 썸네일을 갤러리가 잡지 못했다. |
| 로그인 실기 캡처 하네스 | 장면이 실제로 열렸는지 판정하지 않는다. | M8 재캡처 32장이 모두 `Reconnecting…` 상태의 같은 화면이었는데 종료 코드 0이었다(존재하지 않는 CA 파일을 `SSL_CERT_FILE`로 넘긴 탓). |
| 영역별 픽셀 MAE diff | 어느 요소가 몇 px 어긋났는지, 원인 속성이 무엇인지 알려주지 못한다. | 영역 MAE 수치만 남아 수정 대상을 사람이 다시 찾아야 했다. |
| 수작업 시나리오 목록(32장) | 버튼으로 열리는 창 대부분이 목록에 없다. | 크롤러는 #markdown 한 화면에서 서로 다른 팝아웃·모달 상태 25개를 찾았다. |
| 기준 버전 미고정 | 비교 대상이 서로 다른 버전이다. | 저장소 `fluxer_app` 2026-11-16, 시험 서버 웹 `2026.1117`, 포터블 데스크톱 번들 `Web 2026.1120`이 섞여 있었다. 웹 빌드에만 `Members` 항목과 다운로드 레일 버튼이 있다. |

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

## 7. 첫 비교에서 드러난 차이 (#markdown, 1280×800)

- 채널 헤더가 멤버 목록 위까지 이어지지 않는다(공식 폭 886px, Rust 624px). 공식은 멤버 목록이 헤더 아래(y=89)에서 시작한다.
- 헤더 아이콘 버튼이 공식 32×32(아이콘 24px)인데 Rust는 24×24다. 타이틀바 창 버튼도 공식 48px 폭, Rust 34px다.
- 텍스트 줄 높이가 공식 20px, Rust 23px이라 채널·멤버·메시지 행 위치가 누적해서 어긋난다.
- 컴포저가 공식보다 24px 위에 떠 있고, 사용자 영역 글꼴이 공식 14px/11px과 다르다.
- 웹 빌드의 `Members` 채널 항목은 Rust에 없다(데스크톱 빌드 버전에는 없어 버전 고정이 먼저 필요하다).

### 7.1 수렴 진행 (2026-11-24, 다크 데스크톱 기준 #markdown)

| 단계 | 커밋 | 결과 |
|---|---|---|
| 첫 측정 | `82bed49a3` | 기하 4/53 (±2px) |
| 1~3차 | `00dbe2159` | 헤더가 멤버 목록 위로, 32px 아이콘 버튼, 요소별 줄높이, 하단 사용자 영역 겹침, `background_secondary_lighter`·텍스트 토큰, Segoe UI Variable Text 글꼴, 12시간제 타임스탬프 |
| 4차 | `df21a8585` | 타이틀바 색, 레일 fill 아이콘(Phosphor 원본), 외곽선 별, 사용자 영역 간격 |
| 현재 | — | 셸(타이틀바·레일·사이드바·헤더·멤버 목록·사용자 영역·컴포저)은 대부분 0~1px. 남은 차이는 메시지 영역(아래 고정 목록이라 바닥 여백 → 메시지 높이 순으로 맞춰야 함), 스포일러 박스 미표시, 코드 블록 테두리, 목록 글머리 크기, 컬러 이모지 대신 글리프 |

## 8. 남은 과제

- **검증 사각지대(2026-11-24 사용자 지적 4건, 실측 확인):**
  1. **앱 내부 크기 조절 미구현** — 공식: 채널 목록 폭(`app.sidebar-resize-handle`), 스레드 옆 패널, 통화↔채팅 높이(`voice.compact-voice-call-view…resize-pill`, DM 통화 구분선), 떠 있는 창(PiP, 고정 메시지·받은 편지함·이모지 선택 팝오버, Quick CSS 편집기). Rust: 없음. **못 잡은 이유:** 크롤러는 클릭 대상만 다루고, 비교는 고정 크기 정지 화면 하나였다.
  2. **화면 공유 송수신 미동작** — **못 잡은 이유:** 이전 세션이 코드·단위 테스트만으로 "구현"으로 집계했고 실제 송수신은 한 번도 시험하지 않았다. 크롤러도 OS 대화상자를 피하려고 화면 공유·음성 참가를 제외했다. 모양 비교 도구에는 기능 판정이 없다.
  3. **레일 호버 동작 차이** — 공식: 호버 시 이름 툴팁(예: "Direct messages")이 옆에 뜨고, 탐색·추가 버튼은 점선 원을 유지하며 거의 변하지 않는다. 길드 아이콘에 음성 활동 배지(초록 스피커)가 붙는다. Rust: 툴팁 없음, 탐색 버튼이 파란 채운 원으로 바뀜, 음성 활동 배지 없음. **못 잡은 이유:** 비교와 크롤 모두 캡처 직전에 포인터를 빈 곳으로 치워 호버 상태를 한 번도 찍지 않았다.
  4. **왼쪽 아래 프로필 클릭 결과 차이** — 공식: 레일·사이드바 위에 떠 있는 프로필 카드 [12,304,305,432](배너, 아바타, 이름, 사용자명, 맞춤 상태, `Online ▸`·`Switch accounts ▸` 하위 메뉴, `Copy user ID`, `Edit profile` 버튼). Rust: 하단 패널 위 평면 목록(`Set Status` + 상태 4개, `Edit Profile`, `Switch Accounts`). **못 잡은 이유:** 크롤 상태 023(사용자 메뉴)으로 이미 기록됐고 재생에서도 `trigger_missing`으로 나왔지만, 셸 정지 화면 수렴을 먼저 하느라 팝오버 내용 비교를 작업 목록으로 옮기지 않았다. 이전 M5.4 구현은 공식 구조를 보지 않고 만든 것이었고, 정규식 감사는 존재 여부만 확인했다.
  5. **서버 이니셜 글자 모양 차이** — 공식 `guild-icon-acronym`은 16px/400 Segoe UI Variable Text다. Rust는 기본 텍스트 스타일을 그대로 써서 굵기·크기가 다르다. **못 잡은 이유:** 비교기는 박스 위치·크기와 박스 안쪽 중앙값 색만 보므로, 44px 박스 안의 글자 모양 차이는 수치에 나타나지 않는다.
  6. **테두리 선·모서리·사용자 영역 카드 모양 차이** — 공식은 메인 콘텐츠 프레임(`app.outline-frame.frame`)에 1px 반투명 테두리(rgba(255,255,255,0.035))와 왼쪽 위 14px 둥근 모서리가 있고, 사이드바(`guild-navbar`)와 멤버 목록 프레임도 왼쪽 위 14px 반경이다. 사용자 영역 카드는 반경 8, 배경 rgb(32,30,36)이다. **못 잡은 이유:** 캡처는 `borderRadius`·`borderTopWidth/Color`를 수집했지만 비교기가 이 값을 판정하지 않았다. 그림자(`boxShadow`)·다른 변의 테두리·외곽선은 수집하지도 않았다. 중앙값 색 비교는 가장자리·모서리 차이를 가린다.
  7. **SVG 아이콘 모양 차이** — 공식 패치(`patches/@phosphor-icons__react@2.1.10.patch`)는 TypeScript 타입만 바꾸므로 버전(react 2.1.10 vs Rust가 쓴 core 2.1.1) 차이는 주원인이 아니다. 실제 원인은 두 가지다. ① 공식 자체 아이콘(`ui.icons.*`: 채널 `#`(경로 `M164.193 37.85…`), 받은 편지함(24×24 경로) 등)을 Rust가 비슷한 Phosphor 아이콘(`hash`, `tray`)으로 대체했다. ② 굵기(regular/bold/fill)가 다르다(예: 추가 버튼 공식 bold `M228,128a12…`). M1.3은 이름으로 짐작해 Phosphor로 바꿨고 정규식 감사는 파일 존재만 확인했다. 비교 앱(Web 2026.1120)과 저장소 소스(2026-11-16)의 버전 차이도 일부 아이콘 차이의 원인일 수 있어 기준 버전 고정이 필요하다.
  - 공통 원인: (a) 한 가지 상태(로그인·음성 미참가·포인터 없음·기본 창 크기)만 비교했다. (b) 비교 대상이 "그려진 모양" 중에서도 박스 위치·크기·중앙값 색뿐이라 글자 모양·아이콘 경로·테두리·모서리·그림자를 판정하지 않았고, 조작·기능도 판정하지 않았다. (c) 측정에서 나온 차이를 작업 목록으로 바로 옮기는 절차가 없었다. (d) 진행도가 동작 확인 없이 "구현"을 셌다.
  - 개선 계획은 `docs/UI_PARITY_ROADMAP.md` M9에 단계별로 적었다.

- 메시지 영역 세로 치수: 공식 메시지 = 위 2 + 작성자 줄 23 + 본문 + 아래 2, 그룹 사이 16, 마지막 메시지 아래 25. Rust는 ①·③ 메시지 본문 아래 28·24px가 더 붙고(리액션 바 등 원인 미확정) 코드 블록 메시지가 9px 작다. 인라인 스포일러 박스가 Rust에서 보이지 않고, 이모지가 컬러 이미지가 아니라 글리프로 그려진다.
- 버튼으로 열리는 창: 다크 깊이 2 크롤(`crawl-md-dark`, 52개 상태까지 저장 후 중단)을 쓰기 가드가 들어간 크롤러로 다시 돌린 뒤 `replay.py`로 Rust 체크리스트를 만들고, 커뮤니티 메뉴·알림 설정 메뉴·검색 범위 메뉴·스레드/고정/받은 편지함 팝오버·미디어/스티커/이모지 피커·사용자 메뉴·사용자 설정 모달 순서로 공식 구조를 이식하며 `.flx`를 붙인다.
- Rust 쪽 `.flx` 태그 확대: 현재 약 60개 ID. 장면당 공식 ID 약 240~410개 중 시각적으로 의미 있는 요소(크기·색이 보이는 박스, 텍스트, 버튼)를 우선 태깅한다. 팝아웃·모달도 태깅해야 크롤러 그래프를 Rust에서 재생할 수 있다.
- 두 앱을 같은 시험 계정으로 맞추기: 포터블 데스크톱 사본은 tester2, Rust 앱은 tester1로 로그인돼 있어 테마(계정 동기화)와 본인 시점 표시가 다르다. CDP로 사용자 메뉴 → `Switch accounts`를 여는 자동화는 아직 안정적이지 않다. 그동안은 같은 계정으로 로그인한 웹 캡처(`--height 768`, `--official-offset 0,32`)로 비교한다.
- 기준 버전 고정 절차: 저장소에서 공식 앱을 직접 빌드하려면 corepack의 `pnpm@11.27.0` 설치와 의존성 설치가 필요하다(미실행). 당장은 포터블 데스크톱 빌드 문자열을 기준으로 기록하고 `upstream/main`을 그 버전에 맞춘다.
- 우클릭 컨텍스트 메뉴, 호버 전용 상태(메시지 액션 바), 키보드 단축키, 스크롤 위치, 드래그.
- 글자 렌더링 차이(Chromium vs GPUI 셰이핑)는 텍스트 박스 높이·기준선 허용 오차로 다룬다. 글자 색은 박스 중앙값이 배경색에 끌리므로 텍스트 픽셀만 추린 비교가 필요하다.
- 실제 음성·영상·화면 공유, OS 알림, IME 같은 하드웨어·OS 경로는 이 체계 밖이며 사람의 실기 확인이 필요하다.

## 9. 실행 예

```bash
# 공식 데스크톱(포터블 사본)을 CDP와 함께 실행한 뒤
node tools/parity_lab/web_capture.mjs --attach 9334 --origin fluxer-app://app --out OUT/official --scenes scenes.json
MSYS_NO_PATHCONV=1 node tools/parity_lab/crawl.mjs --attach 9334 --origin fluxer-app://app --route /channels/<guild>/<channel> --out OUT/crawl --depth 2

# Rust 앱(FLUXER_PARITY_PROBE=1로 실행 중)
python -I tools/parity_lab/rust_probe.py resize 1280 800
python -I tools/parity_lab/rust_probe.py link fluxer://channels/<guild>/<channel>
python -I tools/parity_lab/rust_probe.py inspect > OUT/rust.json
python -I tools/parity_lab/rust_probe.py shot OUT/rust.png

python -I tools/parity_lab/compare.py OUT/official/<slug>.dom.json OUT/rust.json OUT/cmp --official-png OUT/official/<slug>.png --rust-png OUT/rust.png
```

Git Bash에서는 `/channels/...` 인자가 Windows 경로로 바뀌지 않도록 `MSYS_NO_PATHCONV=1`을 붙인다. 시험 계정 자격 증명은 환경 변수로만 넘기고 저장소에 두지 않는다.

## 10. 운영 가이드 (인계용)

### 10.1 상시 실행 구성
세 프로세스를 띄워 둔 채로 개발한다. 접속 정보·키·계정은 저장소 밖 비공개 위치에 있다(경로는 에이전트 메모리와 비공개 메모 참고).
1. **시험 서버 터널**: 로컬 443을 시험 서버로 포워딩하는 SSH 터널. hosts에 시험 도메인이 127.0.0.1로 등록돼 있고 서버 루트 인증서는 Windows 신뢰 저장소에 있다.
2. **Rust 클라이언트**: `dist/fluxer_client/fluxer_client.exe --no-os-integration`을 `FLUXER_PARITY_PROBE=1`과 시험 프로필용 `LOCALAPPDATA`로 실행한다. `SSL_CERT_FILE`은 설정하지 않는다(없는 파일을 가리키면 TLS가 실패해 `Reconnecting…`에서 멈춘다). 연결 확인은 `orca computer get-app-state --app fluxer_client --no-screenshot --query Status` → `Status: Online`.
3. **공식 비교 앱**: 포터블 공식 데스크톱 사본을 `--remote-debugging-port=9334`로 실행한다(다크 테마, tester2). 자동 업데이트로 기준 빌드가 바뀌지 않도록 실행 전 환경변수 `FLUXER_DISABLE_MODULE_UPDATES=1`(PowerShell: `$env:FLUXER_DISABLE_MODULE_UPDATES = "1"`)을 설정한다. 사용자가 평소 쓰는 공식 앱(`%LOCALAPPDATA%\fluxer_desktop`)과 같은 이름(`Fluxer.exe`)이므로 경로로 구분하고, 개인 앱에는 절대 붙지 않는다.

- 세 프로세스는 에이전트 도구의 백그라운드 실행(최대 2시간)으로 띄우면 시간이 지나 강제 종료된다. `pwsh`의 `Start-Process`로 분리 실행한다. 클라이언트에 줄 `FLUXER_PARITY_PROBE`·`LOCALAPPDATA`는 같은 `pwsh` 세션에서 `$env:`로 설정한 뒤 실행하면 상속된다. SSH 키 경로에 공백이 있으면 인자에 따옴표를 넣어야 하며, 그렇지 않으면 터널이 조용히 바로 종료된다.

### 10.2 한 장면 수렴 루프
1. 공식 기준 캡처: `web_capture.mjs --attach 9334 ...`(장면 JSON의 `path`, `wait_for`, `settle_ms`, `actions`). 캡처 전에 멤버 목록 표시 상태처럼 레이아웃을 바꾸는 토글을 확인한다.
2. 수정 후 `cargo test --workspace`(`target/fc-root`, `-j 4`, `--test-threads=2`) → 클라이언트 종료 → `scripts/package_fluxer_client.ps1` → 프로브를 켜고 클라이언트 재실행.
3. `check_scene.py <공식 dom.json> <공식 png> <딥링크> <출력>`: 창을 1280×800으로 강제하고 딥링크·Escape·포인터 정리 후 측정해 `report.json`과 불일치 목록을 낸다. 같은 계정 웹 캡처와 비교할 때는 `--official-offset 0,32`.
4. 불일치를 영역별로 묶어 에이전트에 넘긴다. 브리프에는 공식 사각형·글꼴·색(다크 실측)과 공식 CSS 모듈 이름, 소유 파일, 금지 파일을 적는다. 에이전트는 실측을 못 하므로 루트가 다시 측정해 수치로 판정한다.
5. 측정 수치가 개선되고 전체 테스트가 통과하면 영역 단위로 커밋·push한다. `docs/progress/progress.json` 갱신과 `scripts/progress_svg.py`는 기능 상태가 바뀔 때 실행한다.

### 10.3 위임 방식 (난이도 순)
- 모델 선택은 도구별 지침을 따른다(Claude: Opus·Sonnet·Haiku + DeepSeek flash, Antigravity: 전역 GEMINI.md의 Hydrogen·Neon·Argon + DeepSeek flash). 배정은 난이도 기준이다: 명세가 분명한 스크립트·태그 목록은 저가 모델, GPUI 구현은 중급(Sonnet·Neon급), 전략·설계·T3 검토는 상위 모델. 영역별 수정 워커는 1~2개로 파일 소유권을 겹치지 않게 나누고(동시 빌드 최대 2개, 실기 클라이언트를 조작하는 워커는 한 번에 하나), 같은 `target/fc-root`를 쓰므로 잠금 대기·링크 실패는 재시도하게 한다. 루트는 목표 수치 추출, 재측정, 통합, 커밋만 맡는다.
- **Antigravity `Orchestrator (Neon)` 모드(2026-11-25 점검):** 하네스가 계획(Neon)과 실행(`gemini-flash-worker-agy`)을 자동으로 나눈다. `invoke_subagent`·`swarm`을 부르지 않아도 실행 토큰의 약 80%가 Flash 워커로 가므로 이것도 위임으로 친다. 대신 실행 워커가 매 턴 약 25만~50만 토큰의 대화 전체를 다시 읽어 느려지므로, **기능 하나(커밋 하나)마다 새 세션**으로 시작하고, T0(검색·로그 확인)는 Flash 실행 워커가 맡는다(Gemini 구독 한도 안, 별도 비용 없음). DeepSeek(opencode, 종량제)은 본 작업과 병렬로 돌릴 수 있는 독립 작업(여러 파일 감사, 도구 스크립트 작성, 긴 조사)에만 쓴다. Claude 세션은 기존대로 DeepSeek·Haiku가 T0를 맡는다. 수정은 함수·파일 단위로 묶는다.
- **출력 예산(컨텍스트 절약, 2026-11-25 실측):** 한 기능 세션의 도구 출력 약 68만 자가 매 턴 다시 읽혀 실행 워커 컨텍스트가 평균 28만 토큰이 됐다. 큰 것부터: 클라이언트 로그 원문 4.6만 자, 전체 `git diff` 4만, 방금 쓴 파일 다시 읽기 3.3만, 이전 감사·트랜스크립트 2.7만, 명세 JSON 2.4만, DOM 덤프 2.1만. 규칙: `git diff --stat`(본문은 파일·범위 지정), 게이트는 `verify_all.py`·`cycle.py` 요약 표만 읽고, 로그·테스트 원문은 실패했을 때만 `tail`/`grep`(PowerShell `Select-Object -Last 30`)으로 필요한 줄만, 명세·DOM·inspect JSON은 원문 출력 금지(파이썬 한 줄로 필요한 flx·필드만), 방금 쓴 파일과 이전 감사·트랜스크립트는 다시 읽지 않는다.
- **데이터를 지우는 기능은 T3:** 일괄 삭제·purge·탈퇴·밴처럼 서버 데이터를 지우는 경로는 구현 후 다른 계열 모델의 독립 검토를 받고 커밋한다. 실기에서 직접 실행해 시험 데이터를 지우지 않으며(확인 모달까지만), 끝나면 10.5 복구 점검을 한다.
- 측정에서 얻은 사실을 브리프에 그대로 넣는다(예: GPUI 기본 줄높이는 글자 크기의 약 1.6배라 요소마다 `line_height`가 필요, 메시지 목록은 아래 고정이라 위쪽 위치는 아래 높이 합에 따라 결정, gpui-component `Button`은 아이콘 색을 덮어쓰므로 `Icon::new(..).text_color(..)`로 넘김).
- 아이콘 SVG는 기억으로 쓰지 말고 Phosphor 원본 패키지(`@phosphor-icons/core` `assets/{regular,fill}`)에서 복사한다.

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

## 11. 외부 사례 조사 (2026-11-24)

확인 표시: **[직접 확인]** = 루트가 원문을 열어 대조함, **[2차]** = 조사 에이전트 보고만 있음.

- **Discord [직접 확인, 2차 기사 경유]:** CTO Stanislav Vishnevskiy가 2026-11-04 X에서 데스크톱 클라이언트를 Rust + **GPUI**(Discord 자체 수정 포함)로 다시 만든다고 밝혔다. Electron과 React Native를 걷어내고, 웹뷰는 영상·Activities처럼 가치가 있는 곳에만 남긴다. 비공개 테스트 직전이며 출시일은 없고 Electron 앱은 병행 갱신한다(https://www.andrealosavio.com/en/blog/discord-rust-client-rewrite, 원문 X 게시물은 열지 못함). 시각 회귀·디자인 시스템 운영 방식은 공개 자료를 찾지 못했다.
- **Fluxer [직접 확인]:** 데스크톱은 Electron이고 웹과 같은 코드를 쓰며 네이티브 Rust 모듈이 20개 넘게 들어 있다. 웹·데스크톱을 Rust로 다시 만들 계획은 없다. 별도로 Flutter 앱 "Oxygen"(현재 iOS·Android)의 데스크톱판을 개발 중이며 출시일은 없다(https://fluxer.com/help/is-fluxer-electron). 즉 공식 팀도 네이티브 클라이언트는 웹 UI 복제가 아닌 별도 구현(Flutter)으로 간다.
- **Electron → Rust 전환 유형 [2차]:** ① Tauri 계열(GitButler, Spacedrive 등)은 웹 UI를 OS WebView로 그대로 써서 패리티 문제 자체를 피한다. ② Zed·Warp·Lapce는 처음부터 네이티브 디자인이라 웹 UI 복제 사례가 아니다. ③ 웹 UI를 Rust에서 직접 렌더링하는 Dioxus Blitz(Stylo + Taffy + Parley + Vello)는 README 기준 beta이고 JS가 필요한 앱은 범위 밖이다. Servo는 2026-04 crates.io 0.1.0, 임베딩 API가 자주 바뀐다.
- **검증 기법 [2차]:** GPUI 헤드리스 렌더(Zed `HeadlessAppContext`, wgpu 오프스크린 PR #62341 — crates.io 릴리스에는 없어 Zed 커밋 고정 필요), 커뮤니티 골든 이미지 도구(gpui-shot 등, 성숙도 미검증), 구조화 스냅샷(insta), 토큰 단일 출처(Style Dictionary). 웹↔네이티브 렌더러 간 패리티 검증의 공개 사례는 찾지 못했다.

### 우리 계획에 대한 시사점
1. 가장 가까운 선례는 Discord다. 같은 GPUI를 쓰지만 기존 웹 UI의 픽셀 복제가 아니라 자체 수정한 GPUI 위의 새 구현이고, 복제 가치가 낮거나 어려운 영역(영상, Activities)은 웹뷰로 남긴다. 우리도 1:1 비용이 큰 영역(화면 공유 뷰어, 일부 설정 화면 등)에 WebView2 혼용을 선택지로 둔다(`fluxer_desktop_rs`가 이미 WebView2 셸).
2. 웹↔네이티브 패리티를 자동 검증하는 공개 도구가 없으므로, 지금의 Parity Lab(CDP 실측 ↔ Rust 프로브 조인)을 계속 주 경로로 쓴다. 외부 사례가 이를 대체하지 않는다.
3. 토큰 드리프트는 공식 `ThemeVariableManifest.ts`를 단일 출처로 삼아 Rust 테마 상수를 생성하는 방식으로 막는다(Style Dictionary와 같은 발상, 손으로 옮기지 않음).
4. 컴포넌트 단위 회귀는 GPUI 헤드리스 스냅샷(구조는 insta JSON, 픽셀은 허용 오차)으로 CI에서 잡는다. 실기 비교는 장면 단위로만 한다.
5. Blitz·Servo로 공식 HTML/CSS를 그대로 그리는 방식은 공식 앱이 JS 의존이라 현재는 부적합하다. 정적 부분 실험 정도만 가치가 있다.

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
- State & surface gates: `python -I tools/parity_lab/state_matrix.py --strict --launch-rust dist/fluxer_client/fluxer_client.exe --out OUT` (blocks `phantom_hover_ui` spikes and missing tooltips); `python -I tools/parity_lab/check_surface_parity.py --launch-rust dist/fluxer_client/fluxer_client.exe --out OUT` (seam, member corner, hover phantom UI, Media/Stickers/Emojis split, settings contrast).
- Screen share both ways: `python -I tools/parity_lab/share_check.py --guild G --channel C --out OUT` (needs release client with worker).
- Icons: `python -I tools/parity_lab/check_icons.py` (manifest + originals + call-site sync, must be 0 errors); after moving or adding `AppIcon::` call sites run `--sync` and mark sites verified only with same-size official evidence; ground truth list `tools/parity_lab/official_icons.json`.
- Spec loop (preferred over eyeballing): `node tools/parity_lab/spec_official.mjs --attach 9334 --viewport 1280x800 --out SPEC.json [--hover <flx>]` (read-only CDP: computed box/paint props, matched CSS-module rules, floating) → `python -I tools/parity_lab/spec_delta.py SPEC.json --live [--scope <flx-prefix>]... --out-md OUT.md [--max N]` (one line per delta with CSS rule and Rust `file:line` from the probe's `src`/`style`). Hand the delta lines to the implementer as the acceptance list.
- Theme tokens: `node tools/parity_lab/theme_tokens.mjs --attach 9334 --out tokens.json` (raw + engine-resolved CSS variables) → `python -I tools/parity_lab/theme_tokens_rs.py tokens.json out.rs`.
- Same-account official reference (tester1 = Rust account, owner menus): `../Logs/parity/run_web.py tester1 scenes.json OUT --height 800`; scene actions support `click`, `context` (right click), `hover`, `click_text`, `key`; each scene also writes `.svgs.json`. Web has no titlebar: add 32 to y for desktop coordinates.
- Tests: `cd fluxer_client; $env:CARGO_TARGET_DIR="<repo>\target\fc-root"; cargo test --workspace -j 4 -- --test-threads=2`; voice_worker: `cd fluxer_client\voice_worker; $env:CARGO_TARGET_DIR="<repo>\target\vw"; cargo test --release -j 4`.

### 10.8 명세 루프 표준 절차 (도구 공통, 2026-11-25)
판정은 공식 앱 실측 수치로만 한다. 스크린샷 육안 비교는 마지막 확인용이다.

1. **측정 전 점검(매번):** Rust `Status: Online` · Rust 멤버 목록 `Online—2`(아니면 공식 앱 게이트웨이가 끊긴 것이므로 CDP `Page.reload`) · `curl 127.0.0.1:9334/json/version` 응답 · 양쪽 마이크·스피커 켜짐(접근성 이름이 `Mute microphone`인지 확인; 토글은 상태를 읽은 뒤에만).
2. **공식 명세:** 기본 화면은 `spec_official.mjs --attach 9334 --viewport 1280x800 --out X.spec.json`, 열린 상태는 `--scenes <scenes.json> --out-dir <dir>`. 장면 파일: `tools/parity_lab/spec_scenes.json`(팝오버·모달 8개), `spec_scenes_more.json`(DM·탐색·설정 탭), `spec_scenes_msg.json`(메시지 마우스 올림 바). 공식 앱은 tester2, Rust는 tester1이라 본인 시점·상대 이름·색은 결함이 아니다.
3. **차이 목록:** `spec_delta.py X.spec.json --live` 또는 `--scenes ... --spec-dir ... --live --out-dir ...`. 줄마다 공식 CSS 규칙과 Rust `파일:줄`이 붙는다. `floating`, 1~4px 글자 폭(글꼴 메트릭)은 대개 결함이 아니다.
4. **누락 분류:** `coverage.py <spec> <delta>` 또는 `--scenes-dir`. 대부분은 "그려지지만 태그 없음"이므로 먼저 `.flx()` 태그를 붙여 결합 수를 늘린다(`.flx()`는 `.border_*()` 뒤).
5. **수정:** 워커에게 아래 브리프 양식으로 넘기고, 워커가 `cycle.py --skip-tests`로 직접 빌드·재측정해 "범위 안 차이 0줄"을 맞춘다.
6. **루트 재검증(커밋 전 필수):** `python -I tools/parity_lab/verify_all.py --build [--worker]` 하나로 실행한다(측정 전 점검·아이콘·하드코딩 색·래칫·크기 조절·표면·상태 행렬을 돌리고 10줄 표만 출력, 원문은 `target/parity-tmp/verify/<시각>/<단계>.log`). 실패한 단계의 로그만 필요한 줄을 골라 읽는다. 아래는 개별 명령 참고용: `cycle.py --release`(테스트 포함, 실패 시 패키징 전 중단) → `spec_gate.py --live --all`(래칫) → `check_icons.py` → `check_resize.py` → `check_surface_parity.py` → `state_matrix.py --strict` → 해당 화면 스크린샷 육안 확인. 게이트는 상시 실행 클라이언트로 돌리고 `--launch-rust`는 쓰지 않는다.
7. **래칫 갱신:** 차이가 줄면 `spec_gate.py --live --all --update`. 올려야 하면 `--allow-raise --reason "<15자 이상 사유>"`가 필수이고(사유는 `spec_baseline_history.jsonl`에 기록), +20%·+10 초과 상향은 `--confirm-large`까지 요구한다. 새 하드코딩 색은 `check_colors.py`가 막으며, 공식 CSS 리터럴처럼 토큰이 아닌 값만 사유와 함께 `color_allowlist.json`에 올린다.

**워커 브리프 양식(요약):** 목표 1줄 / 입력(공식 명세·차이 목록·스크린샷 경로, 모두 `target/parity-tmp/`) / 루트가 이미 판정한 비결함 목록 / 실제 결함 목록(공식 수치·CSS 규칙·Rust 위치) / 루프 명령 / 회귀 기준(기존 장면 차이 상한, 래칫 OK, 아이콘 0 오류) / 소유 파일·금지 파일 / 규칙(GPUI `uniform_list` 콜백 상태 변경 금지, 숨은 호버 버튼 금지, 버튼마다 고유 핸들러, 기능 없는 항목은 `TODO(backlog)`로 남기고 보고, 하드코딩 색 대신 테마 토큰) / 출력 예산(10.3: `git diff --stat`, 게이트는 `verify_all.py` 요약 표, 로그·테스트 원문은 실패 시 `tail`/`grep`으로 필요한 줄만, JSON 원문 출력 금지, 방금 쓴 파일 재독 금지 — 하위 에이전트는 이 문서를 읽지 않으므로 브리프에 그대로 넣는다) / 보고 형식(수정 전후 표, 남은 차이와 이유, 테스트 합계, 끝 상태).

### 10.9 자동 진행 모드 (오케스트레이터, 2026-11-25)
세션 하나로 백로그를 연속 처리한다. 루트는 구현하지 않고, 기능마다 새 컨텍스트의 하위 에이전트를 띄워 컨텍스트 비대를 막는다.

1. **루트 루프:** PROGRESS Next Steps에서 다음 기능 1개 선택 → 10.8 브리프 양식으로 지시 작성(입력은 `target/parity-tmp/`) → 하위 에이전트 1개 실행 → 25줄 이내 보고 수신 → 루트가 `verify_all.py --build`로 재검증(10.8 6단계) → 통과하면 커밋·push → `OPERATIONS.md` 스냅샷 한 줄 갱신 → 다음 기능. 루트는 코드 파일을 직접 고치지 않는다(게이트 실패 원인이 한두 줄일 때만 예외).
2. **하위 에이전트 배정(Antigravity):** 기능 1개 = 하위 에이전트 1개. 여러 시스템(REST·상태·UI)을 함께 설계하는 기능은 Neon급, 명세 수치 맞추기·태그·스크립트는 Hydrogen급으로 `swarm`에서 모델을 지정한다. 실기 클라이언트를 조작하는 하위 에이전트는 동시에 하나만.
3. **시간 제한(2026-11-26 사고):** 루트가 띄운 CDP 추적 스크립트가 시간 제한 없이 7시간 매달려 자동 진행이 01:48~08:40 멈췄다. 백그라운드 명령과 스크립트는 모두 시간 제한을 건다(`timeout 600 ...`, 스크립트 안 대기에는 상한, CDP 이벤트 추적은 최대 N초 후 종료). 같은 작업 대기가 15분을 넘으면 중단하고 다음 단계로 간다.
4. **루트는 디버깅하지 않는다:** 하위 에이전트가 실패하면 루트가 직접 추적·수정하지 말고, 실패 원인을 담아 브리프를 좁혀 새 하위 에이전트를 띄운다. 2회 실패하면 그 기능을 건너뛰고(`OPERATIONS.md`에 기록) 다음 기능으로 간다. 2026-11-26 세션에서 루트가 화면 공유 기능을 직접 디버깅하다 루트 컨텍스트가 턴당 26만~29만 토큰으로 커졌다.
5. **멈춤 조건(그 외에는 묻지 않고 계속):** 백로그 소진, 같은 게이트 2회 연속 실패, T3(데이터 삭제·인증·권한·저장 형식 변경) 기능 도달(구현 전 멈추고 교차 검토 요청), 외부 승인·결과를 바꾸는 누락 정보.
6. **끝낼 때:** 멈춘 이유, 커밋 목록, 기능별 배정 모델표(작업/등급/모델/호출 방식), 남은 백로그를 보고하고, Rust 클라이언트를 샌드박스에서 띄웠다면 "클라이언트 재기동 필요"를 적는다.

