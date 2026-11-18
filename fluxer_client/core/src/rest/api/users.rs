//! User and DM reads. Routes: `fluxer_api/src/api/user/
//! controllers/{UserAccountController,UserChannelController}.ts`.
//! Guild members are in `guilds.rs`.

use reqwest::Method;
use serde_json::{Value, json};

use super::check_snowflake;
use crate::rest::client::{Auth, RestClient};
use crate::rest::error::RestError;

/// `GET /users/{id}/profile` query (`UserProfileQueryRequest`). The
/// default matches `UserProfileCommands.tsx::profileQuery` (both mutual
/// lists requested).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileQuery {
    /// Guild-specific profile (nickname, guild avatar, roles).
    pub guild_id: Option<String>,
    pub with_mutual_friends: bool,
    pub with_mutual_guilds: bool,
}

impl Default for ProfileQuery {
    fn default() -> Self {
        Self { guild_id: None, with_mutual_friends: true, with_mutual_guilds: true }
    }
}


impl RestClient {
    /// `GET /users/@me` (also the token validity check: 401 = invalid).
    pub async fn get_me(&self) -> Result<Value, RestError> {
        self.request(Method::GET, "/users/@me", &[], None, Auth::Required).await
    }

    /// `GET /users/{id}/profile` → `UserProfileFullResponse`. `user_id` is a
    /// snowflake or `@me`.
    pub async fn get_user_profile(&self, user_id: &str, query: &ProfileQuery) -> Result<Value, RestError> {
        if user_id != "@me" {
            check_snowflake(user_id, "user_id")?;
        }
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(guild_id) = &query.guild_id {
            check_snowflake(guild_id, "guild_id")?;
            params.push(("guild_id", guild_id.clone()));
        }
        params.push(("with_mutual_friends", query.with_mutual_friends.to_string()));
        params.push(("with_mutual_guilds", query.with_mutual_guilds.to_string()));
        let path = format!("/users/{user_id}/profile");
        self.request(Method::GET, &path, &params, None, Auth::Required).await
    }

    /// `GET /users/@me/channels` → open DM / group DM channels.
    pub async fn get_private_channels(&self) -> Result<Value, RestError> {
        self.request(Method::GET, "/users/@me/channels", &[], None, Auth::Required).await
    }

    /// `POST /users/@me/channels` with `{recipient_id}`: opens (or returns
    /// the existing) DM with one user (`PrivateChannelCommands.ts`).
    pub async fn open_dm(&self, recipient_id: &str) -> Result<Value, RestError> {
        check_snowflake(recipient_id, "recipient_id")?;
        let body = json!({ "recipient_id": recipient_id });
        self.request(Method::POST, "/users/@me/channels", &[], Some(&body), Auth::Required).await
    }
}
