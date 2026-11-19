//! Embed media (`EmbedImage`, `EmbedGif`, `EmbedGifv`, `EmbedYouTube`,
//! `EmbedVideo`, `EmbedAudio`, simplified) and the image cache they load
//! through.
//!
//! Memory: every image is a proxy variant at the drawn size in device
//! pixels (never the original). Animated variants decode to all frames, so
//! [`EmbedImageCache`] drops decoded images that were not drawn for a while
//! once the total passes a budget. Only rows in (or near) the viewport are
//! drawn by the virtual list, so only those load or animate.
//!
//! Animation: GPUI's `img()` advances frames and requests the next
//! animation frame only for an element with an id (its frame index lives in
//! element state), so every media image gets one ([`media_img`]). It stops
//! when the row leaves the list or the window is inactive.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use fluxer_client_core::state::{animated_proxy_url, static_proxy_url};
use gpui_kit::{
    AnyElement, App, Entity, FontWeight, Global, ImageCache, ImageCacheError, ImageCacheItem, Img, ObjectFit,
    RenderImage, Resource, SharedString, Stateful, StyledImage as _, Window, div, hsla, img, prelude::*, px, white,
};

use super::{EmbedCx, MediaKind, MediaView, open_url};
use crate::ui::l10n;
use crate::ui::media_viewer::{self, MediaViewerItem};

/// Decoded embed images kept beyond this are dropped, least recently drawn
/// first (frames x width x height x 4 bytes).
const BUDGET_BYTES: usize = 160 * 1024 * 1024;
/// An image drawn this recently is never dropped (it is on screen).
const KEEP_DRAWN: Duration = Duration::from_secs(2);
/// Over this many entries, failed and still-loading entries not drawn
/// recently are dropped (they hold no pixels, so the byte budget never
/// removes them).
pub(super) const MAX_ENTRIES: usize = 2048;

struct CacheEntry {
    item: ImageCacheItem,
    last_used: Instant,
    /// Decoded size; 0 while loading or after a failed load.
    bytes: usize,
}

/// An [`ImageCache`] with a byte budget. Failed loads stay cached (they
/// hold no pixels), so a gifv whose animated variant is not an image is
/// not fetched again while it is drawn; past [`MAX_ENTRIES`] the ones not
/// drawn recently are forgotten.
#[derive(Default)]
pub struct EmbedImageCache {
    entries: HashMap<u64, CacheEntry>,
    bytes: usize,
}

fn image_bytes(image: &RenderImage) -> usize {
    (0..image.frame_count())
        .map(|i| {
            let size = image.size(i);
            usize::try_from(size.width.0).unwrap_or(0) * usize::try_from(size.height.0).unwrap_or(0) * 4
        })
        .sum()
}

impl EmbedImageCache {
    /// Drops images not drawn within [`KEEP_DRAWN`], oldest first, until
    /// the total fits the budget.
    fn evict(&mut self, now: Instant, window: &mut Window, cx: &mut App) {
        let mut stale: Vec<(Instant, u64)> = self
            .entries
            .iter()
            .filter(|(_, e)| e.bytes > 0 && now.duration_since(e.last_used) > KEEP_DRAWN)
            .map(|(key, e)| (e.last_used, *key))
            .collect();
        stale.sort_unstable();
        for (_, key) in stale {
            if self.bytes <= BUDGET_BYTES {
                break;
            }
            if let Some(entry) = self.entries.remove(&key) {
                self.bytes = self.bytes.saturating_sub(entry.bytes);
                if let Some(Ok(image)) = entry.item.get() {
                    cx.drop_image(image, Some(window));
                }
            }
        }
    }

    /// Drops pixel-less entries (failed or still loading) not drawn within
    /// [`KEEP_DRAWN`] once there are more than [`MAX_ENTRIES`].
    pub(super) fn prune_empty(&mut self, now: Instant) {
        if self.entries.len() <= MAX_ENTRIES {
            return;
        }
        self.entries.retain(|_, e| e.bytes > 0 || now.duration_since(e.last_used) <= KEEP_DRAWN);
    }

    /// Entries held (tests).
    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    /// An entry that has not loaded, last drawn at `last_used` (tests).
    #[cfg(test)]
    pub(super) fn insert_loading(&mut self, resource: &Resource, last_used: Instant, cx: &mut App) {
        let item = ImageCacheItem::new(resource, cx);
        self.entries.insert(gpui_kit::hash(resource), CacheEntry { item, last_used, bytes: 0 });
    }
}

impl ImageCache for EmbedImageCache {
    fn load(
        &mut self,
        resource: &Resource,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        let key = gpui_kit::hash(resource);
        let now = Instant::now();
        if !self.entries.contains_key(&key) {
            self.prune_empty(now);
        }
        let entry = self
            .entries
            .entry(key)
            .or_insert_with(|| CacheEntry { item: ImageCacheItem::new(resource, cx), last_used: now, bytes: 0 });
        entry.last_used = now;
        let result = entry.item.use_image(window);
        if entry.bytes == 0
            && let Some(Ok(image)) = &result
        {
            entry.bytes = image_bytes(image).max(1);
            self.bytes += entry.bytes;
            if self.bytes > BUDGET_BYTES {
                self.evict(now, window, cx);
            }
        }
        result
    }
}

struct EmbedImages(Entity<EmbedImageCache>);

impl Global for EmbedImages {}

/// The app-wide embed image cache.
pub fn image_cache(cx: &mut App) -> Entity<EmbedImageCache> {
    if let Some(images) = cx.try_global::<EmbedImages>() {
        return images.0.clone();
    }
    let cache = cx.new(|_| EmbedImageCache::default());
    cx.set_global(EmbedImages(cache.clone()));
    cache
}

/// Device pixels for a css length (`toEmbedMediaPixels`).
fn device(css: f32, scale: f32) -> u32 {
    let scale = if scale.is_finite() && scale > 0. { scale } else { 1. };
    (css * scale).round().max(1.) as u32
}

/// The proxy URL drawn for `media` at `scale`: animated variant for
/// animated kinds, else the still webp.
pub fn variant_url(media: &MediaView, scale: f32) -> Option<String> {
    let (w, h) = (device(media.width, scale), device(media.height, scale));
    match media.kind {
        MediaKind::Image => media.source.as_deref().map(|s| static_proxy_url(s, w, h)),
        MediaKind::Animated | MediaKind::Gifv => media.source.as_deref().map(|s| animated_proxy_url(s, w, h)),
        MediaKind::Video => media.poster.as_deref().map(|s| static_proxy_url(s, w, h)),
        MediaKind::Audio => None,
    }
}

/// The still drawn while an animated variant loads or when it fails.
pub fn still_url(media: &MediaView, scale: f32) -> Option<String> {
    let (w, h) = (device(media.width, scale), device(media.height, scale));
    let still = match media.kind {
        MediaKind::Gifv | MediaKind::Video => media.poster.as_deref(),
        MediaKind::Animated => media.source.as_deref(),
        MediaKind::Image | MediaKind::Audio => None,
    };
    still.map(|s| static_proxy_url(s, w, h))
}

/// A media image through `cache`, with the element id GPUI needs to
/// animate a multi-frame image (unique within the parent's id).
pub(crate) fn media_img(
    id: &'static str,
    url: impl Into<SharedString>,
    cache: &Entity<EmbedImageCache>,
) -> Stateful<Img> {
    img(url.into()).image_cache(cache).id(id).size_full().object_fit(ObjectFit::Cover)
}

fn overlay_bg() -> gpui_kit::Hsla {
    hsla(0., 0., 0., 0.6)
}

/// "GIF" in the top left corner (`GifIndicator`).
fn gif_badge() -> AnyElement {
    div()
        .absolute()
        .top(px(6.))
        .left(px(6.))
        .px(px(4.))
        .rounded(px(4.))
        .bg(overlay_bg())
        .text_color(white())
        .text_xs()
        .font_weight(FontWeight::BOLD)
        .child("GIF")
        .into_any_element()
}

/// A round play button in the middle (`EmbedYouTube` thumbnail).
fn play_button() -> AnyElement {
    div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .size(px(48.))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(overlay_bg())
                .text_color(white())
                .text_size(px(20.))
                .child("▶"),
        )
        .into_any_element()
}

/// A block of `width`x`height` with a centred label; `on_click` reveals.
fn cover(id: SharedString, width: f32, height: f32, label: &str, ecx: &EmbedCx) -> AnyElement {
    div()
        .id(id)
        .debug_selector(|| "embed-cover".to_owned())
        .w(px(width))
        .h(px(height))
        .flex_none()
        .rounded(px(4.))
        .flex()
        .items_center()
        .justify_center()
        .bg(ecx.theme.background_tertiary)
        .text_color(ecx.theme.muted_foreground)
        .text_xs()
        .font_weight(FontWeight::BOLD)
        .cursor_pointer()
        .child(label.to_owned())
        .on_click(ecx.reveal_handler())
        .into_any_element()
}

/// A spoilered media-only embed: nothing loads until it is revealed.
pub fn spoiler_cover(width: f32, height: f32, ecx: &EmbedCx) -> AnyElement {
    cover(ecx.id("spoiler"), width, height, &l10n::tr("Spoiler"), ecx)
}

/// One media item. Mature media shows a cover (and loads nothing) until
/// revealed; image/GIF clicks open the media viewer, video/audio open externally.
pub fn render(media: &MediaView, id: &SharedString, ecx: &EmbedCx, cx: &mut App) -> AnyElement {
    render_with_gallery(media, id, ecx, None, cx)
}

pub fn render_with_gallery(
    media: &MediaView,
    id: &SharedString,
    ecx: &EmbedCx,
    gallery: Option<(Arc<Vec<MediaViewerItem>>, usize)>,
    cx: &mut App,
) -> AnyElement {
    if media.kind == MediaKind::Audio {
        return audio_row(media, id.clone(), ecx);
    }
    if media.explicit && !ecx.revealed(cx) {
        let label = format!("NSFW · {}", l10n::tr("Click to reveal"));
        return cover(id.clone(), media.width, media.height, &label, ecx);
    }
    let cache = image_cache(cx);
    let placeholder = ecx.theme.image_placeholder;
    let blank = move || div().size_full().bg(placeholder).into_any_element();
    let still = still_url(media, ecx.scale);
    let still_img = {
        let (cache, still) = (cache.clone(), still.clone());
        move || match still.clone() {
            Some(url) => media_img("still", url, &cache).with_loading(blank).with_fallback(blank).into_any_element(),
            None => blank(),
        }
    };
    let image = variant_url(media, ecx.scale).map(|url| {
        let image = media_img("media", url, &cache);
        match media.kind {
            // The gifv still shows while the animated variant loads.
            MediaKind::Gifv => image.with_loading(still_img.clone()).with_fallback(still_img),
            MediaKind::Animated => image.with_loading(blank).with_fallback(still_img),
            _ => image.with_loading(blank).with_fallback(blank),
        }
    });
    let viewer_pair = gallery.or_else(|| media_viewer::from_embed_media(media, ecx.scale).map(|item| (Arc::new(vec![item]), 0)));
    let open = media.open_url.clone();
    let box_el = div()
        .id(id.clone())
        .debug_selector(|| "embed-media".to_owned())
        .relative()
        .w(px(media.width))
        .h(px(media.height))
        .flex_none()
        .rounded(px(4.))
        .overflow_hidden()
        .bg(if media.kind == MediaKind::Video { gpui_kit::black() } else { placeholder })
        .children(image)
        .when(media.kind == MediaKind::Gifv, |d| d.child(gif_badge()))
        .when(media.kind == MediaKind::Video, |d| d.child(play_button()));

    if let Some((items, index)) = viewer_pair {
        let view = ecx.view.clone();
        box_el
            .cursor_pointer()
            .on_click(move |_, window, cx| {
                let items = (*items).clone();
                let _ = view.update(cx, |v, cx| v.open_media_viewer(items, index, window, cx));
            })
            .into_any_element()
    } else if let Some(url) = open {
        let view = ecx.view.clone();
        box_el
            .cursor_pointer()
            .on_click(move |_, window, cx| open_url(&url, &view, window, cx))
            .into_any_element()
    } else {
        box_el.into_any_element()
    }
}

/// `EmbedAudio` without a player: the title as a link to the file.
fn audio_row(media: &MediaView, id: SharedString, ecx: &EmbedCx) -> AnyElement {
    let theme = ecx.theme;
    let label = media.label.clone().unwrap_or_else(|| l10n::tr("Audio").into());
    let open = media.open_url.clone();
    let view = ecx.view.clone();
    div()
        .id(id)
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .w(px(media.width))
        .h(px(media.height))
        .px_3()
        .rounded(px(8.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.background_secondary)
        .child(div().text_color(theme.muted_foreground).child("♪"))
        .child(div().flex_1().min_w_0().truncate().text_color(theme.text_link).child(label))
        .when_some(open, |d, url| {
            d.cursor_pointer()
                .on_click(move |_, window, cx| open_url(&url, &view, window, cx))
        })
        .into_any_element()
}
