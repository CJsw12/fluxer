//! Protocol deep links (`fluxer://`), trusted internal HTTP jump links, and
//! short-window duplicate suppression (`DeepLinks.ts` & `DeepLinkUtils.ts`).

use std::time::{Duration, Instant};

use fluxer_client_core::state::Snowflake;

use crate::backend::extract_invite_code;

pub const APP_PROTOCOL: &str = "fluxer";
pub const MAX_DEEP_LINK_LEN: usize = 2048;
const APP_PROTOCOL_PREFIX: &str = "fluxer:";
const DUPLICATE_SUPPRESS_WINDOW: Duration = Duration::from_millis(1500);

const OFFICIAL_WEB_APP_HOSTS: &[&str] = &["fluxer.app", "web.fluxer.app", "canary.fluxer.app"];
const INSTANCE_DESIGNATING_KEYS: &[&str] = &[
    "api",
    "apibase",
    "apiendpoint",
    "domain",
    "endpoint",
    "gateway",
    "host",
    "instance",
    "instancekey",
    "origin",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeepLink {
    Home,
    DmChannel {
        channel_id: Snowflake,
        message_id: Option<Snowflake>,
    },
    Guild {
        guild_id: Snowflake,
    },
    GuildChannel {
        guild_id: Snowflake,
        channel_id: Snowflake,
        message_id: Option<Snowflake>,
    },
    Invite {
        code: String,
    },
    UserProfile {
        user_id: Snowflake,
    },
    Settings,
}

/// Suppresses identical deep links fired twice within 1.5s (Windows protocol double-dispatch).
#[derive(Default)]
pub struct DeepLinkDeduper {
    last: Option<(String, Instant)>,
}

impl DeepLinkDeduper {
    pub fn should_suppress(&mut self, raw: &str) -> bool {
        self.should_suppress_at(Instant::now(), raw)
    }

    pub fn should_suppress_at(&mut self, now: Instant, raw: &str) -> bool {
        let trimmed = raw.trim();
        if let Some((prev, at)) = &self.last
            && prev == trimmed
            && now.duration_since(*at) < DUPLICATE_SUPPRESS_WINDOW
        {
            return true;
        }
        self.last = Some((trimmed.to_owned(), now));
        false
    }
}

/// True when `raw` looks like a `fluxer:...` URI with a non-empty payload after the scheme.
pub fn is_app_protocol_url(raw: &str) -> bool {
    let trimmed = raw.trim();
    trimmed.len() > APP_PROTOCOL_PREFIX.len()
        && trimmed
            .get(..APP_PROTOCOL_PREFIX.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(APP_PROTOCOL_PREFIX))
}

pub fn has_blocked_chars(s: &str) -> bool {
    s.chars()
        .any(|c| c.is_control() || matches!(c, '"' | '\'' | '<' | '>' | '\\' | '|' | '\t' | '\r' | '\n'))
}

fn is_instance_designation_key(name: &str) -> bool {
    let normalized: String = name
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    INSTANCE_DESIGNATING_KEYS.contains(&normalized.as_str())
}

fn has_raw_instance_designation(raw: &str) -> bool {
    let before_fragment = raw.split('#').next().unwrap_or(raw);
    let Some((_, query)) = before_fragment.split_once('?') else {
        return false;
    };
    query.split(['&', '?']).any(|pair| {
        let key = pair.split('=').next().unwrap_or("");
        is_instance_designation_key(key)
    })
}

fn parse_snowflake(seg: &str) -> Option<Snowflake> {
    if seg.is_empty() || !seg.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    seg.parse::<u64>().ok().filter(|&n| n > 0).map(Snowflake)
}

fn has_unsafe_segment(segments: &[&str]) -> bool {
    segments.iter().any(|s| {
        *s == "."
            || *s == ".."
            || s.eq_ignore_ascii_case("%2e")
            || s.eq_ignore_ascii_case("%2e%2e")
            || s.eq_ignore_ascii_case(".%2e")
            || s.eq_ignore_ascii_case("%2e.")
    })
}

fn parse_route_segments(segments: &[&str], allow_non_channel: bool) -> Option<DeepLink> {
    if has_unsafe_segment(segments) {
        return None;
    }
    match segments {
        ["channels", "@me"] => Some(DeepLink::Home),
        ["channels", "@me", ch] => Some(DeepLink::DmChannel {
            channel_id: parse_snowflake(ch)?,
            message_id: None,
        }),
        ["channels", "@me", ch, msg] => Some(DeepLink::DmChannel {
            channel_id: parse_snowflake(ch)?,
            message_id: Some(parse_snowflake(msg)?),
        }),
        ["channels", guild] => Some(DeepLink::Guild {
            guild_id: parse_snowflake(guild)?,
        }),
        ["channels", guild, ch] => Some(DeepLink::GuildChannel {
            guild_id: parse_snowflake(guild)?,
            channel_id: parse_snowflake(ch)?,
            message_id: None,
        }),
        ["channels", guild, ch, msg] => Some(DeepLink::GuildChannel {
            guild_id: parse_snowflake(guild)?,
            channel_id: parse_snowflake(ch)?,
            message_id: Some(parse_snowflake(msg)?),
        }),
        ["users", uid] => Some(DeepLink::UserProfile {
            user_id: parse_snowflake(uid)?,
        }),
        ["friends"] if allow_non_channel => Some(DeepLink::Home),
        ["invite" | "i", code] | ["invite", code, "login"] if allow_non_channel => {
            extract_invite_code(code).map(|code| DeepLink::Invite { code })
        }
        ["settings", ..] if allow_non_channel => Some(DeepLink::Settings),
        _ => None,
    }
}

/// Parses and validates a `fluxer://` deep link against foreground-account security rules.
pub fn parse_protocol_deep_link(raw: &str) -> Option<DeepLink> {
    if has_blocked_chars(raw) {
        return None;
    }
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.len() > MAX_DEEP_LINK_LEN
        || !is_app_protocol_url(trimmed)
        || has_raw_instance_designation(trimmed)
    {
        return None;
    }
    let parsed = url::Url::parse(trimmed).ok()?;
    if !parsed.scheme().eq_ignore_ascii_case(APP_PROTOCOL)
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return None;
    }
    if let Some(host) = parsed.host_str()
        && (host.eq_ignore_ascii_case("handoff") || is_instance_designation_key(host))
    {
        return None;
    }
    if parsed.query_pairs().any(|(k, _)| is_instance_designation_key(&k)) {
        return None;
    }
    let path = match parsed.host_str() {
        Some(host) if !host.is_empty() && host != "-" => format!("/{host}{}", parsed.path()),
        _ => parsed.path().to_owned(),
    };
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    parse_route_segments(&segments, true)
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1" || host == "[::1]"
}

/// Parses internal HTTP(S) channel/user jump links pointing to built-in or instance hosts.
pub fn parse_internal_http_deep_link(raw: &str, instance_hosts: &[String]) -> Option<DeepLink> {
    if has_blocked_chars(raw) {
        return None;
    }
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.len() > MAX_DEEP_LINK_LEN
        || has_raw_instance_designation(trimmed)
    {
        return None;
    }
    let parsed = url::Url::parse(trimmed).ok()?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return None;
    }
    if parsed.query_pairs().any(|(k, _)| is_instance_designation_key(&k)) {
        return None;
    }
    let host = parsed.host_str()?;
    let is_https = parsed.scheme().eq_ignore_ascii_case("https");
    let is_http_loopback = parsed.scheme().eq_ignore_ascii_case("http") && is_loopback_host(host);
    if !is_https && !is_http_loopback {
        return None;
    }
    let is_official = OFFICIAL_WEB_APP_HOSTS
        .iter()
        .any(|official| host.eq_ignore_ascii_case(official));
    if is_official && parsed.port().is_some() {
        return None;
    }
    let is_instance = instance_hosts.iter().any(|h| host.eq_ignore_ascii_case(h));
    if !is_official && !is_instance {
        return None;
    }
    let segments: Vec<&str> = parsed.path().split('/').filter(|s| !s.is_empty()).collect();
    parse_route_segments(&segments, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_protocol_routes_and_slashes() {
        assert_eq!(parse_protocol_deep_link("fluxer://channels/@me"), Some(DeepLink::Home));
        assert_eq!(parse_protocol_deep_link("fluxer://-/channels/@me"), Some(DeepLink::Home));
        assert_eq!(parse_protocol_deep_link("fluxer:///channels/@me"), Some(DeepLink::Home));
        assert_eq!(parse_protocol_deep_link("fluxer://friends"), Some(DeepLink::Home));

        assert_eq!(
            parse_protocol_deep_link("fluxer://channels/@me/123"),
            Some(DeepLink::DmChannel {
                channel_id: Snowflake(123),
                message_id: None
            })
        );
        assert_eq!(
            parse_protocol_deep_link("fluxer://channels/@me/123/456"),
            Some(DeepLink::DmChannel {
                channel_id: Snowflake(123),
                message_id: Some(Snowflake(456))
            })
        );
        assert_eq!(
            parse_protocol_deep_link("fluxer://channels/10"),
            Some(DeepLink::Guild {
                guild_id: Snowflake(10)
            })
        );
        assert_eq!(
            parse_protocol_deep_link("fluxer://channels/10/20"),
            Some(DeepLink::GuildChannel {
                guild_id: Snowflake(10),
                channel_id: Snowflake(20),
                message_id: None
            })
        );
        assert_eq!(
            parse_protocol_deep_link("fluxer:///channels/10/20/30"),
            Some(DeepLink::GuildChannel {
                guild_id: Snowflake(10),
                channel_id: Snowflake(20),
                message_id: Some(Snowflake(30))
            })
        );
        assert_eq!(
            parse_protocol_deep_link("fluxer://invite/rust-team"),
            Some(DeepLink::Invite {
                code: "rust-team".to_owned()
            })
        );
        assert_eq!(
            parse_protocol_deep_link("fluxer://i/abc123"),
            Some(DeepLink::Invite {
                code: "abc123".to_owned()
            })
        );
        assert_eq!(
            parse_protocol_deep_link("fluxer://invite/abc123/login"),
            Some(DeepLink::Invite {
                code: "abc123".to_owned()
            })
        );
        assert_eq!(
            parse_protocol_deep_link("fluxer://users/999"),
            Some(DeepLink::UserProfile {
                user_id: Snowflake(999)
            })
        );
        assert_eq!(parse_protocol_deep_link("fluxer://settings"), Some(DeepLink::Settings));
        assert_eq!(
            parse_protocol_deep_link("fluxer://settings/appearance"),
            Some(DeepLink::Settings)
        );
    }

    #[test]
    fn rejects_spoofed_instance_designation_traversal_and_bad_chars() {
        assert_eq!(parse_protocol_deep_link("fluxer://channels/1/2?api=https://evil.test"), None);
        assert_eq!(parse_protocol_deep_link("fluxer://channels/1/2?Api_Base=x"), None);
        assert_eq!(parse_protocol_deep_link("fluxer://channels/1/2?gateway=wss://evil"), None);
        assert_eq!(parse_protocol_deep_link("fluxer://origin/channels/1/2"), None);
        assert_eq!(parse_protocol_deep_link("fluxer://handoff/foo"), None);
        assert_eq!(parse_protocol_deep_link("fluxer://user:pass@channels/1/2"), None);
        assert_eq!(parse_protocol_deep_link("fluxer://channels/../users/2"), None);
        assert_eq!(parse_protocol_deep_link("fluxer://channels/1/2\n"), None);
        assert_eq!(parse_protocol_deep_link("fluxer://channels/1/2\"<script>"), None);
        assert_eq!(parse_protocol_deep_link(&format!("fluxer://channels/1/{}", "9".repeat(2100))), None);
        assert_eq!(parse_protocol_deep_link("fluxer://channels/0/11"), None);
        assert_eq!(parse_protocol_deep_link("https://fluxer.app/channels/1/2"), None);
    }

    #[test]
    fn parses_internal_http_channels_and_users_only() {
        let hosts = vec!["chat.selfhost.example".to_owned()];
        assert_eq!(
            parse_internal_http_deep_link("https://fluxer.app/channels/1/11/99", &hosts),
            Some(DeepLink::GuildChannel {
                guild_id: Snowflake(1),
                channel_id: Snowflake(11),
                message_id: Some(Snowflake(99)),
            })
        );
        assert_eq!(
            parse_internal_http_deep_link("https://chat.selfhost.example/users/201", &hosts),
            Some(DeepLink::UserProfile {
                user_id: Snowflake(201),
            })
        );
        // External /docs, HTTPS /invite links, plaintext http non-loopback, and non-default official ports are rejected
        assert_eq!(parse_internal_http_deep_link("https://fluxer.app/docs", &hosts), None);
        assert_eq!(parse_internal_http_deep_link("https://chat.selfhost.example/invite/abc", &hosts), None);
        assert_eq!(parse_internal_http_deep_link("https://evil.example/channels/1/11", &hosts), None);
        assert_eq!(parse_internal_http_deep_link("http://fluxer.app/channels/1/11", &hosts), None);
        assert_eq!(parse_internal_http_deep_link("https://fluxer.app:8443/channels/1/11", &hosts), None);
        assert_eq!(
            parse_internal_http_deep_link("http://localhost:3000/channels/1/11", &["localhost".to_owned()]),
            Some(DeepLink::GuildChannel {
                guild_id: Snowflake(1),
                channel_id: Snowflake(11),
                message_id: None,
            })
        );
        assert_eq!(
            parse_internal_http_deep_link("https://fluxer.app/channels/1/11?endpoint=https://evil", &hosts),
            None
        );
    }

    #[test]
    fn deduper_suppresses_within_window() {
        let mut d = DeepLinkDeduper::default();
        let t0 = Instant::now();
        assert!(!d.should_suppress_at(t0, "fluxer://channels/@me"));
        assert!(d.should_suppress_at(t0 + Duration::from_millis(200), " fluxer://channels/@me "));
        assert!(!d.should_suppress_at(t0 + Duration::from_millis(200), "fluxer://settings"));
        assert!(!d.should_suppress_at(t0 + Duration::from_millis(1800), "fluxer://settings"));
    }
}
