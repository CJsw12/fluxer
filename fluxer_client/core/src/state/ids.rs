//! Snowflake ids and the bitfield-as-string wire helpers.

use std::fmt;

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};

/// Fluxer epoch in Unix milliseconds (`packages/constants/src/Core.ts:3`).
pub const FLUXER_EPOCH_MS: u64 = 1_420_070_400_000;
/// Worker id bits + sequence bits (`packages/snowflake/src/Snowflake.ts:6-9`).
const TIMESTAMP_SHIFT: u32 = 22;

/// A snowflake id. On the wire it is a decimal string; numbers are accepted too.
/// Serializes back to a string so wire round-trips are lossless.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Snowflake(pub u64);

impl Snowflake {
    /// Creation time in Unix milliseconds.
    pub fn timestamp_ms(self) -> u64 {
        (self.0 >> TIMESTAMP_SHIFT) + FLUXER_EPOCH_MS
    }

    /// Smallest snowflake for a Unix-ms timestamp (worker 0, sequence 0).
    /// Used for client nonces, like `SnowflakeUtils.fromTimestamp`.
    pub fn from_unix_ms(ms: u64) -> Self {
        Snowflake(ms.saturating_sub(FLUXER_EPOCH_MS) << TIMESTAMP_SHIFT)
    }

    /// Parses a decimal snowflake string.
    pub fn parse(s: &str) -> Option<Self> {
        s.parse::<u64>().ok().map(Snowflake)
    }
}

impl fmt::Display for Snowflake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Serialize for Snowflake {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Snowflake {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(U64Visitor).map(Snowflake)
    }
}

/// Accepts a u64 from either a JSON number or a decimal string.
struct U64Visitor;

impl Visitor<'_> for U64Visitor {
    type Value = u64;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an unsigned integer or a decimal string")
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<u64, E> {
        Ok(v)
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<u64, E> {
        u64::try_from(v).map_err(|_| E::custom("negative id or bitfield"))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<u64, E> {
        v.parse::<u64>().map_err(E::custom)
    }
}

/// serde helper (`#[serde(with)]`) for permission bitfields sent as decimal strings
/// (`PermissionStringType`). Serializes back to a string.
pub(crate) mod bitfield_str {
    use super::U64Visitor;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(v)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        deserializer.deserialize_any(U64Visitor)
    }
}
