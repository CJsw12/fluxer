//! Home Friends surface (`Online`, `All`, `Pending`, `Blocked`, `Add friend`)
//! and per-relationship actions.

use fluxer_client_core::state::{ChannelKind, ClientState, RelationshipKind, Snowflake, Status, User};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::{AnyElement, ClickEvent, Context, FontWeight, SharedString, div, prelude::*, px};

use super::badge;
use super::member_list::status_label;
use super::avatar::{AvatarSpec, render_avatar};
use super::profile_popout::find_1on1_dm;
use crate::client_view::ClientView;
use crate::format::{default_avatar_index, initials};
use crate::profile::format_username_tag;
use crate::theme::Theme;
use crate::ui::l10n;
use crate::view_model::{Place, avatar_url};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FriendsTab {
    #[default]
    Online,
    All,
    Pending,
    Blocked,
    AddFriend,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FriendRow {
    pub user_id: Snowflake,
    pub display_name: SharedString,
    pub username_tag: SharedString,
    pub initials: SharedString,
    pub avatar_url: Option<SharedString>,
    pub default_avatar: usize,
    pub status: Status,
    pub kind: RelationshipKind,
}

fn user_status(state: &ClientState, user_id: Snowflake) -> Status {
    state.presences.get(&user_id).map(|p| p.status).unwrap_or(Status::Offline)
}

fn fallback_user(user_id: Snowflake) -> (String, String, String, Option<String>) {
    let name = format!("User {user_id}");
    let init = initials(&name);
    (name.clone(), name, init, None)
}

fn user_display(state: &ClientState, media: Option<&str>, user_id: Snowflake) -> (String, String, String, Option<String>) {
    let Some(user): Option<&User> = state
        .users
        .get(&user_id)
        .or_else(|| state.channels.values().flat_map(|c| &c.recipients).find(|u| u.id == user_id))
        .or_else(|| state.members.values().find_map(|m| m.get(&user_id).map(|mem| &mem.user)))
    else {
        return fallback_user(user_id);
    };
    let display = user.display_name().to_owned();
    let tag = format_username_tag(&user.username, user.discriminator.as_deref());
    let init = initials(&display);
    let url = media.and_then(|m| avatar_url(m, user));
    (display, tag, init, url)
}

pub fn pending_incoming_count(state: &ClientState) -> u32 {
    let count = state
        .relationships
        .values()
        .filter(|r| r.kind() == RelationshipKind::IncomingRequest)
        .count();
    u32::try_from(count).unwrap_or(u32::MAX)
}

pub fn build_friend_rows(state: &ClientState, media: Option<&str>, tab: FriendsTab) -> Vec<FriendRow> {
    if tab == FriendsTab::AddFriend {
        return Vec::new();
    }
    let mut rows: Vec<FriendRow> = state
        .relationships
        .values()
        .filter_map(|rel| {
            let kind = rel.kind();
            let status = user_status(state, rel.id);
            let keep = match tab {
                FriendsTab::Online => kind == RelationshipKind::Friend && !status.is_offline(),
                FriendsTab::All => kind == RelationshipKind::Friend,
                FriendsTab::Pending => {
                    matches!(kind, RelationshipKind::IncomingRequest | RelationshipKind::OutgoingRequest)
                }
                FriendsTab::Blocked => kind == RelationshipKind::Blocked,
                FriendsTab::AddFriend => false,
            };
            if !keep {
                return None;
            }
            let (display, tag, init, url) = user_display(state, media, rel.id);
            let shown_name = rel
                .nickname
                .as_deref()
                .filter(|n| !n.trim().is_empty())
                .map(str::to_owned)
                .unwrap_or(display);
            Some(FriendRow {
                user_id: rel.id,
                display_name: shown_name.into(),
                username_tag: tag.into(),
                initials: init.into(),
                avatar_url: url.map(Into::into),
                default_avatar: default_avatar_index(rel.id),
                status,
                kind,
            })
        })
        .collect();
    rows.sort_by(|a, b| {
        a.display_name
            .to_ascii_lowercase()
            .cmp(&b.display_name.to_ascii_lowercase())
            .then_with(|| a.user_id.cmp(&b.user_id))
    });
    rows
}

impl ClientView {
    pub(crate) fn open_friends(&mut self, cx: &mut Context<Self>) {
        self.place = Place::Home;
        self.clear_selection(cx);
    }

    /// The "Personal notes" sidebar row: opens the user's self-DM
    /// (`ChannelKind::DmPersonalNotes`). When the READY payload did not carry
    /// it, asks the server for the DM with the user's own id.
    /// TODO(backlog): the web client routes to `dmChannel(currentUserId)` and
    /// the notes purge / context menu (`PersonalNotesContextMenu`) is missing.
    pub(crate) fn open_personal_notes(&mut self, cx: &mut Context<Self>) {
        let (existing, own_id) = {
            let state = self.backend.state();
            let existing = state.dm_channels().into_iter().find(|c| c.kind == ChannelKind::DmPersonalNotes).map(|c| c.id);
            (existing, state.current_user_id())
        };
        match (existing, own_id) {
            (Some(channel_id), _) => self.open_channel(None, channel_id, cx),
            (None, Some(user_id)) => self.backend.open_dm(user_id),
            (None, None) => {}
        }
    }

    pub(crate) fn set_friends_tab(&mut self, tab: FriendsTab, cx: &mut Context<Self>) {
        self.friends_tab = tab;
        self.friends_status = None;
        self.refresh(cx);
    }

    pub(crate) fn submit_add_friend(&mut self, cx: &mut Context<Self>) {
        let raw = self.add_friend_input.read(cx).value().trim().to_owned();
        if raw.is_empty() {
            return;
        }
        self.friends_status = None;
        self.backend.send_friend_request_by_tag(raw);
        cx.notify();
    }

    pub(crate) fn open_or_create_dm(&mut self, user_id: Snowflake, cx: &mut Context<Self>) {
        let existing = find_1on1_dm(&self.backend.state(), user_id);
        match existing {
            Some(channel_id) => self.open_channel(None, channel_id, cx),
            None => self.backend.open_dm(user_id),
        }
    }
}

fn tab_button(
    id: &'static str,
    label: String,
    active: bool,
    badge_count: u32,
    tab: FriendsTab,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> impl IntoElement {
    let accent = theme.accent;
    let fg = theme.foreground;
    let base = div()
        .id(id)
        .role(gpui_kit::Role::Button)
        .debug_selector(move || id.to_owned())
        .cursor_pointer()
        .flex()
        .flex_row()
        .items_center()
        .gap_1p5()
        .px(px(12.))
        .py(px(4.))
        .rounded(px(6.))
        .text_sm()
        .font_weight(FontWeight::SEMIBOLD)
        .on_click(cx.listener(move |this, _, _, cx| this.set_friends_tab(tab, cx)));

    let styled = if tab == FriendsTab::AddFriend {
        base.bg(theme.brand)
            .text_color(theme.on_fill)
            .hover(|s| s.opacity(0.9))
    } else if active {
        base.bg(accent).text_color(fg)
    } else {
        base.text_color(theme.muted_foreground)
            .hover(move |s| s.bg(accent).text_color(fg))
    };

    styled
        .child(label)
        .when(badge_count > 0, |d| d.child(badge(badge_count, theme)))
}

fn render_row(row: &FriendRow, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let uid = row.user_id;
    let subtitle: SharedString = match row.kind {
        RelationshipKind::IncomingRequest => l10n::tr("Incoming friend request").into(),
        RelationshipKind::OutgoingRequest => l10n::tr("Outgoing friend request").into(),
        RelationshipKind::Blocked => l10n::tr("Blocked").into(),
        _ => SharedString::from(status_label(row.status)),
    };
    let mut actions = div().flex().flex_row().items_center().gap_1();
    match row.kind {
        RelationshipKind::Friend => {
            actions = actions
                .child(
                    Button::new(("friend-dm", uid.0))
                        .ghost()
                        .small()
                        .label(l10n::tr("Message"))
                        .on_click(cx.listener(move |this, _, _, cx| this.open_or_create_dm(uid, cx))),
                )
                .child(
                    Button::new(("friend-remove", uid.0))
                        .ghost()
                        .small()
                        .label(l10n::tr("Remove friend"))
                        .on_click(cx.listener(move |this, _, _, _| this.backend.remove_relationship(uid))),
                )
                .child(
                    Button::new(("friend-block", uid.0))
                        .danger()
                        .small()
                        .label(l10n::tr("Block"))
                        .on_click(cx.listener(move |this, _, _, _| this.backend.block_user(uid))),
                );
        }
        RelationshipKind::IncomingRequest => {
            actions = actions
                .child(
                    Button::new(("friend-accept", uid.0))
                        .primary()
                        .small()
                        .label(l10n::tr("Accept"))
                        .on_click(cx.listener(move |this, _, _, _| this.backend.accept_friend_request(uid))),
                )
                .child(
                    Button::new(("friend-ignore", uid.0))
                        .ghost()
                        .small()
                        .label(l10n::tr("Ignore"))
                        .on_click(cx.listener(move |this, _, _, _| this.backend.remove_relationship(uid))),
                )
                .child(
                    Button::new(("friend-block", uid.0))
                        .danger()
                        .small()
                        .label(l10n::tr("Block"))
                        .on_click(cx.listener(move |this, _, _, _| this.backend.block_user(uid))),
                );
        }
        RelationshipKind::OutgoingRequest => {
            actions = actions.child(
                Button::new(("friend-cancel", uid.0))
                    .ghost()
                    .small()
                    .label(l10n::tr("Cancel"))
                    .on_click(cx.listener(move |this, _, _, _| this.backend.remove_relationship(uid))),
            );
        }
        RelationshipKind::Blocked => {
            actions = actions.child(
                Button::new(("friend-unblock", uid.0))
                    .ghost()
                    .small()
                    .label(l10n::tr("Unblock"))
                    .on_click(cx.listener(move |this, _, _, _| this.backend.remove_relationship(uid))),
            );
        }
        RelationshipKind::Unknown(_) => {}
    }

    div()
        .id(("friend-row", uid.0))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_3()
        .px_3()
        .py_2()
        .rounded(px(6.))
        .border_b_1()
        .border_color(theme.border)
        .hover(|s| s.bg(theme.muted))
        .child(
            div()
                .id(("friend-open-profile", uid.0))
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .flex_1()
                .min_w_0()
                .cursor_pointer()
                .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                    if !ev.modifiers().shift {
                        this.open_profile(uid, None, ev.position(), window, cx);
                    }
                }))
                .child(render_avatar(
                    AvatarSpec {
                        user_id: uid,
                        avatar_url: row.avatar_url.clone(),
                        display_name: row.display_name.clone(),
                        size: 32.,
                        status: Some(row.status),
                        status_border_bg: Some(theme.background),
                    },
                    theme,
                ))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .min_w_0()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .child(div().font_weight(FontWeight::SEMIBOLD).truncate().child(row.display_name.clone()))
                                .child(div().text_xs().text_color(theme.muted_foreground).child(row.username_tag.clone())),
                        )
                        .child(div().text_xs().text_color(theme.muted_foreground).child(subtitle)),
                ),
        )
        .child(actions)
        .into_any_element()
}

pub fn render(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> impl IntoElement {
    let tab = view.friends_tab;
    let header = div()
        .h(px(theme.sizes.header_height))
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .px_4()
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(gpui_kit::component::Icon::new(crate::icons::AppIcon::Users).small().text_color(theme.muted_foreground))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(l10n::tr("Friends"))),
        )
        .child(div().w(px(1.)).h(px(24.)).bg(theme.border))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1p5()
                .child(tab_button(
                    "friends-tab-online",
                    l10n::tr("Online"),
                    tab == FriendsTab::Online,
                    0,
                    FriendsTab::Online,
                    theme,
                    cx,
                ))
                .child(tab_button("friends-tab-all", l10n::tr("All"), tab == FriendsTab::All, 0, FriendsTab::All, theme, cx))
                .child(tab_button(
                    "friends-tab-pending",
                    l10n::tr("Pending"),
                    tab == FriendsTab::Pending,
                    view.pending_incoming,
                    FriendsTab::Pending,
                    theme,
                    cx,
                ))
                .child(tab_button(
                    "friends-tab-blocked",
                    l10n::tr("Blocked"),
                    tab == FriendsTab::Blocked,
                    0,
                    FriendsTab::Blocked,
                    theme,
                    cx,
                ))
                .child(tab_button(
                    "friends-tab-add",
                    l10n::tr("Add friend"),
                    tab == FriendsTab::AddFriend,
                    0,
                    FriendsTab::AddFriend,
                    theme,
                    cx,
                )),
        );

    let body: AnyElement = if tab == FriendsTab::AddFriend {
        let status_el = view.friends_status.as_ref().map(|res| match res {
            Ok(msg) => div().text_xs().text_color(theme.status_online).child(msg.clone()),
            Err(err) => div().text_xs().text_color(theme.danger).child(err.clone()),
        });
        div()
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child(l10n::tr("Add friend")))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(l10n::tr("You can add friends with their username.")),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(div().flex_1().child(Input::new(&view.add_friend_input)))
                    .child(
                        div().debug_selector(|| "friends-send-request".to_owned()).child(
                            Button::new("friends-send-request")
                                .primary()
                                .label(l10n::tr("Send friend request"))
                                .on_click(cx.listener(|this, _, _, cx| this.submit_add_friend(cx))),
                        ),
                    ),
            )
            .children(status_el)
            .into_any_element()
    } else if view.friend_rows.is_empty() {
        div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .text_color(theme.muted_foreground)
            .child(l10n::tr("No friends to show."))
            .into_any_element()
    } else {
        div()
            .id("friends-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_3()
            .flex()
            .flex_col()
            .gap_1()
            .children(view.friend_rows.iter().map(|row| render_row(row, theme, cx)))
            .into_any_element()
    };

    div()
        .id("friends-view")
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .child(header)
        .children(
            view.error
                .clone()
                .map(|e| div().debug_selector(|| "friends-error".to_owned()).px_4().pt_2().text_xs().text_color(theme.danger).child(e)),
        )
        .child(body)
}

#[cfg(test)]
mod tests {
    use fluxer_client_core::Dispatch;
    use serde_json::json;

    use super::*;

    fn state_with_relationships() -> ClientState {
        let mut state = ClientState::new();
        state.apply(&Dispatch {
            t: "READY".to_owned(),
            s: 1,
            d: json!({
                "session_id": "s1",
                "user": { "id": "100", "username": "me" },
                "relationships": [
                    { "id": "1", "type": 1, "user": { "id": "1", "username": "zoe", "discriminator": "0001" } },
                    { "id": "2", "type": 1, "user": { "id": "2", "username": "alice", "discriminator": "0000" } },
                    { "id": "3", "type": 3, "user": { "id": "3", "username": "incoming" } },
                    { "id": "4", "type": 4, "user": { "id": "4", "username": "outgoing" } },
                    { "id": "5", "type": 2, "user": { "id": "5", "username": "blocked_u" } }
                ],
                "presences": [
                    { "user": { "id": "2" }, "status": "online" },
                    { "user": { "id": "1" }, "status": "offline" }
                ]
            }),
        });
        state
    }

    #[test]
    fn build_friend_rows_filters_and_sorts_every_tab() {
        let state = state_with_relationships();
        let online = build_friend_rows(&state, None, FriendsTab::Online);
        assert_eq!(online.iter().map(|r| r.user_id).collect::<Vec<_>>(), vec![Snowflake(2)]);

        let all = build_friend_rows(&state, None, FriendsTab::All);
        assert_eq!(
            all.iter().map(|r| (r.user_id, r.username_tag.as_ref())).collect::<Vec<_>>(),
            vec![(Snowflake(2), "alice"), (Snowflake(1), "zoe#0001")]
        );

        let pending = build_friend_rows(&state, None, FriendsTab::Pending);
        assert_eq!(
            pending.iter().map(|r| (r.user_id, r.kind)).collect::<Vec<_>>(),
            vec![
                (Snowflake(3), RelationshipKind::IncomingRequest),
                (Snowflake(4), RelationshipKind::OutgoingRequest),
            ]
        );

        let blocked = build_friend_rows(&state, None, FriendsTab::Blocked);
        assert_eq!(blocked.iter().map(|r| r.user_id).collect::<Vec<_>>(), vec![Snowflake(5)]);

        assert!(build_friend_rows(&state, None, FriendsTab::AddFriend).is_empty());
    }

    #[test]
    fn pending_incoming_count_counts_only_incoming() {
        let state = state_with_relationships();
        assert_eq!(pending_incoming_count(&state), 1);
    }
}
