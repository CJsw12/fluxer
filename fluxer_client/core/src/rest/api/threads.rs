//! Thread endpoints (`ThreadController.ts`, `ThreadCommands.ts`).

use reqwest::Method;
use serde::Serialize;
use serde_json::Value;

use super::check_snowflake;
use crate::rest::client::{Auth, RestClient};
use crate::rest::error::RestError;

/// `THREAD_NAME_MAX_LENGTH` in `ThreadConstants.ts`.
pub const MAX_THREAD_NAME_UTF16: usize = 100;
/// `AUTO_ARCHIVE_DURATIONS` in minutes (`ThreadConstants.ts`).
pub const THREAD_AUTO_ARCHIVE_DURATIONS: &[u32] = &[60, 1440, 4320, 10080];

/// `POST /channels/{channel_id}/messages/{message_id}/threads` body
/// (`StartThreadFromMessageRequest`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CreateThreadFromMessageRequest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_archive_duration: Option<u32>,
}

/// Validates and trims a thread name (`1..=100` UTF-16 units).
pub fn validate_thread_name(name: &str) -> Result<String, RestError> {
    let trimmed = name.trim();
    let utf16_len = trimmed.encode_utf16().count();
    if trimmed.is_empty() || utf16_len > MAX_THREAD_NAME_UTF16 {
        return Err(RestError::InvalidArgument("thread name must be 1..=100 UTF-16 units"));
    }
    Ok(trimmed.to_owned())
}

impl RestClient {
    /// `POST /channels/{channel_id}/messages/{message_id}/threads` -> thread channel object.
    pub async fn create_thread_from_message(
        &self,
        channel_id: &str,
        message_id: &str,
        req: &CreateThreadFromMessageRequest,
    ) -> Result<Value, RestError> {
        check_snowflake(channel_id, "channel_id")?;
        check_snowflake(message_id, "message_id")?;
        let name = validate_thread_name(&req.name)?;
        if let Some(duration) = req.auto_archive_duration
            && !THREAD_AUTO_ARCHIVE_DURATIONS.contains(&duration)
        {
            return Err(RestError::InvalidArgument("invalid thread auto_archive_duration"));
        }
        let body = serde_json::to_value(CreateThreadFromMessageRequest {
            name,
            auto_archive_duration: req.auto_archive_duration,
        })
        .map_err(|e| RestError::Decode(e.to_string()))?;
        let path = format!("/channels/{channel_id}/messages/{message_id}/threads");
        self.request(Method::POST, &path, &[], Some(&body), Auth::Required)
            .await
    }
}
