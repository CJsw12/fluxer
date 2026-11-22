//! Pure message formatting helpers: message grouping, local timestamps,
//! image preview sizes and system message text. Message bodies are parsed
//! by `fluxer_client_markdown`.

use std::ops::Range;

use chrono::{DateTime, Datelike, TimeZone, Weekday};
use fluxer_client_core::state::{MESSAGE_TYPE_REPLY, Message, Snowflake};

use crate::ui::l10n;

/// `MessageFlags.SUPPRESS_NOTIFICATIONS`.
const FLAG_SUPPRESS_NOTIFICATIONS: u64 = 1 << 12;
/// `MessageGroupingUtils.ts` `MESSAGE_GROUP_TIMEOUT`.
const GROUP_TIMEOUT_MS: i64 = 7 * 60 * 1000;

/// TS `isNewMessageGroup` (webhook and crosspost rules omitted: the state
/// model does not carry them). `prev_at`/`cur_at` are the local times.
pub fn starts_group<Tz: TimeZone>(
    prev: Option<(&Message, Option<&DateTime<Tz>>)>,
    cur: &Message,
    cur_at: Option<&DateTime<Tz>>,
) -> bool {
    let Some((prev, prev_at)) = prev else { return true };
    if cur.kind == MESSAGE_TYPE_REPLY {
        return true;
    }
    let is_system = |m: &Message| m.kind != 0 && m.kind != MESSAGE_TYPE_REPLY;
    if is_system(cur) != is_system(prev) {
        return true;
    }
    if prev.kind <= MESSAGE_TYPE_REPLY && prev.author.id != cur.author.id {
        return true;
    }
    let (Some(prev_at), Some(cur_at)) = (prev_at, cur_at) else { return true };
    if prev_at.date_naive() != cur_at.date_naive() {
        return true;
    }
    if cur_at.timestamp_millis() - prev_at.timestamp_millis() > GROUP_TIMEOUT_MS {
        return true;
    }
    let prev_suppressed = prev.flags & FLAG_SUPPRESS_NOTIFICATIONS != 0;
    let cur_suppressed = cur.flags & FLAG_SUPPRESS_NOTIFICATIONS != 0;
    if !prev_suppressed && cur_suppressed {
        return true;
    }
    if prev_suppressed && !cur_suppressed {
        return !cur.mentions.is_empty() || !cur.mention_roles.is_empty() || cur.mention_everyone;
    }
    false
}

/// Message time in `tz`: the server timestamp, else the id's (pending rows
/// have no timestamp but a time-based nonce id).
pub fn message_time<Tz: TimeZone>(m: &Message, tz: &Tz) -> Option<DateTime<Tz>> {
    if let Ok(parsed) = DateTime::parse_from_rfc3339(&m.timestamp) {
        return Some(parsed.with_timezone(tz));
    }
    let ms = i64::try_from(m.id.timestamp_ms()).ok()?;
    tz.timestamp_millis_opt(ms).single()
}

/// `getRelativeDateString`: "Today at 10:20", "Yesterday at 10:20", else
/// the date and time (24-hour; locale presets are not ported).
pub fn format_message_time<Tz: TimeZone>(at: &DateTime<Tz>, now: &DateTime<Tz>) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let day = at.date_naive();
    let today = now.with_timezone(&at.timezone()).date_naive();
    let time = at.format("%H:%M").to_string();
    if day == today {
        l10n::tr_args("Today at {timeString}", &[("timeString", &time)])
    } else if today.pred_opt() == Some(day) {
        l10n::tr_args("Yesterday at {timeString}", &[("timeString", &time)])
    } else {
        at.format("%Y-%m-%d %H:%M").to_string()
    }
}

pub const IMAGE_MAX_WIDTH: f32 = 400.;
pub const IMAGE_MAX_HEIGHT: f32 = 300.;

/// Fits `width`x`height` inside 400x300 keeping the aspect ratio (never
/// upscaled). Unknown dimensions get the full box.
pub fn fit_image(width: Option<u32>, height: Option<u32>) -> (f32, f32) {
    let (Some(w), Some(h)) = (width.filter(|w| *w > 0), height.filter(|h| *h > 0)) else {
        return (IMAGE_MAX_WIDTH, IMAGE_MAX_HEIGHT);
    };
    let (w, h) = (w as f32, h as f32);
    let scale = (IMAGE_MAX_WIDTH / w).min(IMAGE_MAX_HEIGHT / h).min(1.);
    ((w * scale).round().max(1.), (h * scale).round().max(1.))
}

/// `getFormattedFullDate` (en-US): "Wednesday, November 18, 2026".
pub fn format_full_date<Tz: TimeZone>(at: &DateTime<Tz>) -> String
where
    Tz::Offset: std::fmt::Display,
{
    format_date_divider_for_locale(at, l10n::current().locale())
}

fn base_lang(locale: &str) -> String {
    locale.split(['-', '_']).next().unwrap_or(locale).to_ascii_lowercase()
}

/// Formats a chat day-divider date for the given BCP-47 `locale`.
pub(crate) fn format_date_divider_for_locale<Tz: TimeZone>(at: &DateTime<Tz>, locale: &str) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let lang = base_lang(locale);
    let y = at.year();
    let m = at.month();
    let d = at.day();
    let wd = at.weekday();
    match lang.as_str() {
        "ko" => {
            let day_name = match wd {
                Weekday::Mon => "월요일",
                Weekday::Tue => "화요일",
                Weekday::Wed => "수요일",
                Weekday::Thu => "목요일",
                Weekday::Fri => "금요일",
                Weekday::Sat => "토요일",
                Weekday::Sun => "일요일",
            };
            format!("{y}년 {m}월 {d}일 {day_name}")
        }
        "ja" => {
            let day_name = match wd {
                Weekday::Mon => "月",
                Weekday::Tue => "火",
                Weekday::Wed => "水",
                Weekday::Thu => "木",
                Weekday::Fri => "金",
                Weekday::Sat => "土",
                Weekday::Sun => "日",
            };
            format!("{y}年{m}月{d}日 ({day_name})")
        }
        "zh" => format!("{y}年{m}月{d}日"),
        "de" => {
            let day_name = match wd {
                Weekday::Mon => "Montag",
                Weekday::Tue => "Dienstag",
                Weekday::Wed => "Mittwoch",
                Weekday::Thu => "Donnerstag",
                Weekday::Fri => "Freitag",
                Weekday::Sat => "Samstag",
                Weekday::Sun => "Sonntag",
            };
            let month_name = match m {
                1 => "Januar",
                2 => "Februar",
                3 => "März",
                4 => "April",
                5 => "Mai",
                6 => "Juni",
                7 => "Juli",
                8 => "August",
                9 => "September",
                10 => "Oktober",
                11 => "November",
                _ => "Dezember",
            };
            format!("{day_name}, {d}. {month_name} {y}")
        }
        "fr" => {
            let day_name = match wd {
                Weekday::Mon => "lundi",
                Weekday::Tue => "mardi",
                Weekday::Wed => "mercredi",
                Weekday::Thu => "jeudi",
                Weekday::Fri => "vendredi",
                Weekday::Sat => "samedi",
                Weekday::Sun => "dimanche",
            };
            let month_name = match m {
                1 => "janvier",
                2 => "février",
                3 => "mars",
                4 => "avril",
                5 => "mai",
                6 => "juin",
                7 => "juillet",
                8 => "août",
                9 => "septembre",
                10 => "octobre",
                11 => "novembre",
                _ => "décembre",
            };
            format!("{day_name} {d} {month_name} {y}")
        }
        "es" => {
            let day_name = match wd {
                Weekday::Mon => "lunes",
                Weekday::Tue => "martes",
                Weekday::Wed => "miércoles",
                Weekday::Thu => "jueves",
                Weekday::Fri => "viernes",
                Weekday::Sat => "sábado",
                Weekday::Sun => "domingo",
            };
            let month_name = match m {
                1 => "enero",
                2 => "febrero",
                3 => "marzo",
                4 => "abril",
                5 => "mayo",
                6 => "junio",
                7 => "julio",
                8 => "agosto",
                9 => "septiembre",
                10 => "octubre",
                11 => "noviembre",
                _ => "diciembre",
            };
            format!("{day_name}, {d} de {month_name} de {y}")
        }
        _ => at.format("%A, %B %-d, %Y").to_string(),
    }
}

/// Formats a non-negative integer with locale-appropriate thousands grouping.
#[allow(dead_code)]
pub(crate) fn format_number_for_locale(n: u64, locale: &str) -> String {
    let s = n.to_string();
    if s.len() <= 3 {
        return s;
    }
    let sep = match base_lang(locale).as_str() {
        "de" | "es" | "it" | "pt" | "nl" | "tr" | "vi" | "id" => ".",
        "fr" | "ru" | "pl" | "cs" | "sv" | "fi" => "\u{202F}",
        _ => ",",
    };
    let mut out = String::with_capacity(s.len() + (s.len() / 3) * sep.len());
    let rem = s.len() % 3;
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && i >= rem && (i - rem) % 3 == 0 {
            out.push_str(sep);
        }
        out.push(ch);
    }
    out
}

/// `AvatarUtils.DEFAULT_AVATAR_PRIMARY_COLORS`; the default avatar images
/// are indexed the same way.
pub const DEFAULT_AVATAR_COLORS: [u32; 6] = [0x4641d9, 0xf0b100, 0x00bba7, 0x2b7fff, 0xad46ff, 0x6a7282];

/// `getDefaultAvatarIndex`: user id modulo the number of default avatars.
pub fn default_avatar_index(user_id: Snowflake) -> usize {
    (user_id.0 % DEFAULT_AVATAR_COLORS.len() as u64) as usize
}

/// `MessageTypes` handled as system messages.
pub mod message_type {
    pub const RECIPIENT_ADD: u8 = 1;
    pub const RECIPIENT_REMOVE: u8 = 2;
    pub const CALL: u8 = 3;
    pub const CHANNEL_NAME_CHANGE: u8 = 4;
    pub const CHANNEL_ICON_CHANGE: u8 = 5;
    pub const CHANNEL_PINNED_MESSAGE: u8 = 6;
    pub const USER_JOIN: u8 = 7;
    pub const CHANNEL_FOLLOW_ADD: u8 = 12;
    pub const THREAD_CREATED: u8 = 18;
}

/// Icon of a system message row (the TS components' phosphor icons).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemIcon {
    /// Green arrow right: member joined / was added.
    Join,
    /// Red arrow left: member left / was removed.
    Leave,
    Pin,
    Call,
    Edit,
    Thread,
    Follow,
}

impl SystemIcon {
    pub fn glyph(self) -> &'static str {
        match self {
            Self::Join => "→",
            Self::Leave => "←",
            Self::Pin => "📌",
            Self::Call => "📞",
            Self::Edit => "✎",
            Self::Thread => "🧵",
            Self::Follow => "📢",
        }
    }
}

/// `SystemMessageUtils.getGuildJoinMessagesPlaintext` (en-US catalogs), in
/// order; `{username}` is the join actor.
const GUILD_JOIN_MESSAGES: [&str; 17] = [
    "Glad you're here, {username}!",
    "Welcome, {username}! Make yourself at home.",
    "Hello, {username}! Nice to have you here.",
    "Hello, {username}! Jump in whenever you're ready.",
    "Hey {username}, great to see you here!",
    "Hey there, {username}! Hope you enjoy your stay.",
    "Hey, {username}, welcome aboard!",
    "Glad you made it, {username}!",
    "Welcome in, {username}!",
    "Welcome, {username}!",
    "Welcome, {username}! We're glad you're here.",
    "Welcome, {username}! Hope you enjoy your time here.",
    "Welcome, {username}! Your next conversation starts here.",
    "Welcome, {username}. We're happy to have you here.",
    "Great to see you, {username}! Welcome in.",
    "You're here, {username}! Good to have you with us.",
    "You've arrived, {username}! Let's get started.",
];

/// The guild-join template for a message id: `extractTimestamp(id) % 17`.
pub fn guild_join_template(message_id: Snowflake) -> &'static str {
    let ix = (message_id.timestamp_ms() % GUILD_JOIN_MESSAGES.len() as u64) as usize;
    GUILD_JOIN_MESSAGES[ix]
}

/// A system message: icon, plain text and the byte ranges shown in bold
/// (user names, like `SystemMessageUsername`).
#[derive(Debug, Clone, PartialEq)]
pub struct SystemText {
    pub icon: SystemIcon,
    pub text: String,
    pub bold: Vec<Range<usize>>,
}

/// Translates `id` and fills its `{name}` placeholders with `args`,
/// recording each argument's range.
fn fill(icon: SystemIcon, id: &str, args: &[(&str, &str)]) -> SystemText {
    let template = l10n::current().t(id);
    let mut text = String::with_capacity(template.len());
    let mut bold = Vec::new();
    let mut rest = template.as_ref();
    while let Some(open) = rest.find('{') {
        let Some(len) = rest[open..].find('}') else { break };
        let close = open + len;
        text.push_str(&rest[..open]);
        let name = rest[open + 1..close].trim();
        match args.iter().find(|(key, _)| *key == name).map(|(_, value)| *value) {
            Some(arg) => {
                let start = text.len();
                text.push_str(arg);
                bold.push(start..text.len());
            }
            None => text.push_str(&rest[open..=close]),
        }
        rest = &rest[close + 1..];
    }
    text.push_str(rest);
    SystemText { icon, text, bold }
}

/// `SystemMessageUtils.stringify` with the actor and the first mentioned
/// user in bold. `author` is the actor's display name; `mentioned` the
/// first mentioned user's name if known. `None` for message types that
/// render as normal messages.
pub fn system_text(m: &Message, author: &str, mentioned: Option<&str>) -> Option<SystemText> {
    use message_type::*;
    let content = m.content.as_str();
    let text = match (m.kind, mentioned) {
        (USER_JOIN, _) => fill(SystemIcon::Join, guild_join_template(m.id), &[("username", author)]),
        (CHANNEL_PINNED_MESSAGE, _) => fill(
            SystemIcon::Pin,
            "{username} pinned a message to this channel.",
            &[("username", author)],
        ),
        (RECIPIENT_ADD, Some(other)) => fill(
            SystemIcon::Join,
            "{username} added {userName} to the group.",
            &[("username", author), ("userName", other)],
        ),
        (RECIPIENT_ADD, None) => fill(
            SystemIcon::Join,
            "{username} added someone to the group.",
            &[("username", author)],
        ),
        (RECIPIENT_REMOVE, _) if m.mentions.first().is_some_and(|u| u.id == m.author.id) => {
            fill(SystemIcon::Leave, "{username} has left the group.", &[("username", author)])
        }
        (RECIPIENT_REMOVE, Some(other)) => fill(
            SystemIcon::Leave,
            "{username} removed {userName} from the group.",
            &[("username", author), ("userName", other)],
        ),
        (RECIPIENT_REMOVE, None) => fill(
            SystemIcon::Leave,
            "{username} removed someone from the group.",
            &[("username", author)],
        ),
        (CHANNEL_NAME_CHANGE, _) if !content.is_empty() => fill(
            SystemIcon::Edit,
            "{username} changed the channel name to {newName}.",
            &[("username", author), ("newName", content)],
        ),
        (CHANNEL_NAME_CHANGE, _) => {
            fill(SystemIcon::Edit, "{username} changed the channel name.", &[("username", author)])
        }
        (CHANNEL_ICON_CHANGE, _) => {
            fill(SystemIcon::Edit, "{username} changed the channel icon.", &[("username", author)])
        }
        (CALL, _) => fill(SystemIcon::Call, "{username} started a call.", &[("username", author)]),
        (CHANNEL_FOLLOW_ADD, _) => fill(
            SystemIcon::Follow,
            "{username} followed {name} into this channel. Messages published there will appear here.",
            &[("username", author), ("name", content)],
        ),
        (THREAD_CREATED, _) => fill(
            SystemIcon::Thread,
            "{username} started a thread: {threadName}",
            &[("username", author), ("threadName", content)],
        ),
        _ => return None,
    };
    Some(text)
}

/// Up to two leading characters of the first two words.
pub fn initials(name: &str) -> String {
    let out: String = name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect();
    if out.is_empty() { "?".to_owned() } else { out }
}

#[cfg(test)]
mod tests {
    use chrono::FixedOffset;
    use serde_json::json;

    use super::*;

    fn message(id: u64, author: u64, ts: &str, kind: u8, flags: u64) -> Message {
        serde_json::from_value(json!({
            "id": id.to_string(),
            "channel_id": "5",
            "author": { "id": author.to_string(), "username": "u" },
            "content": "x",
            "timestamp": ts,
            "type": kind,
            "flags": flags,
        }))
        .expect("message fixture")
    }

    fn utc() -> FixedOffset {
        FixedOffset::east_opt(0).expect("offset")
    }

    fn group(prev: &Message, cur: &Message) -> bool {
        let tz = utc();
        let (pa, ca) = (message_time(prev, &tz), message_time(cur, &tz));
        starts_group(Some((prev, pa.as_ref())), cur, ca.as_ref())
    }

    #[test]
    fn grouping_follows_ts_rules() {
        let a = message(1, 7, "2026-11-18T10:00:00+00:00", 0, 0);
        assert!(starts_group::<FixedOffset>(None, &a, None));
        // Same author within 7 minutes: grouped.
        assert!(!group(&a, &message(2, 7, "2026-11-18T10:07:00+00:00", 0, 0)));
        // Over 7 minutes, other author, reply, system: new group.
        assert!(group(&a, &message(2, 7, "2026-11-18T10:07:01+00:00", 0, 0)));
        assert!(group(&a, &message(2, 8, "2026-11-18T10:01:00+00:00", 0, 0)));
        assert!(group(&a, &message(2, 7, "2026-11-18T10:01:00+00:00", 19, 0)));
        assert!(group(&a, &message(2, 7, "2026-11-18T10:01:00+00:00", 7, 0)));
        // Day boundary.
        let late = message(1, 7, "2026-11-18T23:59:00+00:00", 0, 0);
        assert!(group(&late, &message(2, 7, "2026-11-19T00:01:00+00:00", 0, 0)));
        // Suppressed notifications start a group.
        assert!(group(&a, &message(2, 7, "2026-11-18T10:01:00+00:00", 0, FLAG_SUPPRESS_NOTIFICATIONS)));
    }

    #[test]
    fn pending_rows_use_the_id_time() {
        let mut m = message(Snowflake::from_unix_ms(1_763_000_000_000).0, 7, "", 0, 0);
        m.timestamp.clear();
        let at = message_time(&m, &utc()).expect("time from id");
        assert_eq!(at.timestamp_millis(), 1_763_000_000_000);
    }

    #[test]
    fn times_are_relative_and_local() {
        let kst = FixedOffset::east_opt(9 * 3600).expect("offset");
        let now = kst.with_ymd_and_hms(2026, 11, 18, 20, 0, 0).single().expect("now");
        let parse = |s: &str| DateTime::parse_from_rfc3339(s).expect("ts").with_timezone(&kst);
        // 10:20 UTC is 19:20 in UTC+9.
        assert_eq!(format_message_time(&parse("2026-11-18T10:20:30+00:00"), &now), "Today at 19:20");
        // 2026-11-17 16:00 UTC is 2026-11-18 01:00 local: still today.
        assert_eq!(format_message_time(&parse("2026-11-17T16:00:00+00:00"), &now), "Today at 01:00");
        assert_eq!(format_message_time(&parse("2026-11-17T10:00:00+00:00"), &now), "Yesterday at 19:00");
        assert_eq!(format_message_time(&parse("2026-11-01T00:00:00+00:00"), &now), "2026-11-01 09:00");
    }

    #[test]
    fn images_fit_the_preview_box() {
        assert_eq!(fit_image(Some(800), Some(600)), (400., 300.));
        assert_eq!(fit_image(Some(1000), Some(200)), (400., 80.));
        assert_eq!(fit_image(Some(100), Some(900)), (33., 300.));
        assert_eq!(fit_image(Some(50), Some(40)), (50., 40.));
        assert_eq!(fit_image(None, Some(40)), (400., 300.));
        assert_eq!(fit_image(Some(0), Some(0)), (400., 300.));
    }

    #[test]
    fn full_dates_are_long_en_us() {
        let at = utc().with_ymd_and_hms(2026, 11, 18, 1, 17, 0).single().expect("date");
        assert_eq!(format_full_date(&at), "Wednesday, November 18, 2026");
        let at = utc().with_ymd_and_hms(2026, 1, 4, 0, 0, 0).single().expect("date");
        assert_eq!(format_full_date(&at), "Sunday, January 4, 2026");
    }

    #[test]
    fn date_dividers_localize_by_locale() {
        let sun = utc().with_ymd_and_hms(2026, 11, 22, 12, 0, 0).single().expect("date");
        assert_eq!(format_date_divider_for_locale(&sun, "ko"), "2026년 11월 22일 일요일");
        assert_eq!(format_date_divider_for_locale(&sun, "ko-KR"), "2026년 11월 22일 일요일");
        assert_eq!(format_date_divider_for_locale(&sun, "ja"), "2026年11月22日 (日)");
        assert_eq!(format_date_divider_for_locale(&sun, "ja-JP"), "2026年11月22日 (日)");
        assert_eq!(format_date_divider_for_locale(&sun, "zh-CN"), "2026年11月22日");
        assert_eq!(format_date_divider_for_locale(&sun, "zh-TW"), "2026年11月22日");
        assert_eq!(format_date_divider_for_locale(&sun, "de"), "Sonntag, 22. November 2026");
        assert_eq!(format_date_divider_for_locale(&sun, "fr"), "dimanche 22 novembre 2026");
        assert_eq!(format_date_divider_for_locale(&sun, "en-US"), "Sunday, November 22, 2026");

        // Switching l10n locale updates format_full_date too.
        l10n::set_locale("ko");
        assert_eq!(format_full_date(&sun), "2026년 11월 22일 일요일");
        l10n::set_locale("en-US");
        assert_eq!(format_full_date(&sun), "Sunday, November 22, 2026");
    }

    #[test]
    fn numbers_group_thousands_by_locale() {
        for locale in ["en-US", "ko", "ja", "zh-CN", "de", "fr"] {
            assert_eq!(format_number_for_locale(0, locale), "0");
            assert_eq!(format_number_for_locale(999, locale), "999");
        }
        assert_eq!(format_number_for_locale(1000, "en-US"), "1,000");
        assert_eq!(format_number_for_locale(1_234_567, "en-US"), "1,234,567");
        assert_eq!(format_number_for_locale(1000, "ko"), "1,000");
        assert_eq!(format_number_for_locale(1_234_567, "ja"), "1,234,567");
        assert_eq!(format_number_for_locale(1000, "de"), "1.000");
        assert_eq!(format_number_for_locale(1_234_567, "de"), "1.234.567");
        assert_eq!(format_number_for_locale(1000, "fr"), "1\u{202F}000");
        assert_eq!(format_number_for_locale(1_234_567, "fr"), "1\u{202F}234\u{202F}567");
    }

    #[test]
    fn default_avatars_follow_the_user_id() {
        assert_eq!(default_avatar_index(Snowflake(6)), 0);
        assert_eq!(default_avatar_index(Snowflake(1_400_000_000_000_000_004)), 1_400_000_000_000_000_004 % 6);
    }

    fn system(kind: u8, ms: u64, content: &str, mentions: serde_json::Value) -> Message {
        serde_json::from_value(json!({
            "id": Snowflake::from_unix_ms(ms).0.to_string(),
            "channel_id": "5",
            "author": { "id": "7", "username": "u" },
            "content": content,
            "timestamp": "2026-11-18T01:17:00+00:00",
            "type": kind,
            "mentions": mentions,
        }))
        .expect("system fixture")
    }

    /// Bold ranges as `**...**`, for readable assertions.
    fn marked(t: &SystemText) -> String {
        let mut out = String::new();
        let mut at = 0;
        for r in &t.bold {
            out.push_str(&t.text[at..r.start]);
            out.push_str(&format!("**{}**", &t.text[r.clone()]));
            at = r.end;
        }
        out.push_str(&t.text[at..]);
        out
    }

    /// The join template is picked by `timestamp_ms % 17`, like
    /// `SystemMessageUtils.getGuildJoinMessage`.
    #[test]
    fn guild_join_text_is_picked_by_timestamp() {
        let base = 1_763_428_620_000 - 1_763_428_620_000 % 17;
        let join = |offset: u64| system(message_type::USER_JOIN, base + offset, "", json!([]));
        let text = system_text(&join(0), "tester2", None).expect("join");
        assert_eq!(text.icon, SystemIcon::Join);
        assert_eq!(marked(&text), "Glad you're here, **tester2**!");
        assert_eq!(
            marked(&system_text(&join(15), "tester2", None).expect("join")),
            "You're here, **tester2**! Good to have you with us."
        );
        // Names are not templates.
        assert_eq!(marked(&system_text(&join(17), "{1}_b", None).expect("join")), "Glad you're here, **{1}_b**!");
    }

    #[test]
    fn other_system_types_follow_stringify() {
        let s = |kind, content, mentions| {
            system_text(&system(kind, 1_763_428_620_000, content, mentions), "Ann", Some("Bob"))
        };
        let text = |kind, content| s(kind, content, json!([{ "id": "8", "username": "bob" }])).map(|t| marked(&t));
        assert_eq!(
            text(message_type::CHANNEL_PINNED_MESSAGE, "").as_deref(),
            Some("**Ann** pinned a message to this channel.")
        );
        assert_eq!(text(message_type::RECIPIENT_ADD, "").as_deref(), Some("**Ann** added **Bob** to the group."));
        assert_eq!(
            text(message_type::RECIPIENT_REMOVE, "").as_deref(),
            Some("**Ann** removed **Bob** from the group.")
        );
        let left = s(message_type::RECIPIENT_REMOVE, "", json!([{ "id": "7", "username": "u" }])).expect("left");
        assert_eq!((left.icon, marked(&left).as_str()), (SystemIcon::Leave, "**Ann** has left the group."));
        assert_eq!(
            text(message_type::CHANNEL_NAME_CHANGE, "new").as_deref(),
            Some("**Ann** changed the channel name to **new**.")
        );
        assert_eq!(text(message_type::CALL, "").as_deref(), Some("**Ann** started a call."));
        assert_eq!(text(message_type::THREAD_CREATED, "t").as_deref(), Some("**Ann** started a thread: **t**"));
        assert_eq!(text(0, "hi"), None);
        assert_eq!(text(19, "reply"), None);
    }

    #[test]
    fn initials_take_two_words() {
        assert_eq!(initials("Fluxer Rust Port"), "FR");
        assert_eq!(initials("  "), "?");
        assert_eq!(initials("한글 서버"), "한서");
    }
}
