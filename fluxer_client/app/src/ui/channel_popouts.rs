//! Header & composer popouts (`NotificationSettings`, `Threads`,
//! `PinnedMessages`, `Inbox`, and composer `+` action menu).

use fluxer_client_core::state::{
    ClientState, Message, PartialEmoji, Snowflake, message_notifications,
};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, Sizable as _};
use std::cell::RefCell;
use std::collections::HashMap;

use gpui_kit::{
    Anchor, AnyElement, Bounds, Context, Div, Entity, Focusable as _, FontWeight, MouseButton, MouseDownEvent, Pixels,
    Point, Role, SharedString, Size, Stateful, Window, anchored, canvas, div, prelude::*, px, rgb, rgba,
};

use crate::client_view::{ClientView, ResizeEdge};
use crate::icons::AppIcon;
use crate::parity_probe::FlxExt as _;
use crate::theme::Theme;
use crate::ui::media_context_menu::{self, MediaContextMenu, MediaContextMenuTarget};
use crate::ui::message_actions::{self, ActionTarget, MessageAction};
use crate::ui::overlay::Overlay;
use crate::ui::search_panel::SearchScopeChoice;
use crate::ui::{attachments, composer_text, l10n};
use crate::view_model::Place;

pub const POPOUT_CONTEXT: &str = "ChannelPopout";

pub(crate) const PINS_POPOUT_WIDTH_DEFAULT: f32 = 480.0;
pub(crate) const PINS_POPOUT_HEIGHT_DEFAULT: f32 = 640.0;
pub(crate) const PINS_POPOUT_WIDTH_MIN: f32 = 320.0;
pub(crate) const PINS_POPOUT_HEIGHT_MIN: f32 = 200.0;

pub(crate) const INBOX_POPOUT_WIDTH_DEFAULT: f32 = 600.0;
pub(crate) const INBOX_POPOUT_HEIGHT_DEFAULT: f32 = 736.0;
pub(crate) const INBOX_POPOUT_WIDTH_MIN: f32 = 440.0;
pub(crate) const INBOX_POPOUT_HEIGHT_MIN: f32 = 400.0;

thread_local! {
    /// Trigger rects recorded at prepaint (UI thread only), keyed by trigger id.
    static TRIGGER_BOUNDS: RefCell<HashMap<&'static str, Bounds<Pixels>>> = RefCell::new(HashMap::new());
}

/// An invisible child for a trigger element that records the trigger's bounds each
/// frame, so popouts anchor to the trigger rect like the official app does.
pub fn track_trigger(id: &'static str) -> impl IntoElement {
    canvas(
        move |bounds, _, _| {
            TRIGGER_BOUNDS.with(|m| m.borrow_mut().insert(id, bounds));
        },
        |_, (), _, _| {},
    )
    .absolute()
    .size_full()
}

fn trigger_rect(id: &'static str) -> Option<Bounds<Pixels>> {
    TRIGGER_BOUNDS.with(|m| m.borrow().get(id).copied())
}

/// Anchor for header popouts: the trigger's (right, bottom); a click at a
/// 32px trigger's centre is the fallback while bounds are unknown.
pub(crate) fn header_anchor(id: &'static str, click: Point<Pixels>) -> Point<Pixels> {
    match trigger_rect(id) {
        Some(b) => gpui_kit::point(b.right(), b.bottom()),
        None => gpui_kit::point(click.x + px(16.), click.y + px(16.)),
    }
}

/// Anchor for the composer `+` menu: the trigger's (left, top).
pub(crate) fn plus_anchor(id: &'static str, click: Point<Pixels>) -> Point<Pixels> {
    match trigger_rect(id) {
        Some(b) => gpui_kit::point(b.left(), b.top()),
        None => gpui_kit::point(click.x - px(16.), click.y - px(16.)),
    }
}

/// Anchor for the expression picker: (trigger right + 16, trigger top - 8).
pub(crate) fn picker_anchor(id: &'static str, click: Point<Pixels>) -> Point<Pixels> {
    match trigger_rect(id) {
        Some(b) => gpui_kit::point(b.right() + px(16.), b.top() - px(8.)),
        None => gpui_kit::point(click.x + px(32.), click.y - px(24.)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThreadsTab {
    #[default]
    Active,
    Closed,
    ClosedPrivate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InboxTab {
    #[default]
    Unread,
    Bookmarks,
    Mentions,
}

pub enum ChannelPopoutKind {
    NotificationSettings {
        channel_id: Snowflake,
    },
    Threads {
        channel_id: Snowflake,
        tab: ThreadsTab,
        query: Entity<InputState>,
        _subscription: gpui_kit::Subscription,
    },
    PinnedMessages {
        channel_id: Snowflake,
        seq: u64,
        loading: bool,
        items: Vec<Message>,
        error: Option<String>,
    },
    Inbox {
        tab: InboxTab,
    },
    ComposerPlusMenu,
    /// The message "More" / right-click menu.
    MessageMenu {
        target: ActionTarget,
        at_cursor: bool,
        translate_submenu: bool,
    },
    /// Header search bar scope selector (`SearchScopeOptions`).
    SearchScope,
    /// Personal notes sidebar item right-click menu (`PersonalNotesContextMenu`).
    PersonalNotesMenu {
        channel_id: Snowflake,
    },
    /// Explore page language filter (`DiscoveryLanguageDropdown`).
    ExploreLanguage,
    /// Media right-click context menu (`MediaContextMenu`).
    MediaContextMenu(MediaContextMenu),
    MemberContextMenu(Snowflake),
}

pub struct ChannelPopout {
    pub kind: ChannelPopoutKind,
    pub anchor: Point<Pixels>,
}

impl ChannelPopoutKind {
    pub fn key(&self) -> &'static str {
        match self {
            Self::NotificationSettings { .. } => "notification-settings",
            Self::Threads { .. } => "threads",
            Self::PinnedMessages { .. } => "pinned-messages",
            Self::Inbox { .. } => "inbox",
            Self::ComposerPlusMenu => "composer-plus-menu",
            Self::MessageMenu { .. } => "message-menu",
            Self::SearchScope => "search-scope",
            Self::PersonalNotesMenu { .. } => "personal-notes-menu",
            Self::ExploreLanguage => "explore-language",
            Self::MediaContextMenu(_) => "media-context-menu",
            Self::MemberContextMenu(_) => "member-context-menu",
        }
    }
}

impl ClientView {
    pub(crate) fn open_member_context_menu(
        &mut self,
        user_id: Snowflake,
        anchor: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        self.overlay = Overlay::ChannelPopout(ChannelPopout {
            kind: ChannelPopoutKind::MemberContextMenu(user_id),
            anchor,
        });
        cx.notify();
    }

    fn toggle_channel_popout_key(
        &mut self,
        key: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let dismissed = self.dismissed_channel_popout.take();
        if matches!(&self.overlay, Overlay::ChannelPopout(p) if p.kind.key() == key)
            || dismissed == Some(key)
        {
            self.close_overlay(window, cx);
            return false;
        }
        true
    }

    pub(crate) fn toggle_channel_notification_popout(
        &mut self,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(channel_id) = self.selected_channel else { return };
        if !self.toggle_channel_popout_key("notification-settings", window, cx) {
            return;
        }
        let anchor = header_anchor("toggle-channel-mute", anchor);
        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        self.overlay = Overlay::ChannelPopout(ChannelPopout {
            kind: ChannelPopoutKind::NotificationSettings { channel_id },
            anchor,
        });
        cx.notify();
    }

    pub(crate) fn toggle_threads_popout(
        &mut self,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.toggle_channel_popout_key("threads", window, cx) {
            return;
        }
        let anchor = header_anchor("header-threads-btn", anchor);
        self.open_threads_popout(anchor, window, cx);
    }

    pub(crate) fn open_threads_popout(
        &mut self,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(channel_id) = self.selected_channel else { return };
        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        let query = cx.new(|cx| InputState::new(window, cx).placeholder(l10n::tr("Search threads")));
        let _sub = cx.subscribe_in(&query, window, |this: &mut Self, _, ev: &InputEvent, _, cx| {
            if matches!(ev, InputEvent::Change) && matches!(this.overlay, Overlay::ChannelPopout(_)) {
                cx.notify();
            }
        });
        let focus = query.read(cx).focus_handle(cx);
        self.overlay = Overlay::ChannelPopout(ChannelPopout {
            kind: ChannelPopoutKind::Threads {
                channel_id,
                tab: ThreadsTab::Active,
                query,
                _subscription: _sub,
            },
            anchor,
        });
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(crate) fn toggle_pins_popout(
        &mut self,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(channel_id) = self.selected_channel else { return };
        if !self.toggle_channel_popout_key("pinned-messages", window, cx) {
            return;
        }
        let anchor = header_anchor("header-pins-btn", anchor);
        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        self.pins_seq = self.pins_seq.wrapping_add(1);
        let seq = self.pins_seq;
        let cached: Vec<Message> = {
            let state = self.backend.state();
            state
                .messages
                .window(channel_id)
                .map(|w| w.messages.iter().filter(|m| m.pinned).cloned().collect())
                .unwrap_or_default()
        };
        self.overlay = Overlay::ChannelPopout(ChannelPopout {
            kind: ChannelPopoutKind::PinnedMessages {
                channel_id,
                seq,
                loading: cached.is_empty(),
                items: cached,
                error: None,
            },
            anchor,
        });
        self.backend.load_pins(seq, channel_id);
        cx.notify();
    }

    pub(crate) fn toggle_inbox_popout(
        &mut self,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.toggle_channel_popout_key("inbox", window, cx) {
            return;
        }
        if !self.backend.state().saved_messages_loaded {
            self.backend.load_saved_messages();
        }
        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        self.overlay = Overlay::ChannelPopout(ChannelPopout {
            kind: ChannelPopoutKind::Inbox { tab: InboxTab::Unread },
            anchor,
        });
        cx.notify();
    }

    pub(crate) fn toggle_composer_plus_menu(
        &mut self,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_channel.is_none() || !self.can_send {
            return;
        }
        if !self.toggle_channel_popout_key("composer-plus-menu", window, cx) {
            return;
        }
        let anchor = plus_anchor("composer-upload-btn", anchor);
        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        self.overlay = Overlay::ChannelPopout(ChannelPopout {
            kind: ChannelPopoutKind::ComposerPlusMenu,
            anchor,
        });
        cx.notify();
    }

    /// Opens the message menu anchored to the hover bar's More button.
    pub(crate) fn open_message_menu(&mut self, target: ActionTarget, window: &mut Window, cx: &mut Context<Self>) {
        if !self.toggle_channel_popout_key("message-menu", window, cx) {
            return;
        }
        let anchor = match trigger_rect(message_actions::MORE_TRIGGER) {
            Some(b) => gpui_kit::point(b.left(), b.top()),
            None => gpui_kit::point(px(0.), px(0.)),
        };
        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        self.overlay = Overlay::ChannelPopout(ChannelPopout {
            kind: ChannelPopoutKind::MessageMenu {
                target,
                at_cursor: false,
                translate_submenu: false,
            },
            anchor,
        });
        cx.notify();
    }

    /// Opens the message context menu at cursor coordinates (right click on a message row).
    pub(crate) fn open_message_menu_at(
        &mut self,
        target: ActionTarget,
        anchor: Point<Pixels>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !target.sent {
            return;
        }
        self.dismissed_channel_popout = None;
        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        self.overlay = Overlay::ChannelPopout(ChannelPopout {
            kind: ChannelPopoutKind::MessageMenu {
                target,
                at_cursor: true,
                translate_submenu: false,
            },
            anchor,
        });
        cx.notify();
    }

    pub(crate) fn set_message_menu_translate_submenu(&mut self, open: bool, cx: &mut Context<Self>) {
        if let Overlay::ChannelPopout(pop) = &mut self.overlay
            && let ChannelPopoutKind::MessageMenu { translate_submenu, .. } = &mut pop.kind
            && *translate_submenu != open
        {
            *translate_submenu = open;
            cx.notify();
        }
    }

    pub(crate) fn toggle_search_scope_popout(
        &mut self,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.toggle_channel_popout_key("search-scope", window, cx) {
            return;
        }
        let resolved = match trigger_rect("header-search-scope-btn") {
            Some(b) => gpui_kit::point(b.left(), b.bottom()),
            None => gpui_kit::point(anchor.x - px(8.), anchor.y + px(8.)),
        };
        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        self.overlay = Overlay::ChannelPopout(ChannelPopout {
            kind: ChannelPopoutKind::SearchScope,
            anchor: resolved,
        });
        cx.notify();
    }

    pub(crate) fn open_personal_notes_menu(
        &mut self,
        channel_id: Snowflake,
        anchor: Point<Pixels>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dismissed_channel_popout = None;
        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        self.overlay = Overlay::ChannelPopout(ChannelPopout {
            kind: ChannelPopoutKind::PersonalNotesMenu { channel_id },
            anchor,
        });
        cx.notify();
    }

    /// Opens the media context menu at cursor coordinates (right click on an attachment or embed).
    pub(crate) fn open_media_context_menu_at(
        &mut self,
        target: MediaContextMenuTarget,
        anchor: Point<Pixels>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dismissed_channel_popout = None;
        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        self.overlay = Overlay::ChannelPopout(ChannelPopout {
            kind: ChannelPopoutKind::MediaContextMenu(MediaContextMenu { target }),
            anchor,
        });
        cx.notify();
    }

    pub(crate) fn toggle_explore_language_popout(
        &mut self,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.toggle_channel_popout_key("explore-language", window, cx) {
            return;
        }
        let anchor = header_anchor("explore-language-btn", anchor);
        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        self.overlay = Overlay::ChannelPopout(ChannelPopout {
            kind: ChannelPopoutKind::ExploreLanguage,
            anchor,
        });
        cx.notify();
    }

    pub(crate) fn set_threads_popout_tab(&mut self, tab: ThreadsTab, cx: &mut Context<Self>) {
        if let Overlay::ChannelPopout(pop) = &mut self.overlay
            && let ChannelPopoutKind::Threads { tab: active, .. } = &mut pop.kind
        {
            *active = tab;
            cx.notify();
        }
    }

    pub(crate) fn set_inbox_popout_tab(&mut self, tab: InboxTab, cx: &mut Context<Self>) {
        if let Overlay::ChannelPopout(pop) = &mut self.overlay
            && let ChannelPopoutKind::Inbox { tab: active } = &mut pop.kind
        {
            *active = tab;
            if tab == InboxTab::Bookmarks && !self.backend.state().saved_messages_loaded {
                self.backend.load_saved_messages();
            }
            cx.notify();
        }
    }

    pub(crate) fn upload_composer_text_as_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let wire = composer_text::wire_text(self.composer.read(cx));
        self.close_overlay(window, cx);
        if wire.is_empty() {
            return;
        }
        let file = attachments::text_file(&wire);
        self.add_files(vec![file], cx);
        self.composer.update(cx, |s, cx| s.set_value("", window, cx));
        cx.notify();
    }
}

pub fn render(
    view: &ClientView,
    pop: &ChannelPopout,
    viewport: Size<Pixels>,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let dark = theme.background.l < 0.5;
    let bg_for_kind = |kind: &ChannelPopoutKind| -> gpui_kit::Hsla {
        if !dark {
            theme.background_secondary
        } else if matches!(kind, ChannelPopoutKind::Threads { .. }) {
            // ThreadBrowser.module.css .container uses --background-secondary
            rgb(0x1a181e).into()
        } else {
            rgb(0x2a2730).into()
        }
    };
    let bg = bg_for_kind(&pop.kind);
    let border: gpui_kit::Hsla = if dark { rgba(0xc9c5d326).into() } else { theme.border };
    let hover_bg: gpui_kit::Hsla = if dark { rgba(0xffffff12).into() } else { theme.hover_bg };
    let active_bg: gpui_kit::Hsla = if dark { rgb(0x3a3644).into() } else { theme.accent };

    let key = pop.kind.key();
    let (width, height, flx, body): (f32, f32, &'static str, AnyElement) = match &pop.kind {
        ChannelPopoutKind::NotificationSettings { channel_id } => (
            208.,
            207.,
            "ui.action-menu.context-menu.popup",
            render_notification_settings(view, *channel_id, theme, hover_bg, cx),
        ),
        ChannelPopoutKind::Threads { channel_id, tab, query, .. } => (
            480.,
            576.,
            "threads.thread-browser.container",
            render_threads(view, *channel_id, *tab, query, theme, active_bg, hover_bg, cx),
        ),
        ChannelPopoutKind::PinnedMessages {
            loading,
            items,
            error,
            ..
        } => (
            view.pins_popout_width,
            view.pins_popout_height,
            "channel.channel-pins-popout.container",
            render_pins(*loading, items, error.as_deref(), view.pins_popout_height, theme, hover_bg, cx),
        ),
        ChannelPopoutKind::Inbox { tab } => (
            view.inbox_popout_width,
            view.inbox_popout_height,
            "messaging.inbox-popout.container",
            render_inbox(view, *tab, view.inbox_popout_height, theme, active_bg, hover_bg, cx),
        ),
        ChannelPopoutKind::ComposerPlusMenu => (
            208.,
            161.,
            "ui.action-menu.context-menu.popup",
            render_composer_plus(!composer_text::wire_text(view.composer.read(cx)).trim().is_empty(), theme, hover_bg, cx),
        ),
        ChannelPopoutKind::MessageMenu { target, .. } => (
            208.,
            message_menu_height(target),
            "ui.action-menu.context-menu.popup",
            render_message_menu(view, target, theme, hover_bg, cx),
        ),
        ChannelPopoutKind::SearchScope => {
            let opt_count = usize::from(view.selected_channel.is_some())
                + usize::from(matches!(view.place, Place::Guild(_)))
                + 1;
            (
                220.,
                14. + (opt_count as f32) * 34.,
                "ui.action-menu.context-menu.popup",
                render_search_scope(view, theme, hover_bg, cx),
            )
        }
        ChannelPopoutKind::PersonalNotesMenu { channel_id } => (
            208.,
            44.,
            "ui.action-menu.personal-notes-context-menu.menu-group",
            render_personal_notes_menu(*channel_id, hover_bg, cx),
        ),
        ChannelPopoutKind::ExploreLanguage => (
            220.,
            360.,
            "discovery.discovery.discovery-language-dropdown.menu-group",
            render_explore_language(view, theme, hover_bg, cx),
        ),
        ChannelPopoutKind::MediaContextMenu(menu) => {
            let target_for_cb = menu.target.clone();
            (
                208.,
                media_context_menu::menu_height(menu.target.kind),
                "ui.action-menu.context-menu.popup",
                media_context_menu::render_menu_body(
                    &menu.target,
                    theme,
                    cx.listener(move |this, action, window, cx| {
                        this.close_overlay(window, cx);
                        media_context_menu::execute_action(this, *action, &target_for_cb, window, cx);
                    }),
                ),
            )
        }
        ChannelPopoutKind::MemberContextMenu(user_id) => {
            let state = view.backend.state();
            let is_other = state.current_user_id() != Some(*user_id);
            let can_message = is_other
                && state.relationship_kind(*user_id)
                    != Some(fluxer_client_core::state::RelationshipKind::Blocked);
            let mention = view
                .selected_channel
                .filter(|&id| view.can_send && state.can_send_messages(id))
                .and_then(|_| {
                    let member = view.current_guild_id()
                        .and_then(|guild| state.members.get(&guild)?.get(user_id));
                    let user = member.map(|member| &member.user).or_else(|| state.users.get(user_id))?;
                    Some(member.and_then(|member| member.nick.as_deref())
                        .filter(|name| !name.is_empty())
                        .unwrap_or_else(|| user.display_name())
                        .to_owned())
                });
            (
                208.,
                44. + 32. * (usize::from(can_message) + usize::from(mention.is_some()) + usize::from(is_other)) as f32,
                "ui.action-menu.context-menu.popup",
                render_member_menu(*user_id, pop.anchor, mention, can_message, is_other, hover_bg, cx),
            )
        }
    };

    let vw = f32::from(viewport.width);
    let vh = f32::from(viewport.height);
    let ax = f32::from(pop.anchor.x);
    let ay = f32::from(pop.anchor.y);
    // Official anchoring (spec rects): header-button popouts are right-aligned to the
    // trigger button (anchor = trigger right/bottom) and sit 8px below it;
    // the inbox keeps its fixed 52px / right-16px slot; the composer menu opens upward
    // from the trigger's left edge with a 4px gap.
    let (anchor_corner, left, top) = match &pop.kind {
        ChannelPopoutKind::Inbox { .. } => (Anchor::TopLeft, (vw - width - 16.).max(16.), 52.0_f32.min((vh - height - 12.).max(44.))),
        // Right-click message / personal-notes / media menus open at cursor clamped within viewport.
        ChannelPopoutKind::MessageMenu { at_cursor: true, .. }
        | ChannelPopoutKind::PersonalNotesMenu { .. }
        | ChannelPopoutKind::MemberContextMenu(_)
        | ChannelPopoutKind::MediaContextMenu(_) => {
            (Anchor::TopLeft, ax.clamp(12., (vw - width - 12.).max(12.)), ay.clamp(12., (vh - height - 12.).max(12.)))
        }
        ChannelPopoutKind::SearchScope => (
            Anchor::TopLeft,
            ax.clamp(12., (vw - width - 12.).max(12.)),
            (ay + 12.).clamp(44., (vh - height - 16.).max(44.)),
        ),
        // The message menu opens left of the More button (8px gap), top-aligned to it and
        // pushed up to stay 12px inside the window.
        ChannelPopoutKind::MessageMenu { at_cursor: false, .. } => {
            (Anchor::TopRight, (ax - 8.).max(width + 16.), ay.min(vh - height - 12.).max(12.))
        }
        ChannelPopoutKind::ComposerPlusMenu => (Anchor::BottomLeft, ax.clamp(16., (vw - width - 16.).max(16.)), ay - 4.),
        _ => (Anchor::TopRight, ax.clamp(width + 16., vw - 16.), (ay + 8.).clamp(44., (vh - height - 16.).max(44.))),
    };

    let weak_dismiss = cx.weak_entity();
    let dismiss_out = move |_: &gpui_kit::MouseDownEvent, window: &mut Window, cx: &mut gpui_kit::App| {
        let _ = weak_dismiss.update(cx, |v, cx| {
            v.dismissed_channel_popout = Some(key);
            v.close_overlay(window, cx);
        });
    };
    let translate_sub_content = match &pop.kind {
        ChannelPopoutKind::MessageMenu { target, translate_submenu: true, .. }
            if !target.content.trim().is_empty() =>
        {
            Some(target.content.clone())
        }
        _ => None,
    };
    let mut panel = div()
        .id(SharedString::from(format!("channel-popout-{key}")))
        .debug_selector(move || format!("channel-popout-{key}"))
        .key_context(POPOUT_CONTEXT)
        .w(px(width))
        .min_h(px(height))
        .max_h(px((vh - 60.).max(200.)))
        .rounded(px(8.))
        .border_1()
        .border_color(border)
        .bg(bg)
        .shadow_lg()
        .occlude()
        .flex()
        .flex_col()
        .overflow_hidden()
        .capture_action(cx.listener(|this, _: &Escape, window, cx| {
            cx.stop_propagation();
            this.close_overlay(window, cx);
        }))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .flx(flx)
        .relative()
        .child(body);

    if matches!(pop.kind, ChannelPopoutKind::PinnedMessages { .. } | ChannelPopoutKind::Inbox { .. }) {
        let is_pins = matches!(pop.kind, ChannelPopoutKind::PinnedMessages { .. });
        let prefix = if is_pins { "pins" } else { "inbox" };
        let make_handle = |suffix: &'static str, edge: ResizeEdge, aria: String| {
            let id = SharedString::from(format!("{prefix}-resize-{suffix}"));
            let dbg = id.clone();
            div()
                .id(id)
                .debug_selector(move || dbg.to_string())
                .aria_label(aria)
                .absolute()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                        if is_pins {
                            this.begin_pins_popout_resize(edge, ev, cx);
                        } else {
                            this.begin_inbox_popout_resize(edge, ev, cx);
                        }
                    }),
                )
                .flx("ui.resizable-pane.resizable-pane-handles.handle.button")
        };
        let left_handle = make_handle(
            "left",
            ResizeEdge::Left,
            l10n::tr(if is_pins {
                "Resize pinned messages from the left edge"
            } else {
                "Resize inbox from the left edge"
            }),
        )
        .top_0()
        .bottom_0()
        .left_0()
        .w(px(8.))
        .cursor_w_resize();
        let bottom_handle = make_handle(
            "bottom",
            ResizeEdge::Bottom,
            l10n::tr(if is_pins {
                "Resize pinned messages from the bottom edge"
            } else {
                "Resize inbox from the bottom edge"
            }),
        )
        .bottom_0()
        .left_0()
        .right_0()
        .h(px(8.))
        .cursor_ns_resize();
        let corner_handle = make_handle(
            "bottom-left",
            ResizeEdge::BottomLeft,
            l10n::tr(if is_pins {
                "Resize pinned messages from bottom left"
            } else {
                "Resize inbox from bottom left"
            }),
        )
        .bottom_0()
        .left_0()
        .size(px(14.))
        .cursor_nesw_resize();
        panel = panel.child(left_handle).child(bottom_handle).child(corner_handle);
    }

    let (final_anchor, final_left, anchored_child): (Anchor, f32, AnyElement) = match translate_sub_content {
        Some(content) => {
            let panel_left = if matches!(anchor_corner, Anchor::TopRight) { left - width } else { left };
            let sub_on_right = panel_left + width + 190. <= vw - 12.;
            let sub_col = div()
                .pt(px(64.))
                .child(render_translate_submenu(&content, theme, bg, border, hover_bg, cx));
            let wrap = div()
                .id("channel-popout-submenu-wrap")
                .flex()
                .flex_row()
                .items_start()
                .gap_1()
                .on_mouse_down_out(dismiss_out);
            if sub_on_right {
                (Anchor::TopLeft, panel_left, wrap.child(panel).child(sub_col).into_any_element())
            } else {
                (Anchor::TopRight, panel_left + width, wrap.child(sub_col).child(panel).into_any_element())
            }
        }
        None => {
            panel = panel.on_mouse_down_out(dismiss_out);
            (anchor_corner, left, panel.into_any_element())
        }
    };

    div()
        .absolute()
        .top_0()
        .left_0()
        .child(anchored().position(gpui_kit::point(px(final_left), px(top))).anchor(final_anchor).child(anchored_child))
        .into_any_element()
}

fn render_member_menu(
    user_id: Snowflake,
    anchor: Point<Pixels>,
    mention: Option<String>,
    can_message: bool,
    can_note: bool,
    hover_bg: gpui_kit::Hsla,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    div()
        .p(px(5.))
        .child(
            context_menu_item("member-menu-profile".to_owned(), hover_bg)
                .child(l10n::tr("Profile"))
                .on_click(cx.listener(move |view, _, window, cx| {
                    view.open_profile(user_id, view.current_guild_id(), anchor, window, cx);
                })),
        )
        .when_some(mention, |menu, name| {
            menu.child(
                context_menu_item("member-menu-mention".to_owned(), hover_bg)
                    .child(l10n::tr("Mention"))
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.close_overlay(window, cx);
                        let mention = composer_text::Insert::Token {
                            display: format!("@{name}"),
                            wire: format!("<@{user_id}>"),
                        };
                        composer_text::insert_at_cursor(&view.composer, &mention, window, cx);
                    })),
            )
        })
        .when(can_message, |menu| {
            menu.child(
                context_menu_item("member-menu-message".to_owned(), hover_bg)
                    .child(l10n::tr("Message"))
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.close_overlay(window, cx);
                        let existing = crate::ui::profile_popout::find_1on1_dm(&view.backend.state(), user_id);
                        match existing {
                            Some(channel_id) => view.open_channel(None, channel_id, cx),
                            None => view.backend.open_dm(user_id),
                        }
                    })),
            )
        })
        .when(can_note, |menu| {
            menu.child(
                context_menu_item("member-menu-note".to_owned(), hover_bg)
                    .child(l10n::tr("Add Note"))
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.open_profile(user_id, view.current_guild_id(), anchor, window, cx);
                        if let Overlay::Profile(pop) = &view.overlay {
                            let focus = pop.note_input.read(cx).focus_handle(cx);
                            window.defer(cx, move |window, cx| focus.focus(window, cx));
                        }
                    })),
            )
        })
        .into_any_element()
}

/// Height of the message menu popup: 1px border + 6px padding each side, 52.5px
/// quick-reaction section, 32px items, 9px between groups (4px margins + 1px line).
pub(crate) fn message_menu_height(target: &ActionTarget) -> f32 {
    let groups = message_actions::menu_groups(target);
    let items: f32 = groups
        .iter()
        .flatten()
        .map(|item| match item {
            message_actions::MenuItem::QuickReactions => 52.5,
            message_actions::MenuItem::Action(_) => 32.,
        })
        .sum();
    14. + items + 9. * groups.len().saturating_sub(1) as f32
}

/// The message "More" / right-click menu (`MessageContextMenu.tsx`). Each item calls
/// `ClientView::message_action` with its own [`MessageAction`].
fn render_message_menu(
    view: &ClientView,
    target: &ActionTarget,
    theme: &Theme,
    hover_bg: gpui_kit::Hsla,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let speaking = view.tts.is_speaking();
    let danger: gpui_kit::Hsla = rgb(0xe8605f).into();
    let separator = || {
        div().mx(px(2.)).my(px(4.)).h(px(1.)).w(px(190.)).flex_none().bg(gpui_kit::Hsla::from(rgba(0xc9c5d326))).flx("ui.action-menu.context-menu.separator")
    };
    let mut body = div().flex().flex_col().flx("ui.action-menu.context-menu.menu-scroller");
    for (gi, group) in message_actions::menu_groups(target).into_iter().enumerate() {
        if gi > 0 {
            body = body.child(separator());
        }
        let mut col = div().flex().flex_col().flx("ui.action-menu.context-menu.menu-group.group");
        for item in group {
            col = col.child(match item {
                message_actions::MenuItem::QuickReactions => {
                    let key = target.message_id.0;
                    let (channel_id, message_id) = (target.channel_id, target.message_id);
                    div()
                        .px(px(6.))
                        .pt(px(4.))
                        .pb(px(6.))
                        .flex()
                        .flex_row()
                        .gap(px(4.))
                        .flx("ui.action-menu.message-context-menu.render-reactions-header.aria-menu-section")
                        .on_mouse_move(cx.listener(|this, _, _, cx| this.set_message_menu_translate_submenu(false, cx)))
                        .children(message_actions::MENU_QUICK_EMOJI.iter().map(|name| {
                            let emoji = PartialEmoji { id: None, name: (*name).to_owned(), animated: false };
                            div()
                                .id(SharedString::from(format!("msg-menu-react-{key}-{name}")))
                                .role(Role::Button)
                                .flex_1()
                                .h(px(42.5))
                                .rounded(px(6.))
                                .bg(gpui_kit::Hsla::from(rgba(0xffffff1a)))
                                .cursor_pointer()
                                .hover(move |s| s.bg(hover_bg))
                                .flex()
                                .items_center()
                                .justify_center()
                                .flx("ui.action-menu.message-context-menu.quick-reaction-context-menu-item.aria-menu-item")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.close_overlay(window, cx);
                                    this.pick_reaction(channel_id, message_id, emoji.clone(), cx);
                                }))
                                .child(
                                    div()
                                        .size(px(24.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_size(px(20.))
                                        .line_height(px(24.))
                                        .flx("ui.action-menu.message-context-menu.quick-reaction-context-menu-item.quick-reaction-emoji")
                                        .child(*name),
                                )
                        }))
                        .into_any_element()
                }
                message_actions::MenuItem::Action(action) => {
                    let label = if action == MessageAction::Speak && speaking {
                        l10n::tr("Stop speaking")
                    } else {
                        message_actions::tooltip(action, target)
                    };
                    let item_icon = if action == MessageAction::Speak && speaking {
                        AppIcon::StopCircleFill
                    } else {
                        message_actions::icon(action, target)
                    };
                    let color = if action == MessageAction::Delete { danger } else { theme.foreground };
                    let icon_color = if action == MessageAction::Delete { danger } else { theme.muted_foreground };
                    let t = target.clone();
                    let row = context_menu_item(format!("msg-menu-{action:?}"), hover_bg)
                        .text_color(color)
                        .on_mouse_move(cx.listener(move |this, _, _, cx| {
                            this.set_message_menu_translate_submenu(action == MessageAction::Translate, cx);
                        }))
                        .on_click(cx.listener(move |this, ev: &gpui_kit::ClickEvent, window, cx| {
                            let dispatched = if ev.modifiers().shift {
                                match action {
                                    MessageAction::Delete => MessageAction::DeleteNow,
                                    MessageAction::Pin => MessageAction::PinNow,
                                    other => other,
                                }
                            } else {
                                action
                            };
                            this.close_overlay(window, cx);
                            this.message_action(dispatched, &t, window, cx);
                        }));
                    if action == MessageAction::React {
                        // A submenu trigger (emoji picker) in the official app.
                        row.flx_text("ui.action-menu.context-menu.sub-menu.trigger", label.clone())
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .h(px(18.))
                                    .flx("ui.action-menu.context-menu.sub-menu.item-label-container")
                                    .flx("ui.action-menu.context-menu.sub-menu.item-label-text")
                                    .child(label),
                            )
                            .child(
                                div()
                                    .size(px(14.))
                                    .flex_none()
                                    .flx("ui.action-menu.context-menu.sub-menu.submenu-caret")
                                    .child(Icon::new(AppIcon::CaretRight).with_size(px(14.)).text_color(theme.muted_foreground)),
                            )
                            .into_any_element()
                    } else {
                        row.flx_text("ui.action-menu.context-menu.menu-item.item", label.clone())
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .h(px(18.))
                                    .flx("ui.action-menu.context-menu.menu-item.item-label")
                                    .child(label),
                            )
                            .child(
                                div()
                                    .size(px(18.))
                                    .flex_none()
                                    .flx("ui.action-menu.context-menu.menu-item.item-icon")
                                    .child(
                                        div()
                                            .size(px(18.))
                                            .flx(message_actions::menu_icon_flx(action))
                                            .child(Icon::new(item_icon).with_size(px(18.)).text_color(icon_color)),
                                    ),
                            )
                            .into_any_element()
                    }
                }
            });
        }
        body = body.child(col);
    }
    div().p(px(6.)).child(body).into_any_element()
}

fn render_translate_submenu(
    content: &SharedString,
    theme: &Theme,
    bg: gpui_kit::Hsla,
    border: gpui_kit::Hsla,
    hover_bg: gpui_kit::Hsla,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    div()
        .id("message-translate-submenu")
        .debug_selector(|| "message-translate-submenu".to_owned())
        .w(px(186.))
        .p(px(6.))
        .rounded(px(8.))
        .border_1()
        .border_color(border)
        .bg(bg)
        .shadow_lg()
        .occlude()
        .flex()
        .flex_col()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .children(message_actions::TRANSLATION_PROVIDERS.iter().map(|(id, name, _)| {
            let provider_id = *id;
            let text = content.clone();
            context_menu_item(format!("translate-provider-{provider_id}"), hover_bg)
                .text_color(theme.foreground)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.close_overlay(window, cx);
                    this.translate_message(provider_id, &text, window, cx);
                }))
                .child(div().flex_1().min_w_0().truncate().child(*name))
                .child(Icon::new(AppIcon::ArrowSquareOut).with_size(px(14.)).text_color(theme.muted_foreground))
        }))
        .into_any_element()
}

fn channel_notification_state(state: &ClientState, channel_id: Snowflake) -> (bool, u8, i8) {
    let guild_id = state.channels.get(&channel_id).and_then(|c| c.guild_id);
    let now_ms = i64::try_from(crate::accounts::now_ms()).unwrap_or(i64::MAX);
    let muted = state.is_channel_muted(guild_id, channel_id, now_ms);
    let override_level = state
        .guild_settings
        .get(&guild_id)
        .and_then(|s| s.channel_overrides.get(&channel_id))
        .and_then(|o| o.message_notifications)
        .and_then(|n| u8::try_from(n).ok())
        .filter(|&v| v <= 3)
        .unwrap_or(3);
    let guild_default = guild_id
        .and_then(|g| state.guilds.get(&g))
        .map(|g| g.effective_message_notifications())
        .unwrap_or(message_notifications::ALL_MESSAGES);
    (muted, override_level, guild_default)
}

fn render_notification_settings(
    view: &ClientView,
    channel_id: Snowflake,
    theme: &Theme,
    hover_bg: gpui_kit::Hsla,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let (muted, level, guild_default) = {
        let state = view.backend.state();
        channel_notification_state(&state, channel_id)
    };
    let default_sub = if guild_default == message_notifications::ONLY_MENTIONS {
        l10n::tr("Only mentions")
    } else {
        l10n::tr("All messages")
    };
    let mute_label = if muted {
        l10n::tr("Unmute channel")
    } else {
        l10n::tr("Mute channel")
    };

    let options: [(u8, String, Option<String>); 4] = [
        (3, l10n::tr("Community default"), Some(default_sub)),
        (0, l10n::tr("All messages"), None),
        (1, l10n::tr("Only mentions"), None),
        (2, l10n::tr("Nothing"), None),
    ];

    let mut options = options.into_iter();
    let default_option = options.next();
    let radio_row = |(opt_val, label, sub): (u8, String, Option<String>)| {
        let selected = level == opt_val;
        let radio_border = if selected { theme.brand } else { gpui_kit::Hsla::from(rgba(0xc9c5d326)) };
        context_menu_item(format!("notif-level-{opt_val}"), hover_bg)
            .aria_selected(selected)
            .flx_text("ui.action-menu.context-menu.selectable-menu-item.radio-item", label.clone())
            .on_click(cx.listener(move |this, _, window, cx| {
                this.set_channel_notification_level(channel_id, opt_val, cx);
                this.close_overlay(window, cx);
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_row()
                    .flx("ui.action-menu.menu-item-radio.item-label")
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .flex_col()
                            .flx("ui.action-menu.menu-item-radio.div")
                            .flx("ui.action-menu.menu-item-radio.span--2")
                            .child(div().h(px(18.)).text_size(px(13.)).text_color(theme.foreground).child(label))
                            .children(sub.map(|s| {
                                div().h(px(17.)).text_size(px(12.)).line_height(px(16.)).text_color(theme.muted_foreground).child(s)
                            })),
                    ),
            )
            .child(
                div()
                    .size(px(18.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .flx("ui.action-menu.menu-item-radio.checkbox-indicator")
                    .child(
                        div()
                            .size(px(16.))
                            .rounded_full()
                            .border_1()
                            .border_color(radio_border)
                            .flex()
                            .items_center()
                            .justify_center()
                            .flx("ui.action-menu.menu-item-radio.div")
                            .when(selected, |d| {
                                d.child(
                                    div()
                                        .size(px(6.))
                                        .rounded_full()
                                        .bg(gpui_kit::Hsla::from(rgb(0xffffff)))
                                        .flx("ui.action-menu.menu-item-radio.div--2"),
                                )
                            }),
                    ),
            )
    };
    let separator = || {
        div().mx(px(2.)).my(px(4.)).h(px(1.)).w(px(190.)).flex_none().bg(gpui_kit::Hsla::from(rgba(0xc9c5d326))).flx("ui.action-menu.context-menu.separator")
    };

    div()
        .p(px(6.))
        .flex()
        .flex_col()
        .child(
            context_menu_item("notif-popout-mute".to_owned(), hover_bg)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_channel_mute(cx);
                    this.close_overlay(window, cx);
                }))
                .flx("ui.action-menu.context-menu.sub-menu.trigger")
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h(px(18.))
                        .text_size(px(13.))
                        .text_color(theme.foreground)
                        .flx("ui.action-menu.context-menu.sub-menu.item-label-container")
                        .flx("ui.action-menu.context-menu.sub-menu.item-label-text")
                        .child(mute_label),
                )
                .child(
                    div()
                        .size(px(14.))
                        .flex_none()
                        .flx("ui.action-menu.context-menu.sub-menu.submenu-caret")
                        .child(Icon::new(AppIcon::CaretRight).with_size(px(14.)).text_color(theme.muted_foreground)),
                ),
        )
        .child(separator())
        .children(default_option.map(radio_row))
        .child(separator())
        .children(options.map(radio_row))
        .into_any_element()
}

/// Shared context-menu row (ContextMenu.module.css `.item`): 1px transparent border,
/// 5px/8px padding, 6px radius, 13px/18px text, 32px minimum height.
fn context_menu_item(id: String, hover_bg: gpui_kit::Hsla) -> Stateful<Div> {
    let selector = id.clone();
    div()
        .id(SharedString::from(id))
        .debug_selector(move || selector.clone())
        .role(Role::Button)
        .min_h(px(32.))
        .px(px(8.))
        .py(px(5.))
        .border_1()
        .border_color(gpui_kit::Hsla::transparent_black())
        .rounded(px(6.))
        .cursor_pointer()
        .hover(move |s| s.bg(hover_bg))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(8.))
        .text_size(px(13.))
        .line_height(px(18.))
        .font_weight(FontWeight::MEDIUM)
}

#[allow(clippy::too_many_arguments)]
fn render_threads(
    view: &ClientView,
    channel_id: Snowflake,
    tab: ThreadsTab,
    query: &Entity<InputState>,
    theme: &Theme,
    active_bg: gpui_kit::Hsla,
    hover_bg: gpui_kit::Hsla,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let q = query.read(cx).value().trim().to_lowercase();
    let threads: Vec<(Snowflake, String)> = {
        let state = view.backend.state();
        let mut out: Vec<(Snowflake, String)> = state
            .channels
            .values()
            .filter(|c| c.kind.is_thread() && c.parent_id == Some(channel_id))
            .filter_map(|c| {
                let name = c.name.clone().unwrap_or_else(|| "thread".to_owned());
                if !q.is_empty() && !name.to_lowercase().contains(&q) {
                    return None;
                }
                Some((c.id, name))
            })
            .collect();
        out.sort_by_key(|(id, _)| *id);
        out
    };
    let show_rows = tab == ThreadsTab::Active && !threads.is_empty();
    let tabs = [
        (ThreadsTab::Active, "Active"),
        (ThreadsTab::Closed, "Closed"),
        (ThreadsTab::ClosedPrivate, "Closed private"),
    ];

    div()
        .h(px(574.))
        .flex()
        .flex_col()
        .child(
            div()
                .flex_none()
                .pl(px(16.))
                .pr(px(12.))
                .py(px(8.))
                .gap_2()
                .border_b_1()
                .border_color(gpui_kit::hsla(252. / 360., 0.17, 0.80, 0.15))
                .flx("threads.thread-browser.header")
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .flex_1()
                        .min_w_0()
                        .gap_2()
                        .child(
                            div()
                                .size(px(20.))
                                .flex_none()
                                .flx("threads.thread-browser.header-icon")
                                .child(Icon::new(AppIcon::Chats).with_size(px(20.)).text_color(theme.foreground)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_base()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.foreground)
                                .line_height(px(20.))
                                .flx("threads.thread-browser.title")
                                .child(l10n::tr("Threads")),
                        ),
                )
                .child(
                    div().debug_selector(|| "threads-create-btn".to_owned()).flx("threads.thread-browser.button.create").h(px(34.)).flex().items_center().child(
                        Button::new("threads-create-btn-inner")
                            .primary()
                            .small()
                            .label(l10n::tr("Create thread"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_overlay(window, cx);
                                this.open_create_channel(None, window, cx);
                            })),
                    ),
                ),
        )
        .child(div().flex_none().px_4().pt_3().child(Input::new(query)))
        .child(
            div()
                .flex_none()
                .mx_4()
                .mt_3()
                .p_1()
                .rounded(px(8.))
                .bg(theme.background)
                .flex()
                .flex_row()
                .gap_1()
                .children(tabs.into_iter().map(|(t, label)| {
                    let active = t == tab;
                    let text = l10n::tr(label);
                    div()
                        .id(SharedString::from(format!("threads-tab-{t:?}")))
                        .debug_selector(move || format!("threads-tab-{t:?}"))
                        .role(Role::Button)
                        .aria_selected(active)
                        .flex_1()
                        .py_1()
                        .rounded(px(6.))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_xs()
                        .font_weight(if active { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM })
                        .text_color(if active { theme.foreground } else { theme.muted_foreground })
                        .when(active, |d| d.bg(active_bg))
                        .when(!active, |d| d.hover(move |s| s.bg(hover_bg)))
                        .flx_text("ui.segmented-tabs.segmented-tabs.tab.button", text.clone())
                        .child(text)
                        .on_click(cx.listener(move |this, _, _, cx| this.set_threads_popout_tab(t, cx)))
                })),
        )
        .child(if show_rows {
            div()
                .id("threads-list-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p_3()
                .flex()
                .flex_col()
                .gap_1()
                .children(threads.into_iter().map(|(tid, name)| {
                    div()
                        .id(SharedString::from(format!("thread-row-{}", tid.0)))
                        .px_3()
                        .py_2()
                        .rounded(px(6.))
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover_bg))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.select_channel(tid, cx);
                            this.close_overlay(window, cx);
                        }))
                        .child(Icon::new(AppIcon::Chats).with_size(px(16.)).text_color(theme.muted_foreground))
                        .child(div().text_sm().text_color(theme.foreground).child(name))
                }))
                .into_any_element()
        } else {
            div()
                .flex_1()
                .min_h_0()
                .px(px(40.))
                .pt(px(10.))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .flx("threads.thread-browser.empty")
                .child(
                    div()
                        .size(px(40.))
                        .flex_none()
                        .flx("threads.thread-browser.empty-state.icon")
                        .child(Icon::new(AppIcon::Chats).with_size(px(40.)).text_color(theme.muted_foreground)),
                )
                .child(
                    div()
                        .text_base()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.foreground)
                        .line_height(px(20.))
                        .flx("threads.thread-browser.empty-state.title")
                        .child(l10n::tr("There are no threads")),
                )
                .child(
                    div()
                        .text_sm()
                        .line_height(px(20.))
                        .text_color(theme.muted_foreground)
                        .flx("threads.thread-browser.empty-state.hint")
                        .child(l10n::tr("Stay focused on a conversation with a thread, a temporary text channel.")),
                )
                .into_any_element()
        })
        .into_any_element()
}

fn render_pins(
    loading: bool,
    items: &[Message],
    error: Option<&str>,
    height: f32,
    theme: &Theme,
    hover_bg: gpui_kit::Hsla,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    div()
        .h(px((height - 2.).max(1.)))
        .flex()
        .flex_col()
        .child(
            div()
                .flex_none()
                .px_4()
                .h(px(68.))
                .pt(px(3.))
                .border_b_1()
                .border_color(theme.border)
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .size(px(24.))
                        .flex_none()
                        .flx("channel.channel-pins-popout.icon-large")
                        .child(Icon::new(AppIcon::PushPin).with_size(px(24.)).text_color(theme.foreground)),
                )
                .child(
                    div()
                        .text_base()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.foreground)
                        .line_height(px(20.))
                        .flx("channel.channel-pins-popout.title")
                        .child(l10n::tr("Pinned messages")),
                ),
        )
        .child(if let Some(err) = error {
            div()
                .flex_1()
                .min_h_0()
                .px_6()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(theme.danger)
                .child(err.to_owned())
                .into_any_element()
        } else if loading && items.is_empty() {
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(l10n::tr("Loading pinned messages…"))
                .into_any_element()
        } else if items.is_empty() {
            div()
                .flex_1()
                .min_h_0()
                .px_8()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .flx("channel.channel-pins-popout.empty")
                .child(Icon::new(AppIcon::PushPin).with_size(px(36.)).text_color(theme.muted_foreground))
                .child(
                    div()
                        .text_base()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.foreground)
                        .child(l10n::tr("No pinned messages")),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(l10n::tr("Pinned messages show up here.")),
                )
                .into_any_element()
        } else {
            div()
                .id("pins-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p_3()
                .flex()
                .flex_col()
                .gap_2()
                .children(items.iter().map(|msg| {
                    let ch_id = msg.channel_id;
                    let msg_id = msg.id;
                    let author = msg.author.global_name.clone().unwrap_or_else(|| msg.author.username.clone());
                    let content = if msg.content.trim().is_empty() {
                        l10n::tr("(attachment)")
                    } else {
                        msg.content.clone()
                    };
                    div()
                        .id(SharedString::from(format!("pin-row-{}", msg_id.0)))
                        .p_3()
                        .rounded(px(8.))
                        .bg(theme.background)
                        .border_1()
                        .border_color(theme.border)
                        .hover(move |s| s.bg(hover_bg))
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).text_color(theme.foreground).child(author))
                                .child(
                                    Button::new(SharedString::from(format!("pin-jump-{}", msg_id.0)))
                                        .ghost()
                                        .small()
                                        .label(l10n::tr("Jump"))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.jump_to_message(ch_id, msg_id, cx);
                                            this.close_overlay(window, cx);
                                        })),
                                ),
                        )
                        .child(div().text_sm().text_color(theme.muted_foreground).child(content))
                }))
                .into_any_element()
        })
        .into_any_element()
}

fn render_inbox(
    view: &ClientView,
    tab: InboxTab,
    height: f32,
    theme: &Theme,
    active_bg: gpui_kit::Hsla,
    hover_bg: gpui_kit::Hsla,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let tab_active_bg: gpui_kit::Hsla = if theme.background.l < 0.5 { rgba(0xffffff1a).into() } else { active_bg };
    let tabs = [
        (InboxTab::Unread, AppIcon::Bell, "Unread"),
        (InboxTab::Bookmarks, AppIcon::BookmarkSimple, "Bookmarks"),
        (InboxTab::Mentions, AppIcon::AtBold, "Mentions"),
    ];
    let has_unread = {
        let state = view.backend.state();
        state.channels.values().any(|c| {
            c.kind.has_messages()
                && state.can_view_channel(c.id)
                && {
                    let u = state.channel_unread(c.id);
                    u.unread || u.mention_count > 0
                }
        })
    };

    let body: AnyElement = match tab {
        InboxTab::Unread => {
            let unread_channels: Vec<(Snowflake, String, u32, Option<Snowflake>)> = {
                let state = view.backend.state();
                let mut rows: Vec<(Snowflake, String, u32, Option<Snowflake>)> = state
                    .channels
                    .values()
                    .filter(|c| c.kind.has_messages() && state.can_view_channel(c.id))
                    .filter_map(|c| {
                        let u = state.channel_unread(c.id);
                        if !u.unread && u.mention_count == 0 {
                            return None;
                        }
                        let name = c.name.clone().unwrap_or_else(|| format!("channel-{}", c.id.0));
                        Some((c.id, format!("# {name}"), u.mention_count, c.last_message_id))
                    })
                    .collect();
                rows.sort_by_key(|(id, _, _, _)| *id);
                rows
            };
            if unread_channels.is_empty() {
                div()
                    .flex_1()
                    .min_h_0()
                    .px_8()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .flx("messaging.inbox-popout.empty")
                    .child(Icon::new(AppIcon::SparkleFill).with_size(px(36.)).text_color(theme.muted_foreground))
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.foreground)
                            .child(l10n::tr("You're all caught up")),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(l10n::tr("No unread channels or direct messages right now.")),
                    )
                    .into_any_element()
            } else {
                div()
                    .id("inbox-unread-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(unread_channels.into_iter().map(|(cid, title, mentions, last_message_id)| {
                        let row_group = SharedString::from(format!("inbox-unread-row-{}", cid.0));
                        div()
                            .id(SharedString::from(format!("inbox-unread-{}", cid.0)))
                            .group(row_group.clone())
                            .px_3()
                            .py_2()
                            .rounded(px(6.))
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover_bg))
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_channel(None, cid, cx);
                                this.close_overlay(window, cx);
                            }))
                            .child(div().flex_1().min_w_0().truncate().text_sm().text_color(theme.foreground).child(title))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_1()
                                    .when(mentions > 0, |d| d.child(crate::ui::badge(mentions, theme)))
                                    .when_some(last_message_id, |d, last| {
                                        d.child(
                                            div()
                                                .id(SharedString::from(format!("inbox-unread-actions-{}", cid.0)))
                                                .flex()
                                                .flex_row()
                                                .items_center()
                                                .gap_1()
                                                .opacity(0.)
                                                .group_hover(row_group, |s| s.opacity(1.))
                                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                                .on_click(|_, _, cx| cx.stop_propagation())
                                                .child(
                                                    Button::new(("inbox-unread-ack", cid.0))
                                                        .ghost()
                                                        .xsmall()
                                                        .icon(AppIcon::Check)
                                                        .tooltip(l10n::tr("Mark as read"))
                                                        .accessibility_label(l10n::tr("Mark as read"))
                                                        .on_click(cx.listener(move |this, _, _, cx| {
                                                            cx.stop_propagation();
                                                            this.backend.ack(cid, last);
                                                            this.refresh(cx);
                                                        })),
                                                ),
                                        )
                                    }),
                            )
                    }))
                    .into_any_element()
            }
        }
        InboxTab::Bookmarks => {
            let bookmarks: Vec<(Snowflake, Snowflake, String, String)> = {
                let state = view.backend.state();
                let mut list: Vec<_> = state
                    .saved_messages
                    .values()
                    .map(|entry| {
                        let (author, content) = match &entry.message {
                            Some(m) => {
                                let author = m.author.global_name.clone().unwrap_or_else(|| m.author.username.clone());
                                let body = if m.content.trim().is_empty() {
                                    l10n::tr("(attachment)")
                                } else {
                                    m.content.clone()
                                };
                                (author, body)
                            }
                            None => (l10n::tr("Message"), l10n::tr("Message preview unavailable.")),
                        };
                        (entry.channel_id, entry.message_id, author, content)
                    })
                    .collect();
                list.sort_by(|a, b| b.1.cmp(&a.1));
                list
            };
            if bookmarks.is_empty() {
                div()
                    .flex_1()
                    .min_h_0()
                    .px_8()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .flx("messaging.inbox-popout.empty")
                    .child(Icon::new(AppIcon::SparkleFill).with_size(px(36.)).text_color(theme.muted_foreground))
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.foreground)
                            .child(l10n::tr("No bookmarks yet")),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(l10n::tr("Bookmark messages to find them here later.")),
                    )
                    .into_any_element()
            } else {
                div()
                    .id("inbox-bookmarks-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .children(bookmarks.into_iter().map(|(ch_id, msg_id, author, content)| {
                        div()
                            .id(SharedString::from(format!("inbox-bookmark-{}", msg_id.0)))
                            .debug_selector(move || format!("inbox-bookmark-{}", msg_id.0))
                            .p_2p5()
                            .rounded(px(8.))
                            .bg(theme.background)
                            .border_1()
                            .border_color(theme.border)
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(theme.foreground)
                                            .child(author),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .debug_selector(move || format!("bookmark-jump-{}", msg_id.0))
                                                    .child(
                                                        Button::new(SharedString::from(format!("bookmark-jump-{}", msg_id.0)))
                                                            .ghost()
                                                            .small()
                                                            .label(l10n::tr("Jump"))
                                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                                this.jump_to_message(ch_id, msg_id, cx);
                                                                this.close_overlay(window, cx);
                                                            })),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .debug_selector(move || format!("bookmark-remove-{}", msg_id.0))
                                                    .child(
                                                        Button::new(SharedString::from(format!("bookmark-remove-{}", msg_id.0)))
                                                            .ghost()
                                                            .small()
                                                            .label(l10n::tr("Remove"))
                                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                                this.backend.toggle_bookmark(ch_id, msg_id);
                                                                this.refresh(cx);
                                                            })),
                                                    ),
                                            ),
                                    ),
                            )
                            .child(div().text_sm().text_color(theme.muted_foreground).child(content))
                    }))
                    .into_any_element()
            }
        }
        InboxTab::Mentions => {
            let my_id = view.user.as_ref().map(|u| u.id);
            let mentions: Vec<(Snowflake, Snowflake, String, String)> = {
                let state = view.backend.state();
                let mut found = Vec::new();
                if let Some(me) = my_id {
                    for ch in state.channels.values() {
                        if let Some(win) = state.messages.window(ch.id) {
                            for m in win.messages.iter().rev() {
                                if m.mention_everyone || m.mentions.iter().any(|u| u.id == me) {
                                    let author =
                                        m.author.global_name.clone().unwrap_or_else(|| m.author.username.clone());
                                    found.push((m.channel_id, m.id, author, m.content.clone()));
                                    if found.len() >= 25 {
                                        break;
                                    }
                                }
                            }
                        }
                        if found.len() >= 25 {
                            break;
                        }
                    }
                }
                found
            };
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .children((!mentions.is_empty()).then(|| {
                    div()
                        .p_3()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .children(mentions.into_iter().map(|(ch_id, msg_id, author, content)| {
                            div()
                                .id(SharedString::from(format!("inbox-mention-{}", msg_id.0)))
                                .p_2p5()
                                .rounded(px(8.))
                                .bg(theme.background)
                                .border_1()
                                .border_color(theme.border)
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(theme.foreground)
                                                .child(author),
                                        )
                                        .child(
                                            Button::new(SharedString::from(format!("mention-jump-{}", msg_id.0)))
                                                .ghost()
                                                .small()
                                                .label(l10n::tr("Jump"))
                                                .on_click(cx.listener(move |this, _, window, cx| {
                                                    this.jump_to_message(ch_id, msg_id, cx);
                                                    this.close_overlay(window, cx);
                                                })),
                                        ),
                                )
                                .child(div().text_sm().text_color(theme.muted_foreground).child(content))
                        }))
                }))
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .px_8()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .flx("messaging.inbox-popout.empty")
                        .child(
                            Icon::new(AppIcon::SparkleFill)
                                .with_size(px(32.))
                                .text_color(theme.muted_foreground),
                        )
                        .child(
                            div()
                                .text_base()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.foreground)
                                .child(l10n::tr("You're at the end")),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .child(l10n::tr("You've seen all your recent mentions. More will appear here soon.")),
                        ),
                )
                .into_any_element()
        }
    };

    div()
        .h(px((height - 2.).max(1.)))
        .flex()
        .flex_row()
        .child(
            div()
                .w(px(46.))
                .h_full()
                .py_2()
                .border_r_1()
                .border_color(theme.border)
                .flx("messaging.inbox-popout.sidebar")
                .flex()
                .flex_col()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_2()
                        .flx("messaging.inbox-popout.sidebar-tab-list")
                        .children(tabs.into_iter().map(|(t, icon, label)| {
                            let active = t == tab;
                            let tr_label = l10n::tr(label);
                            let tip_label = tr_label.clone();
                            div()
                                .id(SharedString::from(format!("inbox-tab-tooltip-{t:?}")))
                                .flx("messaging.inbox-popout.render-tab-button.tooltip")
                                .tooltip(move |window, cx| {
                                    Tooltip::new(SharedString::from(tip_label.clone())).build(window, cx)
                                })
                                .child(
                                    div()
                                        .id(SharedString::from(format!("inbox-tab-{t:?}")))
                                        .debug_selector(move || format!("inbox-tab-{t:?}"))
                                        .role(Role::Button)
                                        .aria_label(tr_label.clone())
                                        .aria_selected(active)
                                        .size(px(30.))
                                        .rounded(px(6.))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .when(active, |d| d.bg(tab_active_bg))
                                        .when(!active, |d| d.hover(move |s| s.bg(hover_bg)))
                                        .flx_text(
                                            "messaging.inbox-popout.render-tab-button.sidebar-tab.set-active-tab.button",
                                            tr_label,
                                        )
                                        .child(
                                            div()
                                                .size(px(16.))
                                                .flex_none()
                                                .flx(match t {
                                                    InboxTab::Unread => "messaging.inbox-popout.icon-small",
                                                    InboxTab::Bookmarks => "messaging.inbox-popout.icon-small--2",
                                                    InboxTab::Mentions => "messaging.inbox-popout.icon-small--3",
                                                })
                                                .child(
                                                    Icon::new(icon)
                                                        .with_size(px(16.))
                                                        .text_color(if active { theme.foreground } else { theme.muted_foreground }),
                                                ),
                                        )
                                        .on_click(cx.listener(move |this, _, _, cx| this.set_inbox_popout_tab(t, cx))),
                                )
                        })),
                )
                .when(tab == InboxTab::Unread, |s| {
                    s.child(
                        div()
                            .w_full()
                            .pt_2()
                            .flex()
                            .items_center()
                            .justify_center()
                            .flx("messaging.inbox-popout.sidebar-footer-actions--2")
                            .child(
                                div()
                                    .id("inbox-mark-all-read-btn")
                                    .role(Role::Button)
                                    .aria_label(l10n::tr("Mark all inbox channels as read"))
                                    .size(px(30.))
                                    .rounded(px(6.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .tooltip(move |window, cx| {
                                        Tooltip::new(SharedString::from(l10n::tr("Mark all as read"))).build(window, cx)
                                    })
                                    .when(has_unread, |d| {
                                        d.cursor_pointer()
                                            .hover(move |st| st.bg(hover_bg))
                                            .on_click(cx.listener(|this, _, _, cx| this.mark_all_inbox_read(cx)))
                                    })
                                    .when(!has_unread, |d| d.opacity(0.5))
                                    .flx("messaging.inbox-popout.sidebar-action-button.mark-all-read")
                                    .child(
                                        Icon::new(AppIcon::Check)
                                            .with_size(px(16.))
                                            .text_color(theme.muted_foreground),
                                    ),
                            ),
                    )
                }),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex_none()
                        .px_4()
                        .py_3()
                        .border_b_1()
                        .border_color(theme.border)
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_base()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.foreground)
                                .child(l10n::tr(match tab {
                                    InboxTab::Unread => "Unread",
                                    InboxTab::Bookmarks => "Bookmarks",
                                    InboxTab::Mentions => "Mentions",
                                })),
                        )
                        .child(
                            div()
                                .id("inbox-close-btn")
                                .role(Role::Button)
                                .aria_label(l10n::tr("Close"))
                                .size(px(24.))
                                .rounded(px(4.))
                                .cursor_pointer()
                                .hover(move |s| s.bg(hover_bg))
                                .flex()
                                .items_center()
                                .justify_center()
                                .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx)))
                                .child(Icon::new(AppIcon::X).with_size(px(16.)).text_color(theme.muted_foreground)),
                        ),
                )
                .child(body),
        )
        .into_any_element()
}

fn render_composer_plus(has_text: bool, theme: &Theme, hover_bg: gpui_kit::Hsla, cx: &mut Context<ClientView>) -> AnyElement {
    let item_row = |id: &'static str, icon: AppIcon, label: String, hint: Option<AnyElement>| {
        let has_hint = hint.is_some();
        context_menu_item(id.to_owned(), hover_bg)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .when(id != "composer-menu-customize", |d| d.flx("ui.action-menu.context-menu.menu-item.item-label"))
                    .when(id == "composer-menu-customize", |d| {
                        d.flx("ui.action-menu.context-menu.sub-menu.item-label-container")
                            .flx("ui.action-menu.context-menu.sub-menu.item-label-text")
                    })
                    .child(
                        div()
                            .h(px(18.))
                            .text_color(theme.foreground)
                            .when(has_hint, |d| d.flx("ui.action-menu.context-menu.menu-item.item-label-text"))
                            .child(label.clone()),
                    )
                    .children(hint.map(|h| div().flx("ui.action-menu.context-menu.menu-item.item-hint").child(h))),
            )
            .child(if id == "composer-menu-customize" {
                div()
                    .size(px(14.))
                    .flex_none()
                    .flx("ui.action-menu.context-menu.sub-menu.submenu-caret")
                    .child(Icon::new(icon).with_size(px(14.)).text_color(theme.muted_foreground))
            } else {
                div()
                    .size(px(18.))
                    .flex_none()
                    .when(icon == AppIcon::Chats, |d| d.flx("ui.action-menu.context-menu-icons.create-thread-icon.chats-icon"))
                    .child(Icon::new(icon).with_size(px(18.)).text_color(theme.muted_foreground))
            })
    };
    let upload_lbl = l10n::tr("Upload file");
    let as_file_lbl = l10n::tr("Upload your message as a file");
    let thread_lbl = l10n::tr("Create thread");
    let voice_lbl = l10n::tr("Send voice message");
    let custom_lbl = l10n::tr("Customize");

    div()
        .p(px(6.))
        .flex()
        .flex_col()
        .child(
            item_row("composer-menu-upload", AppIcon::UploadSimpleFill, upload_lbl.clone(), None)
                .flx_text("ui.action-menu.context-menu.menu-item.item", upload_lbl)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.close_overlay(window, cx);
                    this.pick_files(cx);
                })),
        )
        .children(has_text.then(|| {
            item_row("composer-menu-upload-text", AppIcon::PaperclipBold, as_file_lbl.clone(), None)
                .flx_text("ui.action-menu.context-menu.menu-item.item", as_file_lbl)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.upload_composer_text_as_file(window, cx);
                }))
        }))
        .child(
            item_row("composer-menu-create-thread", AppIcon::Chats, thread_lbl.clone(), None)
                .flx_text("ui.action-menu.context-menu.menu-item.item", thread_lbl)
                .on_click(cx.listener(|this, _, window, cx| {
                    let anchor = header_anchor("header-threads-btn", gpui_kit::point(px(660.), px(70.)));
                    this.open_threads_popout(anchor, window, cx);
                })),
        )
        .child(
            item_row(
                "composer-menu-voice",
                AppIcon::MicrophoneBold,
                voice_lbl.clone(),
                Some(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .h(px(20.))
                        .children(["CTRL", "ALT", "V"].map(|k| {
                            div().px_1().rounded(px(3.)).bg(theme.background).text_size(px(11.)).text_color(theme.muted_foreground).child(k)
                        }))
                        .into_any_element(),
                ),
            )
            .flx_text("ui.action-menu.context-menu.menu-item.item", voice_lbl)
            .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
        )
        .child(
            item_row("composer-menu-customize", AppIcon::CaretRight, custom_lbl.clone(), None)
                .flx_text("ui.action-menu.context-menu.sub-menu.trigger", custom_lbl)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.open_settings(window, cx);
                })),
        )
        .into_any_element()
}

fn render_search_scope(
    view: &ClientView,
    theme: &Theme,
    hover_bg: gpui_kit::Hsla,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let has_channel = view.selected_channel.is_some();
    let has_guild = matches!(view.place, Place::Guild(_));
    let active_scope = view.search.as_ref().map(|s| s.scope).unwrap_or(if has_channel {
        SearchScopeChoice::CurrentChannel
    } else if has_guild {
        SearchScopeChoice::CurrentGuild
    } else {
        SearchScopeChoice::All
    });

    let mut options: Vec<(&'static str, SearchScopeChoice, String)> = Vec::new();
    if has_channel {
        options.push((
            "search-scope-opt-channel",
            SearchScopeChoice::CurrentChannel,
            l10n::tr("Current channel"),
        ));
    }
    if has_guild {
        options.push((
            "search-scope-opt-guild",
            SearchScopeChoice::CurrentGuild,
            l10n::tr("Current community"),
        ));
    }
    options.push(("search-scope-opt-all", SearchScopeChoice::All, l10n::tr("All messages")));

    let mut col = div().p(px(6.)).flex().flex_col();
    for (id, choice, label) in options {
        let selected = active_scope == choice;
        let radio_border = if selected { theme.brand } else { gpui_kit::Hsla::from(rgba(0xc9c5d326)) };
        col = col.child(
            context_menu_item(id.to_owned(), hover_bg)
                .debug_selector(move || id.to_owned())
                .aria_selected(selected)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.close_overlay(window, cx);
                    this.open_search(window, cx);
                    this.search_set_scope(choice, cx);
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h(px(18.))
                        .text_size(px(13.))
                        .text_color(theme.foreground)
                        .child(label),
                )
                .child(
                    div()
                        .size(px(18.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .size(px(16.))
                                .rounded_full()
                                .border_1()
                                .border_color(radio_border)
                                .flex()
                                .items_center()
                                .justify_center()
                                .when(selected, |d| {
                                    d.child(div().size(px(6.)).rounded_full().bg(gpui_kit::Hsla::from(rgb(0xffffff))))
                                }),
                        ),
                ),
        );
    }
    col.into_any_element()
}

fn render_personal_notes_menu(
    channel_id: Snowflake,
    hover_bg: gpui_kit::Hsla,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let danger: gpui_kit::Hsla = rgb(0xe8605f).into();
    let label = l10n::tr("Purge personal notes");
    div()
        .p(px(6.))
        .flex()
        .flex_col()
        .child(
            context_menu_item("personal-notes-purge-item".to_owned(), hover_bg)
                .debug_selector(|| "personal-notes-purge-item".to_owned())
                .flx_text("ui.action-menu.personal-notes-context-menu.menu-item.close", label.clone())
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_purge_personal_notes(channel_id, window, cx);
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h(px(18.))
                        .text_size(px(13.))
                        .text_color(danger)
                        .child(label),
                )
                .child(
                    div()
                        .size(px(16.))
                        .flex_none()
                        .flx("ui.action-menu.personal-notes-context-menu.delete-icon")
                        .child(Icon::new(AppIcon::Trash).with_size(px(16.)).text_color(danger)),
                ),
        )
        .into_any_element()
}

fn render_explore_language(
    view: &ClientView,
    theme: &Theme,
    hover_bg: gpui_kit::Hsla,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let active = view.explore.selected_language.as_deref();
    let radio_row = |id: String, lang: Option<String>, label: String, selected: bool, flx_tag: &'static str| {
        let radio_border = if selected { theme.brand } else { gpui_kit::Hsla::from(rgba(0xc9c5d326)) };
        context_menu_item(id, hover_bg)
            .aria_selected(selected)
            .flx(flx_tag)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.close_overlay(window, cx);
                this.explore_select_language(lang.clone(), cx);
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(px(18.))
                    .truncate()
                    .text_size(px(13.))
                    .text_color(theme.foreground)
                    .child(label),
            )
            .child(
                div()
                    .size(px(18.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .size(px(16.))
                            .rounded_full()
                            .border_1()
                            .border_color(radio_border)
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(selected, |d| {
                                d.child(div().size(px(6.)).rounded_full().bg(gpui_kit::Hsla::from(rgb(0xffffff))))
                            }),
                    ),
            )
    };

    let mut col = div()
        .id("explore-lang-scroll")
        .max_h(px(340.))
        .overflow_y_scroll()
        .p(px(6.))
        .flex()
        .flex_col()
        .child(radio_row(
            "explore-lang-all".to_owned(),
            None,
            l10n::tr("All languages"),
            active.is_none(),
            "discovery.discovery.discovery-language-dropdown.menu-item-radio.all",
        ));

    for (code, name) in crate::ui::explore::DISCOVERY_LANGUAGES {
        col = col.child(radio_row(
            format!("explore-lang-{code}"),
            Some((*code).to_owned()),
            (*name).to_owned(),
            active == Some(*code),
            "discovery.discovery.discovery-language-dropdown.menu-item-radio.language",
        ));
    }

    col.into_any_element()
}
