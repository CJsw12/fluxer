//! Bottom-left unified `UserArea` card (`UserArea.tsx` + `UserArea.module.css`):
//! wraps the optional top `voice_panel` (`.voiceConnectionWrapper`), the dedicated
//! inline `UserAreaPopout` menu (`user-area-popout`), and the bottom `.userAreaContainer`
//! (32px `StatusAwareAvatar`, display name + status/tag, and 32×32 Mic Mute /
//! Speaker Deafen / Settings control buttons with official `.controlButton.active`
//! danger highlight and moderator/permission lock state).

use fluxer_client_core::state::{Snowflake, Status};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, ClipboardItem, Context, FontWeight, Hsla, MouseButton, ObjectFit, Role, SharedString, deferred, div, img,
    prelude::*, px, rgb, rgba,
};

use super::avatar::{AvatarSpec, render_avatar, render_avatar_tagged};
use super::member_list::{status_color, status_label};
use super::settings::SettingsTab;
use crate::profile::build_profile_card;
use super::{l10n, rail_tooltip, voice_panel};
use crate::client_view::{ClientView, Connection, UserControlHoverTarget};
use crate::icons::AppIcon;
use crate::parity_probe::FlxExt as _;
use crate::theme::Theme;

#[allow(clippy::too_many_arguments)]
fn render_control_icon_button(
    id: &'static str,
    icon: AppIcon,
    label: String,
    tooltip_keys: &'static [&'static str],
    active_danger: bool,
    locked: bool,
    hovered: bool,
    theme: &Theme,
    on_hover: impl Fn(&bool, &mut gpui_kit::Window, &mut gpui_kit::App) + 'static,
    on_click: impl Fn(&gpui_kit::ClickEvent, &mut gpui_kit::Window, &mut gpui_kit::App) + 'static,
    on_right_click: impl Fn(&gpui_kit::MouseDownEvent, &mut gpui_kit::Window, &mut gpui_kit::App) + 'static,
) -> AnyElement {
    let mut danger_bg = theme.danger;
    danger_bg.a = 0.10;
    let mut danger_hover_bg = theme.danger;
    danger_hover_bg.a = 0.16;
    let icon_color = if active_danger {
        theme.danger
    } else if hovered && !locked {
        theme.foreground
    } else {
        theme.muted_foreground
    };

    let flx = match id {
        "user-panel-mute" => Some("app.user-area.user-area-inner.control-button.mute"),
        "user-panel-deafen" => Some("app.user-area.user-area-inner.control-button.deafen"),
        "user-panel-settings" => Some("app.user-area.user-area-inner.control-button.settings-click"),
        _ => None,
    };
    let icon_flx = match id {
        "user-panel-mute" => Some("app.user-area.user-area-inner.control-icon"),
        "user-panel-deafen" => Some("app.user-area.user-area-inner.control-icon--4"),
        "user-panel-settings" => Some("app.user-area.user-area-inner.control-icon--5"),
        _ => None,
    };
    let tooltip_flx = match id {
        "user-panel-mute" => Some("app.user-area.user-area-inner.controls-container"),
        "user-panel-deafen" => Some("app.user-area.user-area-inner.control-icon--2"),
        "user-panel-settings" => Some("app.user-area.user-area-inner.tooltip-with-keybind--3"),
        _ => None,
    };
    let tip_text = label.clone();
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .debug_selector(move || id.to_owned())
        .when_some(flx, |d, f| d.flx(f))
        .relative()
        .size(px(32.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .when(active_danger, |d| d.bg(danger_bg))
        .when(locked, |d| d.opacity(0.5))
        .on_hover(on_hover)
        .on_mouse_down(MouseButton::Right, on_right_click)
        .when(!locked, |d| {
            d.cursor_pointer()
                .hover(move |s| {
                    if active_danger {
                        s.bg(danger_hover_bg)
                    } else {
                        s.bg(theme.control_hover_bg)
                    }
                })
                .on_click(on_click)
        })
        .child(
            div()
                .flex_none()
                .size(px(20.))
                .when_some(icon_flx, |d, f| d.flx(f))
                .child(Icon::new(icon).with_size(px(20.)).text_color(icon_color)),
        )
        .when(hovered, |d| {
            let tt = rail_tooltip::above(tip_text, tooltip_keys, theme);
            match tooltip_flx {
                Some(tf) => d.child(div().flx(tf).child(tt)),
                None => d.child(tt),
            }
        })
        .into_any_element()
}

pub fn render(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> impl IntoElement {
    let sizes = theme.sizes;
    let (dot, label): ((Status, Hsla), SharedString) = match view.connection {
        Connection::Online => (
            (view.own_status, status_color(view.own_status, theme)),
            status_label(view.own_status).into(),
        ),
        Connection::Connecting => ((Status::Idle, theme.status_connecting), view.status.clone()),
        Connection::Offline => ((Status::Offline, theme.status_offline), l10n::tr("Offline").into()),
    };
    let (picture, name) = match &view.user {
        Some(user) => (
            render_avatar_tagged(
                AvatarSpec {
                    user_id: user.id,
                    avatar_url: user.avatar_url.clone(),
                    display_name: user.name.clone(),
                    size: sizes.user_panel_avatar,
                    status: None,
                    status_border_bg: None,
                },
                theme,
                &["ui.base-avatar.image"],
            ),
            user.name.clone(),
        ),
        None => (
            div().size(px(sizes.user_panel_avatar)).rounded_full().bg(theme.muted).into_any_element(),
            SharedString::new_static("…"),
        ),
    };
    let self_id = view.user.as_ref().map(|u| u.id);
    let (dot_size, cut_r, cut_c) = super::avatar::status_geometry(sizes.user_panel_avatar);
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
        l10n::tr("Undeafen yourself")
    } else {
        l10n::tr("Deafen yourself")
    };


    // Only roll the status line on hover when a custom status is active; swapping
    // "Online" for the username exceeds the 150px phantom-hover guard (174px).
    let has_custom_status = false;
    let hover_username = {
        let state = view.backend.state();
        state
            .current_user
            .as_ref()
            .map(|u| u.username.clone())
            .unwrap_or_else(|| name.to_string())
    };
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
        // Official card: srgb(0.124 0.116 0.142) = `--background-secondary-lighter`, no border.
        .bg(theme.background_secondary_lighter)
        .overflow_hidden()
        .children(voice_panel::render(view, theme, cx))
        .child(
            div()
                .id("user-panel")
                .debug_selector(|| "user-panel".to_owned())
                .flx("app.user-area.user-area-inner.user-area-container")
                .min_h(px(58.))
                .flex()
                .flex_row()
                .flex_none()
                .items_center()
                .pl(px(14.))
                .pr(px(8.))
                .gap(px(12.))
                .child(
                    div()
                        .id("user-panel-profile")
                        .group("user-panel-profile-group")
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
                                    .left(px(cut_c - cut_r))
                                    .top(px(cut_c - cut_r))
                                    .size(px(cut_r * 2.))
                                    .rounded_full()
                                    .bg(theme.background_secondary_lighter)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        div()
                                            .size(px(dot_size))
                                            .flex_none()
                                            .flx("ui.base-avatar.status-indicator-svg.svg")
                                            .flx("ui.base-avatar.status-indicator-svg.rect")
                                            .child(super::status_indicator::status_indicator(dot.0, dot_size, dot.1)),
                                    ),
                            ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .flx("app.user-area.user-area-inner.user-info-text")
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
                                        .relative()
                                        .flex()
                                        .flex_row()
                                        .w_full()
                                        .min_w_0()
                                        .flx_text("app.user-area.user-area-inner.user-status", label.to_string())
                                        .child(
                                            div()
                                                .truncate()
                                                .text_size(px(11.))
                                                .line_height(px(14.))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(status_color_text)
                                                .when(has_custom_status, |d| {
                                                    d.group_hover("user-panel-profile-group", |s| s.opacity(0.))
                                                })
                                                .flx("app.user-area.user-area-inner.user-status-label")
                                                .child(label),
                                        )
                                        .when(has_custom_status, |d| {
                                            d.child(
                                                div()
                                                    .absolute()
                                                    .inset_0()
                                                    .truncate()
                                                    .text_size(px(11.))
                                                    .line_height(px(14.))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(status_color_text)
                                                    .opacity(0.)
                                                    .group_hover("user-panel-profile-group", |s| s.opacity(1.))
                                                    .child(hover_username),
                                            )
                                        }),
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
                        .child(render_control_icon_button(
                            "user-panel-mute",
                            if effective_muted {
                                AppIcon::MicrophoneSlash
                            } else {
                                AppIcon::Microphone
                            },
                            mic_label,
                            &["CTRL", "SHIFT", "M"],
                            effective_muted,
                            mute_toggle_locked,
                            view.hovered_user_control == Some(UserControlHoverTarget::Mute),
                            theme,
                            cx.listener(|this, hovered: &bool, _, cx| {
                                this.set_user_control_hover(UserControlHoverTarget::Mute, *hovered, cx);
                            }),
                            cx.listener(|this, _, _, cx| this.toggle_voice_mute(cx)),
                            cx.listener(|this, _ev: &gpui_kit::MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                // TODO(backlog): open VoiceAudioSettingsMenu (input)
                                this.close_overlay(window, cx);
                            }),
                        ))
                        .child(render_control_icon_button(
                            "user-panel-deafen",
                            if effective_deafened {
                                AppIcon::SpeakerSlash
                            } else {
                                AppIcon::SpeakerHigh
                            },
                            speaker_label,
                            &["CTRL", "SHIFT", "D"],
                            effective_deafened,
                            deaf_toggle_locked,
                            view.hovered_user_control == Some(UserControlHoverTarget::Deafen),
                            theme,
                            cx.listener(|this, hovered: &bool, _, cx| {
                                this.set_user_control_hover(UserControlHoverTarget::Deafen, *hovered, cx);
                            }),
                            cx.listener(|this, _, _, cx| this.toggle_voice_deaf(cx)),
                            cx.listener(|this, _ev: &gpui_kit::MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                // TODO(backlog): open VoiceAudioSettingsMenu (output)
                                this.close_overlay(window, cx);
                            }),
                        ))
                        .child(render_control_icon_button(
                            "user-panel-settings",
                            AppIcon::Gear,
                            l10n::tr("User settings"),
                            &["CTRL", ","],
                            false,
                            false,
                            view.hovered_user_control == Some(UserControlHoverTarget::Settings),
                            theme,
                            cx.listener(|this, hovered: &bool, _, cx| {
                                this.set_user_control_hover(UserControlHoverTarget::Settings, *hovered, cx);
                            }),
                            cx.listener(|this, _, window, cx| this.open_settings(window, cx)),
                            cx.listener(|this, _ev: &gpui_kit::MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                // TODO(backlog): open SettingsContextMenu
                                this.close_overlay(window, cx);
                            }),
                        )),
                ),
        )
}

/// Submenu opened by hovering a `UserAreaPopout` action row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UserAreaSubmenu {
    Status,
    Expiry(Status),
    Accounts,
}

const CARD_BG: u32 = 0x0c0b0e;
const GROUP_BG: u32 = 0x232028;
const GROUP_DIVIDER: u32 = 0x2f2c36;
const MENU_BG: u32 = 0x2a2730;
const SECONDARY_TEXT: u32 = 0xb1acbb;
const ICON_TEXT: u32 = 0xe5e4e9;
const ACTIVE_GREEN: u32 = 0x1daf6b;
const BRAND_FILL: u32 = 0x413cdd;

const EXPIRY_CHOICES: [&str; 9] = [
    "For 15 minutes",
    "For 30 minutes",
    "For 1 hour",
    "For 3 hours",
    "For 4 hours",
    "For 8 hours",
    "For 24 hours",
    "For 3 days",
    "Until I change it",
];

fn menu_frame(id: &'static str, width: f32) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .occlude()
        .w(px(width))
        .p(px(6.))
        .flex()
        .flex_col()
        .rounded(px(8.))
        .border_1()
        .border_color(rgba(0xc9c5d326))
        .bg(rgb(MENU_BG))
        .shadow_lg()
}

fn hover_submenu(
    target: Option<UserAreaSubmenu>,
    cx: &mut Context<ClientView>,
) -> impl Fn(&bool, &mut gpui_kit::Window, &mut gpui_kit::App) + 'static {
    cx.listener(move |this, hovered: &bool, _, cx| {
        if *hovered && this.user_area_submenu != target {
            this.user_area_submenu = target;
            cx.notify();
        }
    })
}

fn hover_fill() -> Hsla {
    gpui_kit::hsla(0., 0., 1., 0.06)
}

/// Official `UserAreaPopout`: profile card (banner, avatar, names, custom status
/// row, action group, Edit profile) plus the hover submenus on its right.
pub fn render_popout(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> Option<AnyElement> {
    if !view.user_area_popout_open {
        return None;
    }
    let active_saved = view.saved_accounts.iter().find(|a| a.is_active);
    let uid = view
        .user
        .as_ref()
        .map(|u| u.id)
        .or_else(|| active_saved.and_then(|a| a.user_id.parse::<u64>().ok()).map(Snowflake))
        .unwrap_or(Snowflake(0));
    let mut card = {
        let state = view.backend.state();
        build_profile_card(&state, view.backend.media(), uid, None, view.profile_cache.get(&(uid, None)))
    };
    if view.user.is_none()
        && let Some(acct) = active_saved
    {
        card.display_name = acct.display_name.to_string();
        card.avatar_url = acct.avatar_url.as_ref().map(ToString::to_string);
    }
    let banner_color = rgb(card.banner_color);
    let status = view.own_status;
    let secondary: Hsla = rgb(SECONDARY_TEXT).into();
    let primary = theme.foreground;
    let icon_color: Hsla = rgb(ICON_TEXT).into();
    let username = SharedString::from(card.username_tag.clone());
    let copy_name = card.username_tag.clone();
    let user_id_text = uid.0.to_string();
    let submenu = view.user_area_submenu;

    let banner = div()
        .relative()
        .w_full()
        .h(px(105.))
        .flex_none()
        .rounded_t(px(3.5))
        .overflow_hidden()
        .bg(banner_color)
        .flx("user.profile.profile-card.profile-card-banner.banner")
        .children(
            card.banner_url
                .clone()
                .map(|url| img(url).id("user-area-popout-banner").size_full().object_fit(ObjectFit::Cover)),
        );

    let avatar = div()
        .id("user-area-popout-avatar")
        .absolute()
        .left(px(10.))
        .top(px(55.))
        .size(px(86.))
        .rounded_full()
        .bg(rgb(CARD_BG))
        .cursor_pointer()
        .flx("user.profile.profile-card.profile-card-banner.avatar-button.avatar-click")
        .on_click(cx.listener(move |this, ev: &gpui_kit::ClickEvent, window, cx| {
            this.user_area_popout_open = false;
            this.open_profile(uid, None, ev.position(), window, cx);
        }))
        .child(div().absolute().left(px(3.)).top(px(3.)).child(render_avatar(
            AvatarSpec {
                user_id: uid,
                avatar_url: card.avatar_url.clone().map(SharedString::from),
                display_name: card.display_name.clone().into(),
                size: 80.,
                status: None,
                status_border_bg: None,
            },
            theme,
        )))
        .child(
            div()
                .absolute()
                .right(px(0.))
                .bottom(px(0.))
                .size(px(26.))
                .rounded_full()
                .bg(rgb(CARD_BG))
                .flex()
                .items_center()
                .justify_center()
                .child(super::status_indicator::themed(status, 18., theme)),
        );

    let content = div()
        .mt(px(39.))
        .h(px(76.))
        .flex_none()
        .px(px(16.))
        .pt(px(8.))
        .flex()
        .flex_col()
        .gap(px(8.))
        .flx("user.profile.profile-card.profile-card-content.content-section")
        .child(
            div()
                .h(px(44.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .id("user-area-popout-display-name")
                        .h(px(24.))
                        .text_size(px(20.))
                        .line_height(px(24.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(primary)
                        .truncate()
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, ev: &gpui_kit::ClickEvent, window, cx| {
                            this.user_area_popout_open = false;
                            this.open_profile(uid, None, ev.position(), window, cx);
                        }))
                        .child(SharedString::from(card.display_name.clone())),
                )
                .child(
                    div()
                        .id("user-area-popout-username-row")
                        .group("user-area-popout-username")
                        .h(px(20.))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(4.))
                        .child(div().text_size(px(14.)).line_height(px(18.)).text_color(secondary).child(username))
                        .child(
                            div()
                                .id("user-area-copy-username")
                                .size(px(20.))
                                .flex_none()
                                .rounded(px(4.))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .opacity(0.)
                                .group_hover("user-area-popout-username", |s| s.opacity(1.))
                                .hover(|s| s.bg(theme.hover_bg))
                                .flx("app.floating.user-area-popout.copy-username-button.copy-user-tag")
                                .flx("app.floating.user-area-popout.tooltip")
                                .tooltip(move |window, cx| Tooltip::new(l10n::tr("Copy username")).build(window, cx))
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(copy_name.clone()));
                                }))
                                .child(
                                    div()
                                        .size(px(14.))
                                        .flex_none()
                                        .flx("app.floating.user-area-popout.copy-icon")
                                        .child(Icon::new(AppIcon::Copy).with_size(px(14.)).text_color(secondary)),
                                ),
                        ),
                ),
        )
        .child(
            div()
                .id("user-area-custom-status")
                .h(px(16.))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(3.))
                .flx("app.floating.user-area-popout.custom-status-row")
                .child(
                    div()
                        .size(px(14.))
                        .flex_none()
                        .flx("app.floating.user-area-popout.custom-status-placeholder-icon")
                        .child(Icon::new(AppIcon::SmileyRegular).with_size(px(14.)).text_color(secondary)),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(secondary)
                        .flx("app.floating.user-area-popout.custom-status-placeholder-text")
                        .child(l10n::tr("Set a custom status")),
                ),
        );

    let action_row = |id: &'static str, flx: &'static str| {
        div()
            .id(id)
            .debug_selector(move || id.to_owned())
            .flx(flx)
            .h(px(42.))
            .w_full()
            .flex_none()
            .px(px(12.))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(10.))
            .cursor_pointer()
            .hover(|s| s.bg(gpui_kit::hsla(0., 0., 1., 0.04)))
    };
    let action_label = |text: String| {
        div()
            .flex_1()
            .text_size(px(13.))
            .line_height(px(16.25))
            .font_weight(FontWeight::MEDIUM)
            .text_color(primary)
            .flx("app.floating.user-area-popout.action-button.action-label")
            .child(text)
    };
    let chevron = |color: Hsla| {
        div()
            .size(px(14.))
            .flex_none()
            .flx("app.floating.user-area-popout.action-button.action-chevron")
            .child(Icon::new(AppIcon::CaretRight).with_size(px(14.)).text_color(color))
    };
    let divider = |id: &'static str| div().mx(px(12.)).h(px(1.)).flex_none().bg(rgb(GROUP_DIVIDER)).flx(id);
    let icon_box =
        |child: AnyElement| div().size(px(22.)).flex_none().flex().items_center().justify_center().child(child);

    let action_group = div()
        .w_full()
        .flex_none()
        .flex()
        .flex_col()
        .rounded(px(12.))
        .overflow_hidden()
        .bg(rgb(GROUP_BG))
        .flx("app.floating.user-area-popout.action-group")
        .child(
            action_row("user-area-status-row", "app.floating.user-area-popout.action-button")
                .on_hover(hover_submenu(Some(UserAreaSubmenu::Status), cx))
                .child(icon_box(super::status_indicator::themed(status, 14., theme).into_any_element()))
                .child(action_label(status_label(status)))
                .child(chevron(secondary)),
        )
        .child(divider("app.floating.user-area-popout.action-divider"))
        .child(
            action_row(
                "user-area-switch-accounts",
                "app.floating.user-area-popout.action-button.open-manage-accounts",
            )
            .on_hover(hover_submenu(Some(UserAreaSubmenu::Accounts), cx))
            .on_click(cx.listener(|this, _, window, cx| {
                this.user_area_popout_open = false;
                this.open_account_switcher(window, cx);
            }))
            .child(icon_box(
                div()
                    .size(px(16.))
                    .flx("app.floating.user-area-popout.users-three-icon")
                    .child(Icon::new(AppIcon::UsersThree).with_size(px(16.)).text_color(icon_color))
                    .into_any_element(),
            ))
            .child(action_label(l10n::tr("Switch accounts")))
            .child(chevron(secondary)),
        )
        .child(divider("app.floating.user-area-popout.action-divider--2"))
        .child(
            action_row("user-area-copy-user-id", "app.floating.user-area-popout.action-button.copy-user-id")
                .on_hover(hover_submenu(None, cx))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(user_id_text.clone()));
                    this.user_area_popout_open = false;
                    cx.notify();
                }))
                .child(icon_box(
                    div()
                        .size(px(16.))
                        .flx("app.floating.user-area-popout.identification-badge-icon")
                        .child(Icon::new(AppIcon::IdentificationBadge).with_size(px(16.)).text_color(icon_color))
                        .into_any_element(),
                ))
                .child(action_label(l10n::tr("Copy user ID"))),
        );

    let edit_button = div()
        .id("user-area-edit-profile")
        .debug_selector(|| "user-area-edit-profile".to_owned())
        .h(px(36.))
        .w_full()
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .justify_center()
        .gap(px(8.))
        .rounded(px(8.))
        .border_1()
        .border_color(theme.brand_button_border)
        .flx("app.floating.user-area-popout.edit-profile-button.open-edit-profile")
        .bg(rgb(BRAND_FILL))
        .cursor_pointer()
        .hover(|s| s.bg(theme.brand))
        .on_hover(hover_submenu(None, cx))
        .on_click(cx.listener(|this, _, window, cx| {
            this.user_area_popout_open = false;
            if !matches!(this.overlay, crate::ui::overlay::Overlay::Settings(_)) {
                this.open_settings(window, cx);
            }
            this.select_settings_tab(SettingsTab::Profile, cx);
        }))
        .child(
            div()
                .size(px(16.))
                .flex_none()
                .flx("app.floating.user-area-popout.pencil-icon")
                .child(Icon::new(AppIcon::PencilBold).with_size(px(16.)).text_color(theme.on_fill)),
        )
        .child(
            div()
                .text_size(px(13.))
                .line_height(px(16.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.on_fill)
                .child(l10n::tr("Edit profile")),
        );

    let footer = div()
        .h(px(192.))
        .flex_none()
        .mt(px(4.))
        .px(px(12.))
        .pt(px(16.))
        .flex()
        .flex_col()
        .flx("user.profile.profile-card.profile-card-footer.footer-section")
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .flx("app.floating.user-area-popout.footer")
                .child(action_group)
                .child(edit_button),
        );

    let card_el = div()
        .id("user-area-popout")
        .debug_selector(|| "user-area-popout".to_owned())
        .occlude()
        .w(px(305.))
        .h(px(432.))
        .relative()
        .flex()
        .flex_col()
        .rounded(px(6.))
        .border_2()
        .border_color(banner_color)
        .flx("app.floating.user-area-popout.container")
        .bg(rgb(CARD_BG))
        .child(banner)
        .child(content)
        .child(footer)
        .child(avatar);

    let status_menu = matches!(submenu, Some(UserAreaSubmenu::Status | UserAreaSubmenu::Expiry(_))).then(|| {
        let items = [
            (Status::Online, "online", None, 32.),
            (Status::Idle, "idle", None, 32.),
            (Status::Dnd, "dnd", Some("You won't receive notifications on desktop"), 63.),
            (Status::Invisible, "invisible", Some("You'll appear offline"), 47.),
        ];
        menu_frame("user-area-status-menu", 280.)
            .absolute()
            .left(px(315.))
            .top(px(242.))
            .gap(px(0.5))
            .flx("app.floating.user-area-popout.status-menu.status-menu")
            .children(items.into_iter().map(|(st, key, desc, h)| {
                let has_expiry = st != Status::Online;
                let sel_id = format!("user-area-status-{key}");
                let target = if has_expiry { UserAreaSubmenu::Expiry(st) } else { UserAreaSubmenu::Status };
                let active = submenu == Some(UserAreaSubmenu::Expiry(st));
                div()
                    .id(SharedString::from(sel_id.clone()))
                    .debug_selector(move || sel_id.clone())
                    .w_full()
                    .h(px(h))
                    .flex_none()
                    .px(px(8.))
                    .pt(px(5.))
                    .rounded(px(6.))
                    .flex()
                    .flex_row()
                    .items_start()
                    .gap(px(8.))
                    .cursor_pointer()
                    .when(active, |d| d.bg(hover_fill()))
                    .hover(|s| s.bg(hover_fill()))
                    .on_hover(hover_submenu(Some(target), cx))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.own_status = st;
                        this.backend.set_presence(st);
                        this.user_area_popout_open = false;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .size(px(18.))
                            .flex_none()
                            .mt(px(1.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(super::status_indicator::themed(st, 14., theme)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(primary)
                                    .child(status_label(st)),
                            )
                            .children(desc.map(|d| {
                                div().text_size(px(12.)).line_height(px(16.)).text_color(secondary).child(l10n::tr(d))
                            })),
                    )
                    .when(has_expiry, |d| {
                        d.child(
                            div()
                                .flex_none()
                                .mt(px(1.))
                                .child(Icon::new(AppIcon::CaretRight).with_size(px(16.)).text_color(icon_color)),
                        )
                    })
            }))
            .into_any_element()
    });

    let expiry_menu = match submenu {
        Some(UserAreaSubmenu::Expiry(st)) => {
            let item_top = match st {
                Status::Idle => 32.5,
                Status::Dnd => 65.,
                _ => 128.,
            };
            Some(
                menu_frame("user-area-expiry-menu", 220.)
                    .absolute()
                    .left(px(607.))
                    .top(px(242. + 7. + item_top))
                    .flx("app.floating.user-area-popout.status-menu.expiry-popup")
                    .children(EXPIRY_CHOICES.into_iter().enumerate().map(|(i, text)| {
                        div()
                            .id(("user-area-expiry", i))
                            .h(px(32.))
                            .w_full()
                            .px(px(8.))
                            .rounded(px(6.))
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .hover(|s| s.bg(hover_fill()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                // The backend has no status-expiry timer; the status applies indefinitely.
                                this.own_status = st;
                                this.backend.set_presence(st);
                                this.user_area_popout_open = false;
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(primary)
                                    .child(l10n::tr(text)),
                            )
                    }))
                    .into_any_element(),
            )
        }
        _ => None,
    };

    let account_menu = (submenu == Some(UserAreaSubmenu::Accounts)).then(|| {
        let mut rows: Vec<(String, SharedString, Option<SharedString>, Snowflake, bool)> = view
            .saved_accounts
            .iter()
            .map(|a| {
                (
                    a.key.clone(),
                    a.display_name.clone(),
                    a.avatar_url.clone(),
                    Snowflake(a.user_id.parse::<u64>().unwrap_or(0)),
                    a.is_active,
                )
            })
            .collect();
        if rows.is_empty() {
            rows.push((
                String::new(),
                card.display_name.clone().into(),
                card.avatar_url.clone().map(SharedString::from),
                uid,
                true,
            ));
        }
        menu_frame("user-area-account-menu", 280.)
            .absolute()
            .left(px(315.))
            .top(px(285.))
            .flx("app.floating.user-area-account-switch-menu.switch-menu")
            .children(rows.into_iter().enumerate().map(|(i, (key, name, avatar_url, id, active))| {
                div()
                    .id(("user-area-account", i))
                    .w_full()
                    .h(px(44.))
                    .flex_none()
                    .px(px(8.))
                    .rounded(px(6.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .cursor_pointer()
                    .hover(|s| s.bg(hover_fill()))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.user_area_popout_open = false;
                        if !active && !key.is_empty() {
                            this.switch_account(key.clone(), window, cx);
                        }
                        cx.notify();
                    }))
                    .child(render_avatar(
                        AvatarSpec {
                            user_id: id,
                            avatar_url,
                            display_name: name.clone(),
                            size: 24.,
                            status: None,
                            status_border_bg: None,
                        },
                        theme,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(primary)
                                    .truncate()
                                    .child(name),
                            )
                            .when(active, |d| {
                                d.child(
                                    div()
                                        .text_size(px(11.))
                                        .line_height(px(14.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(rgb(ACTIVE_GREEN))
                                        .child(l10n::tr("Active account")),
                                )
                            }),
                    )
                    .when(active, |d| {
                        d.child(
                            div()
                                .size(px(20.))
                                .flex_none()
                                .rounded_full()
                                .bg(rgb(BRAND_FILL))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(Icon::new(AppIcon::Check).with_size(px(10.)).text_color(theme.on_fill)),
                        )
                    })
            }))
            .child(div().my(px(4.)).h(px(1.)).w_full().flex_none().bg(rgba(0xc9c5d326)))
            .child(
                div()
                    .id("user-area-manage-accounts")
                    .debug_selector(|| "user-area-manage-accounts".to_owned())
                    .w_full()
                    .h(px(32.))
                    .px(px(8.))
                    .rounded(px(6.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .cursor_pointer()
                    .hover(|s| s.bg(hover_fill()))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.user_area_popout_open = false;
                        this.open_account_switcher(window, cx);
                    }))
                    .child(Icon::new(AppIcon::Gear).with_size(px(16.)).text_color(icon_color))
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(px(16.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(primary)
                            .child(l10n::tr("Manage accounts")),
                    ),
            )
            .into_any_element()
    });

    Some(
        deferred(
            div()
                .absolute()
                .left(px(12.))
                .bottom(px(64.))
                .w(px(305.))
                .h(px(432.))
                .child(card_el)
                .children(status_menu)
                .children(expiry_menu)
                .children(account_menu),
        )
        .with_priority(2)
        .into_any_element(),
    )
}
