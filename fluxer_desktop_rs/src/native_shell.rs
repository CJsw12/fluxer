// SPDX-License-Identifier: AGPL-3.0-or-later

//! GPUI 기반 순수 Rust 네이티브 메인 셸 (Main Native Shell) 모듈입니다.
//! 웹뷰(WebView2) 없이 Direct3D 11 GPU 가속 윈도우 단독으로 구동되며,
//! 길드 레일, 채널 사이드바, 채팅 뷰포트, 멤버 리스트를 통합 제공합니다.

use gpui::*;
use crate::chat_view::ChatView;
use crate::state::{get_client_state, ClientAppState, GuildModel};

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

    /// 최좌측 길드(서버) 레일 렌더러 (너비 72px)
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
                        .bg(if is_active { rgb(0xffffff) } else { rgba(0xffffff44) }),
                )
                .child(
                    div()
                        .size(px(48.0))
                        .rounded(if is_active { px(16.0) } else { px(24.0) })
                        .bg(if is_active { rgb(0x2563eb) } else { rgb(0x1e293b) })
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(rgb(0xffffff))
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
            .w(px(72.0))
            .h_full()
            .bg(rgb(0x020617)) // #020617 깊은 다크 레일
            .py_3()
            .gap_2()
            .child(
                // Fluxer 홈 / DM 아이콘
                div()
                    .size(px(48.0))
                    .rounded_2xl()
                    .bg(rgb(0x3b82f6))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(rgb(0xffffff))
                    .font_weight(FontWeight::BOLD)
                    .child("⚡"),
            )
            .child(
                div()
                    .w(px(32.0))
                    .h(px(2.0))
                    .bg(rgba(0x33415566))
                    .rounded_full()
                    .my_1(),
            )
            .children(guild_items)
            .child(
                // 길드 추가 버튼
                div()
                    .size(px(48.0))
                    .rounded_full()
                    .bg(rgba(0x1e293b88))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(rgb(0x22c55e))
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .child("+"),
            )
            .into_any_element()
    }

    /// 채널 목록 및 사용자 프로필 사이드바 렌더러 (너비 240px)
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
                .bg(if is_active { rgba(0x33415588) } else { rgba(0x00000000) })
                .hover(|s| s.bg(rgba(0x33415544)))
                .child(
                    div()
                        .text_sm()
                        .text_color(if is_active { rgb(0xf8fafc) } else { rgb(0x94a3b8) })
                        .child(icon),
                )
                .child(
                    div()
                        .text_sm()
                        .font_weight(if is_active { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                        .text_color(if is_active { rgb(0xffffff) } else { rgb(0x94a3b8) })
                        .child(channel.name.clone()),
                )
                .into_any_element()
        });

        div()
            .flex()
            .flex_col()
            .w(px(240.0))
            .h_full()
            .bg(rgb(0x0f172a)) // #0f172a 채널 사이드바
            .border_r_1()
            .border_color(rgba(0x33415588))
            .child(
                // 길드 이름 헤더
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(48.0))
                    .px_4()
                    .border_b_1()
                    .border_color(rgba(0x33415588))
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(0xf8fafc))
                            .text_sm()
                            .child(guild_name),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x94a3b8))
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
                            .text_color(rgb(0x64748b))
                            .child("CHANNELS"),
                    )
                    .children(channel_items),
            )
            .child(
                // 하단 사용자 프로필 미니 독
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(52.0))
                    .px_3()
                    .bg(rgb(0x0a0f1d))
                    .border_t_1()
                    .border_color(rgba(0x33415566))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .size(px(32.0))
                                    .rounded_full()
                                    .bg(rgb(0x2563eb))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_color(rgb(0xffffff))
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
                                            .text_color(rgb(0xf8fafc))
                                            .child(self.state.current_user.username.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(0x22c55e))
                                            .child("Online"),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().text_sm().text_color(rgb(0x94a3b8)).child("🎙️"))
                            .child(div().text_sm().text_color(rgb(0x94a3b8)).child("🎧"))
                            .child(div().text_sm().text_color(rgb(0x94a3b8)).child("⚙️")),
                    ),
            )
            .into_any_element()
    }

    /// 우측 온라인 멤버 목록 렌더러 (너비 200px)
    fn render_member_list(&self) -> AnyElement {
        let members = vec![
            ("Fluxer Bot", "BOT", rgb(0x60a5fa)),
            ("Lead Engineer", "ONLINE", rgb(0x22c55e)),
            ("Community Member", "ONLINE", rgb(0x94a3b8)),
        ];

        let member_items = members.into_iter().map(|(name, status, status_color)| {
            div()
                .flex()
                .items_center()
                .gap_2p5()
                .px_3()
                .py_1p5()
                .rounded_md()
                .hover(|s| s.bg(rgba(0x33415544)))
                .child(
                    div()
                        .size(px(28.0))
                        .rounded_full()
                        .bg(rgb(0x334155))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_xs()
                        .text_color(rgb(0xffffff))
                        .child(name.chars().next().unwrap_or('U').to_string()),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(0xf1f5f9))
                                .child(name),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(status_color)
                                .child(status),
                        ),
                )
                .into_any_element()
        });

        div()
            .flex()
            .flex_col()
            .w(px(200.0))
            .h_full()
            .bg(rgb(0x0f172a))
            .border_l_1()
            .border_color(rgba(0x33415588))
            .py_3()
            .gap_1()
            .child(
                div()
                    .px_3()
                    .pb_1()
                    .text_xs()
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(0x64748b))
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
            .bg(rgb(0x020617))
            .text_color(rgb(0xf8fafc))
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
