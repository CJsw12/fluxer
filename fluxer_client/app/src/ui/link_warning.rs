//! External untrusted link confirmation modal (`LinkWarningModal`):
//! displays the parsed hostname prominently, prevents spoofing via userinfo URLs,
//! and optionally trusts the domain permanently (`prefs.json` + `UserSettings`).

use fluxer_client_core::rest::UserSettingsPatch;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Escape;
use gpui_kit::component::switch::Switch;
use gpui_kit::{
    AnyElement, ClipboardItem, Context, FocusHandle, FontWeight, Window, div, prelude::*, px,
};

use crate::client_view::ClientView;
use crate::theme::Theme;
use crate::trusted_domains;
use crate::ui::overlay::Overlay;
use crate::ui::{l10n, overlay};

pub const CONTEXT: &str = "LinkWarningModal";

pub struct LinkWarningModal {
    pub url: String,
    pub host: String,
    pub always_trust: bool,
    pub focus: FocusHandle,
}

impl ClientView {
    /// Authoritative trusted domains: server-synced `user_settings.trusted_domains`
    /// once present, else device-local `prefs.data.trusted_domains`.
    pub(crate) fn effective_trusted_domains(&self) -> Vec<String> {
        let state = self.backend.state();
        if state.user_settings.raw.get("trusted_domains").is_some() {
            return state.user_settings.trusted_domains();
        }
        self.prefs.data.trusted_domains.clone()
    }

    /// Launches a validated external URL (captured in tests without spawning
    /// the OS browser).
    pub(crate) fn launch_external_url(&mut self, url: &str, cx: &mut Context<Self>) {
        #[cfg(test)]
        {
            let _ = cx;
            self.opened_urls.push(url.to_owned());
        }
        #[cfg(not(test))]
        cx.open_url(url);
    }

    /// Opens an external link if trusted (or safe `mailto:`/`tel:` URI), or
    /// prompts the user when an HTTP/HTTPS destination host is untrusted.
    /// `fluxer://` protocol links and internal HTTP(S) channel/user links are
    /// routed in-app without invoking the external browser.
    pub(crate) fn open_external_url(&mut self, raw_url: &str, window: &mut Window, cx: &mut Context<Self>) {
        if crate::deep_links::is_app_protocol_url(raw_url) {
            if let Some(link) = crate::deep_links::parse_protocol_deep_link(raw_url) {
                self.open_deep_link(link, window, cx);
            }
            return;
        }
        if let Some(link) = crate::deep_links::parse_internal_http_deep_link(raw_url, self.backend.instance_hosts()) {
            self.open_deep_link(link, window, cx);
            return;
        }
        if trusted_domains::is_safe_non_http_uri(raw_url) {
            self.launch_external_url(raw_url.trim(), cx);
            return;
        }
        let Some((url, host)) = trusted_domains::parse_external_http_url(raw_url) else {
            return;
        };
        let user_trusted = self.effective_trusted_domains();
        if trusted_domains::is_trusted(&host, self.backend.instance_hosts(), &user_trusted) {
            self.launch_external_url(&url, cx);
            return;
        }
        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        let focus = cx.focus_handle();
        self.overlay = Overlay::LinkWarning(LinkWarningModal {
            url,
            host,
            always_trust: false,
            focus: focus.clone(),
        });
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(crate) fn link_warning_set_trust(&mut self, always_trust: bool, cx: &mut Context<Self>) {
        if let Overlay::LinkWarning(modal) = &mut self.overlay {
            modal.always_trust = always_trust;
            cx.notify();
        }
    }

    pub(crate) fn confirm_link_warning(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (url, host, always_trust) = {
            let Overlay::LinkWarning(modal) = &self.overlay else { return };
            (modal.url.clone(), modal.host.clone(), modal.always_trust)
        };
        if always_trust {
            let mut merged = self.effective_trusted_domains();
            self.prefs.add_trusted_domain(&host);
            if trusted_domains::add_trusted_domain(&mut merged, &host) {
                self.backend.update_user_settings(UserSettingsPatch {
                    trusted_domains: Some(merged),
                    ..Default::default()
                });
            }
        }
        self.close_overlay(window, cx);
        self.launch_external_url(&url, cx);
    }
}

pub fn render(modal: &LinkWarningModal, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let copy_url = modal.url.clone();
    let content = div()
        .id("link-warning-modal")
        .key_context(CONTEXT)
        .track_focus(&modal.focus)
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .capture_action(cx.listener(|this, _: &Escape, window, cx| {
            cx.stop_propagation();
            this.close_overlay(window, cx);
        }))
        .child(div().text_lg().font_weight(FontWeight::BOLD).child(l10n::tr("External link")))
        .child(
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(l10n::tr("You are about to open an external website:")),
        )
        .child(
            div()
                .debug_selector(|| "link-warning-host".to_owned())
                .px_3()
                .py_2()
                .rounded(px(6.))
                .border_1()
                .border_color(theme.border)
                .bg(theme.background)
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_sm().font_weight(FontWeight::BOLD).child(modal.host.clone()))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(modal.url.clone()),
                ),
        )
        .child(
            div()
                .debug_selector(|| "link-warning-trust-toggle".to_owned())
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_2()
                .child(div().text_xs().child(format!("Always trust {}", modal.host)))
                .child(
                    Switch::new("link-warning-trust-switch")
                        .checked(modal.always_trust)
                        .on_change(cx.listener(|this, checked: &bool, _, cx| {
                            this.link_warning_set_trust(*checked, cx);
                        })),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .gap_2()
                .child(
                    div().debug_selector(|| "link-warning-cancel".to_owned()).child(
                        Button::new("link-warning-cancel")
                            .ghost()
                            .small()
                            .label(l10n::tr("Cancel"))
                            .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
                    ),
                )
                .child(
                    div().debug_selector(|| "link-warning-copy".to_owned()).child(
                        Button::new("link-warning-copy")
                            .ghost()
                            .small()
                            .label(l10n::tr("Copy link"))
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(copy_url.clone()));
                            }),
                    ),
                )
                .child(
                    div().debug_selector(|| "link-warning-continue".to_owned()).child(
                        Button::new("link-warning-continue")
                            .primary()
                            .small()
                            .label(l10n::tr("Continue"))
                            .on_click(cx.listener(|this, _, window, cx| this.confirm_link_warning(window, cx))),
                    ),
                ),
        );

    let view = cx.weak_entity();
    overlay::modal(content, theme, move |window, cx| {
        let _ = view.update(cx, |v, cx| v.close_overlay(window, cx));
    })
}
