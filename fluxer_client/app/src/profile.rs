//! Pure profile-card data for the avatar/name popout (`UserProfileModal`,
//! `ProfileDisplayUtils.ts`). No GPUI types: the UI renders this snapshot.
//!
//! Scope rules (official `ProfileDisplayUtils.ts`): in a guild the guild-member profile
//! overrides the global one field by field, but `AVATAR_UNSET`/`BANNER_UNSET`
//! never fall back upward. URLs are the still media-proxy forms used by
//! `getProfileAvatarUrls`/`getProfileBannerUrls` with `animated = false`:
//! an `a_` hash is stripped and the request is `.webp` (the hover variant
//! keeps `a_` and adds `?animated=true`).

use fluxer_client_core::rest::{
    GuildMemberSummary, PROFILE_FLAG_AVATAR_UNSET, PROFILE_FLAG_BANNER_UNSET, ProfileUser, UserProfileFull,
};
use fluxer_client_core::state::{ClientState, RelationshipKind, Snowflake, User, parse_timestamp_ms};

use crate::format::{DEFAULT_AVATAR_COLORS, default_avatar_index, initials};

/// `AppConstants.DEFAULT_ACCENT_COLOR` (`#4641D9`).
pub const DEFAULT_ACCENT_HEX: u32 = 0x4641d9;

/// A guild role shown as a badge; `color` is `None` for the default role text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileRole {
    pub id: Snowflake,
    pub name: String,
    pub color: Option<u32>,
}

/// A guild the profile user and the current user share.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutualGuildItem {
    pub id: Snowflake,
    pub name: String,
    pub initials: String,
    pub nick: Option<String>,
}

/// A mutual friend row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutualFriendItem {
    pub id: Snowflake,
    pub name: String,
    pub initials: String,
}

/// The voice channel the profile user is currently in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceActivityItem {
    pub channel_name: String,
    pub guild_name: Option<String>,
}

/// Everything the profile surface draws, built under the state lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileCard {
    pub user_id: Snowflake,
    pub guild_id: Option<Snowflake>,
    pub display_name: String,
    pub username_tag: String,
    pub initials: String,
    pub avatar_url: Option<String>,
    pub default_avatar: usize,
    pub banner_url: Option<String>,
    pub banner_color: u32,
    pub bio: Option<String>,
    pub pronouns: Option<String>,
    pub roles: Vec<ProfileRole>,
    pub mutual_guilds: Vec<MutualGuildItem>,
    /// Empty when the fetch has no `mutual_friends` (section hidden).
    pub mutual_friends: Vec<MutualFriendItem>,
    /// Account creation date from the user-id snowflake, e.g. `Nov 28, 2026`.
    pub created_date: Option<String>,
    /// Guild join date from loaded member state; `None` outside a guild.
    pub joined_date: Option<String>,
    /// `None` unless the user is in a voice channel (see `voice_activity_item`).
    pub voice_activity: Option<VoiceActivityItem>,
    pub note: String,
    pub is_self: bool,
    pub is_blocked: bool,
    pub relationship: Option<RelationshipKind>,
    pub bot: bool,
    pub loaded: bool,
}

/// The fields shared by `ProfileUser` and `state::User`.
struct BaseUser<'a> {
    username: &'a str,
    discriminator: Option<&'a str>,
    global_name: Option<&'a str>,
    avatar: Option<&'a str>,
    banner: Option<&'a str>,
    avatar_color: Option<i64>,
    bot: bool,
}

impl<'a> BaseUser<'a> {
    fn from_user(user: &'a User) -> Self {
        Self {
            username: &user.username,
            discriminator: user.discriminator.as_deref(),
            global_name: user.global_name.as_deref(),
            avatar: user.avatar.as_deref(),
            banner: user.banner.as_deref(),
            avatar_color: user.avatar_color,
            bot: user.bot,
        }
    }

    fn from_profile(user: &'a ProfileUser) -> Self {
        Self {
            username: &user.username,
            discriminator: user.discriminator.as_deref(),
            global_name: user.global_name.as_deref(),
            avatar: user.avatar.as_deref(),
            banner: user.banner.as_deref(),
            avatar_color: user.avatar_color,
            bot: user.bot,
        }
    }
}

/// Fetched user, then the user cache, then the session user, then the guild
/// member's embedded user.
fn base_user<'a>(
    state: &'a ClientState,
    fetched: Option<&'a UserProfileFull>,
    user_id: Snowflake,
    guild_id: Option<Snowflake>,
) -> Option<BaseUser<'a>> {
    if let Some(user) = fetched.and_then(|f| f.user.as_ref()) {
        return Some(BaseUser::from_profile(user));
    }
    if let Some(user) = state.users.get(&user_id) {
        return Some(BaseUser::from_user(user));
    }
    if let Some(user) = state.current_user.as_ref().filter(|u| u.id == user_id) {
        return Some(BaseUser::from_user(user));
    }
    guild_id
        .and_then(|guild_id| state.members.get(&guild_id)?.get(&user_id))
        .map(|member| BaseUser::from_user(&member.user))
}

/// `int2hex`: negative values are not colors; the rest are masked to RGB.
fn color_hex(raw: i64) -> Option<u32> {
    (raw >= 0).then(|| u32::try_from(raw & 0xff_ffff).ok()).flatten()
}

/// Strips the animated `a_` marker for the still asset.
fn static_hash(hash: &str) -> Option<&str> {
    let hash = hash.strip_prefix("a_").unwrap_or(hash);
    (!hash.is_empty()).then_some(hash)
}

fn media_base(media: &str) -> &str {
    media.trim_end_matches('/')
}

fn user_avatar_url(media: &str, user_id: Snowflake, hash: &str) -> Option<String> {
    let hash = static_hash(hash)?;
    Some(format!("{}/avatars/{user_id}/{hash}.webp?size=240", media_base(media)))
}

fn guild_avatar_url(media: &str, guild_id: Snowflake, user_id: Snowflake, hash: &str) -> Option<String> {
    let hash = static_hash(hash)?;
    Some(format!(
        "{}/guilds/{guild_id}/users/{user_id}/avatars/{hash}.webp?size=240",
        media_base(media)
    ))
}

fn user_banner_url(media: &str, user_id: Snowflake, hash: &str) -> Option<String> {
    let hash = static_hash(hash)?;
    Some(format!("{}/banners/{user_id}/{hash}.webp?size=1024", media_base(media)))
}

fn guild_banner_url(media: &str, guild_id: Snowflake, user_id: Snowflake, hash: &str) -> Option<String> {
    let hash = static_hash(hash)?;
    Some(format!(
        "{}/guilds/{guild_id}/users/{user_id}/banners/{hash}.webp?size=1024",
        media_base(media)
    ))
}

/// `isAvatarUnset`/`isBannerUnset` from the fetched guild member. The state
/// `Member` does not carry `profile_flags`.
fn flags(fetched_member: Option<&GuildMemberSummary>) -> u32 {
    fetched_member.map(|member| member.profile_flags).unwrap_or(0)
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.is_empty())
}

/// Formats `username#discriminator` while suppressing `0` and `0000` placeholders.
pub fn format_username_tag(username: &str, discriminator: Option<&str>) -> String {
    match discriminator {
        Some(discriminator) if !discriminator.is_empty() && discriminator != "0" && discriminator != "0000" => {
            format!("{username}#{discriminator}")
        }
        _ => username.to_owned(),
    }
}

/// Builds a profile snapshot from the live state and the optional REST result.
/// Without `fetched` this still yields the base user/avatar/name while the
/// fetch is in flight.
pub fn build_profile_card(
    state: &ClientState,
    media: Option<&str>,
    user_id: Snowflake,
    guild_id: Option<Snowflake>,
    fetched: Option<&UserProfileFull>,
) -> ProfileCard {
    let fetched_member = fetched.and_then(|f| f.guild_member.as_ref());
    let state_member = guild_id.and_then(|guild_id| state.members.get(&guild_id)?.get(&user_id));
    let base = base_user(state, fetched, user_id, guild_id);

    let profile_flags = flags(fetched_member);
    let avatar_unset = profile_flags & PROFILE_FLAG_AVATAR_UNSET != 0;
    let banner_unset = profile_flags & PROFILE_FLAG_BANNER_UNSET != 0;

    let guild_profile = fetched.and_then(|f| f.guild_member_profile.as_ref());
    let user_profile = fetched.and_then(|f| f.user_profile.as_ref());

    let nick = fetched_member
        .and_then(|member| non_empty(member.nick.as_deref()))
        .or_else(|| state_member.and_then(|member| non_empty(member.nick.as_deref())));
    let display_name = nick
        .or_else(|| base.as_ref().and_then(|user| non_empty(user.global_name)))
        .or_else(|| base.as_ref().and_then(|user| non_empty(Some(user.username))))
        .unwrap_or("Unknown")
        .to_owned();

    let username = base.as_ref().map(|user| user.username).unwrap_or_default();
    let username_tag = format_username_tag(username, base.as_ref().and_then(|user| user.discriminator));

    let member_avatar = fetched_member.and_then(|member| non_empty(member.avatar.as_deref()));
    let avatar_url = if avatar_unset {
        None
    } else {
        media.and_then(|media| match (guild_id, member_avatar) {
            (Some(guild_id), Some(hash)) => guild_avatar_url(media, guild_id, user_id, hash),
            _ => base
                .as_ref()
                .and_then(|user| non_empty(user.avatar))
                .and_then(|hash| user_avatar_url(media, user_id, hash)),
        })
    };

    let guild_banner = guild_profile
        .and_then(|profile| non_empty(profile.banner.as_deref()));
    let user_banner = user_profile
        .and_then(|profile| non_empty(profile.banner.as_deref()))
        .or_else(|| base.as_ref().and_then(|user| non_empty(user.banner)));
    let banner_url = if banner_unset {
        None
    } else {
        media.and_then(|media| match (guild_id, guild_banner) {
            (Some(guild_id), Some(hash)) => guild_banner_url(media, guild_id, user_id, hash),
            _ => user_banner.and_then(|hash| user_banner_url(media, user_id, hash)),
        })
    };

    let effective_banner_color = if banner_unset {
        None
    } else {
        guild_profile
            .and_then(|profile| profile.banner_color)
            .or_else(|| user_profile.and_then(|profile| profile.banner_color))
    };
    let effective_accent_color = guild_profile
        .and_then(|profile| profile.accent_color)
        .or_else(|| user_profile.and_then(|profile| profile.accent_color));
    let banner_color = effective_accent_color
        .and_then(color_hex)
        .or_else(|| effective_banner_color.and_then(color_hex))
        .or_else(|| base.as_ref().and_then(|user| user.avatar_color).and_then(color_hex))
        .or_else(|| DEFAULT_AVATAR_COLORS.get(default_avatar_index(user_id)).copied())
        .unwrap_or(DEFAULT_ACCENT_HEX);

    let bio = guild_profile
        .and_then(|profile| profile.bio.as_deref())
        .or_else(|| user_profile.and_then(|profile| profile.bio.as_deref()))
        .map(str::to_owned);
    let pronouns = guild_profile
        .and_then(|profile| profile.pronouns.as_deref())
        .or_else(|| user_profile.and_then(|profile| profile.pronouns.as_deref()))
        .map(str::to_owned);

    let roles = match guild_id.and_then(|guild_id| state.guilds.get(&guild_id).map(|guild| (guild_id, guild))) {
        Some((guild_id, guild)) => {
            let role_ids: &[Snowflake] = match fetched_member {
                Some(member) if !member.roles.is_empty() => &member.roles,
                _ => state_member.map(|member| member.roles.as_slice()).unwrap_or(&[]),
            };
            let mut resolved: Vec<_> = role_ids
                .iter()
                .filter(|role_id| **role_id != guild_id)
                .filter_map(|role_id| guild.roles.get(role_id))
                .collect();
            resolved.sort_by(|a, b| b.position.cmp(&a.position).then_with(|| a.id.cmp(&b.id)));
            resolved
                .into_iter()
                .map(|role| ProfileRole {
                    id: role.id,
                    name: role.name.clone(),
                    color: u32::try_from(role.color & 0xff_ffff).ok().filter(|color| *color != 0),
                })
                .collect()
        }
        None => Vec::new(),
    };

    let mutual_guilds = fetched
        .and_then(|f| f.mutual_guilds.as_deref())
        .map(|guilds| {
            guilds
                .iter()
                .filter_map(|mutual| {
                    let guild = state.guilds.get(&mutual.id)?;
                    Some(MutualGuildItem {
                        id: mutual.id,
                        name: guild.name.clone(),
                        initials: initials(&guild.name),
                        nick: mutual.nick.clone(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let mutual_friends = fetched
        .and_then(|f| f.mutual_friends.as_deref())
        .map(|friends| {
            friends
                .iter()
                .map(|friend| {
                    let name = non_empty(friend.global_name.as_deref())
                        .or_else(|| non_empty(Some(friend.username.as_str())))
                        .unwrap_or("Unknown")
                        .to_owned();
                    MutualFriendItem { id: friend.id, initials: initials(&name), name }
                })
                .collect()
        })
        .unwrap_or_default();
    let created_date = Some(format_date_utc(user_id.timestamp_ms() as i64));
    let joined_date = state_member
        .and_then(|member| member.joined_at.as_deref())
        .and_then(parse_timestamp_ms)
        .map(format_date_utc);
    let voice_activity = state.user_voice_channel(user_id, guild_id).and_then(|(voice_guild, channel)| {
        voice_activity_item(
            state.channels.get(&channel).and_then(|c| c.name.as_deref()),
            state.guilds.get(&voice_guild).map(|g| g.name.as_str()),
        )
    });

    let relationship = state.relationship_kind(user_id);
    ProfileCard {
        user_id,
        guild_id,
        display_name: display_name.clone(),
        username_tag,
        initials: initials(&display_name),
        avatar_url,
        default_avatar: default_avatar_index(user_id),
        banner_url,
        banner_color,
        bio,
        pronouns,
        roles,
        mutual_guilds,
        mutual_friends,
        created_date,
        joined_date,
        voice_activity,
        note: state.note(user_id).unwrap_or_default().to_owned(),
        is_self: state.current_user_id() == Some(user_id),
        is_blocked: relationship == Some(RelationshipKind::Blocked),
        relationship,
        bot: base.as_ref().is_some_and(|user| user.bot),
        loaded: fetched.is_some(),
    }
}

/// `Mon D, YYYY` (UTC) for Unix milliseconds.
pub fn format_date_utc(ms: i64) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let days = ms.div_euclid(86_400_000);
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let name = MONTHS.get((month - 1) as usize).copied().unwrap_or("Jan");
    format!("{name} {day}, {year}")
}

/// Voice card content; hidden (`None`) when the user is not in a channel.
pub fn voice_activity_item(channel_name: Option<&str>, guild_name: Option<&str>) -> Option<VoiceActivityItem> {
    let channel_name = non_empty(channel_name)?.to_owned();
    Some(VoiceActivityItem { channel_name, guild_name: non_empty(guild_name).map(str::to_owned) })
}


#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use fluxer_client_core::rest::{MutualGuild, ProfileUser, UserProfileData};
    use fluxer_client_core::state::{
        Guild, Member, Relationship, Role,
    };

    use super::*;

    fn make_user(id: u64, username: &str) -> User {
        User {
            id: Snowflake(id),
            username: username.to_owned(),
            discriminator: None,
            global_name: None,
            avatar: None,
            banner: None,
            avatar_color: None,
            bot: false,
        }
    }

    fn make_guild(id: u64, name: &str, roles: Vec<Role>) -> Guild {
        Guild {
            id: Snowflake(id),
            name: name.to_owned(),
            icon: None,
            owner_id: None,
            mfa_level: None,
            joined_at: None,
            member_count: None,
            visiting: false,
            roles: roles.into_iter().map(|role| (role.id, role)).collect(),
            default_message_notifications: 0,
            large_feature: false,
            member_dms_disabled: false,
            invites_disabled: false,
        }
    }

    fn make_role(id: u64, name: &str, color: i64, position: i32) -> Role {
        Role {
            id: Snowflake(id),
            name: name.to_owned(),
            color,
            position,
            permissions: 0,
            hoist: false,
            mentionable: false,
        }
    }

    fn make_member(user: User, nick: Option<&str>, roles: Vec<Snowflake>) -> Member {
        Member {
            user,
            nick: nick.map(str::to_owned),
            roles,
            joined_at: None,
            communication_disabled_until: None,
            bio: None,
            pronouns: None,
        }
    }

    fn profile_data(bio: Option<&str>, pronouns: Option<&str>, banner: Option<&str>, banner_color: Option<i64>, accent_color: Option<i64>) -> UserProfileData {
        UserProfileData {
            bio: bio.map(str::to_owned),
            pronouns: pronouns.map(str::to_owned),
            banner: banner.map(str::to_owned),
            banner_color,
            accent_color,
        }
    }

    fn fetched_profile() -> UserProfileFull {
        UserProfileFull {
            user: Some(ProfileUser {
                id: Snowflake(10),
                username: "alice".to_owned(),
                discriminator: Some("1234".to_owned()),
                global_name: Some("Alice Global".to_owned()),
                avatar: Some("useravatar".to_owned()),
                avatar_color: Some(0x111111),
                banner: Some("userbanner".to_owned()),
                bot: true,
            }),
            user_profile: Some(profile_data(
                Some("user bio"),
                Some("they"),
                Some("userprofilebanner"),
                Some(0x123456),
                Some(0x654321),
            )),
            guild_member_profile: Some(profile_data(
                Some("guild bio"),
                Some("she"),
                Some("a_guildbanner"),
                Some(0xabcdef),
                Some(0x112233),
            )),
            guild_member: Some(GuildMemberSummary {
                nick: Some("Nick".to_owned()),
                avatar: Some("a_memberavatar".to_owned()),
                banner: None,
                roles: vec![Snowflake(100), Snowflake(3), Snowflake(2), Snowflake(4), Snowflake(999)],
                profile_flags: 0,
            }),
            mutual_friends: None,
            mutual_guilds: Some(vec![
                MutualGuild { id: Snowflake(100), nick: Some("MyNick".to_owned()) },
                MutualGuild { id: Snowflake(999), nick: None },
            ]),
        }
    }

    #[test]
    fn guild_member_profile_overrides_the_global_one() {
        let mut state = ClientState::new();
        let mut alice = make_user(10, "alice");
        alice.discriminator = Some("1234".to_owned());
        alice.global_name = Some("Alice Global".to_owned());
        alice.avatar = Some("useravatar".to_owned());
        alice.banner = Some("userbanner".to_owned());
        alice.avatar_color = Some(0x111111);
        alice.bot = true;
        state.users.insert(alice.id, alice.clone());
        state.current_user = Some(alice);
        state.notes.insert(Snowflake(10), "a note".to_owned());
        state.relationships.insert(
            Snowflake(10),
            Relationship {
                id: Snowflake(10),
                kind_raw: 2,
                since: None,
                nickname: None,
                share_voice_activity: None,
                friend_shares_voice_activity: None,
            },
        );
        state.guilds.insert(
            Snowflake(100),
            make_guild(
                100,
                "Guild",
                vec![
                    make_role(100, "@everyone", 0, 100),
                    make_role(2, "Mod", 0xff0000, 5),
                    make_role(3, "Admin", 0x00ff00, 10),
                    make_role(4, "Tie", 0, 5),
                ],
            ),
        );
        state.members.insert(
            Snowflake(100),
            BTreeMap::from([(
                Snowflake(10),
                make_member(make_user(10, "alice"), Some("State Nick"), vec![Snowflake(2)]),
            )]),
        );

        let fetched = fetched_profile();
        let card = build_profile_card(&state, Some("https://media.example/"), Snowflake(10), Some(Snowflake(100)), Some(&fetched));

        assert_eq!(card.display_name, "Nick");
        assert_eq!(card.username_tag, "alice#1234");
        assert_eq!(card.initials, "N");
        assert_eq!(
            card.avatar_url.as_deref(),
            Some("https://media.example/guilds/100/users/10/avatars/memberavatar.webp?size=240")
        );
        assert_eq!(
            card.banner_url.as_deref(),
            Some("https://media.example/guilds/100/users/10/banners/guildbanner.webp?size=1024")
        );
        assert_eq!(card.banner_color, 0x112233);
        assert_eq!(card.bio.as_deref(), Some("guild bio"));
        assert_eq!(card.pronouns.as_deref(), Some("she"));

        let roles: Vec<_> = card.roles.iter().map(|role| (role.id, role.color)).collect();
        assert_eq!(
            roles,
            vec![
                (Snowflake(3), Some(0x00ff00)),
                (Snowflake(2), Some(0xff0000)),
                (Snowflake(4), None),
            ]
        );

        assert_eq!(card.mutual_guilds.len(), 1);
        assert_eq!(card.mutual_guilds[0].id, Snowflake(100));
        assert_eq!(card.mutual_guilds[0].name, "Guild");
        assert_eq!(card.mutual_guilds[0].initials, "G");
        assert_eq!(card.mutual_guilds[0].nick.as_deref(), Some("MyNick"));

        assert_eq!(card.note, "a note");
        assert!(card.is_self);
        assert!(card.is_blocked);
        assert!(card.bot);
        assert!(card.loaded);
        assert_eq!(card.default_avatar, 10 % DEFAULT_AVATAR_COLORS.len());
    }

    #[test]
    fn unset_flags_never_fall_back_upward() {
        let mut state = ClientState::new();
        let mut alice = make_user(10, "alice");
        alice.avatar = Some("useravatar".to_owned());
        alice.banner = Some("userbanner".to_owned());
        alice.avatar_color = Some(0x010203);
        state.users.insert(alice.id, alice.clone());
        state.guilds.insert(Snowflake(100), make_guild(100, "Guild", Vec::new()));

        let mut fetched = fetched_profile();
        fetched.user_profile = Some(profile_data(None, None, Some("userprofilebanner"), Some(0x111111), Some(0x222222)));
        fetched.guild_member_profile = Some(profile_data(None, None, Some("a_guildbanner"), Some(0x999999), Some(0x222222)));
        fetched.guild_member = Some(GuildMemberSummary {
            nick: None,
            avatar: Some("a_memberavatar".to_owned()),
            banner: None,
            roles: Vec::new(),
            profile_flags: PROFILE_FLAG_AVATAR_UNSET | PROFILE_FLAG_BANNER_UNSET,
        });

        let card = build_profile_card(&state, Some("https://media.example"), Snowflake(10), Some(Snowflake(100)), Some(&fetched));
        assert_eq!(card.avatar_url, None);
        assert_eq!(card.banner_url, None);
        // The banner color skips both banner_color fields but still honours
        // the guild accent color.
        assert_eq!(card.banner_color, 0x222222);
    }

    fn banner_color_for(
        user_id: u64,
        avatar_color: Option<i64>,
        banner_color: Option<i64>,
        accent_color: Option<i64>,
    ) -> u32 {
        let mut state = ClientState::new();
        let mut user = make_user(user_id, "u");
        user.avatar_color = avatar_color;
        state.users.insert(user.id, user);
        let fetched = UserProfileFull {
            user: None,
            user_profile: Some(profile_data(None, None, None, banner_color, accent_color)),
            guild_member_profile: None,
            guild_member: None,
            mutual_friends: None,
            mutual_guilds: None,
        };
        build_profile_card(&state, None, Snowflake(user_id), None, Some(&fetched)).banner_color
    }

    #[test]
    fn banner_color_follows_the_fallback_order() {
        assert_eq!(banner_color_for(7, Some(0x010203), Some(0x111111), Some(0x222222)), 0x222222);
        assert_eq!(banner_color_for(7, Some(0x010203), Some(0x111111), None), 0x111111);
        assert_eq!(banner_color_for(7, Some(0x010203), None, None), 0x010203);
        assert_eq!(
            banner_color_for(7, None, None, None),
            DEFAULT_AVATAR_COLORS[default_avatar_index(Snowflake(7))]
        );
        // Colors are masked to 24 bits.
        assert_eq!(banner_color_for(7, Some(0x1_12_03_04), None, None), 0x12_03_04);
        assert_eq!(DEFAULT_ACCENT_HEX, 0x4641d9);
    }

    #[test]
    fn animated_hashes_use_the_still_webp_form() {
        let mut state = ClientState::new();
        let mut alice = make_user(10, "alice");
        alice.avatar = Some("a_anim".to_owned());
        alice.banner = Some("a_banner".to_owned());
        state.users.insert(alice.id, alice);
        state.guilds.insert(Snowflake(100), make_guild(100, "Guild", Vec::new()));

        let fetched = UserProfileFull {
            user: None,
            user_profile: None,
            guild_member_profile: Some(profile_data(None, None, Some("a_guildbanner"), None, None)),
            guild_member: Some(GuildMemberSummary {
                nick: None,
                avatar: Some("a_memberavatar".to_owned()),
                banner: None,
                roles: Vec::new(),
                profile_flags: 0,
            }),
            mutual_friends: None,
            mutual_guilds: None,
        };

        let guild_card = build_profile_card(&state, Some("https://media.example"), Snowflake(10), Some(Snowflake(100)), Some(&fetched));
        assert_eq!(
            guild_card.avatar_url.as_deref(),
            Some("https://media.example/guilds/100/users/10/avatars/memberavatar.webp?size=240")
        );
        assert_eq!(
            guild_card.banner_url.as_deref(),
            Some("https://media.example/guilds/100/users/10/banners/guildbanner.webp?size=1024")
        );

        let user_card = build_profile_card(&state, Some("https://media.example"), Snowflake(10), None, None);
        assert_eq!(
            user_card.avatar_url.as_deref(),
            Some("https://media.example/avatars/10/anim.webp?size=240")
        );
        assert_eq!(
            user_card.banner_url.as_deref(),
            Some("https://media.example/banners/10/banner.webp?size=1024")
        );
        assert!(!user_card.loaded);
        // No media endpoint means no absolute URL.
        let card = build_profile_card(&state, None, Snowflake(10), None, None);
        assert_eq!(card.avatar_url, None);
        assert_eq!(card.banner_url, None);
    }

    #[test]
    fn roles_need_a_loaded_guild_and_skip_everyone() {
        let mut state = ClientState::new();
        state.users.insert(Snowflake(10), make_user(10, "alice"));
        state.guilds.insert(
            Snowflake(100),
            make_guild(
                100,
                "Guild",
                vec![make_role(100, "@everyone", 0xffffff, 100), make_role(5, "Role", 0x123456, 1)],
            ),
        );

        let fetched = UserProfileFull {
            user: None,
            user_profile: None,
            guild_member_profile: None,
            guild_member: Some(GuildMemberSummary {
                nick: None,
                avatar: None,
                banner: None,
                roles: vec![Snowflake(100), Snowflake(5)],
                profile_flags: 0,
            }),
            mutual_friends: None,
            mutual_guilds: None,
        };

        let card = build_profile_card(&state, None, Snowflake(10), Some(Snowflake(100)), Some(&fetched));
        assert_eq!(card.roles.len(), 1);
        assert_eq!((card.roles[0].id, card.roles[0].name.as_str(), card.roles[0].color), (Snowflake(5), "Role", Some(0x123456)));

        // A guild outside the state hides the section even when roles came
        // with the response.
        let card = build_profile_card(&state, None, Snowflake(10), Some(Snowflake(200)), Some(&fetched));
        assert!(card.roles.is_empty());
    }

    fn card_for(username: &str, global_name: Option<&str>, discriminator: Option<&str>, nick: Option<&str>) -> ProfileCard {
        let mut state = ClientState::new();
        let mut user = make_user(10, username);
        user.global_name = global_name.map(str::to_owned);
        user.discriminator = discriminator.map(str::to_owned);
        state.users.insert(user.id, user);
        state.guilds.insert(Snowflake(100), make_guild(100, "Guild", Vec::new()));
        state.members.insert(
            Snowflake(100),
            BTreeMap::from([(Snowflake(10), make_member(make_user(10, username), nick, Vec::new()))]),
        );
        build_profile_card(&state, None, Snowflake(10), Some(Snowflake(100)), None)
    }

    #[test]
    fn names_and_tags_follow_the_ts_rules() {
        assert_eq!(card_for("alice", Some("Global"), Some("1234"), Some("Nick")).display_name, "Nick");
        assert_eq!(card_for("alice", Some("Global"), Some("1234"), None).display_name, "Global");
        assert_eq!(card_for("alice", Some(""), None, Some("")).display_name, "alice");
        assert_eq!(card_for("", None, None, None).display_name, "Unknown");
        assert_eq!(card_for("", None, None, None).initials, "U");

        assert_eq!(card_for("alice", None, Some("1234"), None).username_tag, "alice#1234");
        assert_eq!(card_for("alice", None, Some("0000"), None).username_tag, "alice");
        assert_eq!(card_for("alice", None, Some("0"), None).username_tag, "alice");
        assert_eq!(card_for("alice", None, Some(""), None).username_tag, "alice");
        assert_eq!(card_for("alice", None, None, None).username_tag, "alice");
    }

    #[test]
    fn format_date_utc_from_snowflake_and_epoch() {
        assert_eq!(format_date_utc(0), "Jan 1, 1970");
        assert_eq!(format_date_utc(1_420_070_400_000), "Jan 1, 2015");
        assert_eq!(format_date_utc(Snowflake(0).timestamp_ms() as i64), "Jan 1, 2015");
        assert_eq!(format_date_utc(1_709_164_800_000), "Feb 29, 2024");
    }

    #[test]
    fn voice_activity_hidden_without_channel() {
        assert_eq!(voice_activity_item(None, Some("G")), None);
        assert_eq!(voice_activity_item(Some(""), Some("G")), None);
        let item = voice_activity_item(Some("Lounge"), Some("G"));
        assert_eq!(item.map(|i| (i.channel_name, i.guild_name)), Some(("Lounge".to_owned(), Some("G".to_owned()))));
    }

    #[test]
    fn mutual_friends_parse_array_empty_null_missing() {
        let parse = |v: &str| serde_json::from_str::<UserProfileFull>(v).map(|p| p.mutual_friends);
        assert_eq!(parse(r#"{"mutual_friends":[{"id":"5","username":"bob"}]}"#).ok().flatten().map(|f| f.len()), Some(1));
        assert_eq!(parse(r#"{"mutual_friends":[]}"#).ok(), Some(Some(Vec::new())));
        assert_eq!(parse(r#"{"mutual_friends":null}"#).ok(), Some(None));
        assert_eq!(parse(r#"{}"#).ok(), Some(None));
    }

    #[test]
    fn card_mutual_friends_and_membership_sections() {
        let state = ClientState::new();
        let mut fetched = fetched_profile();
        fetched.mutual_friends = Some(vec![ProfileUser {
            id: Snowflake(5),
            username: "bob".to_owned(),
            discriminator: None,
            global_name: Some("Bobby".to_owned()),
            avatar: None,
            avatar_color: None,
            banner: None,
            bot: false,
        }]);
        let card = build_profile_card(&state, None, Snowflake(10), None, Some(&fetched));
        assert_eq!(card.mutual_friends.len(), 1);
        assert_eq!(card.mutual_friends[0].name, "Bobby");
        assert_eq!(card.created_date.as_deref(), Some("Jan 1, 2015"));
        assert_eq!(card.joined_date, None);
        assert!(card.voice_activity.is_none());
        let none = build_profile_card(&state, None, Snowflake(10), None, None);
        assert!(none.mutual_friends.is_empty());
    }
}
