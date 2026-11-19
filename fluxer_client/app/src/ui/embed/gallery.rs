//! Several images of one link (`AttachmentMosaic` for embeds,
//! simplified): square tiles in rows of 2 (for 2 or 4 images) or 3, within
//! the 400px embed media width.

use std::sync::Arc;

use fluxer_client_core::state::{EMBED_MAX_WIDTH, ValidMedia, embed_media_size};
use gpui_kit::{AnyElement, App, div, prelude::*, px};

use super::{EmbedCx, MediaView, media};
use crate::ui::media_viewer;

/// Space between tiles.
pub const GAP: f32 = 4.;

/// Columns and square tile side for `count` images; one image keeps its
/// own fitted size.
pub fn tile_side(count: usize) -> (usize, f32) {
    let columns = match count {
        0 | 1 => 1,
        2 | 4 => 2,
        _ => 3,
    };
    let width = EMBED_MAX_WIDTH as f32;
    (columns, ((width - GAP * (columns as f32 - 1.)) / columns as f32).floor())
}

/// The images as tiles; `make` builds one item at a size.
pub fn tiles(images: &[ValidMedia<'_>], make: fn(ValidMedia<'_>, f32, f32) -> MediaView) -> Vec<MediaView> {
    if let [only] = images {
        let (width, height) = embed_media_size(only.width, only.height, false);
        return vec![make(*only, width as f32, height as f32)];
    }
    let (_, side) = tile_side(images.len());
    images.iter().map(|image| make(*image, side, side)).collect()
}

pub fn render(items: &[MediaView], ecx: &EmbedCx, cx: &mut App) -> AnyElement {
    let (columns, side) = tile_side(items.len());
    let width = side * columns as f32 + GAP * (columns as f32 - 1.);
    let mut viewer_items = Vec::new();
    let mut indices = Vec::with_capacity(items.len());
    for item in items {
        if let Some(vi) = media_viewer::from_embed_media(item, ecx.scale) {
            indices.push(Some(viewer_items.len()));
            viewer_items.push(vi);
        } else {
            indices.push(None);
        }
    }
    let viewer_items = Arc::new(viewer_items);
    div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap(px(GAP))
        .w(px(width))
        .children(items.iter().enumerate().map(|(i, item)| {
            media::render_with_gallery(
                item,
                &ecx.id(&format!("g{i}")),
                ecx,
                indices[i].map(|idx| (viewer_items.clone(), idx)),
                cx,
            )
        }))
        .into_any_element()
}
