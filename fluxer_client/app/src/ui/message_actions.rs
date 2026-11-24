//! The action bar shown over a hovered message (`MessageActionBar.tsx`):
//! quick emoji, react, edit (own) or reply, forward, create thread, and More,
//! which opens the message menu (`MessageContextMenu.tsx`: copy, delete, id). Buttons emit [`MessageAction`]s through a callback;
//! the copy actions are done by [`copy_to_clipboard`]. React opens
//! [`quick_reactions`] in place of the bar (there is no emoji picker yet).

use std::rc::Rc;

use fluxer_client_core::state::{ClientState, Message, MessageState, PartialEmoji, Snowflake, bits};
use crate::icons::AppIcon;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::ButtonVariant;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, App, BoxShadow, ClipboardItem, ElementId, Hsla, IntoElement, Pixels, Point, Role, SharedString, Window, div,
    prelude::*, px, rgb, rgba,
};

use crate::parity_probe::FlxExt as _;
use crate::theme::Theme;
use crate::ui::channel_popouts::track_trigger;
use crate::ui::l10n;

/// What a button of the action bar or an item of its "More" menu asks for.
/// Every button/item has its own variant, so each gets its own handler.
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
    /// Open the "More" menu ([`menu_groups`]) of the hovered message.
    More,
    Forward,
    CreateThread,
    Translate,
    Pin,
    Bookmark,
    MarkUnread,
    CopyLink,
    Speak,
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
    /// Manage Messages in the channel (`canPinMessage`).
    pub can_pin: bool,
    /// Guild channel (threads are not offered in DMs).
    pub can_create_thread: bool,
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
            can_pin: manage,
            can_create_thread: !in_dm,
        }
    }
}

/// The inline hover-bar buttons for `target`, in bar order (after the quick
/// emoji): Add Reaction, Edit (own) or Reply, Forward, Create Thread, More.
/// A failed local message gets Retry and Delete instead.
pub fn bar_actions(target: &ActionTarget) -> Vec<MessageAction> {
    let mut actions = Vec::new();
    if target.sent {
        actions.push(MessageAction::React);
        actions.push(if target.own { MessageAction::Edit } else { MessageAction::Reply });
        actions.push(MessageAction::Forward);
        if target.can_create_thread {
            actions.push(MessageAction::CreateThread);
        }
        actions.push(MessageAction::More);
    } else {
        if target.can_retry {
            actions.push(MessageAction::Retry);
        }
        actions.push(MessageAction::Delete);
    }
    actions
}

/// One row of the "More" menu: the quick-reaction header, or an action item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuItem {
    QuickReactions,
    Action(MessageAction),
}

/// The "More" menu of a sent message (`MessageContextMenu.tsx`): groups in
/// order, separators between them. Empty for unsent messages.
pub fn menu_groups(target: &ActionTarget) -> Vec<Vec<MenuItem>> {
    use MessageAction as A;
    if !target.sent {
        return Vec::new();
    }
    let has_text = !target.content.is_empty();
    // "Add reaction" is a submenu trigger under the quick-reaction row; Translate has its own group.
    let reactions = vec![MenuItem::QuickReactions, MenuItem::Action(A::React)];
    let translate: Vec<MessageAction> = if has_text { vec![A::Translate] } else { Vec::new() };
    let mut interact = Vec::new();
    if target.own {
        interact.push(A::Edit);
    }
    interact.extend([A::Reply, A::Forward]);
    if target.can_create_thread {
        interact.push(A::CreateThread);
    }
    let mut utility = Vec::new();
    if has_text {
        utility.push(A::CopyText);
    }
    if target.can_pin {
        utility.push(A::Pin);
    }
    utility.extend([A::Bookmark, A::MarkUnread, A::CopyLink]);
    if has_text {
        utility.push(A::Speak);
    }
    if target.can_suppress_embeds {
        utility.push(A::SuppressEmbeds);
    }
    let danger: Vec<MessageAction> = if target.can_delete { vec![A::Delete] } else { Vec::new() };
    let to_items = |v: Vec<MessageAction>| v.into_iter().map(MenuItem::Action).collect::<Vec<_>>();
    [reactions, to_items(translate), to_items(interact), to_items(utility), to_items(danger), vec![MenuItem::Action(A::CopyId)]]
        .into_iter()
        .filter(|g| !g.is_empty())
        .collect()
}

/// Every action offered for `target`, bar first, then the menu.
#[cfg(test)]
pub fn available_actions(target: &ActionTarget) -> Vec<MessageAction> {
    let mut actions = bar_actions(target);
    for item in menu_groups(target).into_iter().flatten() {
        if let MenuItem::Action(a) = item
            && !actions.contains(&a)
        {
            actions.push(a);
        }
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

/// Emoji of the three quick-reaction buttons of the bar (`useQuickReactionEmojis`
/// without history: :grinning:, :smiley:, :thumbsup:).
pub const BAR_QUICK_EMOJI: [&str; 3] = ["\u{1F600}", "\u{1F603}", "\u{1F44D}"];
/// Emoji of the quick-reaction row of the menu (the bar's three plus :no_mouth:).
pub const MENU_QUICK_EMOJI: [&str; 4] = ["\u{1F600}", "\u{1F603}", "\u{1F44D}", "\u{1F636}"];

/// Trigger id under which the hovered row's More button records its bounds
/// (only one bar exists at a time), so the menu anchors to it.
pub const MORE_TRIGGER: &str = "message-more-btn";

/// The floating bar of buttons (`MessageActionBar.tsx`): the quick-reaction
/// emoji, then one 28px slot per action with a 20px icon. Place it with
/// [`with_hover_actions`].
pub fn action_bar(
    target: &ActionTarget,
    theme: &Theme,
    on_react: impl Fn(PartialEmoji, &mut Window, &mut App) + 'static,
    on_action: impl Fn(MessageAction, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let on_action = Rc::new(on_action);
    let on_react = Rc::new(on_react);
    let key = target.message_id.0;
    let dark = theme.background.l < 0.5;
    let bar_bg: Hsla = if dark { rgb(0x2a2730).into() } else { theme.background };
    let bar_border: Hsla = if dark { rgba(0xc9c5d326).into() } else { theme.border };
    let hover_bg = theme.hover_bg;
    let icon_color = theme.muted_foreground;
    let quick_names: &[&str] = if target.sent { &BAR_QUICK_EMOJI } else { &[] };
    let quick = quick_names.iter().map(|name| {
        let on_react = on_react.clone();
        let emoji = PartialEmoji { id: None, name: (*name).to_owned(), animated: false };
        div()
            .id(SharedString::from(format!("msg-quick-{key}-{name}")))
            .role(Role::Button)
            .aria_label(format!("React with {name}"))
            .size(px(28.))
            .rounded(px(6.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(move |s| s.bg(hover_bg))
            .flx("channel.message-action-bar.button.click")
            .on_click(move |_, window, cx| on_react(emoji.clone(), window, cx))
            .child(
                div()
                    .size(px(20.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(17.))
                    .line_height(px(20.))
                    .flx("channel.message-action-bar.emoji-image")
                    .child(*name),
            )
            .into_any_element()
    });
    let buttons = bar_actions(target).into_iter().map(|action| {
        let on_action = on_action.clone();
        let id = SharedString::from(format!("msg-action-{key}-{action:?}"));
        let label = tooltip(action, target);
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(label)
            .size(px(28.))
            .rounded(px(6.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(move |s| s.bg(hover_bg))
            .flx("channel.message-action-bar.message-action-bar-button.button.click")
            .when(action == MessageAction::More, |d| d.child(track_trigger(MORE_TRIGGER)))
            .on_click(move |event, window, cx| {
                let action = if action == MessageAction::Delete && event.modifiers().shift {
                    MessageAction::DeleteNow
                } else {
                    action
                };
                on_action(action, window, cx);
            })
            .child(
                div()
                    .flx("channel.message-action-bar.message-action-bar-button.action-bar-icon")
                    .when_some(bar_icon_flx(action), |d, id| d.flx(id))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Icon::new(icon(action, target)).with_size(px(20.)).text_color(icon_color)),
            )
            .into_any_element()
    });
    div()
        .h(px(34.))
        .flex()
        .flex_row()
        .items_center()
        .p(px(2.))
        .rounded(px(8.))
        .border_1()
        .border_color(bar_border)
        .bg(bar_bg)
        .shadow(vec![BoxShadow {
            color: rgba(0x0000002e).into(),
            offset: gpui_kit::point(px(0.), px(6.)),
            blur_radius: px(14.),
            spread_radius: px(0.),
            inset: false,
        }])
        .flx("channel.message-action-bar.message-action-bar-core.action-bar")
        .children(quick)
        .children(buttons)
        .into_any_element()
}

/// Tooltip / accessible label of an action.
pub fn tooltip(action: MessageAction, target: &ActionTarget) -> String {
    match action {
        MessageAction::SuppressEmbeds if target.embeds_suppressed => l10n::tr("Unsuppress embeds"),
        MessageAction::SuppressEmbeds => l10n::tr("Suppress embeds"),
        MessageAction::React => l10n::tr("Add reaction"),
        MessageAction::Reply => l10n::tr("Reply"),
        MessageAction::Edit => l10n::tr("Edit message"),
        MessageAction::Retry => l10n::tr("Try again"),
        MessageAction::Delete | MessageAction::DeleteNow => l10n::tr("Delete message"),
        MessageAction::CopyText => l10n::tr("Copy text"),
        MessageAction::CopyId => l10n::tr("Copy message ID"),
        MessageAction::More => l10n::tr("More"),
        MessageAction::Forward => l10n::tr("Forward"),
        MessageAction::CreateThread => l10n::tr("Create thread"),
        MessageAction::Translate => l10n::tr("Translate"),
        MessageAction::Pin => l10n::tr("Pin message"),
        MessageAction::Bookmark => l10n::tr("Bookmark message"),
        MessageAction::MarkUnread => l10n::tr("Mark as unread"),
        MessageAction::CopyLink => l10n::tr("Copy message link"),
        MessageAction::Speak => l10n::tr("Speak message"),
    }
}

/// The icon of each button/item (`icons::AppAssets` serves them); weights
/// follow `ContextMenuIcons.tsx`.
pub fn icon(action: MessageAction, target: &ActionTarget) -> AppIcon {
    match action {
        MessageAction::SuppressEmbeds if target.embeds_suppressed => AppIcon::Eye,
        MessageAction::SuppressEmbeds => AppIcon::EyeSlash,
        MessageAction::React => AppIcon::Smiley,
        MessageAction::Reply => AppIcon::ArrowBendUpLeft,
        MessageAction::Edit => AppIcon::PencilSimple,
        MessageAction::Retry => AppIcon::ArrowClockwise,
        MessageAction::CopyText => AppIcon::Copy,
        MessageAction::CopyId => AppIcon::Snowflake,
        MessageAction::Delete | MessageAction::DeleteNow => AppIcon::Trash,
        MessageAction::More => AppIcon::DotsThreeBold,
        MessageAction::Forward => AppIcon::ArrowBendUpRight,
        MessageAction::CreateThread => AppIcon::Chats,
        MessageAction::Translate => AppIcon::Translate,
        MessageAction::Pin => AppIcon::PushPin,
        MessageAction::Bookmark => AppIcon::BookmarkSimple,
        MessageAction::MarkUnread => AppIcon::EnvelopeOpen,
        MessageAction::CopyLink => AppIcon::LinkBold,
        MessageAction::Speak => AppIcon::SpeakerHigh,
    }
}

/// Official flx id carried by the 20px icon of a bar button (the bar reuses the
/// `ContextMenuIcons` components).
fn bar_icon_flx(action: MessageAction) -> Option<&'static str> {
    match action {
        MessageAction::React => Some("ui.action-menu.context-menu-icons.add-reaction-icon.smiley-icon"),
        MessageAction::Edit => Some("ui.action-menu.context-menu-icons.edit-message-icon.pencil-simple-icon"),
        MessageAction::Reply => Some("ui.action-menu.context-menu-icons.reply-icon.arrow-bend-up-left-icon"),
        MessageAction::Forward => Some("ui.action-menu.context-menu-icons.forward-icon.arrow-bend-up-right-icon"),
        MessageAction::CreateThread => Some("ui.action-menu.context-menu-icons.create-thread-icon.chats-icon"),
        MessageAction::More => Some("ui.action-menu.context-menu-icons.more-icon.dots-three-icon"),
        MessageAction::Delete => Some("ui.action-menu.context-menu-icons.delete-icon.trash-icon"),
        _ => None,
    }
}

/// Official flx id of the 18px icon of a menu item (`ContextMenuIcons`).
pub fn menu_icon_flx(action: MessageAction) -> &'static str {
    match action {
        MessageAction::Translate => "ui.action-menu.context-menu-icons.translate-icon.phosphor-translate-icon",
        MessageAction::Edit => "ui.action-menu.context-menu-icons.edit-message-icon.pencil-simple-icon",
        MessageAction::Reply => "ui.action-menu.context-menu-icons.reply-icon.arrow-bend-up-left-icon",
        MessageAction::Forward => "ui.action-menu.context-menu-icons.forward-icon.arrow-bend-up-right-icon",
        MessageAction::CreateThread => "ui.action-menu.context-menu-icons.create-thread-icon.chats-icon",
        MessageAction::CopyText => "ui.action-menu.context-menu-icons.copy-message-text-icon.phosphor-copy-icon",
        MessageAction::Pin => "ui.action-menu.context-menu-icons.pin-icon.push-pin-icon",
        MessageAction::Bookmark => "ui.action-menu.context-menu-icons.bookmark-icon.bookmark-simple-icon",
        MessageAction::MarkUnread => "ui.action-menu.context-menu-icons.mark-as-unread-icon.envelope-open-icon",
        MessageAction::CopyLink => "ui.action-menu.context-menu-icons.copy-link-icon.link-icon",
        MessageAction::Speak => "ui.action-menu.context-menu-icons.speak-message-icon.speaker-high-icon",
        MessageAction::SuppressEmbeds => "ui.action-menu.context-menu-icons.suppress-embeds-icon.eye-slash-icon",
        MessageAction::Delete | MessageAction::DeleteNow => "ui.action-menu.context-menu-icons.delete-icon.trash-icon",
        MessageAction::CopyId => "ui.action-menu.context-menu-icons.copy-id-icon.snowflake-icon",
        MessageAction::React | MessageAction::Retry | MessageAction::More => "ui.action-menu.context-menu-icons.icon",
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
    let dark = theme.background.l < 0.5;
    let bar_bg: Hsla = if dark { rgb(0x2a2730).into() } else { theme.background };
    let bar_border: Hsla = if dark { rgba(0xc9c5d326).into() } else { theme.border };
    let buttons = QUICK_REACTIONS.iter().map(|name| {
        let on_pick = on_pick.clone();
        let emoji = PartialEmoji { id: None, name: (*name).to_owned(), animated: false };
        div()
            .id(SharedString::from(format!("quick-reaction-{message_key}-{name}")))
            .debug_selector(|| format!("quick-reaction-{name}"))
            .px_1()
            .rounded(px(4.))
            .cursor_pointer()
            .hover(|s| s.bg(theme.hover_bg))
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
        .hover(|s| s.bg(theme.hover_bg))
        .child("+")
        .on_click(move |ev, window, cx| on_more(ev.position(), window, cx));
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(2.))
        .p(px(3.))
        .rounded(px(8.))
        .border_1()
        .border_color(bar_border)
        .bg(bar_bg)
        .shadow_md()
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
                .top(px(-24.))
                .right(px(0.))
                .pl(px(32.))
                .pr(px(14.))
                .pb(px(6.))
                .debug_selector(move || bar_selector)
                .flx("channel.message-action-bar.message-action-bar-core.action-bar-container")
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
            can_pin: false,
            can_create_thread: true,
        }
    }

    fn menu_actions(t: &ActionTarget) -> Vec<Vec<MenuItem>> {
        menu_groups(t)
    }

    #[test]
    fn bar_holds_only_inline_buttons() {
        use MessageAction::*;
        assert_eq!(bar_actions(&target(true, true, true, "hi")), vec![React, Edit, Forward, CreateThread, More]);
        assert_eq!(bar_actions(&target(false, false, true, "hi")), vec![React, Reply, Forward, CreateThread, More]);
        let dm = ActionTarget { can_create_thread: false, ..target(false, false, true, "hi") };
        assert_eq!(bar_actions(&dm), vec![React, Reply, Forward, More]);
        // A failed local message can be retried (when retryable) or discarded.
        assert_eq!(bar_actions(&target(true, true, false, "x")), vec![Delete]);
        let retryable = ActionTarget { can_retry: true, ..target(true, true, false, "x") };
        assert_eq!(bar_actions(&retryable), vec![Retry, Delete]);
    }

    #[test]
    fn menu_follows_the_official_order() {
        use MenuItem::{Action, QuickReactions};
        use MessageAction::*;
        let own = menu_actions(&target(true, true, true, "hi"));
        assert_eq!(
            own,
            vec![
                vec![QuickReactions, Action(React)],
                vec![Action(Translate)],
                vec![Action(Edit), Action(Reply), Action(Forward), Action(CreateThread)],
                vec![Action(CopyText), Action(Bookmark), Action(MarkUnread), Action(CopyLink), Action(Speak)],
                vec![Action(Delete)],
                vec![Action(CopyId)],
            ]
        );
        // Others' messages: no Edit/Delete; pin and suppress embeds only with the right.
        let mut other = target(false, false, true, "hi");
        other.can_pin = true;
        other.can_suppress_embeds = true;
        let groups = menu_actions(&other);
        assert_eq!(groups[2], vec![Action(Reply), Action(Forward), Action(CreateThread)]);
        assert_eq!(
            groups[3],
            vec![Action(CopyText), Action(Pin), Action(Bookmark), Action(MarkUnread), Action(CopyLink), Action(Speak), Action(SuppressEmbeds)]
        );
        assert_eq!(groups.len(), 5, "no delete group");
        assert!(menu_actions(&target(true, true, false, "hi")).is_empty(), "unsent messages have no menu");
        // No text: no Translate, Copy Text or Speak.
        let empty = menu_actions(&target(false, true, true, ""));
        assert_eq!(empty[0], vec![QuickReactions, Action(React)]);
        assert!(!empty.concat().contains(&Action(Translate)));
        assert!(!empty.concat().contains(&Action(CopyText)));
        let suppressed = ActionTarget { embeds_suppressed: true, ..target(true, true, true, "hi") };
        assert_eq!(tooltip(SuppressEmbeds, &suppressed), "Unsuppress embeds");
    }

    #[test]
    fn available_actions_cover_bar_and_menu() {
        use MessageAction::*;
        let all = available_actions(&target(true, true, true, "hi"));
        for a in [React, Edit, Forward, CreateThread, More, Translate, CopyText, Bookmark, MarkUnread, CopyLink, Speak, Delete, CopyId] {
            assert!(all.contains(&a), "{a:?}");
        }
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
