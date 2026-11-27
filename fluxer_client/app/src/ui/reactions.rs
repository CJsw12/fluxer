//! Reaction pills under a message (`MessageReactions.tsx`,
//! `ReactionImage.tsx`): emoji + count, highlighted when the current user
//! reacted; clicking toggles the user's reaction.

use std::rc::Rc;

use fluxer_client_core::state::{PartialEmoji, Reaction};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{AnyElement, App, ObjectFit, Pixels, Point, SharedString, StyledImage as _, Window, div, hsla, img, prelude::*, px};

use crate::icons::AppIcon;
use crate::parity_probe::FlxExt as _;
use crate::theme::Theme;

/// Emoji size in a pill (`.emoji` 1.25rem).
const EMOJI_SIZE: f32 = 20.;

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
/// the clicked emoji (call `Backend::toggle_reaction` with it). `on_add`
/// gets the click position of the trailing add-reaction button, which
/// `MessageReactions.tsx` always shows next to existing pills.
/// `message_key` makes the element ids unique per message.
pub fn reaction_bar(
    message_key: u64,
    pills: &[ReactionPill],
    theme: &Theme,
    on_toggle: impl Fn(&PartialEmoji, &mut Window, &mut App) + 'static,
    on_add: impl Fn(Point<Pixels>, &mut Window, &mut App) + 'static,
) -> AnyElement {
    if pills.is_empty() {
        return div().into_any_element();
    }
    let on_toggle = Rc::new(on_toggle);
    // ThemeVariableManifest: dark `--brand-tint` = 36% brand over `--background-secondary`,
    // `--text-on-brand-tint` = on-brand text; light = 12% brand over `--background-primary`,
    // text in the brand colour. `--brand-tint-border` is the brand itself in both.
    let dark = theme.background.l < 0.5;
    let on_brand = if dark { hsla(0., 0., 0.98, 1.) } else { theme.brand };
    let (tint, tint_hover) = if dark {
        (theme.brand.opacity(0.36), theme.brand.opacity(0.46))
    } else {
        (theme.brand.opacity(0.12), theme.brand.opacity(0.2))
    };
    let pill_els = pills.iter().enumerate().map(|(ix, pill)| {
        let on_toggle = on_toggle.clone();
        let emoji = pill.emoji.clone();
        let (bg, bg_hover, border, border_hover, fg) = if pill.me {
            (tint, tint_hover, theme.brand, theme.brand, on_brand)
        } else {
            (theme.background_secondary, theme.hover_bg, hsla(0., 0., 0., 0.), theme.border, theme.text_tertiary)
        };
        // `.emoji`: 1.25rem square with 0.125rem vertical margin.
        let glyph = match &pill.image_url {
            Some(url) => {
                let alt = pill.label.clone();
                img(url.clone())
                    .size(px(EMOJI_SIZE))
                    .object_fit(ObjectFit::Contain)
                    .with_fallback(move || div().text_xs().child(alt.clone()).into_any_element())
                    .into_any_element()
            }
            None => div()
                .size(px(EMOJI_SIZE))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(16.))
                .line_height(px(20.))
                .child(pill.label.clone())
                .into_any_element(),
        };
        let glyph = div().my(px(2.)).flex_none().child(glyph).flx("channel.message-reactions.message-reaction-item.emoji");
        // `.countWrapper`: 1em tall, 0.75rem min width, 0.375rem start margin.
        let count = div()
            .ml(px(6.))
            .min_w(px(12.))
            .h(px(16.))
            .flex()
            .justify_center()
            .text_size(px(16.))
            .line_height(px(16.))
            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
            .text_color(fg)
            .child(
                div()
                    .min_w(px(12.))
                    .h(px(16.))
                    .flex()
                    .justify_center()
                    .flx("channel.message-reactions.message-reaction-item.div")
                    .child(pill.count.to_string()),
            )
            .flx("channel.message-reactions.message-reaction-item.count-wrapper");
        let inner = div()
            .flex()
            .flex_row()
            .items_center()
            .px(px(6.))
            .py(px(2.))
            .child(glyph)
            .child(count)
            .flx("channel.message-reactions.message-reaction-item.reaction-inner");
        let button = div()
            .id(SharedString::from(format!("reaction-{message_key}-{ix}")))
            .debug_selector(|| format!("reaction-{message_key}-{ix}"))
            .rounded(px(8.))
            .border_1()
            .border_color(border)
            .bg(bg)
            .cursor_pointer()
            .hover(move |s| s.bg(bg_hover).border_color(border_hover))
            .child(inner)
            .on_click(move |_, window, cx| on_toggle(&emoji, window, cx))
            .flx("channel.message-reactions.message-reaction-item.reaction-button.click");
        div()
            .mr(px(4.))
            .rounded(px(8.))
            .child(button)
            .flx("channel.message-reactions.message-reaction-item.div--3")
    });
    // `.reactionPickerButton`: 0.25rem 0.375rem padding, 1.875rem tall
    // (`uniformRows`), tertiary text, hover fill.
    let add = div()
        .id(SharedString::from(format!("reaction-add-{message_key}")))
        .debug_selector(|| format!("reaction-add-{message_key}"))
        .flex()
        .items_center()
        .justify_center()
        .h(px(30.))
        .px(px(6.))
        .py(px(4.))
        .rounded(px(8.))
        .cursor_pointer()
        .hover(|s| s.bg(theme.hover_bg))
        .child(
            div().size(px(20.)).child(Icon::new(AppIcon::Smiley).with_size(px(20.)).text_color(theme.text_tertiary)).flx(
                "channel.message-reactions.smiley-icon--2",
            ),
        )
        .on_click(move |ev, window, cx| on_add(ev.position(), window, cx))
        .flx("channel.message-reactions.add-reaction-button");
    div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        // Shrink-to-fit like the official grid inside the message column.
        .self_start()
        // `.reactionsGrid` padding-top.
        .pt(px(4.))
        .children(pill_els)
        .child(add)
        .flx("channel.message-reactions.reactions-grid")
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
