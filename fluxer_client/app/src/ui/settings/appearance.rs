use super::*;

pub(super) fn render_appearance_preview(view: &ClientView, theme: &Theme) -> AnyElement {
    let display = view.prefs.data.display();
    let msg_size = px(f32::from(display.font_size));
    let user_name = view
        .user
        .as_ref()
        .map(|user| user.name.clone())
        .unwrap_or_else(|| SharedString::new_static("Fluxer User"));

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
        let group = |time: &'static str, messages: &[&'static str]| {
            let avatar = match &view.user {
                Some(user) => render_avatar(
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
                None => div()
                    .size(px(40.))
                    .rounded_full()
                    .bg(theme.muted)
                    .into_any_element(),
            };
            div()
                .flex()
                .flex_row()
                .items_start()
                .gap_3()
                .child(div().flex_none().mt(px(4.)).child(avatar))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .h(px(23.))
                                .flex()
                                .flex_row()
                                .items_baseline()
                                .gap_2()
                                .child(
                                    div()
                                        .text_size(msg_size)
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(user_name.clone()),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(time),
                                ),
                        )
                        .children(messages.iter().enumerate().map(|(index, message)| {
                            div()
                                .when(index != 0, |el| el.mt(px(5.)))
                                .text_size(msg_size)
                                .line_height(px(24.))
                                .child(l10n::tr(message))
                        })),
                )
        };
        div()
            .pl(px(16.))
            .pt(px(2.))
            .flex()
            .flex_col()
            .gap(px(21.))
            .child(group(
                "Today at 12:00 PM",
                &[
                    "This is how messages appear",
                    "With different display modes available",
                    "Customize the spacing and size",
                ],
            ))
            .child(group("Today at 12:20 PM", &["Waiting for you to..."]))
            .into_any_element()
    };

    div()
        .w_full()
        .h(px(176.))
        .flex_none()
        .bg(settings_card_bg(theme))
        .flx("user.appearance-tab.messages-tab.messages-preview.div")
        .child(
            div()
                .w_full()
                .h(px(176.))
                .relative()
                .overflow_hidden()
                .pt(px(2.))
                .px(px(12.))
                .pb(px(0.))
                .rounded(px(8.))
                .border_1()
                .border_color(settings_border(theme))
                .flex()
                .flex_col()
                .debug_selector(|| "settings-appearance-preview".to_owned())
                .flx("user.appearance-tab.messages-tab.messages-preview.div--2")
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .w_full()
                        .overflow_hidden()
                        .flex()
                        .flex_col()
                        .flx("user.appearance-tab.messages-tab.messages-preview.div--3")
                        .child(preview_body),
                )
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .h(px(32.))
                        .bg(settings_card_bg(theme).opacity(0.65))
                        .flx("user.appearance-tab.messages-tab.messages-preview.div--4"),
                ),
        )
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

fn appearance_section(first: bool, title: String, content: AnyElement, theme: &Theme) -> AnyElement {
    let base = div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(16.))
        .pb(px(24.));
    let section = if first {
        base
    } else {
        base.pt(px(24.)).border_t_1().border_color(settings_border(theme))
    };
    section
        .flx("app.settings-section.section")
        .child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(4.))
                .min_w_0()
                .flx("app.settings-section.section-header")
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .min_w_0()
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

pub(super) fn appearance_radio_option(
    id: &'static str,
    title: &'static str,
    desc: &'static str,
    selected: bool,
    theme: &Theme,
    on_click: impl Fn(&mut ClientView, &gpui_kit::ClickEvent, &mut Window, &mut Context<ClientView>) + 'static,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let hover_bg = theme.hover_bg;
    div()
        .id(id)
        .role(gpui_kit::Role::Button)
        .aria_label(l10n::tr(title))
        .aria_selected(selected)
        .py(px(4.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(move |s| s.bg(hover_bg))
        .flex()
        .flex_row()
        .items_start()
        .gap(px(8.))
        .on_click(cx.listener(on_click))
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
                        .child(div().size(px(7.2)).rounded_full().bg(theme.on_fill))
                }),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .line_height(px(21.))
                        .font_weight(FontWeight::MEDIUM)
                        .child(l10n::tr(title)),
                )
                .child(
                    div()
                        .text_sm()
                        .line_height(px(19.6))
                        .text_color(theme.muted_foreground)
                        .child(l10n::tr(desc)),
                ),
        )
        .into_any_element()
}

fn appearance_switch_row(
    id: &'static str,
    label: &'static str,
    description: Option<&'static str>,
    checked: bool,
    theme: &Theme,
    on_click: impl Fn(&mut ClientView, &bool, &mut Window, &mut Context<ClientView>) + 'static,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    div()
        .w_full()
        .flex()
        .flex_row()
        .items_start()
        .justify_between()
        .gap_4()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(l10n::tr(label)))
                .when_some(description, |d, desc| {
                    d.child(div().text_xs().text_color(theme.muted_foreground).child(l10n::tr(desc)))
                }),
        )
        .child(Switch::new(id).checked(checked).on_click(cx.listener(on_click)))
        .into_any_element()
}

pub(super) fn render_appearance_tab(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let display = view.prefs.data.display();
    let hover_bg = theme.hover_bg;

    let themes_link_content = div()
        .w_full()
        .h(px(20.))
        .flex()
        .flex_row()
        .items_center()
        .text_size(px(14.))
        .line_height(px(20.))
        .text_color(theme.text_secondary)
        .flx("user.appearance-tab.themes-link-content.text")
        .child(l10n::tr("You can pick a dark or light theme and change colors and quick CSS in "))
        .child(
            div()
                .id("settings-open-themes-tab")
                .debug_selector(|| "settings-open-themes-tab".to_owned())
                .cursor_pointer()
                .text_color(theme.text_link)
                .hover(|s| s.underline())
                .on_click(cx.listener(|this, _, _, cx| {
                    this.select_settings_tab(SettingsTab::Themes, cx);
                }))
                .flx("user.appearance-tab.themes-link-content.link.open-themes-tab.button")
                .child("\"Themes\""),
        )
        .child(".")
        .into_any_element();

    let hdr_content = div()
        .w_full()
        .h(px(111.19))
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(appearance_radio_option(
            "appearance-hdr-full",
            "Full dynamic range",
            "Display HDR images at full brightness and color range.",
            true,
            theme,
            |_, _, _, _| {
                // TODO(backlog): persist HDR display mode (full)
            },
            cx,
        ))
        .child(appearance_radio_option(
            "appearance-hdr-standard",
            "Standard range",
            "Tone-map HDR images to standard range, reducing peak brightness.",
            false,
            theme,
            |_, _, _, _| {
                // TODO(backlog): persist HDR display mode (standard)
            },
            cx,
        ))
        .into_any_element();

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
    let zoom_content = div()
        .w_full()
        .h(px(50.))
        .debug_selector(|| "settings-zoom-slider".to_owned())
        .flex()
        .flex_col()
        .justify_between()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_3()
                        .children(zoom_ticks),
                )
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
        )
        .into_any_element();

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

    let messages_content = div()
        .w_full()
        .h(px(331.19))
        .flex()
        .flex_col()
        .gap_4()
        .child(font_slider)
        .child(div().flex().flex_col().gap_1p5().children(density_cards))
        .into_any_element();

    let interface_content = div()
        .w_full()
        .h(px(267.78))
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.foreground)
                .child(l10n::tr("Channel list typing indicators")),
        )
        .child(appearance_radio_option(
            "appearance-typing-avatars",
            "Typing indicator + avatars",
            "Show typing indicator with user avatars in the channel list",
            true,
            theme,
            |_, _, _, _| {
                // TODO(backlog): persist channel list typing indicator mode (avatars)
            },
            cx,
        ))
        .child(appearance_radio_option(
            "appearance-typing-only",
            "Typing indicator only",
            "Show just the typing indicator without avatars",
            false,
            theme,
            |_, _, _, _| {
                // TODO(backlog): persist channel list typing indicator mode (indicator only)
            },
            cx,
        ))
        .child(appearance_radio_option(
            "appearance-typing-hidden",
            "Hidden",
            "Don't show typing indicators in the channel list",
            false,
            theme,
            |_, _, _, _| {
                // TODO(backlog): persist channel list typing indicator mode (hidden)
            },
            cx,
        ))
        .into_any_element();

    let channel_list_content = div()
        .w_full()
        .h(px(44.))
        .flex()
        .flex_col()
        .justify_center()
        .child(appearance_switch_row(
            "appearance-muted-unread",
            "Show unread indicator on muted channels",
            None,
            false,
            theme,
            |_, _, _, _| {
                // TODO(backlog): persist muted channel unread indicator
            },
            cx,
        ))
        .into_any_element();

    let active_now_content = div()
        .w_full()
        .h(px(72.))
        .flex()
        .flex_col()
        .justify_center()
        .child(appearance_switch_row(
            "appearance-active-now",
            "Show active now on the home screen",
            Some("Show active now on the home screen to surface friends active in voice."),
            true,
            theme,
            |_, _, _, _| {
                // TODO(backlog): persist active now home visibility
            },
            cx,
        ))
        .into_any_element();

    let streamer_mode_content = div()
        .w_full()
        .h(px(380.8))
        .flex()
        .flex_col()
        .gap_3()
        .child(appearance_switch_row(
            "appearance-streamer-manual",
            "Turn on streaming privacy",
            Some("Mask private details before sharing your screen or stream."),
            false,
            theme,
            |_, _, _, _| {
                // TODO(backlog): persist streamer mode manual toggle
            },
            cx,
        ))
        .child(appearance_switch_row(
            "appearance-streamer-auto",
            "Turn on automatically when OBS or XSplit is detected",
            None,
            false,
            theme,
            |_, _, _, _| {
                // TODO(backlog): persist streamer mode auto-enable toggle
            },
            cx,
        ))
        .child(
            div()
                .pt_2()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.muted_foreground)
                .child(l10n::tr("When streaming privacy is on")),
        )
        .child(appearance_switch_row(
            "appearance-streamer-hide-personal",
            "Mask private account details like email, linked accounts, and notes",
            None,
            true,
            theme,
            |_, _, _, _| {
                // TODO(backlog): persist streamer mode hide personal info
            },
            cx,
        ))
        .child(appearance_switch_row(
            "appearance-streamer-hide-invites",
            "Mask community invite links",
            None,
            true,
            theme,
            |_, _, _, _| {
                // TODO(backlog): persist streamer mode hide invites
            },
            cx,
        ))
        .child(appearance_switch_row(
            "appearance-streamer-disable-sounds",
            "Mute app sound effects",
            None,
            true,
            theme,
            |_, _, _, _| {
                // TODO(backlog): persist streamer mode disable sounds
            },
            cx,
        ))
        .child(appearance_switch_row(
            "appearance-streamer-disable-notifications",
            "Pause notifications",
            None,
            true,
            theme,
            |_, _, _, _| {
                // TODO(backlog): persist streamer mode disable notifications
            },
            cx,
        ))
        .into_any_element();

    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(20.))
        .child(appearance_section(true, l10n::tr("Theme"), themes_link_content, theme))
        .child(appearance_section(false, l10n::tr("High Dynamic Range"), hdr_content, theme))
        .child(appearance_section(false, l10n::tr("App zoom level"), zoom_content, theme))
        .child(appearance_section(false, l10n::tr("Messages"), messages_content, theme))
        .child(appearance_section(false, l10n::tr("Interface"), interface_content, theme))
        .child(appearance_section(false, l10n::tr("Channel List"), channel_list_content, theme))
        .child(appearance_section(false, l10n::tr("Active Now"), active_now_content, theme))
        .child(appearance_section(false, l10n::tr("Streaming privacy"), streamer_mode_content, theme))
        .into_any_element()
}
