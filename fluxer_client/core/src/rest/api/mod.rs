//! REST endpoints, one module per API domain. New endpoints go in the
//! matching module (or a new one registered here) so parallel work does not
//! touch the same file. Bodies are raw JSON until state DTOs are wired in;
//! guild/channel/DM lists come from the gateway READY payload.

mod access;
mod channels;
mod guild_membership;
mod guilds;
mod invites;
mod messages;
mod notes;
mod reactions;
mod read_states;
mod relationships;
mod saved_messages;
mod search;
mod settings;
mod threads;
mod typing;
mod users;

pub use access::{RolePatch, validate_role_name};
pub use channels::{
    CreateGuildChannelRequest, GroupDmPatch, GuildChannelPatch, MAX_CHANNEL_NAME_UTF16,
    MAX_CHANNEL_RATE_LIMIT_PER_USER, MAX_CHANNEL_TOPIC_UTF16, MAX_GROUP_DM_OTHER_RECIPIENTS,
    VOICE_CHANNEL_BITRATE_DEFAULT, VOICE_CHANNEL_CONNECTION_LIMIT_DEFAULT, validate_create_guild_channel,
    validate_guild_channel_patch,
};
pub use guilds::{
    BanMemberRequest, GuildBanEntry, GuildMemberPatch, MAX_AUDIT_LOG_REASON_UTF16, MAX_BAN_DELETE_MESSAGE_SECONDS,
    MAX_MEMBER_BIO_UTF16, MAX_MEMBER_PRONOUNS_UTF16, MAX_NICKNAME_UTF16, MAX_TEMP_BAN_DURATION_SECONDS,
    MAX_TIMEOUT_DURATION_SECONDS, MIN_TEMP_BAN_DURATION_SECONDS, MemberQuery, normalize_audit_log_reason,
    validate_audit_log_reason, validate_ban_request, validate_member_patch,
};
pub use invites::CreateInviteRequest;
pub use relationships::parse_user_tag;
pub use settings::{ChannelOverride, CustomStatus, GuildSettingsPatch, MuteConfig, UserSettingsPatch};
pub(in crate::rest) use messages::message_body;
pub use messages::{EditMessageRequest, MessageQuery, MessageReference, PinsQuery, SendMessageRequest};
pub use reactions::{ReactionEmoji, ReactionUsersQuery, encode_uri_component};
pub use read_states::AckRequest;
pub use saved_messages::SavedMessagesQuery;
pub use threads::{
    CreateThreadFromMessageRequest, MAX_THREAD_NAME_UTF16, THREAD_AUTO_ARCHIVE_DURATIONS, validate_thread_name,
};
pub use typing::{
    TYPING_IMMEDIATE_SEND_IDLE, TYPING_SEND_DELAY, TYPING_THROTTLE, TypingCooldown, TypingPlan, TypingThrottle,
};
pub use search::{
    MessageContentType, MessageSearchOutcome, MessageSearchPage, MessageSearchRequest, MessageSearchScope,
    MessageSearchSortBy, MessageSearchSortOrder, parse_message_search_response, validate_search_request,
};
pub use users::ProfileQuery;

use crate::rest::error::RestError;

/// Snowflakes are decimal u64 strings; checking them keeps caller input from
/// altering the request path.
pub(in crate::rest) fn check_snowflake(value: &str, what: &'static str) -> Result<(), RestError> {
    if !value.is_empty() && value.len() <= 20 && value.bytes().all(|b| b.is_ascii_digit()) {
        Ok(())
    } else {
        Err(RestError::InvalidArgument(what))
    }
}
