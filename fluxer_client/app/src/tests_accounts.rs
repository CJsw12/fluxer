//! Headless GPUI tests for Milestone 6: multi-account switching, adding accounts,
//! removing saved inactive accounts, logout preservation, failure rollback,
//! stale resume protection, and `--token-env` hiding.

use fluxer_client_core::rest::{Endpoints, SecretToken, TransportPolicy};
use fluxer_client_core::session::{
    CachedEndpoints, DisplayInfo, InstanceRecord, NewAccount, SessionError, SessionStore,
};
use gpui_kit::{AppContext as _, Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};
use serde_json::json;

use crate::backend::Instance;
use crate::client_view::ClientView;
use crate::login::{Authenticated, LoginEvent};
use crate::root::{AppRoot, ResumeError, Screen};
use crate::tests::{draw, offline_rest, runtime};
use crate::ui::overlay::Overlay;

fn open_switcher_from_user_area(cx: &mut VisualTestContext) {
    let profile = cx.debug_bounds("user-panel-profile").expect("profile button drawn").center();
    cx.simulate_click(profile, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    let switch_btn = cx
        .debug_bounds("user-area-switch-accounts")
        .expect("switch accounts item in user area popout")
        .center();
    cx.simulate_click(switch_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
}

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let path = std::env::temp_dir().join(format!("fluxer-accounts-test-{name}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&path).expect("temp dir");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const INSTANCE_ONE_ORIGIN: &str = "https://fluxer.invalid";
const INSTANCE_ONE_API: &str = "https://fluxer.invalid/api";
const INSTANCE_TWO_ORIGIN: &str = "https://other.invalid";
const INSTANCE_TWO_API: &str = "https://other.invalid/api";

fn open_store(dir: &TempDir) -> Result<SessionStore, SessionError> {
    SessionStore::open_with(&dir.0, TransportPolicy::default())
}

fn save_account(
    store: &mut SessionStore,
    origin: &str,
    api_endpoint: &str,
    user_id: &str,
    username: &str,
    global_name: Option<&str>,
) -> String {
    store
        .save_account(NewAccount {
            user_id: user_id.to_owned(),
            instance: InstanceRecord {
                origin: origin.to_owned(),
                api_endpoint: api_endpoint.to_owned(),
                endpoints: Some(CachedEndpoints {
                    api_code_version: 1,
                    api: api_endpoint.to_owned(),
                    gateway: format!("{}/gateway", origin.replacen("https://", "wss://", 1)),
                    media: None,
                    static_cdn: None,
                    upload_relay: None,
                    invite: None,
                    webapp: None,
                    discovered_at_ms: 1,
                }),
            },
            display: DisplayInfo {
                username: username.to_owned(),
                global_name: global_name.map(str::to_owned),
                avatar: None,
            },
            token: SecretToken::new(format!("flx_saved.{user_id}-0123456789")),
        })
        .expect("save account")
}

fn offline_auth(origin: &str, api_endpoint: &str, user_id: &str, username: &str) -> Authenticated {
    let endpoints = Endpoints {
        api_code_version: 1,
        api: api_endpoint.to_owned(),
        gateway: format!("{}/gateway", origin.replacen("https://", "wss://", 1)),
        media: None,
        static_cdn: None,
        upload_relay: None,
        invite: None,
        webapp: None,
        discovery_url: format!("{}/.well-known/fluxer", origin.trim_end_matches('/')),
        document: json!({}),
    };
    Authenticated {
        instance: Instance { endpoints, rest: offline_rest() },
        token: SecretToken::new(format!("flx_fresh.{user_id}-0123456789")),
        user_id: user_id.to_owned(),
        user: json!({ "id": user_id, "username": username, "avatar": null }),
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

fn active_client(root: &Entity<AppRoot>, cx: &VisualTestContext) -> Entity<ClientView> {
    root.read_with(cx, |r, _| match &r.screen {
        Screen::Client(client) => client.clone(),
        _ => panic!("expected Screen::Client"),
    })
}

fn stored_keys(root: &Entity<AppRoot>, cx: &VisualTestContext) -> Vec<String> {
    root.read_with(cx, |r, _| {
        r.store.as_ref().expect("store").list_accounts().into_iter().map(|a| a.key).collect()
    })
}

#[gpui_kit::test]
fn switch_accounts_preserves_other_tokens_updates_active_on_success_and_ignores_stale_seq(
    cx: &mut TestAppContext,
) {
    let rt = runtime();
    let dir = TempDir::new("switch-success");
    let mut store = open_store(&dir).expect("store");
    let key_100 = save_account(&mut store, INSTANCE_ONE_ORIGIN, INSTANCE_ONE_API, "100", "alice", Some("Alice"));
    let key_200 = save_account(&mut store, INSTANCE_TWO_ORIGIN, INSTANCE_TWO_API, "200", "bob", Some("Bob"));
    drop(store);

    let (root, cx) = open_root(cx, &rt, open_store(&dir));
    root.update_in(cx, |r, window, cx| {
        r.finish_resume(Ok(offline_auth(INSTANCE_TWO_ORIGIN, INSTANCE_TWO_API, "200", "bob")), window, cx);
    });
    cx.run_until_parked();
    draw(cx);

    let client = active_client(&root, cx);
    client.read_with(cx, |v, _| {
        assert!(v.accounts_enabled);
        assert_eq!(v.saved_accounts.len(), 2);
        assert_eq!(v.saved_accounts[0].key, key_200);
        assert!(v.saved_accounts[0].is_active);
        assert_eq!(v.saved_accounts[1].key, key_100);
        assert!(!v.saved_accounts[1].is_active);
    });
    assert!(cx.debug_bounds("switch-accounts").is_none());

    // Open via user area popout and close via Escape (restores composer focus)
    open_switcher_from_user_area(cx);
    assert!(cx.debug_bounds("account-switcher-modal").is_some());
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    draw(cx);
    assert!(client.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));
    let composer_focused = cx.update(|window, cx| client.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
    assert!(composer_focused, "closing AccountSwitcher restores composer focus");

    // Open via Settings -> Switch accounts
    client.update_in(cx, |v, window, cx| v.open_settings(window, cx));
    cx.run_until_parked();
    draw(cx);
    client.update(cx, |v, cx| {
        if let Overlay::Settings(modal) = &v.overlay {
            modal.sidebar_scroll.scroll_to_bottom();
        }
        cx.notify();
    });
    cx.run_until_parked();
    draw(cx);
    let settings_switch = cx
        .debug_bounds("settings-switch-accounts")
        .expect("settings switch button drawn")
        .center();
    cx.simulate_click(settings_switch, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("account-switcher-modal").is_some());

    // Click inactive account 100: enters Checking without prematurely persisting active switch
    assert_eq!(key_100, "https://fluxer.invalid/api::100");
    let row = cx
        .debug_bounds("acct-switch-https://fluxer.invalid/api::100")
        .expect("account 100 switch row drawn")
        .center();
    cx.simulate_click(row, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    root.read_with(cx, |r, _| {
        assert!(matches!(r.screen, Screen::Checking(_)));
        assert_eq!(r.account_key.as_deref(), Some(key_100.as_str()));
        let active = r.store.as_ref().and_then(|s| s.active_account()).expect("active remains 200");
        assert_eq!(active.key, key_200);
    });

    // Duplicate/overlapping switch while Checking is ignored
    root.update_in(cx, |r, window, cx| {
        r.switch_account(key_200.clone(), window, cx);
        assert_eq!(r.account_key.as_deref(), Some(key_100.as_str()));
    });

    // Stale completion with seq=0 is ignored
    root.update_in(cx, |r, window, cx| {
        r.finish_resume_seq(0, Ok(offline_auth(INSTANCE_ONE_ORIGIN, INSTANCE_ONE_API, "100", "alice")), window, cx);
    });
    cx.run_until_parked();
    assert!(root.read_with(cx, |r, _| matches!(r.screen, Screen::Checking(_))));

    // Current completion opens 100 and commits active=key_100 while stale resave_for(key_200) is rejected
    root.update_in(cx, |r, window, cx| {
        r.finish_resume(Ok(offline_auth(INSTANCE_ONE_ORIGIN, INSTANCE_ONE_API, "100", "alice")), window, cx);
        assert!(!r.resave_for(&key_200, None, None));
    });
    cx.run_until_parked();
    draw(cx);
    root.read_with(cx, |r, _| {
        assert!(matches!(r.screen, Screen::Client(_)));
        assert_eq!(r.account_key.as_deref(), Some(key_100.as_str()));
        let store = r.store.as_ref().expect("store");
        assert_eq!(store.active_account().map(|a| a.key), Some(key_100.clone()));
        assert_eq!(store.list_accounts().len(), 2);
    });

    close_store(&root, cx);
    drop(rt);
}

#[gpui_kit::test]
fn switch_failure_recovery_unreachable_keeps_both_and_rejected_forgets_only_target(cx: &mut TestAppContext) {
    let rt = runtime();
    let dir = TempDir::new("switch-failure");
    let mut store = open_store(&dir).expect("store");
    let key_200 = save_account(&mut store, INSTANCE_TWO_ORIGIN, INSTANCE_TWO_API, "200", "bob", Some("Bob"));
    let key_100 = save_account(&mut store, INSTANCE_ONE_ORIGIN, INSTANCE_ONE_API, "100", "alice", Some("Alice"));
    drop(store);

    let (root, cx) = open_root(cx, &rt, open_store(&dir));
    root.update_in(cx, |r, window, cx| {
        r.finish_resume(Ok(offline_auth(INSTANCE_ONE_ORIGIN, INSTANCE_ONE_API, "100", "alice")), window, cx);
    });
    cx.run_until_parked();
    draw(cx);

    // Trigger switch to 200 and fail Unreachable -> Login shows both + Back to accounts
    assert_eq!(key_200, "https://other.invalid/api::200");
    open_switcher_from_user_area(cx);
    let target = cx
        .debug_bounds("acct-switch-https://other.invalid/api::200")
        .expect("200 row")
        .center();
    cx.simulate_click(target, Modifiers::default());
    cx.run_until_parked();
    root.update_in(cx, |r, window, cx| {
        r.finish_resume(Err(ResumeError::Unreachable("offline".to_owned())), window, cx);
    });
    cx.run_until_parked();
    draw(cx);
    assert_eq!(stored_keys(&root, cx).len(), 2);
    root.read_with(cx, |r, cx| {
        let Screen::Login(login) = &r.screen else { panic!("expected Screen::Login") };
        assert!(login.read(cx).can_cancel);
        assert_eq!(login.read(cx).saved_accounts.len(), 2);
        assert_eq!(login.read(cx).error.as_deref(), Some("offline"));
    });

    // Click "Back to accounts" -> resumes 100 again
    let back = cx.debug_bounds("login-cancel-add").expect("back button").center();
    cx.simulate_click(back, Modifiers::default());
    cx.run_until_parked();
    root.read_with(cx, |r, _| {
        assert!(matches!(r.screen, Screen::Checking(_)));
        assert_eq!(r.account_key.as_deref(), Some(key_100.as_str()));
    });
    root.update_in(cx, |r, window, cx| {
        r.finish_resume(Ok(offline_auth(INSTANCE_ONE_ORIGIN, INSTANCE_ONE_API, "100", "alice")), window, cx);
    });
    cx.run_until_parked();
    draw(cx);

    // Switch to 200 again and fail Rejected (401) -> forgets ONLY 200; keeps 100 usable
    open_switcher_from_user_area(cx);
    let target = cx
        .debug_bounds("acct-switch-https://other.invalid/api::200")
        .expect("200 row")
        .center();
    cx.simulate_click(target, Modifiers::default());
    cx.run_until_parked();
    root.update_in(cx, |r, window, cx| {
        r.finish_resume(Err(ResumeError::Rejected("401 UNAUTHORIZED".to_owned())), window, cx);
    });
    cx.run_until_parked();
    draw(cx);

    assert_eq!(stored_keys(&root, cx), vec![key_100.clone()]);
    root.read_with(cx, |r, cx| {
        let Screen::Login(login) = &r.screen else { panic!("expected Screen::Login") };
        assert!(login.read(cx).error.is_some());
        assert!(login.read(cx).can_cancel);
        assert_eq!(login.read(cx).saved_accounts.len(), 1);
        assert_eq!(login.read(cx).saved_accounts[0].key, key_100);
    });

    let pick_100 = cx
        .debug_bounds("login-saved-https://fluxer.invalid/api::100")
        .expect("saved 100 row")
        .center();
    cx.simulate_click(pick_100, Modifiers::default());
    cx.run_until_parked();
    root.read_with(cx, |r, _| {
        assert!(matches!(r.screen, Screen::Checking(_)));
        assert_eq!(r.account_key.as_deref(), Some(key_100.as_str()));
    });

    close_store(&root, cx);
    drop(rt);
}

#[gpui_kit::test]
fn add_account_cancel_remove_inactive_and_logout_with_remaining_accounts(cx: &mut TestAppContext) {
    let rt = runtime();
    let dir = TempDir::new("add-remove-logout");
    let mut store = open_store(&dir).expect("store");
    let key_100 = save_account(&mut store, INSTANCE_ONE_ORIGIN, INSTANCE_ONE_API, "100", "alice", Some("Alice"));
    drop(store);

    let (root, cx) = open_root(cx, &rt, open_store(&dir));
    root.update_in(cx, |r, window, cx| {
        r.finish_resume(Ok(offline_auth(INSTANCE_ONE_ORIGIN, INSTANCE_ONE_API, "100", "alice")), window, cx);
    });
    cx.run_until_parked();
    draw(cx);

    // Add account opens Login with cancel button; cancelling resumes 100
    open_switcher_from_user_area(cx);
    let add_btn = cx.debug_bounds("acct-add-btn").expect("add button").center();
    cx.simulate_click(add_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert_eq!(stored_keys(&root, cx), vec![key_100.clone()]);
    let cancel_btn = cx.debug_bounds("login-cancel-add").expect("cancel add").center();
    cx.simulate_click(cancel_btn, Modifiers::default());
    cx.run_until_parked();
    root.update_in(cx, |r, window, cx| {
        r.finish_resume(Ok(offline_auth(INSTANCE_ONE_ORIGIN, INSTANCE_ONE_API, "100", "alice")), window, cx);
    });
    cx.run_until_parked();
    draw(cx);

    // Authenticate a second account (200) -> both saved, 200 active
    open_switcher_from_user_area(cx);
    let add_btn = cx.debug_bounds("acct-add-btn").expect("add button").center();
    cx.simulate_click(add_btn, Modifiers::default());
    cx.run_until_parked();
    let login = root.read_with(cx, |r, _| match &r.screen {
        Screen::Login(login) => login.clone(),
        _ => panic!("expected Screen::Login"),
    });
    let auth_200 = offline_auth(INSTANCE_TWO_ORIGIN, INSTANCE_TWO_API, "200", "bob");
    login.update(cx, |_, cx| cx.emit(LoginEvent::Authenticated(auth_200)));
    cx.run_until_parked();
    draw(cx);
    let key_200 = root.read_with(cx, |r, _| r.account_key.clone()).expect("key_200");
    assert_eq!(stored_keys(&root, cx).len(), 2);

    // Remove inactive account 100 from AccountSwitcher (requires two-step confirmation)
    open_switcher_from_user_area(cx);
    let rm_100 = cx
        .debug_bounds("acct-remove-https://fluxer.invalid/api::100")
        .expect("remove 100 button")
        .center();
    cx.simulate_click(rm_100, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert_eq!(stored_keys(&root, cx).len(), 2, "first click only prompts confirmation");
    let rm_100_confirm = cx
        .debug_bounds("acct-remove-confirm-https://fluxer.invalid/api::100")
        .expect("confirm remove 100 button")
        .center();
    cx.simulate_click(rm_100_confirm, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert_eq!(stored_keys(&root, cx), vec![key_200.clone()]);
    let client = active_client(&root, cx);
    assert_eq!(client.read_with(cx, |v, _| v.saved_accounts.len()), 1);

    // Re-add 100 then log out from 100 -> 200 remains saved on LoginView and is clickable
    let add_btn = cx.debug_bounds("acct-add-btn").expect("add button").center();
    cx.simulate_click(add_btn, Modifiers::default());
    cx.run_until_parked();
    let login = root.read_with(cx, |r, _| match &r.screen {
        Screen::Login(login) => login.clone(),
        _ => panic!("expected Screen::Login"),
    });
    login.update(cx, |_, cx| {
        cx.emit(LoginEvent::Authenticated(offline_auth(INSTANCE_ONE_ORIGIN, INSTANCE_ONE_API, "100", "alice")));
    });
    cx.run_until_parked();
    draw(cx);
    let client = active_client(&root, cx);
    client.update(cx, |v, cx| v.log_out(cx));
    cx.run_until_parked();
    draw(cx);

    assert_eq!(stored_keys(&root, cx), vec![key_200.clone()]);
    assert_eq!(key_200, "https://other.invalid/api::200");
    let pick_200 = cx
        .debug_bounds("login-saved-https://other.invalid/api::200")
        .expect("remaining 200 clickable")
        .center();
    cx.simulate_click(pick_200, Modifiers::default());
    cx.run_until_parked();
    root.read_with(cx, |r, _| {
        assert!(matches!(r.screen, Screen::Checking(_)));
        assert_eq!(r.account_key.as_deref(), Some(key_200.as_str()));
    });

    close_store(&root, cx);
    drop(rt);
}

#[gpui_kit::test]
fn token_env_mode_hides_account_switcher(cx: &mut TestAppContext) {
    let rt = runtime();
    cx.update(gpui_kit::init);
    cx.update(crate::shortcuts::bind_keys);
    let handle = rt.handle().clone();
    let mut content = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let root = cx.new(|cx| {
            AppRoot::new_with_token(
                handle,
                false,
                "fluxer.invalid".to_owned(),
                SecretToken::new("flx_env.token-0123456789"),
                window,
                cx,
            )
        });
        content = Some(root.clone());
        gpui_kit::base::Root::new(root, window, cx)
    });
    let root = content.expect("root");
    root.update_in(cx, |r, window, cx| {
        r.start_client(offline_auth(INSTANCE_ONE_ORIGIN, INSTANCE_ONE_API, "100", "alice"), window, cx);
    });
    cx.run_until_parked();
    draw(cx);

    assert!(cx.debug_bounds("switch-accounts").is_none());
    open_switcher_from_user_area(cx);
    let client = active_client(&root, cx);
    client.update_in(cx, |v, window, cx| {
        assert!(!v.accounts_enabled);
        assert!(matches!(v.overlay, Overlay::None));
        v.open_account_switcher(window, cx);
        assert!(matches!(v.overlay, Overlay::None));
    });

    drop(rt);
}

#[gpui_kit::test]
fn start_client_failure_preserves_previous_account_and_active_store(cx: &mut TestAppContext) {
    let rt = runtime();
    let dir = TempDir::new("start-client-fail");
    let mut store = open_store(&dir).expect("store");
    save_account(&mut store, INSTANCE_TWO_ORIGIN, INSTANCE_TWO_API, "200", "bob", Some("Bob"));
    let key_100 = save_account(&mut store, INSTANCE_ONE_ORIGIN, INSTANCE_ONE_API, "100", "alice", Some("Alice"));
    drop(store);

    let (root, cx) = open_root(cx, &rt, open_store(&dir));
    root.update_in(cx, |r, window, cx| {
        r.finish_resume(Ok(offline_auth(INSTANCE_ONE_ORIGIN, INSTANCE_ONE_API, "100", "alice")), window, cx);
    });
    cx.run_until_parked();
    draw(cx);

    open_switcher_from_user_area(cx);
    let target = cx
        .debug_bounds("acct-switch-https://other.invalid/api::200")
        .expect("200 row")
        .center();
    cx.simulate_click(target, Modifiers::default());
    cx.run_until_parked();

    let mut bad_auth = offline_auth(INSTANCE_TWO_ORIGIN, INSTANCE_TWO_API, "200", "bob");
    bad_auth.instance.endpoints.gateway = "not-a-valid-url".to_owned();
    root.update_in(cx, |r, window, cx| {
        r.finish_resume(Ok(bad_auth), window, cx);
    });
    cx.run_until_parked();
    draw(cx);

    root.read_with(cx, |r, cx| {
        let Screen::Login(login) = &r.screen else { panic!("expected Screen::Login") };
        assert!(r.account_key.is_none());
        assert!(login.read(cx).can_cancel);
        assert!(login.read(cx).error.is_some());
        let active = r.store.as_ref().and_then(|s| s.active_account()).expect("active preserved");
        assert_eq!(active.key, key_100);
    });

    let back = cx.debug_bounds("login-cancel-add").expect("back button").center();
    cx.simulate_click(back, Modifiers::default());
    cx.run_until_parked();
    root.read_with(cx, |r, _| {
        assert!(matches!(r.screen, Screen::Checking(_)));
        assert_eq!(r.account_key.as_deref(), Some(key_100.as_str()));
    });

    close_store(&root, cx);
    drop(rt);
}
