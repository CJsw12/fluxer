# Fluxer GPUI 1:1 UI 패리티 로드맵 (Multi-Agent Active Plan)

> **활성 계획서 안내 (Codex · Claude · Antigravity 공통):**
> 이 문서는 `fluxer_client/`(Rust GPUI 네이티브 앱)의 UI·컴포넌트·시각 검증을 공식 Fluxer(`fluxer_app/`)와 1:1 동등하게 맞추기 위한 **유일한 활성 실행 계획서**다.
> - 새 세션 시작 시 `AGENTS.md` → `PROGRESS.md` → 이 문서의 미완료(`[ ]`) 첫 번째 항목 순서로 진행한다.
> - 기존 `docs/archive/RUST_*_PLAN.md`, `NATIVE_MIGRATION_PLAN.md`, `docs/rust_client_core/{FEATURE_GAPS,NEXT_BRIEFS}.md`는 이전 단계 보관본이므로 작업 기준으로 삼지 않는다.
> - 프로토콜·상태 계약 레퍼런스(`docs/rust_client_core/{GATEWAY_PROTOCOL,REST_AND_AUTH,STATE_MODEL}.md`)는 읽기 전용 참조로 유지한다.

---

## 배경 및 핵심 원칙

단발성 스크린샷 눈대중 수정은 비율·아이콘·아바타·하위 화면(`Search messages` 링크 미리보기 누락, 음성 채널 접속 화면 미구현 등) 누락을 반복한다. GPUI 생태계(`Wally869/gpui-preview`, `Ziinc/gpui-visual-test`, `gpui-kit`)와 토큰 코드젠 패턴을 적용해 아래 3원칙을 강제한다.

1. **단일 렌더러 강제 재사용:** 유저 아바타, 메시지·임베드 카드, 2단 설정 셸은 별도 미니 렌더러를 만들지 않고 공용 컴포넌트를 공유한다.
2. **공식 원본 기준 치수·아이콘 동기화:** 같은 저장소의 `fluxer_app/` CSS 모듈·레이아웃 스펙·Phosphor 아이콘을 직접 기준으로 삼는다.
3. **오프라인 결정적 시각 갤러리 + 네이티브 창 대조:** 로그인·CAPTCHA 없이 12개 핵심 화면 상태를 일괄 캡처하고, 공식 네이티브 앱(`fluxer_desktop_rs` 또는 `fluxer_desktop`)과 동일 창 크기(기본 `1280×800` / 최대화)에서 영역별 비교를 수행한다.

---

## 기준선 (2026-11-23)

- 브랜치: `desktop-rs` (`e08ba4815` 이후)
- 완료된 기반:
  - 공용 2단 설정 셸 `overlay::settings_modal` + 사용자 설정창 2단 개편(`588c1ff29`)
  - 채널·역할·밴 설정 2단 셸 통합(`f7e9a18d5`)
  - 메인 셸 1차 마감(DM `Quick switcher CTRL K`, 둥근 채널/호버 아이콘, 헤더 `Search messages` 필, 통합 입력창, 플로팅 사용자 카드 `e4cc0e4fb`·`0e481aa7c`·`e08ba4815`)
  - 음성 `VOICE_SERVER_UPDATE` `https://`→`wss://` 및 루프백 `http://`→`ws://` 정규화 수정(`c5c12c81a`)
  - 자동 업데이트(에셋 선택·128 MiB 스트리밍·`SHA256SUMS.txt` 검증) 및 테스트 **348 passed**(`fluxer_client_app`) + `voice_worker` **10 passed**, 릴리스 경고 0건
- 현재 확인된 핵심 UI 결함:
  1. `Search messages` 결과(`SearchHit`)가 `msg.embeds`/`msg.attachments`/`avatar_url`을 버려 링크 미리보기·이미지·아바타가 표시되지 않음.
  2. DM 사이드바·DM 환영 배너·설정창·음성 참가자·검색 결과 등 곳곳에서 실제 프로필 사진 대신 이니셜 원이 그려짐.
  3. 공식 Fluxer는 **Phosphor Icons**(`@phosphor-icons/react`)를 쓰지만 GPUI 앱은 `gpui-kit` **Lucide** 아이콘을 사용해 전체 아이콘 모양·굵기가 다름.
  4. 음성 채널 클릭/연결 시 공식 앱처럼 중앙 영역이 음성/비디오 참가자 스테이지 화면으로 전환되지 않음.
  5. 창 크기(기본/최대화)별 여백·비율·폰트 크기가 공식 `fluxer_app` CSS 스펙과 완전히 일치하지 않음.

---

## 마일스톤 체크리스트

### M1. 공용 렌더러 강제 재사용 & 에셋 통일
- [x] **M1.1 검색 결과 임베드·이미지·아바타 복구**
  - **대상 파일:** `fluxer_client/app/src/ui/search_panel.rs`, `fluxer_client/app/src/tests_search.rs`
  - **공식 참조:** `fluxer_app/src/features/search/`
  - **작업:** `SearchHit`에 `embeds: Vec<EmbedView>`, `images: Vec<ImagePreview>`, `avatar_url: Option<SharedString>`, `user_id: Snowflake`, `document: Option<Arc<Document>>`를 보존하고, 검색 결과 카드 렌더링에서 `ui::embed`·`ui::avatar`(M1.2)·이미지 미리보기를 직접 재사용한다.
  - **수락 기준:** 링크 임베드(YouTube/일반 카드/GIF)·이미지 첨부·아바타가 포함된 검색 결과 렌더링 GPUI 테스트 통과, 기존 점프/필터/페이지네이션 테스트 무회귀.
- [x] **M1.2 공용 `ui::avatar` 컴포넌트 도입 및 전체 화면 일괄 교체**
  - **대상 파일:** `fluxer_client/app/src/ui/avatar.rs`(신규), `ui/mod.rs`, `ui/message_row.rs`, `ui/member_list.rs`, `ui/channel_list.rs`, `ui/message_list.rs`, `ui/user_panel.rs`, `ui/settings.rs`, `ui/profile_popout.rs`, `ui/search_panel.rs`, `ui/voice_panel.rs`
  - **공식 참조:** `fluxer_app/src/features/ui/components/StatusAwareAvatar.tsx`, `BaseAvatar.tsx`
  - **작업:** `render_avatar(AvatarSpec { user_id, avatar_url, display_name, size, status, status_border_bg })` 단일 컴포넌트를 만들고 앱 전체의 개별 아바타/이니셜 원 코드를 모두 교체한다. 실제 `avatar_url` 로드 + 실패/없음 시 일관된 기본 Fluxer 아바타 폴백 + 선택적 상태 점 배치를 통일한다.
  - **수락 기준:** 위 9개 화면 모두 공용 `ui::avatar` 사용, DM 행·DM 배너·설정 프로필·검색 결과·멤버 목록에서 `avatar_url` 존재 시 이미지 렌더링 확인 테스트 통과.
- [ ] **M1.3 공식 Phosphor Icons SVG 교체**
  - **대상 파일:** `fluxer_client/app/assets/icons/`(신규, MIT `@phosphor-icons/core` 원본 SVG + 라이선스 고지), `fluxer_client/app/src/icons.rs`, 각 UI 아이콘 참조부
  - **공식 참조:** `fluxer_app/package.json` (`@phosphor-icons/react`) 및 각 컴포넌트의 Phosphor 아이콘명
  - **작업:** 앱이 사용하는 모든 아이콘(길드·채널·음성·헤더·설정·입력창·메시지 액션·모달 닫기)을 공식 Phosphor SVG로 교체하고 `AppAssets`가 우선 서빙하게 연결한다.
  - **수락 기준:** `icons::tests::app_icons_load`가 모든 임베드 SVG의 유효성(`<svg`)을 검증하며 릴리스 빌드에 외부 파일 의존성 없이 내장됨.

### M2. 공식 치수 동기화 & 미구현 대형 화면 이식
- [ ] **M2.1 공식 `fluxer_app` 레이아웃 치수·색상 토큰 동기화**
  - **대상 파일:** `fluxer_client/app/src/theme.rs`, 관련 `ui/*.rs`, (선택) `scripts/extract_layout_tokens.py`
  - **공식 참조:** `fluxer_app/src/features/theme/layout/MessageLayoutSpec.ts`, `ColorSystemPalette.ts`, `ChannelChatLayout.module.css`, `SettingsModalLayout.module.css`
  - **작업:** 길드 레일 폭, 채널 사이드바 폭, 헤더 높이, 메시지 그룹 상하 패딩·아바타 크기·들여쓰기, 입력창 높이/반경, 멤버 목록 폭, 설정 2단 폭/간격을 공식 CSS 실측치와 1:1 매핑해 `ThemeSizes`에 고정한다.
  - **수락 기준:** `ThemeSizes` 단위 테스트로 공식 스펙 상수 일치 검증, 기본(`1280×800`)·최대화(`1920×1040`) 창에서 잘림/겹침 0건.
- [ ] **M2.2 음성 채널 접속 시 중앙 스테이지 화면(`VoiceCallView`) 구현**
  - **대상 파일:** `fluxer_client/app/src/ui/voice_stage.rs`(신규), `ui/mod.rs`, `client_view.rs`, `ui/channel_list.rs`, `ui/video_viewer.rs`, `tests_voice.rs`
  - **공식 참조:** `fluxer_app/src/features/voice/components/CompactVoiceCallView.tsx` 및 음성 타일/컨트롤 컴포넌트
  - **작업:** 음성 채널 클릭·연결 시 중앙 영역이 음성/비디오 스테이지로 전환되도록 구현한다(참가자 `ui::avatar` 타일 그리드 + 발화/음소거/헤드셋 상태 배지 + 원격 화면 공유 선택 시 `ui::video_viewer` 메인 스테이지 확대 + 하단 마이크/헤드셋/화면공유/연결끊기 원형 컨트롤 바 + 텍스트 채널로 돌아가기 전환).
  - **수락 기준:** 음성 채널 접속→스테이지 전환→참가자 타일/화면공유 스테이지/컨트롤 동작→연결 해제 복귀를 헤드리스 GPUI 테스트(`tests_voice.rs`)로 검증 + 실서버 음성 연결 실기 확인.
- [ ] **M2.3 커스텀 타이틀바 및 설정창 세부 컨트롤 마감**
  - **대상 파일:** `fluxer_client/app/src/ui/titlebar.rs`(또는 `root.rs`), `fluxer_client/app/src/ui/settings.rs`
  - **작업:** 설정 `Appearance`의 공식 테마 스와치 카드 및 폰트/간격 슬라이더 스타일 정교화, 필요 시 공식 데스크톱과 동일한 상단 타이틀바 스타일 적용.
  - **수락 기준:** 기존 설정·키보드·접근성 테스트 전부 통과 및 경고 0건.

### M3. 오프라인 12-시나리오 시각 갤러리 & 네이티브 창 자동 대조
- [ ] **M3.1 오프라인 시각 갤러리 하네스 구축**
  - **대상 파일:** `fluxer_client/app/examples/visual_gallery.rs`(또는 `--visual-gallery` 모드), `scripts/visual_parity_diff.py`
  - **참고 패턴:** `Wally869/gpui-preview`, `Ziinc/gpui-visual-test`
  - **작업:** 로그인·네트워크·CAPTCHA 없이 결정적 픽스처 `ClientState`로 아래 **12개 시나리오**를 기본(`1280×800`)·최대화(`1920×1040`)로 렌더링해 `Logs/visual-gallery/`(Git 밖)에 PNG를 일괄 출력한다.
    1. `#general` (텍스트·다중 이미지·애니메이션 GIF+배지·YouTube 임베드 카드·멘션)
    2. `#markdown` (서식·코드 블록·인용·목록·스포일러·이모지)
    3. 빈 길드 채널 (`Welcome to #channel`)
    4. DM 목록·대화 (`Quick switcher`, 아바타 행, 답장·반응 pill·입력 중 표시)
    5. **검색 결과 패널 (링크 임베드·이미지·아바타 포함)**
    6. 사용자 설정 2단 (`Appearance`·`Account`·`Voice & video`·`Advanced`)
    7. 채널·역할·밴 설정 2단
    8. **음성 채널 접속 중앙 스테이지 (참가자 타일 + 화면 공유 뷰어)**
    9. 멤버 목록 + 프로필 팝아웃
    10. 이모지 피커
    11. 빠른 전환기 (`Ctrl+K`)
    12. 창 최대화(와이드 `1920×1040`) 레이아웃
  - **수락 기준:** 1회 명령으로 12개 시나리오 PNG가 자동 생성되며 크래시/패닉 0건.
- [ ] **M3.2 공식 네이티브 앱 기준선 자동 대조 및 잔여 결함 수렴**
  - **작업:** 공식 네이티브 창(`fluxer_desktop_rs` 또는 격리 `fluxer_desktop`)과 GPUI `fluxer_client.exe`를 동일 해상도(`1280×800`, 최대화)로 맞춰 영역별 픽셀/바운딩박스 diff를 실행하고, 기준 초과 영역을 모두 수정한 뒤 `docs/progress/progress.json`의 실기 검증 항목을 `done`으로 승격한다.

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
