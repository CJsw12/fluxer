//! Right-hand member list of guild text channels (`ChannelMembers.tsx`):
//! group headers ("Online — 2", hoisted role names) and member rows with an
//! avatar, a status dot and the name in the top role color. The list is a
//! `uniform_list` (only visible rows are built); rows the server has not
//! synced yet render as placeholders. Visible rows are reported back to the
//! view, which subscribes the 100-row pages around them plus one overscan
//! page (`member_list_page_ranges`) through [`MemberListSubscriptions`].

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::rc::Rc;

use fluxer_client_core::gateway::normalize_member_list_ranges;
use fluxer_client_core::state::{ClientState, MemberListRow, Snowflake, Status};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, Context, FontWeight, Hsla, SharedString, UniformListScrollHandle, WeakEntity, div, hsla, prelude::*, px,
    rgb, uniform_list,
};

use super::avatar::{AvatarSpec, render_avatar_tagged};
use crate::client_view::ClientView;
use crate::format::{default_avatar_index, initials};
use crate::icons::AppIcon;
use crate::parity_probe::FlxExt as _;
use crate::theme::Theme;
use crate::ui::l10n;
use crate::view_model::avatar_url;

/// Panel width (`globals.css`: `--layout-member-list-width: 16.5rem` = 264px).
#[cfg(test)]
pub const PANEL_WIDTH: f32 = 264.;
/// Row height; the web uses 44 px members and 32 px headers, a uniform list
/// needs one height so headers take a member row's height.
pub const ROW_HEIGHT: f32 = 44.;
const AVATAR: f32 = 32.;
/// Skeleton rows while the first SYNC is pending (`MemberListSkeleton`).
const SKELETON_ROWS: usize = 8;

/// One member list subscription as sent to the gateway.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberListRequest {
    pub guild_id: Snowflake,
    pub channel_id: Snowflake,
    /// Normalized (`normalize_member_list_ranges`).
    pub ranges: Vec<(u32, u32)>,
    pub sync: bool,
}

/// Delivers a request; returns whether it reached the gateway queue. The
/// app sends through `Backend::send_member_list`; tests record frames.
pub type MemberListSink = Box<dyn Fn(&MemberListRequest) -> bool>;

/// Send side of `MemberSidebar.applyChannelSubscription`: a request is sent
/// only when guild, channel or normalized ranges changed since the last
/// delivered one, with `sync: true` the first time per guild in this
/// gateway session (`syncedMemberListGuildIds`).
pub struct MemberListSubscriptions {
    synced_guilds: BTreeSet<Snowflake>,
    sent: Option<MemberListRequest>,
    sink: MemberListSink,
}

impl MemberListSubscriptions {
    pub fn with_sink(sink: MemberListSink) -> Self {
        Self { synced_guilds: BTreeSet::new(), sent: None, sink }
    }

    /// Returns true if a request was delivered. An undelivered request
    /// (no gateway yet) is retried by the next call.
    pub fn subscribe(&mut self, guild_id: Snowflake, channel_id: Snowflake, ranges: &[(u32, u32)]) -> bool {
        let ranges = normalize_member_list_ranges(ranges);
        let unchanged = self
            .sent
            .as_ref()
            .is_some_and(|s| s.guild_id == guild_id && s.channel_id == channel_id && s.ranges == ranges);
        if unchanged {
            return false;
        }
        let sync = !self.synced_guilds.contains(&guild_id);
        let request = MemberListRequest { guild_id, channel_id, ranges, sync };
        if !(self.sink)(&request) {
            return false;
        }
        self.synced_guilds.insert(guild_id);
        self.sent = Some(request);
        true
    }

    /// A new gateway session: the server forgot the subscriptions, so the
    /// next `subscribe` sends again, with `sync`.
    pub fn reset_session(&mut self) {
        self.synced_guilds.clear();
        self.sent = None;
    }

    /// The last delivered subscription.
    #[cfg(test)]
    pub fn current(&self) -> Option<&MemberListRequest> {
        self.sent.as_ref()
    }
}

/// The status after `status` in the user panel cycle
/// online -> idle -> dnd -> invisible -> online.
pub fn next_status(status: Status) -> Status {
    match status {
        Status::Online => Status::Idle,
        Status::Idle => Status::Dnd,
        Status::Dnd => Status::Invisible,
        Status::Invisible | Status::Offline => Status::Online,
    }
}

pub fn status_label(status: Status) -> String {
    match status {
        Status::Online => l10n::tr("Online"),
        Status::Idle => l10n::tr("Idle"),
        Status::Dnd => l10n::tr("Do not disturb"),
        Status::Invisible => l10n::tr("Invisible"),
        Status::Offline => l10n::tr("Offline"),
    }
}

/// Status dot colors (`StatusConstants`: online green, idle yellow, dnd red).
pub fn status_color(status: Status, theme: &Theme) -> Hsla {
    super::status_indicator::color(status, theme)
}

#[derive(Debug, Clone, PartialEq)]
pub enum MemberRowView {
    /// "Online — 2", "Moderators — 1".
    Group { label: SharedString },
    Member(MemberItem),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemberItem {
    pub user_id: Snowflake,
    pub name: SharedString,
    /// Top colored role; `None` = default text color.
    pub color: Option<Hsla>,
    pub status: Status,
    pub initials: SharedString,
    pub avatar_url: Option<SharedString>,
    pub default_avatar: usize,
    pub is_owner: bool,
}

/// Snapshot of the shown member list.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MemberPanel {
    /// False until the server answered the subscription (skeleton rows).
    pub loaded: bool,
    /// Rows of the whole list (scroll height).
    pub total: u32,
    /// Synced rows; the rest are placeholders.
    pub rows: BTreeMap<u32, MemberRowView>,
}

impl MemberPanel {
    /// Rows the list element shows.
    pub fn item_count(&self) -> usize {
        if self.loaded { self.total as usize } else { SKELETON_ROWS }
    }
}

/// Builds the panel snapshot for `channel_id` of `guild_id`.
pub fn member_panel(state: &ClientState, guild_id: Snowflake, channel_id: Snowflake, media: Option<&str>) -> MemberPanel {
    let Some(list) = state.member_list(guild_id, channel_id) else { return MemberPanel::default() };
    let guild = state.guilds.get(&guild_id);
    let rows = list
        .rows
        .iter()
        .map(|(index, row)| {
            let view = match row {
                MemberListRow::Group(group) => {
                    let name = match group.id.as_str() {
                        "online" => l10n::tr("Online"),
                        "offline" => l10n::tr("Offline"),
                        id => id
                            .parse::<u64>()
                            .ok()
                            .and_then(|id| guild?.roles.get(&Snowflake(id)))
                            .map_or_else(|| id.to_owned(), |role| role.name.clone()),
                    };
                    MemberRowView::Group { label: format!("{name} — {}", group.count).into() }
                }
                MemberListRow::Member(entry) => {
                    let member = &entry.member;
                    let user = &member.user;
                    let name = member.nick.as_deref().filter(|n| !n.is_empty()).unwrap_or_else(|| user.display_name());
                    let color = guild
                        .and_then(|g| {
                            member
                                .roles
                                .iter()
                                .filter_map(|id| g.roles.get(id))
                                .filter(|r| r.color != 0)
                                .max_by_key(|r| (r.position, std::cmp::Reverse(r.id)))
                        })
                        .and_then(|r| u32::try_from(r.color & 0xff_ffff).ok())
                        .map(|c| rgb(c).into());
                    let status = match &entry.presence {
                        Some(presence) => presence.status(),
                        None => state.presence_status(user.id),
                    };
                    let is_owner = guild.is_some_and(|g| g.owner_id == Some(user.id));
                    MemberRowView::Member(MemberItem {
                        user_id: user.id,
                        name: name.to_owned().into(),
                        color,
                        status,
                        initials: initials(name).into(),
                        avatar_url: media.and_then(|m| avatar_url(m, user)).map(Into::into),
                        default_avatar: default_avatar_index(user.id),
                        is_owner,
                    })
                }
            };
            (*index, view)
        })
        .collect();
    MemberPanel { loaded: true, total: list.row_count(), rows }
}

/// The panel (debug selectors `member-panel`, `member-row-<index>`).
pub fn render(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let panel = view.member_panel.clone();
    let count = panel.borrow().item_count();
    let theme = *theme;
    let visible = view.member_visible.clone();
    let weak = cx.entity().downgrade();
    let guild_id = view.member_target.map(|(g, _)| g);
    let list = uniform_list("member-list", count, move |range, _window, cx| {
        report_visible(&visible, &weak, range.clone(), count, cx);
        let panel = panel.borrow();
        range
            .map(|ix| {
                let row = if panel.loaded { u32::try_from(ix).ok().and_then(|i| panel.rows.get(&i)) } else { None };
                render_row(ix, row, guild_id, &weak, &theme)
                    .debug_selector(|| format!("member-row-{ix}"))
                    .into_any_element()
            })
            .collect()
    })
    .track_scroll(&view.member_scroll)
    .flex_1()
    .min_h_0()
    // Official rows start 6px above the container top (group header slot 83..127, container 89).
    .mt(px(-6.))
    .mb(px(4.));
    div()
        .id("member-panel")
        .debug_selector(|| "member-panel".to_owned())
        .flex()
        .flex_row()
        .flex_none()
        .w(px(theme.sizes.member_list_width))
        // Stretches to the row but stops 1px short: the official list ends inside the frame border.
        .mb(px(1.))
        .overflow_hidden()
        .bg(theme.background_secondary_lighter)
        .flx("channel.member-list-container.member-list-container")
        .child(
            div()
                .w(px(1.))
                .h_full()
                .flex_none()
                .bg(theme.user_area_divider)
                .flx("channel.channel-view.channel-view-scaffold.member-list-divider"),
        )
        .child(div().flex_1().min_w_0().flex().flex_col().pl(px(7.)).pr(px(8.)).child(list))
        .into_any_element()
}

/// Reports the visible rows to the view once per change (the list calls
/// this on every layout); the view subscribes in a deferred update.
fn report_visible(
    visible: &Rc<Cell<Option<(usize, usize)>>>,
    weak: &WeakEntity<ClientView>,
    range: Range<usize>,
    count: usize,
    cx: &mut gpui_kit::App,
) {
    // uniform_list also calls the closure with `0..1` to measure the row
    // height; reporting that would flip the span every frame and loop forever.
    if range.is_empty() || (range.len() == 1 && count > 1) {
        return;
    }
    let span = (range.start, range.end - 1);
    if visible.get() == Some(span) {
        return;
    }
    visible.set(Some(span));
    let weak = weak.clone();
    cx.defer(move |cx| {
        let _ = weak.update(cx, |view, cx| view.member_list_scrolled(span.0, span.1, cx));
    });
}

fn render_row(
    ix: usize,
    row: Option<&MemberRowView>,
    guild_id: Option<Snowflake>,
    view: &WeakEntity<ClientView>,
    theme: &Theme,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let base = div().id(("member-item", ix)).h(px(ROW_HEIGHT)).w_full().flex().flex_row().items_start();
    match row {
        // Official group header: 32px box at +10 inside the 44px slot
        // (`MemberListGroupHeader`: 14px/600, lh 16, "Online—2").
        Some(MemberRowView::Group { label }) => {
            let text: SharedString = label.replace(" — ", "—").into();
            base.pt(px(10.)).child(
                div()
                    .h(px(32.))
                    .w_full()
                    .flex()
                    .items_start()
                    .pt(px(12.))
                    .px_2()
                    .text_size(px(14.))
                    .line_height(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.muted_foreground)
                    .flx_text("channel.member-list-group-header", text.to_string())
                    .child(match text.split_once('—') {
                        Some((name, count)) => div()
                            .flex()
                            .flex_row()
                            .child(div().flex_none().flx("channel.member-list-group-header.label").child(name.to_owned()))
                            .child(div().flex_none().flx("channel.member-list-group-header.separator").child("—"))
                            .child(div().flex_none().flx("channel.member-list-group-header.count").child(count.to_owned()))
                            .into_any_element(),
                        None => div().truncate().child(text).into_any_element(),
                    }),
            )
        }
        Some(MemberRowView::Member(m)) => {
            let offline = m.status.is_offline();
            let picture = render_avatar_tagged(
                AvatarSpec {
                    user_id: m.user_id,
                    avatar_url: m.avatar_url.clone(),
                    display_name: m.name.clone(),
                    size: AVATAR,
                    status: Some(m.status),
                    status_border_bg: Some(theme.background_secondary_lighter),
                },
                theme,
                &["ui.base-avatar.image"],
            );
            let user_id = m.user_id;
            let view = view.clone();
            base.child(
                div()
                    .h(px(40.))
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .pl(px(8.)).pr(px(12.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.hover_bg).text_color(theme.foreground).opacity(1.))
                    .when(offline, |d| d.opacity(0.4))
                    .flx("channel.member-list-item.focus-ring-wrapper")
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h(px(32.))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(10.))
                            .flx("channel.member-list-item.content")
                            .child(picture)
                            .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .flx("channel.member-list-item.name-container")
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(m.color.unwrap_or(theme.text_chat))
                                    .flx_text("channel.member-list-item.name", m.name.to_string())
                                    .child(m.name.clone()),
                            )
                            .when(m.is_owner, |d| {
                                d.child(
                                    div()
                                        .flex_none()
                                        .size(px(14.))
                                        .mt(px(1.4))
                                        .debug_selector(move || format!("member-owner-crown-{}", user_id.0))
                                        .flx("channel.member-list-item.crown-icon")
                                        .child(
                                            Icon::new(AppIcon::CrownFill)
                                                .with_size(px(14.))
                                                .text_color(hsla(39. / 360., 0.57, 0.64, 1.)),
                                        ),
                                )
                            }),
                            ),
                    ),
            )
            .on_click(move |ev, window, cx| {
                if !ev.modifiers().shift {
                    let pos = ev.position();
                    let _ = view.update(cx, |v, cx| v.open_profile(user_id, guild_id, pos, window, cx));
                }
            })
        }
        None => base
            .items_center()
            .gap_3()
            .child(div().size(px(AVATAR)).flex_none().rounded_full().bg(theme.muted))
            .child(div().h(px(12.)).w(px(110.)).rounded(px(6.)).bg(theme.muted)),
    }
}

/// The member list subscription for the visible rows `first..=last`.
pub fn page_ranges(first: usize, last: usize, panel: &MemberPanel) -> Vec<(u32, u32)> {
    let first = u32::try_from(first).unwrap_or(u32::MAX);
    let last = u32::try_from(last).unwrap_or(u32::MAX);
    let total = panel.loaded.then_some(panel.total);
    fluxer_client_core::gateway::member_list_page_ranges(first, last, total)
}

/// Shared handles the view keeps for this panel.
pub fn new_panel_state() -> (Rc<RefCell<MemberPanel>>, Rc<Cell<Option<(usize, usize)>>>, UniformListScrollHandle) {
    (Rc::new(RefCell::new(MemberPanel::default())), Rc::new(Cell::new(None)), UniformListScrollHandle::new())
}

#[cfg(test)]
mod tests {
    use fluxer_client_core::Dispatch;
    use serde_json::{Value, json};

    use super::*;

    fn recording() -> (MemberListSubscriptions, Rc<RefCell<Vec<Value>>>) {
        let frames = Rc::new(RefCell::new(Vec::new()));
        let log = frames.clone();
        let subs = MemberListSubscriptions::with_sink(Box::new(move |r| {
            log.borrow_mut().push(fluxer_client_core::gateway::member_list_subscription(
                &r.guild_id.to_string(),
                &r.channel_id.to_string(),
                &r.ranges,
                r.sync,
            ));
            true
        }));
        (subs, frames)
    }

    #[test]
    fn sync_only_first_time_per_guild_and_dedup() {
        let (mut subs, frames) = recording();
        let (g, a, b) = (Snowflake(1), Snowflake(11), Snowflake(12));
        assert!(subs.subscribe(g, a, &[(0, 99)]));
        assert!(!subs.subscribe(g, a, &[(0, 50), (51, 99)]), "same normalized ranges are not re-sent");
        assert!(subs.subscribe(g, b, &[(0, 99)]));
        assert!(subs.subscribe(g, b, &[(0, 99), (100, 199)]));
        let frames = frames.borrow();
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0], json!({"1": {"active": true, "sync": true, "member_list_channels": {"11": [[0, 99]]}}}));
        assert_eq!(frames[1], json!({"1": {"active": true, "member_list_channels": {"12": [[0, 99]]}}}));
        assert_eq!(frames[2], json!({"1": {"active": true, "member_list_channels": {"12": [[0, 99], [100, 199]]}}}));
    }

    #[test]
    fn reset_session_resends_with_sync() {
        let (mut subs, frames) = recording();
        assert!(subs.subscribe(Snowflake(1), Snowflake(11), &[(0, 99)]));
        subs.reset_session();
        assert!(subs.current().is_none());
        assert!(subs.subscribe(Snowflake(1), Snowflake(11), &[(0, 99)]));
        assert_eq!(frames.borrow()[1]["1"]["sync"], json!(true));
    }

    #[test]
    fn undelivered_requests_are_retried_with_sync() {
        let delivered = Rc::new(Cell::new(false));
        let flag = delivered.clone();
        let mut subs = MemberListSubscriptions::with_sink(Box::new(move |_| flag.get()));
        assert!(!subs.subscribe(Snowflake(1), Snowflake(11), &[(0, 99)]));
        assert!(subs.current().is_none());
        delivered.set(true);
        assert!(subs.subscribe(Snowflake(1), Snowflake(11), &[(0, 99)]));
        assert_eq!(subs.current().map(|r| r.sync), Some(true));
    }

    #[test]
    fn status_cycle() {
        let order: Vec<Status> =
            std::iter::successors(Some(Status::Online), |s| Some(next_status(*s))).take(5).collect();
        assert_eq!(order, [Status::Online, Status::Idle, Status::Dnd, Status::Invisible, Status::Online]);
    }

    #[test]
    fn panel_snapshot_names_groups_and_colors_by_top_role() {
        let mut state = ClientState::new();
        state.apply(&Dispatch {
            t: "READY".to_owned(),
            s: 1,
            d: json!({
                "session_id": "s", "user": {"id": "100", "username": "me"},
                "guilds": [{"id": "1", "properties": {"name": "G"},
                    "roles": [{"id": "1", "name": "@everyone", "permissions": "0"},
                              {"id": "5", "name": "Mods", "color": 0xff0000, "position": 2, "hoist": true},
                              {"id": "6", "name": "Blue", "color": 0x0000ff, "position": 1}],
                    "channels": [{"id": "11", "type": 0, "name": "general"}], "members": []}],
                "private_channels": [], "read_states": []
            }),
        });
        let member = |id: u64, name: &str, roles: Value, status: &str| {
            json!({"member": {"user": {"id": id.to_string(), "username": name}, "roles": roles,
                              "presence": {"status": status}}})
        };
        state.apply(&Dispatch {
            t: "GUILD_MEMBER_LIST_UPDATE".to_owned(),
            s: 2,
            d: json!({"guild_id": "1", "id": "everyone", "channel_id": "11", "member_count": 3, "online_count": 2,
                "groups": [{"id": "5", "count": 1}, {"id": "online", "count": 1}, {"id": "offline", "count": 1}],
                "ops": [{"op": "SYNC", "range": [0, 5], "items": [
                    {"group": {"id": "5", "count": 1}}, member(201, "alice", json!(["6", "5"]), "dnd"),
                    {"group": {"id": "online", "count": 1}}, member(202, "bob", json!([]), "online"),
                    {"group": {"id": "offline", "count": 1}}, member(203, "carol", json!(["6"]), "offline")]}]}),
        });
        let panel = member_panel(&state, Snowflake(1), Snowflake(11), None);
        assert!(panel.loaded);
        assert_eq!(panel.total, 6);
        let labels: Vec<String> = panel
            .rows
            .values()
            .map(|r| match r {
                MemberRowView::Group { label } => label.to_string(),
                MemberRowView::Member(m) => m.name.to_string(),
            })
            .collect();
        assert_eq!(labels, ["Mods — 1", "alice", "Online — 1", "bob", "Offline — 1", "carol"]);
        let MemberRowView::Member(alice) = &panel.rows[&1] else { panic!("alice row") };
        assert_eq!(alice.color, Some(rgb(0xff0000).into()), "highest positioned colored role");
        assert_eq!(alice.status, Status::Dnd);
        let MemberRowView::Member(bob) = &panel.rows[&3] else { panic!("bob row") };
        assert_eq!(bob.color, None);
        assert_eq!(page_ranges(250, 260, &MemberPanel { loaded: true, total: 1000, rows: BTreeMap::new() }),
            vec![(0, 99), (100, 199), (200, 299), (300, 399)]);
    }
}

/// Headless tests: the real `ClientView` in a virtual window, with the
/// member list subscription sink replaced by a frame recorder.
#[cfg(test)]
mod headless_tests {
    use std::sync::Arc;

    use fluxer_client_core::Dispatch;
    use fluxer_client_core::rest::{ClientIdentity, Endpoints, RestClient, RestConfig};
    use fluxer_client_core::state::{Snowflake, bits};
    use gpui_kit::{Entity, ScrollStrategy, TestAppContext, VisualTestContext};
    use serde_json::{Value, json};

    use super::*;
    use crate::backend::Backend;
    use crate::view_model::Place;

    const GUILD: Snowflake = Snowflake(1);
    const GENERAL: Snowflake = Snowflake(11);
    const RANDOM: Snowflake = Snowflake(12);
    /// Online 150 + offline 150 members, one header each.
    const TOTAL_ROWS: u32 = 302;

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime")
    }

    fn offline_rest() -> Arc<RestClient> {
        let endpoints = Endpoints {
            api_code_version: 1,
            api: "https://fluxer.invalid/api".to_owned(),
            gateway: "wss://fluxer.invalid/gateway".to_owned(),
            media: None,
            static_cdn: None,
            upload_relay: None,
            invite: None,
            webapp: None,
            discovery_url: "https://fluxer.invalid/.well-known/fluxer".to_owned(),
            document: json!({}),
        };
        Arc::new(RestClient::new(RestConfig::new(&endpoints, ClientIdentity::default())).expect("rest client"))
    }

    fn dispatch(t: &str, s: u64, d: Value) -> Dispatch {
        Dispatch { t: t.to_owned(), s, d }
    }

    fn ready() -> Dispatch {
        let perms = (bits::VIEW_CHANNEL | bits::SEND_MESSAGES).to_string();
        dispatch(
            "READY",
            1,
            json!({
                "session_id": "session-1",
                "user": { "id": "100", "username": "me" },
                "guilds": [{
                    "id": GUILD.to_string(),
                    "properties": { "name": "Rust Port", "owner_id": "999" },
                    "roles": [{ "id": GUILD.to_string(), "name": "@everyone", "permissions": perms },
                              { "id": "5", "name": "Mods", "color": 0xff0000, "position": 1, "hoist": true }],
                    "channels": [
                        { "id": GENERAL.to_string(), "type": 0, "name": "general", "position": 0 },
                        { "id": RANDOM.to_string(), "type": 0, "name": "random", "position": 1 }
                    ],
                    "members": [{ "user": { "id": "100", "username": "me" }, "roles": [] }]
                }],
                "private_channels": [],
                "read_states": []
            }),
        )
    }

    /// GUILD_MEMBER_LIST_UPDATE with a SYNC of rows 0..=99: the online
    /// header and 99 online members; rows 100.. stay unsynced.
    fn member_list_sync() -> Dispatch {
        let mut items = vec![json!({"group": {"id": "online", "count": 150}})];
        for n in 0..99u64 {
            let roles = if n == 0 { json!(["5"]) } else { json!([]) };
            let status = if n == 1 { "idle" } else { "online" };
            items.push(json!({"member": {
                "user": {"id": (1000 + n).to_string(), "username": format!("user{n}")},
                "roles": roles, "presence": {"status": status}}}));
        }
        dispatch(
            "GUILD_MEMBER_LIST_UPDATE",
            2,
            json!({"guild_id": GUILD.to_string(), "id": "everyone", "channel_id": GENERAL.to_string(),
                   "member_count": 300, "online_count": 150,
                   "groups": [{"id": "online", "count": 150}, {"id": "offline", "count": 150}],
                   "ops": [{"op": "SYNC", "range": [0, 99], "items": items}]}),
        )
    }

    /// Draws, runs the deferred visible-rows report, and draws again.
    fn draw(cx: &mut VisualTestContext) {
        for _ in 0..2 {
            cx.update(|window, cx| {
                let _ = window.draw(cx);
            });
            cx.run_until_parked();
        }
    }

    /// Opens the client on the guild with a recording member list sink.
    fn open<'a>(cx: &'a mut TestAppContext, rt: &tokio::runtime::Runtime) -> (Entity<ClientView>, &'a mut VisualTestContext, Rc<RefCell<Vec<Value>>>) {
        let (backend, events) = Backend::new(rt.handle().clone(), offline_rest(), None);
        {
            let mut state = backend.state();
            assert!(!state.apply(&ready()).is_empty());
            assert!(!state.apply(&member_list_sync()).is_empty());
        }
        cx.update(gpui_kit::init);
        let mut content = None;
        let (_, vcx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| ClientView::new(backend, events, window, cx));
            content = Some(view.clone());
            gpui_kit::base::Root::new(view, window, cx)
        });
        let view = content.expect("window built its content");
        let frames = Rc::new(RefCell::new(Vec::new()));
        let log = frames.clone();
        view.update(vcx, |v, _| {
            v.member_subscriptions = MemberListSubscriptions::with_sink(Box::new(move |r| {
                log.borrow_mut().push(fluxer_client_core::gateway::member_list_subscription(
                    &r.guild_id.to_string(),
                    &r.channel_id.to_string(),
                    &r.ranges,
                    r.sync,
                ));
                true
            }));
        });
        view.update(vcx, |v, cx| v.select_place(Place::Guild(GUILD), cx));
        draw(vcx);
        (view, vcx, frames)
    }

    fn ranges_of(frame: &Value, channel: Snowflake) -> Value {
        frame[GUILD.to_string()]["member_list_channels"][channel.to_string()].clone()
    }

    #[gpui_kit::test]
    fn member_list_renders_groups_and_members_from_sync(cx: &mut TestAppContext) {
        let rt = runtime();
        let (view, cx, frames) = open(cx, &rt);

        view.read_with(cx, |v, _| {
            assert_eq!(v.member_target, Some((GUILD, GENERAL)));
            assert!(v.shows_member_list());
            let panel = v.member_panel.borrow();
            assert!(panel.loaded);
            assert_eq!(panel.total, TOTAL_ROWS);
            assert_eq!(panel.rows.get(&0), Some(&MemberRowView::Group { label: "Online — 150".into() }));
            let Some(MemberRowView::Member(first)) = panel.rows.get(&1) else { panic!("row 1 is a member") };
            assert_eq!(first.name.as_ref(), "user0");
            assert_eq!(first.color, Some(rgb(0xff0000).into()), "Mods role color");
            let Some(MemberRowView::Member(idle)) = panel.rows.get(&2) else { panic!("row 2 is a member") };
            assert_eq!(idle.status, Status::Idle);
            assert!(!panel.rows.contains_key(&150), "unsynced rows are placeholders");
        });
        let panel = cx.debug_bounds("member-panel").expect("member panel is laid out");
        assert_eq!(panel.size.width, px(PANEL_WIDTH));
        assert!(cx.debug_bounds("member-row-0").is_some(), "group header row is rendered");
        assert!(cx.debug_bounds("member-row-1").is_some(), "member row is rendered");
        assert!(cx.debug_bounds("member-row-250").is_none(), "rows far below are not built");

        // One subscription: first page plus one overscan page, with sync.
        let frames = frames.borrow();
        assert_eq!(frames.len(), 1, "frames: {frames:?}");
        assert_eq!(frames[0][GUILD.to_string()]["sync"], json!(true));
        assert_eq!(ranges_of(&frames[0], GENERAL), json!([[0, 99], [100, 199]]));
        drop(rt);
    }

    #[gpui_kit::test]
    fn scrolling_and_channel_change_request_new_ranges(cx: &mut TestAppContext) {
        let rt = runtime();
        let (view, cx, frames) = open(cx, &rt);
        assert_eq!(frames.borrow().len(), 1);

        let scroll = view.read_with(cx, |v, _| v.member_scroll.clone());
        scroll.scroll_to_item(250, ScrollStrategy::Top);
        draw(cx);
        assert!(cx.debug_bounds("member-row-250").is_some(), "scrolled rows are built (as placeholders)");
        let (first, _) = view.read_with(cx, |v, _| v.member_visible.get()).expect("visible rows reported");
        assert!(first >= 240, "visible rows follow the scroll, first = {first}");
        {
            let frames = frames.borrow();
            assert_eq!(frames.len(), 2, "frames: {frames:?}");
            let last = &frames[1];
            assert!(last[GUILD.to_string()].get("sync").is_none(), "sync only on the first request");
            assert_eq!(ranges_of(last, GENERAL), json!([[0, 99], [100, 199], [200, 299], [300, 399]]));
        }

        // Another channel: re-subscribed from the top, still without sync.
        view.update(cx, |v, cx| v.select_channel(RANDOM, cx));
        draw(cx);
        let frames = frames.borrow();
        let last = frames.last().expect("frames");
        assert_eq!(ranges_of(last, RANDOM), json!([[0, 99], [100, 199]]));
        assert!(last[GUILD.to_string()].get("sync").is_none());
        view.read_with(cx, |v, _| {
            assert_eq!(v.member_target, Some((GUILD, RANDOM)));
            assert!(!v.member_panel.borrow().loaded, "no list for this channel yet: skeleton");
        });
        drop(rt);
    }

    #[gpui_kit::test]
    fn toggle_and_dms_hide_the_panel(cx: &mut TestAppContext) {
        let rt = runtime();
        let (view, cx, _frames) = open(cx, &rt);
        assert!(cx.debug_bounds("member-panel").is_some());

        view.update(cx, |v, cx| v.toggle_member_list(cx));
        draw(cx);
        assert!(!view.read_with(cx, |v, _| v.shows_member_list()));
        assert!(cx.debug_bounds("member-panel").is_none(), "toggled off");

        view.update(cx, |v, cx| v.toggle_member_list(cx));
        draw(cx);
        assert!(cx.debug_bounds("member-panel").is_some(), "toggled back on");

        view.update(cx, |v, cx| v.select_place(Place::Home, cx));
        draw(cx);
        view.read_with(cx, |v, _| {
            assert_eq!(v.member_target, None);
            assert!(v.member_list_open, "the toggle is kept");
        });
        assert!(cx.debug_bounds("member-panel").is_none(), "no member list in DMs");
        drop(rt);
    }

    #[gpui_kit::test]
    fn status_dot_cycles_the_own_presence(cx: &mut TestAppContext) {
        let rt = runtime();
        let (view, cx, _frames) = open(cx, &rt);
        assert_eq!(view.read_with(cx, |v, _| v.own_status), Status::Online);
        for expected in [Status::Idle, Status::Dnd, Status::Invisible, Status::Online] {
            view.update(cx, |v, cx| v.cycle_status(cx));
            draw(cx);
            view.read_with(cx, |v, _| assert_eq!(v.own_status, expected));
        }
        drop(rt);
    }

    #[gpui_kit::test]
    fn guild_owner_shows_crown_indicator(cx: &mut TestAppContext) {
        let rt = runtime();
        let (view, cx, _frames) = open(cx, &rt);
        // Set guild owner to user 1000 (row 1 = user0) and verify crown appears only for user 1000.
        view.update(cx, |v, cx| {
            {
                let mut st = v.backend.state();
                st.guilds.get_mut(&GUILD).expect("guild").owner_id = Some(Snowflake(1000));
            }
            v.select_place(Place::Home, cx);
            v.select_place(Place::Guild(GUILD), cx);
        });
        draw(cx);
        assert!(cx.debug_bounds("member-owner-crown-1000").is_some(), "owner row shows crown");
        assert!(cx.debug_bounds("member-owner-crown-1001").is_none(), "non-owner has no crown");
        drop(rt);
    }
}
