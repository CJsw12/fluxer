use super::*;

pub(super) fn render_profile_tab(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let (avatar_el, display_name, user_id_str, banner_rgb, banner_url) = match &view.user {
        Some(user) => {
            let card = crate::profile::build_profile_card(&view.backend.state(), view.backend.media(), user.id, None, None);
            (
                render_avatar(
                    AvatarSpec {
                        user_id: user.id,
                        avatar_url: user.avatar_url.clone(),
                        display_name: user.name.clone(),
                        size: 64.,
                        status: Some(view.own_status),
                        status_border_bg: Some(theme.background_secondary),
                    },
                    theme,
                ),
                user.name.clone(),
                user.id.to_string(),
                Some(card.banner_color),
                card.banner_url,
            )
        }
        None => (
            div().size(px(64.)).rounded_full().bg(theme.muted).into_any_element(),
            SharedString::new_static("Fluxer User"),
            String::from("—"),
            None,
            None,
        ),
    };

    let banner = match banner_rgb {
        Some(color) => div().bg(gpui_kit::rgb(color)),
        None => div().bg(theme.brand),
    }
    .relative()
    .w_full()
    .h(px(90.))
    .overflow_hidden()
    .children(banner_url.map(|url| {
        gpui_kit::img(url)
            .id("settings-profile-banner")
            .size_full()
            .object_fit(gpui_kit::ObjectFit::Cover)
    }));

    let header_block = div()
        .relative()
        .w_full()
        .pb(px(42.))
        .child(banner)
        .child(
            div()
                .absolute()
                .left(px(16.))
                .top(px(54.))
                .p(px(6.))
                .rounded_full()
                .bg(theme.background_secondary)
                .child(avatar_el),
        );

    let status_options = [
        (Status::Online, "online"),
        (Status::Idle, "idle"),
        (Status::Dnd, "dnd"),
        (Status::Invisible, "invisible"),
    ];

    div()
        .flex()
        .flex_col()
        .gap_5()
        .child(
            div()
                .debug_selector(|| "settings-profile-customization".to_owned())
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Profile customization"), theme))
                .child(
                    div()
                        .overflow_hidden()
                        .rounded(px(10.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background_secondary)
                        .flex()
                        .flex_col()
                        .child(header_block)
                        .child(
                            div()
                                .px_4()
                                .pb_4()
                                .flex()
                                .flex_col()
                                .gap_4()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .min_w_0()
                                        .child(
                                            div()
                                                .text_base()
                                                .font_weight(FontWeight::BOLD)
                                                .truncate()
                                                .child(display_name),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child(format!("ID: {user_id_str}")),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_1p5()
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(theme.muted_foreground)
                                                .child(l10n::tr("Online presence")),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .flex_row()
                                                .flex_wrap()
                                                .gap_2()
                                                .children(status_options.into_iter().map(|(st, key)| {
                                            let selected = view.own_status == st;
                                            let dot_col = status_color(st, theme);
                                            let label = status_label(st);
                                            let sel_id = format!("settings-profile-status-{key}");
                                            div()
                                                .id(SharedString::from(sel_id.clone()))
                                                .debug_selector(move || sel_id.clone())
                                                .px_3()
                                                .py_1p5()
                                                .rounded(px(6.))
                                                .border_1()
                                                .border_color(if selected { theme.brand } else { theme.border })
                                                .bg(if selected { theme.accent } else { theme.background })
                                                .cursor_pointer()
                                                .flex()
                                                .flex_row()
                                                .items_center()
                                                .gap_1p5()
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.own_status = st;
                                                    this.backend.set_presence(st);
                                                    cx.notify();
                                                }))
                                                .child(div().size(px(8.)).rounded_full().bg(dot_col))
                                                .child(div().text_xs().font_weight(FontWeight::MEDIUM).child(label))
                                        })),
                                        ),
                                ),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Profile & status"), theme))
                .child(info_card_row(
                    l10n::tr("Custom status & avatar"),
                    l10n::tr("Synced with your Fluxer account across all signed-in devices."),
                    Some(l10n::tr("Active")),
                    theme,
                )),
        )
        .into_any_element()
}
