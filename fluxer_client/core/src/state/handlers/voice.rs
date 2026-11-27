//! Per-guild voice activity: `voice_states` in GUILD_CREATE/READY guild
//! objects and VOICE_STATE_UPDATE. Official `hasVoiceActivity`
//! (`useSidebarVoiceSummary.ts`) is true when any user (self included) is in
//! a voice channel of the guild. Entries are keyed by (user, connection) so a
//! user on two connections stays until both leave.

use serde_json::Value;

use super::{id_field, id_value};
use crate::state::client::{ClientState, StateChange};
use crate::state::ids::Snowflake;

fn connection_key(v: &Value) -> Option<(Snowflake, String)> {
    let user = id_field(v, "user_id")?;
    let conn = v.get("connection_id").and_then(Value::as_str).unwrap_or("").to_owned();
    Some((user, conn))
}

impl ClientState {
    /// Replace the guild's voice states from a guild payload.
    pub(in crate::state) fn load_guild_voice_states(&mut self, guild_id: Snowflake, g: &Value) {
        self.guild_voice.remove(&guild_id);
        let Some(list) = g.get("voice_states").and_then(Value::as_array) else { return };
        for v in list {
            let (Some(key), Some(channel)) = (connection_key(v), v.get("channel_id").and_then(id_value)) else {
                continue;
            };
            self.guild_voice.entry(guild_id).or_default().insert(key, channel);
        }
    }

    pub(in crate::state) fn apply_voice_state_update(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let (Some(guild_id), Some(key)) = (id_field(d, "guild_id"), connection_key(d)) else { return };
        let channel = d.get("channel_id").and_then(id_value);
        let changed = match channel {
            Some(channel) => self.guild_voice.entry(guild_id).or_default().insert(key, channel) != Some(channel),
            None => {
                let removed = self.guild_voice.get_mut(&guild_id).is_some_and(|m| m.remove(&key).is_some());
                if self.guild_voice.get(&guild_id).is_some_and(|m| m.is_empty()) {
                    self.guild_voice.remove(&guild_id);
                }
                removed
            }
        };
        if changed {
            // Guild list badge redraw.
            out.push(StateChange::GuildUpserted { guild_id });
        }
    }

    /// Whether any user is in a voice channel of the guild.
    pub fn guild_has_voice_activity(&self, guild_id: Snowflake) -> bool {
        self.guild_voice.get(&guild_id).is_some_and(|m| !m.is_empty())
    }

    /// The `(guild, channel)` the user is connected to, preferring `guild_id`
    /// when given. Read-only view for the profile voice-activity card.
    pub fn user_voice_channel(&self, user_id: Snowflake, guild_id: Option<Snowflake>) -> Option<(Snowflake, Snowflake)> {
        let find = |guild: Snowflake| {
            self.guild_voice
                .get(&guild)?
                .iter()
                .find(|((user, _), _)| *user == user_id)
                .map(|(_, channel)| (guild, *channel))
        };
        guild_id.and_then(find).or_else(|| self.guild_voice.keys().find_map(|g| find(*g)))
    }

    /// `(online, members)` from the guild payload (`GuildCount.hydrate`);
    /// `None` until both are known.
    pub fn guild_counts(&self, guild_id: Snowflake) -> Option<(u64, u64)> {
        let online = *self.guild_online_counts.get(&guild_id)?;
        let members = self.guilds.get(&guild_id)?.member_count?;
        Some((online, members))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Dispatch;
    use crate::state::client::ClientState;
    use crate::state::ids::Snowflake;

    fn upd(user: &str, conn: &str, channel: Option<&str>) -> Dispatch {
        Dispatch {
            t: "VOICE_STATE_UPDATE".into(),
            s: 0,
            d: json!({"guild_id": "10", "user_id": user, "connection_id": conn, "channel_id": channel}),
        }
    }

    #[test]
    fn join_move_leave() {
        let g = Snowflake(10);
        let mut s = ClientState::new();
        assert!(!s.guild_has_voice_activity(g));
        assert_eq!(s.apply(&upd("1", "a", Some("100"))).len(), 1);
        assert!(s.guild_has_voice_activity(g));
        // Move to another channel is a change; same channel again is not.
        assert_eq!(s.apply(&upd("1", "a", Some("101"))).len(), 1);
        assert!(s.apply(&upd("1", "a", Some("101"))).is_empty());
        s.apply(&upd("2", "b", Some("101")));
        s.apply(&upd("1", "a", None));
        assert!(s.guild_has_voice_activity(g));
        s.apply(&upd("2", "b", None));
        assert!(!s.guild_has_voice_activity(g));
        assert!(s.apply(&upd("2", "b", None)).is_empty());
        // Malformed (no user) is ignored.
        let bad = Dispatch { t: "VOICE_STATE_UPDATE".into(), s: 0, d: json!({"guild_id": "10"}) };
        assert!(s.apply(&bad).is_empty());
    }

    #[test]
    fn user_voice_channel_lookup() {
        let mut s = ClientState::new();
        assert_eq!(s.user_voice_channel(Snowflake(1), None), None);
        s.apply(&upd("1", "a", Some("100")));
        assert_eq!(s.user_voice_channel(Snowflake(1), None), Some((Snowflake(10), Snowflake(100))));
        assert_eq!(s.user_voice_channel(Snowflake(1), Some(Snowflake(99))), Some((Snowflake(10), Snowflake(100))));
        assert_eq!(s.user_voice_channel(Snowflake(2), Some(Snowflake(10))), None);
        s.apply(&upd("1", "a", None));
        assert_eq!(s.user_voice_channel(Snowflake(1), None), None);
    }

    #[test]
    fn guild_payload_voice_states_and_counts() {
        let mut s = ClientState::new();
        let d = Dispatch {
            t: "GUILD_CREATE".into(),
            s: 0,
            d: json!({
                "id": "10", "properties": {"name": "G"}, "member_count": 42, "online_count": 7,
                "voice_states": [
                    {"user_id": "1", "connection_id": "a", "channel_id": "100"},
                    {"user_id": "2", "connection_id": "b", "channel_id": null}
                ]
            }),
        };
        s.apply(&d);
        assert!(s.guild_has_voice_activity(Snowflake(10)));
        assert_eq!(s.guild_counts(Snowflake(10)), Some((7, 42)));
        // Recreate without voice states or online_count clears them.
        let d2 = Dispatch { t: "GUILD_CREATE".into(), s: 0, d: json!({"id": "10", "properties": {"name": "G"}, "member_count": 42}) };
        s.apply(&d2);
        assert!(!s.guild_has_voice_activity(Snowflake(10)));
        assert_eq!(s.guild_counts(Snowflake(10)), None);
        s.apply(&Dispatch { t: "GUILD_DELETE".into(), s: 0, d: json!({"id": "10"}) });
        assert_eq!(s.guild_counts(Snowflake(10)), None);
    }
}
