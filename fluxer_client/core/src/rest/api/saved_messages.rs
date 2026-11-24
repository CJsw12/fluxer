//! Saved messages (bookmarks) endpoints (`UserContentController.ts`,
//! `SavedMessageCommands.tsx`).

use reqwest::Method;
use serde_json::{Value, json};

use super::check_snowflake;
use crate::rest::client::{Auth, RestClient};
use crate::rest::error::RestError;

/// `GET /users/@me/saved-messages` query (`UserSavedMessagesQueryRequest`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedMessagesQuery {
    /// 1..=50.
    pub limit: Option<u8>,
    /// Message ID cursor.
    pub before: Option<String>,
}

impl RestClient {
    /// `GET /users/@me/saved-messages` -> array of `{ id, channel_id, message_id, status, message }`.
    pub async fn list_saved_messages(&self, query: &SavedMessagesQuery) -> Result<Value, RestError> {
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(limit) = query.limit {
            if !(1..=50).contains(&limit) {
                return Err(RestError::InvalidArgument("saved messages limit must be 1..=50"));
            }
            params.push(("limit", limit.to_string()));
        }
        if let Some(before) = &query.before {
            check_snowflake(before, "before")?;
            params.push(("before", before.clone()));
        }
        self.request(Method::GET, "/users/@me/saved-messages", &params, None, Auth::Required)
            .await
    }

    /// `POST /users/@me/saved-messages` (204) with `{ channel_id, message_id }`.
    pub async fn save_message(&self, channel_id: &str, message_id: &str) -> Result<(), RestError> {
        check_snowflake(channel_id, "channel_id")?;
        check_snowflake(message_id, "message_id")?;
        let body = json!({
            "channel_id": channel_id,
            "message_id": message_id,
        });
        self.request(Method::POST, "/users/@me/saved-messages", &[], Some(&body), Auth::Required)
            .await?;
        Ok(())
    }

    /// `DELETE /users/@me/saved-messages/{message_id}` (204).
    pub async fn unsave_message(&self, message_id: &str) -> Result<(), RestError> {
        check_snowflake(message_id, "message_id")?;
        let path = format!("/users/@me/saved-messages/{message_id}");
        self.request(Method::DELETE, &path, &[], None, Auth::Required)
            .await?;
        Ok(())
    }
}
