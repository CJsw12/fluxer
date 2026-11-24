//! Social & community actions on [`Backend`]: friend requests, accept/remove,
//! block/unblock, guild invites, and leaving communities.

use fluxer_client_core::Dispatch;
use fluxer_client_core::rest::{CreateInviteRequest, parse_user_tag};
use fluxer_client_core::state::Snowflake;
use serde_json::{Value, json};

use super::{Backend, UiEvent, lock};

/// Metadata returned by `POST /channels/{channel_id}/invites`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedInvite {
    pub code: String,
    pub max_age: u32,
    pub max_uses: u32,
    pub temporary: bool,
}

/// Summary returned by `GET /invites/{code}` before accepting an invite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvitePreview {
    pub code: String,
    pub guild_name: String,
    pub guild_id: Option<Snowflake>,
    pub channel_name: Option<String>,
    pub member_count: Option<u64>,
    pub presence_count: Option<u64>,
}

/// Destination returned by `POST /invites/{code}` for either a guild invite
/// (`guild_id`) or a group DM invite (`channel_id`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcceptedInvite {
    pub guild_id: Option<Snowflake>,
    pub channel_id: Option<Snowflake>,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocialCall {
    SendFriendByTag { username: String, discriminator: Option<String> },
    SendFriendById(Snowflake),
    AcceptFriend(Snowflake),
    RemoveRelationship(Snowflake),
    BlockUser(Snowflake),
    FetchInvite(String),
    AcceptInvite(String),
    LeaveGuild(Snowflake),
}

fn extract_path_code(path: &str) -> Option<&str> {
    let last = path.rsplit('/').find(|segment| !segment.is_empty())?;
    if matches!(last.to_ascii_lowercase().as_str(), "invite" | "i") {
        return None;
    }
    Some(last)
}

/// Extracts a bare invite code from either a code (`abc123`) or URL
/// (`https://fluxer.gg/abc123`, `https://fluxer.app/invite/abc123?x=1`).
pub fn extract_invite_code(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let without_query = trimmed.split(['?', '#']).next().unwrap_or_default();
    if without_query.split('/').any(|segment| segment == "..") {
        return None;
    }
    let code = if let Some((_, rest)) = without_query.split_once("://") {
        let (_, path) = rest.split_once('/')?;
        extract_path_code(path)?
    } else if let Some((_, path)) = without_query.split_once('/') {
        extract_path_code(path)?
    } else {
        without_query
    };
    if code.is_empty()
        || code.len() > 64
        || !code.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        return None;
    }
    Some(code.to_owned())
}

pub fn parse_invite_preview(code: &str, value: &Value) -> InvitePreview {
    let guild = value.get("guild");
    let guild_id = guild
        .and_then(|g| g.get("id"))
        .and_then(Value::as_str)
        .and_then(Snowflake::parse);
    let raw_channel_name = value
        .get("channel")
        .and_then(|c| c.get("name"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let guild_name = guild
        .and_then(|g| g.get("name"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or_else(|| if guild_id.is_none() { raw_channel_name.clone() } else { None })
        .unwrap_or_else(|| code.to_owned());
    let channel_name = if guild_id.is_some() { raw_channel_name } else { None };
    let member_count = value
        .get("member_count")
        .or_else(|| value.get("approximate_member_count"))
        .and_then(Value::as_u64);
    let presence_count = value
        .get("presence_count")
        .or_else(|| value.get("approximate_presence_count"))
        .and_then(Value::as_u64);
    InvitePreview {
        code: code.to_owned(),
        guild_name,
        guild_id,
        channel_name,
        member_count,
        presence_count,
    }
}

impl Backend {
    /// Parses `name#1234` (or bare username) and sends `POST /users/@me/relationships`.
    pub fn send_friend_request_by_tag(&self, input: String) {
        let (username, discriminator) = match parse_user_tag(&input) {
            Ok(parsed) => parsed,
            Err(err) => {
                let _ = self.events.send(UiEvent::FriendRequestSent {
                    input,
                    error: Some(err.to_string()),
                });
                return;
            }
        };
        #[cfg(test)]
        super::lock_sent(&self.sent_social).push(SocialCall::SendFriendByTag {
            username: username.clone(),
            discriminator: discriminator.clone(),
        });
        let rest = self.rest.clone();
        let state = self.state.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            match rest.send_friend_request_by_tag(&username, discriminator.as_deref()).await {
                Ok(val) => {
                    if val.is_object() {
                        lock(&state).apply(&Dispatch {
                            t: "RELATIONSHIP_ADD".to_owned(),
                            s: 0,
                            d: val,
                        });
                        let _ = events.send(UiEvent::StateChanged);
                    }
                    let _ = events.send(UiEvent::FriendRequestSent { input, error: None });
                }
                Err(err) => {
                    let _ = events.send(UiEvent::FriendRequestSent {
                        input,
                        error: Some(err.to_string()),
                    });
                }
            }
        });
    }

    pub fn send_friend_request(&self, user_id: Snowflake) {
        #[cfg(test)]
        super::lock_sent(&self.sent_social).push(SocialCall::SendFriendById(user_id));
        self.run_relationship_upsert(
            "Could not send friend request",
            move |rest| Box::pin(async move { rest.send_friend_request(&user_id.to_string(), false).await }),
        );
    }

    pub fn accept_friend_request(&self, user_id: Snowflake) {
        #[cfg(test)]
        super::lock_sent(&self.sent_social).push(SocialCall::AcceptFriend(user_id));
        self.run_relationship_upsert(
            "Could not accept friend request",
            move |rest| Box::pin(async move { rest.accept_friend_request(&user_id.to_string()).await }),
        );
    }

    pub fn remove_relationship(&self, user_id: Snowflake) {
        #[cfg(test)]
        super::lock_sent(&self.sent_social).push(SocialCall::RemoveRelationship(user_id));
        let rest = self.rest.clone();
        let state = self.state.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            match rest.remove_relationship(&user_id.to_string()).await {
                Ok(()) => {
                    lock(&state).apply(&Dispatch {
                        t: "RELATIONSHIP_REMOVE".to_owned(),
                        s: 0,
                        d: json!({ "id": user_id.to_string() }),
                    });
                    let _ = events.send(UiEvent::StateChanged);
                }
                Err(err) => {
                    let _ = events.send(UiEvent::ActionFailed {
                        error: format!("Could not update relationship: {err}"),
                    });
                }
            }
        });
    }

    pub fn block_user(&self, user_id: Snowflake) {
        #[cfg(test)]
        super::lock_sent(&self.sent_social).push(SocialCall::BlockUser(user_id));
        let rest = self.rest.clone();
        let state = self.state.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            match rest.block_user(&user_id.to_string()).await {
                Ok(val) => {
                    let payload = if val.get("id").is_some() {
                        val
                    } else {
                        json!({ "id": user_id.to_string(), "type": 2 })
                    };
                    lock(&state).apply(&Dispatch {
                        t: "RELATIONSHIP_ADD".to_owned(),
                        s: 0,
                        d: payload,
                    });
                    let _ = events.send(UiEvent::StateChanged);
                }
                Err(err) => {
                    let _ = events.send(UiEvent::ActionFailed {
                        error: format!("Could not block user: {err}"),
                    });
                }
            }
        });
    }

    fn run_relationship_upsert(
        &self,
        action_label: &'static str,
        call: impl FnOnce(
                std::sync::Arc<fluxer_client_core::rest::RestClient>,
            ) -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<Value, fluxer_client_core::rest::RestError>> + Send>,
            > + Send
            + 'static,
    ) {
        let rest = self.rest.clone();
        let state = self.state.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            match call(rest).await {
                Ok(val) => {
                    if val.is_object() {
                        lock(&state).apply(&Dispatch {
                            t: "RELATIONSHIP_ADD".to_owned(),
                            s: 0,
                            d: val,
                        });
                        let _ = events.send(UiEvent::StateChanged);
                    }
                }
                Err(err) => {
                    let _ = events.send(UiEvent::ActionFailed {
                        error: format!("{action_label}: {err}"),
                    });
                }
            }
        });
    }

    pub fn fetch_invite(&self, code: String) {
        #[cfg(test)]
        super::lock_sent(&self.sent_social).push(SocialCall::FetchInvite(code.clone()));
        let rest = self.rest.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let result = rest
                .get_invite(&code)
                .await
                .map(|val| parse_invite_preview(&code, &val))
                .map_err(|err| err.to_string());
            let _ = events.send(UiEvent::InvitePreviewLoaded { code, result });
        });
    }

    pub fn accept_invite(&self, code: String) {
        #[cfg(test)]
        super::lock_sent(&self.sent_social).push(SocialCall::AcceptInvite(code.clone()));
        let rest = self.rest.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let result = rest
                .accept_invite(&code)
                .await
                .map(|val| AcceptedInvite {
                    guild_id: val
                        .get("guild")
                        .and_then(|g| g.get("id"))
                        .and_then(Value::as_str)
                        .and_then(Snowflake::parse),
                    channel_id: val
                        .get("channel")
                        .and_then(|c| c.get("id"))
                        .and_then(Value::as_str)
                        .and_then(Snowflake::parse),
                })
                .map_err(|err| err.to_string());
            let _ = events.send(UiEvent::InviteAccepted { code, result });
        });
    }

    pub fn leave_guild(&self, guild_id: Snowflake) {
        #[cfg(test)]
        super::lock_sent(&self.sent_social).push(SocialCall::LeaveGuild(guild_id));
        let rest = self.rest.clone();
        let state = self.state.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            match rest.leave_guild(&guild_id.to_string(), false).await {
                Ok(()) => {
                    lock(&state).apply(&Dispatch {
                        t: "GUILD_DELETE".to_owned(),
                        s: 0,
                        d: json!({ "id": guild_id.to_string() }),
                    });
                    let _ = events.send(UiEvent::GuildLeft { guild_id, error: None });
                    let _ = events.send(UiEvent::StateChanged);
                }
                Err(err) => {
                    let _ = events.send(UiEvent::GuildLeft {
                        guild_id,
                        error: Some(format!("Could not leave community: {err}")),
                    });
                }
            }
        });
    }

    /// Creates a channel invite (`POST /channels/{c}/invites`), echoing `seq`.
    pub fn create_channel_invite(&self, seq: u64, channel_id: Snowflake, request: CreateInviteRequest) {
        #[cfg(test)]
        super::lock_sent(&self.sent_invites).push((channel_id, request.clone()));
        let rest = self.rest.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let result = rest
                .create_channel_invite(&channel_id.to_string(), &request)
                .await
                .map_err(|err| err.to_string())
                .and_then(|val| {
                    let code = val
                        .get("code")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| "Server response did not include an invite code.".to_owned())?
                        .to_owned();
                    let max_age = val
                        .get("max_age")
                        .and_then(Value::as_u64)
                        .and_then(|n| u32::try_from(n).ok())
                        .unwrap_or_else(|| request.max_age.unwrap_or(0));
                    let max_uses = val
                        .get("max_uses")
                        .and_then(Value::as_u64)
                        .and_then(|n| u32::try_from(n).ok())
                        .unwrap_or_else(|| request.max_uses.unwrap_or(0));
                    let temporary = val
                        .get("temporary")
                        .and_then(Value::as_bool)
                        .unwrap_or_else(|| request.temporary.unwrap_or(false));
                    Ok(CreatedInvite { code, max_age, max_uses, temporary })
                });
            let _ = events.send(UiEvent::InviteCreated { seq, channel_id, result });
        });
    }

    /// Requests deletion of all current user messages in `guild_id`, echoing `seq`.
    pub fn bulk_delete_my_guild_messages(&self, seq: u64, guild_id: Snowflake, password: Option<String>) {
        #[cfg(test)]
        super::lock_sent(&self.sent_guild_bulk_deletes)
            .push((guild_id, password.as_deref().is_some_and(|p| !p.trim().is_empty())));
        let rest = self.rest.clone();
        let state = self.state.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            match rest.bulk_delete_my_guild_messages(&guild_id.to_string(), password.as_deref()).await {
                Ok(()) => {
                    if lock(&state).remove_own_messages_in_guild(guild_id) {
                        let _ = events.send(UiEvent::StateChanged);
                    }
                    let _ = events.send(UiEvent::GuildMessagesBulkDeleted { seq, guild_id, error: None });
                }
                Err(err) => {
                    let _ = events.send(UiEvent::GuildMessagesBulkDeleted {
                        seq,
                        guild_id,
                        error: Some(err.to_string()),
                    });
                }
            }
        });
    }

    /// Purges all messages in the personal notes channel (`POST /channels/{c}/messages/purge`).
    pub fn purge_personal_notes(&self, seq: u64, channel_id: Snowflake) {
        #[cfg(test)]
        super::lock_sent(&self.sent_personal_notes_purges).push(channel_id);
        let rest = self.rest.clone();
        let state = self.state.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let result = match rest.purge_personal_notes_messages(&channel_id.to_string()).await {
                Ok(deleted) => {
                    if lock(&state).clear_channel_messages(channel_id) {
                        let _ = events.send(UiEvent::StateChanged);
                    }
                    Ok(deleted)
                }
                Err(err) => Err(err.to_string()),
            };
            let _ = events.send(UiEvent::PersonalNotesPurged { seq, channel_id, result });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_invite_code_handles_urls_and_rejects_bad_input() {
        assert_eq!(extract_invite_code("abc123").as_deref(), Some("abc123"));
        assert_eq!(
            extract_invite_code("  https://fluxer.gg/fluxer-port/  ").as_deref(),
            Some("fluxer-port")
        );
        assert_eq!(extract_invite_code("fluxer.gg/my_code").as_deref(), Some("my_code"));
        assert_eq!(
            extract_invite_code("https://fluxer.app/invite/my_code?ref=home#top").as_deref(),
            Some("my_code")
        );
        assert_eq!(extract_invite_code(""), None);
        assert_eq!(extract_invite_code("   "), None);
        assert_eq!(extract_invite_code("has space"), None);
        assert_eq!(extract_invite_code("https://fluxer.gg/"), None);
        assert_eq!(extract_invite_code("fluxer.gg/"), None);
        assert_eq!(extract_invite_code("https://fluxer.app/invite/"), None);
        assert_eq!(extract_invite_code("../admin"), None);
        assert_eq!(extract_invite_code("fluxer.gg"), None);
        assert_eq!(extract_invite_code(&"a".repeat(65)), None);
    }

    #[test]
    fn parse_invite_preview_reads_guild_group_dm_and_counts() {
        let preview = parse_invite_preview(
            "port-invite",
            &json!({
                "guild": { "id": "99", "name": "Rust Testers" },
                "channel": { "id": "101", "name": "welcome" },
                "member_count": 42,
                "presence_count": 7
            }),
        );
        assert_eq!(
            preview,
            InvitePreview {
                code: "port-invite".to_owned(),
                guild_name: "Rust Testers".to_owned(),
                guild_id: Some(Snowflake(99)),
                channel_name: Some("welcome".to_owned()),
                member_count: Some(42),
                presence_count: Some(7),
            }
        );

        let dm_preview = parse_invite_preview(
            "dm-invite",
            &json!({
                "channel": { "id": "505", "name": "Weekend Crew" },
                "member_count": 5
            }),
        );
        assert_eq!(
            dm_preview,
            InvitePreview {
                code: "dm-invite".to_owned(),
                guild_name: "Weekend Crew".to_owned(),
                guild_id: None,
                channel_name: None,
                member_count: Some(5),
                presence_count: None,
            }
        );
    }
}
