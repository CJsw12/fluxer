//! REST error types. Nothing here ever carries a token or password: API
//! bodies are stored only after `redact_json`.

use std::time::Duration;

use serde::Deserialize;

/// One entry of the `errors` array of a Fluxer error body
/// (`packages/schema/src/domains/common/ErrorSchemas.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FieldError {
    pub path: String,
    #[serde(default)]
    pub code: Option<String>,
    pub message: String,
}

/// A non-2xx response with a Fluxer error body
/// `{code, message, errors?, ...extra}`.
#[derive(Debug, Clone, PartialEq)]
pub struct ApiError {
    pub status: u16,
    /// Error code, e.g. `UNAUTHORIZED`. `HTTP_<status>` if the body had none.
    pub code: String,
    pub message: String,
    pub errors: Vec<FieldError>,
    /// The whole body with secret keys redacted (extra per-error fields live
    /// at the top level).
    pub body: serde_json::Value,
}

#[derive(Debug, thiserror::Error)]
pub enum RestError {
    #[error("refusing insecure URL {0} (https is required; http only for loopback when allowed)")]
    InsecureUrl(String),
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    #[error("invalid argument: {0}")]
    InvalidArgument(&'static str),
    #[error("HTTP transport error: {0}")]
    Transport(#[source] reqwest::Error),
    #[error("HTTP {status} {code}: {message}", status = .0.status, code = .0.code, message = .0.message)]
    Api(ApiError),
    #[error("rate limited (global: {global}); retry after {retry_after:?}")]
    RateLimited { retry_after: Duration, global: bool },
    #[error("server answered with redirect HTTP {status} to another origin; not followed")]
    UnexpectedRedirect { status: u16 },
    #[error("could not decode response: {0}")]
    Decode(String),
    #[error("no session token set")]
    NotAuthenticated,
    #[error("not supported by this client yet: {0}")]
    Unsupported(&'static str),
    /// The response body exceeded the client's byte cap.
    #[error("response body exceeds the {limit} byte limit")]
    ResponseTooLarge { limit: usize },
    /// An ALTCHA challenge could not be solved (malformed, or no solution
    /// within the counter and time bounds).
    #[error("captcha could not be solved: {0}")]
    Captcha(String),
    /// An attachment exceeds the per-file size limit; nothing was sent.
    #[error("attachment {index} is {size} bytes; the limit is {limit} bytes")]
    AttachmentTooLarge { index: usize, size: u64, limit: u64 },
    /// An attachment file could not be opened or read.
    #[error("could not read attachment: {0}")]
    AttachmentIo(#[source] std::io::Error),
}

impl RestError {
    /// The API error code, if this is an API error.
    pub fn code(&self) -> Option<&str> {
        match self {
            RestError::Api(api) => Some(api.code.as_str()),
            _ => None,
        }
    }

    pub fn status(&self) -> Option<u16> {
        match self {
            RestError::Api(api) => Some(api.status),
            _ => None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[error("\"{0}\" is not a valid instance address")]
    InvalidInput(String),
    #[error("refusing insecure URL {0} (https/wss required; http/ws only for loopback when allowed)")]
    InsecureUrl(String),
    #[error("instance discovery failed for every candidate: {}", format_attempts(.0))]
    Unreachable(Vec<DiscoveryAttempt>),
    #[error("instance speaks generation \"{0}\", which this client does not understand")]
    RequiresNewerClient(String),
    #[error("discovery document from {served_by} declares endpoint {declared} on another origin")]
    OriginMismatch { served_by: String, declared: String },
    #[error("could not build HTTP client: {0}")]
    Client(#[source] reqwest::Error),
}

/// A failed discovery candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryAttempt {
    pub url: String,
    pub reason: String,
}

fn format_attempts(attempts: &[DiscoveryAttempt]) -> String {
    attempts
        .iter()
        .map(|a| format!("{} ({})", a.url, a.reason))
        .collect::<Vec<_>>()
        .join("; ")
}
