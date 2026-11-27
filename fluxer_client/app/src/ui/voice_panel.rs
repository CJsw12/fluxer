//! Bottom-left voice connection status and local media controls
//! (`VoiceConnectionStatusInner.tsx` + `LocalParticipantControls.tsx`),
//! rendered inside the top section (`.voiceConnectionWrapper`) of the unified
//! bottom-left `UserArea` card whenever a voice session is active or failed.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{Context, Div, FontWeight, Hsla, Role, SharedString, Stateful, div, prelude::*, px};

use super::avatar::{AvatarSpec, render_avatar};
use super::l10n;
use crate::backend::voice::{VoiceConnectionState, VoiceTrackSource};
use crate::client_view::{ClientView, VoicePopoutKind};
use crate::icons::AppIcon;
use crate::parity_probe::FlxExt as _;
use crate::settings_model::SettingsEffect;
use crate::theme::Theme;

/// Renders the 3-bar signal strength indicator (`SignalStrengthIcon.tsx`).
fn render_signal_strength_icon(color: Hsla) -> Stateful<Div> {
    div()
        .id("voice-signal-icon")
        .debug_selector(|| "voice-signal-icon".to_owned())
        .size(px(24.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .flx("voice.voice-connection-status.voice-connection-status-inner.signal-icon")
        .child(
            div()
                .size(px(16.))
                .flex()
                .flex_row()
                .items_end()
                .justify_center()
                .gap(px(1.5))
                .pb(px(1.5))
                .flx("voice.signal-strength-icon.svg")
                .child(div().w(px(2.5)).h(px(5.)).rounded(px(1.)).bg(color))
                .child(div().w(px(2.5)).h(px(9.)).rounded(px(1.)).bg(color))
                .child(div().w(px(2.5)).h(px(13.)).rounded(px(1.)).bg(color)),
        )
}

pub fn render(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> Option<impl IntoElement> {
    let (status_text, status_color, failure_detail): (SharedString, Hsla, Option<SharedString>) =
        match &view.voice.state {
            VoiceConnectionState::Disconnected => return None,
            VoiceConnectionState::Connecting => {
                (l10n::tr("Connecting...").into(), theme.status_connecting, None)
            }
            VoiceConnectionState::Connected => (l10n::tr("In voice chat").into(), theme.status_online, None),
            VoiceConnectionState::Reconnecting => {
                (l10n::tr("Connecting...").into(), theme.status_connecting, None)
            }
            VoiceConnectionState::Failed(reason) => (
                l10n::tr("Connection failed").into(),
                theme.danger,
                Some(reason.to_string().into()),
            ),
        };

    let channel_id = view.voice.channel_id;
    let channel_label: SharedString = channel_id
        .map(|id| view.voice_channel_name(id))
        .unwrap_or_default();
    let guild_label: Option<SharedString> = channel_id
        .and_then(|id| view.voice_guild_name(view.voice.guild_id, id));
    let is_connected = view.voice.state == VoiceConnectionState::Connected;
    let is_failed = matches!(view.voice.state, VoiceConnectionState::Failed(_));
    let share_label = if view.voice.self_stream {
        l10n::tr("Configure or end screen share")
    } else {
        l10n::tr("Share Screen")
    };
    let share_audio_label = if view.voice.screen_share_audio {
        l10n::tr("Audio On")
    } else {
        l10n::tr("Audio Off")
    };
    let watchable_video_track = if is_connected {
        view.voice
            .published_tracks
            .iter()
            .find(|track| matches!(track.source, VoiceTrackSource::ScreenShare | VoiceTrackSource::Camera))
            .cloned()
    } else {
        None
    };

    let participants = channel_id
        .map(|cid| view.voice.members_in_channel(cid))
        .unwrap_or_default();

    let mut share_active_bg = theme.status_online;
    share_active_bg.a = 0.16;
    let mut share_active_border = theme.status_online;
    share_active_border.a = 0.35;

    Some(
        div()
            .id("voice-panel")
            .role(Role::Region)
            .aria_label(l10n::tr("Voice connection"))
            .debug_selector(|| "voice-panel".to_owned())
            .flx("app.user-area.user-area-inner.voice-connection-wrapper")
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .gap(px(12.))
                    .p(px(8.))
                    .flx("voice.voice-connection-status.voice-connection-status-inner.voice-connection-container")
                    // Row 1: `.statusRow` — Signal icon + status button on left, audio processing + disconnect icon buttons on right.
                    .child(
                        div()
                            .h(px(32.))
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .flx("voice.voice-connection-status.voice-connection-status-inner.status-row")
                            .child(render_signal_strength_icon(status_color).when(is_connected, |d| {
                                d.tooltip(move |window, cx| {
                                    Tooltip::new(SharedString::from(l10n::tr("Measuring latency..."))).build(window, cx)
                                })
                            }))
                            .child(
                                div()
                                    .id("voice-panel-open-stage")
                                    .debug_selector(|| "voice-panel-open-stage".to_owned())
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| this.open_voice_stage(cx)))
                                    .flex_1()
                                    .min_w_0()
                                    .h(px(18.))
                                    .truncate()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(status_color)
                                    .flx("voice.voice-connection-status.voice-connection-status-inner.focus-ring-wrapper")
                                    .child(status_text),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(8.))
                                    .flex_none()
                                    .flx("voice.voice-connection-status.voice-connection-status-inner.controls")
                                    .when(!is_failed, |d| {
                                        let details_open = view.voice_popout == Some(VoicePopoutKind::Details);
                                        d.child(
                                            div()
                                                .id("voice-noise-suppression-btn")
                                                .role(Role::Button)
                                                .aria_label(l10n::tr("Voice & audio settings"))
                                                .debug_selector(|| "voice-noise-suppression-btn".to_owned())
                                                .size(px(32.))
                                                .rounded(px(6.))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .cursor_pointer()
                                                .when(details_open, |b| b.bg(theme.control_hover_bg))
                                                .hover(|s| s.bg(theme.control_hover_bg))
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.toggle_voice_popout(VoicePopoutKind::Details, cx);
                                                }))
                                                .flx("voice.voice-connection-status.audio-processing-button.control-button.click")
                                                .child(
                                                    Icon::new(AppIcon::Waveform)
                                                        .with_size(px(20.))
                                                        .text_color(if details_open {
                                                            theme.foreground
                                                        } else {
                                                            theme.muted_foreground
                                                        }),
                                                ),
                                        )
                                    })
                                    .when(is_failed, |d| {
                                        d.child(
                                            div()
                                                .id("voice-retry-tooltip")
                                                .flx("voice.voice-connection-status.failed-voice-connection-status.tooltip-retry")
                                                .tooltip(move |window, cx| {
                                                    Tooltip::new(SharedString::from(l10n::tr("Try again"))).build(window, cx)
                                                })
                                                .child(
                                                    div()
                                                        .id("voice-retry-btn")
                                                        .role(Role::Button)
                                                        .aria_label(l10n::tr("Try again"))
                                                        .debug_selector(|| "voice-retry-btn".to_owned())
                                                        .size(px(32.))
                                                        .rounded(px(6.))
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .cursor_pointer()
                                                        .hover(|s| s.bg(theme.control_hover_bg))
                                                        .on_click(cx.listener(move |this, _, _, cx| {
                                                            if let Some(cid) = channel_id {
                                                                this.join_voice_channel(cid, cx);
                                                            }
                                                        }))
                                                        .flx("voice.voice-connection-status.failed-voice-connection-status.retry-button")
                                                        .child(
                                                            Icon::new(AppIcon::ArrowClockwise)
                                                                .with_size(px(20.))
                                                                .text_color(theme.muted_foreground),
                                                        ),
                                                ),
                                        )
                                    })
                                    .child(
                                        div()
                                            .id(if is_failed { "voice-dismiss-tooltip" } else { "voice-disconnect-tooltip" })
                                            .flx(if is_failed {
                                                "voice.voice-connection-status.failed-voice-connection-status.tooltip-dismiss"
                                            } else {
                                                "voice.voice-connection-status.voice-connection-status-inner.tooltip--2"
                                            })
                                            .tooltip(move |window, cx| {
                                                Tooltip::new(SharedString::from(if is_failed {
                                                    l10n::tr("Dismiss")
                                                } else {
                                                    l10n::tr("Disconnect")
                                                }))
                                                .build(window, cx)
                                            })
                                            .child(
                                                div()
                                                    .id("voice-disconnect-btn")
                                                    .role(Role::Button)
                                                    .aria_label(if is_failed {
                                                        l10n::tr("Dismiss")
                                                    } else {
                                                        l10n::tr("Disconnect")
                                                    })
                                                    .debug_selector(|| "voice-disconnect-btn".to_owned())
                                                    .size(px(32.))
                                                    .rounded(px(6.))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .cursor_pointer()
                                                    .hover(|s| s.bg(theme.control_hover_bg))
                                                    .on_click(cx.listener(|this, _, _, cx| this.leave_voice_channel(cx)))
                                                    .flx("voice.voice-connection-status.voice-connection-status-inner.control-button")
                                                    .child(
                                                        Icon::new(if is_failed {
                                                            AppIcon::X
                                                        } else {
                                                            AppIcon::PhoneX
                                                        })
                                                        .with_size(px(20.))
                                                        .text_color(theme.muted_foreground),
                                                    ),
                                            ),
                                    ),
                            ),
                    )
                    // Row 2: `.connectionInfo` — `{channelName} / {guildName}` + connection id + speaking avatar stack.
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .flx("voice.voice-connection-status.voice-connection-status-inner.connection-info")
                            .when(!channel_label.is_empty(), |d| {
                                d.child(
                                    div()
                                        .h(px(16.))
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .flx("voice.voice-connection-status.voice-connection-status-inner.channel-source-row")
                                        .child(
                                            div()
                                                .id("voice-channel-source-link")
                                                .debug_selector(|| "voice-channel-source-link".to_owned())
                                                .cursor_pointer()
                                                .on_click(cx.listener(|this, _, _, cx| this.open_voice_stage(cx)))
                                                .flex()
                                                .flex_row()
                                                .items_center()
                                                .min_w_0()
                                                .max_w_full()
                                                .text_size(px(12.))
                                                .line_height(px(16.))
                                                .text_color(theme.text_secondary)
                                                .hover(|s| s.text_color(theme.foreground))
                                                .flx("voice.voice-connection-status.voice-connection-status-inner.channel-source-link.voice-connection-status-context-menu")
                                                .child(
                                                    div()
                                                        .truncate()
                                                        .flx_text(
                                                            "voice.voice-connection-status.voice-connection-status-inner.channel-source-channel",
                                                            channel_label.to_string(),
                                                        )
                                                        .child(channel_label.clone()),
                                                )
                                                .when_some(guild_label.as_ref().filter(|g| !g.is_empty()), |r, g| {
                                                    r.child(
                                                        div()
                                                            .flex_none()
                                                            .mx(px(2.))
                                                            .flx_text(
                                                                "voice.voice-connection-status.voice-connection-status-inner.channel-source-separator",
                                                                "/",
                                                            )
                                                            .child("/"),
                                                    )
                                                    .child(
                                                        div()
                                                            .truncate()
                                                            .flx_text(
                                                                "voice.voice-connection-status.voice-connection-status-inner.channel-source-guild",
                                                                g.to_string(),
                                                            )
                                                            .child(g.clone()),
                                                    )
                                                }),
                                        ),
                                )
                            })
                            .when(is_connected, |d| {
                                let conn_text = view
                                    .voice
                                    .connection_id
                                    .as_deref()
                                    .filter(|s| !s.is_empty())
                                    .unwrap_or("RTC Connected")
                                    .to_owned();
                                d.child(
                                    div()
                                        .h(px(16.))
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(8.))
                                        .flx("voice.voice-connection-status.voice-connection-status-inner.connection-id-row")
                                        .child(
                                            div()
                                                .size(px(16.))
                                                .flex_none()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .flx("voice.voice-connection-status.voice-connection-status-inner.connection-id-icon")
                                                .child(
                                                    Icon::new(AppIcon::Shield)
                                                        .with_size(px(14.))
                                                        .text_color(theme.text_tertiary),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .flex()
                                                .flex_row()
                                                .items_center()
                                                .text_size(px(12.))
                                                .line_height(px(16.))
                                                .text_color(theme.text_secondary)
                                                .flx("voice.voice-connection-status.voice-connection-status-inner.connection-id-value")
                                                .child(
                                                    div()
                                                        .id("voice-connection-id-tooltip")
                                                        .max_w_full()
                                                        .flx("voice.voice-connection-status.voice-connection-status-inner.tooltip--3")
                                                        .tooltip({
                                                            let tip = conn_text.clone();
                                                            move |window, cx| Tooltip::new(SharedString::from(tip.clone())).build(window, cx)
                                                        })
                                                        .child(
                                                            div()
                                                                .max_w_full()
                                                                .truncate()
                                                                .text_size(px(12.))
                                                                .line_height(px(16.))
                                                                .text_color(theme.text_secondary)
                                                                .flx_text(
                                                                    "voice.voice-connection-status.voice-connection-status-inner.connection-id-value-text",
                                                                    conn_text.clone(),
                                                                )
                                                                .child(conn_text),
                                                        ),
                                                ),
                                        ),
                                )
                            })
                            .when_some(failure_detail, |d, reason| {
                                d.child(
                                    div()
                                        .truncate()
                                        .text_xs()
                                        .text_color(theme.danger)
                                        .child(reason),
                                )
                            })
                            .when(view.voice.speak_denied, |d| {
                                d.child(
                                    div()
                                        .truncate()
                                        .text_xs()
                                        .text_color(theme.danger)
                                        .child(l10n::tr("Mic muted by permissions")),
                                )
                            })
                            .when(is_connected && !participants.is_empty(), |d| {
                                let mut stack = div()
                                    .id("voice-channel-avatar-stack")
                                    .debug_selector(|| "voice-channel-avatar-stack".to_owned())
                                    .self_start()
                                    .h(px(42.))
                                    .pt(px(2.))
                                    .pb(px(6.))
                                    .pr(px(2.))
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(6.))
                                    .flx("voice.voice-connection-status.voice-connection-status-inner.channel-avatar-stack");
                                for member in participants.iter().take(4) {
                                    let name = view.voice_member_name(member.guild_id, member.user_id);
                                    let avatar_url = view.voice_member_avatar_url(member.guild_id, member.user_id);
                                    let speaking = view.voice.is_speaking(member.user_id)
                                        && !member.self_mute
                                        && !member.mute
                                        && !member.self_deaf
                                        && !member.deaf;
                                    stack = stack.child(
                                        div()
                                            .rounded_full()
                                            .when(speaking, |b| b.border_2().border_color(theme.status_online))
                                            .flx("voice.voice-participant-avatar-list.render-avatar.stack-avatar")
                                            .child(render_avatar(
                                                AvatarSpec {
                                                    user_id: member.user_id,
                                                    avatar_url,
                                                    display_name: name,
                                                    size: 32.,
                                                    status: None,
                                                    status_border_bg: None,
                                                },
                                                theme,
                                            )),
                                    );
                                }
                                d.child(stack)
                            }),
                    )
                    // Row 3: `.mediaSection` (`LocalParticipantControls.tsx`) — 2-column grid of Camera + Screen Share icon buttons.
                    .when(!is_failed, |d| {
                        d.child(
                            div()
                                .h(px(32.))
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(4.))
                                .w_full()
                                .flx("voice.voice-connection-status.voice-connection-status-inner.media-section")
                                .child(
                                    div()
                                        .id("voice-camera-btn")
                                        .role(Role::Button)
                                        .aria_label(l10n::tr("Turn on camera"))
                                        .debug_selector(|| "voice-camera-btn".to_owned())
                                        .flex_1()
                                        .h(px(32.))
                                        .rounded(px(6.))
                                        .bg(theme.subtle_fill)
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .when(!is_connected, |b| b.opacity(0.5))
                                        .when(is_connected, |b| {
                                            b.cursor_pointer()
                                                .hover(|s| s.bg(theme.control_hover_bg))
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.open_voice_settings(window, cx);
                                                }))
                                        })
                                        .flx("voice.voice-connection-status.local-participant-controls.media-button.undefined")
                                        .child(
                                            Icon::new(AppIcon::CameraSlash)
                                                .with_size(px(20.))
                                                .text_color(theme.muted_foreground),
                                        ),
                                )
                                .child(
                                    div()
                                        .id("voice-share-btn")
                                        .role(Role::Button)
                                        .aria_label(share_label)
                                        .debug_selector(|| "voice-share-btn".to_owned())
                                        .flex_1()
                                        .h(px(32.))
                                        .rounded(px(6.))
                                        .when(view.voice.self_stream, |b| b.border_1().border_color(share_active_border))
                                        .bg(if view.voice.self_stream {
                                            share_active_bg
                                        } else {
                                            theme.subtle_fill
                                        })
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .when(!is_connected, |b| b.opacity(0.5))
                                        .when(is_connected, |b| {
                                            b.cursor_pointer()
                                                .hover(|s| {
                                                    s.bg(if view.voice.self_stream {
                                                        share_active_bg
                                                    } else {
                                                        theme.control_hover_bg
                                                    })
                                                })
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.handle_screen_share_button(cx);
                                                }))
                                        })
                                        .flx("voice.voice-connection-status.local-participant-controls.media-button.undefined")
                                        .child(
                                            Icon::new(AppIcon::MonitorPlay).with_size(px(20.)).text_color(if view.voice.self_stream {
                                                theme.status_online
                                            } else {
                                                theme.muted_foreground
                                            }),
                                        ),
                                ),
                        )
                    }),
            )
            // Active stream audio toggle & remote stream quick-watch pill when applicable.
            .when(
                is_connected && (view.voice.self_stream || watchable_video_track.is_some()),
                |d| {
                    d.child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .items_center()
                            .gap_1()
                            .when(view.voice.self_stream, |row| {
                                row.child(
                                    div().debug_selector(|| "voice-share-audio-btn".to_owned()).child(
                                        Button::new("voice-share-audio-btn")
                                            .ghost()
                                            .xsmall()
                                            .label(share_audio_label.clone())
                                            .accessibility_label(share_audio_label)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.toggle_screen_share_audio(cx);
                                            })),
                                    ),
                                )
                            })
                            .when_some(watchable_video_track, |row, track| {
                                let is_watching = view.voice.watched_video_track.as_ref() == Some(&track);
                                let watch_label = if is_watching {
                                    l10n::tr("Stop Watching")
                                } else {
                                    l10n::tr("Watch Stream")
                                };
                                row.child(
                                    div().debug_selector(|| "voice-watch-btn".to_owned()).child(
                                        Button::new("voice-watch-btn")
                                            .ghost()
                                            .xsmall()
                                            .label(watch_label.clone())
                                            .accessibility_label(watch_label)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.toggle_watch_video_track(track.clone(), cx);
                                            })),
                                    ),
                                )
                            }),
                    )
                },
            )
            // Inline `VoiceDetailsPopout` (`VoiceDetailsPopout.tsx` / `NoiseSuppressionPopout.tsx`)
            .when(!is_failed && view.voice_popout == Some(VoicePopoutKind::Details), |d| {
                let ns_checked = view.prefs.data.voice_noise_suppression;
                let in_vol = view.prefs.data.voice_input_volume;
                let out_vol = view.prefs.data.voice_output_volume;
                d.child(
                    div()
                        .id("voice-details-popout")
                        .debug_selector(|| "voice-details-popout".to_owned())
                        .p_2()
                        .rounded(px(6.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background)
                        .flex()
                        .flex_col()
                        .gap_1p5()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(status_color)
                                        .child(l10n::tr("RTC Connected · Opus")),
                                )
                                .child(
                                    div()
                                        .text_size(px(10.))
                                        .text_color(theme.muted_foreground)
                                        .child(format!("In {in_vol}% · Out {out_vol}%")),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap_2()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(l10n::tr("Noise Suppression")),
                                )
                                .child(
                                    div()
                                        .debug_selector(|| "voice-popout-noise-suppression".to_owned())
                                        .child(
                                            Switch::new("voice-popout-noise-suppression")
                                                .checked(ns_checked)
                                                .on_change(cx.listener(|this, checked: &bool, window, cx| {
                                                    this.apply_settings_effect(
                                                        SettingsEffect::VoiceNoiseSuppressionChanged(*checked),
                                                        window,
                                                        cx,
                                                    );
                                                })),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .debug_selector(|| "voice-details-open-settings".to_owned())
                                .child(
                                    Button::new("voice-details-open-settings")
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
            }),
    )
}
