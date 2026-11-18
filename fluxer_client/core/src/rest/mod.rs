//! REST and auth for a Fluxer instance: discovery, the HTTP client
//! (headers, error bodies, 429 handling, request pacer, in-flight cap,
//! ALTCHA captcha solving), login/MFA/logout, the first-screen endpoints,
//! message actions (edit/delete/pins/reactions/typing), multipart
//! attachment uploads and user/DM/member reads.
//!
//! Spec: `docs/rust_client_core/REST_AND_AUTH.md`.
//! Security rules: tokens and passwords live only in [`SecretToken`] /
//! [`Secret`], whose `Debug`/`Display` are masked; error bodies are redacted;
//! https/wss is required except for loopback when explicitly allowed; TLS
//! verification is never disabled; redirects are followed only within the
//! same origin.

mod altcha;
mod api;
mod auth;
mod client;
mod discovery;
mod error;
mod secret;
mod upload;

#[cfg(test)]
mod tests;

pub use altcha::{ALTCHA_MAX_COUNTER, ALTCHA_SOLVE_TIMEOUT, solve_altcha_challenge};
pub use api::{
    AckRequest, ChannelOverride, CreateInviteRequest, CustomStatus, GuildSettingsPatch, MuteConfig, UserSettingsPatch,
    parse_user_tag, EditMessageRequest, MemberQuery, MessageQuery, MessageReference, PinsQuery, ProfileQuery,
    ReactionEmoji, ReactionUsersQuery, SendMessageRequest, TYPING_IMMEDIATE_SEND_IDLE, TYPING_SEND_DELAY,
    TYPING_THROTTLE, TypingCooldown, TypingPlan, TypingThrottle, encode_uri_component,
};
pub use auth::{LoginOutcome, LoginSuccess, MfaMethods};
pub use client::{
    ClientIdentity, DEFAULT_REQUESTS_PER_SECOND, MAX_IN_FLIGHT_REQUESTS, MAX_REQUESTS_PER_SECOND, MAX_RESPONSE_BYTES,
    RestClient, RestConfig, SERVER_GLOBAL_LIMIT_PER_SECOND, new_installation_id,
};
pub use discovery::{
    DISCOVERY_MAX_BYTES, DISCOVERY_PATH, DISCOVERY_TIMEOUT, DiscoveryOptions, Endpoints, ReleaseChannel,
    TransportPolicy, discover, discover_with, discovery_candidates, resolve_instance_input,
};
pub use error::{ApiError, DiscoveryAttempt, DiscoveryError, FieldError, RestError};
pub use secret::{Secret, SecretToken, redact_json};
pub use upload::{
    DEFAULT_MAX_ATTACHMENT_FILE_SIZE, MAX_ATTACHMENTS_PER_MESSAGE, UploadFile, UploadOptions, UploadProgress,
    UploadSource,
};
