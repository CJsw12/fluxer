use super::*;

pub(super) fn render_connections_tab(modal: &SettingsModal, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let platform_row = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap_3()
        .mb_6()
        .flx("user.linked-accounts-tab.platform-row")
        .child(
            div()
                .debug_selector(|| "settings-connections-add-domain".to_owned())
                .flx("user.linked-accounts-tab.platform-icon-button.add-connection--2")
                .child(
                    Button::new("settings-connections-add-domain")
                        .ghost()
                        .size(px(40.))
                        .p_0()
                        .rounded(px(6.))
                        .bg(theme.on_fill)
                        .accessibility_label(l10n::tr("Add domain"))
                        .tooltip(l10n::tr("Add domain"))
                        .child(Icon::new(AppIcon::LinkSimple).with_size(px(28.)).text_color(theme.brand))
                        .on_click(cx.listener(|this, _, _, cx| {
                            // TODO(backlog, T3): Connect POST /users/@me/connections domain verification flow.
                            this.settings_set_connections_notice(
                                l10n::tr("Adding domain connections requires OAuth/DNS verification (coming soon)."),
                                cx,
                            );
                        })),
                ),
        )
        .child(
            div()
                .debug_selector(|| "settings-connections-add-oauth".to_owned())
                .child(
                    Button::new("settings-connections-add-oauth")
                        .ghost()
                        .small()
                        .icon(AppIcon::Plus)
                        .label(l10n::tr("Add external account"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            // TODO(backlog, T3): Connect OAuth external account linking flow.
                            this.settings_set_connections_notice(
                                l10n::tr("Linking external OAuth accounts is not yet enabled in this client."),
                                cx,
                            );
                        })),
                ),
        );

    let list_body: AnyElement = if modal.connections_loading && modal.connections.is_empty() {
        div()
            .debug_selector(|| "settings-connections-loading".to_owned())
            .p_4()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(l10n::tr("Loading connections…"))
            .into_any_element()
    } else if let Some(err) = &modal.connections_error {
        div()
            .debug_selector(|| "settings-connections-error".to_owned())
            .p_4()
            .rounded(px(8.))
            .border_1()
            .border_color(settings_border(theme))
            .bg(settings_card_bg(theme))
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap_3()
            .child(div().text_xs().text_color(theme.danger).child(err.clone()))
            .child(
                Button::new("settings-connections-retry")
                    .ghost()
                    .small()
                    .label(l10n::tr("Try again"))
                    .on_click(cx.listener(|this, _, _, cx| this.settings_reload_connections(cx))),
            )
            .into_any_element()
    } else if modal.connections.is_empty() {
        div()
            .debug_selector(|| "settings-connections-empty".to_owned())
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .min_h(px(132.))
            .gap_3()
            .text_center()
            .line_height(px(24.))
            .flx("user.linked-accounts-tab.empty-state")
            .child(Icon::new(AppIcon::UserListFill).with_size(px(44.)).text_color(theme.muted_foreground))
            .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child(l10n::tr("No connections yet")))
            .child(
                div()
                    .text_base()
                    .text_color(theme.muted_foreground)
                    .child(l10n::tr("Verify domain ownership or link accounts to display them on your profile.")),
            )
            .into_any_element()
    } else {
        div()
            .debug_selector(|| "settings-connections-list".to_owned())
            .flex()
            .flex_col()
            .gap_2()
            .flx("user.linked-accounts-tab.connections-list")
            .children(modal.connections.iter().map(|conn| {
                let conn_id = conn.id.clone();
                let conn_kind = conn.kind.clone();
                let conn_name = conn.name.clone();
                let verified = conn.verified;
                let visible = (conn.visibility_flags & CONNECTION_VISIBILITY_EVERYONE) != 0;
                let provider_label = match conn.kind.as_str() {
                    "bsky" => "Bluesky",
                    "domain" => "Domain",
                    "oauth" => "OAuth",
                    other => other,
                };
                let card_sel = format!("settings-connection-card-{}", conn.id);
                let toggle_id = format!("settings-connection-visible-{}", conn.id);
                let remove_id = format!("settings-connection-remove-{}", conn.id);
                let toggle_kind = conn_kind.clone();
                let toggle_conn_id = conn_id.clone();
                let remove_name = conn_name.clone();
                div()
                    .debug_selector(move || card_sel.clone())
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
                    .flx("user.linked-accounts-tab.connection-card.card")
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .min_w_0()
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_2()
                                    .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(conn_name))
                                    .child(
                                        div()
                                            .px_1p5()
                                            .py_0p5()
                                            .rounded(px(4.))
                                            .bg(theme.muted)
                                            .text_size(px(11.))
                                            .text_color(if verified { theme.brand } else { theme.muted_foreground })
                                            .child(if verified {
                                                l10n::tr("Verified")
                                            } else {
                                                l10n::tr("Unverified")
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(format!("{provider_label} · {}", if visible { "Shown on profile" } else { "Hidden" })),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(
                                Switch::new(SharedString::from(toggle_id))
                                    .checked(visible)
                                    .on_change(cx.listener(move |this, _, _, cx| {
                                        this.settings_toggle_connection_visibility(
                                            toggle_kind.clone(),
                                            toggle_conn_id.clone(),
                                            visible,
                                            cx,
                                        );
                                    })),
                            )
                            .child(
                                Button::new(SharedString::from(remove_id))
                                    .ghost()
                                    .small()
                                    .icon(AppIcon::Trash)
                                    .tooltip(l10n::tr("Remove connection"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        // TODO(backlog, T3): Connect DELETE /users/@me/connections/{type}/{id}.
                                        this.settings_set_connections_notice(
                                            format!(
                                                "{} ({remove_name})",
                                                l10n::tr("Removing connections is deferred to the T3 workflow.")
                                            ),
                                            cx,
                                        );
                                    })),
                            ),
                    )
            }))
            .into_any_element()
    };

    div()
        .debug_selector(|| "settings-connections-tab".to_owned())
        .flex()
        .flex_col()
        .gap_4()
        .child(settings_section_block(
            l10n::tr("Connections"),
            Some(l10n::tr(
                "Link external accounts and domains to your Fluxer profile. Verified connections will be displayed on your profile for others to see.",
            )),
            div()
                .flex()
                .flex_col()
                .gap_3()
                .child(platform_row)
                .when_some(modal.connections_notice.clone(), |d, notice| {
                    d.child(
                        div()
                            .debug_selector(|| "settings-connections-notice".to_owned())
                            .p_2p5()
                            .rounded(px(8.))
                            .border_1()
                            .border_color(settings_border(theme))
                            .bg(settings_card_bg(theme))
                            .text_xs()
                            .text_color(theme.foreground)
                            .child(notice),
                    )
                })
                .child(list_body)
                .into_any_element(),
            theme,
        ))
        .into_any_element()
}
