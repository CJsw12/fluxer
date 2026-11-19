//! Full-window image/GIF lightbox (`MediaViewerModal`): gallery navigation
//! (Left/Right arrows & buttons), click-to-zoom (+/-), copy link, and open
//! externally through the trusted-domain check.

use fluxer_client_core::state::{animated_proxy_url, static_proxy_url};
use gpui_kit::component::Disableable as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Escape;
use gpui_kit::{
    AnyElement, ClipboardItem, Context, FocusHandle, FontWeight, KeyDownEvent, MouseButton, ObjectFit, SharedString,
    StyledImage as _, Window, div, img, prelude::*, px,
};

use super::embed::{self, MediaKind, MediaView};
use crate::client_view::ClientView;
use crate::theme::Theme;
use crate::ui::l10n;
use crate::ui::overlay::Overlay;
use crate::view_model::ImagePreview;

pub const CONTEXT: &str = "MediaViewerModal";
const MAX_STAGE_WIDTH: f32 = 960.;
const MAX_STAGE_HEIGHT: f32 = 640.;
const MAX_STILL_DEVICE_WIDTH: f32 = 1920.;
const MAX_STILL_DEVICE_HEIGHT: f32 = 1280.;
const MAX_ANIMATED_DEVICE_WIDTH: f32 = 960.;
const MAX_ANIMATED_DEVICE_HEIGHT: f32 = 720.;

#[derive(Debug, Clone, PartialEq)]
pub struct MediaViewerItem {
    /// Original URL copied to clipboard or opened externally.
    pub url: SharedString,
    /// High-resolution proxy variant rendered inside the viewer.
    pub preview_url: SharedString,
    /// Fallback still URL if animated variant fails to decode.
    pub fallback_url: Option<SharedString>,
    pub width: f32,
    pub height: f32,
    pub animated: bool,
    pub label: Option<SharedString>,
}

pub struct MediaViewerModal {
    pub items: Vec<MediaViewerItem>,
    pub index: usize,
    pub zoomed: bool,
    pub focus: FocusHandle,
}

fn fit_viewer_sizes(width: f32, height: f32, scale: f32, animated: bool) -> (f32, f32, u32, u32) {
    let w = if width.is_finite() && width > 0. { width } else { 800. };
    let h = if height.is_finite() && height > 0. { height } else { 600. };
    let safe_scale = if scale.is_finite() && scale > 0. { scale } else { 1. };
    let css_scale = (MAX_STAGE_WIDTH / w).min(MAX_STAGE_HEIGHT / h).min(2.0);
    let css_w = (w * css_scale).clamp(1., MAX_STAGE_WIDTH);
    let css_h = (h * css_scale).clamp(1., MAX_STAGE_HEIGHT);
    let (max_dev_w, max_dev_h) = if animated {
        (MAX_ANIMATED_DEVICE_WIDTH, MAX_ANIMATED_DEVICE_HEIGHT)
    } else {
        (MAX_STILL_DEVICE_WIDTH, MAX_STILL_DEVICE_HEIGHT)
    };
    let dev_scale = (max_dev_w / css_w).min(max_dev_h / css_h).min(safe_scale);
    let dev_w = (css_w * dev_scale).round().clamp(1., max_dev_w) as u32;
    let dev_h = (css_h * dev_scale).round().clamp(1., max_dev_h) as u32;
    (css_w, css_h, dev_w, dev_h)
}

pub fn from_attachment(image: &ImagePreview, scale: f32) -> MediaViewerItem {
    let (css_w, css_h, dev_w, dev_h) = fit_viewer_sizes(image.width, image.height, scale, image.animated);
    let preview_url: SharedString = match &image.proxy_url {
        Some(proxy) if image.animated => animated_proxy_url(proxy, dev_w, dev_h).into(),
        Some(proxy) => static_proxy_url(proxy, dev_w, dev_h).into(),
        None => image.url.clone(),
    };
    let fallback_url = image
        .proxy_url
        .as_deref()
        .map(|proxy| SharedString::from(static_proxy_url(proxy, dev_w, dev_h)))
        .or_else(|| Some(image.url.clone()));
    MediaViewerItem {
        url: image.url.clone(),
        preview_url,
        fallback_url,
        width: css_w,
        height: css_h,
        animated: image.animated,
        label: None,
    }
}

pub fn from_embed_media(media: &MediaView, scale: f32) -> Option<MediaViewerItem> {
    if !matches!(media.kind, MediaKind::Image | MediaKind::Animated | MediaKind::Gifv) {
        return None;
    }
    let url = media
        .open_url
        .clone()
        .or_else(|| media.source.clone())
        .or_else(|| media.poster.clone())?;
    let animated = matches!(media.kind, MediaKind::Animated | MediaKind::Gifv);
    let (css_w, css_h, dev_w, dev_h) = fit_viewer_sizes(media.width, media.height, scale, animated);
    let preview_url: SharedString = match &media.source {
        Some(src) if animated => animated_proxy_url(src, dev_w, dev_h).into(),
        Some(src) => static_proxy_url(src, dev_w, dev_h).into(),
        None => media.poster.clone().unwrap_or_else(|| url.clone()),
    };
    let fallback_url = media.poster.clone().or_else(|| media.open_url.clone());
    Some(MediaViewerItem {
        url,
        preview_url,
        fallback_url,
        width: css_w,
        height: css_h,
        animated,
        label: media.label.clone(),
    })
}

impl ClientView {
    pub(crate) fn open_media_viewer(
        &mut self,
        items: Vec<MediaViewerItem>,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if items.is_empty() {
            return;
        }
        let clamped = index.min(items.len() - 1);
        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        let focus = cx.focus_handle();
        self.overlay = Overlay::MediaViewer(MediaViewerModal {
            items,
            index: clamped,
            zoomed: false,
            focus: focus.clone(),
        });
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(crate) fn media_viewer_step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Overlay::MediaViewer(modal) = &mut self.overlay else { return };
        if modal.items.is_empty() {
            return;
        }
        let next = (modal.index as isize + delta).clamp(0, (modal.items.len() - 1) as isize) as usize;
        if next != modal.index {
            modal.index = next;
            modal.zoomed = false;
            cx.notify();
        }
    }

    pub(crate) fn media_viewer_set_zoom(&mut self, zoomed: bool, cx: &mut Context<Self>) {
        if let Overlay::MediaViewer(modal) = &mut self.overlay
            && modal.zoomed != zoomed
        {
            modal.zoomed = zoomed;
            cx.notify();
        }
    }

    pub(crate) fn media_viewer_toggle_zoom(&mut self, cx: &mut Context<Self>) {
        if let Overlay::MediaViewer(modal) = &mut self.overlay {
            modal.zoomed = !modal.zoomed;
            cx.notify();
        }
    }
}

pub fn render(modal: &MediaViewerModal, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let Some(item) = modal.items.get(modal.index).cloned() else {
        return div().into_any_element();
    };
    let count = modal.items.len();
    let has_prev = modal.index > 0;
    let has_next = modal.index + 1 < count;
    let zoomed = modal.zoomed;
    let copy_url = item.url.clone();
    let open_url = item.url.clone();

    let cache = embed::image_cache(cx);
    let fallback_url = item.fallback_url.clone().unwrap_or_else(|| item.url.clone());
    let placeholder_bg = theme.image_placeholder;
    let muted_fg = theme.muted_foreground;

    let mut image_el = img(item.preview_url.clone())
        .image_cache(&cache)
        .id(SharedString::from(format!("media-viewer-img-{}", modal.index)))
        .object_fit(ObjectFit::Contain)
        .with_loading(move || div().w(px(320.)).h(px(240.)).bg(placeholder_bg).into_any_element())
        .with_fallback(move || {
            div()
                .p_4()
                .text_xs()
                .text_color(muted_fg)
                .child(fallback_url.clone())
                .into_any_element()
        });

    if zoomed {
        image_el = image_el.w(px(item.width * 2.)).h(px(item.height * 2.));
    } else {
        image_el = image_el.w(px(item.width)).h(px(item.height)).max_w(px(MAX_STAGE_WIDTH)).max_h(px(MAX_STAGE_HEIGHT));
    }

    let mut toolbar = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px_3()
        .py_2()
        .rounded(px(8.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.background_secondary)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());

    if count > 1 {
        toolbar = toolbar
            .child(
                div().debug_selector(|| "media-viewer-prev".to_owned()).child(
                    Button::new("media-viewer-prev")
                        .ghost()
                        .small()
                        .disabled(!has_prev)
                        .label(l10n::tr("Previous"))
                        .on_click(cx.listener(|this, _, _, cx| this.media_viewer_step(-1, cx))),
                ),
            )
            .child(
                div()
                    .px_1()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(format!("{} / {count}", modal.index + 1)),
            )
            .child(
                div().debug_selector(|| "media-viewer-next".to_owned()).child(
                    Button::new("media-viewer-next")
                        .ghost()
                        .small()
                        .disabled(!has_next)
                        .label(l10n::tr("Next"))
                        .on_click(cx.listener(|this, _, _, cx| this.media_viewer_step(1, cx))),
                ),
            );
    }

    if let Some(label) = item.label.filter(|l| !l.trim().is_empty()) {
        toolbar = toolbar.child(div().text_xs().text_color(muted_fg).truncate().max_w(px(240.)).child(label));
    }

    toolbar = toolbar
        .child(
            div().debug_selector(|| "media-viewer-zoom".to_owned()).child(
                Button::new("media-viewer-zoom")
                    .ghost()
                    .small()
                    .label(if zoomed { l10n::tr("Zoom out") } else { l10n::tr("Zoom in") })
                    .on_click(cx.listener(|this, _, _, cx| this.media_viewer_toggle_zoom(cx))),
            ),
        )
        .child(
            div().debug_selector(|| "media-viewer-copy".to_owned()).child(
                Button::new("media-viewer-copy")
                    .ghost()
                    .small()
                    .label(l10n::tr("Copy link"))
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copy_url.to_string()));
                    }),
            ),
        )
        .child(
            div().debug_selector(|| "media-viewer-open".to_owned()).child(
                Button::new("media-viewer-open")
                    .primary()
                    .small()
                    .label(l10n::tr("Open in browser"))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_external_url(&open_url, window, cx);
                    })),
            ),
        )
        .child(
            div().debug_selector(|| "media-viewer-close".to_owned()).child(
                Button::new("media-viewer-close")
                    .ghost()
                    .small()
                    .label(l10n::tr("Close"))
                    .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
            ),
        );

    let stage = div()
        .id("media-viewer-stage")
        .debug_selector(|| "media-viewer-stage".to_owned())
        .max_w_full()
        .max_h(px(680.))
        .overflow_scroll()
        .rounded(px(8.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .p_2()
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(|this, _, _, cx| this.media_viewer_toggle_zoom(cx)))
        .child(image_el);

    div()
        .id("media-viewer-backdrop")
        .key_context(CONTEXT)
        .track_focus(&modal.focus)
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_3()
        .p_4()
        .bg(gpui_kit::hsla(0., 0., 0., 0.78))
        .occlude()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| this.close_overlay(window, cx)),
        )
        .capture_action(cx.listener(|this, _: &Escape, window, cx| {
            cx.stop_propagation();
            this.close_overlay(window, cx);
        }))
        .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
            let m = ev.keystroke.modifiers;
            if m.control || m.alt || m.platform {
                return;
            }
            match ev.keystroke.key.as_str() {
                "left" => {
                    cx.stop_propagation();
                    this.media_viewer_step(-1, cx);
                }
                "right" => {
                    cx.stop_propagation();
                    this.media_viewer_step(1, cx);
                }
                "+" | "=" => {
                    cx.stop_propagation();
                    this.media_viewer_set_zoom(true, cx);
                }
                "-" | "0" => {
                    cx.stop_propagation();
                    this.media_viewer_set_zoom(false, cx);
                }
                _ => {}
            }
        }))
        .child(stage)
        .child(toolbar)
        .into_any_element()
}
