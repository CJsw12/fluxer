//! Secret wrappers. Their `Debug`/`Display` never print the value, so a
//! secret cannot reach logs, tracing fields or error strings by accident.

use std::fmt;

use zeroize::Zeroize;

/// A session token returned by login. Sent as the raw `Authorization`
/// header value (no `Bearer` prefix), exactly like the TS client.
/// The buffer is zeroed on drop (clones are separate buffers, each zeroed
/// when dropped).
#[derive(Clone, PartialEq, Eq)]
pub struct SecretToken(String);

impl SecretToken {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The raw token. Only call this to build a request header or to hand
    /// the token to secure storage.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl Drop for SecretToken {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl fmt::Debug for SecretToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretToken(***)")
    }
}

impl fmt::Display for SecretToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

/// Any other secret string: password, MFA ticket, TOTP code. Zeroed on
/// drop like [`SecretToken`].
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

/// JSON keys whose values are removed before a response body is kept in an
/// error or logged.
const REDACTED_KEYS: &[&str] = &[
    "token",
    "password",
    "ticket",
    "device_proof",
    "poll_secret",
    "email",
    "phone",
    "authorization",
];

/// Returns a copy of `value` with secret-bearing keys replaced by `"***"`
/// at any depth.
pub fn redact_json(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (key, inner) in map {
                let lowered = key.to_ascii_lowercase();
                if REDACTED_KEYS.contains(&lowered.as_str()) {
                    out.insert(key.clone(), serde_json::Value::String("***".to_owned()));
                } else {
                    out.insert(key.clone(), redact_json(inner));
                }
            }
            serde_json::Value::Object(out)
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(redact_json).collect())
        }
        other => other.clone(),
    }
}
