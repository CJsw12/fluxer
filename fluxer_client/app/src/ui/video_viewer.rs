//! Live remote video stream viewer panel (`ScreenShare` / `Camera`).
//! Converts decoded RGBA video frames into GPUI `RenderImage`s (BGRA) and
//! renders either the active frame or a waiting placeholder with a Stop Watching
//! control.

use std::sync::Arc;

use fluxer_client_core::voice::DecodedVideoFrame;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::{
    Context, FontWeight, ImageSource, ObjectFit, RenderImage, Role, SharedString, StyledImage as _, div, img,
    prelude::*, px,
};

use super::l10n;
use crate::backend::voice::{TrackSubscriptionKey, VoiceTrackSource};
use crate::client_view::ClientView;
use crate::theme::Theme;

/// Converts a decoded RGBA `DecodedVideoFrame` into a GPUI `RenderImage` (BGRA).
/// Returns `None` without panicking if dimensions are zero or the buffer length
/// does not match `width * height * 4`.
pub(crate) fn decoded_frame_to_render_image(frame: &DecodedVideoFrame) -> Option<Arc<RenderImage>> {
    if frame.width == 0 || frame.height == 0 {
        return None;
    }
    let expected_len = (frame.width as usize)
        .checked_mul(frame.height as usize)?
        .checked_mul(4)?;
    if frame.data.len() != expected_len {
        return None;
    }

    let mut bgra = frame.data.clone();
    for pixel in bgra.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }

    let buf = image::RgbaImage::from_raw(frame.width, frame.height, bgra)?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buf)])))
}

pub(crate) fn render(
    track: &TrackSubscriptionKey,
    participant_label: SharedString,
    frame_image: Option<(u32, u32, Arc<RenderImage>)>,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> impl IntoElement {
    render_inner(track, participant_label, frame_image, false, theme, cx)
}

/// Expanded video viewer embedded inside `ui::voice_stage`.
pub(crate) fn render_stage(
    track: &TrackSubscriptionKey,
    participant_label: SharedString,
    frame_image: Option<(u32, u32, Arc<RenderImage>)>,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> impl IntoElement {
    render_inner(track, participant_label, frame_image, true, theme, cx)
}

/// Resolves a LiveKit participant identity (e.g. `"201:session"`) to the
/// member's display name when it maps to a known Fluxer `Snowflake`.
pub(crate) fn participant_display_name(view: &ClientView, track: &TrackSubscriptionKey) -> SharedString {
    if let Some(id_part) = track.participant_identity.split(':').next()
        && let Ok(uid) = id_part.parse::<u64>()
    {
        return view.voice_member_name(view.voice.guild_id, fluxer_client_core::state::Snowflake(uid));
    }
    track.participant_identity.clone().into()
}

fn render_inner(
    track: &TrackSubscriptionKey,
    participant_label: SharedString,
    frame_image: Option<(u32, u32, Arc<RenderImage>)>,
    expanded: bool,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> impl IntoElement {
    let source_label: SharedString = match track.source {
        VoiceTrackSource::ScreenShare => l10n::tr("Screen Share").into(),
        VoiceTrackSource::Camera => l10n::tr("Camera").into(),
        VoiceTrackSource::Microphone | VoiceTrackSource::ScreenShareAudio | VoiceTrackSource::Unknown => {
            l10n::tr("Video Stream").into()
        }
    };
    let resolution_label: Option<SharedString> = frame_image
        .as_ref()
        .map(|(w, h, _)| format!("{w}x{h}").into());
    let track_for_stop = track.clone();

    div()
        .id("video-viewer")
        .role(Role::Region)
        .aria_label(l10n::tr("Watched video stream"))
        .debug_selector(|| "video-viewer".to_owned())
        .flex()
        .flex_col()
        .when(expanded, |d| d.flex_1().min_h(px(320.)))
        .when(!expanded, |d| d.flex_none())
        .gap_2()
        .px_3()
        .py_2()
        .border_b_1()
        .border_color(theme.border)
        .bg(theme.background_secondary)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .min_w_0()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.foreground)
                                .child(source_label),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(participant_label),
                        )
                        .when_some(resolution_label, |d, res| {
                            d.child(
                                div()
                                    .px_1()
                                    .rounded(px(4.))
                                    .bg(theme.background)
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(res),
                            )
                        }),
                )
                .child(
                    div()
                        .debug_selector(|| "video-viewer-stop-btn".to_owned())
                        .child(
                            Button::new("video-viewer-stop-btn")
                                .ghost()
                                .xsmall()
                                .label(l10n::tr("Stop Watching"))
                                .accessibility_label(l10n::tr("Stop watching stream"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.toggle_watch_video_track(track_for_stop.clone(), cx);
                                })),
                        ),
                ),
        )
        .child(
            div()
                .when(expanded, |d| d.flex_1().min_h(px(260.0)))
                .when(!expanded, |d| d.h(px(260.0)))
                .w_full()
                .rounded(px(6.))
                .overflow_hidden()
                .bg(theme.background)
                .flex()
                .items_center()
                .justify_center()
                .map(|d| match frame_image {
                    Some((_, _, img_arc)) => d.child(
                        img(ImageSource::Render(img_arc))
                            .object_fit(ObjectFit::Contain)
                            .size_full(),
                    ),
                    None => d.child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(l10n::tr("Waiting for video frames...")),
                    ),
                }),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use fluxer_client_core::voice::VideoCodec;

    #[test]
    fn decoded_frame_to_render_image_swaps_rgba_to_bgra() {
        let frame = DecodedVideoFrame {
            width: 2,
            height: 2,
            timestamp_us: 100,
            codec: Some(VideoCodec::H264),
            // 4 RGBA pixels: (R, G, B, A)
            data: vec![
                10, 20, 30, 255, //
                40, 50, 60, 200, //
                70, 80, 90, 150, //
                100, 110, 120, 100, //
            ],
        };
        let render_img = decoded_frame_to_render_image(&frame).expect("valid 2x2 frame");
        assert_eq!(render_img.frame_count(), 1);
        let bytes = render_img.as_bytes(0).expect("frame 0 bytes");
        assert_eq!(
            bytes,
            &[
                30, 20, 10, 255, //
                60, 50, 40, 200, //
                90, 80, 70, 150, //
                120, 110, 100, 100, //
            ]
        );
    }

    #[test]
    fn decoded_frame_to_render_image_rejects_invalid_dimensions_or_length() {
        let zero_w = DecodedVideoFrame {
            width: 0,
            height: 2,
            timestamp_us: 0,
            codec: None,
            data: vec![],
        };
        assert!(decoded_frame_to_render_image(&zero_w).is_none());

        let zero_h = DecodedVideoFrame {
            width: 2,
            height: 0,
            timestamp_us: 0,
            codec: None,
            data: vec![],
        };
        assert!(decoded_frame_to_render_image(&zero_h).is_none());

        let bad_len = DecodedVideoFrame {
            width: 2,
            height: 2,
            timestamp_us: 0,
            codec: None,
            data: vec![0; 15],
        };
        assert!(decoded_frame_to_render_image(&bad_len).is_none());

        let overflow = DecodedVideoFrame {
            width: u32::MAX,
            height: u32::MAX,
            timestamp_us: 0,
            codec: None,
            data: vec![],
        };
        assert!(decoded_frame_to_render_image(&overflow).is_none());
    }
}
