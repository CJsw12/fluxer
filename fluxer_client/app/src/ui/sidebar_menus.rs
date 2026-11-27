//! Context menus for the guild-rail Fluxer/Favorites buttons and the
//! user-area settings button (`FluxerButtonContextMenu.tsx`,
//! `FavoritesGuildContextMenu.tsx`, `SettingsContextMenu.tsx`).

use gpui_kit::{
    AnyElement, App, Context, MouseDownEvent, Pixels, Point, Role, Size, Window, div, prelude::*, px,
};
use fluxer_client_core::state::{ChannelKind, Snowflake};

use super::settings::SettingsTab;
use super::{l10n, overlay};
use crate::client_view::ClientView;
use crate::parity_probe::FlxExt as _;
use crate::theme::Theme;
use crate::ui::overlay::Overlay;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SidebarMenuKind {
    FluxerButton,
    Favorites,
    Settings,
    Header(Snowflake),
}

#[derive(Clone, Copy)]
pub(crate) enum ReadScope {
    All,
    Dms,
    Communities,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HeaderAction {
    Invite,
    AddPeople,
    CopyLink,
    Notifications,
    Edit,
}

pub(crate) fn header_actions(kind: ChannelKind, invite: bool, manage: bool) -> Vec<HeaderAction> {
    use HeaderAction::*;
    let mut actions = Vec::new();
    if kind == ChannelKind::GroupDm {
        actions.push(AddPeople);
    } else if !kind.is_private() && !kind.is_thread() && invite {
        actions.push(Invite);
    }
    actions.extend([CopyLink, Notifications]);
    if !kind.is_private() && !kind.is_thread() && manage {
        actions.push(Edit);
    }
    actions
}

pub(crate) struct SidebarContextMenu {
    pub(crate) kind: SidebarMenuKind,
    pub(crate) anchor: Point<Pixels>,
}

pub(crate) fn open_context_menu(
    kind: SidebarMenuKind,
    view: &mut ClientView,
    event: &MouseDownEvent,
    cx: &mut Context<ClientView>,
) {
    view.user_area_popout_open = false;
    view.user_area_submenu = None;
    view.guild_header_popout_open = false;
    view.voice_popout = None;
    view.voice_context_menu = None;
    view.sidebar_context_menu = Some(SidebarContextMenu { kind, anchor: event.position });
    cx.notify();
}

const SETTINGS_MENU_TABS: [SettingsTab; 15] = [
    SettingsTab::Profile,
    SettingsTab::Account,
    SettingsTab::PrivacySafety,
    SettingsTab::Connections,
    SettingsTab::Appearance,
    SettingsTab::Themes,
    SettingsTab::Notifications,
    SettingsTab::MessagesMedia,
    SettingsTab::VoiceVideo,
    SettingsTab::Accessibility,
    SettingsTab::Language,
    SettingsTab::Keybinds,
    SettingsTab::WindowsApp,
    SettingsTab::Advanced,
    SettingsTab::Applications,
];

fn close_context_menu(view: &mut ClientView, cx: &mut Context<ClientView>) {
    view.sidebar_context_menu = None;
    cx.notify();
}

pub(crate) fn render_context_menu(
    view: &ClientView,
    viewport: Size<Pixels>,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> Option<AnyElement> {
    let menu = view.sidebar_context_menu.as_ref()?;
    let width = px(280.).min((viewport.width - px(18.)).max(px(1.)));
    let anchor = menu.anchor;
    let handle = cx.entity().downgrade();
    let dismiss = move |_: &mut Window, cx: &mut App| {
        let _ = handle.update(cx, close_context_menu);
    };
    let row = |id: &'static str, tag: &'static str, label: String| {
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(label.clone())
            .min_h(px(30.))
            .px_2()
            .py_1()
            .rounded(px(4.))
            .flex()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .hover(|style| style.bg(theme.control_hover_bg))
            .flx(tag)
            .child(div().min_w_0().child(label))
    };

    let content = div()
        .id("sidebar-context-menu")
        .debug_selector(|| "sidebar-context-menu".to_owned())
        .w(width)
        .max_h((viewport.height - px(18.)).max(px(1.)))
        .overflow_y_scroll()
        .p_1()
        .text_sm()
        .text_color(theme.foreground);

    let content = match menu.kind {
        SidebarMenuKind::FluxerButton => content
            .flx("ui.action-menu.fluxer-button-context-menu.menu-group")
            .child(row("fluxer-menu-mark-dms-read", "ui.action-menu.fluxer-button-context-menu.menu-item.mark-all-dms-read", l10n::tr("Mark All DMs as Read"))
                .on_click(cx.listener(|view, _, _, cx| {
                    close_context_menu(view, cx);
                    view.mark_inbox_scope_read(ReadScope::Dms, cx);
                })))
            .child(row("fluxer-menu-toggle-inline-dms", "ui.action-menu.fluxer-button-context-menu.menu-item.toggle-inline-dms", l10n::tr("Collapse Inline DM List"))
                .on_click(cx.listener(|view, _, _, cx| {
                    // TODO(backlog): Connect to the inline DM list collapse preference.
                    close_context_menu(view, cx);
                })))
            .child(row("fluxer-menu-mark-communities-read", "ui.action-menu.fluxer-button-context-menu.menu-item.mark-all-communities-read", l10n::tr("Mark All Communities as Read"))
                .on_click(cx.listener(|view, _, _, cx| {
                    close_context_menu(view, cx);
                    view.mark_inbox_scope_read(ReadScope::Communities, cx);
                })))
            .child(row("fluxer-menu-bulk-notifications", "ui.action-menu.fluxer-button-context-menu.bulk-guild-settings-menu-items", l10n::tr("Notifications"))
                .on_click(cx.listener(|view, _, window, cx| {
                    close_context_menu(view, cx);
                    open_settings_tab(view, SettingsTab::Notifications, window, cx);
                }))),
        SidebarMenuKind::Favorites => content
            .flx("ui.action-menu.favorites-guild-context-menu.menu-group")
            .child(row("favorites-menu-toggle-mute", "ui.action-menu.favorites-guild-context-menu.menu-item.toggle-mute", l10n::tr("Mute Favorites"))
                .on_click(cx.listener(|view, _, _, cx| {
                    // TODO(backlog): Persist favorites mute state once favorites settings exist.
                    close_context_menu(view, cx);
                }))),
        SidebarMenuKind::Header(channel_id) => {
            let (guild_id, actions) = {
                let state = view.backend.state();
                let channel = state.channels.get(&channel_id)?;
                (
                    channel.guild_id,
                    header_actions(
                        channel.kind,
                        channel.guild_id.is_some_and(|guild_id| {
                            super::invite_modal::can_invite_in_guild(&state, guild_id, Some(channel_id))
                        }),
                        state.can_manage_channel(channel_id),
                    ),
                )
            };
            let mut content = content.flx("channel.channel-header.context-menu");
            for action in actions {
                let (id, label) = match action {
                    HeaderAction::Invite => ("header-menu-invite", "Invite members"),
                    HeaderAction::AddPeople => ("header-menu-add-people", "Add Friends to Group"),
                    HeaderAction::CopyLink => ("header-menu-copy-link", "Copy Channel Link"),
                    HeaderAction::Notifications => ("header-menu-notifications", "Notification settings"),
                    HeaderAction::Edit => ("header-menu-edit", "Edit Channel"),
                };
                content = content.child(
                    row(id, id, l10n::tr(label)).on_click(cx.listener(move |view, event: &gpui_kit::ClickEvent, window, cx| {
                        close_context_menu(view, cx);
                        if view.selected_channel != Some(channel_id) {
                            return;
                        }
                        match action {
                            HeaderAction::Invite => {
                                if let Some(guild_id) = guild_id {
                                    view.open_invite_members(guild_id, window, cx);
                                }
                            }
                            HeaderAction::AddPeople => {
                                view.group_dm_panel_open = true;
                                if !view.group_dm_adding {
                                    view.group_dm_toggle_add(cx);
                                }
                            }
                            HeaderAction::CopyLink => view.copy_channel_link(cx),
                            HeaderAction::Notifications => view.toggle_channel_notification_popout(event.position(), window, cx),
                            HeaderAction::Edit => view.open_edit_channel(channel_id, window, cx),
                        }
                    })),
                );
            }
            content
        }
        SidebarMenuKind::Settings => {
            let mut content = content.flx("ui.action-menu.settings-context-menu.menu-group");
            for tab in SETTINGS_MENU_TABS {
                content = content.child(
                    row(tab.debug_selector(), "ui.action-menu.settings-context-menu.menu-item", tab.label())
                        .on_click(cx.listener(move |view, _, window, cx| {
                            close_context_menu(view, cx);
                            open_settings_tab(view, tab, window, cx);
                        })),
                );
            }
            content
        }
    };
    Some(overlay::popover(content, anchor, theme, dismiss))
}

fn open_settings_tab(view: &mut ClientView, tab: SettingsTab, window: &mut Window, cx: &mut Context<ClientView>) {
    if !matches!(view.overlay, Overlay::Settings(_)) {
        view.open_settings(window, cx);
    }
    view.select_settings_tab(tab, cx);
}
