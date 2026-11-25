//! Community discovery endpoints (`fluxer_api/src/api/guild/controllers/GuildDiscoveryController.ts`).

use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::check_snowflake;
use crate::rest::client::{Auth, RestClient};
use crate::rest::error::RestError;

pub const MAX_DISCOVERY_QUERY_CHARS: usize = 100;
pub const DEFAULT_DISCOVERY_LIMIT: u32 = 36;
pub const MAX_DISCOVERY_LIMIT: u32 = 48;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct DiscoveryCategory {
    pub id: u32,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct DiscoveryCategoryCount {
    pub category_type: u32,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct DiscoveryGuild {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub banner: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub category_type: u32,
    #[serde(default)]
    pub primary_language: Option<String>,
    #[serde(default)]
    pub custom_tags: Vec<String>,
    #[serde(default)]
    pub member_count: u64,
    #[serde(default)]
    pub online_count: u64,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub badge_ids: Vec<String>,
    #[serde(default)]
    pub verification_level: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct DiscoveryGuildList {
    #[serde(default)]
    pub guilds: Vec<DiscoveryGuild>,
    #[serde(default)]
    pub total: u64,
    #[serde(default)]
    pub category_counts: Vec<DiscoveryCategoryCount>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiscoveryGuildQuery {
    pub query: Option<String>,
    pub category: Option<u32>,
    pub language: Option<String>,
    pub tag: Option<String>,
    pub sort_by: Option<String>,
    pub limit: u32,
    pub offset: u32,
}

impl RestClient {
    /// `GET /discovery/categories`.
    pub async fn list_discovery_categories(&self) -> Result<Vec<DiscoveryCategory>, RestError> {
        let val = self
            .request(Method::GET, "/discovery/categories", &[], None, Auth::Required)
            .await?;
        serde_json::from_value(val).map_err(|err| RestError::Decode(err.to_string()))
    }

    /// `GET /discovery/guilds`.
    pub async fn search_discovery_guilds(&self, params: &DiscoveryGuildQuery) -> Result<DiscoveryGuildList, RestError> {
        let limit = if params.limit == 0 {
            DEFAULT_DISCOVERY_LIMIT
        } else {
            params.limit.clamp(1, MAX_DISCOVERY_LIMIT)
        };
        let mut query: Vec<(&str, String)> = vec![
            ("limit", limit.to_string()),
            ("offset", params.offset.to_string()),
        ];
        if let Some(q) = params.query.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            if q.chars().count() > MAX_DISCOVERY_QUERY_CHARS {
                return Err(RestError::InvalidArgument("query"));
            }
            query.push(("query", q.to_owned()));
        }
        if let Some(category) = params.category {
            query.push(("category", category.to_string()));
        }
        if let Some(language) = params.language.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            query.push(("language", language.to_owned()));
        }
        if let Some(tag) = params.tag.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            query.push(("tag", tag.to_owned()));
        }
        if let Some(sort_by) = params.sort_by.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            query.push(("sort_by", sort_by.to_owned()));
        }
        let val = self
            .request(Method::GET, "/discovery/guilds", &query, None, Auth::Required)
            .await?;
        serde_json::from_value(val).map_err(|err| RestError::Decode(err.to_string()))
    }

    /// `POST /discovery/guilds/{guild_id}/join` (204).
    pub async fn join_discovery_guild(&self, guild_id: &str) -> Result<(), RestError> {
        check_snowflake(guild_id, "guild_id")?;
        let path = format!("/discovery/guilds/{guild_id}/join");
        let _: Value = self
            .request(Method::POST, &path, &[], None, Auth::Required)
            .await?;
        Ok(())
    }
}
