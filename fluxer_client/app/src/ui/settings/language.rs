use super::*;

pub(super) fn render_language_tab(
    view: &ClientView,
    modal: &SettingsModal,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let active_locale = l10n::current().locale();
    let use_24h = view.prefs.data.time_format_24h;
    let locale_list = div()
        .id("settings-locale-list")
        .debug_selector(|| "settings-locale-list".to_owned())
        .max_h(px(480.))
        .overflow_y_scroll()
        .track_scroll(&modal.locale_scroll)
        .rounded(px(8.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.background_secondary)
        .p_2()
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
                .px_3()
                .py(px(6.))
                .rounded(px(6.))
                .cursor_pointer()
                .when(selected, |d| d.bg(theme.accent))
                .hover(|s| s.bg(theme.muted))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.apply_settings_effect(SettingsEffect::LocaleChanged(code.to_owned()), window, cx);
                }))
                .child(div().text_sm().child(opt.native_name))
                .child(div().text_xs().text_color(theme.muted_foreground).child(opt.code))
        }));

    div()
        .flex()
        .flex_col()
        .gap_5()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_3()
                .child(section_heading(l10n::tr("Language"), theme))
                .child(locale_list),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Time & date format"), theme))
                .child(
                    div()
                        .p_3()
                        .rounded(px(8.))
                        .border_1()
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
                                .flex_col()
                                .gap_0p5()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(l10n::tr("Regional timestamp formatting")),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(l10n::tr(
                                            "Choose 12-hour (AM/PM) or 24-hour clock format for message timestamps.",
                                        )),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .gap_1()
                                .child(
                                    div().debug_selector(|| "settings-time-format-12h".to_owned()).child({
                                        let mut btn = Button::new("settings-time-format-12h")
                                            .small()
                                            .label(l10n::tr("12-hour"));
                                        btn = if !use_24h { btn.primary() } else { btn.ghost() };
                                        btn.on_click(cx.listener(|this, _, window, cx| {
                                            this.apply_settings_effect(
                                                SettingsEffect::TimeFormat24hChanged(false),
                                                window,
                                                cx,
                                            );
                                        }))
                                    }),
                                )
                                .child(
                                    div().debug_selector(|| "settings-time-format-24h".to_owned()).child({
                                        let mut btn = Button::new("settings-time-format-24h")
                                            .small()
                                            .label(l10n::tr("24-hour"));
                                        btn = if use_24h { btn.primary() } else { btn.ghost() };
                                        btn.on_click(cx.listener(|this, _, window, cx| {
                                            this.apply_settings_effect(
                                                SettingsEffect::TimeFormat24hChanged(true),
                                                window,
                                                cx,
                                            );
                                        }))
                                    }),
                                ),
                        ),
                ),
        )
        .into_any_element()
}
