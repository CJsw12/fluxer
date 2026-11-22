//! Login screen: instance, e-mail, password, then TOTP if the server asks.
//! Discovery and the REST calls run on the tokio runtime.

use fluxer_client_core::rest::{
    LoginOutcome, ReleaseChannel, RestError, Secret, SecretToken, resolve_instance_input,
};
use gpui_kit::Focusable as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::{
    Context, Entity, EventEmitter, FontWeight, SharedString, Subscription, Task, Window, div, prelude::*, px,
};
use serde_json::Value;
use tokio::runtime::Handle;

use crate::accounts::SavedAccountItem;
use crate::backend::{Instance, connect_instance};
use crate::theme::Theme;
use crate::ui::l10n;
use fluxer_client_core::state::Snowflake;
use crate::ui::avatar::{AvatarSpec, render_avatar};

pub const DEFAULT_INSTANCE: &str = "fluxer.app";

/// Login finished: the REST client already carries the token.
#[derive(Clone)]
pub struct Authenticated {
    pub instance: Instance,
    pub token: SecretToken,
    pub user_id: String,
    /// The user object (`GET /users/@me`), for the saved account's display
    /// info; `Value::Null` when it could not be fetched.
    pub user: Value,
}

pub enum LoginEvent {
    Authenticated(Authenticated),
    SelectSaved(String),
    RemoveSaved(String),
    CancelAddAccount,
}

/// Validated login form input.
#[derive(Debug, PartialEq)]
pub struct Credentials {
    pub instance: String,
    pub email: String,
    pub password: Secret,
}

pub fn validate_credentials(instance: &str, email: &str, password: &str) -> Result<Credentials, String> {
    let instance = instance.trim();
    if instance.is_empty() {
        return Err(l10n::tr_or_en("Enter an instance URL", "Enter the instance address."));
    }
    if resolve_instance_input(instance, ReleaseChannel::Stable).is_err() {
        return Err(l10n::tr_args(
            "That doesn't look like an instance address. Try a domain such as {exampleInstanceDomain}.",
            &[("exampleInstanceDomain", instance)],
        ));
    }
    let email = email.trim();
    if email.is_empty() {
        return Err(l10n::tr_or_en("Enter an email address.", "Enter your e-mail address."));
    }
    if !email.contains('@') {
        return Err(l10n::tr_or_en("Enter a valid email address.", "The e-mail address is not valid."));
    }
    if password.is_empty() {
        return Err(l10n::tr("Enter your password."));
    }
    Ok(Credentials { instance: instance.to_owned(), email: email.to_owned(), password: Secret::new(password) })
}

/// TOTP codes are 6 digits; spaces are tolerated.
pub fn validate_totp(code: &str) -> Result<Secret, String> {
    let digits: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    if digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_digit()) {
        Ok(Secret::new(digits))
    } else {
        Err(l10n::tr_args(
            "Enter the {digitCount}-digit code from your authenticator app or a backup code.",
            &[("digitCount", "6")],
        ))
    }
}

/// Result of one login request.
pub(crate) enum Next {
    Done(Authenticated),
    Totp { instance: Instance, ticket: Secret },
}

pub(crate) enum Step {
    Credentials,
    Totp { instance: Instance, ticket: Secret },
}

pub struct LoginView {
    runtime: Handle,
    allow_insecure_loopback: bool,
    pub(crate) instance_input: Entity<InputState>,
    pub(crate) email_input: Entity<InputState>,
    pub(crate) password_input: Entity<InputState>,
    pub(crate) code_input: Entity<InputState>,
    pub(crate) step: Step,
    pub(crate) busy: bool,
    pub(crate) error: Option<SharedString>,
    pub(crate) saved_accounts: Vec<SavedAccountItem>,
    pub(crate) can_cancel: bool,
    pub(crate) confirm_remove: Option<String>,
    request: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<LoginEvent> for LoginView {}

impl LoginView {
    #[cfg(test)]
    pub fn new(
        runtime: Handle,
        allow_insecure_loopback: bool,
        error: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new_with_accounts(runtime, allow_insecure_loopback, error, Vec::new(), false, window, cx)
    }

    pub fn new_with_accounts(
        runtime: Handle,
        allow_insecure_loopback: bool,
        error: Option<SharedString>,
        saved_accounts: Vec<SavedAccountItem>,
        can_cancel: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let instance_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx).placeholder(l10n::tr_args(
                "Enter self-hosted instance URL (e.g. {exampleInstanceDomain})",
                &[("exampleInstanceDomain", DEFAULT_INSTANCE)],
            ));
            s.set_value(DEFAULT_INSTANCE, window, cx);
            s
        });
        let email_input = cx.new(|cx| InputState::new(window, cx).placeholder(l10n::tr("Email")));
        let password_input = cx.new(|cx| InputState::new(window, cx).placeholder(l10n::tr("Password")).masked(true));
        let code_input = cx.new(|cx| InputState::new(window, cx).placeholder(l10n::tr("6-digit code")));
        let mut subscriptions = Vec::new();
        for input in [&instance_input, &email_input, &password_input, &code_input] {
            subscriptions.push(cx.subscribe_in(input, window, |this: &mut Self, _, ev: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    this.submit(window, cx);
                }
            }));
        }
        Self {
            runtime,
            allow_insecure_loopback,
            instance_input,
            email_input,
            password_input,
            code_input,
            step: Step::Credentials,
            busy: false,
            error,
            saved_accounts,
            can_cancel,
            confirm_remove: None,
            request: None,
            _subscriptions: subscriptions,
        }
    }

    pub(crate) fn set_saved_accounts(
        &mut self,
        saved_accounts: Vec<SavedAccountItem>,
        can_cancel: bool,
        cx: &mut Context<Self>,
    ) {
        self.saved_accounts = saved_accounts;
        self.can_cancel = can_cancel;
        if let Some(target) = &self.confirm_remove
            && !self.saved_accounts.iter().any(|a| &a.key == target)
        {
            self.confirm_remove = None;
        }
        cx.notify();
    }

    pub(crate) fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let job = match &self.step {
            Step::Credentials => {
                let creds = validate_credentials(
                    &self.instance_input.read(cx).value(),
                    &self.email_input.read(cx).value(),
                    &self.password_input.read(cx).value(),
                );
                match creds {
                    Ok(creds) => self.runtime.spawn(login_with_password(creds, self.allow_insecure_loopback)),
                    Err(message) => return self.fail(message.into(), cx),
                }
            }
            Step::Totp { instance, ticket } => match validate_totp(&self.code_input.read(cx).value()) {
                Ok(code) => self.runtime.spawn(login_with_totp(instance.clone(), ticket.clone(), code)),
                Err(message) => return self.fail(message.into(), cx),
            },
        };
        self.busy = true;
        self.error = None;
        cx.notify();
        self.request = Some(cx.spawn_in(window, async move |this, cx| {
            let result = match job.await {
                Ok(result) => result,
                Err(err) => {
                    tracing::warn!(%err, "login task failed");
                    Err(l10n::tr("Something went wrong. Try again."))
                }
            };
            let _ = this.update_in(cx, |view, window, cx| view.finish(result, window, cx));
        }));
    }

    pub(crate) fn finish(&mut self, result: Result<Next, String>, window: &mut Window, cx: &mut Context<Self>) {
        self.busy = false;
        self.request = None;
        match result {
            Ok(Next::Done(auth)) => {
                self.password_input.update(cx, |s, cx| s.set_value("", window, cx));
                cx.emit(LoginEvent::Authenticated(auth));
            }
            Ok(Next::Totp { instance, ticket }) => {
                self.step = Step::Totp { instance, ticket };
                self.code_input.update(cx, |s, cx| s.set_value("", window, cx));
                let focus = self.code_input.read(cx).focus_handle(cx);
                window.focus(&focus, cx);
            }
            Err(message) => self.error = Some(message.into()),
        }
        cx.notify();
    }

    fn fail(&mut self, message: SharedString, cx: &mut Context<Self>) {
        self.error = Some(message);
        cx.notify();
    }

    fn back(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.step = Step::Credentials;
        self.error = None;
        cx.notify();
    }
}

impl Render for LoginView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (bg, fg, muted_fg, danger, border) = {
            let theme = cx.theme();
            (theme.background, theme.foreground, theme.muted_foreground, theme.danger, theme.border)
        };
        let app_theme = Theme::current(cx);
        let busy = self.busy;
        let saved_section = (!self.saved_accounts.is_empty()).then(|| {
            let mut list = div()
                .id("login-saved-accounts")
                .max_h(px(200.))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_1();
            for (ix, item) in self.saved_accounts.iter().enumerate() {
                let key = item.key.clone();
                let select_key = key.clone();
                let select_sel = format!("login-saved-{key}");
                let confirming = self.confirm_remove.as_deref() == Some(key.as_str());
                let subtitle = if item.username.is_empty() {
                    item.instance_label.to_string()
                } else {
                    format!("@{} · {}", item.username, item.instance_label)
                };
                let trailing = if confirming {
                    let confirm_key = key.clone();
                    let confirm_sel = format!("login-remove-confirm-{key}");
                    let cancel_sel = format!("login-remove-cancel-{key}");
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .child(
                            div().debug_selector(move || confirm_sel.clone()).child(
                                Button::new(("login-remove-confirm", ix))
                                    .danger()
                                    .small()
                                    .label(l10n::tr("Remove"))
                                    .disabled(busy)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        if !this.busy {
                                            this.confirm_remove = None;
                                            cx.emit(LoginEvent::RemoveSaved(confirm_key.clone()));
                                        }
                                    })),
                            ),
                        )
                        .child(
                            div().debug_selector(move || cancel_sel.clone()).child(
                                Button::new(("login-remove-cancel", ix))
                                    .ghost()
                                    .small()
                                    .label(l10n::tr("Cancel"))
                                    .disabled(busy)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.confirm_remove = None;
                                        cx.notify();
                                    })),
                            ),
                        )
                        .into_any_element()
                } else {
                    let remove_key = key.clone();
                    let remove_sel = format!("login-remove-{key}");
                    div()
                        .debug_selector(move || remove_sel.clone())
                        .child(
                            Button::new(("login-remove", ix))
                                .ghost()
                                .small()
                                .label(l10n::tr("Remove"))
                                .disabled(busy)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    if !this.busy {
                                        this.confirm_remove = Some(remove_key.clone());
                                        cx.notify();
                                    }
                                })),
                        )
                        .into_any_element()
                };
                let row = div()
                    .id(("login-saved-row", ix))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(border)
                    .hover(|s| s.bg(app_theme.muted))
                    .child(
                        div()
                            .id(("login-saved", ix))
                            .debug_selector(move || select_sel.clone())
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .when(!busy, |d| d.cursor_pointer())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.busy {
                                    this.error = None;
                                    cx.emit(LoginEvent::SelectSaved(select_key.clone()));
                                }
                            }))
                            .child(render_avatar(
                                AvatarSpec {
                                    user_id: item.user_id.parse::<u64>().map(Snowflake).unwrap_or(Snowflake(0)),
                                    avatar_url: item.avatar_url.clone(),
                                    display_name: item.display_name.clone(),
                                    size: 28.,
                                    status: None,
                                    status_border_bg: None,
                                },
                                &app_theme,
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .truncate()
                                            .text_sm()
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(item.display_name.clone()),
                                    )
                                    .child(div().truncate().text_xs().text_color(muted_fg).child(subtitle)),
                            ),
                    )
                    .child(trailing);
                list = list.child(row);
            }
            div()
                .flex()
                .flex_col()
                .gap_2()
                .pt_2()
                .border_t_1()
                .border_color(border)
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(muted_fg)
                        .child(l10n::tr("Saved accounts")),
                )
                .child(list)
        });

        let mut form = div()
            .flex()
            .flex_col()
            .gap_3()
            .w(px(380.))
            .p_6()
            .border_1()
            .border_color(border)
            .rounded(px(8.));
        form = match &self.step {
            Step::Credentials => form
                .child(div().text_size(px(20.)).font_weight(FontWeight::BOLD).child(l10n::tr("Welcome back")))
                .child(Input::new(&self.instance_input).disabled(busy))
                .child(Input::new(&self.email_input).disabled(busy))
                .child(Input::new(&self.password_input).mask_toggle().disabled(busy))
                .child(
                    Button::new("login-submit")
                        .primary()
                        .label(l10n::tr("Sign in"))
                        .loading(busy)
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                )
                .when_some(saved_section, |f, section| f.child(section))
                .when(self.can_cancel, |f| f.child(cancel_add_button(busy, cx))),
            Step::Totp { .. } => form
                .child(div().text_size(px(20.)).font_weight(FontWeight::BOLD).child(l10n::tr("Two-factor authentication")))
                .child(div().text_color(muted_fg).child(l10n::tr("Enter the code from your authenticator app.")))
                .child(Input::new(&self.code_input).disabled(busy))
                .child(
                    Button::new("mfa-submit")
                        .primary()
                        .label(l10n::tr("Verify"))
                        .loading(busy)
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                )
                .child(
                    Button::new("mfa-back")
                        .label(l10n::tr("Back"))
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, _, cx| this.back(cx))),
                )
                .when(self.can_cancel, |f| f.child(cancel_add_button(busy, cx))),
        };
        if let Some(error) = &self.error {
            form = form.child(div().id("login-error").text_color(danger).child(error.clone()));
        }
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(bg)
            .text_color(fg)
            .text_size(px(14.))
            .image_cache(crate::ui::embed::image_cache(cx))
            .child(form)
    }
}

fn cancel_add_button(busy: bool, cx: &mut Context<LoginView>) -> impl IntoElement {
    div().debug_selector(|| "login-cancel-add".to_owned()).child(
        Button::new("login-cancel-add")
            .ghost()
            .label(l10n::tr("Back to accounts"))
            .disabled(busy)
            .on_click(cx.listener(|this, _, _, cx| {
                if !this.busy {
                    cx.emit(LoginEvent::CancelAddAccount);
                }
            })),
    )
}

async fn login_with_password(creds: Credentials, allow_insecure_loopback: bool) -> Result<Next, String> {
    let domain = creds.instance.clone();
    let instance = connect_instance(&creds.instance, allow_insecure_loopback).await.map_err(|err| {
        tracing::warn!(%err, "instance discovery failed");
        l10n::tr_args("Couldn't connect to {domain}. Try again.", &[("domain", &domain)])
    })?;
    let outcome = instance.rest.login(&creds.email, &creds.password).await.map_err(describe_rest_error)?;
    interpret(instance, outcome, Some(&creds.email)).await
}

async fn login_with_totp(instance: Instance, ticket: Secret, code: Secret) -> Result<Next, String> {
    let outcome = instance.rest.login_mfa_totp(&ticket, &code).await.map_err(describe_rest_error)?;
    match outcome {
        LoginOutcome::Mfa { .. } => Err("The server asked for another second factor.".to_owned()),
        other => interpret(instance, other, None).await,
    }
}

async fn interpret(instance: Instance, outcome: LoginOutcome, email: Option<&str>) -> Result<Next, String> {
    match outcome {
        LoginOutcome::Token(success) => {
            instance.rest.set_token(Some(success.token.clone())).await;
            // The login response may not carry the user; @me also has the
            // avatar for the saved account. Not fatal: the token is valid.
            let user = match instance.rest.get_me().await {
                Ok(me) => me,
                Err(err) => {
                    tracing::warn!(%err, "GET /users/@me after login failed");
                    success.user
                }
            };
            Ok(Next::Done(Authenticated { instance, token: success.token, user_id: success.user_id, user }))
        }
        LoginOutcome::Mfa { ticket, methods } if methods.totp => Ok(Next::Totp { instance, ticket }),
        LoginOutcome::Mfa { .. } => Err(
            "This account needs a security key or backup code, which this client does not support yet.".to_owned(),
        ),
        LoginOutcome::CaptchaRequired { .. } => Err(
            "The server requires a captcha for this login. Captcha is not supported in this client yet; \
             log in once from the web or desktop app, then try again."
                .to_owned(),
        ),
        LoginOutcome::IpAuthorizationRequired { .. } => match email.filter(|email| !email.is_empty()) {
            Some(email) => Err(l10n::tr_args(
                "Check your inbox at {emailAddress} for the authorization link.",
                &[("emailAddress", email)],
            )),
            None => Err(
                "New login location: approve it from the e-mail Fluxer sent you, then log in again.".to_owned(),
            ),
        },
    }
}

/// Server message plus field errors. `RestError` never carries secrets
/// (bodies are redacted in core), but only messages are shown anyway.
pub fn describe_rest_error(err: RestError) -> String {
    match err {
        RestError::Api(api) => {
            let mut text = api.message;
            for field in api.errors {
                text.push_str(&format!("\n{}: {}", field.path, field.message));
            }
            text
        }
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_validation() {
        assert_eq!(validate_credentials("", "a@b.c", "pw").err().as_deref(), Some("Enter the instance address."));
        assert_eq!(validate_credentials("fluxer.app", " ", "pw").err().as_deref(), Some("Enter your e-mail address."));
        assert_eq!(
            validate_credentials("fluxer.app", "nobody", "pw").err().as_deref(),
            Some("The e-mail address is not valid.")
        );
        assert_eq!(validate_credentials("fluxer.app", "a@b.c", "").err().as_deref(), Some("Enter your password."));
        let ok = validate_credentials(" fluxer.app ", " a@b.c ", "pw").expect("valid");
        assert_eq!(ok.instance, "fluxer.app");
        assert_eq!(ok.email, "a@b.c");
        assert_eq!(ok.password.expose(), "pw");
        // Secrets never show up in Debug output.
        assert!(!format!("{ok:?}").contains("\"pw\""));
    }

    #[test]
    fn totp_validation() {
        assert_eq!(validate_totp("123 456").map(|s| s.expose().to_owned()), Ok("123456".to_owned()));
        assert!(validate_totp("12345").is_err());
        assert!(validate_totp("12345a").is_err());
    }
}
