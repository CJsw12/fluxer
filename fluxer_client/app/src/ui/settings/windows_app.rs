use super::*;

pub(super) fn render_windows_app_tab(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let autostart_row = div()
        .debug_selector(|| "settings-windows-autostart".to_owned())
        .p_3()
        .rounded(px(8.))
        .border_1()
        .border_color(settings_border(theme))
        .bg(settings_card_bg(theme))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(l10n::tr("Desktop startup")))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(l10n::tr("Launch Fluxer automatically when you sign in to Windows.")),
                ),
        )
        .child(
            Switch::new("settings-autostart")
                .checked(view.prefs.data.autostart)
                .on_change(cx.listener(|this, checked: &bool, window, cx| {
                    this.apply_settings_effect(SettingsEffect::AutostartChanged(*checked), window, cx);
                })),
        );

    let close_to_tray_row = div()
        .debug_selector(|| "settings-windows-close-to-tray".to_owned())
        .p_3()
        .rounded(px(8.))
        .border_1()
        .border_color(settings_border(theme))
        .bg(settings_card_bg(theme))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(l10n::tr("Minimize to tray")))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(l10n::tr("Keep Fluxer running in the system tray when closing the window.")),
                ),
        )
        .child(
            Switch::new("settings-close-to-tray")
                .checked(view.prefs.data.close_to_tray)
                .on_change(cx.listener(|this, checked: &bool, window, cx| {
                    this.apply_settings_effect(SettingsEffect::CloseToTrayChanged(*checked), window, cx);
                })),
        );

    let desktop_window_section = settings_section_block(
        l10n::tr("Desktop window"),
        None,
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(autostart_row)
            .child(close_to_tray_row)
            .into_any_element(),
        theme,
    );

    let tray_icon_section = settings_section_block(
        l10n::tr("Tray icon color"),
        None,
        info_card_row(
            l10n::tr("Match the taskbar"),
            l10n::tr("Pick white or black from the color of your taskbar or panel."),
            Some(l10n::tr("Auto")),
            theme,
        ),
        theme,
    );

    div()
        .debug_selector(|| "settings-windows-app-tab".to_owned())
        .flex()
        .flex_col()
        .gap_5()
        .child(desktop_window_section)
        .child(settings_divider(theme))
        .child(tray_icon_section)
        .into_any_element()
}
