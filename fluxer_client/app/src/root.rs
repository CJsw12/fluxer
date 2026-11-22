//! Window content: login screen, sign-in check, the client layout, or the
//! "already running" notice. Owns the session store: a login is saved, a
//! saved account is resumed on startup and forgotten on logout or when the
//! server rejects its token.

use fluxer_client_core::rest::{Endpoints, SecretToken};
use fluxer_client_core::session::{
    AccountSummary, CachedEndpoints, DisplayInfo, NewAccount, SessionError, SessionStore,
};
use gpui_kit::Focusable as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::{Context, Entity, FontWeight, SharedString, Subscription, Task, Window, div, prelude::*, px};
use serde_json::Value;
use tokio::runtime::Handle;

use crate::accounts::{
    SavedAccountItem, cached_endpoints, display_info, endpoints_from_cache, new_account, now_ms, same_instance,
    saved_account_items,
};
use crate::backend::{Backend, connect_instance, instance_from_endpoints};
use crate::client_view::{ClientEvent, ClientView};
use crate::frecency::{self, FrecencyStore};
use crate::login::{Authenticated, LoginEvent, LoginView, describe_rest_error};
use crate::prefs::{self, PrefsStore};
use crate::ui::l10n;

pub(crate) enum Screen {
    Login(Entity<LoginView>),
    /// Checking a token (`--token-env` or a saved account).
    Checking(SharedString),
    Client(Entity<ClientView>),
    /// Another instance holds the session store.
    AlreadyRunning,
}

/// Why a saved account could not be resumed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ResumeError {
    /// The server rejected the token (HTTP 401): the account is forgotten.
    Rejected(String),
    /// The instance could not be reached; the account is kept.
    Unreachable(String),
}

pub struct AppRoot {
    runtime: Handle,
    allow_insecure_loopback: bool,
    pub(crate) screen: Screen,
    /// `None` with `--token-env` (never persists) or when the store could
    /// not be opened.
    pub(crate) store: Option<SessionStore>,
    /// Stored account of the current session.
    pub(crate) account_key: Option<String>,
    /// Previously active account while adding/switching so failure or cancel can return to it.
    previous_account_key: Option<String>,
    /// Monotonic transition sequence ignoring stale async resume completions.
    pub(crate) transition_seq: u64,
    /// Cached rediscovery target deferred until `finish_resume_seq(Ok)` succeeds.
    pending_rediscover: Option<(String, String, String)>,
    /// Protocol deep link queued before a client session is active.
    pub(crate) pending_deep_link: Option<crate::deep_links::DeepLink>,
    /// Suppresses duplicate deep-link launches within 1.5s.
    deep_link_deduper: crate::deep_links::DeepLinkDeduper,
    _subscription: Option<Subscription>,
    _task: Option<Task<()>>,
    /// Background rediscovery that refreshes the cached endpoints.
    _rediscover: Option<Task<()>>,
}

impl AppRoot {
    fn empty(runtime: Handle, allow_insecure_loopback: bool) -> Self {
        Self {
            runtime,
            allow_insecure_loopback,
            screen: Screen::Checking(SharedString::default()),
            store: None,
            account_key: None,
            previous_account_key: None,
            transition_seq: 0,
            pending_rediscover: None,
            pending_deep_link: None,
            deep_link_deduper: crate::deep_links::DeepLinkDeduper::default(),
            _subscription: None,
            _task: None,
            _rediscover: None,
        }
    }

    fn next_transition_seq(&mut self) -> u64 {
        self.transition_seq = self.transition_seq.saturating_add(1);
        self._task = None;
        self._rediscover = None;
        self.pending_rediscover = None;
        self.transition_seq
    }

    fn close_active_client(&self, cx: &mut Context<Self>) {
        if let Screen::Client(client) = &self.screen {
            client.update(cx, |v, _| v.close_session());
        }
    }

    fn saved_account_items(&self) -> Vec<SavedAccountItem> {
        self.store.as_ref().map_or_else(Vec::new, |store| {
            saved_account_items(&store.list_accounts(), self.account_key.as_deref())
        })
    }

    fn sync_client_accounts(&self, client: &Entity<ClientView>, cx: &mut Context<Self>) {
        let enabled = self.store.is_some();
        let items = self.saved_account_items();
        client.update(cx, |v, cx| v.set_saved_accounts(enabled, items, cx));
    }

    fn sync_active_client_accounts(&self, cx: &mut Context<Self>) {
        if let Screen::Client(client) = &self.screen {
            self.sync_client_accounts(client, cx);
        }
    }

    fn has_saved_account(&self, key: &str) -> bool {
        self.store
            .as_ref()
            .is_some_and(|s| s.list_accounts().iter().any(|a| a.key == key))
    }

    /// Normal startup: resume the active saved account, else show the login
    /// form. A locked store means another instance is running.
    pub fn new_session(
        runtime: Handle,
        allow_insecure_loopback: bool,
        store: Result<SessionStore, SessionError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut root = Self::empty(runtime, allow_insecure_loopback);
        match store {
            Ok(store) => {
                for issue in store.issues() {
                    tracing::warn!(?issue, "session store issue");
                }
                let active = store.active_account();
                root.store = Some(store);
                match active {
                    Some(account) => root.resume(account, window, cx),
                    None => root.show_login(None, window, cx),
                }
            }
            Err(SessionError::Locked { .. }) => root.screen = Screen::AlreadyRunning,
            Err(err) => {
                tracing::warn!(%err, "session store unavailable");
                let message = l10n::tr_args("{appName} cannot open its local data", &[("appName", "Fluxer")]);
                root.show_login(Some(message.into()), window, cx);
            }
        }
        root
    }

    /// `--token-env`: discover the instance, set the token and check it with
    /// `GET /users/@me` before connecting the gateway. Nothing is saved.
    pub fn new_with_token(
        runtime: Handle,
        allow_insecure_loopback: bool,
        instance: String,
        token: SecretToken,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let job = runtime.spawn(async move {
            let domain = instance.clone();
            let instance = connect_instance(&instance, allow_insecure_loopback).await.map_err(|err| {
                tracing::warn!(%err, "instance discovery failed");
                l10n::tr_args("Couldn't connect to {domain}. Try again.", &[("domain", &domain)])
            })?;
            instance.rest.set_token(Some(token.clone())).await;
            let user = instance.rest.get_me().await.map_err(|err| {
                let detail = describe_rest_error(err);
                tracing::warn!(%detail, "token check rejected");
                l10n::tr("Couldn't complete sign-in. Try again.")
            })?;
            let user_id = user.get("id").and_then(Value::as_str).unwrap_or_default().to_owned();
            Ok::<_, String>(Authenticated { instance, token, user_id, user })
        });
        let task = cx.spawn_in(window, async move |this, cx| {
            let result = match job.await {
                Ok(result) => result,
                Err(err) => {
                    tracing::warn!(%err, "token check failed");
                    Err(l10n::tr("Something went wrong. Try again."))
                }
            };
            let _ = this.update_in(cx, |root, window, cx| match result {
                Ok(auth) => {
                    root.start_client(auth, window, cx);
                }
                Err(message) => root.show_login(Some(message.into()), window, cx),
            });
        });
        let mut root = Self::empty(runtime, allow_insecure_loopback);
        root.screen = Screen::Checking(l10n::tr("Checking the sign-in request…").into());
        root._task = Some(task);
        root
    }

    /// Connects with a saved account's token: cached endpoints when present
    /// (rediscovered after resume succeeds), else discovery first.
    fn resume(&mut self, account: AccountSummary, window: &mut Window, cx: &mut Context<Self>) {
        let seq = self.next_transition_seq();
        let token = match self.store.as_ref().map(|store| store.load_token(&account.key)) {
            Some(Ok(token)) => token,
            Some(Err(err)) => {
                tracing::warn!(%err, "saved token is not usable");
                self.forget(&account.key);
                self.account_key = None;
                self.show_login(Some("The saved login could not be read. Log in again.".into()), window, cx);
                return;
            }
            None => {
                self.account_key = None;
                return self.show_login(None, window, cx);
            }
        };
        self.account_key = Some(account.key.clone());
        let insecure = self.allow_insecure_loopback;
        let origin = account.instance.origin.clone();
        let cached = account.instance.endpoints.as_ref().map(|cache| endpoints_from_cache(cache, &origin));
        if cached.is_some() {
            self.pending_rediscover = Some((account.key.clone(), origin.clone(), account.instance.api_endpoint.clone()));
        }
        let job = self.runtime.spawn(resume_session(origin, cached, token, account.user_id, insecure));
        self._task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = match job.await {
                Ok(result) => result,
                Err(err) => {
                    tracing::warn!(%err, "sign-in failed");
                    Err(ResumeError::Unreachable(l10n::tr("Couldn't complete sign-in. Try again.")))
                }
            };
            let _ = this.update_in(cx, |root, window, cx| root.finish_resume_seq(seq, result, window, cx));
        }));
        self.screen = Screen::Checking(l10n::tr("Signing in…").into());
        cx.notify();
    }

    #[cfg(test)]
    pub(crate) fn finish_resume(
        &mut self,
        result: Result<Authenticated, ResumeError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.finish_resume_seq(self.transition_seq, result, window, cx);
    }

    pub(crate) fn finish_resume_seq(
        &mut self,
        seq: u64,
        result: Result<Authenticated, ResumeError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if seq != self.transition_seq {
            return;
        }
        match result {
            Ok(auth) => {
                let rediscover_target = self.pending_rediscover.take();
                let user = auth.user.clone();
                let key = self.account_key.clone();
                if !self.start_client(auth, window, cx) {
                    return;
                }
                if let Some(key) = key {
                    let saved = !user.is_null() && self.resave_for(&key, Some(display_info(&user)), None);
                    if !saved
                        && let Some(store) = self.store.as_mut()
                        && let Err(err) = store.set_active(&key)
                    {
                        tracing::warn!(%err, "could not mark account active");
                    }
                }
                self.sync_active_client_accounts(cx);
                if let Some((rkey, origin, api_endpoint)) = rediscover_target {
                    self.rediscover(rkey, origin, api_endpoint, cx);
                }
            }
            Err(ResumeError::Rejected(reason)) => {
                tracing::info!(%reason, "saved token rejected");
                self.pending_rediscover = None;
                self._rediscover = None;
                self.pending_deep_link = None;
                if let Some(key) = self.account_key.take() {
                    self.forget(&key);
                }
                if self.previous_account_key.as_deref().is_some_and(|k| !self.has_saved_account(k)) {
                    self.previous_account_key = None;
                }
                self.show_login(Some(l10n::tr("Session expired").into()), window, cx);
            }
            Err(ResumeError::Unreachable(message)) => {
                self.pending_rediscover = None;
                self._rediscover = None;
                self.pending_deep_link = None;
                self.account_key = None;
                self.show_login(Some(message.into()), window, cx);
            }
        }
    }

    /// Discovers the instance again and caches the endpoints for the next
    /// start. The running session keeps the endpoints it started with.
    fn rediscover(
        &mut self,
        expected_key: String,
        origin: String,
        api_endpoint: String,
        cx: &mut Context<Self>,
    ) {
        let insecure = self.allow_insecure_loopback;
        let job = self.runtime.spawn(async move { connect_instance(&origin, insecure).await.map(|i| i.endpoints) });
        self._rediscover = Some(cx.spawn(async move |this, cx| {
            let endpoints: Endpoints = match job.await {
                Ok(Ok(endpoints)) => endpoints,
                Ok(Err(err)) => {
                    tracing::warn!(error = %format!("{err:#}"), "rediscovery failed; keeping the cached endpoints");
                    return;
                }
                Err(err) => {
                    tracing::warn!(%err, "rediscovery task failed");
                    return;
                }
            };
            if !same_instance(&endpoints.api, &api_endpoint) {
                tracing::warn!("the instance now declares another API endpoint; cached endpoints kept");
                return;
            }
            let cache = cached_endpoints(&endpoints, now_ms());
            let _ = this.update(cx, |root, _| root.resave_for(&expected_key, None, Some(cache)));
        }));
    }

    /// Re-saves `expected_key` only if it is still the active account.
    pub(crate) fn resave_for(
        &mut self,
        expected_key: &str,
        display: Option<DisplayInfo>,
        endpoints: Option<CachedEndpoints>,
    ) -> bool {
        if self.account_key.as_deref() != Some(expected_key) {
            return false;
        }
        let Some(store) = self.store.as_mut() else { return false };
        let Some(summary) = store.list_accounts().into_iter().find(|a| a.key == expected_key) else {
            return false;
        };
        let token = match store.load_token(expected_key) {
            Ok(token) => token,
            Err(err) => {
                tracing::warn!(%err, "could not re-read the saved token");
                return false;
            }
        };
        let mut instance = summary.instance;
        if endpoints.is_some() {
            instance.endpoints = endpoints;
        }
        let account = NewAccount {
            user_id: summary.user_id,
            instance,
            display: display.unwrap_or(summary.display),
            token,
        };
        match store.save_account(account) {
            Ok(_) => true,
            Err(err) => {
                tracing::warn!(%err, "could not update the saved account");
                false
            }
        }
    }

    /// Saves a fresh login as the active account.
    fn persist(&mut self, auth: &Authenticated) {
        let Some(store) = self.store.as_mut() else { return };
        let Some(account) = new_account(auth) else {
            tracing::warn!("login not saved: the API endpoint has no origin");
            return;
        };
        match store.save_account(account) {
            Ok(key) => self.account_key = Some(key),
            Err(err) => tracing::warn!(%err, "login not saved"),
        }
    }

    fn forget(&mut self, key: &str) {
        let Some(store) = self.store.as_mut() else { return };
        if let Err(err) = store.remove_account(key) {
            tracing::warn!(%err, "could not remove the saved account");
        }
    }

    fn show_login(&mut self, error: Option<SharedString>, window: &mut Window, cx: &mut Context<Self>) {
        let _ = self.next_transition_seq();
        self.close_active_client(cx);
        self.account_key = None;
        let runtime = self.runtime.clone();
        let insecure = self.allow_insecure_loopback;
        let saved = self.saved_account_items();
        let can_cancel = self.previous_account_key.as_deref().is_some_and(|k| self.has_saved_account(k));
        let login = cx.new(|cx| LoginView::new_with_accounts(runtime, insecure, error, saved, can_cancel, window, cx));
        self._subscription = Some(cx.subscribe_in(&login, window, |this: &mut Self, _, ev: &LoginEvent, window, cx| {
            match ev {
                LoginEvent::Authenticated(auth) => {
                    if this.start_client(auth.clone(), window, cx) {
                        this.persist(auth);
                        this.sync_active_client_accounts(cx);
                    }
                }
                LoginEvent::SelectSaved(key) => this.switch_account(key.clone(), window, cx),
                LoginEvent::RemoveSaved(key) => this.remove_saved_account(key.clone(), window, cx),
                LoginEvent::CancelAddAccount => this.cancel_add_account(window, cx),
            }
        }));
        let focus = login.read(cx).email_input.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        self.screen = Screen::Login(login);
        cx.notify();
    }

    pub(crate) fn start_client(&mut self, auth: Authenticated, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match Backend::start(self.runtime.clone(), auth.instance, auth.token) {
            Ok((backend, events)) => {
                self.transition_seq = self.transition_seq.saturating_add(1);
                self._task = None;
                self.pending_rediscover = None;
                self.previous_account_key = None;
                let client = cx.new(|cx| ClientView::new(backend, events, window, cx));
                // Emoji usage is kept next to the session store (never with
                // `--token-env`, which does not open the store).
                if let Some(dir) = self.store.as_ref().map(|s| s.dir().to_path_buf()) {
                    let store = FrecencyStore::open(dir.join(frecency::FILE_NAME), now_ms());
                    let prefs = PrefsStore::open(dir.join(prefs::FILE_NAME));
                    client.update(cx, |v, cx| {
                        v.frecency = store;
                        v.apply_prefs(prefs, window, cx);
                    });
                }
                let focus = client.read(cx).composer.read(cx).focus_handle(cx);
                window.focus(&focus, cx);
                self._subscription =
                    Some(cx.subscribe_in(&client, window, |this: &mut Self, _, ev: &ClientEvent, window, cx| {
                        match ev {
                            ClientEvent::LoggedOut => this.logged_out(window, cx),
                            ClientEvent::SwitchAccount(key) => this.switch_account(key.clone(), window, cx),
                            ClientEvent::AddAccount => this.open_add_account(window, cx),
                            ClientEvent::RemoveAccount(key) => this.remove_saved_account(key.clone(), window, cx),
                        }
                    }));
                self.sync_client_accounts(&client, cx);
                let pending = self.pending_deep_link.take();
                self.screen = Screen::Client(client.clone());
                if let Some(link) = pending {
                    client.update(cx, |v, cx| v.open_deep_link(link, window, cx));
                }
                cx.notify();
                true
            }
            Err(err) => {
                tracing::warn!(%err, "client start failed");
                self.show_login(Some(l10n::tr("Connection failed").into()), window, cx);
                false
            }
        }
    }

    /// Switches to `key` without revoking the previous session's token.
    pub(crate) fn switch_account(&mut self, key: String, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.screen, Screen::Checking(_)) {
            return;
        }
        if self.account_key.as_deref() == Some(key.as_str()) && matches!(self.screen, Screen::Client(_)) {
            return;
        }
        let account = self
            .store
            .as_ref()
            .and_then(|store| store.list_accounts().into_iter().find(|a| a.key == key));
        let Some(account) = account else {
            if let Screen::Client(client) = &self.screen {
                let client = client.clone();
                self.sync_client_accounts(&client, cx);
            } else if let Screen::Login(login) = &self.screen {
                let items = self.saved_account_items();
                let can_cancel = self.previous_account_key.as_deref().is_some_and(|k| self.has_saved_account(k));
                login.update(cx, |v, cx| v.set_saved_accounts(items, can_cancel, cx));
            }
            return;
        };
        let prev = if matches!(self.screen, Screen::Client(_)) {
            self.account_key.clone()
        } else {
            self.previous_account_key.clone()
        };
        self.close_active_client(cx);
        if self.account_key.as_deref() != Some(key.as_str()) {
            self.pending_deep_link = None;
        }
        self.previous_account_key = prev.filter(|k| k != &key);
        self.resume(account, window, cx);
    }

    /// Opens the login screen to sign in to an additional account while preserving saved sessions.
    pub(crate) fn open_add_account(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.store.is_none() {
            return;
        }
        let prev = self.account_key.clone();
        self.close_active_client(cx);
        self.previous_account_key = prev;
        self.account_key = None;
        self.pending_deep_link = None;
        self.show_login(None, window, cx);
    }

    /// Removes an inactive saved account from local storage without touching the server.
    pub(crate) fn remove_saved_account(&mut self, key: String, _window: &mut Window, cx: &mut Context<Self>) {
        if self.account_key.as_deref() == Some(key.as_str()) && matches!(self.screen, Screen::Client(_)) {
            return;
        }
        self.forget(&key);
        if self.previous_account_key.as_deref() == Some(key.as_str()) {
            self.previous_account_key = None;
        }
        if let Screen::Client(client) = &self.screen {
            let client = client.clone();
            self.sync_client_accounts(&client, cx);
        } else if let Screen::Login(login) = &self.screen {
            let items = self.saved_account_items();
            let can_cancel = self.previous_account_key.as_deref().is_some_and(|k| self.has_saved_account(k));
            login.update(cx, |v, cx| v.set_saved_accounts(items, can_cancel, cx));
        }
    }

    /// Cancels "Add account" / failed switch and returns to the previously active saved account.
    pub(crate) fn cancel_add_account(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.screen, Screen::Checking(_)) {
            return;
        }
        if let Some(prev) = self.previous_account_key.take()
            && self.has_saved_account(&prev)
        {
            self.switch_account(prev, window, cx);
        }
    }

    /// Handles an incoming `fluxer://` deep link for the foreground session,
    /// queuing it until login/READY finishes if no client is active yet.
    pub(crate) fn handle_deep_link(&mut self, raw: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.deep_link_deduper.should_suppress(raw) {
            return;
        }
        let Some(link) = crate::deep_links::parse_protocol_deep_link(raw) else {
            tracing::warn!("ignoring invalid or disallowed fluxer:// deep link");
            return;
        };
        if let Screen::Client(client) = &self.screen {
            let client = client.clone();
            client.update(cx, |v, cx| v.open_deep_link(link, window, cx));
        } else {
            self.pending_deep_link = Some(link);
        }
    }

    /// The client view logged out (REST logout already sent): forget the
    /// saved account and show the login form.
    fn logged_out(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.previous_account_key = None;
        self._rediscover = None;
        self.pending_rediscover = None;
        self.pending_deep_link = None;
        if let Some(key) = self.account_key.take() {
            self.forget(&key);
        }
        self.show_login(None, window, cx);
    }
}

fn verify_me_user_id(expected_user_id: &str, me: &Value) -> Result<(), ResumeError> {
    if let Some(actual) = me.get("id").and_then(Value::as_str)
        && actual != expected_user_id
    {
        return Err(ResumeError::Rejected("saved token user id mismatch".to_owned()));
    }
    Ok(())
}

/// Builds the REST client for a saved account and checks its token.
/// Only a 401 rejects the account; other failures (offline, 5xx) still
/// start the client, whose gateway retries.
async fn resume_session(
    origin: String,
    cached: Option<Endpoints>,
    token: SecretToken,
    user_id: String,
    allow_insecure_loopback: bool,
) -> Result<Authenticated, ResumeError> {
    let unreachable = |err: anyhow::Error| {
        tracing::warn!(%err, "saved instance unreachable");
        ResumeError::Unreachable(l10n::tr_args(
            "Couldn't connect to {domain}. Try again.",
            &[("domain", &origin)],
        ))
    };
    let instance = match cached {
        Some(endpoints) => instance_from_endpoints(endpoints, allow_insecure_loopback).map_err(unreachable)?,
        None => connect_instance(&origin, allow_insecure_loopback).await.map_err(unreachable)?,
    };
    instance.rest.set_token(Some(token.clone())).await;
    let user = match instance.rest.get_me().await {
        Ok(me) => {
            verify_me_user_id(&user_id, &me)?;
            me
        }
        Err(err) if err.status() == Some(401) => return Err(ResumeError::Rejected(describe_rest_error(err))),
        Err(err) => {
            tracing::warn!(%err, "could not check the saved token; connecting anyway");
            Value::Null
        }
    };
    Ok(Authenticated { instance, token, user_id, user })
}

impl Render for AppRoot {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let app_theme = crate::theme::Theme::current(cx);
        let title = match &self.screen {
            Screen::Client(view) => {
                let t = view.read(cx).window_title.clone();
                if t.is_empty() {
                    SharedString::new_static("Fluxer")
                } else {
                    t.into()
                }
            }
            _ => SharedString::new_static("Fluxer"),
        };
        let centered = || div().size_full().flex().items_center().justify_center().bg(cx.theme().background);
        let body = match &self.screen {
            Screen::Login(view) => view.clone().into_any_element(),
            Screen::Client(view) => view.clone().into_any_element(),
            Screen::Checking(text) => {
                centered().text_color(cx.theme().muted_foreground).child(text.clone()).into_any_element()
            }
            Screen::AlreadyRunning => centered()
                .text_color(cx.theme().foreground)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .text_size(px(20.))
                                .font_weight(FontWeight::BOLD)
                                .child(l10n::tr_args("{appName} cannot open its local data", &[("appName", "Fluxer")])),
                        )
                        .child(
                            div().text_color(cx.theme().muted_foreground).child(l10n::tr_args(
                                "Quit {appName}, make sure no other copy is running, and open it again.",
                                &[("appName", "Fluxer")],
                            )),
                        )
                        .child(
                            Button::new("already-running-quit")
                                .primary()
                                .label(l10n::tr("Quit"))
                                .on_click(|_, _, cx| cx.quit()),
                        ),
                )
                .into_any_element(),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(app_theme.background)
            .child(crate::ui::titlebar::render(title, &app_theme))
            .child(div().flex_1().min_h_0().w_full().child(body))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn verify_me_user_id_rejects_mismatched_subject() {
        assert_eq!(verify_me_user_id("100", &json!({ "id": "100" })), Ok(()));
        assert_eq!(verify_me_user_id("100", &json!({ "username": "me" })), Ok(()));
        assert!(matches!(
            verify_me_user_id("100", &json!({ "id": "999" })),
            Err(ResumeError::Rejected(_))
        ));
    }
}
