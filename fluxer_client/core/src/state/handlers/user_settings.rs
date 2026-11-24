//! Account settings: READY `user_settings` / `user_guild_settings`,
//! USER_SETTINGS_UPDATE and USER_GUILD_SETTINGS_UPDATE (`UserSettings.ts`,
//! `UserGuildSettings.ts`). The raw JSON is kept (merged like TS
//! `{...existing, ...updates}`) and the essentials are typed from it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{id_value, parse, parse_list};
use crate::state::client::{ClientState, StateChange};
use crate::state::handlers::presence::CustomStatus;
use crate::state::ids::Snowflake;
use crate::state::model::{null_default, parse_timestamp_ms};

/// `user_settings` essentials.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct UserSettings {
    /// The full settings object as last received/merged (`Null` before READY).
    pub raw: Value,
    /// Own status: online, idle, dnd, invisible.
    pub status: Option<String>,
    /// `dark`, `light`, `system`, ...
    pub theme: Option<String>,
    /// BCP 47 locale, e.g. `en-US`.
    pub locale: Option<String>,
    pub custom_status: Option<CustomStatus>,
}

impl UserSettings {
    /// Whether compact message display is enabled in `raw`.
    pub fn message_display_compact(&self) -> bool {
        self.raw
            .get("message_display_compact")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    /// Trusted external link domains configured by the user.
    pub fn trusted_domains(&self) -> Vec<String> {
        self.raw
            .get("trusted_domains")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Guild IDs where non-friend DMs from members are restricted.
    pub fn restricted_guilds(&self) -> Vec<Snowflake> {
        self.raw
            .get("restricted_guilds")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(id_value).collect())
            .unwrap_or_default()
    }

    /// Guild IDs where DMs from bots in that community are restricted.
    pub fn bot_restricted_guilds(&self) -> Vec<Snowflake> {
        self.raw
            .get("bot_restricted_guilds")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(id_value).collect())
            .unwrap_or_default()
    }

    fn from_raw(raw: Value) -> Self {
        let text = |key: &str| raw.get(key).and_then(Value::as_str).map(str::to_owned);
        Self {
            status: text("status"),
            theme: text("theme"),
            locale: text("locale"),
            custom_status: raw
                .get("custom_status")
                .filter(|v| !v.is_null())
                .and_then(|v| parse::<CustomStatus>(v, "custom_status")),
            raw,
        }
    }
}

/// `mute_config`: a timed mute ends at `end_time`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MuteConfig {
    #[serde(default)]
    pub selected_time_window: Option<i64>,
    #[serde(default)]
    pub end_time: Option<String>,
}

/// `ChannelOverride` (`UserGuildSettings.ts:20-31`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ChannelOverride {
    #[serde(default)]
    pub channel_id: Option<Snowflake>,
    #[serde(default, deserialize_with = "null_default")]
    pub collapsed: bool,
    /// `MessageNotifications`; 3 = INHERIT.
    #[serde(default)]
    pub message_notifications: Option<i8>,
    #[serde(default, deserialize_with = "null_default")]
    pub muted: bool,
    #[serde(default)]
    pub mute_config: Option<MuteConfig>,
}

/// `MessageNotifications` (`NotificationConstants.ts`).
pub mod message_notifications {
    pub const ALL_MESSAGES: i8 = 0;
    pub const ONLY_MENTIONS: i8 = 1;
    pub const NO_MESSAGES: i8 = 2;
    pub const INHERIT: i8 = 3;
}

/// One guild's (or the DM context's) notification settings.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct UserGuildSettings {
    pub raw: Value,
    pub muted: bool,
    pub mute_config: Option<MuteConfig>,
    /// `MessageNotifications`, default ALL_MESSAGES.
    pub message_notifications: i8,
    pub suppress_everyone: bool,
    pub suppress_roles: bool,
    pub hide_muted_channels: bool,
    pub channel_overrides: BTreeMap<Snowflake, ChannelOverride>,
}

impl UserGuildSettings {
    fn from_raw(raw: Value) -> Self {
        let flag = |key: &str| raw.get(key).and_then(Value::as_bool).unwrap_or(false);
        // `channel_overrides` is an array or a record keyed by channel id.
        let mut channel_overrides = BTreeMap::new();
        match raw.get("channel_overrides") {
            overrides @ Some(Value::Array(_)) => {
                for o in parse_list::<ChannelOverride>(overrides, "channel_overrides") {
                    if let Some(id) = o.channel_id {
                        channel_overrides.insert(id, o);
                    }
                }
            }
            Some(Value::Object(map)) => {
                for (key, value) in map {
                    let Some(mut o) = parse::<ChannelOverride>(value, "channel_override") else { continue };
                    let Some(id) = o.channel_id.or_else(|| Snowflake::parse(key)) else { continue };
                    o.channel_id = Some(id);
                    channel_overrides.insert(id, o);
                }
            }
            _ => {}
        }
        Self {
            muted: flag("muted"),
            mute_config: raw
                .get("mute_config")
                .filter(|v| !v.is_null())
                .and_then(|v| parse::<MuteConfig>(v, "mute_config")),
            message_notifications: raw
                .get("message_notifications")
                .and_then(Value::as_i64)
                .and_then(|n| i8::try_from(n).ok())
                .unwrap_or(message_notifications::ALL_MESSAGES),
            suppress_everyone: flag("suppress_everyone"),
            suppress_roles: flag("suppress_roles"),
            hide_muted_channels: flag("hide_muted_channels"),
            channel_overrides,
            raw,
        }
    }
}

/// Muted and not past `mute_config.end_time` (TS clears expired mutes on a
/// timer). An unparsable end time counts as muted.
fn mute_active(muted: bool, config: Option<&MuteConfig>, now_ms: i64) -> bool {
    let end = config.and_then(|c| c.end_time.as_deref()).map(parse_timestamp_ms);
    muted && !matches!(end, Some(Some(end_ms)) if end_ms <= now_ms)
}

/// `{...base, ...patch}` on JSON objects; a non-object base is replaced.
fn merge_object(base: &Value, patch: &Value) -> Value {
    match (base, patch) {
        (Value::Object(base), Value::Object(patch)) => {
            let mut merged = base.clone();
            for (k, v) in patch {
                merged.insert(k.clone(), v.clone());
            }
            Value::Object(merged)
        }
        _ => patch.clone(),
    }
}

/// `guild_id` of a guild settings entry: `None` for null / missing / `@me`.
fn settings_guild_id(value: &Value) -> Option<Snowflake> {
    value.get("guild_id").and_then(id_value)
}

impl ClientState {
    pub(in crate::state) fn load_ready_user_settings(&mut self, d: &Value) {
        let raw = d.get("user_settings").cloned().unwrap_or(Value::Null);
        self.set_user_settings(raw);
    }

    pub(in crate::state) fn load_ready_guild_settings(&mut self, d: &Value) {
        self.guild_settings.clear();
        for item in d.get("user_guild_settings").and_then(Value::as_array).into_iter().flatten() {
            self.guild_settings.insert(settings_guild_id(item), UserGuildSettings::from_raw(item.clone()));
        }
    }

    /// Partial or full settings object, merged into the current one.
    pub(in crate::state) fn apply_user_settings_update(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        if !d.is_object() {
            return;
        }
        let merged = merge_object(&self.user_settings.raw, d);
        self.set_user_settings(merged);
        out.push(StateChange::UserSettingsChanged);
    }

    pub(in crate::state) fn apply_user_guild_settings_update(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        if !d.is_object() {
            return;
        }
        let guild_id = settings_guild_id(d);
        let merged = match self.guild_settings.get(&guild_id) {
            Some(existing) => merge_object(&existing.raw, d),
            None => d.clone(),
        };
        self.guild_settings.insert(guild_id, UserGuildSettings::from_raw(merged));
        out.push(StateChange::GuildSettingsChanged { guild_id });
    }

    /// Typed fields and the guild order (`guild_folders[].guild_ids`) from
    /// the raw settings.
    fn set_user_settings(&mut self, raw: Value) {
        if let Some(folders) = raw.get("guild_folders").and_then(Value::as_array) {
            self.guild_order = folders
                .iter()
                .filter_map(|f| f.get("guild_ids").and_then(Value::as_array))
                .flatten()
                .filter_map(id_value)
                .collect();
        }
        self.user_settings = UserSettings::from_raw(raw);
    }

    /// Guild (`None` = DMs) muted at `now_ms`.
    pub fn is_guild_muted(&self, guild_id: Option<Snowflake>, now_ms: i64) -> bool {
        self.guild_settings
            .get(&guild_id)
            .is_some_and(|s| mute_active(s.muted, s.mute_config.as_ref(), now_ms))
    }

    /// Channel muted by its own override at `now_ms` (guild mute not
    /// included; combine with [`Self::is_guild_muted`]).
    pub fn is_channel_muted(&self, guild_id: Option<Snowflake>, channel_id: Snowflake, now_ms: i64) -> bool {
        self.guild_settings
            .get(&guild_id)
            .and_then(|s| s.channel_overrides.get(&channel_id))
            .is_some_and(|o| mute_active(o.muted, o.mute_config.as_ref(), now_ms))
    }

    /// True when the user enabled "Hide muted channels" for `guild_id`.
    pub fn guild_hides_muted_channels(&self, guild_id: Snowflake) -> bool {
        self.guild_settings
            .get(&Some(guild_id))
            .is_some_and(|s| s.hide_muted_channels)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Dispatch;
    use serde_json::json;

    #[test]
    fn compact_and_trusted_domains_read_from_user_settings_updates() {
        let mut state = ClientState::new();
        assert!(!state.user_settings.message_display_compact());
        assert!(state.user_settings.trusted_domains().is_empty());

        state.apply(&Dispatch {
            t: "USER_SETTINGS_UPDATE".to_owned(),
            s: 1,
            d: json!({
                "message_display_compact": true,
                "trusted_domains": ["example.org", 42, "docs.rs"]
            }),
        });
        assert!(state.user_settings.message_display_compact());
        assert_eq!(state.user_settings.trusted_domains(), vec!["example.org", "docs.rs"]);
    }
}
