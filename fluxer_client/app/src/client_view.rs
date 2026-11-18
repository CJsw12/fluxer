//! Main layout after login: owns the shared view state (selection,
//! snapshots, scroller, composer) and the actions; the pieces are rendered
//! by `crate::ui`.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use fluxer_client_core::rest::UserProfileFull;
use fluxer_client_core::state::{MessageState, PartialEmoji, Snowflake, Status, Unread};
use gpui_kit::component::input::{InputContent, InputEvent, InputState};
use gpui_kit::component::message_scroller::MessageScrollerState;
use gpui_kit::{
    AnyElement, AnyWindowHandle, Context, Entity, EntityInputHandler as _, EventEmitter, ExternalPaths, FocusHandle,
    Focusable as _, ScrollHandle, SharedString, Subscription, Task, UniformListScrollHandle, Window, div,
    prelude::*, px,
};
use tokio::sync::mpsc;

use crate::ack::AckScheduler;
use crate::backend::{Backend, GatewayStatus, TypingSender, UiEvent};
use crate::frecency::FrecencyStore;
use crate::nav;
use crate::notify::Notifier;
use crate::prefs::PrefsStore;
use crate::shortcuts;
use crate::theme::{self, Theme, ThemeKind};
use crate::ui;
use crate::ui::attachments::AttachmentTray;
use crate::ui::autocomplete::Autocomplete;
use crate::ui::composer::{
    ComposerController, ComposerKeys, ComposerMode, EditTarget, EscapeOutcome, Submit, TypingSignal,
    last_own_message,
};
use crate::ui::composer_text;
use crate::ui::l10n;
use crate::ui::overlay::Overlay;
use crate::ui::member_list::{self, MemberListSubscriptions, MemberPanel};
use crate::ui::message_actions::{self, ActionTarget, MessageAction};
use crate::ui::reply::ReplyingTo;
use crate::view_model::{
    ChannelIcon, ChannelItem, GuildItem, MessageRow, Place, UserSummary, channel_items, channel_name, current_user,
    dm_unread, guild_items, message_pane, opens_message_pane,
};

/// Gateway connection as shown by the user panel's status dot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Connection {
    Connecting,
    Online,
    Offline,
}

/// Emitted to the window root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientEvent {
    /// The user logged out; the session is closed and REST logout was sent.
    LoggedOut,
}

pub struct ClientView {
    pub(crate) backend: Backend,
    pub(crate) place: Place,
    pub(crate) selected_channel: Option<Snowflake>,
    pub(crate) guilds: Vec<GuildItem>,
    pub(crate) home_unread: Unread,
    pub(crate) channels: Vec<ChannelItem>,
    pub(crate) channel_title: SharedString,
    /// Glyph of the selected channel (`#`, `@`...), for the window title.
    pub(crate) channel_glyph: &'static str,
    /// Last title given to the OS window.
    pub(crate) window_title: String,
    acks: AckScheduler,
    _ack_timer: Option<Task<()>>,
    /// Rows shown by the scroller; shared with its row renderer.
    pub(crate) rows: Rc<RefCell<Vec<MessageRow>>>,
    has_more_before: bool,
    /// Newer messages were trimmed from the window (pages back towards the
    /// newest load when the list reaches its bottom).
    has_more_after: bool,
    /// `ClientState::unread_anchor` of the selected channel when it was
    /// opened; places the "NEW" divider until another channel is opened.
    unread_anchor: Option<Snowflake>,
    pub(crate) loading_history: bool,
    pub(crate) can_send: bool,
    pub(crate) gateway_status: GatewayStatus,
    pub(crate) status: SharedString,
    pub(crate) connection: Connection,
    /// The logged-in user (user panel); `None` before READY.
    pub(crate) user: Option<UserSummary>,
    pub(crate) error: Option<SharedString>,
    pub(crate) scroller: Entity<MessageScrollerState>,
    pub(crate) composer: Entity<InputState>,
    /// Normal / replying / editing, driven by the composer's keys.
    pub(crate) composer_mode: ComposerController,
    /// Paces the typing signal of the composer.
    typing: TypingSender,
    /// "X is typing…" for the selected channel.
    pub(crate) typing_text: Option<SharedString>,
    /// Drops expired typers and redraws; armed by `refresh`.
    _typing_timer: Option<Task<()>>,
    /// Message whose quick reactions are open (the action bar's React).
    pub(crate) reaction_picker: Option<Snowflake>,
    /// Message row under the pointer: only it shows the action bar.
    pub(crate) hovered_message: Option<Snowflake>,
    /// For composer updates outside a window callback.
    window_handle: AnyWindowHandle,
    /// Own status (user panel dot); `user_settings.status`.
    pub(crate) own_status: Status,
    /// Member list panel toggle (header button); the panel shows only when
    /// `member_target` is set too.
    pub(crate) member_list_open: bool,
    /// (guild, channel) whose member list the panel shows: the selected
    /// channel when it is a guild channel; `None` for DMs.
    pub(crate) member_target: Option<(Snowflake, Snowflake)>,
    /// Member list snapshot; shared with the list's row renderer.
    pub(crate) member_panel: Rc<RefCell<MemberPanel>>,
    /// Last visible member rows (first, last) reported by the list.
    pub(crate) member_visible: Rc<Cell<Option<(usize, usize)>>>,
    pub(crate) member_scroll: UniformListScrollHandle,
    /// Member list LAZY_REQUEST bookkeeping (tests swap the sink).
    pub(crate) member_subscriptions: MemberListSubscriptions,
    /// The modal / popover slot (quick switcher, emoji picker, profile, settings).
    pub(crate) overlay: Overlay,
    /// Fetched user profiles keyed by `(user_id, guild_id)`.
    pub(crate) profile_cache: BTreeMap<(Snowflake, Option<Snowflake>), UserProfileFull>,
    /// In-flight profile fetches to coalesce duplicate opens.
    pub(crate) profile_in_flight: BTreeSet<(Snowflake, Option<Snowflake>)>,
    /// Device-local settings persisted next to the session store.
    pub(crate) prefs: PrefsStore,
    /// Opened channels, most recent first (quick switcher "Previous").
    pub(crate) recent_visits: Vec<Snowflake>,
    /// Last opened channel per place (`None` = Home), reopened when the
    /// place is selected again.
    last_channel: BTreeMap<Option<Snowflake>, Snowflake>,
    /// The composer's @ / # / : popup.
    pub(crate) autocomplete: Option<Autocomplete>,
    pub(crate) autocomplete_scroll: ScrollHandle,
    /// Emoji usage for autocomplete ranking.
    pub(crate) frecency: FrecencyStore,
    /// Toasts and sounds for incoming messages (tests swap the seams).
    pub(crate) notifier: Notifier,
    /// Whether the OS window is active (notification decision, title).
    pub(crate) window_active: bool,
    /// Unread mentions over all guilds and DMs ("(N) " title prefix).
    pub(crate) mention_total: u32,
    /// Files queued for the next message, per channel.
    pub(crate) attachments: AttachmentTray,
    /// Upload progress (percent) of pending messages with files, by nonce.
    pub(crate) uploads: BTreeMap<Snowflake, u8>,
    /// Files of uploads in flight, by nonce; back to the tray on failure.
    pub(crate) sent_files: BTreeMap<Snowflake, (Snowflake, Vec<ui::attachments::PendingAttachment>)>,
    /// Keeps the layout's key context reachable when nothing inside has focus.
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
    _events: Task<()>,
}

impl ClientView {
    pub fn new(
        backend: Backend,
        mut events: mpsc::UnboundedReceiver<UiEvent>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let scroller = cx.new(|cx| MessageScrollerState::new(0, cx));
        let composer = cx.new(|cx| InputState::new(window, cx).placeholder(l10n::tr("Select a channel")));
        composer.update(cx, |s, cx| s.set_disabled(true, cx));
        let subscriptions = vec![
            cx.subscribe_in(&composer, window, |this: &mut Self, _, ev: &InputEvent, window, cx| match ev {
                InputEvent::PressEnter { shift: false, .. } => this.submit(window, cx),
                InputEvent::Change => {
                    this.composer_changed(cx);
                    this.update_autocomplete(cx);
                }
                _ => {}
            }),
            cx.observe_window_activation(window, |this: &mut Self, window, cx| {
                this.window_activation_changed(window.is_window_active(), cx);
            }),
            cx.observe_window_appearance(window, |_, window, cx| {
                if theme::kind(cx) == ThemeKind::System {
                    theme::apply(ThemeKind::System, window, cx);
                    cx.notify();
                }
            }),
            cx.observe(&scroller, |this: &mut Self, scroller, cx| {
                let s = scroller.read(cx);
                let scrolled_up = s.is_scrolled_up();
                let at_top = scrolled_up && s.logical_scroll_top().item_ix == 0;
                this.scrolled(scrolled_up, at_top, cx);
            }),
        ];
        // tokio -> gpui: the receiver is runtime-agnostic, so it is awaited on
        // the GPUI foreground executor. Bursts are coalesced into one refresh.
        let events_task = cx.spawn(async move |this, cx| {
            while let Some(first) = events.recv().await {
                let mut batch = vec![first];
                while let Ok(more) = events.try_recv() {
                    batch.push(more);
                }
                if this.update(cx, |view, cx| view.handle_events(batch, cx)).is_err() {
                    break;
                }
            }
        });
        // Created here, not first in a render.
        ui::embed::image_cache(cx);
        let (member_panel, member_visible, member_scroll) = member_list::new_panel_state();
        let sink_backend = backend.clone();
        let member_subscriptions = MemberListSubscriptions::with_sink(Box::new(move |r| {
            sink_backend.send_member_list(r.guild_id, r.channel_id, &r.ranges, r.sync)
        }));
        let window_active = window.is_window_active();
        backend.notify_focus().set_focused(window_active);
        let notifier = Self::system_notifier(&backend);
        let mut view = Self {
            backend,
            place: Place::Home,
            selected_channel: None,
            guilds: Vec::new(),
            home_unread: Unread::default(),
            channels: Vec::new(),
            channel_title: SharedString::default(),
            channel_glyph: "",
            window_title: String::new(),
            acks: AckScheduler::default(),
            _ack_timer: None,
            rows: Rc::new(RefCell::new(Vec::new())),
            has_more_before: false,
            has_more_after: false,
            unread_anchor: None,
            loading_history: false,
            can_send: false,
            gateway_status: GatewayStatus::Connecting,
            status: l10n::tr("Connecting…").into(),
            connection: Connection::Connecting,
            user: None,
            error: None,
            scroller,
            composer,
            composer_mode: ComposerController::new(),
            typing: TypingSender::new(),
            typing_text: None,
            _typing_timer: None,
            reaction_picker: None,
            hovered_message: None,
            window_handle: window.window_handle(),
            own_status: Status::Online,
            member_list_open: true,
            member_target: None,
            member_panel,
            member_visible,
            member_scroll,
            member_subscriptions,
            overlay: Overlay::None,
            profile_cache: BTreeMap::new(),
            profile_in_flight: BTreeSet::new(),
            prefs: PrefsStore::default(),
            recent_visits: Vec::new(),
            last_channel: BTreeMap::new(),
            autocomplete: None,
            autocomplete_scroll: ScrollHandle::new(),
            frecency: FrecencyStore::default(),
            notifier,
            window_active,
            mention_total: 0,
            attachments: AttachmentTray::default(),
            uploads: BTreeMap::new(),
            sent_files: BTreeMap::new(),
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
            _events: events_task,
        };
        view.refresh(cx);
        view
    }

    /// Applies device-local preferences loaded from the session directory.
    pub(crate) fn apply_prefs(&mut self, prefs: PrefsStore, window: &mut Window, cx: &mut Context<Self>) {
        self.notifier.enabled = prefs.data.notifications;
        self.notifier.sounds_enabled = prefs.data.sounds;
        if std::env::var_os("FLUXER_THEME").is_none() {
            theme::set_kind(prefs.data.theme, cx);
            theme::apply(prefs.data.theme, window, cx);
        }
        if std::env::var_os("FLUXER_LOCALE").is_none() {
            l10n::set_locale(&prefs.data.locale);
            self.refresh_status_text();
        }
        self.prefs = prefs;
        self.refresh(cx);
    }

    pub(crate) fn current_guild_id(&self) -> Option<Snowflake> {
        match self.place {
            Place::Guild(id) => Some(id),
            Place::Home => None,
        }
    }

    pub(crate) fn prepare_overlay_replace(&mut self, cx: &mut Context<Self>) {
        if matches!(self.overlay, Overlay::Profile(_)) {
            self.save_profile_note(cx);
        }
    }

    pub(crate) fn close_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.overlay.is_open() {
            return;
        }
        self.prepare_overlay_replace(cx);
        let restore_composer = matches!(self.overlay, Overlay::QuickSwitcher(_) | Overlay::EmojiPicker(_));
        self.overlay = Overlay::None;
        if restore_composer {
            self.focus_composer(window, cx);
        }
        cx.notify();
    }

    pub(crate) fn refresh_status_text(&mut self) {
        self.status = match &self.gateway_status {
            GatewayStatus::Connecting => l10n::tr("Connecting…").into(),
            GatewayStatus::Ready => l10n::tr("Connected").into(),
            GatewayStatus::Reconnecting { attempt } => {
                format!("{} ({attempt})", l10n::tr("Reconnecting…")).into()
            }
            GatewayStatus::Closed { reason } => {
                format!("{}: {reason}", l10n::tr("Disconnected")).into()
            }
        };
    }

    pub(crate) fn handle_events(&mut self, batch: Vec<UiEvent>, cx: &mut Context<Self>) {
        let mut connected = false;
        for event in batch {
            match event {
                UiEvent::StateChanged => {}
                UiEvent::Gateway(status) => {
                    // READY/RESUMED: subscriptions are sent again (with
                    // `sync`) by the refresh below.
                    if status == GatewayStatus::Ready {
                        self.member_subscriptions.reset_session();
                        connected = true;
                    }
                    self.connection = match status {
                        GatewayStatus::Ready => Connection::Online,
                        GatewayStatus::Connecting | GatewayStatus::Reconnecting { .. } => Connection::Connecting,
                        GatewayStatus::Closed { .. } => Connection::Offline,
                    };
                    self.gateway_status = status;
                    self.refresh_status_text();
                }
                UiEvent::HistoryLoaded { channel_id, error } => {
                    // A page of a channel left meanwhile neither ends this
                    // channel's load nor shows its error here.
                    if Some(channel_id) == self.selected_channel {
                        self.loading_history = false;
                        if let Some(error) = error {
                            self.error =
                                Some(format!("{}: {error}", l10n::tr("Messages failed to load")).into());
                        }
                    }
                }
                UiEvent::SendFailed { error } => {
                    self.error = Some(format!("{}: {error}", l10n::tr("Failed to send message.")).into());
                }
                UiEvent::ActionFailed { error } => {
                    self.error = Some(error.into());
                }
                UiEvent::DmOpened { channel_id } => self.open_channel(None, channel_id, cx),
                UiEvent::Notify(notification) => self.notify(&notification, cx),
                UiEvent::NotificationClicked { guild_id, channel_id } => {
                    self.notification_clicked(guild_id, channel_id, cx);
                }
                UiEvent::UploadProgress { nonce, percent } => {
                    self.uploads.insert(nonce, percent);
                }
                UiEvent::UploadEnded { nonce } => self.upload_ended(nonce),
                UiEvent::ProfileLoaded { user_id, guild_id, result } => {
                    self.profile_loaded(user_id, guild_id, result, cx);
                }
                UiEvent::NoteSaved { user_id, error } => {
                    if let Some(error) = error {
                        self.error = Some(error.into());
                        self.sync_profile_note_from_state(user_id);
                    }
                }
            }
        }
        if connected {
            self.reload_cleared_channel();
        }
        self.refresh(cx);
    }

    /// After READY the windows of a new session are cleared (and those of
    /// the same session invalidated): the open channel loads its latest
    /// page again instead of staying empty until it is reselected.
    fn reload_cleared_channel(&mut self) {
        let Some(channel_id) = self.selected_channel else { return };
        let cleared = self.backend.state().messages.window(channel_id).is_none_or(|w| !w.ready);
        if cleared {
            self.loading_history = true;
            self.backend.load_messages(channel_id, None);
        }
    }

    /// The message list scrolled (or its rows changed): remember whether it
    /// shows the newest message (incoming messages trim the far side only
    /// then), and load older / newer pages at either end.
    pub(crate) fn scrolled(&mut self, scrolled_up: bool, at_top: bool, cx: &mut Context<Self>) {
        if let Some(channel_id) = self.selected_channel {
            self.backend.state().messages.set_pinned_to_end(channel_id, !scrolled_up);
        }
        if at_top {
            self.load_older(cx);
        } else if !scrolled_up {
            self.load_newer(cx);
        }
        self.maybe_ack(cx);
    }

    /// Re-reads the state and updates every snapshot, the scroller rows and
    /// the composer.
    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        let (pane, can_send, title, glyph, typing_text) = {
            let state = self.backend.state();
            if let Place::Guild(id) = self.place
                && !state.guilds.contains_key(&id)
            {
                self.place = Place::Home;
            }
            if self.selected_channel.is_some_and(|id| !state.channels.contains_key(&id)) {
                self.selected_channel = None;
            }
            self.attachments.retain_channels(|id| state.channels.contains_key(&id));
            self.guilds = guild_items(&state);
            self.home_unread = dm_unread(&state);
            self.mention_total = self
                .guilds
                .iter()
                .fold(self.home_unread.mention_count, |n, g| n.saturating_add(g.unread.mention_count));
            self.user = current_user(&state, self.backend.media());
            self.channels = channel_items(&state, self.place);
            let target = match (self.place, self.selected_channel) {
                (Place::Guild(guild_id), Some(channel_id))
                    if state.channels.get(&channel_id).is_some_and(|c| c.guild_id == Some(guild_id)) =>
                {
                    Some((guild_id, channel_id))
                }
                _ => None,
            };
            if target != self.member_target {
                // Another channel: start at the top with nothing reported.
                self.member_target = target;
                self.member_visible.set(None);
                self.member_scroll = UniformListScrollHandle::new();
            }
            *self.member_panel.borrow_mut() = match target {
                Some((guild_id, channel_id)) => {
                    member_list::member_panel(&state, guild_id, channel_id, self.backend.media())
                }
                None => MemberPanel::default(),
            };
            // A reply whose target was deleted is dropped.
            let reply_gone = match self.composer_mode.mode() {
                ComposerMode::Replying(t) if state.messages.message(t.channel_id, t.message_id).is_none() => {
                    Some(t.message_id)
                }
                _ => None,
            };
            if let Some(id) = reply_gone {
                self.composer_mode.message_deleted(id);
            }
            match self.selected_channel {
                Some(id) => {
                    let channel = state.channels.get(&id);
                    (
                        message_pane(&state, id, self.backend.media(), self.unread_anchor),
                        state.can_send_messages(id),
                        channel.map(channel_name).unwrap_or_default(),
                        channel.map_or("", |c| ChannelIcon::of(c.kind).glyph()),
                        ui::typing::channel_typing_text(&state, id),
                    )
                }
                None => (Default::default(), false, String::new(), "", None),
            }
        };
        self.typing_text = typing_text;
        self._typing_timer = ui::typing::schedule_expiry(&self.backend, cx, Self::refresh);
        self.has_more_before = pane.has_more_before;
        self.has_more_after = pane.has_more_after;
        self.channel_title = title.into();
        self.channel_glyph = glyph;
        self.backend.notify_focus().set_viewing(self.selected_channel);
        let mut rows = pane.rows;
        if !self.uploads.is_empty() {
            for row in rows.iter_mut().filter(|r| r.state == MessageState::Sending) {
                row.upload_percent = self.uploads.get(&row.id).copied();
            }
        }
        self.sync_rows(rows, cx);
        self.set_can_send(can_send, cx);
        self.own_status = self.backend.own_status();
        self.subscribe_member_list();
        self.maybe_ack(cx);
        cx.notify();
    }

    /// Whether the member panel is shown (guild channel and toggled on).
    pub(crate) fn shows_member_list(&self) -> bool {
        self.member_list_open && self.member_target.is_some()
    }

    pub(crate) fn toggle_member_list(&mut self, cx: &mut Context<Self>) {
        self.member_list_open = !self.member_list_open;
        self.member_visible.set(None);
        self.subscribe_member_list();
        cx.notify();
    }

    /// Subscribes the shown channel's member list around the visible rows
    /// (the first page before the list reported any). The tracker skips
    /// unchanged requests, so this runs on every refresh.
    fn subscribe_member_list(&mut self) {
        let Some((guild_id, channel_id)) = self.member_target.filter(|_| self.member_list_open) else { return };
        let (first, last) = self.member_visible.get().unwrap_or((0, 0));
        let ranges = member_list::page_ranges(first, last, &self.member_panel.borrow());
        self.member_subscriptions.subscribe(guild_id, channel_id, &ranges);
    }

    /// The member list showed rows `first..=last`.
    pub(crate) fn member_list_scrolled(&mut self, first: usize, last: usize, cx: &mut Context<Self>) {
        self.member_visible.set(Some((first, last)));
        self.subscribe_member_list();
        cx.notify();
    }

    /// User panel status dot: online -> idle -> dnd -> invisible.
    pub(crate) fn cycle_status(&mut self, cx: &mut Context<Self>) {
        let next = member_list::next_status(self.own_status);
        self.backend.set_presence(next);
        self.own_status = next;
        cx.notify();
    }

    /// Window title: "Fluxer", or "Fluxer - #channel" with a channel open;
    /// "(N) " in front while there are N unread mentions.
    pub(crate) fn title_text(&self) -> String {
        let base = if self.selected_channel.is_none() || self.channel_title.is_empty() {
            "Fluxer".to_owned()
        } else {
            format!("Fluxer - {}{}", self.channel_glyph, self.channel_title)
        };
        match self.mention_total {
            0 => base,
            n => format!("({n}) {base}"),
        }
    }

    /// Schedules a read ack when the open channel is unread and the list is
    /// at the bottom; it is sent after `ACK_DELAY` if that still holds.
    pub(crate) fn maybe_ack(&mut self, cx: &mut Context<Self>) {
        let Some(channel_id) = self.selected_channel else {
            self.cancel_ack();
            return;
        };
        if self.loading_history || self.scroller.read(cx).is_scrolled_up() {
            self.cancel_ack();
            return;
        }
        let newest = self.rows.borrow().iter().rev().find(|r| r.state == MessageState::Sent).map(|r| r.id);
        let Some(newest) = newest else { return };
        let unread = self.backend.state().channel_unread(channel_id);
        if !unread.unread && unread.mention_count == 0 {
            return;
        }
        let now = cx.background_executor().now();
        if let Some(delay) = self.acks.offer(channel_id, newest, now) {
            self.start_ack_timer(delay, cx);
        }
    }

    fn start_ack_timer(&mut self, delay: std::time::Duration, cx: &mut Context<Self>) {
        self._ack_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let _ = this.update(cx, |view, cx| view.flush_ack(cx));
        }));
    }

    fn cancel_ack(&mut self) {
        self.acks.cancel();
        self._ack_timer = None;
    }

    fn flush_ack(&mut self, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        match self.acks.take_due(now) {
            Some((channel_id, message_id)) if Some(channel_id) == self.selected_channel => {
                self.backend.ack(channel_id, message_id);
                self.refresh(cx);
            }
            Some(_) => {}
            None => {
                if let Some(left) = self.acks.remaining(now) {
                    self.start_ack_timer(left, cx);
                }
            }
        }
    }

    /// Splices only the changed middle of the row list so the scroll anchor
    /// survives prepends (older pages) and appends (new messages).
    fn sync_rows(&mut self, new_rows: Vec<MessageRow>, cx: &mut Context<Self>) {
        let (start, old_end, new_count) = {
            let old = self.rows.borrow();
            let prefix = old.iter().zip(&new_rows).take_while(|(a, b)| a == b).count();
            let max_suffix = old.len().min(new_rows.len()) - prefix;
            let suffix = old
                .iter()
                .rev()
                .zip(new_rows.iter().rev())
                .take(max_suffix)
                .take_while(|(a, b)| a == b)
                .count();
            (prefix, old.len() - suffix, new_rows.len() - prefix - suffix)
        };
        *self.rows.borrow_mut() = new_rows;
        if start == old_end && new_count == 0 {
            return;
        }
        self.scroller.update(cx, |s, cx| {
            s.splice(start..old_end, new_count, cx);
        });
    }

    fn set_can_send(&mut self, can_send: bool, cx: &mut Context<Self>) {
        self.can_send = can_send;
        self.composer.update(cx, |s, cx| s.set_disabled(!can_send, cx));
    }

    pub(crate) fn select_place(&mut self, place: Place, cx: &mut Context<Self>) {
        self.place = place;
        if let Place::Guild(id) = place {
            self.backend.subscribe_guild(id);
        }
        self.refresh(cx);
        let opens = |id: Snowflake| self.channels.iter().any(|c| c.id == id && c.has_messages);
        let remembered = self.last_channel.get(&nav::place_key(place)).copied().filter(|id| opens(*id));
        let first = remembered.or_else(|| self.channels.iter().find(|c| c.has_messages).map(|c| c.id));
        match first {
            Some(id) => self.select_channel(id, cx),
            None => {
                self.selected_channel = None;
                self.refresh(cx);
            }
        }
    }

    pub(crate) fn select_channel(&mut self, channel_id: Snowflake, cx: &mut Context<Self>) {
        let opens_pane = self.backend.state().channels.get(&channel_id).is_some_and(|c| opens_message_pane(c.kind));
        if !opens_pane {
            return;
        }
        self.cancel_ack();
        self.selected_channel = Some(channel_id);
        nav::record_visit(&mut self.recent_visits, channel_id);
        self.last_channel.insert(nav::place_key(self.place), channel_id);
        self.autocomplete = None;
        self.error = None;
        self.reaction_picker = None;
        self.hovered_message = None;
        // Replies and edits belong to the old channel.
        if let Some(restore) = self.composer_mode.channel_changed() {
            self.set_composer_later(restore, cx);
        }
        let needs_load = {
            let mut state = self.backend.state();
            self.unread_anchor = state.unread_anchor(channel_id);
            state.messages.select(Some(channel_id));
            state.messages.window(channel_id).is_none_or(|w| !w.ready)
        };
        // New channel: start from an empty list that follows the tail.
        self.rows.borrow_mut().clear();
        self.scroller.update(cx, |s, cx| s.reset(0, cx));
        self.loading_history = needs_load;
        if needs_load {
            self.backend.load_messages(channel_id, None);
        }
        self.refresh(cx);
    }

    fn load_older(&mut self, cx: &mut Context<Self>) {
        if self.loading_history || !self.has_more_before {
            return;
        }
        let (Some(channel_id), Some(oldest)) = (self.selected_channel, self.rows.borrow().first().map(|r| r.id)) else {
            return;
        };
        self.loading_history = true;
        self.backend.load_messages(channel_id, Some(oldest));
        cx.notify();
    }

    /// At the bottom of a window whose newer messages were trimmed: the
    /// page after the newest loaded message (until caught up, incoming
    /// messages are not appended to such a window).
    fn load_newer(&mut self, cx: &mut Context<Self>) {
        if self.loading_history || !self.has_more_after {
            return;
        }
        let newest = self.rows.borrow().iter().rev().find(|r| r.state == MessageState::Sent).map(|r| r.id);
        let (Some(channel_id), Some(newest)) = (self.selected_channel, newest) else { return };
        self.loading_history = true;
        self.backend.load_newer(channel_id, newest);
        cx.notify();
    }

    /// Enter in the composer: send, reply or save the edit as the composer
    /// mode decides.
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Enter during IME composition only commits the composition.
        if self.composer.update(cx, |s, cx| s.marked_text_range(window, cx)).is_some() {
            return;
        }
        let Some(channel_id) = self.selected_channel else { return };
        let editing = matches!(self.composer_mode.mode(), ComposerMode::Editing(_));
        if !self.can_send && !editing {
            return;
        }
        // Mention / channel / custom emoji tokens go out in wire form.
        let text = composer_text::wire_text(self.composer.read(cx));
        // The server counts UTF-16 units; an over-long message keeps the
        // draft instead of failing after the input was cleared.
        if ui::attachments::is_long_text(text.trim()) {
            self.error = Some(l10n::tr("Message is too long").into());
            cx.notify();
            return;
        }
        if !editing && !self.attachments.files(channel_id).is_empty() {
            self.submit_with_files(channel_id, text, window, cx);
            return;
        }
        match self.composer_mode.submit(&text) {
            Submit::Nothing => {}
            Submit::Send { content } => {
                if self.backend.send_message(channel_id, content) {
                    self.after_send(window, cx);
                } else {
                    self.not_ready(cx);
                }
            }
            Submit::Reply { content, target } => {
                let (reply_to, mention) = (target.message_id, target.mention);
                if self.backend.send_reply(target.channel_id, content, reply_to, mention) {
                    self.after_send(window, cx);
                } else {
                    self.composer_mode.start_reply(target);
                    self.not_ready(cx);
                }
            }
            Submit::Edit { channel_id, message_id, content, restore } => {
                self.backend.edit_message(channel_id, message_id, content);
                self.set_composer(restore, window, cx);
                self.error = None;
                self.refresh(cx);
            }
            Submit::EditUnchanged { restore } => {
                self.set_composer(restore, window, cx);
                cx.notify();
            }
            Submit::ConfirmDelete { channel_id, message_id } => {
                self.confirm_delete(channel_id, message_id, window, cx);
            }
        }
    }

    pub(crate) fn after_send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_composer(String::new(), window, cx);
        self.typing.message_sent();
        self.error = None;
        self.refresh(cx);
        self.scroller.update(cx, |s, cx| s.scroll_to_end(cx));
    }

    pub(crate) fn not_ready(&mut self, cx: &mut Context<Self>) {
        self.error = Some(l10n::tr("Check your connection and try again.").into());
        cx.notify();
    }

    /// Replaces the input (text and tokens). `set_value` emits no change
    /// event, so the popup computed for the old text is dropped here: its
    /// byte range would otherwise be applied to the new text.
    fn set_composer(&mut self, content: impl Into<InputContent>, window: &mut Window, cx: &mut Context<Self>) {
        self.autocomplete = None;
        let content = content.into();
        self.composer.update(cx, |s, cx| s.set_value(content, window, cx));
    }

    /// [`Self::set_composer`] from a call without the window (channel
    /// switch): runs after the current update.
    fn set_composer_later(&mut self, content: InputContent, cx: &mut Context<Self>) {
        self.autocomplete = None;
        let (composer, handle) = (self.composer.clone(), self.window_handle);
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| composer.update(cx, |s, cx| s.set_value(content, window, cx)));
        });
    }

    /// IME composition in progress: Enter / Tab / arrows / Esc belong to
    /// the IME (`ComposerIME.ts`), not to the popup or the composer modes.
    fn composing(&self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.composer.update(cx, |s, cx| s.marked_text_range(window, cx)).is_some()
    }

    fn focus_composer(&self, window: &mut Window, cx: &mut Context<Self>) {
        let focus = self.composer.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
    }

    /// The composer text changed: the typing signal.
    fn composer_changed(&mut self, cx: &mut Context<Self>) {
        let Some(channel_id) = self.selected_channel.filter(|_| self.can_send) else { return };
        let text = self.composer.read(cx).value().to_string();
        match self.composer_mode.typing_signal(&text) {
            TypingSignal::Started => self.backend.typing_started(&self.typing, channel_id),
            TypingSignal::Stopped => self.typing.stopped(channel_id),
            TypingSignal::None => {}
        }
    }

    /// Esc in the composer: during IME composition it only ends the
    /// composition (`event.isComposing` returns early in the web client);
    /// otherwise it closes the autocomplete popup, else the quick
    /// reactions, else cancels a reply or edit; with none of those it marks
    /// the channel read (`useChannelComposerGlobalShortcuts` dispatches
    /// `ESCAPE_PRESSED` after the composer affordances). False lets the
    /// input handle it.
    fn on_escape(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.composing(window, cx) {
            self.composer.update(cx, |s, cx| s.unmark_text(window, cx));
            return true;
        }
        if self.autocomplete.take().is_some() {
            cx.notify();
            return true;
        }
        if self.reaction_picker.take().is_some() {
            cx.notify();
            return true;
        }
        if self.cancel_composer_mode(window, cx) {
            return true;
        }
        self.mark_channel_read(cx);
        false
    }

    /// Leaves replying / editing (the edit's draft comes back).
    fn cancel_composer_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match self.composer_mode.escape() {
            EscapeOutcome::Ignored => false,
            EscapeOutcome::CancelledReply => {
                cx.notify();
                true
            }
            EscapeOutcome::CancelledEdit { restore } => {
                self.set_composer(restore, window, cx);
                cx.notify();
                true
            }
        }
    }

    /// Up moves in the autocomplete popup; in an empty composer it edits
    /// the last own message.
    fn on_up(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.composing(window, cx) {
            return false;
        }
        if self.autocomplete.is_some() {
            self.autocomplete_move(-1, cx);
            return true;
        }
        let Some(channel_id) = self.selected_channel else { return false };
        let empty = self.composer.read(cx).value().is_empty();
        let backend = &self.backend;
        match self.composer_mode.arrow_up(empty, || last_own_message(&backend.state(), channel_id)) {
            Some(text) => {
                self.set_composer(text, window, cx);
                cx.notify();
                true
            }
            None => false,
        }
    }

    /// A button of a message's hover action bar.
    pub(crate) fn message_action(
        &mut self,
        action: MessageAction,
        target: &ActionTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (channel_id, message_id) = (target.channel_id, target.message_id);
        match action {
            MessageAction::React => {
                self.reaction_picker = (self.reaction_picker != Some(message_id)).then_some(message_id);
                cx.notify();
            }
            MessageAction::Reply => self.start_reply(channel_id, message_id, window, cx),
            MessageAction::Edit => self.start_edit(channel_id, message_id, window, cx),
            MessageAction::Delete => self.confirm_delete(channel_id, message_id, window, cx),
            MessageAction::DeleteNow => self.delete_message(channel_id, message_id, window, cx),
            MessageAction::CopyText | MessageAction::CopyId => {
                message_actions::copy_to_clipboard(action, target, cx);
            }
            MessageAction::SuppressEmbeds => {
                self.backend.toggle_suppress_embeds(channel_id, message_id);
            }
        }
    }

    /// Something only the rows draw changed (an embed spoiler was revealed):
    /// redraw the list.
    pub(crate) fn redraw_messages(&mut self, cx: &mut Context<Self>) {
        // The rows are drawn inside the scroller entity; redraw it too.
        self.scroller.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    pub(crate) fn start_reply(
        &mut self,
        channel_id: Snowflake,
        message_id: Snowflake,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = {
            let state = self.backend.state();
            state.messages.message(channel_id, message_id).map(|m| ReplyingTo::of(&state, m))
        };
        let Some(target) = target else { return };
        if let Some(restore) = self.composer_mode.start_reply(target) {
            self.set_composer(restore, window, cx);
        }
        self.focus_composer(window, cx);
        cx.notify();
    }

    pub(crate) fn start_edit(
        &mut self,
        channel_id: Snowflake,
        message_id: Snowflake,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self.backend.state().messages.message(channel_id, message_id).map(EditTarget::of);
        let Some(target) = target else { return };
        let current = self.composer.read(cx).content();
        let text = self.composer_mode.start_edit(target, current);
        self.set_composer(text, window, cx);
        self.focus_composer(window, cx);
        cx.notify();
    }

    /// The "Delete Message" dialog; deletes when confirmed.
    fn confirm_delete(&mut self, channel_id: Snowflake, message_id: Snowflake, window: &mut Window, cx: &mut Context<Self>) {
        let preview = self.backend.state().messages.message(channel_id, message_id).map(|m| m.content.clone());
        let view = cx.weak_entity();
        message_actions::confirm_delete(preview.unwrap_or_default().into(), window, cx, move |window, cx| {
            let _ = view.update(cx, |v, cx| v.delete_message(channel_id, message_id, window, cx));
        });
    }

    fn delete_message(&mut self, channel_id: Snowflake, message_id: Snowflake, window: &mut Window, cx: &mut Context<Self>) {
        self.backend.delete_message(channel_id, message_id);
        if let Some(restore) = self.composer_mode.message_deleted(message_id) {
            self.set_composer(restore, window, cx);
        }
        self.refresh(cx);
    }

    /// A reaction pill was clicked: adds or removes the user's reaction.
    pub(crate) fn toggle_reaction(
        &mut self,
        channel_id: Snowflake,
        message_id: Snowflake,
        emoji: PartialEmoji,
        cx: &mut Context<Self>,
    ) {
        if self.backend.toggle_reaction(channel_id, message_id, emoji) {
            self.refresh(cx);
        }
    }

    /// The pointer moved over message `message_id`'s row, or over no
    /// message (a system row, a divider, outside the list): `None`.
    pub(crate) fn hover_message(&mut self, message_id: Option<Snowflake>, cx: &mut Context<Self>) {
        if message_id != self.hovered_message {
            self.hovered_message = message_id;
            // The rows are drawn inside the scroller entity; redraw it too.
            self.scroller.update(cx, |_, cx| cx.notify());
            cx.notify();
        }
    }

    /// An emoji of the quick reactions: closes them and toggles it.
    pub(crate) fn pick_reaction(
        &mut self,
        channel_id: Snowflake,
        message_id: Snowflake,
        emoji: PartialEmoji,
        cx: &mut Context<Self>,
    ) {
        self.reaction_picker = None;
        self.toggle_reaction(channel_id, message_id, emoji, cx);
        cx.notify();
    }

    /// Opens the message's channel and scrolls to it when it is loaded
    /// (reply lines, the "Replying to" bar).
    pub(crate) fn jump_to_message(&mut self, channel_id: Snowflake, message_id: Snowflake, cx: &mut Context<Self>) {
        if self.selected_channel != Some(channel_id) {
            self.select_channel(channel_id, cx);
        }
        let ix = self.rows.borrow().iter().position(|r| r.id == message_id);
        if let Some(ix) = ix {
            self.scroller.update(cx, |s, cx| {
                s.scroll_to_item(ix, cx);
            });
        }
        cx.notify();
    }

    /// "Log out": closes the gateway, sends `POST /auth/logout` and tells the
    /// root, which forgets the stored account and shows the login screen.
    pub(crate) fn log_out(&mut self, cx: &mut Context<Self>) {
        self.cancel_ack();
        self.backend.logout();
        cx.emit(ClientEvent::LoggedOut);
    }
}

impl EventEmitter<ClientEvent> for ClientView {}

impl Render for ClientView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = self.title_text();
        if title != self.window_title {
            window.set_window_title(&title);
            self.window_title = title;
        }
        ui::composer::sync_placeholder(self, window, cx);
        let theme = Theme::current(cx);
        let member_panel = if self.shows_member_list() {
            Some(ui::member_list::render(self, &theme, cx))
        } else {
            None
        };
        let handle = cx.weak_entity();
        let composer = self.render_composer(&theme, cx);
        let overlay = match &self.overlay {
            Overlay::QuickSwitcher(sw) => Some(ui::quick_switcher::render(sw, &theme, cx)),
            Overlay::EmojiPicker(picker) => {
                Some(ui::emoji_picker::render(picker, self.backend.media(), &theme, cx))
            }
            Overlay::Profile(pop) => Some(ui::profile_popout::render(pop, &theme, cx)),
            Overlay::Settings(modal) => Some(ui::settings::render(self, modal, &theme, cx)),
            Overlay::None => None,
        };
        // Every image below without its own cache (avatars, custom emoji,
        // reactions) goes through the budgeted embed cache instead of
        // GPUI's app-wide asset cache, which never drops decoded images.
        let images = ui::embed::image_cache(cx);
        shortcuts::on_actions(div(), cx)
            .image_cache(images)
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .flex()
            .flex_row()
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_size(px(theme.sizes.text))
            .child(ui::guild_rail::render(self, &theme, cx))
            .child(ui::channel_list::render(self, &theme, cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    // Files dropped anywhere on the message pane go to the tray.
                    .on_drop(cx.listener(|v, paths: &ExternalPaths, _, cx| v.add_paths(paths.paths(), cx)))
                    .child(ui::header::render(self, &theme, cx))
                    .child(ui::message_list::render(self, handle, &theme))
                    .child(composer),
            )
            .children(member_panel)
            .children(overlay)
    }
}

impl ClientView {
    /// The composer with its mode bar (reply / edit), the typing row and
    /// the Esc / Up hooks.
    fn render_composer(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let (jump, mention, cancel) = (cx.weak_entity(), cx.weak_entity(), cx.weak_entity());
        let top_bar = ui::composer::mode_bar(
            self.composer_mode.mode(),
            theme,
            move |channel_id, message_id, _, cx| {
                let _ = jump.update(cx, |v, cx| v.jump_to_message(channel_id, message_id, cx));
            },
            move |_, cx| {
                let _ = mention.update(cx, |v, cx| {
                    v.composer_mode.toggle_mention();
                    cx.notify();
                });
            },
            move |window, cx| {
                let _ = cancel.update(cx, |v, cx| v.cancel_composer_mode(window, cx));
            },
        );
        let typing_row = ui::typing::typing_indicator(self.typing_text.clone(), theme);
        let keys = ComposerKeys {
            on_escape: Self::on_escape,
            on_up: Self::on_up,
            on_down: Self::on_down,
            on_accept: Self::on_accept,
        };
        let popup = ui::autocomplete::render(self, theme, cx);
        ui::composer::render_with(self, theme, popup, top_bar, typing_row, keys, cx).into_any_element()
    }
}

mod keyboard;
mod notify_upload;

impl Drop for ClientView {
    fn drop(&mut self) {
        self.backend.close();
    }
}
