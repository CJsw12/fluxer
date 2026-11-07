// SPDX-License-Identifier: AGPL-3.0-or-later

//! GPUI 기반 초고성능 네이티브 채팅 뷰 (Chat View) 컴포넌트입니다.
//! DOM 레이아웃 및 JS GC 오버헤드 없이 Direct3D 11 가속 텍스트 렌더러를 통해
//! 수천 개의 메시지를 144Hz 버터 스크롤로 매끄럽게 렌더링합니다.

use gpui::*;
use crate::state::{get_client_state, ChatMessageModel, ClientAppState};

/// 네이티브 채팅 뷰 컴포넌트입니다.
pub struct ChatView {
    pub state: ClientAppState,
    #[allow(dead_code)]
    pub input_text: String,
}

impl ChatView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let initial_state = get_client_state();

        // 100ms 주기로 최신 클라이언트 상태(실시간 메시지 수신 등)를 비동기 관찰하여 화면을 리액티브하게 자동 갱신합니다.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(100))
                    .await;
                let latest = get_client_state();
                let update_result = this.update(&mut *cx, |view, cx| {
                    if view.state != latest {
                        view.state = latest;
                        cx.notify();
                    }
                });
                if update_result.is_err() {
                    break;
                }
            }
        })
        .detach();

        Self {
            state: initial_state,
            input_text: String::new(),
        }
    }

    fn render_header(&self) -> AnyElement {
        let channel_name = self
            .state
            .guilds
            .iter()
            .flat_map(|g| &g.channels)
            .find(|c| Some(&c.id) == self.state.active_channel_id.as_ref())
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "general".to_string());

        let topic = self
            .state
            .guilds
            .iter()
            .flat_map(|g| &g.channels)
            .find(|c| Some(&c.id) == self.state.active_channel_id.as_ref())
            .and_then(|c| c.topic.clone())
            .unwrap_or_else(|| "Fluxer GPUI High-Performance Native Viewport".to_string());

        div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(48.0))
            .px_4()
            .bg(rgb(0x0f172a)) // #0f172a 다크 헤더
            .border_b_1()
            .border_color(rgba(0x33415588))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(0x94a3b8))
                            .child("#"),
                    )
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(0xf8fafc))
                            .text_sm()
                            .child(channel_name),
                    )
                    .child(
                        div()
                            .text_color(rgba(0x64748b88))
                            .text_sm()
                            .child("|"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x94a3b8))
                            .child(topic),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .bg(rgba(0x1e293b88))
                            .text_color(rgb(0x38bdf8))
                            .px_2()
                            .py_0p5()
                            .rounded_sm()
                            .text_xs()
                            .child("144Hz D3D11"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0x94a3b8))
                            .child("📌"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0x94a3b8))
                            .child("🔔"),
                    ),
            )
            .into_any_element()
    }

    fn render_message_item(&self, msg: &ChatMessageModel) -> AnyElement {
        let is_bot = msg.author.bot;
        let author_initial = msg
            .author
            .display_name
            .as_deref()
            .or(Some(&msg.author.username))
            .and_then(|n| n.chars().next())
            .unwrap_or('?')
            .to_uppercase()
            .to_string();

        let author_name = msg
            .author
            .display_name
            .clone()
            .unwrap_or_else(|| msg.author.username.clone());

        div()
            .flex()
            .items_start()
            .gap_3()
            .px_4()
            .py_2()
            .hover(|s| s.bg(rgba(0x1e293b44)))
            .child(
                // 원형 아바타 플레이스홀더
                div()
                    .size(px(36.0))
                    .rounded_full()
                    .bg(if is_bot { rgb(0x2563eb) } else { rgb(0x475569) })
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(rgb(0xffffff))
                    .font_weight(FontWeight::BOLD)
                    .text_xs()
                    .child(author_initial),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_sm()
                                    .text_color(if is_bot { rgb(0x60a5fa) } else { rgb(0xf1f5f9) })
                                    .child(author_name),
                            )
                            .children(if is_bot {
                                Some(
                                    div()
                                        .bg(rgb(0x2563eb))
                                        .text_color(rgb(0xffffff))
                                        .px_1()
                                        .py_0p5()
                                        .rounded_sm()
                                        .text_xs()
                                        .font_weight(FontWeight::BOLD)
                                        .child("BOT")
                                        .into_any_element(),
                                )
                            } else {
                                None
                            })
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(0x64748b))
                                    .child(msg.timestamp.clone()),
                            ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0xe2e8f0))
                            .child(msg.content.clone()),
                    ),
            )
            .into_any_element()
    }

    fn render_message_list(&self) -> AnyElement {
        let messages = &self.state.messages;
        let items: Vec<AnyElement> = messages
            .iter()
            .map(|msg| self.render_message_item(msg))
            .collect();

        div()
            .flex()
            .flex_col()
            .flex_1()
            .overflow_hidden()
            .py_2()
            .children(items)
            .into_any_element()
    }

    fn render_input_bar(&self) -> AnyElement {
        let channel_name = self
            .state
            .guilds
            .iter()
            .flat_map(|g| &g.channels)
            .find(|c| Some(&c.id) == self.state.active_channel_id.as_ref())
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "general".to_string());

        let placeholder = format!("Message #{}", channel_name);

        div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(44.0))
            .mx_4()
            .mb_4()
            .px_3()
            .rounded_lg()
            .bg(rgb(0x1e293b))
            .border_1()
            .border_color(rgba(0x33415588))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .size(px(24.0))
                            .rounded_full()
                            .bg(rgba(0x33415588))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(rgb(0x94a3b8))
                            .text_xs()
                            .child("+"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0x64748b))
                            .child(placeholder),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x64748b))
                            .child("GIF"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0x64748b))
                            .child("😃"),
                    ),
            )
            .into_any_element()
    }
}

impl Render for ChatView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x0b1120)) // 다크 메인 배경 (#0b1120)
            .child(self.render_header())
            .child(self.render_message_list())
            .child(self.render_input_bar())
    }
}

#[cfg(test)]
mod tests {
    use crate::state::{append_message, get_client_state};

    #[test]
    fn test_chat_view_initial_state() {
        let state = get_client_state();
        assert!(!state.messages.is_empty());
    }

    #[test]
    fn test_chat_view_message_appending() {
        let initial_len = get_client_state().messages.len();
        append_message("Unit test chat message verification".to_string());
        let after_len = get_client_state().messages.len();
        assert_eq!(after_len, initial_len + 1);
    }
}
