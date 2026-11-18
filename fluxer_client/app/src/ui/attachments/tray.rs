//! The strip of queued files above the composer input: a preview (image
//! thumbnail, or a file glyph), the name and size, a spoiler toggle and a
//! remove button per file (`ChannelAttachmentArea.tsx`, simplified).

use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::{AnyElement, Context, ObjectFit, StyledImage as _, div, img, prelude::*, px};

use super::{PendingAttachment, Preview, format_size};
use crate::client_view::ClientView;
use crate::theme::Theme;

const CHIP_WIDTH: f32 = 148.;
const THUMB_HEIGHT: f32 = 84.;

/// `None` when nothing is queued.
pub fn render(files: &[PendingAttachment], theme: &Theme, cx: &mut Context<ClientView>) -> Option<AnyElement> {
    if files.is_empty() {
        return None;
    }
    let chips: Vec<AnyElement> = files.iter().map(|file| chip(file, theme, cx)).collect();
    Some(
        div()
            .id("attachment-tray")
            .flex()
            .flex_row()
            .gap_2()
            .p_2()
            .rounded_t(px(8.))
            .bg(theme.muted)
            .overflow_x_scroll()
            .children(chips)
            .into_any_element(),
    )
}

fn chip(file: &PendingAttachment, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let id = file.id;
    let placeholder = theme.image_placeholder;
    let thumb = match &file.file.preview {
        Preview::Path(path) => Some(img(path.clone())),
        Preview::Image(image) => Some(img(image.clone())),
        Preview::None => None,
    };
    let thumb = match thumb {
        Some(image) => image
            .size_full()
            .object_fit(ObjectFit::Cover)
            .with_loading(move || div().size_full().bg(placeholder).into_any_element())
            .with_fallback(move || div().size_full().bg(placeholder).into_any_element())
            .into_any_element(),
        None => div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(file_glyph(&file.file.name))
            .into_any_element(),
    };
    let spoiler_cover = file.spoiler.then(|| {
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.background)
            .opacity(0.9)
            .text_xs()
            .font_weight(gpui_kit::FontWeight::BOLD)
            .text_color(theme.foreground)
            .child("SPOILER")
    });
    let spoiler = Button::new(("attachment-spoiler", id))
        .ghost()
        .xsmall()
        .icon(if file.spoiler { IconName::EyeOff } else { IconName::Eye })
        .tooltip(if file.spoiler { "Remove spoiler" } else { "Mark as spoiler" })
        .on_click(cx.listener(move |view, _, _, cx| view.toggle_attachment_spoiler(id, cx)));
    let remove = Button::new(("attachment-remove", id))
        .ghost()
        .xsmall()
        .icon(IconName::Close)
        .tooltip("Remove attachment")
        .on_click(cx.listener(move |view, _, _, cx| view.remove_attachment(id, cx)));
    div()
        .flex()
        .flex_col()
        .flex_none()
        .w(px(CHIP_WIDTH))
        .p_1()
        .gap_1()
        .rounded(px(6.))
        .bg(theme.background)
        .child(
            div()
                .relative()
                .w_full()
                .h(px(THUMB_HEIGHT))
                .rounded(px(4.))
                .overflow_hidden()
                .child(thumb)
                .children(spoiler_cover),
        )
        .child(div().text_xs().truncate().child(file.file.name.clone()))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .child(div().flex_1().text_xs().text_color(theme.muted_foreground).child(format_size(file.file.size)))
                .child(spoiler)
                .child(remove),
        )
        .into_any_element()
}

/// Upper-case extension ("PDF"), or "FILE".
fn file_glyph(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_uppercase())
        .filter(|ext| !ext.is_empty() && ext.len() <= 5)
        .unwrap_or_else(|| "FILE".to_owned())
}
