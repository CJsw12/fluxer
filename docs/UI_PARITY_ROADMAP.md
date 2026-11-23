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
- [x] **M1.3 공식 Phosphor Icons SVG 교체**
  - **대상 파일:** `fluxer_client/app/assets/icons/`(신규, MIT `@phosphor-icons/core` 원본 SVG + 라이선스 고지), `fluxer_client/app/src/icons.rs`, 각 UI 아이콘 참조부
  - **공식 참조:** `fluxer_app/package.json` (`@phosphor-icons/react`) 및 각 컴포넌트의 Phosphor 아이콘명
  - **작업:** 앱이 사용하는 모든 아이콘(길드·채널·음성·헤더·설정·입력창·메시지 액션·모달 닫기)을 공식 Phosphor SVG로 교체하고 `AppAssets`가 우선 서빙하게 연결한다.
  - **수락 기준:** `icons::tests::app_icons_load`가 모든 임베드 SVG의 유효성(`<svg`)을 검증하며 릴리스 빌드에 외부 파일 의존성 없이 내장됨.

### M2. 공식 치수 동기화 & 미구현 대형 화면 이식
- [x] **M2.1 공식 `fluxer_app` 레이아웃 치수·색상 토큰 동기화**
  - **대상 파일:** `fluxer_client/app/src/theme.rs`, 관련 `ui/*.rs`, (선택) `scripts/extract_layout_tokens.py`
  - **공식 참조:** `fluxer_app/src/features/theme/layout/MessageLayoutSpec.ts`, `ColorSystemPalette.ts`, `ChannelChatLayout.module.css`, `SettingsModalLayout.module.css`
  - **작업:** 길드 레일 폭, 채널 사이드바 폭, 헤더 높이, 메시지 그룹 상하 패딩·아바타 크기·들여쓰기, 입력창 높이/반경, 멤버 목록 폭, 설정 2단 폭/간격을 공식 CSS 실측치와 1:1 매핑해 `ThemeSizes`에 고정한다.
  - **수락 기준:** `ThemeSizes` 단위 테스트로 공식 스펙 상수 일치 검증, 기본(`1280×800`)·최대화(`1920×1040`) 창에서 잘림/겹침 0건.
- [x] **M2.2 음성 채널 접속 시 중앙 스테이지 화면(`VoiceCallView`) 구현**
  - **대상 파일:** `fluxer_client/app/src/ui/voice_stage.rs`(신규), `ui/mod.rs`, `client_view.rs`, `ui/channel_list.rs`, `ui/video_viewer.rs`, `tests_voice.rs`
  - **공식 참조:** `fluxer_app/src/features/voice/components/CompactVoiceCallView.tsx` 및 음성 타일/컨트롤 컴포넌트
  - **작업:** 음성 채널 클릭·연결 시 중앙 영역이 음성/비디오 스테이지로 전환되도록 구현한다(참가자 `ui::avatar` 타일 그리드 + 발화/음소거/헤드셋 상태 배지 + 원격 화면 공유 선택 시 `ui::video_viewer` 메인 스테이지 확대 + 하단 마이크/헤드셋/화면공유/연결끊기 원형 컨트롤 바 + 텍스트 채널로 돌아가기 전환).
  - **수락 기준:** 음성 채널 접속→스테이지 전환→참가자 타일/화면공유 스테이지/컨트롤 동작→연결 해제 복귀를 헤드리스 GPUI 테스트(`tests_voice.rs`)로 검증 + 실서버 음성 연결 실기 확인.
- [x] **M2.3 커스텀 타이틀바 및 설정창 세부 컨트롤 마감**
  - **대상 파일:** `fluxer_client/app/src/ui/titlebar.rs`(또는 `root.rs`), `fluxer_client/app/src/ui/settings.rs`
  - **작업:** 설정 `Appearance`의 공식 테마 스와치 카드 및 폰트/간격 슬라이더 스타일 정교화, 필요 시 공식 데스크톱과 동일한 상단 타이틀바 스타일 적용.
  - **수락 기준:** 기존 설정·키보드·접근성 테스트 전부 통과 및 경고 0건.

### M3. 오프라인 12-시나리오 시각 갤러리 & 네이티브 창 자동 대조
- [x] **M3.1 오프라인 시각 갤러리 하네스 구축**
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
- [x] **M3.2 공식 네이티브 앱 기준선 자동 대조 및 잔여 결함 수렴**
  - **작업:** 공식 네이티브 창(`fluxer_desktop_rs` 또는 격리 `fluxer_desktop`)과 GPUI `fluxer_client.exe`를 동일 해상도(`1280×800`, 최대화)로 맞춰 영역별 픽셀/바운딩박스 diff를 실행하고, 기준 초과 영역을 모두 수정한 뒤 `docs/progress/progress.json`의 실기 검증 항목을 `done`으로 승격한다.
  - **실기 검증 결과:** 전용 테스트 서버에서 포터블 공식 앱과 최신 릴리스 `dist/fluxer_client/fluxer_client.exe`를 동일 `#markdown` 채널·동일 해상도(`1280×800`, `2560×1392` 최대화)로 직접 대조했고(`main_content` 6.67%·`composer` 6.01%·`right_panel` 3.03% diff @ 최대화), 대조 과정에서 발견한 결함(첨부 이미지 수직 오버플로우, `button_primary` 브랜드 인디고, 화면 공유 표시명, 타이틀바 좌측 점, 카테고리 화살표 우측 정렬, 사용자 패널 음소거/데픈 버튼, 라이트 테마 스포일러 색상)을 모두 수정·재검증했다. 실서버에서 확인된 4건(`설정 화면`·`글꼴·밀도·확대`·`미디어 뷰어`·`첨부 업로드`)을 `docs/progress/progress.json` `done`으로 승격해 전체 **89%**·일상 사용 **91%**(실기 55·구현 37·부분 0·미착수 1)로 동기화했다.

### M4. 공식 원본(`fluxer_app/src`) 기반 전수 자동 감사 & 음성·하단 패널·설정창 1:1 수렴 (WebView 미사용)
> **목적:** 사람이 눈으로 일일이 차이를 찾지 않아도 저장소 내 공식 TypeScript/React 원본(`fluxer_app/src`)의 `data-flx` 컴포넌트 트리, XState/Selector 상태 머신, CSS Module 토큰, `SettingsSectionRegistry`를 정적·동적으로 자동 대조해 순수 Rust GPUI(`fluxer_client/`)만으로 1:1 동일 화면·동작을 보장한다.

- [x] **M4.1 공식 `fluxer_app/src` ↔ `fluxer_client/app/src` 자동 패리티 감사 도구(`scripts/audit_ui_parity.py`) 구축**
  - **대상 파일:** `scripts/audit_ui_parity.py`(신규)
  - **공식 참조:** `fluxer_app/src/features/voice/state/LocalVoiceStateMachine.ts`, `UserAreaState.ts`, `UserArea.tsx`, `VoiceConnectionStatusInner.tsx`, `LocalParticipantControls.tsx`, `CompactVoiceCallViewInner.tsx`, `VoiceControlBar.tsx`, `DesktopSettingsView.tsx`, `SettingsConstants.tsx`, `SettingsSectionRegistry.ts`
  - **작업:** 공식 `.tsx`/`.ts`/`.module.css` 원본에서 ① 음성 상태 머신 불변 조건(데픈↔뮤트 연동, `shouldUnmuteOnUndeafen`, 권한/서버 뮤트 잠금), ② 좌측 하단 `UserArea` + `VoiceConnectionStatus` 컴포넌트 트리·아이콘·액션, ③ 음성 참가 화면(`CompactVoiceCallView` 상하 분할, 오디오 전용 원형 아바타 리스트, 드롭다운 분할 `VoiceControlBar`), ④ 사용자 설정 모달(`UserSettingsModal`) 탭·좌측 하위 섹션 트리(`SidebarSections`)·내부 섹션 카드를 추출해 Rust GPUI 소스와 1:1 자동 대조하고 JSON/콘솔 리포트를 출력한다.
  - **수락 기준:** `python -I scripts/audit_ui_parity.py`가 모든 영역의 계약·셀렉터·상태 전이 일치 여부를 검사하고 누락 0건일 때 종료 코드 `0`을 반환함(52/52 통과 확인).

- [x] **M4.2 공식 `LocalVoiceStateMachine` & `UserAreaState` 1:1 상태 머신 이식 (스피커 단독 뮤트 방지 및 자동 복원)**
  - **대상 파일:** `fluxer_client/app/src/backend/voice.rs`, `fluxer_client/app/src/client_view.rs`, `fluxer_client/app/src/tests_voice.rs`
  - **공식 참조:** `fluxer_app/src/features/voice/state/LocalVoiceStateMachine.ts#L307-L402`, `fluxer_app/src/features/app/components/layout/UserAreaState.ts`
  - **작업:**
    1. `VoiceManager` 및 `VoiceSnapshot`에 `should_unmute_on_undeafen: bool`과 `effective_muted()` / `effective_deafened()` / `mute_toggle_locked()` 셀렉터를 추가한다.
    2. `toggle_self_deaf`: 스피커 뮤트(Deafen) 활성화 시 직전 마이크 뮤트 여부(`was_muted_before`)를 확인해 **`self_deaf = true, self_mute = true`로 마이크까지 강제 뮤트**하고 `should_unmute_on_undeafen = !was_muted_before`를 기록한다. 스피커 뮤트 해제 시 `should_unmute_on_undeafen && !mute_toggle_locked()`이면 마이크 뮤트도 함께 해제한다.
    3. `toggle_self_mute`: 스피커가 뮤트된 상태(`self_deaf == true`)에서 마이크 버튼을 눌러 언뮤트하면 **`self_mute = false, self_deaf = false`로 스피커 뮤트까지 동시에 해제**한다. 서버 뮤트/데픈 또는 발언 권한 없음(`speak_denied`) 상태에서는 로컬 마이크 언뮤트 토글을 잠근다.
  - **수락 기준:** 데픈↔뮤트 진리표 6가지 전이 시나리오 단위·GPUI 테스트 통과, 어떠한 경로로도 `(self_mute: false, self_deaf: true)` 상태가 발생하지 않음.

- [x] **M4.3 좌측 하단 통합 `UserArea` + `VoiceConnectionStatus` + `LocalParticipantControls` 1:1 레이아웃**
  - **대상 파일:** `fluxer_client/app/src/ui/user_panel.rs`, `fluxer_client/app/src/ui/voice_panel.rs`, `fluxer_client/app/src/ui/channel_list.rs`, `fluxer_client/app/src/tests_voice.rs`
  - **공식 참조:** `fluxer_app/src/features/app/components/layout/UserArea.tsx`, `UserArea.module.css`, `VoiceConnectionStatusInner.tsx`, `VoiceConnectionStatus.module.css`, `LocalParticipantControls.tsx`
  - **작업:**
    1. 별도 상자로 분리되어 있던 `voice_panel`과 `user_panel`을 공식 `.userAreaInnerWrapper` 단일 카드(`rounded(8px)`, `border_1`, 내부 `.voiceConnectionWrapper` `1px` 하단 구분선)로 통합한다.
    2. `voice_panel` 1행(`.statusRow`): 좌측 `[신호 강도 아이콘(SignalStrengthIcon) + 상태 버튼("In Voice" / "Connecting..." / "Connection failed")]`, 우측 `[오디오 처리 아이콘 버튼 + 연결 끊기 아이콘 버튼(PhoneDisconnect 32×32)]` (기존 24px 내 아바타 및 텍스트 `"Disconnect"` 버튼 제거).
    3. `voice_panel` 2행(`.connectionInfo`): 클릭 시 음성 스테이지로 이동하는 **`{채널명} / {서버명}`** 링크(`.channelSourceRow`) + 통화 참가자 아바타 스택(`.channelAvatarStack`, 최대 4명).
    4. `voice_panel` 3행(`.mediaSection` / `LocalParticipantControls`): 2열 균등 그리드(`repeat(2, 1fr)`) 아이콘 버튼 `[카메라 토글]` + `[화면 공유 토글]` (하단 `user_panel`과 중복되던 텍스트 `"Mute"` / `"Deafen"` 버튼 제거).
    5. `user_panel` 우측 컨트롤(`.controlsContainer`): 마이크 뮤트·스피커 데픈 활성 시 공식 `.controlButton.active`(붉은 위험 색상 아이콘 + 12% 위험 배경 틴트) 및 권한 잠금(`.disabled` 50% 투명도) 스타일을 1:1 적용한다.
  - **수락 기준:** 헤드리스 GPUI 테스트 및 시각 갤러리에서 통합 카드·3행 음성 상태·2열 미디어 버튼·활성 붉은 틴트 렌더링 확인.

- [x] **M4.4 음성 참가 화면(`CompactVoiceCallView` 상하 분할 · 오디오 원형 아바타 리스트 · 드롭다운 분할 `VoiceControlBar`) 1:1 개편**
  - **대상 파일:** `fluxer_client/app/src/ui/voice_stage.rs`, `fluxer_client/app/src/client_view.rs`, `fluxer_client/app/src/tests_voice.rs`
  - **공식 참조:** `fluxer_app/src/features/channel/components/channel_view/GuildChannelView.tsx#L449-L566`, `CompactVoiceCallViewInner.tsx`, `VoiceParticipantAvatarList.tsx`, `VoiceControlBar.tsx`, `VoiceControlBar.module.css`
  - **작업:**
    1. **상하 분할(`voiceTextSplitView`) 및 전체화면 전환:** 음성 채널 화면에서 상단 다크 통화 스테이지(`CompactVoiceCallView`)와 하단 해당 채널 텍스트 채팅(`Messages` + 입력창)을 동시에 볼 수 있는 분할 모드와 전체 높이 확장 토글(`voice-stage-expand-btn`)을 지원한다.
    2. **오디오 전용 원형 아바타 뷰 vs 비디오 타일 그리드:** 화면 공유/카메라 시청 트랙이 없을 때는 공식 `VoiceParticipantWrappedAvatarList`(중앙 정렬 원형 아바타 + 발화 링 + 우하단 뮤트/데픈 배지 + 하단 이름 필)로 표시하고, 스트림/비디오가 있을 때는 16:9 타일 그리드 및 포커스 스테이지로 표시한다.
    3. **공식 드롭다운 분할 `VoiceControlBar`:** 하단 컨트롤 바를 공식 `.buttonContainer` 분할 버튼 그룹 `[마이크 | ▾]`, `[스피커(SpeakerHigh/SpeakerSlash) | ▾]`, `[카메라 | ▾]`, `[화면 공유(Monitor) | ▾]`, `[더보기]`, 수직 구분선(`.buttonSeparator`), `[연결 끊기(PhoneDisconnect)]`, (스트림 시청 중) `[시청 중지(EyeSlash)]` 구조로 개편한다.
  - **수락 기준:** 분할 뷰 토글·오디오 아바타 뷰/비디오 스테이지 전환·분할 컨트롤 바 전 버튼 GPUI 테스트 통과.

- [x] **M4.5 사용자 설정창(`UserSettingsModal`) 공식 탭 계층 · 좌측 하위 섹션 트리(`SidebarSections`) · 프로필 탭 · 내부 섹션 카드 전수 구현**
  - **대상 파일:** `fluxer_client/app/src/ui/settings.rs`, `fluxer_client/app/src/tests_settings.rs`, `fluxer_client/app/src/visual_gallery.rs`
  - **공식 참조:** `fluxer_app/src/features/app/components/dialogs/components/DesktopSettingsView.tsx`, `SettingsConstants.tsx`, `SettingsSectionRegistry.ts`, `section_registry/*.ts`
  - **작업:**
    1. 좌측 사이드바 상단 40px 프로필 카드(`Edit your profile`)를 클릭 가능한 `Profile`(`my_profile`) 탭 진입 버튼으로 연결하고 프로필 커스터마이징·상태·배지 섹션을 렌더링한다.
    2. 공식 카테고리(`Your account` / `App settings`)의 전체 탭(`Account & security`, `Privacy & safety`, `Look & feel`, `Themes`, `Notifications`, `Messages & media`, `Voice & video`, `Accessibility`, `Language & time`, `Keybinds`, `Windows app / Advanced`)과 선택된 탭 아래에 펼쳐지는 **좌측 하위 섹션 트리(`SidebarSections`)**를 구현한다.
    3. 각 탭 내부에 `SettingsSectionRegistry.ts`에 정의된 모든 공식 섹션 카드(로그인 정보·2FA/패스키·신뢰 도메인·계정 위험 구역, DM/프로필 개인정보·민감 미디어 필터, 테마/폰트/줌/메시지 밀도/스트리머 모드, 알림/멘션/사운드, 인라인 미디어/링크 미리보기/스포일러/입력기, 입출력 장치/볼륨/입력 모드 VAD·PTT/노이즈 억제/마이크 테스트/화면 공유 품질, 동작 줄이기/고대비/포커스 링, 언어/시간·날짜 형식/맞춤법, 단축키 일람, Windows 시작/트레이/업데이트)를 1:1 배치한다.
    4. 오프라인 시각 갤러리(`visual_gallery.rs`)를 확장해 음성 연결 후 통합 `UserArea`, 상하 분할 음성 스테이지, 확장된 설정 탭 화면을 모두 결정적으로 캡처한다.
  - **수락 기준:** `scripts/audit_ui_parity.py` 감사 100%(52/52) 통과, 기존 및 신규 설정·음성 GPUI 테스트 전수(총 700건) 통과, 릴리스 패키징 경고 0건.

### M5. 세부 인터랙션 팝아웃 · 길드 레일 아이콘 · 모달 탭 통합 · 사이드바 음성 행 1:1 마감
- [x] **M5.1 채널 사이드바 음성 참가자 행 우측 상태 아이콘·LIVE 배지 정렬**
  - **대상 파일:** `fluxer_client/app/src/ui/channel_list.rs`, `fluxer_client/app/src/tests_voice.rs`
  - **공식 참조:** `fluxer_app/src/features/app/components/layout/ChannelItem.tsx`, `VoiceParticipant.tsx`
  - **작업:** 음성 채널 아래 참가자 행에서 텍스트 `"(muted)"` / `"(deafened)"` 접미사를 제거하고, 좌측에 20px 아바타+이름, 우측 끝에 `MicrophoneSlash` / `SpeakerSlash` 아이콘과 화면 공유 시 붉은 `"LIVE"` 배지(`voice-member-live-{uid}`)를 정렬한다.
  - **수락 기준:** 사이드바 음성 참가자 행에서 우측 상태 아이콘·LIVE 배지 셀렉터 렌더링 GPUI 테스트 통과.

- [x] **M5.2 채널 설정(`Overview` + `Permissions`) 및 서버 설정(`Roles` + `Bans`) 2단 모달 사이드바 탭 통합**
  - **대상 파일:** `fluxer_client/app/src/ui/channel_modal.rs`, `fluxer_client/app/src/ui/access.rs`, `fluxer_client/app/src/ui/moderation_modal.rs`
  - **공식 참조:** `fluxer_app/src/features/channel/components/modals/ChannelSettingsModal.tsx`, `GuildSettingsModal.tsx`
  - **작업:** 채널 설정 2단 모달 좌측 사이드바에 `Overview`(`channel-settings-tab-overview`)와 `Permissions`(`channel-settings-tab-permissions`) 탭을 함께 배치해 즉시 전환되게 하고, 서버 설정 2단 모달 좌측 사이드바에도 `Roles`(`guild-settings-tab-roles`)와 `Bans`(`guild-settings-tab-bans`) 탭을 통합한다.
  - **수락 기준:** 채널 설정 `Overview ↔ Permissions` 및 서버 설정 `Roles ↔ Bans` 탭 클릭 전환 GPUI 테스트 통과, 기존 채널/권한/밴 테스트 무회귀.

- [x] **M5.3 길드 레일(좌측 서버 바) 홈 브랜드 아이콘 · 서버 `icon_url` 이미지 · 좌측 알약(pill) 인디케이터**
  - **대상 파일:** `fluxer_client/app/src/ui/guild_rail.rs`, `fluxer_client/app/src/view_model.rs`, `fluxer_client/app/src/client_view.rs`
  - **공식 참조:** `fluxer_app/src/features/app/components/layout/GuildsSidebar.tsx`, `GuildItem.tsx`
  - **작업:** `GuildItem`에 `icon_url: Option<SharedString>`(`guild_icon_url`)을 연결해 서버 아이콘 이미지가 있으면 이미지를 렌더링(없을 때만 약어 폴백)하고, 홈 버튼을 텍스트 `"DM"` 대신 브랜드 아이콘 버튼(`rail-home-icon`)으로 렌더링한다.
  - **수락 기준:** `guild_icon_url` 단위 테스트 및 길드 레일 홈 아이콘·서버 이미지 렌더링 GPUI 테스트 통과.

- [x] **M5.4 좌측 하단 `UserArea` 클릭 시 전용 `UserAreaPopout` 상태·계정 메뉴 팝아웃**
  - **대상 파일:** `fluxer_client/app/src/ui/user_panel.rs`, `fluxer_client/app/src/client_view.rs`
  - **공식 참조:** `fluxer_app/src/features/app/components/floating/UserAreaPopout.tsx`
  - **작업:** 좌측 하단 `UserArea` 프로필 영역 클릭 시 하단 카드 바로 위에 전용 `UserAreaPopout`(`user-area-popout`)을 열어 `Online` / `Idle` / `Do Not Disturb` / `Invisible` 4종 상태 직접 선택, `Edit profile` 설정 진입, `Switch accounts` 계정 전환을 지원한다.
  - **수락 기준:** `user-panel-profile` 클릭 → `user-area-popout` 오픈 및 상태 선택 전환 GPUI 테스트 통과.

- [x] **M5.5 음성 컨트롤 바 `[▾]` 드롭다운 및 하단 음성 패널 인라인 설정·연결 상태 팝아웃**
  - **대상 파일:** `fluxer_client/app/src/ui/voice_stage.rs`, `fluxer_client/app/src/ui/voice_panel.rs`, `fluxer_client/app/src/client_view.rs`, `fluxer_client/app/src/tests_voice.rs`
  - **공식 참조:** `fluxer_app/src/features/voice/components/VoiceSettingsMenus.tsx`, `VoiceDetailsPopout.tsx`
  - **작업:** 음성 컨트롤 바의 `[마이크 | ▾]`·`[스피커 | ▾]`·`[화면 공유 | ▾]` 화살표 클릭 시 통화 화면을 가리지 않고 컨트롤 바 바로 위에 인라인 장치·음량·스트림 팝아웃(`voice-control-popout`)을 열고, 좌측 하단 음성 패널의 오디오 처리 버튼 클릭 시 인라인 오디오·연결 상태 요약 팝아웃(`voice-details-popout`)을 연다.
  - **수락 기준:** 컨트롤 바 `[▾]` 클릭 시 `voice-control-popout` 오픈·음량/장치 조작 및 하단 `voice-details-popout` 토글 GPUI 테스트 통과.

- [x] **M5.6 설정창 신규 섹션 실시간 `Switch`/모드 토글(`prefs.rs`) 연동 및 프로필 팝아웃 레이아웃 정돈**
  - **대상 파일:** `fluxer_client/app/src/prefs.rs`, `fluxer_client/app/src/ui/settings.rs`, `fluxer_client/app/src/ui/profile_popout.rs`, `scripts/audit_ui_parity.py`
  - **공식 참조:** `fluxer_app/src/features/user/components/popouts/UserProfilePopout.tsx`, `VoiceVideoTab.tsx`, `PrivacySafetyTab.tsx`, `ChatSettingsTab.tsx`
  - **작업:** `Prefs`에 `voice_push_to_talk`, `voice_noise_suppression`, `allow_guild_dms`, `inline_media_previews`를 추가해 설정창의 입력 모드(VAD/PTT)·노이즈 억제·길드 DM 허용·인라인 미디어 미리보기 카드를 실제 인터랙티브 `Switch`/버튼으로 연결하고, 프로필 팝아웃의 아바타 6px 배너 컷아웃 링과 모더레이션 액션 카드를 공식 레이아웃으로 정돈한다.
  - **수락 기준:** `scripts/audit_ui_parity.py` 확장 항목 포함 100% 통과, 전체 테스트 통과 및 무경고 Windows 릴리스 패키징 완료.

### M6. 로그인 실기 전 시나리오 자동 제어(`fluxer://scene/*`) · 공식 앱(`fluxer-ref`) 실기 대조 격차 100% 수렴
- [x] **M6.1 로그인 상태 전 시나리오 제어 라우트(`DeepLink::Scene`) 및 라이브 서버 전수 검증 하네스**
  - **대상 파일:** `fluxer_client/app/src/deep_links.rs`, `fluxer_client/app/src/client_view.rs`
  - **작업:** 오프라인 합성 픽스처(`--visual-gallery`)만으로는 실제 서버 응답(YouTube unfurler 등)과 로그인 상태 전용 상호작용을 모두 검증할 수 없으므로, 로그인된 `fluxer_client.exe` 프로세스를 단일 인스턴스 IPC(`--url fluxer://scene/...`)로 모든 시나리오 장면(텍스트/마크다운/공지/DM/친구 목록, 음성 채널 접속·중앙 통화 스테이지·상하 분할 채팅·인라인 음성 컨트롤/상세 팝아웃, 하단 `UserAreaPopout`, 서버 헤더 `GuildHeaderPopout`, 설정 모달 12개 탭 전수, 채널 설정 `Overview`/`Permissions`, 서버 설정 `Roles`/`Bans`, 프로필 팝아웃, 검색 패널, 빠른 전환기, 이모지 피커)으로 즉시 전환·캡처하는 라이브 실기 하네스를 구축한다.
  - **수락 기준:** `DeepLink::Scene` 라우트 단위 테스트 통과 및 전용 테스트 서버 로그인 상태에서 전 시나리오 연속 전환·캡처 성공.

- [x] **M6.2 로그인 실기 대조(`live_official` vs `live_rust`)에서 확인된 메인 셸 1:1 패리티 격차 수렴**
  - **대상 파일:** `fluxer_client/app/src/ui/guild_rail.rs`, `fluxer_client/app/src/ui/channel_list.rs`, `fluxer_client/app/src/ui/header.rs`, `fluxer_client/app/src/ui/composer.rs`
  - **공식 참조:** `GuildsSidebar.tsx`, `GuildHeader.tsx`, `GuildHeaderPopout.tsx`, `ChannelHeader.tsx`, `ChannelComposer.tsx`
  - **작업:**
    1. 좌측 길드 레일에 공식 `Favorites`(`rail-favorites-btn`) 버튼, 상단 구분선(`rail-separator`), 선택된 서버의 브랜드 블루(`theme.brand`) 배경 및 `theme.on_fill` 텍스트, 하단 `Discover`(`Globe`, `rail-discover-btn`) 및 48×48 원형 `Join/Create`(`Plus`, `join-guild`) 버튼을 구현한다.
    2. 서버 헤더 우측에 공식 `CaretDown` 드롭다운 버튼(`guild-header-menu-btn`)과 클릭 시 열리는 `GuildHeaderPopout`(`guild-header-popout`) 메뉴를 추가한다.
    3. 채널 헤더 우측 액션 바에 공식 `ChannelHeader.tsx` 아이콘(`header-threads-btn`, `header-pins-btn`, `header-favorite-btn`, `header-inbox-btn`)을 배치하고, 메시지 입력창 우측에 `[GIF/Image (`composer-gif`)]` · `[Sticker (`composer-sticker`)]` · `[Emoji (`composer-emoji`)]` 아이콘 바를 완성한다.
  - **수락 기준:** `scripts/audit_ui_parity.py` M6 항목 통과 및 기존 GPUI 테스트 무회귀.

- [x] **M6.3 실제 서버 YouTube 임베드 중복 썸네일 수정 및 본인 멘션(`@user`) 행 전체 하이라이트**
  - **대상 파일:** `fluxer_client/core/src/state/embed_model.rs`, `fluxer_client/app/src/ui/embed.rs`, `fluxer_client/app/src/view_model.rs`, `fluxer_client/app/src/ui/message_row.rs`
  - **공식 참조:** `EmbedMediaRenderer.tsx`, `EmbedYouTube.tsx`, `MessageItem.tsx`, `Message.module.css`
  - **작업:**
    1. 실제 서버 unfurler가 반환하는 YouTube 임베드(`video.proxy_url == None`, 동일 URL 썸네일 임베드 2개)를 비디오 포스터(`MediaKind::Video`, 재생 버튼 오버레이) 단일 카드로 렌더링하고 중복 2열 갤러리 폴백을 방지한다.
    2. 로그인한 본인이 멘션(`@user`, `@everyone`, 보유 역할 멘션)된 메시지 행(`MessageRow.mentioned`)에 공식 앰버 배경(`rgba(250, 166, 26, 0.10)`)과 좌측 2px 앰버 강조선(`msg-mentioned-{id}`)을 렌더링한다.
  - **수락 기준:** 실제 서버 YouTube 임베드 단일 비디오 카드 렌더링 및 멘션 행 하이라이트 단위·GPUI 테스트 통과.

- [x] **M6.4 전용 테스트 서버 로그인 상태에서 공식 앱(`fluxer-ref`) 및 Rust 앱 전 시나리오 실기 캡처·대조**
  - **대상 파일:** `scripts/audit_ui_parity.py`, `PROGRESS.md`
  - **작업:** 전용 테스트 서버 SSH 터널을 연결하고 로그인된 프로필로 전 시나리오(32개 장면) 실기 화면을 캡처(`Logs/live-verify-m6/`)하여 공식 앱 화면 및 컴포넌트 계약과 대조 검증한다.
  - **수락 기준:** `scripts/audit_ui_parity.py` 100%(90/90) 통과, 전체 테스트 통과, 로그인 실기 32개 시나리오 캡처·검증 완료.

### M7. 포팅 검증 방법론 고도화(정적 배지 치환 금지 · 호버/인디케이터 상태 머신 감사) 및 길드 레일 호버 · 음성/비디오/전체 설정 인터랙티브 컨트롤 100% 이식
> **목적:** 섹션 제목이나 껍데기(`info_card_row` 정적 배지)만 있고 실제 조작 컨트롤(비디오 해상도·프레임레이트·코덱·비트레이트, 음성 처리 프로필·노이즈 억제 백엔드·VAD 감도·PTT 딜레이·마이크 테스트, 길드 레일 좌측 흰색 인디케이터 바 `40px`/`20px`/`8px` 호버 애니메이션 등)이 누락되는 문제를 근본적으로 차단하기 위해, `scripts/audit_ui_parity.py`가 공식 `fluxer_app/src` 원본(`GuildListIndicator.ts`, `GuildsLayout.module.css`, `VoiceSettings.ts`, `UserVoiceTab.tsx`, `UserVideoTab.tsx`, `UserVideoTabState.ts`, `DisplayTab.tsx`, `PrivacySafetyTabInline.tsx`, `NotificationsTab.tsx`)의 인터랙티브 컨트롤과 호버 상태 머신을 전수 파싱·검증하도록 고도화한다.

- [x] **M7.1 포팅 검증 도구(`scripts/audit_ui_parity.py` v2) 고도화 — 컨트롤 전수 인벤토리 & 정적 배지 치환 차단**
  - **대상 파일:** `scripts/audit_ui_parity.py`
  - **공식 참조:** `GuildListIndicator.ts`, `GuildsLayout.module.css`, `VoiceSettings.ts`, `UserVoiceTab.tsx`, `UserVideoTab.tsx`, `UserVideoTabState.ts`, `DisplayTab.tsx`, `ChatSettingsTabInputTab.tsx`, `PrivacySafetyTabInline.tsx`, `NotificationsTab.tsx`
  - **작업:**
    1. 공식 원본 파일들에서 호버/선택 상태 머신(`resolveGuildListIndicatorHeight`: 선택 `40px`, 호버 `20px`, 미읽음 `8px`)과 모든 설정 탭의 인터랙티브 컨트롤 목록을 추출해 Rust GPUI(`ui/guild_rail.rs`, `prefs.rs`, `settings_model.rs`, `ui/settings.rs`, `ui/voice_stage.rs`)의 실제 상태 필드·이펙트·셀렉터와 1:1 대조한다.
    2. 설정 탭(`Voice & video`, `Privacy & safety`, `Messages & media`, `Notifications`, `Language`)에서 조작 가능한 설정을 읽기 전용 `info_card_row` 배지로 대체한 경우 감사를 실패 처리한다.
  - **수락 기준:** `python -I scripts/audit_ui_parity.py`가 M4~M7 전체 계약을 자동 감사하고 100% 통과 시 종료 코드 `0`을 반환함.

- [x] **M7.2 좌측 길드 레일(`Direct messages` · `Favorites` · `Guild` · `Discover` · `Join`) 1:1 호버 모핑 및 좌측 흰색 인디케이터 바(`40px`/`20px`/`8px`) 상태 머신 이식**
  - **대상 파일:** `fluxer_client/app/src/ui/guild_rail.rs`, `fluxer_client/app/src/client_view.rs`
  - **공식 참조:** `fluxer_app/src/features/app/components/layout/sidebar_nav/GuildListIndicator.ts`, `GuildListDMItem.tsx`, `GuildListItemPresentation.tsx`, `GuildsLayout.module.css`
  - **작업:**
    1. `GuildListIndicator.ts`의 `resolveGuildListIndicatorHeight`를 `resolve_rail_indicator_height(selected: bool, hovered: bool, unread: bool) -> Option<f32>`(`selected => Some(40.)`, `hovered => Some(20.)`, `unread => Some(8.)`, `_ => None`)로 1:1 이식한다.
    2. `ClientView`에 `hovered_rail_item: Option<RailHoverTarget>`(`Home`, `Favorites`, `Guild(Snowflake)`, `Discover`, `Join`)과 ` active_favorites: bool` 상태를 추가하고 `.on_hover(cx.listener(...))`로 실시간 갱신한다.
    3. `Direct messages`(`Home`), `Favorites`, 각 `Guild` 버튼을 모두 동일한 `rail_item` + `rail_button_content` 구조로 통일하여, 마우스 호버 시 ① 좌측 흰색 인디케이터 바(`rail-pill-*`)가 즉시 `20px`(선택 시 `40px`, 미읽음 시 `8px`)로 표시·전환되고, ② 버튼 모양이 원형(`24px`)에서 둥근 사각형(`12px`)으로 모핑되며, ③ 배경색이 `theme.brand`(`#5865F2`), 아이콘/텍스트 색상이 `theme.on_fill`(`#FFFFFF`)로 전환되도록 구현한다.
  - **수락 기준:** `resolve_rail_indicator_height` 단위 테스트 및 `Home`/`Favorites`/`Guild` 호버·선택 인디케이터(`40px`/`20px`/`8px`) GPUI 테스트 통과.

- [x] **M7.3 설정 `Voice & Video` 탭(`UserVoiceTab.tsx` + `UserVideoTab.tsx` + `VoiceSettings.ts`) 인터랙티브 컨트롤 전수 이식**
  - **대상 파일:** `fluxer_client/app/src/prefs.rs`, `fluxer_client/app/src/settings_model.rs`, `fluxer_client/app/src/ui/settings.rs`, `fluxer_client/app/src/ui/voice_stage.rs`, `fluxer_client/app/src/tests_settings.rs`
  - **공식 참조:** `UserVoiceTab.tsx`, `UserVideoTab.tsx`, `UserVideoTabState.ts`, `VoiceSettings.ts`
  - **작업:**
    1. **음성 처리 및 입력 세부 설정:**
       - 음성 처리 프로필(`voice_processing_mode`: `Focused Voice` / `Direct Input` / `Custom`) 선택 컨트롤
       - 에코 캔슬레이션(`voice_echo_cancellation`: `Switch`)
       - 노이즈 억제 백엔드(`voice_noise_suppression_backend`: `Off` / `Standard (WebRTC)` / `DeepFilterNet`) 및 토글
       - 자동 게인 제어(`voice_auto_gain_control`: `Switch`)
       - 음성 감도(`voice_vad_auto_threshold`: `Auto` `Switch` + 수동 `voice_vad_threshold` `0%`~`100%` 스테퍼/슬라이더)
       - Push-to-Talk 해제 지연(`voice_ptt_release_delay_ms`: `20ms` / `100ms` / `250ms` / `500ms` / `1000ms` / `2000ms`)
       - 인터랙티브 마이크 테스트(`mic_test_active`: `Start Mic Test` / `Stop Mic Test` 토글 버튼 + 실시간 레벨 미터 바)
    2. **카메라 및 화면 공유 비디오 품질·코덱·비트레이트 설정:**
       - 카메라 해상도(`camera_resolution`: `480p` / `720p` / `1080p`) 및 카메라 좌우 반전(`mirror_camera`: `Switch`)
       - 화면 공유 프리셋(`streaming_mode`: `Screen Share` / `Gaming` / `Custom`)
       - 화면 공유 해상도(`video_resolution`: `480p` / `720p` / `1080p` / `1440p` / `Source`)
       - 화면 공유 프레임레이트(`video_fps`: `15 FPS` / `30 FPS` / `60 FPS`)
       - 비디오 코덱(`video_codec`: `Auto` / `H.265 (HEVC)` / `H.264` / `VP9` / `VP8` / `AV1`)
       - 화면 공유 비디오 비트레이트(`video_bitrate_kbps`: `Auto` / `2 Mbps` / `4 Mbps` / `6 Mbps` / `8 Mbps` / `10 Mbps` / `15 Mbps`)
       - 하드웨어 가속(`video_hardware_acceleration`: `Switch`)
    3. 통화 화면(`ui/voice_stage.rs`)의 화면 공유 `[▾]` 인라인 팝아웃(`VoicePopoutKind::Stream`)에도 `prefs`의 현재 해상도·FPS·코덱·비트레이트를 실시간 반영하고 빠른 품질 전환 버튼을 제공한다.
  - **수락 기준:** 모든 신규 `Prefs` 필드 직렬화 왕복 단위 테스트 및 설정창 인터랙티브 변경 GPUI 테스트 통과.

- [x] **M7.4 나머지 설정 탭(`Privacy & Safety`, `Messages & Media`, `Notifications`, `Language & Time`)의 정적 배지(`info_card_row`)를 인터랙티브 컨트롤로 전수 전환**
  - **대상 파일:** `fluxer_client/app/src/prefs.rs`, `fluxer_client/app/src/settings_model.rs`, `fluxer_client/app/src/ui/settings.rs`, `fluxer_client/app/src/tests_settings.rs`
  - **공식 참조:** `PrivacySafetyTabInline.tsx`, `DisplayTab.tsx`, `ChatSettingsTabInputTab.tsx`, `NotificationsTab.tsx`
  - **작업:**
    1. `Privacy & safety`: 친구 요청 허용 범위(`friend_request_access`: `Everyone` / `Friends of Friends` / `No one`), 민감 미디어·스포일러 표시 방식(`spoiler_display`: `On click` / `In channels I moderate` / `Always`) 인터랙티브 선택기 연결.
    2. `Messages & media`: 웹사이트 링크 미리보기 및 임베드 표시(`render_embeds`: `Switch`), 스포일러 표시 방식(`spoiler_display`), 메시지 이모지 반응 표시(`render_reactions`: `Switch`), 텍스트 이모티콘 자동 변환 및 자동완성(`convert_emoticons`: `Switch`) 연결.
    3. `Notifications`: 미읽음 채널 및 멘션 배지 표시(`unread_badge_enabled`: `Switch`) 연결.
    4. `Language`: 시간 표시 형식(`time_format_24h`: `Auto / 12-hour` vs `24-hour` 인터랙티브 선택) 연결.
  - **수락 기준:** 설정 모달 내 조작 가능한 항목의 정적 `info_card_row` 잔여 0건, GPUI 설정 테스트 전수 통과.

- [x] **M7.5 전용 테스트 서버 실기 검증 · 릴리스 패키징 · 클라이언트 상시 연결 유지**
  - **대상 파일:** `dist/fluxer_client/fluxer_client.exe`, `PROGRESS.md`
  - **작업:** `python -I scripts/audit_ui_parity.py` 100% 통과 확인, 워크스페이스 전체 테스트 및 `voice_worker` 테스트 통과, `scripts/package_fluxer_client.ps1`로 릴리스 바이너리 패키징 후 전용 테스트 서버에 연결된 상태로 클라이언트를 실행·검증한다.
  - **수락 기준:** 감사 스크립트 100% 통과, 전체 테스트 무회귀, 테스트 서버 연결 클라이언트 실행 확인.

### M8. 화면 단위 픽셀 · CSS 레이아웃 · 아이콘 1:1 수렴
> **목적:** 로그인 실기 화면을 공식 앱과 나란히 놓았을 때 남아 있던 레이아웃 정렬, 아이콘 형상, 호버 전용 버튼, 탭 스타일, 통화 스테이지 색상 차이를 공식 CSS 모듈 값 기준으로 맞추고 감사 스크립트에 계약으로 고정한다.

- [x] **M8.1 좌측 열 통합 및 가로 경계선 정렬**
  - **대상 파일:** `fluxer_client/app/src/client_view.rs`, `fluxer_client/app/src/ui/channel_list.rs`, `fluxer_client/app/src/ui/header.rs`
  - **작업:** 하단 `UserArea`가 길드 레일과 채널 사이드바 너비 전체에 걸치도록 좌측 열을 통합하고, `UserArea`↔`Composer` 하단 경계와 `GuildHeader`↔`ChannelHeader` 상단 경계(`56px`)를 같은 높이에 맞춘다. 채널 헤더 제목은 잘리지 않게 한다.
- [x] **M8.2 공식 Phosphor 아이콘 · 호버 전용 버튼 · 친구 탭 스타일**
  - **대상 파일:** `fluxer_client/app/src/icons.rs`, `fluxer_client/app/assets/icons/{star,compass,tray,image,sticker}.svg`, `fluxer_client/app/src/ui/{guild_rail,header,composer,friends,user_panel}.rs`
  - **공식 참조:** `DMFriendsView.module.css`(`.tabButton`, `.tabButton.active`, `.tabButton.primary`)
  - **작업:** 즐겨찾기·탐색·받은 편지함·미디어·스티커 아이콘을 공식 Phosphor SVG로 교체하고, 서버 헤더·채널 헤더 관리 버튼과 `UserArea`의 계정 전환 버튼을 그룹 호버 시에만 노출한다. 친구 탭은 `Add friend`를 브랜드 색 알약 버튼으로, 선택된 일반 탭을 `accent` 배경으로 맞춘다.
- [x] **M8.3 통화 스테이지 · 설정 프로필 탭 헤더**
  - **대상 파일:** `fluxer_client/app/src/ui/voice_stage.rs`, `fluxer_client/app/src/ui/settings.rs`
  - **공식 참조:** `CompactVoiceCallView`, `VoiceControlBar`, `UserProfileTab.tsx`
  - **작업:** 통화 스테이지를 검은 배경과 떠 있는 컨트롤 알약, 반투명 빨강·초록 활성 상태, 우상단 `17px` `CaretDown` 배지로 맞추고, 설정 `Profile` 탭 상단에 배너와 `6px` 컷아웃 링 아바타 헤더를 넣는다.
- [x] **M8.4 감사 계약 확장 및 로그인 실기 재검증**
  - **대상 파일:** `scripts/audit_ui_parity.py`
  - **작업:** `audit_m8_screen_layout_and_icon_parity()`로 위 레이아웃·아이콘·호버·탭 스타일 계약 29개를 추가하고, 전용 테스트 서버 로그인 32-시나리오 캡처와 공식 앱 대조를 다시 수행한다.
  - **수락 기준:** 감사 스크립트 100% 통과, 전체 테스트 무회귀, 릴리스 패키징 경고 0건.

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
- [ ] **M9.5 기능 자동 시험(사람 개입 없이)**
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
