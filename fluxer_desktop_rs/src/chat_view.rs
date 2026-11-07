// SPDX-License-Identifier: AGPL-3.0-or-later

//! GPUI 기반 초고성능 네이티브 채팅 뷰 (Chat View) 컴포넌트입니다.
//! 원본 Fluxer의 DM 뷰포트 및 우측 상단 실시간 화면공유 PiP 비디오 타일(이미지 1)을 100% 일치하도록 구현합니다.

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

    /// 상단 DM 헤더 바 렌더러 (높이 48px, 이미지 1과 100% 동일)
    fn render_header(&self) -> AnyElement {
        let active_dm = self
            .state
            .dm_channels
            .iter()
            .find(|d| Some(&d.id) == self.state.active_dm_id.as_ref())
            .or_else(|| self.state.dm_channels.last());

        let recipient_name = active_dm
            .map(|d| d.recipient.display_name.clone())
            .unwrap_or_else(|| "Ann".to_string());

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
                // 좌측: 아바타 + 닉네임
                div()
                    .flex()
                    .items_center()
                    .gap_2p5()
                    .child(
                        div()
                            .relative()
                            .size(px(24.0))
                            .rounded_full()
                            .bg(rgb(0x2ecc71))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(TEXT_PRIMARY)
                            .child("A")
                            .child(
                                div()
                                    .absolute()
                                    .bottom_0()
                                    .right_0()
                                    .size(px(8.0))
                                    .rounded_full()
                                    .bg(STATUS_ONLINE)
                                    .border_1()
                                    .border_color(BG_PRIMARY),
                            ),
                    )
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .text_color(TEXT_PRIMARY)
                            .text_base()
                            .child(recipient_name),
                    ),
            )
            .child(
                // 우측 액션 아이콘들 (수화기, 캠코더, 핀, 친구추가, 알림)
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .child(
                        div()
                            .text_sm()
                            .text_color(TEXT_MUTED)
                            .hover(|s| s.text_color(TEXT_PRIMARY))
                            .cursor(CursorStyle::PointingHand)
                            .child("📞"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(TEXT_MUTED)
                            .hover(|s| s.text_color(TEXT_PRIMARY))
                            .cursor(CursorStyle::PointingHand)
                            .child("📹"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(TEXT_MUTED)
                            .hover(|s| s.text_color(TEXT_PRIMARY))
                            .cursor(CursorStyle::PointingHand)
                            .child("📌"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(TEXT_MUTED)
                            .hover(|s| s.text_color(TEXT_PRIMARY))
                            .cursor(CursorStyle::PointingHand)
                            .child("👤+"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(TEXT_MUTED)
                            .hover(|s| s.text_color(TEXT_PRIMARY))
                            .cursor(CursorStyle::PointingHand)
                            .child("🔔"),
                    ),
            )
            .into_any_element()
    }

    /// 우측 상단 픽처-인-픽처 (PiP) 실시간 화면공유 비디오 타일 (이미지 1의 핵심 시그니처 1:1 구현)
    fn render_pip_video_stream_tile(&self) -> AnyElement {
        if !self.state.pip_stream.is_visible {
            return div().into_any_element();
        }

        div()
            .absolute()
            .top(px(16.0))
            .right(px(16.0))
            .w(px(320.0))
            .h(px(180.0))
            .rounded_xl()
            .overflow_hidden()
            .border_2()
            .border_color(rgba(0x00000088))
            .bg(rgb(0x1a252f)) // 비디오 캔버스 배경
            .shadow_lg()
            .child(
                // 비디오 프레임 시뮬레이터 (애니메이션 비주얼 그라데이션)
                div()
                    .size_full()
                    .relative()
                    .flex()
                    .flex_col()
                    .justify_between()
                    .p_3()
                    .bg(rgb(0x27ae60)) // 선명한 그린/애니메이션 테마
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(rgba(0x000000aa))
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(0x2ecc71))
                                    .child("LIVE 60 FPS"),
                            )
                            .child(
                                div()
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(rgba(0x000000aa))
                                    .text_xs()
                                    .text_color(TEXT_PRIMARY)
                                    .cursor(CursorStyle::PointingHand)
                                    .child("⛶"),
                            ),
                    )
                    .child(
                        // 이미지 1의 실제 자막 문구: "You used to be a lot gloomier, didn't you?"
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .w_full()
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .bg(rgba(0x000000bb))
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(0xffffff))
                                    .child("\"You used to be a lot gloomier, didn't you?\""),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// 본문 메시지 아이템 렌더러
    fn render_message_item(&self, msg: &ChatMessageModel) -> AnyElement {
        let is_bot = msg.author.bot;
        let author_name = msg
            .author
            .display_name
            .clone()
            .unwrap_or_else(|| msg.author.username.clone());

        div()
            .flex()
            .items_start()
            .gap_4()
            .px_4()
            .py_1p5()
            .hover(|s| s.bg(BG_MESSAGE_HOVER))
            .child(
                // 40x40 원형 아바타
                div()
                    .size(px(40.0))
                    .rounded_full()
                    .bg(if is_bot { BRAND_ACCENT } else { rgb(0x8e44ad) })
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .font_weight(FontWeight::BOLD)
                    .text_color(TEXT_PRIMARY)
                    .child(author_name.chars().next().unwrap_or('U').to_string()),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
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

    /// 메인 대화 뷰포트 본문 렌더러 (이미지 1의 대형 원형 아바타, Ann#4873, mutual community, Remove friend 버튼 1:1 완벽 구현)
    fn render_message_list(&self) -> AnyElement {
        let active_dm = self
            .state
            .dm_channels
            .iter()
            .find(|d| Some(&d.id) == self.state.active_dm_id.as_ref())
            .or_else(|| self.state.dm_channels.last());

        let (username, discriminator, mutual) = active_dm
            .map(|d| (d.recipient.username.clone(), d.recipient.discriminator.clone(), d.recipient.mutual_community.clone()))
            .unwrap_or_else(|| ("Ann".to_string(), "4873".to_string(), "FR 1 mutual community".to_string()));

        let channel_messages: Vec<&ChatMessageModel> = self
            .state
            .messages
            .iter()
            .collect();

        let message_rows: Vec<AnyElement> = channel_messages
            .into_iter()
            .map(|msg| self.render_message_item(msg))
            .collect();

        div()
            .flex()
            .flex_col()
            .flex_1()
            .overflow_hidden()
            .justify_end()
            .pb_4()
            .child(
                // 이미지 1의 DM 시작 환영 섹션 (대형 아바타 + Ann#4873)
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .py_6()
                    .gap_2()
                    .child(
                        // 대형 80x80 원형 아바타
                        div()
                            .relative()
                            .size(px(80.0))
                            .rounded_full()
                            .bg(rgb(0x2ecc71))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_2xl()
                            .font_weight(FontWeight::BOLD)
                            .text_color(TEXT_PRIMARY)
                            .child("A")
                            .child(
                                div()
                                    .absolute()
                                    .bottom_1()
                                    .right_1()
                                    .size(px(20.0))
                                    .rounded_full()
                                    .bg(STATUS_ONLINE)
                                    .border_3()
                                    .border_color(BG_PRIMARY),
                            ),
                    )
                    .child(
                        div()
                            .text_2xl()
                            .font_weight(FontWeight::BOLD)
                            .text_color(TEXT_PRIMARY)
                            .child(format!("{}#{}", username, discriminator)),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(TEXT_MUTED)
                            .child(format!("Say hi to {}. Your DM starts here.", username)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .pt_2()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1p5()
                                    .child(
                                        div()
                                            .size(px(16.0))
                                            .rounded_full()
                                            .bg(STATUS_ONLINE)
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_xs()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(TEXT_PRIMARY)
                                            .child("FR"),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(TEXT_MUTED)
                                            .child(mutual),
                                    ),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .bg(BG_SECONDARY)
                                    .hover(|s| s.bg(BG_CHANNEL_HOVER))
                                    .cursor(CursorStyle::PointingHand)
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(TEXT_PRIMARY)
                                    .child("Remove friend"),
                            ),
                    ),
            )
            .children(message_rows)
            .into_any_element()
    }

    /// 하단 메시지 입력 바 렌더러 (플레이스홀더 Message @Ann, 실제 키보드 타이핑 완벽 지원)
    fn render_input_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let active_dm = self
            .state
            .dm_channels
            .iter()
            .find(|d| Some(&d.id) == self.state.active_dm_id.as_ref())
            .or_else(|| self.state.dm_channels.last());

        let recipient_name = active_dm
            .map(|d| d.recipient.display_name.clone())
            .unwrap_or_else(|| "Ann".to_string());

        let placeholder = format!("Message @{}", recipient_name);
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
                            .cursor(CursorStyle::PointingHand)
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
                            .cursor(CursorStyle::PointingHand)
                            .child("GIF"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(TEXT_MUTED)
                            .hover(|s| s.text_color(TEXT_PRIMARY))
                            .cursor(CursorStyle::PointingHand)
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
            .relative()
            .bg(BG_PRIMARY)
            .text_color(TEXT_BODY)
            .child(self.render_header())
            .child(self.render_message_list())
            .child(self.render_input_bar(cx))
            .child(self.render_pip_video_stream_tile())
    }
}

#[cfg(test)]
mod tests {
    use crate::state::{append_message, get_client_state};

    #[test]
    fn test_chat_view_initial_state() {
        let state = get_client_state();
        assert!(!state.dm_channels.is_empty());
        assert!(state.active_dm_id.is_some());
    }

    #[test]
    fn test_chat_view_message_appending() {
        let initial_len = get_client_state().messages.len();
        append_message("Unit test chat message verification".to_string());
        let after_len = get_client_state().messages.len();
        assert_eq!(after_len, initial_len + 1);
    }
}
