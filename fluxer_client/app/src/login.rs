//! Login screen: instance, e-mail, password, then TOTP if the server asks.
//! Discovery and the REST calls run on the tokio runtime.

use fluxer_client_core::rest::{
    LoginOutcome, ReleaseChannel, RestError, Secret, SecretToken, resolve_instance_input,
};
use gpui_kit::Focusable as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::Disableable as _;
use gpui_kit::{
    Context, Entity, EventEmitter, FontWeight, SharedString, Subscription, Task, Window, div, prelude::*, px,
};
use serde_json::Value;
use tokio::runtime::Handle;

use crate::backend::{Instance, connect_instance};

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
}

/// Validated login form input.
#[derive(Debug, PartialEq)]
pub struct Credentials {
    pub instance: String,
    pub email: String,
    pub password: Secret,
}

pub fn validate_credentials(instance: &str, email: &str, password: &str) -> Result<Credentials, &'static str> {
    let instance = instance.trim();
    if instance.is_empty() {
        return Err("Enter the instance address.");
    }
    if resolve_instance_input(instance, ReleaseChannel::Stable).is_err() {
        return Err("The instance address is not valid.");
    }
    let email = email.trim();
    if email.is_empty() {
        return Err("Enter your e-mail address.");
    }
    if !email.contains('@') {
        return Err("The e-mail address is not valid.");
    }
    if password.is_empty() {
        return Err("Enter your password.");
    }
    Ok(Credentials { instance: instance.to_owned(), email: email.to_owned(), password: Secret::new(password) })
}

/// TOTP codes are 6 digits; spaces are tolerated.
pub fn validate_totp(code: &str) -> Result<Secret, &'static str> {
    let digits: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    if digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_digit()) {
        Ok(Secret::new(digits))
    } else {
        Err("Enter the 6-digit code from your authenticator app.")
    }
}

/// Result of one login request.
enum Next {
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
    request: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<LoginEvent> for LoginView {}

impl LoginView {
    pub fn new(
        runtime: Handle,
        allow_insecure_loopback: bool,
        error: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let instance_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx).placeholder("Instance (e.g. fluxer.app)");
            s.set_value(DEFAULT_INSTANCE, window, cx);
            s
        });
        let email_input = cx.new(|cx| InputState::new(window, cx).placeholder("E-mail"));
        let password_input = cx.new(|cx| InputState::new(window, cx).placeholder("Password").masked(true));
        let code_input = cx.new(|cx| InputState::new(window, cx).placeholder("6-digit code"));
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
            request: None,
            _subscriptions: subscriptions,
        }
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
                Err(err) => Err(format!("Login task failed: {err}")),
            };
            let _ = this.update_in(cx, |view, window, cx| view.finish(result, window, cx));
        }));
    }

    fn finish(&mut self, result: Result<Next, String>, window: &mut Window, cx: &mut Context<Self>) {
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
        let theme = cx.theme();
        let (bg, fg, muted_fg, danger, border) =
            (theme.background, theme.foreground, theme.muted_foreground, theme.danger, theme.border);
        let busy = self.busy;
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
                .child(div().text_size(px(20.)).font_weight(FontWeight::BOLD).child("Log in to Fluxer"))
                .child(Input::new(&self.instance_input).disabled(busy))
                .child(Input::new(&self.email_input).disabled(busy))
                .child(Input::new(&self.password_input).mask_toggle().disabled(busy))
                .child(
                    Button::new("login-submit")
                        .primary()
                        .label("Log in")
                        .loading(busy)
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                ),
            Step::Totp { .. } => form
                .child(div().text_size(px(20.)).font_weight(FontWeight::BOLD).child("Two-factor authentication"))
                .child(div().text_color(muted_fg).child("Enter the code from your authenticator app."))
                .child(Input::new(&self.code_input).disabled(busy))
                .child(
                    Button::new("mfa-submit")
                        .primary()
                        .label("Verify")
                        .loading(busy)
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                )
                .child(
                    Button::new("mfa-back")
                        .label("Back")
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, _, cx| this.back(cx))),
                ),
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
            .child(form)
    }
}

async fn login_with_password(creds: Credentials, allow_insecure_loopback: bool) -> Result<Next, String> {
    let instance = connect_instance(&creds.instance, allow_insecure_loopback)
        .await
        .map_err(|err| format!("Could not reach the instance: {err:#}"))?;
    let outcome = instance.rest.login(&creds.email, &creds.password).await.map_err(describe_rest_error)?;
    interpret(instance, outcome).await
}

async fn login_with_totp(instance: Instance, ticket: Secret, code: Secret) -> Result<Next, String> {
    let outcome = instance.rest.login_mfa_totp(&ticket, &code).await.map_err(describe_rest_error)?;
    match outcome {
        LoginOutcome::Mfa { .. } => Err("The server asked for another second factor.".to_owned()),
        other => interpret(instance, other).await,
    }
}

async fn interpret(instance: Instance, outcome: LoginOutcome) -> Result<Next, String> {
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
        LoginOutcome::IpAuthorizationRequired { .. } => Err(
            "New login location: approve it from the e-mail Fluxer sent you, then log in again.".to_owned(),
        ),
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
        assert_eq!(validate_credentials("", "a@b.c", "pw").err(), Some("Enter the instance address."));
        assert_eq!(validate_credentials("fluxer.app", " ", "pw").err(), Some("Enter your e-mail address."));
        assert_eq!(validate_credentials("fluxer.app", "nobody", "pw").err(), Some("The e-mail address is not valid."));
        assert_eq!(validate_credentials("fluxer.app", "a@b.c", "").err(), Some("Enter your password."));
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
