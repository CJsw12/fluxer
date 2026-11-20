//! Guild role and channel permission writes.

use reqwest::Method;
use serde::Serialize;
use serde_json::{Value, json};

use crate::rest::client::{Auth, RestClient};
use crate::rest::error::RestError;
use crate::state::{PermissionOverwrite, Role, Snowflake};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RolePatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hoist: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mentionable: Option<bool>,
}

pub fn validate_role_name(name: &str) -> Result<(), RestError> {
    if (1..=100).contains(&name.trim().encode_utf16().count()) {
        Ok(())
    } else {
        Err(RestError::InvalidArgument("name"))
    }
}

impl RestClient {
    pub async fn list_guild_roles(&self, guild: Snowflake) -> Result<Vec<Role>, RestError> {
        let path = format!("/guilds/{guild}/roles");
        let response = self.request(Method::GET, &path, &[], None, Auth::Required).await?;
        serde_json::from_value(response).map_err(|error| RestError::Decode(error.to_string()))
    }

    pub async fn create_guild_role(&self, guild: Snowflake, name: &str) -> Result<Role, RestError> {
        validate_role_name(name)?;
        let path = format!("/guilds/{guild}/roles");
        let body = json!({ "name": name.trim() });
        let response = self
            .request(Method::POST, &path, &[], Some(&body), Auth::Required)
            .await?;
        serde_json::from_value(response).map_err(|error| RestError::Decode(error.to_string()))
    }

    pub async fn update_guild_role(
        &self,
        guild: Snowflake,
        role: Snowflake,
        patch: &RolePatch,
    ) -> Result<Role, RestError> {
        if let Some(name) = &patch.name {
            validate_role_name(name)?;
        }
        let mut body = serde_json::to_value(patch).map_err(|error| RestError::Decode(error.to_string()))?;
        if let Some(name) = &patch.name {
            body["name"] = json!(name.trim());
        }
        if let Some(permissions) = patch.permissions {
            body["permissions"] = json!(permissions.to_string());
        }
        let path = format!("/guilds/{guild}/roles/{role}");
        let response = self
            .request(Method::PATCH, &path, &[], Some(&body), Auth::Required)
            .await?;
        let updated: Role =
            serde_json::from_value(response).map_err(|error| RestError::Decode(error.to_string()))?;
        if updated.id != role {
            return Err(RestError::Decode("role ID did not match the request".to_owned()));
        }
        Ok(updated)
    }

    pub async fn delete_guild_role(&self, guild: Snowflake, role: Snowflake) -> Result<(), RestError> {
        if role == guild {
            return Err(RestError::InvalidArgument("role_id"));
        }
        let path = format!("/guilds/{guild}/roles/{role}");
        self.request(Method::DELETE, &path, &[], None, Auth::Required)
            .await
            .map(|_| ())
    }

    pub async fn reorder_guild_roles(
        &self,
        guild: Snowflake,
        positions: &[(Snowflake, i32)],
    ) -> Result<(), RestError> {
        if positions.is_empty() || positions.len() > 10_000 || positions.iter().any(|(id, _)| *id == guild) {
            return Err(RestError::InvalidArgument("positions"));
        }
        let path = format!("/guilds/{guild}/roles");
        let body: Value = positions
            .iter()
            .map(|(id, position)| json!({ "id": id, "position": position }))
            .collect();
        self.request(Method::PATCH, &path, &[], Some(&body), Auth::Required)
            .await
            .map(|_| ())
    }

    pub async fn get_guild_channel(&self, channel: Snowflake) -> Result<Value, RestError> {
        let path = format!("/channels/{channel}");
        self.request(Method::GET, &path, &[], None, Auth::Required).await
    }

    pub async fn set_channel_overwrite(
        &self,
        channel: Snowflake,
        overwrite: &PermissionOverwrite,
    ) -> Result<(), RestError> {
        if overwrite.kind > 1 || overwrite.allow & overwrite.deny != 0 {
            return Err(RestError::InvalidArgument("permission_overwrite"));
        }
        let path = format!("/channels/{channel}/permissions/{}", overwrite.id);
        let body = json!({ "type": overwrite.kind, "allow": overwrite.allow.to_string(), "deny": overwrite.deny.to_string() });
        self.request(Method::PUT, &path, &[], Some(&body), Auth::Required)
            .await
            .map(|_| ())
    }

    pub async fn delete_channel_overwrite(&self, channel: Snowflake, target: Snowflake) -> Result<(), RestError> {
        let path = format!("/channels/{channel}/permissions/{target}");
        self.request(Method::DELETE, &path, &[], None, Auth::Required)
            .await
            .map(|_| ())
    }

    pub async fn move_channel_and_sync(
        &self,
        guild: Snowflake,
        channel: Snowflake,
        parent: Snowflake,
    ) -> Result<(), RestError> {
        let path = format!("/guilds/{guild}/channels");
        let body = json!([{ "id": channel, "parent_id": parent, "lock_permissions": true }]);
        self.request(Method::PATCH, &path, &[], Some(&body), Auth::Required)
            .await
            .map(|_| ())
    }
}
