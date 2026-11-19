//! HTTP layer: URL building, Fluxer headers, error bodies, 429 handling,
//! automatic ALTCHA captcha retries, the request pacer and the in-flight cap.
//!
//! Port of `fluxer_app/src/features/platform/transport/RestTransport.ts`
//! (URL `<api>/v<N><path>`, `assembleHeaders`, `reactToRateLimit`).

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use reqwest::header::{self, HeaderMap, HeaderValue};
use reqwest::{Method, StatusCode};
use serde_json::Value;
use tokio::sync::{Mutex, RwLock, Semaphore};
use tokio::time::Instant;
use url::Url;

use super::altcha::{MAX_CAPTCHA_SOLVES, read_altcha_challenge, solve_off_executor};
use super::discovery::{Endpoints, ReleaseChannel, TransportPolicy, has_userinfo, redact_userinfo};
use super::error::{ApiError, FieldError, RestError};
use super::secret::{Secret, SecretToken, redact_json};
use super::upload::MultipartUpload;

/// `X-Fluxer-Features` sent by the web client.
const FEATURES: &str = "view_channel_members_permission,channel_threads";
/// `X-Fluxer-Capabilities` sent by the web client.
const CAPABILITIES: &str = "guild_visits";
const MAX_REDIRECTS: usize = 5;
const PACER_WINDOW: Duration = Duration::from_secs(1);
/// Server-side global limit (`RateLimitMiddleware.ts`): a leaky bucket of
/// capacity 50 that leaks 50/s, keyed per user across all of that user's
/// sessions. Overflowing it makes the server revoke the session token.
pub const SERVER_GLOBAL_LIMIT_PER_SECOND: u32 = 50;
/// Highest accepted [`RestConfig::max_requests_per_second`].
///
/// The pacer only bounds when *this client* starts requests. It cannot see
/// other sessions of the same user (which share the server bucket), and a
/// network stall can make paced requests arrive at the server together, so
/// the cap stays well under the server limit.
pub const MAX_REQUESTS_PER_SECOND: u32 = 40;
/// Default [`RestConfig::max_requests_per_second`].
pub const DEFAULT_REQUESTS_PER_SECOND: u32 = 25;
/// At most this many requests of one client are in flight at once, so a
/// stalled connection can release at most this many requests in a burst.
pub const MAX_IN_FLIGHT_REQUESTS: usize = 8;
/// Byte cap for a REST response body (JSON).
pub const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
/// Byte cap for a 429 body; it is only read for `retry_after` / `global`.
const MAX_RATE_LIMIT_BODY_BYTES: usize = 64 * 1024;
/// `POST /auth/logout` timeout (`AuthSessionDependencies.ts`: 5 s, no retries).
const LOGOUT_TIMEOUT: Duration = Duration::from_secs(5);

/// What this client reports about itself. Contains no user data.
#[derive(Debug, Clone)]
pub struct ClientIdentity {
    /// App version, `^\d{1,6}(\.\d{1,6}){0,3}$` (server validation).
    pub client_version: String,
    pub release_channel: ReleaseChannel,
    /// Persisted per-install ID (32 lowercase hex). `None` generates a new
    /// one; read it back with [`RestClient::installation_id`] and persist it.
    pub installation_id: Option<String>,
}

impl Default for ClientIdentity {
    fn default() -> Self {
        Self {
            client_version: env!("CARGO_PKG_VERSION").to_owned(),
            release_channel: ReleaseChannel::Stable,
            installation_id: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RestConfig {
    /// REST base, e.g. `https://web.fluxer.app/api` (no trailing slash).
    pub api_base: String,
    pub api_code_version: u32,
    pub identity: ClientIdentity,
    pub policy: TransportPolicy,
    /// How many times a 429 is retried after waiting.
    pub max_rate_limit_retries: u32,
    /// A 429 whose wait is longer than this is returned as
    /// [`RestError::RateLimited`] instead of waiting.
    pub max_rate_limit_wait: Duration,
    /// Cap on how many requests this client *starts* in any 1 s window
    /// (1..=[`MAX_REQUESTS_PER_SECOND`], default
    /// [`DEFAULT_REQUESTS_PER_SECOND`]). This is a per-client start rate, not
    /// the server's per-user limit: the server bucket (50, leaking 50/s) is
    /// shared by every session of the user and sees arrival times, so the
    /// default leaves room for other sessions and for network jitter. Bursts
    /// after a stall are further bounded by [`MAX_IN_FLIGHT_REQUESTS`].
    pub max_requests_per_second: u32,
    /// Whole-request timeout.
    pub request_timeout: Duration,
}

impl RestConfig {
    pub fn new(endpoints: &Endpoints, identity: ClientIdentity) -> Self {
        Self {
            api_base: endpoints.api.clone(),
            api_code_version: endpoints.api_code_version,
            identity,
            policy: TransportPolicy::default(),
            max_rate_limit_retries: 2,
            max_rate_limit_wait: Duration::from_secs(10),
            max_requests_per_second: DEFAULT_REQUESTS_PER_SECOND,
            request_timeout: Duration::from_secs(30),
        }
    }
}

/// Whether a request carries the session token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Auth {
    Required,
    None,
}

/// Per-request overrides of the client defaults.
#[derive(Default)]
pub(crate) struct RequestOptions<'a> {
    /// A caller-solved captcha token sent as `X-Captcha-Token`.
    pub captcha_token: Option<&'a Secret>,
    /// Send this session token instead of the stored one ([`Auth::Required`]).
    pub token: Option<&'a SecretToken>,
    /// Whole-request timeout instead of [`RestConfig::request_timeout`].
    pub timeout: Option<Duration>,
    /// 429 retries instead of [`RestConfig::max_rate_limit_retries`].
    pub rate_limit_retries: Option<u32>,
    /// Optional reason sent percent-encoded in `X-Audit-Log-Reason`.
    pub audit_log_reason: Option<&'a str>,
}

/// The body of one request attempt.
enum RequestBody<'a> {
    Empty,
    Json(Vec<u8>),
    Multipart(&'a MultipartUpload),
}

/// The session token plus a generation bumped by every token change, so
/// `logout` clears only the token it logged out.
#[derive(Default)]
struct TokenSlot {
    token: Option<SecretToken>,
    generation: u64,
}

pub struct RestClient {
    http: reqwest::Client,
    /// `<api_base>/v<N>`
    versioned_base: String,
    installation_id: String,
    config: RestConfig,
    token: RwLock<TokenSlot>,
    rate: Mutex<RateState>,
    in_flight: Semaphore,
    /// At most one ALTCHA solve at a time.
    solver: Arc<Semaphore>,
}

impl std::fmt::Debug for RestClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RestClient")
            .field("versioned_base", &self.versioned_base)
            .finish_non_exhaustive()
    }
}

impl RestClient {
    pub fn new(config: RestConfig) -> Result<Self, RestError> {
        let base = config.api_base.trim_end_matches('/').to_owned();
        let base_url = Url::parse(&base).map_err(|_| RestError::InvalidUrl(redact_userinfo(&base)))?;
        if has_userinfo(&base_url) {
            return Err(RestError::InvalidUrl(format!(
                "{} (credentials in the API URL are not allowed)",
                redact_userinfo(&base)
            )));
        }
        if !config.policy.permits(&base_url) || !matches!(base_url.scheme(), "http" | "https") {
            return Err(RestError::InsecureUrl(base));
        }
        if config.api_code_version == 0 {
            return Err(RestError::InvalidArgument("api_code_version must be >= 1"));
        }
        if config.max_requests_per_second == 0 || config.max_requests_per_second > MAX_REQUESTS_PER_SECOND {
            return Err(RestError::InvalidArgument("max_requests_per_second must be 1..=40"));
        }
        let installation_id = match &config.identity.installation_id {
            Some(id) if is_installation_id(id) => id.clone(),
            Some(_) => return Err(RestError::InvalidArgument("installation_id must be 32 lowercase hex chars")),
            None => new_installation_id(),
        };
        let mut defaults = HeaderMap::new();
        defaults.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
        defaults.insert("x-fluxer-features", HeaderValue::from_static(FEATURES));
        defaults.insert("x-fluxer-capabilities", HeaderValue::from_static(CAPABILITIES));
        defaults.insert(
            "x-fluxer-client-installation-id",
            HeaderValue::from_str(&installation_id).map_err(|_| RestError::InvalidArgument("installation_id"))?,
        );
        defaults.insert(
            "x-fluxer-client-properties",
            HeaderValue::from_str(&client_properties_header(&config.identity))
                .map_err(|_| RestError::InvalidArgument("client properties"))?,
        );
        let http = reqwest::Client::builder()
            .user_agent(user_agent(&config.identity))
            .default_headers(defaults)
            .redirect(same_origin_redirects())
            .timeout(config.request_timeout)
            .build()
            .map_err(RestError::Transport)?;
        let versioned_base = format!("{base}/v{}", config.api_code_version);
        let rate = RateState::new(config.max_requests_per_second);
        Ok(Self {
            http,
            versioned_base,
            installation_id,
            config,
            token: RwLock::new(TokenSlot::default()),
            rate: Mutex::new(rate),
            in_flight: Semaphore::new(MAX_IN_FLIGHT_REQUESTS),
            solver: Arc::new(Semaphore::new(1)),
        })
    }

    /// [`RestConfig::request_timeout`].
    pub(crate) fn request_timeout(&self) -> Duration {
        self.config.request_timeout
    }

    /// The installation ID sent in `X-Fluxer-Client-Installation-ID`.
    pub fn installation_id(&self) -> &str {
        &self.installation_id
    }

    pub async fn set_token(&self, token: Option<SecretToken>) {
        let mut slot = self.token.write().await;
        slot.token = token;
        slot.generation = slot.generation.wrapping_add(1);
    }

    pub async fn has_token(&self) -> bool {
        self.token.read().await.token.is_some()
    }

    /// The stored token and its generation.
    pub(crate) async fn token_snapshot(&self) -> (Option<SecretToken>, u64) {
        let slot = self.token.read().await;
        (slot.token.clone(), slot.generation)
    }

    /// Clears the stored token only if it was not replaced since
    /// `generation` was read. Returns whether it cleared.
    pub(crate) async fn clear_token_if_generation(&self, generation: u64) -> bool {
        let mut slot = self.token.write().await;
        if slot.generation != generation {
            return false;
        }
        slot.token = None;
        slot.generation = slot.generation.wrapping_add(1);
        true
    }

    /// `POST /auth/logout` options (`AuthSessionDependencies.ts`): the given
    /// token, 5 s timeout, no 429 retries.
    pub(crate) fn logout_options(token: &SecretToken) -> RequestOptions<'_> {
        RequestOptions {
            token: Some(token),
            timeout: Some(LOGOUT_TIMEOUT),
            rate_limit_retries: Some(0),
            ..RequestOptions::default()
        }
    }

    /// Sends one request and returns the JSON body (`Value::Null` for an empty
    /// body such as 204). `path` starts with `/` and excludes `/v<N>`.
    pub(crate) async fn request(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
        auth: Auth,
    ) -> Result<Value, RestError> {
        self.request_with(method, path, query, body, auth, RequestOptions::default()).await
    }

    pub(crate) async fn request_with_reason(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
        auth: Auth,
        audit_log_reason: Option<&str>,
    ) -> Result<Value, RestError> {
        let options = RequestOptions { audit_log_reason, ..RequestOptions::default() };
        self.request_with(method, path, query, body, auth, options).await
    }

    /// [`Self::request`] plus an optional solved captcha token, sent as
    /// `X-Captcha-Token` (`CaptchaInterceptor.ts`).
    pub(crate) async fn request_with_captcha(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
        auth: Auth,
        captcha_token: Option<&Secret>,
    ) -> Result<Value, RestError> {
        let options = RequestOptions { captcha_token, ..RequestOptions::default() };
        self.request_with(method, path, query, body, auth, options).await
    }

    /// The request loop. Like `CaptchaInterceptor.ts`, an HTTP 400
    /// `CAPTCHA_REQUIRED` / `INVALID_CAPTCHA` carrying an ALTCHA challenge is
    /// solved off the async executor and the request is resent with
    /// `X-Captcha-Token`, at most `MAX_CAPTCHA_SOLVES` (2) times per call.
    /// If solving fails, the captcha error is returned unchanged.
    pub(crate) async fn request_with(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
        auth: Auth,
        options: RequestOptions<'_>,
    ) -> Result<Value, RestError> {
        let body = match body {
            Some(value) => RequestBody::Json(serde_json::to_vec(value).map_err(|e| RestError::Decode(e.to_string()))?),
            None => RequestBody::Empty,
        };
        self.send_request(method, path, query, body, auth, options).await
    }

    /// `multipart/form-data` request with the stored token. The form is
    /// rebuilt (files reopened) for every attempt, so 429 and captcha
    /// retries resend the whole body.
    pub(crate) async fn request_multipart(
        &self,
        method: Method,
        path: &str,
        upload: &MultipartUpload,
        timeout: Duration,
    ) -> Result<Value, RestError> {
        let options = RequestOptions { timeout: Some(timeout), ..RequestOptions::default() };
        self.send_request(method, path, &[], RequestBody::Multipart(upload), Auth::Required, options)
            .await
    }

    async fn send_request(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: RequestBody<'_>,
        auth: Auth,
        options: RequestOptions<'_>,
    ) -> Result<Value, RestError> {
        let mut captcha = match options.captcha_token {
            Some(token) => Some(captcha_header(token)?),
            None => None,
        };
        let audit_reason = match options.audit_log_reason {
            Some(reason) => {
                let normalized = super::api::normalize_audit_log_reason(reason);
                if normalized.is_empty() {
                    None
                } else {
                    if normalized.encode_utf16().count() > super::api::MAX_AUDIT_LOG_REASON_UTF16 {
                        return Err(RestError::InvalidArgument("reason"));
                    }
                    let encoded = super::api::encode_uri_component(&normalized);
                    Some(HeaderValue::from_str(&encoded).map_err(|_| RestError::InvalidArgument("reason"))?)
                }
            }
            None => None,
        };
        let url = self.build_url(path, query)?;
        // Generation of the stored token the header was built from; checked
        // again before each retry so a cleared or replaced token is not reused.
        let mut stored_generation: Option<u64> = None;
        let authorization = match (auth, options.token) {
            (Auth::Required, Some(token)) => Some(sensitive_header(token.expose(), "token")?),
            (Auth::Required, None) => {
                let slot = self.token.read().await;
                let token = slot.token.as_ref().ok_or(RestError::NotAuthenticated)?;
                stored_generation = Some(slot.generation);
                Some(sensitive_header(token.expose(), "token")?)
            }
            (Auth::None, _) => None,
        };
        let max_retries = options.rate_limit_retries.unwrap_or(self.config.max_rate_limit_retries);
        let bucket = format!("{method} {path}");
        let mut attempt = 0u32;
        let mut solves = 0u32;
        loop {
            self.wait_for_bucket(&bucket).await?;
            // Held until the body is read: bounds how many requests a stalled
            // connection can release at once.
            let permit = self
                .in_flight
                .acquire()
                .await
                .map_err(|_| RestError::InvalidArgument("request limiter closed"))?;
            self.pace().await;
            if let Some(generation) = stored_generation {
                let slot = self.token.read().await;
                if slot.token.is_none() || slot.generation != generation {
                    return Err(RestError::NotAuthenticated);
                }
            }
            let mut builder = self.http.request(method.clone(), url.clone());
            if let Some(timeout) = options.timeout {
                builder = builder.timeout(timeout);
            }
            if let Some(value) = &authorization {
                builder = builder.header(header::AUTHORIZATION, value.clone());
            }
            if let Some(value) = &captcha {
                builder = builder.header("x-captcha-token", value.clone());
            }
            if let Some(value) = &audit_reason {
                builder = builder.header("x-audit-log-reason", value.clone());
            }
            match &body {
                RequestBody::Empty => {}
                RequestBody::Json(bytes) => {
                    builder = builder
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(bytes.clone());
                }
                RequestBody::Multipart(upload) => builder = builder.multipart(upload.build_form().await?),
            }
            let response = builder.send().await.map_err(RestError::Transport)?;
            let status = response.status();
            tracing::debug!(%method, path, status = status.as_u16(), attempt, "rest response");
            if status == StatusCode::TOO_MANY_REQUESTS {
                let limited = self.record_rate_limit(&bucket, response).await;
                drop(permit);
                if attempt < max_retries && limited.wait <= self.config.max_rate_limit_wait {
                    attempt += 1;
                    tracing::debug!(%method, path, wait_ms = limited.wait.as_millis() as u64, global = limited.global, "rate limited; retrying");
                    continue;
                }
                return Err(RestError::RateLimited { retry_after: limited.wait, global: limited.global });
            }
            if status.is_redirection() {
                return Err(RestError::UnexpectedRedirect { status: status.as_u16() });
            }
            let bytes = read_body_capped(response, MAX_RESPONSE_BYTES).await?;
            drop(permit);
            if status.is_success() {
                if bytes.iter().all(u8::is_ascii_whitespace) {
                    return Ok(Value::Null);
                }
                return serde_json::from_slice(&bytes)
                    .map_err(|e| RestError::Decode(format!("response body is not JSON: {e}")));
            }
            let raw: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
            if solves < MAX_CAPTCHA_SOLVES && is_captcha_error(status.as_u16(), &raw) {
                if let Some(challenge) = read_altcha_challenge(&raw) {
                    solves += 1;
                    tracing::debug!(%method, path, solves, "solving ALTCHA challenge");
                    match solve_off_executor(challenge.clone(), self.solver.clone()).await {
                        Ok(token) => {
                            captcha = Some(captcha_header(&token)?);
                            continue;
                        }
                        Err(error) => tracing::warn!(%method, path, %error, "ALTCHA solve failed"),
                    }
                }
            }
            return Err(RestError::Api(api_error(status.as_u16(), &raw)));
        }
    }

    fn build_url(&self, path: &str, query: &[(&str, String)]) -> Result<Url, RestError> {
        if !path.starts_with('/') {
            return Err(RestError::InvalidArgument("path must start with '/'"));
        }
        let mut url = Url::parse(&format!("{}{path}", self.versioned_base))
            .map_err(|_| RestError::InvalidUrl(format!("{}{path}", self.versioned_base)))?;
        if !query.is_empty() {
            let mut pairs = url.query_pairs_mut();
            for (key, value) in query {
                pairs.append_pair(key, value);
            }
        }
        Ok(url)
    }

    /// Waits for any recorded 429 on this bucket (or globally) and for a
    /// pacer slot. Fails fast when the remaining wait exceeds the limit.
    #[cfg(test)]
    pub(super) async fn wait_for_slot(&self, bucket: &str) -> Result<(), RestError> {
        self.wait_for_bucket(bucket).await?;
        self.pace().await;
        Ok(())
    }

    /// Waits for any recorded 429 on this bucket (or globally). Fails fast
    /// when the remaining wait exceeds the limit.
    async fn wait_for_bucket(&self, bucket: &str) -> Result<(), RestError> {
        let blocked = {
            let mut rate = self.rate.lock().await;
            rate.blocked_for(bucket, Instant::now())
        };
        if let Some((wait, global)) = blocked {
            if wait > self.config.max_rate_limit_wait {
                return Err(RestError::RateLimited { retry_after: wait, global });
            }
            tokio::time::sleep(wait).await;
        }
        Ok(())
    }

    /// Reserves the next pacer slot and sleeps until it.
    async fn pace(&self) {
        let pace = {
            let mut rate = self.rate.lock().await;
            rate.pacer.reserve(Instant::now())
        };
        if !pace.is_zero() {
            tracing::debug!(wait_ms = pace.as_millis() as u64, "request pacer delaying request");
            tokio::time::sleep(pace).await;
        }
    }

    async fn record_rate_limit(&self, bucket: &str, response: reqwest::Response) -> RateLimitInfo {
        let headers = response.headers().clone();
        let body: Value = read_body_capped(response, MAX_RATE_LIMIT_BODY_BYTES)
            .await
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or(Value::Null);
        let info = parse_rate_limit(&headers, &body);
        let until = Instant::now() + info.wait;
        let mut rate = self.rate.lock().await;
        if info.global {
            rate.global_until = Some(rate.global_until.map_or(until, |u| u.max(until)));
        } else {
            rate.buckets.insert(bucket.to_owned(), until);
        }
        info
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RateLimitInfo {
    pub wait: Duration,
    pub global: bool,
}

/// `reactToRateLimit` + `recordPacing`: `X-RateLimit-Reset-After` (seconds)
/// wins, then `Retry-After` (seconds or an HTTP date), then the body's
/// `retry_after`; 1 s when none is usable (missing, non-positive or a date
/// in the past). `global` comes from `X-RateLimit-Global: true` or the body.
pub(crate) fn parse_rate_limit(headers: &HeaderMap, body: &Value) -> RateLimitInfo {
    let header_text = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).map(str::trim);
    let positive = |v: f64| (v.is_finite() && v > 0.0).then_some(v);
    let header_secs = |name: &str| header_text(name).and_then(|v| v.parse::<f64>().ok()).and_then(positive);
    let retry_after_date = || {
        let date = httpdate::parse_http_date(header_text("retry-after")?).ok()?;
        let wait = date.duration_since(SystemTime::now()).ok()?;
        positive(wait.as_secs_f64())
    };
    let secs = header_secs("x-ratelimit-reset-after")
        .or_else(|| header_secs("retry-after"))
        .or_else(retry_after_date)
        .or_else(|| body.get("retry_after").and_then(Value::as_f64).and_then(positive))
        .unwrap_or(1.0);
    let global = headers
        .get("x-ratelimit-global")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("true"))
        || body.get("global").and_then(Value::as_bool).unwrap_or(false);
    // Cap absurd values so Duration::from_secs_f64 cannot panic.
    let wait = Duration::from_secs_f64(secs.min(86_400.0));
    RateLimitInfo { wait, global }
}

/// `readCaptchaChallenge` status/code check (`CaptchaInterceptor.ts`).
fn is_captcha_error(status: u16, body: &Value) -> bool {
    status == 400 && matches!(body.get("code").and_then(Value::as_str), Some("CAPTCHA_REQUIRED" | "INVALID_CAPTCHA"))
}

fn sensitive_header(value: &str, what: &'static str) -> Result<HeaderValue, RestError> {
    let mut header = HeaderValue::from_str(value).map_err(|_| RestError::InvalidArgument(what))?;
    header.set_sensitive(true);
    Ok(header)
}

fn captcha_header(token: &Secret) -> Result<HeaderValue, RestError> {
    sensitive_header(token.expose(), "captcha token is not a valid header value")
}

/// Reads a response body, failing with [`RestError::ResponseTooLarge`] as
/// soon as it would exceed `limit`, with or without `Content-Length`.
pub(crate) async fn read_body_capped(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>, RestError> {
    if response.content_length().is_some_and(|len| len > limit as u64) {
        return Err(RestError::ResponseTooLarge { limit });
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(RestError::Transport)? {
        if body.len() + chunk.len() > limit {
            return Err(RestError::ResponseTooLarge { limit });
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn api_error(status: u16, body: &Value) -> ApiError {
    let code = body
        .get("code")
        .and_then(Value::as_str)
        .map_or_else(|| format!("HTTP_{status}"), str::to_owned);
    let message = body.get("message").and_then(Value::as_str).unwrap_or_default().to_owned();
    let errors = body
        .get("errors")
        .and_then(|e| serde_json::from_value::<Vec<FieldError>>(e.clone()).ok())
        .unwrap_or_default();
    ApiError { status, code, message, errors, body: redact_json(body) }
}

struct RateState {
    pacer: Pacer,
    buckets: HashMap<String, Instant>,
    global_until: Option<Instant>,
}

impl RateState {
    fn new(per_second: u32) -> Self {
        Self { pacer: Pacer::new(per_second), buckets: HashMap::new(), global_until: None }
    }

    /// Remaining wait from a recorded 429, and whether it is the global one.
    fn blocked_for(&mut self, bucket: &str, now: Instant) -> Option<(Duration, bool)> {
        if self.global_until.is_some_and(|u| u <= now) {
            self.global_until = None;
        }
        if self.buckets.get(bucket).is_some_and(|u| *u <= now) {
            self.buckets.remove(bucket);
        }
        let global = self.global_until.map(|u| u - now);
        let local = self.buckets.get(bucket).map(|u| *u - now);
        match (global, local) {
            (Some(g), Some(l)) if l > g => Some((l, false)),
            (Some(g), _) => Some((g, true)),
            (None, Some(l)) => Some((l, false)),
            (None, None) => None,
        }
    }
}

/// Sliding-window limiter: at most `limit` request starts in any 1 s window.
#[derive(Debug)]
pub(crate) struct Pacer {
    limit: usize,
    starts: VecDeque<Instant>,
}

impl Pacer {
    pub(crate) fn new(per_second: u32) -> Self {
        let limit = usize::try_from(per_second.max(1)).unwrap_or(usize::MAX);
        Self { limit, starts: VecDeque::with_capacity(limit) }
    }

    /// Reserves the next start slot and returns how long to wait for it.
    pub(crate) fn reserve(&mut self, now: Instant) -> Duration {
        while self.starts.front().is_some_and(|t| *t + PACER_WINDOW <= now) {
            self.starts.pop_front();
        }
        let mut at = now;
        if self.starts.len() >= self.limit {
            if let Some(oldest) = self.starts.pop_front() {
                at = at.max(oldest + PACER_WINDOW);
            }
        }
        if let Some(last) = self.starts.back() {
            at = at.max(*last);
        }
        self.starts.push_back(at);
        at - now
    }
}

/// Follows redirects only within the same origin (scheme, host, port), so the
/// `Authorization` header and Fluxer headers never reach another host and
/// https is never downgraded. Anything else stops and surfaces as
/// [`RestError::UnexpectedRedirect`].
fn same_origin_redirects() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() > MAX_REDIRECTS {
            return attempt.stop();
        }
        let same = attempt
            .previous()
            .last()
            .is_some_and(|prev| prev.origin() == attempt.url().origin());
        if same { attempt.follow() } else { attempt.stop() }
    })
}

/// `Fluxer Desktop/<version>` matches the server's native-client regex
/// `^Fluxer (Android|iOS|Linux|Desktop|Client)(?=[/ ]|$)` so the server reads
/// `X-Fluxer-Client-Properties`.
pub(crate) fn user_agent(identity: &ClientIdentity) -> String {
    format!("Fluxer Desktop/{}", identity.client_version)
}

/// Base64 JSON `{os, client_platform, client_version, release_channel}`
/// (`SessionClientIdentity.ts`). No user data.
pub(crate) fn client_properties_header(identity: &ClientIdentity) -> String {
    let properties = serde_json::json!({
        "os": current_os(),
        "client_platform": "desktop",
        "client_version": identity.client_version,
        "release_channel": identity.release_channel.as_str(),
    });
    base64_encode(properties.to_string().as_bytes())
}

fn current_os() -> &'static str {
    match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "macos",
        "linux" => "linux",
        "android" => "android",
        "ios" => "ios",
        _ => "linux",
    }
}

fn is_installation_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// 16 random bytes as 32 lowercase hex chars (`ClientInstallationId.ts`).
pub fn new_installation_id() -> String {
    let bytes: [u8; 16] = rand::random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        let n = (u32::from(b0) << 16) | (u32::from(b1) << 8) | u32::from(b2);
        out.push(char::from(TABLE[(n >> 18) as usize & 63]));
        out.push(char::from(TABLE[(n >> 12) as usize & 63]));
        out.push(if chunk.len() > 1 { char::from(TABLE[(n >> 6) as usize & 63]) } else { '=' });
        out.push(if chunk.len() > 2 { char::from(TABLE[n as usize & 63]) } else { '=' });
    }
    out
}
