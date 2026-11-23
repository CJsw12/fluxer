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
- 안전장치: 창 제어, 채널·길드·DM 이동, 삭제·나가기·추방·차단·신고·로그아웃·전송·통화 같은 파괴적·외부 동작은 누르지 않는다. 레이어 없이 상태만 바꾸는 토글(즐겨찾기, 음소거)은 기록 직후 다시 눌러 되돌린다. 매 클릭 전 기준 화면과 ID 집합이 하나라도 다르면 경로를 다시 불러온다.
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

## 8. 남은 과제

- Rust 쪽 `.flx` 태그 확대: 현재 40개 ID. 장면당 공식 ID 약 240~410개 중 시각적으로 의미 있는 요소(크기·색이 보이는 박스, 텍스트, 버튼)를 우선 태깅한다. 팝아웃·모달도 태깅해야 크롤러 그래프를 Rust에서 재생할 수 있다.
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
