//! Bottom-left voice connection status and local media controls
//! (`VoiceConnectionStatusInner.tsx` + `LocalParticipantControls.tsx`),
//! rendered inside the top section (`.voiceConnectionWrapper`) of the unified
//! bottom-left `UserArea` card whenever a voice session is active or failed.

use fluxer_client_core::state::Snowflake;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, App, Context, Div, FontWeight, Hsla, MouseButton, MouseDownEvent, Pixels, Point, Role,
    SharedString, Size, Stateful, Window, div, prelude::*, px,
};

use super::avatar::{AvatarSpec, render_avatar};
use super::{l10n, overlay};
use crate::backend::voice::{VoiceConnectionState, VoiceTrackSource};
use crate::client_view::{ClientView, VoicePopoutKind};
use crate::icons::AppIcon;
use crate::parity_probe::FlxExt as _;
use crate::settings_model::SettingsEffect;
use crate::theme::Theme;

#[derive(Clone, Copy)]
pub(crate) enum VoiceMenuKind {
    Connection,
    Camera,
    ScreenShare,
    ParticipantTile(Snowflake),
    ParticipantAvatar(Snowflake),
    DisableScreenSharePopouts,
    EnableScreenSharePopouts,
    ConfirmDisableScreenSharePopouts,
}

pub(crate) struct VoiceContextMenu {
    kind: VoiceMenuKind,
    anchor: Point<Pixels>,
}

pub(crate) fn open_context_menu(
    kind: VoiceMenuKind,
    cx: &Context<ClientView>,
) -> impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static + use<> {
    let handle = cx.entity().downgrade();
    move |event: &MouseDownEvent, _: &mut Window, cx: &mut App| {
        cx.stop_propagation();
        let _ = handle.update(cx, |view, cx| {
            view.user_area_popout_open = false;
            view.user_area_submenu = None;
            view.guild_header_popout_open = false;
            view.voice_popout = None;
            view.voice_context_menu = Some(VoiceContextMenu { kind, anchor: event.position });
            cx.notify();
        });
    }
}

fn close_context_menu(view: &mut ClientView, cx: &mut Context<ClientView>) {
    view.voice_context_menu = None;
    cx.notify();
}

pub(crate) fn render_context_menu(
    view: &ClientView,
    viewport: Size<Pixels>,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> Option<AnyElement> {
    let menu = view.voice_context_menu.as_ref()?;
    let width = px(280.).min((viewport.width - px(18.)).max(px(1.)));
    let anchor = menu.anchor;
    let handle = cx.entity().downgrade();
    let dismiss = move |_: &mut Window, cx: &mut App| {
        let _ = handle.update(cx, close_context_menu);
    };
    let row = |id: &'static str, tag, label, checked: Option<bool>| {
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(l10n::tr(label))
            .min_h(px(30.))
            .px_2()
            .py_1()
            .rounded(px(4.))
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .cursor_pointer()
            .hover(|style| style.bg(theme.control_hover_bg))
            .flx(tag)
            .child(div().min_w_0().child(l10n::tr(label)))
            .when_some(checked, |d, checked| {
                d.child(if checked { "☑" } else { "☐" })
            })
    };

    let content = div()
        .id("voice-context-menu")
        .w(width)
        .max_h((viewport.height - px(18.)).max(px(1.)))
        .overflow_y_scroll()
        .p_1()
        .text_sm()
        .text_color(theme.foreground);

    let content = match menu.kind {
        VoiceMenuKind::Connection => content
            .flx("voice.voice-connection-status.handle-voice-connection-status-context-menu.menu-group")
            .child(row("voice-show-call-avatars", "voice.voice-connection-status.handle-voice-connection-status-context-menu.checkbox-item", "Show call avatars", Some(view.voice_show_call_avatars))
                .on_click(cx.listener(|view, _, _, cx| {
                    view.voice_show_call_avatars = !view.voice_show_call_avatars;
                    close_context_menu(view, cx);
                })))
            .child(row("voice-show-connection-id", "voice.voice-connection-status.handle-voice-connection-status-context-menu.checkbox-item--2", "Show connection ID", Some(view.voice_show_connection_id))
                .on_click(cx.listener(|view, _, _, cx| {
                    view.voice_show_connection_id = !view.voice_show_connection_id;
                    close_context_menu(view, cx);
                }))),
        VoiceMenuKind::Camera => content
            .flx("voice.voice-connection-status.open-camera-settings-menu.voice-camera-settings-menu")
            .child(row("voice-camera-device", "voice.voice-settings-menus.voice-camera-settings-menu.voice-camera-device-submenu", "Camera", None)
                .on_click(cx.listener(|view, _, _, cx| {
                    // TODO(backlog): Select an actual capture device when camera publishing is supported.
                    close_context_menu(view, cx);
                })))
            .child(row("voice-mirror-camera", "voice.voice-settings-menus.voice-camera-settings-menu.checkbox-item.mirror-camera", "Mirror camera", Some(view.prefs.data.mirror_camera))
                .on_click(cx.listener(|view, _, window, cx| {
                    close_context_menu(view, cx);
                    view.apply_settings_effect(SettingsEffect::MirrorCameraChanged(!view.prefs.data.mirror_camera), window, cx);
                })))
            .child(row("voice-preview-camera", "voice.voice-settings-menus.voice-camera-settings-menu.menu-item.close", "Preview camera", None)
                .on_click(cx.listener(|view, _, _, cx| {
                    // TODO(backlog): Display a preview once the camera capture path is available.
                    close_context_menu(view, cx);
                })))
            .child(row("voice-camera-video-settings", "voice.voice-settings-menus.voice-camera-settings-menu.menu-item.close--2", "Video settings", None)
                .on_click(cx.listener(|view, _, window, cx| {
                    close_context_menu(view, cx);
                    view.open_voice_settings(window, cx);
                }))),
        VoiceMenuKind::ScreenShare => content
            .flx("voice.voice-connection-status.open-screen-share-menu.active-screen-share-menu")
            .child(row("voice-share-stop", "voice.active-screen-share-menu.stop", "Stop streaming", None)
                .text_color(theme.danger)
                .on_click(cx.listener(|view, _, _, cx| {
                    close_context_menu(view, cx);
                    if view.voice.self_stream {
                        view.handle_screen_share_button(cx);
                    }
                })))
            .child(row("voice-share-change", "voice.active-screen-share-menu.change", "Change stream", None)
                .on_click(cx.listener(|view, _, _, cx| {
                    close_context_menu(view, cx);
                    view.open_screen_share_picker(cx);
                }))),
        VoiceMenuKind::ParticipantTile(user_id) | VoiceMenuKind::ParticipantAvatar(user_id) => {
            let is_self = view.user.as_ref().is_some_and(|user| user.id == user_id);
            let guild_id = view.voice.guild_id;
            content
                .flx(match menu.kind {
                    VoiceMenuKind::ParticipantTile(_) => "voice.voice-participant-tile.handle-context-menu.voice-participant-context-menu",
                    _ => "voice.voice-participant-avatar-list.handle-context-menu.voice-participant-context-menu",
                })
                .child(row("voice-participant-profile", "voice.participant-context-menu.view-profile", "View Profile", None)
                    .on_click(cx.listener(move |view, _, window, cx| {
                        close_context_menu(view, cx);
                        view.open_profile(user_id, guild_id, anchor, window, cx);
                    })))
                .when(is_self, |content| {
                    content
                        .child(row("voice-participant-mic", "voice.participant-context-menu.mute-microphone", "Mute microphone", Some(view.voice.effective_muted()))
                            .on_click(cx.listener(|view, _, _, cx| {
                                close_context_menu(view, cx);
                                view.toggle_voice_mute(cx);
                            })))
                        .child(row("voice-participant-deafen", "voice.participant-context-menu.deafen", "Deafen", Some(view.voice.effective_deafened()))
                            .on_click(cx.listener(|view, _, _, cx| {
                                close_context_menu(view, cx);
                                view.toggle_voice_deaf(cx);
                            })))
                })
                .when(!is_self, |content| {
                    content
                        .child(row("voice-participant-volume", "voice.participant-context-menu.user-volume", "User volume", None)
                            .on_click(cx.listener(|view, _, _, cx| {
                                // TODO(backlog): Connect this control to per-user playback gain.
                                close_context_menu(view, cx);
                            })))
                        .child(row("voice-participant-playback", "voice.participant-context-menu.mute-playback", "Mute playback", None)
                            .on_click(cx.listener(|view, _, _, cx| {
                                // TODO(backlog): Connect this control to per-user playback mute.
                                close_context_menu(view, cx);
                            })))
                })
        }
        VoiceMenuKind::DisableScreenSharePopouts => content
            .flx("voice.pi-p-overlay.context-menu.menu-group")
            .child(row("voice-pip-disable", "voice.pi-p-overlay.context-menu.disable-screen-share-pip.menu-item", "Disable screen share picture-in-picture popouts", None)
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.voice_context_menu = Some(VoiceContextMenu {
                        kind: VoiceMenuKind::ConfirmDisableScreenSharePopouts,
                        anchor,
                    });
                    cx.notify();
                }))),
        VoiceMenuKind::EnableScreenSharePopouts => content
            .child(row("voice-pip-enable", "voice.pi-p-overlay.context-menu.enable-screen-share-pip.menu-item", "Enable screen share picture-in-picture popouts", None)
                .on_click(cx.listener(|view, _, _, cx| {
                    view.voice_screen_share_pip_enabled = true;
                    close_context_menu(view, cx);
                }))),
        VoiceMenuKind::ConfirmDisableScreenSharePopouts => {
            let confirmation = div()
                .p_4()
                .flex()
                .flex_col()
                .gap_3()
                .flx("voice.disable-screen-share-pi-p-confirm-modal.modal-root")
                .child(div().font_weight(FontWeight::SEMIBOLD).child(l10n::tr("Disable screen share picture-in-picture popouts?")))
                .child(div().text_sm().child(l10n::tr("The stream will move to the call view. You can turn popouts back on from the stream’s context menu.")))
                .child(div().flex().justify_end().gap_2()
                    .child(Button::new("voice-pip-cancel").label(l10n::tr("Cancel"))
                        .on_click(cx.listener(|view, _, _, cx| close_context_menu(view, cx))))
                    .child(Button::new("voice-pip-confirm").primary().label(l10n::tr("Disable popouts"))
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.voice_screen_share_pip_enabled = false;
                            close_context_menu(view, cx);
                            view.open_voice_stage(cx);
                        }))));
            return Some(overlay::modal(confirmation, theme, dismiss));
        }
    };
    Some(overlay::popover(content, anchor, theme, dismiss))
}

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
    let camera_label = if is_connected {
        l10n::tr("Turn on camera")
    } else {
        l10n::tr("Waiting for connection...")
    };
    let share_label = if !is_connected {
        l10n::tr("Waiting for connection...")
    } else if view.voice.self_stream {
        l10n::tr("Configure or end screen share")
    } else {
        l10n::tr("Share your screen")
    };
    let processing_label = l10n::tr(match (
        view.prefs.data.voice_processing_mode.as_str(),
        view.prefs.data.voice_noise_suppression_backend.as_str(),
    ) {
        ("studio", _) => "Audio processing (direct input, raw)",
        ("voice", _) => "Audio processing (focused voice)",
        (_, "deep_filter" | "gtcrn" | "rnnoise") => "Audio processing (custom, enhanced)",
        (_, "none") => "Audio processing (custom, no suppression)",
        _ => "Audio processing (custom)",
    });
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
            .border_b_1()
            .border_color(theme.border)
            .flx("app.user-area.user-area-inner.voice-connection-wrapper")
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
                                    .when(!is_failed, |d| {
                                        d.on_mouse_down(MouseButton::Right, open_context_menu(VoiceMenuKind::Connection, cx))
                                            .flx("voice.voice-connection-status.voice-connection-status-inner.status-button.voice-connection-status-context-menu")
                                    })
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
                                                .aria_label(processing_label.clone())
                                                .tooltip(move |window, cx| Tooltip::new(processing_label.clone()).build(window, cx))
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
                                                .flx("voice.voice-connection-status.audio-processing-button.tooltip")
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
                                            .when(is_failed, |d| d.flx("voice.voice-connection-status.failed-voice-connection-status.tooltip-dismiss"))
                                            .when(!is_failed, |d| d.flx("voice.voice-connection-status.voice-connection-status-inner.tooltip--2"))
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
                                                .when(!is_failed, |d| {
                                                    d.on_mouse_down(MouseButton::Right, open_context_menu(VoiceMenuKind::Connection, cx))
                                                })
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
                            .when(is_connected && view.voice_show_connection_id, |d| {
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
                            .when(is_connected && view.voice_show_call_avatars && !participants.is_empty(), |d| {
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
                                    let uid = member.user_id;
                                    stack = stack.child(
                                        div()
                                            .id(("voice-channel-avatar", uid.0))
                                            .on_mouse_down(MouseButton::Right, open_context_menu(VoiceMenuKind::ParticipantAvatar(uid), cx))
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
                                        .aria_label(camera_label.clone())
                                        .tooltip(move |window, cx| Tooltip::new(camera_label.clone()).build(window, cx))
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
                                                .on_mouse_down(MouseButton::Right, open_context_menu(VoiceMenuKind::Camera, cx))
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.open_voice_settings(window, cx);
                                                }))
                                        })
                                        .flx("voice.voice-connection-status.local-participant-controls.media-button.undefined")
                                        .flx("voice.voice-connection-status.local-participant-controls.tooltip")
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
                                        .aria_label(share_label.clone())
                                        .tooltip(move |window, cx| Tooltip::new(share_label.clone()).build(window, cx))
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
                                                .when(view.voice.self_stream, |b| {
                                                    b.on_mouse_down(MouseButton::Right, open_context_menu(VoiceMenuKind::ScreenShare, cx))
                                                })
                                        })
                                        .flx("voice.voice-connection-status.local-participant-controls.media-button.undefined")
                                        .flx("voice.voice-connection-status.local-participant-controls.tooltip--2")
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
                        .on_mouse_up_out(MouseButton::Left, cx.listener(ClientView::dismiss_voice_popout_outside))
                        .debug_selector(|| "voice-details-popout".to_owned())
                        .p_2()
                        .rounded(px(6.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background)
                        .flex()
                        .flex_col()
                        .gap_1p5()
                        .when_some(view.voice.connection_id.as_ref().filter(|id| !id.is_empty()), |d, id| {
                            let id = id.clone();
                            d.child(div().flex().justify_between().gap_2().text_xs()
                                .child(l10n::tr("Device"))
                                .child(div().id("voice-details-device")
                                    .tooltip(move |window, cx| Tooltip::new(id.clone()).build(window, cx))
                                    .flx("voice.voice-connection-status.voice-details-popout.tooltip")
                                    .child(l10n::tr("Desktop"))))
                        })
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
