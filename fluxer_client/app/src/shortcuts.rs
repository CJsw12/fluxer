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

use gpui_kit::{Action, App, Context, Div, KeyBinding, Keystroke, NoAction, prelude::*};

use crate::client_view::ClientView;
use crate::nav::ChannelFilter;
use crate::prefs::{CustomKeybindEntry, Prefs};
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
    out.push(KeyBinding::new(keys, NoAction, Some(explore::PREVIEW_CONTEXT)));
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
    out.push(KeyBinding::new("ctrl-k", NoAction, Some(explore::PREVIEW_CONTEXT)));
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
            } else if v.should_run_builtin_shortcut("chat_mark_channel_read") {
                v.mark_channel_read(cx);
            }
        }))
        .on_action(cx.listener(|v, _: &MarkGuildRead, _, cx| {
            if v.should_run_builtin_shortcut("chat_mark_guild_read") {
                v.mark_guild_read(cx);
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
            Self::Chat => "Messages & Media",
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
            "Show keyboard shortcuts list",
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
            Single(spec("nav_add_guild", "Create or join a community", "ctrl-shift-n", S::Navigation)),
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
                combo: "space",
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
            Single(spec("chat_toggle_pins", "Toggle pinned messages", "ctrl-p", S::Chat)),
            Single(spec("chat_toggle_member_list", "Toggle the member list or voice chat", "ctrl-u", S::Chat)),
            Pair(
                spec("chat_scroll_up", "Scroll chat up", "pageup", S::Chat),
                spec("chat_scroll_down", "Scroll chat down", "pagedown", S::Chat),
            ),
            Single(spec("chat_mark_channel_read", "Mark channel as read", "escape", S::Chat)),
            Single(spec("chat_mark_guild_read", "Mark community as read", "shift-escape", S::Chat)),
        ],
        S::VoiceAndVideo => vec![
            Single(spec("voice_toggle_mute", "Toggle mute", "ctrl-shift-m", S::VoiceAndVideo)),
            Single(spec("voice_toggle_deafen", "Toggle deafen", "ctrl-shift-d", S::VoiceAndVideo)),
            Single(spec("voice_answer_call", "Answer the incoming call", "ctrl-enter", S::VoiceAndVideo)),
            Single(spec("voice_decline_call", "Decline the incoming call", "escape", S::VoiceAndVideo)),
        ],
        S::Accounts => vec![
            Single(spec("system_open_account_switcher", "Open account switcher", "ctrl-shift-a", S::Accounts)),
            Pair(
                spec("system_switch_account_prev", "Switch to previous account", "ctrl-alt-[", S::Accounts),
                spec("system_switch_account_next", "Switch to next account", "ctrl-alt-]", S::Accounts),
            ),
        ],
        S::Misc => vec![
            Single(spec("misc_search", "Search messages", "ctrl-f", S::Misc)),
            Single(spec("misc_open_context_menu", "Open the context menu", "shift-f10", S::Misc)),
            Single(spec("system_toggle_settings", "Open your settings", "ctrl-,", S::Misc)),
            Single(spec("system_zoom_in", "Zoom in", "ctrl-=", S::Misc)),
            Single(spec("system_zoom_out", "Zoom out", "ctrl--", S::Misc)),
            Single(spec("system_zoom_reset", "Reset zoom", "ctrl-0", S::Misc)),
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
    ("nav_history_back", "Channel history back"),
    ("nav_history_forward", "Channel history forward"),
    ("nav_toggle_last_guild_dms", "Toggle last community and DMs"),
    ("nav_add_guild", "Create or join a community"),
    ("misc_search", "Search messages"),
    ("chat_toggle_emoji", "Toggle the emoji picker"),
    ("chat_mark_channel_read", "Mark channel as read"),
    ("chat_mark_guild_read", "Mark community as read"),
    ("voice_toggle_mute", "Toggle mute"),
    ("voice_toggle_deafen", "Toggle deafen"),
    ("system_toggle_settings", "Open your settings"),
    ("system_zoom_in", "Zoom in"),
    ("system_zoom_out", "Zoom out"),
    ("system_zoom_reset", "Reset zoom"),
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
    None
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
/// into canonical modifier order `"ctrl-alt-shift-meta-<key>"`.
pub fn normalize_combo(raw: &str) -> String {
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
    if alt {
        parts.push("alt");
    }
    if shift {
        parts.push("shift");
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
    if ks.modifiers.alt {
        parts.push("alt");
    }
    if ks.modifiers.shift {
        parts.push("shift");
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
        "backspace" => "Backspace".to_owned(),
        "delete" => "Del".to_owned(),
        "up" => "↑".to_owned(),
        "down" => "↓".to_owned(),
        "left" => "←".to_owned(),
        "right" => "→".to_owned(),
        "pageup" => "PgUp".to_owned(),
        "pagedown" => "PgDn".to_owned(),
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
        assert_eq!(seen.len(), 391);
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
        assert_eq!(format_combo_chips("alt-up"), vec!["Alt", "↑"]);
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
