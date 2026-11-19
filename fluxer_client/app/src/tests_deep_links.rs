//! Headless GPUI tests for Milestone 7: `fluxer://` deep link routing,
//! internal HTTP(S) channel/user links, cold-start deferral across login/READY,
//! invite preview without auto-join, spoof rejection, and logout cleanup.

use fluxer_client_core::Dispatch;
use fluxer_client_core::rest::{Endpoints, SecretToken, TransportPolicy};
use fluxer_client_core::session::{
    CachedEndpoints, DisplayInfo, InstanceRecord, NewAccount, SessionError, SessionStore,
};
use fluxer_client_core::state::{LoadMode, Snowflake};
use gpui_kit::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use serde_json::json;

use crate::backend::{Instance, SocialCall, UiEvent, lock_sent};
use crate::login::Authenticated;
use crate::root::{AppRoot, Screen};
use crate::tests::{backend_with_fixture, draw, msg_id, offline_rest, open_client, runtime};
use crate::ui::overlay::Overlay;
use crate::view_model::Place;

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let path = std::env::temp_dir().join(format!("fluxer-deeplink-test-{name}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&path).expect("temp dir");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn open_store(dir: &TempDir) -> Result<SessionStore, SessionError> {
    SessionStore::open_with(&dir.0, TransportPolicy::default())
}

fn save_account(dir: &TempDir) {
    let mut store = open_store(dir).expect("store");
    store
        .save_account(NewAccount {
            user_id: "100".to_owned(),
            instance: InstanceRecord {
                origin: "https://fluxer.invalid".to_owned(),
                api_endpoint: "https://fluxer.invalid/api".to_owned(),
                endpoints: Some(CachedEndpoints {
                    api_code_version: 1,
                    api: "https://fluxer.invalid/api".to_owned(),
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
    drop(store);
}

fn offline_auth() -> Authenticated {
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
    Authenticated {
        instance: Instance { endpoints, rest: offline_rest() },
        token: SecretToken::new("flx_fresh.token-0123456789"),
        user_id: "100".to_owned(),
        user: json!({ "id": "100", "username": "me", "avatar": null }),
    }
}

fn open_root<'a>(
    cx: &'a mut TestAppContext,
    rt: &tokio::runtime::Runtime,
    store: Result<SessionStore, SessionError>,
) -> (Entity<AppRoot>, &'a mut VisualTestContext) {
    cx.update(gpui_kit::init);
    cx.update(crate::shortcuts::bind_keys);
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

fn close_store(root: &Entity<AppRoot>, cx: &mut VisualTestContext) {
    root.update(cx, |r, _| r.store = None);
}

#[gpui_kit::test]
fn cold_start_deep_link_waits_for_login_and_ready_then_jumps_to_message(cx: &mut TestAppContext) {
    let rt = runtime();
    let dir = TempDir::new("cold-start");
    save_account(&dir);
    let (root, cx) = open_root(cx, &rt, open_store(&dir));

    let jump_msg = msg_id(5);
    let deep_link = format!("fluxer://channels/1/11/{}", jump_msg.0);
    root.update_in(cx, |r, window, cx| {
        assert!(matches!(r.screen, Screen::Checking(_)));
        r.handle_deep_link(&deep_link, window, cx);
        assert!(r.pending_deep_link.is_some());
    });

    // Completing resume opens ClientView but keeps deep link queued until READY populates guilds/channels
    root.update_in(cx, |r, window, cx| r.finish_resume(Ok(offline_auth()), window, cx));
    cx.run_until_parked();
    let client = root.read_with(cx, |r, _| {
        assert!(r.pending_deep_link.is_none());
        match &r.screen {
            Screen::Client(client) => client.clone(),
            _ => panic!("expected Screen::Client"),
        }
    });
    assert!(client.read_with(cx, |v, _| v.pending_deep_link.is_some()));

    client.update(cx, |v, cx| {
        v.backend.state().apply(&Dispatch {
            t: "READY".to_owned(),
            s: 1,
            d: json!({
                "session_id": "s1",
                "user": { "id": "100", "username": "me" },
                "guilds": [{
                    "id": "1",
                    "properties": { "name": "Rust Port", "owner_id": "999" },
                    "roles": [],
                    "channels": [{ "id": "11", "type": 0, "name": "general", "position": 0 }]
                }],
                "private_channels": []
            }),
        });
        v.handle_events(vec![UiEvent::StateChanged], cx);
    });
    cx.run_until_parked();
    draw(cx);

    client.read_with(cx, |v, _| {
        assert!(v.pending_deep_link.is_none());
        assert_eq!((v.place, v.selected_channel), (Place::Guild(Snowflake(1)), Some(Snowflake(11))));
        let loads = lock_sent(&v.backend.page_loads);
        assert!(loads.contains(&(Snowflake(11), LoadMode::Around, Some(jump_msg))));
    });

    close_store(&root, cx);
    drop(rt);
}

#[gpui_kit::test]
fn in_app_protocol_and_internal_http_links_route_all_targets_and_reject_spoofed_links(cx: &mut TestAppContext) {
    let rt = runtime();
    let (backend, events) = backend_with_fixture(&rt);
    {
        let mut state = backend.state();
        state.apply(&Dispatch {
            t: "CHANNEL_CREATE".to_owned(),
            s: 2,
            d: json!({
                "id": "555",
                "type": 1,
                "recipients": [{ "id": "201", "username": "alice", "global_name": "Alice" }]
            }),
        });
    }
    let (view, cx) = open_client(cx, backend.clone(), events);

    // Invite deep link opens confirmation modal pre-filled and only previews (no auto-join)
    view.update_in(cx, |v, window, cx| {
        v.open_external_url("fluxer://invite/join-me", window, cx);
    });
    cx.run_until_parked();
    draw(cx);
    view.read_with(cx, |v, cx| {
        let Overlay::JoinGuild(modal) = &v.overlay else { panic!("expected Overlay::JoinGuild") };
        assert_eq!(modal.input.read(cx).value().as_str(), "join-me");
    });
    assert_eq!(*lock_sent(&backend.sent_social), vec![SocialCall::FetchInvite("join-me".to_owned())]);

    // DM channel and Home deep links route in-app cleanly
    view.update_in(cx, |v, window, cx| {
        v.open_external_url("fluxer://channels/@me/555", window, cx);
    });
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));
    assert_eq!(view.read_with(cx, |v, _| (v.place, v.selected_channel)), (Place::Home, Some(Snowflake(555))));

    view.update_in(cx, |v, window, cx| {
        v.open_external_url("fluxer://channels/@me", window, cx);
    });
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |v, _| (v.place, v.selected_channel)), (Place::Home, None));

    // User profile and Settings deep links open their overlays
    view.update_in(cx, |v, window, cx| {
        v.open_external_url("fluxer://users/201", window, cx);
    });
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(&v.overlay, Overlay::Profile(p) if p.user_id == Snowflake(201))));

    view.update_in(cx, |v, window, cx| {
        v.open_external_url("fluxer://settings", window, cx);
    });
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::Settings(_))));

    // Internal HTTPS link navigates to guild channel without external launch or LinkWarning
    view.update_in(cx, |v, window, cx| {
        v.open_external_url("https://fluxer.app/channels/1/11", window, cx);
    });
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, _| (v.place, v.selected_channel)),
        (Place::Guild(Snowflake(1)), Some(Snowflake(11)))
    );
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None) && v.opened_urls.is_empty()));

    // Spoofed/unknown/mismatched-guild protocol links never open external URLs or corrupt selection
    view.update_in(cx, |v, window, cx| {
        v.open_external_url("fluxer://channels/1/11?api=https://evil.example", window, cx);
        v.open_external_url("fluxer://unknown/route", window, cx);
    });
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| v.opened_urls.is_empty()));

    view.update_in(cx, |v, window, cx| {
        v.open_external_url("fluxer://channels/999/11", window, cx);
    });
    cx.run_until_parked();
    view.read_with(cx, |v, _| {
        assert_eq!((v.place, v.selected_channel), (Place::Guild(Snowflake(1)), Some(Snowflake(11))));
        assert!(v.error.is_some());
        assert!(v.opened_urls.is_empty());
    });

    drop(rt);
}

#[gpui_kit::test]
fn rejected_resume_and_logout_clear_pending_deep_link_and_delayed_guild_create_resolves(cx: &mut TestAppContext) {
    let rt = runtime();
    let dir = TempDir::new("pending-lifecycle");
    save_account(&dir);
    let (root, cx) = open_root(cx, &rt, open_store(&dir));

    // Rejected resume drops queued deep link so it never applies to a subsequent account
    root.update_in(cx, |r, window, cx| {
        r.handle_deep_link("fluxer://settings", window, cx);
        assert!(r.pending_deep_link.is_some());
        r.finish_resume(Err(crate::root::ResumeError::Rejected("401".to_owned())), window, cx);
        assert!(matches!(r.screen, Screen::Login(_)));
        assert!(r.pending_deep_link.is_none());
    });

    // Queue deep link before sign-in; READY without target guild keeps it until GUILD_CREATE arrives
    root.update_in(cx, |r, window, cx| {
        r.handle_deep_link("fluxer://channels/88/880", window, cx);
        r.start_client(offline_auth(), window, cx);
    });
    cx.run_until_parked();
    let client = root.read_with(cx, |r, _| match &r.screen {
        Screen::Client(client) => client.clone(),
        _ => panic!("expected Screen::Client"),
    });
    client.update(cx, |v, cx| {
        v.backend.state().apply(&Dispatch {
            t: "READY".to_owned(),
            s: 1,
            d: json!({
                "session_id": "s1",
                "user": { "id": "100", "username": "me" },
                "guilds": [],
                "private_channels": []
            }),
        });
        v.handle_events(vec![UiEvent::StateChanged], cx);
    });
    cx.run_until_parked();
    assert!(client.read_with(cx, |v, _| v.pending_deep_link.is_some()), "kept waiting for GUILD_CREATE");

    client.update(cx, |v, cx| {
        v.backend.state().apply(&Dispatch {
            t: "GUILD_CREATE".to_owned(),
            s: 2,
            d: json!({
                "id": "88",
                "properties": { "name": "Delayed Guild", "owner_id": "999" },
                "roles": [],
                "channels": [{ "id": "880", "type": 0, "name": "general", "position": 0 }]
            }),
        });
        v.handle_events(vec![UiEvent::StateChanged], cx);
    });
    cx.run_until_parked();
    client.read_with(cx, |v, _| {
        assert!(v.pending_deep_link.is_none());
        assert_eq!((v.place, v.selected_channel), (Place::Guild(Snowflake(88)), Some(Snowflake(880))));
    });

    // Logout clears any remaining AppRoot pending_deep_link
    root.update(cx, |r, _| {
        r.pending_deep_link = Some(crate::deep_links::DeepLink::Settings);
    });
    client.update(cx, |v, cx| v.log_out(cx));
    cx.run_until_parked();
    assert!(root.read_with(cx, |r, _| matches!(r.screen, Screen::Login(_)) && r.pending_deep_link.is_none()));

    close_store(&root, cx);
    drop(rt);
}

#[test]
fn parse_args_accepts_positional_and_url_flag_deep_links_and_blocks_smuggling() {
    let positional = crate::parse_args(["fluxer://channels/@me/123"].map(String::from)).expect("positional deep link");
    assert_eq!(positional.deep_link.as_deref(), Some("fluxer://channels/@me/123"));

    let flagged = crate::parse_args(["--url", "fluxer://invite/rust-team"].map(String::from)).expect("--url deep link");
    assert_eq!(flagged.deep_link.as_deref(), Some("fluxer://invite/rust-team"));

    let url_sep =
        crate::parse_args(["--url", "--", "fluxer://settings"].map(String::from)).expect("--url -- separator");
    assert_eq!(url_sep.deep_link.as_deref(), Some("fluxer://settings"));

    assert!(crate::parse_args(["--url".to_owned()]).is_err());
    assert!(crate::parse_args(["--url", "https://evil.example"].map(String::from)).is_err());
    assert!(crate::parse_args(["--", "--instance"].map(String::from)).is_err());
    assert!(crate::parse_args(["fluxer://a", "fluxer://b"].map(String::from)).is_err());
    assert!(crate::parse_args(["--instance", "https://evil", "fluxer://settings"].map(String::from)).is_err());
}
