//! Bottom-left unified `UserArea` card (`UserArea.tsx` + `UserArea.module.css`):
//! wraps the optional top `voice_panel` (`.voiceConnectionWrapper`), the dedicated
//! inline `UserAreaPopout` menu (`user-area-popout`), and the bottom `.userAreaContainer`
//! (32px `StatusAwareAvatar`, display name + status/tag, and 32×32 Mic Mute /
//! Speaker Deafen / Settings control buttons with official `.controlButton.active`
//! danger highlight and moderator/permission lock state).

use fluxer_client_core::state::Status;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{AnyElement, Context, FontWeight, MouseButton, Role, SharedString, div, prelude::*, px};

use super::avatar::{AvatarSpec, render_avatar};
use super::member_list::{status_color, status_label};
use super::settings::SettingsTab;
use super::{l10n, voice_panel};
use crate::client_view::{ClientView, Connection};
use crate::icons::AppIcon;
use crate::parity_probe::FlxExt as _;
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

    let flx = match id {
        "user-panel-mute" => Some("app.user-area.user-area-inner.control-button.mute"),
        "user-panel-deafen" => Some("app.user-area.user-area-inner.control-button.deafen"),
        "user-panel-settings" => Some("app.user-area.user-area-inner.control-button.settings-click"),
        _ => None,
    };
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .debug_selector(move || id.to_owned())
        .when_some(flx, |d, f| d.flx(f))
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
        .child(Icon::new(icon).with_size(px(20.)).text_color(icon_color))
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

    let popout = view.user_area_popout_open.then(|| {
        let status_choices = [
            (Status::Online, "online"),
            (Status::Idle, "idle"),
            (Status::Dnd, "dnd"),
            (Status::Invisible, "invisible"),
        ];
        div()
            .id("user-area-popout")
            .debug_selector(|| "user-area-popout".to_owned())
            .px_2p5()
            .py_2()
            .border_b_1()
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
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme.muted_foreground)
                            .child(l10n::tr("Set Status")),
                    )
                    .when_some(self_id, |d, uid| {
                        d.child(
                            div()
                                .id("user-area-view-profile")
                                .debug_selector(|| "user-area-view-profile".to_owned())
                                .cursor_pointer()
                                .text_xs()
                                .text_color(theme.brand)
                                .hover(|s| s.underline())
                                .on_click(cx.listener(move |this, ev: &gpui_kit::ClickEvent, window, cx| {
                                    this.user_area_popout_open = false;
                                    this.open_profile(uid, None, ev.position(), window, cx);
                                }))
                                .child(l10n::tr("View profile")),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .children(status_choices.into_iter().map(|(st, key)| {
                        let selected = view.own_status == st;
                        let st_color = status_color(st, theme);
                        let st_label = status_label(st);
                        let sel_id = format!("user-area-status-{key}");
                        div()
                            .id(SharedString::from(sel_id.clone()))
                            .debug_selector(move || sel_id.clone())
                            .px_2()
                            .py_1()
                            .rounded(px(5.))
                            .cursor_pointer()
                            .when(selected, |d| d.bg(theme.accent).font_weight(FontWeight::SEMIBOLD))
                            .when(!selected, |d| d.hover(|s| s.bg(theme.muted)))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.own_status = st;
                                this.backend.set_presence(st);
                                this.user_area_popout_open = false;
                                cx.notify();
                            }))
                            .child(div().size(px(8.)).rounded_full().bg(st_color))
                            .child(div().text_xs().child(st_label))
                    })),
            )
            .child(div().h(px(1.)).bg(theme.border))
            .child(
                div()
                    .id("user-area-edit-profile")
                    .debug_selector(|| "user-area-edit-profile".to_owned())
                    .px_2()
                    .py_1()
                    .rounded(px(5.))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.muted))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.user_area_popout_open = false;
                        if !matches!(this.overlay, crate::ui::overlay::Overlay::Settings(_)) {
                            this.open_settings(window, cx);
                        }
                        this.select_settings_tab(SettingsTab::Profile, cx);
                    }))
                    .child(Icon::new(AppIcon::PencilSimple).xsmall().text_color(theme.muted_foreground))
                    .child(div().text_xs().font_weight(FontWeight::MEDIUM).child(l10n::tr("Edit Profile"))),
            )
            .when(view.accounts_enabled, |d| {
                d.child(
                    div()
                        .id("user-area-switch-accounts")
                        .debug_selector(|| "user-area-switch-accounts".to_owned())
                        .px_2()
                        .py_1()
                        .rounded(px(5.))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.muted))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.user_area_popout_open = false;
                            this.open_account_switcher(window, cx);
                        }))
                        .child(Icon::new(AppIcon::Users).xsmall().text_color(theme.muted_foreground))
                        .child(div().text_xs().font_weight(FontWeight::MEDIUM).child(l10n::tr("Switch Accounts"))),
                )
            })
    });

    let panel_group: SharedString = "user-panel-group".into();

    let mut status_color_text = theme.muted_foreground;
    status_color_text.a = 0.85;

    div()
        .id("user-area-wrapper")
        .role(Role::Region)
        .aria_label(l10n::tr("User controls"))
        .debug_selector(|| "user-area-wrapper".to_owned())
        .flx("app.user-area.user-area-inner.section")
        .flex()
        .flex_col()
        .flex_none()
        .rounded(px(8.))
        .bg(theme.background_secondary)
        .overflow_hidden()
        .children(voice_panel::render(view, theme, cx))
        .children(popout)
        .child(
            div()
                .id("user-panel")
                .group(panel_group.clone())
                .debug_selector(|| "user-panel".to_owned())
                .flx("app.user-area.user-area-inner.user-area-container")
                .min_h(px(58.))
                .flex()
                .flex_row()
                .flex_none()
                .items_center()
                .pl(px(14.))
                .pr(px(8.))
                .child(
                    div()
                        .id("user-panel-profile")
                        .debug_selector(|| "user-panel-profile".to_owned())
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .flex_1()
                        .min_w_0()
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, ev: &gpui_kit::ClickEvent, window, cx| {
                            if ev.modifiers().shift {
                                if let Some(id) = self_id {
                                    this.open_profile(id, None, ev.position(), window, cx);
                                }
                            } else {
                                this.toggle_user_area_popout(cx);
                            }
                        }))
                        .flx("app.user-area.user-area-inner.focus-ring-wrapper")
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
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.foreground)
                                        .flx_text("app.user-area.user-area-inner.user-name", name.to_string())
                                        .child(name),
                                )
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(11.))
                                        .line_height(px(14.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(status_color_text)
                                        .flx_text("app.user-area.user-area-inner.user-status", label.to_string())
                                        .child(label),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .flex_none()
                        .relative()
                        .pl(px(12.))
                        .flx("app.user-area.user-area-inner.controls-container")
                        .when(view.accounts_enabled, |d| {
                            d.child(
                                div()
                                    .debug_selector(|| "switch-accounts".to_owned())
                                    .absolute()
                                    .left(px(-36.))
                                    .opacity(0.)
                                    .group_hover(panel_group, |s| s.opacity(1.))
                                    .child(
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
                        .child(render_control_icon_button(
                            "user-panel-settings",
                            AppIcon::Gear,
                            l10n::tr("User settings"),
                            false,
                            false,
                            theme,
                            cx.listener(|this, _, window, cx| this.open_settings(window, cx)),
                        )),
                ),
        )
}
