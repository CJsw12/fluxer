//! "X is typing…" under the composer (`RollingTypingText.tsx`,
//! `TypingTierText.ts`) and the timer that drops expired typers.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fluxer_client_core::state::{ClientState, Snowflake};
use gpui_kit::{AnyElement, Context, FontWeight, SharedString, Task, div, prelude::*, px};

use crate::backend::Backend;
use crate::theme::Theme;
use crate::ui::l10n;
use crate::ui::reply::member_name;

/// Height of the indicator row, kept when nobody types so the layout does
/// not jump.
const ROW_HEIGHT: f32 = 20.;

/// Indicator text for the typers' names, `None` when nobody types. Up to
/// three names (`TYPING_ROLLING_MAX_NAMES`), then a tier text.
pub fn typing_text(names: &[String]) -> Option<String> {
    match names {
        [] => None,
        [a] => Some(ellipsis(l10n::tr_args("{a} is typing...", &[("a", a.as_str())]))),
        [a, b] => Some(ellipsis(l10n::tr_args(
            "{a} and {b} are typing...",
            &[("a", a.as_str()), ("b", b.as_str())],
        ))),
        [a, b, c] => Some(ellipsis(l10n::tr_args(
            "{a}, {b} and {c} are typing...",
            &[("a", a.as_str()), ("b", b.as_str()), ("c", c.as_str())],
        ))),
        more => Some(tier_text(more.len())),
    }
}

/// The catalogs write the typing suffix as ASCII `...`; the UI shows `…`.
fn ellipsis(text: String) -> String {
    text.replace("...", "…")
}

/// `getTypingTierText` (en-US); only the general tier is in the catalogs.
fn tier_text(count: usize) -> String {
    match count {
        0..=4 => ellipsis(l10n::tr("Several people are typing...")),
        5..=9 => "A handful of keyboard warriors are assembling...".to_owned(),
        10..=14 => "A symphony of clacking keys is underway...".to_owned(),
        15..=19 => "It's a full-blown typing fiesta in here".to_owned(),
        _ => "Whoa, it's a typing apocalypse".to_owned(),
    }
}

/// Names of the users typing in `channel_id` at `now_ms`, in start order
/// (guild nick, else display name). Users the state does not know are
/// skipped.
pub fn typing_names(state: &ClientState, channel_id: Snowflake, now_ms: i64) -> Vec<String> {
    let guild_id = state.channels.get(&channel_id).and_then(|c| c.guild_id);
    state
        .typing_users(channel_id, now_ms)
        .into_iter()
        .filter_map(|user_id| {
            let member = guild_id.and_then(|g| state.members.get(&g)?.get(&user_id));
            let user = member
                .map(|m| &m.user)
                .or_else(|| state.users.get(&user_id))
                .or_else(|| state.channels.get(&channel_id)?.recipients.iter().find(|u| u.id == user_id))?;
            Some(member_name(state, guild_id, user))
        })
        .collect()
}

/// The indicator text for a channel now.
pub fn channel_typing_text(state: &ClientState, channel_id: Snowflake) -> Option<SharedString> {
    typing_text(&typing_names(state, channel_id, now_ms())).map(Into::into)
}

/// The indicator row: three dots and the text, or an empty row of the same
/// height.
pub fn typing_indicator(text: Option<SharedString>, theme: &Theme) -> AnyElement {
    div()
        .h(px(ROW_HEIGHT))
        .px_4()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .text_xs()
        .text_color(theme.muted_foreground)
        .when_some(text, |row, text| {
            row.child(div().font_weight(FontWeight::BOLD).child("•••")).child(div().truncate().child(text))
        })
        .into_any_element()
}

/// Time until the next typer expires at `now_ms` (`None`: nobody types).
/// One extra millisecond so the expiry has passed when the timer fires.
pub fn expiry_delay(state: &ClientState, now_ms: i64) -> Option<Duration> {
    let next = state.next_typing_expiry_ms()?;
    let wait = next.saturating_sub(now_ms).max(0).saturating_add(1);
    Some(Duration::from_millis(u64::try_from(wait).unwrap_or(0)))
}

/// Wall clock in Unix ms, the clock `ClientState::apply` stamps typers with.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}

/// Starts a timer for the next typing expiry: when it fires, expired typers
/// are dropped from the state and `on_expired` runs (re-read the state and
/// call this again to arm the next one). Keep the returned task; dropping
/// it cancels the timer. `None` when nobody types.
pub fn schedule_expiry<V: 'static>(
    backend: &Backend,
    cx: &mut Context<V>,
    on_expired: fn(&mut V, &mut Context<V>),
) -> Option<Task<()>> {
    let delay = expiry_delay(&backend.state(), now_ms())?;
    let backend = backend.clone();
    Some(cx.spawn(async move |this, cx| {
        cx.background_executor().timer(delay).await;
        backend.state().expire_typing(now_ms());
        let _ = this.update(cx, |view, cx| on_expired(view, cx));
    }))
}

#[cfg(test)]
mod tests {
    use fluxer_client_core::Dispatch;
    use serde_json::json;

    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn typing_text_follows_the_web_client() {
        assert_eq!(typing_text(&[]), None);
        assert_eq!(typing_text(&names(&["Ann"])).as_deref(), Some("Ann is typing…"));
        assert_eq!(typing_text(&names(&["Ann", "Bob"])).as_deref(), Some("Ann and Bob are typing…"));
        assert_eq!(typing_text(&names(&["Ann", "Bob", "Cy"])).as_deref(), Some("Ann, Bob and Cy are typing…"));
        assert_eq!(typing_text(&names(&["a", "b", "c", "d"])).as_deref(), Some("Several people are typing…"));
        assert_eq!(
            typing_text(&names(&["a", "b", "c", "d", "e"])).as_deref(),
            Some("A handful of keyboard warriors are assembling...")
        );
        assert_eq!(tier_text(25), "Whoa, it's a typing apocalypse");
    }

    #[test]
    fn names_use_guild_nicks_and_expire() {
        let mut state = ClientState::new();
        state.apply(&Dispatch {
            t: "READY".to_owned(),
            s: 1,
            d: json!({
                "session_id": "s", "user": { "id": "100", "username": "me" },
                "guilds": [{
                    "id": "1", "properties": { "name": "G", "owner_id": "100" },
                    "roles": [], "channels": [{ "id": "11", "type": 0, "name": "general" }],
                    "members": [
                        { "user": { "id": "200", "username": "ann", "global_name": "Ann" }, "nick": "Annie", "roles": [] },
                        { "user": { "id": "201", "username": "bob" }, "roles": [] }
                    ]
                }],
                "private_channels": [], "read_states": []
            }),
        });
        let channel = Snowflake(11);
        for (user, at) in [("200", 1_000), ("201", 2_000), ("100", 2_000)] {
            state.apply_at(
                &Dispatch {
                    t: "TYPING_START".to_owned(),
                    s: 2,
                    d: json!({ "channel_id": "11", "guild_id": "1", "user_id": user, "timestamp": 0 }),
                },
                at,
            );
        }
        assert_eq!(typing_names(&state, channel, 3_000), names(&["Annie", "bob"]), "self is never listed");
        assert_eq!(expiry_delay(&state, 3_000), Some(Duration::from_millis(8_001)));
        state.expire_typing(11_500);
        assert_eq!(typing_names(&state, channel, 11_500), names(&["bob"]));
        state.expire_typing(12_500);
        assert_eq!(expiry_delay(&state, 12_500), None);
    }
}
