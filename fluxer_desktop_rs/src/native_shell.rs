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
}

impl NativeShellView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let chat_view = cx.new(|cx| ChatView::new(cx));
        Self {
            state: get_client_state(),
            chat_view,
        }
    }

    /// 최좌측 길드(서버) 레일 렌더러 (너비 72px, 원본 Fluxer 룩앤필 1:1 일치)
    fn render_guild_rail(&self) -> AnyElement {
        let guilds = &self.state.guilds;
        let active_id = self.state.active_guild_id.as_ref();

        let guild_items = guilds.iter().map(|guild| {
            let is_active = Some(&guild.id) == active_id;
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

    /// 채널 목록 및 사용자 프로필 사이드바 렌더러 (너비 240px, 원본 Fluxer 룩앤필 1:1 일치)
    fn render_channel_sidebar(&self) -> AnyElement {
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

        let channel_items = channels.iter().map(|channel| {
            let is_active = Some(&channel.id) == active_channel_id;
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
                .bg(if is_active { BG_CHANNEL_SELECTED } else { rgba(0x00000000) })
                .hover(|s| s.bg(if is_active { BG_CHANNEL_SELECTED } else { BG_CHANNEL_HOVER }))
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
                // 채널 리스트
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
                            .child("CHANNELS"),
                    )
                    .children(channel_items),
            )
            .child(
                // 하단 사용자 프로필 미니 독 (높이 52px)
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
                                    .text_sm()
                                    .text_color(TEXT_MUTED)
                                    .hover(|s| s.text_color(TEXT_PRIMARY))
                                    .child("🎙️"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(TEXT_MUTED)
                                    .hover(|s| s.text_color(TEXT_PRIMARY))
                                    .child("🎧"),
                            )
                            .child(
                                div()
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
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .bg(BG_TERTIARY)
            .text_color(TEXT_PRIMARY)
            .child(self.render_guild_rail())
            .child(self.render_channel_sidebar())
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
}
