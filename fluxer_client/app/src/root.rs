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

use crate::accounts::{cached_endpoints, display_info, endpoints_from_cache, new_account, now_ms, same_instance};
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
            _subscription: None,
            _task: None,
            _rediscover: None,
        }
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
                Ok(auth) => root.start_client(auth, window, cx),
                Err(message) => root.show_login(Some(message.into()), window, cx),
            });
        });
        let mut root = Self::empty(runtime, allow_insecure_loopback);
        root.screen = Screen::Checking(l10n::tr("Checking the sign-in request…").into());
        root._task = Some(task);
        root
    }

    /// Connects with a saved account's token: cached endpoints when present
    /// (rediscovered in the background), else discovery first.
    fn resume(&mut self, account: AccountSummary, window: &mut Window, cx: &mut Context<Self>) {
        let token = match self.store.as_ref().map(|store| store.load_token(&account.key)) {
            Some(Ok(token)) => token,
            Some(Err(err)) => {
                tracing::warn!(%err, "saved token is not usable");
                self.forget(&account.key);
                self.show_login(Some("The saved login could not be read. Log in again.".into()), window, cx);
                return;
            }
            None => return self.show_login(None, window, cx),
        };
        self.account_key = Some(account.key.clone());
        let insecure = self.allow_insecure_loopback;
        let origin = account.instance.origin.clone();
        let cached = account.instance.endpoints.as_ref().map(|cache| endpoints_from_cache(cache, &origin));
        if cached.is_some() {
            self.rediscover(origin.clone(), account.instance.api_endpoint.clone(), cx);
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
            let _ = this.update_in(cx, |root, window, cx| root.finish_resume(result, window, cx));
        }));
        self.screen = Screen::Checking(l10n::tr("Signing in…").into());
        cx.notify();
    }

    pub(crate) fn finish_resume(
        &mut self,
        result: Result<Authenticated, ResumeError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(auth) => {
                if !auth.user.is_null() {
                    self.resave(Some(display_info(&auth.user)), None);
                }
                self.start_client(auth, window, cx);
            }
            Err(ResumeError::Rejected(reason)) => {
                tracing::info!(%reason, "saved token rejected");
                self._rediscover = None;
                if let Some(key) = self.account_key.take() {
                    self.forget(&key);
                }
                self.show_login(Some(l10n::tr("Session expired").into()), window, cx);
            }
            Err(ResumeError::Unreachable(message)) => {
                self._rediscover = None;
                self.account_key = None;
                self.show_login(Some(message.into()), window, cx);
            }
        }
    }

    /// Discovers the instance again and caches the endpoints for the next
    /// start. The running session keeps the endpoints it started with.
    fn rediscover(&mut self, origin: String, api_endpoint: String, cx: &mut Context<Self>) {
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
            let _ = this.update(cx, |root, _| root.resave(None, Some(cache)));
        }));
    }

    /// Re-saves the current account with new display info and/or endpoints.
    fn resave(&mut self, display: Option<DisplayInfo>, endpoints: Option<CachedEndpoints>) {
        let (Some(store), Some(key)) = (self.store.as_mut(), self.account_key.as_deref()) else { return };
        let Some(summary) = store.list_accounts().into_iter().find(|a| a.key == key) else { return };
        let token = match store.load_token(key) {
            Ok(token) => token,
            Err(err) => {
                tracing::warn!(%err, "could not re-read the saved token");
                return;
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
        if let Err(err) = store.save_account(account) {
            tracing::warn!(%err, "could not update the saved account");
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
        let runtime = self.runtime.clone();
        let insecure = self.allow_insecure_loopback;
        let login = cx.new(|cx| LoginView::new(runtime, insecure, error, window, cx));
        self._subscription = Some(cx.subscribe_in(&login, window, |this: &mut Self, _, ev: &LoginEvent, window, cx| {
            let LoginEvent::Authenticated(auth) = ev;
            this.persist(auth);
            this.start_client(auth.clone(), window, cx);
        }));
        let focus = login.read(cx).email_input.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        self.screen = Screen::Login(login);
        cx.notify();
    }

    pub(crate) fn start_client(&mut self, auth: Authenticated, window: &mut Window, cx: &mut Context<Self>) {
        match Backend::start(self.runtime.clone(), auth.instance, auth.token) {
            Ok((backend, events)) => {
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
                        let ClientEvent::LoggedOut = ev;
                        this.logged_out(window, cx);
                    }));
                self.screen = Screen::Client(client);
                cx.notify();
            }
            Err(err) => {
                tracing::warn!(%err, "client start failed");
                self.show_login(Some(l10n::tr("Connection failed").into()), window, cx)
            }
        }
    }

    /// The client view logged out (REST logout already sent): forget the
    /// saved account and show the login form.
    fn logged_out(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self._rediscover = None;
        if let Some(key) = self.account_key.take() {
            self.forget(&key);
        }
        self.show_login(None, window, cx);
    }
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
        Ok(me) => me,
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
        let centered = || div().size_full().flex().items_center().justify_center().bg(cx.theme().background);
        match &self.screen {
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
        }
    }
}
