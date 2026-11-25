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
mod profile;
mod secret;
mod upload;

#[cfg(test)]
mod tests;

pub use altcha::{ALTCHA_MAX_COUNTER, ALTCHA_SOLVE_TIMEOUT, solve_altcha_challenge};
pub use api::{
    CONNECTION_VISIBILITY_EVERYONE, CONNECTION_VISIBILITY_FRIENDS, CONNECTION_VISIBILITY_MUTUAL_GUILDS,
    ConnectionPatch, DeveloperApplication, DeveloperApplicationBot, MAX_CONNECTIONS_PER_USER, RolePatch,
    UserConnection, validate_role_name,
};
pub use api::{
    AckRequest, BanMemberRequest, ChannelOverride, CreateGuildChannelRequest, CreateInviteRequest,
    CreateThreadFromMessageRequest, CustomStatus, DiscoveryCategory, DiscoveryCategoryCount,
    DiscoveryGuild, DiscoveryGuildList, DiscoveryGuildQuery, EditMessageRequest, GroupDmPatch, GuildBanEntry,
    GuildChannelPatch, GuildMemberPatch, GuildSettingsPatch, MAX_AUDIT_LOG_REASON_UTF16,
    MAX_BAN_DELETE_MESSAGE_SECONDS, MAX_CHANNEL_NAME_UTF16, MAX_CHANNEL_RATE_LIMIT_PER_USER,
    MAX_CHANNEL_TOPIC_UTF16, MAX_GROUP_DM_OTHER_RECIPIENTS, MAX_MEMBER_BIO_UTF16,
    MAX_MEMBER_PRONOUNS_UTF16, MAX_NICKNAME_UTF16,
    MAX_TEMP_BAN_DURATION_SECONDS, MAX_THREAD_NAME_UTF16, MAX_TIMEOUT_DURATION_SECONDS,
    MIN_TEMP_BAN_DURATION_SECONDS, MemberQuery, MessageContentType, MessageQuery, MessageReference,
    MessageSearchOutcome, MessageSearchPage, MessageSearchRequest, MessageSearchScope,
    MessageSearchSortBy, MessageSearchSortOrder, MuteConfig, PinsQuery, ProfileQuery, ReactionEmoji,
    ReactionUsersQuery, SavedMessagesQuery, SendMessageRequest, THREAD_AUTO_ARCHIVE_DURATIONS,
    TYPING_IMMEDIATE_SEND_IDLE, TYPING_SEND_DELAY, TYPING_THROTTLE, TypingCooldown, TypingPlan,
    TypingThrottle, UserSettingsPatch, VOICE_CHANNEL_BITRATE_DEFAULT,
    VOICE_CHANNEL_CONNECTION_LIMIT_DEFAULT, encode_uri_component, normalize_audit_log_reason,
    parse_message_search_response, parse_user_tag, validate_audit_log_reason, validate_ban_request,
    validate_create_guild_channel, validate_guild_channel_patch, validate_member_patch,
    validate_search_request, validate_thread_name,
};
pub use auth::{LoginOutcome, LoginSuccess, MfaMethods, WebAuthnCredential};
pub use client::{
    ClientIdentity, DEFAULT_REQUESTS_PER_SECOND, MAX_IN_FLIGHT_REQUESTS, MAX_REQUESTS_PER_SECOND, MAX_RESPONSE_BYTES,
    RestClient, RestConfig, SERVER_GLOBAL_LIMIT_PER_SECOND, new_installation_id,
};
pub use discovery::{
    DISCOVERY_MAX_BYTES, DISCOVERY_PATH, DISCOVERY_TIMEOUT, DiscoveryOptions, Endpoints, ReleaseChannel,
    TransportPolicy, discover, discover_with, discovery_candidates, resolve_instance_input,
};
pub use error::{ApiError, DiscoveryAttempt, DiscoveryError, FieldError, RestError};
pub use profile::{
    GuildMemberSummary, MutualGuild, PROFILE_FLAG_AVATAR_UNSET, PROFILE_FLAG_BANNER_UNSET, ProfileUser,
    UserProfileData, UserProfileFull,
};
pub use secret::{Secret, SecretToken, redact_json};
pub use upload::{
    DEFAULT_MAX_ATTACHMENT_FILE_SIZE, MAX_ATTACHMENTS_PER_MESSAGE, UploadFile, UploadOptions, UploadProgress,
    UploadSource,
};
