//! Central voice/video call stage (`CompactVoiceCallView.tsx` / `VoiceCallView`):
//! displayed in the main content pane when the user joins or clicks an active
//! voice channel. Shows the channel header with a "Back to Chat" toggle,
//! an expanded `ui::video_viewer` stage when watching a remote screen share or
//! camera, a responsive grid of participant avatar tiles with speaking/mute/
//! deafen/live badges, and a bottom circular call-control bar.

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
            view.voice.self_mute || view.voice.server_mute || member.self_mute || member.mute
        } else {
            member.self_mute || member.mute
        };
        let deafened = if is_self {
            view.voice.self_deaf || view.voice.server_deaf || member.self_deaf || member.deaf
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
                muted: view.voice.self_mute || view.voice.server_mute,
                deafened: view.voice.self_deaf || view.voice.server_deaf,
                streaming: view.voice.self_stream,
                is_self: true,
                watch_track: None,
            },
        );
    }

    out
}

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

fn circle_control(
    id: &'static str,
    icon: AppIcon,
    label: String,
    active_danger: bool,
    active_brand: bool,
    disabled: bool,
    theme: &Theme,
    on_click: impl Fn(&gpui_kit::ClickEvent, &mut gpui_kit::Window, &mut gpui_kit::App) + 'static,
) -> AnyElement {
    let (bg, fg) = if active_danger {
        (theme.danger, theme.on_fill)
    } else if active_brand {
        (theme.brand, theme.on_fill)
    } else {
        (theme.background_secondary, theme.foreground)
    };
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .debug_selector(move || id.to_owned())
        .size(px(44.))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .border_1()
        .border_color(if active_danger || active_brand { bg } else { theme.border })
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

/// Renders the central Voice/Video Call Stage for the active voice channel.
pub fn render(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let Some(channel_id) = view.voice.channel_id else {
        return div().into_any_element();
    };
    let channel_name = view.voice_channel_name(channel_id);
    let is_connected = view.voice.state == VoiceConnectionState::Connected;
    let (status_text, status_color): (SharedString, Hsla) = match &view.voice.state {
        VoiceConnectionState::Disconnected => (l10n::tr("Disconnected").into(), theme.status_offline),
        VoiceConnectionState::Connecting => (l10n::tr("Voice Connecting…").into(), theme.status_connecting),
        VoiceConnectionState::Connected => (l10n::tr("Voice Connected").into(), theme.status_online),
        VoiceConnectionState::Reconnecting => (l10n::tr("Voice Reconnecting…").into(), theme.status_connecting),
        VoiceConnectionState::Failed(reason) => (reason.to_string().into(), theme.danger),
    };

    let participants = build_participants(view, channel_id);
    let participant_count = participants.len();
    let watched_track = view.voice.watched_video_track.clone();
    let watched_sid = watched_track.as_ref().map(|t| t.track_sid.as_str());

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
            div().debug_selector(|| "voice-stage-back-btn".to_owned()).child(
                Button::new("voice-stage-back-btn")
                    .ghost()
                    .small()
                    .label(l10n::tr("Back to Chat"))
                    .accessibility_label(l10n::tr("Back to Chat"))
                    .on_click(cx.listener(|this, _, _, cx| this.close_voice_stage(cx))),
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

    let mute_label = if view.voice.self_mute { l10n::tr("Unmute") } else { l10n::tr("Mute") };
    let deaf_label = if view.voice.self_deaf { l10n::tr("Undeafen") } else { l10n::tr("Deafen") };
    let share_label = if view.voice.self_stream {
        l10n::tr("Stop Sharing")
    } else {
        l10n::tr("Share Screen")
    };

    let controls = div()
        .id("voice-stage-controls")
        .debug_selector(|| "voice-stage-controls".to_owned())
        .flex_none()
        .py_4()
        .px_4()
        .border_t_1()
        .border_color(theme.border)
        .bg(theme.background_secondary)
        .flex()
        .flex_row()
        .items_center()
        .justify_center()
        .gap_3()
        .child(circle_control(
            "voice-stage-mute-btn",
            if view.voice.self_mute { AppIcon::MicrophoneSlash } else { AppIcon::Microphone },
            mute_label,
            view.voice.self_mute,
            false,
            false,
            theme,
            cx.listener(|this, _, _, cx| this.toggle_voice_mute(cx)),
        ))
        .child(circle_control(
            "voice-stage-deaf-btn",
            if view.voice.self_deaf { AppIcon::SpeakerSlash } else { AppIcon::Headphones },
            deaf_label,
            view.voice.self_deaf,
            false,
            false,
            theme,
            cx.listener(|this, _, _, cx| this.toggle_voice_deaf(cx)),
        ))
        .child(circle_control(
            "voice-stage-share-btn",
            AppIcon::Screencast,
            share_label,
            false,
            view.voice.self_stream,
            !is_connected,
            theme,
            cx.listener(|this, _, _, cx| this.toggle_screen_share(false, cx)),
        ))
        .child(circle_control(
            "voice-stage-disconnect-btn",
            AppIcon::PhoneDisconnect,
            l10n::tr("Disconnect"),
            true,
            false,
            false,
            theme,
            cx.listener(|this, _, _, cx| this.leave_voice_channel(cx)),
        ));

    div()
        .id("voice-stage")
        .role(Role::Region)
        .aria_label(l10n::tr("Voice call stage"))
        .debug_selector(|| "voice-stage".to_owned())
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .h_full()
        .bg(theme.background_tertiary)
        .child(header)
        .child(stage_body)
        .child(controls)
        .into_any_element()
}
