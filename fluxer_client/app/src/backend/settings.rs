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
#[cfg(test)]
pub(crate) fn build_channel_override_patch(
    existing: Option<&UserGuildSettings>,
    channel_id: Snowflake,
    muted: Option<bool>,
    message_notifications: Option<u8>,
) -> GuildSettingsPatch {
    build_channel_override_patch_with(existing, channel_id, muted, message_notifications, None)
}

/// Official `createMuteConfig`: `None` duration (indefinite) -> no config (`null`); otherwise
/// `selected_time_window` is the duration in milliseconds and `end_time` is `now + duration`.
pub(crate) fn mute_config_for(now: chrono::DateTime<chrono::Utc>, duration_ms: Option<i64>) -> Option<MuteConfig> {
    let ms = duration_ms?;
    let end = now.checked_add_signed(chrono::Duration::milliseconds(ms))?;
    Some(MuteConfig {
        end_time: Some(end.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
        selected_time_window: ms,
    })
}

/// Like [`build_channel_override_patch`], additionally carrying a timed `mute_config`
/// when `muted == Some(true)` and `mute_config` is `Some`.
pub(crate) fn build_channel_override_patch_with(
    existing: Option<&UserGuildSettings>,
    channel_id: Snowflake,
    muted: Option<bool>,
    message_notifications: Option<u8>,
    mute_config: Option<MuteConfig>,
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
        target.mute_config = Some(if m { mute_config } else { None });
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
        self.update_channel_override(guild_id, channel_id, Some(muted), None, None, "Could not update channel mute");
    }

    /// Mutes `channel_id` for `duration_ms` milliseconds (`None` = until unmuted) using the
    /// official `mute_config { end_time, selected_time_window }` shape. Expiry is handled by
    /// `mute_active` on read.
    pub fn set_channel_muted_until(
        &self,
        guild_id: Option<Snowflake>,
        channel_id: Snowflake,
        duration_ms: Option<i64>,
    ) {
        let config = mute_config_for(chrono::Utc::now(), duration_ms);
        self.update_channel_override(guild_id, channel_id, Some(true), None, config, "Could not update channel mute");
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
            None,
            "Could not update notification settings",
        );
    }

    fn update_channel_override(
        &self,
        guild_id: Option<Snowflake>,
        channel_id: Snowflake,
        muted: Option<bool>,
        message_notifications: Option<u8>,
        mute_config: Option<MuteConfig>,
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
            let local_mute_config = match (&mute_config, muted) {
                (Some(c), Some(true)) => Some(json!({
                    "end_time": c.end_time,
                    "selected_time_window": c.selected_time_window,
                })),
                _ => None,
            };
            let patch = build_channel_override_patch_with(
                state.guild_settings.get(&guild_id),
                channel_id,
                muted,
                message_notifications,
                mute_config,
            );
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
                entry.insert("mute_config".to_owned(), local_mute_config.unwrap_or(Value::Null));
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

    /// Optimistically updates top-level guild notification settings fields
    /// (`muted`, `message_notifications`, `suppress_everyone`, `suppress_roles`,
    /// `hide_muted_channels`) via `PATCH /users/@me/guilds/{g}/settings`.
    pub fn update_guild_settings_fields(
        &self,
        guild_id: Snowflake,
        patch: GuildSettingsPatch,
        err_prefix: &'static str,
    ) {
        let seq = {
            let mut map = self.guild_settings_seq.lock().unwrap_or_else(PoisonError::into_inner);
            let entry = map.entry(Some(guild_id)).or_insert(0);
            *entry += 1;
            *entry
        };
        let patch_value = serde_json::to_value(&patch).unwrap_or(Value::Null);
        let patch_obj = patch_value.as_object().cloned().unwrap_or_default();
        let previous_raw = {
            let mut state = self.state();
            let prev = state
                .guild_settings
                .get(&Some(guild_id))
                .map(|s| s.raw.clone())
                .unwrap_or(Value::Null);
            let mut dispatch_obj = patch_obj.clone();
            dispatch_obj.insert("guild_id".to_owned(), Value::String(guild_id.to_string()));
            state.apply(&Dispatch {
                t: "USER_GUILD_SETTINGS_UPDATE".to_owned(),
                s: 0,
                d: Value::Object(dispatch_obj),
            });
            prev
        };
        #[cfg(test)]
        super::lock_sent(&self.sent_guild_settings).push((Some(guild_id), patch.clone()));
        let _ = self.events.send(UiEvent::StateChanged);

        let rest = self.rest.clone();
        let state = self.state.clone();
        let events = self.events.clone();
        let guild_settings_seq = self.guild_settings_seq.clone();
        self.runtime.spawn(async move {
            let res = rest.update_guild_settings(&guild_id.to_string(), &patch).await;
            let seq_matches = || {
                guild_settings_seq
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get(&Some(guild_id))
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
                        let mut revert = revert_payload(&previous_raw, &patch_obj);
                        if let Value::Object(map) = &mut revert {
                            map.insert("guild_id".to_owned(), Value::String(guild_id.to_string()));
                        }
                        lock(&state).apply(&Dispatch {
                            t: "USER_GUILD_SETTINGS_UPDATE".to_owned(),
                            s: 0,
                            d: revert,
                        });
                        let _ = events.send(UiEvent::ActionFailed {
                            error: format!("{err_prefix}: {err}"),
                        });
                        let _ = events.send(UiEvent::StateChanged);
                    }
                }
            }
        });
    }

    pub fn set_guild_hide_muted_channels(&self, guild_id: Snowflake, hide: bool) {
        self.update_guild_settings_fields(
            guild_id,
            GuildSettingsPatch {
                hide_muted_channels: Some(hide),
                ..Default::default()
            },
            "Could not update hidden channels filter",
        );
    }

    /// Removes `channel_id` from `channel_overrides` for `guild_id` while
    /// preserving sibling overrides, rolling back on REST error.
    pub fn remove_channel_override(&self, guild_id: Snowflake, channel_id: Snowflake) {
        let seq = {
            let mut map = self.guild_settings_seq.lock().unwrap_or_else(PoisonError::into_inner);
            let entry = map.entry(Some(guild_id)).or_insert(0);
            *entry += 1;
            *entry
        };
        let (patch, previous_entry) = {
            let mut state = self.state();
            let mut patch_overrides = BTreeMap::new();
            if let Some(settings) = state.guild_settings.get(&Some(guild_id)) {
                for (cid, ov) in &settings.channel_overrides {
                    if *cid == channel_id {
                        continue;
                    }
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
            let patch = GuildSettingsPatch {
                channel_overrides: Some(if patch_overrides.is_empty() {
                    None
                } else {
                    Some(patch_overrides)
                }),
                ..Default::default()
            };
            let mut merged = existing_channel_overrides(state.guild_settings.get(&Some(guild_id)).map(|s| &s.raw));
            let previous_entry = merged.remove(&channel_id.to_string());
            state.apply(&Dispatch {
                t: "USER_GUILD_SETTINGS_UPDATE".to_owned(),
                s: 0,
                d: json!({
                    "guild_id": guild_id.to_string(),
                    "channel_overrides": Value::Object(merged),
                }),
            });
            (patch, previous_entry)
        };
        #[cfg(test)]
        super::lock_sent(&self.sent_guild_settings).push((Some(guild_id), patch.clone()));
        let _ = self.events.send(UiEvent::StateChanged);

        let rest = self.rest.clone();
        let state = self.state.clone();
        let events = self.events.clone();
        let guild_settings_seq = self.guild_settings_seq.clone();
        self.runtime.spawn(async move {
            let res = rest.update_guild_settings(&guild_id.to_string(), &patch).await;
            let seq_matches = || {
                guild_settings_seq
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get(&Some(guild_id))
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
                        apply_channel_mute_rollback(&mut lock(&state), Some(guild_id), channel_id, previous_entry);
                        let _ = events.send(UiEvent::ActionFailed {
                            error: format!("Could not remove channel override: {err}"),
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

    use chrono::{TimeZone as _, Utc};


    #[test]
    fn mute_config_for_sets_end_time_and_ms_window() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).single().expect("valid time");
        let c = mute_config_for(now, Some(15 * 60_000)).expect("config");
        assert_eq!(c.selected_time_window, 900_000);
        assert_eq!(c.end_time.as_deref(), Some("2026-01-01T00:15:00.000Z"));
        let c = mute_config_for(now, Some(3 * 24 * 60 * 60_000)).expect("config");
        assert_eq!(c.end_time.as_deref(), Some("2026-01-04T00:00:00.000Z"));
        assert_eq!(c.selected_time_window, 259_200_000);
    }

    #[test]
    fn indefinite_mute_has_no_config() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).single().expect("valid time");
        assert!(mute_config_for(now, None).is_none());
        let patch = build_channel_override_patch_with(None, Snowflake(5), Some(true), None, None);
        let ov = &patch.channel_overrides.flatten().expect("overrides")["5"];
        assert_eq!(ov.muted, Some(true));
        assert_eq!(ov.mute_config, Some(None));
    }

    #[test]
    fn timed_mute_patch_carries_config_and_unmute_clears_it() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).single().expect("valid time");
        let cfg = mute_config_for(now, Some(60 * 60_000));
        let patch = build_channel_override_patch_with(None, Snowflake(5), Some(true), None, cfg.clone());
        let ov = &patch.channel_overrides.flatten().expect("overrides")["5"];
        assert_eq!(ov.mute_config, Some(cfg.clone()));
        let patch = build_channel_override_patch_with(None, Snowflake(5), Some(false), None, cfg);
        let ov = &patch.channel_overrides.flatten().expect("overrides")["5"];
        assert_eq!(ov.muted, Some(false));
        assert_eq!(ov.mute_config, Some(None));
    }

    #[test]
    fn expired_end_time_is_not_muted_and_rollback_keeps_sibling() {
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
                        "channel_id": target.to_string(), "muted": true,
                        "mute_config": {"end_time": "2026-01-01T00:15:00.000Z", "selected_time_window": 900000}
                    },
                    sibling.to_string(): {"channel_id": sibling.to_string(), "muted": true}
                }
            }),
        });
        let before = Utc.with_ymd_and_hms(2026, 1, 1, 0, 14, 0).single().expect("t").timestamp_millis();
        let after = Utc.with_ymd_and_hms(2026, 1, 1, 0, 16, 0).single().expect("t").timestamp_millis();
        assert!(state.is_channel_muted(Some(guild), target, before));
        assert!(!state.is_channel_muted(Some(guild), target, after));
        apply_channel_mute_rollback(&mut state, Some(guild), target, None);
        assert!(!state.is_channel_muted(Some(guild), target, before));
        assert!(state.is_channel_muted(Some(guild), sibling, before));
    }
}
