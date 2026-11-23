//! Bottom-left unified `UserArea` card (`UserArea.tsx` + `UserArea.module.css`):
//! wraps the optional top `voice_panel` (`.voiceConnectionWrapper`) and the
//! bottom `.userAreaContainer` (32px `StatusAwareAvatar`, display name + status/tag,
//! and 32×32 Mic Mute / Speaker Deafen / Settings control buttons with official
//! `.controlButton.active` danger highlight and moderator/permission lock state).

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{AnyElement, Context, FontWeight, MouseButton, Role, SharedString, div, prelude::*, px};

use super::avatar::{AvatarSpec, render_avatar};
use super::member_list::{status_color, status_label};
use super::{l10n, voice_panel};
use crate::client_view::{ClientView, Connection};
use crate::icons::AppIcon;
use crate::theme::Theme;

fn render_control_icon_button(
    id: &'static str,
    icon: AppIcon,
    label: String,
    active_danger: bool,
    locked: bool,
    theme: &Theme,
    on_click: impl Fn(&gpui_kit::ClickEvent, &mut gpui_kit::Window, &mut gpui_kit::App) + 'static,
) -> AnyElement {
    let mut danger_bg = theme.danger;
    danger_bg.a = 0.12;
    let mut danger_hover_bg = theme.danger;
    danger_hover_bg.a = 0.20;
    let icon_color = if active_danger {
        theme.danger
    } else {
        theme.muted_foreground
    };

    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .debug_selector(move || id.to_owned())
        .size(px(32.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .when(active_danger, |d| d.bg(danger_bg))
        .when(locked, |d| d.opacity(0.5))
        .when(!locked, |d| {
            d.cursor_pointer()
                .hover(move |s| {
                    if active_danger {
                        s.bg(danger_hover_bg)
                    } else {
                        s.bg(theme.muted)
                    }
                })
                .on_click(on_click)
        })
        .child(Icon::new(icon).small().text_color(icon_color))
        .into_any_element()
}

pub fn render(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> impl IntoElement {
    let sizes = theme.sizes;
    let (dot, label): (_, SharedString) = match view.connection {
        Connection::Online => (status_color(view.own_status, theme), status_label(view.own_status).into()),
        Connection::Connecting => (theme.status_connecting, view.status.clone()),
        Connection::Offline => (theme.status_offline, l10n::tr("Offline").into()),
    };
    let (picture, name) = match &view.user {
        Some(user) => (
            render_avatar(
                AvatarSpec {
                    user_id: user.id,
                    avatar_url: user.avatar_url.clone(),
                    display_name: user.name.clone(),
                    size: sizes.user_panel_avatar,
                    status: None,
                    status_border_bg: None,
                },
                theme,
            ),
            user.name.clone(),
        ),
        None => (
            div().size(px(sizes.user_panel_avatar)).rounded_full().bg(theme.muted).into_any_element(),
            SharedString::new_static("…"),
        ),
    };
    let self_id = view.user.as_ref().map(|u| u.id);
    let dot_size = sizes.status_dot;
    let status_aria_label = format!("{}: {}", l10n::tr("Status"), label);

    let effective_muted = view.voice.effective_muted();
    let effective_deafened = view.voice.effective_deafened();
    let mute_toggle_locked = view.voice.mute_toggle_locked();
    let deaf_toggle_locked = view.voice.server_deaf;

    let mic_label = if view.voice.server_deaf {
        l10n::tr("Server deafened")
    } else if view.voice.server_mute {
        l10n::tr("Server muted")
    } else if view.voice.speak_denied {
        l10n::tr("Mic muted by permissions")
    } else if effective_muted {
        l10n::tr("Unmute microphone")
    } else {
        l10n::tr("Mute microphone")
    };

    let speaker_label = if view.voice.server_deaf {
        l10n::tr("Server deafened")
    } else if effective_deafened {
        l10n::tr("Undeafen")
    } else {
        l10n::tr("Deafen")
    };

    div()
        .id("user-area-wrapper")
        .role(Role::Region)
        .aria_label(l10n::tr("User controls"))
        .debug_selector(|| "user-area-wrapper".to_owned())
        .flex()
        .flex_col()
        .flex_none()
        .mx_2()
        .mb_2()
        .rounded(px(8.))
        .bg(theme.background_secondary)
        .border_1()
        .border_color(theme.border)
        .overflow_hidden()
        .children(voice_panel::render(view, theme, cx))
        .child(
            div()
                .id("user-panel")
                .debug_selector(|| "user-panel".to_owned())
                .flex()
                .flex_row()
                .flex_none()
                .items_center()
                .gap_2()
                .px_2()
                .py_1p5()
                .child(
                    div().relative().flex_none().child(picture).child(
                        div()
                            .id("user-status-dot")
                            .role(Role::Button)
                            .aria_label(status_aria_label)
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(|this, _, _, cx| {
                                cx.stop_propagation();
                                this.cycle_status(cx);
                            }))
                            .absolute()
                            .right(px(-1.))
                            .bottom(px(-1.))
                            .size(px(dot_size))
                            .rounded_full()
                            .border_2()
                            .border_color(theme.background_secondary)
                            .bg(dot),
                    ),
                )
                .child(
                    div()
                        .id("user-panel-profile")
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .when(self_id.is_some(), |d| d.cursor_pointer())
                        .on_click(cx.listener(move |this, ev: &gpui_kit::ClickEvent, window, cx| {
                            if let Some(id) = self_id
                                && !ev.modifiers().shift
                            {
                                this.open_profile(id, None, ev.position(), window, cx);
                            }
                        }))
                        .child(div().truncate().text_sm().font_weight(FontWeight::MEDIUM).child(name))
                        .child(div().truncate().text_xs().text_color(theme.muted_foreground).child(label)),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .flex_none()
                        .when(view.accounts_enabled, |d| {
                            d.child(
                                div().debug_selector(|| "switch-accounts".to_owned()).child(
                                    Button::new("switch-accounts")
                                        .ghost()
                                        .small()
                                        .icon(AppIcon::Users)
                                        .tooltip(l10n::tr("Switch accounts"))
                                        .accessibility_label(l10n::tr("Switch accounts"))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.open_account_switcher(window, cx);
                                        })),
                                ),
                            )
                        })
                        .child(render_control_icon_button(
                            "user-panel-mute",
                            if effective_muted {
                                AppIcon::MicrophoneSlash
                            } else {
                                AppIcon::Microphone
                            },
                            mic_label,
                            effective_muted,
                            mute_toggle_locked,
                            theme,
                            cx.listener(|this, _, _, cx| this.toggle_voice_mute(cx)),
                        ))
                        .child(render_control_icon_button(
                            "user-panel-deafen",
                            if effective_deafened {
                                AppIcon::SpeakerSlash
                            } else {
                                AppIcon::SpeakerHigh
                            },
                            speaker_label,
                            effective_deafened,
                            deaf_toggle_locked,
                            theme,
                            cx.listener(|this, _, _, cx| this.toggle_voice_deaf(cx)),
                        ))
                        .child(
                            Button::new("open-settings")
                                .ghost()
                                .small()
                                .icon(AppIcon::Gear)
                                .tooltip(l10n::tr("User settings"))
                                .accessibility_label(l10n::tr("User settings"))
                                .on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx))),
                        ),
                ),
        )
}
