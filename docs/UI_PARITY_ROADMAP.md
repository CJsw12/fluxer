# Fluxer GPUI 1:1 UI 패리티 로드맵

> `fluxer_client/`(Rust GPUI 앱)의 화면·동작을 공식 Fluxer(`fluxer_app/`, 데스크톱 Electron)와 1:1로 맞추는 **유일한 활성 실행 계획서**다.
> - 진행 순서와 다음 할 일은 `PROGRESS.md`의 Next Steps 한 곳에서만 관리한다. 이 문서는 마일스톤의 목표와 수락 기준을 기록한다. 검증·측정 방법은 `docs/PARITY_LAB.md`, 에이전트 규칙은 `AGENTS.md`.
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

- [x] **M9.1 조작 요소 목록 자동 추출과 대조** — 완료(2026-11-26): `tools/parity_lab/extract_interactions.py`로 공식 5개 범주 총 770건(`resize_drag` 27, `tooltips` 280, `context_menus` 292, `keybinds` 108, `hover_only` 63)을 추출해 `fluxer_client/app/src`와 대조(`present` 53·`missing` 643·`differs` 74, 후속 작업 717건)하고 `tools/parity_lab/interactions.json`에 저장했다(`--selftest` 통과).
  - 공식 소스(`fluxer_app/src`)에서 크기 조절·드래그 손잡이(`resize`, `drag-handle`, `splitter`, `resize-pill`), 툴팁, 우클릭 메뉴(`onContextMenu`), 키보드 단축키(keybind 정의), 호버 전용 요소(`:hover`로만 보이는 버튼·액션 바)를 스크립트로 뽑아 `tools/parity_lab/interactions.json`을 만든다.
  - Rust 쪽은 같은 `data-flx` 태그·기능 유무로 대조해 "없음/있음/동작 다름" 목록을 낸다.
  - **수락 기준:** 목록이 생성되고 누락 항목마다 작업 항목이 생긴다.
- [x] **M9.2 상태 행렬 캡처** — 완료(2026-11-24, `8f49ada84`): `state_official.mjs` + `state_matrix.py`로 기본·호버·음성(음소거/스피커 끔/음성 무대) 상태의 DOM·툴팁·음성 영역 실측 대조를 구축했고, 실기 `state_matrix.py --voice`에서 헤더·사용자 영역 툴팁(`tips=3`)·액션 바·음성 패널(`delta=[0,0,0,0]`) 일치를 검증했다.
- [x] **M9.3 끌기 비교 (`probe_drag`)** — 완료(2026-11-24): 프로브에 `probe_drag`·`probe_dblclick`을 추가하고 `measure_resize.mjs` 실측 기준(`resize_spec.json`)에 맞춰 사이드바 폭(`200..=480`, 기본 `320`), 이모지 피커(`48px` 그리드 스냅 `400..=976` × `360..=686`, 기본 `592×686`), 분할 음성 통화 높이(`320..=590`, ±16px)의 드래그·더블클릭·키보드·환경설정 영속화를 구현·검증했다. (고정 메시지·받은 편지함 팝오버와 스레드 측면 패널은 아직 Rust 별도 화면이 없으므로 추후 이식 시 `resizable_popover`와 크기 조절 손잡이를 적용한다.)
- [x] **M9.4 팝오버·모달 내용 비교를 작업 목록으로 연결** — 완료(2026-11-24):
  - `tools/parity_lab/replay.py`에 깔끔한 Rust 기준선 캡처, `toggle_reverted` 복원, 깊이 1 `duplicate_screen` 중복 화면 검출(`--strict-no-duplicates`), 추가 팝오버 프로브(`extra_user_area_popout` 5/5·`extra_settings_modal` 2/2), 상태별 작업 목록(`backlog.json`·`backlog.md`) 생성을 구현해 59개 크롤 상태(`duplicate_screen 0`, `trigger_missing 27`, `opened_partial 32`)를 분류했다.
  - 복붙 핸들러 대신 공식 UI 흐름에 맞춰 길드 헤더 팝오버(`007`), 채널 알림 설정(`013`), 스레드(`014`), 고정 메시지(`017`), 받은 편지함(`025`), 작성창 `+` 메뉴(`043`)를 구현하고 릴리스 실기 검증(`replay.py --strict-no-duplicates`·`check_surface_parity.py` 10/10·`state_matrix.py --strict`·`check_resize.py` 25/25)과 `cargo test --workspace` 709개를 통과했다.
  - 크롤은 쓰기 가드를 켠 상태로만 실행하고, 끝나면 `docs/PARITY_LAB.md` 10.5 복구 점검을 한다. 남은 미구현/부분 항목(`trigger_missing` 27·`opened_partial` 32)은 생성된 작업 목록의 우선순위에 따라 후속 단계에서 구현한다.
- [ ] **M9.5 기능 자동 시험(사람 개입 없이)** — 구현 완료(2026-11-26, 실기 `share_check.py` 게이트 통과 시 최종 완료):
  - 1차(2026-11-24): `tools/parity_lab/share_check.py` + `voice_official.mjs` 양방향 화면 공유 자동 시험 구축 및 수신 색 변환(`ABGR`) 수정.
  - 2차(2026-11-26):
    - `voice_worker` + `voice_ipc.rs` + `voice.rs`에 `ListScreenSources`/`ScreenSources`, `SetScreenShare` 소스·해상도·FPS·코덱·비트레이트 전달(`bgra_to_i420_scaled`, `VideoEncoding`, `VideoCodec`), `RoomEvent::ActiveSpeakersChanged` -> `ActiveSpeakers` 파이프라인 구현.
    - `Overlay::ScreenSharePicker`(`ui/screen_share_modal.rs`) 모달 구현(`Screens`/`Applications` 탭, 소스 선택, `720p`/`1080p`/`1440p`/`Source`, `15`/`30`/`60` FPS, `Auto`/`H264`/`VP8`/`VP9`/`AV1`, 시스템 오디오 토글, `Go Live`).
    - `parse_participant_user_id` 공용 파서로 `"user_<id>_<session>"` 원시 노출 제거, `video_viewer::render_stage` 헤더를 `"<이름>'s screen · <height>p <fps> FPS"`(실시간 FPS) 및 하단 참가자 줄 레이아웃으로 개선, 음성 무대 말하기 테두리(speaking ring) 및 프로브 상태(`.flx_text`) 노출.
    - `share_check.py`에 `--launch-rust`, `.flx` 기반 피커·시청 조작, `voice_audio` 오디오·말하기 자동 판정(`voice_official.mjs audio` + Rust 프로브), `finally` 양쪽 정리·`#markdown` 복원·프로세스 종료 추가.
  - **실기 검증 현황(2026-11-26):** Rust 측 음성 참가·`ScreenSharePicker`(`source_count=4`)·`1080p`/`30fps` 선택·`Go Live`·`LIVE` 송출·`speaking_state` 노출은 `share_check.py --launch-rust` 실기 스크린샷으로 확인했으나, 공식 비교 앱(`Web 2026.1124.64728`)이 테스트 서버와 세션/음성 타임아웃(`GatewayReadyTimeoutError`·`Voice server timeout`)을 겪어 양방향 송수신 완료 판정은 버전 정렬 후 재검증이 필요하다.
  - **수락 기준:** 단위 테스트 통과 후 실기 `share_check.py --launch-rust`에서 `rust_to_official`, `official_to_rust`(헤더 `'s screen`·`user_` 미노출·색·움직임), `voice_audio`가 모두 통과한다.
- [x] **M9.7 요소 단위 픽셀 비교** — 완료(2026-11-26): `tools/parity_lab/compare.py`에 `official_offset`을 반영한 4x4 이상 짝지은 요소 박스별 RGB `pixel_mae` 및 휘도 기반 `ssim`(`C1=(0.01*255)^2`, `C2=(0.03*255)^2`) 계산, SSIM 최저·MAE 최고 상위 10개 나란히 비교 이미지(`<out>/worst_crops/<rank>_<safe_flx>.png`, 공식 | Rust | 3배 증폭 diff), `element_pixel_summary` 기록, `compare.py --selftest`를 구현·검증했다.
  - 짝지은 요소마다 두 스크린샷의 같은 박스를 잘라 픽셀 차이·구조 유사도(SSIM)를 계산한다.
  - 글자 모양(서버 이니셜 등), 아이콘 모양, 테두리·모서리 차이를 수치로 잡고, 차이가 큰 요소를 잘라낸 비교 이미지와 함께 보고한다.
  - 중앙값 색 비교는 보조로만 쓴다.
- [x] **M9.8 스타일 속성 비교 확대** — 완료(2026-11-26): `tools/parity_lab/spec_official.mjs`에 `outline`·`outlineWidth`·`outlineColor`·`textTransform` 등 확장 속성을 보강하고, `tools/parity_lab/spec_delta.py`에 `extended_style` 감사 및 `--extended-styles` 플래그(기본 실행은 기존 `spec_baseline.json` 0회귀 유지)를 추가했으며, `tools/parity_lab/compare.py`에 첫 대상 4개(메인 콘텐츠 프레임·사이드바/멤버 목록 프레임·사용자 영역 카드·서버 이니셜) 픽셀 기반 가장자리·모서리 감사(`style_pixel_audit`)를 추가했다.
  - 캡처 항목에 변별 테두리 두께·색, 모서리별 반경, `boxShadow`, `outline`, `letterSpacing`, `textTransform`을 추가하고, 비교기가 이 값을 판정한다.
  - Rust 쪽은 픽셀 기반(가장자리·모서리 샘플)으로 대조한다.
  - 첫 대상:
    - 메인 콘텐츠 프레임 1px 테두리와 왼쪽 위 14px 모서리
    - 사이드바·멤버 목록 프레임 반경
    - 사용자 영역 카드
    - 서버 이니셜(16px/400)
- [ ] **M9.9 공식 아이콘 원본 수확**
  - 진행(2026-11-26): `target/parity-tmp/m99_harvest/harvest_all.json`(공식 빌드 `Stable Desktop 2026.1117.40548, Web 2026.1124.64728`, 1,589건/70개 고유 `data-flx`)을 `official_icons.json`(244개 항목)에 병합하고, `icon_manifest.json` 대조로 225곳 중 113곳을 `verified`(미검증 112곳, 불일치 사유 기록)로 갱신했으며 `app_icons_load`와 `check_icons.py --selftest`로 `<path d=...>` 및 자산 일치를 검증했다.
  - 진행(2026-11-24): 같은 계정(소유자) 공식 화면 9개(길드 메뉴·사용자 팝오버·메시지/채널/길드/멤버 우클릭 메뉴·설정)에서 SVG를 수확해 `official_icons.json`에 33개를 더했다. `check_icons.py --sync/--check-sync`로 소스 호출 위치와 매니페스트를 일치시키고, 크기까지 맞는 근거가 있을 때만 검증으로 표시한다(149곳 중 80곳 검증).
  - 실행 중인 공식 앱의 모든 상태(크롤 그래프 포함)에서 아이콘 `data-flx`의 SVG 마크업(경로·viewBox·크기·굵기)을 추출해 기준 세트로 저장한다.
  - Rust 아이콘 자산을 이 원본으로 교체한다. Fluxer 자체 아이콘(`ui.icons.*`)과 굵기 차이를 포함한다.
  - 경로 일치를 확인하는 테스트를 추가한다.
  - 기준 세트에 공식 빌드 문자열을 함께 기록한다.
- [ ] **M9.10 기준 버전 고정** — 진행(2026-11-26):
  - `docs/PARITY_LAB.md` 10.1에 비교 앱 자동 업데이트 방지 환경변수(`FLUXER_DISABLE_MODULE_UPDATES=1`) 사용법을 문서화했다.
  - `tools/parity_lab/spec_official.mjs`와 `tools/parity_lab/compare.py`에 공식 캡처 `client_info`(`Web YYYY.MMDD...`)와 기준 빌드(`--expected-web-build` 또는 `spec_baseline.json`의 `official_client`, 기본 `Web 2026.1120`)가 다를 때 stderr 경고 및 JSON `version_warning` 필드를 기록하는 기능을 추가했다(`--selftest` 통과).
  - **남은 작업:** 저장소 `main`은 업스트림 `3608a387c`(2026-11-23, `desktop-rs` 병합 `3cc9d8ffb`)인 반면 현재 비교 앱은 `Web 2026.1124.64728`이라 테스트 서버와 세션/음성 타임아웃이 발생한다. 비교 앱 버전을 테스트 서버·저장소와 정렬하거나 저장소 `fluxer_app` 빌드를 기준으로 고정해야 완료된다.
- [x] **M9.11 테마 토큰 자동 생성** — 완료(2026-11-26): `tools/parity_lab/theme_tokens_rs.py`에 `--multi` 모드와 `color(srgb ...)` 해석을 추가해 `fluxer_client/app/src/theme_tokens_generated.rs`(dark 654·light 656, 총 1,310개 토큰·682개 RGBA 상수)를 자동 생성하고, `Theme::from_kind(Dark/Light)` 공식 토큰 필드를 생성 상수에 연동 및 단위 테스트로 검증했다.
- [x] **M9.12 컴포넌트 헤드리스 스냅샷** — 기술 검토·결정 완료(2026-11-26):
  - `fluxer_client/app/Cargo.toml`의 `dev-dependencies`는 `gpui = { package = "gpui-pre", version = "=0.3.12", features = ["test-support"] }`를 사용하며(`gpui-kit`과 동일 핀), 실제 확인 결과 `gpui-pre 0.3.12`의 `TestWindow::render_to_image`는 `HeadlessRenderer` 팩토리가 설정되지 않아 `"render_to_image not available: no HeadlessRenderer configured"` 오류를 반환한다.
  - **결정:** 현 단계에서는 `TestAppContext` 기반 구조적 단위 테스트와 동일 데스크톱 실기 프로브 스크린샷(`rust_probe.py`·`spec_gate.py`·`compare.py`) 체계를 유지하고, 추후 업스트림(`gpui-pre`/Zed)에서 헤드리스 렌더러가 제공될 때 컴포넌트 픽셀 스냅샷을 확장한다.
- [x] **M9.13 웹뷰 혼용 판단 기준** — 기준 수립·결정 완료(2026-11-26):
  - **결정:** 채팅·레일·사이드바·설정·팝오버·음성 스테이지 및 화면 공유 뷰어 등 핵심 UI는 모두 네이티브 GPUI를 유지한다. `fluxer_desktop_rs`(WebView2) 혼용은 향후 고비용 리치 임베드나 인터랙티브 웹 액티비티처럼 네이티브 구현 비용이 과도한 영역에만 제한적으로 검토한다.
  - | 영역 | 결정 | 근거 |
    |---|---|---|
    | 채팅·메시지·마크다운·컴포저 | 네이티브 GPUI 유지 | 가상 목록 선택·단축키·응답성과 `.flx` 실측 패리티 이미 확보 |
    | 길드 레일·사이드바·멤버 목록·DM | 네이티브 GPUI 유지 | `spec_gate` 수렴 완료, 저지연 상태 갱신 필수 |
    | 설정·팝오버·모달·단축키 편집 | 네이티브 GPUI 유지 | 공식 토큰(`Theme`)·아이콘·`AppState` 직접 연동 |
    | 음성 스테이지·화면 공유 피커/뷰어 | 네이티브 GPUI 유지 | `voice_worker` I420/BGRA 네이티브 파이프라인 및 피커 구현 완료 |
    | 고비용 리치 임베드·외부 웹 액티비티(향후) | WebView2 혼용 후보 | 복잡한 동적 JS/HTML 위젯은 네이티브 재구현 대비 비용이 과도함 |
- [x] **M9.6 진행도 집계 기준 변경** — 완료(2026-11-26): `docs/progress/progress.json` 모든 항목에 `"verified": true | false`를 명시하고 `scripts/progress_svg.py`·배지·진행 지도·`README.md` 범례에 `"미검증"`(`unverified`) 집계를 반영했다(`--selftest` 통과).

---

## 검증·빌드 명령과 공통 규칙

명령은 `docs/PARITY_LAB.md` 10.7, 공통 규칙(Safe Rust, GPUI, 빌드 부하, 시험 안전, 커밋 제외)은 `AGENTS.md`를 따른다. 마일스톤을 마치면 이 문서 체크박스와 `PROGRESS.md`를 함께 갱신한다.
