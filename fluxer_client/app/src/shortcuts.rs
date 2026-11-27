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

use std::collections::HashMap;

use gpui_kit::{Action, App, Context, Div, Focusable as _, KeyBinding, Keystroke, NoAction, prelude::*};

use crate::client_view::ClientView;
use crate::nav::ChannelFilter;
use crate::prefs::{CustomKeybindEntry, Prefs};
use crate::ui::message_actions::MessageAction;
use crate::ui::{
    access, account_switcher, channel_modal, create_thread_modal, explore, forward_modal, guild_settings_modals,
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
        GuildTabPrev,
        GuildTabNext,
        GuildSlot1,
        GuildSlot2,
        GuildSlot3,
        GuildSlot4,
        GuildSlot5,
        GuildSlot6,
        GuildSlot7,
        GuildSlot8,
        GuildSlot9,
        JumpCurrentCall,
        HistoryBack,
        HistoryForward,
        ToggleLastGuild,
        OpenJoinGuild,
        OpenSearch,
        ToggleQuickSwitcher,
        FocusTextarea,
        UploadFile,
        ToggleEmojiPicker,
        ToggleGifPicker,
        ToggleStickerPicker,
        TogglePins,
        ToggleInbox,
        ToggleMemberList,
        ScrollChatUp,
        ScrollChatDown,
        JumpOldestUnread,
        MarkInboxRead,
        NewDm,
        CopyChannelLink,
        OpenSettings,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        MarkChannelRead,
        MarkGuildRead,
        SwitcherPrev,
        SwitcherNext,
        MessageReply,
        MessageReplyPrev,
        MessageReplyNext,
        MessageEdit,
        MessageEditPrev,
        MessageEditNext,
        MessageReact,
        MessageForward,
        MessagePin,
        MessageMarkUnread,
        MessageCopyText,
        MessageSpeak,
        MessageBookmark,
        MessageCopyId,
        MessageCopyLink,
        MessageToggleEmbeds,
        ChatMarkAllInboxRead,
        ChatToggleSavedMedia,
        ChatSendVoiceMessage,
        VoiceToggleMute,
        VoiceToggleDeafen,
        VoiceToggleSoundboard,
        VoiceStartDmCall,
        VoiceAnswerCall,
        VoiceToggleCompactCallView,
        VoiceToggleCamera,
        VoiceSwitchChannel,
        VoiceDisconnect,
        VoicePushToTalk,
        VoicePushToTalkPriority,
        VoicePushToMute,
        VoicePriorityVad,
        VoiceToggleVad,
        DndStart,
        DndMoveUp,
        DndMoveDown,
        DndDrop,
        SystemToggleShortcutsOverlay,
        MiscHelp,
        MiscOpenContextMenu,
        SystemToggleDownloads,
        SystemOpenThemeStudioPopout,
        SystemToggleCustomCss,
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

const MODAL_CONTEXTS: &[&str] = &[
    search_panel::CONTEXT,
    account_switcher::CONTEXT,
    channel_modal::CREATE_CONTEXT,
    channel_modal::EDIT_CONTEXT,
    moderation_modal::ACTION_CONTEXT,
    moderation_modal::BANS_CONTEXT,
    access::CONTEXT,
    forward_modal::CONTEXT,
    create_thread_modal::CONTEXT,
    invite_modal::CONTEXT,
    guild_settings_modals::NOTIFICATIONS_CONTEXT,
    guild_settings_modals::PRIVACY_CONTEXT,
    guild_settings_modals::PROFILE_CONTEXT,
    guild_settings_modals::DELETE_MESSAGES_CONTEXT,
    guild_settings_modals::PURGE_NOTES_CONTEXT,
    explore::PREVIEW_CONTEXT,
];

/// Binds `keys` in the layout and inside the composer, and disables them
/// inside the quick switcher and search panel (whose inputs would otherwise
/// fall back to the layout's binding).
fn both<A: Action + Clone>(out: &mut Vec<KeyBinding>, keys: &'static str, action: A) {
    out.push(KeyBinding::new(keys, action.clone(), Some(CONTEXT)));
    out.push(KeyBinding::new(keys, action, Some(IN_COMPOSER)));
    out.push(KeyBinding::new(keys, NoAction, Some(quick_switcher::CONTEXT)));
    for ctx in MODAL_CONTEXTS {
        out.push(KeyBinding::new(keys, NoAction, Some(ctx)));
    }
}

fn bind_layout_only<A: Action + Clone>(out: &mut Vec<KeyBinding>, keys: &'static str, action: A) {
    out.push(KeyBinding::new(keys, action, Some(CONTEXT)));
    out.push(KeyBinding::new(keys, NoAction, Some(quick_switcher::CONTEXT)));
    for ctx in MODAL_CONTEXTS {
        out.push(KeyBinding::new(keys, NoAction, Some(ctx)));
    }
}

fn bind_layout_only_no_input<A: Action + Clone>(out: &mut Vec<KeyBinding>, keys: &'static str, action: A) {
    bind_layout_only(out, keys, action);
    out.push(KeyBinding::new(keys, NoAction, Some(IN_COMPOSER)));
    out.push(KeyBinding::new(keys, NoAction, Some(IN_INPUT)));
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
    both(&mut out, "ctrl-shift-tab", GuildTabPrev);
    both(&mut out, "ctrl-tab", GuildTabNext);
    both(&mut out, "ctrl-1", GuildSlot1);
    both(&mut out, "ctrl-2", GuildSlot2);
    both(&mut out, "ctrl-3", GuildSlot3);
    both(&mut out, "ctrl-4", GuildSlot4);
    both(&mut out, "ctrl-5", GuildSlot5);
    both(&mut out, "ctrl-6", GuildSlot6);
    both(&mut out, "ctrl-7", GuildSlot7);
    both(&mut out, "ctrl-8", GuildSlot8);
    both(&mut out, "ctrl-9", GuildSlot9);
    both(&mut out, "ctrl-alt-shift-v", JumpCurrentCall);
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
    out.push(KeyBinding::new("ctrl-k", NoAction, Some(explore::PREVIEW_CONTEXT)));
    both(&mut out, "ctrl-e", ToggleEmojiPicker);
    both(&mut out, "ctrl-g", ToggleGifPicker);
    both(&mut out, "ctrl-s", ToggleStickerPicker);
    both(&mut out, "ctrl-i", ToggleInbox);
    both(&mut out, "ctrl-u", ToggleMemberList);
    both(&mut out, "ctrl-shift-u", UploadFile);
    both(&mut out, "pageup", ScrollChatUp);
    both(&mut out, "pagedown", ScrollChatDown);
    both(&mut out, "shift-pageup", JumpOldestUnread);
    both(&mut out, "ctrl-shift-e", MarkInboxRead);
    both(&mut out, "ctrl-shift-t", NewDm);
    both(&mut out, "ctrl-shift-l", CopyChannelLink);
    // Ctrl+P is SwitcherPrev inside quick_switcher::CONTEXT.
    out.push(KeyBinding::new("ctrl-p", TogglePins, Some(CONTEXT)));
    out.push(KeyBinding::new("ctrl-p", TogglePins, Some(IN_COMPOSER)));
    for ctx in MODAL_CONTEXTS {
        out.push(KeyBinding::new("ctrl-p", NoAction, Some(ctx)));
    }
    bind_layout_only(&mut out, "tab", FocusTextarea);
    both(&mut out, "ctrl-,", OpenSettings);
    both(&mut out, "ctrl-=", ZoomIn);
    both(&mut out, "ctrl--", ZoomOut);
    both(&mut out, "ctrl-0", ZoomReset);
    both(&mut out, "shift-escape", MarkGuildRead);
    out.push(KeyBinding::new("escape", MarkChannelRead, Some(CONTEXT)));
    bind_layout_only_no_input(&mut out, "r", MessageReply);
    both(&mut out, "ctrl-up", MessageReplyPrev);
    both(&mut out, "ctrl-down", MessageReplyNext);
    bind_layout_only_no_input(&mut out, "e", MessageEdit);
    both(&mut out, "ctrl-shift-up", MessageEditPrev);
    both(&mut out, "ctrl-shift-down", MessageEditNext);
    bind_layout_only_no_input(&mut out, "+", MessageReact);
    bind_layout_only_no_input(&mut out, "f", MessageForward);
    bind_layout_only_no_input(&mut out, "p", MessagePin);
    bind_layout_only_no_input(&mut out, "alt-enter", MessageMarkUnread);
    bind_layout_only(&mut out, "ctrl-c", MessageCopyText);
    bind_layout_only_no_input(&mut out, "s", MessageSpeak);
    both(&mut out, "ctrl-/", SystemToggleShortcutsOverlay);
    both(&mut out, "ctrl-shift-m", VoiceToggleMute);
    both(&mut out, "ctrl-shift-d", VoiceToggleDeafen);
    both(&mut out, "ctrl-shift-b", VoiceToggleSoundboard);
    both(&mut out, "ctrl-shift-v", VoiceToggleCompactCallView);
    both(&mut out, "ctrl-enter", VoiceAnswerCall);
    both(&mut out, "ctrl-shift-g", ChatToggleSavedMedia);
    both(&mut out, "ctrl-alt-v", ChatSendVoiceMessage);
    both(&mut out, "ctrl-shift-h", MiscHelp);
    both(&mut out, "ctrl-alt-shift-t", SystemOpenThemeStudioPopout);
    both(&mut out, "ctrl-alt-shift-k", SystemToggleCustomCss);
    bind_layout_only_no_input(&mut out, "shift-f10", MiscOpenContextMenu);
    bind_layout_only_no_input(&mut out, "ctrl-d", DndStart);
    bind_layout_only(&mut out, "up", DndMoveUp);
    bind_layout_only(&mut out, "down", DndMoveDown);
    bind_layout_only(&mut out, "space", DndDrop);
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
        .on_action(cx.listener(|v, _: &ChannelPrev, _, cx| {
            if v.should_run_builtin_shortcut("nav_channel_prev") {
                v.cycle_channel(-1, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &ChannelNext, _, cx| {
            if v.should_run_builtin_shortcut("nav_channel_next") {
                v.cycle_channel(1, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &UnreadChannelPrev, _, cx| {
            if v.should_run_builtin_shortcut("nav_unread_prev") {
                v.cycle_filtered(ChannelFilter::Unread, -1, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &UnreadChannelNext, _, cx| {
            if v.should_run_builtin_shortcut("nav_unread_next") {
                v.cycle_filtered(ChannelFilter::Unread, 1, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &MentionChannelPrev, _, cx| {
            if v.should_run_builtin_shortcut("nav_mention_prev") {
                v.cycle_filtered(ChannelFilter::Mentioned, -1, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &MentionChannelNext, _, cx| {
            if v.should_run_builtin_shortcut("nav_mention_next") {
                v.cycle_filtered(ChannelFilter::Mentioned, 1, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &GuildPrev, _, cx| {
            if v.should_run_builtin_shortcut("nav_guild_prev") {
                v.cycle_guild(-1, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &GuildNext, _, cx| {
            if v.should_run_builtin_shortcut("nav_guild_next") {
                v.cycle_guild(1, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &GuildTabPrev, _, cx| {
            if v.should_run_builtin_shortcut("nav_guild_tab_prev") {
                v.cycle_guild(-1, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &GuildTabNext, _, cx| {
            if v.should_run_builtin_shortcut("nav_guild_tab_next") {
                v.cycle_guild(1, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &GuildSlot1, _, cx| {
            if v.should_run_builtin_shortcut("nav_guild_slot_1") {
                v.select_guild_slot(0, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &GuildSlot2, _, cx| {
            if v.should_run_builtin_shortcut("nav_guild_slot_2") {
                v.select_guild_slot(1, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &GuildSlot3, _, cx| {
            if v.should_run_builtin_shortcut("nav_guild_slot_3") {
                v.select_guild_slot(2, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &GuildSlot4, _, cx| {
            if v.should_run_builtin_shortcut("nav_guild_slot_4") {
                v.select_guild_slot(3, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &GuildSlot5, _, cx| {
            if v.should_run_builtin_shortcut("nav_guild_slot_5") {
                v.select_guild_slot(4, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &GuildSlot6, _, cx| {
            if v.should_run_builtin_shortcut("nav_guild_slot_6") {
                v.select_guild_slot(5, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &GuildSlot7, _, cx| {
            if v.should_run_builtin_shortcut("nav_guild_slot_7") {
                v.select_guild_slot(6, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &GuildSlot8, _, cx| {
            if v.should_run_builtin_shortcut("nav_guild_slot_8") {
                v.select_guild_slot(7, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &GuildSlot9, _, cx| {
            if v.should_run_builtin_shortcut("nav_guild_slot_9") {
                v.select_guild_slot(8, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &JumpCurrentCall, _, cx| {
            if v.should_run_builtin_shortcut("nav_current_call") {
                v.jump_to_current_call(cx);
            }
        }))
        .on_action(cx.listener(|v, _: &HistoryBack, _, cx| {
            if v.should_run_builtin_shortcut("nav_history_back") {
                v.history_back(cx);
            }
        }))
        .on_action(cx.listener(|v, _: &HistoryForward, _, cx| {
            if v.should_run_builtin_shortcut("nav_history_forward") {
                v.history_forward(cx);
            }
        }))
        .on_action(cx.listener(|v, _: &ToggleLastGuild, _, cx| {
            if v.should_run_builtin_shortcut("nav_toggle_last_guild_dms") {
                v.toggle_last_guild(cx);
            }
        }))
        .on_action(cx.listener(|v, _: &OpenJoinGuild, window, cx| {
            if v.should_run_builtin_shortcut("nav_add_guild") {
                v.open_join_guild(window, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &OpenSearch, window, cx| {
            if v.should_run_builtin_shortcut("misc_search") {
                v.open_search(window, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &ToggleQuickSwitcher, window, cx| {
            if v.should_run_builtin_shortcut("nav_quick_switcher") {
                v.toggle_quick_switcher(window, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &ToggleEmojiPicker, window, cx| {
            if v.should_run_builtin_shortcut("chat_toggle_emoji") {
                v.toggle_emoji_picker(window, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &FocusTextarea, window, cx| {
            if v.should_run_builtin_shortcut("chat_focus_textarea") && !v.overlay.is_open() {
                v.focus_composer(window, cx);
            } else {
                cx.propagate();
            }
        }))
        .on_action(cx.listener(|v, _: &UploadFile, _, cx| {
            if v.should_run_builtin_shortcut("chat_upload") {
                v.pick_files(cx);
            }
        }))
        .on_action(cx.listener(|v, _: &ToggleGifPicker, window, cx| {
            if v.should_run_builtin_shortcut("chat_toggle_gif") {
                v.toggle_gif_picker(window, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &ToggleStickerPicker, window, cx| {
            if v.should_run_builtin_shortcut("chat_toggle_sticker") {
                v.toggle_sticker_picker(window, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &TogglePins, window, cx| {
            if v.should_run_builtin_shortcut("chat_toggle_pins") {
                v.toggle_pins_shortcut(window, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &ToggleInbox, window, cx| {
            if v.should_run_builtin_shortcut("chat_toggle_inbox") && v.is_composer_empty_or_unfocused(window, cx) {
                v.toggle_inbox_shortcut(window, cx);
            } else {
                cx.propagate();
            }
        }))
        .on_action(cx.listener(|v, _: &ToggleMemberList, _, cx| {
            if v.should_run_builtin_shortcut("chat_toggle_member_list") {
                v.toggle_member_list(cx);
            }
        }))
        .on_action(cx.listener(|v, _: &ScrollChatUp, window, cx| {
            if v.should_run_builtin_shortcut("chat_scroll_up") && v.is_composer_empty_or_unfocused(window, cx) {
                v.scroll_chat_page(-1, cx);
            } else {
                cx.propagate();
            }
        }))
        .on_action(cx.listener(|v, _: &ScrollChatDown, window, cx| {
            if v.should_run_builtin_shortcut("chat_scroll_down") && v.is_composer_empty_or_unfocused(window, cx) {
                v.scroll_chat_page(1, cx);
            } else {
                cx.propagate();
            }
        }))
        .on_action(cx.listener(|v, _: &JumpOldestUnread, _, cx| {
            if v.should_run_builtin_shortcut("chat_jump_oldest_unread") {
                v.jump_to_unread(cx);
            }
        }))
        .on_action(cx.listener(|v, _: &MarkInboxRead, _, cx| {
            if v.should_run_builtin_shortcut("chat_mark_inbox_read") {
                v.mark_top_inbox_read(cx);
            }
        }))
        .on_action(cx.listener(|v, _: &NewDm, window, cx| {
            if v.should_run_builtin_shortcut("chat_new_dm") {
                v.open_create_group_dm(window, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &CopyChannelLink, _, cx| {
            if v.should_run_builtin_shortcut("chat_copy_channel_link") {
                v.copy_channel_link(cx);
            }
        }))
        .on_action(cx.listener(|v, _: &OpenSettings, window, cx| {
            if v.should_run_builtin_shortcut("system_toggle_settings") {
                v.open_settings(window, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &ZoomIn, _, cx| {
            if v.should_run_builtin_shortcut("system_zoom_in") {
                v.step_ui_zoom(10, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &ZoomOut, _, cx| {
            if v.should_run_builtin_shortcut("system_zoom_out") {
                v.step_ui_zoom(-10, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &ZoomReset, _, cx| {
            if v.should_run_builtin_shortcut("system_zoom_reset") {
                v.settings_set_zoom(100, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &MarkChannelRead, window, cx| {
            if v.is_recording_keybind() {
                v.settings_cancel_record_keybind(cx);
            } else if matches!(&v.overlay, crate::ui::overlay::Overlay::EditChannel(_)) {
                v.handle_edit_channel_escape(window, cx);
            } else if v.overlay.is_open() {
                v.close_overlay(window, cx);
            } else if v.search.is_some() {
                v.close_search(window, cx);
            } else if v.is_dnd_active() && v.should_run_builtin_shortcut("dnd_cancel") {
                v.cancel_dnd(cx);
            } else if v.has_incoming_call() && v.should_run_builtin_shortcut("voice_decline_call") {
                v.decline_incoming_call(cx);
            } else if v.keyboard_message.is_some() && v.should_run_builtin_shortcut("message_focus_textarea") {
                v.focus_textarea_from_message_cursor(window, cx);
            } else if v.should_run_builtin_shortcut("chat_mark_channel_read") {
                v.mark_channel_read(cx);
            }
        }))
        .on_action(cx.listener(|v, _: &MarkGuildRead, _, cx| {
            if v.should_run_builtin_shortcut("chat_mark_guild_read") {
                v.mark_guild_read(cx);
            }
        }))
        .on_action(cx.listener(|v, _: &MessageReply, window, cx| {
            if v.should_run_builtin_shortcut("message_reply")
                && !v.overlay.is_open()
                && !v.composer.read(cx).focus_handle(cx).is_focused(window)
            {
                v.trigger_cursor_message_action(MessageAction::Reply, window, cx);
            } else {
                cx.propagate();
            }
        }))
        .on_action(cx.listener(|v, _: &MessageReplyPrev, window, cx| {
            if v.should_run_builtin_shortcut("message_reply_prev") && !v.overlay.is_open() {
                v.step_cursor_message_action(false, -1, window, cx);
            } else {
                cx.propagate();
            }
        }))
        .on_action(cx.listener(|v, _: &MessageReplyNext, window, cx| {
            if v.should_run_builtin_shortcut("message_reply_next") && !v.overlay.is_open() {
                v.step_cursor_message_action(false, 1, window, cx);
            } else {
                cx.propagate();
            }
        }))
        .on_action(cx.listener(|v, _: &MessageEdit, window, cx| {
            if v.should_run_builtin_shortcut("message_edit")
                && !v.overlay.is_open()
                && !v.composer.read(cx).focus_handle(cx).is_focused(window)
            {
                v.trigger_cursor_message_action(MessageAction::Edit, window, cx);
            } else {
                cx.propagate();
            }
        }))
        .on_action(cx.listener(|v, _: &MessageEditPrev, window, cx| {
            if v.should_run_builtin_shortcut("message_edit_prev") && !v.overlay.is_open() {
                v.step_cursor_message_action(true, -1, window, cx);
            } else {
                cx.propagate();
            }
        }))
        .on_action(cx.listener(|v, _: &MessageEditNext, window, cx| {
            if v.should_run_builtin_shortcut("message_edit_next") && !v.overlay.is_open() {
                v.step_cursor_message_action(true, 1, window, cx);
            } else {
                cx.propagate();
            }
        }))
        .on_action(cx.listener(|v, _: &MessageReact, window, cx| {
            if v.should_run_builtin_shortcut("message_react")
                && !v.overlay.is_open()
                && !v.composer.read(cx).focus_handle(cx).is_focused(window)
            {
                v.trigger_cursor_message_action(MessageAction::React, window, cx);
            } else {
                cx.propagate();
            }
        }))
        .on_action(cx.listener(|v, _: &MessageForward, window, cx| {
            if v.should_run_builtin_shortcut("message_forward")
                && !v.overlay.is_open()
                && !v.composer.read(cx).focus_handle(cx).is_focused(window)
            {
                v.trigger_cursor_message_action(MessageAction::Forward, window, cx);
            } else {
                cx.propagate();
            }
        }))
        .on_action(cx.listener(|v, _: &MessagePin, window, cx| {
            if v.should_run_builtin_shortcut("message_pin")
                && !v.overlay.is_open()
                && !v.composer.read(cx).focus_handle(cx).is_focused(window)
            {
                v.trigger_cursor_message_action(MessageAction::Pin, window, cx);
            } else {
                cx.propagate();
            }
        }))
        .on_action(cx.listener(|v, _: &MessageMarkUnread, window, cx| {
            if v.should_run_builtin_shortcut("message_mark_unread")
                && !v.overlay.is_open()
                && !v.composer.read(cx).focus_handle(cx).is_focused(window)
            {
                v.trigger_cursor_message_action(MessageAction::MarkUnread, window, cx);
            } else {
                cx.propagate();
            }
        }))
        .on_action(cx.listener(|v, _: &MessageCopyText, window, cx| {
            if v.should_run_builtin_shortcut("message_copy_text")
                && !v.overlay.is_open()
                && !v.composer.read(cx).focus_handle(cx).is_focused(window)
            {
                v.trigger_cursor_message_action(MessageAction::CopyText, window, cx);
            } else {
                cx.propagate();
            }
        }))
        .on_action(cx.listener(|v, _: &MessageSpeak, window, cx| {
            if v.should_run_builtin_shortcut("message_speak")
                && !v.overlay.is_open()
                && !v.composer.read(cx).focus_handle(cx).is_focused(window)
            {
                v.trigger_cursor_message_action(MessageAction::Speak, window, cx);
            } else {
                cx.propagate();
            }
        }))
        .on_action(cx.listener(|v, _: &MessageBookmark, window, cx| {
            if v.should_run_builtin_shortcut("message_bookmark")
                && !v.overlay.is_open()
                && !v.composer.read(cx).focus_handle(cx).is_focused(window)
            {
                v.trigger_extended_cursor_message_action(MessageAction::Bookmark, window, cx);
            } else {
                cx.propagate();
            }
        }))
        .on_action(cx.listener(|v, _: &MessageCopyId, window, cx| {
            if v.should_run_builtin_shortcut("message_copy_id")
                && !v.overlay.is_open()
                && !v.composer.read(cx).focus_handle(cx).is_focused(window)
            {
                v.trigger_extended_cursor_message_action(MessageAction::CopyId, window, cx);
            } else {
                cx.propagate();
            }
        }))
        .on_action(cx.listener(|v, _: &MessageCopyLink, window, cx| {
            if v.should_run_builtin_shortcut("message_copy_link")
                && !v.overlay.is_open()
                && !v.composer.read(cx).focus_handle(cx).is_focused(window)
            {
                v.trigger_extended_cursor_message_action(MessageAction::CopyLink, window, cx);
            } else {
                cx.propagate();
            }
        }))
        .on_action(cx.listener(|v, _: &MessageToggleEmbeds, window, cx| {
            if v.should_run_builtin_shortcut("message_toggle_embeds")
                && !v.overlay.is_open()
                && !v.composer.read(cx).focus_handle(cx).is_focused(window)
            {
                v.trigger_extended_cursor_message_action(MessageAction::SuppressEmbeds, window, cx);
            } else {
                cx.propagate();
            }
        }))
        .on_action(cx.listener(|v, _: &ChatMarkAllInboxRead, _, cx| {
            if v.should_run_builtin_shortcut("chat_mark_all_inbox_read") {
                v.mark_all_inbox_read(cx);
            }
        }))
        .on_action(cx.listener(|v, _: &ChatToggleSavedMedia, window, cx| {
            if v.should_run_builtin_shortcut("chat_toggle_saved_media") {
                v.toggle_saved_media_shortcut(window, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &ChatSendVoiceMessage, _, cx| {
            if v.should_run_builtin_shortcut("chat_send_voice_message") {
                v.start_voice_message(cx);
            }
        }))
        .on_action(cx.listener(|v, _: &VoiceToggleMute, _, cx| {
            if v.should_run_builtin_shortcut("voice_toggle_mute") {
                v.toggle_voice_mute(cx);
            }
        }))
        .on_action(cx.listener(|v, _: &VoiceToggleDeafen, _, cx| {
            if v.should_run_builtin_shortcut("voice_toggle_deafen") {
                v.toggle_voice_deaf(cx);
            }
        }))
        .on_action(cx.listener(|v, _: &VoiceToggleSoundboard, _, cx| {
            if v.should_run_builtin_shortcut("voice_toggle_soundboard") {
                v.toggle_soundboard(cx);
            }
        }))
        .on_action(cx.listener(|v, _: &VoiceStartDmCall, _, cx| {
            if v.should_run_builtin_shortcut("voice_start_dm_call") {
                v.start_dm_voice_call(cx);
            }
        }))
        .on_action(cx.listener(|v, _: &VoiceAnswerCall, _, cx| {
            if v.has_incoming_call() && v.should_run_builtin_shortcut("voice_answer_call") {
                v.answer_incoming_call(cx);
            } else {
                cx.propagate();
            }
        }))
        .on_action(cx.listener(|v, _: &VoiceToggleCompactCallView, _, cx| {
            if v.should_run_builtin_shortcut("voice_toggle_compact_call_view") {
                v.toggle_compact_call_view(cx);
            }
        }))
        .on_action(cx.listener(|v, _: &VoiceToggleCamera, _, cx| {
            if v.should_run_builtin_shortcut("voice_toggle_camera") {
                v.toggle_voice_camera(cx);
            }
        }))
        .on_action(cx.listener(|v, _: &VoiceSwitchChannel, _, cx| {
            if v.should_run_builtin_shortcut("voice_switch_channel") {
                v.switch_voice_channel(cx);
            }
        }))
        .on_action(cx.listener(|v, _: &VoiceDisconnect, _, cx| {
            if v.should_run_builtin_shortcut("voice_disconnect") {
                v.leave_voice_channel(cx);
            }
        }))
        .on_action(cx.listener(|v, _: &VoicePushToTalk, _, cx| {
            if v.should_run_builtin_shortcut("voice_push_to_talk") {
                v.trigger_voice_push_to_talk(cx);
            }
        }))
        .on_action(cx.listener(|v, _: &VoicePushToTalkPriority, _, cx| {
            if v.should_run_builtin_shortcut("voice_push_to_talk_priority") {
                v.trigger_voice_push_to_talk_priority(cx);
            }
        }))
        .on_action(cx.listener(|v, _: &VoicePushToMute, _, cx| {
            if v.should_run_builtin_shortcut("voice_push_to_mute") {
                v.trigger_voice_push_to_mute(cx);
            }
        }))
        .on_action(cx.listener(|v, _: &VoicePriorityVad, _, cx| {
            if v.should_run_builtin_shortcut("voice_priority_vad") {
                v.trigger_voice_priority_vad(cx);
            }
        }))
        .on_action(cx.listener(|v, _: &VoiceToggleVad, _, cx| {
            if v.should_run_builtin_shortcut("voice_toggle_vad") {
                v.toggle_voice_vad(cx);
            }
        }))
        .on_action(cx.listener(|v, _: &DndStart, window, cx| {
            if v.should_run_builtin_shortcut("dnd_start")
                && !v.overlay.is_open()
                && !v.composer.read(cx).focus_handle(cx).is_focused(window)
            {
                v.start_dnd(cx);
            } else {
                cx.propagate();
            }
        }))
        .on_action(cx.listener(|v, _: &DndMoveUp, _, cx| {
            if v.is_dnd_active() && v.should_run_builtin_shortcut("dnd_move_up") {
                v.move_dnd(-1, cx);
            } else {
                cx.propagate();
            }
        }))
        .on_action(cx.listener(|v, _: &DndMoveDown, _, cx| {
            if v.is_dnd_active() && v.should_run_builtin_shortcut("dnd_move_down") {
                v.move_dnd(1, cx);
            } else {
                cx.propagate();
            }
        }))
        .on_action(cx.listener(|v, _: &DndDrop, _, cx| {
            if v.is_dnd_active() && v.should_run_builtin_shortcut("dnd_drop") {
                v.drop_dnd(cx);
            } else {
                cx.propagate();
            }
        }))
        .on_action(cx.listener(|v, _: &SystemToggleShortcutsOverlay, window, cx| {
            if v.should_run_builtin_shortcut("system_toggle_shortcuts_overlay") {
                v.toggle_shortcuts_overlay(window, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &MiscHelp, _, cx| {
            if v.should_run_builtin_shortcut("misc_help") {
                v.open_help(cx);
            }
        }))
        .on_action(cx.listener(|v, _: &MiscOpenContextMenu, window, cx| {
            if v.should_run_builtin_shortcut("misc_open_context_menu")
                && !v.overlay.is_open()
                && !v.composer.read(cx).focus_handle(cx).is_focused(window)
            {
                v.open_cursor_context_menu(window, cx);
            } else {
                cx.propagate();
            }
        }))
        .on_action(cx.listener(|v, _: &SystemToggleDownloads, _, cx| {
            if v.should_run_builtin_shortcut("system_toggle_downloads") {
                v.toggle_downloads_panel(cx);
            }
        }))
        .on_action(cx.listener(|v, _: &SystemOpenThemeStudioPopout, _, cx| {
            if v.should_run_builtin_shortcut("system_open_theme_studio_popout") {
                v.open_theme_studio_popout(cx);
            }
        }))
        .on_action(cx.listener(|v, _: &SystemToggleCustomCss, _, cx| {
            if v.should_run_builtin_shortcut("system_toggle_custom_css") {
                v.toggle_custom_css(cx);
            }
        }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeybindSection {
    Defaults,
    Messages,
    Navigation,
    DragAndDrop,
    Chat,
    VoiceAndVideo,
    Accounts,
    Misc,
}

impl KeybindSection {
    pub const ALL: &'static [Self] = &[
        Self::Defaults,
        Self::Messages,
        Self::Navigation,
        Self::DragAndDrop,
        Self::Chat,
        Self::VoiceAndVideo,
        Self::Accounts,
        Self::Misc,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Defaults => "Default keybinds",
            Self::Messages => "Messages",
            Self::Navigation => "Navigation",
            Self::DragAndDrop => "Drag and drop",
            Self::Chat => "Chat",
            Self::VoiceAndVideo => "Voice and video",
            Self::Accounts => "Accounts",
            Self::Misc => "Miscellaneous",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultDisplayKind {
    Normal,
    AnyKey,
    SpaceOrEnter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DefaultKeybindSpec {
    pub action: &'static str,
    pub label: &'static str,
    pub combo: &'static str,
    pub section: KeybindSection,
    pub display_kind: DefaultDisplayKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultShortcutRowSpec {
    Single(DefaultKeybindSpec),
    Pair(DefaultKeybindSpec, DefaultKeybindSpec),
}

impl DefaultShortcutRowSpec {
    pub fn primary_action(&self) -> &'static str {
        match self {
            Self::Single(s) => s.action,
            Self::Pair(a, _) => a.action,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Single(s) => s.label,
            Self::Pair(a, _) => a.label,
        }
    }

    pub fn specs(&self) -> Vec<DefaultKeybindSpec> {
        match self {
            Self::Single(s) => vec![*s],
            Self::Pair(a, b) => vec![*a, *b],
        }
    }
}

const fn spec(
    action: &'static str,
    label: &'static str,
    combo: &'static str,
    section: KeybindSection,
) -> DefaultKeybindSpec {
    DefaultKeybindSpec {
        action,
        label,
        combo,
        section,
        display_kind: DefaultDisplayKind::Normal,
    }
}

pub fn default_shortcut_rows(section: KeybindSection) -> Vec<DefaultShortcutRowSpec> {
    use DefaultShortcutRowSpec::{Pair, Single};
    use KeybindSection as S;
    match section {
        S::Defaults => vec![Single(spec(
            "system_toggle_shortcuts_overlay",
            "Show keybind list",
            "ctrl-/",
            S::Defaults,
        ))],
        S::Messages => vec![
            Single(spec("message_reply", "Reply to message", "r", S::Messages)),
            Pair(
                spec("message_reply_prev", "Reply to messages above or below", "ctrl-up", S::Messages),
                spec("message_reply_next", "Reply to messages above or below", "ctrl-down", S::Messages),
            ),
            Single(spec("message_edit", "Edit message", "e", S::Messages)),
            Pair(
                spec("message_edit_prev", "Edit your messages above or below", "ctrl-shift-up", S::Messages),
                spec("message_edit_next", "Edit your messages above or below", "ctrl-shift-down", S::Messages),
            ),
            Single(spec("message_delete", "Delete message", "backspace", S::Messages)),
            Single(spec("message_react", "Add reaction", "+", S::Messages)),
            Single(spec("message_forward", "Forward message", "f", S::Messages)),
            Single(spec("message_pin", "Pin message", "p", S::Messages)),
            Single(spec("message_mark_unread", "Mark as unread", "alt-enter", S::Messages)),
            Single(spec("message_copy_text", "Copy text", "ctrl-c", S::Messages)),
            Single(spec("message_speak", "Read message aloud", "s", S::Messages)),
            Single(spec("message_focus_textarea", "Focus text area", "escape", S::Messages)),
        ],
        S::Navigation => vec![
            Single(spec("nav_quick_switcher", "Toggle quick switcher", "ctrl-k", S::Navigation)),
            Pair(
                spec("nav_guild_prev", "Switch between communities", "ctrl-alt-up", S::Navigation),
                spec("nav_guild_next", "Switch between communities", "ctrl-alt-down", S::Navigation),
            ),
            Pair(
                spec("nav_channel_prev", "Switch between channels", "alt-up", S::Navigation),
                spec("nav_channel_next", "Switch between channels", "alt-down", S::Navigation),
            ),
            Single(spec(
                "nav_toggle_last_guild_dms",
                "Toggle between last community and DMs",
                "ctrl-alt-right",
                S::Navigation,
            )),
            Pair(
                spec("nav_history_back", "Move back through viewed channel history", "alt-left", S::Navigation),
                spec("nav_history_forward", "Move forward through viewed channel history", "alt-right", S::Navigation),
            ),
            Pair(
                spec("nav_unread_prev", "Jump between unread channels", "alt-shift-up", S::Navigation),
                spec("nav_unread_next", "Jump between unread channels", "alt-shift-down", S::Navigation),
            ),
            Pair(
                spec(
                    "nav_mention_prev",
                    "Jump between unread channels with mentions",
                    "ctrl-alt-shift-up",
                    S::Navigation,
                ),
                spec(
                    "nav_mention_next",
                    "Jump between unread channels with mentions",
                    "ctrl-alt-shift-down",
                    S::Navigation,
                ),
            ),
            Single(spec("nav_current_call", "Jump to the current call", "ctrl-alt-shift-v", S::Navigation)),
            Single(spec("nav_add_guild", "Create or join a community", "ctrl-shift-n", S::Navigation)),
            Single(spec("nav_guild_tab_prev", "Switch to previous community or DMs", "ctrl-shift-tab", S::Navigation)),
            Single(spec("nav_guild_tab_next", "Switch to next community or DMs", "ctrl-tab", S::Navigation)),
            Single(spec("nav_guild_slot_1", "Go to direct messages", "ctrl-1", S::Navigation)),
            Single(spec("nav_guild_slot_2", "Go to first community", "ctrl-2", S::Navigation)),
            Single(spec("nav_guild_slot_3", "Go to second community", "ctrl-3", S::Navigation)),
            Single(spec("nav_guild_slot_4", "Go to third community", "ctrl-4", S::Navigation)),
            Single(spec("nav_guild_slot_5", "Go to fourth community", "ctrl-5", S::Navigation)),
            Single(spec("nav_guild_slot_6", "Go to fifth community", "ctrl-6", S::Navigation)),
            Single(spec("nav_guild_slot_7", "Go to sixth community", "ctrl-7", S::Navigation)),
            Single(spec("nav_guild_slot_8", "Go to seventh community", "ctrl-8", S::Navigation)),
            Single(spec("nav_guild_slot_9", "Go to eighth community", "ctrl-9", S::Navigation)),
        ],
        S::DragAndDrop => vec![
            Single(spec("dnd_start", "Start drag and drop", "ctrl-d", S::DragAndDrop)),
            Pair(
                spec("dnd_move_up", "Move", "up", S::DragAndDrop),
                spec("dnd_move_down", "Move", "down", S::DragAndDrop),
            ),
            Single(DefaultKeybindSpec {
                action: "dnd_drop",
                label: "Drop item",
                combo: " ",
                section: S::DragAndDrop,
                display_kind: DefaultDisplayKind::SpaceOrEnter,
            }),
            Single(spec("dnd_cancel", "Cancel", "escape", S::DragAndDrop)),
        ],
        S::Chat => vec![
            Single(DefaultKeybindSpec {
                action: "chat_focus_textarea",
                label: "Focus the text area",
                combo: "tab",
                section: S::Chat,
                display_kind: DefaultDisplayKind::AnyKey,
            }),
            Single(spec("chat_upload", "Upload a file", "ctrl-shift-u", S::Chat)),
            Single(spec("chat_toggle_emoji", "Toggle the emoji picker", "ctrl-e", S::Chat)),
            Single(spec("chat_toggle_gif", "Toggle the GIF picker", "ctrl-g", S::Chat)),
            Single(spec("chat_toggle_sticker", "Toggle the sticker picker", "ctrl-s", S::Chat)),
            Single(spec("chat_toggle_saved_media", "Toggle saved media", "ctrl-shift-g", S::Chat)),
            Single(spec("chat_send_voice_message", "Send voice message", "ctrl-alt-v", S::Chat)),
            Single(spec("chat_toggle_pins", "Toggle pinned messages", "ctrl-p", S::Chat)),
            Single(spec("chat_toggle_inbox", "Toggle the inbox", "ctrl-i", S::Chat)),
            Single(spec("chat_toggle_member_list", "Toggle the member list or voice chat", "ctrl-u", S::Chat)),
            Pair(
                spec("chat_scroll_up", "Scroll chat up", "pageup", S::Chat),
                spec("chat_scroll_down", "Scroll chat down", "pagedown", S::Chat),
            ),
            Single(spec("chat_jump_oldest_unread", "Jump to the oldest unread message", "shift-pageup", S::Chat)),
            Single(spec("chat_mark_channel_read", "Mark channel as read", "escape", S::Chat)),
            Single(spec("chat_mark_guild_read", "Mark community as read", "shift-escape", S::Chat)),
            Single(spec("chat_mark_inbox_read", "Mark top inbox channel as read", "ctrl-shift-e", S::Chat)),
            Single(spec("chat_mark_all_inbox_read", "Mark all inbox channels as read", "", S::Chat)),
            Single(spec("chat_new_dm", "Start a group DM", "ctrl-shift-t", S::Chat)),
            Single(spec("chat_copy_channel_link", "Copy channel link", "ctrl-shift-l", S::Chat)),
        ],
        S::VoiceAndVideo => vec![
            Single(spec("voice_toggle_mute", "Toggle mute", "ctrl-shift-m", S::VoiceAndVideo)),
            Single(spec("voice_toggle_deafen", "Toggle deafen", "ctrl-shift-d", S::VoiceAndVideo)),
            Single(spec("voice_toggle_soundboard", "Toggle the soundboard", "ctrl-shift-b", S::VoiceAndVideo)),
            Single(spec("voice_start_dm_call", "Start a call in a DM or group", "ctrl-'", S::VoiceAndVideo)),
            Single(spec("voice_answer_call", "Answer the incoming call", "ctrl-enter", S::VoiceAndVideo)),
            Single(spec("voice_decline_call", "Decline the incoming call", "escape", S::VoiceAndVideo)),
            Single(spec("voice_toggle_compact_call_view", "Expand or collapse compact call view", "ctrl-shift-v", S::VoiceAndVideo)),
        ],
        S::Accounts => vec![
            Single(spec("system_open_account_switcher", "Open the account switcher", "ctrl-shift-o", S::Accounts)),
            Single(spec("system_switch_last_account", "Switch back to the last account", "ctrl-j", S::Accounts)),
            Pair(
                spec("system_switch_account_prev", "Switch to the previous account", "ctrl-shift-[", S::Accounts),
                spec("system_switch_account_next", "Switch to the previous account", "ctrl-shift-]", S::Accounts),
            ),
            Single(spec("system_jump_newest_mention_account", "Jump to the newest mention on another account", "ctrl-shift-j", S::Accounts)),
            Single(spec("system_search_other_accounts", "Search other accounts", "ctrl-shift-k", S::Accounts)),
            Single(spec("system_switch_account_1", "Switch to account 1", "ctrl-shift-1", S::Accounts)),
            Single(spec("system_switch_account_2", "Switch to account 2", "ctrl-shift-2", S::Accounts)),
            Single(spec("system_switch_account_3", "Switch to account 3", "ctrl-shift-3", S::Accounts)),
            Single(spec("system_switch_account_4", "Switch to account 4", "ctrl-shift-4", S::Accounts)),
            Single(spec("system_switch_account_5", "Switch to account 5", "ctrl-shift-5", S::Accounts)),
            Single(spec("system_switch_account_6", "Switch to account 6", "ctrl-shift-6", S::Accounts)),
            Single(spec("system_switch_account_7", "Switch to account 7", "ctrl-shift-7", S::Accounts)),
            Single(spec("system_switch_account_8", "Switch to account 8", "ctrl-shift-8", S::Accounts)),
            Single(spec("system_switch_account_9", "Switch to account 9", "ctrl-shift-9", S::Accounts)),
        ],
        S::Misc => vec![
            Single(spec("misc_search", "Search messages", "ctrl-f", S::Misc)),
            Single(spec("misc_open_context_menu", "Open the context menu", "shift-f10", S::Misc)),
            Single(spec("system_toggle_downloads", "Toggle the downloads panel", "", S::Misc)),
            Single(spec("system_open_theme_studio_popout", "Open quick CSS editor", "ctrl-alt-shift-t", S::Misc)),
            Single(spec("system_toggle_custom_css", "Turn custom CSS on or off", "ctrl-alt-shift-k", S::Misc)),
        ],
    }
}

pub const ASSIGNABLE_ACTIONS: &[(&str, &str)] = &[
    ("nav_quick_switcher", "Toggle quick switcher"),
    ("nav_channel_prev", "Previous channel"),
    ("nav_channel_next", "Next channel"),
    ("nav_unread_prev", "Previous unread channel"),
    ("nav_unread_next", "Next unread channel"),
    ("nav_mention_prev", "Previous mention channel"),
    ("nav_mention_next", "Next mention channel"),
    ("nav_guild_prev", "Previous community"),
    ("nav_guild_next", "Next community"),
    ("nav_guild_tab_prev", "Switch to previous community or DMs"),
    ("nav_guild_tab_next", "Switch to next community or DMs"),
    ("nav_guild_slot_1", "Go to direct messages"),
    ("nav_guild_slot_2", "Go to first community"),
    ("nav_guild_slot_3", "Go to second community"),
    ("nav_guild_slot_4", "Go to third community"),
    ("nav_guild_slot_5", "Go to fourth community"),
    ("nav_guild_slot_6", "Go to fifth community"),
    ("nav_guild_slot_7", "Go to sixth community"),
    ("nav_guild_slot_8", "Go to seventh community"),
    ("nav_guild_slot_9", "Go to eighth community"),
    ("nav_current_call", "Jump to the current call"),
    ("nav_history_back", "Channel history back"),
    ("nav_history_forward", "Channel history forward"),
    ("nav_toggle_last_guild_dms", "Toggle last community and DMs"),
    ("nav_add_guild", "Create or join a community"),
    ("misc_search", "Search messages"),
    ("chat_focus_textarea", "Focus the text area"),
    ("chat_upload", "Upload a file"),
    ("chat_toggle_emoji", "Toggle the emoji picker"),
    ("chat_toggle_gif", "Toggle the GIF picker"),
    ("chat_toggle_sticker", "Toggle the sticker picker"),
    ("chat_toggle_pins", "Toggle pinned messages"),
    ("chat_toggle_inbox", "Toggle the inbox"),
    ("chat_toggle_member_list", "Toggle the member list or voice chat"),
    ("chat_scroll_up", "Scroll chat up"),
    ("chat_scroll_down", "Scroll chat down"),
    ("chat_jump_oldest_unread", "Jump to the oldest unread message"),
    ("chat_mark_channel_read", "Mark channel as read"),
    ("chat_mark_guild_read", "Mark community as read"),
    ("chat_mark_inbox_read", "Mark top inbox channel as read"),
    ("chat_mark_all_inbox_read", "Mark all inbox channels as read"),
    ("chat_send_voice_message", "Send voice message"),
    ("chat_new_dm", "Start a group DM"),
    ("chat_copy_channel_link", "Copy channel link"),
    ("voice_toggle_mute", "Toggle mute"),
    ("voice_toggle_deafen", "Toggle deafen"),
    ("voice_toggle_soundboard", "Toggle the soundboard"),
    ("voice_toggle_compact_call_view", "Expand or collapse compact call view"),
    ("voice_toggle_camera", "Toggle camera"),
    ("voice_switch_channel", "Switch voice channel"),
    ("voice_disconnect", "Disconnect from voice"),
    ("voice_push_to_talk", "Push to talk"),
    ("voice_push_to_talk_priority", "Push to talk (Priority)"),
    ("voice_push_to_mute", "Push to mute"),
    ("voice_priority_vad", "Voice activity (Priority)"),
    ("voice_toggle_vad", "Toggle voice activity"),
    ("system_toggle_settings", "Open your settings"),
    ("system_toggle_downloads", "Toggle the downloads panel"),
    ("system_open_theme_studio_popout", "Open quick CSS editor"),
    ("system_toggle_custom_css", "Turn custom CSS on or off"),
    ("system_zoom_in", "Zoom in"),
    ("system_zoom_out", "Zoom out"),
    ("system_zoom_reset", "Reset zoom"),
    ("message_reply", "Reply to message"),
    ("message_reply_prev", "Reply to previous message"),
    ("message_reply_next", "Reply to next message"),
    ("message_edit", "Edit message"),
    ("message_edit_prev", "Edit previous message"),
    ("message_edit_next", "Edit next message"),
    ("message_react", "Add reaction"),
    ("message_forward", "Forward message"),
    ("message_pin", "Pin message"),
    ("message_bookmark", "Bookmark message"),
    ("message_toggle_embeds", "Toggle embed suppression"),
    ("message_copy_link", "Copy message link"),
    ("message_copy_id", "Copy message ID"),
    ("message_mark_unread", "Mark as unread"),
    ("message_copy_text", "Copy text"),
    ("message_speak", "Read message aloud"),
    ("message_focus_textarea", "Focus text area"),
];

pub fn action_label(action: &str) -> Option<&'static str> {
    if let Some((_, label)) = ASSIGNABLE_ACTIONS.iter().find(|(id, _)| *id == action) {
        return Some(*label);
    }
    for section in KeybindSection::ALL {
        for row in default_shortcut_rows(*section) {
            for s in row.specs() {
                if s.action == action {
                    return Some(s.label);
                }
            }
        }
    }
    match action {
        "misc_help" => Some("Open help"),
        _ => None,
    }
}

fn canonical_key_token(raw: &str) -> Option<String> {
    let lower = raw.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return None;
    }
    let mapped = match lower.as_str() {
        "control" | "ctrl" | "shift" | "alt" | "meta" | "cmd" | "super" | "platform" | "function" => return None,
        "esc" | "escape" => "escape",
        "return" | "enter" => "enter",
        " " | "spacebar" | "space" => "space",
        "arrowup" | "up" => "up",
        "arrowdown" | "down" => "down",
        "arrowleft" | "left" => "left",
        "arrowright" | "right" => "right",
        "pgup" | "pageup" => "pageup",
        "pgdn" | "pagedown" => "pagedown",
        "del" | "delete" => "delete",
        "bs" | "backspace" => "backspace",
        "minus" => "-",
        "equal" | "equals" => "=",
        "comma" => ",",
        "period" => ".",
        "slash" => "/",
        other => other,
    };
    Some(mapped.to_owned())
}

/// Normalizes a shortcut string such as `"Shift+Ctrl+M"` or `"ctrl-shift-m"`
/// into canonical modifier order `"ctrl-shift-alt-meta-<key>"`.
pub fn normalize_combo(raw: &str) -> String {
    if raw == " " {
        return "space".to_owned();
    }
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut meta = false;
    let mut key: Option<String> = None;

    // Handle trailing literal "-" or "+" (e.g. "ctrl--" or "ctrl-+")
    let mut tokens: Vec<&str> = Vec::new();
    if trimmed == "-" || trimmed == "+" {
        tokens.push(trimmed);
    } else if let Some(prefix) = trimmed.strip_suffix("--") {
        for t in prefix.split(['-', '+']) {
            if !t.is_empty() {
                tokens.push(t);
            }
        }
        tokens.push("-");
    } else if let Some(prefix) = trimmed.strip_suffix("-+") {
        for t in prefix.split(['-', '+']) {
            if !t.is_empty() {
                tokens.push(t);
            }
        }
        tokens.push("+");
    } else {
        for t in trimmed.split(['-', '+']) {
            let t = t.trim();
            if !t.is_empty() {
                tokens.push(t);
            }
        }
    }

    for token in tokens {
        match token.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => ctrl = true,
            "alt" | "option" => alt = true,
            "shift" => shift = true,
            "meta" | "cmd" | "super" | "win" | "platform" => meta = true,
            _ => {
                if let Some(k) = canonical_key_token(token) {
                    key = Some(k);
                }
            }
        }
    }

    let Some(key) = key else {
        return String::new();
    };
    let mut parts = Vec::with_capacity(5);
    if ctrl {
        parts.push("ctrl");
    }
    if shift {
        parts.push("shift");
    }
    if alt {
        parts.push("alt");
    }
    if meta {
        parts.push("meta");
    }
    parts.push(&key);
    parts.join("-")
}

/// Converts a GPUI [`Keystroke`] into a canonical combo string. Returns `None`
/// for bare modifier presses without a main key.
pub fn keystroke_to_combo(ks: &Keystroke) -> Option<String> {
    let key = canonical_key_token(&ks.key)?;
    let mut parts = Vec::with_capacity(5);
    if ks.modifiers.control {
        parts.push("ctrl");
    }
    if ks.modifiers.shift {
        parts.push("shift");
    }
    if ks.modifiers.alt {
        parts.push("alt");
    }
    if ks.modifiers.platform {
        parts.push("meta");
    }
    parts.push(&key);
    Some(parts.join("-"))
}

pub fn combos_equal(a: &str, b: &str) -> bool {
    let na = normalize_combo(a);
    let nb = normalize_combo(b);
    !na.is_empty() && na == nb
}

fn chip_label_for_token(token: &str) -> String {
    match token {
        "ctrl" => "Ctrl".to_owned(),
        "alt" => "Alt".to_owned(),
        "shift" => "Shift".to_owned(),
        "meta" => "Win".to_owned(),
        "escape" => "Esc".to_owned(),
        "enter" => "Enter".to_owned(),
        "space" => "Space".to_owned(),
        "tab" => "Tab".to_owned(),
        "backspace" => "⌫".to_owned(),
        "delete" => "Del".to_owned(),
        "up" => "▲".to_owned(),
        "down" => "▼".to_owned(),
        "left" => "◀".to_owned(),
        "right" => "▶".to_owned(),
        "pageup" => "Page up".to_owned(),
        "pagedown" => "Page down".to_owned(),
        other if other.len() == 1 => other.to_ascii_uppercase(),
        other if other.starts_with('f') && other[1..].chars().all(|c| c.is_ascii_digit()) => {
            other.to_ascii_uppercase()
        }
        other => {
            let mut chars = other.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        }
    }
}

pub fn format_combo_chips(combo: &str) -> Vec<String> {
    let norm = normalize_combo(combo);
    if norm.is_empty() {
        return Vec::new();
    }
    let mut parts = Vec::new();
    if let Some(prefix) = norm.strip_suffix("--") {
        for t in prefix.split('-').filter(|s| !s.is_empty()) {
            parts.push(chip_label_for_token(t));
        }
        parts.push("-".to_owned());
    } else if norm == "-" {
        parts.push("-".to_owned());
    } else {
        for t in norm.split('-').filter(|s| !s.is_empty()) {
            parts.push(chip_label_for_token(t));
        }
    }
    parts
}

pub fn format_combo_display(combo: &str) -> String {
    format_combo_chips(combo).join(" + ")
}

pub fn chips_for_default_spec(spec: &DefaultKeybindSpec) -> Vec<String> {
    match spec.display_kind {
        DefaultDisplayKind::AnyKey => vec!["Any key".to_owned()],
        DefaultDisplayKind::SpaceOrEnter => vec!["Space".to_owned(), "Enter".to_owned()],
        DefaultDisplayKind::Normal => format_combo_chips(spec.combo),
    }
}

pub fn is_builtin_action_overridden(prefs: &Prefs, action: &str) -> bool {
    prefs.custom_keybinds.iter().any(|entry| {
        entry.enabled
            && entry.action.as_deref() == Some(action)
            && !normalize_combo(&entry.combo).is_empty()
    })
}

pub fn is_builtin_action_active(prefs: &Prefs, action: &str) -> bool {
    if prefs.disable_builtin_keybinds {
        return false;
    }
    if prefs.disabled_builtin_actions.iter().any(|a| a == action) {
        return false;
    }
    if is_builtin_action_overridden(prefs, action) {
        return false;
    }
    true
}

pub fn matching_custom_action<'a>(prefs: &'a Prefs, combo: &str) -> Option<&'a str> {
    let target = normalize_combo(combo);
    if target.is_empty() {
        return None;
    }
    prefs.custom_keybinds.iter().find_map(|entry| {
        if !entry.enabled {
            return None;
        }
        let action = entry.action.as_deref()?;
        if combos_equal(&entry.combo, &target) {
            Some(action)
        } else {
            None
        }
    })
}

/// Detects conflicts among enabled custom keybinds sharing the same combo.
/// Maps `entry.id` -> conflicting peer action label (or `"(unassigned)"`).
pub fn find_custom_conflicts(entries: &[CustomKeybindEntry]) -> HashMap<String, String> {
    let mut conflicts = HashMap::new();
    for (i, target) in entries.iter().enumerate() {
        if !target.enabled || normalize_combo(&target.combo).is_empty() {
            continue;
        }
        for (j, other) in entries.iter().enumerate() {
            if i == j || !other.enabled || normalize_combo(&other.combo).is_empty() {
                continue;
            }
            if combos_equal(&target.combo, &other.combo) {
                let label = other
                    .action
                    .as_deref()
                    .and_then(action_label)
                    .or(other.action.as_deref())
                    .unwrap_or("(unassigned)")
                    .to_owned();
                conflicts.insert(target.id.clone(), label);
                break;
            }
        }
    }
    conflicts
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
        assert_eq!(seen.len(), 1401);
    }

    #[test]
    fn normalize_and_format_combos() {
        assert_eq!(normalize_combo("Shift+Ctrl+M"), "ctrl-shift-m");
        assert_eq!(normalize_combo("ctrl--"), "ctrl--");
        assert_eq!(normalize_combo("alt-ArrowUp"), "alt-up");
        assert_eq!(normalize_combo("ctrl"), "");
        assert!(combos_equal("Ctrl+Shift+M", "shift-ctrl-m"));
        assert!(!combos_equal("", ""));
        assert_eq!(format_combo_chips("ctrl-shift-m"), vec!["Ctrl", "Shift", "M"]);
        assert_eq!(format_combo_chips("alt-up"), vec!["Alt", "▲"]);
        assert_eq!(format_combo_chips("ctrl--"), vec!["Ctrl", "-"]);
        assert_eq!(format_combo_display("ctrl-k"), "Ctrl + K");
    }

    #[test]
    fn custom_resolution_override_and_conflicts() {
        let mut prefs = Prefs::default();
        assert!(is_builtin_action_active(&prefs, "nav_quick_switcher"));

        prefs.disabled_builtin_actions.push("nav_quick_switcher".to_owned());
        assert!(!is_builtin_action_active(&prefs, "nav_quick_switcher"));
        prefs.disabled_builtin_actions.clear();

        prefs.disable_builtin_keybinds = true;
        assert!(!is_builtin_action_active(&prefs, "nav_quick_switcher"));
        prefs.disable_builtin_keybinds = false;

        prefs.custom_keybinds.push(CustomKeybindEntry {
            id: "kb-1".to_owned(),
            action: Some("nav_quick_switcher".to_owned()),
            combo: "alt-j".to_owned(),
            enabled: true,
        });
        assert!(!is_builtin_action_active(&prefs, "nav_quick_switcher"));
        assert_eq!(matching_custom_action(&prefs, "Alt+J"), Some("nav_quick_switcher"));

        prefs.custom_keybinds.push(CustomKeybindEntry {
            id: "kb-2".to_owned(),
            action: Some("misc_search".to_owned()),
            combo: "Alt+J".to_owned(),
            enabled: true,
        });
        let conflicts = find_custom_conflicts(&prefs.custom_keybinds);
        assert_eq!(conflicts.get("kb-1").map(String::as_str), Some("Search messages"));
        assert_eq!(conflicts.get("kb-2").map(String::as_str), Some("Toggle quick switcher"));

        // Disabling one entry clears the conflict.
        prefs.custom_keybinds[1].enabled = false;
        assert!(find_custom_conflicts(&prefs.custom_keybinds).is_empty());
    }
}
