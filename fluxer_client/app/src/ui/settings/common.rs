use super::*;

pub(super) fn settings_sidebar_bg(theme: &Theme) -> Hsla {
    theme.background_tertiary
}

pub(super) fn settings_card_bg(theme: &Theme) -> Hsla {
    theme.background_secondary_lighter
}

pub(super) fn settings_border(theme: &Theme) -> Hsla {
    theme.background_modifier_accent
}

pub(super) fn settings_sub_selected_bg(theme: &Theme) -> Hsla {
    theme.accent
}

pub(super) fn settings_divider(theme: &Theme) -> AnyElement {
    div().w_full().h(px(1.)).flex_none().bg(theme.divider).into_any_element()
}

pub(super) fn sidebar_section_heading(title: String, theme: &Theme) -> impl IntoElement {
    div()
        .text_size(px(12.))
        .line_height(px(16.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.text_tertiary)
        .child(title)
}

pub(super) fn section_heading(title: String, theme: &Theme) -> impl IntoElement {
    // `SettingsSection`: 28px title row, 18px/600 title (line-height 1.3), 8px gap to the link icon.
    div()
        .h(px(28.))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.))
        .flx("app.settings-section.section-title-row")
        .child(
            div()
                .text_size(px(18.))
                .line_height(px(23.4))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.foreground)
                .flx("app.settings-section.section-title")
                .child(title),
        )
        .child(overlay::heading_link_button(None, None, theme))
}

pub(super) fn info_card_row(title: String, subtitle: String, badge: Option<String>, theme: &Theme) -> AnyElement {
    div()
        .p_3()
        .rounded(px(8.))
        .border_1()
        .border_color(settings_border(theme))
        .bg(settings_card_bg(theme))
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
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(title))
                .child(div().text_xs().text_color(theme.muted_foreground).child(subtitle)),
        )
        .when_some(badge, |d, b| {
            d.child(
                div()
                    .px_2()
                    .py_0p5()
                    .rounded(px(4.))
                    .bg(theme.background)
                    .border_1()
                    .border_color(settings_border(theme))
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.foreground)
                    .child(b),
            )
        })
        .into_any_element()
}

pub(super) fn settings_section_block(title: String, description: Option<String>, content: AnyElement, theme: &Theme) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .flx("app.settings-section.section")
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .flx("app.settings-section.section-header")
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .flx("app.settings-section.section-header-text")
                        .child(section_heading(title, theme))
                        .when_some(description, |d, desc| {
                            d.child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .flx("app.settings-section.section-description")
                                    .child(desc),
                            )
                        }),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .flx("app.settings-section.section-content")
                .child(content),
        )
        .into_any_element()
}
