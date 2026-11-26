use super::*;

pub(super) fn render_appearance_preview(view: &ClientView, theme: &Theme) -> AnyElement {
    let display = view.prefs.data.display();
    let msg_size = px(f32::from(display.font_size));
    let (avatar_el, user_name) = match &view.user {
        Some(user) => (
            render_avatar(
                AvatarSpec {
                    user_id: user.id,
                    avatar_url: user.avatar_url.clone(),
                    display_name: user.name.clone(),
                    size: 40.,
                    status: None,
                    status_border_bg: None,
                },
                theme,
            ),
            user.name.clone(),
        ),
        None => (
            div().size(px(40.)).rounded_full().bg(theme.muted).into_any_element(),
            SharedString::new_static("Fluxer User"),
        ),
    };

    let preview_body = if display.compact {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_baseline()
                    .gap_2()
                    .child(div().text_xs().text_color(theme.muted_foreground).child("12:00"))
                    .child(div().text_size(msg_size).font_weight(FontWeight::SEMIBOLD).child(user_name.clone()))
                    .child(div().text_size(msg_size).child(l10n::tr("This is how messages appear"))),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_baseline()
                    .gap_2()
                    .child(div().text_xs().text_color(theme.muted_foreground).child("12:00"))
                    .child(div().text_size(msg_size).font_weight(FontWeight::SEMIBOLD).child(user_name))
                    .child(div().text_size(msg_size).child(l10n::tr("With different display modes available"))),
            )
            .into_any_element()
    } else {
        div()
            .flex()
            .flex_row()
            .items_start()
            .gap_3()
            .child(div().flex_none().child(avatar_el))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_baseline()
                            .gap_2()
                            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(user_name))
                            .child(div().text_xs().text_color(theme.muted_foreground).child("Today at 12:00 PM")),
                    )
                    .child(div().text_size(msg_size).child(l10n::tr("This is how messages appear")))
                    .child(div().text_size(msg_size).child(l10n::tr("With different display modes available"))),
            )
            .into_any_element()
    };

    div()
        .debug_selector(|| "settings-appearance-preview".to_owned())
        .p_4()
        .rounded(px(8.))
        .border_1()
        .border_color(settings_border(theme))
        .bg(settings_card_bg(theme))
        .child(preview_body)
        .into_any_element()
}

pub(super) fn render_theme_swatch(
    ix: usize,
    kind: ThemeKind,
    label: &'static str,
    selected: bool,
    theme: &Theme,
    cx: &Context<ClientView>,
) -> AnyElement {
    let dark_preview = Theme::preview(ThemeKind::Dark);
    let light_preview = Theme::preview(ThemeKind::Light);
    let preview = Theme::preview(kind);
    let sel_id = format!("settings-theme-swatch-{}", kind.as_str());

    let mini_window = if kind == ThemeKind::System {
        div()
            .w_full()
            .h(px(54.))
            .rounded(px(6.))
            .overflow_hidden()
            .border_1()
            .border_color(theme.border)
            .flex()
            .flex_row()
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .bg(dark_preview.background)
                    .flex()
                    .flex_row()
                    .child(div().w(px(12.)).h_full().bg(dark_preview.background_tertiary))
                    .child(
                        div()
                            .flex_1()
                            .p_1p5()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().w(px(24.)).h(px(5.)).rounded(px(2.)).bg(dark_preview.brand))
                            .child(div().w(px(30.)).h(px(4.)).rounded(px(2.)).bg(dark_preview.muted_foreground)),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .bg(light_preview.background)
                    .flex()
                    .flex_row()
                    .child(div().w(px(12.)).h_full().bg(light_preview.background_secondary))
                    .child(
                        div()
                            .flex_1()
                            .p_1p5()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().w(px(24.)).h(px(5.)).rounded(px(2.)).bg(light_preview.brand))
                            .child(div().w(px(30.)).h(px(4.)).rounded(px(2.)).bg(light_preview.muted_foreground)),
                    ),
            )
    } else {
        div()
            .w_full()
            .h(px(54.))
            .rounded(px(6.))
            .overflow_hidden()
            .border_1()
            .border_color(preview.border)
            .bg(preview.background)
            .flex()
            .flex_row()
            .child(div().w(px(14.)).h_full().bg(preview.background_tertiary))
            .child(div().w(px(24.)).h_full().bg(preview.background_secondary))
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .p_1p5()
                    .flex()
                    .flex_col()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().w(px(34.)).h(px(5.)).rounded(px(2.)).bg(preview.foreground))
                            .child(div().w(px(46.)).h(px(4.)).rounded(px(2.)).bg(preview.muted_foreground)),
                    )
                    .child(div().w(px(22.)).h(px(6.)).rounded(px(3.)).bg(preview.brand)),
            )
    };

    div()
        .id(("settings-theme", ix))
        .role(gpui_kit::Role::Button)
        .aria_label(l10n::tr(label))
        .aria_selected(selected)
        .debug_selector(move || sel_id.clone())
        .relative()
        .w(px(120.))
        .p_2()
        .rounded(px(10.))
        .border_2()
        .border_color(if selected { theme.brand } else { settings_border(theme) })
        .bg(if selected {
            settings_sub_selected_bg(theme)
        } else {
            settings_card_bg(theme)
        })
        .cursor_pointer()
        .hover(|s| s.border_color(theme.brand))
        .flex()
        .flex_col()
        .gap_1p5()
        .on_click(cx.listener(move |this, _, window, cx| {
            this.apply_settings_effect(SettingsEffect::ThemeChanged(kind), window, cx);
        }))
        .child(mini_window)
        .when(selected, |d| {
            d.child(
                div()
                    .absolute()
                    .top_1()
                    .right_1()
                    .size(px(18.))
                    .rounded_full()
                    .bg(theme.brand)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Icon::new(AppIcon::Check).xsmall().text_color(theme.on_fill)),
            )
        })
        .child(
            div()
                .text_xs()
                .font_weight(if selected { FontWeight::BOLD } else { FontWeight::MEDIUM })
                .text_color(if selected { theme.foreground } else { theme.muted_foreground })
                .truncate()
                .child(l10n::tr(label)),
        )
        .into_any_element()
}

pub(super) fn render_appearance_tab(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let active_theme = theme::kind(cx);
    let display = view.prefs.data.display();
    let hover_bg = theme.hover_bg;

    let theme_swatches = THEME_OPTIONS.iter().enumerate().map(|(ix, (kind, label))| {
        let kind = *kind;
        render_theme_swatch(ix, kind, label, active_theme == kind, theme, cx)
    });

    let density_cards = [
        (false, "Comfy", "Spacious layout with clear visual separation between messages.", "settings-density-cozy"),
        (true, "Dense", "Maximizes visible messages with minimal spacing.", "settings-density-compact"),
    ]
    .into_iter()
    .map(|(compact, label, desc, sel_id)| {
        let selected = display.compact == compact;
        div()
            .id(sel_id)
            .role(gpui_kit::Role::Button)
            .aria_label(l10n::tr(label))
            .aria_selected(selected)
            .debug_selector(move || sel_id.to_owned())
            .py_1p5()
            .px_2()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(move |s| s.bg(hover_bg))
            .flex()
            .flex_row()
            .items_start()
            .gap_3()
            .on_click(cx.listener(move |this, _, window, cx| {
                this.apply_settings_effect(SettingsEffect::CompactChanged(compact), window, cx);
            }))
            .child(
                div()
                    .mt(px(2.))
                    .size(px(18.))
                    .flex_none()
                    .rounded_full()
                    .border_2()
                    .border_color(if selected { theme.brand } else { settings_border(theme) })
                    .when(selected, |d| {
                        d.bg(theme.brand)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(div().size(px(6.)).rounded_full().bg(theme.on_fill))
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(l10n::tr(label)))
                    .child(div().text_xs().text_color(theme.muted_foreground).child(l10n::tr(desc))),
            )
    });

    let font_fraction = {
        let idx = FONT_SIZE_OPTIONS
            .iter()
            .position(|&s| s == display.font_size)
            .unwrap_or(1);
        idx as f32 / (FONT_SIZE_OPTIONS.len().saturating_sub(1).max(1)) as f32
    };
    let font_stops = FONT_SIZE_OPTIONS.iter().map(|&size| {
        let selected = display.font_size == size;
        let is_default = size == 16;
        let sel_id = format!("settings-font-{size}");
        let btn_id = sel_id.clone();
        div()
            .id(SharedString::from(btn_id))
            .role(gpui_kit::Role::Button)
            .aria_label(format!("{size}px"))
            .aria_selected(selected)
            .debug_selector(move || sel_id.clone())
            .cursor_pointer()
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .on_click(cx.listener(move |this, _, window, cx| {
                this.apply_settings_effect(SettingsEffect::FontSizeChanged(size), window, cx);
            }))
            .child(
                div()
                    .size(px(if selected { 12. } else { 8. }))
                    .rounded_full()
                    .border_2()
                    .border_color(if selected {
                        theme.brand
                    } else if is_default {
                        theme.status_online
                    } else {
                        settings_border(theme)
                    })
                    .bg(if selected { theme.on_fill } else { settings_card_bg(theme) }),
            )
            .child(
                div()
                    .text_xs()
                    .font_weight(if selected || is_default { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                    .text_color(if selected {
                        theme.foreground
                    } else if is_default {
                        theme.status_online
                    } else {
                        theme.muted_foreground
                    })
                    .child(format!("{size}px")),
            )
    });

    let font_slider = div()
        .debug_selector(|| "settings-font-slider".to_owned())
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(l10n::tr("Chat font size")))
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.muted_foreground)
                        .child(format!("{}px", display.font_size)),
                ),
        )
        .child(
            div()
                .relative()
                .w_full()
                .pt_1()
                .child(
                    div()
                        .w_full()
                        .h(px(4.))
                        .rounded_full()
                        .bg(settings_border(theme))
                        .overflow_hidden()
                        .child(div().h_full().bg(theme.brand).w(gpui_kit::relative(font_fraction))),
                )
                .child(
                    div()
                        .mt_2()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .children(font_stops),
                ),
        );

    let zoom_fraction = (f32::from(display.ui_zoom.saturating_sub(MIN_UI_ZOOM))
        / f32::from(MAX_UI_ZOOM.saturating_sub(MIN_UI_ZOOM).max(1)))
    .clamp(0., 1.);
    let zoom_ticks = [50u16, 75, 100, 125, 150, 175, 200].into_iter().map(|pct| {
        let is_default = pct == 100;
        let is_current = display.ui_zoom == pct;
        div()
            .text_xs()
            .text_color(if is_default {
                theme.status_online
            } else if is_current {
                theme.foreground
            } else {
                theme.muted_foreground
            })
            .child(format!("{pct}%"))
    });
    let zoom_controls = div()
        .debug_selector(|| "settings-zoom-slider".to_owned())
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(section_heading(l10n::tr("App zoom level"), theme))
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
                                    .xsmall()
                                    .label("-")
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        let next = display.ui_zoom.saturating_sub(10).max(MIN_UI_ZOOM);
                                        this.apply_settings_effect(SettingsEffect::ZoomChanged(next), window, cx);
                                    })),
                            ),
                        )
                        .child(
                            div()
                                .px_2()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.foreground)
                                .child(format!("{}%", display.ui_zoom)),
                        )
                        .child(
                            div().debug_selector(|| "settings-zoom-in".to_owned()).child(
                                Button::new("settings-zoom-in")
                                    .ghost()
                                    .xsmall()
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
                                    .xsmall()
                                    .label(l10n::tr("Reset"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.apply_settings_effect(SettingsEffect::ZoomChanged(100), window, cx);
                                    })),
                            ),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .children(zoom_ticks),
        )
        .child(
            div()
                .relative()
                .w_full()
                .h(px(14.))
                .flex()
                .flex_row()
                .items_center()
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .h(px(4.))
                        .rounded_full()
                        .bg(settings_border(theme)),
                )
                .child(div().h(px(4.)).rounded_full().bg(theme.brand).w(gpui_kit::relative(zoom_fraction)))
                .child(div().ml(px(-6.)).size(px(12.)).flex_none().rounded_full().bg(theme.foreground)),
        );

    div()
        .flex()
        .flex_col()
        .gap_5()
        .child(render_appearance_preview(view, theme))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Theme"), theme))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(l10n::tr("You can pick a dark or light theme and customize colors in Themes.")),
                )
                .child(div().pt_1().flex().flex_row().flex_wrap().gap_3().children(theme_swatches)),
        )
        .child(settings_divider(theme))
        .child(zoom_controls)
        .child(settings_divider(theme))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_4()
                .child(section_heading(l10n::tr("Messages"), theme))
                .child(font_slider)
                .child(div().flex().flex_col().gap_1p5().children(density_cards)),
        )
        .into_any_element()
}
