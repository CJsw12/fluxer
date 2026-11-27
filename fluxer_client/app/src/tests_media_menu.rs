//! Tests for the media context menu `Copy media` action (PNG/JPEG only).

use std::io::Cursor;

use gpui_kit::{ClipboardEntry, ClipboardItem, ImageFormat, TestAppContext};

use crate::ui::media_context_menu::{
    CopyMediaMode, MediaContextMenuTarget, MediaMenuAction, MediaMenuKind, apply_copy_result, copy_candidate_urls,
    copy_media_mode, encode_clipboard_png, menu_groups,
};

const ALL_KINDS: [MediaMenuKind; 6] = [
    MediaMenuKind::Image,
    MediaMenuKind::Gif,
    MediaMenuKind::Gifv,
    MediaMenuKind::Video,
    MediaMenuKind::Audio,
    MediaMenuKind::File,
];

fn sample(format: image::ImageFormat) -> Vec<u8> {
    let img = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(4, 3, image::Rgb([200, 40, 10])));
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), format).expect("encode sample");
    out
}

#[test]
fn only_still_images_have_media_copy_implemented() {
    for kind in ALL_KINDS {
        let expected = if kind == MediaMenuKind::Image {
            CopyMediaMode::Image
        } else {
            CopyMediaMode::Unsupported
        };
        assert_eq!(copy_media_mode(kind), expected, "{kind:?}");
    }
}

#[test]
fn menu_groups_keep_official_layout_for_every_kind() {
    for kind in ALL_KINDS {
        let groups = menu_groups(kind);
        assert_eq!(groups.len(), 2, "{kind:?}");
        assert_eq!(groups[0], vec![MediaMenuAction::AddToFavorites]);
        assert_eq!(groups[1][0], MediaMenuAction::CopyMedia);
        assert_eq!(groups[1][1], MediaMenuAction::DownloadMedia);
        assert_eq!(
            groups[1].contains(&MediaMenuAction::CopyLink),
            kind != MediaMenuKind::File,
            "{kind:?}"
        );
        assert_eq!(*groups[1].last().expect("open link"), MediaMenuAction::OpenLink);
    }
}

#[test]
fn candidate_urls_prefer_proxy_and_dedupe() {
    let t = MediaContextMenuTarget::new(MediaMenuKind::Image, "https://cdn.example/a.png")
        .with_proxy(Some("https://media.example/a.png".into()));
    assert_eq!(
        copy_candidate_urls(&t),
        ["https://media.example/a.png", "https://cdn.example/a.png"]
    );
    let same = MediaContextMenuTarget::new(MediaMenuKind::Image, "https://cdn.example/a.png");
    assert_eq!(copy_candidate_urls(&same), ["https://cdn.example/a.png"]);
    assert!(copy_candidate_urls(&MediaContextMenuTarget::new(MediaMenuKind::Image, "")).is_empty());
}

#[test]
fn png_passes_through_and_jpeg_becomes_png() {
    let png = sample(image::ImageFormat::Png);
    assert_eq!(encode_clipboard_png(&png).expect("png"), png);

    let jpeg = sample(image::ImageFormat::Jpeg);
    let converted = encode_clipboard_png(&jpeg).expect("jpeg");
    assert_eq!(
        image::guess_format(&converted).expect("format"),
        image::ImageFormat::Png
    );
}

#[test]
fn gif_webp_and_garbage_are_rejected() {
    assert!(encode_clipboard_png(b"GIF89a\0\0\0\0").is_err());
    assert!(encode_clipboard_png(b"RIFF\x00\x00\x00\x00WEBPVP8 ").is_err());
    assert!(encode_clipboard_png(b"not an image").is_err());
    let mut truncated = sample(image::ImageFormat::Png);
    truncated.truncate(20);
    assert!(encode_clipboard_png(&truncated).is_err());
}

#[gpui_kit::test]
fn failure_leaves_clipboard_unchanged_and_success_writes_png(cx: &mut TestAppContext) {
    cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string("keep me".to_owned())));

    let wrote = cx.update(|cx| apply_copy_result(Err(anyhow::anyhow!("network down")), cx));
    assert!(!wrote);
    let text = cx.update(|cx| cx.read_from_clipboard().and_then(|item| item.text()));
    assert_eq!(text.as_deref(), Some("keep me"));

    let png = sample(image::ImageFormat::Png);
    let wrote = cx.update(|cx| apply_copy_result(Ok(png.clone()), cx));
    assert!(wrote);
    let item = cx.update(|cx| cx.read_from_clipboard()).expect("clipboard item");
    match &item.entries[0] {
        ClipboardEntry::Image(image) => {
            assert_eq!(image.format, ImageFormat::Png);
            assert_eq!(image.bytes, png);
        }
        other => panic!("expected an image entry, got {other:?}"),
    }
}
