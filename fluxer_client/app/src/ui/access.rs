//! Guild roles and per-channel permission overwrites.

use fluxer_client_core::rest::{RolePatch, validate_role_name};
use fluxer_client_core::state::{ClientState, PermissionOverwrite, Role, Snowflake, bits};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::{Disableable as _, Selectable as _, Sizable as _};
use gpui_kit::{
    AnyElement, AppContext as _, Context, Entity, EntityInputHandler as _, FocusHandle, Focusable as _, FontWeight,
    SharedString, Subscription, Window, div, prelude::*, px,
};

use crate::backend::access::{AccessChange, find_overwrite};
use crate::client_view::ClientView;
use crate::theme::Theme;
use crate::ui::{
    l10n,
    overlay::{self, Overlay},
};
use crate::view_model::Place;

pub const CONTEXT: &str = "AccessEditor";

#[derive(Clone, Copy)]
pub enum Scope {
    Guild(Snowflake),
    Channel { guild: Snowflake, channel: Snowflake },
}

impl Scope {
    fn guild(self) -> Snowflake {
        match self {
            Self::Guild(guild) | Self::Channel { guild, .. } => guild,
        }
    }

    fn allows(self, place: Place, state: &ClientState) -> bool {
        place == Place::Guild(self.guild())
            && match self {
                Self::Guild(guild) => state.can_manage_guild_roles(guild),
                Self::Channel { guild, channel } => {
                    state.channels.get(&channel).is_some_and(|c| c.guild_id == Some(guild))
                        && state.can_manage_channel_permissions(channel)
                }
            }
    }
}

enum Draft {
    List,
    NewRole,
    Role {
        before: Role,
        permissions: u64,
        hoist: bool,
        mentionable: bool,
    },
    Overwrite {
        label: String,
        before: Option<PermissionOverwrite>,
        after: PermissionOverwrite,
    },
}

pub struct AccessModal {
    scope: Scope,
    draft: Draft,
    input: Entity<InputState>,
    focus: FocusHandle,
    confirming_delete: bool,
    error: Option<String>,
    _subscription: Subscription,
}

impl AccessModal {
    pub fn allowed(&self, place: Place, state: &ClientState) -> bool {
        self.scope.allows(place, state)
    }
}

impl ClientView {
    pub(crate) fn open_access(&mut self, scope: Scope, window: &mut Window, cx: &mut Context<Self>) {
        if self.access_pending.is_some() || !scope.allows(self.place, &self.backend.state()) {
            return;
        }
        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(l10n::tr("Role name")));
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this: &mut Self, input, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { shift: false, .. }
                    if input.update(cx, |s, cx| s.marked_text_range(window, cx)).is_none() =>
                {
                    this.submit_access(false, cx);
                }
                InputEvent::Change => cx.notify(),
                _ => {}
            },
        );
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        self.overlay = Overlay::Access(AccessModal {
            scope,
            draft: Draft::List,
            input,
            focus,
            confirming_delete: false,
            error: None,
            _subscription: subscription,
        });
        cx.notify();
    }

    fn change_access_draft(&mut self, draft: Draft, window: &mut Window, cx: &mut Context<Self>) {
        let Overlay::Access(modal) = &mut self.overlay else {
            return;
        };
        if self.access_pending.is_some() {
            return;
        }
        match &draft {
            Draft::NewRole => {
                modal.input.update(cx, |input, cx| input.set_value("", window, cx));
                modal.input.read(cx).focus_handle(cx).focus(window, cx);
            }
            Draft::Role { before, .. } if before.id != modal.scope.guild() => {
                modal
                    .input
                    .update(cx, |input, cx| input.set_value(&before.name, window, cx));
                modal.input.read(cx).focus_handle(cx).focus(window, cx);
            }
            _ => modal.focus.focus(window, cx),
        }
        modal.draft = draft;
        modal.confirming_delete = false;
        modal.error = None;
        cx.notify();
    }

    fn set_permission(&mut self, bit: u64, value: Option<bool>, cx: &mut Context<Self>) {
        let Overlay::Access(modal) = &mut self.overlay else {
            return;
        };
        if self.access_pending.is_some() {
            return;
        }
        match &mut modal.draft {
            Draft::Role { permissions, .. } => {
                if value == Some(true) {
                    *permissions |= bit;
                } else {
                    *permissions &= !bit;
                }
            }
            Draft::Overwrite { after, .. } => {
                after.allow &= !bit;
                after.deny &= !bit;
                match value {
                    Some(true) => after.allow |= bit,
                    Some(false) => after.deny |= bit,
                    None => {}
                }
            }
            _ => return,
        }
        cx.notify();
    }

    fn submit_access(&mut self, delete: bool, cx: &mut Context<Self>) {
        let Overlay::Access(modal) = &mut self.overlay else {
            return;
        };
        if self.access_pending.is_some() || (!delete && modal.confirming_delete) {
            return;
        }
        let guild = modal.scope.guild();
        let name = modal.input.read(cx).value().trim().to_owned();
        let change = match &modal.draft {
            Draft::NewRole if !delete => AccessChange::CreateRole { guild, name },
            Draft::Role {
                before,
                permissions,
                hoist,
                mentionable,
            } if !delete => {
                let patch = RolePatch {
                    name: (before.id != guild && name != before.name).then_some(name),
                    permissions: (*permissions != before.permissions).then_some(*permissions),
                    hoist: (*hoist != before.hoist).then_some(*hoist),
                    mentionable: (*mentionable != before.mentionable).then_some(*mentionable),
                };
                if patch == RolePatch::default() {
                    return;
                }
                AccessChange::EditRole {
                    guild,
                    before: before.clone(),
                    patch,
                }
            }
            Draft::Role { before, .. } if modal.confirming_delete => AccessChange::DeleteRole {
                guild,
                before: before.clone(),
            },
            Draft::Overwrite { before, after, .. } => {
                let Scope::Channel { channel, .. } = modal.scope else {
                    return;
                };
                if delete && (!modal.confirming_delete || before.is_none())
                    || !delete
                        && (before.as_ref() == Some(after) || before.is_none() && after.allow | after.deny == 0)
                {
                    return;
                }
                AccessChange::Overwrite {
                    channel,
                    target: after.id,
                    before: before.clone(),
                    after: (!delete).then(|| after.clone()),
                }
            }
            _ => return,
        };
        let name = match &change {
            AccessChange::CreateRole { name, .. } => Some(name),
            AccessChange::EditRole { patch, .. } => patch.name.as_ref(),
            _ => None,
        };
        if name.is_some_and(|name| validate_role_name(name).is_err()) {
            modal.error = Some(l10n::tr("Role name must be 1–100 characters."));
            cx.notify();
            return;
        }
        self.start_access_change(change, cx);
    }

    fn start_access_change(&mut self, change: AccessChange, cx: &mut Context<Self>) {
        if self.access_pending.is_some() {
            return;
        }
        let seq = self.next_channel_mgmt_seq();
        if let Overlay::Access(modal) = &mut self.overlay {
            modal.error = None;
            modal.confirming_delete = false;
        }
        self.access_pending = Some(seq);
        self.backend.change_access(seq, change);
        cx.notify();
    }

    pub(crate) fn finish_access_change(&mut self, seq: u64, error: Option<String>, cx: &mut Context<Self>) {
        if self.access_pending != Some(seq) {
            return;
        }
        self.access_pending = None;
        if let Overlay::Access(modal) = &mut self.overlay {
            if error.is_none() {
                modal.draft = Draft::List;
                let focus = modal.focus.clone();
                let _ = self.window_handle.update(cx, |_, window, cx| focus.focus(window, cx));
            }
            modal.error = error;
        } else if let Some(error) = error {
            self.error = Some(error.into());
        }
        cx.notify();
    }
}

// (permission, label, available in the channel editor)
const PERMISSIONS: &[(u64, &str, bool)] = &[
    (bits::ADMINISTRATOR, "Administrator", false),
    (bits::MANAGE_GUILD, "Manage community", false),
    (bits::MANAGE_ROLES, "Manage roles", true),
    (bits::MANAGE_CHANNELS, "Manage channels", true),
    (bits::VIEW_AUDIT_LOG, "View activity log", false),
    (bits::KICK_MEMBERS, "Kick members", false),
    (bits::BAN_MEMBERS, "Ban members", false),
    (bits::MODERATE_MEMBERS, "Timeout members", false),
    (bits::CHANGE_NICKNAME, "Change nickname", false),
    (bits::MANAGE_NICKNAMES, "Manage nicknames", false),
    (bits::CREATE_EXPRESSIONS, "Create expressions", false),
    (bits::MANAGE_EXPRESSIONS, "Manage expressions", false),
    (bits::MANAGE_WEBHOOKS, "Manage webhooks", true),
    (bits::CREATE_INSTANT_INVITE, "Create invite links", true),
    (bits::VIEW_CHANNEL, "View channel", true),
    (bits::VIEW_CHANNEL_MEMBERS, "View channel members", true),
    (bits::SEND_MESSAGES, "Send messages", true),
    (bits::SEND_TTS_MESSAGES, "Send text-to-speech messages", true),
    (bits::READ_MESSAGE_HISTORY, "Read message history", true),
    (bits::MANAGE_MESSAGES, "Manage messages", true),
    (bits::PIN_MESSAGES, "Pin messages", true),
    (bits::EMBED_LINKS, "Embed links", true),
    (bits::ATTACH_FILES, "Attach files", true),
    (bits::ADD_REACTIONS, "Add reactions", true),
    (bits::MENTION_EVERYONE, "Mention everyone", true),
    (bits::USE_EXTERNAL_EMOJIS, "Use external emoji", true),
    (bits::USE_EXTERNAL_STICKERS, "Use external stickers", true),
    (bits::BYPASS_SLOWMODE, "Bypass slowmode", true),
    (bits::CONNECT, "Connect", true),
    (bits::SPEAK, "Speak", true),
    (bits::STREAM, "Video and screen share", true),
    (bits::PRIORITY_SPEAKER, "Priority speaker", true),
    (bits::USE_VAD, "Use voice activity", true),
    (bits::MUTE_MEMBERS, "Mute members", true),
    (bits::DEAFEN_MEMBERS, "Deafen members", true),
    (bits::MOVE_MEMBERS, "Move members", true),
    (bits::UPDATE_RTC_REGION, "Change voice region", true),
];

fn permission_rows(
    allow: u64,
    overwrite: Option<(u64, Option<&PermissionOverwrite>)>,
    editable: u64,
    busy: bool,
    cx: &mut Context<ClientView>,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let mut list = div()
        .id("access-permission-list")
        .max_h(px(235.))
        .overflow_y_scroll()
        .flex()
        .flex_col();
    for &(bit, label, channel) in PERMISSIONS {
        if overwrite.is_some() && !channel {
            continue;
        }
        let mut row = div()
            .flex()
            .items_center()
            .gap_2()
            .py_1()
            .child(div().flex_1().child(l10n::tr(label)));
        if let Some((deny, before)) = overwrite {
            let (saved_allow, saved_deny) = before.map_or((0, 0), |o| (o.allow, o.deny));
            for (i, value, label, selected) in [
                (0, Some(true), "Allow", allow & bit != 0),
                (1, None, "Inherit", (allow | deny) & bit == 0),
                (2, Some(false), "Deny", deny & bit != 0),
            ] {
                let would_grant =
                    value == Some(true) && saved_allow & bit == 0 || value != Some(false) && saved_deny & bit != 0;
                row = row.child(
                    Button::new(("overwrite-perm", bit * 4 + i))
                        .small()
                        .ghost()
                        .selected(selected)
                        .label(l10n::tr(label))
                        .disabled(busy || would_grant && editable & bit == 0)
                        .on_click(cx.listener(move |this, _, _, cx| this.set_permission(bit, value, cx))),
                );
            }
        } else {
            let enabled = allow & bit != 0;
            row = row.child(
                Button::new(("role-perm", bit))
                    .small()
                    .ghost()
                    .selected(enabled)
                    .label(l10n::tr(if enabled { "On" } else { "Off" }))
                    .disabled(busy || !enabled && editable & bit == 0)
                    .on_click(cx.listener(move |this, _, _, cx| this.set_permission(bit, Some(!enabled), cx))),
            );
        }
        list = list.child(row);
    }
    list
}

fn role_rows(
    state: &ClientState,
    guild: Snowflake,
    busy: bool,
    cx: &mut Context<ClientView>,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let mut roles: Vec<_> = state.guilds[&guild].roles.values().collect();
    roles.sort_by_key(|role| (role.id == guild, std::cmp::Reverse(role.position), role.id));
    let mut list = div()
        .id("access-role-list")
        .max_h(px(295.))
        .overflow_y_scroll()
        .flex()
        .flex_col();
    for (index, role) in roles.iter().enumerate() {
        let editable = state.can_manage_role(guild, role.id);
        let id = role.id.0;
        let role = (*role).clone();
        let label = if role.id == guild {
            "@everyone".to_owned()
        } else {
            role.name.clone()
        };
        let mut row = div().flex().items_center().gap_2().py_1().child(
            Button::new(("access-role", id))
                .small()
                .ghost()
                .label(label)
                .disabled(busy || !editable)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.change_access_draft(
                        Draft::Role {
                            permissions: role.permissions,
                            hoist: role.hoist,
                            mentionable: role.mentionable,
                            before: role.clone(),
                        },
                        window,
                        cx,
                    );
                })),
        );
        if editable && id != guild.0 {
            for (offset, title) in [(-1, "Move up"), (1, "Move down")] {
                let adjacent = index
                    .checked_add_signed(offset)
                    .and_then(|index| roles.get(index))
                    .filter(|role| role.id != guild && state.can_manage_role(guild, role.id));
                if let Some(adjacent) = adjacent {
                    let adjacent = adjacent.id;
                    row = row.child(
                        Button::new((title, id))
                            .small()
                            .ghost()
                            .label(l10n::tr(title))
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.start_access_change(
                                    AccessChange::MoveRole {
                                        guild,
                                        role: Snowflake(id),
                                        adjacent,
                                    },
                                    cx,
                                )
                            })),
                    );
                }
            }
        }
        list = list.child(row);
    }
    list
}

fn overwrite_rows(
    state: &ClientState,
    guild: Snowflake,
    channel: Snowflake,
    busy: bool,
    cx: &mut Context<ClientView>,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let mut targets: Vec<_> = state.guilds[&guild]
        .roles
        .values()
        .map(|role| {
            (
                role.id,
                0,
                if role.id == guild {
                    "@everyone".to_owned()
                } else {
                    role.name.clone()
                },
            )
        })
        .collect();
    // shortcut: only cached members appear here, add server search for large guilds.
    if let Some(members) = state.members.get(&guild) {
        targets.extend(members.iter().map(|(&id, member)| {
            (
                id,
                1,
                member
                    .nick
                    .as_deref()
                    .unwrap_or_else(|| member.user.display_name())
                    .to_owned(),
            )
        }));
    }
    for existing in &state.channels[&channel].permission_overwrites {
        if !targets.iter().any(|(id, _, _)| *id == existing.id) {
            targets.push((existing.id, existing.kind, existing.id.to_string()));
        }
    }
    targets.sort_unstable_by(|a, b| (a.0 != guild, a.1, &a.2, a.0).cmp(&(b.0 != guild, b.1, &b.2, b.0)));
    let mut list = div()
        .id("access-overwrite-list")
        .max_h(px(320.))
        .overflow_y_scroll()
        .flex()
        .flex_col();
    for (id, kind, name) in targets {
        let before = find_overwrite(state, channel, id).cloned();
        let after = before.clone().unwrap_or(PermissionOverwrite {
            id,
            kind,
            allow: 0,
            deny: 0,
        });
        let tag = if kind == 0 { "Role" } else { "Member" };
        list = list.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .py_1()
                .child(div().flex_1().min_w_0().truncate().child(name.clone()))
                .child(div().child(l10n::tr(tag)))
                .child(
                    Button::new(("access-overwrite", id.0))
                        .small()
                        .ghost()
                        .label(l10n::tr(if before.is_some() { "Edit" } else { "Add" }))
                        .disabled(busy)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.change_access_draft(
                                Draft::Overwrite {
                                    label: name.clone(),
                                    before: before.clone(),
                                    after: after.clone(),
                                },
                                window,
                                cx,
                            )
                        })),
                ),
        );
    }
    list
}

pub fn render(view: &ClientView, modal: &AccessModal, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let busy = view.access_pending.is_some();
    let state = view.backend.state();
    let guild = modal.scope.guild();
    let title = match modal.scope {
        Scope::Guild(_) => "Roles",
        Scope::Channel { .. } => "Channel permissions",
    };
    let mut body = div().flex().flex_col().gap_2();
    let mut can_delete = false;
    let mut save = false;
    let mut can_save = true;
    let mut stale = false;
    if modal.allowed(view.place, &state) {
        match &modal.draft {
            Draft::List => match modal.scope {
                Scope::Guild(_) => {
                    body = body
                        .child(
                            Button::new("access-new-role")
                                .small()
                                .label(l10n::tr("Create role"))
                                .disabled(busy)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.change_access_draft(Draft::NewRole, window, cx)
                                })),
                        )
                        .child(role_rows(&state, guild, busy, cx));
                }
                Scope::Channel { channel, .. } => {
                    body = body.child(overwrite_rows(&state, guild, channel, busy, cx))
                }
            },
            Draft::NewRole => {
                save = true;
                body = body
                    .child(div().child(l10n::tr("Role name")))
                    .child(Input::new(&modal.input).small());
            }
            Draft::Role {
                before,
                permissions,
                hoist,
                mentionable,
            } => {
                let editable = state.can_manage_role(guild, before.id)
                    && state.guilds[&guild].roles.get(&before.id) == Some(before);
                stale = !editable;
                let own = state.guild_permissions(guild);
                save = true;
                can_delete = editable && before.id != guild;
                can_save = editable
                    && (*permissions == before.permissions
                        || state.can_write_role_permissions(guild, *permissions));
                body = body
                    .when(before.id != guild, |body| {
                        body.child(div().child(l10n::tr("Role name")))
                            .child(Input::new(&modal.input).small())
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(
                                        Button::new("access-role-hoist")
                                            .small()
                                            .ghost()
                                            .selected(*hoist)
                                            .label(l10n::tr("Display role members separately"))
                                            .disabled(busy || !editable)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                if let Overlay::Access(modal) = &mut this.overlay
                                                    && let Draft::Role { hoist, .. } = &mut modal.draft
                                                {
                                                    *hoist = !*hoist;
                                                    cx.notify();
                                                }
                                            })),
                                    )
                                    .child(
                                        Button::new("access-role-mentionable")
                                            .small()
                                            .ghost()
                                            .selected(*mentionable)
                                            .label(l10n::tr("Allow mentions for this role"))
                                            .disabled(busy || !editable)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                if let Overlay::Access(modal) = &mut this.overlay
                                                    && let Draft::Role { mentionable, .. } = &mut modal.draft
                                                {
                                                    *mentionable = !*mentionable;
                                                    cx.notify();
                                                }
                                            })),
                                    ),
                            )
                    })
                    .child(permission_rows(*permissions, None, own, busy || !editable, cx));
            }
            Draft::Overwrite { label, before, after } => {
                let Scope::Channel { channel, .. } = modal.scope else {
                    unreachable!()
                };
                let editable = find_overwrite(&state, channel, after.id) == before.as_ref();
                stale = !editable;
                save = true;
                can_delete = editable
                    && before.is_some()
                    && state.can_write_channel_overwrite(channel, before.as_ref(), None);
                let changed = before.as_ref() != Some(after) && (before.is_some() || after.allow | after.deny != 0);
                can_save =
                    editable && changed && state.can_write_channel_overwrite(channel, before.as_ref(), Some(after));
                body = body.child(div().child(label.clone())).child(permission_rows(
                    after.allow,
                    Some((after.deny, before.as_ref())),
                    state.channel_permissions(channel),
                    busy || !editable,
                    cx,
                ));
            }
        }
    }
    drop(state);
    let mut footer = div().flex().justify_end().gap_2();
    if save {
        footer = footer.child(
            Button::new("access-back")
                .small()
                .ghost()
                .label(l10n::tr("Back"))
                .disabled(busy)
                .on_click(cx.listener(|this, _, window, cx| this.change_access_draft(Draft::List, window, cx))),
        );
        if can_delete {
            let confirm = modal.confirming_delete;
            footer = footer.child(
                Button::new("access-delete")
                    .small()
                    .danger()
                    .label(l10n::tr(if confirm { "Confirm deletion" } else { "Delete" }))
                    .disabled(busy)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if confirm {
                            this.submit_access(true, cx);
                        } else if let Overlay::Access(modal) = &mut this.overlay {
                            modal.confirming_delete = true;
                            cx.notify();
                        }
                    })),
            );
        }
        footer = footer.child(
            Button::new("access-save")
                .small()
                .primary()
                .label(l10n::tr("Save"))
                .disabled(busy || !can_save || modal.confirming_delete)
                .on_click(cx.listener(|this, _, _, cx| this.submit_access(false, cx))),
        );
    } else {
        footer = footer.child(
            Button::new("access-close")
                .small()
                .ghost()
                .label(l10n::tr("Close"))
                .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
        );
    }
    let error = modal
        .error
        .clone()
        .or_else(|| stale.then(|| l10n::tr("This item changed. Go back and reopen it.")));
    let content = div()
        .id("access-modal")
        .debug_selector(|| "access-modal".to_owned())
        .key_context(CONTEXT)
        .track_focus(&modal.focus)
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .capture_action(cx.listener(|this, _: &Escape, window, cx| {
            cx.stop_propagation();
            this.close_overlay(window, cx);
        }))
        .child(div().text_lg().font_weight(FontWeight::BOLD).child(l10n::tr(title)))
        .child(body)
        .when_some(error.map(SharedString::from), |d, error| {
            d.child(div().text_color(theme.danger).child(error))
        })
        .child(footer);
    let view = cx.weak_entity();
    overlay::modal(content, theme, move |window, cx| {
        let _ = view.update(cx, |this, cx| this.close_overlay(window, cx));
    })
}

#[cfg(test)]
impl ClientView {
    pub(crate) fn test_access_create_role(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.change_access_draft(Draft::NewRole, window, cx);
        if let Overlay::Access(modal) = &self.overlay {
            modal.input.update(cx, |input, cx| input.set_value(name, window, cx));
        }
        self.submit_access(false, cx);
    }

    pub(crate) fn test_access_edit_role(
        &mut self,
        before: Role,
        new_name: &str,
        add_perm: u64,
        hoist: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let permissions = before.permissions;
        let mentionable = before.mentionable;
        self.change_access_draft(
            Draft::Role {
                before,
                permissions,
                hoist,
                mentionable,
            },
            window,
            cx,
        );
        if let Overlay::Access(modal) = &self.overlay {
            modal
                .input
                .update(cx, |input, cx| input.set_value(new_name, window, cx));
        }
        self.set_permission(add_perm, Some(true), cx);
        self.submit_access(false, cx);
    }

    pub(crate) fn test_access_delete_role(&mut self, before: Role, window: &mut Window, cx: &mut Context<Self>) {
        let permissions = before.permissions;
        let hoist = before.hoist;
        let mentionable = before.mentionable;
        self.change_access_draft(
            Draft::Role {
                before,
                permissions,
                hoist,
                mentionable,
            },
            window,
            cx,
        );
        if let Overlay::Access(modal) = &mut self.overlay {
            modal.confirming_delete = true;
        }
        self.submit_access(true, cx);
    }
}
