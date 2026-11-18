//! User and per-guild settings. Route: `fluxer_api/src/api/user/controllers/UserAccountController.ts`
//! (`PATCH /users/@me/settings`, `PATCH /users/@me/guilds/{g|@me}/settings`).
//! Every field is optional: the server treats both bodies as partial updates.

use std::collections::BTreeMap;

use reqwest::Method;
use serde::Serialize;
use serde_json::Value;

use super::check_snowflake;
use crate::rest::client::{Auth, RestClient};
use crate::rest::error::RestError;

/// `UserSettingsUpdateRequest.custom_status` (`CustomStatusPayload`). The
/// server requires `text` 1..=128 chars, `expires_at` in the future and
/// ignores `emoji_name` when `emoji_id` is set.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct CustomStatus {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// ISO 8601 timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emoji_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emoji_name: Option<String>,
}

/// Subset of `UserSettingsUpdateRequest`. `None` omits a field;
/// `custom_status: Some(None)` sends `null` to clear it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct UserSettingsPatch {
    /// `online` | `dnd` | `idle` | `invisible`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_status: Option<Option<CustomStatus>>,
    /// `dark` | `dark_legacy` | `coal` | `light` | `system`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    /// Locale code such as `en-US`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    /// Ordered guild IDs of the sidebar.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guild_positions: Option<Vec<String>>,
}

/// `MuteConfigSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MuteConfig {
    /// ISO 8601 end time; `None` serializes as `null` (muted until unmuted).
    pub end_time: Option<String>,
    /// Selected duration in seconds (the web client's `selected_time_window`).
    pub selected_time_window: i64,
}

/// `ChannelOverrideSchema` (all fields optional).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ChannelOverride {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collapsed: Option<bool>,
    /// 0 all, 1 mentions only, 2 none, 3 inherit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_notifications: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub muted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mute_config: Option<Option<MuteConfig>>,
}

/// Subset of `UserGuildSettingsUpdateRequest`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct GuildSettingsPatch {
    /// 0 all, 1 mentions only, 2 none, 3 inherit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_notifications: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub muted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mute_config: Option<Option<MuteConfig>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suppress_everyone: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suppress_roles: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hide_muted_channels: Option<bool>,
    /// Channel ID to override; `Some(None)` sends `null` (clears all).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_overrides: Option<Option<BTreeMap<String, ChannelOverride>>>,
}

impl RestClient {
    /// `PATCH /users/@me/settings` → `UserSettingsResponse`.
    pub async fn update_user_settings(&self, patch: &UserSettingsPatch) -> Result<Value, RestError> {
        if let Some(Some(status)) = &patch.custom_status {
            if let Some(id) = &status.emoji_id {
                check_snowflake(id, "emoji_id")?;
            }
        }
        for id in patch.guild_positions.iter().flatten() {
            check_snowflake(id, "guild_positions")?;
        }
        let body = to_body(patch)?;
        self.request(Method::PATCH, "/users/@me/settings", &[], Some(&body), Auth::Required).await
    }

    /// `PATCH /users/@me/guilds/{guild_id}/settings` → `UserGuildSettingsResponse`.
    pub async fn update_guild_settings(&self, guild_id: &str, patch: &GuildSettingsPatch) -> Result<Value, RestError> {
        check_snowflake(guild_id, "guild_id")?;
        let path = format!("/users/@me/guilds/{guild_id}/settings");
        self.patch_guild_settings(&path, patch).await
    }

    /// `PATCH /users/@me/guilds/@me/settings`: the same settings for DMs and group DMs.
    pub async fn update_dm_settings(&self, patch: &GuildSettingsPatch) -> Result<Value, RestError> {
        self.patch_guild_settings("/users/@me/guilds/@me/settings", patch).await
    }

    async fn patch_guild_settings(&self, path: &str, patch: &GuildSettingsPatch) -> Result<Value, RestError> {
        if let Some(Some(overrides)) = &patch.channel_overrides {
            for id in overrides.keys() {
                check_snowflake(id, "channel_overrides key")?;
            }
        }
        let body = to_body(patch)?;
        self.request(Method::PATCH, path, &[], Some(&body), Auth::Required).await
    }
}

fn to_body(patch: &impl Serialize) -> Result<Value, RestError> {
    serde_json::to_value(patch).map_err(|_| RestError::InvalidArgument("settings body"))
}
