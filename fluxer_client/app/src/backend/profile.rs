//! Profile fetch and private user notes on [`Backend`].

use std::sync::atomic::Ordering;

use fluxer_client_core::rest::{ProfileQuery, UserProfileFull};
use fluxer_client_core::state::Snowflake;

use super::{Backend, UiEvent, lock};

impl Backend {
    /// `GET /users/{id}/profile`: loads the global and optional guild-member
    /// profile; result arrives as `UiEvent::ProfileLoaded`.
    pub fn request_profile(&self, user_id: Snowflake, guild_id: Option<Snowflake>) {
        let rest = self.rest.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let query = ProfileQuery {
                guild_id: guild_id.map(|id| id.to_string()),
                with_mutual_friends: false,
                with_mutual_guilds: true,
            };
            let result = match rest.get_user_profile(&user_id.to_string(), &query).await {
                Ok(body) => serde_json::from_value::<UserProfileFull>(body).map_err(|err| err.to_string()),
                Err(err) => Err(err.to_string()),
            };
            let _ = events.send(UiEvent::ProfileLoaded { user_id, guild_id, result });
        });
    }

    /// `PUT /users/@me/notes/{user_id}`: updates the local note immediately
    /// and rolls back if the request fails. Empty or whitespace-only notes
    /// clear the entry.
    pub fn save_note(&self, user_id: Snowflake, note: Option<String>) {
        let cleaned = note.and_then(|s| {
            let t = s.trim();
            (!t.is_empty()).then(|| t.to_owned())
        });
        #[cfg(test)]
        super::lock_sent(&self.saved_notes).push((user_id, cleaned.clone()));
        let seq = self.note_seq.fetch_add(1, Ordering::Relaxed) + 1;
        let previous = {
            let mut state = self.state();
            let prev = state.note(user_id).map(str::to_owned);
            state.set_local_note(user_id, cleaned.clone());
            prev
        };
        let _ = self.events.send(UiEvent::StateChanged);
        let rest = self.rest.clone();
        let state = self.state.clone();
        let events = self.events.clone();
        let note_seq = self.note_seq.clone();
        self.runtime.spawn(async move {
            let event = match rest.set_user_note(&user_id.to_string(), cleaned.as_deref()).await {
                Ok(()) => UiEvent::NoteSaved { user_id, error: None },
                Err(err) => {
                    if note_seq.load(Ordering::Relaxed) == seq {
                        lock(&state).set_local_note(user_id, previous);
                        let _ = events.send(UiEvent::StateChanged);
                    }
                    UiEvent::NoteSaved { user_id, error: Some(err.to_string()) }
                }
            };
            let _ = events.send(event);
        });
    }
}
