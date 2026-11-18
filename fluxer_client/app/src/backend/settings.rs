//! Optimistic `PATCH /users/@me/settings` on [`Backend`].

use std::sync::atomic::Ordering;

use fluxer_client_core::Dispatch;
use fluxer_client_core::rest::UserSettingsPatch;
use serde_json::{Map, Value};

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
                }
                let _ = events.send(UiEvent::ActionFailed { error: format!("Could not save settings: {err}") });
                let _ = events.send(UiEvent::StateChanged);
            }
        });
    }
}
