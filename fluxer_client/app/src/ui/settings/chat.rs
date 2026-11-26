use super::*;

pub(super) fn render_messages_media_tab(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let active_spoiler = view.prefs.data.spoiler_display.as_str();
    let spoiler_options: &[(&str, &str)] = &[
        ("on_click", "On click"),
        ("moderator", "In channels I moderate"),
        ("always", "Always"),
    ];
    div()
        .debug_selector(|| "settings-chat-display".to_owned())
        .flex()
        .flex_col()
        .gap_5()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Display & previews"), theme))
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
                                        .child(l10n::tr("Inline image & GIF previews")),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(l10n::tr(
                                            "Display uploaded images, animated GIFs, and media attachments directly in chat.",
                                        )),
                                ),
                        )
                        .child(
                            div().debug_selector(|| "settings-inline-media".to_owned()).child(
                                Switch::new("settings-inline-media")
                                    .checked(view.prefs.data.inline_media_previews)
                                    .on_change(cx.listener(|this, checked: &bool, window, cx| {
                                        this.apply_settings_effect(
                                            SettingsEffect::InlineMediaPreviewsChanged(*checked),
                                            window,
                                            cx,
                                        );
                                    })),
                            ),
                        ),
                )
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
                                        .child(l10n::tr("Website link previews & embeds")),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(l10n::tr(
                                            "Show rich embed cards and YouTube thumbnails for links posted in chat.",
                                        )),
                                ),
                        )
                        .child(
                            div().debug_selector(|| "settings-render-embeds".to_owned()).child(
                                Switch::new("settings-render-embeds")
                                    .checked(view.prefs.data.render_embeds)
                                    .on_change(cx.listener(|this, checked: &bool, window, cx| {
                                        this.apply_settings_effect(
                                            SettingsEffect::RenderEmbedsChanged(*checked),
                                            window,
                                            cx,
                                        );
                                    })),
                            ),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Spoilers & reactions"), theme))
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
                                        .child(l10n::tr("Spoiler content")),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(l10n::tr("Choose how hidden spoiler blocks are revealed in chat.")),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .flex_wrap()
                                .gap_1()
                                .children(spoiler_options.iter().map(|&(val, label)| {
                                    let selected = active_spoiler == val;
                                    let sel_id = format!("settings-spoiler-mode-{val}");
                                    let val_owned = val.to_owned();
                                    let mut btn = Button::new(SharedString::from(format!("chat-{sel_id}")))
                                        .small()
                                        .label(l10n::tr(label));
                                    btn = if selected { btn.primary() } else { btn.ghost() };
                                    div().debug_selector(move || sel_id.clone()).child(btn.on_click(cx.listener(
                                        move |this, _, window, cx| {
                                            this.apply_settings_effect(
                                                SettingsEffect::SpoilerDisplayChanged(val_owned.clone()),
                                                window,
                                                cx,
                                            );
                                        },
                                    )))
                                })),
                        ),
                )
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
                                        .child(l10n::tr("Emoji reactions")),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(l10n::tr(
                                            "Display reaction pills under messages and allow quick reaction toggling.",
                                        )),
                                ),
                        )
                        .child(
                            div().debug_selector(|| "settings-render-reactions".to_owned()).child(
                                Switch::new("settings-render-reactions")
                                    .checked(view.prefs.data.render_reactions)
                                    .on_change(cx.listener(|this, checked: &bool, window, cx| {
                                        this.apply_settings_effect(
                                            SettingsEffect::RenderReactionsChanged(*checked),
                                            window,
                                            cx,
                                        );
                                    })),
                            ),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Text input & autocomplete"), theme))
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
                                        .child(l10n::tr("Convert emoticons & autocomplete suggestions")),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(l10n::tr(
                                            "Automatically convert :) emoticons to emoji and show @ / # / : popup suggestions.",
                                        )),
                                ),
                        )
                        .child(
                            div().debug_selector(|| "settings-convert-emoticons".to_owned()).child(
                                Switch::new("settings-convert-emoticons")
                                    .checked(view.prefs.data.convert_emoticons)
                                    .on_change(cx.listener(|this, checked: &bool, window, cx| {
                                        this.apply_settings_effect(
                                            SettingsEffect::ConvertEmoticonsChanged(*checked),
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
