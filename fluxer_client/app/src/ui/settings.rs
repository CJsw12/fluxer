//! Minimal settings modal (`UserSettingsModal.tsx`): theme (Dark, Light,
//! System, Dark Legacy, Coal), desktop notifications & sounds, 44 languages,
//! and account logout with confirmation.

use fluxer_client_core::rest::UserSettingsPatch;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Escape;
use gpui_kit::component::switch::Switch;
use gpui_kit::{AnyElement, Context, FontWeight, ScrollHandle, Window, div, prelude::*, px};

use crate::client_view::ClientView;
use crate::settings_model::{LOCALES, SettingsEffect, THEME_OPTIONS, theme_patch_value};
use crate::theme::{self, Theme, ThemeKind};
use crate::ui::overlay::Overlay;
use crate::ui::{l10n, overlay};

pub const CONTEXT: &str = "SettingsModal";

pub struct SettingsModal {
    pub confirm_logout: bool,
    pub locale_scroll: ScrollHandle,
}

impl ClientView {
    pub(crate) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.autocomplete = None;
        if matches!(self.overlay, Overlay::Settings(_)) {
            self.close_overlay(window, cx);
            return;
        }
        self.prepare_overlay_replace(cx);
        self.overlay = Overlay::Settings(SettingsModal {
            confirm_logout: false,
            locale_scroll: ScrollHandle::new(),
        });
        cx.notify();
    }

    pub(crate) fn apply_settings_effect(&mut self, effect: SettingsEffect, window: &mut Window, cx: &mut Context<Self>) {
        match effect {
            SettingsEffect::ThemeChanged(kind) => self.settings_set_theme(kind, window, cx),
            SettingsEffect::LocaleChanged(code) => self.settings_set_locale(&code, cx),
            SettingsEffect::NotificationsChanged(enabled) => self.settings_set_notifications(enabled, cx),
            SettingsEffect::SoundsChanged(enabled) => self.settings_set_sounds(enabled, cx),
            SettingsEffect::AutostartChanged(enabled) => self.settings_set_autostart(enabled, cx),
            SettingsEffect::CloseToTrayChanged(enabled) => self.settings_set_close_to_tray(enabled, cx),
            SettingsEffect::LogOutRequested => {
                self.close_overlay(window, cx);
                self.log_out(cx);
            }
        }
    }

    pub(crate) fn settings_set_theme(&mut self, kind: ThemeKind, window: &mut Window, cx: &mut Context<Self>) {
        self.prefs.set_theme(kind);
        theme::set_kind(kind, cx);
        theme::apply(kind, window, cx);
        self.backend.update_user_settings(UserSettingsPatch {
            theme: Some(theme_patch_value(kind).to_owned()),
            ..Default::default()
        });
        cx.notify();
    }

    pub(crate) fn settings_set_notifications(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.prefs.set_notifications(enabled);
        self.notifier.enabled = enabled;
        cx.notify();
    }

    pub(crate) fn settings_set_sounds(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.prefs.set_sounds(enabled);
        self.notifier.sounds_enabled = enabled;
        cx.notify();
    }

    pub(crate) fn settings_set_autostart(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if let Err(err) = crate::platform::set_autostart_enabled(enabled) {
            tracing::warn!(error = %format!("{err:#}"), "could not update autostart setting");
            self.error = Some(l10n::tr("Something went wrong. Try again.").into());
            cx.notify();
            return;
        }
        self.prefs.set_autostart(enabled);
        cx.notify();
    }

    pub(crate) fn settings_set_close_to_tray(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.prefs.set_close_to_tray(enabled);
        crate::tray::set_close_to_tray(enabled);
        cx.notify();
    }

    pub(crate) fn settings_set_locale(&mut self, code: &str, cx: &mut Context<Self>) {
        let resolved = fluxer_client_i18n::resolve_locale(code);
        self.prefs.set_locale(resolved);
        l10n::set_locale(resolved);
        self.error = None;
        self.refresh_status_text();
        self.refresh(cx);
        self.backend.update_user_settings(UserSettingsPatch {
            locale: Some(resolved.to_owned()),
            ..Default::default()
        });
        cx.refresh_windows();
        cx.notify();
    }

    pub(crate) fn settings_prompt_logout(&mut self, confirm: bool, cx: &mut Context<Self>) {
        if let Overlay::Settings(modal) = &mut self.overlay {
            modal.confirm_logout = confirm;
            cx.notify();
        }
    }
}

fn section_heading(title: String, theme: &Theme) -> impl IntoElement {
    div()
        .text_xs()
        .font_weight(FontWeight::BOLD)
        .text_color(theme.muted_foreground)
        .child(title)
}

pub fn render(view: &ClientView, modal: &SettingsModal, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let active_theme = theme::kind(cx);
    let active_locale = l10n::current().locale();

    let theme_buttons = THEME_OPTIONS.iter().enumerate().map(|(ix, (kind, label))| {
        let kind = *kind;
        let selected = active_theme == kind;
        let mut btn = Button::new(("settings-theme", ix)).small().label(l10n::tr(label));
        btn = if selected { btn.primary() } else { btn.ghost() };
        btn.on_click(cx.listener(move |this, _, window, cx| {
            this.apply_settings_effect(SettingsEffect::ThemeChanged(kind), window, cx);
        }))
    });

    let notifications_row = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(div().text_sm().child(l10n::tr("Desktop notifications")))
        .child(
            Switch::new("settings-notifications")
                .checked(view.notifier.enabled)
                .on_change(cx.listener(|this, checked: &bool, window, cx| {
                    this.apply_settings_effect(SettingsEffect::NotificationsChanged(*checked), window, cx);
                })),
        );

    let sounds_row = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(div().text_sm().child(l10n::tr("Notification sounds")))
        .child(
            Switch::new("settings-sounds")
                .checked(view.notifier.sounds_enabled)
                .on_change(cx.listener(|this, checked: &bool, window, cx| {
                    this.apply_settings_effect(SettingsEffect::SoundsChanged(*checked), window, cx);
                })),
        );

    let autostart_row = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(div().text_sm().child(l10n::tr("Desktop startup")))
        .child(
            Switch::new("settings-autostart")
                .checked(view.prefs.data.autostart)
                .on_change(cx.listener(|this, checked: &bool, window, cx| {
                    this.apply_settings_effect(SettingsEffect::AutostartChanged(*checked), window, cx);
                })),
        );

    let close_to_tray_row = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(div().text_sm().child(l10n::tr("Minimize to tray")))
        .child(
            Switch::new("settings-close-to-tray")
                .checked(view.prefs.data.close_to_tray)
                .on_change(cx.listener(|this, checked: &bool, window, cx| {
                    this.apply_settings_effect(SettingsEffect::CloseToTrayChanged(*checked), window, cx);
                })),
        );

    let locale_list = div()
        .id("settings-locale-list")
        .max_h(px(140.))
        .overflow_y_scroll()
        .track_scroll(&modal.locale_scroll)
        .rounded(px(6.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .p_1()
        .flex()
        .flex_col()
        .gap(px(2.))
        .children(LOCALES.iter().enumerate().map(|(ix, opt)| {
            let selected = active_locale == opt.code;
            let code = opt.code;
            div()
                .id(("settings-locale", ix))
                .debug_selector(|| format!("settings-locale-{code}"))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .px_2()
                .py_1()
                .rounded(px(4.))
                .cursor_pointer()
                .when(selected, |d| d.bg(theme.accent))
                .hover(|s| s.bg(theme.muted))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.apply_settings_effect(SettingsEffect::LocaleChanged(code.to_owned()), window, cx);
                }))
                .child(div().text_sm().child(opt.native_name))
                .child(div().text_xs().text_color(theme.muted_foreground).child(opt.code))
        }));

    let account_section = if modal.confirm_logout {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .rounded(px(6.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(l10n::tr("Sign out?")))
            .child(div().text_xs().text_color(theme.muted_foreground).child(l10n::tr("You can sign back in at any time.")))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        div()
                            .debug_selector(|| "settings-logout-confirm".to_owned())
                            .child(
                                Button::new("settings-logout-confirm")
                                    .danger()
                                    .small()
                                    .label(l10n::tr("Sign out"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.apply_settings_effect(SettingsEffect::LogOutRequested, window, cx);
                                    })),
                            ),
                    )
                    .child(
                        Button::new("settings-logout-cancel")
                            .ghost()
                            .small()
                            .label(l10n::tr("Stay signed in"))
                            .on_click(cx.listener(|this, _, _, cx| this.settings_prompt_logout(false, cx))),
                    ),
            )
            .into_any_element()
    } else {
        Button::new("settings-logout")
            .danger()
            .small()
            .label(l10n::tr("Log out"))
            .on_click(cx.listener(|this, _, _, cx| this.settings_prompt_logout(true, cx)))
            .into_any_element()
    };

    let content = div()
        .id("settings-modal")
        .key_context(CONTEXT)
        .p_4()
        .flex()
        .flex_col()
        .gap_4()
        .capture_action(cx.listener(|this, _: &Escape, window, cx| {
            cx.stop_propagation();
            this.close_overlay(window, cx);
        }))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(div().text_lg().font_weight(FontWeight::BOLD).child(l10n::tr("Settings")))
                .child(
                    Button::new("settings-close")
                        .ghost()
                        .small()
                        .label("✕")
                        .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Look and feel"), theme))
                .child(div().flex().flex_row().flex_wrap().gap_2().children(theme_buttons)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Notifications"), theme))
                .child(notifications_row)
                .child(sounds_row),
        )
        .when(cfg!(any(target_os = "windows", test)), |d| {
            d.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(section_heading(l10n::tr("Startup"), theme))
                    .child(autostart_row)
                    .child(close_to_tray_row),
            )
        })
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Language"), theme))
                .child(locale_list),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Account"), theme))
                .child(account_section),
        );

    let view = cx.weak_entity();
    overlay::modal(content, theme, move |window, cx| {
        let _ = view.update(cx, |v, cx| v.close_overlay(window, cx));
    })
}
