//! Multi-account switcher modal (`Overlay::AccountSwitcher`).

use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Escape;
use gpui_kit::{AnyElement, Context, FocusHandle, FontWeight, Window, div, prelude::*, px};

use super::l10n;
use fluxer_client_core::state::Snowflake;
use super::avatar::{AvatarSpec, render_avatar};
use super::overlay;
use crate::client_view::{ClientEvent, ClientView};
use crate::theme::Theme;
use crate::ui::overlay::Overlay;

pub const CONTEXT: &str = "AccountSwitcherModal";

pub struct AccountSwitcherModal {
    pub focus: FocusHandle,
    pub confirm_remove: Option<String>,
}

impl ClientView {
    pub(crate) fn open_account_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.accounts_enabled {
            return;
        }
        if matches!(self.overlay, Overlay::AccountSwitcher(_)) {
            self.close_overlay(window, cx);
            return;
        }
        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        let focus = cx.focus_handle();
        self.overlay = Overlay::AccountSwitcher(AccountSwitcherModal {
            focus: focus.clone(),
            confirm_remove: None,
        });
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(crate) fn prompt_remove_saved_account(&mut self, key: Option<String>, cx: &mut Context<Self>) {
        if let Overlay::AccountSwitcher(modal) = &mut self.overlay {
            modal.confirm_remove = key;
            cx.notify();
        }
    }

    pub(crate) fn switch_account(&mut self, key: String, window: &mut Window, cx: &mut Context<Self>) {
        if !self.accounts_enabled {
            return;
        }
        if self.saved_accounts.iter().any(|a| a.is_active && a.key == key) {
            return;
        }
        self.close_overlay(window, cx);
        cx.emit(ClientEvent::SwitchAccount(key));
    }

    pub(crate) fn add_account(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.accounts_enabled {
            return;
        }
        self.close_overlay(window, cx);
        cx.emit(ClientEvent::AddAccount);
    }

    pub(crate) fn remove_saved_account(&mut self, key: String, cx: &mut Context<Self>) {
        if !self.accounts_enabled {
            return;
        }
        if self.saved_accounts.iter().any(|a| a.is_active && a.key == key) {
            return;
        }
        if let Overlay::AccountSwitcher(modal) = &mut self.overlay {
            modal.confirm_remove = None;
        }
        cx.emit(ClientEvent::RemoveAccount(key));
    }
}

pub fn render(
    view: &ClientView,
    modal: &AccountSwitcherModal,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let mut list = div()
        .id("account-switcher-list")
        .max_h(px(280.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_1();

    for (ix, item) in view.saved_accounts.iter().enumerate() {
        let subtitle = if item.username.is_empty() {
            item.instance_label.to_string()
        } else {
            format!("@{} · {}", item.username, item.instance_label)
        };
        let info = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .child(render_avatar(
                AvatarSpec {
                    user_id: item.user_id.parse::<u64>().map(Snowflake).unwrap_or(Snowflake(0)),
                    avatar_url: item.avatar_url.clone(),
                    display_name: item.display_name.clone(),
                    size: 32.,
                    status: None,
                    status_border_bg: None,
                },
                theme,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().truncate().text_sm().font_weight(FontWeight::MEDIUM).child(item.display_name.clone()))
                    .child(div().truncate().text_xs().text_color(theme.muted_foreground).child(subtitle)),
            );

        let key = item.key.clone();
        if item.is_active {
            let badge_sel = format!("acct-active-{key}");
            let row = div()
                .id(("acct-row", ix))
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .px_2()
                .py_1()
                .rounded(px(6.))
                .bg(theme.accent)
                .child(info)
                .child(
                    div()
                        .debug_selector(move || badge_sel.clone())
                        .px_2()
                        .py(px(2.))
                        .rounded(px(4.))
                        .bg(theme.brand)
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.on_fill)
                        .child(l10n::tr("Active")),
                );
            list = list.child(row);
        } else {
            let switch_key = key.clone();
            let switch_sel = format!("acct-switch-{key}");
            let confirming = modal.confirm_remove.as_deref() == Some(key.as_str());
            let trailing = if confirming {
                let confirm_key = key.clone();
                let confirm_sel = format!("acct-remove-confirm-{key}");
                let cancel_sel = format!("acct-remove-cancel-{key}");
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .child(
                        div().debug_selector(move || confirm_sel.clone()).child(
                            Button::new(("acct-remove-confirm", ix))
                                .danger()
                                .small()
                                .label(l10n::tr("Remove"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.remove_saved_account(confirm_key.clone(), cx);
                                })),
                        ),
                    )
                    .child(
                        div().debug_selector(move || cancel_sel.clone()).child(
                            Button::new(("acct-remove-cancel", ix))
                                .ghost()
                                .small()
                                .label(l10n::tr("Cancel"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.prompt_remove_saved_account(None, cx);
                                })),
                        ),
                    )
                    .into_any_element()
            } else {
                let remove_key = key.clone();
                let remove_sel = format!("acct-remove-{key}");
                div()
                    .debug_selector(move || remove_sel.clone())
                    .child(
                        Button::new(("acct-remove", ix))
                            .ghost()
                            .small()
                            .label(l10n::tr("Remove"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.prompt_remove_saved_account(Some(remove_key.clone()), cx);
                            })),
                    )
                    .into_any_element()
            };
            let row = div()
                .id(("acct-row", ix))
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .px_2()
                .py_1()
                .rounded(px(6.))
                .hover(|s| s.bg(theme.muted))
                .child(
                    div()
                        .id(("acct-switch", ix))
                        .debug_selector(move || switch_sel.clone())
                        .flex_1()
                        .min_w_0()
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.switch_account(switch_key.clone(), window, cx);
                        }))
                        .child(info),
                )
                .child(trailing);
            list = list.child(row);
        }
    }

    let content = div()
        .id("account-switcher-modal")
        .debug_selector(|| "account-switcher-modal".to_owned())
        .track_focus(&modal.focus)
        .key_context(CONTEXT)
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .capture_action(cx.listener(|this, _: &Escape, window, cx| {
            cx.stop_propagation();
            this.close_overlay(window, cx);
        }))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(div().text_lg().font_weight(FontWeight::BOLD).child(l10n::tr("Switch accounts")))
                .child(
                    Button::new("account-switcher-close")
                        .ghost()
                        .small()
                        .label("✕")
                        .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
                ),
        )
        .child(list)
        .child(
            div()
                .pt_2()
                .border_t_1()
                .border_color(theme.border)
                .flex()
                .flex_row()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("acct-cancel-btn")
                        .ghost()
                        .small()
                        .label(l10n::tr("Cancel"))
                        .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
                )
                .child(
                    div().debug_selector(|| "acct-add-btn".to_owned()).child(
                        Button::new("acct-add-btn")
                            .primary()
                            .small()
                            .label(l10n::tr("Add account"))
                            .on_click(cx.listener(|this, _, window, cx| this.add_account(window, cx))),
                    ),
                ),
        );

    let view = cx.weak_entity();
    overlay::modal(content, theme, move |window, cx| {
        let _ = view.update(cx, |v, cx| v.close_overlay(window, cx));
    })
}
