//! Guild member moderation (`Overlay::MemberModeration`: kick, ban, timeout,
//! nickname) and ban list management (`Overlay::GuildBans`).

use std::collections::{BTreeMap, BTreeSet};

use fluxer_client_core::rest::{
    BanMemberRequest, GuildBanEntry, GuildMemberPatch, MAX_AUDIT_LOG_REASON_UTF16, MAX_NICKNAME_UTF16,
    MAX_TIMEOUT_DURATION_SECONDS, RestError, validate_audit_log_reason, validate_ban_request, validate_member_patch,
};
use fluxer_client_core::state::Snowflake;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::{Disableable as _, Selectable as _, Sizable as _};
use gpui_kit::{
    AnyElement, AppContext as _, Context, Entity, EntityInputHandler as _, FocusHandle, Focusable as _, FontWeight,
    SharedString, Subscription, Window, div, prelude::*, px,
};

use crate::client_view::ClientView;
use crate::theme::Theme;
use crate::ui::overlay::Overlay;
use crate::ui::{l10n, overlay};

pub const ACTION_CONTEXT: &str = "MemberModerationModal";
pub const BANS_CONTEXT: &str = "GuildBansModal";

pub const BAN_DELETE_OPTIONS: &[(u32, &str)] = &[
    (0, "Don't delete any"),
    (3600, "1 hour"),
    (21600, "6 hours"),
    (86400, "24 hours"),
    (604800, "7 days"),
];

pub const BAN_DURATION_OPTIONS: &[(u32, &str)] = &[
    (0, "Permanent"),
    (3600, "1 hour"),
    (86400, "1 day"),
    (604800, "7 days"),
    (2_592_000, "30 days"),
];

pub const TIMEOUT_DURATION_OPTIONS: &[(u32, &str)] = &[
    (60, "60 secs"),
    (300, "5 mins"),
    (600, "10 mins"),
    (3600, "1 hour"),
    (86400, "1 day"),
    (604800, "1 week"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModerationKind {
    Kick,
    Ban,
    Timeout { currently_timed_out: bool },
    Nickname { is_self: bool, original_nick: String },
}

pub struct MemberModerationModal {
    pub seq: u64,
    pub guild_id: Snowflake,
    pub user_id: Snowflake,
    pub display_name: SharedString,
    pub kind: ModerationKind,
    pub ban_delete_seconds: u32,
    pub ban_duration_seconds: u32,
    pub timeout_seconds: u32,
    pub primary_input: Entity<InputState>,
    pub submitting: bool,
    pub error: Option<String>,
    pub focus: FocusHandle,
    pub _subscription: Subscription,
}

pub struct GuildBansModal {
    pub seq: u64,
    pub guild_id: Snowflake,
    pub filter_input: Entity<InputState>,
    pub bans: Vec<GuildBanEntry>,
    pub loading: bool,
    pub error: Option<String>,
    pub confirming_unban: Option<Snowflake>,
    pub unbanning: BTreeSet<Snowflake>,
    pub unban_seqs: BTreeMap<Snowflake, u64>,
    /// Tracks unban sequence numbers so an older in-flight `ListBans` response
    /// cannot re-add a member that was already unbanned more recently.
    pub recently_unbanned: BTreeMap<Snowflake, u64>,
    pub focus: FocusHandle,
    pub _subscription: Subscription,
}

fn friendly_moderation_error(err: RestError) -> String {
    match err {
        RestError::InvalidArgument("reason") => {
            format!("Reason must be at most {MAX_AUDIT_LOG_REASON_UTF16} characters.")
        }
        RestError::InvalidArgument("nick") => {
            format!("Nickname must be 1–{MAX_NICKNAME_UTF16} characters.")
        }
        other => other.to_string(),
    }
}

fn section_label(label: String, theme: &Theme) -> gpui_kit::Div {
    div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(label)
}

impl ClientView {
    pub(crate) fn open_member_moderation(
        &mut self,
        guild_id: Snowflake,
        user_id: Snowflake,
        kind: ModerationKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (allowed, display_name, resolved_kind) = {
            let state = self.backend.state();
            if !state.guilds.contains_key(&guild_id) {
                return;
            }
            let member = state.members.get(&guild_id).and_then(|m| m.get(&user_id));
            let name = member
                .and_then(|m| m.nick.as_deref().map(str::trim).filter(|n| !n.is_empty()))
                .or_else(|| member.map(|m| m.user.display_name()))
                .or_else(|| state.users.get(&user_id).map(|u| u.display_name()))
                .map(str::to_owned)
                .unwrap_or_else(|| user_id.to_string());
            let now = i64::try_from(crate::accounts::now_ms()).unwrap_or(i64::MAX);
            match kind {
                ModerationKind::Kick => (state.can_kick_member(guild_id, user_id), name, ModerationKind::Kick),
                ModerationKind::Ban => (state.can_ban_member(guild_id, user_id), name, ModerationKind::Ban),
                ModerationKind::Timeout { .. } => {
                    let timed_out = member.is_some_and(|m| m.is_timed_out(now));
                    (
                        state.can_timeout_member(guild_id, user_id),
                        name,
                        ModerationKind::Timeout { currently_timed_out: timed_out },
                    )
                }
                ModerationKind::Nickname { .. } => {
                    let is_self = state.current_user_id() == Some(user_id);
                    let original_nick = member
                        .and_then(|m| m.nick.as_deref())
                        .map(str::trim)
                        .unwrap_or_default()
                        .to_owned();
                    (
                        state.can_edit_member_nickname(guild_id, user_id, now),
                        name,
                        ModerationKind::Nickname { is_self, original_nick },
                    )
                }
            }
        };
        if !allowed {
            return;
        }

        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        let seq = self.next_moderation_seq();
        let initial_value = match &resolved_kind {
            ModerationKind::Nickname { original_nick, .. } => Some(original_nick.clone()),
            _ => None,
        };
        let placeholder = match &resolved_kind {
            ModerationKind::Nickname { .. } => l10n::tr("Nickname"),
            _ => l10n::tr("Reason"),
        };
        let primary_input = cx.new(|cx| {
            let mut input = InputState::new(window, cx).placeholder(placeholder);
            if let Some(val) = &initial_value
                && !val.is_empty()
            {
                input.set_value(val, window, cx);
            }
            input
        });
        let subscription = cx.subscribe_in(
            &primary_input,
            window,
            |this: &mut Self, input_entity, ev: &InputEvent, window, cx| match ev {
                InputEvent::PressEnter { shift: false, .. } => {
                    if input_entity.update(cx, |s, cx| s.marked_text_range(window, cx)).is_some() {
                        return;
                    }
                    this.submit_member_moderation(cx);
                }
                InputEvent::Change => {
                    if let Overlay::MemberModeration(modal) = &mut this.overlay {
                        modal.error = None;
                        cx.notify();
                    }
                }
                _ => {}
            },
        );
        let focus_input = primary_input.read(cx).focus_handle(cx);
        self.overlay = Overlay::MemberModeration(MemberModerationModal {
            seq,
            guild_id,
            user_id,
            display_name: display_name.into(),
            kind: resolved_kind,
            ban_delete_seconds: 86_400,
            ban_duration_seconds: 0,
            timeout_seconds: 300,
            primary_input,
            submitting: false,
            error: None,
            focus: cx.focus_handle(),
            _subscription: subscription,
        });
        window.focus(&focus_input, cx);
        cx.notify();
    }

    pub(crate) fn set_ban_delete_seconds(&mut self, secs: u32, cx: &mut Context<Self>) {
        let Overlay::MemberModeration(modal) = &mut self.overlay else { return };
        if modal.submitting || modal.kind != ModerationKind::Ban {
            return;
        }
        modal.ban_delete_seconds = secs;
        modal.error = None;
        cx.notify();
    }

    pub(crate) fn set_ban_duration_seconds(&mut self, secs: u32, cx: &mut Context<Self>) {
        let Overlay::MemberModeration(modal) = &mut self.overlay else { return };
        if modal.submitting || modal.kind != ModerationKind::Ban {
            return;
        }
        modal.ban_duration_seconds = secs;
        modal.error = None;
        cx.notify();
    }

    pub(crate) fn set_timeout_seconds(&mut self, secs: u32, cx: &mut Context<Self>) {
        let Overlay::MemberModeration(modal) = &mut self.overlay else { return };
        if modal.submitting || !matches!(modal.kind, ModerationKind::Timeout { .. }) {
            return;
        }
        modal.timeout_seconds = secs.clamp(60, MAX_TIMEOUT_DURATION_SECONDS);
        modal.error = None;
        cx.notify();
    }

    pub(crate) fn submit_member_moderation(&mut self, cx: &mut Context<Self>) {
        let (guild_id, user_id, kind, ban_del, ban_dur, timeout_secs, trimmed) = {
            let Overlay::MemberModeration(modal) = &mut self.overlay else { return };
            if modal.submitting {
                return;
            }
            let raw = modal.primary_input.read(cx).value().to_string();
            let trimmed = raw.trim().to_owned();
            let allowed = {
                let state = self.backend.state();
                let now = i64::try_from(crate::accounts::now_ms()).unwrap_or(i64::MAX);
                match &modal.kind {
                    ModerationKind::Kick => state.can_kick_member(modal.guild_id, modal.user_id),
                    ModerationKind::Ban => state.can_ban_member(modal.guild_id, modal.user_id),
                    ModerationKind::Timeout { .. } => state.can_timeout_member(modal.guild_id, modal.user_id),
                    ModerationKind::Nickname { .. } => state.can_edit_member_nickname(modal.guild_id, modal.user_id, now),
                }
            };
            if !allowed {
                modal.error = Some("Missing permission for this action.".to_owned());
                cx.notify();
                return;
            }
            (
                modal.guild_id,
                modal.user_id,
                modal.kind.clone(),
                modal.ban_delete_seconds,
                modal.ban_duration_seconds,
                modal.timeout_seconds,
                trimmed,
            )
        };

        enum PendingAction {
            Kick(Option<String>),
            Ban(BanMemberRequest),
            Patch { is_self: bool, patch: GuildMemberPatch, reason: Option<String> },
        }

        let action = match kind {
            ModerationKind::Kick => {
                if let Err(err) = validate_audit_log_reason(&trimmed) {
                    if let Overlay::MemberModeration(modal) = &mut self.overlay {
                        modal.error = Some(friendly_moderation_error(err));
                        cx.notify();
                    }
                    return;
                }
                PendingAction::Kick((!trimmed.is_empty()).then_some(trimmed))
            }
            ModerationKind::Ban => {
                let req = BanMemberRequest {
                    delete_message_seconds: Some(ban_del),
                    reason: (!trimmed.is_empty()).then_some(trimmed),
                    ban_duration_seconds: Some(ban_dur),
                };
                if let Err(err) = validate_ban_request(&req) {
                    if let Overlay::MemberModeration(modal) = &mut self.overlay {
                        modal.error = Some(friendly_moderation_error(err));
                        cx.notify();
                    }
                    return;
                }
                PendingAction::Ban(req)
            }
            ModerationKind::Timeout { .. } => {
                if let Err(err) = validate_audit_log_reason(&trimmed) {
                    if let Overlay::MemberModeration(modal) = &mut self.overlay {
                        modal.error = Some(friendly_moderation_error(err));
                        cx.notify();
                    }
                    return;
                }
                let secs = i64::from(timeout_secs.min(MAX_TIMEOUT_DURATION_SECONDS));
                let until = (chrono::Utc::now() + chrono::Duration::seconds(secs)).to_rfc3339();
                let reason = (!trimmed.is_empty()).then_some(trimmed);
                let patch = GuildMemberPatch {
                    nick: None,
                    communication_disabled_until: Some(Some(until)),
                    timeout_reason: reason.clone(),
                };
                if let Err(err) = validate_member_patch(&patch) {
                    if let Overlay::MemberModeration(modal) = &mut self.overlay {
                        modal.error = Some(friendly_moderation_error(err));
                        cx.notify();
                    }
                    return;
                }
                PendingAction::Patch { is_self: false, patch, reason }
            }
            ModerationKind::Nickname { is_self, original_nick } => {
                if trimmed == original_nick {
                    return;
                }
                let patch = GuildMemberPatch {
                    nick: Some((!trimmed.is_empty()).then_some(trimmed)),
                    communication_disabled_until: None,
                    timeout_reason: None,
                };
                if let Err(err) = validate_member_patch(&patch) {
                    if let Overlay::MemberModeration(modal) = &mut self.overlay {
                        modal.error = Some(friendly_moderation_error(err));
                        cx.notify();
                    }
                    return;
                }
                PendingAction::Patch { is_self, patch, reason: None }
            }
        };

        let seq = self.next_moderation_seq();
        if let Overlay::MemberModeration(modal) = &mut self.overlay {
            modal.seq = seq;
            modal.submitting = true;
            modal.error = None;
        } else {
            return;
        }
        match action {
            PendingAction::Kick(reason) => self.backend.kick_member(seq, guild_id, user_id, reason),
            PendingAction::Ban(req) => self.backend.ban_member(seq, guild_id, user_id, req),
            PendingAction::Patch { is_self, patch, reason } => {
                self.backend.update_guild_member_moderation(seq, guild_id, user_id, is_self, patch, reason);
            }
        }
        cx.notify();
    }

    pub(crate) fn submit_clear_timeout(&mut self, cx: &mut Context<Self>) {
        let (guild_id, user_id, trimmed) = {
            let Overlay::MemberModeration(modal) = &mut self.overlay else { return };
            if modal.submitting || !matches!(modal.kind, ModerationKind::Timeout { currently_timed_out: true }) {
                return;
            }
            if !self.backend.state().can_timeout_member(modal.guild_id, modal.user_id) {
                modal.error = Some("Missing permission for this action.".to_owned());
                cx.notify();
                return;
            }
            let raw = modal.primary_input.read(cx).value().to_string();
            (modal.guild_id, modal.user_id, raw.trim().to_owned())
        };
        if let Err(err) = validate_audit_log_reason(&trimmed) {
            if let Overlay::MemberModeration(modal) = &mut self.overlay {
                modal.error = Some(friendly_moderation_error(err));
                cx.notify();
            }
            return;
        }
        let reason = (!trimmed.is_empty()).then_some(trimmed);
        let patch = GuildMemberPatch {
            nick: None,
            communication_disabled_until: Some(None),
            timeout_reason: reason.clone(),
        };
        let seq = self.next_moderation_seq();
        if let Overlay::MemberModeration(modal) = &mut self.overlay {
            modal.seq = seq;
            modal.submitting = true;
            modal.error = None;
        } else {
            return;
        }
        self.backend.update_guild_member_moderation(seq, guild_id, user_id, false, patch, reason);
        cx.notify();
    }

    pub(crate) fn open_guild_bans(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let crate::view_model::Place::Guild(guild_id) = self.place else { return };
        if !self.backend.state().can_ban_members(guild_id) {
            return;
        }
        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        let seq = self.next_moderation_seq();
        let filter_input = cx.new(|cx| InputState::new(window, cx).placeholder(l10n::tr("Search")));
        let subscription = cx.subscribe_in(&filter_input, window, |this: &mut Self, _, ev: &InputEvent, _, cx| {
            if matches!(ev, InputEvent::Change) {
                if let Overlay::GuildBans(modal) = &mut this.overlay {
                    modal.confirming_unban = None;
                }
                cx.notify();
            }
        });
        let focus_input = filter_input.read(cx).focus_handle(cx);
        self.overlay = Overlay::GuildBans(GuildBansModal {
            seq,
            guild_id,
            filter_input,
            bans: Vec::new(),
            loading: true,
            error: None,
            confirming_unban: None,
            unbanning: BTreeSet::new(),
            unban_seqs: BTreeMap::new(),
            recently_unbanned: BTreeMap::new(),
            focus: cx.focus_handle(),
            _subscription: subscription,
        });
        window.focus(&focus_input, cx);
        self.backend.list_guild_bans(seq, guild_id);
        cx.notify();
    }

    pub(crate) fn refresh_guild_bans(&mut self, cx: &mut Context<Self>) {
        let guild_id = {
            let Overlay::GuildBans(modal) = &self.overlay else { return };
            modal.guild_id
        };
        if !self.backend.state().can_ban_members(guild_id) {
            if let Overlay::GuildBans(modal) = &mut self.overlay {
                modal.loading = false;
                modal.confirming_unban = None;
                modal.error = Some("Missing permission to view bans.".to_owned());
                cx.notify();
            }
            return;
        }
        let seq = self.next_moderation_seq();
        if let Overlay::GuildBans(modal) = &mut self.overlay {
            modal.seq = seq;
            modal.loading = true;
            modal.confirming_unban = None;
            modal.error = None;
        } else {
            return;
        }
        self.backend.list_guild_bans(seq, guild_id);
        cx.notify();
    }

    pub(crate) fn request_unban_guild_member(&mut self, user_id: Snowflake, cx: &mut Context<Self>) {
        let guild_id = {
            let Overlay::GuildBans(modal) = &self.overlay else { return };
            if modal.unbanning.contains(&user_id) {
                return;
            }
            modal.guild_id
        };
        if !self.backend.state().can_ban_members(guild_id) {
            if let Overlay::GuildBans(modal) = &mut self.overlay {
                modal.confirming_unban = None;
                modal.error = Some("Missing permission to unban members.".to_owned());
                cx.notify();
            }
            return;
        }
        if let Overlay::GuildBans(modal) = &mut self.overlay {
            modal.confirming_unban = Some(user_id);
            modal.error = None;
            cx.notify();
        }
    }

    pub(crate) fn cancel_unban_guild_member(&mut self, cx: &mut Context<Self>) {
        if let Overlay::GuildBans(modal) = &mut self.overlay {
            modal.confirming_unban = None;
            cx.notify();
        }
    }

    pub(crate) fn unban_guild_member(&mut self, user_id: Snowflake, cx: &mut Context<Self>) {
        let guild_id = {
            let Overlay::GuildBans(modal) = &self.overlay else { return };
            if modal.unbanning.contains(&user_id) {
                return;
            }
            modal.guild_id
        };
        if !self.backend.state().can_ban_members(guild_id) {
            if let Overlay::GuildBans(modal) = &mut self.overlay {
                modal.confirming_unban = None;
                modal.error = Some("Missing permission to unban members.".to_owned());
                cx.notify();
            }
            return;
        }
        let seq = self.next_moderation_seq();
        if let Overlay::GuildBans(modal) = &mut self.overlay {
            modal.confirming_unban = None;
            modal.unbanning.insert(user_id);
            modal.unban_seqs.insert(user_id, seq);
            modal.error = None;
        } else {
            return;
        }
        self.backend.unban_member(seq, guild_id, user_id, None);
        cx.notify();
    }
}

pub fn render_action(modal: &MemberModerationModal, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let title = match &modal.kind {
        ModerationKind::Kick => format!("{} — {}", l10n::tr("Kick member"), modal.display_name),
        ModerationKind::Ban => format!("{} — {}", l10n::tr("Ban member"), modal.display_name),
        ModerationKind::Timeout { .. } => format!("{} — {}", l10n::tr("Timeout"), modal.display_name),
        ModerationKind::Nickname { .. } => format!("{} — {}", l10n::tr("Change nickname"), modal.display_name),
    };

    let ban_options = (modal.kind == ModerationKind::Ban).then(|| {
        let mut del_row = div().flex().flex_row().flex_wrap().gap_1();
        for &(secs, label) in BAN_DELETE_OPTIONS {
            let sel = format!("mod-ban-del-{secs}");
            del_row = del_row.child(
                div().debug_selector(move || sel.clone()).child(
                    Button::new(("mod-ban-del", secs as u64))
                        .ghost()
                        .small()
                        .selected(modal.ban_delete_seconds == secs)
                        .label(l10n::tr(label))
                        .on_click(cx.listener(move |this, _, _, cx| this.set_ban_delete_seconds(secs, cx))),
                ),
            );
        }
        let mut dur_row = div().flex().flex_row().flex_wrap().gap_1();
        for &(secs, label) in BAN_DURATION_OPTIONS {
            let sel = format!("mod-ban-dur-{secs}");
            dur_row = dur_row.child(
                div().debug_selector(move || sel.clone()).child(
                    Button::new(("mod-ban-dur", secs as u64))
                        .ghost()
                        .small()
                        .selected(modal.ban_duration_seconds == secs)
                        .label(l10n::tr(label))
                        .on_click(cx.listener(move |this, _, _, cx| this.set_ban_duration_seconds(secs, cx))),
                ),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(div().flex().flex_col().gap_1().child(section_label(l10n::tr("Delete message history"), theme)).child(del_row))
            .child(div().flex().flex_col().gap_1().child(section_label(l10n::tr("Ban duration"), theme)).child(dur_row))
    });

    let timeout_options = match modal.kind {
        ModerationKind::Timeout { currently_timed_out } => {
            let mut row = div().flex().flex_row().flex_wrap().gap_1();
            for &(secs, label) in TIMEOUT_DURATION_OPTIONS {
                let sel = format!("mod-timeout-{secs}");
                row = row.child(
                    div().debug_selector(move || sel.clone()).child(
                        Button::new(("mod-timeout", secs as u64))
                            .ghost()
                            .small()
                            .selected(modal.timeout_seconds == secs)
                            .label(l10n::tr(label))
                            .on_click(cx.listener(move |this, _, _, cx| this.set_timeout_seconds(secs, cx))),
                    ),
                );
            }
            Some(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(div().flex().flex_col().gap_1().child(section_label(l10n::tr("Duration"), theme)).child(row))
                    .when(currently_timed_out, |d| {
                        d.child(
                            div().debug_selector(|| "mod-timeout-clear".to_owned()).child(
                                Button::new("mod-timeout-clear")
                                    .ghost()
                                    .small()
                                    .disabled(modal.submitting)
                                    .label(l10n::tr("Remove timeout"))
                                    .on_click(cx.listener(|this, _, _, cx| this.submit_clear_timeout(cx))),
                            ),
                        )
                    }),
            )
        }
        _ => None,
    };

    let input_label = match &modal.kind {
        ModerationKind::Nickname { .. } => l10n::tr("Nickname"),
        _ => l10n::tr("Reason"),
    };

    let is_unchanged_nick = match &modal.kind {
        ModerationKind::Nickname { original_nick, .. } => modal.primary_input.read(cx).value().trim() == original_nick,
        _ => false,
    };

    let (submit_label, is_danger) = match &modal.kind {
        ModerationKind::Kick => (l10n::tr("Kick"), true),
        ModerationKind::Ban => (l10n::tr("Ban"), true),
        ModerationKind::Timeout { .. } => (l10n::tr("Timeout"), false),
        ModerationKind::Nickname { .. } => (l10n::tr("Save"), false),
    };

    let mut submit_btn = Button::new("moderation-submit")
        .small()
        .disabled(modal.submitting || is_unchanged_nick)
        .label(if modal.submitting { l10n::tr("Loading…") } else { submit_label })
        .on_click(cx.listener(|this, _, _, cx| this.submit_member_moderation(cx)));
    submit_btn = if is_danger { submit_btn.danger() } else { submit_btn.primary() };

    let content = div()
        .id("moderation-modal")
        .debug_selector(|| "moderation-modal".to_owned())
        .key_context(ACTION_CONTEXT)
        .track_focus(&modal.focus)
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .capture_action(cx.listener(|this, _: &Escape, window, cx| {
            cx.stop_propagation();
            this.close_overlay(window, cx);
        }))
        .child(div().text_lg().font_weight(FontWeight::BOLD).child(title))
        .children(ban_options)
        .children(timeout_options)
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(section_label(input_label, theme))
                .child(Input::new(&modal.primary_input).small()),
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
                    div().debug_selector(|| "moderation-cancel".to_owned()).child(
                        Button::new("moderation-cancel")
                            .ghost()
                            .small()
                            .label(l10n::tr("Cancel"))
                            .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
                    ),
                )
                .child(div().debug_selector(|| "moderation-submit".to_owned()).child(submit_btn)),
        );

    let view = cx.weak_entity();
    overlay::modal(content, theme, move |window, cx| {
        let _ = view.update(cx, |v, cx| v.close_overlay(window, cx));
    })
}

pub fn render_bans(modal: &GuildBansModal, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let needle = modal.filter_input.read(cx).value().trim().to_lowercase();
    let filtered: Vec<&GuildBanEntry> = modal
        .bans
        .iter()
        .filter(|entry| {
            if needle.is_empty() {
                return true;
            }
            entry.user.username.to_lowercase().contains(&needle)
                || entry.user.display_name().to_lowercase().contains(&needle)
                || entry.user.id.to_string().contains(&needle)
                || entry.reason.as_deref().unwrap_or_default().to_lowercase().contains(&needle)
        })
        .collect();

    let mut list = div().id("guild-bans-list").flex().flex_col().max_h(px(280.)).overflow_y_scroll().gap_1();
    if modal.loading && modal.bans.is_empty() {
        list = list.child(div().py_3().text_xs().text_color(theme.muted_foreground).child(l10n::tr("Loading…")));
    } else if filtered.is_empty() {
        list = list.child(div().py_3().text_xs().text_color(theme.muted_foreground).child(l10n::tr("No results")));
    } else {
        for entry in filtered {
            let uid = entry.user.id;
            let unbanning = modal.unbanning.contains(&uid);
            let sel = format!("unban-user-{}", uid.0);
            let title = format!("{} ({})", entry.user.display_name(), entry.user.username);
            let subtitle = match (entry.reason.as_deref().filter(|s| !s.is_empty()), entry.expires_at.as_deref()) {
                (Some(r), Some(exp)) => format!("{r} · {exp}"),
                (Some(r), None) => r.to_owned(),
                (None, Some(exp)) => exp.to_owned(),
                (None, None) => uid.to_string(),
            };
            list = list.child(
                div()
                    .id(("ban-row", uid.0))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.background)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().truncate().text_sm().font_weight(FontWeight::MEDIUM).child(title))
                            .child(div().truncate().text_xs().text_color(theme.muted_foreground).child(subtitle)),
                    )
                    .child(if !unbanning && modal.confirming_unban == Some(uid) {
                        let cancel_sel = format!("unban-cancel-{}", uid.0);
                        let confirm_sel = format!("unban-confirm-{}", uid.0);
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .child(
                                div().debug_selector(move || cancel_sel.clone()).child(
                                    Button::new(("unban-cancel", uid.0))
                                        .ghost()
                                        .small()
                                        .label(l10n::tr("Cancel"))
                                        .on_click(cx.listener(|this, _, _, cx| this.cancel_unban_guild_member(cx))),
                                ),
                            )
                            .child(
                                div().debug_selector(move || confirm_sel.clone()).child(
                                    Button::new(("unban-confirm", uid.0))
                                        .danger()
                                        .small()
                                        .label(l10n::tr("Confirm"))
                                        .on_click(cx.listener(move |this, _, _, cx| this.unban_guild_member(uid, cx))),
                                ),
                            )
                            .into_any_element()
                    } else {
                        div()
                            .debug_selector(move || sel.clone())
                            .child(
                                Button::new(("unban-user", uid.0))
                                    .danger()
                                    .small()
                                    .disabled(unbanning)
                                    .label(if unbanning { l10n::tr("Loading…") } else { l10n::tr("Unban") })
                                    .on_click(cx.listener(move |this, _, _, cx| this.request_unban_guild_member(uid, cx))),
                            )
                            .into_any_element()
                    }),
            );
        }
    }

    let content = div()
        .id("guild-bans-modal")
        .debug_selector(|| "guild-bans-modal".to_owned())
        .key_context(BANS_CONTEXT)
        .track_focus(&modal.focus)
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
                .child(div().text_lg().font_weight(FontWeight::BOLD).child(l10n::tr("Banned users")))
                .child(
                    div().debug_selector(|| "guild-bans-refresh".to_owned()).child(
                        Button::new("guild-bans-refresh")
                            .ghost()
                            .small()
                            .disabled(modal.loading)
                            .label(if modal.loading { l10n::tr("Loading…") } else { l10n::tr("Refresh") })
                            .on_click(cx.listener(|this, _, _, cx| this.refresh_guild_bans(cx))),
                    ),
                ),
        )
        .child(Input::new(&modal.filter_input).small())
        .child(list)
        .when_some(modal.error.clone(), |d, err| {
            d.child(div().text_xs().text_color(theme.danger).child(err))
        })
        .child(
            div().flex().flex_row().justify_end().child(
                div().debug_selector(|| "guild-bans-close".to_owned()).child(
                    Button::new("guild-bans-close")
                        .ghost()
                        .small()
                        .label(l10n::tr("Close"))
                        .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
                ),
            ),
        );

    let view = cx.weak_entity();
    overlay::modal(content, theme, move |window, cx| {
        let _ = view.update(cx, |v, cx| v.close_overlay(window, cx));
    })
}
