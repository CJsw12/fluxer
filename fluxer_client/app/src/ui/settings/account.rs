use super::*;

pub(super) fn render_account_tab(
    view: &ClientView,
    modal: &SettingsModal,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let trusted_count = view.effective_trusted_domains().len();
    let switch_label = if view.saved_accounts.len() > 1 {
        format!("{} ({})", l10n::tr("Switch accounts"), view.saved_accounts.len())
    } else {
        l10n::tr("Switch accounts")
    };
    let (user_name, user_id_str) = match &view.user {
        Some(u) => (u.name.to_string(), u.id.to_string()),
        None => (l10n::tr("Signed-in user"), String::from("—")),
    };
    let webauthn_supported = crate::webauthn::is_supported();
    let platform_auth = crate::webauthn::is_platform_authenticator_available();
    let support_note = if platform_auth {
        l10n::tr("Windows Hello & FIDO2 security keys ready")
    } else if webauthn_supported {
        l10n::tr("Hardware security keys (USB/NFC) ready")
    } else {
        l10n::tr("Passkeys are not supported on this platform")
    };

    let mut passkey_list = div().flex().flex_col().gap_1();
    if modal.passkey_loading {
        passkey_list = passkey_list.child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(l10n::tr("Loading passkeys…")),
        );
    } else if modal.passkeys.is_empty() {
        passkey_list = passkey_list.child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(l10n::tr("No passkeys or security keys registered yet.")),
        );
    } else {
        for (ix, cred) in modal.passkeys.iter().enumerate() {
            let cid = cred.id.clone();
            let display_name = if cred.name.is_empty() {
                cred.id.clone()
            } else {
                cred.name.clone()
            };
            passkey_list = passkey_list.child(
                div()
                    .px_3()
                    .py_2()
                    .rounded(px(6.))
                    .bg(theme.background)
                    .border_1()
                    .border_color(theme.border)
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .child(div().truncate().text_sm().font_weight(FontWeight::MEDIUM).child(display_name))
                            .when_some(cred.created_at.clone(), |d, created| {
                                d.child(div().text_xs().text_color(theme.muted_foreground).child(created))
                            }),
                    )
                    .child(
                        Button::new(("settings-passkey-remove", ix))
                            .danger()
                            .xsmall()
                            .disabled(modal.passkey_busy)
                            .label(l10n::tr("Remove"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.settings_delete_passkey(cid.clone(), window, cx);
                            })),
                    ),
            );
        }
    }

    div()
        .flex()
        .flex_col()
        .gap_5()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Sign-in details"), theme))
                .child(info_card_row(
                    user_name,
                    format!("{}: {user_id_str}", l10n::tr("Account ID")),
                    Some(l10n::tr("Verified")),
                    theme,
                )),
        )
        .child(
            div()
                .debug_selector(|| "settings-account-passkeys".to_owned())
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Passkeys & security keys"), theme))
                .child(
                    div()
                        .p_4()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(settings_border(theme))
                        .bg(settings_card_bg(theme))
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap_2()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(support_note),
                                )
                                .child(
                                    Button::new("settings-passkey-refresh")
                                        .ghost()
                                        .xsmall()
                                        .disabled(modal.passkey_loading || modal.passkey_busy)
                                        .label(l10n::tr("Refresh"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if let Overlay::Settings(m) = &mut this.overlay {
                                                m.passkey_loading = true;
                                                m.passkey_error = None;
                                            }
                                            this.backend.load_webauthn_credentials();
                                            cx.notify();
                                        })),
                                ),
                        )
                        .child(passkey_list)
                        .child(Input::new(&modal.passkey_password).mask_toggle().disabled(modal.passkey_busy))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .child(Input::new(&modal.passkey_name).disabled(modal.passkey_busy)),
                                )
                                .child(
                                    div().debug_selector(|| "settings-passkey-add-btn".to_owned()).child(
                                        Button::new("settings-passkey-add-btn")
                                            .primary()
                                            .small()
                                            .loading(modal.passkey_busy)
                                            .disabled(modal.passkey_busy || !webauthn_supported)
                                            .label(l10n::tr("Add passkey"))
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.settings_register_passkey(window, cx);
                                            })),
                                    ),
                                ),
                        )
                        .when_some(modal.passkey_status.clone(), |d, msg| {
                            d.child(div().text_xs().text_color(theme.status_online).child(msg))
                        })
                        .when_some(modal.passkey_error.clone(), |d, err| {
                            d.child(div().text_xs().text_color(theme.danger).child(err))
                        }),
                ),
        )
        .when(view.accounts_enabled, |d| {
            d.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(section_heading(l10n::tr("Accounts"), theme))
                    .child(
                        div()
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
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(l10n::tr("Switch accounts")),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme.muted_foreground)
                                            .child(l10n::tr("Add or switch between saved accounts.")),
                                    ),
                            )
                            .child(
                                div().debug_selector(|| "settings-account-switch-accounts".to_owned()).child(
                                    Button::new("settings-account-switch-accounts")
                                        .ghost()
                                        .small()
                                        .label(switch_label)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.apply_settings_effect(SettingsEffect::OpenAccountSwitcher, window, cx);
                                        })),
                                ),
                            ),
                    ),
            )
        })
        .child(
            div()
                .debug_selector(|| "settings-account-trusted".to_owned())
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Trusted domains"), theme))
                .child(
                    div()
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
                        .child(
                            div()
                                .text_sm()
                                .child(format!("{} ({trusted_count})", l10n::tr("Trusted domains"))),
                        )
                        .when(trusted_count > 0, |d| {
                            d.child(
                                div().debug_selector(|| "settings-clear-trusted".to_owned()).child(
                                    Button::new("settings-clear-trusted")
                                        .ghost()
                                        .small()
                                        .label(l10n::tr("Clear"))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.apply_settings_effect(SettingsEffect::ClearTrustedDomains, window, cx);
                                        })),
                                ),
                            )
                        }),
                ),
        )
        .into_any_element()
}
