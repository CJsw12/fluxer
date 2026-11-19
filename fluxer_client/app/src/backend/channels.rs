//! Guild channel CRUD actions on [`Backend`]: create (`POST /guilds/{guild_id}/channels`),
//! update (`PATCH /channels/{channel_id}`), and delete (`DELETE /channels/{channel_id}`).

use fluxer_client_core::Dispatch;
use fluxer_client_core::rest::{CreateGuildChannelRequest, GuildChannelPatch};
use fluxer_client_core::state::Snowflake;
use serde_json::{Value, json};

use super::{Backend, UiEvent, lock};
#[cfg(test)]
use super::lock_sent;

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuildChannelCall {
    Create { guild_id: Snowflake, request: CreateGuildChannelRequest },
    Update { channel_id: Snowflake, patch: GuildChannelPatch },
    Delete { channel_id: Snowflake },
}

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
}
