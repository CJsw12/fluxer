//! REST endpoints, one module per API domain. New endpoints go in the
//! matching module (or a new one registered here) so parallel work does not
//! touch the same file. Bodies are raw JSON until state DTOs are wired in;
//! guild/channel/DM lists come from the gateway READY payload.

mod guild_membership;
mod guilds;
mod invites;
mod messages;
mod notes;
mod reactions;
mod read_states;
mod relationships;
mod settings;
mod typing;
mod users;

pub use guilds::MemberQuery;
pub use invites::CreateInviteRequest;
pub use relationships::parse_user_tag;
pub use settings::{ChannelOverride, CustomStatus, GuildSettingsPatch, MuteConfig, UserSettingsPatch};
pub(in crate::rest) use messages::message_body;
pub use messages::{EditMessageRequest, MessageQuery, MessageReference, PinsQuery, SendMessageRequest};
pub use reactions::{ReactionEmoji, ReactionUsersQuery, encode_uri_component};
pub use read_states::AckRequest;
pub use typing::{
    TYPING_IMMEDIATE_SEND_IDLE, TYPING_SEND_DELAY, TYPING_THROTTLE, TypingCooldown, TypingPlan, TypingThrottle,
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
