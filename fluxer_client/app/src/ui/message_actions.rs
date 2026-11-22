//! The action bar shown over a hovered message (`MessageActionBar.tsx`):
//! react, reply, edit (own), delete (own or Manage Messages, confirmed),
//! copy text, copy id. Buttons emit [`MessageAction`]s through a callback;
//! the copy actions are done by [`copy_to_clipboard`]. React opens
//! [`quick_reactions`] in place of the bar (there is no emoji picker yet).

use std::rc::Rc;

use fluxer_client_core::state::{ClientState, Message, MessageState, PartialEmoji, Snowflake, bits};
use crate::icons::AppIcon;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::{AnyElement, App, ClipboardItem, ElementId, IntoElement, Pixels, Point, SharedString, Window, div, prelude::*, px};

use crate::theme::Theme;
use crate::ui::l10n;

/// What a button of the action bar asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageAction {
    /// Open the reaction choices ([`quick_reactions`]).
    React,
    /// Reply to the message (composer goes to `Replying`).
    Reply,
    /// Edit the message (composer goes to `Editing`); own messages only.
    Edit,
    /// Retry sending a failed local message (`MessageStates.FAILED`).
    Retry,
    /// Delete after the confirm dialog ([`confirm_delete`]).
    Delete,
    /// Shift+click on delete: delete without confirming (TS behavior).
    DeleteNow,
    CopyText,
    CopyId,
    /// Toggle SUPPRESS_EMBEDS (`MessageCommands.toggleSuppressEmbeds`).
    SuppressEmbeds,
}

/// The message the bar acts on.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionTarget {
    pub channel_id: Snowflake,
    pub message_id: Snowflake,
    /// Authored by the current user: edit is offered.
    pub own: bool,
    /// Own message, or Manage Messages in the channel.
    pub can_delete: bool,
    /// Sent (not pending/failed): react/reply/edit need a server id.
    pub sent: bool,
    /// Failed local send (`MessageState::Failed`): retry is offered when
    /// retryable in place (plain text or reply; failed file uploads restore
    /// their files to the attachment tray instead).
    pub can_retry: bool,
    pub content: SharedString,
    /// Author, or Manage Messages outside DMs (`canSuppressEmbeds`), on a
    /// message with embeds or with them already suppressed.
    pub can_suppress_embeds: bool,
    /// SUPPRESS_EMBEDS is set: the button offers to show them again.
    pub embeds_suppressed: bool,
}

impl ActionTarget {
    pub fn of(state: &ClientState, message: &Message) -> Self {
        let own = state.current_user_id() == Some(message.author.id);
        let manage = state.channel_permissions(message.channel_id) & bits::MANAGE_MESSAGES != 0;
        let in_dm = state.channels.get(&message.channel_id).is_none_or(|c| c.guild_id.is_none());
        let embeds_suppressed = message.suppresses_embeds();
        let has_embeds = embeds_suppressed || !message.embeds.is_empty();
        let can_retry = message.state == MessageState::Failed
            && message.nonce.is_some()
            && message.attachments.is_empty()
            && state.can_send_messages(message.channel_id);
        Self {
            channel_id: message.channel_id,
            message_id: message.id,
            own,
            can_delete: own || manage,
            sent: message.state == MessageState::Sent,
            can_retry,
            content: message.content.clone().into(),
            can_suppress_embeds: has_embeds && (own || (manage && !in_dm)),
            embeds_suppressed,
        }
    }
}

/// The buttons offered for `target`, in bar order.
pub fn available_actions(target: &ActionTarget) -> Vec<MessageAction> {
    let mut actions = Vec::new();
    if target.sent {
        actions.extend([MessageAction::React, MessageAction::Reply]);
        if target.own {
            actions.push(MessageAction::Edit);
        }
    } else if target.can_retry {
        actions.push(MessageAction::Retry);
    }
    if target.sent && target.can_suppress_embeds {
        actions.push(MessageAction::SuppressEmbeds);
    }
    if !target.content.is_empty() {
        actions.push(MessageAction::CopyText);
    }
    if target.sent {
        actions.push(MessageAction::CopyId);
    }
    if target.can_delete || !target.sent {
        actions.push(MessageAction::Delete);
    }
    actions
}

/// Copies the text or the id for the copy actions; returns false for the
/// other actions (the caller handles those).
pub fn copy_to_clipboard(action: MessageAction, target: &ActionTarget, cx: &mut App) -> bool {
    let text = match action {
        MessageAction::CopyText => target.content.to_string(),
        MessageAction::CopyId => target.message_id.to_string(),
        _ => return false,
    };
    cx.write_to_clipboard(ClipboardItem::new_string(text));
    true
}

/// The floating bar of icon buttons. Place it with [`with_hover_actions`].
pub fn action_bar(
    target: &ActionTarget,
    theme: &Theme,
    on_action: impl Fn(MessageAction, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let on_action = Rc::new(on_action);
    let key = target.message_id.0;
    let buttons = available_actions(target).into_iter().map(|action| {
        let on_action = on_action.clone();
        let id = SharedString::from(format!("msg-action-{key}-{action:?}"));
        let button = Button::new(id).ghost().compact().tooltip(tooltip(action, target));
        let button = button.icon(icon(action, target));
        button
            .on_click(move |event, window, cx| {
                let action = if action == MessageAction::Delete && event.modifiers().shift {
                    MessageAction::DeleteNow
                } else {
                    action
                };
                on_action(action, window, cx);
            })
            .into_any_element()
    });
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(2.))
        .p(px(2.))
        .rounded(px(6.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .shadow_sm()
        .children(buttons)
        .into_any_element()
}

fn tooltip(action: MessageAction, target: &ActionTarget) -> String {
    match action {
        MessageAction::SuppressEmbeds if target.embeds_suppressed => l10n::tr("Unsuppress embeds"),
        MessageAction::SuppressEmbeds => l10n::tr("Suppress embeds"),
        MessageAction::React => l10n::tr("Add reaction"),
        MessageAction::Reply => l10n::tr("Reply"),
        MessageAction::Edit => l10n::tr("Edit"),
        MessageAction::Retry => l10n::tr("Try again"),
        MessageAction::Delete | MessageAction::DeleteNow => l10n::tr("Delete"),
        MessageAction::CopyText => l10n::tr("Copy text"),
        MessageAction::CopyId => l10n::tr("Copy message ID"),
    }
}

/// The icon of each button (`icons::AppAssets` serves them).
fn icon(action: MessageAction, target: &ActionTarget) -> AppIcon {
    match action {
        MessageAction::SuppressEmbeds if target.embeds_suppressed => AppIcon::Eye,
        MessageAction::SuppressEmbeds => AppIcon::EyeSlash,
        MessageAction::React => AppIcon::Smiley,
        MessageAction::Reply => AppIcon::ArrowBendUpLeft,
        MessageAction::Edit => AppIcon::PencilSimple,
        MessageAction::Retry => AppIcon::ArrowClockwise,
        MessageAction::CopyText => AppIcon::Copy,
        MessageAction::CopyId => AppIcon::Hash,
        MessageAction::Delete | MessageAction::DeleteNow => AppIcon::Trash,
    }
}

/// Unicode emoji offered by [`quick_reactions`].
pub const QUICK_REACTIONS: [&str; 6] = ["👍", "❤️", "😂", "😮", "😢", "🎉"];

/// A row of emoji buttons for reacting to message `message_key`;
/// `on_pick` gets the chosen emoji (toggle it with
/// `Backend::toggle_reaction`).
pub fn quick_reactions(
    message_key: u64,
    theme: &Theme,
    on_pick: impl Fn(PartialEmoji, &mut Window, &mut App) + 'static,
    on_more: impl Fn(Point<Pixels>, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let on_pick = Rc::new(on_pick);
    let buttons = QUICK_REACTIONS.iter().map(|name| {
        let on_pick = on_pick.clone();
        let emoji = PartialEmoji { id: None, name: (*name).to_owned(), animated: false };
        div()
            .id(SharedString::from(format!("quick-reaction-{message_key}-{name}")))
            .debug_selector(|| format!("quick-reaction-{name}"))
            .px_1()
            .rounded(px(4.))
            .cursor_pointer()
            .hover(|s| s.bg(theme.muted))
            .child(*name)
            .on_click(move |_, window, cx| on_pick(emoji.clone(), window, cx))
            .into_any_element()
    });
    let more = div()
        .id(SharedString::from(format!("quick-reaction-more-{message_key}")))
        .debug_selector(|| format!("quick-reaction-more-{message_key}"))
        .px_1()
        .rounded(px(4.))
        .cursor_pointer()
        .text_color(theme.muted_foreground)
        .hover(|s| s.bg(theme.muted))
        .child("+")
        .on_click(move |ev, window, cx| on_more(ev.position(), window, cx));
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(2.))
        .p(px(2.))
        .rounded(px(6.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .shadow_sm()
        .children(buttons)
        .child(more)
        .into_any_element()
}

/// Wraps a message row: `bar` (the action bar of the hovered row, or open
/// [`quick_reactions`]) goes at its top right, and `on_pointer` runs on
/// every mouse move over the row. The owner keeps the hovered row (set
/// from `on_pointer`, cleared when the pointer leaves the message list),
/// so only that row builds its bar.
///
/// Mouse moves rather than `on_hover`: enter/leave pairs of rows in a
/// virtual list are not reliable (a row leaving the list never reports
/// its leave), while the last row moved over is always the hovered one.
pub fn with_hover_actions(
    id: impl Into<ElementId>,
    row: impl IntoElement,
    bar: Option<AnyElement>,
    on_pointer: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    let id: ElementId = id.into();
    let row_selector = format!("{id}");
    let bar_selector = format!("{id}-bar");
    let on_pointer = Rc::new(on_pointer);
    let on_bar = on_pointer.clone();
    div()
        .id(id)
        .debug_selector(move || row_selector)
        .relative()
        .on_mouse_move(move |_, window, cx| on_pointer(window, cx))
        .child(row)
        .children(bar.map(|bar| {
            // The bar pokes above the row, over the previous one: moves over
            // it keep this row hovered and stop there, so neither the row
            // behind nor the list's leave check takes the bar away.
            div()
                .absolute()
                .top(px(-12.))
                .right(px(16.))
                .debug_selector(move || bar_selector)
                .on_mouse_move(move |_, window, cx| {
                    on_bar(window, cx);
                    cx.stop_propagation();
                })
                .child(bar)
        }))
        .into_any_element()
}

/// Opens the "Delete Message" confirm dialog; `on_confirm` runs when the
/// user presses Delete (or Enter). Needs the window's `Root`.
pub fn confirm_delete(
    preview: SharedString,
    window: &mut Window,
    cx: &mut App,
    on_confirm: impl Fn(&mut Window, &mut App) + 'static,
) {
    let on_confirm = Rc::new(on_confirm);
    window.open_alert_dialog(cx, move |alert, _, _| {
        let on_confirm = on_confirm.clone();
        alert
            .confirm()
            .title(l10n::tr("Delete message"))
            .description(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(l10n::tr("Delete this message? Can't be undone."))
                    .when(!preview.is_empty(), |d| d.child(div().italic().line_clamp(3).child(preview.clone()))),
            )
            .ok_text(l10n::tr("Delete"))
            .ok_variant(ButtonVariant::Danger)
            .on_ok(move |_, window, cx| {
                on_confirm(window, cx);
                true
            })
    });
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use gpui_kit::{Render, TestAppContext, VisualTestContext};

    use super::*;

    fn target(own: bool, can_delete: bool, sent: bool, content: &str) -> ActionTarget {
        ActionTarget {
            channel_id: Snowflake(11),
            message_id: Snowflake(42),
            own,
            can_delete,
            sent,
            can_retry: false,
            content: content.to_owned().into(),
            can_suppress_embeds: false,
            embeds_suppressed: false,
        }
    }

    #[test]
    fn actions_depend_on_ownership() {
        use MessageAction::*;
        assert_eq!(available_actions(&target(true, true, true, "hi")), vec![React, Reply, Edit, CopyText, CopyId, Delete]);
        assert_eq!(available_actions(&target(false, false, true, "hi")), vec![React, Reply, CopyText, CopyId]);
        assert_eq!(available_actions(&target(false, true, true, "")), vec![React, Reply, CopyId, Delete]);
        // A failed local message can be retried (when retryable), copied or discarded.
        assert_eq!(available_actions(&target(true, true, false, "x")), vec![CopyText, Delete]);
        let retryable = ActionTarget { can_retry: true, ..target(true, true, false, "x") };
        assert_eq!(available_actions(&retryable), vec![Retry, CopyText, Delete]);
        // Suppress Embeds sits before the copy actions, only when sent.
        let with_embeds = ActionTarget { can_suppress_embeds: true, ..target(true, true, true, "hi") };
        assert_eq!(
            available_actions(&with_embeds),
            vec![React, Reply, Edit, SuppressEmbeds, CopyText, CopyId, Delete]
        );
        let pending = ActionTarget { can_suppress_embeds: true, ..target(true, true, false, "hi") };
        assert!(!available_actions(&pending).contains(&SuppressEmbeds));
        let suppressed = ActionTarget { embeds_suppressed: true, ..with_embeds };
        assert_eq!(tooltip(SuppressEmbeds, &suppressed), "Unsuppress embeds");
    }

    struct Empty;

    impl Render for Empty {
        fn render(&mut self, _: &mut Window, _: &mut gpui_kit::Context<Self>) -> impl IntoElement {
            div().size_full()
        }
    }

    fn open(cx: &mut TestAppContext) -> &mut VisualTestContext {
        cx.update(gpui_kit::init);
        let (_, vcx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|_| Empty);
            gpui_kit::base::Root::new(view, window, cx)
        });
        vcx.run_until_parked();
        vcx
    }

    fn open_confirm(cx: &mut VisualTestContext, confirmed: Rc<Cell<u32>>) {
        cx.update(|window, cx| {
            confirm_delete("hello".into(), window, cx, move |_, _| confirmed.set(confirmed.get() + 1));
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
    }

    #[gpui_kit::test]
    fn delete_confirm_runs_only_on_ok(cx: &mut TestAppContext) {
        let cx = open(cx);
        let confirmed = Rc::new(Cell::new(0));
        open_confirm(cx, confirmed.clone());
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert_eq!(confirmed.get(), 0, "Escape cancels");
        assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));

        open_confirm(cx, confirmed.clone());
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(confirmed.get(), 1, "Enter confirms the delete");
        assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
    }

    #[gpui_kit::test]
    fn copy_actions_write_the_clipboard(cx: &mut TestAppContext) {
        let t = target(true, true, true, "copy me");
        cx.update(|cx| {
            assert!(copy_to_clipboard(MessageAction::CopyText, &t, cx));
        });
        assert_eq!(cx.read_from_clipboard().and_then(|c| c.text()).as_deref(), Some("copy me"));
        cx.update(|cx| {
            assert!(copy_to_clipboard(MessageAction::CopyId, &t, cx));
            assert!(!copy_to_clipboard(MessageAction::Reply, &t, cx));
        });
        assert_eq!(cx.read_from_clipboard().and_then(|c| c.text()).as_deref(), Some("42"));
    }
}
