//! Guild channel create (`Overlay::CreateChannel`) and edit/delete
//! (`Overlay::EditChannel`) modals.

use fluxer_client_core::rest::{
    CreateGuildChannelRequest, GuildChannelPatch, MAX_CHANNEL_NAME_UTF16, MAX_CHANNEL_TOPIC_UTF16, RestError,
    VOICE_CHANNEL_BITRATE_DEFAULT, VOICE_CHANNEL_CONNECTION_LIMIT_DEFAULT, validate_create_guild_channel,
    validate_guild_channel_patch,
};
use fluxer_client_core::state::{ChannelKind, ClientState, Snowflake};
use gpui_kit::component::{Disableable as _, Selectable as _, Sizable as _};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::switch::Switch;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, EntityInputHandler as _, FocusHandle, Focusable as _,
    FontWeight, SharedString, Subscription, Window, div, prelude::*,
};

use crate::client_view::ClientView;
use crate::theme::Theme;
use crate::ui::overlay::Overlay;
use crate::ui::{l10n, overlay};
use crate::view_model::{Place, channel_name};

pub const CREATE_CONTEXT: &str = "CreateChannelModal";
pub const EDIT_CONTEXT: &str = "EditChannelModal";

pub const SLOWMODE_OPTIONS: &[(u32, &str)] = &[
    (0, "Off"),
    (5, "5s"),
    (10, "10s"),
    (15, "15s"),
    (30, "30s"),
    (60, "1 minute"),
    (300, "5 minutes"),
    (600, "10 minutes"),
    (900, "15 minutes"),
    (1800, "30 minutes"),
    (3600, "1 hour"),
    (21600, "6 hours"),
];

pub struct CreateChannelModal {
    pub seq: u64,
    pub guild_id: Snowflake,
    pub can_manage_root: bool,
    pub parent_id: Option<Snowflake>,
    pub categories: Vec<(Snowflake, SharedString)>,
    pub kind: ChannelKind,
    pub name_input: Entity<InputState>,
    pub nsfw: bool,
    pub submitting: bool,
    pub error: Option<String>,
    pub _subscription: Subscription,
}

pub struct EditChannelModal {
    pub seq: u64,
    pub guild_id: Snowflake,
    pub channel_id: Snowflake,
    pub original_kind: ChannelKind,
    pub kind: ChannelKind,
    pub original_name: String,
    pub original_topic: String,
    pub original_parent_id: Option<Snowflake>,
    pub original_nsfw: bool,
    pub original_slowmode: u32,
    pub can_manage_root: bool,
    pub can_change_parent: bool,
    pub can_sync_parent: bool,
    pub sync_permissions: bool,
    pub categories: Vec<(Snowflake, SharedString)>,
    pub name_input: Entity<InputState>,
    pub topic_input: Option<Entity<InputState>>,
    pub parent_id: Option<Snowflake>,
    pub nsfw: bool,
    pub slowmode: u32,
    pub confirming_delete: bool,
    pub submitting: bool,
    pub error: Option<String>,
    pub focus: FocusHandle,
    pub _subscriptions: Vec<Subscription>,
}

impl EditChannelModal {
    pub fn sync_state(&mut self, state: &ClientState) {
        self.categories = manageable_categories(state, self.guild_id);
        if let Some(channel) = state.channels.get(&self.channel_id)
            && channel.parent_id == self.parent_id
            && channel.parent_id != self.original_parent_id
        {
            self.original_parent_id = channel.parent_id;
            self.sync_permissions = false;
        }
        self.can_sync_parent = self.parent_id.is_some_and(|parent| state.can_sync_channel_parent(self.channel_id, parent));
        let valid_cat = |pid: Snowflake| {
            state
                .channels
                .get(&pid)
                .is_some_and(|c| c.guild_id == Some(self.guild_id) && c.kind.is_category())
        };
        if let Some(pid) = self.parent_id
            && !valid_cat(pid)
        {
            self.parent_id = None;
        }
        if let Some(pid) = self.original_parent_id
            && !valid_cat(pid)
        {
            self.original_parent_id = None;
        }
        self.can_manage_root = state.can_manage_guild_channels(self.guild_id);
        self.can_change_parent = !self.original_kind.is_category()
            && (!self.categories.is_empty() || (self.can_manage_root && self.original_parent_id.is_some()));
    }

    pub fn build_patch(&self, cx: &App) -> GuildChannelPatch {
        let name = self.name_input.read(cx).value().trim().to_owned();
        let name_patch = (name != self.original_name).then_some(name);
        let kind_patch = (self.kind != self.original_kind).then_some(self.kind);
        let topic_patch = self.topic_input.as_ref().and_then(|input| {
            let trimmed = input.read(cx).value().trim().to_owned();
            if trimmed == self.original_topic {
                None
            } else if trimmed.is_empty() {
                Some(None)
            } else {
                Some(Some(trimmed))
            }
        });
        let parent_patch = if !self.original_kind.is_category() && self.parent_id != self.original_parent_id {
            Some(self.parent_id.map(|id| id.to_string()))
        } else {
            None
        };
        let nsfw_override =
            (!self.original_kind.is_category() && self.nsfw != self.original_nsfw).then_some(Some(self.nsfw));
        let slowmode_patch = (matches!(
            self.original_kind,
            ChannelKind::GuildText | ChannelKind::GuildAnnouncement
        ) && self.slowmode != self.original_slowmode)
            .then_some(self.slowmode);
        GuildChannelPatch {
            name: name_patch,
            kind: kind_patch,
            topic: topic_patch,
            parent_id: parent_patch,
            nsfw: None,
            nsfw_override,
            rate_limit_per_user: slowmode_patch,
        }
    }
}

fn manageable_categories(state: &ClientState, guild_id: Snowflake) -> Vec<(Snowflake, SharedString)> {
    let mut cats: Vec<_> = state
        .channels
        .values()
        .filter(|c| c.guild_id == Some(guild_id) && c.kind.is_category() && state.can_manage_channel(c.id))
        .collect();
    cats.sort_by_key(|c| (c.position, c.id));
    cats.into_iter().map(|c| (c.id, SharedString::from(channel_name(c)))).collect()
}

fn friendly_channel_error(err: RestError) -> String {
    match err {
        RestError::InvalidArgument("name") => {
            format!("Channel name must be 1–{MAX_CHANNEL_NAME_UTF16} characters.")
        }
        RestError::InvalidArgument("topic") => {
            format!("Channel topic must be 1–{MAX_CHANNEL_TOPIC_UTF16} characters.")
        }
        other => other.to_string(),
    }
}

fn section_label(label: String, theme: &Theme) -> gpui_kit::Div {
    div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(label)
}

impl ClientView {
    pub(crate) fn open_create_channel(
        &mut self,
        parent_id: Option<Snowflake>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Place::Guild(guild_id) = self.place else { return };
        let (can_manage_root, categories) = {
            let state = self.backend.state();
            if !state.guilds.contains_key(&guild_id) {
                return;
            }
            let can_manage_root = state.can_manage_guild_channels(guild_id);
            match parent_id {
                None => {
                    if !can_manage_root {
                        return;
                    }
                }
                Some(cat_id) => {
                    let valid_cat = state
                        .channels
                        .get(&cat_id)
                        .is_some_and(|c| c.guild_id == Some(guild_id) && c.kind.is_category())
                        && state.can_manage_channel(cat_id);
                    if !valid_cat {
                        return;
                    }
                }
            }
            (can_manage_root, manageable_categories(&state, guild_id))
        };

        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        let seq = self.next_channel_mgmt_seq();
        let name_input = cx.new(|cx| InputState::new(window, cx).placeholder(l10n::tr("Channel name")));
        let subscription =
            cx.subscribe_in(&name_input, window, |this: &mut Self, input_entity, ev: &InputEvent, window, cx| match ev {
                InputEvent::PressEnter { shift: false, .. } => {
                    if input_entity.update(cx, |s, cx| s.marked_text_range(window, cx)).is_some() {
                        return;
                    }
                    this.submit_create_channel(cx);
                }
                InputEvent::Change => {
                    if let Overlay::CreateChannel(modal) = &mut this.overlay {
                        modal.error = None;
                        cx.notify();
                    }
                }
                _ => {}
            });
        let focus = name_input.read(cx).focus_handle(cx);
        self.overlay = Overlay::CreateChannel(CreateChannelModal {
            seq,
            guild_id,
            can_manage_root,
            parent_id,
            categories,
            kind: ChannelKind::GuildText,
            name_input,
            nsfw: false,
            submitting: false,
            error: None,
            _subscription: subscription,
        });
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(crate) fn set_create_channel_kind(&mut self, kind: ChannelKind, cx: &mut Context<Self>) {
        let Overlay::CreateChannel(modal) = &mut self.overlay else { return };
        if modal.submitting {
            return;
        }
        if !matches!(
            kind,
            ChannelKind::GuildText | ChannelKind::GuildAnnouncement | ChannelKind::GuildVoice | ChannelKind::GuildCategory
        ) {
            return;
        }
        if kind == ChannelKind::GuildCategory && !modal.can_manage_root {
            return;
        }
        modal.kind = kind;
        modal.error = None;
        if kind == ChannelKind::GuildCategory {
            modal.parent_id = None;
            modal.nsfw = false;
        }
        cx.notify();
    }

    pub(crate) fn set_create_channel_parent(&mut self, parent_id: Option<Snowflake>, cx: &mut Context<Self>) {
        let Overlay::CreateChannel(modal) = &mut self.overlay else { return };
        if modal.submitting || modal.kind == ChannelKind::GuildCategory {
            return;
        }
        match parent_id {
            None if !modal.can_manage_root => return,
            Some(pid) if !modal.categories.iter().any(|(id, _)| *id == pid) => return,
            _ => {}
        }
        modal.parent_id = parent_id;
        modal.error = None;
        cx.notify();
    }

    pub(crate) fn set_create_channel_nsfw(&mut self, checked: bool, cx: &mut Context<Self>) {
        let Overlay::CreateChannel(modal) = &mut self.overlay else { return };
        if modal.submitting || modal.kind == ChannelKind::GuildCategory {
            return;
        }
        modal.nsfw = checked;
        cx.notify();
    }

    pub(crate) fn submit_create_channel(&mut self, cx: &mut Context<Self>) {
        let (guild_id, request) = {
            let Overlay::CreateChannel(modal) = &mut self.overlay else { return };
            if modal.submitting {
                return;
            }
            let kind = modal.kind;
            let parent_id = if kind == ChannelKind::GuildCategory { None } else { modal.parent_id };
            let allowed = {
                let state = self.backend.state();
                match parent_id {
                    None => state.can_manage_guild_channels(modal.guild_id),
                    Some(cat_id) => {
                        state
                            .channels
                            .get(&cat_id)
                            .is_some_and(|c| c.guild_id == Some(modal.guild_id) && c.kind.is_category())
                            && state.can_manage_channel(cat_id)
                    }
                }
            };
            if !allowed {
                modal.error = Some("Missing permission to create channel here.".to_owned());
                cx.notify();
                return;
            }
            let raw_name = modal.name_input.read(cx).value().to_string();
            let req = CreateGuildChannelRequest {
                name: raw_name.trim().to_owned(),
                kind,
                parent_id: parent_id.map(|id| id.to_string()),
                topic: None,
                nsfw: if kind == ChannelKind::GuildCategory { None } else { Some(modal.nsfw) },
                rate_limit_per_user: None,
                bitrate: (kind == ChannelKind::GuildVoice).then_some(VOICE_CHANNEL_BITRATE_DEFAULT),
                user_limit: (kind == ChannelKind::GuildVoice).then_some(0),
                voice_connection_limit: (kind == ChannelKind::GuildVoice)
                    .then_some(VOICE_CHANNEL_CONNECTION_LIMIT_DEFAULT),
            };
            if let Err(err) = validate_create_guild_channel(&req) {
                modal.error = Some(friendly_channel_error(err));
                cx.notify();
                return;
            }
            (modal.guild_id, req)
        };
        let seq = self.next_channel_mgmt_seq();
        if let Overlay::CreateChannel(modal) = &mut self.overlay {
            modal.seq = seq;
            modal.submitting = true;
            modal.error = None;
        } else {
            return;
        }
        self.backend.create_guild_channel(seq, guild_id, request);
        cx.notify();
    }

    pub(crate) fn open_edit_channel(
        &mut self,
        channel_id: Snowflake,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (guild_id, kind, name, topic, parent_id, nsfw, slowmode, can_manage_root, categories) = {
            let state = self.backend.state();
            let Some(ch) = state.channels.get(&channel_id) else { return };
            let Some(guild_id) = ch.guild_id else { return };
            if ch.kind.is_private() || ch.kind.is_thread() || !state.can_manage_channel(channel_id) {
                return;
            }
            let can_manage_root = state.can_manage_guild_channels(guild_id);
            let categories = manageable_categories(&state, guild_id);
            (
                guild_id,
                ch.kind,
                ch.name.as_deref().map(str::trim).unwrap_or_default().to_owned(),
                ch.topic.as_deref().map(str::trim).unwrap_or_default().to_owned(),
                ch.parent_id,
                ch.nsfw,
                ch.rate_limit_per_user.unwrap_or(0),
                can_manage_root,
                categories,
            )
        };

        let can_change_parent =
            !kind.is_category() && (!categories.is_empty() || (can_manage_root && parent_id.is_some()));

        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        let seq = self.next_channel_mgmt_seq();
        let initial_name = name.clone();
        let name_input = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder(if kind.is_category() {
                l10n::tr("Category name")
            } else {
                l10n::tr("Channel name")
            });
            state.set_value(&initial_name, window, cx);
            state
        });
        let mut subscriptions = Vec::new();
        subscriptions.push(cx.subscribe_in(
            &name_input,
            window,
            |this: &mut Self, input_entity, ev: &InputEvent, window, cx| match ev {
                InputEvent::PressEnter { shift: false, .. } => {
                    if input_entity.update(cx, |s, cx| s.marked_text_range(window, cx)).is_some() {
                        return;
                    }
                    this.submit_edit_channel(cx);
                }
                InputEvent::Change => {
                    if let Overlay::EditChannel(modal) = &mut this.overlay {
                        modal.error = None;
                        cx.notify();
                    }
                }
                _ => {}
            },
        ));
        let topic_input = if matches!(kind, ChannelKind::GuildText | ChannelKind::GuildAnnouncement) {
            let initial_topic = topic.clone();
            let input = cx.new(|cx| {
                let mut state = InputState::new(window, cx).placeholder(l10n::tr("Topic"));
                if !initial_topic.is_empty() {
                    state.set_value(&initial_topic, window, cx);
                }
                state
            });
            subscriptions.push(cx.subscribe_in(
                &input,
                window,
                |this: &mut Self, input_entity, ev: &InputEvent, window, cx| match ev {
                    InputEvent::PressEnter { shift: false, .. } => {
                        if input_entity.update(cx, |s, cx| s.marked_text_range(window, cx)).is_some() {
                            return;
                        }
                        this.submit_edit_channel(cx);
                    }
                    InputEvent::Change => {
                        if let Overlay::EditChannel(modal) = &mut this.overlay {
                            modal.error = None;
                            cx.notify();
                        }
                    }
                    _ => {}
                },
            ));
            Some(input)
        } else {
            None
        };

        let name_focus = name_input.read(cx).focus_handle(cx);
        self.overlay = Overlay::EditChannel(EditChannelModal {
            seq,
            guild_id,
            channel_id,
            original_kind: kind,
            kind,
            original_name: name,
            original_topic: topic,
            original_parent_id: parent_id,
            original_nsfw: nsfw,
            original_slowmode: slowmode,
            can_manage_root,
            can_change_parent,
            can_sync_parent: false,
            sync_permissions: false,
            categories,
            name_input,
            topic_input,
            parent_id,
            nsfw,
            slowmode,
            confirming_delete: false,
            submitting: false,
            error: None,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        });
        window.focus(&name_focus, cx);
        cx.notify();
    }

    pub(crate) fn toggle_edit_channel_announcement(&mut self, checked: bool, cx: &mut Context<Self>) {
        let Overlay::EditChannel(modal) = &mut self.overlay else { return };
        if modal.submitting
            || !matches!(modal.original_kind, ChannelKind::GuildText | ChannelKind::GuildAnnouncement)
        {
            return;
        }
        modal.kind = if checked { ChannelKind::GuildAnnouncement } else { ChannelKind::GuildText };
        modal.error = None;
        cx.notify();
    }

    pub(crate) fn set_edit_channel_nsfw(&mut self, checked: bool, cx: &mut Context<Self>) {
        let Overlay::EditChannel(modal) = &mut self.overlay else { return };
        if modal.submitting || modal.original_kind.is_category() {
            return;
        }
        modal.nsfw = checked;
        modal.error = None;
        cx.notify();
    }

    pub(crate) fn set_edit_channel_slowmode(&mut self, secs: u32, cx: &mut Context<Self>) {
        let Overlay::EditChannel(modal) = &mut self.overlay else { return };
        if modal.submitting
            || !matches!(modal.original_kind, ChannelKind::GuildText | ChannelKind::GuildAnnouncement)
        {
            return;
        }
        modal.slowmode = secs;
        modal.error = None;
        cx.notify();
    }

    pub(crate) fn set_edit_channel_parent(&mut self, parent_id: Option<Snowflake>, cx: &mut Context<Self>) {
        let Overlay::EditChannel(modal) = &mut self.overlay else { return };
        if modal.submitting || modal.original_kind.is_category() {
            return;
        }
        if parent_id != modal.original_parent_id {
            match parent_id {
                None if !modal.can_manage_root => return,
                Some(pid) if !modal.categories.iter().any(|(id, _)| *id == pid) => return,
                _ => {}
            }
        }
        modal.parent_id = parent_id;
        modal.sync_permissions = false;
        modal.can_sync_parent = parent_id.is_some_and(|parent| self.backend.state().can_sync_channel_parent(modal.channel_id, parent));
        modal.error = None;
        cx.notify();
    }

    pub(crate) fn submit_edit_channel(&mut self, cx: &mut Context<Self>) {
        let (channel_id, patch, sync_move) = {
            let Overlay::EditChannel(modal) = &mut self.overlay else { return };
            if modal.submitting {
                return;
            }
            let patch = modal.build_patch(cx);
            if patch.is_empty() {
                return;
            }
            let allowed = {
                let state = self.backend.state();
                let base = state.can_manage_channel(modal.channel_id);
                let parent_ok = match &patch.parent_id {
                    None => true,
                    Some(None) => state.can_manage_guild_channels(modal.guild_id),
                    Some(Some(_)) => modal.parent_id.is_some_and(|pid| {
                        state
                            .channels
                            .get(&pid)
                            .is_some_and(|c| c.guild_id == Some(modal.guild_id) && c.kind.is_category())
                            && state.can_manage_channel(pid)
                    }),
                };
                base && parent_ok
            };
            if !allowed {
                modal.error = Some("Missing permission to edit this channel.".to_owned());
                cx.notify();
                return;
            }
            if let Err(err) = validate_guild_channel_patch(&patch) {
                modal.error = Some(friendly_channel_error(err));
                cx.notify();
                return;
            }
            let sync_move = if modal.sync_permissions {
                let Some(parent) = modal.parent_id else { return };
                if !modal.can_sync_parent {
                    modal.error = Some(l10n::tr("You no longer have permission to sync with this category."));
                    cx.notify();
                    return;
                }
                Some((modal.guild_id, modal.original_parent_id, parent))
            } else {
                None
            };
            (modal.channel_id, patch, sync_move)
        };
        let seq = self.next_channel_mgmt_seq();
        if let Overlay::EditChannel(modal) = &mut self.overlay {
            modal.seq = seq;
            modal.submitting = true;
            modal.confirming_delete = false;
            modal.error = None;
        } else {
            return;
        }
        if let Some((guild, before, parent)) = sync_move {
            let other = GuildChannelPatch { parent_id: None, ..patch };
            self.backend.move_channel_and_sync(seq, guild, channel_id, before, parent, other);
        } else {
            self.backend.update_guild_channel(seq, channel_id, patch);
        }
        cx.notify();
    }

    pub(crate) fn request_edit_channel_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Overlay::EditChannel(modal) = &mut self.overlay else { return };
        if modal.submitting {
            return;
        }
        modal.confirming_delete = true;
        modal.error = None;
        let focus = modal.focus.clone();
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(crate) fn cancel_edit_channel_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Overlay::EditChannel(modal) = &mut self.overlay else { return };
        if modal.submitting {
            return;
        }
        modal.confirming_delete = false;
        modal.error = None;
        let focus = modal.focus.clone();
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(crate) fn handle_edit_channel_escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(&self.overlay, Overlay::EditChannel(m) if m.confirming_delete) {
            self.cancel_edit_channel_delete(window, cx);
        } else {
            self.close_overlay(window, cx);
        }
    }

    pub(crate) fn confirm_delete_channel(&mut self, cx: &mut Context<Self>) {
        let channel_id = {
            let Overlay::EditChannel(modal) = &mut self.overlay else { return };
            if modal.submitting || !modal.confirming_delete {
                return;
            }
            if !self.backend.state().can_manage_channel(modal.channel_id) {
                modal.confirming_delete = false;
                modal.error = Some("Missing permission to delete this channel.".to_owned());
                cx.notify();
                return;
            }
            modal.channel_id
        };
        let seq = self.next_channel_mgmt_seq();
        if let Overlay::EditChannel(modal) = &mut self.overlay {
            modal.seq = seq;
            modal.submitting = true;
            modal.error = None;
        } else {
            return;
        }
        self.backend.delete_guild_channel(seq, channel_id);
        cx.notify();
    }
}

pub fn render_create(modal: &CreateChannelModal, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let type_choices: &[(ChannelKind, &str, &str)] = if modal.can_manage_root {
        &[
            (ChannelKind::GuildText, "Text", "create-channel-type-text"),
            (ChannelKind::GuildAnnouncement, "Announcement", "create-channel-type-announcement"),
            (ChannelKind::GuildVoice, "Voice", "create-channel-type-voice"),
            (ChannelKind::GuildCategory, "Category", "create-channel-type-category"),
        ]
    } else {
        &[
            (ChannelKind::GuildText, "Text", "create-channel-type-text"),
            (ChannelKind::GuildAnnouncement, "Announcement", "create-channel-type-announcement"),
            (ChannelKind::GuildVoice, "Voice", "create-channel-type-voice"),
        ]
    };

    let mut type_row = div().flex().flex_row().flex_wrap().gap_1();
    for (kind, label, selector) in type_choices {
        let k = *kind;
        let selected = modal.kind == k;
        let sel_str = (*selector).to_owned();
        type_row = type_row.child(
            div().debug_selector(move || sel_str.clone()).child(
                Button::new((*selector).to_owned())
                    .ghost()
                    .small()
                    .selected(selected)
                    .label(l10n::tr(label))
                    .on_click(cx.listener(move |this, _, _, cx| this.set_create_channel_kind(k, cx))),
            ),
        );
    }

    let show_parent = modal.kind != ChannelKind::GuildCategory && (!modal.categories.is_empty() || modal.can_manage_root);
    let parent_section = if show_parent {
        let mut row = div().flex().flex_row().flex_wrap().gap_1();
        if modal.can_manage_root {
            row = row.child(
                div().debug_selector(|| "create-channel-parent-none".to_owned()).child(
                    Button::new("create-channel-parent-none")
                        .ghost()
                        .small()
                        .selected(modal.parent_id.is_none())
                        .label(l10n::tr("No category"))
                        .on_click(cx.listener(|this, _, _, cx| this.set_create_channel_parent(None, cx))),
                ),
            );
        }
        for (cat_id, cat_name) in &modal.categories {
            let cid = *cat_id;
            let sel = format!("create-channel-parent-{}", cid.0);
            row = row.child(
                div().debug_selector(move || sel.clone()).child(
                    Button::new(("create-channel-parent", cid.0))
                        .ghost()
                        .small()
                        .selected(modal.parent_id == Some(cid))
                        .label(cat_name.clone())
                        .on_click(cx.listener(move |this, _, _, cx| this.set_create_channel_parent(Some(cid), cx))),
                ),
            );
        }
        Some(div().flex().flex_col().gap_1().child(section_label(l10n::tr("Category"), theme)).child(row))
    } else {
        None
    };

    let nsfw_row = (modal.kind != ChannelKind::GuildCategory).then(|| {
        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .child(div().text_sm().child("NSFW / Age-restricted"))
            .child(
                div().debug_selector(|| "create-channel-nsfw".to_owned()).child(
                    Switch::new("create-channel-nsfw")
                        .checked(modal.nsfw)
                        .on_change(cx.listener(|this, checked: &bool, _, cx| {
                            this.set_create_channel_nsfw(*checked, cx);
                        })),
                ),
            )
    });

    let title = if modal.kind == ChannelKind::GuildCategory {
        l10n::tr("Create category")
    } else {
        l10n::tr("Create channel")
    };

    let content = div()
        .id("create-channel-modal")
        .debug_selector(|| "create-channel-modal".to_owned())
        .key_context(CREATE_CONTEXT)
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .capture_action(cx.listener(|this, _: &Escape, window, cx| {
            cx.stop_propagation();
            this.close_overlay(window, cx);
        }))
        .child(div().text_lg().font_weight(FontWeight::BOLD).child(title))
        .child(div().flex().flex_col().gap_1().child(section_label(l10n::tr("Channel type"), theme)).child(type_row))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(section_label(
                    if modal.kind == ChannelKind::GuildCategory {
                        l10n::tr("Category name")
                    } else {
                        l10n::tr("Channel name")
                    },
                    theme,
                ))
                .child(Input::new(&modal.name_input).small()),
        )
        .children(parent_section)
        .children(nsfw_row)
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
                    div().debug_selector(|| "create-channel-cancel".to_owned()).child(
                        Button::new("create-channel-cancel")
                            .ghost()
                            .small()
                            .label(l10n::tr("Cancel"))
                            .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
                    ),
                )
                .child(
                    div().debug_selector(|| "create-channel-submit".to_owned()).child(
                        Button::new("create-channel-submit")
                            .primary()
                            .small()
                            .disabled(modal.submitting)
                            .label(if modal.submitting {
                                l10n::tr("Loading…")
                            } else if modal.kind == ChannelKind::GuildCategory {
                                l10n::tr("Create category")
                            } else {
                                l10n::tr("Create channel")
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.submit_create_channel(cx))),
                    ),
                ),
        );

    let view = cx.weak_entity();
    overlay::modal(content, theme, move |window, cx| {
        let _ = view.update(cx, |v, cx| v.close_overlay(window, cx));
    })
}

pub fn render_edit(modal: &EditChannelModal, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let dirty = !modal.build_patch(cx).is_empty();
    let is_cat = modal.original_kind.is_category();
    let is_text_or_ann = matches!(modal.original_kind, ChannelKind::GuildText | ChannelKind::GuildAnnouncement);

    let title = if is_cat {
        l10n::tr("Category settings")
    } else {
        l10n::tr("Channel settings")
    };

    let topic_section = modal.topic_input.as_ref().map(|topic_input| {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(section_label(l10n::tr("Topic"), theme))
            .child(Input::new(topic_input).small())
    });

    let announcement_section = is_text_or_ann.then(|| {
        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .child(div().text_sm().child(l10n::tr("Announcement")))
            .child(
                div().debug_selector(|| "edit-channel-announcement".to_owned()).child(
                    Switch::new("edit-channel-announcement")
                        .checked(modal.kind == ChannelKind::GuildAnnouncement)
                        .on_change(cx.listener(|this, checked: &bool, _, cx| {
                            this.toggle_edit_channel_announcement(*checked, cx);
                        })),
                ),
            )
    });

    let slowmode_section = is_text_or_ann.then(|| {
        let mut row = div().flex().flex_row().flex_wrap().gap_1();
        for &(secs, label) in SLOWMODE_OPTIONS {
            let sel = format!("edit-channel-slowmode-{secs}");
            row = row.child(
                div().debug_selector(move || sel.clone()).child(
                    Button::new(("edit-channel-slowmode", secs as u64))
                        .ghost()
                        .small()
                        .selected(modal.slowmode == secs)
                        .label(l10n::tr(label))
                        .on_click(cx.listener(move |this, _, _, cx| this.set_edit_channel_slowmode(secs, cx))),
                ),
            );
        }
        div().flex().flex_col().gap_1().child(section_label(l10n::tr("Slowmode"), theme)).child(row)
    });

    let parent_section = if modal.can_change_parent {
        let mut row = div().flex().flex_row().flex_wrap().gap_1();
        if modal.can_manage_root || modal.original_parent_id.is_none() {
            row = row.child(
                div().debug_selector(|| "edit-channel-parent-none".to_owned()).child(
                    Button::new("edit-channel-parent-none")
                        .ghost()
                        .small()
                        .selected(modal.parent_id.is_none())
                        .label(l10n::tr("No category"))
                        .on_click(cx.listener(|this, _, _, cx| this.set_edit_channel_parent(None, cx))),
                ),
            );
        }
        for (cat_id, cat_name) in &modal.categories {
            let cid = *cat_id;
            let sel = format!("edit-channel-parent-{}", cid.0);
            row = row.child(
                div().debug_selector(move || sel.clone()).child(
                    Button::new(("edit-channel-parent", cid.0))
                        .ghost()
                        .small()
                        .selected(modal.parent_id == Some(cid))
                        .label(cat_name.clone())
                        .on_click(cx.listener(move |this, _, _, cx| this.set_edit_channel_parent(Some(cid), cx))),
                ),
            );
        }
        Some(div().flex().flex_col().gap_1().child(section_label(l10n::tr("Category"), theme)).child(row)
            .when(modal.can_sync_parent || modal.sync_permissions, |d| d.child(
                Button::new("edit-channel-sync-parent").small().ghost().selected(modal.sync_permissions)
                    .label(l10n::tr("Sync permissions with category")).disabled(modal.submitting)
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Overlay::EditChannel(modal) = &mut this.overlay
                            && !modal.submitting && (modal.can_sync_parent || modal.sync_permissions)
                        {
                            modal.sync_permissions = !modal.sync_permissions;
                            cx.notify();
                        }
                    }))
            )))
    } else {
        None
    };

    let nsfw_section = (!is_cat).then(|| {
        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .child(div().text_sm().child("NSFW / Age-restricted"))
            .child(
                div().debug_selector(|| "edit-channel-nsfw".to_owned()).child(
                    Switch::new("edit-channel-nsfw")
                        .checked(modal.nsfw)
                        .on_change(cx.listener(|this, checked: &bool, _, cx| {
                            this.set_edit_channel_nsfw(*checked, cx);
                        })),
                ),
            )
    });

    let delete_label = if is_cat {
        l10n::tr("Delete category")
    } else {
        l10n::tr("Delete channel")
    };

    let footer = if modal.confirming_delete {
        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap_2()
            .pt_1()
            .child(
                div()
                    .text_xs()
                    .text_color(theme.danger)
                    .child(format!("{delete_label}: {}", modal.original_name)),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        div().debug_selector(|| "edit-channel-delete-cancel".to_owned()).child(
                            Button::new("edit-channel-delete-cancel")
                                .ghost()
                                .small()
                                .disabled(modal.submitting)
                                .label(l10n::tr("Cancel"))
                                .on_click(cx.listener(|this, _, window, cx| this.cancel_edit_channel_delete(window, cx))),
                        ),
                    )
                    .child(
                        div().debug_selector(|| "edit-channel-delete-confirm".to_owned()).child(
                            Button::new("edit-channel-delete-confirm")
                                .danger()
                                .small()
                                .disabled(modal.submitting)
                                .label(if modal.submitting { l10n::tr("Loading…") } else { delete_label })
                                .on_click(cx.listener(|this, _, _, cx| this.confirm_delete_channel(cx))),
                        ),
                    ),
            )
    } else {
        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap_2()
            .pt_1()
            .child(
                div().debug_selector(|| "edit-channel-delete".to_owned()).child(
                    Button::new("edit-channel-delete")
                        .danger()
                        .small()
                        .disabled(modal.submitting)
                        .label(delete_label)
                        .on_click(cx.listener(|this, _, window, cx| this.request_edit_channel_delete(window, cx))),
                ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        div().debug_selector(|| "edit-channel-cancel".to_owned()).child(
                            Button::new("edit-channel-cancel")
                                .ghost()
                                .small()
                                .label(l10n::tr("Cancel"))
                                .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
                        ),
                    )
                    .child(
                        div().debug_selector(|| "edit-channel-save".to_owned()).child(
                            Button::new("edit-channel-save")
                                .primary()
                                .small()
                                .disabled(!dirty || modal.submitting)
                                .label(if modal.submitting { l10n::tr("Loading…") } else { l10n::tr("Save changes") })
                                .on_click(cx.listener(|this, _, _, cx| this.submit_edit_channel(cx))),
                        ),
                    ),
            )
    };

    let content = div()
        .id("edit-channel-modal")
        .debug_selector(|| "edit-channel-modal".to_owned())
        .key_context(EDIT_CONTEXT)
        .track_focus(&modal.focus)
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .capture_action(cx.listener(|this, _: &Escape, window, cx| {
            cx.stop_propagation();
            this.handle_edit_channel_escape(window, cx);
        }))
        .child(div().text_lg().font_weight(FontWeight::BOLD).child(title))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(section_label(
                    if is_cat { l10n::tr("Category name") } else { l10n::tr("Channel name") },
                    theme,
                ))
                .child(Input::new(&modal.name_input).small()),
        )
        .children(topic_section)
        .children(announcement_section)
        .children(slowmode_section)
        .children(parent_section)
        .children(nsfw_section)
        .when_some(modal.error.clone(), |d, err| {
            d.child(div().text_xs().text_color(theme.danger).child(err))
        })
        .child(footer);

    let view = cx.weak_entity();
    overlay::modal(content, theme, move |window, cx| {
        let _ = view.update(cx, |v, cx| v.close_overlay(window, cx));
    })
}
