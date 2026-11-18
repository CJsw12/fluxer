//! User notes: READY `notes` and USER_NOTE_UPDATE (`UserNoteController.ts`).
//! Notes are private to the current user, so they are never emitted by
//! `apply_ready`; UI work after READY reads them through [`ClientState::note`].

use serde_json::Value;

use super::id_field;
use crate::state::client::{ClientState, StateChange};
use crate::state::ids::Snowflake;

impl ClientState {
    /// `loadNotes`: READY `notes` is `{user_id: note}`. Keys that are not
    /// snowflakes and empty notes are skipped.
    pub(in crate::state) fn load_ready_notes(&mut self, d: &Value) {
        let Some(notes) = d.get("notes").and_then(Value::as_object) else {
            return;
        };
        for (key, value) in notes {
            let Some(user_id) = Snowflake::parse(key) else { continue };
            let Some(note) = value.as_str().filter(|note| !note.is_empty()) else { continue };
            self.notes.insert(user_id, note.to_owned());
        }
    }

    /// USER_NOTE_UPDATE `{id, note}`: a `null` note removes the entry.
    pub(in crate::state) fn apply_user_note_update(&mut self, d: &Value, out: &mut Vec<StateChange>) {
        let Some(user_id) = id_field(d, "id") else { return };
        match d.get("note").and_then(Value::as_str).filter(|note| !note.is_empty()) {
            Some(note) => {
                self.notes.insert(user_id, note.to_owned());
            }
            None => {
                self.notes.remove(&user_id);
            }
        }
        out.push(StateChange::NotesChanged);
    }

    /// The note on a user, if any.
    pub fn note(&self, user_id: Snowflake) -> Option<&str> {
        self.notes.get(&user_id).map(String::as_str)
    }

    /// Local echo of a note edit before the server confirms it. `None` or an
    /// empty note removes the entry.
    pub fn set_local_note(&mut self, user_id: Snowflake, note: Option<String>) {
        match note.filter(|note| !note.is_empty()) {
            Some(note) => {
                self.notes.insert(user_id, note);
            }
            None => {
                self.notes.remove(&user_id);
            }
        }
    }
}
