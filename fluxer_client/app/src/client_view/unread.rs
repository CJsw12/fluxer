//! Jump to the first sent message after a channel's read cursor.

use fluxer_client_core::state::{MessageState, Snowflake};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Disableable as _, Sizable as _};
use gpui_kit::{Context, div, prelude::*};

use super::ClientView;
use crate::theme::Theme;
use crate::ui::l10n;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JumpTarget {
    Message(Snowflake),
    FirstUnreadAfter(Snowflake),
}

impl JumpTarget {
    pub(super) fn missing_label(self) -> &'static str {
        match self {
            Self::Message(_) => "Message not found",
            Self::FirstUnreadAfter(_) => "No unread messages",
        }
    }
}

impl ClientView {
    pub(super) fn reveal_jump(&mut self, channel: Snowflake, target: JumpTarget, cx: &mut Context<Self>) -> bool {
        let found = self.rows.borrow().iter().enumerate().find_map(|(index, row)| {
            let matches = match target {
                JumpTarget::Message(id) => row.id == id,
                JumpTarget::FirstUnreadAfter(read) => row.id > read && row.state == MessageState::Sent,
            };
            matches.then_some((index, row.id))
        });
        let Some((index, id)) = found else { return false };
        self.cancel_ack();
        self.suppress_jump_scroll = true;
        self.scroller
            .update(cx, |scroller, cx| scroller.scroll_to_item(index, cx));
        self.highlighted_message = Some((channel, id));
        true
    }

    pub(crate) fn jump_to_unread(&mut self, cx: &mut Context<Self>) {
        if self.loading_history {
            return;
        }
        let (Some(channel), Some(read)) = (self.selected_channel, self.unread_anchor) else {
            return;
        };
        let target = JumpTarget::FirstUnreadAfter(read);
        let covers_cursor = self.backend.state().messages.window(channel).is_some_and(|window| {
            window.ready
                && (!window.has_more_before || window.messages.first().is_some_and(|message| message.id <= read))
        });
        self.error = None;
        self.cancel_ack();
        if !covers_cursor || !self.reveal_jump(channel, target, cx) {
            self.pending_jump = Some((channel, target));
            self.highlighted_message = None;
            self.loading_history = true;
            self.backend.load_around(channel, read);
        }
        cx.notify();
    }

    pub(super) fn render_unread_bar(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("unread-bar")
            .debug_selector(|| "unread-bar".to_owned())
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .px_4()
            .py_1()
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.background_secondary)
            .child(
                div().debug_selector(|| "jump-to-unread-btn".to_owned()).child(
                    Button::new("jump-to-unread-btn")
                        .small()
                        .ghost()
                        .label(l10n::tr("Jump to first unread message"))
                        .disabled(self.loading_history)
                        .on_click(cx.listener(|view, _, _, cx| view.jump_to_unread(cx))),
                ),
            )
            .child(
                div().debug_selector(|| "mark-unread-bar-read-btn".to_owned()).child(
                    Button::new("mark-unread-bar-read-btn")
                        .small()
                        .ghost()
                        .label(l10n::tr("Mark as read"))
                        .disabled(self.loading_history)
                        .on_click(cx.listener(|view, _, _, cx| view.mark_channel_read(cx))),
                ),
            )
    }
}
