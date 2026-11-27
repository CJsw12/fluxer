//! `GET /users/{id}/profile` response types (`UserProfileFullResponse`).
//! Every field tolerates `null` (and absence) so one odd field does not
//! drop the whole profile.

use serde::{Deserialize, Deserializer, Serialize};

use crate::state::Snowflake;
use super::UserConnection;

/// The profile explicitly cleared its avatar (`PROFILE_FLAGS.AVATAR_UNSET`).
pub const PROFILE_FLAG_AVATAR_UNSET: u32 = 1 << 0;
/// The profile explicitly cleared its banner (`PROFILE_FLAGS.BANNER_UNSET`).
pub const PROFILE_FLAG_BANNER_UNSET: u32 = 1 << 1;

/// `null` or missing -> `T::default()`.
fn null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// `UserProfileFullResponse`: the user, their profile and the guild-scoped
/// member profile when `guild_id` was requested.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UserProfileFull {
    #[serde(default)]
    pub user: Option<ProfileUser>,
    #[serde(default)]
    pub user_profile: Option<UserProfileData>,
    #[serde(default)]
    pub guild_member_profile: Option<UserProfileData>,
    #[serde(default)]
    pub guild_member: Option<GuildMemberSummary>,
    #[serde(default)]
    pub mutual_guilds: Option<Vec<MutualGuild>>,
    /// `with_mutual_friends=true` users; `null`/absent -> `None`.
    #[serde(default)]
    pub mutual_friends: Option<Vec<ProfileUser>>,
    #[serde(default, deserialize_with = "null_default")]
    pub profile_badges: Vec<ProfileBadge>,
    #[serde(default, deserialize_with = "null_default")]
    pub connected_accounts: Vec<UserConnection>,
    #[serde(default, deserialize_with = "null_default")]
    pub profile_limited: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileBadge {
    pub key: String,
    pub kind: String,
    pub label: String,
    pub asset: Option<String>,
    pub asset_url: Option<String>,
    pub small_asset: Option<String>,
    pub aspect_ratio: Option<f32>,
    pub caption: Option<String>,
    pub details: Option<ProfileBadgeDetails>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileBadgeDetails {
    pub title: String,
    pub lines: Vec<String>,
}

/// The profile's user subset. `username` falls back to an empty string when
/// the wire sends `null`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileUser {
    pub id: Snowflake,
    #[serde(default, deserialize_with = "null_default")]
    pub username: String,
    #[serde(default)]
    pub discriminator: Option<String>,
    #[serde(default)]
    pub global_name: Option<String>,
    #[serde(default)]
    pub avatar: Option<String>,
    #[serde(default)]
    pub avatar_color: Option<i64>,
    #[serde(default)]
    pub banner: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub bot: bool,
}

/// Bio, pronouns and the banner/accent colors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserProfileData {
    #[serde(default)]
    pub bio: Option<String>,
    #[serde(default)]
    pub pronouns: Option<String>,
    #[serde(default)]
    pub banner: Option<String>,
    #[serde(default)]
    pub banner_color: Option<i64>,
    #[serde(default)]
    pub accent_color: Option<i64>,
}

/// Guild member part of the profile (`nick`, guild avatar/banner, roles).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GuildMemberSummary {
    #[serde(default)]
    pub nick: Option<String>,
    #[serde(default)]
    pub avatar: Option<String>,
    #[serde(default)]
    pub banner: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub roles: Vec<Snowflake>,
    #[serde(default, deserialize_with = "null_default")]
    pub profile_flags: u32,
}

/// A guild the profile user and the current user share.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MutualGuild {
    pub id: Snowflake,
    #[serde(default)]
    pub nick: Option<String>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn null_tolerant_deserialization() {
        let profile: UserProfileFull = serde_json::from_value(json!({
            "user": {"id": "42", "username": null, "avatar_color": null, "banner": null, "bot": null},
            "user_profile": null,
            "guild_member_profile": {"bio": "hi", "accent_color": 12},
            "guild_member": {"nick": null, "roles": null, "profile_flags": null},
        }))
        .unwrap();

        let user = profile.user.unwrap();
        assert_eq!((user.id, user.username.as_str(), user.bot), (Snowflake(42), "", false));
        assert_eq!(user.avatar_color, None);
        assert_eq!(profile.user_profile, None);
        let member_profile = profile.guild_member_profile.unwrap();
        assert_eq!(member_profile.bio.as_deref(), Some("hi"));
        assert_eq!(member_profile.accent_color, Some(12));
        let member = profile.guild_member.unwrap();
        assert!(member.roles.is_empty());
        assert_eq!(member.profile_flags, 0);
        assert_eq!(profile.mutual_guilds, None);
    }

    #[test]
    fn profile_flags_match_the_ts_bits() {
        assert_eq!(PROFILE_FLAG_AVATAR_UNSET, 1);
        assert_eq!(PROFILE_FLAG_BANNER_UNSET, 2);
    }
}
