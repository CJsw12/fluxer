//! Presences: PRESENCE_UPDATE, PRESENCE_UPDATE_BULK and the `presences`
//! of READY and GUILD_CREATE (`Presence.ts`, flattened per user).
//!
//! Simplification: TS keeps an offline presence while other guilds still
//! reference the user and counts online members per guild; here an offline
//! update removes the user's presence and no counts are kept (the lazy
//! member list carries `online_count`).

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{id_field, parse, parse_list};
use crate::state::client::{ClientState, StateChange};
use crate::state::ids::Snowflake;

/// `StatusTypes` (`StatusConstants.ts`). Unknown strings normalise to
/// offline (`normalizeStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Online,
    Idle,
    Dnd,
    Invisible,
    #[default]
    Offline,
}

impl Status {
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("online") => Self::Online,
            Some("idle") => Self::Idle,
            Some("dnd") => Self::Dnd,
            Some("invisible") => Self::Invisible,
            _ => Self::Offline,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::Idle => "idle",
            Self::Dnd => "dnd",
            Self::Invisible => "invisible",
            Self::Offline => "offline",
        }
    }

    /// `isOfflineStatus`: offline or invisible.
    pub fn is_offline(self) -> bool {
        matches!(self, Self::Offline | Self::Invisible)
    }
}

/// `GatewayCustomStatusPayload` (`CustomStatus.ts`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CustomStatus {
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub emoji_id: Option<Snowflake>,
    #[serde(default)]
    pub emoji_name: Option<String>,
    #[serde(default)]
    pub emoji_animated: Option<bool>,
}

/// Another user's flattened presence.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Presence {
    pub status: Status,
    pub afk: bool,
    pub mobile: bool,
    pub custom_status: Option<CustomStatus>,
    /// Where the presence came from; `None` = DM / friends context.
    pub guild_ids: BTreeSet<Option<Snowflake>>,
}

/// `PresenceRecord` on the wire.
#[derive(Debug, Clone, Deserialize)]
struct PresenceWire {
    user: PresenceUser,
    #[serde(default)]
    guild_id: Option<Snowflake>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    afk: Option<bool>,
    #[serde(default)]
    mobile: Option<bool>,
    #[serde(default)]
    custom_status: Option<CustomStatus>,
}

#[derive(Debug, Clone, Deserialize)]
struct PresenceUser {
    id: Snowflake,
}

impl ClientState {
    pub(in crate::state) fn load_ready_presences(&mut self, d: &Value) {
        for p in parse_list::<PresenceWire>(d.get("presences"), "READY.presences") {
            self.upsert_presence(p);
        }
    }

    pub(in crate::state) fn load_guild_presences(&mut self, guild_id: Snowflake, g: &Value) {
        for mut p in parse_list::<PresenceWire>(g.get("presences"), "guild.presences") {
            p.guild_id = Some(guild_id);
            self.upsert_presence(p);
        }
    }

    pub(in crate::state) fn apply_presence_update(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let Some(p) = parse::<PresenceWire>(d, "PRESENCE_UPDATE") else { return };
        if let Some(user_id) = self.upsert_presence(p) {
            out.push(StateChange::PresenceUpdated { user_id });
        }
    }

    /// `{presences: [...], guild_id?}`; the outer `guild_id` fills entries
    /// without one.
    pub(in crate::state) fn apply_presence_update_bulk(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let guild_id = id_field(d, "guild_id");
        for mut p in parse_list::<PresenceWire>(d.get("presences"), "PRESENCE_UPDATE_BULK") {
            if p.guild_id.is_none() {
                p.guild_id = guild_id;
            }
            if let Some(user_id) = self.upsert_presence(p) {
                out.push(StateChange::PresenceUpdated { user_id });
            }
        }
    }

    /// Returns the user id if the stored presence changed.
    fn upsert_presence(&mut self, p: PresenceWire) -> Option<Snowflake> {
        let user_id = p.user.id;
        if Some(user_id) == self.current_user_id() {
            return None;
        }
        let status = Status::parse(p.status.as_deref());
        if status == Status::Offline {
            return self.presences.remove(&user_id).map(|_| user_id);
        }
        let entry = self.presences.entry(user_id).or_default();
        let before = entry.clone();
        entry.status = status;
        if let Some(afk) = p.afk {
            entry.afk = afk;
        }
        if let Some(mobile) = p.mobile {
            entry.mobile = mobile;
        }
        entry.custom_status = p.custom_status;
        entry.guild_ids.insert(p.guild_id);
        (*entry != before).then_some(user_id)
    }

    /// Another user's status; offline when unknown. The current user's
    /// own status is `user_settings.status`.
    pub fn presence_status(&self, user_id: Snowflake) -> Status {
        self.presences.get(&user_id).map_or(Status::Offline, |p| p.status)
    }
}
