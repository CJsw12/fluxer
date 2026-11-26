use super::*;

struct RegionalFormatChoice {
    id: &'static str,
    name: &'static str,
    sample: &'static str,
}

const REGIONAL_FORMAT_CHOICES: &[RegionalFormatChoice] = &[
    RegionalFormatChoice {
        id: "language",
        name: "Same as language",
        sample: "11/24/2026, 2:30 PM · 1,234.56",
    },
    RegionalFormatChoice {
        id: "system",
        name: "System format",
        sample: "11/24/2026, 2:30 PM · 1,234.56",
    },
    RegionalFormatChoice {
        id: "en-US",
        name: "English (United States)",
        sample: "11/24/2026, 2:30 PM · 1,234.56",
    },
    RegionalFormatChoice {
        id: "en-GB",
        name: "English (United Kingdom)",
        sample: "24/11/2026, 14:30 · 1,234.56",
    },
    RegionalFormatChoice {
        id: "ja",
        name: "Japanese",
        sample: "2026/11/24 14:30 · 1,234.56",
    },
    RegionalFormatChoice {
        id: "ko",
        name: "Korean",
        sample: "2026. 11. 24. 오후 2:30 · 1,234.56",
    },
];

fn language_section(title: String, is_first: bool, content: AnyElement, theme: &Theme) -> AnyElement {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(16.))
        .pb(px(24.))
        .when(!is_first, |d| d.pt(px(24.)).border_t_1().border_color(settings_border(theme)))
        .flx("app.settings-section.section")
        .child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(4.))
                .flx("app.settings-section.section-header")
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .flx("app.settings-section.section-header-text")
                        .child(section_heading(title, theme)),
                ),
        )
        .child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(20.))
                .flx("app.settings-section.section-content")
                .child(content),
        )
        .into_any_element()
}

fn language_subsection(title: String, content: AnyElement, theme: &Theme) -> AnyElement {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(12.))
        .flx("app.settings-tab-layout.settings-tab-section.subsection")
        .child(
            div()
                .w_full()
                .h(px(22.))
                .flex()
                .flex_row()
                .items_center()
                .flx("app.settings-tab-layout.settings-tab-section.title-row")
                .child(
                    div()
                        .text_size(px(15.))
                        .line_height(px(19.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.foreground)
                        .flx("app.settings-tab-layout.settings-tab-section.h4")
                        .child(title),
                ),
        )
        .child(content)
        .into_any_element()
}

fn language_radio_option(
    id: &'static str,
    debug_sel: Option<&'static str>,
    title: String,
    desc: String,
    selected: bool,
    theme: &Theme,
    on_click: impl Fn(&mut ClientView, &gpui_kit::ClickEvent, &mut Window, &mut Context<ClientView>) + 'static,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let hover_bg = theme.hover_bg;
    let row = div()
        .id(id)
        .role(gpui_kit::Role::Button)
        .aria_label(title.clone())
        .aria_selected(selected)
        .w_full()
        .py(px(4.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(move |s| s.bg(hover_bg))
        .flex()
        .flex_row()
        .items_start()
        .gap(px(8.))
        .flx("ui.radio-group.radio-group.radio-option-item.radio-group-option")
        .on_click(cx.listener(on_click))
        .child(
            div()
                .mt(px(2.))
                .size(px(18.))
                .flex_none()
                .flx("ui.radio-group.radio-group.radio-option-item.radio-indicator")
                .child(
                    div()
                        .size(px(18.))
                        .rounded_full()
                        .border_2()
                        .border_color(if selected { theme.brand } else { settings_border(theme) })
                        .when(selected, |d| d.bg(theme.brand))
                        .flex()
                        .items_center()
                        .justify_center()
                        .flx("ui.radio-group.radio-group.radio-option-item.outer-radio-fill")
                        .when(selected, |d| {
                            d.child(
                                div()
                                    .size(px(7.2))
                                    .rounded_full()
                                    .bg(theme.on_fill)
                                    .flx("ui.radio-group.radio-group.radio-option-item.inner-dot-radio"),
                            )
                        }),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(4.))
                .flx("ui.radio-group.radio-group.radio-option-item.stack")
                .child(
                    div()
                        .w_full()
                        .h(px(21.))
                        .flex()
                        .flex_row()
                        .items_center()
                        .flx("ui.radio-group.radio-group.radio-option-item.label")
                        .child(
                            div()
                                .text_size(px(15.))
                                .line_height(px(21.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.foreground)
                                .flx("ui.radio-group.radio-group.radio-option-item.label-text")
                                .child(title),
                        ),
                )
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(19.2))
                        .text_color(theme.muted_foreground)
                        .flx("ui.radio-group.radio-group.radio-option-item.description")
                        .child(desc),
                ),
        );
    match debug_sel {
        Some(sel) => row.debug_selector(move || sel.to_owned()).into_any_element(),
        None => row.into_any_element(),
    }
}

fn language_switch_row(
    id: &'static str,
    label: String,
    checked: bool,
    theme: &Theme,
    on_change: impl Fn(&mut ClientView, &bool, &mut Window, &mut Context<ClientView>) + 'static,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    div()
        .w_full()
        .h(px(44.))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(16.))
        .flx("ui.form.switch.container")
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_row()
                .items_center()
                .flx("ui.form.switch.label-container")
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.foreground)
                        .flx("ui.form.switch.label")
                        .child(label),
                ),
        )
        .child(
            div()
                .flex_none()
                .debug_selector(move || id.to_owned())
                .flx("ui.form.switch.switch-root")
                .child(
                    Switch::new(id)
                        .checked(checked)
                        .on_change(cx.listener(on_change)),
                ),
        )
        .into_any_element()
}

pub(super) fn render_language_tab(
    view: &ClientView,
    modal: &SettingsModal,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let active_locale = l10n::current().locale();
    let active_opt = LOCALES
        .iter()
        .find(|opt| opt.code == active_locale)
        .unwrap_or(&LOCALES[10]); // en-US fallback

    // 1. Interface language
    let lang_trigger = div()
        .id("settings-locale-trigger")
        .debug_selector(|| "settings-locale-list".to_owned())
        .w_full()
        .h(px(34.))
        .pl(px(10.))
        .pr(px(0.))
        .rounded(px(8.))
        .border_1()
        .border_color(settings_border(theme))
        .bg(theme.background_textarea)
        .cursor_pointer()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(2.))
        .flx("user.language-selector.locale-change")
        .on_click(cx.listener(|this, _, _, cx| {
            if let Overlay::Settings(m) = &mut this.overlay {
                m.lang_dropdown_open = !m.lang_dropdown_open;
                m.regional_dropdown_open = false;
            }
            cx.notify();
        }))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h(px(24.))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .flx("user.language-selector.render-language-content.language-option")
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.foreground)
                        .flx("user.language-selector.render-language-content.language-name")
                        .child(active_opt.native_name),
                )
                .child(
                    div()
                        .h(px(20.))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .flx("user.language-selector.render-language-content.language-details")
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(theme.muted_foreground)
                                .flx("user.language-selector.render-language-content.language-code")
                                .child(active_opt.english_name),
                        )
                        .child(
                            div()
                                .size(px(20.))
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(15.))
                                .flx("user.language-selector.render-language-content.flag-image")
                                .child(if active_opt.flag.is_empty() {
                                    Icon::new(AppIcon::Translate)
                                        .size(px(16.))
                                        .text_color(theme.muted_foreground)
                                        .into_any_element()
                                } else {
                                    div().child(active_opt.flag).into_any_element()
                                }),
                        ),
                ),
        )
        .child(
            div()
                .w(px(34.))
                .h(px(28.))
                .flex_none()
                .flex()
                .items_center()
                .justify_end()
                .flx("ui.form.combobox.action-buttons")
                .child(
                    div()
                        .size(px(28.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .flx("ui.form.combobox.trigger-button")
                        .child(
                            div()
                                .size(px(16.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .flx("ui.form.combobox.trigger-icon")
                                .child(
                                    Icon::new(AppIcon::CaretDown)
                                        .size(px(16.))
                                        .text_color(theme.muted_foreground),
                                ),
                        ),
                ),
        );

    let lang_dropdown = modal.lang_dropdown_open.then(|| {
        div()
            .id("settings-locale-dropdown")
            .max_h(px(280.))
            .overflow_y_scroll()
            .track_scroll(&modal.locale_scroll)
            .rounded(px(8.))
            .border_1()
            .border_color(settings_border(theme))
            .bg(theme.background_secondary)
            .p_1()
            .flex()
            .flex_col()
            .gap(px(2.))
            .children(LOCALES.iter().enumerate().map(|(ix, opt)| {
                let selected = active_locale == opt.code;
                let code = opt.code;
                let hover_bg = theme.hover_bg;
                div()
                    .id(("settings-locale", ix))
                    .debug_selector(|| format!("settings-locale-{code}"))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .py(px(6.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(selected, |d| d.bg(theme.accent))
                    .hover(move |s| s.bg(hover_bg))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Overlay::Settings(m) = &mut this.overlay {
                            m.lang_dropdown_open = false;
                        }
                        this.apply_settings_effect(SettingsEffect::LocaleChanged(code.to_owned()), window, cx);
                    }))
                    .child(div().text_sm().text_color(theme.foreground).child(opt.native_name))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .child(div().text_xs().text_color(theme.muted_foreground).child(opt.english_name))
                            .when(!opt.flag.is_empty(), |d| d.child(div().text_sm().child(opt.flag))),
                    )
            }))
    });

    let notice_box = div()
        .w_full()
        .p(px(12.))
        .rounded(px(6.))
        .border_1()
        .border_color(settings_border(theme))
        .bg(theme.background_secondary)
        .flx("user.language-tab.notice")
        .child(
            div()
                .w_full()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_baseline()
                .text_size(px(14.))
                .line_height(px(18.9))
                .text_color(theme.muted_foreground)
                .flx("user.language-tab.notice-text")
                .child(l10n::tr("Help translate Fluxer into your language on "))
                .child(
                    div()
                        .id("settings-language-translate-link")
                        .cursor_pointer()
                        .text_color(theme.text_link)
                        .underline()
                        .flx("user.language-tab.link")
                        .on_click(cx.listener(|_, _, _, _| {
                            // TODO(backlog): open https://translate.fluxer.app in external browser
                        }))
                        .child("translate.fluxer.app"),
                )
                .child("."),
        );

    let interface_section_content = div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(16.))
        .flx("user.language-tab.language-controls")
        .child(lang_trigger)
        .when_some(lang_dropdown, |d, dd| d.child(dd))
        .child(notice_box)
        .into_any_element();

    // 2. Regional format
    let active_regional = REGIONAL_FORMAT_CHOICES
        .iter()
        .find(|c| c.id == modal.regional_format)
        .unwrap_or(&REGIONAL_FORMAT_CHOICES[0]);

    let regional_trigger = div()
        .id("settings-regional-format-trigger")
        .w_full()
        .h(px(34.))
        .pl(px(10.))
        .pr(px(0.))
        .rounded(px(8.))
        .border_1()
        .border_color(settings_border(theme))
        .bg(theme.background_textarea)
        .cursor_pointer()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(2.))
        .flx("user.regional-format-selector.combobox")
        .on_click(cx.listener(|this, _, _, cx| {
            if let Overlay::Settings(m) = &mut this.overlay {
                m.regional_dropdown_open = !m.regional_dropdown_open;
                m.lang_dropdown_open = false;
            }
            cx.notify();
        }))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h(px(24.))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .flx("user.regional-format-selector.option")
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.foreground)
                        .flx("user.regional-format-selector.option-name")
                        .child(l10n::tr(active_regional.name)),
                )
                .child(
                    div()
                        .h(px(18.))
                        .flex()
                        .flex_row()
                        .items_center()
                        .flx("user.regional-format-selector.option-details")
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(theme.muted_foreground)
                                .flx("user.regional-format-selector.option-sample")
                                .child(active_regional.sample),
                        ),
                ),
        )
        .child(
            div()
                .w(px(34.))
                .h(px(28.))
                .flex_none()
                .flex()
                .items_center()
                .justify_end()
                .child(
                    div()
                        .size(px(28.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            Icon::new(AppIcon::CaretDown)
                                .size(px(16.))
                                .text_color(theme.muted_foreground),
                        ),
                ),
        );

    let regional_dropdown = modal.regional_dropdown_open.then(|| {
        div()
            .id("settings-regional-format-dropdown")
            .rounded(px(8.))
            .border_1()
            .border_color(settings_border(theme))
            .bg(theme.background_secondary)
            .p_1()
            .flex()
            .flex_col()
            .gap(px(2.))
            .children(REGIONAL_FORMAT_CHOICES.iter().enumerate().map(|(ix, choice)| {
                let selected = modal.regional_format == choice.id;
                let choice_id = choice.id;
                let hover_bg = theme.hover_bg;
                div()
                    .id(("settings-regional-opt", ix))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .py(px(6.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(selected, |d| d.bg(theme.accent))
                    .hover(move |s| s.bg(hover_bg))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        // TODO(backlog): persist regional_format to UserSettings
                        if let Overlay::Settings(m) = &mut this.overlay {
                            m.regional_format = choice_id;
                            m.regional_dropdown_open = false;
                        }
                        cx.notify();
                    }))
                    .child(div().text_sm().text_color(theme.foreground).child(l10n::tr(choice.name)))
                    .child(div().text_xs().text_color(theme.muted_foreground).child(choice.sample))
            }))
    });

    let regional_section_content = div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(16.))
        .flx("user.language-tab.regional-format-controls")
        .child(regional_trigger)
        .when_some(regional_dropdown, |d, dd| d.child(dd))
        .child(
            div()
                .w_full()
                .text_size(px(14.))
                .line_height(px(18.9))
                .text_color(theme.muted_foreground)
                .flx("user.language-tab.regional-format-help")
                .child(l10n::tr(
                    "Dates, times and numbers follow this format. Prices stay in the currency of your billing country.",
                )),
        )
        .into_any_element();

    // 3. Time format
    let time_format_mode = modal.time_format_mode;
    let time_section_content = div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(16.))
        .flx("user.language-tab.time-controls")
        .child(
            div()
                .w_full()
                .h(px(169.78))
                .flex()
                .flex_col()
                .gap(px(6.))
                .flx("ui.radio-group.radio-group.group")
                .child(language_radio_option(
                    "settings-time-format-auto",
                    None,
                    l10n::tr("Auto"),
                    l10n::tr("Regional format: 2:30 PM"),
                    time_format_mode == 0,
                    theme,
                    |this, _, _, cx| {
                        // TODO(backlog): persist Auto time format mode to UserSettings
                        if let Overlay::Settings(m) = &mut this.overlay {
                            m.time_format_mode = 0;
                        }
                        cx.notify();
                    },
                    cx,
                ))
                .child(language_radio_option(
                    "settings-time-format-12h",
                    Some("settings-time-format-12h"),
                    l10n::tr("12-hour"),
                    "2:30 PM".to_owned(),
                    time_format_mode == 1,
                    theme,
                    |this, _, window, cx| {
                        if let Overlay::Settings(m) = &mut this.overlay {
                            m.time_format_mode = 1;
                        }
                        this.apply_settings_effect(SettingsEffect::TimeFormat24hChanged(false), window, cx);
                    },
                    cx,
                ))
                .child(language_radio_option(
                    "settings-time-format-24h",
                    Some("settings-time-format-24h"),
                    l10n::tr("24-hour"),
                    "14:30".to_owned(),
                    time_format_mode == 2,
                    theme,
                    |this, _, window, cx| {
                        if let Overlay::Settings(m) = &mut this.overlay {
                            m.time_format_mode = 2;
                        }
                        this.apply_settings_effect(SettingsEffect::TimeFormat24hChanged(true), window, cx);
                    },
                    cx,
                )),
        )
        .into_any_element();

    // 4. Date format
    let date_format_mode = modal.date_format_mode;
    let date_section_content = div()
        .w_full()
        .h(px(228.38))
        .flex()
        .flex_col()
        .gap(px(6.))
        .flx("ui.radio-group.radio-group.group")
        .child(language_radio_option(
            "settings-date-format-auto",
            None,
            l10n::tr("Auto"),
            l10n::tr("Regional format: 11/24/2026, 2:30 PM"),
            date_format_mode == 0,
            theme,
            |this, _, _, cx| {
                // TODO(backlog): persist timestamp_format to UserSettings
                if let Overlay::Settings(m) = &mut this.overlay {
                    m.date_format_mode = 0;
                }
                cx.notify();
            },
            cx,
        ))
        .child(language_radio_option(
            "settings-date-format-iso",
            None,
            l10n::tr("ISO 8601"),
            "2026-11-24 14:30".to_owned(),
            date_format_mode == 1,
            theme,
            |this, _, _, cx| {
                // TODO(backlog): persist timestamp_format to UserSettings
                if let Overlay::Settings(m) = &mut this.overlay {
                    m.date_format_mode = 1;
                }
                cx.notify();
            },
            cx,
        ))
        .child(language_radio_option(
            "settings-date-format-dmy",
            None,
            l10n::tr("Day first"),
            "24/11/2026, 2:30 PM".to_owned(),
            date_format_mode == 2,
            theme,
            |this, _, _, cx| {
                // TODO(backlog): persist timestamp_format to UserSettings
                if let Overlay::Settings(m) = &mut this.overlay {
                    m.date_format_mode = 2;
                }
                cx.notify();
            },
            cx,
        ))
        .child(language_radio_option(
            "settings-date-format-mdy",
            None,
            l10n::tr("Month first"),
            "11/24/2026, 2:30 PM".to_owned(),
            date_format_mode == 3,
            theme,
            |this, _, _, cx| {
                // TODO(backlog): persist timestamp_format to UserSettings
                if let Overlay::Settings(m) = &mut this.overlay {
                    m.date_format_mode = 3;
                }
                cx.notify();
            },
            cx,
        ))
        .into_any_element();

    // 5. Spellcheck
    let spellcheck_engine = modal.spellcheck_engine;
    let engine_group = div()
        .w_full()
        .h(px(189.38))
        .flex()
        .flex_col()
        .gap(px(6.))
        .flx("ui.radio-group.radio-group.group")
        .child(language_radio_option(
            "settings-spellcheck-engine-auto",
            None,
            l10n::tr("Recommended"),
            l10n::tr("Use your operating system's spellchecker when available. Otherwise, use Fluxer's in-app dictionaries."),
            spellcheck_engine == 0,
            theme,
            |this, _, _, cx| {
                // TODO(backlog): wire native spellcheck engine setting
                if let Overlay::Settings(m) = &mut this.overlay {
                    m.spellcheck_engine = 0;
                }
                cx.notify();
            },
            cx,
        ))
        .child(language_radio_option(
            "settings-spellcheck-engine-hunspell",
            None,
            l10n::tr("In-app dictionaries"),
            l10n::tr("Always use Fluxer's Hunspell dictionaries, downloaded on demand."),
            spellcheck_engine == 1,
            theme,
            |this, _, _, cx| {
                // TODO(backlog): wire native spellcheck engine setting
                if let Overlay::Settings(m) = &mut this.overlay {
                    m.spellcheck_engine = 1;
                }
                cx.notify();
            },
            cx,
        ))
        .child(language_radio_option(
            "settings-spellcheck-engine-system",
            None,
            l10n::tr("Operating system"),
            l10n::tr("Use the spellchecker built into your operating system."),
            spellcheck_engine == 2,
            theme,
            |this, _, _, cx| {
                // TODO(backlog): wire native spellcheck engine setting
                if let Overlay::Settings(m) = &mut this.overlay {
                    m.spellcheck_engine = 2;
                }
                cx.notify();
            },
            cx,
        ))
        .into_any_element();

    let personal_dict_subsection = (spellcheck_engine == 1).then(|| {
        let words = modal.spellcheck_personal_words.clone();
        let add_row = div()
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .child(div().flex_1().min_w_0().child(Input::new(&modal.spellcheck_personal_word)))
            .child(
                Button::new("settings-spellcheck-add-word")
                    .small()
                    .label(l10n::tr("Add"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        // TODO(backlog): persist personal dictionary words
                        if let Overlay::Settings(m) = &mut this.overlay {
                            let word = m.spellcheck_personal_word.read(cx).value().trim().to_owned();
                            if !word.is_empty() && !m.spellcheck_personal_words.contains(&word) {
                                m.spellcheck_personal_words.push(word);
                            }
                            m.spellcheck_personal_word.update(cx, |input, cx| {
                                input.set_value("", window, cx);
                            });
                        }
                        cx.notify();
                    })),
            );
        let word_list = if words.is_empty() {
            div()
                .text_size(px(13.))
                .text_color(theme.muted_foreground)
                .child(l10n::tr("No personal words yet."))
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap(px(6.))
                .children(words.into_iter().enumerate().map(|(ix, word)| {
                    div()
                        .px(px(10.))
                        .py(px(4.))
                        .rounded(px(12.))
                        .bg(settings_border(theme))
                        .text_size(px(13.))
                        .text_color(theme.foreground)
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(4.))
                        .child(word.clone())
                        .child(
                            div()
                                .id(("settings-spellcheck-remove-word", ix))
                                .cursor_pointer()
                                .text_color(theme.muted_foreground)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Overlay::Settings(m) = &mut this.overlay {
                                        m.spellcheck_personal_words.retain(|w| w != &word);
                                    }
                                    cx.notify();
                                }))
                                .child("×"),
                        )
                }))
                .into_any_element()
        };
        language_subsection(
            l10n::tr("Personal dictionary"),
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(add_row)
                .child(word_list)
                .into_any_element(),
            theme,
        )
    });

    let spellcheck_section_content = div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(20.))
        .child(language_switch_row(
            "settings-spellcheck-enabled",
            l10n::tr("Enable spellcheck"),
            modal.spellcheck_enabled,
            theme,
            |this, checked: &bool, _, cx| {
                // TODO(backlog): wire native spellcheck enabled setting
                if let Overlay::Settings(m) = &mut this.overlay {
                    m.spellcheck_enabled = *checked;
                }
                cx.notify();
            },
            cx,
        ))
        .when(modal.spellcheck_enabled, |d| {
            d.child(language_subsection(
                l10n::tr("Spellcheck engine"),
                engine_group,
                theme,
            ))
            .child(language_subsection(
                l10n::tr("Languages"),
                language_switch_row(
                    "settings-spellcheck-auto-detect",
                    l10n::tr("Detect while typing"),
                    modal.spellcheck_auto_detect,
                    theme,
                    |this, checked: &bool, _, cx| {
                        // TODO(backlog): wire native spellcheck auto-detect setting
                        if let Overlay::Settings(m) = &mut this.overlay {
                            m.spellcheck_auto_detect = *checked;
                        }
                        cx.notify();
                    },
                    cx,
                ),
                theme,
            ))
            .when_some(personal_dict_subsection, |d, sub| d.child(sub))
        })
        .into_any_element();

    let _ = view;

    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(20.))
        .child(language_section(
            l10n::tr("Interface language"),
            true,
            interface_section_content,
            theme,
        ))
        .child(language_section(
            l10n::tr("Regional format"),
            false,
            regional_section_content,
            theme,
        ))
        .child(language_section(
            l10n::tr("Time format"),
            false,
            time_section_content,
            theme,
        ))
        .child(language_section(
            l10n::tr("Date format"),
            false,
            date_section_content,
            theme,
        ))
        .child(language_section(
            l10n::tr("Spellcheck"),
            false,
            spellcheck_section_content,
            theme,
        ))
        .into_any_element()
}
