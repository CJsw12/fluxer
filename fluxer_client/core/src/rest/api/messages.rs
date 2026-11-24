//! Message endpoints: list, send, fetch one, edit, delete and pins.
//! Routes: `fluxer_api/src/api/channel/controllers/MessageController.ts`;
//! client shapes: `fluxer_app/src/features/messaging/commands/
//! MessageCommands.tsx`, `ChannelPinsCommands.tsx`. Reactions and typing
//! live in `reactions.rs` / `typing.rs`.

use reqwest::Method;
use serde::Serialize;
use serde_json::{Value, json};

use super::check_snowflake;
use crate::rest::client::{Auth, RestClient};
use crate::rest::error::RestError;

/// `GET /channels/{id}/messages` query (`MessageRequestSchemas.ts`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MessageQuery {
    pub before: Option<String>,
    pub after: Option<String>,
    pub around: Option<String>,
    /// 1..=100, server default 50.
    pub limit: Option<u8>,
}

/// `MessageReferenceRequest` (`SharedMessageSchemas.ts`): reply (type 0,
/// default) or forward (type 1).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct MessageReference {
    pub message_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guild_id: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<u8>,
}

/// JSON body of `POST /channels/{c}/messages` (`MessageRequestSchema`),
/// limited to the fields this client sends. Also the `payload_json` of an
/// attachment upload.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SendMessageRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// 1..=32 chars; echoed back so the optimistic message can be matched.
    pub nonce: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_reference: Option<MessageReference>,
    /// `AllowedMentionsRequest`, passed through as-is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_mentions: Option<Value>,
    /// `MessageFlags` bitfield; omitted when 0 (server default).
    #[serde(skip_serializing_if = "is_zero")]
    pub flags: u32,
}

/// JSON body of `PATCH /channels/{c}/messages/{m}` (`buildMessageEditRequest`):
/// only the fields that are `Some` are sent.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct EditMessageRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flags: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_mentions: Option<Value>,
}

/// `GET /channels/{c}/messages/pins` query (`ChannelPinsQuerySchema`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PinsQuery {
    /// 1..=50.
    pub limit: Option<u8>,
    /// ISO 8601 `pinned_at` of the last pin of the previous page.
    pub before: Option<String>,
}

impl RestClient {
    /// `GET /channels/{channel_id}/messages`; newest first.
    pub async fn get_messages(&self, channel_id: &str, query: &MessageQuery) -> Result<Value, RestError> {
        check_snowflake(channel_id, "channel_id")?;
        let mut params: Vec<(&str, String)> = Vec::new();
        for (key, value) in [("before", &query.before), ("after", &query.after), ("around", &query.around)] {
            if let Some(id) = value {
                check_snowflake(id, "message id")?;
                params.push((key, id.clone()));
            }
        }
        if let Some(limit) = query.limit {
            if !(1..=100).contains(&limit) {
                return Err(RestError::InvalidArgument("limit must be 1..=100"));
            }
            params.push(("limit", limit.to_string()));
        }
        let path = format!("/channels/{channel_id}/messages");
        self.request(Method::GET, &path, &params, None, Auth::Required).await
    }

    /// `POST /channels/{channel_id}/messages` with `{content, nonce}`.
    /// `nonce` is 1..=32 chars; the TS client uses a snowflake-like value.
    pub async fn send_message(&self, channel_id: &str, content: &str, nonce: &str) -> Result<Value, RestError> {
        check_snowflake(channel_id, "channel_id")?;
        if nonce.is_empty() || nonce.chars().count() > 32 {
            return Err(RestError::InvalidArgument("nonce must be 1..=32 chars"));
        }
        let path = format!("/channels/{channel_id}/messages");
        let body = json!({ "content": content, "nonce": nonce });
        self.request(Method::POST, &path, &[], Some(&body), Auth::Required).await
    }

    /// `POST /channels/{c}/messages` with a typed JSON body (replies,
    /// flags, allowed mentions). See [`Self::send_message`] for plain text.
    pub async fn send_message_request(&self, channel_id: &str, message: &SendMessageRequest) -> Result<Value, RestError> {
        let body = message_body(message)?;
        let path = format!("/channels/{channel_id}/messages");
        self.request(Method::POST, &path, &[], Some(&body), Auth::Required).await
    }

    /// `GET /channels/{c}/messages/{m}`.
    pub async fn get_message(&self, channel_id: &str, message_id: &str) -> Result<Value, RestError> {
        let path = message_path(channel_id, message_id)?;
        self.request(Method::GET, &path, &[], None, Auth::Required).await
    }

    /// `PATCH /channels/{c}/messages/{m}`; returns the updated message.
    pub async fn edit_message(
        &self,
        channel_id: &str,
        message_id: &str,
        edit: &EditMessageRequest,
    ) -> Result<Value, RestError> {
        let path = message_path(channel_id, message_id)?;
        let body = serde_json::to_value(edit).map_err(|e| RestError::Decode(e.to_string()))?;
        self.request(Method::PATCH, &path, &[], Some(&body), Auth::Required).await
    }

    /// `PATCH /channels/{c}/messages/{m}` with `{flags}` only
    /// (`requestMessageFlagsPatch`, used to toggle SUPPRESS_EMBEDS);
    /// returns the updated message.
    pub async fn set_message_flags(&self, channel_id: &str, message_id: &str, flags: u32) -> Result<Value, RestError> {
        let edit = EditMessageRequest { flags: Some(flags), ..EditMessageRequest::default() };
        self.edit_message(channel_id, message_id, &edit).await
    }

    /// `DELETE /channels/{c}/messages/{m}` (204).
    pub async fn delete_message(&self, channel_id: &str, message_id: &str) -> Result<(), RestError> {
        let path = message_path(channel_id, message_id)?;
        self.request(Method::DELETE, &path, &[], None, Auth::Required).await?;
        Ok(())
    }

    /// `GET /channels/{c}/messages/pins` → `{items: [{message, pinned_at}], has_more}`.
    pub async fn get_pins(&self, channel_id: &str, query: &PinsQuery) -> Result<Value, RestError> {
        check_snowflake(channel_id, "channel_id")?;
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(limit) = query.limit {
            if !(1..=50).contains(&limit) {
                return Err(RestError::InvalidArgument("pins limit must be 1..=50"));
            }
            params.push(("limit", limit.to_string()));
        }
        if let Some(before) = &query.before {
            if before.is_empty() || before.len() > 64 {
                return Err(RestError::InvalidArgument("pins before"));
            }
            params.push(("before", before.clone()));
        }
        let path = format!("/channels/{channel_id}/messages/pins");
        self.request(Method::GET, &path, &params, None, Auth::Required).await
    }

    /// `PUT /channels/{c}/pins/{m}` (204).
    pub async fn pin_message(&self, channel_id: &str, message_id: &str) -> Result<(), RestError> {
        let path = pin_path(channel_id, message_id)?;
        self.request(Method::PUT, &path, &[], None, Auth::Required).await?;
        Ok(())
    }

    /// `DELETE /channels/{c}/pins/{m}` (204).
    pub async fn unpin_message(&self, channel_id: &str, message_id: &str) -> Result<(), RestError> {
        let path = pin_path(channel_id, message_id)?;
        self.request(Method::DELETE, &path, &[], None, Auth::Required).await?;
        Ok(())
    }

    /// `POST /channels/{c}/messages/purge` -> `{ "deleted_count": N }`.
    pub async fn purge_personal_notes_messages(&self, channel_id: &str) -> Result<u64, RestError> {
        check_snowflake(channel_id, "channel_id")?;
        let path = format!("/channels/{channel_id}/messages/purge");
        let body: Value = self.request(Method::POST, &path, &[], None, Auth::Required).await?;
        Ok(body.get("deleted_count").and_then(Value::as_u64).unwrap_or(0))
    }
}

/// Validates and serialises a [`SendMessageRequest`].
pub(in crate::rest) fn message_body(message: &SendMessageRequest) -> Result<Value, RestError> {
    if message.nonce.is_empty() || message.nonce.chars().count() > 32 {
        return Err(RestError::InvalidArgument("nonce must be 1..=32 chars"));
    }
    if let Some(reference) = &message.message_reference {
        check_snowflake(&reference.message_id, "message_reference.message_id")?;
        for id in [&reference.channel_id, &reference.guild_id].into_iter().flatten() {
            check_snowflake(id, "message_reference id")?;
        }
    }
    serde_json::to_value(message).map_err(|e| RestError::Decode(e.to_string()))
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

pub(super) fn message_path(channel_id: &str, message_id: &str) -> Result<String, RestError> {
    check_snowflake(channel_id, "channel_id")?;
    check_snowflake(message_id, "message_id")?;
    Ok(format!("/channels/{channel_id}/messages/{message_id}"))
}

fn pin_path(channel_id: &str, message_id: &str) -> Result<String, RestError> {
    check_snowflake(channel_id, "channel_id")?;
    check_snowflake(message_id, "message_id")?;
    Ok(format!("/channels/{channel_id}/pins/{message_id}"))
}
