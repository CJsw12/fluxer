//! Email/password login, TOTP MFA and logout.
//!
//! Port of `fluxer_app/src/features/auth/commands/AuthenticationCommands.ts`
//! (`login`, `loginMfaTotp`) and the logout call in
//! `AuthSessionDependencies.ts`. ALTCHA captchas are solved automatically by
//! the request layer (`CaptchaInterceptor.ts`, up to 2 solves per call);
//! `LoginOutcome::CaptchaRequired` is returned only when solving fails or the
//! challenge is unusable. [`RestClient::login_with_captcha`] sends a token
//! the caller solved itself. WebAuthn/passkeys surface as
//! `RestError::Unsupported`.

use std::fmt;

use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::api::encode_uri_component;
use super::client::{Auth, RestClient};
use super::error::RestError;
use super::secret::{Secret, SecretToken, redact_json};

/// Registered WebAuthn passkey or hardware security key (`GET /users/@me/mfa/webauthn/credentials`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebAuthnCredential {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub last_used_at: Option<String>,
}

/// A successful login. `user` is the server's `UserPartial`.
#[derive(Clone)]
pub struct LoginSuccess {
    pub token: SecretToken,
    pub user_id: String,
    pub user: Value,
}

/// Masks the token and redacts e-mail/phone in `user`.
impl fmt::Debug for LoginSuccess {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoginSuccess")
            .field("token", &self.token)
            .field("user_id", &self.user_id)
            .field("user", &redact_json(&self.user))
            .finish()
    }
}

/// MFA methods offered by the server for this ticket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MfaMethods {
    /// `allowed_methods` as sent by the server.
    pub allowed: Vec<String>,
    pub totp: bool,
    pub webauthn: bool,
    pub backup_codes: bool,
}

#[derive(Debug, Clone)]
pub enum LoginOutcome {
    /// Logged in. The token is NOT stored in the client automatically; call
    /// [`RestClient::set_token`] after persisting it.
    Token(LoginSuccess),
    /// Second factor required; finish with [`RestClient::login_mfa_totp`].
    Mfa { ticket: Secret, methods: MfaMethods },
    /// HTTP 400 `CAPTCHA_REQUIRED` / `INVALID_CAPTCHA` left after the
    /// automatic ALTCHA solves (unsupported or malformed challenge, solve
    /// timeout, or still rejected after 2 solves). `challenge` is the
    /// `altcha_challenge: {parameters, signature}`; a token from
    /// [`super::solve_altcha_challenge`] or another solver can be sent with
    /// [`RestClient::login_with_captcha`].
    CaptchaRequired { code: String, provider: Option<String>, challenge: Option<Value> },
    /// HTTP 403 `IP_AUTHORIZATION_REQUIRED`: the user must approve this IP
    /// from the e-mail link. Polling `/auth/ip-authorization/poll` is not
    /// implemented yet, so the server's ticket is not surfaced.
    IpAuthorizationRequired { resend_available_in: Option<u64> },
}

impl RestClient {
    /// `POST /auth/login` with `{email, password}`. A captcha challenge is
    /// solved and the request resent transparently.
    pub async fn login(&self, email: &str, password: &Secret) -> Result<LoginOutcome, RestError> {
        self.login_inner(email, password, None).await
    }

    /// Retries `POST /auth/login` after a [`LoginOutcome::CaptchaRequired`]
    /// with the solved ALTCHA token in `X-Captcha-Token`.
    pub async fn login_with_captcha(
        &self,
        email: &str,
        password: &Secret,
        captcha_token: &Secret,
    ) -> Result<LoginOutcome, RestError> {
        if captcha_token.expose().is_empty() {
            return Err(RestError::InvalidArgument("captcha token is required"));
        }
        self.login_inner(email, password, Some(captcha_token)).await
    }

    async fn login_inner(
        &self,
        email: &str,
        password: &Secret,
        captcha_token: Option<&Secret>,
    ) -> Result<LoginOutcome, RestError> {
        if email.trim().is_empty() || password.expose().is_empty() {
            return Err(RestError::InvalidArgument("email and password are required"));
        }
        let body = json!({ "email": email, "password": password.expose() });
        let result = self
            .request_with_captcha(Method::POST, "/auth/login", &[], Some(&body), Auth::None, captcha_token)
            .await;
        interpret_login(result)
    }

    /// `POST /auth/login/mfa/totp` with `{code, ticket}`.
    pub async fn login_mfa_totp(&self, ticket: &Secret, code: &Secret) -> Result<LoginOutcome, RestError> {
        let code = code.expose().trim();
        if code.is_empty() || ticket.expose().is_empty() {
            return Err(RestError::InvalidArgument("ticket and code are required"));
        }
        let body = json!({ "code": code, "ticket": ticket.expose() });
        let result = self.request(Method::POST, "/auth/login/mfa/totp", &[], Some(&body), Auth::None).await;
        interpret_login(result)
    }

    /// `POST /auth/login/mfa/webauthn/authentication-options` with `{ticket}`.
    pub async fn webauthn_mfa_options(&self, ticket: &Secret) -> Result<Value, RestError> {
        if ticket.expose().is_empty() {
            return Err(RestError::InvalidArgument("MFA ticket is required"));
        }
        let body = json!({ "ticket": ticket.expose() });
        self.request(
            Method::POST,
            "/auth/login/mfa/webauthn/authentication-options",
            &[],
            Some(&body),
            Auth::None,
        )
        .await
    }

    /// `POST /auth/login/mfa/webauthn` with `{response, challenge, ticket}`.
    pub async fn login_mfa_webauthn(
        &self,
        ticket: &Secret,
        challenge: &str,
        response: Value,
    ) -> Result<LoginOutcome, RestError> {
        if ticket.expose().is_empty() || challenge.trim().is_empty() {
            return Err(RestError::InvalidArgument("MFA ticket and challenge are required"));
        }
        let body = json!({
            "response": response,
            "challenge": challenge,
            "ticket": ticket.expose(),
        });
        let result = self.request(Method::POST, "/auth/login/mfa/webauthn", &[], Some(&body), Auth::None).await;
        interpret_login(result)
    }

    /// `POST /auth/webauthn/authentication-options` (optional `?rp_id=...`).
    pub async fn webauthn_auth_options(&self, rp_id: Option<&str>) -> Result<Value, RestError> {
        let mut query = Vec::new();
        if let Some(rp) = rp_id.map(str::trim).filter(|s| !s.is_empty()) {
            query.push(("rp_id", rp.to_owned()));
        }
        self.request(Method::POST, "/auth/webauthn/authentication-options", &query, None, Auth::None)
            .await
    }

    /// `POST /auth/webauthn/authenticate` with `{response, challenge}`.
    pub async fn login_passkey(&self, challenge: &str, response: Value) -> Result<LoginOutcome, RestError> {
        if challenge.trim().is_empty() {
            return Err(RestError::InvalidArgument("passkey challenge is required"));
        }
        let body = json!({
            "response": response,
            "challenge": challenge,
        });
        let result = self.request(Method::POST, "/auth/webauthn/authenticate", &[], Some(&body), Auth::None).await;
        interpret_login(result)
    }

    /// `GET /users/@me/mfa/webauthn/credentials`.
    pub async fn list_webauthn_credentials(&self) -> Result<Vec<WebAuthnCredential>, RestError> {
        let value = self
            .request(Method::GET, "/users/@me/mfa/webauthn/credentials", &[], None, Auth::Required)
            .await?;
        if value.is_array() {
            serde_json::from_value(value).map_err(|err| RestError::Decode(err.to_string()))
        } else if let Some(list) = value.get("credentials").filter(|v| v.is_array()).cloned() {
            serde_json::from_value(list).map_err(|err| RestError::Decode(err.to_string()))
        } else {
            Err(RestError::Decode("unexpected WebAuthn credential list response shape".to_owned()))
        }
    }

    /// `POST /users/@me/mfa/webauthn/credentials/registration-options`.
    /// Requires sudo verification (`password` when no sudo token is active).
    pub async fn webauthn_registration_options(
        &self,
        rp_id: Option<&str>,
        sudo_password: Option<&Secret>,
    ) -> Result<Value, RestError> {
        let mut query = Vec::new();
        if let Some(rp) = rp_id.map(str::trim).filter(|s| !s.is_empty()) {
            query.push(("rp_id", rp.to_owned()));
        }
        let body = match sudo_password.filter(|p| !p.expose().is_empty()) {
            Some(password) => json!({ "password": password.expose() }),
            None => json!({}),
        };
        self.request(
            Method::POST,
            "/users/@me/mfa/webauthn/credentials/registration-options",
            &query,
            Some(&body),
            Auth::Required,
        )
        .await
    }

    /// `POST /users/@me/mfa/webauthn/credentials` with `{response, challenge, name}`.
    pub async fn register_webauthn_credential(
        &self,
        challenge: &str,
        response: Value,
        name: &str,
    ) -> Result<(), RestError> {
        let name = name.trim();
        if challenge.trim().is_empty() || name.is_empty() {
            return Err(RestError::InvalidArgument("passkey challenge and name are required"));
        }
        let body = json!({
            "response": response,
            "challenge": challenge,
            "name": name,
        });
        self.request(
            Method::POST,
            "/users/@me/mfa/webauthn/credentials",
            &[],
            Some(&body),
            Auth::Required,
        )
        .await
        .map(|_| ())
    }

    /// `DELETE /users/@me/mfa/webauthn/credentials/{credential_id}` with sudo body.
    pub async fn delete_webauthn_credential(
        &self,
        credential_id: &str,
        sudo_password: Option<&Secret>,
    ) -> Result<(), RestError> {
        let id = credential_id.trim();
        if id.is_empty() {
            return Err(RestError::InvalidArgument("credential id is required"));
        }
        let body = match sudo_password.filter(|p| !p.expose().is_empty()) {
            Some(password) => json!({ "password": password.expose() }),
            None => json!({}),
        };
        let path = format!("/users/@me/mfa/webauthn/credentials/{}", encode_uri_component(id));
        self.request(Method::DELETE, &path, &[], Some(&body), Auth::Required)
            .await
            .map(|_| ())
    }

    /// `POST /auth/logout` (204) with a 5 s timeout and no 429 retries
    /// (`AuthSessionDependencies.ts`). Clears the stored token even if the
    /// server call fails, because the caller is discarding the session, but
    /// only if it is still the token that was logged out: a token set by a
    /// newer login while the call was in flight is kept.
    pub async fn logout(&self) -> Result<(), RestError> {
        let (token, generation) = self.token_snapshot().await;
        let token = token.ok_or(RestError::NotAuthenticated)?;
        let result = self
            .request_with(Method::POST, "/auth/logout", &[], None, Auth::Required, Self::logout_options(&token))
            .await;
        self.clear_token_if_generation(generation).await;
        result.map(|_| ())
    }
}

fn interpret_login(result: Result<Value, RestError>) -> Result<LoginOutcome, RestError> {
    match result {
        Ok(body) => parse_login_body(&body),
        Err(RestError::Api(api)) => match (api.status, api.code.as_str()) {
            (400, "CAPTCHA_REQUIRED" | "INVALID_CAPTCHA") => Ok(LoginOutcome::CaptchaRequired {
                provider: api.body.get("captcha_provider").and_then(Value::as_str).map(str::to_owned),
                challenge: api.body.get("altcha_challenge").filter(|v| v.is_object()).cloned(),
                code: api.code,
            }),
            (403, "IP_AUTHORIZATION_REQUIRED") => Ok(LoginOutcome::IpAuthorizationRequired {
                resend_available_in: api.body.get("resend_available_in").and_then(Value::as_u64),
            }),
            _ => Err(RestError::Api(api)),
        },
        Err(other) => Err(other),
    }
}

/// `mfa === true` selects the MFA shape, otherwise the token shape.
fn parse_login_body(body: &Value) -> Result<LoginOutcome, RestError> {
    if body.get("mfa").and_then(Value::as_bool) == Some(true) {
        let ticket = body
            .get("ticket")
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| RestError::Decode("MFA response has no ticket".to_owned()))?;
        let flag = |key: &str| body.get(key).and_then(Value::as_bool).unwrap_or(false);
        let allowed = body
            .get("allowed_methods")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(Value::as_str).map(str::to_owned).collect())
            .unwrap_or_default();
        return Ok(LoginOutcome::Mfa {
            ticket: Secret::new(ticket),
            methods: MfaMethods {
                allowed,
                totp: flag("totp"),
                webauthn: flag("webauthn"),
                backup_codes: flag("backup_codes"),
            },
        });
    }
    let token = body
        .get("token")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| RestError::Decode("login response has no token".to_owned()))?;
    let user_id = body
        .get("user_id")
        .and_then(Value::as_str)
        .ok_or_else(|| RestError::Decode("login response has no user_id".to_owned()))?;
    Ok(LoginOutcome::Token(LoginSuccess {
        token: SecretToken::new(token),
        user_id: user_id.to_owned(),
        user: body.get("user").cloned().unwrap_or(Value::Null),
    }))
}
