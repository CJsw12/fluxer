use super::*;

pub(super) fn render_tab_body(
    tab: SettingsTab,
    view: &ClientView,
    modal: &SettingsModal,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    match tab {
        SettingsTab::Profile => render_profile_tab(view, theme, cx),
        SettingsTab::Account => render_account_tab(view, modal, theme, cx),
        SettingsTab::PrivacySafety => render_privacy_tab(view, modal, theme, cx),
        SettingsTab::Connections => render_connections_tab(modal, theme, cx),
        SettingsTab::Appearance => render_appearance_tab(view, theme, cx),
        SettingsTab::Themes => render_themes_tab(view, theme, cx),
        SettingsTab::Notifications => render_notifications_tab(view, theme, cx),
        SettingsTab::MessagesMedia => render_messages_media_tab(view, theme, cx),
        SettingsTab::VoiceVideo => render_voice_tab(view, modal.active_section, theme, cx),
        SettingsTab::Accessibility => render_accessibility_tab(view, theme, cx),
        SettingsTab::Language => render_language_tab(view, modal, theme, cx),
        SettingsTab::Keybinds => render_keybinds_tab(view, modal, theme, cx),
        SettingsTab::WindowsApp => render_windows_app_tab(view, theme, cx),
        SettingsTab::Advanced => render_advanced_tab(view, theme, cx),
        SettingsTab::Applications => render_applications_tab(modal, theme, cx),
    }
}

pub(super) fn render_sidebar_tab(
    tab: SettingsTab,
    selected: bool,
    active_section: Option<&'static str>,
    show_sections: bool,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> Vec<AnyElement> {
    let sel_id = tab.debug_selector();
    let sections = tab.sections();
    let selected_bg = settings_sub_selected_bg(theme);
    let active_text = theme.foreground;
    let idle_text = theme.muted_foreground;
    let sub_idle_text = theme.text_tertiary;
    let hover_bg = theme.hover_bg;
    let has_sections = sections.len() > 1;
    let has_tree = selected && show_sections && has_sections;

    let item_btn = div()
        .id(sel_id)
        .group(sel_id)
        .debug_selector(move || sel_id.to_owned())
        .w_full()
        .h(px(34.))
        .px(px(10.))
        .rounded(px(8.))
        .border_1()
        .border_color(gpui_kit::transparent_black())
        .cursor_pointer()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(if selected { active_text } else { idle_text })
        .when(selected, |d| d.bg(selected_bg))
        .when(!selected, |d| {
            d.hover(move |s| s.bg(hover_bg).text_color(active_text))
        })
        .flx("app.settings-modal-layout.settings-modal-sidebar-item.button")
        .on_click(cx.listener(move |this, _, _, cx| {
            this.select_settings_tab(tab, cx);
        }))
        .child(
            div()
                .size(px(20.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .flx("app.settings-modal-layout.settings-modal-sidebar-item.sidebar-item-icon")
                .child(Icon::new(tab.icon()).with_size(px(20.))),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h(px(18.))
                .text_size(px(13.))
                .line_height(px(18.))
                .truncate()
                .flx("app.settings-modal-layout.settings-modal-sidebar-item.sidebar-item-label")
                .child(tab.label()),
        )
        .when(has_sections, |d| {
            d.child(
                div()
                    .size(px(16.))
                    .ml(px(4.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .flx("app.settings-modal-layout.settings-modal-sidebar-item.sidebar-item-trailing")
                    .child(
                        div()
                            .size(px(16.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .opacity(if selected || has_tree { 1. } else { 0. })
                            .when(!selected && !has_tree, |d| {
                                d.group_hover(sel_id, |s| s.opacity(1.))
                            })
                            .flx("app.settings-modal-layout.settings-modal-sidebar-item.sidebar-item-chevron")
                            .child(
                                Icon::new(if has_tree {
                                    AppIcon::CaretDown
                                } else {
                                    AppIcon::CaretRight
                                })
                                .with_size(px(16.))
                                .text_color(sub_idle_text),
                            ),
                    ),
            )
        });

    let mut out = vec![item_btn.into_any_element()];

    if show_sections && has_sections && tab != SettingsTab::Profile {
        // Collapsed tabs keep their rows inside zero-height wrappers like the
        // official sidebar; only the selected tab's list is visible.
        let mut sub_list = if has_tree {
            div().ml(px(21.)).pl(px(6.)).border_l_1().border_color(theme.border)
        } else {
            div().ml(px(21.)).pl(px(6.)).h(px(0.)).overflow_hidden().opacity(0.)
        }
        .flex()
        .flex_col()
        .gap(px(2.))
        .flx("app.settings-modal-layout.settings-modal-sidebar-sub-items.sidebar-sub-items");
        for &(sec_id, sec_label) in sections.iter() {
            let is_active_sec = active_section == Some(sec_id);
            let sub_sel = format!("settings-subitem-{sec_id}");
            let sub_id = SharedString::from(format!("settings-subitem-{}-{sec_id}", tab.debug_selector()));
            let label_el = div()
                .flex_1()
                .min_w_0()
                .h(px(18.))
                .line_height(px(18.))
                .truncate()
                .flx("app.settings-modal-layout.settings-modal-sidebar-sub-item.sidebar-sub-item-label")
                .child(l10n::tr(sec_label));
            // Tagged before `.id(..)` so the probe can scope the row's children.
            let row = div()
                .debug_selector(move || sub_sel.clone())
                .w_full()
                .h(px(30.))
                .flex_none()
                .px(px(10.))
                .rounded(px(6.))
                .border_1()
                .border_color(gpui_kit::transparent_black())
                .cursor_pointer()
                .flex()
                .items_center()
                .text_size(px(13.))
                .line_height(px(18.))
                .text_color(if is_active_sec { active_text } else { sub_idle_text })
                .font_weight(if is_active_sec {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::MEDIUM
                })
                .bg(if is_active_sec {
                    selected_bg
                } else {
                    gpui_kit::transparent_black()
                })
                .when(!is_active_sec, |d| {
                    d.hover(move |s| s.bg(hover_bg).text_color(active_text))
                })
                .flx("app.settings-modal-layout.settings-modal-sidebar-sub-item.sidebar-sub-item.click.button")
                .id(sub_id)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.select_settings_section(tab, sec_id, cx);
                }))
                .child(label_el);
            sub_list = sub_list.child(row);
        }
        let wrap = if has_tree {
            div().w_full().mt(px(3.)).mb(px(4.))
        } else {
            div().w_full().h(px(0.)).overflow_hidden()
        }
        .flx("app.settings-modal-layout.settings-modal-sidebar-sub-items.sidebar-sub-items-wrap")
        .child(sub_list);
        out.push(wrap.into_any_element());
    }

    out
}

pub(super) fn render_sidebar(
    view: &ClientView,
    modal: &SettingsModal,
    query_lower: &str,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let sidebar_bg = settings_sidebar_bg(theme);
    let hover_bg = theme.hover_bg;
    let foreground = theme.foreground;
    let danger_hover = theme.danger.opacity(0.10);
    let (avatar_el, user_name) = match &view.user {
        Some(user) => (
            div()
                .debug_selector(|| "settings-profile-avatar".to_owned())
                .child(render_avatar(
                    AvatarSpec {
                        user_id: user.id,
                        avatar_url: user.avatar_url.clone(),
                        display_name: user.name.clone(),
                        size: 40.,
                        status: Some(view.own_status),
                        status_border_bg: Some(sidebar_bg),
                    },
                    theme,
                ))
                .into_any_element(),
            user.name.clone(),
        ),
        None => (
            div().size(px(40.)).rounded_full().bg(theme.muted).into_any_element(),
            SharedString::new_static("…"),
        ),
    };

    let account_tabs: Vec<SettingsTab> = SettingsTab::ACCOUNT_TABS
        .iter()
        .copied()
        .filter(|t| t.matches_query(query_lower))
        .collect();
    let app_tabs: Vec<SettingsTab> = SettingsTab::APP_TABS
        .iter()
        .copied()
        .filter(|t| t.matches_query(query_lower))
        .collect();
    let developer_tabs: Vec<SettingsTab> = SettingsTab::DEVELOPER_TABS
        .iter()
        .copied()
        .filter(|t| t.matches_query(query_lower))
        .collect();
    let show_sections = query_lower.is_empty();
    let profile_selected = modal.active_tab == SettingsTab::Profile;

    let footer = if modal.confirm_logout {
        div()
            .flex_none()
            .px_3()
            .py_3()
            .flex()
            .flex_col()
            .gap_2()
            .child(settings_divider(theme))
            .child(div().pt_1().text_sm().font_weight(FontWeight::SEMIBOLD).child(l10n::tr("Sign out?")))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(l10n::tr("You can sign back in at any time.")),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        div().debug_selector(|| "settings-logout-confirm".to_owned()).child(
                            Button::new("settings-logout-confirm")
                                .danger()
                                .small()
                                .label(l10n::tr("Sign out"))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.apply_settings_effect(SettingsEffect::LogOutRequested, window, cx);
                                })),
                        ),
                    )
                    .child(
                        Button::new("settings-logout-cancel")
                            .ghost()
                            .small()
                            .label(l10n::tr("Stay signed in"))
                            .on_click(cx.listener(|this, _, _, cx| this.settings_prompt_logout(false, cx))),
                    ),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(theme.muted_foreground)
                    .opacity(0.65)
                    .child(format!("Fluxer Rust v{CURRENT_VERSION}")),
            )
            .into_any_element()
    } else {
        div()
            .flex_none()
            .w_full()
            .py(px(16.))
            .child(
                div()
                    .w_full()
                    .min_h(px(74.))
                    .pl(px(12.))
                    .flex()
                    .flex_col()
                    .justify_between()
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .when(view.accounts_enabled, |d| {
                                let switch_label = if view.saved_accounts.len() > 1 {
                                    format!("{} ({})", l10n::tr("Switch accounts"), view.saved_accounts.len())
                                } else {
                                    l10n::tr("Switch accounts")
                                };
                                d.child(
                                    div()
                                        .id("settings-switch-accounts")
                                        .debug_selector(|| "settings-switch-accounts".to_owned())
                                        .h(px(32.))
                                        .px_2p5()
                                        .rounded(px(8.))
                                        .cursor_pointer()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap_2p5()
                                        .text_color(theme.muted_foreground)
                                        .hover(move |s| s.bg(hover_bg).text_color(foreground))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.apply_settings_effect(SettingsEffect::OpenAccountSwitcher, window, cx);
                                        }))
                                        .child(Icon::new(AppIcon::Users).with_size(px(16.)))
                                        .child(div().text_size(px(13.)).truncate().child(switch_label)),
                                )
                            })
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .line_height(px(14.))
                                    .text_color(theme.muted_foreground)
                                    .opacity(0.65)
                                    .child(format!("Fluxer Rust v{CURRENT_VERSION}")),
                            ),
                    )
                    .child(div().h(px(12.))),
            )
            .into_any_element()
    };

    let no_tabs_found = account_tabs.is_empty() && app_tabs.is_empty() && developer_tabs.is_empty();
    let sign_out_item = (!no_tabs_found).then(|| {
        div()
            .id("settings-logout")
            .debug_selector(|| "settings-logout".to_owned())
            .w_full()
            .h(px(34.))
            .px(px(10.))
            .rounded(px(8.))
            .border_1()
            .border_color(gpui_kit::transparent_black())
            .cursor_pointer()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .text_color(theme.settings_danger_text)
            .text_size(px(13.))
            .line_height(px(18.))
            .font_weight(FontWeight::MEDIUM)
            .hover(move |s| s.bg(danger_hover))
            .flx("app.settings-modal-layout.settings-modal-sidebar-item.button--2")
            .on_click(cx.listener(|this, _, _, cx| this.settings_prompt_logout(true, cx)))
            .child(
                div()
                    .size(px(20.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .flx("app.settings-modal-layout.settings-modal-sidebar-item.sidebar-item-icon")
                    .child(Icon::new(AppIcon::SignOutFill).with_size(px(20.)).text_color(theme.danger)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(px(18.))
                    .line_height(px(18.))
                    .truncate()
                    .flx("app.settings-modal-layout.settings-modal-sidebar-item.sidebar-item-label")
                    .child(l10n::tr("Sign out")),
            )
    });

    div()
        .flex_1()
        .min_h_0()
        .w_full()
        .px(px(12.))
        .pt(px(16.))
        .flex()
        .flex_col()
        .child(
            div()
                .w_full()
                .h(px(106.))
                .pb(px(8.))
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(10.))
                .flx("app.settings-modal-layout.settings-modal-sidebar-nav.sidebar-header")
                .child(
                    div()
                        .w_full()
                        .h(px(34.))
                        .flex_none()
                        .flx("app.settings-search.container")
                        .child(
                            div()
                                .relative()
                                .w_full()
                                .h(px(34.))
                                .flx("app.settings-search.input-container")
                                .child(
                                    div()
                                        .debug_selector(|| "settings-search".to_owned())
                                        .w_full()
                                        .h(px(34.))
                                        .rounded(px(8.))
                                        .bg(sidebar_bg)
                                        .border_1()
                                        .border_color(settings_border(theme))
                                        .pl(px(36.))
                                        .pr(px(16.))
                                        .flex()
                                        .items_center()
                                        .flx("app.settings-search.input.query-change.text")
                                        .child(Input::new(&modal.search).appearance(false).small()),
                                )
                                .child(
                                    div()
                                        .absolute()
                                        .left(px(12.))
                                        .top(px(9.))
                                        .size(px(16.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .flx("app.settings-search.magnifying-glass-icon")
                                        .child(
                                            Icon::new(AppIcon::MagnifyingGlass)
                                                .with_size(px(16.))
                                                .text_color(theme.muted_foreground),
                                        ),
                                ),
                        ),
                )
                .child(
                    div()
                        .id("settings-profile-card")
                        .debug_selector(|| "settings-profile-card".to_owned())
                        .w_full()
                        .h(px(54.))
                        .flex_none()
                        .px(px(9.))
                        .py(px(7.))
                        .rounded(px(8.))
                        .bg(if profile_selected {
                            settings_sub_selected_bg(theme)
                        } else {
                            gpui_kit::transparent_black()
                        })
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover_bg))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.select_settings_tab(SettingsTab::Profile, cx);
                        }))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2p5()
                        .child(div().flex_none().child(avatar_el))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).truncate().child(user_name))
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap_1()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(Icon::new(AppIcon::PencilBold).with_size(px(12.)))
                                        .child(div().truncate().child(l10n::tr("Edit your profile"))),
                                ),
                        ),
                ),
        )
        .child(
            div()
                .w_full()
                .mt(px(4.))
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .flx("app.settings-modal-layout.settings-modal-sidebar-nav.sidebar-nav-wrapper")
                .relative()
                .child(
                    div()
                        .absolute()
                        .left(px(-1.))
                        .top(px(-1.))
                        .w(px(1.))
                        .h(px(1.))
                        .overflow_hidden()
                        .flx("app.settings-modal-layout.settings-modal-sidebar-nav.sidebar-nav-instructions"),
                )
                .child(
                    div()
                        .mr(px(-8.))
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .flx("app.settings-modal-layout.settings-modal-sidebar-nav.sidebar-nav")
                        .child(
                            div()
                                .id("settings-sidebar-scroll")
                                .w_full()
                                .pr(px(8.))
                                .flex_1()
                                .min_h_0()
                                .overflow_y_scroll()
                                .track_scroll(&modal.sidebar_scroll)
                                .flex()
                                .flex_col()
                                .flx("app.settings-modal-layout.settings-modal-sidebar-nav.sidebar-nav-content")
                                .child(
                                    div()
                                        .w_full()
                                        .flex()
                                        .flex_col()
                                        .gap(px(3.))
                                        .flx("app.settings-modal-layout.settings-modal-sidebar-nav.sidebar-nav-list.sidebar-nav-key-down")
                                        .when(!account_tabs.is_empty(), |d| {
                                            d.child(
                                                div()
                                                    .w_full()
                                                    .mb(px(8.))
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(3.))
                                                    .flx("app.settings-modal-layout.settings-modal-sidebar-category.sidebar-category")
                                                    .child(
                                                        div()
                                                            .w_full()
                                                            .px(px(10.))
                                                            .pt(px(4.))
                                                            .pb(px(3.))
                                                            .mb(px(1.))
                                                            .flx("app.settings-modal-layout.settings-modal-sidebar-category-title.sidebar-category-title")
                                                            .child(sidebar_section_heading(
                                                                l10n::tr("Your account"),
                                                                theme,
                                                            )),
                                                    )
                                                    .children(account_tabs.iter().flat_map(|&tab| {
                                                        render_sidebar_tab(
                                                            tab,
                                                            modal.active_tab == tab,
                                                            modal.active_section,
                                                            show_sections,
                                                            theme,
                                                            cx,
                                                        )
                                                    })),
                                            )
                                        })
                                        .when(!app_tabs.is_empty(), |d| {
                                            d.child(
                                                div()
                                                    .w_full()
                                                    .mb(px(8.))
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(3.))
                                                    .flx("app.settings-modal-layout.settings-modal-sidebar-category.sidebar-category")
                                                    .child(
                                                        div()
                                                            .w_full()
                                                            .px(px(10.))
                                                            .pt(px(4.))
                                                            .pb(px(3.))
                                                            .mb(px(1.))
                                                            .flx("app.settings-modal-layout.settings-modal-sidebar-category-title.sidebar-category-title")
                                                            .child(sidebar_section_heading(
                                                                l10n::tr("App settings"),
                                                                theme,
                                                            )),
                                                    )
                                                    .children(app_tabs.iter().flat_map(|&tab| {
                                                        render_sidebar_tab(
                                                            tab,
                                                            modal.active_tab == tab,
                                                            modal.active_section,
                                                            show_sections,
                                                            theme,
                                                            cx,
                                                        )
                                                    })),
                                            )
                                        })
                                        .when(!developer_tabs.is_empty(), |d| {
                                            d.child(
                                                div()
                                                    .w_full()
                                                    .mb(px(8.))
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(3.))
                                                    .flx("app.settings-modal-layout.settings-modal-sidebar-category.sidebar-category")
                                                    .child(
                                                        div()
                                                            .w_full()
                                                            .px(px(10.))
                                                            .pt(px(4.))
                                                            .pb(px(3.))
                                                            .mb(px(1.))
                                                            .flx("app.settings-modal-layout.settings-modal-sidebar-category-title.sidebar-category-title")
                                                            .child(sidebar_section_heading(
                                                                l10n::tr("Developer"),
                                                                theme,
                                                            )),
                                                    )
                                                    .children(developer_tabs.iter().flat_map(|&tab| {
                                                        render_sidebar_tab(
                                                            tab,
                                                            modal.active_tab == tab,
                                                            modal.active_section,
                                                            show_sections,
                                                            theme,
                                                            cx,
                                                        )
                                                    })),
                                            )
                                        })
                                        .children(sign_out_item)
                                        .when(no_tabs_found, |d| {
                                            d.child(
                                                div()
                                                    .px_2()
                                                    .py_2()
                                                    .text_xs()
                                                    .text_color(theme.muted_foreground)
                                                    .child(l10n::tr("No results")),
                                            )
                                        }),
                                )
                                .child(
                                    div()
                                        .w_full()
                                        .mt_auto()
                                        .flx("app.settings-modal-layout.settings-modal-sidebar-nav.sidebar-nav-footer")
                                        .child(
                                            div()
                                                .w_full()
                                                .flx("app.settings-modal-layout.settings-modal-sidebar-footer.sidebar-footer")
                                                .child(footer),
                                        ),
                                ),
                        ),
                ),
        )
        .into_any_element()
}

pub fn render(view: &ClientView, modal: &SettingsModal, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let query = modal.search.read(cx).value().trim().to_owned();
    let query_lower = query.to_lowercase();

    let sidebar = render_sidebar(view, modal, &query_lower, theme, cx);

    let (title, page_link, body): (SharedString, Option<String>, AnyElement) = if query_lower.is_empty() {
        let link = modal
            .selected_application_id
            .is_none()
            .then(|| modal.active_tab.deep_link_url().to_owned());
        (
            SharedString::from(modal.active_tab.label()),
            link,
            render_tab_body(modal.active_tab, view, modal, theme, cx),
        )
    } else {
        let matching: Vec<SettingsTab> = SettingsTab::ALL
            .iter()
            .copied()
            .filter(|t| t.matches_query(&query_lower))
            .collect();
        let search_body = if matching.is_empty() {
            div()
                .py_6()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(l10n::tr("No results"))
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .gap_6()
                .children(matching.into_iter().map(|tab| {
                    div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(div().text_base().font_weight(FontWeight::BOLD).child(tab.label()))
                        .child(render_tab_body(tab, view, modal, theme, cx))
                }))
                .into_any_element()
        };
        (SharedString::from(l10n::tr("Search results")), None, search_body)
    };

    let preview = (query_lower.is_empty() && modal.active_tab == SettingsTab::Appearance)
        .then(|| render_appearance_preview(view, theme));

    let view_weak = cx.weak_entity();
    div()
        .id("settings-modal")
        .key_context(CONTEXT)
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .capture_action(cx.listener(|this, _: &Escape, window, cx| {
            cx.stop_propagation();
            if this.is_recording_keybind() {
                this.settings_cancel_record_keybind(cx);
            } else {
                this.close_overlay(window, cx);
            }
        }))
        .child(overlay::settings_modal_with_preview(
            sidebar,
            title,
            page_link,
            preview,
            body,
            theme,
            move |window, cx| {
                let _ = view_weak.update(cx, |v, cx| v.close_overlay(window, cx));
            },
        ))
        .into_any_element()
}
