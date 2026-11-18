//! Relationship endpoints. Route: `fluxer_api/src/api/user/controllers/UserRelationshipController.ts`;
//! web caller: `fluxer_app/src/features/relationship/commands/RelationshipCommands.ts`.

use reqwest::Method;
use serde_json::{Value, json};

use super::check_snowflake;
use crate::rest::client::{Auth, RestClient};
use crate::rest::error::RestError;

/// `RelationshipTypes.BLOCKED`.
const RELATIONSHIP_BLOCKED: u8 = 2;
/// `RelationshipNicknameUpdateRequest`: 0..=256 chars.
const MAX_NICKNAME_CHARS: usize = 256;
/// `UsernameType`: 1..=32 chars.
const MAX_USERNAME_CHARS: usize = 32;

/// Splits user input like `name#1234` into `(username, discriminator)`.
/// Input without a `#` is a bare username (instances without tags).
pub fn parse_user_tag(input: &str) -> Result<(String, Option<String>), RestError> {
    let input = input.trim();
    let (name, tag) = match input.rsplit_once('#') {
        Some((name, tag)) => (name, Some(tag)),
        None => (input, None),
    };
    check_username(name)?;
    match tag {
        Some(tag) => {
            check_discriminator(tag)?;
            Ok((name.to_owned(), Some(tag.to_owned())))
        }
        None => Ok((name.to_owned(), None)),
    }
}

fn check_username(name: &str) -> Result<(), RestError> {
    let count = name.chars().count();
    if count == 0 || count > MAX_USERNAME_CHARS || name.chars().any(|c| c.is_control() || c == '#') {
        return Err(RestError::InvalidArgument("username"));
    }
    Ok(())
}

fn check_discriminator(tag: &str) -> Result<(), RestError> {
    if (1..=4).contains(&tag.len()) && tag.bytes().all(|b| b.is_ascii_digit()) {
        Ok(())
    } else {
        Err(RestError::InvalidArgument("discriminator"))
    }
}

fn relationship_path(user_id: &str) -> Result<String, RestError> {
    check_snowflake(user_id, "user_id")?;
    Ok(format!("/users/@me/relationships/{user_id}"))
}

impl RestClient {
    /// `GET /users/@me/relationships` → `RelationshipResponse[]`
    /// (`type`: 1 friend, 2 blocked, 3 incoming, 4 outgoing).
    pub async fn list_relationships(&self) -> Result<Value, RestError> {
        self.request(Method::GET, "/users/@me/relationships", &[], None, Auth::Required).await
    }

    /// `POST /users/@me/relationships/{user_id}` with `{}` → `RelationshipResponse`.
    /// `staff_force_accept` is staff-only and sent only when true.
    pub async fn send_friend_request(&self, user_id: &str, staff_force_accept: bool) -> Result<Value, RestError> {
        let path = relationship_path(user_id)?;
        let body = if staff_force_accept { json!({ "staff_force_accept": true }) } else { json!({}) };
        self.request(Method::POST, &path, &[], Some(&body), Auth::Required).await
    }

    /// `POST /users/@me/relationships` with `{username, discriminator?}` →
    /// `RelationshipResponse`. The discriminator is optional on instances
    /// without tags.
    pub async fn send_friend_request_by_tag(
        &self,
        username: &str,
        discriminator: Option<&str>,
    ) -> Result<Value, RestError> {
        check_username(username)?;
        let mut body = json!({ "username": username });
        if let Some(tag) = discriminator {
            check_discriminator(tag)?;
            body["discriminator"] = json!(tag);
        }
        self.request(Method::POST, "/users/@me/relationships", &[], Some(&body), Auth::Required).await
    }

    /// `PUT /users/@me/relationships/{user_id}` without a body: accepts an
    /// incoming request → `RelationshipResponse`.
    pub async fn accept_friend_request(&self, user_id: &str) -> Result<Value, RestError> {
        let path = relationship_path(user_id)?;
        self.request(Method::PUT, &path, &[], None, Auth::Required).await
    }

    /// `DELETE /users/@me/relationships/{user_id}` (204): removes a friend,
    /// declines/cancels a request, or unblocks.
    pub async fn remove_relationship(&self, user_id: &str) -> Result<(), RestError> {
        let path = relationship_path(user_id)?;
        self.request(Method::DELETE, &path, &[], None, Auth::Required).await?;
        Ok(())
    }

    /// `PUT /users/@me/relationships/{user_id}` with `{type: 2}` →
    /// `RelationshipResponse`.
    pub async fn block_user(&self, user_id: &str) -> Result<Value, RestError> {
        let path = relationship_path(user_id)?;
        let body = json!({ "type": RELATIONSHIP_BLOCKED });
        self.request(Method::PUT, &path, &[], Some(&body), Auth::Required).await
    }

    /// Unblocking is deleting the relationship; see [`Self::remove_relationship`].
    pub async fn unblock_user(&self, user_id: &str) -> Result<(), RestError> {
        self.remove_relationship(user_id).await
    }

    /// `PATCH /users/@me/relationships/{user_id}` with `{nickname}` (`None`
    /// clears it, sent as `null`) → `RelationshipResponse`.
    pub async fn set_relationship_nickname(&self, user_id: &str, nickname: Option<&str>) -> Result<Value, RestError> {
        let path = relationship_path(user_id)?;
        if let Some(nick) = nickname {
            if nick.chars().count() > MAX_NICKNAME_CHARS {
                return Err(RestError::InvalidArgument("nickname must be 0..=256 chars"));
            }
        }
        let body = json!({ "nickname": nickname });
        self.request(Method::PATCH, &path, &[], Some(&body), Auth::Required).await
    }
}
