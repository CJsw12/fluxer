//! Global keyboard shortcuts of the client layout (`InputKeybind.ts`
//! defaults, Windows/Linux keys; no user remapping).
//!
//! Every binding is registered for the `ClientView` context and again for
//! `ClientView > Composer > Input`, so it also works while the composer has
//! focus: the input's own bindings sit deeper in the context stack and
//! would otherwise win (Ctrl+Alt+Up/Down add cursors there on Windows).
//! Other inputs (the quick switcher's, a modal) do not get them, like the
//! web client's modal suppression; only Ctrl+K works in any input. Plain
//! Esc is the exception: inside the composer it reaches
//! `ClientView::on_escape` through the input's `Escape` action, which first
//! dismisses the composer's own affordances.

use gpui_kit::{Action, App, Context, Div, KeyBinding, NoAction, prelude::*};

use crate::client_view::ClientView;
use crate::nav::ChannelFilter;
use crate::ui::{
    access, account_switcher, channel_modal, create_thread_modal, forward_modal, guild_settings_modals,
    invite_modal, moderation_modal, quick_switcher, search_panel,
};

gpui_kit::actions!(
    fluxer,
    [
        ChannelPrev,
        ChannelNext,
        UnreadChannelPrev,
        UnreadChannelNext,
        MentionChannelPrev,
        MentionChannelNext,
        GuildPrev,
        GuildNext,
        HistoryBack,
        HistoryForward,
        ToggleLastGuild,
        OpenJoinGuild,
        OpenSearch,
        ToggleQuickSwitcher,
        ToggleEmojiPicker,
        OpenSettings,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        MarkChannelRead,
        MarkGuildRead,
        SwitcherPrev,
        SwitcherNext,
    ]
);

/// Key context of the client layout's root element.
pub const CONTEXT: &str = "ClientView";
const IN_INPUT: &str = "ClientView > Input";
const IN_COMPOSER: &str = "ClientView > Composer > Input";
const IN_CREATE_CHANNEL_INPUT: &str = "CreateChannelModal > Input";
const IN_EDIT_CHANNEL_INPUT: &str = "EditChannelModal > Input";
const IN_MODERATION_INPUT: &str = "MemberModerationModal > Input";
const IN_BANS_INPUT: &str = "GuildBansModal > Input";
const IN_ACCESS_INPUT: &str = "AccessEditor > Input";
const IN_FORWARD_INPUT: &str = "ForwardMessageModal > Input";
const IN_CREATE_THREAD_INPUT: &str = "CreateThreadModal > Input";
const IN_INVITE_INPUT: &str = "InviteMembersModal > Input";
const IN_GUILD_PROFILE_INPUT: &str = "GuildProfileModal > Input";
const IN_DELETE_GUILD_MESSAGES_INPUT: &str = "DeleteMyGuildMessagesModal > Input";

/// Binds `keys` in the layout and inside the composer, and disables them
/// inside the quick switcher and search panel (whose inputs would otherwise
/// fall back to the layout's binding).
fn both<A: Action + Clone>(out: &mut Vec<KeyBinding>, keys: &'static str, action: A) {
    out.push(KeyBinding::new(keys, action.clone(), Some(CONTEXT)));
    out.push(KeyBinding::new(keys, action, Some(IN_COMPOSER)));
    out.push(KeyBinding::new(keys, NoAction, Some(quick_switcher::CONTEXT)));
    out.push(KeyBinding::new(keys, NoAction, Some(search_panel::CONTEXT)));
    out.push(KeyBinding::new(keys, NoAction, Some(account_switcher::CONTEXT)));
    out.push(KeyBinding::new(keys, NoAction, Some(channel_modal::CREATE_CONTEXT)));
    out.push(KeyBinding::new(keys, NoAction, Some(channel_modal::EDIT_CONTEXT)));
    out.push(KeyBinding::new(keys, NoAction, Some(moderation_modal::ACTION_CONTEXT)));
    out.push(KeyBinding::new(keys, NoAction, Some(moderation_modal::BANS_CONTEXT)));
    out.push(KeyBinding::new(keys, NoAction, Some(access::CONTEXT)));
    out.push(KeyBinding::new(keys, NoAction, Some(forward_modal::CONTEXT)));
    out.push(KeyBinding::new(keys, NoAction, Some(create_thread_modal::CONTEXT)));
    out.push(KeyBinding::new(keys, NoAction, Some(invite_modal::CONTEXT)));
    out.push(KeyBinding::new(keys, NoAction, Some(guild_settings_modals::NOTIFICATIONS_CONTEXT)));
    out.push(KeyBinding::new(keys, NoAction, Some(guild_settings_modals::PRIVACY_CONTEXT)));
    out.push(KeyBinding::new(keys, NoAction, Some(guild_settings_modals::PROFILE_CONTEXT)));
    out.push(KeyBinding::new(keys, NoAction, Some(guild_settings_modals::DELETE_MESSAGES_CONTEXT)));
    out.push(KeyBinding::new(keys, NoAction, Some(guild_settings_modals::PURGE_NOTES_CONTEXT)));
}

/// Every shortcut binding of the client layout.
fn bindings() -> Vec<KeyBinding> {
    let mut out = Vec::new();
    both(&mut out, "alt-up", ChannelPrev);
    both(&mut out, "alt-down", ChannelNext);
    both(&mut out, "alt-shift-up", UnreadChannelPrev);
    both(&mut out, "alt-shift-down", UnreadChannelNext);
    both(&mut out, "ctrl-alt-shift-up", MentionChannelPrev);
    both(&mut out, "ctrl-alt-shift-down", MentionChannelNext);
    both(&mut out, "ctrl-alt-up", GuildPrev);
    both(&mut out, "ctrl-alt-down", GuildNext);
    both(&mut out, "alt-left", HistoryBack);
    both(&mut out, "alt-right", HistoryForward);
    both(&mut out, "ctrl-alt-right", ToggleLastGuild);
    both(&mut out, "ctrl-shift-n", OpenJoinGuild);
    both(&mut out, "ctrl-f", OpenSearch);
    // Ctrl+K also closes the switcher from its own input.
    out.push(KeyBinding::new("ctrl-k", ToggleQuickSwitcher, Some(CONTEXT)));
    out.push(KeyBinding::new("ctrl-k", ToggleQuickSwitcher, Some(IN_INPUT)));
    out.push(KeyBinding::new("ctrl-k", NoAction, Some(account_switcher::CONTEXT)));
    out.push(KeyBinding::new("ctrl-k", NoAction, Some(channel_modal::CREATE_CONTEXT)));
    out.push(KeyBinding::new("ctrl-k", NoAction, Some(IN_CREATE_CHANNEL_INPUT)));
    out.push(KeyBinding::new("ctrl-k", NoAction, Some(channel_modal::EDIT_CONTEXT)));
    out.push(KeyBinding::new("ctrl-k", NoAction, Some(IN_EDIT_CHANNEL_INPUT)));
    out.push(KeyBinding::new("ctrl-k", NoAction, Some(moderation_modal::ACTION_CONTEXT)));
    out.push(KeyBinding::new("ctrl-k", NoAction, Some(IN_MODERATION_INPUT)));
    out.push(KeyBinding::new("ctrl-k", NoAction, Some(moderation_modal::BANS_CONTEXT)));
    out.push(KeyBinding::new("ctrl-k", NoAction, Some(IN_BANS_INPUT)));
    out.push(KeyBinding::new("ctrl-k", NoAction, Some(access::CONTEXT)));
    out.push(KeyBinding::new("ctrl-k", NoAction, Some(IN_ACCESS_INPUT)));
    out.push(KeyBinding::new("ctrl-k", NoAction, Some(forward_modal::CONTEXT)));
    out.push(KeyBinding::new("ctrl-k", NoAction, Some(IN_FORWARD_INPUT)));
    out.push(KeyBinding::new("ctrl-k", NoAction, Some(create_thread_modal::CONTEXT)));
    out.push(KeyBinding::new("ctrl-k", NoAction, Some(IN_CREATE_THREAD_INPUT)));
    out.push(KeyBinding::new("ctrl-k", NoAction, Some(invite_modal::CONTEXT)));
    out.push(KeyBinding::new("ctrl-k", NoAction, Some(IN_INVITE_INPUT)));
    out.push(KeyBinding::new("ctrl-k", NoAction, Some(guild_settings_modals::NOTIFICATIONS_CONTEXT)));
    out.push(KeyBinding::new("ctrl-k", NoAction, Some(guild_settings_modals::PRIVACY_CONTEXT)));
    out.push(KeyBinding::new("ctrl-k", NoAction, Some(guild_settings_modals::PROFILE_CONTEXT)));
    out.push(KeyBinding::new("ctrl-k", NoAction, Some(IN_GUILD_PROFILE_INPUT)));
    out.push(KeyBinding::new("ctrl-k", NoAction, Some(guild_settings_modals::DELETE_MESSAGES_CONTEXT)));
    out.push(KeyBinding::new("ctrl-k", NoAction, Some(IN_DELETE_GUILD_MESSAGES_INPUT)));
    out.push(KeyBinding::new("ctrl-k", NoAction, Some(guild_settings_modals::PURGE_NOTES_CONTEXT)));
    both(&mut out, "ctrl-e", ToggleEmojiPicker);
    both(&mut out, "ctrl-,", OpenSettings);
    both(&mut out, "ctrl-=", ZoomIn);
    both(&mut out, "ctrl--", ZoomOut);
    both(&mut out, "ctrl-0", ZoomReset);
    both(&mut out, "shift-escape", MarkGuildRead);
    out.push(KeyBinding::new("escape", MarkChannelRead, Some(CONTEXT)));
    out.push(KeyBinding::new("ctrl-p", SwitcherPrev, Some(quick_switcher::CONTEXT)));
    out.push(KeyBinding::new("ctrl-n", SwitcherNext, Some(quick_switcher::CONTEXT)));
    out
}

/// Registers the bindings; call after `gpui_kit::init` so they take
/// precedence over the input's bindings of the same keys.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys(bindings());
}

/// Hooks the action handlers onto the layout root.
pub fn on_actions(root: Div, cx: &mut Context<ClientView>) -> Div {
    root.key_context(CONTEXT)
        .on_action(cx.listener(|v, _: &ChannelPrev, _, cx| v.cycle_channel(-1, cx)))
        .on_action(cx.listener(|v, _: &ChannelNext, _, cx| v.cycle_channel(1, cx)))
        .on_action(cx.listener(|v, _: &UnreadChannelPrev, _, cx| v.cycle_filtered(ChannelFilter::Unread, -1, cx)))
        .on_action(cx.listener(|v, _: &UnreadChannelNext, _, cx| v.cycle_filtered(ChannelFilter::Unread, 1, cx)))
        .on_action(cx.listener(|v, _: &MentionChannelPrev, _, cx| v.cycle_filtered(ChannelFilter::Mentioned, -1, cx)))
        .on_action(cx.listener(|v, _: &MentionChannelNext, _, cx| v.cycle_filtered(ChannelFilter::Mentioned, 1, cx)))
        .on_action(cx.listener(|v, _: &GuildPrev, _, cx| v.cycle_guild(-1, cx)))
        .on_action(cx.listener(|v, _: &GuildNext, _, cx| v.cycle_guild(1, cx)))
        .on_action(cx.listener(|v, _: &HistoryBack, _, cx| v.history_back(cx)))
        .on_action(cx.listener(|v, _: &HistoryForward, _, cx| v.history_forward(cx)))
        .on_action(cx.listener(|v, _: &ToggleLastGuild, _, cx| v.toggle_last_guild(cx)))
        .on_action(cx.listener(|v, _: &OpenJoinGuild, window, cx| v.open_join_guild(window, cx)))
        .on_action(cx.listener(|v, _: &OpenSearch, window, cx| v.open_search(window, cx)))
        .on_action(cx.listener(|v, _: &ToggleQuickSwitcher, window, cx| v.toggle_quick_switcher(window, cx)))
        .on_action(cx.listener(|v, _: &ToggleEmojiPicker, window, cx| v.toggle_emoji_picker(window, cx)))
        .on_action(cx.listener(|v, _: &OpenSettings, window, cx| v.open_settings(window, cx)))
        .on_action(cx.listener(|v, _: &ZoomIn, _, cx| v.step_ui_zoom(10, cx)))
        .on_action(cx.listener(|v, _: &ZoomOut, _, cx| v.step_ui_zoom(-10, cx)))
        .on_action(cx.listener(|v, _: &ZoomReset, _, cx| v.settings_set_zoom(100, cx)))
        .on_action(cx.listener(|v, _: &MarkChannelRead, window, cx| {
            if matches!(&v.overlay, crate::ui::overlay::Overlay::EditChannel(_)) {
                v.handle_edit_channel_escape(window, cx);
            } else if v.overlay.is_open() {
                v.close_overlay(window, cx);
            } else if v.search.is_some() {
                v.close_search(window, cx);
            } else {
                v.mark_channel_read(cx);
            }
        }))
        .on_action(cx.listener(|v, _: &MarkGuildRead, _, cx| v.mark_guild_read(cx)))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn no_duplicate_keystrokes_per_context() {
        let mut seen = BTreeSet::new();
        for b in bindings() {
            let key = (format!("{:?}", b.keystrokes()), b.predicate().map(|p| p.to_string()));
            assert!(seen.insert(key.clone()), "{key:?} bound twice");
        }
        assert_eq!(seen.len(), 371);
    }
}
