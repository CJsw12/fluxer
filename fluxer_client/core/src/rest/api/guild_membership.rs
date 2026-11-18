//! Leaving a guild. Route: `fluxer_api/src/api/guild/controllers/GuildBaseController.ts`
//! (`leave_guild`); web caller: `GuildCommands.ts::leave`.

use reqwest::Method;
use serde_json::Value;

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
}
