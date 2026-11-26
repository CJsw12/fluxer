use super::*;

pub(super) fn render_notifications_tab(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let notifications_row = div()
        .p_3()
        .rounded(px(8.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.background_secondary)
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(l10n::tr("Desktop notifications")))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(l10n::tr("Show OS toast notifications for direct messages and @mentions.")),
                ),
        )
        .child(
            div().debug_selector(|| "settings-notifications".to_owned()).child(
                Switch::new("settings-notifications")
                    .checked(view.notifier.enabled)
                    .on_change(cx.listener(|this, checked: &bool, window, cx| {
                        this.apply_settings_effect(SettingsEffect::NotificationsChanged(*checked), window, cx);
                    })),
            ),
        );

    let sounds_row = div()
        .p_3()
        .rounded(px(8.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.background_secondary)
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(l10n::tr("Message sounds")))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(l10n::tr("Play alert sounds when new messages or mentions arrive.")),
                ),
        )
        .child(
            div().debug_selector(|| "settings-sounds".to_owned()).child(
                Switch::new("settings-sounds")
                    .checked(view.notifier.sounds_enabled)
                    .on_change(cx.listener(|this, checked: &bool, window, cx| {
                        this.apply_settings_effect(SettingsEffect::SoundsChanged(*checked), window, cx);
                    })),
            ),
        );

    div()
        .flex()
        .flex_col()
        .gap_5()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Notifications"), theme))
                .child(notifications_row)
                .child(sounds_row),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Unread & mention badges"), theme))
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
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(l10n::tr("Unread channel & mention indicators")),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(l10n::tr(
                                            "Highlight unread channels in the sidebar and show red mention count pills.",
                                        )),
                                ),
                        )
                        .child(
                            div().debug_selector(|| "settings-unread-badge".to_owned()).child(
                                Switch::new("settings-unread-badge")
                                    .checked(view.prefs.data.unread_badge_enabled)
                                    .on_change(cx.listener(|this, checked: &bool, window, cx| {
                                        this.apply_settings_effect(
                                            SettingsEffect::UnreadBadgeEnabledChanged(*checked),
                                            window,
                                            cx,
                                        );
                                    })),
                            ),
                        ),
                ),
        )
        .into_any_element()
}
