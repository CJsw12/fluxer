//! Read marking from shortcuts and opening DMs from the quick switcher.

use fluxer_client_core::Dispatch;
use fluxer_client_core::rest::AckRequest;
use fluxer_client_core::state::Snowflake;

use super::{Backend, UiEvent, lock};

/// `ReadStateAckRequest.read_states` max length.
const ACKS_PER_REQUEST: usize = 100;

impl Backend {
    /// Marks several channels read at once (`ReadStateCommands.bulkAckChannels`):
    /// local state now, then `POST /read-states/ack` in pages of 100.
    pub fn ack_many(&self, acks: &[(Snowflake, Snowflake)]) {
        if acks.is_empty() {
            return;
        }
        let changed = {
            let mut state = self.state();
            let mut changed = false;
            for (channel_id, message_id) in acks {
                changed |= !state.local_ack(*channel_id, *message_id).is_empty();
            }
            changed
        };
        if changed {
            let _ = self.events.send(UiEvent::StateChanged);
        }
        let requests: Vec<AckRequest> = acks
            .iter()
            .map(|(c, m)| AckRequest {
                channel_id: c.to_string(),
                message_id: m.to_string(),
                mention_count: None,
                manual: false,
            })
            .collect();
        let rest = self.rest.clone();
        self.runtime.spawn(async move {
            for page in requests.chunks(ACKS_PER_REQUEST) {
                if let Err(err) = rest.ack_messages(page).await {
                    tracing::warn!(%err, "bulk read ack failed");
                }
            }
        });
    }

    /// `POST /users/@me/channels`: opens (or finds) the DM with `user_id`,
    /// adds it to the state and sends `UiEvent::DmOpened`.
    pub fn open_dm(&self, user_id: Snowflake) {
        #[cfg(test)]
        super::lock_sent(&self.opened_dms).push(user_id);
        let rest = self.rest.clone();
        let state = self.state.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let event = match rest.open_dm(&user_id.to_string()).await {
                Ok(body) => {
                    let channel_id = body.get("id").and_then(|v| v.as_str()).and_then(Snowflake::parse);
                    match channel_id {
                        Some(channel_id) => {
                            let dispatch = Dispatch { t: "CHANNEL_CREATE".to_owned(), s: 0, d: body };
                            lock(&state).apply(&dispatch);
                            UiEvent::DmOpened { channel_id }
                        }
                        None => UiEvent::ActionFailed { error: "Could not open the DM: bad response".to_owned() },
                    }
                }
                Err(err) => UiEvent::ActionFailed { error: format!("Could not open the DM: {err}") },
            };
            let _ = events.send(event);
        });
    }
}
