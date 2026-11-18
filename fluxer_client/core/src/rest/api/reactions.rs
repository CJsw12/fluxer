//! Reaction endpoints (`MessageInteractionController.ts`,
//! `ReactionCommands.tsx`).

use reqwest::Method;
use serde_json::Value;

use super::check_snowflake;
use crate::rest::client::{Auth, RestClient};
use crate::rest::error::RestError;
use super::messages::message_path;

/// A reaction emoji: unicode (`id: None`, `name` is the emoji itself) or
/// custom (`name` plus emoji `id`), like `ReactionEmoji` in `ReactionUtils.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReactionEmoji {
    pub name: String,
    pub id: Option<String>,
}

impl ReactionEmoji {
    pub fn unicode(emoji: impl Into<String>) -> Self {
        Self { name: emoji.into(), id: None }
    }

    pub fn custom(name: impl Into<String>, id: impl Into<String>) -> Self {
        Self { name: name.into(), id: Some(id.into()) }
    }

    /// The `:emoji` path segment: `encodeURIComponent(id ? name:id : name)`
    /// (`ReactionCommands.tsx::makeUrl`).
    pub fn path_segment(&self) -> Result<String, RestError> {
        let name = self.name.as_str();
        if name.is_empty() || name.len() > MAX_EMOJI_NAME_BYTES || name.chars().any(char::is_control) {
            return Err(RestError::InvalidArgument("emoji name"));
        }
        // "." / ".." would be removed by URL path normalisation.
        if name.bytes().all(|b| b == b'.') {
            return Err(RestError::InvalidArgument("emoji name"));
        }
        let raw = match &self.id {
            Some(id) => {
                check_snowflake(id, "emoji id")?;
                format!("{name}:{id}")
            }
            None => name.to_owned(),
        };
        Ok(encode_uri_component(&raw))
    }
}

const MAX_EMOJI_NAME_BYTES: usize = 128;

/// `GET .../reactions/{emoji}/users` query (`ReactionUsersQuerySchema`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReactionUsersQuery {
    /// 1..=100.
    pub limit: Option<u8>,
    /// User ID cursor (`next_after` of the previous page).
    pub after: Option<String>,
}

impl RestClient {
    /// `PUT .../reactions/{emoji}/@me` (204). `session_id` is the gateway
    /// session, sent like `ReactionCommands.tsx::sessionQuery`.
    pub async fn add_reaction(
        &self,
        channel_id: &str,
        message_id: &str,
        emoji: &ReactionEmoji,
        session_id: Option<&str>,
    ) -> Result<(), RestError> {
        let path = format!("{}/@me", reaction_path(channel_id, message_id, emoji)?);
        let query = session_query(session_id)?;
        self.request(Method::PUT, &path, &query, None, Auth::Required).await?;
        Ok(())
    }

    /// `DELETE .../reactions/{emoji}/@me` (204).
    pub async fn remove_own_reaction(
        &self,
        channel_id: &str,
        message_id: &str,
        emoji: &ReactionEmoji,
        session_id: Option<&str>,
    ) -> Result<(), RestError> {
        let path = format!("{}/@me", reaction_path(channel_id, message_id, emoji)?);
        let query = session_query(session_id)?;
        self.request(Method::DELETE, &path, &query, None, Auth::Required).await?;
        Ok(())
    }

    /// `GET .../reactions/{emoji}/users` → `{items: [UserPartial], has_more, next_after?}`.
    pub async fn get_reaction_users(
        &self,
        channel_id: &str,
        message_id: &str,
        emoji: &ReactionEmoji,
        query: &ReactionUsersQuery,
    ) -> Result<Value, RestError> {
        let path = format!("{}/users", reaction_path(channel_id, message_id, emoji)?);
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(limit) = query.limit {
            if !(1..=100).contains(&limit) {
                return Err(RestError::InvalidArgument("reaction users limit must be 1..=100"));
            }
            params.push(("limit", limit.to_string()));
        }
        if let Some(after) = &query.after {
            check_snowflake(after, "after")?;
            params.push(("after", after.clone()));
        }
        self.request(Method::GET, &path, &params, None, Auth::Required).await
    }
}

fn reaction_path(channel_id: &str, message_id: &str, emoji: &ReactionEmoji) -> Result<String, RestError> {
    let base = message_path(channel_id, message_id)?;
    Ok(format!("{base}/reactions/{}", emoji.path_segment()?))
}

/// `SessionIdQuerySchema`: optional, 1..=64 chars.
fn session_query(session_id: Option<&str>) -> Result<Vec<(&'static str, String)>, RestError> {
    match session_id {
        None => Ok(Vec::new()),
        Some(id) if !id.is_empty() && id.chars().count() <= 64 => Ok(vec![("session_id", id.to_owned())]),
        Some(_) => Err(RestError::InvalidArgument("session_id must be 1..=64 chars")),
    }
}

/// JavaScript `encodeURIComponent`: UTF-8 bytes percent-encoded except
/// `A-Z a-z 0-9 - _ . ! ~ * ' ( )`.
pub fn encode_uri_component(input: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(input.len() * 3);
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')') {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    out
}
