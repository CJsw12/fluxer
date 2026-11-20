//! Guild channel CRUD actions on [`Backend`]: create (`POST /guilds/{guild_id}/channels`),
//! update (`PATCH /channels/{channel_id}`), and delete (`DELETE /channels/{channel_id}`).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use fluxer_client_core::Dispatch;
use fluxer_client_core::rest::{CreateGuildChannelRequest, GuildChannelPatch, RestClient, RestError};
use fluxer_client_core::state::{Channel, ClientState, PermissionOverwrite, Snowflake};
use serde_json::{Value, json};
use tokio::sync::mpsc;

#[cfg(test)]
use super::lock_sent;
use super::{Backend, UiEvent, lock};

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuildChannelCall {
    Create {
        guild_id: Snowflake,
        request: CreateGuildChannelRequest,
    },
    Update {
        channel_id: Snowflake,
        patch: GuildChannelPatch,
    },
    Delete {
        channel_id: Snowflake,
    },
    SyncMove {
        channel_id: Snowflake,
        parent_id: Snowflake,
        settings: GuildChannelPatch,
    },
}

pub(super) struct PendingChannelSync {
    session: Option<String>,
    guild: Snowflake,
    parent: Snowflake,
    previous_overwrites: Vec<PermissionOverwrite>,
}

pub(super) type PendingChannelSyncs = Arc<Mutex<BTreeMap<Snowflake, PendingChannelSync>>>;

impl Backend {
    /// `POST /guilds/{guild_id}/channels`, echoing `seq`.
    pub fn create_guild_channel(&self, seq: u64, guild_id: Snowflake, request: CreateGuildChannelRequest) {
        #[cfg(test)]
        lock_sent(&self.sent_guild_channels).push(GuildChannelCall::Create {
            guild_id,
            request: request.clone(),
        });
        let rest = self.rest.clone();
        let state = self.state.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let event = match rest.create_guild_channel(&guild_id.to_string(), &request).await {
                Ok(body) => {
                    let channel_id = body.get("id").and_then(Value::as_str).and_then(Snowflake::parse);
                    match channel_id {
                        Some(id) => {
                            let dispatch = Dispatch { t: "CHANNEL_CREATE".to_owned(), s: 0, d: body };
                            lock(&state).apply(&dispatch);
                            let _ = events.send(UiEvent::StateChanged);
                            UiEvent::ChannelCreated { seq, guild_id, result: Ok(id) }
                        }
                        None => UiEvent::ChannelCreated {
                            seq,
                            guild_id,
                            result: Err("Could not create channel: bad response".to_owned()),
                        },
                    }
                }
                Err(err) => UiEvent::ChannelCreated { seq, guild_id, result: Err(err.to_string()) },
            };
            let _ = events.send(event);
        });
    }

    /// `PATCH /channels/{channel_id}`, echoing `seq`.
    pub fn update_guild_channel(&self, seq: u64, channel_id: Snowflake, patch: GuildChannelPatch) {
        #[cfg(test)]
        lock_sent(&self.sent_guild_channels).push(GuildChannelCall::Update {
            channel_id,
            patch: patch.clone(),
        });
        let rest = self.rest.clone();
        let state = self.state.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let event = match rest.update_guild_channel(&channel_id.to_string(), &patch).await {
                Ok(body) => {
                    lock(&state).apply(&Dispatch { t: "CHANNEL_UPDATE".to_owned(), s: 0, d: body });
                    let _ = events.send(UiEvent::StateChanged);
                    UiEvent::ChannelUpdated { seq, channel_id, error: None }
                }
                Err(err) => UiEvent::ChannelUpdated { seq, channel_id, error: Some(err.to_string()) },
            };
            let _ = events.send(event);
        });
    }

    /// `DELETE /channels/{channel_id}`, echoing `seq`.
    pub fn delete_guild_channel(&self, seq: u64, channel_id: Snowflake) {
        #[cfg(test)]
        lock_sent(&self.sent_guild_channels).push(GuildChannelCall::Delete { channel_id });
        let rest = self.rest.clone();
        let state = self.state.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let event = match rest.delete_channel(&channel_id.to_string()).await {
                Ok(()) => {
                    lock(&state).apply(&Dispatch {
                        t: "CHANNEL_DELETE".to_owned(),
                        s: 0,
                        d: json!({ "id": channel_id.to_string() }),
                    });
                    let _ = events.send(UiEvent::StateChanged);
                    UiEvent::ChannelDeleted { seq, channel_id, error: None }
                }
                Err(err) => UiEvent::ChannelDeleted { seq, channel_id, error: Some(err.to_string()) },
            };
            let _ = events.send(event);
        });
    }

    pub fn move_channel_and_sync(
        &self,
        seq: u64,
        guild_id: Snowflake,
        channel_id: Snowflake,
        expected_parent: Option<Snowflake>,
        parent_id: Snowflake,
        settings: GuildChannelPatch,
    ) {
        #[cfg(test)]
        lock_sent(&self.sent_guild_channels).push(GuildChannelCall::SyncMove {
            channel_id,
            parent_id,
            settings: settings.clone(),
        });
        let rest = self.rest.clone();
        let state = self.state.clone();
        let events = self.events.clone();
        let pending = self.parent_syncs.clone();
        self.runtime.spawn(async move {
            let before = {
                let state = lock(&state);
                state
                    .channels
                    .get(&channel_id)
                    .filter(|c| c.guild_id == Some(guild_id) && c.parent_id == expected_parent)
                    .filter(|_| state.can_sync_channel_parent(channel_id, parent_id))
                    .map(|c| (state.session_id.clone(), c.clone()))
            };
            let Some((session, before)) = before else {
                let _ = events.send(UiEvent::ChannelUpdated {
                    seq,
                    channel_id,
                    error: Some("The channel or permissions changed. Reopen the editor.".to_owned()),
                });
                return;
            };
            lock(&pending).remove(&channel_id);
            let result = rest.move_channel_and_sync(guild_id, channel_id, parent_id).await;
            let mut settings_error = None;
            if result.is_ok() && !settings.is_empty() {
                settings_error = rest
                    .update_guild_channel(&channel_id.to_string(), &settings)
                    .await
                    .err();
            }
            // The server can commit the move and emit it before it attempts to sync.
            let mut refreshed = fetch_sync_channel(&rest, guild_id, channel_id).await;
            if refreshed
                .as_ref()
                .is_err_and(|err| err.status().is_none_or(|status| status >= 500))
            {
                refreshed = fetch_sync_channel(&rest, guild_id, channel_id).await;
            }
            {
                let mut state = lock(&state);
                if state.session_id == session
                    && let Some(current) = state.channels.get(&channel_id)
                {
                    let unchanged = current.permission_overwrites == before.permission_overwrites;
                    let current_parent = current.parent_id;
                    let needs_recheck = current_parent == expected_parent
                        || (current_parent == Some(parent_id) && refreshed.is_err());
                    if unchanged && needs_recheck {
                        lock(&pending).insert(
                            channel_id,
                            PendingChannelSync {
                                session,
                                guild: guild_id,
                                parent: parent_id,
                                previous_overwrites: before.permission_overwrites.clone(),
                            },
                        );
                    }
                    if refreshed.as_ref().is_err_and(access_was_denied) {
                        if unchanged && (current_parent == expected_parent || current_parent == Some(parent_id)) {
                            state.set_denied_channel_access(channel_id, true);
                        }
                    } else if let Ok(after) = &refreshed {
                        let mut patch = json!({ "id": channel_id });
                        if current.parent_id == expected_parent || current.parent_id == after.parent_id {
                            if current.parent_id == before.parent_id && current.position == before.position {
                                patch["parent_id"] = json!(after.parent_id);
                                patch["position"] = json!(after.position);
                            }
                            if current.permission_overwrites == before.permission_overwrites {
                                patch["permission_overwrites"] = json!(after.permission_overwrites);
                            }
                        }
                        if result.is_ok() && settings_error.is_none() {
                            if settings.name.is_some() && current.name == before.name {
                                patch["name"] = json!(after.name);
                            }
                            if settings.topic.is_some() && current.topic == before.topic {
                                patch["topic"] = json!(after.topic);
                            }
                            if settings.kind.is_some() && current.kind == before.kind {
                                patch["type"] = json!(after.kind);
                            }
                            if settings.nsfw_override.is_some() && current.nsfw == before.nsfw {
                                patch["nsfw"] = json!(after.nsfw);
                            }
                            if settings.rate_limit_per_user.is_some()
                                && current.rate_limit_per_user == before.rate_limit_per_user
                            {
                                patch["rate_limit_per_user"] = json!(after.rate_limit_per_user);
                            }
                        }
                        if unchanged && current_parent == after.parent_id {
                            state.set_denied_channel_access(channel_id, false);
                        }
                        state.apply(&Dispatch {
                            t: "CHANNEL_UPDATE".to_owned(),
                            s: 0,
                            d: patch,
                        });
                    }
                }
            }
            let error = match (result, refreshed) {
                (Ok(()), _) if let Some(error) = settings_error => Some(format!(
                    "Moved and synced, but other channel settings could not be saved: {error}"
                )),
                (Ok(()), Ok(_)) => None,
                (Ok(()), Err(error)) => Some(format!(
                    "Moved and synced, but the channel could not be refreshed: {error}"
                )),
                (Err(error), Ok(after)) if after.parent_id == Some(parent_id) => {
                    Some(format!("Channel moved, but permissions could not be synced: {error}"))
                }
                (Err(error), _) => Some(error.to_string()),
            };
            let _ = events.send(UiEvent::StateChanged);
            let _ = events.send(UiEvent::ChannelUpdated { seq, channel_id, error });
        });
    }
}

async fn fetch_sync_channel(rest: &RestClient, guild: Snowflake, id: Snowflake) -> Result<Channel, RestError> {
    let value = rest.get_guild_channel(id).await?;
    if !value.get("permission_overwrites").is_some_and(Value::is_array) {
        return Err(RestError::Decode(
            "Channel permissions missing from response".to_owned(),
        ));
    }
    let channel: Channel = serde_json::from_value(value).map_err(|error| RestError::Decode(error.to_string()))?;
    if channel.id != id || channel.guild_id != Some(guild) {
        return Err(RestError::Decode("Channel did not match the request".to_owned()));
    }
    Ok(channel)
}

pub(super) fn access_was_denied(error: &RestError) -> bool {
    matches!(error.code(), Some("MISSING_ACCESS" | "UNKNOWN_CHANNEL"))
}

pub(super) fn recheck_parent_sync(
    dispatch: &Dispatch,
    state: &Arc<Mutex<ClientState>>,
    rest: &Arc<RestClient>,
    events: &mpsc::UnboundedSender<UiEvent>,
    pending: &PendingChannelSyncs,
) {
    let updates = match dispatch.t.as_str() {
        "READY" => {
            lock(pending).clear();
            return;
        }
        "CHANNEL_UPDATE" | "CHANNEL_DELETE" => std::slice::from_ref(&dispatch.d),
        "CHANNEL_UPDATE_BULK" => dispatch
            .d
            .get("channels")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice),
        _ => return,
    };
    for update in updates {
        let Some(id) = update.get("id").and_then(Value::as_str).and_then(Snowflake::parse) else {
            continue;
        };
        let sync = {
            let state = lock(state);
            take_parent_sync(&state, &mut lock(pending), id)
        };
        let Some(sync) = sync else {
            continue;
        };
        // The move dispatch is built before the server copies the category's permissions.
        let (state, rest, events, pending) = (state.clone(), rest.clone(), events.clone(), pending.clone());
        tokio::spawn(async move {
            let refreshed = fetch_sync_channel(&rest, sync.guild, id).await;
            let mut state = lock(&state);
            if is_pre_sync_snapshot(&state, id, &sync) {
                match refreshed {
                    Ok(channel) if channel.parent_id == Some(sync.parent) => {
                        state.set_denied_channel_access(id, false);
                        state.apply(&Dispatch {
                            t: "CHANNEL_UPDATE".to_owned(),
                            s: 0,
                            d: json!({ "id": id, "permission_overwrites": channel.permission_overwrites }),
                        });
                    }
                    Err(error) if access_was_denied(&error) => state.set_denied_channel_access(id, true),
                    Err(_) => {
                        lock(&pending).entry(id).or_insert(sync);
                        return;
                    }
                    Ok(_) => return,
                }
                let _ = events.send(UiEvent::StateChanged);
            }
        });
    }
}

fn take_parent_sync(
    state: &ClientState,
    pending: &mut BTreeMap<Snowflake, PendingChannelSync>,
    id: Snowflake,
) -> Option<PendingChannelSync> {
    let sync = pending.get(&id)?;
    if state.session_id != sync.session || !state.channels.contains_key(&id) {
        pending.remove(&id);
        return None;
    }
    if is_pre_sync_snapshot(state, id, sync) {
        return pending.remove(&id);
    }
    None
}

fn is_pre_sync_snapshot(state: &ClientState, id: Snowflake, sync: &PendingChannelSync) -> bool {
    state.session_id == sync.session
        && state.channels.get(&id).is_some_and(|channel| {
            channel.guild_id == Some(sync.guild)
                && channel.parent_id == Some(sync.parent)
                && channel.permission_overwrites == sync.previous_overwrites
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn late_parent_move_detects_only_pre_sync_permissions_in_the_same_session() {
        let mut state = ClientState::new();
        let channel = Snowflake(7);
        let initial = json!({ "id": channel, "guild_id": "1", "parent_id": "2", "permission_overwrites": [] });
        state.channels.insert(channel, serde_json::from_value(initial).unwrap());
        let sync = PendingChannelSync {
            session: None,
            guild: Snowflake(1),
            parent: Snowflake(3),
            previous_overwrites: Vec::new(),
        };
        assert!(!is_pre_sync_snapshot(&state, channel, &sync));
        let mut pending = BTreeMap::from([(channel, sync)]);
        assert!(take_parent_sync(&state, &mut pending, channel).is_none());
        assert!(pending.contains_key(&channel));
        state.apply(&Dispatch {
            t: "CHANNEL_UPDATE_BULK".to_owned(),
            s: 1,
            d: json!({ "channels": [{ "id": channel, "parent_id": "3", "permission_overwrites": [] }] }),
        });
        let sync = take_parent_sync(&state, &mut pending, channel).unwrap();
        assert!(pending.is_empty());
        assert!(is_pre_sync_snapshot(&state, channel, &sync));
        state.apply(&Dispatch {
            t: "CHANNEL_UPDATE".to_owned(),
            s: 2,
            d: json!({ "id": channel, "permission_overwrites": [{ "id": "1", "type": 0, "deny": "2048" }] }),
        });
        assert!(!is_pre_sync_snapshot(&state, channel, &sync));
        state.channels.get_mut(&channel).unwrap().permission_overwrites.clear();
        state.session_id = Some("new-session".to_owned());
        assert!(!is_pre_sync_snapshot(&state, channel, &sync));
    }
}
