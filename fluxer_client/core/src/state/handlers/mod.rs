//! Dispatch reducers, one module per domain. Each module adds
//! `impl ClientState` methods (`apply_<event>`) that `ClientState::apply_at`
//! routes to with a one-line arm, plus the domain's model types and
//! queries. New events go in their own module so parallel work does not
//! touch the same file.

pub(super) mod channels;
pub(super) mod emojis;
pub(super) mod guilds;
pub(super) mod member_list;
pub(super) mod members;
pub(super) mod messages;
pub(super) mod notes;
pub(super) mod presence;
pub(super) mod reactions;
pub(super) mod read_states;
pub(super) mod ready;
pub(super) mod relationships;
pub(super) mod typing;
pub(super) mod user_settings;
pub(super) mod users;
pub(super) mod voice;

use serde::de::DeserializeOwned;
use serde_json::Value;

use super::ids::Snowflake;

/// Deserialize every array item that parses; log and skip the rest so one
/// malformed entity does not drop a whole READY. `None`/non-arrays are empty.
fn parse_list<T: DeserializeOwned>(value: Option<&Value>, what: &str) -> Vec<T> {
    let Some(items) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| match serde_json::from_value::<T>(item.clone()) {
            Ok(v) => Some(v),
            Err(err) => {
                tracing::debug!(what, %err, "skipping malformed entity");
                None
            }
        })
        .collect()
}

fn parse<T: DeserializeOwned>(value: &Value, what: &str) -> Option<T> {
    match serde_json::from_value::<T>(value.clone()) {
        Ok(v) => Some(v),
        Err(err) => {
            tracing::debug!(what, %err, "malformed dispatch payload");
            None
        }
    }
}

/// A snowflake field given as a string or a number.
fn id_field(value: &Value, key: &str) -> Option<Snowflake> {
    id_value(value.get(key)?)
}

fn id_value(value: &Value) -> Option<Snowflake> {
    match value {
        Value::String(s) => Snowflake::parse(s),
        Value::Number(n) => n.as_u64().map(Snowflake),
        _ => None,
    }
}

fn is_newer(a: Snowflake, b: Option<Snowflake>) -> bool {
    b.is_none_or(|b| a > b)
}
