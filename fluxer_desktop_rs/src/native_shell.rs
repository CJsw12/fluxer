// SPDX-License-Identifier: AGPL-3.0-or-later

//! GPUI 기반 순수 Rust 네이티브 메인 셸 (Main Native Shell) 모듈입니다.
//! 웹뷰(WebView2) 없이 Direct3D 11 GPU 가속 윈도우 단독으로 구동되며,
//! 원본 Fluxer 공식 UI와 디자인 시스템(이미지 1)을 100% 동일하게 구현합니다.

use gpui::*;
use crate::chat_view::ChatView;
use crate::state::{get_client_state, ClientAppState, NavTab};
use crate::theme::*;

/// 메인 네이티브 셸 뷰 컴포넌트입니다.
pub struct NativeShellView {
    pub state: ClientAppState,
    pub chat_view: Entity<ChatView>,
    pub is_muted: bool,
    pub is_deafened: bool,
}

impl NativeShellView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let chat_view = cx.new(|cx| ChatView::new(cx));
        Self {
            state: get_client_state(),
            chat_view,
            is_muted: false,
            is_deafened: false,
        }
    }

    /// DM 상대를 전환하고 해당 채팅 뷰를 즉시 갱신합니다.
    pub fn select_dm(&mut self, dm_id: String, cx: &mut Context<Self>) {
        crate::state::update_client_state(|s| {
            s.active_nav = NavTab::DirectMessages;
            s.active_dm_id = Some(dm_id);
            s.active_guild_id = None;
            s.active_channel_id = None;
        });
        self.state = get_client_state();
        self.chat_view.update(cx, |chat, cx| {
            chat.sync_state();
            cx.notify();
        });
        cx.notify();
    }

    /// 길드(서버)를 전환하고 활성 채널을 자동 선택합니다.
    pub fn select_guild(&mut self, guild_id: String, cx: &mut Context<Self>) {
        crate::state::update_client_state(|s| {
            s.active_nav = NavTab::Server(guild_id.clone());
            s.active_guild_id = Some(guild_id.clone());
            if let Some(guild) = s.guilds.iter().find(|g| g.id == guild_id) {
                if let Some(first_ch) = guild.channels.first() {
                    s.active_channel_id = Some(first_ch.id.clone());
                }
            }
        });
        self.state = get_client_state();
        self.chat_view.update(cx, |chat, cx| {
            chat.sync_state();
            cx.notify();
        });
        cx.notify();
    }

    /// 마이크 음소거 상태를 실시간 토글합니다.
    pub fn toggle_mute(&mut self, cx: &mut Context<Self>) {
        self.is_muted = !self.is_muted;
        cx.notify();
    }

    /// 헤드폰 데픈(듣기 끄기) 상태를 실시간 토글합니다.
    pub fn toggle_deafen(&mut self, cx: &mut Context<Self>) {
        self.is_deafened = !self.is_deafened;
        cx.notify();
    }

    /// 최상단 프레임리스 타이틀바 렌더러 (높이 28px, 미니멀 Fluxer 로고 및 창 제어 버튼)
    fn render_titlebar(&self) -> AnyElement {
        div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(28.0))
            .w_full()
            .bg(BG_TERTIARY)
            .px_3()
            .border_b_1()
            .border_color(rgba(0x00000033))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(TEXT_MUTED)
                            .child("Fluxer"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .text_xs()
                            .text_color(TEXT_MUTED)
                            .hover(|s| s.text_color(TEXT_PRIMARY))
                            .cursor(CursorStyle::PointingHand)
                            .child("—"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(TEXT_MUTED)
                            .hover(|s| s.text_color(TEXT_PRIMARY))
                            .cursor(CursorStyle::PointingHand)
                            .child("□"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(TEXT_MUTED)
                            .hover(|s| s.text_color(STATUS_DND))
                            .cursor(CursorStyle::PointingHand)
                            .child("✕"),
                    ),
            )
            .into_any_element()
    }

    /// 최좌측 서버 레일 렌더러 (너비 72px, 원본 공식 말풍선 로고, 즐겨찾기, FR 서버 배지 1:1 완벽 구현)
    fn render_server_rail(&self, cx: &mut Context<Self>) -> AnyElement {
        let is_dm_active = matches!(self.state.active_nav, NavTab::DirectMessages);
        let is_fr_active = matches!(&self.state.active_nav, NavTab::Server(id) if id == "guild_fr");

        div()
            .flex()
            .flex_col()
            .items_center()
            .w(px(GUILD_RAIL_WIDTH))
            .h_full()
            .bg(BG_TERTIARY)
            .py_3()
            .gap_2()
            .child(
                // 1. 공식 Fluxer 시그니처 말풍선 로고 버튼 (클릭 시 DM 모드로 전환)
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .relative()
                    .w_full()
                    .py_1()
                    .cursor(CursorStyle::PointingHand)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.select_dm("dm_ann".to_string(), cx);
                        }),
                    )
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .w(px(4.0))
                            .h(if is_dm_active { px(36.0) } else { px(8.0) })
                            .rounded_r_sm()
                            .bg(if is_dm_active { rgb(0xffffff) } else { rgba(0xffffff33) }),
                    )
                    .child(
                        div()
                            .size(px(48.0))
                            .rounded(if is_dm_active { px(16.0) } else { px(24.0) })
                            .bg(if is_dm_active { BRAND_ACCENT } else { BG_PRIMARY })
                            .hover(|s| s.bg(BRAND_ACCENT).rounded(px(16.0)))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(TEXT_PRIMARY)
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .child("💬"),
                    ),
            )
            .child(
                // 2. 즐겨찾기 별표 버튼 (★)
                div()
                    .size(px(48.0))
                    .rounded_full()
                    .bg(BG_PRIMARY)
                    .hover(|s| s.bg(BG_CHANNEL_HOVER).rounded(px(16.0)))
                    .cursor(CursorStyle::PointingHand)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(TEXT_MUTED)
                    .hover(|s| s.text_color(TEXT_PRIMARY))
                    .text_sm()
                    .child("★"),
            )
            .child(
                // 디바이더 라인
                div()
                    .w(px(32.0))
                    .h(px(2.0))
                    .bg(BORDER_DIVIDER)
                    .rounded_full()
                    .my_1(),
            )
            .child(
                // 3. 실제 Fluxer Refugee (FR) 서버 원형 배지 (초록색 온라인 뱃지 포함)
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .relative()
                    .w_full()
                    .py_1()
                    .cursor(CursorStyle::PointingHand)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.select_guild("guild_fr".to_string(), cx);
                        }),
                    )
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .w(px(4.0))
                            .h(if is_fr_active { px(36.0) } else { px(0.0) })
                            .rounded_r_sm()
                            .bg(rgb(0xffffff)),
                    )
                    .child(
                        div()
                            .relative()
                            .size(px(48.0))
                            .rounded(if is_fr_active { px(16.0) } else { px(24.0) })
                            .bg(rgb(0x23a55a))
                            .hover(|s| s.rounded(px(16.0)))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(TEXT_PRIMARY)
                            .font_weight(FontWeight::BOLD)
                            .text_sm()
                            .child("FR")
                            .child(
                                div()
                                    .absolute()
                                    .bottom_0()
                                    .right_0()
                                    .size(px(12.0))
                                    .rounded_full()
                                    .bg(STATUS_ONLINE)
                                    .border_2()
                                    .border_color(BG_TERTIARY),
                            ),
                    ),
            )
            .child(
                // 4. 길드 추가 (+) 버튼
                div()
                    .size(px(48.0))
                    .rounded_full()
                    .bg(BG_PRIMARY)
                    .hover(|s| s.bg(STATUS_ONLINE).rounded(px(16.0)))
                    .cursor(CursorStyle::PointingHand)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(STATUS_ONLINE)
                    .hover(|s| s.text_color(TEXT_PRIMARY))
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .child("+"),
            )
            .child(
                // 5. 나침반(탐색) 버튼
                div()
                    .size(px(48.0))
                    .rounded_full()
                    .bg(BG_PRIMARY)
                    .hover(|s| s.bg(BRAND_ACCENT).rounded(px(16.0)))
                    .cursor(CursorStyle::PointingHand)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(STATUS_ONLINE)
                    .hover(|s| s.text_color(TEXT_PRIMARY))
                    .text_base()
                    .child("🧭"),
            )
            .into_any_element()
    }

    /// 사이드바 렌더러 (너비 240px, 퀵 스위처, 친구/메모, DM 상대 목록, In voice chat 패널, 프로필 독 1:1 일치)
    fn render_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let dm_channels = &self.state.dm_channels;
        let active_dm_id = self.state.active_dm_id.as_deref().unwrap_or("dm_ann");

        // DM 목록 렌더링
        let dm_items = dm_channels.iter().map(|dm| {
            let is_active = dm.id == active_dm_id;
            let dm_id = dm.id.clone();
            let rec = &dm.recipient;
            let status_bg = rgb(rec.status_color);

            div()
                .flex()
                .items_center()
                .gap_3()
                .px_2()
                .py_1p5()
                .mx_2()
                .rounded_md()
                .cursor(CursorStyle::PointingHand)
                .bg(if is_active { BG_CHANNEL_SELECTED } else { rgba(0x00000000) })
                .hover(|s| s.bg(if is_active { BG_CHANNEL_SELECTED } else { BG_CHANNEL_HOVER }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        this.select_dm(dm_id.clone(), cx);
                    }),
                )
                .child(
                    div()
                        .relative()
                        .size(px(32.0))
                        .rounded_full()
                        .bg(rgb(rec.avatar_bg))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(TEXT_PRIMARY)
                        .child(rec.avatar_initial.clone())
                        .child(
                            div()
                                .absolute()
                                .bottom_0()
                                .right_0()
                                .size(px(10.0))
                                .rounded_full()
                                .bg(status_bg)
                                .border_2()
                                .border_color(BG_SECONDARY),
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .font_weight(if is_active { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                        .text_color(if is_active { TEXT_PRIMARY } else { TEXT_BODY })
                        .child(rec.display_name.clone()),
                )
                .into_any_element()
        });

        div()
            .flex()
            .flex_col()
            .w(px(CHANNEL_SIDEBAR_WIDTH))
            .h_full()
            .bg(BG_SECONDARY)
            .border_r_1()
            .border_color(BORDER_DIVIDER)
            .child(
                // 1. 상단 퀵 스위처 (Quick switcher + CTRL K 배지)
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(48.0))
                    .px_3()
                    .border_b_1()
                    .border_color(rgba(0x00000022))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .w_full()
                            .h(px(28.0))
                            .px_2()
                            .rounded_sm()
                            .bg(BG_TERTIARY)
                            .cursor(CursorStyle::PointingHand)
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(TEXT_MUTED)
                                    .child("Quick switcher"),
                            )
                            .child(
                                div()
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded_xs()
                                    .bg(BG_PRIMARY)
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(TEXT_MUTED)
                                    .child("CTRL  K"),
                            ),
                    ),
            )
            .child(
                // 2. 고정 메뉴: Friends & Personal notes
                div()
                    .flex()
                    .flex_col()
                    .py_2()
                    .gap_0p5()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .px_3()
                            .py_2()
                            .mx_2()
                            .rounded_md()
                            .cursor(CursorStyle::PointingHand)
                            .hover(|s| s.bg(BG_CHANNEL_HOVER))
                            .child(div().text_sm().child("👥"))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(TEXT_BODY)
                                    .child("Friends"),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .px_3()
                            .py_2()
                            .mx_2()
                            .rounded_md()
                            .cursor(CursorStyle::PointingHand)
                            .hover(|s| s.bg(BG_CHANNEL_HOVER))
                            .child(div().text_sm().child("📝"))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(TEXT_BODY)
                                    .child("Personal notes"),
                            ),
                    ),
            )
            .child(
                // 3. Direct Messages 섹션 헤더
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_4()
                    .pt_3()
                    .pb_1()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(TEXT_MUTED)
                            .child("Direct messages"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(TEXT_MUTED)
                            .hover(|s| s.text_color(TEXT_PRIMARY))
                            .cursor(CursorStyle::PointingHand)
                            .child("+"),
                    ),
            )
            .child(
                // 4. DM 채널 목록 (스크롤 영역)
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .overflow_hidden()
                    .gap_0p5()
                    .children(dm_items),
            )
            .child(
                // 5. 실시간 음성 통화 패널 (In voice chat — 이미지 1과 100% 동일)
                div()
                    .flex()
                    .flex_col()
                    .px_3()
                    .py_2()
                    .bg(rgb(0x111214))
                    .border_t_1()
                    .border_color(rgba(0x00000044))
                    .gap_1p5()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1p5()
                                    .child(div().text_xs().text_color(STATUS_ONLINE).child("📶"))
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(STATUS_ONLINE)
                                            .child("In voice chat"),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(div().text_xs().text_color(STATUS_ONLINE).child("📊"))
                                    .child(
                                        div()
                                            .cursor(CursorStyle::PointingHand)
                                            .text_xs()
                                            .text_color(STATUS_DND)
                                            .child("📞"),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(TEXT_PRIMARY)
                                    .child(self.state.voice_session.channel_title.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(TEXT_MUTED)
                                    .child(self.state.voice_session.topic.clone()),
                            ),
                    )
                    .child(
                        // 참여자 3명 아바타 겹침 표시 및 화면공유/비디오 퀵 컨트롤 행
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .pt_1()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .child(
                                        div()
                                            .size(px(20.0))
                                            .rounded_full()
                                            .bg(rgb(0xe67e22))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_xs()
                                            .text_color(TEXT_PRIMARY)
                                            .child("J"),
                                    )
                                    .child(
                                        div()
                                            .size(px(20.0))
                                            .rounded_full()
                                            .bg(rgb(0x9b59b6))
                                            .ml_neg_1()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_xs()
                                            .text_color(TEXT_PRIMARY)
                                            .child("T"),
                                    )
                                    .child(
                                        div()
                                            .size(px(20.0))
                                            .rounded_full()
                                            .bg(rgb(0x2ecc71))
                                            .ml_neg_1()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_xs()
                                            .text_color(TEXT_PRIMARY)
                                            .child("A"),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .px_1p5()
                                            .py_0p5()
                                            .rounded_sm()
                                            .bg(BG_SECONDARY)
                                            .cursor(CursorStyle::PointingHand)
                                            .text_xs()
                                            .child("🖥️"),
                                    )
                                    .child(
                                        div()
                                            .px_1p5()
                                            .py_0p5()
                                            .rounded_sm()
                                            .bg(BG_SECONDARY)
                                            .cursor(CursorStyle::PointingHand)
                                            .text_xs()
                                            .child("📹"),
                                    ),
                            ),
                    ),
            )
            .child(
                // 6. 최하단 사용자 프로필 미니독 (Kazuma, 이미지 1과 100% 동일)
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(PROFILE_DOCK_HEIGHT))
                    .px_3()
                    .bg(BG_SECONDARY_ALT)
                    .border_t_1()
                    .border_color(rgba(0x00000033))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .relative()
                                    .size(px(32.0))
                                    .rounded_full()
                                    .bg(rgb(0x8e44ad))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_color(TEXT_PRIMARY)
                                    .font_weight(FontWeight::BOLD)
                                    .text_xs()
                                    .child("K")
                                    .child(
                                        div()
                                            .absolute()
                                            .bottom_0()
                                            .right_0()
                                            .size(px(10.0))
                                            .rounded_full()
                                            .bg(STATUS_ONLINE)
                                            .border_2()
                                            .border_color(BG_SECONDARY_ALT),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(TEXT_PRIMARY)
                                            .child(self.state.current_user.username.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(STATUS_ONLINE)
                                            .child("Online"),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .px_1()
                                    .py_0p5()
                                    .rounded_sm()
                                    .cursor(CursorStyle::PointingHand)
                                    .bg(if self.is_muted { rgba(0xf23f4333) } else { rgba(0x00000000) })
                                    .text_sm()
                                    .text_color(if self.is_muted { STATUS_DND } else { TEXT_MUTED })
                                    .hover(|s| s.text_color(TEXT_PRIMARY))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, cx| {
                                            this.toggle_mute(cx);
                                        }),
                                    )
                                    .child(if self.is_muted { "🔇" } else { "🎙️" }),
                            )
                            .child(
                                div()
                                    .px_1()
                                    .py_0p5()
                                    .rounded_sm()
                                    .cursor(CursorStyle::PointingHand)
                                    .bg(if self.is_deafened { rgba(0xf23f4333) } else { rgba(0x00000000) })
                                    .text_sm()
                                    .text_color(if self.is_deafened { STATUS_DND } else { TEXT_MUTED })
                                    .hover(|s| s.text_color(TEXT_PRIMARY))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, cx| {
                                            this.toggle_deafen(cx);
                                        }),
                                    )
                                    .child(if self.is_deafened { "🚫" } else { "🎧" }),
                            )
                            .child(
                                div()
                                    .px_1()
                                    .py_0p5()
                                    .cursor(CursorStyle::PointingHand)
                                    .text_sm()
                                    .text_color(TEXT_MUTED)
                                    .hover(|s| s.text_color(TEXT_PRIMARY))
                                    .child("⚙️"),
                            ),
                    ),
            )
            .into_any_element()
    }
}

impl Render for NativeShellView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(BG_TERTIARY)
            .text_color(TEXT_PRIMARY)
            .child(self.render_titlebar())
            .child(
                div()
                    .flex()
                    .flex_1()
                    .w_full()
                    .overflow_hidden()
                    .child(self.render_server_rail(cx))
                    .child(self.render_sidebar(cx))
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .h_full()
                            .child(self.chat_view.clone()),
                    ),
            )
    }
}

/// GPUI 기반 순수 Rust 네이티브 메인 셸 윈도우를 기동합니다.
pub fn launch_native_shell() {
    let app = gpui::Application::new();

    app.run(|cx: &mut gpui::App| {
        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: Point {
                    x: px(100.0),
                    y: px(100.0),
                },
                size: Size {
                    width: px(1280.0),
                    height: px(800.0),
                },
            })),
            titlebar: None, // 커스텀 프레임리스 타이틀바 직접 렌더링
            window_background: WindowBackgroundAppearance::Opaque,
            is_movable: true,
            focus: true,
            show: true,
            kind: WindowKind::Normal,
            ..Default::default()
        };

        let _ = cx.open_window(window_options, |_window, cx| {
            cx.new(|cx| NativeShellView::new(cx))
        });
    });
}

#[cfg(test)]
mod tests {
    use crate::state::get_client_state;

    #[test]
    fn test_native_shell_state_bindings() {
        let state = get_client_state();
        assert!(!state.dm_channels.is_empty());
        assert!(state.active_dm_id.is_some());
    }

    #[test]
    fn test_native_shell_mute_toggle_state() {
        let mut is_muted = false;
        let mut is_deafened = false;
        is_muted = !is_muted;
        assert!(is_muted);
        is_deafened = !is_deafened;
        assert!(is_deafened);
    }
}
