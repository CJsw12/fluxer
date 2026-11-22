//! Group DM creation modal (`Overlay::CreateGroupDm`), leave confirmation
//! (`Overlay::LeaveGroupDm`), and right-hand Group DM member/rename panel.

use std::collections::BTreeSet;

use fluxer_client_core::rest::{MAX_CHANNEL_NAME_UTF16, MAX_GROUP_DM_OTHER_RECIPIENTS};
use fluxer_client_core::state::{ChannelKind, ClientState, Snowflake, Status};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::{
    AnyElement, AppContext as _, Context, Entity, EntityInputHandler as _, FocusHandle, Focusable as _, FontWeight,
    SharedString, Subscription, Window, div, prelude::*, px,
};

use super::friends::{FriendRow, FriendsTab, build_friend_rows};
use super::avatar::{AvatarSpec, render_avatar};
use super::overlay;
use crate::client_view::ClientView;
use crate::format::{default_avatar_index, initials};
use crate::theme::Theme;
use crate::ui::l10n;
use crate::ui::overlay::Overlay;
use crate::view_model::{Place, avatar_url, channel_name};

pub const CREATE_CONTEXT: &str = "CreateGroupDmModal";
pub const LEAVE_CONTEXT: &str = "LeaveGroupDmModal";
pub const PANEL_WIDTH: f32 = 260.;
const AVATAR_SIZE: f32 = 30.;

pub struct CreateGroupDmModal {
    pub filter_input: Entity<InputState>,
    pub candidates: Vec<FriendRow>,
    pub selected: BTreeSet<Snowflake>,
    pub request_seq: Option<u64>,
    pub loading: bool,
    pub error: Option<String>,
    pub _subscription: Subscription,
}

pub struct LeaveGroupDmModal {
    pub channel_id: Snowflake,
    pub channel_name: SharedString,
    pub focus: FocusHandle,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupDmMemberRow {
    pub user_id: Snowflake,
    pub name: SharedString,
    pub initials: SharedString,
    pub avatar_url: Option<SharedString>,
    pub default_avatar: usize,
    pub status: Status,
    pub is_owner: bool,
    pub is_self: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupDmSnapshot {
    pub channel_id: Snowflake,
    pub custom_name: Option<String>,
    pub display_name: SharedString,
    pub owner_id: Option<Snowflake>,
    pub is_owner: bool,
    pub members: Vec<GroupDmMemberRow>,
    pub addable_friends: Vec<FriendRow>,
    pub at_member_cap: bool,
}

/// Builds a right-panel snapshot for a `ChannelKind::GroupDm` under the state lock.
/// Pass `include_addable = true` only when the inline "Add friends" picker is open
/// so unrelated chat dispatches skip sorting/allocating the full friend roster.
pub fn build_snapshot(
    state: &ClientState,
    media: Option<&str>,
    channel_id: Snowflake,
    include_addable: bool,
) -> Option<GroupDmSnapshot> {
    let ch = state.channels.get(&channel_id)?;
    if ch.kind != ChannelKind::GroupDm {
        return None;
    }
    let me = state.current_user.as_ref();
    let me_id = me.map(|u| u.id);
    let owner_id = ch.owner_id;
    let is_owner = me_id.is_some() && owner_id == me_id;
    let custom_name = ch.name.clone().filter(|n| !n.trim().is_empty());
    let display_name: SharedString = channel_name(ch).into();

    let mut seen = BTreeSet::new();
    let mut members = Vec::new();
    let users = me.into_iter().chain(ch.recipients.iter());
    for user in users {
        if !seen.insert(user.id) {
            continue;
        }
        let name = user.display_name().to_owned();
        let init = initials(&name);
        let url = media.and_then(|m| avatar_url(m, user));
        let status = state.presences.get(&user.id).map(|p| p.status).unwrap_or(Status::Offline);
        members.push(GroupDmMemberRow {
            user_id: user.id,
            name: name.into(),
            initials: init.into(),
            avatar_url: url.map(Into::into),
            default_avatar: default_avatar_index(user.id),
            status,
            is_owner: owner_id == Some(user.id),
            is_self: me_id == Some(user.id),
        });
    }
    members.sort_by(|a, b| {
        b.is_owner
            .cmp(&a.is_owner)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.user_id.cmp(&b.user_id))
    });

    let addable_friends: Vec<FriendRow> = if include_addable {
        build_friend_rows(state, media, FriendsTab::All)
            .into_iter()
            .filter(|f| !seen.contains(&f.user_id))
            .collect()
    } else {
        Vec::new()
    };
    let other_count = ch.recipients.iter().filter(|r| Some(r.id) != me_id).count();
    let at_member_cap = other_count >= MAX_GROUP_DM_OTHER_RECIPIENTS;

    Some(GroupDmSnapshot {
        channel_id,
        custom_name,
        display_name,
        owner_id,
        is_owner,
        members,
        addable_friends,
        at_member_cap,
    })
}

impl ClientView {
    pub(crate) fn open_create_group_dm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        let candidates = {
            let state = self.backend.state();
            build_friend_rows(&state, self.backend.media(), FriendsTab::All)
        };
        let filter_input = cx.new(|cx| InputState::new(window, cx).placeholder(l10n::tr("Search")));
        let subscription = cx.subscribe_in(&filter_input, window, |this: &mut Self, input_entity, ev: &InputEvent, window, cx| match ev {
            InputEvent::Change => cx.notify(),
            InputEvent::PressEnter { shift: false, .. } => {
                if input_entity.update(cx, |s, cx| s.marked_text_range(window, cx)).is_some() {
                    return;
                }
                this.submit_create_group_dm(cx);
            }
            _ => {}
        });
        let focus = filter_input.read(cx).focus_handle(cx);
        self.overlay = Overlay::CreateGroupDm(CreateGroupDmModal {
            filter_input,
            candidates,
            selected: BTreeSet::new(),
            request_seq: None,
            loading: false,
            error: None,
            _subscription: subscription,
        });
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(crate) fn refresh_create_group_dm_candidates(&mut self) {
        if !matches!(self.overlay, Overlay::CreateGroupDm(_)) {
            return;
        }
        let candidates = {
            let state = self.backend.state();
            build_friend_rows(&state, self.backend.media(), FriendsTab::All)
        };
        if let Overlay::CreateGroupDm(modal) = &mut self.overlay {
            let valid: BTreeSet<Snowflake> = candidates.iter().map(|c| c.user_id).collect();
            modal.selected.retain(|id| valid.contains(id));
            modal.candidates = candidates;
        }
    }

    pub(crate) fn toggle_group_dm_pick(&mut self, user_id: Snowflake, cx: &mut Context<Self>) {
        let Overlay::CreateGroupDm(modal) = &mut self.overlay else { return };
        if modal.loading {
            return;
        }
        if !modal.selected.remove(&user_id) {
            if modal.selected.len() >= MAX_GROUP_DM_OTHER_RECIPIENTS {
                modal.error = Some(format!("Maximum {MAX_GROUP_DM_OTHER_RECIPIENTS} members can be selected."));
                cx.notify();
                return;
            }
            modal.selected.insert(user_id);
        }
        modal.error = None;
        cx.notify();
    }

    pub(crate) fn submit_create_group_dm(&mut self, cx: &mut Context<Self>) {
        let recipients: Vec<Snowflake> = {
            let Overlay::CreateGroupDm(modal) = &mut self.overlay else { return };
            if modal.loading {
                return;
            }
            if modal.selected.is_empty() {
                modal.error = Some("Select at least one friend.".to_owned());
                cx.notify();
                return;
            }
            modal.selected.iter().copied().collect()
        };
        let seq = self.next_group_dm_seq();
        if let Overlay::CreateGroupDm(modal) = &mut self.overlay {
            modal.loading = true;
            modal.error = None;
            modal.request_seq = Some(seq);
        } else {
            return;
        }
        self.backend.create_group_dm(seq, recipients);
        cx.notify();
    }

    pub(crate) fn group_dm_created(&mut self, seq: u64, result: Result<Snowflake, String>, cx: &mut Context<Self>) {
        let matches_active = matches!(&self.overlay, Overlay::CreateGroupDm(modal) if modal.request_seq == Some(seq));
        if !matches_active {
            return;
        }
        match result {
            Ok(channel_id) => {
                self.overlay = Overlay::None;
                let (composer, handle) = (self.composer.clone(), self.window_handle);
                cx.defer(move |cx| {
                    let _ = handle.update(cx, |_, window, cx| {
                        let focus = composer.read(cx).focus_handle(cx);
                        window.focus(&focus, cx);
                    });
                });
                self.place = Place::Home;
                self.open_channel(None, channel_id, cx);
            }
            Err(err) => {
                if let Overlay::CreateGroupDm(modal) = &mut self.overlay {
                    modal.loading = false;
                    modal.error = Some(err);
                    cx.notify();
                }
            }
        }
    }

    pub(crate) fn open_leave_group_dm(&mut self, channel_id: Snowflake, window: &mut Window, cx: &mut Context<Self>) {
        let title = {
            let state = self.backend.state();
            let Some(ch) = state.channels.get(&channel_id) else { return };
            if ch.kind != ChannelKind::GroupDm {
                return;
            }
            SharedString::from(channel_name(ch))
        };
        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        let focus = cx.focus_handle();
        self.overlay = Overlay::LeaveGroupDm(LeaveGroupDmModal {
            channel_id,
            channel_name: title,
            focus: focus.clone(),
        });
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(crate) fn confirm_leave_group_dm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Overlay::LeaveGroupDm(modal) = &self.overlay else { return };
        let channel_id = modal.channel_id;
        self.close_overlay(window, cx);
        self.backend.leave_group_dm(channel_id, false);
    }

    pub(crate) fn group_dm_toggle_add(&mut self, cx: &mut Context<Self>) {
        if self.group_dm.is_none() {
            return;
        }
        self.group_dm_adding = !self.group_dm_adding;
        self.refresh(cx);
    }

    pub(crate) fn group_dm_save_name(&mut self, cx: &mut Context<Self>) {
        let Some(snap) = &self.group_dm else { return };
        let channel_id = snap.channel_id;
        let current_name = snap.custom_name.clone();
        let raw = self.group_dm_rename.read(cx).value().to_string();
        let trimmed = raw.trim().to_owned();
        if trimmed.encode_utf16().count() > MAX_CHANNEL_NAME_UTF16 {
            self.error = Some(format!("Group name exceeds {MAX_CHANNEL_NAME_UTF16} characters.").into());
            cx.notify();
            return;
        }
        let next = (!trimmed.is_empty()).then_some(trimmed);
        if next == current_name {
            return;
        }
        self.error = None;
        self.backend.rename_group_dm(channel_id, next);
        cx.notify();
    }

    pub(crate) fn group_dm_add_member(&mut self, user_id: Snowflake, cx: &mut Context<Self>) {
        let Some(snap) = &self.group_dm else { return };
        if snap.at_member_cap {
            return;
        }
        let channel_id = snap.channel_id;
        self.group_dm_adding = false;
        self.backend.add_group_dm_recipient(channel_id, user_id);
        cx.notify();
    }

    pub(crate) fn group_dm_remove_member(&mut self, user_id: Snowflake, cx: &mut Context<Self>) {
        let Some(snap) = &self.group_dm else { return };
        if !snap.is_owner {
            return;
        }
        let channel_id = snap.channel_id;
        self.backend.remove_group_dm_recipient(channel_id, user_id);
        cx.notify();
    }
}

pub fn render_create_modal(
    modal: &CreateGroupDmModal,
    theme: &Theme,
    cx: &mut Context<ClientView>,
    filter_query: &str,
) -> AnyElement {
    let needle = filter_query.trim().to_lowercase();
    let filtered: Vec<&FriendRow> = modal
        .candidates
        .iter()
        .filter(|f| {
            needle.is_empty()
                || f.display_name.to_lowercase().contains(&needle)
                || f.username_tag.to_lowercase().contains(&needle)
        })
        .collect();

    let mut list = div().id("create-group-dm-list").flex().flex_col().max_h(px(260.)).overflow_y_scroll().gap_1();
    if filtered.is_empty() {
        list = list.child(div().py_3().text_xs().text_color(theme.muted_foreground).child(l10n::tr("No results")));
    } else {
        for f in filtered {
            let uid = f.user_id;
            let picked = modal.selected.contains(&uid);
            let selector = format!("gdm-pick-{}", uid.0);
            list = list.child(
                div()
                    .id(("gdm-pick", uid.0))
                    .debug_selector(move || selector.clone())
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(picked, |d| d.bg(theme.accent))
                    .hover(|s| s.bg(theme.muted))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_group_dm_pick(uid, cx)))
                    .child(render_avatar(
                        AvatarSpec {
                            user_id: f.user_id,
                            avatar_url: f.avatar_url.clone(),
                            display_name: f.display_name.clone(),
                            size: AVATAR_SIZE,
                            status: None,
                            status_border_bg: None,
                        },
                        theme,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().truncate().text_sm().font_weight(FontWeight::MEDIUM).child(f.display_name.clone()))
                            .child(div().truncate().text_xs().text_color(theme.muted_foreground).child(f.username_tag.clone())),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(if picked { theme.brand } else { theme.muted_foreground })
                            .child(if picked { "✓" } else { "+" }),
                    ),
            );
        }
    }

    let selected_count = modal.selected.len();
    let content = div()
        .id("create-group-dm-modal")
        .debug_selector(|| "create-group-dm-modal".to_owned())
        .key_context(CREATE_CONTEXT)
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .capture_action(cx.listener(|this, _: &Escape, window, cx| {
            cx.stop_propagation();
            this.close_overlay(window, cx);
        }))
        .child(div().text_lg().font_weight(FontWeight::BOLD).child(l10n::tr("Create group DM")))
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(format!("{selected_count}/{MAX_GROUP_DM_OTHER_RECIPIENTS} {}", l10n::tr("Members"))),
        )
        .child(Input::new(&modal.filter_input).small())
        .child(list)
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
                    Button::new("create-group-dm-cancel")
                        .ghost()
                        .small()
                        .label(l10n::tr("Cancel"))
                        .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
                )
                .child(
                    div().debug_selector(|| "create-group-dm-submit".to_owned()).child(
                        Button::new("create-group-dm-submit")
                            .primary()
                            .small()
                            .label(if modal.loading { l10n::tr("Loading…") } else { l10n::tr("Create group DM") })
                            .on_click(cx.listener(|this, _, _, cx| this.submit_create_group_dm(cx))),
                    ),
                ),
        );

    let view = cx.weak_entity();
    overlay::modal(content, theme, move |window, cx| {
        let _ = view.update(cx, |v, cx| v.close_overlay(window, cx));
    })
}

pub fn render_leave_modal(modal: &LeaveGroupDmModal, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let content = div()
        .id("leave-group-dm-modal")
        .debug_selector(|| "leave-group-dm-modal".to_owned())
        .key_context(LEAVE_CONTEXT)
        .track_focus(&modal.focus)
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .capture_action(cx.listener(|this, _: &Escape, window, cx| {
            cx.stop_propagation();
            this.close_overlay(window, cx);
        }))
        .child(div().text_lg().font_weight(FontWeight::BOLD).child(l10n::tr("Leave group")))
        .child(div().text_sm().text_color(theme.muted_foreground).child(modal.channel_name.clone()))
        .child(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("leave-group-dm-cancel")
                        .ghost()
                        .small()
                        .label(l10n::tr("Cancel"))
                        .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
                )
                .child(
                    div().debug_selector(|| "leave-group-dm-confirm".to_owned()).child(
                        Button::new("leave-group-dm-confirm")
                            .danger()
                            .small()
                            .label(l10n::tr("Leave group"))
                            .on_click(cx.listener(|this, _, window, cx| this.confirm_leave_group_dm(window, cx))),
                    ),
                ),
        );

    let view = cx.weak_entity();
    overlay::modal(content, theme, move |window, cx| {
        let _ = view.update(cx, |v, cx| v.close_overlay(window, cx));
    })
}

pub fn render_panel(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let Some(snap) = &view.group_dm else {
        return div().into_any_element();
    };
    let channel_id = snap.channel_id;
    let mut member_list = div().id("group-dm-members").flex().flex_col().flex_1().min_h_0().overflow_y_scroll().p_3().gap_2();
    for m in &snap.members {
        let uid = m.user_id;
        let can_remove = snap.is_owner && !m.is_self;
        let remove_sel = format!("gdm-remove-{}", uid.0);
        let picture = render_avatar(
            AvatarSpec {
                user_id: m.user_id,
                avatar_url: m.avatar_url.clone(),
                display_name: m.name.clone(),
                size: AVATAR_SIZE,
                status: Some(m.status),
                status_border_bg: Some(theme.background_secondary),
            },
            theme,
        );

        member_list = member_list.child(
            div()
                .id(("gdm-member", uid.0))
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .py_1()
                .child(picture)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .child(div().truncate().text_sm().font_weight(FontWeight::MEDIUM).child(m.name.clone()))
                        .when(m.is_owner, |d| {
                            d.child(div().text_xs().text_color(theme.brand).child("★"))
                        }),
                )
                .when(can_remove, |d| {
                    d.child(
                        div().debug_selector(move || remove_sel.clone()).child(
                            Button::new(("gdm-remove", uid.0))
                                .ghost()
                                .small()
                                .label(l10n::tr("Remove"))
                                .on_click(cx.listener(move |v, _, _, cx| v.group_dm_remove_member(uid, cx))),
                        ),
                    )
                }),
        );
    }

    let add_section = if view.group_dm_adding {
        let mut picker = div()
            .id("group-dm-add-picker")
            .flex()
            .flex_col()
            .max_h(px(180.))
            .overflow_y_scroll()
            .gap_1()
            .p_2()
            .rounded(px(6.))
            .bg(theme.background)
            .border_1()
            .border_color(theme.border);
        if snap.addable_friends.is_empty() {
            picker = picker.child(div().text_xs().text_color(theme.muted_foreground).child(l10n::tr("No results")));
        } else {
            for f in &snap.addable_friends {
                let uid = f.user_id;
                let add_sel = format!("gdm-add-{}", uid.0);
                picker = picker.child(
                    div()
                        .id(("gdm-add-row", uid.0))
                        .debug_selector(move || add_sel.clone())
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .px_2()
                        .py_1()
                        .rounded(px(4.))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.muted))
                        .on_click(cx.listener(move |v, _, _, cx| v.group_dm_add_member(uid, cx)))
                        .child(div().flex_1().min_w_0().truncate().text_xs().child(f.display_name.clone()))
                        .child(div().text_xs().text_color(theme.brand).child("+")),
                );
            }
        }
        Some(picker)
    } else {
        None
    };

    div()
        .id("group-dm-panel")
        .debug_selector(|| "group-dm-panel".to_owned())
        .flex()
        .flex_col()
        .flex_none()
        .w(px(PANEL_WIDTH))
        .h_full()
        .border_l_1()
        .border_color(theme.border)
        .bg(theme.background_secondary)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .px_3()
                .py_2()
                .border_b_1()
                .border_color(theme.border)
                .child(div().text_sm().font_weight(FontWeight::BOLD).child(format!("{} — {}", l10n::tr("Members"), snap.members.len())))
                .when(!snap.at_member_cap, |d| {
                    d.child(
                        div().debug_selector(|| "gdm-add-toggle".to_owned()).child(
                            Button::new("gdm-add-toggle")
                                .ghost()
                                .small()
                                .label(l10n::tr("Add friends"))
                                .on_click(cx.listener(|v, _, _, cx| v.group_dm_toggle_add(cx))),
                        ),
                    )
                }),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .p_3()
                .border_b_1()
                .border_color(theme.border)
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .child(div().flex_1().min_w_0().child(Input::new(&view.group_dm_rename).small()))
                        .child(
                            div().debug_selector(|| "gdm-rename-save".to_owned()).child(
                                Button::new("gdm-rename-save")
                                    .ghost()
                                    .small()
                                    .label(l10n::tr("Save"))
                                    .on_click(cx.listener(|v, _, _, cx| v.group_dm_save_name(cx))),
                            ),
                        ),
                )
                .children(add_section),
        )
        .child(member_list)
        .child(
            div()
                .p_3()
                .border_t_1()
                .border_color(theme.border)
                .child(
                    div().debug_selector(|| "gdm-leave-btn".to_owned()).child(
                        Button::new("gdm-leave-btn")
                            .danger()
                            .small()
                            .label(l10n::tr("Leave group"))
                            .on_click(cx.listener(move |v, _, window, cx| v.open_leave_group_dm(channel_id, window, cx))),
                    ),
                ),
        )
        .into_any_element()
}
