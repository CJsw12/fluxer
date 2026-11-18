//! `<t:unix:style>` text, as `formatTimestamp` (`DateFormatter.ts`) gives it
//! for the `en-US` locale without a date format preset:
//! `Intl.DateTimeFormat` with the per-style options of
//! `DateTimestampStyle.ts`, and `Intl.RelativeTimeFormat` with
//! `numeric: 'auto'` for the relative style.

use chrono::{DateTime, Datelike, FixedOffset, Local, NaiveDateTime, Timelike, Utc};

use crate::model::TimestampStyle;

/// The time zone timestamps are shown in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    /// The system's local zone (the web client's default).
    Local,
    Fixed(FixedOffset),
}

/// Settings and clock for formatting timestamps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeFormat {
    pub zone: Zone,
    /// 12-hour clock (`shouldUse12HourFormat`; `en-US` default is true).
    pub hour12: bool,
    /// "Now" for relative timestamps, in Unix milliseconds.
    pub now_ms: i64,
}

impl TimeFormat {
    /// Local zone, current time.
    pub fn local(hour12: bool) -> Self {
        Self { zone: Zone::Local, hour12, now_ms: Utc::now().timestamp_millis() }
    }
}

/// JavaScript `Date`'s range: |ms| <= 8.64e15.
const MAX_JS_DATE_MS: i128 = 8_640_000_000_000_000;

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

const WEEKDAYS: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

fn to_local(unix: u64, time: &TimeFormat) -> Option<NaiveDateTime> {
    let ms = i128::from(unix) * 1000;
    if ms > MAX_JS_DATE_MS {
        return None;
    }
    let utc = DateTime::<Utc>::from_timestamp_millis(i64::try_from(ms).ok()?)?;
    Some(match time.zone {
        Zone::Local => utc.with_timezone(&Local).naive_local(),
        Zone::Fixed(offset) => utc.with_timezone(&offset).naive_local(),
    })
}

/// `formatTimestamp`: the text a timestamp shows; the number itself for a
/// date JavaScript cannot represent.
pub fn format_timestamp(unix: u64, style: TimestampStyle, time: &TimeFormat) -> String {
    let Some(date) = to_local(unix, time) else {
        return unix.to_string();
    };
    if style == TimestampStyle::RelativeTime {
        return relative_time(i128::from(unix) * 1000 - i128::from(time.now_ms));
    }
    let hour12 = time.hour12;
    match style {
        TimestampStyle::ShortTime => clock(&date, hour12, false),
        TimestampStyle::LongTime => clock(&date, hour12, true),
        TimestampStyle::ShortDate => numeric_date(&date),
        TimestampStyle::LongDate => long_date(&date),
        TimestampStyle::LongDateTime => format!(
            "{}, {} at {}",
            WEEKDAYS[date.weekday().num_days_from_monday() as usize],
            long_date(&date),
            clock(&date, hour12, false)
        ),
        TimestampStyle::ShortDateShortTime => format!("{}, {}", numeric_date(&date), clock(&date, hour12, false)),
        TimestampStyle::ShortDateMediumTime => format!("{}, {}", numeric_date(&date), clock(&date, hour12, true)),
        // ShortDateTime, and the default options for anything else.
        TimestampStyle::ShortDateTime | TimestampStyle::RelativeTime => {
            format!("{} at {}", long_date(&date), clock(&date, hour12, false))
        }
    }
}

/// Tooltip text (`getFormattedDateTimeWithSeconds`): weekday, long date and
/// the time with seconds.
pub fn format_full_date_time(unix: u64, time: &TimeFormat) -> Option<String> {
    let date = to_local(unix, time)?;
    Some(format!(
        "{}, {} {}",
        WEEKDAYS[date.weekday().num_days_from_monday() as usize],
        long_date(&date),
        clock(&date, time.hour12, true)
    ))
}

fn month_name(date: &NaiveDateTime) -> &'static str {
    MONTHS[date.month0() as usize]
}

/// `November 18, 2026`.
fn long_date(date: &NaiveDateTime) -> String {
    format!("{} {}, {}", month_name(date), date.day(), date.year())
}

/// `11/18/2026`.
fn numeric_date(date: &NaiveDateTime) -> String {
    format!("{}/{}/{}", date.month(), date.day(), date.year())
}

/// `4:05 PM` / `16:05` (`hour: 'numeric', minute: 'numeric'`), with
/// `:SS` when `seconds`.
fn clock(date: &NaiveDateTime, hour12: bool, seconds: bool) -> String {
    let (pm, hour) = date.hour12();
    let mut out = if hour12 {
        format!("{hour}:{:02}", date.minute())
    } else {
        format!("{:02}:{:02}", date.hour(), date.minute())
    };
    if seconds {
        out.push_str(&format!(":{:02}", date.second()));
    }
    if hour12 {
        out.push_str(if pm { " PM" } else { " AM" });
    }
    out
}

const MS_PER_SECOND: i128 = 1000;
const MS_PER_MINUTE: i128 = 60 * MS_PER_SECOND;
const MS_PER_HOUR: i128 = 60 * MS_PER_MINUTE;
const MS_PER_DAY: i128 = 24 * MS_PER_HOUR;

/// `formatRelativeTime` for `diff_ms = date - now`.
fn relative_time(diff_ms: i128) -> String {
    let abs = diff_ms.abs();
    let future = diff_ms >= 0;
    let days = abs / MS_PER_DAY;
    let (amount, unit) = if days >= 365 {
        (days / 365, Unit::Year)
    } else if days >= 30 {
        (days / 30, Unit::Month)
    } else if days >= 7 {
        (days / 7, Unit::Week)
    } else if days > 0 {
        (days, Unit::Day)
    } else if abs / MS_PER_HOUR > 0 {
        (abs / MS_PER_HOUR, Unit::Hour)
    } else if abs / MS_PER_MINUTE > 0 {
        (abs / MS_PER_MINUTE, Unit::Minute)
    } else {
        (abs / MS_PER_SECOND, Unit::Second)
    };
    relative_format(amount, unit, future)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Unit {
    Year,
    Month,
    Week,
    Day,
    Hour,
    Minute,
    Second,
}

/// `Intl.RelativeTimeFormat('en', {numeric: 'auto'}).format(±amount, unit)`.
fn relative_format(amount: i128, unit: Unit, future: bool) -> String {
    let phrase = match (unit, amount, future) {
        (Unit::Second, 0, _) => Some("now"),
        (Unit::Day, 1, true) => Some("tomorrow"),
        (Unit::Day, 1, false) => Some("yesterday"),
        (Unit::Week, 1, true) => Some("next week"),
        (Unit::Week, 1, false) => Some("last week"),
        (Unit::Month, 1, true) => Some("next month"),
        (Unit::Month, 1, false) => Some("last month"),
        (Unit::Year, 1, true) => Some("next year"),
        (Unit::Year, 1, false) => Some("last year"),
        _ => None,
    };
    if let Some(phrase) = phrase {
        return phrase.to_owned();
    }
    let name = match unit {
        Unit::Year => "year",
        Unit::Month => "month",
        Unit::Week => "week",
        Unit::Day => "day",
        Unit::Hour => "hour",
        Unit::Minute => "minute",
        Unit::Second => "second",
    };
    let plural = if amount == 1 { "" } else { "s" };
    let count = group_thousands(amount);
    if future { format!("in {count} {name}{plural}") } else { format!("{count} {name}{plural} ago") }
}

/// `1,234` (`en` number grouping).
fn group_thousands(value: i128) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(now_ms: i64, hour12: bool) -> TimeFormat {
        TimeFormat { zone: Zone::Fixed(FixedOffset::east_opt(0).expect("utc")), hour12, now_ms }
    }

    // 2026-11-18T16:05:09Z, a Wednesday.
    const T: u64 = 1_795_017_909;

    #[test]
    fn absolute_styles_match_intl_en_us() {
        let time = utc(0, true);
        let cases = [
            (TimestampStyle::ShortTime, "4:05 PM"),
            (TimestampStyle::LongTime, "4:05:09 PM"),
            (TimestampStyle::ShortDate, "11/18/2026"),
            (TimestampStyle::LongDate, "November 18, 2026"),
            (TimestampStyle::ShortDateTime, "November 18, 2026 at 4:05 PM"),
            (TimestampStyle::LongDateTime, "Wednesday, November 18, 2026 at 4:05 PM"),
            (TimestampStyle::ShortDateShortTime, "11/18/2026, 4:05 PM"),
            (TimestampStyle::ShortDateMediumTime, "11/18/2026, 4:05:09 PM"),
        ];
        for (style, expected) in cases {
            assert_eq!(format_timestamp(T, style, &time), expected, "{style:?}");
        }
        assert_eq!(format_full_date_time(T, &time).as_deref(), Some("Wednesday, November 18, 2026 4:05:09 PM"));
    }

    #[test]
    fn twenty_four_hour_clock_and_midnight_noon() {
        let h24 = utc(0, false);
        assert_eq!(format_timestamp(T, TimestampStyle::ShortTime, &h24), "16:05");
        assert_eq!(format_timestamp(T, TimestampStyle::LongTime, &h24), "16:05:09");
        let h12 = utc(0, true);
        // 2026-11-18T00:07:00Z and 12:07:00Z.
        assert_eq!(format_timestamp(1_794_960_420, TimestampStyle::ShortTime, &h12), "12:07 AM");
        assert_eq!(format_timestamp(1_794_960_420 + 12 * 3600, TimestampStyle::ShortTime, &h12), "12:07 PM");
        assert_eq!(format_timestamp(1_794_960_420, TimestampStyle::ShortTime, &h24), "00:07");
    }

    #[test]
    fn zone_offset_shifts_the_wall_clock() {
        let kst = TimeFormat {
            zone: Zone::Fixed(FixedOffset::east_opt(9 * 3600).expect("kst")),
            hour12: true,
            now_ms: 0,
        };
        // 16:05Z is 01:05 the next day in UTC+9.
        assert_eq!(format_timestamp(T, TimestampStyle::ShortDateShortTime, &kst), "11/19/2026, 1:05 AM");
    }

    #[test]
    fn relative_time_matches_intl_numeric_auto() {
        let now = T as i64 * 1000;
        let time = utc(now, true);
        let at = |delta_s: i64| format_timestamp((T as i64 + delta_s) as u64, TimestampStyle::RelativeTime, &time);
        assert_eq!(at(0), "now");
        assert_eq!(at(1), "in 1 second");
        assert_eq!(at(-30), "30 seconds ago");
        assert_eq!(at(-60), "1 minute ago");
        assert_eq!(at(150), "in 2 minutes");
        assert_eq!(at(-3600), "1 hour ago");
        assert_eq!(at(-86_400), "yesterday");
        assert_eq!(at(86_400), "tomorrow");
        assert_eq!(at(-3 * 86_400), "3 days ago");
        assert_eq!(at(7 * 86_400), "next week");
        assert_eq!(at(-14 * 86_400), "2 weeks ago");
        assert_eq!(at(-30 * 86_400), "last month");
        assert_eq!(at(65 * 86_400), "in 2 months");
        assert_eq!(at(-365 * 86_400), "last year");
        assert_eq!(at(-3 * 365 * 86_400), "3 years ago");
        assert_eq!(group_thousands(1234567), "1,234,567");
    }

    #[test]
    fn dates_beyond_javascript_range_show_the_number() {
        let time = utc(0, true);
        let too_big = 8_640_000_000_001;
        assert_eq!(format_timestamp(too_big, TimestampStyle::ShortDate, &time), too_big.to_string());
        assert_eq!(format_full_date_time(too_big, &time), None);
    }
}
