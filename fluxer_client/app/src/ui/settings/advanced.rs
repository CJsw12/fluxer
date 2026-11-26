use super::*;

pub(super) fn render_advanced_tab(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let updates_section = settings_section_block(
        l10n::tr("About & Updates"),
        None,
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
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
                    .gap_2()
                    .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(format!("v{CURRENT_VERSION}")))
                    .child(
                        div().debug_selector(|| "settings-check-updates".to_owned()).child(
                            Button::new("settings-check-updates")
                                .ghost()
                                .small()
                                .label(l10n::tr("Check for updates"))
                                .accessibility_label(l10n::tr("Check for updates"))
                                .disabled(matches!(
                                    view.update_status,
                                    UpdateStatus::Checking | UpdateStatus::Downloading { .. }
                                ))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.check_for_updates(cx);
                                })),
                        ),
                    ),
            )
            .map(|d| match &view.update_status {
                UpdateStatus::Idle => d,
                UpdateStatus::Checking => d.child(
                    div()
                        .debug_selector(|| "settings-update-checking".to_owned())
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(l10n::tr("Checking for updates…")),
                ),
                UpdateStatus::UpToDate { .. } => d.child(
                    div()
                        .debug_selector(|| "settings-update-uptodate".to_owned())
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(l10n::tr("Up to date")),
                ),
                UpdateStatus::Available {
                    latest,
                    release_url,
                    asset_name,
                    asset_url,
                    ..
                } => {
                    let release_url = release_url.clone();
                    let has_asset = asset_name.is_some() && asset_url.is_some();
                    d.child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .debug_selector(|| "settings-update-available".to_owned())
                                    .text_xs()
                                    .text_color(theme.foreground)
                                    .child(format!("{}: v{latest}", l10n::tr("Update available"))),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_2()
                                    .when(has_asset, |row| {
                                        row.child(
                                            div()
                                                .debug_selector(|| "settings-download-update".to_owned())
                                                .child(
                                                    Button::new("settings-download-update")
                                                        .primary()
                                                        .small()
                                                        .label(l10n::tr("Download update"))
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.download_available_update(cx);
                                                        })),
                                                ),
                                        )
                                    })
                                    .child(
                                        div().debug_selector(|| "settings-open-release".to_owned()).child(
                                            Button::new("settings-open-release")
                                                .ghost()
                                                .small()
                                                .label(l10n::tr("Open release page"))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    #[cfg(test)]
                                                    this.launch_external_url(&release_url, cx);
                                                    #[cfg(not(test))]
                                                    {
                                                        let _ = this;
                                                        cx.open_url(&release_url);
                                                    }
                                                })),
                                        ),
                                    ),
                            ),
                    )
                }
                UpdateStatus::Downloading { latest } => d.child(
                    div()
                        .debug_selector(|| "settings-update-downloading".to_owned())
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(format!("{} v{latest}…", l10n::tr("Downloading"))),
                ),
                UpdateStatus::Downloaded {
                    latest,
                    path,
                    sha256,
                    verified,
                } => {
                    let short_hash = &sha256[..sha256.len().min(16)];
                    let verify_label = if *verified {
                        l10n::tr("SHA-256 verified")
                    } else {
                        l10n::tr("Unverified")
                    };
                    d.child(
                        div()
                            .debug_selector(|| "settings-update-downloaded".to_owned())
                            .flex()
                            .flex_col()
                            .gap_1()
                            .text_xs()
                            .text_color(theme.foreground)
                            .child(format!(
                                "{} v{latest} ({verify_label} · {short_hash})",
                                l10n::tr("Downloaded")
                            ))
                            .child(div().text_color(theme.muted_foreground).child(path.clone())),
                    )
                }
                UpdateStatus::Error(err) => d.child(
                    div()
                        .debug_selector(|| "settings-update-error".to_owned())
                        .text_xs()
                        .text_color(theme.danger)
                        .child(err.clone()),
                ),
            })
            .into_any_element(),
        theme,
    );

    div()
        .flex()
        .flex_col()
        .gap_5()
        .child(updates_section)
        .into_any_element()
}
