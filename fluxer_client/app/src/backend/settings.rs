//! Optimistic `PATCH /users/@me/settings` on [`Backend`].

use std::collections::BTreeMap;
use std::sync::PoisonError;
use std::sync::atomic::Ordering;

use fluxer_client_core::Dispatch;
use fluxer_client_core::rest::{ChannelOverride, GuildSettingsPatch, MuteConfig, UserSettingsPatch};
use fluxer_client_core::state::{ClientState, Snowflake, UserGuildSettings};
use serde_json::{Map, Value, json};

use super::{Backend, UiEvent, lock};

/// Builds a `USER_SETTINGS_UPDATE` payload that restores the keys present in
/// `patch_obj` back to their values in `previous_raw` (or `null` when absent).
fn revert_payload(previous_raw: &Value, patch_obj: &Map<String, Value>) -> Value {
    let mut out = Map::new();
    for key in patch_obj.keys() {
        let prev = previous_raw.get(key).cloned().unwrap_or(Value::Null);
        out.insert(key.clone(), prev);
    }
    Value::Object(out)
}

/// Copies existing `channel_overrides` from `raw` (either a keyed object or an
/// array of `{channel_id, ...}`) into a `channel_id -> Value` map so setting one
/// channel's mute never drops sibling overrides.
fn existing_channel_overrides(raw: Option<&Value>) -> Map<String, Value> {
    let mut out = Map::new();
    match raw.and_then(|v| v.get("channel_overrides")) {
        Some(Value::Object(map)) => {
            for (k, v) in map {
                out.insert(k.clone(), v.clone());
            }
        }
        Some(Value::Array(items)) => {
            for item in items {
                if let Some(id) = item.get("channel_id").and_then(Value::as_str) {
                    out.insert(id.to_owned(), item.clone());
                }
            }
        }
        _ => {}
    }
    out
}

/// Builds the full `channel_overrides` map sent to `PATCH /users/@me/guilds/{g|@me}/settings`
/// (the server replaces the whole map, so siblings and existing channel flags must be preserved).
pub(crate) fn build_channel_override_patch(
    existing: Option<&UserGuildSettings>,
    channel_id: Snowflake,
    muted: Option<bool>,
    message_notifications: Option<u8>,
) -> GuildSettingsPatch {
    let mut patch_overrides = BTreeMap::new();
    if let Some(settings) = existing {
        for (cid, ov) in &settings.channel_overrides {
            patch_overrides.insert(
                cid.to_string(),
                ChannelOverride {
                    collapsed: Some(ov.collapsed),
                    message_notifications: ov.message_notifications.and_then(|n| u8::try_from(n).ok()),
                    muted: Some(ov.muted),
                    mute_config: ov.mute_config.as_ref().map(|m| {
                        Some(MuteConfig {
                            end_time: m.end_time.clone(),
                            selected_time_window: m.selected_time_window.unwrap_or(0),
                        })
                    }),
                },
            );
        }
    }
    let key = channel_id.to_string();
    let mut target = patch_overrides.get(&key).cloned().unwrap_or_default();
    target.collapsed.get_or_insert(false);
    target.muted.get_or_insert(false);
    if let Some(m) = muted {
        target.muted = Some(m);
        target.mute_config = Some(None);
    }
    if let Some(level) = message_notifications {
        target.message_notifications = Some(level);
    }
    patch_overrides.insert(key, target);
    GuildSettingsPatch {
        channel_overrides: Some(Some(patch_overrides)),
        ..Default::default()
    }
}

#[cfg(test)]
pub(crate) fn build_channel_mute_patch(
    existing: Option<&UserGuildSettings>,
    channel_id: Snowflake,
    muted: bool,
) -> GuildSettingsPatch {
    build_channel_override_patch(existing, channel_id, Some(muted), None)
}

/// Reverts only `channel_id` inside `channel_overrides` so concurrent gateway
/// updates to sibling channels or guild-wide settings are preserved.
pub(crate) fn apply_channel_mute_rollback(
    state: &mut ClientState,
    guild_id: Option<Snowflake>,
    channel_id: Snowflake,
    previous_entry: Option<Value>,
) {
    let current_raw = state.guild_settings.get(&guild_id).map(|s| &s.raw);
    let mut merged = existing_channel_overrides(current_raw);
    let key = channel_id.to_string();
    match previous_entry {
        Some(prev) => {
            merged.insert(key, prev);
        }
        None => {
            merged.remove(&key);
        }
    }
    state.apply(&Dispatch {
        t: "USER_GUILD_SETTINGS_UPDATE".to_owned(),
        s: 0,
        d: json!({
            "guild_id": guild_id.map(|g| g.to_string()),
            "channel_overrides": Value::Object(merged),
        }),
    });
}

impl Backend {
    /// Applies `patch` to local `user_settings` immediately via a synthetic
    /// `USER_SETTINGS_UPDATE` dispatch, then sends `PATCH /users/@me/settings`
    /// and reverts those keys if the request fails.
    pub fn update_user_settings(&self, patch: UserSettingsPatch) {
        #[cfg(test)]
        super::lock_sent(&self.sent_settings).push(patch.clone());
        let seq = self.settings_seq.fetch_add(1, Ordering::Relaxed) + 1;
        let patch_value = serde_json::to_value(&patch).unwrap_or(Value::Null);
        let patch_obj = patch_value.as_object().cloned().unwrap_or_default();
        let previous_raw = {
            let mut state = self.state();
            let prev = state.user_settings.raw.clone();
            let dispatch = Dispatch { t: "USER_SETTINGS_UPDATE".to_owned(), s: 0, d: patch_value };
            state.apply(&dispatch);
            prev
        };
        let _ = self.events.send(UiEvent::StateChanged);
        let rest = self.rest.clone();
        let state = self.state.clone();
        let events = self.events.clone();
        let settings_seq = self.settings_seq.clone();
        self.runtime.spawn(async move {
            if let Err(err) = rest.update_user_settings(&patch).await {
                if settings_seq.load(Ordering::Relaxed) == seq {
                    let revert = revert_payload(&previous_raw, &patch_obj);
                    let dispatch = Dispatch { t: "USER_SETTINGS_UPDATE".to_owned(), s: 0, d: revert };
                    lock(&state).apply(&dispatch);
                    let _ = events.send(UiEvent::ActionFailed { error: format!("Could not save settings: {err}") });
                    let _ = events.send(UiEvent::StateChanged);
                }
            }
        });
    }

    /// Optimistically mutes or unmutes `channel_id` (inside `guild_id` or `@me`
    /// for DMs), then calls `PATCH /users/@me/guilds/{g|@me}/settings`.
    pub fn set_channel_muted(&self, guild_id: Option<Snowflake>, channel_id: Snowflake, muted: bool) {
        self.update_channel_override(guild_id, channel_id, Some(muted), None, "Could not update channel mute");
    }

    /// Optimistically updates `message_notifications` (`0`=all, `1`=mentions,
    /// `2`=nothing, `3`=community default) for `channel_id` via
    /// `PATCH /users/@me/guilds/{g|@me}/settings`.
    pub fn set_channel_notification_level(&self, guild_id: Option<Snowflake>, channel_id: Snowflake, level: u8) {
        self.update_channel_override(
            guild_id,
            channel_id,
            None,
            Some(level),
            "Could not update notification settings",
        );
    }

    fn update_channel_override(
        &self,
        guild_id: Option<Snowflake>,
        channel_id: Snowflake,
        muted: Option<bool>,
        message_notifications: Option<u8>,
        err_prefix: &'static str,
    ) {
        let seq = {
            let mut map = self.guild_settings_seq.lock().unwrap_or_else(PoisonError::into_inner);
            let entry = map.entry(guild_id).or_insert(0);
            *entry += 1;
            *entry
        };
        let (patch, previous_entry) = {
            let mut state = self.state();
            let patch =
                build_channel_override_patch(state.guild_settings.get(&guild_id), channel_id, muted, message_notifications);
            let mut merged = existing_channel_overrides(state.guild_settings.get(&guild_id).map(|s| &s.raw));
            let key = channel_id.to_string();
            let previous_entry = merged.get(&key).cloned();
            let mut entry = merged
                .get(&key)
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            entry.insert("channel_id".to_owned(), Value::String(key.clone()));
            if let Some(m) = muted {
                entry.insert("muted".to_owned(), Value::Bool(m));
                entry.insert("mute_config".to_owned(), Value::Null);
            } else {
                entry.entry("muted".to_owned()).or_insert(Value::Bool(false));
            }
            if let Some(level) = message_notifications {
                entry.insert("message_notifications".to_owned(), Value::from(level));
            }
            entry.entry("collapsed".to_owned()).or_insert(Value::Bool(false));
            merged.insert(key, Value::Object(entry));

            let payload = json!({
                "guild_id": guild_id.map(|g| g.to_string()),
                "channel_overrides": Value::Object(merged),
            });
            state.apply(&Dispatch { t: "USER_GUILD_SETTINGS_UPDATE".to_owned(), s: 0, d: payload });
            (patch, previous_entry)
        };
        #[cfg(test)]
        super::lock_sent(&self.sent_guild_settings).push((guild_id, patch.clone()));
        let _ = self.events.send(UiEvent::StateChanged);

        let rest = self.rest.clone();
        let state = self.state.clone();
        let events = self.events.clone();
        let guild_settings_seq = self.guild_settings_seq.clone();
        self.runtime.spawn(async move {
            let res = match guild_id {
                Some(g) => rest.update_guild_settings(&g.to_string(), &patch).await,
                None => rest.update_dm_settings(&patch).await,
            };
            let seq_matches = || {
                guild_settings_seq
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get(&guild_id)
                    .copied()
                    == Some(seq)
            };
            match res {
                Ok(updated) => {
                    if updated.is_object() && seq_matches() {
                        lock(&state).apply(&Dispatch {
                            t: "USER_GUILD_SETTINGS_UPDATE".to_owned(),
                            s: 0,
                            d: updated,
                        });
                        let _ = events.send(UiEvent::StateChanged);
                    }
                }
                Err(err) => {
                    if seq_matches() {
                        apply_channel_mute_rollback(&mut lock(&state), guild_id, channel_id, previous_entry);
                        let _ = events.send(UiEvent::ActionFailed {
                            error: format!("{err_prefix}: {err}"),
                        });
                        let _ = events.send(UiEvent::StateChanged);
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_notification_level_patch_preserves_muted_collapsed_and_siblings() {
        let mut state = ClientState::new();
        let guild = Snowflake(1);
        let target = Snowflake(11);
        let sibling = Snowflake(12);
        state.apply(&Dispatch {
            t: "USER_GUILD_SETTINGS_UPDATE".to_owned(),
            s: 1,
            d: json!({
                "guild_id": guild.to_string(),
                "channel_overrides": {
                    target.to_string(): {
                        "channel_id": target.to_string(),
                        "collapsed": false,
                        "message_notifications": 0,
                        "muted": true
                    },
                    sibling.to_string(): {
                        "channel_id": sibling.to_string(),
                        "collapsed": true,
                        "message_notifications": 2,
                        "muted": false
                    }
                }
            }),
        });

        let patch = build_channel_override_patch(state.guild_settings.get(&Some(guild)), target, None, Some(1));
        let overrides = patch.channel_overrides.flatten().expect("overrides map");
        assert_eq!(overrides.len(), 2);
        let target_ov = overrides.get(&target.to_string()).expect("target override");
        assert_eq!(target_ov.message_notifications, Some(1));
        assert_eq!(target_ov.muted, Some(true));
        assert_eq!(target_ov.collapsed, Some(false));
        let sibling_ov = overrides.get(&sibling.to_string()).expect("sibling override");
        assert_eq!(sibling_ov.message_notifications, Some(2));
        assert_eq!(sibling_ov.muted, Some(false));
    }

    #[test]
    fn channel_mute_patch_preserves_siblings_and_target_fields() {
        let mut state = ClientState::new();
        let guild = Snowflake(1);
        let target = Snowflake(11);
        let sibling = Snowflake(12);
        state.apply(&Dispatch {
            t: "USER_GUILD_SETTINGS_UPDATE".to_owned(),
            s: 1,
            d: json!({
                "guild_id": guild.to_string(),
                "channel_overrides": {
                    target.to_string(): {
                        "channel_id": target.to_string(),
                        "collapsed": true,
                        "message_notifications": 2,
                        "muted": false
                    },
                    sibling.to_string(): {
                        "channel_id": sibling.to_string(),
                        "collapsed": false,
                        "message_notifications": 1,
                        "muted": true
                    }
                }
            }),
        });

        let patch = build_channel_mute_patch(state.guild_settings.get(&Some(guild)), target, true);
        let overrides = patch.channel_overrides.flatten().expect("overrides map");
        assert_eq!(overrides.len(), 2);
        let target_ov = overrides.get(&target.to_string()).expect("target override");
        assert_eq!(target_ov.muted, Some(true));
        assert_eq!(target_ov.collapsed, Some(true));
        assert_eq!(target_ov.message_notifications, Some(2));
        let sibling_ov = overrides.get(&sibling.to_string()).expect("sibling override");
        assert_eq!(sibling_ov.muted, Some(true));
        assert_eq!(sibling_ov.message_notifications, Some(1));
    }

    #[test]
    fn channel_mute_rollback_restores_only_target_channel() {
        let mut state = ClientState::new();
        let guild = Snowflake(1);
        let target = Snowflake(11);
        let sibling = Snowflake(12);
        state.apply(&Dispatch {
            t: "USER_GUILD_SETTINGS_UPDATE".to_owned(),
            s: 1,
            d: json!({
                "guild_id": guild.to_string(),
                "suppress_everyone": true,
                "channel_overrides": {
                    target.to_string(): { "channel_id": target.to_string(), "muted": true },
                    sibling.to_string(): { "channel_id": sibling.to_string(), "muted": true }
                }
            }),
        });

        // Concurrent gateway update unmuted sibling while target's PATCH was in flight;
        // rollback reverts target back to None without clobbering sibling or suppress_everyone.
        state.apply(&Dispatch {
            t: "USER_GUILD_SETTINGS_UPDATE".to_owned(),
            s: 2,
            d: json!({
                "guild_id": guild.to_string(),
                "channel_overrides": {
                    target.to_string(): { "channel_id": target.to_string(), "muted": true },
                    sibling.to_string(): { "channel_id": sibling.to_string(), "muted": false }
                }
            }),
        });

        apply_channel_mute_rollback(&mut state, Some(guild), target, None);
        assert!(!state.is_channel_muted(Some(guild), target, 0));
        assert!(!state.is_channel_muted(Some(guild), sibling, 0));
        assert!(state.guild_settings.get(&Some(guild)).is_some_and(|s| s.suppress_everyone));

        // Restoring an explicit previous value puts it back while leaving sibling untouched.
        apply_channel_mute_rollback(
            &mut state,
            Some(guild),
            target,
            Some(json!({ "channel_id": target.to_string(), "muted": true, "collapsed": true })),
        );
        assert!(state.is_channel_muted(Some(guild), target, 0));
        assert!(!state.is_channel_muted(Some(guild), sibling, 0));
    }
}
