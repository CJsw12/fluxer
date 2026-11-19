//! Main layout after login: owns the shared view state (selection,
//! snapshots, scroller, composer) and the actions; the pieces are rendered
//! by `crate::ui`.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use fluxer_client_core::rest::UserProfileFull;
use fluxer_client_core::state::{MessageState, PartialEmoji, Snowflake, Status, Unread};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
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
use crate::ui::friends::{self, FriendRow, FriendsTab};
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
    /// Switch to another saved account by storage key (preserves current token).
    SwitchAccount(String),
    /// Open login view to add another account (preserves current saved accounts).
    AddAccount,
    /// Remove a saved inactive account from local storage.
    RemoveAccount(String),
}

pub struct ClientView {
    pub(crate) backend: Backend,
    pub(crate) place: Place,
    pub(crate) selected_channel: Option<Snowflake>,
    pub(crate) guilds: Vec<GuildItem>,
    pub(crate) home_unread: Unread,
    pub(crate) channels: Vec<ChannelItem>,
    pub(crate) channel_title: SharedString,
    /// Trimmed non-empty topic of the selected channel, if any.
    pub(crate) channel_topic: Option<SharedString>,
    /// Glyph of the selected channel (`#`, `@`...), for the window title.
    pub(crate) channel_glyph: &'static str,
    /// Current user holds guild-level `MANAGE_CHANNELS` in the active guild.
    pub(crate) can_manage_guild_channels: bool,
    /// Current user holds channel-level `MANAGE_CHANNELS` on `selected_channel`.
    pub(crate) can_manage_selected_channel: bool,
    /// Last title given to the OS window.
    pub(crate) window_title: String,
    acks: AckScheduler,
    _ack_timer: Option<Task<()>>,
    /// Rows shown by the scroller; shared with its row renderer.
    pub(crate) rows: Rc<RefCell<Vec<MessageRow>>>,
    has_more_before: bool,
    /// Newer messages were trimmed from the window (pages back towards the
    /// newest load when the list reaches its bottom).
    pub(crate) has_more_after: bool,
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
    pub(crate) window_handle: AnyWindowHandle,
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
    /// Unsent composer text and tokens per channel (`MAX_DRAFTS` cap).
    pub(crate) drafts: BTreeMap<Snowflake, InputContent>,
    /// Latest deferred composer value not yet applied to `composer`.
    pending_composer: Option<InputContent>,
    /// Monotonic token so older `set_composer_later` defers do not clobber newer switches.
    composer_seq: Rc<Cell<u64>>,
    /// Back/forward navigation stack across visited channels.
    pub(crate) nav_history: nav::NavHistory,
    /// Suppresses pushing a duplicate entry while stepping through history.
    navigating_history: bool,
    /// Most recently visited guild, used by Ctrl+Alt+Right (`ToggleLastGuild`).
    pub(crate) last_guild: Option<Snowflake>,
    /// Active tab in the home Friends surface.
    pub(crate) friends_tab: FriendsTab,
    /// Filtered & sorted rows currently displayed in Friends.
    pub(crate) friend_rows: Vec<FriendRow>,
    /// Count of incoming friend requests (shown on Home / Pending badges).
    pub(crate) pending_incoming: u32,
    /// Username/tag input inside the "Add friend" tab.
    pub(crate) add_friend_input: Entity<InputState>,
    /// Result banner for the most recent "Add friend" request.
    pub(crate) friends_status: Option<Result<String, String>>,
    /// Newly joined guild awaiting its `GUILD_CREATE` gateway snapshot.
    pub(crate) pending_joined_guild: Option<Snowflake>,
    /// Newly joined group DM channel awaiting its `CHANNEL_CREATE` gateway event.
    pub(crate) pending_joined_channel: Option<Snowflake>,
    /// Invite code for the active in-flight accept request.
    pub(crate) pending_accept_code: Option<String>,
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
    /// Right-hand message search drawer (Ctrl+F).
    pub(crate) search: Option<ui::search_panel::SearchPanelState>,
    /// Monotonic counter shared across open/close of the search panel so stale
    /// responses from an earlier panel never apply to a reopened panel.
    pub(crate) search_seq: u64,
    /// Historical message waiting for its `?around=` page before scrolling/highlighting.
    pub(crate) pending_jump: Option<(Snowflake, Snowflake)>,
    /// Currently highlighted message `(channel_id, message_id)` after jumping.
    pub(crate) highlighted_message: Option<(Snowflake, Snowflake)>,
    /// Ignores transient pre-layout scroller observer ticks right after a programmatic jump.
    pub(crate) suppress_jump_scroll: bool,
    /// Group DM right-hand panel toggle (independent of guild `member_list_open`).
    pub(crate) group_dm_panel_open: bool,
    /// Sequence counter for `CreateGroupDm` responses.
    create_group_dm_seq: u64,
    /// Sequence counter for guild channel create/edit/delete responses.
    channel_mgmt_seq: u64,
    /// Snapshot of the open Group DM channel (members, owner, addable friends).
    pub(crate) group_dm: Option<ui::group_dm::GroupDmSnapshot>,
    /// Persistent inline Group DM rename input (reused across frames).
    pub(crate) group_dm_rename: Entity<InputState>,
    /// Whether the inline "Add friends" picker is open in the Group DM panel.
    pub(crate) group_dm_adding: bool,
    /// `(channel_id, custom_name)` last synced into `group_dm_rename`.
    group_dm_rename_synced: Option<(Snowflake, Option<String>)>,
    /// True when backed by a persistent `SessionStore` (false under `--token-env`).
    pub(crate) accounts_enabled: bool,
    /// Token-free list of saved accounts for switching/removing in the UI.
    pub(crate) saved_accounts: Vec<crate::accounts::SavedAccountItem>,
    /// Deep link deferred until READY completes (`self.user.is_some()`) and target arrives.
    pub(crate) pending_deep_link: Option<(crate::deep_links::DeepLink, std::time::Instant)>,
    /// Keeps the layout's key context reachable when nothing inside has focus.
    focus: FocusHandle,
    /// Captured external URLs opened via `launch_external_url` in headless tests.
    #[cfg(test)]
    pub(crate) opened_urls: Vec<String>,
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
        let add_friend_input = cx.new(|cx| InputState::new(window, cx).placeholder("username#0000"));
        let group_dm_rename = cx.new(|cx| InputState::new(window, cx).placeholder(l10n::tr("Group name")));
        let subscriptions = vec![
            cx.subscribe_in(&add_friend_input, window, |this: &mut Self, _, ev: &InputEvent, _, cx| {
                if let InputEvent::PressEnter { shift: false, .. } = ev {
                    this.submit_add_friend(cx);
                }
            }),
            cx.subscribe_in(&group_dm_rename, window, |this: &mut Self, input_entity, ev: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { shift: false, .. } = ev {
                    if input_entity.update(cx, |s, cx| s.marked_text_range(window, cx)).is_some() {
                        return;
                    }
                    this.group_dm_save_name(cx);
                }
            }),
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
            channel_topic: None,
            channel_glyph: "",
            can_manage_guild_channels: false,
            can_manage_selected_channel: false,
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
            drafts: BTreeMap::new(),
            pending_composer: None,
            composer_seq: Rc::new(Cell::new(0)),
            nav_history: nav::NavHistory::default(),
            navigating_history: false,
            last_guild: None,
            friends_tab: FriendsTab::Online,
            friend_rows: Vec::new(),
            pending_incoming: 0,
            add_friend_input,
            friends_status: None,
            pending_joined_guild: None,
            pending_joined_channel: None,
            pending_accept_code: None,
            autocomplete: None,
            autocomplete_scroll: ScrollHandle::new(),
            frecency: FrecencyStore::default(),
            notifier,
            window_active,
            mention_total: 0,
            attachments: AttachmentTray::default(),
            uploads: BTreeMap::new(),
            sent_files: BTreeMap::new(),
            search: None,
            search_seq: 0,
            pending_jump: None,
            highlighted_message: None,
            suppress_jump_scroll: false,
            group_dm_panel_open: true,
            create_group_dm_seq: 0,
            channel_mgmt_seq: 0,
            group_dm: None,
            group_dm_rename,
            group_dm_adding: false,
            group_dm_rename_synced: None,
            accounts_enabled: false,
            saved_accounts: Vec::new(),
            pending_deep_link: None,
            focus: cx.focus_handle(),
            #[cfg(test)]
            opened_urls: Vec::new(),
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
        crate::tray::set_close_to_tray(prefs.data.close_to_tray);
        theme::set_display(prefs.data.display(), cx);
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
        if matches!(self.overlay, Overlay::JoinGuild(_)) {
            self.pending_accept_code = None;
        }
    }

    pub(crate) fn close_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.overlay.is_open() {
            return;
        }
        self.prepare_overlay_replace(cx);
        let restore_composer = matches!(
            self.overlay,
            Overlay::QuickSwitcher(_)
                | Overlay::EmojiPicker(_)
                | Overlay::AccountSwitcher(_)
                | Overlay::JoinGuild(_)
                | Overlay::LeaveGuild(_)
                | Overlay::CreateGroupDm(_)
                | Overlay::LeaveGroupDm(_)
                | Overlay::MediaViewer(_)
                | Overlay::LinkWarning(_)
                | Overlay::CreateChannel(_)
                | Overlay::EditChannel(_)
        );
        self.overlay = Overlay::None;
        if restore_composer {
            self.focus_composer(window, cx);
        }
        cx.notify();
    }

    pub(crate) fn focus_composer_later(&self, cx: &mut Context<Self>) {
        let (composer, handle) = (self.composer.clone(), self.window_handle);
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                let focus = composer.read(cx).focus_handle(cx);
                window.focus(&focus, cx);
            });
        });
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
                            if self.pending_jump.is_some_and(|(ch, _)| ch == channel_id) {
                                self.pending_jump = None;
                            }
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
                UiEvent::FriendRequestSent { input, error } => {
                    self.friends_status = Some(match error {
                        Some(err) => Err(err),
                        None => {
                            if self.add_friend_input.read(cx).value().trim() == input.trim() {
                                let (input_entity, handle) = (self.add_friend_input.clone(), self.window_handle);
                                cx.defer(move |cx| {
                                    let _ = handle.update(cx, |_, window, cx| {
                                        input_entity.update(cx, |s, cx| s.set_value("", window, cx));
                                    });
                                });
                            }
                            Ok(l10n::tr("Friend request sent!"))
                        }
                    });
                }
                UiEvent::InvitePreviewLoaded { code, result } => {
                    if let Overlay::JoinGuild(modal) = &mut self.overlay {
                        let current = modal.input.read(cx).value().to_string();
                        if crate::backend::extract_invite_code(&current).as_deref() != Some(code.as_str()) {
                            continue;
                        }
                        modal.loading = false;
                        match result {
                            Ok(preview) => {
                                modal.preview = Some(preview);
                                modal.error = None;
                            }
                            Err(err) => {
                                modal.preview = None;
                                modal.error = Some(err);
                            }
                        }
                    }
                }
                UiEvent::InviteAccepted { code, result } => {
                    let is_current = self.pending_accept_code.as_deref() == Some(code.as_str())
                        && match &self.overlay {
                            Overlay::JoinGuild(modal) => {
                                crate::backend::extract_invite_code(&modal.input.read(cx).value()).as_deref()
                                    == Some(code.as_str())
                            }
                            _ => false,
                        };
                    if !is_current {
                        continue;
                    }
                    self.pending_accept_code = None;
                    match result {
                        Ok(accepted) => {
                            if matches!(self.overlay, Overlay::JoinGuild(_)) {
                                self.overlay = Overlay::None;
                                let composer = self.composer.clone();
                                let handle = self.window_handle;
                                cx.defer(move |cx| {
                                    let _ = handle.update(cx, |_, window, cx| {
                                        let focus = composer.read(cx).focus_handle(cx);
                                        window.focus(&focus, cx);
                                    });
                                });
                            }
                            if let Some(id) = accepted.guild_id {
                                self.pending_joined_channel = None;
                                if self.backend.state().guilds.contains_key(&id) {
                                    self.pending_joined_guild = None;
                                    self.select_place(Place::Guild(id), cx);
                                } else {
                                    self.pending_joined_guild = Some(id);
                                }
                            } else if let Some(ch) = accepted.channel_id {
                                self.pending_joined_guild = None;
                                if self.backend.state().channels.contains_key(&ch) {
                                    self.pending_joined_channel = None;
                                    self.open_channel(None, ch, cx);
                                } else {
                                    self.pending_joined_channel = Some(ch);
                                }
                            }
                        }
                        Err(err) => {
                            if let Overlay::JoinGuild(modal) = &mut self.overlay {
                                modal.loading = false;
                                modal.error = Some(err);
                            } else {
                                self.error = Some(err.into());
                            }
                        }
                    }
                }
                UiEvent::GuildLeft { guild_id, error } => {
                    if let Some(err) = error {
                        self.error = Some(err.into());
                    } else {
                        self.last_channel.remove(&Some(guild_id));
                        if self.last_guild == Some(guild_id) {
                            self.last_guild = None;
                        }
                        if self.place == Place::Guild(guild_id) {
                            self.place = Place::Home;
                            self.selected_channel = None;
                        }
                    }
                }
                UiEvent::SearchLoaded { seq, result } => {
                    self.search_loaded(seq, result, cx);
                }
                UiEvent::GroupDmCreated { seq, result } => {
                    self.group_dm_created(seq, result, cx);
                }
                UiEvent::GroupDmLeft { channel_id, error } => {
                    if let Some(err) = error {
                        self.error = Some(err.into());
                    } else {
                        if self.last_channel.get(&None).copied() == Some(channel_id) {
                            self.last_channel.remove(&None);
                        }
                        if self.selected_channel == Some(channel_id) {
                            self.clear_selection(cx);
                        }
                    }
                }
                UiEvent::ChannelCreated { seq, guild_id, result } => {
                    if !matches!(&self.overlay, Overlay::CreateChannel(m) if m.seq == seq) {
                        continue;
                    }
                    match result {
                        Ok(channel_id) => {
                            self.overlay = Overlay::None;
                            self.focus_composer_later(cx);
                            if self.place == Place::Guild(guild_id) {
                                let opens = self
                                    .backend
                                    .state()
                                    .channels
                                    .get(&channel_id)
                                    .is_some_and(|c| opens_message_pane(c.kind));
                                if opens {
                                    self.select_channel(channel_id, cx);
                                }
                            }
                        }
                        Err(err) => {
                            if let Overlay::CreateChannel(modal) = &mut self.overlay {
                                modal.submitting = false;
                                modal.error = Some(err);
                            }
                        }
                    }
                }
                UiEvent::ChannelUpdated { seq, error, .. } => {
                    if !matches!(&self.overlay, Overlay::EditChannel(m) if m.seq == seq) {
                        continue;
                    }
                    match error {
                        None => {
                            self.overlay = Overlay::None;
                            self.focus_composer_later(cx);
                        }
                        Some(err) => {
                            if let Overlay::EditChannel(modal) = &mut self.overlay {
                                modal.submitting = false;
                                modal.error = Some(err);
                            }
                        }
                    }
                }
                UiEvent::ChannelDeleted { seq, channel_id, error } => {
                    match error {
                        Some(err) => {
                            if let Overlay::EditChannel(modal) = &mut self.overlay {
                                if modal.seq != seq {
                                    continue;
                                }
                                modal.submitting = false;
                                modal.confirming_delete = false;
                                modal.error = Some(err);
                            } else {
                                self.error = Some(err.into());
                            }
                        }
                        None => {
                            self.drafts.remove(&channel_id);
                            self.last_channel.retain(|_, v| *v != channel_id);
                            if matches!(&self.overlay, Overlay::EditChannel(m) if m.channel_id == channel_id) {
                                self.overlay = Overlay::None;
                                self.focus_composer_later(cx);
                            }
                            if self.selected_channel == Some(channel_id) {
                                let next = {
                                    let state = self.backend.state();
                                    channel_items(&state, self.place)
                                        .into_iter()
                                        .find(|c| c.has_messages && c.id != channel_id)
                                        .map(|c| c.id)
                                };
                                match next {
                                    Some(id) => self.select_channel(id, cx),
                                    None => self.clear_selection(cx),
                                }
                            }
                        }
                    }
                }
            }
        }
        if connected {
            self.reload_cleared_channel();
        }
        self.refresh(cx);
        if self.user.is_some()
            && let Some((link, queued_at)) = self.pending_deep_link.take()
        {
            let expired = queued_at.elapsed() >= std::time::Duration::from_secs(10);
            let (view, handle) = (cx.weak_entity(), self.window_handle);
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, cx| {
                    let _ = view.update(cx, |v, cx| {
                        if !v.try_apply_deep_link(&link, expired, window, cx) && !expired {
                            v.pending_deep_link = Some((link, queued_at));
                        }
                    });
                });
            });
        }
        if let Some((jump_ch, jump_msg)) = self.pending_jump
            && Some(jump_ch) == self.selected_channel
            && !self.loading_history
        {
            self.pending_jump = None;
            if let Some(ix) = self.rows.borrow().iter().position(|r| r.id == jump_msg) {
                self.suppress_jump_scroll = true;
                self.scroller.update(cx, |s, cx| {
                    s.scroll_to_item(ix, cx);
                });
                self.highlighted_message = Some((jump_ch, jump_msg));
            } else if self.error.is_none() {
                self.error = Some(l10n::tr("Message not found").into());
            }
            cx.notify();
        }
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
        if self.suppress_jump_scroll {
            if scrolled_up && !at_top {
                self.suppress_jump_scroll = false;
            } else {
                return;
            }
        }
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
        let (
            pane,
            can_send,
            title,
            topic,
            glyph,
            typing_text,
            can_manage_guild,
            can_manage_selected,
            dismiss_modal,
            joined_ready,
            joined_channel_ready,
        ) = {
            let state = self.backend.state();
            let joined_ready = self.pending_joined_guild.filter(|id| state.guilds.contains_key(id));
            let joined_channel_ready = self.pending_joined_channel.filter(|id| state.channels.contains_key(id));
            if state.user_settings.raw.is_object() {
                if state.user_settings.raw.get("message_display_compact").is_some() {
                    let server_compact = state.user_settings.message_display_compact();
                    if self.prefs.data.compact != server_compact {
                        self.prefs.set_compact(server_compact);
                    }
                }
                if state.user_settings.raw.get("trusted_domains").is_some() {
                    self.prefs.set_trusted_domains(state.user_settings.trusted_domains());
                }
            }
            if let Place::Guild(id) = self.place
                && !state.guilds.contains_key(&id)
            {
                self.place = Place::Home;
                self.selected_channel = None;
                self.last_channel.remove(&Some(id));
                if self.last_guild == Some(id) {
                    self.last_guild = None;
                }
            }
            let mut fallback_guild_channel: Option<Option<Snowflake>> = None;
            if let Some(removed_id) = self.selected_channel.filter(|id| !state.channels.contains_key(id)) {
                self.drafts.remove(&removed_id);
                self.last_channel.retain(|_, v| *v != removed_id);
                self.selected_channel = None;
                if matches!(self.place, Place::Guild(_)) {
                    let next = channel_items(&state, self.place)
                        .into_iter()
                        .find(|c| c.has_messages)
                        .map(|c| c.id);
                    fallback_guild_channel = Some(next);
                }
            }
            if !state.channels.is_empty() {
                self.drafts.retain(|id, _| state.channels.contains_key(id));
                self.last_channel.retain(|_, v| state.channels.contains_key(v));
            }
            self.attachments.retain_channels(|id| state.channels.contains_key(&id));
            let dismiss_modal = match &self.overlay {
                Overlay::CreateChannel(m) => {
                    !state.guilds.contains_key(&m.guild_id)
                        || match m.parent_id {
                            None => !state.can_manage_guild_channels(m.guild_id),
                            Some(pid) => {
                                !state
                                    .channels
                                    .get(&pid)
                                    .is_some_and(|c| c.guild_id == Some(m.guild_id) && c.kind.is_category())
                                    || !state.can_manage_channel(pid)
                            }
                        }
                }
                Overlay::EditChannel(m) => {
                    !state.guilds.contains_key(&m.guild_id)
                        || !state.channels.contains_key(&m.channel_id)
                        || !state.can_manage_channel(m.channel_id)
                }
                _ => false,
            };
            if dismiss_modal || fallback_guild_channel.is_some() {
                if let Some(next_opt) = fallback_guild_channel {
                    drop(state);
                    if dismiss_modal {
                        self.overlay = Overlay::None;
                        self.focus_composer_later(cx);
                    }
                    match next_opt {
                        Some(id) => self.select_channel(id, cx),
                        None => self.clear_selection(cx),
                    }
                    return;
                }
            }
            if !dismiss_modal && let Overlay::EditChannel(m) = &mut self.overlay {
                m.sync_state(&state);
            }
            self.guilds = guild_items(&state);
            self.home_unread = dm_unread(&state);
            self.mention_total = self
                .guilds
                .iter()
                .fold(self.home_unread.mention_count, |n, g| n.saturating_add(g.unread.mention_count));
            self.user = current_user(&state, self.backend.media());
            self.pending_incoming = friends::pending_incoming_count(&state);
            self.friend_rows = if self.place == Place::Home && self.selected_channel.is_none() {
                friends::build_friend_rows(&state, self.backend.media(), self.friends_tab)
            } else {
                Vec::new()
            };
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
            let include_addable = self.group_dm_panel_open && self.search.is_none() && self.group_dm_adding;
            self.group_dm = match (self.place, self.selected_channel) {
                (Place::Home, Some(id)) => ui::group_dm::build_snapshot(&state, self.backend.media(), id, include_addable),
                _ => None,
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
            let can_manage_guild = match self.place {
                Place::Guild(gid) => state.can_manage_guild_channels(gid),
                Place::Home => false,
            };
            let can_manage_selected = self
                .selected_channel
                .filter(|_| matches!(self.place, Place::Guild(_)))
                .is_some_and(|id| state.can_manage_channel(id));
            match self.selected_channel {
                Some(id) => {
                    let channel = state.channels.get(&id);
                    let topic = channel
                        .and_then(|c| c.topic.as_deref())
                        .map(str::trim)
                        .filter(|t| !t.is_empty())
                        .map(SharedString::from);
                    (
                        message_pane(&state, id, self.backend.media(), self.unread_anchor),
                        state.can_send_messages(id),
                        channel.map(channel_name).unwrap_or_default(),
                        topic,
                        channel.map_or("", |c| ChannelIcon::of(c.kind).glyph()),
                        ui::typing::channel_typing_text(&state, id),
                        can_manage_guild,
                        can_manage_selected,
                        dismiss_modal,
                        joined_ready,
                        joined_channel_ready,
                    )
                }
                None => (
                    Default::default(),
                    false,
                    String::new(),
                    None,
                    "",
                    None,
                    can_manage_guild,
                    false,
                    dismiss_modal,
                    joined_ready,
                    joined_channel_ready,
                ),
            }
        };
        if dismiss_modal {
            self.overlay = Overlay::None;
            self.focus_composer_later(cx);
        }
        if let Some(joined) = joined_ready {
            self.pending_joined_guild = None;
            self.pending_joined_channel = None;
            self.select_place(Place::Guild(joined), cx);
            self.refresh_open_profile();
            return;
        }
        if let Some(ch) = joined_channel_ready {
            self.pending_joined_channel = None;
            self.open_channel(None, ch, cx);
            self.refresh_open_profile();
            return;
        }
        theme::sync_display_global(self.prefs.data.display(), cx);
        self.typing_text = typing_text;
        self._typing_timer = ui::typing::schedule_expiry(&self.backend, cx, Self::refresh);
        self.has_more_before = pane.has_more_before;
        self.has_more_after = pane.has_more_after;
        self.channel_title = title.into();
        self.channel_topic = topic;
        self.channel_glyph = glyph;
        self.can_manage_guild_channels = can_manage_guild;
        self.can_manage_selected_channel = can_manage_selected;
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
        self.refresh_create_group_dm_candidates();
        let next_sync = self.group_dm.as_ref().map(|g| (g.channel_id, g.custom_name.clone()));
        if next_sync != self.group_dm_rename_synced {
            let old_channel = self.group_dm_rename_synced.as_ref().map(|(id, _)| *id);
            let new_channel = next_sync.as_ref().map(|(id, _)| *id);
            let channel_changed = old_channel != new_channel;
            self.group_dm_rename_synced = next_sync;
            if channel_changed {
                self.group_dm_adding = false;
            }
            let text = self
                .group_dm
                .as_ref()
                .and_then(|g| g.custom_name.clone())
                .unwrap_or_default();
            let (entity, handle) = (self.group_dm_rename.clone(), self.window_handle);
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, cx| {
                    if channel_changed || !entity.read(cx).focus_handle(cx).is_focused(window) {
                        entity.update(cx, |s, cx| s.set_value(text, window, cx));
                    }
                });
            });
        }
        self.refresh_open_profile();
        self.subscribe_member_list();
        self.maybe_ack(cx);
        cx.notify();
    }

    /// Whether the member panel is shown (guild channel and toggled on).
    pub(crate) fn shows_member_list(&self) -> bool {
        self.member_list_open && self.member_target.is_some()
    }

    pub(crate) fn toggle_member_list(&mut self, cx: &mut Context<Self>) {
        if self.group_dm.is_some() {
            self.group_dm_panel_open = !self.group_dm_panel_open;
            if !self.group_dm_panel_open {
                self.group_dm_adding = false;
            }
            cx.notify();
            return;
        }
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
        if self.loading_history || self.has_more_after || self.scroller.read(cx).is_scrolled_up() {
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
            self.last_guild = Some(id);
            self.backend.subscribe_guild(id);
        }
        self.refresh(cx);
        let opens = |id: Snowflake| self.channels.iter().any(|c| c.id == id && c.has_messages);
        let remembered = self.last_channel.get(&nav::place_key(place)).copied().filter(|id| opens(*id));
        let first = remembered.or_else(|| self.channels.iter().find(|c| c.has_messages).map(|c| c.id));
        match first {
            Some(id) => self.select_channel(id, cx),
            None => self.clear_selection(cx),
        }
    }

    pub(crate) fn select_channel(&mut self, channel_id: Snowflake, cx: &mut Context<Self>) {
        self.select_channel_inner(channel_id, true, cx);
    }

    pub(crate) fn select_channel_inner(
        &mut self,
        channel_id: Snowflake,
        auto_load_latest: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let opens_pane = self.backend.state().channels.get(&channel_id).is_some_and(|c| opens_message_pane(c.kind));
        if !opens_pane {
            return false;
        }
        self.cancel_ack();
        let old_channel = self.selected_channel;
        self.selected_channel = Some(channel_id);
        nav::record_visit(&mut self.recent_visits, channel_id);
        self.last_channel.insert(nav::place_key(self.place), channel_id);
        if !self.navigating_history {
            self.nav_history.push(nav::HistoryEntry { place: self.place, channel_id });
        }
        self.autocomplete = None;
        self.error = None;
        self.reaction_picker = None;
        self.hovered_message = None;
        // Replies and edits belong to the old channel; stash/restore plain + token drafts.
        if old_channel != Some(channel_id) {
            self.pending_jump = None;
            self.highlighted_message = None;
            let old_draft = self
                .composer_mode
                .channel_changed()
                .unwrap_or_else(|| self.current_composer_content(cx));
            if let Some(old_id) = old_channel {
                self.stash_channel_draft(old_id, old_draft);
            }
            let next_draft = self.drafts.remove(&channel_id).unwrap_or_else(|| InputContent::new(""));
            self.set_composer_later(next_draft, cx);
        } else if let Some(restore) = self.composer_mode.channel_changed() {
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
        self.suppress_jump_scroll = false;
        self.scroller.update(cx, |s, cx| s.reset(0, cx));
        self.loading_history = needs_load && auto_load_latest;
        if needs_load && auto_load_latest {
            self.backend.load_messages(channel_id, None);
        }
        self.refresh(cx);
        true
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
        if let Some(id) = self.selected_channel {
            self.drafts.remove(&id);
        }
        self.typing.message_sent();
        self.error = None;
        self.refresh(cx);
        self.scroller.update(cx, |s, cx| s.scroll_to_end(cx));
    }

    pub(crate) fn not_ready(&mut self, cx: &mut Context<Self>) {
        self.error = Some(l10n::tr("Check your connection and try again.").into());
        cx.notify();
    }

    /// Stores a non-empty draft for `channel_id` (evicting the lowest-ID
    /// entry if the `MAX_DRAFTS` map cap is reached).
    pub(super) fn stash_channel_draft(&mut self, channel_id: Snowflake, content: InputContent) {
        const MAX_DRAFTS: usize = 100;
        if content.text().is_empty() && content.tokens().is_empty() {
            self.drafts.remove(&channel_id);
            return;
        }
        if !self.drafts.contains_key(&channel_id)
            && self.drafts.len() >= MAX_DRAFTS
            && let Some(oldest) = self.drafts.keys().next().copied()
        {
            self.drafts.remove(&oldest);
        }
        self.drafts.insert(channel_id, content);
    }

    pub(super) fn current_composer_content(&mut self, cx: &mut Context<Self>) -> InputContent {
        self.pending_composer.take().unwrap_or_else(|| self.composer.read(cx).content())
    }

    /// Replaces the input (text and tokens). `set_value` emits no change
    /// event, so the popup computed for the old text is dropped here: its
    /// byte range would otherwise be applied to the new text.
    fn set_composer(&mut self, content: impl Into<InputContent>, window: &mut Window, cx: &mut Context<Self>) {
        self.autocomplete = None;
        self.composer_seq.set(self.composer_seq.get().wrapping_add(1));
        self.pending_composer = None;
        let content = content.into();
        self.composer.update(cx, |s, cx| s.set_value(content, window, cx));
    }

    /// [`Self::set_composer`] from a call without the window (channel
    /// switch): runs after the current update.
    fn set_composer_later(&mut self, content: InputContent, cx: &mut Context<Self>) {
        self.autocomplete = None;
        let seq = self.composer_seq.get().wrapping_add(1);
        self.composer_seq.set(seq);
        self.pending_composer = Some(content.clone());
        let (composer_seq, view, composer, handle) =
            (self.composer_seq.clone(), cx.weak_entity(), self.composer.clone(), self.window_handle);
        cx.defer(move |cx| {
            if composer_seq.get() != seq {
                return;
            }
            let _ = view.update(cx, |v, _| v.pending_composer = None);
            let _ = handle.update(cx, |_, window, cx| composer.update(cx, |s, cx| s.set_value(content, window, cx)));
        });
    }

    /// IME composition in progress: Enter / Tab / arrows / Esc belong to
    /// the IME (`ComposerIME.ts`), not to the popup or the composer modes.
    fn composing(&self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.composer.update(cx, |s, cx| s.marked_text_range(window, cx)).is_some()
    }

    pub(crate) fn focus_composer(&self, window: &mut Window, cx: &mut Context<Self>) {
        let focus = self.composer.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
    }

    /// The composer text changed: the typing signal.
    fn composer_changed(&mut self, cx: &mut Context<Self>) {
        self.pending_composer = None;
        self.composer_seq.set(self.composer_seq.get().wrapping_add(1));
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
        if self.search.is_some() {
            self.close_search(window, cx);
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
        let current = self.current_composer_content(cx);
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

    /// Opens `channel_id` (and switches guild/Home when needed), scrolls to
    /// `message_id` and highlights it; if outside the current cached window,
    /// loads a `?around=` slice without firing a redundant latest-page fetch.
    pub(crate) fn jump_to_channel_message(
        &mut self,
        guild_id: Option<Snowflake>,
        channel_id: Snowflake,
        message_id: Snowflake,
        cx: &mut Context<Self>,
    ) {
        let (opens, resolved_guild, already_loaded) = {
            let state = self.backend.state();
            let Some(ch) = state.channels.get(&channel_id) else { return };
            let opens = opens_message_pane(ch.kind);
            let resolved_guild = guild_id.or(ch.guild_id);
            let already_loaded = state
                .messages
                .window(channel_id)
                .is_some_and(|w| w.ready && w.messages.iter().any(|m| m.id == message_id));
            (opens, resolved_guild, already_loaded)
        };
        if !opens {
            return;
        }
        let target_place = resolved_guild.map_or(Place::Home, Place::Guild);
        if self.place != target_place {
            self.place = target_place;
            if let Some(gid) = resolved_guild {
                self.last_guild = Some(gid);
                self.backend.subscribe_guild(gid);
            }
        }
        if self.selected_channel != Some(channel_id) {
            if !self.select_channel_inner(channel_id, false, cx) {
                return;
            }
        } else {
            self.refresh(cx);
        }
        if already_loaded {
            self.pending_jump = None;
            if let Some(ix) = self.rows.borrow().iter().position(|r| r.id == message_id) {
                self.suppress_jump_scroll = true;
                self.scroller.update(cx, |s, cx| {
                    s.scroll_to_item(ix, cx);
                });
                self.highlighted_message = Some((channel_id, message_id));
            }
        } else {
            self.pending_jump = Some((channel_id, message_id));
            self.highlighted_message = None;
            self.loading_history = true;
            self.backend.load_around(channel_id, message_id);
        }
        cx.notify();
    }

    /// Opens the message's channel and scrolls to it when it is loaded
    /// (reply lines, the "Replying to" bar).
    pub(crate) fn jump_to_message(&mut self, channel_id: Snowflake, message_id: Snowflake, cx: &mut Context<Self>) {
        self.jump_to_channel_message(None, channel_id, message_id, cx);
    }

    /// Routes a validated deep link inside the window, deferring workspace links until `READY`.
    pub(crate) fn open_deep_link(
        &mut self,
        link: crate::deep_links::DeepLink,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(link, crate::deep_links::DeepLink::Settings) {
            let _ = self.try_apply_deep_link(&link, true, window, cx);
            return;
        }
        if self.user.is_some() {
            let _ = self.try_apply_deep_link(&link, true, window, cx);
        } else {
            self.pending_deep_link = Some((link, std::time::Instant::now()));
        }
    }

    /// Tries to navigate to `link`. Returns `true` when resolved (or failed with
    /// `show_missing_error`), or `false` when still waiting for deferred guild/channel data.
    pub(crate) fn try_apply_deep_link(
        &mut self,
        link: &crate::deep_links::DeepLink,
        show_missing_error: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        use crate::deep_links::DeepLink;

        self.error = None;
        match link {
            DeepLink::Home => {
                self.close_overlay(window, cx);
                self.place = Place::Home;
                self.clear_selection(cx);
                true
            }
            DeepLink::DmChannel { channel_id, message_id } => {
                let valid = self
                    .backend
                    .state()
                    .channels
                    .get(channel_id)
                    .is_some_and(|ch| ch.guild_id.is_none() && opens_message_pane(ch.kind));
                if !valid {
                    if show_missing_error {
                        self.error = Some(l10n::tr("Unknown channel").into());
                        cx.notify();
                    }
                    return show_missing_error;
                }
                self.close_overlay(window, cx);
                if let Some(msg) = *message_id {
                    self.jump_to_channel_message(None, *channel_id, msg, cx);
                } else {
                    self.open_channel(None, *channel_id, cx);
                }
                true
            }
            DeepLink::Guild { guild_id } => {
                if !self.backend.state().guilds.contains_key(guild_id) {
                    if show_missing_error {
                        self.error = Some(l10n::tr("Not found").into());
                        cx.notify();
                    }
                    return show_missing_error;
                }
                self.close_overlay(window, cx);
                self.select_place(Place::Guild(*guild_id), cx);
                true
            }
            DeepLink::GuildChannel { guild_id, channel_id, message_id } => {
                let valid = self
                    .backend
                    .state()
                    .channels
                    .get(channel_id)
                    .is_some_and(|ch| ch.guild_id == Some(*guild_id) && opens_message_pane(ch.kind));
                if !valid {
                    if show_missing_error {
                        self.error = Some(l10n::tr("Unknown channel").into());
                        cx.notify();
                    }
                    return show_missing_error;
                }
                self.close_overlay(window, cx);
                if let Some(msg) = *message_id {
                    self.jump_to_channel_message(Some(*guild_id), *channel_id, msg, cx);
                } else {
                    self.open_channel(Some(*guild_id), *channel_id, cx);
                }
                true
            }
            DeepLink::Invite { code } => {
                self.open_join_guild_with_code(code, window, cx);
                true
            }
            DeepLink::UserProfile { user_id } => {
                self.open_profile(
                    *user_id,
                    self.current_guild_id(),
                    gpui_kit::point(px(240.), px(120.)),
                    window,
                    cx,
                );
                true
            }
            DeepLink::Settings => {
                if !matches!(self.overlay, Overlay::Settings(_)) {
                    self.open_settings(window, cx);
                }
                true
            }
        }
    }

    pub(crate) fn next_search_seq(&mut self) -> u64 {
        self.search_seq = self.search_seq.saturating_add(1);
        self.search_seq
    }

    pub(crate) fn next_group_dm_seq(&mut self) -> u64 {
        self.create_group_dm_seq = self.create_group_dm_seq.saturating_add(1);
        self.create_group_dm_seq
    }

    pub(crate) fn next_channel_mgmt_seq(&mut self) -> u64 {
        self.channel_mgmt_seq = self.channel_mgmt_seq.saturating_add(1);
        self.channel_mgmt_seq
    }

    /// Leaves a historical `?around=` jump window and returns to the latest messages.
    pub(crate) fn jump_to_present(&mut self, cx: &mut Context<Self>) {
        if self.loading_history {
            return;
        }
        let Some(channel_id) = self.selected_channel else { return };
        self.pending_jump = None;
        self.highlighted_message = None;
        self.loading_history = true;
        self.rows.borrow_mut().clear();
        self.suppress_jump_scroll = false;
        self.scroller.update(cx, |s, cx| s.reset(0, cx));
        self.backend.load_messages(channel_id, None);
        cx.notify();
    }

    fn render_jump_to_present(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let label = if self.loading_history { l10n::tr("Loading…") } else { l10n::tr("Jump to present") };
        div()
            .id("jump-to-present-bar")
            .debug_selector(|| "jump-to-present".to_owned())
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .px_4()
            .py_1()
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.background_secondary)
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(l10n::tr("Viewing older messages")),
            )
            .child(
                div().debug_selector(|| "jump-to-present-btn".to_owned()).child(
                    Button::new("jump-to-present-btn")
                        .primary()
                        .small()
                        .label(label)
                        .on_click(cx.listener(|v, _, _, cx| v.jump_to_present(cx))),
                ),
            )
    }

    pub(crate) fn set_saved_accounts(
        &mut self,
        enabled: bool,
        accounts: Vec<crate::accounts::SavedAccountItem>,
        cx: &mut Context<Self>,
    ) {
        self.accounts_enabled = enabled;
        self.saved_accounts = accounts;
        cx.notify();
    }

    /// Closes this session's timers and gateway without revoking the token on the server.
    pub(crate) fn close_session(&mut self) {
        self.cancel_ack();
        self._typing_timer = None;
        self.backend.close();
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
        let right_panel = if self.search.is_some() {
            Some(ui::search_panel::render(self, &theme, cx))
        } else if self.shows_member_list() {
            Some(ui::member_list::render(self, &theme, cx))
        } else if self.group_dm_panel_open && self.group_dm.is_some() {
            Some(ui::group_dm::render_panel(self, &theme, cx))
        } else {
            None
        };
        let handle = cx.weak_entity();
        let composer = self.render_composer(&theme, cx);
        let show_friends = self.place == Place::Home && self.selected_channel.is_none();
        let overlay = match &self.overlay {
            Overlay::QuickSwitcher(sw) => Some(ui::quick_switcher::render(sw, &theme, cx)),
            Overlay::EmojiPicker(picker) => {
                Some(ui::emoji_picker::render(picker, self.backend.media(), &theme, cx))
            }
            Overlay::Profile(pop) => Some(ui::profile_popout::render(pop, &theme, cx)),
            Overlay::Settings(modal) => Some(ui::settings::render(self, modal, &theme, cx)),
            Overlay::AccountSwitcher(modal) => Some(ui::account_switcher::render(self, modal, &theme, cx)),
            Overlay::JoinGuild(modal) => Some(ui::guild_modal::render_join(modal, &theme, cx)),
            Overlay::LeaveGuild(modal) => Some(ui::guild_modal::render_leave(modal, &theme, cx)),
            Overlay::CreateGroupDm(modal) => {
                let query = modal.filter_input.read(cx).value().to_string();
                Some(ui::group_dm::render_create_modal(modal, &theme, cx, &query))
            }
            Overlay::LeaveGroupDm(modal) => Some(ui::group_dm::render_leave_modal(modal, &theme, cx)),
            Overlay::MediaViewer(modal) => Some(ui::media_viewer::render(modal, &theme, cx)),
            Overlay::LinkWarning(modal) => Some(ui::link_warning::render(modal, &theme, cx)),
            Overlay::CreateChannel(modal) => Some(ui::channel_modal::render_create(modal, &theme, cx)),
            Overlay::EditChannel(modal) => Some(ui::channel_modal::render_edit(modal, &theme, cx)),
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
            .child(if show_friends {
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(ui::friends::render(self, &theme, cx))
                    // Keep the composer mounted in a 0px hidden slot so the
                    // `ClientView > Composer > Input` shortcut context stays live.
                    .child(div().h(px(0.)).overflow_hidden().child(composer))
            } else {
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
                    .when(self.has_more_after && self.selected_channel.is_some(), |d| {
                        d.child(self.render_jump_to_present(&theme, cx))
                    })
                    .child(composer)
            })
            .children(right_panel)
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
