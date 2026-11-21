//! Minimal settings modal (`UserSettingsModal.tsx`): theme (Dark, Light,
//! System, Dark Legacy, Coal), desktop notifications & sounds, 44 languages,
//! and account logout with confirmation.

use fluxer_client_core::rest::UserSettingsPatch;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Escape;
use gpui_kit::component::switch::Switch;
use gpui_kit::{AnyElement, Context, FontWeight, ScrollHandle, SharedString, Window, div, prelude::*, px};

use crate::backend::voice::VoiceAudioSettings;
use crate::client_view::ClientView;
use crate::prefs::{DEFAULT_VOICE_VOLUME, MAX_VOICE_VOLUME, MIN_VOICE_VOLUME, ReducedMotionPref};
use crate::settings_model::{FONT_SIZE_OPTIONS, LOCALES, SettingsEffect, THEME_OPTIONS, theme_patch_value};
use crate::theme::{self, MAX_FONT_SIZE, MAX_UI_ZOOM, MIN_FONT_SIZE, MIN_UI_ZOOM, Theme, ThemeKind};
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
        self.refresh_audio_devices(cx);
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
            SettingsEffect::FontSizeChanged(size) => self.settings_set_font_size(size, cx),
            SettingsEffect::ZoomChanged(zoom) => self.settings_set_zoom(zoom, cx),
            SettingsEffect::CompactChanged(compact) => self.settings_set_compact(compact, cx),
            SettingsEffect::VoiceInputDeviceChanged(device) => self.settings_set_voice_input_device(device, cx),
            SettingsEffect::VoiceOutputDeviceChanged(device) => self.settings_set_voice_output_device(device, cx),
            SettingsEffect::VoiceInputVolumeChanged(volume) => self.settings_set_voice_input_volume(volume, cx),
            SettingsEffect::VoiceOutputVolumeChanged(volume) => self.settings_set_voice_output_volume(volume, cx),
            SettingsEffect::ReducedMotionChanged(pref) => {
                self.prefs.set_reduced_motion(pref);
                cx.set_reduce_motion(crate::platform::resolve_reduced_motion(pref));
                cx.notify();
            }
            SettingsEffect::ClearTrustedDomains => self.settings_clear_trusted_domains(cx),
            SettingsEffect::OpenAccountSwitcher => self.open_account_switcher(window, cx),
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

    pub(crate) fn settings_set_font_size(&mut self, size: u8, cx: &mut Context<Self>) {
        let clamped = size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        self.prefs.set_font_size(clamped);
        theme::set_display(self.prefs.data.display(), cx);
        cx.notify();
    }

    pub(crate) fn settings_set_zoom(&mut self, zoom: u16, cx: &mut Context<Self>) {
        let clamped = zoom.clamp(MIN_UI_ZOOM, MAX_UI_ZOOM);
        self.prefs.set_ui_zoom(clamped);
        theme::set_display(self.prefs.data.display(), cx);
        cx.notify();
    }

    pub(crate) fn step_ui_zoom(&mut self, delta: i16, cx: &mut Context<Self>) {
        let current = i16::try_from(self.prefs.data.ui_zoom).unwrap_or(100);
        let next = (current + delta).clamp(MIN_UI_ZOOM as i16, MAX_UI_ZOOM as i16) as u16;
        self.settings_set_zoom(next, cx);
    }

    pub(crate) fn settings_set_compact(&mut self, compact: bool, cx: &mut Context<Self>) {
        self.prefs.set_compact(compact);
        theme::set_display(self.prefs.data.display(), cx);
        self.backend.update_user_settings(UserSettingsPatch {
            message_display_compact: Some(compact),
            ..Default::default()
        });
        cx.notify();
    }

    fn sync_voice_audio_settings(&self) {
        self.backend.set_voice_audio_settings(VoiceAudioSettings {
            input_device: self.prefs.data.voice_input_device.clone(),
            output_device: self.prefs.data.voice_output_device.clone(),
            input_volume: self.prefs.data.voice_input_volume,
            output_volume: self.prefs.data.voice_output_volume,
        });
    }

    pub(crate) fn settings_set_voice_input_device(&mut self, device: Option<String>, cx: &mut Context<Self>) {
        self.prefs.set_voice_input_device(device);
        self.sync_voice_audio_settings();
        cx.notify();
    }

    pub(crate) fn settings_set_voice_output_device(&mut self, device: Option<String>, cx: &mut Context<Self>) {
        self.prefs.set_voice_output_device(device);
        self.sync_voice_audio_settings();
        cx.notify();
    }

    pub(crate) fn settings_set_voice_input_volume(&mut self, volume: u16, cx: &mut Context<Self>) {
        self.prefs.set_voice_input_volume(volume);
        self.sync_voice_audio_settings();
        cx.notify();
    }

    pub(crate) fn settings_set_voice_output_volume(&mut self, volume: u16, cx: &mut Context<Self>) {
        self.prefs.set_voice_output_volume(volume);
        self.sync_voice_audio_settings();
        cx.notify();
    }

    pub(crate) fn settings_clear_trusted_domains(&mut self, cx: &mut Context<Self>) {
        self.prefs.clear_trusted_domains();
        self.backend.update_user_settings(UserSettingsPatch {
            trusted_domains: Some(Vec::new()),
            ..Default::default()
        });
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

fn voice_device_row(
    label: &str,
    device: Option<&str>,
    available_devices: &[String],
    id_prefix: &'static str,
    reset_id: &'static str,
    theme: &Theme,
    make_select: impl Fn(Option<String>) -> SettingsEffect + Copy + 'static,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let label_text = l10n::tr(label);
    let default_label = l10n::tr("System default");
    let display_name = device.map(str::to_owned).unwrap_or_else(|| default_label.clone());

    let default_selected = device.is_none();
    let default_sel_id = format!("{id_prefix}-default");
    let mut default_btn = Button::new(default_sel_id.clone())
        .small()
        .label(default_label.clone())
        .accessibility_label(format!("{label_text}: {default_label}"));
    default_btn = if default_selected {
        default_btn.primary()
    } else {
        default_btn.ghost()
    };
    let default_button = div().debug_selector(move || default_sel_id.clone()).child(
        default_btn.on_click(cx.listener(move |this, _, window, cx| {
            this.apply_settings_effect(make_select(None), window, cx);
        })),
    );

    let device_buttons = available_devices.iter().enumerate().map(|(ix, name)| {
        let selected = device == Some(name.as_str());
        let sel_id = format!("{id_prefix}-{ix}");
        let device_name = name.clone();
        let mut btn = Button::new((id_prefix, ix + 1))
            .small()
            .label(name.clone())
            .accessibility_label(format!("{label_text}: {name}"));
        btn = if selected { btn.primary() } else { btn.ghost() };
        div().debug_selector(move || sel_id.clone()).child(btn.on_click(cx.listener(
            move |this, _, window, cx| {
                this.apply_settings_effect(make_select(Some(device_name.clone())), window, cx);
            },
        )))
    });

    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_2()
                .child(div().text_sm().child(label_text.clone()))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(div().text_xs().text_color(theme.muted_foreground).child(display_name))
                        .when(device.is_some(), |d| {
                            d.child(
                                div().debug_selector(move || reset_id.to_owned()).child(
                                    Button::new(reset_id)
                                        .ghost()
                                        .small()
                                        .label(l10n::tr("Reset"))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.apply_settings_effect(make_select(None), window, cx);
                                        })),
                                ),
                            )
                        }),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap_1()
                .child(default_button)
                .children(device_buttons),
        )
        .into_any_element()
}

fn voice_volume_row(
    label: &str,
    volume: u16,
    dec_id: &'static str,
    inc_id: &'static str,
    reset_id: &'static str,
    make_effect: impl Fn(u16) -> SettingsEffect + Copy + 'static,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let clamped = volume.clamp(MIN_VOICE_VOLUME, MAX_VOICE_VOLUME);
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(div().text_sm().child(l10n::tr(label)))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .child(
                    div().debug_selector(move || dec_id.to_owned()).child(
                        Button::new(dec_id)
                            .ghost()
                            .small()
                            .label("-")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let next = clamped.saturating_sub(10).max(MIN_VOICE_VOLUME);
                                this.apply_settings_effect(make_effect(next), window, cx);
                            })),
                    ),
                )
                .child(div().px_2().text_xs().font_weight(FontWeight::SEMIBOLD).child(format!("{clamped}%")))
                .child(
                    div().debug_selector(move || inc_id.to_owned()).child(
                        Button::new(inc_id)
                            .ghost()
                            .small()
                            .label("+")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let next = clamped.saturating_add(10).min(MAX_VOICE_VOLUME);
                                this.apply_settings_effect(make_effect(next), window, cx);
                            })),
                    ),
                )
                .child(
                    div().debug_selector(move || reset_id.to_owned()).child(
                        Button::new(reset_id)
                            .ghost()
                            .small()
                            .label(l10n::tr("Reset"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.apply_settings_effect(make_effect(DEFAULT_VOICE_VOLUME), window, cx);
                            })),
                    ),
                ),
        )
        .into_any_element()
}

pub fn render(view: &ClientView, modal: &SettingsModal, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let active_theme = theme::kind(cx);
    let active_locale = l10n::current().locale();
    let display = view.prefs.data.display();
    let trusted_count = view.effective_trusted_domains().len();

    let theme_buttons = THEME_OPTIONS.iter().enumerate().map(|(ix, (kind, label))| {
        let kind = *kind;
        let selected = active_theme == kind;
        let mut btn = Button::new(("settings-theme", ix)).small().label(l10n::tr(label));
        btn = if selected { btn.primary() } else { btn.ghost() };
        btn.on_click(cx.listener(move |this, _, window, cx| {
            this.apply_settings_effect(SettingsEffect::ThemeChanged(kind), window, cx);
        }))
    });

    let density_buttons = [(false, "Cozy", "settings-density-cozy"), (true, "Compact", "settings-density-compact")]
        .into_iter()
        .map(|(compact, label, sel_id)| {
            let selected = display.compact == compact;
            let mut btn = Button::new(sel_id).small().label(l10n::tr(label));
            btn = if selected { btn.primary() } else { btn.ghost() };
            div().debug_selector(move || sel_id.to_owned()).child(btn.on_click(cx.listener(move |this, _, window, cx| {
                this.apply_settings_effect(SettingsEffect::CompactChanged(compact), window, cx);
            })))
        });

    let font_buttons = FONT_SIZE_OPTIONS.iter().map(|&size| {
        let selected = display.font_size == size;
        let sel_id = format!("settings-font-{size}");
        let btn_id = sel_id.clone();
        let mut btn = Button::new(SharedString::from(btn_id)).small().label(format!("{size}px"));
        btn = if selected { btn.primary() } else { btn.ghost() };
        div().debug_selector(move || sel_id.clone()).child(btn.on_click(cx.listener(move |this, _, window, cx| {
            this.apply_settings_effect(SettingsEffect::FontSizeChanged(size), window, cx);
        })))
    });

    let zoom_controls = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(div().text_sm().child(l10n::tr("Zoom")))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .child(
                    div().debug_selector(|| "settings-zoom-out".to_owned()).child(
                        Button::new("settings-zoom-out")
                            .ghost()
                            .small()
                            .label("-")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let next = display.ui_zoom.saturating_sub(10).max(MIN_UI_ZOOM);
                                this.apply_settings_effect(SettingsEffect::ZoomChanged(next), window, cx);
                            })),
                    ),
                )
                .child(div().px_2().text_xs().font_weight(FontWeight::SEMIBOLD).child(format!("{}%", display.ui_zoom)))
                .child(
                    div().debug_selector(|| "settings-zoom-in".to_owned()).child(
                        Button::new("settings-zoom-in")
                            .ghost()
                            .small()
                            .label("+")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let next = display.ui_zoom.saturating_add(10).min(MAX_UI_ZOOM);
                                this.apply_settings_effect(SettingsEffect::ZoomChanged(next), window, cx);
                            })),
                    ),
                )
                .child(
                    div().debug_selector(|| "settings-zoom-reset".to_owned()).child(
                        Button::new("settings-zoom-reset")
                            .ghost()
                            .small()
                            .label(l10n::tr("Reset"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.apply_settings_effect(SettingsEffect::ZoomChanged(100), window, cx);
                            })),
                    ),
                ),
        );

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
        .max_h(px(110.))
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
        let switch_label = if view.saved_accounts.len() > 1 {
            format!("{} ({})", l10n::tr("Switch accounts"), view.saved_accounts.len())
        } else {
            l10n::tr("Switch accounts")
        };
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .child(
                Button::new("settings-logout")
                    .danger()
                    .small()
                    .label(l10n::tr("Log out"))
                    .on_click(cx.listener(|this, _, _, cx| this.settings_prompt_logout(true, cx))),
            )
            .when(view.accounts_enabled, |d| {
                d.child(
                    div().debug_selector(|| "settings-switch-accounts".to_owned()).child(
                        Button::new("settings-switch-accounts")
                            .ghost()
                            .small()
                            .label(switch_label)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.apply_settings_effect(SettingsEffect::OpenAccountSwitcher, window, cx);
                            })),
                    ),
                )
            })
            .into_any_element()
    };

    let body = div()
        .id("settings-body")
        .max_h(px(380.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_3()
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
                .child(section_heading(l10n::tr("Display"), theme))
                .child(div().flex().flex_row().flex_wrap().gap_2().children(density_buttons))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(div().text_sm().child(l10n::tr("Font size")))
                        .child(div().flex().flex_row().flex_wrap().gap_1().children(font_buttons)),
                )
                .child(zoom_controls)
                .when(trusted_count > 0, |d| {
                    d.child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .child(div().text_sm().child(format!("{} ({trusted_count})", l10n::tr("Trusted domains"))))
                            .child(
                                div().debug_selector(|| "settings-clear-trusted".to_owned()).child(
                                    Button::new("settings-clear-trusted")
                                        .ghost()
                                        .small()
                                        .label(l10n::tr("Clear"))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.apply_settings_effect(SettingsEffect::ClearTrustedDomains, window, cx);
                                        })),
                                ),
                            ),
                    )
                }),
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
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(section_heading(l10n::tr("Voice & Audio"), theme))
                        .child(
                            div().debug_selector(|| "voice-refresh-devices".to_owned()).child(
                                Button::new("voice-refresh-devices")
                                    .ghost()
                                    .small()
                                    .label(l10n::tr("Refresh devices"))
                                    .accessibility_label(l10n::tr("Refresh devices"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.refresh_audio_devices(cx);
                                    })),
                            ),
                        ),
                )
                .child(voice_device_row(
                    "Input device",
                    view.prefs.data.voice_input_device.as_deref(),
                    &view.audio_devices.input_devices,
                    "voice-input-device",
                    "voice-input-device-reset",
                    theme,
                    SettingsEffect::VoiceInputDeviceChanged,
                    cx,
                ))
                .child(voice_volume_row(
                    "Input volume",
                    view.prefs.data.voice_input_volume,
                    "voice-input-vol-dec",
                    "voice-input-vol-inc",
                    "voice-input-vol-reset",
                    SettingsEffect::VoiceInputVolumeChanged,
                    cx,
                ))
                .child(voice_device_row(
                    "Output device",
                    view.prefs.data.voice_output_device.as_deref(),
                    &view.audio_devices.output_devices,
                    "voice-output-device",
                    "voice-output-device-reset",
                    theme,
                    SettingsEffect::VoiceOutputDeviceChanged,
                    cx,
                ))
                .child(voice_volume_row(
                    "Output volume",
                    view.prefs.data.voice_output_volume,
                    "voice-output-vol-dec",
                    "voice-output-vol-inc",
                    "voice-output-vol-reset",
                    SettingsEffect::VoiceOutputVolumeChanged,
                    cx,
                )),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Accessibility"), theme))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(div().text_sm().child(l10n::tr("Reduced motion")))
                        .child(
                            div().flex().flex_row().gap_1().children(
                                [
                                    (ReducedMotionPref::System, "System", "reduced-motion-system"),
                                    (ReducedMotionPref::On, "On", "reduced-motion-on"),
                                    (ReducedMotionPref::Off, "Off", "reduced-motion-off"),
                                ]
                                .into_iter()
                                .map(|(pref, label, sel_id)| {
                                    let selected = view.prefs.data.reduced_motion == pref;
                                    let mut btn = Button::new(sel_id).small().label(l10n::tr(label));
                                    btn = if selected { btn.primary() } else { btn.ghost() };
                                    div().debug_selector(move || sel_id.to_owned()).child(btn.on_click(
                                        cx.listener(move |this, _, window, cx| {
                                            this.apply_settings_effect(
                                                SettingsEffect::ReducedMotionChanged(pref),
                                                window,
                                                cx,
                                            );
                                        }),
                                    ))
                                }),
                            ),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Language"), theme))
                .child(locale_list),
        );

    let content = div()
        .id("settings-modal")
        .key_context(CONTEXT)
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
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
        .child(body)
        .child(
            div()
                .pt_3()
                .border_t_1()
                .border_color(theme.border)
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
