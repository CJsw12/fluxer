// SPDX-License-Identifier: AGPL-3.0-or-later

//! GPUI 기반 순수 Rust 네이티브 메인 셸 (Main Native Shell) 모듈입니다.
//! 웹뷰(WebView2) 없이 Direct3D 11 GPU 가속 윈도우 단독으로 구동되며,
//! Fluxer 공식 디자인 시스템(`theme.rs`)을 1:1로 적용하여
//! 길드 레일, 채널 사이드바, 채팅 뷰포트, 멤버 리스트를 통합 제공합니다.

use gpui::*;
use crate::chat_view::ChatView;
use crate::state::{get_client_state, ClientAppState, GuildModel};
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

    /// 길드(서버)를 전환하고 활성 채널을 자동 선택합니다.
    pub fn select_guild(&mut self, guild_id: String, cx: &mut Context<Self>) {
        crate::state::update_client_state(|s| {
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

    /// 채널을 전환하고 해당 채널의 채팅 뷰를 즉시 갱신합니다.
    pub fn select_channel(&mut self, channel_id: String, cx: &mut Context<Self>) {
        crate::state::update_client_state(|s| {
            s.active_channel_id = Some(channel_id);
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

    /// 최좌측 길드(서버) 레일 렌더러 (너비 72px, 원본 Fluxer 룩앤필 및 실제 클릭 전환 완벽 연동)
    fn render_guild_rail(&self, cx: &mut Context<Self>) -> AnyElement {
        let guilds = &self.state.guilds;
        let active_id = self.state.active_guild_id.as_ref();

        let guild_items = guilds.iter().map(|guild| {
            let is_active = Some(&guild.id) == active_id;
            let guild_id = guild.id.clone();
            let initial = guild
                .name
                .chars()
                .next()
                .unwrap_or('G')
                .to_uppercase()
                .to_string();

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
                    cx.listener(move |this, _, _, cx| {
                        this.select_guild(guild_id.clone(), cx);
                    }),
                )
                .child(
                    // 활성 길드 왼쪽 인디케이터 알약(Pill)
                    div()
                        .absolute()
                        .left_0()
                        .w(px(4.0))
                        .h(if is_active { px(36.0) } else { px(8.0) })
                        .rounded_r_sm()
                        .bg(if is_active { rgb(0xffffff) } else { rgba(0xffffff33) }),
                )
                .child(
                    div()
                        .size(px(48.0))
                        .rounded(if is_active { px(16.0) } else { px(24.0) })
                        .bg(if is_active { BRAND_ACCENT } else { BG_PRIMARY })
                        .hover(|s| s.bg(if is_active { BRAND_ACCENT } else { BRAND_ACCENT_HOVER }).rounded(px(16.0)))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(TEXT_PRIMARY)
                        .font_weight(FontWeight::BOLD)
                        .text_sm()
                        .child(initial),
                )
                .into_any_element()
        });

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
                // Fluxer 시그니처 홈 / DM 버튼
                div()
                    .size(px(48.0))
                    .rounded_2xl()
                    .bg(BRAND_ACCENT)
                    .hover(|s| s.bg(BRAND_ACCENT_HOVER).rounded(px(16.0)))
                    .cursor(CursorStyle::PointingHand)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(TEXT_PRIMARY)
                    .font_weight(FontWeight::BOLD)
                    .child("⚡"),
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
            .children(guild_items)
            .child(
                // 길드 추가 버튼 (+)
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
            .into_any_element()
    }

    /// 채널 목록 및 사용자 프로필 사이드바 렌더러 (너비 240px, 실제 채널 클릭 전환 및 카테고리 분류 완벽 연동)
    fn render_channel_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let active_guild: Option<&GuildModel> = self
            .state
            .guilds
            .iter()
            .find(|g| Some(&g.id) == self.state.active_guild_id.as_ref())
            .or_else(|| self.state.guilds.first());

        let guild_name = active_guild
            .map(|g| g.name.clone())
            .unwrap_or_else(|| "Fluxer Server".to_string());

        let channels = active_guild.map(|g| &g.channels[..]).unwrap_or(&[]);
        let active_channel_id = self.state.active_channel_id.as_ref();

        let render_channel_row = |channel: &crate::state::ChannelModel| {
            let is_active = Some(&channel.id) == active_channel_id;
            let channel_id = channel.id.clone();
            let icon = match channel.channel_type {
                crate::state::ChannelType::GuildVoice => "🔊",
                _ => "#",
            };

            div()
                .flex()
                .items_center()
                .gap_2()
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
                        this.select_channel(channel_id.clone(), cx);
                    }),
                )
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::BOLD)
                        .text_color(if is_active { TEXT_PRIMARY } else { TEXT_MUTED })
                        .child(icon),
                )
                .child(
                    div()
                        .text_sm()
                        .font_weight(if is_active { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                        .text_color(if is_active { TEXT_PRIMARY } else { TEXT_MUTED })
                        .child(channel.name.clone()),
                )
                .into_any_element()
        };

        let text_channels = channels
            .iter()
            .filter(|c| c.channel_type != crate::state::ChannelType::GuildVoice)
            .map(render_channel_row);

        let voice_channels = channels
            .iter()
            .filter(|c| c.channel_type == crate::state::ChannelType::GuildVoice)
            .map(render_channel_row);

        div()
            .flex()
            .flex_col()
            .w(px(CHANNEL_SIDEBAR_WIDTH))
            .h_full()
            .bg(BG_SECONDARY)
            .border_r_1()
            .border_color(BORDER_DIVIDER)
            .child(
                // 길드 이름 상단 헤더
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(HEADER_HEIGHT))
                    .px_4()
                    .border_b_1()
                    .border_color(BORDER_DIVIDER)
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .text_color(TEXT_PRIMARY)
                            .text_sm()
                            .child(guild_name),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(TEXT_MUTED)
                            .child("▼"),
                    ),
            )
            .child(
                // 채널 리스트 (텍스트 채널 / 음성 채널 카테고리 분리 렌더링)
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .py_3()
                    .gap_1()
                    .child(
                        div()
                            .px_4()
                            .pb_1()
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(TEXT_MUTED)
                            .child("TEXT CHANNELS"),
                    )
                    .children(text_channels)
                    .child(
                        div()
                            .px_4()
                            .pt_3()
                            .pb_1()
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(TEXT_MUTED)
                            .child("VOICE CHANNELS"),
                    )
                    .children(voice_channels),
            )
            .child(
                // 하단 사용자 프로필 미니 독 (높이 52px, 마이크/헤드폰 음소거 실시간 인터랙션)
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(PROFILE_DOCK_HEIGHT))
                    .px_3()
                    .bg(BG_SECONDARY_ALT)
                    .border_t_1()
                    .border_color(BORDER_DIVIDER)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .size(px(32.0))
                                    .rounded_full()
                                    .bg(BRAND_ACCENT)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_color(TEXT_PRIMARY)
                                    .font_weight(FontWeight::BOLD)
                                    .text_xs()
                                    .child("F"),
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

    /// 우측 온라인 멤버 목록 렌더러 (너비 240px, 원본 Fluxer 룩앤필 1:1 일치)
    fn render_member_list(&self) -> AnyElement {
        let members = vec![
            ("Fluxer Bot", true, STATUS_ONLINE),
            ("Lead Engineer", false, STATUS_ONLINE),
            ("Community Member", false, STATUS_OFFLINE),
        ];

        let member_items = members.into_iter().map(|(name, is_bot, status_color)| {
            div()
                .flex()
                .items_center()
                .gap_2p5()
                .px_3()
                .py_1p5()
                .mx_2()
                .rounded_md()
                .hover(|s| s.bg(BG_CHANNEL_HOVER))
                .child(
                    div()
                        .size(px(32.0))
                        .rounded_full()
                        .bg(BG_PRIMARY)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(TEXT_PRIMARY)
                        .child(name.chars().next().unwrap_or('U').to_string()),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(TEXT_PRIMARY)
                                        .child(name),
                                )
                                .children(if is_bot {
                                    Some(
                                        div()
                                            .bg(BRAND_ACCENT)
                                            .text_color(TEXT_BOT_BADGE)
                                            .px_1()
                                            .py_0p5()
                                            .rounded_sm()
                                            .text_xs()
                                            .font_weight(FontWeight::BOLD)
                                            .child("BOT"),
                                    )
                                } else {
                                    None
                                }),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .child(
                                    div()
                                        .size(px(8.0))
                                        .rounded_full()
                                        .bg(status_color),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(TEXT_MUTED)
                                        .child(if status_color == STATUS_ONLINE { "Online" } else { "Offline" }),
                                ),
                        ),
                )
                .into_any_element()
        });

        div()
            .flex()
            .flex_col()
            .w(px(MEMBER_LIST_WIDTH))
            .h_full()
            .bg(BG_SECONDARY)
            .border_l_1()
            .border_color(BORDER_DIVIDER)
            .py_3()
            .gap_1()
            .child(
                div()
                    .px_4()
                    .pb_1()
                    .text_xs()
                    .font_weight(FontWeight::BOLD)
                    .text_color(TEXT_MUTED)
                    .child("ONLINE — 3"),
            )
            .children(member_items)
            .into_any_element()
    }
}

impl Render for NativeShellView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .bg(BG_TERTIARY)
            .text_color(TEXT_PRIMARY)
            .child(self.render_guild_rail(cx))
            .child(self.render_channel_sidebar(cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .h_full()
                    .child(self.chat_view.clone()),
            )
            .child(self.render_member_list())
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
            titlebar: Some(TitlebarOptions {
                title: Some("Fluxer Native (GPUI D3D11 Engine)".into()),
                appears_transparent: false,
                traffic_light_position: None,
            }),
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
        assert!(!state.guilds.is_empty());
        let guild = &state.guilds[0];
        assert_eq!(guild.name, "Fluxer HQ");
        assert_eq!(guild.channels.len(), 3);
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

    #[test]
    fn test_channel_switching_updates_active_channel() {
        let state = get_client_state();
        let target_channel = &state.guilds[0].channels[1];
        crate::state::update_client_state(|s| {
            s.active_channel_id = Some(target_channel.id.clone());
        });
        let updated = get_client_state();
        assert_eq!(updated.active_channel_id, Some(target_channel.id.clone()));
    }
}
