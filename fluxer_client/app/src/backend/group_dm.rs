//! Group DM actions on [`Backend`]: create (`POST /users/@me/channels`),
//! rename (`PATCH /channels/{id}`), add/remove recipients, and leave (`DELETE /channels/{id}`).

use std::sync::PoisonError;

use fluxer_client_core::Dispatch;
use fluxer_client_core::rest::GroupDmPatch;
use fluxer_client_core::state::Snowflake;
use serde_json::{Value, json};

use super::{Backend, UiEvent, lock};
#[cfg(test)]
use super::lock_sent;

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupDmCall {
    Create(Vec<Snowflake>),
    Rename { channel_id: Snowflake, name: Option<String> },
    AddRecipient { channel_id: Snowflake, user_id: Snowflake },
    RemoveRecipient { channel_id: Snowflake, user_id: Snowflake },
    Leave { channel_id: Snowflake, silent: bool },
}

impl Backend {
    /// `POST /users/@me/channels` with `{ recipients }`, echoing `seq`.
    pub fn create_group_dm(&self, seq: u64, recipients: Vec<Snowflake>) {
        #[cfg(test)]
        lock_sent(&self.sent_group_dm).push(GroupDmCall::Create(recipients.clone()));
        let rest = self.rest.clone();
        let state = self.state.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let ids: Vec<String> = recipients.iter().map(Snowflake::to_string).collect();
            let event = match rest.create_group_dm(&ids).await {
                Ok(body) => {
                    let channel_id = body.get("id").and_then(Value::as_str).and_then(Snowflake::parse);
                    match channel_id {
                        Some(id) => {
                            let dispatch = Dispatch { t: "CHANNEL_CREATE".to_owned(), s: 0, d: body };
                            lock(&state).apply(&dispatch);
                            let _ = events.send(UiEvent::StateChanged);
                            UiEvent::GroupDmCreated { seq, result: Ok(id) }
                        }
                        None => UiEvent::GroupDmCreated {
                            seq,
                            result: Err("Could not create group DM: bad response".to_owned()),
                        },
                    }
                }
                Err(err) => UiEvent::GroupDmCreated { seq, result: Err(err.to_string()) },
            };
            let _ = events.send(event);
        });
    }

    /// `PATCH /channels/{id}` updating or clearing (`None`) the custom group name.
    pub fn rename_group_dm(&self, channel_id: Snowflake, name: Option<String>) {
        #[cfg(test)]
        lock_sent(&self.sent_group_dm).push(GroupDmCall::Rename { channel_id, name: name.clone() });
        let seq = {
            let mut map = self.rename_group_dm_seq.lock().unwrap_or_else(PoisonError::into_inner);
            let next = map.get(&channel_id).copied().unwrap_or(0).saturating_add(1);
            map.insert(channel_id, next);
            next
        };
        let rest = self.rest.clone();
        let state = self.state.clone();
        let seq_map = self.rename_group_dm_seq.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let patch = GroupDmPatch { name: Some(name), owner_id: None };
            let result = rest.update_group_dm(&channel_id.to_string(), &patch).await;
            let stale = seq_map.lock().unwrap_or_else(PoisonError::into_inner).get(&channel_id).copied().unwrap_or(0) != seq;
            if stale {
                return;
            }
            match result {
                Ok(body) => {
                    lock(&state).apply(&Dispatch { t: "CHANNEL_UPDATE".to_owned(), s: 0, d: body });
                    let _ = events.send(UiEvent::StateChanged);
                }
                Err(err) => {
                    let _ = events.send(UiEvent::ActionFailed {
                        error: format!("Could not rename group DM: {err}"),
                    });
                }
            }
        });
    }

    /// `PUT /channels/{id}/recipients/{user_id}`.
    pub fn add_group_dm_recipient(&self, channel_id: Snowflake, user_id: Snowflake) {
        #[cfg(test)]
        lock_sent(&self.sent_group_dm).push(GroupDmCall::AddRecipient { channel_id, user_id });
        let rest = self.rest.clone();
        let state = self.state.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            match rest.add_group_dm_recipient(&channel_id.to_string(), &user_id.to_string()).await {
                Ok(()) => {
                    let user_json = {
                        let st = lock(&state);
                        st.users.get(&user_id).and_then(|u| serde_json::to_value(u).ok())
                    };
                    if let Some(user) = user_json {
                        lock(&state).apply(&Dispatch {
                            t: "CHANNEL_RECIPIENT_ADD".to_owned(),
                            s: 0,
                            d: json!({ "channel_id": channel_id.to_string(), "user": user }),
                        });
                    }
                    let _ = events.send(UiEvent::StateChanged);
                }
                Err(err) => {
                    let _ = events.send(UiEvent::ActionFailed {
                        error: format!("Could not add member: {err}"),
                    });
                }
            }
        });
    }

    /// `DELETE /channels/{id}/recipients/{user_id}`.
    pub fn remove_group_dm_recipient(&self, channel_id: Snowflake, user_id: Snowflake) {
        #[cfg(test)]
        lock_sent(&self.sent_group_dm).push(GroupDmCall::RemoveRecipient { channel_id, user_id });
        let rest = self.rest.clone();
        let state = self.state.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            match rest.remove_group_dm_recipient(&channel_id.to_string(), &user_id.to_string(), false).await {
                Ok(()) => {
                    lock(&state).apply(&Dispatch {
                        t: "CHANNEL_RECIPIENT_REMOVE".to_owned(),
                        s: 0,
                        d: json!({ "channel_id": channel_id.to_string(), "user": { "id": user_id.to_string() } }),
                    });
                    let _ = events.send(UiEvent::StateChanged);
                }
                Err(err) => {
                    let _ = events.send(UiEvent::ActionFailed {
                        error: format!("Could not remove member: {err}"),
                    });
                }
            }
        });
    }

    /// `DELETE /channels/{id}`: leaves the group DM cleanly.
    pub fn leave_group_dm(&self, channel_id: Snowflake, silent: bool) {
        #[cfg(test)]
        lock_sent(&self.sent_group_dm).push(GroupDmCall::Leave { channel_id, silent });
        let rest = self.rest.clone();
        let state = self.state.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            match rest.leave_channel(&channel_id.to_string(), silent).await {
                Ok(()) => {
                    lock(&state).apply(&Dispatch {
                        t: "CHANNEL_DELETE".to_owned(),
                        s: 0,
                        d: json!({ "id": channel_id.to_string() }),
                    });
                    let _ = events.send(UiEvent::GroupDmLeft { channel_id, error: None });
                    let _ = events.send(UiEvent::StateChanged);
                }
                Err(err) => {
                    let _ = events.send(UiEvent::GroupDmLeft {
                        channel_id,
                        error: Some(format!("Could not leave group: {err}")),
                    });
                }
            }
        });
    }
}
