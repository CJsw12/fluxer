//! Guild reads & member moderation endpoints.
//! Routes: `fluxer_api/src/api/guild/controllers/GuildMemberController.ts`.
//!
//! The web client does not page members over REST: the member sidebar uses
//! gateway `LAZY_REQUEST` and lookups use gateway `REQUEST_GUILD_MEMBERS`
//! (`GatewaySocket.ts`). [`RestClient::list_guild_members`] exists for
//! tooling and small guilds; member kick/ban/unban/timeout/nickname edit map
//! directly to `DELETE/PUT/PATCH /guilds/{g}/members/...` and `/bans/...`.

use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::check_snowflake;
use crate::rest::client::{Auth, RestClient};
use crate::rest::error::RestError;
use crate::state::{User, parse_timestamp_ms};

/// `AUDIT_LOG_REASON_MAX_LENGTH` (`ChannelValidators.ts:118`).
pub const MAX_AUDIT_LOG_REASON_UTF16: usize = 512;
/// `GuildMemberUpdateRequest.nick` max length (`GuildRequestSchemas.ts:135`).
pub const MAX_NICKNAME_UTF16: usize = 32;
/// Maximum recent message purge window on ban (`GuildRequestSchemas.ts:319`).
pub const MAX_BAN_DELETE_MESSAGE_SECONDS: u32 = 604_800;
/// `MIN_TEMP_BAN_DURATION_SECONDS` (`LimitConstants.ts:112`).
pub const MIN_TEMP_BAN_DURATION_SECONDS: u32 = 60;
/// `MAX_TEMP_BAN_DURATION_SECONDS` (`LimitConstants.ts:113`).
pub const MAX_TEMP_BAN_DURATION_SECONDS: u32 = 2 * 365 * 24 * 60 * 60;
/// Maximum timeout duration (28 days).
pub const MAX_TIMEOUT_DURATION_SECONDS: u32 = 28 * 24 * 60 * 60;

/// Entry returned by `GET /guilds/{g}/bans` (`GuildBanResponse`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GuildBanEntry {
    pub user: User,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub moderator_id: Option<String>,
    #[serde(default)]
    pub banned_at: Option<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
}

/// Request body for `PUT /guilds/{g}/bans/{u}` (`GuildBanCreateRequest`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct BanMemberRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delete_message_seconds: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ban_duration_seconds: Option<u32>,
}

/// Partial update body for `PATCH /guilds/{g}/members/{u|@me}`.
/// Double-option fields emit explicit `null` when `Some(None)` to clear a
/// nickname or remove a timeout.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct GuildMemberPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nick: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub communication_disabled_until: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_reason: Option<String>,
}

impl GuildMemberPatch {
    pub fn is_empty(&self) -> bool {
        self.nick.is_none() && self.communication_disabled_until.is_none() && self.timeout_reason.is_none()
    }
}

/// Normalizes an audit log reason to match server `normalizeString`
/// (`SchemaPrimitives.ts:20-22`): strips `\u{000C}` and `\u{202E}` then trims.
pub fn normalize_audit_log_reason(reason: &str) -> String {
    reason
        .chars()
        .filter(|&c| c != '\u{000C}' && c != '\u{202E}')
        .collect::<String>()
        .trim()
        .to_owned()
}

pub fn validate_audit_log_reason(reason: &str) -> Result<(), RestError> {
    if normalize_audit_log_reason(reason).encode_utf16().count() > MAX_AUDIT_LOG_REASON_UTF16 {
        return Err(RestError::InvalidArgument("reason"));
    }
    Ok(())
}

pub fn validate_ban_request(req: &BanMemberRequest) -> Result<(), RestError> {
    if let Some(secs) = req.delete_message_seconds
        && secs > MAX_BAN_DELETE_MESSAGE_SECONDS
    {
        return Err(RestError::InvalidArgument("delete_message_seconds"));
    }
    if let Some(dur) = req.ban_duration_seconds
        && dur != 0
        && !(MIN_TEMP_BAN_DURATION_SECONDS..=MAX_TEMP_BAN_DURATION_SECONDS).contains(&dur)
    {
        return Err(RestError::InvalidArgument("ban_duration_seconds"));
    }
    if let Some(reason) = &req.reason {
        validate_audit_log_reason(reason)?;
    }
    Ok(())
}

/// Validates member nickname/timeout patch fields. Note: wall-clock relative
/// timeout bounds (server 1-year cap, future check) remain outside this
/// clock-free validator; the UI clamps durations to `60..=MAX_TIMEOUT_DURATION_SECONDS`.
pub fn validate_member_patch(patch: &GuildMemberPatch) -> Result<(), RestError> {
    if patch.is_empty() {
        return Err(RestError::InvalidArgument("patch"));
    }
    if let Some(Some(nick)) = &patch.nick {
        let len = nick.trim().encode_utf16().count();
        if len == 0 || len > MAX_NICKNAME_UTF16 {
            return Err(RestError::InvalidArgument("nick"));
        }
    }
    if let Some(Some(until)) = &patch.communication_disabled_until {
        let trimmed = until.trim();
        if trimmed.is_empty() || parse_timestamp_ms(trimmed).is_none() {
            return Err(RestError::InvalidArgument("communication_disabled_until"));
        }
    }
    if let Some(reason) = &patch.timeout_reason {
        validate_audit_log_reason(reason)?;
    }
    Ok(())
}

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

    /// `DELETE /guilds/{guild_id}/members/{user_id}` (204).
    pub async fn kick_guild_member(
        &self,
        guild_id: &str,
        user_id: &str,
        reason: Option<&str>,
    ) -> Result<(), RestError> {
        check_snowflake(guild_id, "guild_id")?;
        check_snowflake(user_id, "user_id")?;
        if let Some(r) = reason {
            validate_audit_log_reason(r)?;
        }
        let path = format!("/guilds/{guild_id}/members/{user_id}");
        self.request_with_reason(Method::DELETE, &path, &[], None, Auth::Required, reason)
            .await?;
        Ok(())
    }

    /// `PUT /guilds/{guild_id}/bans/{user_id}` (204).
    pub async fn ban_guild_member(
        &self,
        guild_id: &str,
        user_id: &str,
        req: &BanMemberRequest,
    ) -> Result<(), RestError> {
        check_snowflake(guild_id, "guild_id")?;
        check_snowflake(user_id, "user_id")?;
        validate_ban_request(req)?;
        let mut normalized = req.clone();
        normalized.reason = normalized
            .reason
            .as_deref()
            .map(normalize_audit_log_reason)
            .filter(|s| !s.is_empty());
        let body = serde_json::to_value(&normalized).map_err(|err| RestError::Decode(err.to_string()))?;
        let path = format!("/guilds/{guild_id}/bans/{user_id}");
        self.request_with_reason(
            Method::PUT,
            &path,
            &[],
            Some(&body),
            Auth::Required,
            normalized.reason.as_deref(),
        )
        .await?;
        Ok(())
    }

    /// `DELETE /guilds/{guild_id}/bans/{user_id}` (204).
    pub async fn unban_guild_member(
        &self,
        guild_id: &str,
        user_id: &str,
        reason: Option<&str>,
    ) -> Result<(), RestError> {
        check_snowflake(guild_id, "guild_id")?;
        check_snowflake(user_id, "user_id")?;
        if let Some(r) = reason {
            validate_audit_log_reason(r)?;
        }
        let path = format!("/guilds/{guild_id}/bans/{user_id}");
        self.request_with_reason(Method::DELETE, &path, &[], None, Auth::Required, reason)
            .await?;
        Ok(())
    }

    /// `GET /guilds/{guild_id}/bans` → `Vec<GuildBanEntry>`.
    pub async fn list_guild_bans(&self, guild_id: &str) -> Result<Vec<GuildBanEntry>, RestError> {
        check_snowflake(guild_id, "guild_id")?;
        let path = format!("/guilds/{guild_id}/bans");
        let val = self.request(Method::GET, &path, &[], None, Auth::Required).await?;
        serde_json::from_value(val).map_err(|err| RestError::Decode(err.to_string()))
    }

    /// `PATCH /guilds/{guild_id}/members/{user_id}` → updated `GuildMemberResponse`.
    pub async fn update_guild_member(
        &self,
        guild_id: &str,
        user_id: &str,
        patch: &GuildMemberPatch,
        reason: Option<&str>,
    ) -> Result<Value, RestError> {
        check_snowflake(guild_id, "guild_id")?;
        check_snowflake(user_id, "user_id")?;
        if let Some(r) = reason {
            validate_audit_log_reason(r)?;
        }
        let body = normalized_member_patch_json(patch)?;
        let path = format!("/guilds/{guild_id}/members/{user_id}");
        self.request_with_reason(Method::PATCH, &path, &[], Some(&body), Auth::Required, reason)
            .await
    }

    /// `PATCH /guilds/{guild_id}/members/@me` → updated `GuildMemberResponse` (nick only).
    pub async fn update_own_guild_member(
        &self,
        guild_id: &str,
        patch: &GuildMemberPatch,
    ) -> Result<Value, RestError> {
        check_snowflake(guild_id, "guild_id")?;
        if patch.communication_disabled_until.is_some() || patch.timeout_reason.is_some() {
            return Err(RestError::InvalidArgument("communication_disabled_until"));
        }
        let body = normalized_member_patch_json(patch)?;
        let path = format!("/guilds/{guild_id}/members/@me");
        self.request(Method::PATCH, &path, &[], Some(&body), Auth::Required).await
    }
}

fn normalized_member_patch_json(patch: &GuildMemberPatch) -> Result<Value, RestError> {
    validate_member_patch(patch)?;
    let mut normalized = patch.clone();
    if let Some(Some(nick)) = &normalized.nick {
        normalized.nick = Some(Some(nick.trim().to_owned()));
    }
    if let Some(Some(until)) = &normalized.communication_disabled_until {
        normalized.communication_disabled_until = Some(Some(until.trim().to_owned()));
    }
    normalized.timeout_reason = normalized
        .timeout_reason
        .as_deref()
        .map(normalize_audit_log_reason)
        .filter(|s| !s.is_empty());
    serde_json::to_value(&normalized).map_err(|err| RestError::Decode(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validates_ban_reason_nickname_and_timeout_bounds() {
        assert!(validate_ban_request(&BanMemberRequest::default()).is_ok());
        assert!(matches!(
            validate_ban_request(&BanMemberRequest {
                delete_message_seconds: Some(MAX_BAN_DELETE_MESSAGE_SECONDS + 1),
                ..Default::default()
            }),
            Err(RestError::InvalidArgument("delete_message_seconds"))
        ));
        assert!(matches!(
            validate_ban_request(&BanMemberRequest {
                ban_duration_seconds: Some(MIN_TEMP_BAN_DURATION_SECONDS - 1),
                ..Default::default()
            }),
            Err(RestError::InvalidArgument("ban_duration_seconds"))
        ));
        assert!(matches!(
            validate_ban_request(&BanMemberRequest {
                ban_duration_seconds: Some(MAX_TEMP_BAN_DURATION_SECONDS + 1),
                ..Default::default()
            }),
            Err(RestError::InvalidArgument("ban_duration_seconds"))
        ));
        assert!(validate_ban_request(&BanMemberRequest {
            ban_duration_seconds: Some(0),
            reason: Some("😀".repeat(256)),
            ..Default::default()
        })
        .is_ok());
        assert!(validate_audit_log_reason(&format!("\u{000C}{}\u{202E} ", "😀".repeat(256))).is_ok());
        assert_eq!(normalize_audit_log_reason(" \u{000C}spam\u{202E} "), "spam");
        assert!(matches!(
            validate_audit_log_reason(&"😀".repeat(257)),
            Err(RestError::InvalidArgument("reason"))
        ));

        assert!(matches!(
            validate_member_patch(&GuildMemberPatch::default()),
            Err(RestError::InvalidArgument("patch"))
        ));
        assert!(matches!(
            validate_member_patch(&GuildMemberPatch { nick: Some(Some("   ".into())), ..Default::default() }),
            Err(RestError::InvalidArgument("nick"))
        ));
        assert!(matches!(
            validate_member_patch(&GuildMemberPatch {
                nick: Some(Some("😀".repeat(17))),
                ..Default::default()
            }),
            Err(RestError::InvalidArgument("nick"))
        ));
        let clear_nick = GuildMemberPatch { nick: Some(None), ..Default::default() };
        assert!(validate_member_patch(&clear_nick).is_ok());
        assert_eq!(serde_json::to_value(&clear_nick).unwrap(), json!({ "nick": null }));

        let bad_ts = GuildMemberPatch {
            communication_disabled_until: Some(Some("not-a-timestamp".into())),
            ..Default::default()
        };
        assert!(matches!(
            validate_member_patch(&bad_ts),
            Err(RestError::InvalidArgument("communication_disabled_until"))
        ));
        let clear_timeout = GuildMemberPatch {
            communication_disabled_until: Some(None),
            ..Default::default()
        };
        assert_eq!(serde_json::to_value(&clear_timeout).unwrap(), json!({ "communication_disabled_until": null }));
    }
}
