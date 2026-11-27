use super::*;

pub(super) fn render_applications_tab(modal: &SettingsModal, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    const DOCS_URL: &str = "https://docs.fluxer.app";

    if let Some(selected_id) = &modal.selected_application_id {
        let selected_app = modal.applications.iter().find(|a| &a.id == selected_id);
        let detail_body: AnyElement = if let Some(app) = selected_app {
            let app_name = app.name.clone();
            let app_id = app.id.clone();
            let redirect_summary = if app.redirect_uris.is_empty() {
                l10n::tr("None configured")
            } else {
                app.redirect_uris.join(", ")
            };
            let bot_summary = match &app.bot {
                Some(bot) => format!("{}#{} (ID: {})", bot.username, bot.discriminator, bot.id),
                None => l10n::tr("No bot user attached"),
            };
            div()
                .debug_selector(|| "settings-application-detail".to_owned())
                .flex()
                .flex_col()
                .gap_3()
                .child(info_card_row(
                    l10n::tr("Application name"),
                    app_name.clone(),
                    Some(format!("ID: {app_id}")),
                    theme,
                ))
                .child(info_card_row(
                    l10n::tr("Bot user"),
                    bot_summary,
                    Some(if app.bot_public {
                        l10n::tr("Public")
                    } else {
                        l10n::tr("Private")
                    }),
                    theme,
                ))
                .child(info_card_row(
                    l10n::tr("OAuth2 redirect URIs"),
                    redirect_summary,
                    Some(if app.bot_require_code_grant {
                        l10n::tr("Code grant required")
                    } else {
                        l10n::tr("Standard")
                    }),
                    theme,
                ))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .gap_2()
                        .child(
                            Button::new("settings-application-reset-token")
                                .ghost()
                                .small()
                                .label(l10n::tr("Reset bot token"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    // TODO(backlog, T3): Connect bot token reset flow with sudo confirmation.
                                    this.settings_set_applications_notice(
                                        l10n::tr("Resetting bot tokens is deferred to the T3 workflow."),
                                        cx,
                                    );
                                })),
                        )
                        .child(
                            Button::new("settings-application-delete")
                                .danger()
                                .small()
                                .label(l10n::tr("Delete application"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    // TODO(backlog, T3): Connect DELETE /oauth2/applications/{id} with confirmation.
                                    this.settings_set_applications_notice(
                                        format!(
                                            "{} ({app_name})",
                                            l10n::tr("Deleting applications is deferred to the T3 workflow.")
                                        ),
                                        cx,
                                    );
                                })),
                        ),
                )
                .into_any_element()
        } else if modal.selected_application_loading {
            div()
                .p_4()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(l10n::tr("Loading application details…"))
                .into_any_element()
        } else {
            div()
                .p_4()
                .text_xs()
                .text_color(theme.danger)
                .child(
                    modal
                        .applications_error
                        .clone()
                        .unwrap_or_else(|| l10n::tr("Unable to load application details.").into()),
                )
                .into_any_element()
        };

        return div()
            .debug_selector(|| "settings-applications-tab".to_owned())
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .debug_selector(|| "settings-application-back".to_owned())
                    .child(
                        Button::new("settings-application-back")
                            .ghost()
                            .small()
                            .icon(AppIcon::ArrowBendUpLeft)
                            .label(l10n::tr("Back to applications"))
                            .on_click(cx.listener(|this, _, _, cx| this.settings_back_to_applications(cx))),
                    ),
            )
            .when_some(modal.applications_notice.clone(), |d, notice| {
                d.child(
                    div()
                        .debug_selector(|| "settings-applications-notice".to_owned())
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
            .child(settings_section_block(l10n::tr("Application details"), None, detail_body, theme))
            .into_any_element();
    }

    let action_bar = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap_3()
        .flx("user.applications-tab.button-container")
        .child(
            div()
                .debug_selector(|| "settings-applications-create".to_owned())
                .flx("user.applications-tab.button.open-create-modal--2")
                .child(
                    Button::new("settings-applications-create")
                        .primary()
                        .small()
                        .label(l10n::tr("Create application"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            // TODO(backlog, T3): Connect POST /oauth2/applications creation modal.
                            this.settings_set_applications_notice(
                                l10n::tr("Creating applications is deferred to the T3 workflow."),
                                cx,
                            );
                        })),
                ),
        )
        .child(
            div()
                .debug_selector(|| "settings-applications-docs".to_owned())
                .flx("user.applications-tab.documentation-link")
                .child(
                    Button::new("settings-applications-docs")
                        .ghost()
                        .small()
                        .icon(AppIcon::BookOpenFill)
                        .label(l10n::tr("Read the documentation (docs.fluxer.app)"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            #[cfg(test)]
                            this.launch_external_url(DOCS_URL, cx);
                            #[cfg(not(test))]
                            {
                                let _ = this;
                                cx.open_url(DOCS_URL);
                            }
                        })),
                ),
        );

    let list_body: AnyElement = if modal.applications_loading && modal.applications.is_empty() {
        div()
            .debug_selector(|| "settings-applications-loading".to_owned())
            .p_4()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(l10n::tr("Loading applications…"))
            .into_any_element()
    } else if let Some(err) = &modal.applications_error {
        div()
            .debug_selector(|| "settings-applications-error".to_owned())
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
                Button::new("settings-applications-retry")
                    .ghost()
                    .small()
                    .label(l10n::tr("Try again"))
                    .on_click(cx.listener(|this, _, _, cx| this.settings_reload_applications(cx))),
            )
            .into_any_element()
    } else if modal.applications.is_empty() {
        div()
            .debug_selector(|| "settings-applications-empty".to_owned())
            .p_6()
            .rounded(px(8.))
            .border_1()
            .border_color(settings_border(theme))
            .bg(settings_card_bg(theme))
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .flx("user.applications-tab.applications-list.empty-state")
            .child(Icon::new(AppIcon::AppWindowFill).with_size(px(28.)).text_color(theme.muted_foreground))
            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(l10n::tr("No applications yet")))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(l10n::tr("Create your first application to get started with the Fluxer API.")),
            )
            .into_any_element()
    } else {
        div()
            .debug_selector(|| "settings-applications-list".to_owned())
            .flex()
            .flex_col()
            .gap_2()
            .flx("user.applications-tab.applications-list.list-container")
            .children(modal.applications.iter().map(|app| {
                let app_id = app.id.clone();
                let click_id = app.id.clone();
                let sel_id = format!("settings-application-item-{}", app.id);
                let initial = app
                    .name
                    .chars()
                    .next()
                    .map(|c| c.to_uppercase().to_string())
                    .unwrap_or_else(|| "A".to_owned());
                div()
                    .id(SharedString::from(sel_id.clone()))
                    .debug_selector(move || sel_id.clone())
                    .p_3()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(settings_border(theme))
                    .bg(settings_card_bg(theme))
                    .cursor_pointer()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .flx("user.applications-tab.applications-list.item-button.select-application")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.settings_select_application(click_id.clone(), cx);
                    }))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_3()
                            .min_w_0()
                            .child(
                                div()
                                    .size(px(36.))
                                    .rounded(px(8.))
                                    .bg(theme.muted)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(initial),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_0p5()
                                    .min_w_0()
                                    .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(app.name.clone()))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme.muted_foreground)
                                            .child(format!("ID: {app_id}")),
                                    ),
                            ),
                    )
                    .child(Icon::new(AppIcon::CaretRight).with_size(px(16.)).text_color(theme.muted_foreground))
            }))
            .into_any_element()
    };

    div()
        .debug_selector(|| "settings-applications-tab".to_owned())
        .flex()
        .flex_col()
        .gap_4()
        .child(settings_section_block(
            l10n::tr("Applications"),
            None,
            div()
                .flex()
                .flex_col()
                .gap_3()
                .child(action_bar)
                .when_some(modal.applications_notice.clone(), |d, notice| {
                    d.child(
                        div()
                            .debug_selector(|| "settings-applications-notice".to_owned())
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
