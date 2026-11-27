use super::*;

pub(super) fn render_messages_media_tab(
    view: &ClientView,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let prefs = &view.prefs.data;
    let toggle = |id: &'static str,
                      label: &'static str,
                      checked: bool,
                      effect: fn(bool) -> SettingsEffect| {
        div()
            .min_h(px(44.))
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(l10n::tr(label)),
            )
            .child(div().debug_selector(move || id.to_owned()).child(
                Switch::new(id).checked(checked).on_change(cx.listener(
                    move |this, checked: &bool, window, cx| {
                        this.apply_settings_effect(effect(*checked), window, cx);
                    },
                )),
            ))
    };
    let media = toggle(
        "settings-inline-media",
        "Inline image & GIF previews",
        prefs.inline_media_previews,
        SettingsEffect::InlineMediaPreviewsChanged,
    );
    let embeds = toggle(
        "settings-render-embeds",
        "Show embeds and preview website links",
        prefs.render_embeds,
        SettingsEffect::RenderEmbedsChanged,
    );
    let reactions = toggle(
        "settings-render-reactions",
        "Show emoji reactions on messages",
        prefs.render_reactions,
        SettingsEffect::RenderReactionsChanged,
    );
    let emoticons = toggle(
        "settings-convert-emoticons",
        "Convert emoticons & autocomplete suggestions",
        prefs.convert_emoticons,
        SettingsEffect::ConvertEmoticonsChanged,
    );
    let spoiler_options = [
        (
            "settings-spoiler-mode-on_click",
            "on_click",
            "On click",
            "Show spoiler content when clicked",
        ),
        (
            "settings-spoiler-mode-moderator",
            "moderator",
            "In channels I moderate",
            "Always show spoiler content in channels where you have the \"Manage messages\" permission",
        ),
        (
            "settings-spoiler-mode-always",
            "always",
            "Always",
            "Always show spoiler content",
        ),
    ];
    let spoilers = div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .children(
            spoiler_options
                .into_iter()
                .map(|(id, value, label, description)| {
                    div().debug_selector(move || id.to_owned()).child(
                        super::appearance::appearance_radio_option(
                            id,
                            label,
                            description,
                            prefs.spoiler_display.as_str() == value,
                            theme,
                            move |this, _, window, cx| {
                                this.apply_settings_effect(
                                    SettingsEffect::SpoilerDisplayChanged(value.to_owned()),
                                    window,
                                    cx,
                                );
                            },
                            cx,
                        ),
                    )
                }),
        );
    let group = |title: &'static str, body: AnyElement| {
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(l10n::tr(title)),
            )
            .child(body)
    };
    div()
        .debug_selector(|| "settings-chat-display".to_owned())
        .flex()
        .flex_col()
        .gap(px(20.))
        .child(section_heading(l10n::tr("Display"), theme))
        .child(group("Media display", media.into_any_element()))
        .child(group("Link previews", embeds.into_any_element()))
        .child(group("Reactions", reactions.into_any_element()))
        .child(group("Spoiler content", spoilers.into_any_element()))
        .child(
            div()
                .border_t_1()
                .border_color(theme.border)
                .pt_6()
                .flex()
                .flex_col()
                .gap_4()
                .child(section_heading(l10n::tr("Input"), theme))
                .child(emoticons),
        )
        .into_any_element()
}
