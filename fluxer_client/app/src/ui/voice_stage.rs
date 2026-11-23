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
use gpui_kit::{AnyElement, Context, FontWeight, Hsla, Role, SharedString, div, prelude::*, px, rgb};

use super::avatar::{AvatarSpec, render_avatar};
use super::{l10n, video_viewer};
use crate::backend::voice::{TrackSubscriptionKey, VoiceConnectionState, VoiceTrackSource};
use crate::client_view::ClientView;
use crate::icons::AppIcon;
use crate::theme::Theme;

struct StageParticipant {
    user_id: Snowflake,
    display_name: SharedString,
    avatar_url: Option<SharedString>,
    muted: bool,
    deafened: bool,
    streaming: bool,
    is_self: bool,
    watch_track: Option<TrackSubscriptionKey>,
}

fn find_member_video_track(
    tracks: &[TrackSubscriptionKey],
    user_id: Snowflake,
) -> Option<TrackSubscriptionKey> {
    let uid_str = user_id.to_string();
    tracks
        .iter()
        .find(|t| {
            matches!(t.source, VoiceTrackSource::ScreenShare | VoiceTrackSource::Camera)
                && (t.participant_identity == uid_str
                    || t.participant_identity.starts_with(&format!("{uid_str}:")))
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
    let active_ring = connected && !participant.muted && !participant.deafened;
    let ring_color = if active_ring {
        theme.status_online
    } else {
        theme.border
    };
    let name_label = if participant.is_self {
        format!("{} ({})", participant.display_name, l10n::tr("You"))
    } else {
        participant.display_name.to_string()
    };

    div()
        .id(("voice-stage-tile", uid.0))
        .debug_selector(move || format!("voice-stage-tile-{}", uid.0))
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
                                                .xsmall()
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
                                                .xsmall()
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
    let active_ring = connected && !participant.muted && !participant.deafened;
    let border_color = if active_ring {
        theme.status_online
    } else {
        theme.border
    };
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
                                    .child(Icon::new(AppIcon::MicrophoneSlash).xsmall().text_color(theme.danger)),
                            )
                        })
                        .when(participant.deafened, |d| {
                            d.child(
                                div()
                                    .debug_selector(move || format!("voice-tile-deaf-{}", uid.0))
                                    .child(Icon::new(AppIcon::SpeakerSlash).xsmall().text_color(theme.danger)),
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

/// Official `VoiceControlBar.tsx` split button group (`.buttonContainer`: primary action + `CaretDown` settings trigger).
fn split_control_group(
    main_id: &'static str,
    caret_id: &'static str,
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
    let (bg, fg) = if active_danger {
        (theme.danger, theme.on_fill)
    } else if active_online {
        (theme.status_online, theme.on_fill)
    } else {
        (theme.background, theme.foreground)
    };
    let border_col = if active_danger || active_online {
        bg
    } else {
        theme.border
    };

    div()
        .flex()
        .flex_row()
        .items_center()
        .rounded(px(10.))
        .border_1()
        .border_color(border_col)
        .bg(theme.background)
        .overflow_hidden()
        .child(
            div()
                .id(main_id)
                .role(Role::Button)
                .aria_label(main_label)
                .debug_selector(move || main_id.to_owned())
                .w(px(40.))
                .h(px(36.))
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
                .child(Icon::new(icon).small().text_color(fg)),
        )
        .child(
            div()
                .id(caret_id)
                .role(Role::Button)
                .aria_label(caret_label)
                .debug_selector(move || caret_id.to_owned())
                .w(px(22.))
                .h(px(36.))
                .border_l_1()
                .border_color(theme.border)
                .bg(theme.background)
                .text_color(theme.muted_foreground)
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|s| s.bg(theme.muted).text_color(theme.foreground))
                .on_click(on_caret_click)
                .child(Icon::new(AppIcon::CaretDown).xsmall().text_color(theme.muted_foreground)),
        )
        .into_any_element()
}

fn single_control_button(
    id: &'static str,
    icon: AppIcon,
    label: String,
    active_danger: bool,
    disabled: bool,
    theme: &Theme,
    on_click: impl Fn(&gpui_kit::ClickEvent, &mut gpui_kit::Window, &mut gpui_kit::App) + 'static,
) -> AnyElement {
    let (bg, fg) = if active_danger {
        (theme.danger, theme.on_fill)
    } else {
        (theme.background, theme.foreground)
    };
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .debug_selector(move || id.to_owned())
        .w(px(40.))
        .h(px(36.))
        .rounded(px(10.))
        .flex()
        .items_center()
        .justify_center()
        .border_1()
        .border_color(if active_danger { bg } else { theme.border })
        .bg(bg)
        .text_color(fg)
        .when(disabled, |d| d.opacity(0.45))
        .when(!disabled, |d| {
            d.cursor_pointer()
                .hover(|s| s.opacity(0.88))
                .on_click(on_click)
        })
        .child(Icon::new(icon).small().text_color(fg))
        .into_any_element()
}

/// Renders the central Voice/Video Call Stage (`CompactVoiceCallView` / `VoiceCallView`).
pub fn render(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
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
                            .accessibility_label(expand_label)
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
        .flex_wrap()
        .items_center()
        .justify_center()
        .gap_4()
        .p_4();

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

    let mut stage_body = div()
        .id("voice-stage-body")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .justify_center()
        .gap_3()
        .bg(theme.background_tertiary);

    if let Some(track) = &watched_track {
        let label = video_viewer::participant_display_name(view, track);
        stage_body = stage_body
            .justify_start()
            .child(video_viewer::render_stage(
                track,
                label,
                view.watched_video_frame.clone(),
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
            let label = if is_watching {
                format!("{} ({})", l10n::tr("Stop Watching"), track.participant_identity)
            } else {
                format!("{} ({})", l10n::tr("Watch Stream"), track.participant_identity)
            };
            let sel = format!("voice-stage-watch-{}", track.track_sid);
            bar = bar.child(
                div().debug_selector(move || sel.clone()).child(
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

    // Official `VoiceControlBar.tsx`: grouped split buttons + separator + disconnect.
    let control_pill = div()
        .px_3()
        .py_2()
        .rounded(px(14.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.background_secondary)
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .justify_center()
        .gap_2()
        .child(split_control_group(
            "voice-stage-mute-btn",
            "voice-stage-input-settings-btn",
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
            cx.listener(|this, _, window, cx| this.open_voice_settings(window, cx)),
        ))
        .child(split_control_group(
            "voice-stage-deaf-btn",
            "voice-stage-output-settings-btn",
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
            cx.listener(|this, _, window, cx| this.open_voice_settings(window, cx)),
        ))
        .child(split_control_group(
            "voice-stage-camera-btn",
            "voice-stage-camera-settings-btn",
            AppIcon::EyeSlash,
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
            AppIcon::Monitor,
            share_label,
            l10n::tr("Screen share settings"),
            false,
            view.voice.self_stream,
            !is_connected,
            theme,
            cx.listener(|this, _, _, cx| this.toggle_screen_share(false, cx)),
            cx.listener(|this, _, window, cx| {
                if this.voice.self_stream {
                    this.toggle_screen_share_audio(cx);
                } else {
                    this.open_voice_settings(window, cx);
                }
            }),
        ))
        .child(single_control_button(
            "voice-stage-more-btn",
            AppIcon::SlidersHorizontal,
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
                .h(px(24.))
                .bg(theme.border),
        )
        .child(single_control_button(
            "voice-stage-disconnect-btn",
            AppIcon::PhoneDisconnect,
            l10n::tr("Disconnect"),
            true,
            false,
            theme,
            cx.listener(|this, _, _, cx| this.leave_voice_channel(cx)),
        ))
        .when_some(watched_track, |d, track| {
            d.child(single_control_button(
                "voice-stage-stop-watching-btn",
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

    let controls = div()
        .id("voice-stage-controls")
        .debug_selector(|| "voice-stage-controls".to_owned())
        .flex_none()
        .py_3()
        .px_4()
        .border_t_1()
        .border_color(theme.border)
        .bg(theme.background_tertiary)
        .flex()
        .flex_row()
        .items_center()
        .justify_center()
        .child(control_pill);

    let is_split_view = !view.voice_stage_expanded && view.selected_channel.is_some();

    div()
        .id("voice-stage")
        .role(Role::Region)
        .aria_label(l10n::tr("Voice call stage"))
        .debug_selector(|| "voice-stage".to_owned())
        .flex()
        .flex_col()
        .w_full()
        .when(!is_split_view, |d| d.flex_1().h_full())
        .when(is_split_view, |d| {
            d.h(px(360.))
                .flex_none()
                .border_b_1()
                .border_color(theme.border)
        })
        .min_w_0()
        .bg(theme.background_tertiary)
        .child(header)
        .child(stage_body)
        .child(controls)
        .into_any_element()
}
