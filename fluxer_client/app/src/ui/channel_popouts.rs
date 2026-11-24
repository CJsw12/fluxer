//! Header & composer popouts (`NotificationSettings`, `Threads`,
//! `PinnedMessages`, `Inbox`, and composer `+` action menu).

use fluxer_client_core::state::{
    ClientState, Message, Snowflake, message_notifications,
};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    Anchor, AnyElement, Context, Div, Entity, Focusable as _, FontWeight, MouseButton, Pixels, Point, Role, SharedString,
    Size, Stateful, Window, anchored, div, prelude::*, px, rgb, rgba,
};

use crate::client_view::ClientView;
use crate::icons::AppIcon;
use crate::parity_probe::FlxExt as _;
use crate::theme::Theme;
use crate::ui::overlay::Overlay;
use crate::ui::{attachments, composer_text, l10n};

pub const POPOUT_CONTEXT: &str = "ChannelPopout";

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
        }
    }
}

impl ClientView {
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
        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        self.overlay = Overlay::ChannelPopout(ChannelPopout {
            kind: ChannelPopoutKind::ComposerPlusMenu,
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
            480.,
            640.,
            "channel.channel-pins-popout.container",
            render_pins(*loading, items, error.as_deref(), theme, hover_bg, cx),
        ),
        ChannelPopoutKind::Inbox { tab } => (
            600.,
            736.,
            "messaging.inbox-popout.container",
            render_inbox(view, *tab, theme, active_bg, hover_bg, cx),
        ),
        ChannelPopoutKind::ComposerPlusMenu => (
            208.,
            161.,
            "ui.action-menu.context-menu.popup",
            render_composer_plus(!composer_text::wire_text(view.composer.read(cx)).trim().is_empty(), theme, hover_bg, cx),
        ),
    };

    let vw = f32::from(viewport.width);
    let vh = f32::from(viewport.height);
    let ax = f32::from(pop.anchor.x);
    let ay = f32::from(pop.anchor.y);
    // Official anchoring (spec rects): header-button popouts are right-aligned to the
    // trigger button (32px, anchor = click point at its centre) and sit 8px below it;
    // the inbox keeps its fixed 52px / right-16px slot; the composer menu opens upward
    // from the trigger's left edge with a 4px gap.
    let (anchor_corner, left, top) = match &pop.kind {
        ChannelPopoutKind::Inbox { .. } => (Anchor::TopLeft, (vw - width - 16.).max(16.), 52.0_f32.min((vh - height - 12.).max(44.))),
        ChannelPopoutKind::ComposerPlusMenu => (Anchor::BottomLeft, (ax - 16.).clamp(16., (vw - width - 16.).max(16.)), ay - 20.),
        _ => (Anchor::TopRight, (ax + 16.).clamp(width + 16., vw - 16.), (ay + 24.).clamp(44., (vh - height - 16.).max(44.))),
    };

    let weak_dismiss = cx.weak_entity();
    let panel = div()
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
        .on_mouse_down_out(move |_, window, cx| {
            let _ = weak_dismiss.update(cx, |v, cx| {
                v.dismissed_channel_popout = Some(key);
                v.close_overlay(window, cx);
            });
        })
        .flx(flx)
        .child(body);

    div()
        .absolute()
        .top_0()
        .left_0()
        .child(anchored().position(gpui_kit::point(px(left), px(top))).anchor(anchor_corner).child(panel))
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
        let radio_border = if selected { theme.accent } else { theme.muted_foreground };
        context_menu_item(format!("notif-level-{opt_val}"), hover_bg)
            .aria_selected(selected)
            .flx_text("ui.action-menu.context-menu.selectable-menu-item.radio-item", label.clone())
            .on_click(cx.listener(move |this, _, window, cx| {
                this.set_channel_notification_level(channel_id, opt_val, cx);
                this.close_overlay(window, cx);
            }))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(div().h(px(18.)).text_size(px(13.)).text_color(theme.foreground).child(label))
                    .children(sub.map(|s| {
                        div().h(px(17.)).text_size(px(12.)).line_height(px(16.)).text_color(theme.muted_foreground).child(s)
                    })),
            )
            .child(
                div()
                    .size(px(16.))
                    .rounded_full()
                    .border_1()
                    .border_color(radio_border)
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(selected, |d| d.child(div().size(px(6.)).rounded_full().bg(theme.accent))),
            )
    };
    let separator = || div().mx(px(2.)).my(px(4.)).h(px(1.)).w_full().flex_none().bg(theme.border);

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
                .child(div().h(px(18.)).text_size(px(13.)).text_color(theme.foreground).child(mute_label))
                .child(Icon::new(AppIcon::CaretRight).with_size(px(14.)).text_color(theme.muted_foreground)),
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
                .px(px(12.))
                .py(px(8.))
                .border_b_1()
                .border_color(theme.border)
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(Icon::new(AppIcon::Chats).with_size(px(18.)).text_color(theme.foreground))
                        .child(
                            div()
                                .text_base()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.foreground)
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
                .px_8()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .flx("threads.thread-browser.empty")
                .child(Icon::new(AppIcon::Chats).with_size(px(36.)).text_color(theme.muted_foreground))
                .child(
                    div()
                        .text_base()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.foreground)
                        .child(l10n::tr("There are no threads")),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
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
    theme: &Theme,
    hover_bg: gpui_kit::Hsla,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    div()
        .h(px(638.))
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
                .gap_2()
                .child(Icon::new(AppIcon::PushPin).with_size(px(18.)).text_color(theme.foreground))
                .child(
                    div()
                        .text_base()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.foreground)
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
                .child(Icon::new(AppIcon::Star).with_size(px(36.)).text_color(theme.muted_foreground))
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
    theme: &Theme,
    active_bg: gpui_kit::Hsla,
    hover_bg: gpui_kit::Hsla,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let tab_active_bg: gpui_kit::Hsla = if theme.background.l < 0.5 { rgba(0xffffff1a).into() } else { active_bg };
    let tabs = [
        (InboxTab::Unread, AppIcon::Bell, "Unread"),
        (InboxTab::Bookmarks, AppIcon::Star, "Bookmarks"),
        (InboxTab::Mentions, AppIcon::ChatCircleFill, "Mentions"),
    ];

    let body: AnyElement = match tab {
        InboxTab::Unread => {
            let unread_channels: Vec<(Snowflake, String, u32)> = {
                let state = view.backend.state();
                let mut rows: Vec<(Snowflake, String, u32)> = state
                    .channels
                    .values()
                    .filter(|c| c.kind.has_messages() && state.can_view_channel(c.id))
                    .filter_map(|c| {
                        let u = state.channel_unread(c.id);
                        if !u.unread && u.mention_count == 0 {
                            return None;
                        }
                        let name = c.name.clone().unwrap_or_else(|| format!("channel-{}", c.id.0));
                        Some((c.id, format!("# {name}"), u.mention_count))
                    })
                    .collect();
                rows.sort_by_key(|(id, _, _)| *id);
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
                    .child(Icon::new(AppIcon::Bell).with_size(px(36.)).text_color(theme.muted_foreground))
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
                    .children(unread_channels.into_iter().map(|(cid, title, mentions)| {
                        div()
                            .id(SharedString::from(format!("inbox-unread-{}", cid.0)))
                            .px_3()
                            .py_2()
                            .rounded(px(6.))
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover_bg))
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_channel(None, cid, cx);
                                this.close_overlay(window, cx);
                            }))
                            .child(div().text_sm().text_color(theme.foreground).child(title))
                            .when(mentions > 0, |d| d.child(crate::ui::badge(mentions, theme)))
                    }))
                    .into_any_element()
            }
        }
        InboxTab::Bookmarks => div()
            .flex_1()
            .min_h_0()
            .px_8()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .flx("messaging.inbox-popout.empty")
            .child(Icon::new(AppIcon::Star).with_size(px(36.)).text_color(theme.muted_foreground))
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
            .into_any_element(),
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
                            Icon::new(AppIcon::ChatCircleFill)
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
        .h(px(734.))
        .flex()
        .flex_row()
        .child(
            div()
                .w(px(46.))
                .h_full()
                .py_2()
                .border_r_1()
                .border_color(theme.border)
                .flex()
                .flex_col()
                .items_center()
                .gap_2()
                .children(tabs.into_iter().map(|(t, icon, label)| {
                    let active = t == tab;
                    let tr_label = l10n::tr(label);
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
                            Icon::new(icon)
                                .with_size(px(18.))
                                .text_color(if active { theme.foreground } else { theme.muted_foreground }),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.set_inbox_popout_tab(t, cx)))
                })),
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
        context_menu_item(id.to_owned(), hover_bg)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(div().h(px(18.)).text_color(theme.foreground).child(label.clone()))
                    .children(hint),
            )
            .child(Icon::new(icon).with_size(px(18.)).text_color(theme.muted_foreground))
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
            item_row("composer-menu-upload", AppIcon::LinkSimple, upload_lbl.clone(), None)
                .flx_text("ui.action-menu.context-menu.menu-item.item", upload_lbl)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.close_overlay(window, cx);
                    this.pick_files(cx);
                })),
        )
        .children(has_text.then(|| {
            item_row("composer-menu-upload-text", AppIcon::Plus, as_file_lbl.clone(), None)
                .flx_text("ui.action-menu.context-menu.menu-item.item", as_file_lbl)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.upload_composer_text_as_file(window, cx);
                }))
        }))
        .child(
            item_row("composer-menu-create-thread", AppIcon::Chats, thread_lbl.clone(), None)
                .flx_text("ui.action-menu.context-menu.menu-item.item", thread_lbl)
                .on_click(cx.listener(|this, _, window, cx| {
                    let anchor = gpui_kit::point(px(660.), px(70.));
                    this.open_threads_popout(anchor, window, cx);
                })),
        )
        .child(
            item_row(
                "composer-menu-voice",
                AppIcon::Microphone,
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
