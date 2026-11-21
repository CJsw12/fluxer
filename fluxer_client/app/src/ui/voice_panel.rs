//! Compact voice connection bar rendered above `user_panel` whenever the user
//! is connecting to, connected to, reconnecting, or has a failed voice attempt.
//! Reads only the UI-safe `VoiceSnapshot` (never holds tokens or endpoints).

use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::{Context, FontWeight, Role, SharedString, div, prelude::*};

use super::l10n;
use crate::backend::voice::VoiceConnectionState;
use crate::client_view::ClientView;
use crate::theme::Theme;

pub fn render(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> Option<impl IntoElement> {
    let (status_text, status_color): (SharedString, _) = match &view.voice.state {
        VoiceConnectionState::Disconnected => return None,
        VoiceConnectionState::Connecting => (l10n::tr("Voice Connecting…").into(), theme.status_connecting),
        VoiceConnectionState::Connected => (l10n::tr("Voice Connected").into(), theme.status_online),
        VoiceConnectionState::Reconnecting => (l10n::tr("Voice Reconnecting…").into(), theme.status_connecting),
        VoiceConnectionState::Failed(reason) => (reason.to_string().into(), theme.danger),
    };
    let channel_label: SharedString = view
        .voice
        .channel_id
        .map(|id| view.voice_channel_name(id))
        .unwrap_or_default();
    let mute_label = if view.voice.self_mute { l10n::tr("Unmute") } else { l10n::tr("Mute") };
    let deaf_label = if view.voice.self_deaf { l10n::tr("Undeafen") } else { l10n::tr("Deafen") };
    let is_connected = view.voice.state == VoiceConnectionState::Connected;
    let share_label = if view.voice.self_stream {
        l10n::tr("Stop Sharing")
    } else {
        l10n::tr("Share Screen")
    };
    let share_audio_label = if view.voice.screen_share_audio {
        l10n::tr("Audio On")
    } else {
        l10n::tr("Audio Off")
    };

    Some(
        div()
            .id("voice-panel")
            .role(Role::Region)
            .aria_label(l10n::tr("Voice connection"))
            .debug_selector(|| "voice-panel".to_owned())
            .flex()
            .flex_col()
            .flex_none()
            .gap_1()
            .px_2()
            .py_2()
            .border_t_1()
            .border_color(theme.border)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(status_color)
                                    .child(status_text),
                            )
                            .when(!channel_label.is_empty(), |d| {
                                d.child(
                                    div()
                                        .truncate()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(channel_label),
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
                            }),
                    )
                    .child(
                        div().debug_selector(|| "voice-disconnect-btn".to_owned()).child(
                            Button::new("voice-disconnect-btn")
                                .ghost()
                                .xsmall()
                                .label(l10n::tr("Disconnect"))
                                .accessibility_label(l10n::tr("Disconnect"))
                                .on_click(cx.listener(|this, _, _, cx| this.leave_voice_channel(cx))),
                        ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .items_center()
                    .gap_1()
                    .child(
                        div().debug_selector(|| "voice-mute-btn".to_owned()).child(
                            Button::new("voice-mute-btn")
                                .ghost()
                                .xsmall()
                                .label(mute_label.clone())
                                .accessibility_label(mute_label)
                                .on_click(cx.listener(|this, _, _, cx| this.toggle_voice_mute(cx))),
                        ),
                    )
                    .child(
                        div().debug_selector(|| "voice-deaf-btn".to_owned()).child(
                            Button::new("voice-deaf-btn")
                                .ghost()
                                .xsmall()
                                .label(deaf_label.clone())
                                .accessibility_label(deaf_label)
                                .on_click(cx.listener(|this, _, _, cx| this.toggle_voice_deaf(cx))),
                        ),
                    )
                    .when(is_connected, |d| {
                        d.child(
                            div().debug_selector(|| "voice-share-btn".to_owned()).child(
                                Button::new("voice-share-btn")
                                    .ghost()
                                    .xsmall()
                                    .label(share_label.clone())
                                    .accessibility_label(share_label)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.toggle_screen_share(false, cx);
                                    })),
                            ),
                        )
                    })
                    .when(is_connected && view.voice.self_stream, |d| {
                        d.child(
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
                    }),
            ),
    )
}
