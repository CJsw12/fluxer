use super::*;

pub(super) fn render_themes_tab(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let active_theme = theme::kind(cx);
    let display = view.prefs.data.display();
    let theme_swatches = THEME_OPTIONS.iter().enumerate().map(|(ix, (kind, label))| {
        let kind = *kind;
        render_theme_swatch(ix + 10, kind, label, active_theme == kind, theme, cx)
    });

    div()
        .flex()
        .flex_col()
        .gap_5()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Base theme"), theme))
                .child(div().flex().flex_row().flex_wrap().gap_3().children(theme_swatches)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Colors and fonts"), theme))
                .child(info_card_row(
                    l10n::tr("Fluxer design tokens"),
                    format!(
                        "{}: {}px · {}: {}%",
                        l10n::tr("Chat font size"),
                        display.font_size,
                        l10n::tr("Interface scale"),
                        display.ui_zoom
                    ),
                    Some(active_theme.as_str().to_owned()),
                    theme,
                )),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Desktop window"), theme))
                .child(info_card_row(
                    l10n::tr("Native GPUI titlebar & window chrome"),
                    l10n::tr("Uses DirectComposition hardware-accelerated rendering without WebView."),
                    Some(l10n::tr("Active")),
                    theme,
                )),
        )
        .into_any_element()
}
