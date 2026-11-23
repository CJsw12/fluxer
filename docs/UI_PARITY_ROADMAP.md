# Fluxer GPUI 1:1 UI 패리티 로드맵

> `fluxer_client/`(Rust GPUI 앱)의 화면·동작을 공식 Fluxer(`fluxer_app/`, 데스크톱 Electron)와 1:1로 맞추는 **유일한 활성 실행 계획서**다.
> - 새 세션은 `AGENTS.md` → `PROGRESS.md` → 이 문서의 미완료(`[ ]`) 첫 항목 순서로 진행한다. 검증·측정 방법과 운영 절차는 `docs/PARITY_LAB.md`(특히 8장 사각지대, 10장 운영 가이드)를 따른다.
> - 프로토콜·상태 레퍼런스 `docs/rust_client_core/{GATEWAY_PROTOCOL,REST_AND_AUTH,STATE_MODEL}.md`는 읽기 전용 참조다.
> - 끝난 M1~M8의 상세 작업 내역은 Git 이력(아래 커밋)과 로컬 보관본 `docs/archive/UI_PARITY_ROADMAP-M1-M8.local.md`에 있다.

---

## 완료 요약 (M1~M8)

| 단계 | 내용 | 커밋 |
|---|---|---|
| M1 | 공용 렌더러 재사용, 공용 아바타, Phosphor 아이콘 교체 | `324718aa8`·`108d53231`·`1b9d2167d` |
| M2 | 공식 치수(`ThemeSizes`) 동기화, 중앙 음성/비디오 스테이지, 커스텀 타이틀바·Appearance | `301fade44`·`1725a44fa`·`52afe5f9a` |
| M3 | 오프라인 12장면 시각 갤러리, 공식 앱·시험 서버 대조 | `47c1ced3a`·`2e9df9f26`·`55cf2c879`·`b512e911c` |
| — | 패스키·보안 키(WebAuthn/FIDO2) | `4b4138821` |
| M4 | 공식 소스 기반 정규식 감사, 음성 상태 머신, 사용자 영역·통화 화면·설정 12개 탭 | `80b09962a` |
| M5 | 팝아웃·레일 아이콘·모달 탭·사이드바 음성 행 | `16d75565f` |
| M6 | 로그인 상태 장면 제어(`fluxer://scene/*`), 임베드·멘션 강조 | `393e32888` |
| M7 | 감사 v2, 레일 호버·인디케이터, 음성/비디오 설정 컨트롤 | `94a387c38` |
| M8 | 좌측 열 통합, 아이콘 5종, 호버 전용 버튼, 통화 스테이지 | `5bd83899d` |
| Parity Lab | CDP 실측 비교 체계 구축과 메인 화면 1~5차 수렴 | `82bed49a3`·`9acf3254c`·`00dbe2159`·`df21a8585`·`7ddeff5ed` |

> 주의: M1~M8의 "완료"는 정규식 감사·합성 갤러리 기준이었다. 2026-11-24 실측에서 상당수가 공식과 다르다는 것이 확인됐으므로(아래 M9), 완료 표시를 동작 확인으로 간주하지 않는다.

---

## 마일스톤 체크리스트


### M9. 검증 사각지대 보완 — 상태 행렬 · 조작 · 기능 시험 (다음 세션 최우선)
> **목적:** 2026-11-24 사용자가 지적한 7건(앱 내부 크기 조절 미구현, 화면 공유 송수신 미동작, 레일 호버 동작 차이, 왼쪽 아래 프로필 클릭 결과 차이, 서버 이니셜 글자 모양, 테두리·모서리·사용자 영역 카드 모양, SVG 아이콘 모양)을 고치고, 같은 종류의 차이가 다시 빠지지 않도록 Parity Lab의 비교 범위를 "정지 화면 모양"에서 "상태·조작·기능"으로 넓힌다. 원인 분석은 `docs/PARITY_LAB.md` 8장 첫 항목.

- [ ] **M9.1 조작 요소 목록 자동 추출과 대조**
  - 공식 소스(`fluxer_app/src`)에서 크기 조절·드래그 손잡이(`resize`, `drag-handle`, `splitter`, `resize-pill`), 툴팁, 우클릭 메뉴(`onContextMenu`), 키보드 단축키(keybind 정의), 호버 전용 요소(`:hover`로만 보이는 버튼·액션 바)를 스크립트로 뽑아 `tools/parity_lab/interactions.json`을 만든다.
  - Rust 쪽은 같은 `data-flx` 태그·기능 유무로 대조해 "없음/있음/동작 다름" 목록을 낸다.
  - **수락 기준:** 목록이 생성되고 누락 항목마다 작업 항목이 생긴다.
- [ ] **M9.2 상태 행렬 캡처**
  - 장면마다 기본, 각 조작 요소 호버, 누름·선택, 음성 참가 중, 음소거·스피커 끔, 화면 공유 중, 읽지 않음·멘션 상태를 양쪽 앱에서 같은 방법으로 만든 뒤 캡처·비교한다.
  - 공식은 CDP 마우스 이동·API, Rust는 `probe_hover`·딥링크(`fluxer://scene/voice/...`)를 쓴다.
  - 크롤러에 호버 순회(`mouseMoved` 후 새로 생긴 툴팁·스타일 변화 기록)를 추가한다.
  - **수락 기준:** 레일 호버 툴팁·탐색 버튼 호버 모양·길드 아이콘 음성 활동 배지 차이가 보고서에 자동으로 나온다.
- [ ] **M9.3 끌기 비교 (`probe_drag`)**
  - 프로브에 끌기 명령을 추가하고, 크롤러는 손잡이를 끌어 최소·최대·기본 크기와 키보드 조절을 측정한다.
  - 채널 목록 폭, 스레드 패널, 통화↔채팅 높이, 떠 있는 창 크기 조절을 공식 동작대로 구현한다.
- [ ] **M9.4 팝오버·모달 내용 비교를 작업 목록으로 연결**
  - 크롤 그래프의 모든 상태에 대해 `replay.py` 결과(`trigger_missing`/`opened_partial`)를 상태별 작업 항목으로 자동 생성한다.
  - 각 항목에는 공식 스크린샷·DOM·트리거 경로를 붙인다. 첫 대상은 왼쪽 아래 사용자 영역 팝오버(공식 프로필 카드 [12,304,305,432]: 배너·아바타·이름·맞춤 상태·`Online ▸`·`Switch accounts ▸`·`Copy user ID`·`Edit profile`)다.
  - 크롤은 쓰기 가드를 켠 상태로만 실행하고, 끝나면 `docs/PARITY_LAB.md` 10.5 복구 점검을 한다.
- [ ] **M9.5 기능 자동 시험(사람 개입 없이)** — 1차 완료(2026-11-24): `tools/parity_lab/share_check.py` + `voice_official.mjs`로 양방향 화면 공유를 자동 시험한다(두 앱 음소거 참가 → Rust 공유 → 공식 시청·프레임 확인 → 공식이 Rust 창 공유 → Rust 시청·색·프레임 확인 → 정리). 결과: Rust→공식 2560×1440·약 14fps 통과. 공식→Rust는 수신 색이 틀어지는 결함(워커가 libyuv `RGBA`를 RGBA 바이트로 오인, `ABGR`이 맞음)을 찾아 고친 뒤 통과(붉은 기운 0.15→0.002). 남은 항목:
  - 화면 공유 품질 설정(해상도·FPS·코덱·비트레이트)이 워커에 전달되지 않는다(`SetScreenShare`는 켜기/소리만). 실제 송출은 720p·약 14fps.
  - Rust에는 공유 대상 선택 창(응용 프로그램/전체 화면/장치, 스트림 설정)이 없고 항상 주 모니터를 공유한다.
  - Rust 시청 화면이 공식과 다르다(시청 버튼에 내부 식별자 `user_<id>_<세션>` 노출, 공식은 스테이지를 채우고 헤더에 "<이름>'s screen · 720p 14 FPS", 하단 참가자 줄).
  - 음성(말하기 표시·오디오 레벨) 자동 판정은 아직 없다.
  - 양쪽 앱이 시험 서버의 같은 음성 채널에 참가한다.
  - Rust가 화면 공유를 시작하면, 공식 비교 앱이 원격 영상 트랙의 프레임 수신(`getStats`·`videoWidth > 0`·프레임 증가)을 확인한다. 반대 방향도 시험한다.
  - 음성은 말하기 표시(speaking)와 오디오 레벨로 판정한다.
  - **수락 기준:** 화면 공유 실패가 자동 시험에서 재현되고, 수정 뒤 통과한다.
- [ ] **M9.7 요소 단위 픽셀 비교**
  - 짝지은 요소마다 두 스크린샷의 같은 박스를 잘라 픽셀 차이·구조 유사도(SSIM)를 계산한다.
  - 글자 모양(서버 이니셜 등), 아이콘 모양, 테두리·모서리 차이를 수치로 잡고, 차이가 큰 요소를 잘라낸 비교 이미지와 함께 보고한다.
  - 중앙값 색 비교는 보조로만 쓴다.
- [ ] **M9.8 스타일 속성 비교 확대**
  - 캡처 항목에 변별 테두리 두께·색, 모서리별 반경, `boxShadow`, `outline`, `letterSpacing`, `textTransform`을 추가하고, 비교기가 이 값을 판정한다.
  - Rust 쪽은 픽셀 기반(가장자리·모서리 샘플)으로 대조한다.
  - 첫 대상:
    - 메인 콘텐츠 프레임 1px 테두리와 왼쪽 위 14px 모서리
    - 사이드바·멤버 목록 프레임 반경
    - 사용자 영역 카드
    - 서버 이니셜(16px/400)
- [ ] **M9.9 공식 아이콘 원본 수확**
  - 실행 중인 공식 앱의 모든 상태(크롤 그래프 포함)에서 아이콘 `data-flx`의 SVG 마크업(경로·viewBox·크기·굵기)을 추출해 기준 세트로 저장한다.
  - Rust 아이콘 자산을 이 원본으로 교체한다. Fluxer 자체 아이콘(`ui.icons.*`)과 굵기 차이를 포함한다.
  - 경로 일치를 확인하는 테스트를 추가한다.
  - 기준 세트에 공식 빌드 문자열을 함께 기록한다.
- [ ] **M9.10 기준 버전 고정**
  - 비교 앱의 자동 업데이트를 막는다(`FLUXER_DISABLE_MODULE_UPDATES`).
  - 저장소 `upstream/main`을 그 빌드(현재 Web 2026.1120)에 맞추거나, 저장소 `fluxer_app`을 직접 빌드해 기준으로 쓴다. 버전이 다르면 비교 결과에 경고를 낸다.
- [ ] **M9.11 테마 토큰 자동 생성**
  - 공식 `fluxer_app/src/features/theme/variables/ThemeVariableManifest.ts`(다크·라이트·기타 테마 해석값)를 읽어 Rust 테마 상수를 생성한다.
  - 손으로 옮긴 토큰을 대체하고, 생성 결과와 공식 값 일치를 테스트한다.
- [ ] **M9.12 컴포넌트 헤드리스 스냅샷**
  - GPUI 헤드리스 렌더로 주요 컴포넌트(레일 항목, 채널 행, 메시지, 팝오버)를 상태별로 그려 구조(JSON)와 픽셀(허용 오차) 스냅샷을 CI에서 비교한다.
  - crates.io 릴리스에 오프스크린 렌더가 없으면 Zed 커밋 고정 여부를 먼저 검토한다.
- [ ] **M9.13 웹뷰 혼용 판단 기준**
  - Discord처럼 1:1 비용이 큰 영역(영상·화면 공유 뷰어 등)은 WebView2 혼용을 선택지로 평가한다.
  - 영역마다 "네이티브 구현 / 웹뷰" 결정과 근거를 기록한다.
- [ ] **M9.6 진행도 집계 기준 변경**
  - `docs/progress/progress.json`에 `verified`(자동 기능 시험 또는 실측 비교 통과)를 따로 두고, 동작을 끝까지 확인하지 않은 항목은 "구현"이 아니라 "미검증"으로 센다.
  - README 배지도 이 기준으로 다시 계산한다.

---

## 표준 검증·빌드 명령 (모든 에이전트 공통)

```powershell
# 1. 앱 단위·헤드리스 GPUI 테스트 (target/fc-root 재사용, 부하 제한 준수)
$env:CARGO_TARGET_DIR = (Join-Path $PWD "target\fc-root")
cargo test --manifest-path fluxer_client/Cargo.toml -p fluxer_client_app --bin fluxer_client --jobs 4 -- --test-threads=2

# 2. 전체 워크스페이스 테스트 + voice_worker 테스트
cargo test --manifest-path fluxer_client/Cargo.toml --workspace --jobs 4 -- --test-threads=2
cargo test --manifest-path fluxer_client/voice_worker/Cargo.toml --jobs 4 -- --test-threads=2

# 3. Windows 릴리스 패키징 (dist/fluxer_client/fluxer_client.exe)
pwsh -NoProfile -File scripts\package_fluxer_client.ps1

# 4. 진행도 SVG 및 README 동기화 (progress.json 변경 시)
python -I scripts/progress_svg.py
```

---

## 필수 불변 조건 (인계 시 반드시 준수)

- **Safe Rust:** 비즈니스 로직·UI는 Safe Rust만 사용하며 비테스트 `unwrap()`/`expect()`/`unsafe`를 금지한다.
- **GPUI 규칙:** `uniform_list` / `MessageScroller` 렌더·레이아웃 콜백에서 상태 변경이나 `cx.notify()`를 호출하지 않는다.
- **실행 환경 보호:** 실행 중인 공식 앱(`Fluxer.exe`)을 종료하지 않으며 `HKCU\Software\Classes\fluxer`를 덮어쓰지 않는다(격리 실행 시 `--no-os-integration` + 별도 `LOCALAPPDATA` 사용).
- **시험 안전:** 실기 검증은 자체 테스트 서버·시험 계정에서만 수행하며, CAPTCHA/ALTCHA를 스크립트로 우회하지 않는다. 공개 문서·커밋에 로컬 접속 정보·IP·도메인·개인 경로·토큰을 남기지 않는다.
- **인계 갱신:** 각 마일스톤 완료 시 이 문서의 체크박스(`[x]`)와 루트 `PROGRESS.md`의 `Baseline`/`Done`/`Next Steps`를 함께 갱신하고 관련 파일만 선별해 `origin/desktop-rs`에 커밋·푸시한다.
