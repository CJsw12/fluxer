//! Guild reads. Route: `fluxer_api/src/api/guild/controllers/GuildMemberController.ts`.
//!
//! The web client does not page members over REST: the member sidebar uses
//! gateway `LAZY_REQUEST` and lookups use gateway `REQUEST_GUILD_MEMBERS`
//! (`GatewaySocket.ts`). [`RestClient::list_guild_members`] exists for
//! tooling and small guilds.

use reqwest::Method;
use serde_json::Value;

use super::check_snowflake;
use crate::rest::client::{Auth, RestClient};
use crate::rest::error::RestError;

/// `GET /guilds/{g}/members` query (`GuildMemberListQuery`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemberQuery {
    /// 1..=1000; the server default is 1.
    pub limit: Option<u16>,
    /// User ID cursor: members after this user.
    pub after: Option<String>,
}

impl RestClient {
    /// `GET /guilds/{g}/members` → `GuildMemberResponse[]`, paged with an `after` user ID cursor.
    pub async fn list_guild_members(&self, guild_id: &str, query: &MemberQuery) -> Result<Value, RestError> {
        check_snowflake(guild_id, "guild_id")?;
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(limit) = query.limit {
            if !(1..=1000).contains(&limit) {
                return Err(RestError::InvalidArgument("member limit must be 1..=1000"));
            }
            params.push(("limit", limit.to_string()));
        }
        if let Some(after) = &query.after {
            check_snowflake(after, "after")?;
            params.push(("after", after.clone()));
        }
        let path = format!("/guilds/{guild_id}/members");
        self.request(Method::GET, &path, &params, None, Auth::Required).await
    }
}
