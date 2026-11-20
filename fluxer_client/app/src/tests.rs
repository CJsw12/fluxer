//! Headless UI tests: real views in a virtual window, no network. The
//! `ClientState` fixture is built from a READY dispatch and a REST page,
//! the same inputs the gateway and REST client produce.

use std::sync::Arc;

use fluxer_client_core::Dispatch;
use fluxer_client_core::rest::{ClientIdentity, Endpoints, RestClient, RestConfig, SecretToken, TransportPolicy};
use fluxer_client_core::session::{
    CachedEndpoints, DisplayInfo, InstanceRecord, NewAccount, SessionError, SessionStore,
};
use fluxer_client_core::state::{LoadMode, Message, MessageState, Snowflake, bits};
use gpui_kit::base::TextSelection;
use gpui_kit::component::input::InputState;
use gpui_kit::{
    AppContext as _, Entity, Focusable as _, Modifiers, MouseButton, Pixels, Point, TestAppContext,
    VisualTestContext, point, px,
};
use chrono::{DateTime, Local};
use serde_json::json;

use crate::backend::{Backend, GatewayStatus, Instance, UiEvent, lock_sent};
use crate::client_view::{ClientView, Connection};
use crate::login::{Authenticated, LoginEvent, LoginView, Step};
use crate::root::{AppRoot, ResumeError, Screen};
use crate::ack::ACK_DELAY;
use crate::format::format_message_time;
use crate::ui::composer::ComposerMode;
use crate::ui::message_actions::{ActionTarget, MessageAction};
use crate::view_model::{ChannelIcon, Place};
use crate::{Args, parse_args};

const GUILD: Snowflake = Snowflake(1);
const GENERAL: Snowflake = Snowflake(11);
const ANNOUNCEMENTS: Snowflake = Snowflake(12);
const VOICE: Snowflake = Snowflake(13);

/// A current-thread runtime that is never driven: network jobs the views
/// spawn (history load, send) are queued but never run, so no foreign thread
/// wakes the deterministic GPUI test scheduler.
pub(crate) fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
}

/// A REST client for an unreachable instance; tests never wait on it.
pub(crate) fn offline_rest() -> Arc<RestClient> {
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

pub(crate) fn msg_id(n: u64) -> Snowflake {
    Snowflake::from_unix_ms(1_763_000_000_000 + n * 1000)
}

fn ready() -> Dispatch {
    let send_and_view = bits::VIEW_CHANNEL | bits::SEND_MESSAGES | bits::READ_MESSAGE_HISTORY | bits::ATTACH_FILES;
    Dispatch {
        t: "READY".to_owned(),
        s: 1,
        d: json!({
            "session_id": "session-1",
            "user": { "id": "100", "username": "me" },
            "guilds": [{
                "id": GUILD.to_string(),
                "properties": { "name": "Rust Port", "owner_id": "999" },
                "roles": [{ "id": GUILD.to_string(), "name": "@everyone", "permissions": send_and_view.to_string() }],
                "channels": [
                    { "id": "10", "type": 4, "name": "Text", "position": 0 },
                    { "id": GENERAL.to_string(), "type": 0, "name": "general", "parent_id": "10", "position": 0,
                      "last_message_id": msg_id(3).to_string() },
                    { "id": ANNOUNCEMENTS.to_string(), "type": 0, "name": "announcements", "parent_id": "10",
                      "position": 1,
                      "permission_overwrites": [{ "id": GUILD.to_string(), "type": 0, "allow": "0",
                                                  "deny": bits::SEND_MESSAGES.to_string() }] },
                    { "id": VOICE.to_string(), "type": 2, "name": "Lounge", "parent_id": "10", "position": 2 }
                ],
                "members": [{ "user": { "id": "100", "username": "me" }, "roles": [] }]
            }],
            "private_channels": [],
            "read_states": [{ "id": GENERAL.to_string(), "last_message_id": msg_id(1).to_string(), "mention_count": 2 }]
        }),
    }
}

/// `GET /channels/11/messages` response (newest first).
fn general_page() -> Vec<Message> {
    let bodies = [
        (1, "Alice", "**hello** from alice"),
        (2, "Bob", "second `message`"),
        (3, "Alice", "selectme third line"),
    ];
    let mut page: Vec<Message> = bodies
        .iter()
        .map(|(n, author, content)| {
            serde_json::from_value(json!({
                "id": msg_id(*n).to_string(),
                "channel_id": GENERAL.to_string(),
                "author": { "id": (200 + n).to_string(), "username": author.to_lowercase(), "global_name": author },
                "content": content,
                "timestamp": "2026-11-18T10:20:30.000000+00:00"
            }))
            .expect("message fixture")
        })
        .collect();
    page.reverse();
    page
}

pub(crate) fn backend_with_fixture(rt: &tokio::runtime::Runtime) -> (Backend, tokio::sync::mpsc::UnboundedReceiver<crate::backend::UiEvent>) {
    let (backend, events) = Backend::new(rt.handle().clone(), offline_rest(), None);
    {
        let mut state = backend.state();
        assert!(!state.apply(&ready()).is_empty());
        state.messages.load(GENERAL, general_page(), LoadMode::Replace);
    }
    (backend, events)
}

pub(crate) fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
}

pub(crate) fn open_client(cx: &mut TestAppContext, backend: Backend, events: tokio::sync::mpsc::UnboundedReceiver<crate::backend::UiEvent>) -> (Entity<ClientView>, &mut VisualTestContext) {
    cx.update(gpui_kit::init);
    cx.update(crate::shortcuts::bind_keys);
    let mut content = None;
    let (_, vcx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| ClientView::new(backend, events, window, cx));
        content = Some(view.clone());
        gpui_kit::base::Root::new(view, window, cx)
    });
    let view = content.expect("window built its content");
    vcx.run_until_parked();
    draw(vcx);
    (view, vcx)
}

/// Scans from the bottom of the message pane for a selectable text point.
fn find_selectable_from_bottom(cx: &mut VisualTestContext, x: Pixels) -> Option<Point<Pixels>> {
    let height = cx.update(|window, _| window.viewport_size().height);
    let mut y = height - px(1.);
    while y > px(0.) {
        let p = point(x, y);
        cx.simulate_mouse_move(p, None, Modifiers::default());
        draw(cx);
        if cx.update(|window, cx| TextSelection::is_selectable_at(p, window, cx)) {
            return Some(p);
        }
        y -= px(3.);
    }
    None
}

#[gpui_kit::test]
fn client_layout_renders_fixture_state(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_client(cx, backend, events);

    view.update(cx, |v, cx| v.select_place(Place::Guild(GUILD), cx));
    cx.run_until_parked();
    draw(cx);

    view.read_with(cx, |v, _| {
        assert_eq!(v.guilds.len(), 1);
        assert_eq!(v.guilds[0].name.as_ref(), "Rust Port");
        assert!(v.guilds[0].unread.unread, "guild should show the unread channel");
        let names: Vec<&str> = v.channels.iter().map(|c| c.name.as_ref()).collect();
        assert_eq!(names, ["Text", "general", "announcements", "Lounge"]);
        assert_eq!(v.channels[3].icon, ChannelIcon::Voice);
        assert!(!v.channels[3].has_messages, "voice channels have no message pane here");
        assert!(v.channels[0].is_category);
        let general = &v.channels[1];
        assert!(general.unread.unread);
        assert_eq!(general.unread.mention_count, 2);
        assert!(!v.channels[2].unread.unread);
        // Selecting the guild opens its first text channel.
        assert_eq!(v.selected_channel, Some(GENERAL));
        assert_eq!(v.channel_title.as_ref(), "general");
        let rows = v.rows.borrow();
        let authors: Vec<&str> = rows.iter().map(|r| r.author.as_ref()).collect();
        assert_eq!(authors, ["Alice", "Bob", "Alice"]);
        let at = DateTime::parse_from_rfc3339("2026-11-18T10:20:30+00:00").expect("ts").with_timezone(&Local);
        assert_eq!(rows[0].time.as_ref(), format_message_time(&at, &Local::now()));
        assert!(rows.iter().all(|r| r.group_start), "alternating authors each start a group");
        assert_eq!(v.window_title, "(2) Fluxer - #general", "two unread mentions");
        assert!(rows.iter().all(|r| r.state == MessageState::Sent));
    });

    // The message rows are actually laid out: drag over the newest one and
    // its markdown body is selected as text.
    // Rail 64 + channel list 220 + row padding 16 + avatar gutter 52.
    let text_left = 64. + 220. + 16. + 52.;
    let x = px(text_left + 30.);
    let start = find_selectable_from_bottom(cx, x).expect("no selectable message text in the pane");
    cx.simulate_mouse_down(point(px(text_left + 1.), start.y), MouseButton::Left, Modifiers::default());
    draw(cx);
    let width = cx.update(|window, _| window.viewport_size().width);
    for step in [x, x + px(100.), width - px(2.)] {
        cx.simulate_mouse_move(point(step, start.y), Some(MouseButton::Left), Modifiers::default());
        draw(cx);
    }
    cx.simulate_mouse_up(point(width - px(2.), start.y), MouseButton::Left, Modifiers::default());
    draw(cx);
    let selected = cx.update(|window, cx| TextSelection::selected_text(window, cx));
    assert!(selected.contains("selectme"), "dragging over the newest message selected {selected:?}");
    drop(rt);
}

#[gpui_kit::test]
fn composer_follows_send_permission(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_client(cx, backend, events);

    // Nothing selected: disabled.
    let composer = view.read_with(cx, |v, _| v.composer.clone());
    assert!(composer.read_with(cx, |s, _| s.presentation().is_disabled()));

    view.update(cx, |v, cx| v.select_place(Place::Guild(GUILD), cx));
    view.update(cx, |v, cx| v.select_channel(ANNOUNCEMENTS, cx));
    cx.run_until_parked();
    draw(cx);
    assert!(!view.read_with(cx, |v, _| v.can_send));
    assert!(composer.read_with(cx, |s, _| s.presentation().is_disabled()), "@everyone is denied SEND_MESSAGES here");

    view.update(cx, |v, cx| v.select_channel(GENERAL, cx));
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| v.can_send));
    assert!(!composer.read_with(cx, |s, _| s.presentation().is_disabled()));

    // Typing + Enter adds an optimistic pending row at the end and clears the input.
    cx.update(|window, cx| {
        let focus = composer.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
    });
    cx.simulate_input("pending hello");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    draw(cx);
    view.read_with(cx, |v, _| {
        let rows = v.rows.borrow();
        let last = rows.last().expect("rows");
        assert_eq!(last.body.as_ref(), "pending hello");
        assert_eq!(last.author.as_ref(), "me");
        assert_ne!(last.state, MessageState::Sent);
        assert_eq!(rows.len(), 4);
    });
    assert_eq!(composer.read_with(cx, |s, _| s.value()).as_ref(), "");

    // The pending row gets no action bar.
    let pending = view.read_with(cx, |v, _| v.rows.borrow()[3].id);
    let selector: &'static str = Box::leak(format!("message-{}", pending.0).into_boxed_str());
    let at = cx.debug_bounds(selector).expect("pending row drawn").center();
    cx.simulate_mouse_move(at, None, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert_eq!(view.read_with(cx, |v, _| v.hovered_message), None, "pending rows have no actions");
    drop(rt);
}

/// Gateway MESSAGE_CREATE and an older REST page both reach the scroller:
/// rows are appended/prepended and the virtual list's count follows.
#[gpui_kit::test]
fn dispatches_and_older_pages_update_rows(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_client(cx, backend.clone(), events);
    view.update(cx, |v, cx| v.select_place(Place::Guild(GUILD), cx));

    let create = Dispatch {
        t: "MESSAGE_CREATE".to_owned(),
        s: 2,
        d: json!({
            "id": msg_id(4).to_string(),
            "channel_id": GENERAL.to_string(),
            "author": { "id": "201", "username": "alice", "global_name": "Alice" },
            "content": "live message",
            "timestamp": "2026-11-18T10:21:00+00:00"
        }),
    };
    assert!(!backend.state().apply(&create).is_empty());
    view.update(cx, |v, cx| v.handle_events(vec![UiEvent::StateChanged], cx));
    cx.run_until_parked();
    draw(cx);
    view.read_with(cx, |v, cx| {
        assert_eq!(v.rows.borrow().len(), 4);
        assert_eq!(v.rows.borrow()[3].body.as_ref(), "live message");
        assert_eq!(v.scroller.read(cx).item_count(), 4);
    });

    let older: Vec<Message> = (0..2u64)
        .rev()
        .map(|n| {
            serde_json::from_value(json!({
                "id": Snowflake(msg_id(0).0 - 10 + n).to_string(),
                "channel_id": GENERAL.to_string(),
                "author": { "id": "202", "username": "bob" },
                "content": format!("older {n}"),
            }))
            .expect("message fixture")
        })
        .collect();
    backend.state().messages.load(GENERAL, older, LoadMode::Before);
    view.update(cx, |v, cx| {
        v.handle_events(vec![backend.history_loaded(GENERAL, None)], cx)
    });
    cx.run_until_parked();
    draw(cx);
    view.read_with(cx, |v, cx| {
        let rows = v.rows.borrow();
        let bodies: Vec<&str> = rows.iter().map(|r| r.body.as_ref()).collect();
        assert_eq!(bodies[..2], ["older 0", "older 1"]);
        assert_eq!(rows.len(), 6);
        assert_eq!(v.scroller.read(cx).item_count(), 6);
    });
    drop(rt);
}

#[gpui_kit::test]
fn voice_channels_do_not_open(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_client(cx, backend, events);
    view.update(cx, |v, cx| v.select_place(Place::Guild(GUILD), cx));
    view.update(cx, |v, cx| v.select_channel(VOICE, cx));
    cx.run_until_parked();
    draw(cx);
    view.read_with(cx, |v, _| {
        assert_eq!(v.selected_channel, Some(GENERAL), "clicking a voice channel keeps the text channel open");
        assert_eq!(v.channel_title.as_ref(), "general");
    });
    drop(rt);
}

fn message_create(n: u64, content: &str) -> Dispatch {
    Dispatch {
        t: "MESSAGE_CREATE".to_owned(),
        s: 10 + n,
        d: json!({
            "id": msg_id(n).to_string(),
            "channel_id": GENERAL.to_string(),
            "author": { "id": "201", "username": "alice", "global_name": "Alice" },
            "content": content,
            "timestamp": "2026-11-18T10:21:00+00:00"
        }),
    }
}

/// Viewing an unread channel at the bottom acks its newest message after
/// the debounce delay, once; a new message is acked again.
#[gpui_kit::test]
fn viewing_a_channel_acks_it_after_a_delay(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_client(cx, backend.clone(), events);
    view.update(cx, |v, cx| v.select_place(Place::Guild(GUILD), cx));
    cx.run_until_parked();
    draw(cx);
    let ack_of = |backend: &Backend| backend.state().read_states.get(&GENERAL).and_then(|r| r.ack_message_id);
    assert_eq!(ack_of(&backend), Some(msg_id(1)), "not acked before the delay");

    cx.executor().advance_clock(ACK_DELAY);
    cx.run_until_parked();
    draw(cx);
    assert_eq!(ack_of(&backend), Some(msg_id(3)));
    view.read_with(cx, |v, _| {
        let general = &v.channels[1];
        assert!(!general.unread.unread, "the unread dot clears");
        assert_eq!(general.unread.mention_count, 0);
        assert!(!v.guilds[0].unread.unread);
    });

    assert!(!backend.state().apply(&message_create(4, "new")).is_empty());
    view.update(cx, |v, cx| v.handle_events(vec![UiEvent::StateChanged], cx));
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| v.channels[1].unread.unread));
    cx.executor().advance_clock(ACK_DELAY);
    cx.run_until_parked();
    draw(cx);
    assert_eq!(ack_of(&backend), Some(msg_id(4)));
    assert!(!view.read_with(cx, |v, _| v.channels[1].unread.unread));
    drop(rt);
}

fn open_login<'a>(cx: &'a mut TestAppContext, rt: &tokio::runtime::Runtime) -> (Entity<LoginView>, &'a mut VisualTestContext) {
    cx.update(gpui_kit::init);
    let handle = rt.handle().clone();
    let mut content = None;
    let (_, vcx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| LoginView::new(handle, false, None, window, cx));
        content = Some(view.clone());
        gpui_kit::base::Root::new(view, window, cx)
    });
    let view = content.expect("window built its content");
    vcx.run_until_parked();
    draw(vcx);
    (view, vcx)
}

#[gpui_kit::test]
fn login_form_validates_before_any_request(cx: &mut TestAppContext) {
    let rt = runtime();
    let (view, cx) = open_login(cx, &rt);
    let (instance, email, password) =
        view.read_with(cx, |v, _| (v.instance_input.clone(), v.email_input.clone(), v.password_input.clone()));
    assert!(password.read_with(cx, |s, _| s.presentation().is_masked()), "password must be masked");
    assert_eq!(instance.read_with(cx, |s, _| s.value()).as_ref(), "fluxer.app");

    // Type an invalid e-mail and press Enter in the field.
    cx.update(|window, cx| {
        let focus = email.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
    });
    cx.simulate_input("nobody");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    view.read_with(cx, |v, _| {
        assert_eq!(v.error.as_ref().map(|e| e.as_ref()), Some("The e-mail address is not valid."));
        assert!(!v.busy, "invalid input must not start a request");
        assert!(matches!(v.step, Step::Credentials));
    });

    // Fix the e-mail; the empty password is the next error.
    cx.update(|window, cx| email.update(cx, |s, cx| s.set_value("me@example.com", window, cx)));
    cx.update(|window, cx| view.update(cx, |v, cx| v.submit(window, cx)));
    cx.run_until_parked();
    draw(cx);
    view.read_with(cx, |v, _| {
        assert_eq!(v.error.as_ref().map(|e| e.as_ref()), Some("Enter your password."));
        assert!(!v.busy);
    });

    // Clearing the instance is caught too.
    cx.update(|window, cx| instance.update(cx, |s, cx| s.set_value("", window, cx)));
    cx.update(|window, cx| password.update(cx, |s, cx| s.set_value("pw", window, cx)));
    cx.update(|window, cx| view.update(cx, |v, cx| v.submit(window, cx)));
    view.read_with(cx, |v, _| {
        assert_eq!(v.error.as_ref().map(|e| e.as_ref()), Some("Enter the instance address."));
        assert!(!v.busy);
    });
    drop(rt);
}

/// A unique store directory, removed on drop.
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let path = std::env::temp_dir().join(format!("fluxer-app-test-{name}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&path).expect("temp dir");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const OFFLINE_API: &str = "https://fluxer.invalid/api";

fn open_store(dir: &TempDir) -> Result<SessionStore, SessionError> {
    SessionStore::open_with(&dir.0, TransportPolicy::default())
}

/// A saved account for the unreachable test instance, with cached endpoints.
fn saved_account(dir: &TempDir) -> String {
    let mut store = open_store(dir).expect("store");
    let key = store
        .save_account(NewAccount {
            user_id: "100".to_owned(),
            instance: InstanceRecord {
                origin: "https://fluxer.invalid".to_owned(),
                api_endpoint: OFFLINE_API.to_owned(),
                endpoints: Some(CachedEndpoints {
                    api_code_version: 1,
                    api: OFFLINE_API.to_owned(),
                    gateway: "wss://fluxer.invalid/gateway".to_owned(),
                    media: None,
                    static_cdn: None,
                    upload_relay: None,
                    invite: None,
                    webapp: None,
                    discovered_at_ms: 1,
                }),
            },
            display: DisplayInfo { username: "me".to_owned(), global_name: None, avatar: None },
            token: SecretToken::new("flx_saved.token-0123456789"),
        })
        .expect("save account");
    // Dropping the store releases its lock for the app under test.
    drop(store);
    key
}

fn offline_auth(user: serde_json::Value) -> Authenticated {
    let endpoints = Endpoints {
        api_code_version: 1,
        api: OFFLINE_API.to_owned(),
        gateway: "wss://fluxer.invalid/gateway".to_owned(),
        media: None,
        static_cdn: None,
        upload_relay: None,
        invite: None,
        webapp: None,
        discovery_url: "https://fluxer.invalid/.well-known/fluxer".to_owned(),
        document: json!({}),
    };
    Authenticated {
        instance: Instance { endpoints, rest: offline_rest() },
        token: SecretToken::new("flx_fresh.token-0123456789"),
        user_id: "100".to_owned(),
        user,
    }
}

fn open_root<'a>(
    cx: &'a mut TestAppContext,
    rt: &tokio::runtime::Runtime,
    store: Result<SessionStore, SessionError>,
) -> (Entity<AppRoot>, &'a mut VisualTestContext) {
    cx.update(gpui_kit::init);
    let handle = rt.handle().clone();
    let mut content = None;
    let (_, vcx) = cx.add_window_view(|window, cx| {
        let root = cx.new(|cx| AppRoot::new_session(handle, false, store, window, cx));
        content = Some(root.clone());
        gpui_kit::base::Root::new(root, window, cx)
    });
    let root = content.expect("window built its content");
    vcx.run_until_parked();
    draw(vcx);
    (root, vcx)
}

/// Releases the store lock before the temp dir is removed.
fn close_store(root: &Entity<AppRoot>, cx: &mut VisualTestContext) {
    root.update(cx, |r, _| r.store = None);
}

fn stored_keys(root: &Entity<AppRoot>, cx: &mut VisualTestContext) -> Vec<String> {
    root.read_with(cx, |r, _| {
        r.store.as_ref().expect("store").list_accounts().into_iter().map(|a| a.key).collect()
    })
}

/// Startup with an active saved account: the token check stays queued on
/// the runtime; its success opens the client and refreshes the display info.
#[gpui_kit::test]
fn startup_resumes_the_saved_account(cx: &mut TestAppContext) {
    let rt = runtime();
    let dir = TempDir::new("resume");
    let key = saved_account(&dir);
    let (root, cx) = open_root(cx, &rt, open_store(&dir));
    root.read_with(cx, |r, _| {
        assert!(matches!(&r.screen, Screen::Checking(text) if text.as_ref() == "Signing in…"));
        assert_eq!(r.account_key.as_deref(), Some(key.as_str()));
    });

    let user = json!({ "id": "100", "username": "me", "global_name": "Me Again", "avatar": null });
    root.update_in(cx, |r, window, cx| r.finish_resume(Ok(offline_auth(user)), window, cx));
    cx.run_until_parked();
    draw(cx);
    root.read_with(cx, |r, _| {
        assert!(matches!(r.screen, Screen::Client(_)));
        let active = r.store.as_ref().and_then(|s| s.active_account()).expect("still saved");
        assert_eq!(active.key, key);
        assert_eq!(active.display.global_name.as_deref(), Some("Me Again"));
        assert!(active.instance.endpoints.is_some(), "the cached endpoints are kept");
    });
    close_store(&root, cx);
    drop(rt);
}

/// A 401 on the saved token forgets the account and shows the login form.
#[gpui_kit::test]
fn rejected_saved_token_is_forgotten(cx: &mut TestAppContext) {
    let rt = runtime();
    let dir = TempDir::new("rejected");
    saved_account(&dir);
    let (root, cx) = open_root(cx, &rt, open_store(&dir));
    root.update_in(cx, |r, window, cx| {
        r.finish_resume(Err(ResumeError::Rejected("401 UNAUTHORIZED".to_owned())), window, cx)
    });
    cx.run_until_parked();
    draw(cx);
    assert!(stored_keys(&root, cx).is_empty());
    root.read_with(cx, |r, cx| {
        assert!(r.account_key.is_none());
        let Screen::Login(login) = &r.screen else { panic!("login screen expected") };
        assert!(login.read(cx).error.is_some(), "the login form says why");
    });
    close_store(&root, cx);
    drop(rt);
}

/// Login saves the account; "Log out" sends REST logout (queued) and
/// removes it from the store.
#[gpui_kit::test]
fn login_saves_and_logout_clears_the_store(cx: &mut TestAppContext) {
    let rt = runtime();
    let dir = TempDir::new("logout");
    let (root, cx) = open_root(cx, &rt, open_store(&dir));
    let login = root.read_with(cx, |r, _| match &r.screen {
        Screen::Login(login) => login.clone(),
        _ => panic!("no saved account: login screen expected"),
    });
    let user = json!({ "id": "100", "username": "me", "avatar": "abc" });
    login.update(cx, |_, cx| cx.emit(LoginEvent::Authenticated(offline_auth(user))));
    cx.run_until_parked();
    draw(cx);
    let keys = stored_keys(&root, cx);
    assert_eq!(keys, ["https://fluxer.invalid/api::100"]);
    let client = root.read_with(cx, |r, _| {
        let active = r.store.as_ref().and_then(|s| s.active_account()).expect("saved and active");
        assert_eq!(active.instance.origin, "https://fluxer.invalid");
        assert_eq!(active.display.avatar.as_deref(), Some("abc"));
        assert_eq!(r.account_key.as_deref(), Some(keys[0].as_str()));
        match &r.screen {
            Screen::Client(client) => client.clone(),
            _ => panic!("client screen expected"),
        }
    });

    client.update(cx, |v, cx| v.log_out(cx));
    cx.run_until_parked();
    draw(cx);
    assert!(stored_keys(&root, cx).is_empty());
    root.read_with(cx, |r, _| {
        assert!(matches!(r.screen, Screen::Login(_)));
        assert!(r.account_key.is_none());
        assert!(r.store.as_ref().is_some_and(|s| s.active_account().is_none()));
    });
    close_store(&root, cx);
    drop(rt);
}

/// A second instance finds the store locked and only shows the notice.
#[gpui_kit::test]
fn locked_store_shows_already_running(cx: &mut TestAppContext) {
    let rt = runtime();
    let dir = TempDir::new("locked");
    let first = open_store(&dir).expect("first instance");
    let second = open_store(&dir);
    assert!(matches!(second, Err(SessionError::Locked { .. })), "second open must be Locked");
    let (root, cx) = open_root(cx, &rt, second);
    root.read_with(cx, |r, _| {
        assert!(matches!(r.screen, Screen::AlreadyRunning));
        assert!(r.store.is_none());
    });
    drop(first);
    drop(rt);
}

/// The user panel shows the READY user and the gateway state.
#[gpui_kit::test]
fn user_panel_follows_user_and_gateway(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_client(cx, backend, events);
    view.read_with(cx, |v, _| {
        let user = v.user.as_ref().expect("READY user");
        assert_eq!(user.name.as_ref(), "me");
        assert_eq!(user.initials.as_ref(), "m");
        assert_eq!(v.connection, Connection::Connecting);
    });
    view.update(cx, |v, cx| v.handle_events(vec![UiEvent::Gateway(GatewayStatus::Ready)], cx));
    draw(cx);
    assert_eq!(view.read_with(cx, |v, _| v.connection), Connection::Online);
    view.update(cx, |v, cx| {
        v.handle_events(vec![UiEvent::Gateway(GatewayStatus::Closed { reason: "4004".to_owned() })], cx)
    });
    draw(cx);
    assert_eq!(view.read_with(cx, |v, _| v.connection), Connection::Offline);
    drop(rt);
}

#[test]
fn args_parse() {
    let parsed = parse_args(["--token-env", "FLUXER_TOKEN", "--instance", "localhost:8080", "--allow-insecure-loopback", "--autostart"]
        .map(String::from))
    .expect("args");
    assert_eq!(
        parsed,
        Args {
            instance: Some("localhost:8080".to_owned()),
            token_env: Some("FLUXER_TOKEN".to_owned()),
            allow_insecure_loopback: true,
            autostart: true,
            deep_link: None,
        }
    );
    assert!(parse_args(["--token-env".to_owned()]).is_err());
    assert!(parse_args(["--bogus".to_owned()]).is_err());
}

/// The test window's composer, focused.
pub(crate) fn focus_composer(view: &Entity<ClientView>, cx: &mut VisualTestContext) -> Entity<InputState> {
    let composer = view.read_with(cx, |v, _| v.composer.clone());
    cx.update(|window, cx| {
        let focus = composer.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
    });
    composer
}

pub(crate) fn composer_text(composer: &Entity<InputState>, cx: &mut VisualTestContext) -> String {
    composer.read_with(cx, |s, _| s.value().to_string())
}

pub(crate) fn refresh(view: &Entity<ClientView>, cx: &mut VisualTestContext) {
    view.update(cx, |v, cx| v.handle_events(vec![UiEvent::StateChanged], cx));
    cx.run_until_parked();
    draw(cx);
}

pub(crate) fn open_general(
    cx: &mut TestAppContext,
    backend: Backend,
    events: tokio::sync::mpsc::UnboundedReceiver<UiEvent>,
) -> (Entity<ClientView>, &mut VisualTestContext) {
    let (view, cx) = open_client(cx, backend, events);
    view.update(cx, |v, cx| v.select_place(Place::Guild(GUILD), cx));
    cx.run_until_parked();
    draw(cx);
    (view, cx)
}

fn action_target(view: &Entity<ClientView>, row: usize, cx: &mut VisualTestContext) -> ActionTarget {
    view.read_with(cx, |v, _| v.rows.borrow()[row].actions.clone())
}

/// Reply from the action bar: the composer is in "Replying to Bob", Enter
/// posts with a `message_reference` to Bob's message and the mention on.
#[gpui_kit::test]
fn reply_sends_a_message_reference(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_general(cx, backend.clone(), events);
    let target = action_target(&view, 1, cx);
    assert_eq!(target.message_id, msg_id(2));
    view.update_in(cx, |v, window, cx| v.message_action(MessageAction::Reply, &target, window, cx));
    draw(cx);
    view.read_with(cx, |v, _| match v.composer_mode.mode() {
        ComposerMode::Replying(r) => assert_eq!((r.author.as_ref(), r.mention), ("Bob", true)),
        other => panic!("replying expected, got {other:?}"),
    });

    let composer = focus_composer(&view, cx);
    cx.simulate_input("thanks bob");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    draw(cx);
    let (channel_id, request) = lock_sent(&backend.sent).last().cloned().expect("a POST was sent");
    assert_eq!(channel_id, GENERAL);
    assert_eq!(request.content.as_deref(), Some("thanks bob"));
    let reference = request.message_reference.expect("message_reference");
    assert_eq!(reference.message_id, msg_id(2).to_string());
    assert_eq!(reference.channel_id, Some(GENERAL.to_string()));
    assert_eq!(reference.guild_id, Some(GUILD.to_string()));
    assert_eq!(request.allowed_mentions, Some(json!({ "replied_user": true })));
    assert_eq!(composer_text(&composer, cx), "");
    view.read_with(cx, |v, _| {
        assert_eq!(v.composer_mode.mode(), &ComposerMode::Normal);
        let rows = v.rows.borrow();
        let last = rows.last().expect("rows");
        assert_eq!((last.body.as_ref(), last.state), ("thanks bob", MessageState::Sending));
    });

    // A plain message afterwards has no reference.
    cx.simulate_input("plain");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let (_, plain) = lock_sent(&backend.sent).last().cloned().expect("second POST");
    assert_eq!((plain.content.as_deref(), plain.message_reference), (Some("plain"), None));
    drop(rt);
}

fn own_message_create(n: u64, content: &str) -> Dispatch {
    Dispatch {
        t: "MESSAGE_CREATE".to_owned(),
        s: 10 + n,
        d: json!({
            "id": msg_id(n).to_string(),
            "channel_id": GENERAL.to_string(),
            "author": { "id": "100", "username": "me" },
            "content": content,
            "timestamp": "2026-11-18T10:21:00+00:00"
        }),
    }
}

/// Up in the empty composer edits the newest own message, Esc restores
/// the draft from before the edit, and Enter saves an edit optimistically.
#[gpui_kit::test]
fn up_arrow_edits_the_last_own_message(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_general(cx, backend.clone(), events);
    assert!(!backend.state().apply(&own_message_create(4, "my last words")).is_empty());
    refresh(&view, cx);
    let composer = focus_composer(&view, cx);

    cx.simulate_keystrokes("up");
    cx.run_until_parked();
    assert_eq!(composer_text(&composer, cx), "my last words");
    assert!(view.read_with(cx, |v, _| matches!(v.composer_mode.mode(), ComposerMode::Editing(_))));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(composer_text(&composer, cx), "");
    assert!(view.read_with(cx, |v, _| v.composer_mode.mode() == &ComposerMode::Normal));

    // Edit from the action bar keeps the typed draft for afterwards.
    cx.simulate_input("half a draft");
    let target = action_target(&view, 3, cx);
    view.update_in(cx, |v, window, cx| v.message_action(MessageAction::Edit, &target, window, cx));
    draw(cx);
    assert_eq!(composer_text(&composer, cx), "my last words");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(composer_text(&composer, cx), "half a draft");

    // Save an edit: the row changes now and the draft comes back.
    view.update_in(cx, |v, window, cx| v.message_action(MessageAction::Edit, &target, window, cx));
    cx.update(|window, cx| composer.update(cx, |s, cx| s.set_value("my edited words", window, cx)));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    draw(cx);
    assert_eq!(composer_text(&composer, cx), "half a draft");
    view.read_with(cx, |v, _| {
        assert_eq!(v.rows.borrow()[3].body.as_ref(), "my edited words");
        assert_eq!(v.composer_mode.mode(), &ComposerMode::Normal);
    });
    drop(rt);
}

fn reaction_of(view: &Entity<ClientView>, row: usize, name: &str, cx: &mut VisualTestContext) -> Option<(u32, bool)> {
    view.read_with(cx, |v, _| {
        v.rows.borrow()[row].reactions.iter().find(|p| p.emoji.name == name).map(|p| (p.count, p.me))
    })
}

fn click_selector(cx: &mut VisualTestContext, selector: String) {
    let selector: &'static str = Box::leak(selector.into_boxed_str());
    let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} not drawn"));
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.run_until_parked();
    draw(cx);
}

/// Clicking a reaction pill adds the user's reaction at once (REST is still
/// queued); the quick reactions add a new one.
#[gpui_kit::test]
fn reaction_click_toggles_the_pill(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_general(cx, backend.clone(), events);
    let add = Dispatch {
        t: "MESSAGE_REACTION_ADD".to_owned(),
        s: 5,
        d: json!({ "channel_id": GENERAL.to_string(), "message_id": msg_id(3).to_string(), "user_id": "201",
                   "emoji": { "id": null, "name": "\u{1F44D}" } }),
    };
    assert!(!backend.state().apply(&add).is_empty());
    refresh(&view, cx);
    assert_eq!(reaction_of(&view, 2, "\u{1F44D}", cx), Some((1, false)));

    click_selector(cx, format!("reaction-{}-0", msg_id(3).0));
    assert_eq!(reaction_of(&view, 2, "\u{1F44D}", cx), Some((2, true)), "own reaction added optimistically");

    // Hovering the row shows its action bar (only that row builds one).
    let pill: &'static str = Box::leak(format!("reaction-{}-0", msg_id(3).0).into_boxed_str());
    let at = cx.debug_bounds(pill).expect("pill drawn").center();
    cx.simulate_mouse_move(at, None, Modifiers::default());
    draw(cx);
    assert_eq!(view.read_with(cx, |v, _| v.hovered_message), Some(msg_id(3)));

    let target = action_target(&view, 2, cx);
    view.update_in(cx, |v, window, cx| v.message_action(MessageAction::React, &target, window, cx));
    draw(cx);
    assert_eq!(view.read_with(cx, |v, _| v.reaction_picker), Some(msg_id(3)));
    click_selector(cx, "quick-reaction-\u{1F389}".to_owned());
    assert_eq!(reaction_of(&view, 2, "\u{1F389}", cx), Some((1, true)));
    assert_eq!(view.read_with(cx, |v, _| v.reaction_picker), None, "picking closes the quick reactions");
    drop(rt);
}

/// A TYPING_START shows "Alice is typing..." and the row empties when the
/// typer expires (10 s after the event).
#[gpui_kit::test]
fn typing_row_appears_and_expires(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_general(cx, backend.clone(), events);
    let typing = Dispatch {
        t: "TYPING_START".to_owned(),
        s: 6,
        d: json!({ "channel_id": GENERAL.to_string(), "guild_id": GUILD.to_string(), "user_id": "201",
                   "timestamp": 0,
                   "member": { "user": { "id": "201", "username": "alice", "global_name": "Alice" }, "roles": [] } }),
    };
    // Started 9.95 s ago: expires 50 ms from now.
    let started = crate::ui::typing::now_ms() - 9_950;
    assert!(!backend.state().apply_at(&typing, started).is_empty());
    refresh(&view, cx);
    let text = view.read_with(cx, |v, _| v.typing_text.clone());
    assert_eq!(text.as_deref(), Some("Alice is typing\u{2026}"));

    // Expiry compares wall-clock times: let them pass, then fire the timer.
    std::thread::sleep(std::time::Duration::from_millis(80));
    cx.executor().advance_clock(std::time::Duration::from_millis(200));
    cx.run_until_parked();
    draw(cx);
    assert_eq!(view.read_with(cx, |v, _| v.typing_text.clone()), None);
    drop(rt);
}

/// The drawn action bar of message `n`, if any.
fn bar_drawn(n: u64, cx: &mut VisualTestContext) -> bool {
    let selector: &'static str = Box::leak(format!("message-{}-bar", msg_id(n).0).into_boxed_str());
    cx.debug_bounds(selector).is_some()
}

/// Moves the pointer to the center of message `n`'s row in small steps,
/// drawing after each, like a real mouse.
fn move_over_message(n: u64, from: Point<Pixels>, cx: &mut VisualTestContext) -> Point<Pixels> {
    let selector: &'static str = Box::leak(format!("message-{}", msg_id(n).0).into_boxed_str());
    let to = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} not drawn")).center();
    for step in 1..=8 {
        let t = step as f32 / 8.;
        let p = point(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t);
        cx.simulate_mouse_move(p, None, Modifiers::default());
        cx.run_until_parked();
        draw(cx);
    }
    to
}

/// The action bar follows the pointer from row to row, and leaves when the
/// pointer leaves the messages.
#[gpui_kit::test]
fn hover_bar_follows_the_pointer(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_general(cx, backend, events);
    let hovered = |cx: &mut VisualTestContext| view.read_with(cx, |v, _| v.hovered_message);

    let at = move_over_message(1, point(px(1.), px(1.)), cx);
    assert_eq!(hovered(cx), Some(msg_id(1)));
    assert!(bar_drawn(1, cx) && !bar_drawn(2, cx), "bar on the first row");

    let at = move_over_message(2, at, cx);
    assert_eq!(hovered(cx), Some(msg_id(2)));
    assert!(bar_drawn(2, cx) && !bar_drawn(1, cx), "bar moved to the second row");

    move_over_message(3, at, cx);
    assert_eq!(hovered(cx), Some(msg_id(3)));
    assert!(bar_drawn(3, cx) && !bar_drawn(2, cx), "bar moved to the third row");

    // The bar pokes above its row, over the previous one: reaching for it
    // keeps it on its own row.
    let bar_selector: &'static str = Box::leak(format!("message-{}-bar", msg_id(3).0).into_boxed_str());
    let bar = cx.debug_bounds(bar_selector).expect("bar drawn");
    let row2: &'static str = Box::leak(format!("message-{}", msg_id(2).0).into_boxed_str());
    assert!(bar.top() < cx.debug_bounds(row2).expect("row 2 drawn").bottom(), "fixture: bar overlaps row 2");
    let on_bar = point(bar.center().x, bar.top() + px(2.));
    cx.simulate_mouse_move(on_bar, None, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert_eq!(hovered(cx), Some(msg_id(3)), "the bar keeps its row hovered");

    // Back up, then onto the date divider above the first message.
    let at = move_over_message(1, on_bar, cx);
    assert!(bar_drawn(1, cx) && !bar_drawn(3, cx), "bar moved back up");
    let row1: &'static str = Box::leak(format!("message-{}", msg_id(1).0).into_boxed_str());
    let above = point(at.x, cx.debug_bounds(row1).expect("row 1 drawn").top() - px(16.));
    cx.simulate_mouse_move(above, None, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert_eq!(hovered(cx), None, "the divider hovers no message");

    // Then out of the message list (onto the channel list).
    let at = move_over_message(1, above, cx);
    assert_eq!(hovered(cx), Some(msg_id(1)));
    for step in 1..=8 {
        let p = point(at.x - (at.x - px(20.)) * (step as f32 / 8.), at.y);
        cx.simulate_mouse_move(p, None, Modifiers::default());
        cx.run_until_parked();
        draw(cx);
    }
    assert_eq!(hovered(cx), None);
    assert!(!bar_drawn(1, cx), "bar gone once the pointer leaves");
    drop(rt);
}

/// A new session's READY clears the message windows: the open channel
/// loads its latest page again. RESUMED keeps the window and loads nothing.
#[gpui_kit::test]
fn ready_reloads_the_open_channel(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_general(cx, backend.clone(), events);
    view.update(cx, |v, cx| v.handle_events(vec![UiEvent::Gateway(GatewayStatus::Ready)], cx));
    assert!(lock_sent(&backend.page_loads).is_empty(), "resumed: the window is intact");

    let mut new_session = ready();
    new_session.d["session_id"] = json!("session-2");
    backend.state().apply(&new_session);
    assert!(backend.state().messages.window(GENERAL).is_none());
    view.update(cx, |v, cx| v.handle_events(vec![UiEvent::Gateway(GatewayStatus::Ready)], cx));
    assert_eq!(*lock_sent(&backend.page_loads), [(GENERAL, LoadMode::Replace, None)]);
    assert!(view.read_with(cx, |v, _| v.loading_history));
    drop(rt);
}

#[gpui_kit::test]
fn ready_retries_pending_jump_and_ignores_the_previous_completion(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_general(cx, backend.clone(), events);
    let target = msg_id(40);
    view.update(cx, |v, cx| v.jump_to_message(GENERAL, target, cx));
    let previous = backend.history_loaded(GENERAL, Some("stale failure".to_owned()));

    let mut fresh_ready = ready();
    fresh_ready.d["session_id"] = json!("session-2");
    backend.state().apply(&fresh_ready);
    view.update(cx, |v, cx| {
        v.handle_events(vec![UiEvent::Gateway(GatewayStatus::Ready), previous], cx);
    });

    assert_eq!(*lock_sent(&backend.page_loads), [
        (GENERAL, LoadMode::Around, Some(target)),
        (GENERAL, LoadMode::Around, Some(target)),
    ]);
    assert!(view.read_with(cx, |v, _| v.loading_history && v.pending_jump.is_some() && v.error.is_none()));
    let mut message = general_page().pop().unwrap();
    message.id = target;
    backend.state().messages.load_around(GENERAL, vec![message], target, Some(target));
    view.update(cx, |v, cx| v.handle_events(vec![backend.history_loaded(GENERAL, None)], cx));
    assert_eq!(view.read_with(cx, |v, _| v.highlighted_message), Some((GENERAL, target)));
}

#[gpui_kit::test]
fn ready_drops_revoked_channel_without_fetching_its_history(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_general(cx, backend.clone(), events);
    view.update(cx, |v, cx| v.jump_to_message(GENERAL, msg_id(40), cx));
    lock_sent(&backend.page_loads).clear();
    let mut snapshot = ready();
    snapshot.d["guilds"][0]["channels"][1]["permission_overwrites"] =
        json!([{ "id": GUILD, "type": 0, "allow": "0", "deny": bits::VIEW_CHANNEL.to_string() }]);
    backend.state().apply(&snapshot);
    view.update(cx, |v, cx| v.handle_events(vec![UiEvent::Gateway(GatewayStatus::Ready)], cx));
    assert_eq!(view.read_with(cx, |v, _| (v.selected_channel, v.pending_jump)), (Some(ANNOUNCEMENTS), None));
    assert_eq!(*lock_sent(&backend.page_loads), [(ANNOUNCEMENTS, LoadMode::Replace, None)]);
}

/// A failed page of a channel left meanwhile shows no error here.
#[gpui_kit::test]
fn history_error_of_another_channel_is_ignored(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_general(cx, backend.clone(), events);
    let failed = |channel_id| backend.history_loaded(channel_id, Some("503".to_owned()));
    view.update(cx, |v, cx| v.handle_events(vec![failed(ANNOUNCEMENTS)], cx));
    assert!(view.read_with(cx, |v, _| v.error.is_none()));
    view.update(cx, |v, cx| v.handle_events(vec![failed(GENERAL)], cx));
    assert!(view.read_with(cx, |v, _| v.error.is_some()));
    drop(rt);
}

/// Scrolling up unpins the window (incoming messages stop trimming the
/// top); at the bottom of a window whose newer messages were trimmed, the
/// page after the newest row loads (else live messages are never shown).
#[gpui_kit::test]
fn scroll_position_pins_and_loads_newer_pages(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    let (view, cx) = open_general(cx, backend.clone(), events);
    view.update(cx, |v, cx| v.scrolled(true, false, cx));
    assert!(!backend.state().messages.window(GENERAL).expect("window").pinned_to_end);
    view.update(cx, |v, cx| v.scrolled(false, false, cx));
    assert!(backend.state().messages.window(GENERAL).expect("window").pinned_to_end);
    assert!(lock_sent(&backend.page_loads).is_empty(), "nothing newer to load");

    // Four full older pages: over 200, so the newest messages are trimmed.
    for page in 0..4u64 {
        let older: Vec<Message> = (0..50u64)
            .rev()
            .map(|n| {
                serde_json::from_value(json!({
                    "id": Snowflake(msg_id(0).0 - 1_000_000 * (page + 1) + n).to_string(),
                    "channel_id": GENERAL.to_string(),
                    "author": { "id": "202", "username": "bob" },
                    "content": format!("older {page}.{n}"),
                }))
                .expect("message fixture")
            })
            .collect();
        backend.state().messages.load(GENERAL, older, LoadMode::Before);
    }
    assert!(backend.state().messages.window(GENERAL).expect("window").has_more_after);
    refresh(&view, cx);
    let newest = view.read_with(cx, |v, _| v.rows.borrow().last().map(|r| r.id)).expect("rows");
    view.update(cx, |v, cx| v.scrolled(false, false, cx));
    assert_eq!(*lock_sent(&backend.page_loads), [(GENERAL, LoadMode::After, Some(newest))]);
    assert!(view.read_with(cx, |v, _| v.loading_history));
    drop(rt);
}
