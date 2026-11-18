//! Typing indicators: TYPING_START (`RollingTypingStore.ts`). Each typer
//! expires [`TYPING_EXPIRY_MS`] after the last TYPING_START received; the
//! author's MESSAGE_CREATE removes them early. The current user is never
//! listed. The state has no timer: the UI calls
//! [`ClientState::expire_typing`] at [`ClientState::next_typing_expiry_ms`].

use serde::Serialize;
use serde_json::Value;

use super::id_field;
use crate::state::client::{ClientState, StateChange};
use crate::state::ids::Snowflake;

/// `TYPING_ROLLING_EXPIRY_MS` (`TypingSendThrottle.ts:3`).
pub const TYPING_EXPIRY_MS: i64 = 10_000;

/// A user typing in a channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Typer {
    pub user_id: Snowflake,
    /// Unix ms after which the indicator is gone.
    pub expires_at_ms: i64,
}

impl ClientState {
    /// `{channel_id, user_id, timestamp, guild_id?, member?}`.
    pub(in crate::state) fn apply_typing_start(&mut self, d: &Value, now_ms: i64, out: &mut Vec<StateChange>) {
        let (Some(channel_id), Some(user_id)) = (id_field(d, "channel_id"), id_field(d, "user_id")) else {
            return;
        };
        if let (Some(guild_id), Some(member)) = (id_field(d, "guild_id"), d.get("member")) {
            self.hydrate_member(guild_id, member);
        }
        if Some(user_id) == self.current_user_id() {
            return;
        }
        let expires_at_ms = now_ms.saturating_add(TYPING_EXPIRY_MS);
        let typers = self.typing.entry(channel_id).or_default();
        match typers.iter_mut().find(|t| t.user_id == user_id) {
            // Refresh only: the set of typers is unchanged.
            Some(typer) => typer.expires_at_ms = expires_at_ms,
            None => {
                typers.push(Typer { user_id, expires_at_ms });
                out.push(StateChange::TypingChanged { channel_id });
            }
        }
    }

    /// Remove one typer (their message arrived).
    pub(in crate::state) fn stop_typing(&mut self, channel_id: Snowflake, user_id: Snowflake, out: &mut Vec<StateChange>) {
        let Some(typers) = self.typing.get_mut(&channel_id) else { return };
        let before = typers.len();
        typers.retain(|t| t.user_id != user_id);
        if typers.len() != before {
            if typers.is_empty() {
                self.typing.remove(&channel_id);
            }
            out.push(StateChange::TypingChanged { channel_id });
        }
    }

    /// Users typing in the channel at `now_ms`, in start order.
    pub fn typing_users(&self, channel_id: Snowflake, now_ms: i64) -> Vec<Snowflake> {
        self.typing
            .get(&channel_id)
            .into_iter()
            .flatten()
            .filter(|t| t.expires_at_ms > now_ms)
            .map(|t| t.user_id)
            .collect()
    }

    /// Drop expired typers; one `TypingChanged` per affected channel.
    pub fn expire_typing(&mut self, now_ms: i64) -> Vec<StateChange> {
        let mut out = Vec::new();
        self.typing.retain(|channel_id, typers| {
            let before = typers.len();
            typers.retain(|t| t.expires_at_ms > now_ms);
            if typers.len() != before {
                out.push(StateChange::TypingChanged { channel_id: *channel_id });
            }
            !typers.is_empty()
        });
        out
    }

    /// The earliest expiry, for scheduling the next [`Self::expire_typing`].
    pub fn next_typing_expiry_ms(&self) -> Option<i64> {
        self.typing.values().flatten().map(|t| t.expires_at_ms).min()
    }
}
