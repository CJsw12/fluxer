//! Linked external account connections (`GET /users/@me/connections`,
//! `PATCH /users/@me/connections/{type}/{connection_id}`,
//! `POST /users/@me/connections/reorder`).
//! Schema: `packages/schema/src/domains/connection/ConnectionSchemas.ts`.

use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::rest::client::{Auth, RestClient};
use crate::rest::error::RestError;

/// Maximum connections allowed per user (`MAX_CONNECTIONS_PER_USER`).
pub const MAX_CONNECTIONS_PER_USER: usize = 20;

/// Bitfield controlling who can see a linked connection (`ConnectionVisibilityFlags`).
pub const CONNECTION_VISIBILITY_EVERYONE: i32 = 1 << 0;
pub const CONNECTION_VISIBILITY_FRIENDS: i32 = 1 << 1;
pub const CONNECTION_VISIBILITY_MUTUAL_GUILDS: i32 = 1 << 2;

/// One linked external account or domain (`ConnectionResponse`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct UserConnection {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub visibility_flags: i32,
    #[serde(default)]
    pub sort_order: i32,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

/// Non-destructive update body for `PATCH /users/@me/connections/{type}/{connection_id}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ConnectionPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility_flags: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_order: Option<i32>,
}

fn check_path_segment(value: &str, what: &'static str) -> Result<(), RestError> {
    if value.is_empty()
        || value.len() > 128
        || value.contains('/')
        || value.contains('\\')
        || value.contains("..")
        || value.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err(RestError::InvalidArgument(what));
    }
    Ok(())
}

impl RestClient {
    /// `GET /users/@me/connections` -> `Vec<UserConnection>`.
    pub async fn list_connections(&self) -> Result<Vec<UserConnection>, RestError> {
        let value = self
            .request(Method::GET, "/users/@me/connections", &[], None, Auth::Required)
            .await?;
        serde_json::from_value(value).map_err(|e| RestError::Decode(format!("connections list: {e}")))
    }

    /// `PATCH /users/@me/connections/{type}/{connection_id}` -> `UserConnection`.
    pub async fn update_connection(
        &self,
        kind: &str,
        connection_id: &str,
        patch: &ConnectionPatch,
    ) -> Result<UserConnection, RestError> {
        check_path_segment(kind, "connection type")?;
        check_path_segment(connection_id, "connection_id")?;
        let body = serde_json::to_value(patch).map_err(|_| RestError::InvalidArgument("connection patch"))?;
        let path = format!("/users/@me/connections/{kind}/{connection_id}");
        let value = self.request(Method::PATCH, &path, &[], Some(&body), Auth::Required).await?;
        serde_json::from_value(value).map_err(|e| RestError::Decode(format!("connection: {e}")))
    }

    /// `POST /users/@me/connections/reorder` with `{ connection_ids }`.
    pub async fn reorder_connections(&self, connection_ids: &[String]) -> Result<(), RestError> {
        if connection_ids.is_empty() || connection_ids.len() > MAX_CONNECTIONS_PER_USER {
            return Err(RestError::InvalidArgument("connection_ids"));
        }
        for id in connection_ids {
            check_path_segment(id, "connection_id")?;
        }
        let body = json!({ "connection_ids": connection_ids });
        self.request(Method::POST, "/users/@me/connections/reorder", &[], Some(&body), Auth::Required)
            .await?;
        Ok(())
    }
}
