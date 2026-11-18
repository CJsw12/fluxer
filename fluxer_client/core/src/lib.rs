//! Fluxer client core: gateway connection, REST/auth, and client state.
//! UI-free; the GPUI app and tests drive it.

pub mod gateway;
pub mod rest;
pub mod search;
pub mod session;
pub mod state;

/// A gateway DISPATCH (op 0) frame handed from `gateway` to `state`.
/// This is the only contract between the two modules.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct Dispatch {
    /// Event name, e.g. "READY", "MESSAGE_CREATE".
    pub t: String,
    /// Sequence number.
    pub s: u64,
    /// Raw event payload.
    pub d: serde_json::Value,
}
