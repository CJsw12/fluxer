//! Screen-share source & quality picker modal (`Overlay::ScreenSharePicker`).
//! Allows choosing between Screens / Applications, capture source, resolution,
//! frame rate, codec, and system audio before clicking `"Go Live"`.

use gpui_kit::component::{Disableable as _, Sizable as _};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::switch::Switch;
use gpui_kit::{AnyElement, Context, FontWeight, Role, SharedString, div, prelude::*, px};

use super::{l10n, overlay};
use crate::backend::voice::{ScreenShareOptions, ScreenSourceInfo};
use crate::client_view::ClientView;
use crate::parity_probe::FlxExt as _;
use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenShareTab {
    Screens,
    Applications,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenSharePickerModal {
    pub tab: ScreenShareTab,
    pub selected_source_id: Option<u64>,
    pub selected_source_kind: String,
    pub resolution: String,
    pub fps: u32,
    pub codec: String,
    pub include_audio: bool,
}

impl ScreenSharePickerModal {
    pub fn from_prefs(resolution: &str, fps: u8, codec: &str) -> Self {
        let norm_res = match resolution {
            "720p" | "1080p" | "1440p" | "source" => resolution.to_owned(),
            _ => "1080p".to_owned(),
        };
        let norm_fps = match fps {
            15 | 30 | 60 => u32::from(fps),
            _ => 30,
        };
        let norm_codec = match codec.to_ascii_lowercase().as_str() {
            "h264" | "vp8" | "vp9" | "av1" => codec.to_ascii_lowercase(),
            _ => "auto".to_owned(),
        };
        Self {
            tab: ScreenShareTab::Screens,
            selected_source_id: None,
            selected_source_kind: "screen".to_owned(),
            resolution: norm_res,
            fps: norm_fps,
            codec: norm_codec,
            include_audio: false,
        }
    }

    pub fn resolution_dimensions(&self) -> (Option<u32>, Option<u32>) {
        match self.resolution.as_str() {
            "720p" => (Some(1280), Some(720)),
            "1080p" => (Some(1920), Some(1080)),
            "1440p" => (Some(2560), Some(1440)),
            _ => (None, None),
        }
    }

    pub fn to_options(&self, bitrate_kbps: u16) -> ScreenShareOptions {
        let (width, height) = self.resolution_dimensions();
        let codec = if self.codec == "auto" {
            None
        } else {
            Some(self.codec.clone())
        };
        let max_bitrate = (bitrate_kbps > 0).then(|| u64::from(bitrate_kbps) * 1000);
        ScreenShareOptions {
            with_audio: self.include_audio,
            source_id: self.selected_source_id,
            source_kind: Some(self.selected_source_kind.clone()),
            width,
            height,
            fps: Some(self.fps),
            codec,
            max_bitrate,
        }
    }
}

/// Result of matching the modal's selected source ID against the current source list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    /// A concrete source is selected (explicitly, or the first one when nothing was picked yet).
    Ok(u64),
    /// The previously selected source ID is no longer in the list.
    Missing,
    /// There are no sources to pick from.
    Empty,
}

/// Pure selection check; `sources` must already be filtered to the active tab.
pub fn resolve_selection(sources: &[ScreenSourceInfo], selected_id: Option<u64>) -> Selection {
    match (sources.first(), selected_id) {
        (None, _) => Selection::Empty,
        (Some(first), None) => Selection::Ok(first.id),
        (Some(_), Some(id)) if sources.iter().any(|s| s.id == id) => Selection::Ok(id),
        (Some(_), Some(_)) => Selection::Missing,
    }
}

pub fn go_live_enabled(selection: Selection) -> bool {
    matches!(selection, Selection::Ok(_))
}

/// Sources belonging to the picker tab, in list order.
pub fn sources_for_tab(all: &[ScreenSourceInfo], tab: ScreenShareTab) -> Vec<ScreenSourceInfo> {
    let want_kind = match tab {
        ScreenShareTab::Screens => "screen",
        ScreenShareTab::Applications => "window",
    };
    all.iter()
        .filter(|s| s.kind.eq_ignore_ascii_case(want_kind))
        .cloned()
        .collect()
}

pub fn render(
    view: &ClientView,
    modal: &ScreenSharePickerModal,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let sources = sources_for_tab(&view.voice.screen_sources, modal.tab);
    let selection = resolve_selection(&sources, modal.selected_source_id);
    let selected_id = match selection {
        Selection::Ok(id) => Some(id),
        Selection::Missing | Selection::Empty => None,
    };
    let source_count = sources.len();

    let tabs = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(
            div()
                .debug_selector(|| "screen-share-tab-screens".to_owned())
                .flx("voice.screen-share-modal.tab.screens")
                .child({
                    let mut b = Button::new("screen-share-tab-screens")
                        .small()
                        .label(l10n::tr("Screens"));
                    b = if modal.tab == ScreenShareTab::Screens {
                        b.primary()
                    } else {
                        b.ghost()
                    };
                    b.on_click(cx.listener(|this, _, _, cx| {
                        this.set_screen_share_picker_tab(ScreenShareTab::Screens, cx);
                    }))
                }),
        )
        .child(
            div()
                .debug_selector(|| "screen-share-tab-applications".to_owned())
                .flx("voice.screen-share-modal.tab.applications")
                .child({
                    let mut b = Button::new("screen-share-tab-applications")
                        .small()
                        .label(l10n::tr("Applications"));
                    b = if modal.tab == ScreenShareTab::Applications {
                        b.primary()
                    } else {
                        b.ghost()
                    };
                    b.on_click(cx.listener(|this, _, _, cx| {
                        this.set_screen_share_picker_tab(ScreenShareTab::Applications, cx);
                    }))
                }),
        );

    let mut source_list = div()
        .id("screen-share-source-list")
        .debug_selector(|| "screen-share-source-list".to_owned())
        .max_h(px(180.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_1()
        .p_2()
        .rounded(px(6.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.background);

    if sources.is_empty() {
        let empty_text = match modal.tab {
            ScreenShareTab::Screens => "No capturable screens found",
            ScreenShareTab::Applications => "No capturable application windows found",
        };
        source_list = source_list.child(
            div()
                .py_4()
                .text_xs()
                .text_color(theme.muted_foreground)
                .flx("screen-share-empty")
                .child(l10n::tr(empty_text)),
        );
    } else {
        for (idx, src) in sources.into_iter().enumerate() {
            let is_selected = selected_id == Some(src.id)
                && modal.selected_source_kind.eq_ignore_ascii_case(&src.kind);
            let src_id = src.id;
            let src_kind = src.kind.clone();
            let title = src.title.clone();
            let preview_group = SharedString::from(format!("screen-share-source-preview-{idx}"));
            source_list = source_list.child(
                div()
                    .id(("screen-share-source-item", idx))
                    .role(Role::Button)
                    .aria_label(title.clone())
                    .debug_selector(move || format!("screen-share-source-{idx}"))
                    .group(preview_group.clone())
                    .relative()
                    .px_3()
                    .py_2()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(if is_selected { theme.status_online } else { theme.border })
                    .bg(if is_selected {
                        theme.control_hover_bg
                    } else {
                        theme.background_secondary
                    })
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.control_hover_bg))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_screen_share_source(src_id, src_kind.clone(), cx);
                    }))
                    .flx_text("voice.screen-share-modal.source-item", title.clone())
                    .child(
                        div()
                            .truncate()
                            .text_sm()
                            .font_weight(if is_selected {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(theme.foreground)
                            .child(title),
                    )
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_end()
                            .px_2()
                            .opacity(0.)
                            .group_hover(preview_group, |style| style.opacity(1.))
                            .flx("voice.screen-share-picker-modal.picker-card-button.picker-card-preview.preview-overlay")
                            .child(div().rounded(px(4.)).px_2().py_0p5()
                                .bg(theme.background_tertiary)
                                .text_xs()
                                .text_color(theme.foreground)
                                .flx("voice.screen-share-picker-modal.picker-card-button.picker-card-preview.preview-action")
                                .child(l10n::tr("Select source"))),
                    ),
            );
        }
    }

    let missing_notice = (selection == Selection::Missing).then(|| {
        div()
            .text_xs()
            .text_color(theme.muted_foreground)
            .flx("screen-share-missing-source")
            .child(l10n::tr("The selected source is no longer available. Pick another source."))
    });

    let source_header = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(format!("{} {}", source_count, l10n::tr("sources"))),
        )
        .child(
            div()
                .debug_selector(|| "screen-share-refresh-btn".to_owned())
                .flx("screen-share-refresh")
                .child(
                    Button::new("screen-share-refresh-btn")
                        .ghost()
                        .xsmall()
                        .label(l10n::tr("Refresh sources"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.refresh_screen_share_sources(cx);
                        })),
                ),
        );

    let resolution_row = div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.muted_foreground)
                .child(l10n::tr("Resolution")),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .children(
                    [
                        ("720p", "720p", "voice.screen-share-modal.quality.720p"),
                        ("1080p", "1080p", "voice.screen-share-modal.quality.1080p"),
                        ("1440p", "1440p", "voice.screen-share-modal.quality.1440p"),
                        ("source", "Source", "voice.screen-share-modal.quality.source"),
                    ]
                    .into_iter()
                    .map(|(val, label, flx_id)| {
                        let selected = modal.resolution == val;
                        let val_owned = val.to_owned();
                        div()
                            .debug_selector(move || format!("screen-share-res-{val}"))
                            .flx(flx_id)
                            .child({
                                let mut b = Button::new(SharedString::from(format!(
                                    "screen-share-res-{val}"
                                )))
                                .xsmall()
                                .label(label);
                                b = if selected { b.primary() } else { b.ghost() };
                                b.on_click(cx.listener(move |this, _, _, cx| {
                                    this.set_screen_share_picker_resolution(val_owned.clone(), cx);
                                }))
                            })
                    }),
                ),
        );

    let fps_row = div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.muted_foreground)
                .child(l10n::tr("Frame Rate")),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .children(
                    [
                        (15u32, "15 FPS", "voice.screen-share-modal.fps.15"),
                        (30u32, "30 FPS", "voice.screen-share-modal.fps.30"),
                        (60u32, "60 FPS", "voice.screen-share-modal.fps.60"),
                    ]
                    .into_iter()
                    .map(|(fps_val, label, flx_id)| {
                        let selected = modal.fps == fps_val;
                        div()
                            .debug_selector(move || format!("screen-share-fps-{fps_val}"))
                            .flx(flx_id)
                            .child({
                                let mut b = Button::new(SharedString::from(format!(
                                    "screen-share-fps-{fps_val}"
                                )))
                                .xsmall()
                                .label(label);
                                b = if selected { b.primary() } else { b.ghost() };
                                b.on_click(cx.listener(move |this, _, _, cx| {
                                    this.set_screen_share_picker_fps(fps_val, cx);
                                }))
                            })
                    }),
                ),
        );

    let codec_row = div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.muted_foreground)
                .child(l10n::tr("Codec")),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .children(
                    [
                        ("auto", "Auto", "voice.screen-share-modal.codec.auto"),
                        ("h264", "H264", "voice.screen-share-modal.codec.h264"),
                        ("vp8", "VP8", "voice.screen-share-modal.codec.vp8"),
                        ("vp9", "VP9", "voice.screen-share-modal.codec.vp9"),
                        ("av1", "AV1", "voice.screen-share-modal.codec.av1"),
                    ]
                    .into_iter()
                    .map(|(val, label, flx_id)| {
                        let selected = modal.codec == val;
                        let val_owned = val.to_owned();
                        div()
                            .debug_selector(move || format!("screen-share-codec-{val}"))
                            .flx(flx_id)
                            .child({
                                let mut b = Button::new(SharedString::from(format!(
                                    "screen-share-codec-{val}"
                                )))
                                .xsmall()
                                .label(label);
                                b = if selected { b.primary() } else { b.ghost() };
                                b.on_click(cx.listener(move |this, _, _, cx| {
                                    this.set_screen_share_picker_codec(val_owned.clone(), cx);
                                }))
                            })
                    }),
                ),
        );

    let audio_row = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_2()
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.foreground)
                .child(l10n::tr("Share system audio")),
        )
        .child(
            div()
                .debug_selector(|| "screen-share-audio-toggle".to_owned())
                .flx("voice.screen-share-modal.audio-toggle")
                .child(
                    Switch::new("screen-share-audio-switch")
                        .checked(modal.include_audio)
                        .on_change(cx.listener(|this, checked: &bool, _, cx| {
                            this.set_screen_share_picker_audio(*checked, cx);
                        })),
                ),
        );

    let footer = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_end()
        .gap_2()
        .pt_2()
        .border_t_1()
        .border_color(theme.border)
        .child(
            div()
                .debug_selector(|| "screen-share-cancel-btn".to_owned())
                .flx("voice.screen-share-modal.cancel")
                .child(
                    Button::new("screen-share-cancel-btn")
                        .ghost()
                        .small()
                        .label(l10n::tr("Cancel"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.close_overlay(window, cx);
                        })),
                ),
        )
        .child(
            div()
                .debug_selector(|| "screen-share-go-live-btn".to_owned())
                .flx("voice.screen-share-modal.go-live")
                .child(
                    Button::new("screen-share-go-live-btn")
                        .primary()
                        .small()
                        .disabled(!go_live_enabled(selection))
                        .label(l10n::tr("Go Live"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.confirm_screen_share_picker(window, cx);
                        })),
                ),
        );

    let content = div()
        .id("screen-share-picker-modal")
        .role(Role::Dialog)
        .aria_label(l10n::tr("Share Your Screen"))
        .debug_selector(|| "screen-share-picker-modal".to_owned())
        .flx("voice.screen-share-modal.container")
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_base()
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme.foreground)
                        .child(l10n::tr("Share Your Screen")),
                )
                .child(
                    Button::new("screen-share-close-btn")
                        .ghost()
                        .xsmall()
                        .label("✕")
                        .accessibility_label(l10n::tr("Close"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.close_overlay(window, cx);
                        })),
                ),
        )
        .child(tabs)
        .child(source_header)
        .child(source_list)
        .children(missing_notice)
        .child(resolution_row)
        .child(fps_row)
        .child(codec_row)
        .child(audio_row)
        .child(footer);

    let view_handle = cx.entity().downgrade();
    overlay::modal(content, theme, move |window, cx| {
        let _ = view_handle.update(cx, |this, cx| this.close_overlay(window, cx));
    })
}
