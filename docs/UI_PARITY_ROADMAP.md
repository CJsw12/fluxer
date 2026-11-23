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
