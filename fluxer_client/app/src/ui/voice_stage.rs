//! Central voice/video call stage (`CompactVoiceCallViewInner.tsx` / `VoiceCallView.tsx`
//! + `VoiceParticipantAvatarList.tsx` + `VoiceControlBar.tsx`):
//! displayed in the main content pane when the user joins or clicks an active
//! voice channel. Supports:
//! 1. Split mode (top compact call surface + bottom channel text chat) vs Full-height mode
//!    (`voice-stage-expand-btn` toggle) + `"Back to Chat"` header button (`voice-stage-back-btn`).
//! 2. Audio-only wrapped circular avatar list (`VoiceParticipantWrappedAvatarList`,
//!    `voice-stage-avatar-list`) when no video/screen-share stream is active, switching
//!    automatically to 16:9 participant tiles + `ui::video_viewer` stage when streaming.
//! 3. Official grouped split-button `VoiceControlBar` (`[Mic|▾]`, `[Speaker|▾]`,
//!    `[Camera|▾]`, `[ScreenShare|▾]`, `[More]`, separator, `[Disconnect]`, `[StopWatching]`).

use fluxer_client_core::state::Snowflake;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, Context, FontWeight, Hsla, KeyDownEvent, MouseButton, MouseDownEvent, Role, SharedString, div,
    prelude::*, px, rgb,
};

use super::avatar::{AvatarSpec, render_avatar};
use super::{l10n, video_viewer};
use crate::backend::voice::{
    TrackSubscriptionKey, VoiceConnectionState, VoiceTrackSource, parse_participant_user_id,
};
use crate::client_view::ClientView;
use crate::icons::AppIcon;
use crate::parity_probe::FlxExt as _;
use crate::theme::Theme;

struct StageParticipant {
    user_id: Snowflake,
    display_name: SharedString,
    avatar_url: Option<SharedString>,
    muted: bool,
    deafened: bool,
    streaming: bool,
    speaking: bool,
    audio_level: u16,
    is_self: bool,
    watch_track: Option<TrackSubscriptionKey>,
}

fn find_member_video_track(
    tracks: &[TrackSubscriptionKey],
    user_id: Snowflake,
) -> Option<TrackSubscriptionKey> {
    tracks
        .iter()
        .find(|t| {
            matches!(t.source, VoiceTrackSource::ScreenShare | VoiceTrackSource::Camera)
                && parse_participant_user_id(&t.participant_identity) == Some(user_id)
        })
        .cloned()
}

fn build_participants(view: &ClientView, channel_id: Snowflake) -> Vec<StageParticipant> {
    let mut out = Vec::new();
    let self_id = view.user.as_ref().map(|u| u.id);
    let mut saw_self = false;

    for member in view.voice.members_in_channel(channel_id) {
        let is_self = self_id == Some(member.user_id);
        if is_self {
            saw_self = true;
        }
        let display_name = view.voice_member_name(member.guild_id, member.user_id);
        let avatar_url = view.voice_member_avatar_url(member.guild_id, member.user_id);
        let muted = if is_self {
            view.voice.effective_muted() || member.self_mute || member.mute
        } else {
            member.self_mute || member.mute || member.self_deaf || member.deaf
        };
        let deafened = if is_self {
            view.voice.effective_deafened() || member.self_deaf || member.deaf
        } else {
            member.self_deaf || member.deaf
        };
        let watch_track = if is_self {
            None
        } else {
            find_member_video_track(&view.voice.published_tracks, member.user_id)
        };
        let streaming = if is_self {
            view.voice.self_stream || member.self_stream
        } else {
            member.self_stream || watch_track.is_some()
        };
        out.push(StageParticipant {
            user_id: member.user_id,
            display_name,
            avatar_url,
            muted,
            deafened,
            streaming,
            speaking: view.voice.is_speaking(member.user_id),
            audio_level: view.voice.audio_level_milli(member.user_id),
            is_self,
            watch_track,
        });
    }

    if !saw_self
        && let Some(user) = &view.user
    {
        out.insert(
            0,
            StageParticipant {
                user_id: user.id,
                display_name: user.name.clone(),
                avatar_url: user.avatar_url.clone(),
                muted: view.voice.effective_muted(),
                deafened: view.voice.effective_deafened(),
                streaming: view.voice.self_stream,
                speaking: view.voice.is_speaking(user.id),
                audio_level: view.voice.audio_level_milli(user.id),
                is_self: true,
                watch_track: None,
            },
        );
    }

    out
}

/// Official `VoiceParticipantWrappedAvatarList.tsx` circular avatar entry for audio-only calls.
fn render_audio_avatar_slot(
    participant: &StageParticipant,
    connected: bool,
    theme: &Theme,
) -> AnyElement {
    let uid = participant.user_id;
    let active_ring = connected && participant.speaking && !participant.muted && !participant.deafened;
    let ring_color = if active_ring {
        theme.status_online
    } else {
        theme.border
    };
    let speaking_meta = format!("{}:{}:{}", uid.0, active_ring, participant.audio_level);
    let name_label = if participant.is_self {
        format!("{} ({})", participant.display_name, l10n::tr("You"))
    } else {
        participant.display_name.to_string()
    };

    div()
        .id(("voice-stage-tile", uid.0))
        .debug_selector(move || format!("voice-stage-tile-{}", uid.0))
        .flx_text("voice.voice-stage.participant-speaking", speaking_meta)
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_2()
        .px_3()
        .py_2()
        .child(
            div()
                .relative()
                .p(px(3.))
                .rounded_full()
                .border_2()
                .border_color(ring_color)
                .bg(theme.background_secondary)
                .child(render_avatar(
                    AvatarSpec {
                        user_id: uid,
                        avatar_url: participant.avatar_url.clone(),
                        display_name: participant.display_name.clone(),
                        size: 64.,
                        status: None,
                        status_border_bg: None,
                    },
                    theme,
                ))
                .when(participant.muted || participant.deafened, |d| {
                    d.child(
                        div()
                            .absolute()
                            .right(px(-2.))
                            .bottom(px(-2.))
                            .px_1()
                            .py_0p5()
                            .rounded_full()
                            .bg(theme.background_secondary)
                            .border_1()
                            .border_color(theme.border)
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_0p5()
                            .when(participant.muted, |b| {
                                b.child(
                                    div()
                                        .debug_selector(move || format!("voice-tile-muted-{}", uid.0))
                                        .child(
                                            Icon::new(AppIcon::MicrophoneSlash)
                                                .with_size(px(14.))
                                                .text_color(theme.danger),
                                        ),
                                )
                            })
                            .when(participant.deafened, |b| {
                                b.child(
                                    div()
                                        .debug_selector(move || format!("voice-tile-deaf-{}", uid.0))
                                        .child(
                                            Icon::new(AppIcon::SpeakerSlash)
                                                .with_size(px(14.))
                                                .text_color(theme.danger),
                                        ),
                                )
                            }),
                    )
                }),
        )
        .child(
            div()
                .max_w(px(140.))
                .px_2()
                .py_0p5()
                .rounded(px(999.))
                .bg(theme.background_secondary)
                .border_1()
                .border_color(theme.border)
                .truncate()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.foreground)
                .child(name_label),
        )
        .into_any_element()
}

/// 16:9 media tile (`VoiceParticipantTile.tsx`) used when video or screen share is active in the room.
fn render_participant_tile(
    participant: &StageParticipant,
    compact: bool,
    connected: bool,
    watched_sid: Option<&str>,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let uid = participant.user_id;
    let active_ring = connected && participant.speaking && !participant.muted && !participant.deafened;
    let border_color = if active_ring {
        theme.status_online
    } else {
        theme.border
    };
    let speaking_meta = format!("{}:{}:{}", uid.0, active_ring, participant.audio_level);
    let (tile_w, tile_h, avatar_size) = if compact {
        (168., 112., 44.)
    } else {
        (224., 164., 68.)
    };
    let mut pill_bg: Hsla = rgb(0x000000).into();
    pill_bg.a = 0.55;

    let mut tile = div()
        .id(("voice-stage-tile", uid.0))
        .debug_selector(move || format!("voice-stage-tile-{}", uid.0))
        .flx_text("voice.voice-stage.participant-speaking", speaking_meta)
        .relative()
        .w(px(tile_w))
        .h(px(tile_h))
        .rounded(px(12.))
        .border_2()
        .border_color(border_color)
        .bg(theme.background_secondary)
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_2()
        .p_3()
        .child(render_avatar(
            AvatarSpec {
                user_id: uid,
                avatar_url: participant.avatar_url.clone(),
                display_name: participant.display_name.clone(),
                size: avatar_size,
                status: None,
                status_border_bg: None,
            },
            theme,
        ))
        .when(participant.streaming, |d| {
            d.child(
                div()
                    .debug_selector(move || format!("voice-stage-live-{}", uid.0))
                    .absolute()
                    .top_2()
                    .right_2()
                    .px_1p5()
                    .py_0p5()
                    .rounded(px(4.))
                    .bg(theme.danger)
                    .text_color(theme.on_fill)
                    .text_size(px(10.))
                    .font_weight(FontWeight::BOLD)
                    .child("LIVE"),
            )
        })
        .child(
            div()
                .absolute()
                .left_2()
                .right_2()
                .bottom_2()
                .px_2()
                .py_1()
                .rounded(px(6.))
                .bg(pill_bg)
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_1()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.on_fill)
                        .child(if participant.is_self {
                            format!("{} ({})", participant.display_name, l10n::tr("You"))
                        } else {
                            participant.display_name.to_string()
                        }),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .when(participant.muted, |d| {
                            d.child(
                                div()
                                    .debug_selector(move || format!("voice-tile-muted-{}", uid.0))
                                    .child(Icon::new(AppIcon::MicrophoneSlash).with_size(px(14.)).text_color(theme.danger)),
                            )
                        })
                        .when(participant.deafened, |d| {
                            d.child(
                                div()
                                    .debug_selector(move || format!("voice-tile-deaf-{}", uid.0))
                                    .child(Icon::new(AppIcon::SpeakerSlash).with_size(px(14.)).text_color(theme.danger)),
                            )
                        }),
                ),
        );

    if let Some(track) = participant.watch_track.clone() {
        let is_watching = watched_sid == Some(track.track_sid.as_str());
        let label = if is_watching {
            l10n::tr("Stop Watching")
        } else {
            l10n::tr("Watch Stream")
        };
        let track_id_sel = format!("voice-stage-watch-{}", track.track_sid);
        tile = tile.child(
            div()
                .absolute()
                .top_2()
                .left_2()
                .debug_selector(move || track_id_sel.clone())
                .flx("voice.voice-stage.watch-stream")
                .child(
                    Button::new(SharedString::from(format!("stage-watch-{}", track.track_sid)))
                        .primary()
                        .xsmall()
                        .label(label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.toggle_watch_video_track(track.clone(), cx);
                        })),
                ),
        );
    }

    tile.into_any_element()
}

/// Official `VoiceControlBar.tsx` + `VoiceControlBar.module.css` button container:
/// `44x44px` (`--vc-button-size: 2.75rem`) rounded square (`--vc-button-radius: 1rem` = 16px)
/// with top-right floating `17x17px` (`rounded 7px`) `.settingsButton` (`CaretDown`) corner badge.
#[allow(clippy::too_many_arguments)]
fn split_control_group(
    main_id: &'static str,
    caret_id: &'static str,
    caret_flx: &'static str,
    icon: AppIcon,
    main_label: String,
    caret_label: String,
    active_danger: bool,
    active_online: bool,
    disabled: bool,
    theme: &Theme,
    on_main_click: impl Fn(&gpui_kit::ClickEvent, &mut gpui_kit::Window, &mut gpui_kit::App) + 'static,
    on_caret_click: impl Fn(&gpui_kit::ClickEvent, &mut gpui_kit::Window, &mut gpui_kit::App) + 'static,
) -> AnyElement {
    let (bg, fg): (Hsla, Hsla) = if active_danger {
        let mut danger_bg: Hsla = rgb(0xff453a).into();
        danger_bg.a = 0.16;
        (danger_bg, rgb(0xff453a).into())
    } else if active_online {
        let mut success_bg: Hsla = rgb(0x30d158).into();
        success_bg.a = 0.16;
        (success_bg, rgb(0x30d158).into())
    } else {
        (gpui_kit::hsla(0., 0., 1., 0.06), gpui_kit::hsla(0., 0., 1., 0.90))
    };

    div()
        .relative()
        .size(px(44.))
        .flex_none()
        .flx("voice.voice-control-bar.voice-control-bar-inner.button-container")
        .child(
            div()
                .id(main_id)
                .role(Role::Button)
                .aria_label(main_label)
                .debug_selector(move || main_id.to_owned())
                .size(px(44.))
                .rounded(px(16.))
                .bg(bg)
                .text_color(fg)
                .flex()
                .items_center()
                .justify_center()
                .when(disabled, |d| d.opacity(0.45))
                .when(!disabled, |d| {
                    d.cursor_pointer()
                        .hover(|s| s.opacity(0.88))
                        .on_click(on_main_click)
                })
                .flx("voice.voice-control-bar.voice-control-bar-inner.button.undefined")
                .child(Icon::new(icon).with_size(px(22.)).text_color(fg)),
        )
        .child(
            div()
                .id(caret_id)
                .role(Role::Button)
                .aria_label(caret_label)
                .debug_selector(move || caret_id.to_owned())
                .absolute()
                .top(px(-2.))
                .right(px(-2.))
                .size(px(17.))
                .rounded(px(7.))
                .border_1()
                .border_color(theme.vc_settings_border)
                .bg(rgb(0x000000))
                .text_color(gpui_kit::hsla(0., 0., 1., 0.88))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|s| s.border_color(gpui_kit::hsla(0., 0., 1., 0.40)))
                .on_click(on_caret_click)
                .flx(caret_flx)
                .child(Icon::new(AppIcon::CaretDown).with_size(px(10.)).text_color(gpui_kit::hsla(0., 0., 1., 0.88))),
        )
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn single_control_button(
    id: &'static str,
    flx: &'static str,
    icon: AppIcon,
    label: String,
    active_danger: bool,
    disabled: bool,
    _theme: &Theme,
    on_click: impl Fn(&gpui_kit::ClickEvent, &mut gpui_kit::Window, &mut gpui_kit::App) + 'static,
) -> AnyElement {
    let (bg, fg): (Hsla, Hsla) = if active_danger {
        (rgb(0xff3b30).into(), gpui_kit::white())
    } else {
        (gpui_kit::hsla(0., 0., 1., 0.06), gpui_kit::hsla(0., 0., 1., 0.90))
    };
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .debug_selector(move || id.to_owned())
        .size(px(44.))
        .rounded(px(16.))
        .flex()
        .items_center()
        .justify_center()
        .bg(bg)
        .text_color(fg)
        .when(disabled, |d| d.opacity(0.45))
        .when(!disabled, |d| {
            d.cursor_pointer()
                .hover(|s| s.opacity(0.88))
                .on_click(on_click)
        })
        .flx(flx)
        .child(Icon::new(icon).with_size(px(22.)).text_color(fg))
        .into_any_element()
}

/// Renders the central Voice/Video Call Stage (`CompactVoiceCallView` / `VoiceCallView`).
pub fn render(view: &ClientView, theme: &Theme, split_height: f32, cx: &mut Context<ClientView>) -> AnyElement {
    let Some(channel_id) = view.voice.channel_id else {
        return div().into_any_element();
    };
    let channel_name = view.voice_channel_name(channel_id);
    let is_connected = view.voice.state == VoiceConnectionState::Connected;
    let (status_text, status_color): (SharedString, Hsla) = match &view.voice.state {
        VoiceConnectionState::Disconnected => (l10n::tr("Disconnected").into(), theme.status_offline),
        VoiceConnectionState::Connecting => (l10n::tr("Connecting...").into(), theme.status_connecting),
        VoiceConnectionState::Connected => (l10n::tr("In Voice").into(), theme.status_online),
        VoiceConnectionState::Reconnecting => (l10n::tr("Reconnecting...").into(), theme.status_connecting),
        VoiceConnectionState::Failed(reason) => (reason.to_string().into(), theme.danger),
    };

    let participants = build_participants(view, channel_id);
    let participant_count = participants.len();
    let watched_track = view.voice.watched_video_track.clone();
    let watched_sid = watched_track.as_ref().map(|t| t.track_sid.as_str());
    let has_renderable_media = watched_track.is_some()
        || participants.iter().any(|p| p.streaming || p.watch_track.is_some())
        || !view.voice.published_tracks.is_empty();

    // Any published remote video tracks not matched to a tile still get a quick Watch pill.
    let matched_sids: Vec<&str> = participants
        .iter()
        .filter_map(|p| p.watch_track.as_ref().map(|t| t.track_sid.as_str()))
        .collect();
    let extra_streams: Vec<TrackSubscriptionKey> = if is_connected {
        view.voice
            .published_tracks
            .iter()
            .filter(|t| {
                matches!(t.source, VoiceTrackSource::ScreenShare | VoiceTrackSource::Camera)
                    && !matched_sids.contains(&t.track_sid.as_str())
            })
            .cloned()
            .collect()
    } else {
        Vec::new()
    };

    let expand_label = if view.voice_stage_expanded {
        l10n::tr("Show Text Chat")
    } else {
        l10n::tr("Expand Call View")
    };

    let header = div()
        .h(px(theme.sizes.header_height))
        .flex_none()
        .px_4()
        .border_b_1()
        .border_color(theme.border)
        .bg(theme.background_secondary)
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_3()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .min_w_0()
                .child(Icon::new(AppIcon::SpeakerHigh).small().text_color(status_color))
                .child(div().truncate().font_weight(FontWeight::BOLD).child(channel_name))
                .child(
                    div()
                        .px_2()
                        .py_0p5()
                        .rounded(px(999.))
                        .bg(theme.background)
                        .border_1()
                        .border_color(theme.border)
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(status_color)
                        .child(status_text),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(format!("{participant_count}")),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    div().debug_selector(|| "voice-stage-expand-btn".to_owned()).child(
                        Button::new("voice-stage-expand-btn")
                            .ghost()
                            .small()
                            .label(expand_label.clone())
                            .accessibility_label(expand_label.clone())
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_voice_stage_expanded(cx))),
                    ),
                )
                .child(
                    div().debug_selector(|| "voice-stage-back-btn".to_owned()).child(
                        Button::new("voice-stage-back-btn")
                            .ghost()
                            .small()
                            .label(l10n::tr("Back to Chat"))
                            .accessibility_label(l10n::tr("Back to Chat"))
                            .on_click(cx.listener(|this, _, _, cx| this.close_voice_stage(cx))),
                    ),
                ),
        );

    let compact_tiles = watched_track.is_some();
    let mut tiles_grid = div()
        .id("voice-stage-grid")
        .debug_selector(|| "voice-stage-grid".to_owned())
        .flex()
        .flex_row()
        .when(compact_tiles, |d| d.flex_none().overflow_x_scroll().py_2().px_4().gap_2())
        .when(!compact_tiles, |d| d.flex_wrap().gap_4().p_4())
        .items_center()
        .justify_center();

    if has_renderable_media {
        for p in &participants {
            tiles_grid = tiles_grid.child(render_participant_tile(
                p,
                compact_tiles,
                is_connected,
                watched_sid,
                theme,
                cx,
            ));
        }
    } else {
        // Official `VoiceParticipantWrappedAvatarList` presentation when audio-only.
        let mut avatar_list = div()
            .id("voice-stage-avatar-list")
            .debug_selector(|| "voice-stage-avatar-list".to_owned())
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .justify_center()
            .gap_4();
        for p in &participants {
            avatar_list = avatar_list.child(render_audio_avatar_slot(p, is_connected, theme));
        }
        tiles_grid = tiles_grid.child(avatar_list);
    }

    let speaking_entries: Vec<String> = view
        .voice
        .active_speakers
        .iter()
        .map(|uid| format!("{}:{}", uid.0, view.voice.audio_level_milli(*uid)))
        .collect();
    let speaking_summary = format!(
        "active={};speakers={}",
        view.voice.active_speakers.len(),
        speaking_entries.join(",")
    );

    let mut stage_body = div()
        .id("voice-stage-body")
        .flx_text("voice.voice-stage.speaking-state", speaking_summary)
        .flex_1()
        .min_h_0()
        .when(!compact_tiles, |d| d.overflow_y_scroll().justify_center().gap_3())
        .when(compact_tiles, |d| d.overflow_hidden().justify_between().gap_2())
        .flex()
        .flex_col()
        .bg(rgb(0x000000));

    if let Some(track) = &watched_track {
        let label = video_viewer::participant_display_name(view, track);
        stage_body = stage_body.child(video_viewer::render_stage(
            track,
            label,
            view.watched_video_frame.clone(),
            Some(view.watched_video_fps),
            theme,
            cx,
        ));
    }

    if !extra_streams.is_empty() {
        let mut bar = div()
            .px_4()
            .pt_2()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .justify_center()
            .gap_2();
        for track in extra_streams {
            let is_watching = watched_sid == Some(track.track_sid.as_str());
            let display = video_viewer::participant_display_name(view, &track);
            let label = if is_watching {
                format!("{} ({display})", l10n::tr("Stop Watching"))
            } else {
                format!("{} ({display})", l10n::tr("Watch Stream"))
            };
            let sel = format!("voice-stage-watch-{}", track.track_sid);
            bar = bar.child(
                div()
                    .debug_selector(move || sel.clone())
                    .flx("voice.voice-stage.watch-stream")
                    .child(
                        Button::new(SharedString::from(format!("stage-extra-watch-{}", track.track_sid)))
                            .primary()
                            .small()
                            .icon(AppIcon::Monitor)
                            .label(label)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.toggle_watch_video_track(track.clone(), cx);
                            })),
                    ),
            );
        }
        stage_body = stage_body.child(bar);
    }

    stage_body = stage_body.child(tiles_grid);

    let effective_muted = view.voice.effective_muted();
    let effective_deafened = view.voice.effective_deafened();
    let mute_locked = view.voice.mute_toggle_locked();
    let mute_label = if effective_muted { l10n::tr("Unmute") } else { l10n::tr("Mute") };
    let deaf_label = if effective_deafened { l10n::tr("Undeafen") } else { l10n::tr("Deafen") };
    let share_label = if view.voice.self_stream {
        l10n::tr("End screen share")
    } else {
        l10n::tr("Share Screen")
    };

    // Official `VoiceControlBar.tsx` + `VoiceControlBar.module.css`:
    // black floating capsule (`--vc-control-material-bg: #000`, `border-radius: 24px`, `padding: 6px`, `gap: 4px`).
    let control_pill = div()
        .h(px(58.))
        .p(px(6.))
        .rounded(px(24.))
        .border_1()
        .border_color(gpui_kit::hsla(0., 0., 1., 0.10))
        .bg(rgb(0x000000))
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .justify_center()
        .gap_1()
        .flx("voice.voice-control-bar.voice-control-bar-inner.container")
        .child(split_control_group(
            "voice-stage-mute-btn",
            "voice-stage-input-settings-btn",
            "voice.voice-control-bar.voice-control-bar-inner.settings-button.input-settings-click",
            if effective_muted {
                AppIcon::MicrophoneSlash
            } else {
                AppIcon::Microphone
            },
            mute_label,
            l10n::tr("Input settings"),
            effective_muted,
            false,
            mute_locked,
            theme,
            cx.listener(|this, _, _, cx| this.toggle_voice_mute(cx)),
            cx.listener(|this, _, _, cx| {
                this.toggle_voice_popout(crate::client_view::VoicePopoutKind::Input, cx);
            }),
        ))
        .child(split_control_group(
            "voice-stage-deaf-btn",
            "voice-stage-output-settings-btn",
            "voice.voice-control-bar.voice-control-bar-inner.settings-button.output-settings-click",
            if effective_deafened {
                AppIcon::SpeakerSlash
            } else {
                AppIcon::SpeakerHigh
            },
            deaf_label,
            l10n::tr("Output settings"),
            effective_deafened,
            false,
            view.voice.server_deaf,
            theme,
            cx.listener(|this, _, _, cx| this.toggle_voice_deaf(cx)),
            cx.listener(|this, _, _, cx| {
                this.toggle_voice_popout(crate::client_view::VoicePopoutKind::Output, cx);
            }),
        ))
        .child(split_control_group(
            "voice-stage-camera-btn",
            "voice-stage-camera-settings-btn",
            "voice.voice-control-bar.voice-control-bar-inner.settings-button.camera-settings-click",
            AppIcon::CameraSlash,
            l10n::tr("Turn on camera"),
            l10n::tr("Camera settings"),
            false,
            false,
            !is_connected,
            theme,
            cx.listener(|this, _, window, cx| this.open_voice_settings(window, cx)),
            cx.listener(|this, _, window, cx| this.open_voice_settings(window, cx)),
        ))
        .child(split_control_group(
            "voice-stage-share-btn",
            "voice-stage-share-settings-btn",
            "voice.voice-control-bar.voice-control-bar-inner.settings-button.screen-share-settings-click",
            AppIcon::MonitorPlay,
            share_label,
            l10n::tr("Screen share settings"),
            false,
            view.voice.self_stream,
            !is_connected,
            theme,
            cx.listener(|this, _, _, cx| this.handle_screen_share_button(cx)),
            cx.listener(|this, _, _, cx| {
                this.toggle_voice_popout(crate::client_view::VoicePopoutKind::Stream, cx);
            }),
        ))
        .child(single_control_button(
            "voice-stage-more-btn",
            "voice.voice-control-bar.voice-control-bar-inner.button.more-options-click",
            AppIcon::DotsThreeBold,
            l10n::tr("More options"),
            false,
            false,
            theme,
            cx.listener(|this, _, window, cx| this.open_voice_settings(window, cx)),
        ))
        .child(
            div()
                .id("voice-stage-control-separator")
                .debug_selector(|| "voice-stage-control-separator".to_owned())
                .w(px(1.))
                .h(px(22.))
                .bg(theme.vc_separator_bg)
                .flx("voice.voice-control-bar.voice-control-bar-inner.button-separator"),
        )
        .child(single_control_button(
            "voice-stage-disconnect-btn",
            "voice.voice-control-bar.voice-control-bar-inner.button.disconnect",
            AppIcon::PhoneX,
            l10n::tr("Disconnect"),
            true,
            false,
            theme,
            cx.listener(|this, _, _, cx| this.leave_voice_channel(cx)),
        ))
        .when_some(watched_track, |d, track| {
            d.child(single_control_button(
                "voice-stage-stop-watching-btn",
                "voice.voice-control-bar.voice-control-bar-inner.button.stop-watching",
                AppIcon::EyeSlash,
                l10n::tr("Stop watching stream"),
                false,
                false,
                theme,
                cx.listener(move |this, _, _, cx| {
                    this.toggle_watch_video_track(track.clone(), cx);
                }),
            ))
        });

    let control_popout = match view.voice_popout {
        Some(crate::client_view::VoicePopoutKind::Input) => {
            let active_dev = view
                .prefs
                .data
                .voice_input_device
                .clone()
                .unwrap_or_else(|| l10n::tr("System default"));
            let vol = view.prefs.data.voice_input_volume;
            Some(
                div()
                    .id("voice-control-popout")
                    .debug_selector(|| "voice-control-popout".to_owned())
                    .w(px(320.))
                    .p_3()
                    .rounded(px(10.))
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.background_secondary)
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .child(div().text_xs().font_weight(FontWeight::BOLD).child(l10n::tr("Input Device")))
                            .child(div().text_xs().text_color(theme.muted_foreground).truncate().child(active_dev)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(div().text_xs().font_weight(FontWeight::MEDIUM).child(l10n::tr("Input Volume")))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        Button::new("popout-in-vol-dec")
                                            .ghost()
                                            .xsmall()
                                            .label("-10%")
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.apply_settings_effect(
                                                    crate::settings_model::SettingsEffect::VoiceInputVolumeChanged(
                                                        vol.saturating_sub(10),
                                                    ),
                                                    window,
                                                    cx,
                                                );
                                            })),
                                    )
                                    .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).child(format!("{vol}%")))
                                    .child(
                                        Button::new("popout-in-vol-inc")
                                            .ghost()
                                            .xsmall()
                                            .label("+10%")
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.apply_settings_effect(
                                                    crate::settings_model::SettingsEffect::VoiceInputVolumeChanged(
                                                        (vol + 10).min(crate::prefs::MAX_VOICE_VOLUME),
                                                    ),
                                                    window,
                                                    cx,
                                                );
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .debug_selector(|| "voice-control-popout-settings".to_owned())
                            .child(
                                Button::new("voice-control-popout-settings")
                                    .ghost()
                                    .xsmall()
                                    .w_full()
                                    .label(l10n::tr("Voice & video settings"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_voice_settings(window, cx);
                                    })),
                            ),
                    ),
            )
        }
        Some(crate::client_view::VoicePopoutKind::Output) => {
            let active_dev = view
                .prefs
                .data
                .voice_output_device
                .clone()
                .unwrap_or_else(|| l10n::tr("System default"));
            let vol = view.prefs.data.voice_output_volume;
            Some(
                div()
                    .id("voice-control-popout")
                    .debug_selector(|| "voice-control-popout".to_owned())
                    .w(px(320.))
                    .p_3()
                    .rounded(px(10.))
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.background_secondary)
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .child(div().text_xs().font_weight(FontWeight::BOLD).child(l10n::tr("Output Device")))
                            .child(div().text_xs().text_color(theme.muted_foreground).truncate().child(active_dev)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(div().text_xs().font_weight(FontWeight::MEDIUM).child(l10n::tr("Output Volume")))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        Button::new("popout-out-vol-dec")
                                            .ghost()
                                            .xsmall()
                                            .label("-10%")
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.apply_settings_effect(
                                                    crate::settings_model::SettingsEffect::VoiceOutputVolumeChanged(
                                                        vol.saturating_sub(10),
                                                    ),
                                                    window,
                                                    cx,
                                                );
                                            })),
                                    )
                                    .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).child(format!("{vol}%")))
                                    .child(
                                        Button::new("popout-out-vol-inc")
                                            .ghost()
                                            .xsmall()
                                            .label("+10%")
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.apply_settings_effect(
                                                    crate::settings_model::SettingsEffect::VoiceOutputVolumeChanged(
                                                        (vol + 10).min(crate::prefs::MAX_VOICE_VOLUME),
                                                    ),
                                                    window,
                                                    cx,
                                                );
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .debug_selector(|| "voice-control-popout-settings".to_owned())
                            .child(
                                Button::new("voice-control-popout-settings")
                                    .ghost()
                                    .xsmall()
                                    .w_full()
                                    .label(l10n::tr("Voice & video settings"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_voice_settings(window, cx);
                                    })),
                            ),
                    ),
            )
        }
        Some(crate::client_view::VoicePopoutKind::Stream) => {
            let audio_lbl = if view.voice.screen_share_audio {
                l10n::tr("System Audio: On")
            } else {
                l10n::tr("System Audio: Off")
            };
            let res_label = match view.prefs.data.video_resolution.as_str() {
                "source" => l10n::tr("Source"),
                other => other.to_owned(),
            };
            let fps_val = view.prefs.data.video_fps;
            let codec_label = view.prefs.data.video_codec.to_uppercase();
            let bitrate_label = if view.prefs.data.video_bitrate_kbps == 0 {
                l10n::tr("Auto bitrate")
            } else {
                format!("{} Mbps", view.prefs.data.video_bitrate_kbps / 1000)
            };
            let summary = format!("{res_label} · {fps_val}fps · {codec_label} · {bitrate_label}");
            let active_res = view.prefs.data.video_resolution.clone();
            Some(
                div()
                    .id("voice-control-popout")
                    .debug_selector(|| "voice-control-popout".to_owned())
                    .w(px(340.))
                    .p_3()
                    .rounded(px(10.))
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.background_secondary)
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .child(div().text_xs().font_weight(FontWeight::BOLD).child(l10n::tr("Screen Share")))
                            .child(div().text_xs().text_color(theme.muted_foreground).child(summary)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap_1()
                            .children(["720p", "1080p", "1440p", "source"].into_iter().map(|res| {
                                let selected = active_res == res;
                                let res_owned = res.to_owned();
                                let lbl = if res == "source" { "Source" } else { res };
                                let mut btn = Button::new(SharedString::from(format!("popout-res-{res}")))
                                    .xsmall()
                                    .label(lbl);
                                btn = if selected { btn.primary() } else { btn.ghost() };
                                btn.on_click(cx.listener(move |this, _, window, cx| {
                                    this.apply_settings_effect(
                                        crate::settings_model::SettingsEffect::VideoResolutionChanged(
                                            res_owned.clone(),
                                        ),
                                        window,
                                        cx,
                                    );
                                }))
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap_1()
                            .children([15u8, 30u8, 60u8].into_iter().map(|fps| {
                                let selected = fps_val == fps;
                                let mut btn = Button::new(SharedString::from(format!("popout-fps-{fps}")))
                                    .xsmall()
                                    .label(format!("{fps} FPS"));
                                btn = if selected { btn.primary() } else { btn.ghost() };
                                btn.on_click(cx.listener(move |this, _, window, cx| {
                                    this.apply_settings_effect(
                                        crate::settings_model::SettingsEffect::VideoFpsChanged(fps),
                                        window,
                                        cx,
                                    );
                                }))
                            })),
                    )
                    .when(view.voice.self_stream, |d| {
                        d.child(
                            Button::new("popout-stream-audio-toggle")
                                .ghost()
                                .xsmall()
                                .w_full()
                                .label(audio_lbl)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.toggle_screen_share_audio(cx);
                                })),
                        )
                    })
                    .child(
                        div()
                            .debug_selector(|| "voice-control-popout-settings".to_owned())
                            .child(
                                Button::new("voice-control-popout-settings")
                                    .ghost()
                                    .xsmall()
                                    .w_full()
                                    .label(l10n::tr("Voice & video settings"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_voice_settings(window, cx);
                                    })),
                            ),
                    ),
            )
        }
        _ => None,
    };

    let controls = div()
        .id("voice-stage-controls")
        .debug_selector(|| "voice-stage-controls".to_owned())
        .flex_none()
        .pt(px(16.))
        .pb(px(12.))
        .px(px(16.))
        .bg(rgb(0x000000))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_2()
        .flx("voice.compact-voice-call-view.compact-voice-call-view-inner.control-bar-section")
        .children(control_popout)
        .child(control_pill);

    let is_split_view = !view.voice_stage_expanded && view.selected_channel.is_some();
    let pill_opacity = if matches!(
        view.active_resize,
        Some(crate::client_view::ActiveResize::VoiceCallHeight { .. })
    ) {
        1.0
    } else if view.voice_resize_hovered {
        0.92
    } else {
        0.35
    };

    let stage = div()
        .id("voice-stage")
        .role(Role::Region)
        .aria_label(l10n::tr("Voice call stage"))
        .debug_selector(|| "voice-stage".to_owned())
        .flex()
        .flex_col()
        .w_full()
        .when(!is_split_view, |d| d.flex_1().h_full().mr(px(-1.)))
        .when(is_split_view, |d| d.relative().h(px(split_height)).flex_none())
        .min_w_0()
        .bg(rgb(0x000000))
        .flx("voice.compact-voice-call-view.compact-voice-call-view-inner.section.voice-root-pointer-activity")
        .when(!is_split_view, |d| d.child(header))
        .child(stage_body)
        .child(controls)
        .when(is_split_view, |d| {
            d.child(
                div()
                    .id("voice-split-resize-handle")
                    .debug_selector(|| "voice-split-resize-handle".to_owned())
                    .aria_label(l10n::tr("Resize call view"))
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .h(px(16.))
                    .cursor_ns_resize()
                    .track_focus(&view.voice_resize_focus)
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        this.set_voice_resize_hover(*hovered, cx);
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            this.begin_voice_height_resize(ev, window, cx);
                        }),
                    )
                    .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                        this.handle_voice_resize_key(ev, window, cx);
                    }))
                    .flx("voice.compact-voice-call-view.compact-voice-call-view-inner.resize-handle.resize-key-down")
                    .child(
                        div()
                            .w(px(36.))
                            .h(px(4.))
                            .rounded_full()
                            .bg(rgb(0x69686d))
                            .opacity(pill_opacity)
                            .flx("voice.compact-voice-call-view.compact-voice-call-view-inner.resize-pill"),
                    ),
            )
            .child(
                div()
                    .id("voice-stage-expand-btn")
                    .role(Role::Button)
                    .aria_label(expand_label)
                    .debug_selector(|| "voice-stage-expand-btn".to_owned())
                    .absolute()
                    .left(px(12.))
                    .bottom(px(12.))
                    .size(px(42.))
                    .rounded(px(14.))
                    .border_1()
                    .border_color(theme.vc_separator_bg)
                    .bg(theme.vc_surface_bg)
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|s| s.border_color(theme.vc_corner_hover_border))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_voice_stage_expanded(cx)))
                    .child(Icon::new(AppIcon::ChatCircleFill).with_size(px(20.)).text_color(theme.vc_control_fg)),
            )
        });

    if is_split_view {
        div()
            .w_full()
            .mr(px(-1.))
            .flex_none()
            .border_b_1()
            .border_color(theme.border)
            .child(stage)
            .into_any_element()
    } else {
        stage.into_any_element()
    }
}
