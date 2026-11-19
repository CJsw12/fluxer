//! Private channel & group DM management endpoints.
//! Routes: `fluxer_api/src/api/user/controllers/UserChannelController.ts`
//! (`POST /users/@me/channels`) and `ChannelController.ts`
//! (`PATCH/DELETE /channels/{id}`, `PUT/DELETE /channels/{id}/recipients/{user_id}`).

use std::collections::BTreeSet;

use reqwest::Method;
use serde::Serialize;
use serde_json::{Value, json};

use super::check_snowflake;
use crate::rest::client::{Auth, RestClient};
use crate::rest::error::RestError;

/// `MAX_GROUP_DM_OTHER_RECIPIENTS` (`packages/constants/src/LimitConstants.ts:68`).
pub const MAX_GROUP_DM_OTHER_RECIPIENTS: usize = 49;
/// `GeneralChannelNameType` UTF-16 max length (`ChannelValidators.ts:109`).
pub const MAX_CHANNEL_NAME_UTF16: usize = 100;

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
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
