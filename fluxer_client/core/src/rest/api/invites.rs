//! Invite endpoints. Route: `fluxer_api/src/api/invite/InviteController.ts`.

use reqwest::Method;
use serde_json::{Map, Value, json};

use super::{check_snowflake, encode_uri_component};
use crate::rest::client::{Auth, RestClient};
use crate::rest::error::RestError;

/// Limits like `MAX_INVITE_USES` are enforced by the server; only obviously
/// bad input is rejected here.
const MAX_INVITE_CODE_BYTES: usize = 64;

/// `ChannelInviteCreateRequest`. `None` fields are omitted so the server
/// defaults apply (0 = unlimited / never, `unique` false, `temporary` false).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CreateInviteRequest {
    /// Seconds before expiry; 0 for never.
    pub max_age: Option<u32>,
    /// Maximum uses; 0 for unlimited.
    pub max_uses: Option<u32>,
    pub temporary: Option<bool>,
    /// Create a new invite instead of reusing an existing one.
    pub unique: Option<bool>,
}

fn invite_path(code: &str) -> Result<String, RestError> {
    if code.is_empty() || code.len() > MAX_INVITE_CODE_BYTES || code.chars().any(char::is_control) {
        return Err(RestError::InvalidArgument("invite code"));
    }
    Ok(format!("/invites/{}", encode_uri_component(code)))
}

impl RestClient {
    /// `GET /invites/{code}` → invite with `approximate_member_count` /
    /// `approximate_presence_count` for guild invites.
    pub async fn get_invite(&self, code: &str) -> Result<Value, RestError> {
        let path = invite_path(code)?;
        self.request(Method::GET, &path, &[], None, Auth::Required).await
    }

    /// `POST /invites/{code}` (no body) → the invite; the guild arrives over
    /// the gateway as `GUILD_CREATE`.
    pub async fn accept_invite(&self, code: &str) -> Result<Value, RestError> {
        let path = invite_path(code)?;
        self.request(Method::POST, &path, &[], None, Auth::Required).await
    }

    /// `POST /channels/{c}/invites` → `InviteMetadataResponse`.
    pub async fn create_channel_invite(
        &self,
        channel_id: &str,
        request: &CreateInviteRequest,
    ) -> Result<Value, RestError> {
        check_snowflake(channel_id, "channel_id")?;
        let mut body = Map::new();
        if let Some(v) = request.max_age {
            body.insert("max_age".into(), json!(v));
        }
        if let Some(v) = request.max_uses {
            body.insert("max_uses".into(), json!(v));
        }
        if let Some(v) = request.temporary {
            body.insert("temporary".into(), json!(v));
        }
        if let Some(v) = request.unique {
            body.insert("unique".into(), json!(v));
        }
        let path = format!("/channels/{channel_id}/invites");
        self.request(Method::POST, &path, &[], Some(&Value::Object(body)), Auth::Required).await
    }

    /// `DELETE /invites/{code}` (204).
    pub async fn delete_invite(&self, code: &str) -> Result<(), RestError> {
        let path = invite_path(code)?;
        self.request(Method::DELETE, &path, &[], None, Auth::Required).await?;
        Ok(())
    }

    /// `GET /guilds/{g}/invites` → `InviteMetadataResponse[]`.
    pub async fn list_guild_invites(&self, guild_id: &str) -> Result<Value, RestError> {
        check_snowflake(guild_id, "guild_id")?;
        let path = format!("/guilds/{guild_id}/invites");
        self.request(Method::GET, &path, &[], None, Auth::Required).await
    }

    /// `GET /channels/{c}/invites` → `InviteMetadataResponse[]`.
    pub async fn list_channel_invites(&self, channel_id: &str) -> Result<Value, RestError> {
        check_snowflake(channel_id, "channel_id")?;
        let path = format!("/channels/{channel_id}/invites");
        self.request(Method::GET, &path, &[], None, Auth::Required).await
    }
}
