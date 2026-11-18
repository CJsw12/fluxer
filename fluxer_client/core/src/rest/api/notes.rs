//! User notes (`PUT /users/@me/notes/{user_id}`). Route:
//! `fluxer_api/src/api/user/controllers/UserNoteController.ts`.

use reqwest::Method;
use serde_json::json;

use super::check_snowflake;
use crate::rest::client::{Auth, RestClient};
use crate::rest::error::RestError;

impl RestClient {
    /// `PUT /users/@me/notes/{user_id}` with `{note}` (204). An empty or
    /// missing note is sent as `null`, clearing it.
    pub async fn set_user_note(&self, user_id: &str, note: Option<&str>) -> Result<(), RestError> {
        check_snowflake(user_id, "user_id")?;
        let body = json!({ "note": note.filter(|s| !s.is_empty()) });
        let path = format!("/users/@me/notes/{user_id}");
        self.request(Method::PUT, &path, &[], Some(&body), Auth::Required).await?;
        Ok(())
    }
}
