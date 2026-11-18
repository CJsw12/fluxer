//! Reaction pills under a message (`MessageReactions.tsx`,
//! `ReactionImage.tsx`): emoji + count, highlighted when the current user
//! reacted; clicking toggles the user's reaction.

use std::rc::Rc;

use fluxer_client_core::state::{PartialEmoji, Reaction};
use gpui_kit::{AnyElement, App, ObjectFit, SharedString, StyledImage as _, Window, div, img, prelude::*, px};

use crate::theme::Theme;

/// Emoji image size in a pill (`?size=44` is the 2x source of ~22px).
const EMOJI_SIZE: f32 = 18.;

/// One pill, ready to render.
#[derive(Debug, Clone, PartialEq)]
pub struct ReactionPill {
    pub emoji: PartialEmoji,
    /// Custom emoji image (`{media}/emojis/{id}.webp?size=44`); `None` for
    /// unicode emoji, which render as text.
    pub image_url: Option<SharedString>,
    /// The unicode emoji itself, or `:name:` for a custom one (alt text).
    pub label: SharedString,
    pub count: u32,
    pub me: bool,
}

/// Custom emoji image on the media endpoint (`AvatarUtils.getEmojiURL`).
pub fn emoji_url(media: &str, emoji: &PartialEmoji) -> Option<String> {
    let id = emoji.id?;
    Some(format!("{}/emojis/{id}.webp?size=44", media.trim_end_matches('/')))
}

/// Pills for a message's reactions, in server order. Reactions whose count
/// dropped to zero are skipped.
pub fn reaction_pills(reactions: &[Reaction], media: Option<&str>) -> Vec<ReactionPill> {
    reactions
        .iter()
        .filter(|r| r.count > 0)
        .map(|r| ReactionPill {
            image_url: media.and_then(|media| emoji_url(media, &r.emoji)).map(Into::into),
            label: if r.emoji.id.is_some() { format!(":{}:", r.emoji.name).into() } else { r.emoji.name.clone().into() },
            emoji: r.emoji.clone(),
            count: r.count,
            me: r.me,
        })
        .collect()
}

/// The pill row; empty element when there are no pills. `on_toggle` gets
/// the clicked emoji (call `Backend::toggle_reaction` with it).
/// `message_key` makes the element ids unique per message.
pub fn reaction_bar(
    message_key: u64,
    pills: &[ReactionPill],
    theme: &Theme,
    on_toggle: impl Fn(&PartialEmoji, &mut Window, &mut App) + 'static,
) -> AnyElement {
    if pills.is_empty() {
        return div().into_any_element();
    }
    let on_toggle = Rc::new(on_toggle);
    div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap_1()
        .mt_1()
        .children(pills.iter().enumerate().map(|(ix, pill)| {
            let on_toggle = on_toggle.clone();
            let emoji = pill.emoji.clone();
            let (bg, border, fg) = if pill.me {
                (theme.mention_bg, theme.mention_fg, theme.mention_fg)
            } else {
                (theme.muted, theme.muted, theme.muted_foreground)
            };
            let glyph = match &pill.image_url {
                Some(url) => {
                    let alt = pill.label.clone();
                    img(url.clone())
                        .size(px(EMOJI_SIZE))
                        .object_fit(ObjectFit::Contain)
                        .with_fallback(move || div().text_xs().child(alt.clone()).into_any_element())
                        .into_any_element()
                }
                None => div().child(pill.label.clone()).into_any_element(),
            };
            div()
                .id(SharedString::from(format!("reaction-{message_key}-{ix}")))
                .debug_selector(|| format!("reaction-{message_key}-{ix}"))
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .h(px(24.))
                .px(px(6.))
                .rounded(px(8.))
                .border_1()
                .border_color(border)
                .bg(bg)
                .cursor_pointer()
                .hover(|s| s.border_color(theme.border))
                .child(glyph)
                .child(div().text_xs().font_weight(gpui_kit::FontWeight::SEMIBOLD).text_color(fg).child(pill.count.to_string()))
                .on_click(move |_, window, cx| on_toggle(&emoji, window, cx))
        }))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use fluxer_client_core::state::Snowflake;

    use super::*;

    fn reaction(id: Option<u64>, name: &str, count: u32, me: bool) -> Reaction {
        Reaction { emoji: PartialEmoji { id: id.map(Snowflake), name: name.to_owned(), animated: false }, count, me }
    }

    #[test]
    fn pills_keep_order_and_build_custom_urls() {
        let reactions = [reaction(None, "👍", 2, true), reaction(Some(77), "blob", 1, false), reaction(None, "x", 0, false)];
        let pills = reaction_pills(&reactions, Some("https://media.example/"));
        assert_eq!(pills.len(), 2, "zero-count reactions are hidden");
        assert_eq!((pills[0].label.as_ref(), pills[0].count, pills[0].me), ("👍", 2, true));
        assert_eq!(pills[0].image_url, None);
        assert_eq!(pills[1].label.as_ref(), ":blob:");
        assert_eq!(pills[1].image_url.as_ref().map(|u| u.as_ref()), Some("https://media.example/emojis/77.webp?size=44"));
        assert_eq!(reaction_pills(&reactions, None)[1].image_url, None);
    }
}
