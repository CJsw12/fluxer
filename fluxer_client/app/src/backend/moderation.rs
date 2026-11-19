//! Guild member moderation actions on [`Backend`]: kick, ban, unban, list bans,
//! timeout, and nickname updates.

use fluxer_client_core::Dispatch;
use fluxer_client_core::rest::{BanMemberRequest, GuildMemberPatch};
use fluxer_client_core::state::Snowflake;
use serde_json::{Value, json};

use super::{Backend, UiEvent, lock};
#[cfg(test)]
use super::lock_sent;

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModerationCall {
    Kick { guild_id: Snowflake, user_id: Snowflake, reason: Option<String> },
    Ban { guild_id: Snowflake, user_id: Snowflake, request: BanMemberRequest },
    Unban { guild_id: Snowflake, user_id: Snowflake, reason: Option<String> },
    ListBans { guild_id: Snowflake },
    UpdateMember {
        guild_id: Snowflake,
        user_id: Snowflake,
        is_self: bool,
        patch: GuildMemberPatch,
        reason: Option<String>,
    },
}

fn member_remove_dispatch(guild_id: Snowflake, user_id: Snowflake) -> Dispatch {
    Dispatch {
        t: "GUILD_MEMBER_REMOVE".to_owned(),
        s: 0,
        d: json!({
            "guild_id": guild_id.to_string(),
            "user": { "id": user_id.to_string() },
        }),
    }
}

impl Backend {
    /// `DELETE /guilds/{guild_id}/members/{user_id}`, echoing `seq`.
    pub fn kick_member(&self, seq: u64, guild_id: Snowflake, user_id: Snowflake, reason: Option<String>) {
        #[cfg(test)]
        lock_sent(&self.sent_moderation).push(ModerationCall::Kick {
            guild_id,
            user_id,
            reason: reason.clone(),
        });
        let rest = self.rest.clone();
        let state = self.state.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let error = match rest
                .kick_guild_member(&guild_id.to_string(), &user_id.to_string(), reason.as_deref())
                .await
            {
                Ok(()) => {
                    lock(&state).apply(&member_remove_dispatch(guild_id, user_id));
                    let _ = events.send(UiEvent::StateChanged);
                    None
                }
                Err(err) => Some(err.to_string()),
            };
            let _ = events.send(UiEvent::MemberModerated { seq, guild_id, user_id, error });
        });
    }

    /// `PUT /guilds/{guild_id}/bans/{user_id}`, echoing `seq`.
    pub fn ban_member(&self, seq: u64, guild_id: Snowflake, user_id: Snowflake, request: BanMemberRequest) {
        #[cfg(test)]
        lock_sent(&self.sent_moderation).push(ModerationCall::Ban {
            guild_id,
            user_id,
            request: request.clone(),
        });
        let rest = self.rest.clone();
        let state = self.state.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let error = match rest
                .ban_guild_member(&guild_id.to_string(), &user_id.to_string(), &request)
                .await
            {
                Ok(()) => {
                    lock(&state).apply(&member_remove_dispatch(guild_id, user_id));
                    let _ = events.send(UiEvent::StateChanged);
                    None
                }
                Err(err) => Some(err.to_string()),
            };
            let _ = events.send(UiEvent::MemberModerated { seq, guild_id, user_id, error });
        });
    }

    /// `GET /guilds/{guild_id}/bans`, echoing `seq`.
    pub fn list_guild_bans(&self, seq: u64, guild_id: Snowflake) {
        #[cfg(test)]
        lock_sent(&self.sent_moderation).push(ModerationCall::ListBans { guild_id });
        let rest = self.rest.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let result = rest.list_guild_bans(&guild_id.to_string()).await.map_err(|e| e.to_string());
            let _ = events.send(UiEvent::GuildBansLoaded { seq, guild_id, result });
        });
    }

    /// `DELETE /guilds/{guild_id}/bans/{user_id}`, echoing `seq`.
    pub fn unban_member(&self, seq: u64, guild_id: Snowflake, user_id: Snowflake, reason: Option<String>) {
        #[cfg(test)]
        lock_sent(&self.sent_moderation).push(ModerationCall::Unban {
            guild_id,
            user_id,
            reason: reason.clone(),
        });
        let rest = self.rest.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let error = rest
                .unban_guild_member(&guild_id.to_string(), &user_id.to_string(), reason.as_deref())
                .await
                .err()
                .map(|e| e.to_string());
            let _ = events.send(UiEvent::MemberUnbanned { seq, guild_id, user_id, error });
        });
    }

    /// `PATCH /guilds/{guild_id}/members/{user_id|@me}`, echoing `seq`.
    pub fn update_guild_member_moderation(
        &self,
        seq: u64,
        guild_id: Snowflake,
        user_id: Snowflake,
        is_self: bool,
        patch: GuildMemberPatch,
        reason: Option<String>,
    ) {
        #[cfg(test)]
        lock_sent(&self.sent_moderation).push(ModerationCall::UpdateMember {
            guild_id,
            user_id,
            is_self,
            patch: patch.clone(),
            reason: reason.clone(),
        });
        let rest = self.rest.clone();
        let state = self.state.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let res = if is_self {
                rest.update_own_guild_member(&guild_id.to_string(), &patch).await
            } else {
                rest.update_guild_member(&guild_id.to_string(), &user_id.to_string(), &patch, reason.as_deref())
                    .await
            };
            let error = match res {
                Ok(mut body) => {
                    if let Value::Object(map) = &mut body {
                        map.entry("guild_id".to_owned()).or_insert_with(|| json!(guild_id.to_string()));
                    }
                    lock(&state).apply(&Dispatch {
                        t: "GUILD_MEMBER_UPDATE".to_owned(),
                        s: 0,
                        d: body,
                    });
                    let _ = events.send(UiEvent::StateChanged);
                    None
                }
                Err(err) => Some(err.to_string()),
            };
            let _ = events.send(UiEvent::MemberModerated { seq, guild_id, user_id, error });
        });
    }
}
