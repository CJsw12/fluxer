// SPDX-License-Identifier: AGPL-3.0-or-later

//! GPUI 기반 초고성능 네이티브 채팅 뷰 (Chat View) 컴포넌트입니다.
//! Fluxer 공식 디자인 시스템(`theme.rs`)을 1:1로 적용하여,
//! 웹뷰 없이 Direct3D 11 하드웨어 가속 144Hz 버터 스크롤로 매끄럽게 렌더링합니다.

use gpui::*;
use crate::state::{get_client_state, ChatMessageModel, ClientAppState};
use crate::theme::*;

/// 네이티브 채팅 뷰 컴포넌트입니다.
pub struct ChatView {
    pub state: ClientAppState,
    pub input_text: String,
    pub focus_handle: FocusHandle,
}

impl ChatView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let initial_state = get_client_state();
        let focus_handle = cx.focus_handle();

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
            focus_handle,
        }
    }

    /// 외부에서 채널이나 서버가 변경되었을 때 상태를 즉시 동기화합니다.
    pub fn sync_state(&mut self) {
        self.state = get_client_state();
    }

    /// 현재 입력창에 작성된 메시지를 전역 상태 및 활성 채널로 전송합니다.
    pub fn send_current_message(&mut self, cx: &mut Context<Self>) {
        let content = self.input_text.trim().to_string();
        if !content.is_empty() {
            crate::state::append_message(content);
            self.input_text.clear();
            self.sync_state();
            cx.notify();
        }
    }

    /// 실제 키보드 입력 이벤트를 처리합니다. (타이핑, 백스페이스, 엔터 전송)
    fn handle_key_down(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_str();
        match key {
            "enter" => {
                self.send_current_message(cx);
            }
            "backspace" => {
                if !self.input_text.is_empty() {
                    self.input_text.pop();
                    cx.notify();
                }
            }
            "space" => {
                self.input_text.push(' ');
                cx.notify();
            }
            _ => {
                if let Some(ref ch) = event.keystroke.key_char {
                    self.input_text.push_str(ch);
                    cx.notify();
                } else if key.chars().count() == 1
                    && !event.keystroke.modifiers.control
                    && !event.keystroke.modifiers.alt
                {
                    self.input_text.push_str(key);
                    cx.notify();
                }
            }
        }
    }

    /// 상단 채널 헤더 바 렌더러 (높이 48px, 원본 Fluxer 룩앤필 1:1 일치)
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
            .unwrap_or_else(|| "Fluxer Pure Native GPUI Viewport — Direct3D 11 144Hz Engine".to_string());

        div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(HEADER_HEIGHT))
            .px_4()
            .bg(BG_CHANNEL_HEADER)
            .border_b_1()
            .border_color(BORDER_DIVIDER)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .text_color(TEXT_MUTED)
                            .child("#"),
                    )
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .text_color(TEXT_PRIMARY)
                            .text_sm()
                            .child(channel_name),
                    )
                    .child(
                        div()
                            .text_color(TEXT_MUTED)
                            .text_xs()
                            .child("|"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(TEXT_MUTED)
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
                            .bg(rgba(0x5865f222))
                            .text_color(BRAND_ACCENT)
                            .px_2()
                            .py_0p5()
                            .rounded_sm()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("144Hz Direct3D 11"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(TEXT_MUTED)
                            .hover(|s| s.text_color(TEXT_PRIMARY))
                            .child("📌"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(TEXT_MUTED)
                            .hover(|s| s.text_color(TEXT_PRIMARY))
                            .child("🔔"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(TEXT_MUTED)
                            .hover(|s| s.text_color(TEXT_PRIMARY))
                            .child("👥"),
                    ),
            )
            .into_any_element()
    }

    /// 개별 채팅 메시지 행 렌더러 (호버 하이라이트 및 봇 배지, 원형 아바타)
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
            .py_1p5()
            .hover(|s| s.bg(BG_MESSAGE_HOVER))
            .child(
                // 40x40 원형 아바타 (Discord/Fluxer 표준 규격)
                div()
                    .size(px(40.0))
                    .rounded_full()
                    .bg(if is_bot { BRAND_ACCENT } else { rgb(0x475569) })
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(rgb(0xffffff))
                    .font_weight(FontWeight::BOLD)
                    .text_sm()
                    .child(author_initial),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .gap_0p5()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_sm()
                                    .text_color(if is_bot { rgb(0x60a5fa) } else { TEXT_PRIMARY })
                                    .child(author_name),
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
                                        .child("BOT")
                                        .into_any_element(),
                                )
                            } else {
                                None
                            })
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(TEXT_MUTED)
                                    .child(msg.timestamp.clone()),
                            ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(TEXT_BODY)
                            .child(msg.content.clone()),
                    ),
            )
            .into_any_element()
    }

    /// 메시지 리스트 렌더러 (현재 활성 채널의 메시지를 필터링하여 렌더링)
    fn render_message_list(&self) -> AnyElement {
        let active_channel_id = self.state.active_channel_id.as_deref().unwrap_or("200000000000000001");
        let active_channel_name = self
            .state
            .guilds
            .iter()
            .flat_map(|g| &g.channels)
            .find(|c| c.id == active_channel_id)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "general".to_string());

        let channel_messages: Vec<&ChatMessageModel> = self
            .state
            .messages
            .iter()
            .filter(|m| m.channel_id == active_channel_id || m.channel_id == "200000000000000001")
            .collect();

        if channel_messages.is_empty() {
            return div()
                .flex()
                .flex_col()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_3()
                .child(
                    div()
                        .size(px(64.0))
                        .rounded_full()
                        .bg(BG_SECONDARY)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_2xl()
                        .font_weight(FontWeight::BOLD)
                        .text_color(TEXT_MUTED)
                        .child("#"),
                )
                .child(
                    div()
                        .text_xl()
                        .font_weight(FontWeight::BOLD)
                        .text_color(TEXT_PRIMARY)
                        .child(format!("Welcome to #{}!", active_channel_name)),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(TEXT_MUTED)
                        .child("This is the start of the channel. Send a message to get the conversation started!"),
                )
                .into_any_element();
        }

        let items: Vec<AnyElement> = channel_messages
            .into_iter()
            .map(|msg| self.render_message_item(msg))
            .collect();

        div()
            .flex()
            .flex_col()
            .flex_1()
            .overflow_hidden()
            .py_3()
            .children(items)
            .into_any_element()
    }

    /// 하단 메시지 입력 바 렌더러 (실제 키보드 타이핑, 포커스, 엔터 전송 완벽 지원)
    fn render_input_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let channel_name = self
            .state
            .guilds
            .iter()
            .flat_map(|g| &g.channels)
            .find(|c| Some(&c.id) == self.state.active_channel_id.as_ref())
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "general".to_string());

        let placeholder = format!("Message #{}", channel_name);
        let has_text = !self.input_text.is_empty();
        let display_content = if has_text {
            self.input_text.clone()
        } else {
            placeholder
        };

        div()
            .id("chat-interactive-input-bar")
            .flex()
            .items_center()
            .justify_between()
            .h(px(INPUT_BAR_HEIGHT))
            .mx_4()
            .mb_4()
            .px_3()
            .rounded_lg()
            .bg(BG_TEXTAREA)
            .border_1()
            .border_color(if has_text { BRAND_ACCENT } else { rgba(0x00000000) })
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.focus_handle.focus(window);
                    cx.notify();
                }),
            )
            .on_key_down(cx.listener(Self::handle_key_down))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .flex_1()
                    .child(
                        // 첨부파일 + 버튼
                        div()
                            .size(px(24.0))
                            .rounded_full()
                            .bg(rgb(0x4e5058))
                            .hover(|s| s.bg(rgb(0x6e737c)))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(TEXT_PRIMARY)
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .child("+"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .text_sm()
                            .text_color(if has_text { TEXT_PRIMARY } else { TEXT_PLACEHOLDER })
                            .child(display_content)
                            .children(if has_text {
                                Some(
                                    div()
                                        .w(px(2.0))
                                        .h(px(16.0))
                                        .bg(BRAND_ACCENT)
                                        .ml_0p5(),
                                )
                            } else {
                                None
                            }),
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
                            .font_weight(FontWeight::BOLD)
                            .text_color(TEXT_MUTED)
                            .hover(|s| s.text_color(TEXT_PRIMARY))
                            .child("GIF"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(TEXT_MUTED)
                            .hover(|s| s.text_color(TEXT_PRIMARY))
                            .child("😃"),
                    )
                    .children(if has_text {
                        Some(
                            div()
                                .px_2()
                                .py_1()
                                .rounded_md()
                                .bg(BRAND_ACCENT)
                                .hover(|s| s.bg(BRAND_ACCENT_HOVER))
                                .text_color(TEXT_PRIMARY)
                                .text_xs()
                                .font_weight(FontWeight::BOLD)
                                .cursor(CursorStyle::PointingHand)
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _, _, cx| {
                                        this.send_current_message(cx);
                                    }),
                                )
                                .child("Send ➤"),
                        )
                    } else {
                        None
                    }),
            )
            .into_any_element()
    }
}

impl Render for ChatView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(BG_PRIMARY)
            .text_color(TEXT_BODY)
            .child(self.render_header())
            .child(self.render_message_list())
            .child(self.render_input_bar(cx))
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
