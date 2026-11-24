//! Leaving a guild. Route: `fluxer_api/src/api/guild/controllers/GuildBaseController.ts`
//! (`leave_guild`); web caller: `GuildCommands.ts::leave`.

use reqwest::Method;
use serde_json::{Value, json};

use super::check_snowflake;
use crate::rest::client::{Auth, RestClient};
use crate::rest::error::RestError;

impl RestClient {
    /// `DELETE /users/@me/guilds/{g}` (204), with `?delete_messages=true`
    /// when requested. The server requires sudo verification only for the
    /// `delete_messages` path; this sends no sudo body, so that path fails
    /// with a sudo error until the caller adds sudo support.
    pub async fn leave_guild(&self, guild_id: &str, delete_messages: bool) -> Result<(), RestError> {
        check_snowflake(guild_id, "guild_id")?;
        let path = format!("/users/@me/guilds/{guild_id}");
        let query: Vec<(&str, String)> =
            if delete_messages { vec![("delete_messages", "true".to_owned())] } else { Vec::new() };
        let _: Value = self.request(Method::DELETE, &path, &query, None, Auth::Required).await?;
        Ok(())
    }

    /// `POST /users/@me/guilds/{g}/messages/bulk-delete-mine` (202 Accepted).
    /// Sends optional account password for sudo verification when provided.
    pub async fn bulk_delete_my_guild_messages(
        &self,
        guild_id: &str,
        password: Option<&str>,
    ) -> Result<(), RestError> {
        check_snowflake(guild_id, "guild_id")?;
        let body = match password.map(str::trim).filter(|p| !p.is_empty()) {
            Some(pwd) => json!({ "password": pwd }),
            None => json!({}),
        };
        let path = format!("/users/@me/guilds/{guild_id}/messages/bulk-delete-mine");
        let _: Value = self
            .request(Method::POST, &path, &[], Some(&body), Auth::Required)
            .await?;
        Ok(())
    }
}
