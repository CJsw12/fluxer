use super::*;

pub(super) fn render_accessibility_tab(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let display = view.prefs.data.display();
    div()
        .flex()
        .flex_col()
        .gap_5()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_3()
                .child(section_heading(l10n::tr("Motion"), theme))
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
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_0p5()
                                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(l10n::tr("Reduced motion")))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(l10n::tr("Pause animated GIF thumbnails and reduce interface transitions.")),
                                ),
                        )
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
                .child(section_heading(l10n::tr("Visual & contrast"), theme))
                .child(info_card_row(
                    l10n::tr("Text scaling & interface zoom"),
                    format!("{}px · {}%", display.font_size, display.ui_zoom),
                    Some(l10n::tr("Synced")),
                    theme,
                )),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Keyboard & screen reader"), theme))
                .child(info_card_row(
                    l10n::tr("AccessKit assistive tree"),
                    l10n::tr("Exposes semantic roles, labels, and keyboard focus navigation to Windows Narrator and NVDA."),
                    Some(l10n::tr("Enabled")),
                    theme,
                )),
        )
        .into_any_element()
}
