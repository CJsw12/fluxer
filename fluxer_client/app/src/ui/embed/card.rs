//! The rich embed card (`RichEmbed.tsx`, `EmbedParts.tsx`,
//! `ChannelEmbed.module.css`): 1px border, radius 8, a 4px colour bar,
//! padding 12 (14 at the bottom), parts 8px apart, max width 516 (432 with
//! full-width media), an 80px thumbnail on the right.

use fluxer_client_markdown::MessageMarkdown;
use gpui_kit::{AnyElement, App, FontWeight, ObjectFit, SharedString, StyledImage as _, div, img, prelude::*, px, rgb};

use super::{AuthorView, CardMedia, CardView, EmbedCx, EmbedText, FooterView, LinkText, gallery, media, open_url};
use crate::ui::message_row::markdown_style;

/// Author icon side; the footer icon is 20.
const AUTHOR_ICON: f32 = 24.;
const FOOTER_ICON: f32 = 20.;

fn text_element(text: &EmbedText, id: SharedString, ecx: &EmbedCx) -> AnyElement {
    match text {
        EmbedText::Plain(text) => div().child(text.clone()).into_any_element(),
        EmbedText::Markdown(document) => {
            MessageMarkdown::new(id, document.clone()).style(markdown_style(&ecx.theme)).into_any_element()
        }
    }
}

/// Text that opens `url` on click (no markdown inside links).
fn link(text: &LinkText, id: SharedString, ecx: &EmbedCx) -> AnyElement {
    let element = text_element(&text.text, id.clone(), ecx);
    match &text.url {
        Some(url) => {
            let url = url.clone();
            div()
                .id(id)
                .cursor_pointer()
                .hover(|s| s.underline())
                .child(element)
                .on_click(move |_, _, cx| open_url(&url, cx))
                .into_any_element()
        }
        None => element,
    }
}

/// A small round proxied icon at `side` css px (static webp at that size).
fn icon(url: &SharedString, side: f32, ecx: &EmbedCx, cx: &mut App) -> AnyElement {
    let device = (side * ecx.scale.max(1.)).round().max(1.) as u32;
    let src = fluxer_client_core::state::static_proxy_url(url, device, device);
    let cache = media::image_cache(cx);
    let placeholder = ecx.theme.image_placeholder;
    img(SharedString::from(src))
        .size(px(side))
        .flex_none()
        .rounded_full()
        .object_fit(ObjectFit::Cover)
        .image_cache(&cache)
        .with_loading(move || div().size(px(side)).rounded_full().bg(placeholder).into_any_element())
        .with_fallback(move || div().size(px(side)).into_any_element())
        .into_any_element()
}

fn author(author: &AuthorView, ecx: &EmbedCx, cx: &mut App) -> AnyElement {
    let name = LinkText { text: EmbedText::Plain(author.name.clone()), url: author.url.clone() };
    div()
        .debug_selector(|| "embed-author".to_owned())
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .min_w_0()
        .text_sm()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(ecx.theme.foreground)
        .children(author.icon.as_ref().map(|url| icon(url, AUTHOR_ICON, ecx, cx)))
        .child(div().min_w_0().truncate().child(link(&name, ecx.id("author"), ecx)))
        .into_any_element()
}

fn footer(footer: &FooterView, ecx: &EmbedCx, cx: &mut App) -> AnyElement {
    let theme = ecx.theme;
    let mut parts = div().flex().flex_row().items_center().min_w_0().gap_1();
    if let Some(url) = &footer.icon {
        parts = parts.child(div().mr_1().child(icon(url, FOOTER_ICON, ecx, cx)));
    }
    if let Some(text) = &footer.text {
        parts = parts.child(div().min_w_0().truncate().child(text.clone()));
    }
    if let Some(time) = &footer.time {
        if footer.text.is_some() {
            parts = parts.child(div().size(px(4.)).rounded_full().bg(theme.muted_foreground));
        }
        parts = parts.child(div().child(time.clone()));
    }
    parts.debug_selector(|| "embed-footer".to_owned()).text_xs().font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).into_any_element()
}

/// Rows of fields; the fields of a row share it equally (12-column grid).
fn fields(rows: &[Vec<super::FieldView>], ecx: &EmbedCx) -> Option<AnyElement> {
    if rows.is_empty() {
        return None;
    }
    let rows = rows.iter().enumerate().map(|(r, row)| {
        div().flex().flex_row().gap_2().children(row.iter().enumerate().map(|(c, field)| {
            div()
                .debug_selector(|| "embed-field".to_owned())
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(ecx.theme.foreground)
                        .child(text_element(&field.name, ecx.id(&format!("f{r}-{c}-n")), ecx)),
                )
                .child(text_element(&field.value, ecx.id(&format!("f{r}-{c}-v")), ecx))
        }))
    });
    Some(div().flex().flex_col().gap_2().mt_1().text_sm().children(rows).into_any_element())
}

/// The card. `hidden` (an unrevealed spoiler) draws the card text behind a
/// cover and leaves its media unloaded.
pub fn render(card: &CardView, hidden: bool, ecx: &EmbedCx, cx: &mut App) -> AnyElement {
    let theme = ecx.theme;
    let bar_color = card.color.map_or(theme.brand, |c| rgb(c).into());
    let mut content = div().flex().flex_col().gap_2().flex_1().min_w_0();
    if let Some(provider) = &card.provider {
        content = content.child(
            div()
                .debug_selector(|| "embed-provider".to_owned())
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.muted_foreground)
                .child(link(provider, ecx.id("provider"), ecx)),
        );
    }
    if let Some(a) = &card.author {
        content = content.child(author(a, ecx, cx));
    }
    if let Some(title) = &card.title {
        let color = if title.url.is_some() { theme.text_link } else { theme.foreground };
        content = content.child(
            div()
                .debug_selector(|| "embed-title".to_owned())
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color)
                .child(link(title, ecx.id("title"), ecx)),
        );
    }
    if let Some(description) = &card.description {
        content = content
            .child(
                div()
                    .debug_selector(|| "embed-description".to_owned())
                    .text_sm()
                    .text_color(theme.foreground)
                    .child(text_element(description, ecx.id("desc"), ecx)),
            );
    }
    content = content.children(fields(&card.fields, ecx));
    let media_box = |media_size: (f32, f32)| {
        div().w(px(media_size.0)).h(px(media_size.1)).rounded(px(4.)).bg(theme.image_placeholder).into_any_element()
    };
    let media = match &card.media {
        CardMedia::None => None,
        CardMedia::Single(m) if hidden => Some(media_box((m.width, m.height))),
        CardMedia::Single(m) => Some(media::render(m, &ecx.id("media"), ecx, cx)),
        CardMedia::Gallery(items) if hidden => Some(media_box((gallery::tile_side(items.len()).1, 80.))),
        CardMedia::Gallery(items) => Some(gallery::render(items, ecx, cx)),
    };
    let thumbnail = card.thumbnail.as_ref().map(|t| {
        let element = if hidden { media_box((t.width, t.height)) } else { media::render(t, &ecx.id("thumb"), ecx, cx) };
        div().ml_4().flex_none().child(element)
    });
    let body = div()
        .flex()
        .flex_col()
        .gap_2()
        .min_w_0()
        .pt(px(16.))
        .px(px(12.))
        .pb(px(14.))
        .child(div().flex().flex_row().items_start().child(content).children(thumbnail))
        .children(media.map(|m| div().mt_1().child(m)))
        .children(card.footer.as_ref().map(|f| footer(f, ecx, cx)));
    div()
        .id(ecx.id("card"))
        .debug_selector(|| "embed-card".to_owned())
        .relative()
        .flex()
        .flex_row()
        .max_w(px(card.max_width))
        .min_w_0()
        .rounded(px(8.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .overflow_hidden()
        .child(div().debug_selector(|| "embed-color-bar".to_owned()).w(px(4.)).flex_none().bg(bar_color))
        .child(body)
        .when(hidden, |d| {
            d.child(
                div()
                    .id(ecx.id("card-spoiler"))
                    .debug_selector(|| "embed-cover".to_owned())
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(theme.background_tertiary)
                    .text_color(theme.muted_foreground)
                    .text_xs()
                    .font_weight(FontWeight::BOLD)
                    .cursor_pointer()
                    .child("SPOILER")
                    .on_click(ecx.reveal_handler()),
            )
        })
        .into_any_element()
}
