use fluxer_client_core::{Dispatch, state::{ClientState, Snowflake, bits}};
use gpui_kit::{Modifiers, MouseButton, TestAppContext, point, px};
use serde_json::json;

use super::{ReadScope, inbox_acks};
use crate::backend::Backend;
use crate::tests::{draw, offline_rest, open_client, runtime};
use crate::ui::sidebar_menus::{HeaderAction, SidebarContextMenu, SidebarMenuKind, header_actions};
use crate::view_model::Place;

#[test]
fn header_actions_follow_channel_kind_and_permissions() {
    use fluxer_client_core::state::ChannelKind::*;
    use HeaderAction::*;

    assert_eq!(header_actions(GuildText, true, true), [Invite, CopyLink, Notifications, Edit]);
    assert_eq!(header_actions(GuildText, false, false), [CopyLink, Notifications]);
    assert_eq!(header_actions(Dm, true, true), [CopyLink, Notifications]);
    assert_eq!(header_actions(GroupDm, true, true), [AddPeople, CopyLink, Notifications]);
    assert_eq!(header_actions(PublicThread, true, true), [CopyLink, Notifications]);
}

fn ready() -> Dispatch {
    Dispatch {
        t: "READY".to_owned(),
        s: 1,
        d: json!({
            "session_id": "menus",
            "user": {"id":"100","username":"me"},
            "guilds": [{
                "id":"1", "properties":{"name":"Community","owner_id":"100"},
                "roles":[{"id":"1","name":"@everyone","permissions":bits::VIEW_CHANNEL.to_string()}],
                "channels":[{"id":"11","type":0,"name":"markdown","last_message_id":"211"}],
                "members":[{"user":{"id":"100","username":"me"},"roles":[]}]
            }],
            "private_channels": [
                {"id":"31","type":1,"last_message_id":"231"},
                {"id":"32","type":3,"last_message_id":"232"},
                {"id":"33","type":999,"last_message_id":"233"},
                {"id":"34","type":1}
            ],
            "read_states": [
                {"id":"11","last_message_id":"201","mention_count":1},
                {"id":"31","last_message_id":"201","mention_count":1},
                {"id":"32","last_message_id":"201","mention_count":1},
                {"id":"33","last_message_id":"201","mention_count":1}
            ]
        }),
    }
}

#[test]
fn read_scopes_send_channel_and_latest_message_pairs() {
    let mut state = ClientState::new();
    assert!(!state.apply(&ready()).is_empty());
    let mut dms = inbox_acks(&state, ReadScope::Dms);
    dms.sort_unstable();
    assert_eq!(dms, [(Snowflake(31), Snowflake(231)), (Snowflake(32), Snowflake(232))]);
    let communities = inbox_acks(&state, ReadScope::Communities);
    assert_eq!(communities, [(Snowflake(11), Snowflake(211))]);
    for (channel, last) in dms.into_iter().chain(communities) {
        state.local_ack(channel, last);
    }
    assert!(inbox_acks(&state, ReadScope::Dms).is_empty());
    assert!(inbox_acks(&state, ReadScope::Communities).is_empty());
}

#[gpui_kit::test]
fn offscreen_header_menu_stays_in_viewport_and_escape_closes_it(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = Backend::new(rt.handle().clone(), offline_rest(), None);
    backend.state().apply(&ready());
    let (view, cx) = open_client(cx, backend, events);
    view.update(cx, |view, cx| view.select_place(Place::Guild(Snowflake(1)), cx));
    cx.run_until_parked();
    draw(cx);
    let header = cx.debug_bounds("header-channel-name").expect("channel name");
    cx.simulate_mouse_down(header.center(), MouseButton::Right, Modifiers::default());
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| {
        view.sidebar_context_menu.as_ref().map(|menu| menu.kind) == Some(SidebarMenuKind::Header(Snowflake(11)))
    }));
    let viewport = cx.update(|window, _| window.viewport_size());
    view.update_in(cx, |view, _, cx| {
        view.sidebar_context_menu = Some(SidebarContextMenu {
            kind: SidebarMenuKind::Header(Snowflake(11)),
            anchor: point(viewport.width + px(50.), viewport.height + px(50.)),
        });
        cx.notify();
    });
    draw(cx);
    let menu = cx.debug_bounds("sidebar-context-menu").expect("header menu");
    assert!(menu.origin.x >= px(0.) && menu.origin.y >= px(0.));
    assert!(menu.origin.x + menu.size.width <= viewport.width);
    assert!(menu.origin.y + menu.size.height <= viewport.height);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.sidebar_context_menu.is_none()));
}
