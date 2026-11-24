//! Community settings modals launched from the guild header popover:
//! Notification settings, Privacy settings, Community profile editor,
//! and Bulk-delete my messages confirmation.

use fluxer_client_core::rest::{
    GuildMemberPatch, GuildSettingsPatch, MAX_MEMBER_BIO_UTF16, MAX_MEMBER_PRONOUNS_UTF16, MAX_NICKNAME_UTF16,
    UserSettingsPatch, validate_member_patch,
};
use fluxer_client_core::state::{ChannelKind, Snowflake, message_notifications};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Disableable as _, Icon, Selectable as _, Sizable as _};
use gpui_kit::{
    AnyElement, AppContext as _, Context, Entity, EntityInputHandler as _, FocusHandle, Focusable as _,
    FontWeight, Role, SharedString, Subscription, Window, div, prelude::*, px,
};

use crate::client_view::ClientView;
use crate::icons::AppIcon;
use crate::theme::Theme;
use crate::ui::overlay::Overlay;
use crate::ui::{l10n, overlay};

pub const NOTIFICATIONS_CONTEXT: &str = "GuildNotificationsModal";
pub const PRIVACY_CONTEXT: &str = "GuildPrivacyModal";
pub const PROFILE_CONTEXT: &str = "GuildProfileModal";
pub const DELETE_MESSAGES_CONTEXT: &str = "DeleteMyGuildMessagesModal";
pub const PURGE_NOTES_CONTEXT: &str = "PurgePersonalNotesModal";

pub struct GuildNotificationsModal {
    pub guild_id: Snowflake,
    pub add_override_open: bool,
    pub focus_handle: FocusHandle,
}

pub struct GuildPrivacyModal {
    pub guild_id: Snowflake,
    pub focus_handle: FocusHandle,
}

pub struct GuildProfileModal {
    pub seq: u64,
    pub guild_id: Snowflake,
    pub can_edit_nick: bool,
    pub initial_nick: String,
    pub initial_pronouns: String,
    pub initial_bio: String,
    pub nick_input: Entity<InputState>,
    pub pronouns_input: Entity<InputState>,
    pub bio_input: Entity<InputState>,
    pub submitting: bool,
    pub error: Option<String>,
    pub _subscriptions: Vec<Subscription>,
}

pub struct DeleteMyGuildMessagesModal {
    pub seq: u64,
    pub guild_id: Snowflake,
    pub guild_name: SharedString,
    pub password_input: Entity<InputState>,
    pub submitting: bool,
    pub error: Option<String>,
    pub _subscription: Subscription,
}

pub struct PurgePersonalNotesModal {
    pub seq: u64,
    pub channel_id: Snowflake,
    pub submitting: bool,
    pub error: Option<String>,
    pub focus_handle: FocusHandle,
}

fn channel_icon_for(kind: ChannelKind) -> AppIcon {
    match kind {
        ChannelKind::GuildCategory => AppIcon::CaretDown,
        ChannelKind::GuildVoice => AppIcon::VoiceChannel,
        ChannelKind::GuildAnnouncement => AppIcon::AnnouncementChannel,
        _ => AppIcon::Hash,
    }
}

fn toggle_restricted_list(mut current: Vec<Snowflake>, guild_id: Snowflake, allow_dms: bool) -> Vec<String> {
    if allow_dms {
        current.retain(|id| *id != guild_id);
    } else if !current.contains(&guild_id) {
        current.push(guild_id);
    }
    current.into_iter().map(|id| id.to_string()).collect()
}

fn trim_non_empty(s: &str) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

impl ClientView {
    pub(crate) fn open_guild_notifications(
        &mut self,
        guild_id: Snowflake,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.backend.state().guilds.contains_key(&guild_id) {
            return;
        }
        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        let focus_handle = cx.focus_handle();
        self.overlay = Overlay::GuildNotifications(GuildNotificationsModal {
            guild_id,
            add_override_open: false,
            focus_handle: focus_handle.clone(),
        });
        window.focus(&focus_handle, cx);
        cx.notify();
    }

    pub(crate) fn toggle_guild_muted(&mut self, guild_id: Snowflake, muted: bool, cx: &mut Context<Self>) {
        self.backend.update_guild_settings_fields(
            guild_id,
            GuildSettingsPatch {
                muted: Some(muted),
                mute_config: Some(None),
                ..Default::default()
            },
            "Could not update mute setting",
        );
        cx.notify();
    }

    pub(crate) fn set_guild_notification_level(&mut self, guild_id: Snowflake, level: u8, cx: &mut Context<Self>) {
        self.backend.update_guild_settings_fields(
            guild_id,
            GuildSettingsPatch {
                message_notifications: Some(level),
                ..Default::default()
            },
            "Could not update notification level",
        );
        cx.notify();
    }

    pub(crate) fn toggle_guild_suppress_everyone(&mut self, guild_id: Snowflake, suppress: bool, cx: &mut Context<Self>) {
        self.backend.update_guild_settings_fields(
            guild_id,
            GuildSettingsPatch {
                suppress_everyone: Some(suppress),
                ..Default::default()
            },
            "Could not update suppress everyone setting",
        );
        cx.notify();
    }

    pub(crate) fn toggle_guild_suppress_roles(&mut self, guild_id: Snowflake, suppress: bool, cx: &mut Context<Self>) {
        self.backend.update_guild_settings_fields(
            guild_id,
            GuildSettingsPatch {
                suppress_roles: Some(suppress),
                ..Default::default()
            },
            "Could not update suppress roles setting",
        );
        cx.notify();
    }

    pub(crate) fn toggle_notifications_add_override(&mut self, cx: &mut Context<Self>) {
        let Overlay::GuildNotifications(modal) = &mut self.overlay else { return };
        modal.add_override_open = !modal.add_override_open;
        cx.notify();
    }

    pub(crate) fn pick_notification_override(
        &mut self,
        guild_id: Snowflake,
        channel_id: Snowflake,
        cx: &mut Context<Self>,
    ) {
        self.backend
            .set_channel_notification_level(Some(guild_id), channel_id, message_notifications::ONLY_MENTIONS as u8);
        if let Overlay::GuildNotifications(modal) = &mut self.overlay {
            modal.add_override_open = false;
        }
        cx.notify();
    }

    pub(crate) fn open_guild_privacy(&mut self, guild_id: Snowflake, window: &mut Window, cx: &mut Context<Self>) {
        if !self.backend.state().guilds.contains_key(&guild_id) {
            return;
        }
        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        let focus_handle = cx.focus_handle();
        self.overlay = Overlay::GuildPrivacy(GuildPrivacyModal {
            guild_id,
            focus_handle: focus_handle.clone(),
        });
        window.focus(&focus_handle, cx);
        cx.notify();
    }

    pub(crate) fn set_guild_member_dms(&mut self, guild_id: Snowflake, allow: bool, cx: &mut Context<Self>) {
        let (member_dms_disabled, restricted) = {
            let state = self.backend.state();
            let Some(g) = state.guilds.get(&guild_id) else { return };
            (g.member_dms_disabled, state.user_settings.restricted_guilds())
        };
        if member_dms_disabled {
            return;
        }
        let next = toggle_restricted_list(restricted, guild_id, allow);
        self.backend.update_user_settings(UserSettingsPatch {
            restricted_guilds: Some(next),
            ..Default::default()
        });
        cx.notify();
    }

    pub(crate) fn set_guild_bot_dms(&mut self, guild_id: Snowflake, allow: bool, cx: &mut Context<Self>) {
        let bot_restricted = self.backend.state().user_settings.bot_restricted_guilds();
        let next = toggle_restricted_list(bot_restricted, guild_id, allow);
        self.backend.update_user_settings(UserSettingsPatch {
            bot_restricted_guilds: Some(next),
            ..Default::default()
        });
        cx.notify();
    }

    pub(crate) fn open_guild_profile(&mut self, guild_id: Snowflake, window: &mut Window, cx: &mut Context<Self>) {
        let now_ms = i64::try_from(crate::accounts::now_ms()).unwrap_or(i64::MAX);
        let (can_edit_nick, initial_nick, initial_pronouns, initial_bio) = {
            let state = self.backend.state();
            let Some(me) = state.current_user_id() else { return };
            if !state.guilds.contains_key(&guild_id) {
                return;
            }
            let can_edit_nick = state.can_edit_member_nickname(guild_id, me, now_ms);
            let member = state.members.get(&guild_id).and_then(|m| m.get(&me));
            let nick = member.and_then(|m| m.nick.clone()).unwrap_or_default();
            let pronouns = member.and_then(|m| m.pronouns.clone()).unwrap_or_default();
            let bio = member.and_then(|m| m.bio.clone()).unwrap_or_default();
            (can_edit_nick, nick, pronouns, bio)
        };

        self.prepare_overlay_replace(cx);
        self.autocomplete = None;

        let init_nick = initial_nick.clone();
        let init_pronouns = initial_pronouns.clone();
        let init_bio = initial_bio.clone();

        let nick_input = cx.new(|cx| {
            let mut input = InputState::new(window, cx).placeholder(l10n::tr("Community nickname"));
            if !init_nick.is_empty() {
                input.set_value(&init_nick, window, cx);
            }
            input
        });
        let pronouns_input = cx.new(|cx| {
            let mut input = InputState::new(window, cx).placeholder(l10n::tr("Pronouns"));
            if !init_pronouns.is_empty() {
                input.set_value(&init_pronouns, window, cx);
            }
            input
        });
        let bio_input = cx.new(|cx| {
            let mut input = InputState::new(window, cx).placeholder(l10n::tr("Bio"));
            if !init_bio.is_empty() {
                input.set_value(&init_bio, window, cx);
            }
            input
        });

        let sub_nick = cx.subscribe_in(&nick_input, window, |this: &mut Self, ent, ev: &InputEvent, window, cx| match ev {
            InputEvent::PressEnter { shift: false, .. } => {
                if ent.update(cx, |s, cx| s.marked_text_range(window, cx)).is_some() {
                    return;
                }
                this.submit_guild_profile(window, cx);
            }
            InputEvent::Change => {
                if let Overlay::GuildProfile(modal) = &mut this.overlay {
                    modal.error = None;
                    cx.notify();
                }
            }
            _ => {}
        });
        let sub_pronouns =
            cx.subscribe_in(&pronouns_input, window, |this: &mut Self, ent, ev: &InputEvent, window, cx| match ev {
                InputEvent::PressEnter { shift: false, .. } => {
                    if ent.update(cx, |s, cx| s.marked_text_range(window, cx)).is_some() {
                        return;
                    }
                    this.submit_guild_profile(window, cx);
                }
                InputEvent::Change => {
                    if let Overlay::GuildProfile(modal) = &mut this.overlay {
                        modal.error = None;
                        cx.notify();
                    }
                }
                _ => {}
            });
        let sub_bio = cx.subscribe_in(&bio_input, window, |this: &mut Self, _, ev: &InputEvent, _, cx| {
            if matches!(ev, InputEvent::Change)
                && let Overlay::GuildProfile(modal) = &mut this.overlay
            {
                modal.error = None;
                cx.notify();
            }
        });

        let focus = if can_edit_nick {
            nick_input.read(cx).focus_handle(cx)
        } else {
            pronouns_input.read(cx).focus_handle(cx)
        };
        let seq = self.next_moderation_seq();
        self.overlay = Overlay::GuildProfile(GuildProfileModal {
            seq,
            guild_id,
            can_edit_nick,
            initial_nick,
            initial_pronouns,
            initial_bio,
            nick_input,
            pronouns_input,
            bio_input,
            submitting: false,
            error: None,
            _subscriptions: vec![sub_nick, sub_pronouns, sub_bio],
        });
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(crate) fn submit_guild_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(me) = self.backend.state().current_user_id() else { return };
        let (guild_id, patch, validation_error, unchanged) = {
            let Overlay::GuildProfile(modal) = &mut self.overlay else { return };
            if modal.submitting {
                return;
            }
            let next_nick = trim_non_empty(&modal.nick_input.read(cx).value());
            let next_pronouns = trim_non_empty(&modal.pronouns_input.read(cx).value());
            let next_bio = trim_non_empty(&modal.bio_input.read(cx).value());

            let patch = GuildMemberPatch {
                nick: (modal.can_edit_nick && next_nick != trim_non_empty(&modal.initial_nick)).then_some(next_nick),
                pronouns: (next_pronouns != trim_non_empty(&modal.initial_pronouns)).then_some(next_pronouns),
                bio: (next_bio != trim_non_empty(&modal.initial_bio)).then_some(next_bio),
                ..Default::default()
            };

            if patch.is_empty() {
                modal.error = None;
                (modal.guild_id, patch, None, true)
            } else {
                let err = match validate_member_patch(&patch) {
                    Ok(()) => None,
                    Err(_) => Some(profile_validation_error(&patch)),
                };
                (modal.guild_id, patch, err, false)
            }
        };

        if unchanged {
            self.close_overlay(window, cx);
            return;
        }

        if let Some(err) = validation_error {
            if let Overlay::GuildProfile(modal) = &mut self.overlay {
                modal.error = Some(err);
                cx.notify();
            }
            return;
        }

        let seq = self.next_moderation_seq();
        if let Overlay::GuildProfile(modal) = &mut self.overlay {
            modal.seq = seq;
            modal.submitting = true;
            modal.error = None;
        }
        self.backend
            .update_guild_member_moderation(seq, guild_id, me, true, patch, None);
        cx.notify();
    }

    pub(crate) fn open_delete_my_guild_messages(
        &mut self,
        guild_id: Snowflake,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let guild_name: SharedString = {
            let state = self.backend.state();
            let Some(guild) = state.guilds.get(&guild_id) else { return };
            if state.current_user_id().is_some() && guild.owner_id == state.current_user_id() {
                return;
            }
            guild.name.clone().into()
        };

        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        let seq = self.next_guild_delete_seq();
        let password_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(l10n::tr("Password (if required)"))
                .masked(true)
        });
        let sub = cx.subscribe_in(&password_input, window, |this: &mut Self, ent, ev: &InputEvent, window, cx| {
            match ev {
                InputEvent::PressEnter { shift: false, .. } => {
                    if ent.update(cx, |s, cx| s.marked_text_range(window, cx)).is_some() {
                        return;
                    }
                    this.submit_delete_my_guild_messages(cx);
                }
                InputEvent::Change => {
                    if let Overlay::DeleteMyGuildMessages(modal) = &mut this.overlay {
                        modal.error = None;
                        cx.notify();
                    }
                }
                _ => {}
            }
        });
        let focus = password_input.read(cx).focus_handle(cx);
        self.overlay = Overlay::DeleteMyGuildMessages(DeleteMyGuildMessagesModal {
            seq,
            guild_id,
            guild_name,
            password_input,
            submitting: false,
            error: None,
            _subscription: sub,
        });
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(crate) fn submit_delete_my_guild_messages(&mut self, cx: &mut Context<Self>) {
        let seq = self.next_guild_delete_seq();
        let (guild_id, password) = {
            let Overlay::DeleteMyGuildMessages(modal) = &mut self.overlay else { return };
            if modal.submitting {
                return;
            }
            modal.seq = seq;
            modal.submitting = true;
            modal.error = None;
            let raw = modal.password_input.read(cx).value().to_string();
            let password = (!raw.trim().is_empty()).then_some(raw);
            (modal.guild_id, password)
        };
        self.backend.bulk_delete_my_guild_messages(seq, guild_id, password);
        cx.notify();
    }

    pub(crate) fn open_purge_personal_notes(
        &mut self,
        channel_id: Snowflake,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        let seq = self.next_purge_notes_seq();
        let focus_handle = cx.focus_handle();
        self.overlay = Overlay::PurgePersonalNotes(PurgePersonalNotesModal {
            seq,
            channel_id,
            submitting: false,
            error: None,
            focus_handle: focus_handle.clone(),
        });
        window.focus(&focus_handle, cx);
        cx.notify();
    }

    pub(crate) fn submit_purge_personal_notes(&mut self, cx: &mut Context<Self>) {
        let seq = self.next_purge_notes_seq();
        let channel_id = {
            let Overlay::PurgePersonalNotes(modal) = &mut self.overlay else { return };
            if modal.submitting {
                return;
            }
            modal.seq = seq;
            modal.submitting = true;
            modal.error = None;
            modal.channel_id
        };
        self.backend.purge_personal_notes(seq, channel_id);
        cx.notify();
    }
}

fn profile_validation_error(patch: &GuildMemberPatch) -> String {
    if let Some(Some(nick)) = &patch.nick
        && nick.trim().encode_utf16().count() > MAX_NICKNAME_UTF16
    {
        return format!("Nickname must be at most {MAX_NICKNAME_UTF16} characters.");
    }
    if let Some(Some(pronouns)) = &patch.pronouns
        && pronouns.trim().encode_utf16().count() > MAX_MEMBER_PRONOUNS_UTF16
    {
        return format!("Pronouns must be at most {MAX_MEMBER_PRONOUNS_UTF16} characters.");
    }
    if let Some(Some(bio)) = &patch.bio
        && bio.trim().encode_utf16().count() > MAX_MEMBER_BIO_UTF16
    {
        return format!("Bio must be at most {MAX_MEMBER_BIO_UTF16} characters.");
    }
    "Invalid community profile update.".to_owned()
}

pub fn render_notifications(
    view: &ClientView,
    modal: &GuildNotificationsModal,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let guild_id = modal.guild_id;
    let (
        guild_name,
        muted,
        effective_level,
        suppress_everyone,
        suppress_roles,
        active_overrides,
        available_channels,
    ) = {
        let state = view.backend.state();
        let guild = state.guilds.get(&guild_id);
        let guild_name: SharedString = guild
            .map(|g| g.name.clone())
            .unwrap_or_else(|| "Community".to_owned())
            .into();
        let settings = state.guild_settings.get(&Some(guild_id));
        let muted = settings.is_some_and(|s| s.muted);
        let raw_level = settings
            .map(|s| s.message_notifications)
            .unwrap_or(message_notifications::ALL_MESSAGES);
        let effective_level = if raw_level == message_notifications::INHERIT || settings.is_none() {
            guild
                .map(|g| g.effective_message_notifications())
                .unwrap_or(message_notifications::ALL_MESSAGES)
        } else {
            raw_level
        };
        let suppress_everyone = settings.is_some_and(|s| s.suppress_everyone);
        let suppress_roles = settings.is_some_and(|s| s.suppress_roles);

        let mut guild_channels: Vec<_> = state
            .channels
            .values()
            .filter(|c| {
                c.guild_id == Some(guild_id)
                    && !c.kind.is_private()
                    && (c.kind.is_category() || state.can_view_channel(c.id))
            })
            .collect();
        guild_channels.sort_by(|a, b| a.position.cmp(&b.position).then_with(|| a.id.cmp(&b.id)));

        let mut active_overrides = Vec::new();
        let mut available_channels = Vec::new();
        for ch in guild_channels {
            let name = ch
                .name
                .clone()
                .unwrap_or_else(|| if ch.kind.is_category() { "Category" } else { "channel" }.to_owned());
            let ov = settings.and_then(|s| s.channel_overrides.get(&ch.id));
            let is_overridden = ov.is_some_and(|o| {
                o.muted
                    || o.message_notifications
                        .is_some_and(|lvl| lvl != message_notifications::INHERIT)
            });
            if is_overridden {
                let ov = ov.expect("checked overridden");
                active_overrides.push((
                    ch.id,
                    name,
                    ch.kind,
                    ov.muted,
                    ov.message_notifications.unwrap_or(message_notifications::INHERIT),
                ));
            } else {
                available_channels.push((ch.id, name, ch.kind));
            }
        }

        (
            guild_name,
            muted,
            effective_level,
            suppress_everyone,
            suppress_roles,
            active_overrides,
            available_channels,
        )
    };

    let levels = [
        (message_notifications::ALL_MESSAGES as u8, "All messages"),
        (message_notifications::ONLY_MENTIONS as u8, "Only @mentions"),
        (message_notifications::NO_MESSAGES as u8, "Nothing"),
    ];

    let mut level_buttons = div().flex().flex_row().flex_wrap().gap_1();
    for (lvl, label) in levels {
        let selected = effective_level == lvl as i8;
        let sel = format!("guild-notif-level-{lvl}");
        level_buttons = level_buttons.child(
            div().debug_selector(move || sel.clone()).child(
                Button::new(("guild-notif-level", lvl as u64))
                    .ghost()
                    .small()
                    .selected(selected)
                    .label(l10n::tr(label))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_guild_notification_level(guild_id, lvl, cx);
                    })),
            ),
        );
    }

    let mut overrides_col = div()
        .id("guild-notif-overrides-list")
        .max_h(px(180.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_1p5();

    if active_overrides.is_empty() {
        overrides_col = overrides_col.child(
            div()
                .py_2()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(l10n::tr("No channel or category overrides configured.")),
        );
    } else {
        for (cid, name, kind, ch_muted, ch_lvl) in active_overrides {
            let mut ch_lvl_row = div().flex().flex_row().items_center().gap_1();
            for (lvl, short_label) in [(0u8, "All"), (1u8, "@mentions"), (2u8, "Nothing")] {
                let selected = ch_lvl == lvl as i8;
                let sel = format!("guild-notif-ov-level-{}-{lvl}", cid.0);
                ch_lvl_row = ch_lvl_row.child(
                    div().debug_selector(move || sel.clone()).child(
                        Button::new(("guild-notif-ov-level", cid.0 ^ ((lvl as u64) << 56)))
                            .ghost()
                            .small()
                            .selected(selected)
                            .label(short_label)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.backend.set_channel_notification_level(Some(guild_id), cid, lvl);
                                cx.notify();
                            })),
                    ),
                );
            }
            let mute_sel = format!("guild-notif-ov-mute-{}", cid.0);
            let rm_sel = format!("guild-notif-ov-remove-{}", cid.0);
            overrides_col = overrides_col.child(
                div()
                    .id(("guild-notif-ov", cid.0))
                    .p_2()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.background)
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_1p5()
                                    .min_w_0()
                                    .child(
                                        Icon::new(channel_icon_for(kind))
                                            .with_size(px(14.))
                                            .text_color(theme.muted_foreground),
                                    )
                                    .child(div().truncate().text_sm().font_weight(FontWeight::MEDIUM).child(name)),
                            )
                            .child(
                                div().debug_selector(move || rm_sel.clone()).child(
                                    Button::new(("guild-notif-ov-remove", cid.0))
                                        .ghost()
                                        .small()
                                        .icon(AppIcon::X)
                                        .accessibility_label(l10n::tr("Remove override"))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.backend.remove_channel_override(guild_id, cid);
                                            cx.notify();
                                        })),
                                ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(ch_lvl_row)
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_1p5()
                                    .child(div().text_xs().text_color(theme.muted_foreground).child(l10n::tr("Mute")))
                                    .child(
                                        div().debug_selector(move || mute_sel.clone()).child(
                                            Switch::new(("guild-notif-ov-mute", cid.0))
                                                .checked(ch_muted)
                                                .on_change(cx.listener(move |this, checked: &bool, _, cx| {
                                                    this.backend.set_channel_muted(Some(guild_id), cid, *checked);
                                                    cx.notify();
                                                })),
                                        ),
                                    ),
                            ),
                    ),
            );
        }
    }

    let picker = modal.add_override_open.then(|| {
        let mut col = div()
            .id("guild-notif-picker-list")
            .max_h(px(140.))
            .overflow_y_scroll()
            .p_1p5()
            .rounded(px(6.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .flex()
            .flex_col()
            .gap_1();
        if available_channels.is_empty() {
            col = col.child(
                div()
                    .p_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(l10n::tr("All visible channels already have overrides.")),
            );
        } else {
            for (cid, name, kind) in available_channels {
                let pick_sel = format!("guild-notif-pick-{}", cid.0);
                col = col.child(
                    div()
                        .id(("guild-notif-pick", cid.0))
                        .debug_selector(move || pick_sel.clone())
                        .role(Role::Button)
                        .px_2()
                        .py_1()
                        .rounded(px(4.))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.hover_bg))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1p5()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.pick_notification_override(guild_id, cid, cx);
                        }))
                        .child(
                            Icon::new(channel_icon_for(kind))
                                .with_size(px(13.))
                                .text_color(theme.muted_foreground),
                        )
                        .child(div().text_xs().font_weight(FontWeight::MEDIUM).truncate().child(name)),
                );
            }
        }
        col
    });

    let content = div()
        .id("guild-notifications-modal")
        .debug_selector(|| "guild-notifications-modal".to_owned())
        .track_focus(&modal.focus_handle)
        .key_context(NOTIFICATIONS_CONTEXT)
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .capture_action(cx.listener(|this, _: &Escape, window, cx| {
            cx.stop_propagation();
            this.close_overlay(window, cx);
        }))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .child(div().text_lg().font_weight(FontWeight::BOLD).child(l10n::tr("Notification settings")))
                        .child(div().text_xs().text_color(theme.muted_foreground).child(guild_name)),
                )
                .child(
                    div().debug_selector(|| "guild-notif-close-btn".to_owned()).child(
                        Button::new("guild-notif-close-btn")
                            .ghost()
                            .small()
                            .icon(AppIcon::X)
                            .accessibility_label(l10n::tr("Close"))
                            .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
                    ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .min_w_0()
                        .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(l10n::tr("Mute community")))
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(l10n::tr("Prevents unread indicators and notifications across channels.")),
                        ),
                )
                .child(
                    div().debug_selector(|| "guild-notif-mute-switch".to_owned()).child(
                        Switch::new("guild-notif-mute-switch")
                            .checked(muted)
                            .on_change(cx.listener(move |this, checked: &bool, _, cx| {
                                this.toggle_guild_muted(guild_id, *checked, cx);
                            })),
                    ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted_foreground)
                        .child(l10n::tr("Community notification settings")),
                )
                .child(level_buttons),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_3()
                .child(div().text_sm().child(l10n::tr("Suppress @everyone and @here")))
                .child(
                    div().debug_selector(|| "guild-notif-suppress-everyone".to_owned()).child(
                        Switch::new("guild-notif-suppress-everyone")
                            .checked(suppress_everyone)
                            .on_change(cx.listener(move |this, checked: &bool, _, cx| {
                                this.toggle_guild_suppress_everyone(guild_id, *checked, cx);
                            })),
                    ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_3()
                .child(div().text_sm().child(l10n::tr("Suppress all role @mentions")))
                .child(
                    div().debug_selector(|| "guild-notif-suppress-roles".to_owned()).child(
                        Switch::new("guild-notif-suppress-roles")
                            .checked(suppress_roles)
                            .on_change(cx.listener(move |this, checked: &bool, _, cx| {
                                this.toggle_guild_suppress_roles(guild_id, *checked, cx);
                            })),
                    ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1p5()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.muted_foreground)
                                .child(l10n::tr("Notification overrides")),
                        )
                        .child(
                            div().debug_selector(|| "guild-notif-add-override-btn".to_owned()).child(
                                Button::new("guild-notif-add-override-btn")
                                    .ghost()
                                    .small()
                                    .label(l10n::tr("Add channel or category"))
                                    .on_click(cx.listener(|this, _, _, cx| this.toggle_notifications_add_override(cx))),
                            ),
                        ),
                )
                .children(picker)
                .child(overrides_col),
        );

    let view = cx.weak_entity();
    overlay::modal(content, theme, move |window, cx| {
        let _ = view.update(cx, |v, cx| v.close_overlay(window, cx));
    })
}

pub fn render_privacy(
    view: &ClientView,
    modal: &GuildPrivacyModal,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let guild_id = modal.guild_id;
    let (guild_name, member_dms_disabled, allow_member_dms, allow_bot_dms) = {
        let state = view.backend.state();
        let guild = state.guilds.get(&guild_id);
        let guild_name: SharedString = guild
            .map(|g| g.name.clone())
            .unwrap_or_else(|| "Community".to_owned())
            .into();
        let disabled = guild.is_some_and(|g| g.member_dms_disabled);
        let restricted = state.user_settings.restricted_guilds();
        let bot_restricted = state.user_settings.bot_restricted_guilds();
        (
            guild_name,
            disabled,
            !disabled && !restricted.contains(&guild_id),
            !bot_restricted.contains(&guild_id),
        )
    };

    let content = div()
        .id("guild-privacy-modal")
        .debug_selector(|| "guild-privacy-modal".to_owned())
        .track_focus(&modal.focus_handle)
        .key_context(PRIVACY_CONTEXT)
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .capture_action(cx.listener(|this, _: &Escape, window, cx| {
            cx.stop_propagation();
            this.close_overlay(window, cx);
        }))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .child(div().text_lg().font_weight(FontWeight::BOLD).child(l10n::tr("Privacy settings")))
                        .child(div().text_xs().text_color(theme.muted_foreground).child(guild_name)),
                )
                .child(
                    div().debug_selector(|| "guild-privacy-close-btn".to_owned()).child(
                        Button::new("guild-privacy-close-btn")
                            .ghost()
                            .small()
                            .icon(AppIcon::X)
                            .accessibility_label(l10n::tr("Close"))
                            .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
                    ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .min_w_0()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .child(l10n::tr("Direct messages")),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(if member_dms_disabled {
                                    l10n::tr("Direct messages are disabled by this community.")
                                } else {
                                    l10n::tr("Allow direct messages from other members in this community.")
                                }),
                        ),
                )
                .child(
                    div().debug_selector(|| "guild-privacy-dms-switch".to_owned()).child(
                        Switch::new("guild-privacy-dms-switch")
                            .checked(allow_member_dms)
                            .disabled(member_dms_disabled)
                            .on_change(cx.listener(move |this, checked: &bool, _, cx| {
                                this.set_guild_member_dms(guild_id, *checked, cx);
                            })),
                    ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .min_w_0()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .child(l10n::tr("Bot direct messages")),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(l10n::tr("Allow bots and applications in this community to message you.")),
                        ),
                )
                .child(
                    div().debug_selector(|| "guild-privacy-bot-dms-switch".to_owned()).child(
                        Switch::new("guild-privacy-bot-dms-switch")
                            .checked(allow_bot_dms)
                            .on_change(cx.listener(move |this, checked: &bool, _, cx| {
                                this.set_guild_bot_dms(guild_id, *checked, cx);
                            })),
                    ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .child(
                    div().debug_selector(|| "guild-privacy-done-btn".to_owned()).child(
                        Button::new("guild-privacy-done-btn")
                            .primary()
                            .small()
                            .label(l10n::tr("Done"))
                            .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
                    ),
                ),
        );

    let view = cx.weak_entity();
    overlay::modal(content, theme, move |window, cx| {
        let _ = view.update(cx, |v, cx| v.close_overlay(window, cx));
    })
}

pub fn render_profile(modal: &GuildProfileModal, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let content = div()
        .id("guild-profile-modal")
        .debug_selector(|| "guild-profile-modal".to_owned())
        .key_context(PROFILE_CONTEXT)
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .capture_action(cx.listener(|this, _: &Escape, window, cx| {
            cx.stop_propagation();
            this.close_overlay(window, cx);
        }))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(div().text_lg().font_weight(FontWeight::BOLD).child(l10n::tr("Edit community profile")))
                .child(
                    div().debug_selector(|| "guild-profile-close-btn".to_owned()).child(
                        Button::new("guild-profile-close-btn")
                            .ghost()
                            .small()
                            .icon(AppIcon::X)
                            .accessibility_label(l10n::tr("Close"))
                            .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
                    ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted_foreground)
                        .child(l10n::tr("Community nickname")),
                )
                .child(
                    div().debug_selector(|| "guild-profile-nick-input".to_owned()).child(
                        Input::new(&modal.nick_input)
                            .small()
                            .disabled(modal.submitting || !modal.can_edit_nick),
                    ),
                )
                .when(!modal.can_edit_nick, |d| {
                    d.child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(l10n::tr("You do not have permission to change your nickname in this community.")),
                    )
                }),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted_foreground)
                        .child(l10n::tr("Pronouns")),
                )
                .child(
                    div()
                        .debug_selector(|| "guild-profile-pronouns-input".to_owned())
                        .child(Input::new(&modal.pronouns_input).small().disabled(modal.submitting)),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted_foreground)
                        .child(l10n::tr("Bio")),
                )
                .child(
                    div()
                        .debug_selector(|| "guild-profile-bio-input".to_owned())
                        .child(Input::new(&modal.bio_input).small().disabled(modal.submitting)),
                ),
        )
        .when_some(modal.error.clone(), |d, err| {
            d.child(div().text_xs().text_color(theme.danger).child(err))
        })
        .child(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .gap_2()
                .child(
                    div().debug_selector(|| "guild-profile-cancel-btn".to_owned()).child(
                        Button::new("guild-profile-cancel-btn")
                            .ghost()
                            .small()
                            .disabled(modal.submitting)
                            .label(l10n::tr("Cancel"))
                            .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
                    ),
                )
                .child(
                    div().debug_selector(|| "guild-profile-save-btn".to_owned()).child(
                        Button::new("guild-profile-save-btn")
                            .primary()
                            .small()
                            .disabled(modal.submitting)
                            .label(if modal.submitting {
                                l10n::tr("Saving…")
                            } else {
                                l10n::tr("Save")
                            })
                            .on_click(cx.listener(|this, _, window, cx| this.submit_guild_profile(window, cx))),
                    ),
                ),
        );

    let view = cx.weak_entity();
    overlay::modal(content, theme, move |window, cx| {
        let _ = view.update(cx, |v, cx| v.close_overlay(window, cx));
    })
}

pub fn render_delete_messages(
    modal: &DeleteMyGuildMessagesModal,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let body = l10n::tr_args(
        "Are you sure you want to delete all of your messages in {guildName}? This cannot be undone.",
        &[("guildName", modal.guild_name.as_ref())],
    );

    let content = div()
        .id("guild-delete-messages-modal")
        .debug_selector(|| "guild-delete-messages-modal".to_owned())
        .key_context(DELETE_MESSAGES_CONTEXT)
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .capture_action(cx.listener(|this, _: &Escape, window, cx| {
            cx.stop_propagation();
            this.close_overlay(window, cx);
        }))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(div().text_lg().font_weight(FontWeight::BOLD).child(l10n::tr("Delete your messages")))
                .child(
                    div().debug_selector(|| "guild-delete-messages-close-btn".to_owned()).child(
                        Button::new("guild-delete-messages-close-btn")
                            .ghost()
                            .small()
                            .icon(AppIcon::X)
                            .accessibility_label(l10n::tr("Close"))
                            .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
                    ),
                ),
        )
        .child(div().text_sm().text_color(theme.muted_foreground).child(body))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted_foreground)
                        .child(l10n::tr("Password (if required)")),
                )
                .child(
                    div()
                        .debug_selector(|| "guild-delete-messages-password-input".to_owned())
                        .child(
                            Input::new(&modal.password_input)
                                .small()
                                .mask_toggle()
                                .disabled(modal.submitting),
                        ),
                ),
        )
        .when_some(modal.error.clone(), |d, err| {
            d.child(div().text_xs().text_color(theme.danger).child(err))
        })
        .child(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .gap_2()
                .child(
                    div().debug_selector(|| "guild-delete-messages-cancel-btn".to_owned()).child(
                        Button::new("guild-delete-messages-cancel-btn")
                            .ghost()
                            .small()
                            .disabled(modal.submitting)
                            .label(l10n::tr("Cancel"))
                            .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
                    ),
                )
                .child(
                    div().debug_selector(|| "guild-delete-messages-confirm-btn".to_owned()).child(
                        Button::new("guild-delete-messages-confirm-btn")
                            .danger()
                            .small()
                            .disabled(modal.submitting)
                            .label(if modal.submitting {
                                l10n::tr("Deleting…")
                            } else {
                                l10n::tr("Delete my messages")
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.submit_delete_my_guild_messages(cx))),
                    ),
                ),
        );

    let view = cx.weak_entity();
    overlay::modal(content, theme, move |window, cx| {
        let _ = view.update(cx, |v, cx| v.close_overlay(window, cx));
    })
}

pub fn render_purge_personal_notes(
    modal: &PurgePersonalNotesModal,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let content = div()
        .id("purge-personal-notes-modal")
        .debug_selector(|| "purge-personal-notes-modal".to_owned())
        .track_focus(&modal.focus_handle)
        .key_context(PURGE_NOTES_CONTEXT)
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .capture_action(cx.listener(|this, _: &Escape, window, cx| {
            cx.stop_propagation();
            this.close_overlay(window, cx);
        }))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(div().text_lg().font_weight(FontWeight::BOLD).child(l10n::tr("Purge personal notes")))
                .child(
                    div().debug_selector(|| "purge-personal-notes-close-btn".to_owned()).child(
                        Button::new("purge-personal-notes-close-btn")
                            .ghost()
                            .small()
                            .icon(AppIcon::X)
                            .accessibility_label(l10n::tr("Close"))
                            .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
                    ),
                ),
        )
        .child(
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(l10n::tr(
                    "Are you sure you want to delete every message in your personal notes? This cannot be undone.",
                )),
        )
        .when_some(modal.error.clone(), |d, err| {
            d.child(div().text_xs().text_color(theme.danger).child(err))
        })
        .child(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .gap_2()
                .child(
                    div().debug_selector(|| "purge-personal-notes-cancel-btn".to_owned()).child(
                        Button::new("purge-personal-notes-cancel-btn")
                            .ghost()
                            .small()
                            .disabled(modal.submitting)
                            .label(l10n::tr("Cancel"))
                            .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
                    ),
                )
                .child(
                    div().debug_selector(|| "purge-personal-notes-confirm-btn".to_owned()).child(
                        Button::new("purge-personal-notes-confirm-btn")
                            .danger()
                            .small()
                            .disabled(modal.submitting)
                            .label(if modal.submitting {
                                l10n::tr("Purging…")
                            } else {
                                l10n::tr("Purge personal notes")
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.submit_purge_personal_notes(cx))),
                    ),
                ),
        );

    let view = cx.weak_entity();
    overlay::modal(content, theme, move |window, cx| {
        let _ = view.update(cx, |v, cx| v.close_overlay(window, cx));
    })
}
