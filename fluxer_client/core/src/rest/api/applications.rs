//! Developer OAuth2 applications read-only endpoints
//! (`GET /oauth2/applications/@me`, `GET /oauth2/applications/{application_id}`).
//! Reference: `fluxer_app/src/features/devtools/models/DeveloperApplication.ts`.

use reqwest::Method;
use serde::{Deserialize, Serialize};

use super::check_snowflake;
use crate::rest::client::{Auth, RestClient};
use crate::rest::error::RestError;

/// Bot summary attached to a developer application.
/// Secret fields (such as `token`) are intentionally not deserialized.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct DeveloperApplicationBot {
    pub id: String,
    pub username: String,
    #[serde(default)]
    pub discriminator: String,
    #[serde(default)]
    pub avatar: Option<String>,
    #[serde(default)]
    pub bio: Option<String>,
    #[serde(default)]
    pub banner: Option<String>,
    #[serde(default)]
    pub flags: Option<u64>,
}

/// Developer application metadata (`DeveloperApplicationWire`).
/// Secret fields (such as `client_secret`) are intentionally not deserialized.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct DeveloperApplication {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub redirect_uris: Vec<String>,
    #[serde(default)]
    pub bot_public: bool,
    #[serde(default)]
    pub bot_require_code_grant: bool,
    #[serde(default)]
    pub bot: Option<DeveloperApplicationBot>,
}

impl RestClient {
    /// `GET /oauth2/applications/@me` -> `Vec<DeveloperApplication>`.
    pub async fn list_applications(&self) -> Result<Vec<DeveloperApplication>, RestError> {
        let value = self
            .request(Method::GET, "/oauth2/applications/@me", &[], None, Auth::Required)
            .await?;
        serde_json::from_value(value).map_err(|e| RestError::Decode(format!("applications list: {e}")))
    }

    /// `GET /oauth2/applications/{application_id}` -> `DeveloperApplication`.
    pub async fn get_application(&self, application_id: &str) -> Result<DeveloperApplication, RestError> {
        check_snowflake(application_id, "application_id")?;
        let path = format!("/oauth2/applications/{application_id}");
        let value = self.request(Method::GET, &path, &[], None, Auth::Required).await?;
        serde_json::from_value(value).map_err(|e| RestError::Decode(format!("application: {e}")))
    }
}
