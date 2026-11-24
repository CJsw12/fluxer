//! Private channel & group DM + guild channel management endpoints.
//! Routes: `fluxer_api/src/api/user/controllers/UserChannelController.ts`
//! (`POST /users/@me/channels`), `GuildChannelController.ts`
//! (`POST /guilds/{guild_id}/channels`), and `ChannelController.ts`
//! (`PATCH/DELETE /channels/{id}`, `PUT/DELETE /channels/{id}/recipients/{user_id}`).

use std::collections::BTreeSet;

use reqwest::Method;
use serde::Serialize;
use serde_json::{Value, json};

use super::check_snowflake;
use crate::rest::client::{Auth, RestClient};
use crate::rest::error::RestError;
use crate::state::ChannelKind;

/// `MAX_GROUP_DM_OTHER_RECIPIENTS` (`packages/constants/src/LimitConstants.ts:68`).
pub const MAX_GROUP_DM_OTHER_RECIPIENTS: usize = 49;
/// `GeneralChannelNameType` UTF-16 max length (`ChannelValidators.ts:109`).
pub const MAX_CHANNEL_NAME_UTF16: usize = 100;
/// `CHANNEL_TOPIC_MAX_LENGTH` (`LimitConstants.ts:27`).
pub const MAX_CHANNEL_TOPIC_UTF16: usize = 1024;
/// `CHANNEL_RATE_LIMIT_PER_USER_MAX` (`LimitConstants.ts:25`).
pub const MAX_CHANNEL_RATE_LIMIT_PER_USER: u32 = 21_600;
/// `VOICE_CHANNEL_BITRATE_DEFAULT` (`LimitConstants.ts:11`).
pub const VOICE_CHANNEL_BITRATE_DEFAULT: u32 = 64_000;
/// `VOICE_CHANNEL_CONNECTION_LIMIT_DEFAULT` (`LimitConstants.ts:20`).
pub const VOICE_CHANNEL_CONNECTION_LIMIT_DEFAULT: u32 = 5;

/// Request body for `POST /guilds/{guild_id}/channels`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CreateGuildChannelRequest {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: ChannelKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nsfw: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit_per_user: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bitrate: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice_connection_limit: Option<u32>,
}

/// Partial update body for `PATCH /channels/{channel_id}` on a guild channel.
/// Double-option fields emit explicit `null` when `Some(None)` (e.g. clearing a
/// topic or detaching a channel from its parent category).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct GuildChannelPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<ChannelKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nsfw: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nsfw_override: Option<Option<bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit_per_user: Option<u32>,
}

impl GuildChannelPatch {
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.kind.is_none()
            && self.topic.is_none()
            && self.parent_id.is_none()
            && self.nsfw.is_none()
            && self.nsfw_override.is_none()
            && self.rate_limit_per_user.is_none()
    }
}

fn check_trimmed_utf16(value: &str, max: usize, field: &'static str) -> Result<(), RestError> {
    let len = value.trim().encode_utf16().count();
    if len == 0 || len > max { Err(RestError::InvalidArgument(field)) } else { Ok(()) }
}

/// Validates a `POST /guilds/{guild_id}/channels` request body.
pub fn validate_create_guild_channel(req: &CreateGuildChannelRequest) -> Result<(), RestError> {
    if !matches!(
        req.kind,
        ChannelKind::GuildText | ChannelKind::GuildAnnouncement | ChannelKind::GuildVoice | ChannelKind::GuildCategory
    ) {
        return Err(RestError::InvalidArgument("type"));
    }
    check_trimmed_utf16(&req.name, MAX_CHANNEL_NAME_UTF16, "name")?;
    if let Some(pid) = &req.parent_id {
        if req.kind == ChannelKind::GuildCategory {
            return Err(RestError::InvalidArgument("parent_id"));
        }
        check_snowflake(pid, "parent_id")?;
    }
    if let Some(topic) = &req.topic {
        if matches!(req.kind, ChannelKind::GuildCategory | ChannelKind::GuildVoice) {
            return Err(RestError::InvalidArgument("topic"));
        }
        check_trimmed_utf16(topic, MAX_CHANNEL_TOPIC_UTF16, "topic")?;
    }
    if let Some(slowmode) = req.rate_limit_per_user
        && (matches!(req.kind, ChannelKind::GuildCategory | ChannelKind::GuildVoice)
            || slowmode > MAX_CHANNEL_RATE_LIMIT_PER_USER)
    {
        return Err(RestError::InvalidArgument("rate_limit_per_user"));
    }
    Ok(())
}

/// Validates a `PATCH /channels/{channel_id}` update body for a guild channel.
pub fn validate_guild_channel_patch(patch: &GuildChannelPatch) -> Result<(), RestError> {
    if patch.is_empty() {
        return Err(RestError::InvalidArgument("patch"));
    }
    if let Some(name) = &patch.name {
        check_trimmed_utf16(name, MAX_CHANNEL_NAME_UTF16, "name")?;
    }
    if let Some(kind) = patch.kind
        && !matches!(kind, ChannelKind::GuildText | ChannelKind::GuildAnnouncement)
    {
        return Err(RestError::InvalidArgument("type"));
    }
    if let Some(Some(topic)) = &patch.topic {
        check_trimmed_utf16(topic, MAX_CHANNEL_TOPIC_UTF16, "topic")?;
    }
    if let Some(Some(parent_id)) = &patch.parent_id {
        check_snowflake(parent_id, "parent_id")?;
    }
    if let Some(slowmode) = patch.rate_limit_per_user
        && slowmode > MAX_CHANNEL_RATE_LIMIT_PER_USER
    {
        return Err(RestError::InvalidArgument("rate_limit_per_user"));
    }
    Ok(())
}

/// Partial update body for `PATCH /channels/{channel_id}` on a group DM.
/// `name: Some(None)` sends `"name": null` to clear a custom name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct GroupDmPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<String>,
}

/// Validates initial recipient list for `POST /users/@me/channels`.
pub fn validate_group_dm_recipients(recipients: &[String]) -> Result<(), RestError> {
    if recipients.is_empty() || recipients.len() > MAX_GROUP_DM_OTHER_RECIPIENTS {
        return Err(RestError::InvalidArgument("recipients"));
    }
    let mut seen = BTreeSet::new();
    for id in recipients {
        check_snowflake(id, "recipients")?;
        if !seen.insert(id.as_str()) {
            return Err(RestError::InvalidArgument("recipients"));
        }
    }
    Ok(())
}

/// Validates a group DM `PATCH /channels/{channel_id}` update body.
pub fn validate_group_dm_patch(patch: &GroupDmPatch) -> Result<(), RestError> {
    if let Some(Some(name)) = &patch.name {
        let trimmed = name.trim();
        let len = trimmed.encode_utf16().count();
        if len == 0 || len > MAX_CHANNEL_NAME_UTF16 {
            return Err(RestError::InvalidArgument("name"));
        }
    }
    if let Some(owner_id) = &patch.owner_id {
        check_snowflake(owner_id, "owner_id")?;
    }
    Ok(())
}

impl RestClient {
    /// `POST /users/@me/channels` with `{ recipients }` -> `ChannelResponse` (Group DM).
    pub async fn create_group_dm(&self, recipients: &[String]) -> Result<Value, RestError> {
        validate_group_dm_recipients(recipients)?;
        let body = json!({ "recipients": recipients });
        self.request(Method::POST, "/users/@me/channels", &[], Some(&body), Auth::Required).await
    }

    /// `PATCH /channels/{channel_id}` -> updated `ChannelResponse`.
    pub async fn update_group_dm(&self, channel_id: &str, patch: &GroupDmPatch) -> Result<Value, RestError> {
        check_snowflake(channel_id, "channel_id")?;
        validate_group_dm_patch(patch)?;
        let body = serde_json::to_value(patch).map_err(|err| RestError::Decode(err.to_string()))?;
        let path = format!("/channels/{channel_id}");
        self.request(Method::PATCH, &path, &[], Some(&body), Auth::Required).await
    }

    /// `DELETE /channels/{channel_id}?silent=...` (204): closes/leaves a group DM.
    pub async fn leave_channel(&self, channel_id: &str, silent: bool) -> Result<(), RestError> {
        check_snowflake(channel_id, "channel_id")?;
        let path = format!("/channels/{channel_id}");
        let mut params = Vec::new();
        if silent {
            params.push(("silent", "true".to_owned()));
        }
        self.request(Method::DELETE, &path, &params, None, Auth::Required).await?;
        Ok(())
    }

    /// `PUT /channels/{channel_id}/recipients/{user_id}` (204): adds a recipient to a group DM.
    pub async fn add_group_dm_recipient(&self, channel_id: &str, user_id: &str) -> Result<(), RestError> {
        check_snowflake(channel_id, "channel_id")?;
        check_snowflake(user_id, "user_id")?;
        let path = format!("/channels/{channel_id}/recipients/{user_id}");
        self.request(Method::PUT, &path, &[], None, Auth::Required).await?;
        Ok(())
    }

    /// `DELETE /channels/{channel_id}/recipients/{user_id}?silent=...` (204): removes a recipient.
    pub async fn remove_group_dm_recipient(
        &self,
        channel_id: &str,
        user_id: &str,
        silent: bool,
    ) -> Result<(), RestError> {
        check_snowflake(channel_id, "channel_id")?;
        check_snowflake(user_id, "user_id")?;
        let path = format!("/channels/{channel_id}/recipients/{user_id}");
        let mut params = Vec::new();
        if silent {
            params.push(("silent", "true".to_owned()));
        }
        self.request(Method::DELETE, &path, &params, None, Auth::Required).await?;
        Ok(())
    }

    /// `POST /guilds/{guild_id}/channels` -> created `ChannelResponse`.
    pub async fn create_guild_channel(
        &self,
        guild_id: &str,
        req: &CreateGuildChannelRequest,
    ) -> Result<Value, RestError> {
        check_snowflake(guild_id, "guild_id")?;
        validate_create_guild_channel(req)?;
        let mut normalized = req.clone();
        normalized.name = normalized.name.trim().to_owned();
        normalized.topic = normalized.topic.as_deref().map(|t| t.trim().to_owned());
        let body = serde_json::to_value(&normalized).map_err(|err| RestError::Decode(err.to_string()))?;
        let path = format!("/guilds/{guild_id}/channels");
        self.request(Method::POST, &path, &[], Some(&body), Auth::Required).await
    }

    /// `PATCH /channels/{channel_id}` -> updated `ChannelResponse`.
    pub async fn update_guild_channel(
        &self,
        channel_id: &str,
        patch: &GuildChannelPatch,
    ) -> Result<Value, RestError> {
        check_snowflake(channel_id, "channel_id")?;
        validate_guild_channel_patch(patch)?;
        let mut normalized = patch.clone();
        normalized.name = normalized.name.as_deref().map(|n| n.trim().to_owned());
        if let Some(Some(topic)) = &normalized.topic {
            normalized.topic = Some(Some(topic.trim().to_owned()));
        }
        let body = serde_json::to_value(&normalized).map_err(|err| RestError::Decode(err.to_string()))?;
        let path = format!("/channels/{channel_id}");
        self.request(Method::PATCH, &path, &[], Some(&body), Auth::Required).await
    }

    /// `DELETE /channels/{channel_id}` (204): deletes a guild channel or category.
    pub async fn delete_channel(&self, channel_id: &str) -> Result<(), RestError> {
        self.leave_channel(channel_id, false).await
    }

    /// `POST /channels/{channel_id}/call/ring` (204): rings call recipients (or all DM members when `None`).
    pub async fn ring_call_recipients(&self, channel_id: &str, recipients: Option<&[String]>) -> Result<(), RestError> {
        self.post_call_ring(channel_id, "ring", recipients).await
    }

    /// `POST /channels/{channel_id}/call/stop-ringing` (204): cancels active call ringing.
    pub async fn stop_ringing_call_recipients(
        &self,
        channel_id: &str,
        recipients: Option<&[String]>,
    ) -> Result<(), RestError> {
        self.post_call_ring(channel_id, "stop-ringing", recipients).await
    }

    async fn post_call_ring(
        &self,
        channel_id: &str,
        action: &str,
        recipients: Option<&[String]>,
    ) -> Result<(), RestError> {
        check_snowflake(channel_id, "channel_id")?;
        if let Some(list) = recipients {
            if list.len() > 1000 {
                return Err(RestError::InvalidArgument("recipients"));
            }
            for id in list {
                check_snowflake(id, "recipients")?;
            }
        }
        let body = match recipients {
            Some(list) => json!({ "recipients": list }),
            None => json!({}),
        };
        let path = format!("/channels/{channel_id}/call/{action}");
        self.request(Method::POST, &path, &[], Some(&body), Auth::Required).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_create() -> CreateGuildChannelRequest {
        CreateGuildChannelRequest {
            name: "general".into(),
            kind: ChannelKind::GuildText,
            parent_id: None,
            topic: None,
            nsfw: None,
            rate_limit_per_user: None,
            bitrate: None,
            user_limit: None,
            voice_connection_limit: None,
        }
    }

    #[test]
    fn validates_recipients_and_patch_bounds() {
        assert!(validate_group_dm_recipients(&[]).is_err());
        assert!(validate_group_dm_recipients(&["100".into(), "100".into()]).is_err());
        assert!(validate_group_dm_recipients(&["abc".into()]).is_err());
        let too_many: Vec<String> = (1..=50).map(|n| n.to_string()).collect();
        assert!(validate_group_dm_recipients(&too_many).is_err());
        let max_ok: Vec<String> = (1..=MAX_GROUP_DM_OTHER_RECIPIENTS).map(|n| n.to_string()).collect();
        assert!(validate_group_dm_recipients(&max_ok).is_ok());

        let blank_name = GroupDmPatch { name: Some(Some("   ".into())), owner_id: None };
        assert!(validate_group_dm_patch(&blank_name).is_err());
        let long_utf16 = GroupDmPatch { name: Some(Some("😀".repeat(51))), owner_id: None };
        assert!(validate_group_dm_patch(&long_utf16).is_err());
        let clear_name = GroupDmPatch { name: Some(None), owner_id: None };
        assert!(validate_group_dm_patch(&clear_name).is_ok());
        let bad_owner = GroupDmPatch { name: None, owner_id: Some("bad-id".into()) };
        assert!(validate_group_dm_patch(&bad_owner).is_err());
    }

    #[test]
    fn validates_guild_channel_create_and_patch_bounds() {
        assert!(validate_create_guild_channel(&valid_create()).is_ok());

        // Name bounds (blank or >100 UTF-16 code units).
        let blank = CreateGuildChannelRequest { name: "   ".into(), ..valid_create() };
        assert!(matches!(validate_create_guild_channel(&blank), Err(RestError::InvalidArgument("name"))));
        let long_name = CreateGuildChannelRequest { name: "😀".repeat(51), ..valid_create() };
        assert!(matches!(validate_create_guild_channel(&long_name), Err(RestError::InvalidArgument("name"))));

        // Unsupported type (e.g. DM or thread).
        let bad_kind = CreateGuildChannelRequest { kind: ChannelKind::Dm, ..valid_create() };
        assert!(matches!(validate_create_guild_channel(&bad_kind), Err(RestError::InvalidArgument("type"))));

        // Nested category disallowed; bad parent snowflake disallowed.
        let nested_cat = CreateGuildChannelRequest {
            kind: ChannelKind::GuildCategory,
            parent_id: Some("100".into()),
            ..valid_create()
        };
        assert!(matches!(
            validate_create_guild_channel(&nested_cat),
            Err(RestError::InvalidArgument("parent_id"))
        ));
        let bad_parent = CreateGuildChannelRequest { parent_id: Some("not-a-sf".into()), ..valid_create() };
        assert!(matches!(
            validate_create_guild_channel(&bad_parent),
            Err(RestError::InvalidArgument("parent_id"))
        ));

        // Topic disallowed on category/voice, blank disallowed, >1024 UTF-16 disallowed.
        let cat_topic = CreateGuildChannelRequest {
            kind: ChannelKind::GuildCategory,
            topic: Some("hello".into()),
            ..valid_create()
        };
        assert!(matches!(validate_create_guild_channel(&cat_topic), Err(RestError::InvalidArgument("topic"))));
        let long_topic = CreateGuildChannelRequest { topic: Some("😀".repeat(513)), ..valid_create() };
        assert!(matches!(validate_create_guild_channel(&long_topic), Err(RestError::InvalidArgument("topic"))));
        let voice_slow = CreateGuildChannelRequest {
            kind: ChannelKind::GuildVoice,
            rate_limit_per_user: Some(5),
            ..valid_create()
        };
        assert!(matches!(
            validate_create_guild_channel(&voice_slow),
            Err(RestError::InvalidArgument("rate_limit_per_user"))
        ));
        let over_slow = CreateGuildChannelRequest {
            rate_limit_per_user: Some(MAX_CHANNEL_RATE_LIMIT_PER_USER + 1),
            ..valid_create()
        };
        assert!(matches!(
            validate_create_guild_channel(&over_slow),
            Err(RestError::InvalidArgument("rate_limit_per_user"))
        ));

        // Patch checks: empty rejected; blank topic string rejected vs Some(None) allowed; type conversion checked.
        assert!(matches!(
            validate_guild_channel_patch(&GuildChannelPatch::default()),
            Err(RestError::InvalidArgument("patch"))
        ));
        let empty_topic_str = GuildChannelPatch { topic: Some(Some("   ".into())), ..Default::default() };
        assert!(matches!(
            validate_guild_channel_patch(&empty_topic_str),
            Err(RestError::InvalidArgument("topic"))
        ));
        let clear_topic = GuildChannelPatch { topic: Some(None), ..Default::default() };
        assert!(validate_guild_channel_patch(&clear_topic).is_ok());
        assert_eq!(serde_json::to_value(&clear_topic).unwrap(), json!({ "topic": null }));

        let disable_inherited_nsfw = GuildChannelPatch { nsfw_override: Some(Some(false)), ..Default::default() };
        assert!(validate_guild_channel_patch(&disable_inherited_nsfw).is_ok());
        assert_eq!(serde_json::to_value(&disable_inherited_nsfw).unwrap(), json!({ "nsfw_override": false }));

        let bad_patch_kind = GuildChannelPatch { kind: Some(ChannelKind::GuildVoice), ..Default::default() };
        assert!(matches!(
            validate_guild_channel_patch(&bad_patch_kind),
            Err(RestError::InvalidArgument("type"))
        ));
        let good_patch_kind = GuildChannelPatch {
            kind: Some(ChannelKind::GuildAnnouncement),
            parent_id: Some(None),
            rate_limit_per_user: Some(MAX_CHANNEL_RATE_LIMIT_PER_USER),
            ..Default::default()
        };
        assert!(validate_guild_channel_patch(&good_patch_kind).is_ok());
        let bad_patch_slow = GuildChannelPatch {
            rate_limit_per_user: Some(MAX_CHANNEL_RATE_LIMIT_PER_USER + 1),
            ..Default::default()
        };
        assert!(matches!(
            validate_guild_channel_patch(&bad_patch_slow),
            Err(RestError::InvalidArgument("rate_limit_per_user"))
        ));
    }
}
