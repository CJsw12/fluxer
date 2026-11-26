use super::*;
use crate::ui::friends::{FriendsTab, build_friend_rows};

#[derive(Clone, Copy)]
enum AccountButtonVariant {
    Primary,
    Secondary,
    Danger,
}

fn account_button(
    id: &'static str,
    label: String,
    variant: AccountButtonVariant,
    flx: &'static str,
    theme: &Theme,
    on_click: impl Fn(&mut ClientView, &gpui_kit::ClickEvent, &mut Window, &mut Context<ClientView>) + 'static,
    cx: &mut Context<ClientView>,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let (bg, border, fg, hover_bg) = match variant {
        AccountButtonVariant::Primary => (
            theme.brand,
            theme.brand_button_border,
            theme.on_fill,
            theme.brand.opacity(0.9),
        ),
        AccountButtonVariant::Secondary => (
            theme.background_textarea,
            theme.background_modifier_accent,
            theme.foreground,
            theme.hover_bg,
        ),
        AccountButtonVariant::Danger => (
            theme.danger,
            theme.danger.opacity(0.7),
            theme.on_fill,
            theme.danger.opacity(0.85),
        ),
    };
    div()
        .id(id)
        .h(px(34.))
        .min_w(px(56.))
        .px(px(10.))
        .py(px(6.))
        .rounded(px(8.))
        .border_1()
        .border_color(border)
        .bg(bg)
        .hover(move |s| s.bg(hover_bg))
        .cursor_pointer()
        .flex()
        .flex_row()
        .items_center()
        .justify_center()
        .text_size(px(13.))
        .line_height(px(18.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(fg)
        .on_click(cx.listener(on_click))
        .flx(flx)
        .child(label)
}

fn account_section(title: String, is_first: bool, content: AnyElement, theme: &Theme) -> AnyElement {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(16.))
        .pb(px(24.))
        .when(!is_first, |d| d.pt(px(24.)).border_t_1().border_color(settings_border(theme)))
        .flx("app.settings-section.section")
        .child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(4.))
                .flx("app.settings-section.section-header")
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
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

fn account_card(header: Option<AnyElement>, rows: Vec<AnyElement>, theme: &Theme) -> gpui_kit::Div {
    div()
        .w_full()
        .flex()
        .flex_col()
        .rounded(px(12.))
        .border_1()
        .border_color(theme.background_modifier_accent)
        .flx("user.account-security-card.card")
        .when_some(header, |d, h| d.child(h))
        .child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .flx("user.account-security-card.rows")
                .children(rows),
        )
}

fn account_card_header(
    title: String,
    description: String,
    action: Option<AnyElement>,
    theme: &Theme,
) -> AnyElement {
    div()
        .w_full()
        .px_4()
        .pt_4()
        .pb(px(17.))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_4()
        .flx("user.account-security-card.header")
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_1()
                .flx("user.account-security-card.header-text")
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .flx("user.account-security-card.title-row")
                        .child(
                            div()
                                .text_size(px(15.))
                                .line_height(px(19.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.foreground)
                                .flx("user.account-security-card.title")
                                .child(title),
                        ),
                )
                .child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(18.2))
                        .text_color(theme.text_tertiary)
                        .flx("user.account-security-card.description")
                        .child(description),
                ),
        )
        .when_some(action, |d, act| {
            d.child(
                div()
                    .flex_none()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .flx("user.account-security-card.header-action")
                    .child(act),
            )
        })
        .into_any_element()
}

fn account_card_row(
    label: String,
    description: AnyElement,
    actions: Option<AnyElement>,
    has_top_border: bool,
    theme: &Theme,
) -> AnyElement {
    div()
        .w_full()
        .px_4()
        .py_3()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_4()
        .when(has_top_border, |d| d.border_t_1().border_color(theme.background_modifier_accent))
        .flx("user.account-security-card.row")
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .flx("user.account-security-card.row-text")
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(21.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.foreground)
                        .flx("user.account-security-card.row-label")
                        .child(label),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_start()
                        .text_size(px(13.))
                        .line_height(px(18.2))
                        .text_color(theme.muted_foreground)
                        .flx("user.account-security-card.row-description")
                        .child(description),
                ),
        )
        .when_some(actions, |d, act| {
            d.child(
                div()
                    .flex_none()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .flx("user.account-security-card.row-actions")
                    .child(act),
            )
        })
        .into_any_element()
}

fn mask_email(email: &str) -> String {
    match email.split_once('@') {
        Some((user, domain)) => {
            let masked: String = user.chars().map(|_| '*').collect();
            format!("{masked}@{domain}")
        }
        None => email.chars().map(|_| '*').collect(),
    }
}

pub(super) fn render_account_tab(
    view: &ClientView,
    modal: &SettingsModal,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let raw_email = match &view.user {
        Some(u) if !u.name.is_empty() => format!("{}@fluxer.local", u.name),
        _ => String::from("user@fluxer.local"),
    };
    let displayed_email = if modal.show_masked_email {
        raw_email
    } else {
        mask_email(&raw_email)
    };
    let toggle_label = if modal.show_masked_email {
        l10n::tr("Hide")
    } else {
        l10n::tr("Reveal")
    };

    // 1. Sign-in details
    let email_desc = div()
        .self_start()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.))
        .flx("user.account-security-tab.account-tab.account-tab-content.email-value")
        .child(
            div()
                .text_size(px(13.))
                .line_height(px(18.2))
                .text_color(theme.muted_foreground)
                .flx("user.account-security-tab.account-tab.account-tab-content.email-text")
                .child(displayed_email),
        )
        .child(
            div()
                .id("settings-account-email-toggle")
                .cursor_pointer()
                .text_size(px(13.))
                .line_height(px(18.2))
                .text_color(theme.text_link)
                .hover(|s| s.underline())
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Overlay::Settings(m) = &mut this.overlay {
                        m.show_masked_email = !m.show_masked_email;
                        cx.notify();
                    }
                }))
                .flx("user.account-security-tab.account-tab.account-tab-content.toggle-button")
                .child(toggle_label),
        )
        .into_any_element();

    let change_password_btn = account_button(
        "settings-account-change-password",
        l10n::tr("Change password"),
        AccountButtonVariant::Secondary,
        "user.account-security-tab.account-tab.account-tab-content.button.change-password",
        theme,
        |this, _, _, cx| {
            // TODO(backlog, T3): Connect verified password change modal flow.
            this.settings_set_account_notice(
                l10n::tr("Password changes are managed in the web client for now."),
                cx,
            );
        },
        cx,
    )
    .into_any_element();

    let sign_in_card = account_card(
        None,
        vec![
            account_card_row(l10n::tr("Email address"), email_desc, None, false, theme),
            account_card_row(
                l10n::tr("Password"),
                div().child(l10n::tr("Last changed: never")).into_any_element(),
                Some(change_password_btn),
                true,
                theme,
            ),
        ],
        theme,
    )
    .into_any_element();

    // 2. Security
    let two_factor_btn = account_button(
        "settings-account-2fa-setup",
        l10n::tr("Set up"),
        AccountButtonVariant::Primary,
        "user.account-security-tab.security-tab.security-tab-content.button.push--2",
        theme,
        |this, _, _, cx| {
            // TODO(backlog): Connect TOTP authenticator app setup modal.
            this.settings_set_account_notice(
                l10n::tr("Two-factor authenticator setup is managed in the web client for now."),
                cx,
            );
        },
        cx,
    )
    .into_any_element();

    let two_factor_card = account_card(
        Some(account_card_header(
            l10n::tr("Two-factor authentication"),
            l10n::tr("Ask for a second step after your password when you sign in."),
            None,
            theme,
        )),
        vec![account_card_row(
            l10n::tr("Authenticator app"),
            div()
                .child(l10n::tr(
                    "Use an authenticator app to generate codes for two-factor authentication",
                ))
                .into_any_element(),
            Some(two_factor_btn),
            true,
            theme,
        )],
        theme,
    )
    .into_any_element();

    let add_passkey_btn = account_button(
        "settings-passkey-add-btn",
        l10n::tr("Add passkey"),
        AccountButtonVariant::Primary,
        "user.account-security-tab.security-tab.security-tab-content.button.add-passkey",
        theme,
        |this, _, _, cx| {
            if let Overlay::Settings(m) = &mut this.overlay {
                m.show_passkey_form = !m.show_passkey_form;
                cx.notify();
            }
        },
        cx,
    )
    .debug_selector(|| "settings-passkey-add-btn".to_owned())
    .into_any_element();

    let mut passkey_rows: Vec<AnyElement> = Vec::new();
    if modal.passkeys.is_empty() && !modal.show_passkey_form && !modal.passkey_loading {
        passkey_rows.push(
            div()
                .w_full()
                .px_4()
                .py_3()
                .border_t_1()
                .border_color(theme.background_modifier_accent)
                .text_size(px(13.))
                .line_height(px(19.5))
                .text_color(theme.muted_foreground)
                .flx("user.account-security-card.empty-row")
                .child(l10n::tr(
                    "No passkeys yet. Add one to sign in with your fingerprint, face or a security key.",
                ))
                .into_any_element(),
        );
    } else {
        if modal.passkey_loading {
            passkey_rows.push(
                div()
                    .w_full()
                    .px_4()
                    .py_3()
                    .border_t_1()
                    .border_color(theme.background_modifier_accent)
                    .text_size(px(13.))
                    .text_color(theme.muted_foreground)
                    .child(l10n::tr("Loading passkeys…"))
                    .into_any_element(),
            );
        }
        for (ix, cred) in modal.passkeys.iter().enumerate() {
            let cid = cred.id.clone();
            let display_name = if cred.name.is_empty() {
                cred.id.clone()
            } else {
                cred.name.clone()
            };
            let desc = cred.created_at.clone().unwrap_or_default();
            let remove_btn = Button::new(("settings-passkey-remove", ix))
                .danger()
                .xsmall()
                .disabled(modal.passkey_busy)
                .label(l10n::tr("Remove"))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.settings_delete_passkey(cid.clone(), window, cx);
                }))
                .into_any_element();
            passkey_rows.push(account_card_row(
                display_name,
                div().child(desc).into_any_element(),
                Some(remove_btn),
                true,
                theme,
            ));
        }
        if modal.show_passkey_form {
            let webauthn_supported = crate::webauthn::is_supported();
            passkey_rows.push(
                div()
                    .w_full()
                    .p_4()
                    .border_t_1()
                    .border_color(theme.background_modifier_accent)
                    .flex()
                    .flex_col()
                    .gap_3()
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
                                Button::new("settings-passkey-submit-btn")
                                    .primary()
                                    .small()
                                    .loading(modal.passkey_busy)
                                    .disabled(modal.passkey_busy || !webauthn_supported)
                                    .label(l10n::tr("Register passkey"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.settings_register_passkey(window, cx);
                                    })),
                            ),
                    )
                    .when_some(modal.passkey_status.clone(), |d, msg| {
                        d.child(div().text_xs().text_color(theme.status_online).child(msg))
                    })
                    .when_some(modal.passkey_error.clone(), |d, err| {
                        d.child(div().text_xs().text_color(theme.danger).child(err))
                    })
                    .into_any_element(),
            );
        }
    }

    let passkeys_card = account_card(
        Some(account_card_header(
            l10n::tr("Passkeys"),
            l10n::tr("Use passkeys to sign in without a password"),
            Some(add_passkey_btn),
            theme,
        )),
        passkey_rows,
        theme,
    )
    .debug_selector(|| "settings-account-passkeys".to_owned())
    .into_any_element();

    let manage_apps_btn = account_button(
        "settings-account-manage-apps",
        l10n::tr("Manage"),
        AccountButtonVariant::Secondary,
        "user.account-security-tab.security-tab.security-tab-content.account-access.button.manage-authorized-apps",
        theme,
        |this, _, _, cx| {
            this.select_settings_tab(SettingsTab::Applications, cx);
        },
        cx,
    )
    .into_any_element();

    let manage_devices_btn = account_button(
        "settings-account-manage-devices",
        l10n::tr("Manage"),
        AccountButtonVariant::Secondary,
        "user.account-security-tab.security-tab.security-tab-content.account-access.button.manage-linked-devices",
        theme,
        |this, _, _, cx| {
            // TODO(backlog): Connect linked devices session management modal.
            this.settings_set_account_notice(
                l10n::tr("Linked device management is available in the web client for now."),
                cx,
            );
        },
        cx,
    )
    .into_any_element();

    let apps_devices_card = account_card(
        Some(account_card_header(
            l10n::tr("Apps and devices"),
            l10n::tr("Manage apps and devices with access to your account."),
            None,
            theme,
        )),
        vec![
            account_card_row(
                l10n::tr("Authorized apps"),
                div().child(l10n::tr("Review apps that can access your account.")).into_any_element(),
                Some(manage_apps_btn),
                true,
                theme,
            ),
            account_card_row(
                l10n::tr("Linked devices"),
                div().child(l10n::tr("Review signed-in devices.")).into_any_element(),
                Some(manage_devices_btn),
                true,
                theme,
            ),
        ],
        theme,
    )
    .into_any_element();

    let security_content = div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(20.))
        .child(two_factor_card)
        .child(passkeys_card)
        .child(apps_devices_card)
        .into_any_element();

    // 3. Danger zone
    let disable_account_btn = account_button(
        "settings-account-disable",
        l10n::tr("Disable account"),
        AccountButtonVariant::Secondary,
        "user.account-security-tab.danger-zone-tab.button.disable-account",
        theme,
        |this, _, _, cx| {
            // TODO(backlog, T3): Connect verified account disable flow.
            this.settings_set_account_notice(
                l10n::tr("Account disabling requires verified confirmation in the web client."),
                cx,
            );
        },
        cx,
    )
    .into_any_element();

    let delete_account_btn = account_button(
        "settings-account-delete",
        l10n::tr("Delete account"),
        AccountButtonVariant::Danger,
        "user.account-security-tab.danger-zone-tab.button.delete-account",
        theme,
        |this, _, _, cx| {
            // TODO(backlog, T3): Connect verified account deletion flow.
            this.settings_set_account_notice(
                l10n::tr("Account deletion requires verified confirmation in the web client."),
                cx,
            );
        },
        cx,
    )
    .into_any_element();

    let danger_card = account_card(
        None,
        vec![
            account_card_row(
                l10n::tr("Disable account"),
                div()
                    .child(l10n::tr(
                        "Temporarily disable your account. You can reactivate it later by signing back in.",
                    ))
                    .into_any_element(),
                Some(disable_account_btn),
                false,
                theme,
            ),
            account_card_row(
                l10n::tr("Delete account"),
                div()
                    .child(l10n::tr(
                        "Permanently delete your account and all associated data. This action cannot be undone.",
                    ))
                    .into_any_element(),
                Some(delete_account_btn),
                true,
                theme,
            ),
        ],
        theme,
    )
    .into_any_element();

    // 4. Blocked users
    let blocked_rows = {
        let state = view.backend.state();
        build_friend_rows(&state, view.backend.media(), FriendsTab::Blocked)
    };
    let blocked_content = if blocked_rows.is_empty() {
        div()
            .w_full()
            .py(px(8.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(12.))
            .flx("app.status-slate.container")
            .child(
                div()
                    .size(px(44.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(theme.muted_foreground)
                    .flx("app.status-slate.icon")
                    .child(Icon::new(AppIcon::Prohibit).size(px(44.))),
            )
            .child(
                div()
                    .text_size(px(18.))
                    .line_height(px(24.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.foreground)
                    .flx("app.status-slate.title")
                    .child(l10n::tr("No blocked users")),
            )
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(24.))
                    .text_color(theme.muted_foreground)
                    .flx("app.status-slate.description")
                    .child(l10n::tr("You haven't blocked anyone yet.")),
            )
            .into_any_element()
    } else {
        let rows = blocked_rows
            .into_iter()
            .enumerate()
            .map(|(ix, row)| {
                account_card_row(
                    row.display_name.to_string(),
                    div().child(row.username_tag).into_any_element(),
                    None,
                    ix > 0,
                    theme,
                )
            })
            .collect();
        account_card(None, rows, theme).into_any_element()
    };

    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(20.))
        .when_some(modal.account_notice.clone(), |d, notice| {
            d.child(
                div()
                    .w_full()
                    .px_4()
                    .py_3()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(theme.background_modifier_accent)
                    .bg(settings_card_bg(theme))
                    .text_size(px(13.))
                    .text_color(theme.muted_foreground)
                    .child(notice),
            )
        })
        .child(account_section(l10n::tr("Sign-in details"), true, sign_in_card, theme))
        .child(account_section(l10n::tr("Security"), false, security_content, theme))
        .child(account_section(l10n::tr("Danger zone"), false, danger_card, theme))
        .child(account_section(l10n::tr("Blocked users"), false, blocked_content, theme))
        .into_any_element()
}
