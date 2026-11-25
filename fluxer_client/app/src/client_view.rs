//! Main layout after login: owns the shared view state (selection,
//! snapshots, scroller, composer) and the actions; the pieces are rendered
//! by `crate::ui`.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::sync::Arc;

use fluxer_client_core::rest::{DiscoveryGuild, DiscoveryGuildQuery, UserProfileFull};
use fluxer_client_core::state::{MessageState, PartialEmoji, Snowflake, Status, Unread};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{InputContent, InputEvent, InputState};
use gpui_kit::component::message_scroller::MessageScrollerState;
use gpui_kit::{
    AnyElement, AnyWindowHandle, Context, Entity, EntityInputHandler as _, EventEmitter, ExternalPaths, FocusHandle,
    Focusable as _, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, RenderImage, ScrollHandle,
    SharedString, Subscription, Task, UniformListScrollHandle, Window, div, prelude::*, px, rgb,
};
use tokio::sync::mpsc;

use crate::parity_probe::FlxExt as _;
use crate::ack::AckScheduler;
use crate::backend::{Backend, GatewayStatus, TypingSender, UiEvent};
use crate::frecency::FrecencyStore;
use crate::nav;
use crate::notify::Notifier;
use crate::prefs::{
    EMOJI_PICKER_HEIGHT_DEFAULT, EMOJI_PICKER_HEIGHT_MAX, EMOJI_PICKER_HEIGHT_MIN, EMOJI_PICKER_WIDTH_DEFAULT,
    EMOJI_PICKER_WIDTH_MAX, EMOJI_PICKER_WIDTH_MIN, PrefsStore, SIDEBAR_WIDTH_DEFAULT, SIDEBAR_WIDTH_MAX,
    SIDEBAR_WIDTH_MIN, VOICE_CALL_HEIGHT_DEFAULT, VOICE_CALL_HEIGHT_MAX, VOICE_CALL_HEIGHT_MIN,
    snap_emoji_picker_width,
};
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
    ChannelIcon, ChannelItem, GuildItem, MessageRow, Place, UserSummary, channel_items_with_active, channel_name,
    current_user,
    dm_unread, guild_items_with_media, message_pane, opens_message_pane,
};

/// Gateway connection as shown by the user panel's status dot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Connection {
    Connecting,
    Online,
    Offline,
}

/// Which inline voice popout (`VoiceControlBar.tsx` / `VoiceConnectionStatusInner.tsx`) is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VoicePopoutKind {
    Input,
    Output,
    Stream,
    Details,
}

/// Which left Guild Rail item is currently hovered (`GuildListIndicator.ts` / `GuildsLayout.module.css`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RailHoverTarget {
    Home,
    Favorites,
    Guild(Snowflake),
    Discover,
    Join,
}

/// Which channel header right-section icon button is currently hovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HeaderHoverTarget {
    Mute,
    VoiceCall,
    VideoCall,
    Threads,
    Pins,
    Favorite,
    Members,
    Inbox,
    Update,
}

/// Which bottom-left user-area control button is currently hovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UserControlHoverTarget {
    Mute,
    Deafen,
    Settings,
}

/// Drag edge for bottom-right anchored resizable popouts such as the emoji picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResizeEdge {
    Top,
    Left,
    TopLeft,
}

/// Active interactive in-app layout resize operation.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ActiveResize {
    Sidebar { start_x: f32, start_width: f32 },
    VoiceCallHeight { start_y: f32, start_height: f32 },
    EmojiPicker { edge: ResizeEdge, start_x: f32, start_y: f32, start_w: f32, start_h: f32 },
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
    /// Current user holds guild-level `BAN_MEMBERS` in the active guild.
    pub(crate) can_ban_members: bool,
    /// Current user holds channel-level `MANAGE_CHANNELS` on `selected_channel`.
    pub(crate) can_manage_selected_channel: bool,
    /// Last title given to the OS window.
    pub(crate) window_title: String,
    acks: AckScheduler,
    _ack_timer: Option<Task<()>>,
    /// Rows shown by the scroller; shared with its row renderer.
    pub(crate) rows: Rc<RefCell<Vec<MessageRow>>>,
    /// Contiguous ranges in [`Self::rows`] rendered as `channel.message-group.group`
    /// items by [`Self::scroller`].
    pub(crate) row_groups: Rc<RefCell<Vec<std::ops::Range<usize>>>>,
    has_more_before: bool,
    /// Newer messages were trimmed from the window (pages back towards the
    /// newest load when the list reaches its bottom).
    pub(crate) has_more_after: bool,
    /// `ClientState::unread_anchor` of the selected channel when it was
    /// opened; places the "NEW" divider until another channel is opened.
    pub(crate) unread_anchor: Option<Snowflake>,
    pub(crate) loading_history: bool,
    pub(crate) can_send: bool,
    pub(crate) guild_unavailable: bool,
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
    /// Live search filter input for Home Friends tabs.
    pub(crate) friends_search_input: Entity<InputState>,
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
    /// Historical jump waiting for its `?around=` page.
    pub(crate) pending_jump: Option<(Snowflake, unread::JumpTarget)>,
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
    pub(crate) access_pending: Option<u64>,
    /// Sequence counter for guild member moderation / ban list responses.
    moderation_seq: u64,
    /// Sequence counter for message forward requests.
    forward_seq: u64,
    /// Sequence counter for thread creation from a message.
    thread_seq: u64,
    /// Sequence counter for guild invite creation requests.
    invite_seq: u64,
    /// Sequence counter for bulk-delete-my-messages requests.
    guild_delete_seq: u64,
    /// Sequence counter for purging personal notes messages.
    purge_notes_seq: u64,
    /// Explore communities directory state and inputs.
    pub(crate) explore: ui::explore::ExploreState,
    /// Text-to-speech playback state for "Speak message".
    pub(crate) tts: crate::tts::TtsPlayer,
    /// Channel explicitly marked unread by the user; suppresses auto-ack until cleared.
    pub(crate) manual_unread_channel: Option<Snowflake>,
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
    /// UI-safe snapshot of the active voice session and channel voice members.
    pub(crate) voice: crate::backend::voice::VoiceSnapshot,
    /// When true and voice is active, the central pane renders `ui::voice_stage`.
    pub(crate) voice_stage_open: bool,
    /// When true, `ui::voice_stage` fills the central pane; when false, renders top call stage + bottom text chat split view.
    pub(crate) voice_stage_expanded: bool,
    /// When true, the dedicated bottom-left `UserAreaPopout` menu (`UserArea.tsx`) is open above the user row.
    pub(crate) user_area_popout_open: bool,
    /// Expression picker tab closed on the current `MouseDown` outside click so repeat clicks toggle instead of reopening.
    pub(crate) dismissed_expression_tab: Option<(crate::ui::emoji_picker::sections::PickerTarget, crate::ui::emoji_picker::ExpressionTab)>,
    /// Header/composer popout key closed on the current `MouseDown` outside click so repeat clicks toggle cleanly.
    pub(crate) dismissed_channel_popout: Option<&'static str>,
    pub(crate) pins_seq: u64,
    /// Hovered `UserAreaPopout` submenu (status / expiry / accounts).
    pub(crate) user_area_submenu: Option<crate::ui::user_panel::UserAreaSubmenu>,
    /// When true, the Guild Header dropdown menu (`GuildHeaderPopout.tsx`) is open below the guild header.
    pub(crate) guild_header_popout_open: bool,
    /// Currently hovered item in the left Guild Rail (`GuildListIndicator.ts`).
    pub(crate) hovered_rail_item: Option<RailHoverTarget>,
    /// Currently hovered icon button in the channel header.
    pub(crate) hovered_header_item: Option<HeaderHoverTarget>,
    /// Currently hovered control button in the bottom-left user area.
    pub(crate) hovered_user_control: Option<UserControlHoverTarget>,
    /// True when the `Favorites` virtual rail button (`FavoritesGuildButton`) is selected.
    pub(crate) favorites_selected: bool,
    /// True while the user is running the interactive Microphone Test monitor in Settings -> Voice & Video.
    pub(crate) mic_test_active: bool,
    /// Active inline voice settings/details popout (`VoiceControlBar.tsx` / `VoiceConnectionStatusInner.tsx`).
    pub(crate) voice_popout: Option<VoicePopoutKind>,
    /// Enumerated OS audio input/output devices for the settings modal.
    pub(crate) audio_devices: crate::audio_devices::AudioDeviceLists,
    /// Track SID currently synced into `watched_video_frame`.
    pub(crate) watched_video_track_sid: Option<String>,
    /// Generation sequence of the last decoded frame converted for display.
    pub(crate) watched_video_frame_seq: u64,
    /// Latest decoded remote video frame `(width, height, RenderImage)`.
    pub(crate) watched_video_frame: Option<(u32, u32, Arc<RenderImage>)>,
    /// Deep link deferred until READY completes (`self.user.is_some()`) and target arrives.
    pub(crate) pending_deep_link: Option<(crate::deep_links::DeepLink, std::time::Instant)>,
    /// Latest update check state for Settings -> About & Updates.
    pub(crate) update_status: crate::updater::UpdateStatus,
    pub(crate) _update_task: Option<Task<()>>,
    /// Persisted interactive layout sizes and active resize drag state.
    pub(crate) sidebar_width: f32,
    pub(crate) voice_call_height: f32,
    pub(crate) emoji_picker_width: f32,
    pub(crate) emoji_picker_height: f32,
    pub(crate) active_resize: Option<ActiveResize>,
    pub(crate) sidebar_resize_hovered: bool,
    pub(crate) voice_resize_hovered: bool,
    pub(crate) sidebar_resize_focus: FocusHandle,
    pub(crate) voice_resize_focus: FocusHandle,
    pub(crate) emoji_resize_focus: FocusHandle,
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
        let friends_search_input = cx.new(|cx| InputState::new(window, cx).placeholder(l10n::tr("Search")));
        let explore_search_input = cx.new(|cx| InputState::new(window, cx).placeholder(l10n::tr("Search communities")));
        let group_dm_rename = cx.new(|cx| InputState::new(window, cx).placeholder(l10n::tr("Group name")));
        let subscriptions = vec![
            cx.subscribe_in(&add_friend_input, window, |this: &mut Self, _, ev: &InputEvent, _, cx| {
                if let InputEvent::PressEnter { shift: false, .. } = ev {
                    this.submit_add_friend(cx);
                }
            }),
            cx.subscribe_in(&friends_search_input, window, |_, _, ev: &InputEvent, _, cx| {
                if matches!(ev, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe_in(&explore_search_input, window, |this: &mut Self, _, ev: &InputEvent, _, cx| match ev {
                InputEvent::PressEnter { shift: false, .. } => this.submit_explore_search(cx),
                InputEvent::Change => cx.notify(),
                _ => {}
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
        let weak_for_keys = cx.weak_entity();
        let target_window = window.window_handle();
        let mut subscriptions = subscriptions;
        subscriptions.push(cx.intercept_keystrokes(move |event, window, cx| {
            if window.window_handle() != target_window {
                return;
            }
            let handled = weak_for_keys
                .update(cx, |this, cx| this.intercept_keystroke(&event.keystroke, window, cx))
                .unwrap_or(false);
            if handled {
                cx.stop_propagation();
            }
        }));
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
            can_ban_members: false,
            can_manage_selected_channel: false,
            window_title: String::new(),
            acks: AckScheduler::default(),
            _ack_timer: None,
            rows: Rc::new(RefCell::new(Vec::new())),
            row_groups: Rc::new(RefCell::new(Vec::new())),
            has_more_before: false,
            has_more_after: false,
            unread_anchor: None,
            loading_history: false,
            can_send: false,
            guild_unavailable: false,
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
            friends_search_input,
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
            access_pending: None,
            moderation_seq: 0,
            forward_seq: 0,
            thread_seq: 0,
            invite_seq: 0,
            guild_delete_seq: 0,
            purge_notes_seq: 0,
            explore: ui::explore::ExploreState::new(explore_search_input),
            tts: crate::tts::TtsPlayer::new(),
            manual_unread_channel: None,
            group_dm: None,
            group_dm_rename,
            group_dm_adding: false,
            group_dm_rename_synced: None,
            accounts_enabled: false,
            saved_accounts: Vec::new(),
            voice: crate::backend::voice::VoiceSnapshot::default(),
            voice_stage_open: false,
            voice_stage_expanded: true,
            user_area_popout_open: false,
            dismissed_expression_tab: None,
            dismissed_channel_popout: None,
            pins_seq: 0,
            user_area_submenu: None,
            guild_header_popout_open: false,
            hovered_rail_item: None,
            hovered_header_item: None,
            hovered_user_control: None,
            favorites_selected: false,
            mic_test_active: false,
            voice_popout: None,
            audio_devices: crate::audio_devices::AudioDeviceLists::default(),
            watched_video_track_sid: None,
            watched_video_frame_seq: 0,
            watched_video_frame: None,
            pending_deep_link: None,
            update_status: crate::updater::UpdateStatus::Idle,
            _update_task: None,
            sidebar_width: SIDEBAR_WIDTH_DEFAULT as f32,
            voice_call_height: VOICE_CALL_HEIGHT_DEFAULT as f32,
            emoji_picker_width: EMOJI_PICKER_WIDTH_DEFAULT as f32,
            emoji_picker_height: EMOJI_PICKER_HEIGHT_DEFAULT as f32,
            active_resize: None,
            sidebar_resize_hovered: false,
            voice_resize_hovered: false,
            sidebar_resize_focus: cx.focus_handle(),
            voice_resize_focus: cx.focus_handle(),
            emoji_resize_focus: cx.focus_handle(),
            focus: cx.focus_handle(),
            #[cfg(test)]
            opened_urls: Vec::new(),
            _subscriptions: subscriptions,
            _events: events_task,
        };
        view.refresh(cx);
        view
    }

    pub(crate) fn refresh_audio_devices(&mut self, cx: &mut Context<Self>) {
        self.audio_devices = crate::audio_devices::enumerate_audio_devices();
        cx.notify();
    }

    /// Applies device-local preferences loaded from the session directory.
    pub(crate) fn apply_prefs(&mut self, prefs: PrefsStore, window: &mut Window, cx: &mut Context<Self>) {
        self.notifier.enabled = prefs.data.notifications;
        self.notifier.sounds_enabled = prefs.data.sounds;
        crate::tray::set_close_to_tray(prefs.data.close_to_tray);
        cx.set_reduce_motion(crate::platform::resolve_reduced_motion(prefs.data.reduced_motion));
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
        self.sidebar_width = self.prefs.data.sidebar_width as f32;
        self.voice_call_height = self.prefs.data.voice_call_height as f32;
        self.emoji_picker_width = self.prefs.data.emoji_picker_width as f32;
        // 686 was the previous default; the official default is 690.
        self.emoji_picker_height = match self.prefs.data.emoji_picker_height {
            686 => EMOJI_PICKER_HEIGHT_DEFAULT as f32,
            h => h as f32,
        };
        if let crate::ui::overlay::Overlay::EmojiPicker(picker) = &mut self.overlay {
            picker.set_width(self.emoji_picker_width);
        }
        self.refresh(cx);
    }

    pub(crate) fn current_guild_id(&self) -> Option<Snowflake> {
        match self.place {
            Place::Guild(id) => Some(id),
            Place::Home | Place::Explore => None,
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
                | Overlay::ChannelPopout(_)
                | Overlay::AccountSwitcher(_)
                | Overlay::JoinGuild(_)
                | Overlay::LeaveGuild(_)
                | Overlay::CreateGroupDm(_)
                | Overlay::LeaveGroupDm(_)
                | Overlay::MediaViewer(_)
                | Overlay::LinkWarning(_)
                | Overlay::CreateChannel(_)
                | Overlay::EditChannel(_)
                | Overlay::MemberModeration(_)
                | Overlay::GuildBans(_)
                | Overlay::Access(_)
                | Overlay::ForwardMessage(_)
                | Overlay::CreateThread(_)
                | Overlay::InviteMembers(_)
                | Overlay::GuildNotifications(_)
                | Overlay::GuildPrivacy(_)
                | Overlay::GuildProfile(_)
                | Overlay::DeleteMyGuildMessages(_)
                | Overlay::PurgePersonalNotes(_)
                | Overlay::DiscoveryPreview(_)
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
        let mut guild_available = false;
        for event in batch {
            match event {
                UiEvent::StateChanged => {}
                UiEvent::VoiceChanged => {
                    self.voice = self.backend.voice_snapshot();
                    self.sync_watched_video_frame();
                }
                UiEvent::GuildAvailable { guild_id } => {
                    guild_available |= self.place == Place::Guild(guild_id);
                }
                UiEvent::Gateway(status) => {
                    // READY/RESUMED: subscriptions are sent again (with
                    // `sync`) by the refresh below.
                    if status == GatewayStatus::Ready {
                        self.member_subscriptions.reset_session();
                        connected = true;
                        if !self.backend.state().saved_messages_loaded {
                            self.backend.load_saved_messages();
                        }
                    }
                    self.connection = match status {
                        GatewayStatus::Ready => Connection::Online,
                        GatewayStatus::Connecting | GatewayStatus::Reconnecting { .. } => Connection::Connecting,
                        GatewayStatus::Closed { .. } => Connection::Offline,
                    };
                    self.gateway_status = status;
                    self.refresh_status_text();
                }
                UiEvent::HistoryLoaded { channel_id, stamp, error } => {
                    // A page of a channel left meanwhile neither ends this
                    // channel's load nor shows its error here.
                    if Some(channel_id) == self.selected_channel && self.backend.history_stamp(channel_id) == stamp {
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
                UiEvent::UploadEnded { nonce, error } => self.upload_ended(nonce, error),
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
                        if self.channel_mgmt_seq == seq && let Some(error) = error {
                            self.error = Some(error.into());
                        }
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
                UiEvent::AccessChanged { seq, error } => self.finish_access_change(seq, error, cx),
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
                                    channel_items_with_active(
                                        &state,
                                        self.place,
                                        self.backend.media(),
                                        self.selected_channel,
                                        self.voice.channel_id,
                                    )
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
                UiEvent::MemberModerated { seq, guild_id, user_id, error } => {
                    if matches!(&self.overlay, Overlay::MemberModeration(m) if m.seq == seq) {
                        match error {
                            None => {
                                self.profile_cache.remove(&(user_id, Some(guild_id)));
                                self.overlay = Overlay::None;
                                self.focus_composer_later(cx);
                            }
                            Some(err) => {
                                if let Overlay::MemberModeration(modal) = &mut self.overlay {
                                    modal.submitting = false;
                                    modal.error = Some(err);
                                }
                            }
                        }
                    } else if matches!(&self.overlay, Overlay::GuildProfile(m) if m.seq == seq) {
                        match error {
                            None => {
                                self.profile_cache.remove(&(user_id, Some(guild_id)));
                                self.overlay = Overlay::None;
                                self.focus_composer_later(cx);
                            }
                            Some(err) => {
                                if let Overlay::GuildProfile(modal) = &mut self.overlay {
                                    modal.submitting = false;
                                    modal.error = Some(err);
                                }
                            }
                        }
                    } else if seq == self.moderation_seq && matches!(self.overlay, Overlay::None) {
                        if let Some(err) = error {
                            self.error = Some(err.into());
                        } else {
                            self.profile_cache.remove(&(user_id, Some(guild_id)));
                        }
                    }
                }
                UiEvent::GuildBansLoaded { seq, guild_id, result } => {
                    let Overlay::GuildBans(modal) = &mut self.overlay else {
                        continue;
                    };
                    if modal.seq != seq || modal.guild_id != guild_id {
                        continue;
                    }
                    modal.loading = false;
                    match result {
                        Ok(mut bans) => {
                            bans.retain(|b| !modal.recently_unbanned.get(&b.user.id).is_some_and(|&u_seq| u_seq > seq));
                            modal.recently_unbanned.retain(|_, &mut u_seq| u_seq > seq);
                            modal.bans = bans;
                            modal.error = None;
                        }
                        Err(err) => {
                            modal.error = Some(err);
                        }
                    }
                }
                UiEvent::MemberUnbanned { seq, guild_id, user_id, error } => {
                    let Overlay::GuildBans(modal) = &mut self.overlay else {
                        continue;
                    };
                    if modal.guild_id != guild_id || modal.unban_seqs.remove(&user_id) != Some(seq) {
                        continue;
                    }
                    modal.unbanning.remove(&user_id);
                    match error {
                        None => {
                            modal.bans.retain(|b| b.user.id != user_id);
                            modal.recently_unbanned.insert(user_id, seq);
                            modal.error = None;
                        }
                        Some(err) => {
                            modal.error = Some(err);
                        }
                    }
                }
                UiEvent::WebAuthnCredentialsLoaded { result } => {
                    if let Overlay::Settings(modal) = &mut self.overlay {
                        modal.passkey_loading = false;
                        match result {
                            Ok(creds) => {
                                modal.passkeys = creds;
                                modal.passkey_error = None;
                            }
                            Err(err) => {
                                modal.passkey_error = Some(err.into());
                            }
                        }
                    }
                }
                UiEvent::WebAuthnMutationFinished { status, error } => {
                    if let Overlay::Settings(modal) = &mut self.overlay {
                        modal.passkey_busy = false;
                        modal.passkey_status = status.map(Into::into);
                        modal.passkey_error = error.clone().map(Into::into);
                        if error.is_none() {
                            modal.passkey_loading = true;
                            self.backend.load_webauthn_credentials();
                        }
                    }
                }
                UiEvent::ConnectionsLoaded { result } => {
                    if let Overlay::Settings(modal) = &mut self.overlay {
                        modal.connections_loading = false;
                        modal.connections_loaded = true;
                        match result {
                            Ok(mut items) => {
                                items.sort_by_key(|c| c.sort_order);
                                modal.connections = items;
                                modal.connections_error = None;
                            }
                            Err(err) => {
                                modal.connections_error = Some(err.into());
                            }
                        }
                    }
                }
                UiEvent::ConnectionUpdated { result } => {
                    if let Overlay::Settings(modal) = &mut self.overlay {
                        match result {
                            Ok(updated) => {
                                if let Some(slot) = modal.connections.iter_mut().find(|c| c.id == updated.id) {
                                    *slot = updated;
                                } else {
                                    modal.connections.push(updated);
                                }
                                modal.connections.sort_by_key(|c| c.sort_order);
                                modal.connections_error = None;
                            }
                            Err(err) => {
                                modal.connections_error = Some(err.into());
                            }
                        }
                    }
                }
                UiEvent::ApplicationsLoaded { result } => {
                    if let Overlay::Settings(modal) = &mut self.overlay {
                        modal.applications_loading = false;
                        modal.applications_loaded = true;
                        match result {
                            Ok(items) => {
                                modal.applications = items;
                                modal.applications_error = None;
                            }
                            Err(err) => {
                                modal.applications_error = Some(err.into());
                            }
                        }
                    }
                }
                UiEvent::ApplicationLoaded { app_id, result } => {
                    if let Overlay::Settings(modal) = &mut self.overlay
                        && modal.selected_application_id.as_deref() == Some(app_id.as_str())
                    {
                        modal.selected_application_loading = false;
                        match result {
                            Ok(app) => {
                                if let Some(slot) = modal.applications.iter_mut().find(|a| a.id == app.id) {
                                    *slot = app;
                                } else {
                                    modal.applications.push(app);
                                }
                                modal.applications_error = None;
                            }
                            Err(err) => {
                                modal.applications_error = Some(err.into());
                            }
                        }
                    }
                }
                UiEvent::PinsLoaded { seq, channel_id, result } => {
                    if let Overlay::ChannelPopout(pop) = &mut self.overlay
                        && let crate::ui::channel_popouts::ChannelPopoutKind::PinnedMessages {
                            channel_id: pop_ch,
                            seq: pop_seq,
                            loading,
                            items,
                            error: pop_err,
                        } = &mut pop.kind
                        && *pop_seq == seq
                        && *pop_ch == channel_id
                    {
                        *loading = false;
                        match result {
                            Ok(msgs) => {
                                *items = msgs;
                                *pop_err = None;
                            }
                            Err(err) => {
                                *pop_err = Some(err);
                            }
                        }
                    }
                }
                UiEvent::SavedMessagesLoaded { error } => {
                    if let Some(err) = error
                        && matches!(
                            &self.overlay,
                            Overlay::ChannelPopout(pop)
                                if matches!(pop.kind, crate::ui::channel_popouts::ChannelPopoutKind::Inbox { tab: crate::ui::channel_popouts::InboxTab::Bookmarks })
                        )
                    {
                        self.error = Some(err.into());
                    }
                }
                UiEvent::MessageForwarded { seq, target_channels: _, error } => {
                    if matches!(&self.overlay, Overlay::ForwardMessage(m) if m.seq == seq) {
                        match error {
                            None => {
                                self.overlay = Overlay::None;
                                self.focus_composer_later(cx);
                            }
                            Some(err) => {
                                if let Overlay::ForwardMessage(modal) = &mut self.overlay {
                                    modal.submitting = false;
                                    modal.error = Some(err);
                                }
                            }
                        }
                    }
                }
                UiEvent::ThreadCreated { seq, result } => {
                    if matches!(&self.overlay, Overlay::CreateThread(m) if m.seq == seq) {
                        match result {
                            Ok(thread_id) => {
                                self.overlay = Overlay::None;
                                self.focus_composer_later(cx);
                                self.select_channel(thread_id, cx);
                            }
                            Err(err) => {
                                if let Overlay::CreateThread(modal) = &mut self.overlay {
                                    modal.submitting = false;
                                    modal.error = Some(err);
                                }
                            }
                        }
                    }
                }
                UiEvent::InviteCreated { seq, channel_id, result } => {
                    if let Overlay::InviteMembers(modal) = &mut self.overlay
                        && modal.seq == seq
                        && modal.channel_id == channel_id
                    {
                        modal.loading = false;
                        match result {
                            Ok(inv) => {
                                modal.invite = Some(inv);
                                modal.error = None;
                            }
                            Err(err) => {
                                modal.error = Some(err);
                            }
                        }
                    }
                }
                UiEvent::GuildMessagesBulkDeleted { seq, guild_id, error } => {
                    if matches!(
                        &self.overlay,
                        Overlay::DeleteMyGuildMessages(m) if m.seq == seq && m.guild_id == guild_id
                    ) {
                        match error {
                            None => {
                                self.overlay = Overlay::None;
                                self.focus_composer_later(cx);
                            }
                            Some(err) => {
                                if let Overlay::DeleteMyGuildMessages(modal) = &mut self.overlay {
                                    modal.submitting = false;
                                    modal.error = Some(err);
                                }
                            }
                        }
                    }
                }
                UiEvent::PersonalNotesPurged { seq, channel_id, result } => {
                    if matches!(
                        &self.overlay,
                        Overlay::PurgePersonalNotes(m) if m.seq == seq && m.channel_id == channel_id
                    ) {
                        match result {
                            Ok(_) => {
                                self.overlay = Overlay::None;
                                self.focus_composer_later(cx);
                            }
                            Err(err) => {
                                if let Overlay::PurgePersonalNotes(modal) = &mut self.overlay {
                                    modal.submitting = false;
                                    modal.error = Some(err);
                                }
                            }
                        }
                    }
                }
                UiEvent::DiscoveryCategoriesLoaded { result } => {
                    if let Ok(cats) = result && !cats.is_empty() {
                        self.explore.categories = cats;
                    }
                }
                UiEvent::DiscoveryGuildsLoaded { seq, offset, result } => {
                    if self.explore.search_seq == seq {
                        self.explore.loading = false;
                        self.explore.has_loaded = true;
                        match result {
                            Ok(page) => {
                                if offset == 0 {
                                    self.explore.guilds = page.guilds;
                                } else {
                                    for g in page.guilds {
                                        if self.explore.guilds.iter().all(|existing| existing.id != g.id) {
                                            self.explore.guilds.push(g);
                                        }
                                    }
                                }
                                self.explore.total = page.total;
                                self.explore.error = None;
                            }
                            Err(err) => {
                                self.explore.error = Some(err);
                            }
                        }
                    }
                }
                UiEvent::DiscoveryGuildJoined { seq, guild_id, error } => {
                    if matches!(&self.overlay, Overlay::DiscoveryPreview(m) if m.join_seq == Some(seq)) {
                        match error {
                            None => {
                                self.overlay = Overlay::None;
                                self.focus_composer_later(cx);
                                self.pending_joined_channel = None;
                                if self.backend.state().guilds.contains_key(&guild_id) {
                                    self.pending_joined_guild = None;
                                    self.select_place(Place::Guild(guild_id), cx);
                                } else {
                                    self.pending_joined_guild = Some(guild_id);
                                }
                            }
                            Some(err) => {
                                if let Overlay::DiscoveryPreview(modal) = &mut self.overlay {
                                    modal.joining = false;
                                    modal.join_seq = None;
                                    modal.error = Some(err);
                                }
                            }
                        }
                    }
                }
            }
        }
        if connected || guild_available {
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
        if let Some((jump_ch, target)) = self.pending_jump
            && Some(jump_ch) == self.selected_channel
            && !self.guild_unavailable
            && !self.loading_history
        {
            self.pending_jump = None;
            if !self.reveal_jump(jump_ch, target, cx) && self.error.is_none() {
                self.error = Some(l10n::tr(target.missing_label()).into());
            }
            cx.notify();
        }
    }

    /// After READY the windows of a new session are cleared (and those of
    /// the same session invalidated): retry a pending jump or reload the
    /// current channel's latest page.
    fn reload_cleared_channel(&mut self) {
        let Some(channel_id) = self.selected_channel else { return };
        let (can_view, cleared) = {
            let state = self.backend.state();
            if state.is_unavailable_channel(channel_id) {
                return;
            }
            (state.can_view_channel(channel_id), state.messages.window(channel_id).is_none_or(|w| !w.ready))
        };
        if !can_view {
            self.loading_history = false;
            self.pending_jump = None;
            return;
        }
        if cleared {
            self.loading_history = true;
            if let Some((channel, target)) = self.pending_jump
                && channel == channel_id
            {
                self.backend.load_around(channel_id, target.anchor());
            } else {
                self.backend.load_messages(channel_id, None);
            }
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
        self.voice = self.backend.voice_snapshot();
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
            self.guild_unavailable = matches!(self.place, Place::Guild(id) if state.unavailable_guilds.contains(&id));
            if self.guild_unavailable {
                self.loading_history = false;
            }
            if let Place::Guild(id) = self.place
                && !state.guilds.contains_key(&id)
                && !self.guild_unavailable
            {
                self.place = Place::Home;
                self.selected_channel = None;
                self.last_channel.remove(&Some(id));
                if self.last_guild == Some(id) {
                    self.last_guild = None;
                }
            }
            let mut fallback_guild_channel: Option<Option<Snowflake>> = None;
            let retains_channel = |id: Snowflake| state.channels.contains_key(&id) || state.is_unavailable_channel(id);
            if let Some(removed_id) = self.selected_channel.filter(|id| !retains_channel(*id)) {
                self.drafts.remove(&removed_id);
                self.last_channel.retain(|_, v| *v != removed_id);
                self.selected_channel = None;
                if matches!(self.place, Place::Guild(_)) {
                    let next = channel_items_with_active(
                        &state,
                        self.place,
                        self.backend.media(),
                        self.selected_channel,
                        self.voice.channel_id,
                    )
                    .into_iter()
                    .find(|c| c.has_messages)
                    .map(|c| c.id);
                    fallback_guild_channel = Some(next);
                }
            } else if self.selected_channel.is_some_and(|id| !state.can_view_channel(id))
                && matches!(self.place, Place::Guild(_))
                && !self.guild_unavailable
            {
                let next = channel_items_with_active(
                    &state,
                    self.place,
                    self.backend.media(),
                    self.selected_channel,
                    self.voice.channel_id,
                )
                .into_iter()
                .find(|c| c.has_messages)
                .map(|c| c.id);
                fallback_guild_channel = Some(next);
            }
            if state.current_user_id().is_some() {
                self.drafts.retain(|id, _| retains_channel(*id));
                self.last_channel.retain(|_, v| retains_channel(*v));
            }
            self.attachments.retain_channels(retains_channel);
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
                Overlay::MemberModeration(m) => {
                    let now = i64::try_from(crate::accounts::now_ms()).unwrap_or(i64::MAX);
                    !state.guilds.contains_key(&m.guild_id)
                        || !match &m.kind {
                            ui::moderation_modal::ModerationKind::Kick => state.can_kick_member(m.guild_id, m.user_id),
                            ui::moderation_modal::ModerationKind::Ban => state.can_ban_member(m.guild_id, m.user_id),
                            ui::moderation_modal::ModerationKind::Timeout { .. } => {
                                state.can_timeout_member(m.guild_id, m.user_id)
                            }
                            ui::moderation_modal::ModerationKind::Nickname { .. } => {
                                state.can_edit_member_nickname(m.guild_id, m.user_id, now)
                            }
                        }
                }
                Overlay::GuildBans(m) => {
                    self.place != Place::Guild(m.guild_id)
                        || !state.guilds.contains_key(&m.guild_id)
                        || !state.can_ban_members(m.guild_id)
                }
                Overlay::Access(m) => !m.allowed(self.place, &state),
                Overlay::InviteMembers(m) => {
                    !state.guilds.contains_key(&m.guild_id)
                        || !ui::invite_modal::can_invite_in_guild(&state, m.guild_id, Some(m.channel_id))
                }
                Overlay::GuildNotifications(m) => !state.guilds.contains_key(&m.guild_id),
                Overlay::GuildPrivacy(m) => !state.guilds.contains_key(&m.guild_id),
                Overlay::GuildProfile(m) => !state.guilds.contains_key(&m.guild_id),
                Overlay::DeleteMyGuildMessages(m) => {
                    !state.guilds.contains_key(&m.guild_id)
                        || state
                            .current_user_id()
                            .is_some_and(|me| state.guilds.get(&m.guild_id).is_some_and(|g| g.owner_id == Some(me)))
                }
                _ => false,
            };
            if dismiss_modal {
                self.overlay = Overlay::None;
                self.focus_composer_later(cx);
            }
            if let Some(next_opt) = fallback_guild_channel {
                drop(state);
                match next_opt {
                    Some(id) => self.select_channel(id, cx),
                    None => self.clear_selection(cx),
                }
                return;
            }
            if !dismiss_modal && let Overlay::EditChannel(m) = &mut self.overlay {
                m.sync_state(&state);
            }
            self.guilds = guild_items_with_media(&state, self.backend.media());
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
            self.channels = channel_items_with_active(
                &state,
                self.place,
                self.backend.media(),
                self.selected_channel,
                self.voice.channel_id,
            );
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
                ComposerMode::Replying(t)
                    if state.messages.window(t.channel_id).is_some_and(|window| {
                        window.ready && window.messages.iter().all(|message| message.id != t.message_id)
                    }) =>
                {
                    Some(t.message_id)
                }
                _ => None,
            };
            if let Some(id) = reply_gone {
                self.composer_mode.message_deleted(id);
            }
            self.can_ban_members = match self.place {
                Place::Guild(gid) => state.can_ban_members(gid),
                Place::Home | Place::Explore => false,
            };
            let can_manage_guild = match self.place {
                Place::Guild(gid) => state.can_manage_guild_channels(gid),
                Place::Home | Place::Explore => false,
            };
            let can_manage_selected = self
                .selected_channel
                .filter(|_| matches!(self.place, Place::Guild(_)))
                .is_some_and(|id| state.can_manage_channel(id));
            match self.selected_channel {
                Some(id) => {
                    if self.unread_anchor.is_none() && self.scroller.read(cx).is_scrolled_up() {
                        self.unread_anchor = state.unread_anchor(id);
                    }
                    let channel = state.channels.get(&id);
                    let topic = channel
                        .and_then(|c| c.topic.as_deref())
                        .map(str::trim)
                        .filter(|t| !t.is_empty())
                        .map(SharedString::from);
                    (
                        message_pane(&state, id, self.backend.media(), self.unread_anchor),
                        !self.guild_unavailable && state.can_send_messages(id),
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
        if self.guild_unavailable {
            self.cancel_ack();
        }
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
        self.backend.notify_focus().set_viewing(self.selected_channel.filter(|_| !self.guild_unavailable));
        let mut rows = pane.rows;
        for row in &rows {
            if let Some(ui::reply::ReplyLine::NotLoaded { channel_id, message_id }) = row.reply {
                drop(self.backend.fetch_reply_target(channel_id, message_id));
            }
        }
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
        if self.manual_unread_channel == Some(channel_id)
            || self.loading_history
            || self.pending_jump.is_some()
            || self.has_more_after
            || self.scroller.read(cx).is_scrolled_up()
        {
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

    pub(crate) fn check_for_updates(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.update_status,
            crate::updater::UpdateStatus::Checking | crate::updater::UpdateStatus::Downloading { .. }
        ) {
            return;
        }
        self.update_status = crate::updater::UpdateStatus::Checking;
        cx.notify();
        let bg = cx.background_executor().clone();
        self._update_task = Some(cx.spawn(async move |this, cx| {
            let status = bg
                .spawn(async move {
                    let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() else {
                        return crate::updater::UpdateStatus::Error("Could not start update runtime".to_owned());
                    };
                    rt.block_on(crate::updater::check_for_updates(
                        crate::updater::DEFAULT_RELEASES_API_URL,
                        crate::updater::CURRENT_VERSION,
                    ))
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.update_status = status;
                view._update_task = None;
                cx.notify();
            });
        }));
    }

    pub(crate) fn download_available_update(&mut self, cx: &mut Context<Self>) {
        let crate::updater::UpdateStatus::Available {
            latest,
            asset_name: Some(asset_name),
            asset_url: Some(asset_url),
            checksum_url,
            ..
        } = &self.update_status
        else {
            return;
        };
        let latest = latest.clone();
        let asset_name = asset_name.clone();
        let asset_url = asset_url.clone();
        let checksum_url = checksum_url.clone();
        let dest_dir = std::env::var_os("LOCALAPPDATA")
            .filter(|v| !v.is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("FluxerClient")
            .join("updates");

        self.update_status = crate::updater::UpdateStatus::Downloading {
            latest: latest.clone(),
        };
        cx.notify();
        let bg = cx.background_executor().clone();
        self._update_task = Some(cx.spawn(async move |this, cx| {
            let status = bg
                .spawn(async move {
                    let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() else {
                        return crate::updater::UpdateStatus::Error(
                            "Could not start update download runtime".to_owned(),
                        );
                    };
                    rt.block_on(crate::updater::download_update(
                        &latest,
                        &asset_name,
                        &asset_url,
                        checksum_url.as_deref(),
                        &dest_dir,
                    ))
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.update_status = status;
                view._update_task = None;
                cx.notify();
            });
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

    /// Splices only the changed middle of the grouped row list so the scroll anchor
    /// survives prepends (older pages) and appends (new messages).
    fn sync_rows(&mut self, new_rows: Vec<MessageRow>, cx: &mut Context<Self>) {
        let new_groups = ui::message_list::compute_row_groups(&new_rows);
        let (start, old_end, new_count) = {
            let old = self.rows.borrow();
            let old_groups = self.row_groups.borrow();
            let group_eq = |a: &std::ops::Range<usize>, b: &std::ops::Range<usize>| {
                old.get(a.clone()) == new_rows.get(b.clone())
            };
            let prefix = old_groups.iter().zip(&new_groups).take_while(|(a, b)| group_eq(a, b)).count();
            let max_suffix = old_groups.len().min(new_groups.len()) - prefix;
            let suffix = old_groups
                .iter()
                .rev()
                .zip(new_groups.iter().rev())
                .take(max_suffix)
                .take_while(|(a, b)| group_eq(a, b))
                .count();
            (prefix, old_groups.len() - suffix, new_groups.len() - prefix - suffix)
        };
        *self.rows.borrow_mut() = new_rows;
        *self.row_groups.borrow_mut() = new_groups;
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
        self.voice_stage_open = false;
        self.favorites_selected = false;
        self.place = place;
        if let Place::Guild(id) = place {
            self.last_guild = Some(id);
            self.backend.subscribe_guild(id);
        }
        if place == Place::Explore {
            self.clear_selection(cx);
            self.ensure_explore_loaded(cx);
            return;
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

    pub(crate) fn open_explore(&mut self, cx: &mut Context<Self>) {
        self.select_place(Place::Explore, cx);
    }

    fn ensure_explore_loaded(&mut self, cx: &mut Context<Self>) {
        if !self.explore.categories_fetched {
            self.explore.categories_fetched = true;
            self.backend.fetch_discovery_categories();
        }
        if !self.explore.has_loaded && !self.explore.loading {
            self.fetch_explore_guilds(0, cx);
        }
    }

    fn fetch_explore_guilds(&mut self, offset: u32, cx: &mut Context<Self>) {
        self.explore.search_seq = self.explore.search_seq.wrapping_add(1);
        let seq = self.explore.search_seq;
        self.explore.loading = true;
        self.explore.error = None;
        if offset == 0 {
            self.explore.guilds.clear();
            self.explore.total = 0;
        }
        let query_text = self.explore.submitted_query.trim().to_owned();
        let query = DiscoveryGuildQuery {
            query: (!query_text.is_empty()).then_some(query_text),
            category: self.explore.selected_category,
            language: self.explore.selected_language.clone(),
            tag: None,
            sort_by: None,
            limit: ui::explore::PAGE_SIZE,
            offset,
        };
        self.backend.search_discovery_guilds(seq, query);
        cx.notify();
    }

    pub(crate) fn explore_select_category(&mut self, category: Option<u32>, cx: &mut Context<Self>) {
        self.explore.selected_category = category;
        self.explore.submitted_query.clear();
        self.fetch_explore_guilds(0, cx);
    }

    pub(crate) fn explore_select_language(&mut self, language: Option<String>, cx: &mut Context<Self>) {
        self.explore.selected_language = language;
        self.fetch_explore_guilds(0, cx);
    }

    pub(crate) fn submit_explore_search(&mut self, cx: &mut Context<Self>) {
        let q = self.explore.search_input.read(cx).value().trim().to_owned();
        self.explore.submitted_query = q;
        self.fetch_explore_guilds(0, cx);
    }

    pub(crate) fn clear_explore_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.explore.submitted_query.clear();
        self.explore.search_input.update(cx, |s, cx| s.set_value("", window, cx));
        self.fetch_explore_guilds(0, cx);
    }

    pub(crate) fn explore_reset_to_all(&mut self, cx: &mut Context<Self>) {
        self.explore.selected_category = None;
        self.explore.submitted_query.clear();
        let (input, handle) = (self.explore.search_input.clone(), self.window_handle);
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                input.update(cx, |s, cx| s.set_value("", window, cx));
            });
        });
        self.fetch_explore_guilds(0, cx);
    }

    pub(crate) fn explore_load_more(&mut self, cx: &mut Context<Self>) {
        if self.explore.loading || (self.explore.guilds.len() as u64) >= self.explore.total {
            return;
        }
        let offset = u32::try_from(self.explore.guilds.len()).unwrap_or(u32::MAX);
        self.fetch_explore_guilds(offset, cx);
    }

    pub(crate) fn explore_retry(&mut self, cx: &mut Context<Self>) {
        self.fetch_explore_guilds(0, cx);
    }

    pub(crate) fn open_discovery_preview(
        &mut self,
        guild: DiscoveryGuild,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        self.overlay = Overlay::DiscoveryPreview(Box::new(ui::explore::DiscoveryPreviewModal {
            guild,
            joining: false,
            join_seq: None,
            error: None,
        }));
        cx.notify();
    }

    pub(crate) fn confirm_join_discovery_guild(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Overlay::DiscoveryPreview(modal) = &mut self.overlay else {
            return;
        };
        let Some(guild_id) = Snowflake::parse(&modal.guild.id) else {
            modal.error = Some(l10n::tr("Invalid community ID."));
            cx.notify();
            return;
        };
        if self.backend.state().guilds.contains_key(&guild_id) {
            self.close_overlay(window, cx);
            self.select_place(Place::Guild(guild_id), cx);
            return;
        }
        if modal.joining {
            return;
        }
        self.explore.join_seq = self.explore.join_seq.wrapping_add(1);
        let seq = self.explore.join_seq;
        if let Overlay::DiscoveryPreview(modal) = &mut self.overlay {
            modal.joining = true;
            modal.join_seq = Some(seq);
            modal.error = None;
        }
        self.backend.join_discovery_guild(seq, guild_id);
        cx.notify();
    }

    pub(crate) fn select_favorites(&mut self, cx: &mut Context<Self>) {
        self.select_place(Place::Home, cx);
        self.favorites_selected = true;
        cx.notify();
    }

    pub(crate) fn set_rail_hover(&mut self, target: RailHoverTarget, hovered: bool, cx: &mut Context<Self>) {
        let next = if hovered {
            Some(target)
        } else if self.hovered_rail_item == Some(target) {
            None
        } else {
            self.hovered_rail_item
        };
        if self.hovered_rail_item != next {
            self.hovered_rail_item = next;
            cx.notify();
        }
    }

    pub(crate) fn set_header_hover(&mut self, target: HeaderHoverTarget, hovered: bool, cx: &mut Context<Self>) {
        let next = if hovered {
            Some(target)
        } else if self.hovered_header_item == Some(target) {
            None
        } else {
            self.hovered_header_item
        };
        if self.hovered_header_item != next {
            self.hovered_header_item = next;
            cx.notify();
        }
    }

    pub(crate) fn set_user_control_hover(
        &mut self,
        target: UserControlHoverTarget,
        hovered: bool,
        cx: &mut Context<Self>,
    ) {
        let next = if hovered {
            Some(target)
        } else if self.hovered_user_control == Some(target) {
            None
        } else {
            self.hovered_user_control
        };
        if self.hovered_user_control != next {
            self.hovered_user_control = next;
            cx.notify();
        }
    }

    pub(crate) fn select_channel(&mut self, channel_id: Snowflake, cx: &mut Context<Self>) {
        self.voice_stage_open = false;
        self.select_channel_inner(channel_id, true, cx);
    }

    pub(crate) fn open_voice_stage(&mut self, cx: &mut Context<Self>) {
        if self.voice.channel_id.is_some()
            && self.voice.state != crate::backend::voice::VoiceConnectionState::Disconnected
        {
            self.voice_stage_open = true;
            cx.notify();
        }
    }

    pub(crate) fn close_voice_stage(&mut self, cx: &mut Context<Self>) {
        if self.voice_stage_open {
            self.voice_stage_open = false;
            self.voice_popout = None;
            cx.notify();
        }
    }

    pub(crate) fn toggle_voice_stage_expanded(&mut self, cx: &mut Context<Self>) {
        self.voice_stage_expanded = !self.voice_stage_expanded;
        cx.notify();
    }

    pub(crate) fn effective_sidebar_width(&self) -> f32 {
        self.sidebar_width.clamp(SIDEBAR_WIDTH_MIN as f32, SIDEBAR_WIDTH_MAX as f32)
    }

    pub(crate) fn max_voice_call_height(&self, window: &Window) -> f32 {
        let vh = f32::from(window.viewport_size().height);
        // Reserve 32px desktop title bar + ~178px minimum split chat/composer space, capped at official 1049px.
        ((vh - 32.0 - 178.0).min(VOICE_CALL_HEIGHT_MAX as f32)).max(VOICE_CALL_HEIGHT_MIN as f32)
    }

    pub(crate) fn effective_voice_call_height(&self, window: &Window) -> f32 {
        self.voice_call_height.clamp(VOICE_CALL_HEIGHT_MIN as f32, self.max_voice_call_height(window))
    }

    fn max_emoji_picker_bounds(&self, window: &Window) -> (f32, f32) {
        let viewport = window.viewport_size();
        let vw = f32::from(viewport.width);
        let vh = f32::from(viewport.height);
        let min_top = if vh >= 700. { 44. } else { 16. };
        let anchor = match &self.overlay {
            crate::ui::overlay::Overlay::EmojiPicker(picker) => picker.anchor,
            _ => gpui_kit::point(px((vw - 16.).max(0.)), px((vh - 16.).max(0.))),
        };
        let min_right = (EMOJI_PICKER_WIDTH_MIN as f32 + 16.).min(vw - 16.);
        let max_right = (vw - 16.).max(EMOJI_PICKER_WIDTH_MIN as f32 + 16.);
        let eff_right = f32::from(anchor.x).clamp(min_right, max_right);
        let min_bottom = (min_top + EMOJI_PICKER_HEIGHT_MIN as f32).min(vh - 16.);
        let max_bottom = (vh - 16.).max(min_top + EMOJI_PICKER_HEIGHT_MIN as f32);
        let eff_bottom = f32::from(anchor.y).clamp(min_bottom, max_bottom);
        let max_w = (eff_right - 16.).clamp(EMOJI_PICKER_WIDTH_MIN as f32, EMOJI_PICKER_WIDTH_MAX as f32);
        let max_h = (eff_bottom - min_top).clamp(EMOJI_PICKER_HEIGHT_MIN as f32, EMOJI_PICKER_HEIGHT_MAX as f32);
        (max_w, max_h)
    }

    fn apply_emoji_picker_size(&mut self, width: u16, height: u16, persist: bool, _cx: &mut Context<Self>) {
        self.emoji_picker_width = width as f32;
        self.emoji_picker_height = height as f32;
        if let crate::ui::overlay::Overlay::EmojiPicker(picker) = &mut self.overlay {
            picker.set_width(width as f32);
        }
        if persist {
            self.prefs.set_emoji_picker_size(width, height);
        }
    }

    pub(crate) fn set_sidebar_resize_hover(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if self.sidebar_resize_hovered != hovered {
            self.sidebar_resize_hovered = hovered;
            cx.notify();
        }
    }

    pub(crate) fn set_voice_resize_hover(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if self.voice_resize_hovered != hovered {
            self.voice_resize_hovered = hovered;
            cx.notify();
        }
    }

    pub(crate) fn begin_sidebar_resize(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if ev.button != MouseButton::Left {
            return;
        }
        window.focus(&self.sidebar_resize_focus, cx);
        if ev.click_count >= 2 {
            self.active_resize = None;
            self.sidebar_width = SIDEBAR_WIDTH_DEFAULT as f32;
            self.prefs.set_sidebar_width(SIDEBAR_WIDTH_DEFAULT);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        self.active_resize = Some(ActiveResize::Sidebar {
            start_x: f32::from(ev.position.x),
            start_width: self.effective_sidebar_width(),
        });
        cx.stop_propagation();
        cx.notify();
    }

    pub(crate) fn begin_voice_height_resize(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if ev.button != MouseButton::Left {
            return;
        }
        window.focus(&self.voice_resize_focus, cx);
        if ev.click_count >= 2 {
            self.active_resize = None;
            self.voice_call_height = VOICE_CALL_HEIGHT_DEFAULT as f32;
            self.prefs.set_voice_call_height(VOICE_CALL_HEIGHT_DEFAULT);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        self.active_resize = Some(ActiveResize::VoiceCallHeight {
            start_y: f32::from(ev.position.y),
            start_height: self.effective_voice_call_height(window),
        });
        cx.stop_propagation();
        cx.notify();
    }

    pub(crate) fn begin_emoji_picker_resize(
        &mut self,
        edge: ResizeEdge,
        ev: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if ev.button != MouseButton::Left {
            return;
        }
        window.focus(&self.emoji_resize_focus, cx);
        if ev.click_count >= 2 {
            self.active_resize = None;
            self.apply_emoji_picker_size(EMOJI_PICKER_WIDTH_DEFAULT, EMOJI_PICKER_HEIGHT_DEFAULT, true, cx);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        self.active_resize = Some(ActiveResize::EmojiPicker {
            edge,
            start_x: f32::from(ev.position.x),
            start_y: f32::from(ev.position.y),
            start_w: self.emoji_picker_width,
            start_h: self.emoji_picker_height,
        });
        cx.stop_propagation();
        cx.notify();
    }

    pub(crate) fn handle_resize_move(&mut self, ev: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(active) = self.active_resize else {
            return;
        };
        match active {
            ActiveResize::Sidebar { start_x, start_width } => {
                let next = (start_width + (f32::from(ev.position.x) - start_x))
                    .round()
                    .clamp(SIDEBAR_WIDTH_MIN as f32, SIDEBAR_WIDTH_MAX as f32);
                if (next - self.sidebar_width).abs() > 0.5 {
                    self.sidebar_width = next;
                    cx.notify();
                }
            }
            ActiveResize::VoiceCallHeight { start_y, start_height } => {
                let max_h = self.max_voice_call_height(window);
                let next = (start_height + (f32::from(ev.position.y) - start_y))
                    .round()
                    .clamp(VOICE_CALL_HEIGHT_MIN as f32, max_h);
                if (next - self.voice_call_height).abs() > 0.5 {
                    self.voice_call_height = next;
                    cx.notify();
                }
            }
            ActiveResize::EmojiPicker {
                edge,
                start_x,
                start_y,
                start_w,
                start_h,
            } => {
                let (max_w, max_h) = self.max_emoji_picker_bounds(window);
                let next_w = if matches!(edge, ResizeEdge::Left | ResizeEdge::TopLeft) {
                    let dx = start_x - f32::from(ev.position.x);
                    snap_emoji_picker_width(start_w + dx, max_w)
                } else {
                    self.emoji_picker_width.round() as u16
                };
                let next_h = if matches!(edge, ResizeEdge::Top | ResizeEdge::TopLeft) {
                    let dy = start_y - f32::from(ev.position.y);
                    (start_h + dy).round().clamp(EMOJI_PICKER_HEIGHT_MIN as f32, max_h) as u16
                } else {
                    self.emoji_picker_height.round() as u16
                };
                if (next_w as f32 - self.emoji_picker_width).abs() > 0.5
                    || (next_h as f32 - self.emoji_picker_height).abs() > 0.5
                {
                    self.apply_emoji_picker_size(next_w, next_h, false, cx);
                    cx.notify();
                }
            }
        }
    }

    pub(crate) fn finish_resize(&mut self, cx: &mut Context<Self>) {
        let Some(active) = self.active_resize.take() else {
            return;
        };
        match active {
            ActiveResize::Sidebar { .. } => {
                self.prefs.set_sidebar_width(self.effective_sidebar_width().round() as u16);
            }
            ActiveResize::VoiceCallHeight { .. } => {
                self.prefs.set_voice_call_height(self.voice_call_height.round() as u16);
            }
            ActiveResize::EmojiPicker { .. } => {
                self.prefs.set_emoji_picker_size(
                    self.emoji_picker_width.round() as u16,
                    self.emoji_picker_height.round() as u16,
                );
            }
        }
        cx.notify();
    }

    pub(crate) fn focus_root(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
    }

    pub(crate) fn is_recording_keybind(&self) -> bool {
        matches!(
            &self.overlay,
            Overlay::Settings(modal) if modal.keybinds_recording_id.is_some()
        )
    }

    pub(crate) fn should_run_builtin_shortcut(&self, action: &str) -> bool {
        !self.is_recording_keybind() && shortcuts::is_builtin_action_active(&self.prefs.data, action)
    }

    pub(crate) fn dispatch_keybind_action(
        &mut self,
        action: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match action {
            "nav_channel_prev" => self.cycle_channel(-1, cx),
            "nav_channel_next" => self.cycle_channel(1, cx),
            "nav_unread_prev" => self.cycle_filtered(crate::nav::ChannelFilter::Unread, -1, cx),
            "nav_unread_next" => self.cycle_filtered(crate::nav::ChannelFilter::Unread, 1, cx),
            "nav_mention_prev" => self.cycle_filtered(crate::nav::ChannelFilter::Mentioned, -1, cx),
            "nav_mention_next" => self.cycle_filtered(crate::nav::ChannelFilter::Mentioned, 1, cx),
            "nav_guild_prev" => self.cycle_guild(-1, cx),
            "nav_guild_next" => self.cycle_guild(1, cx),
            "nav_history_back" => self.history_back(cx),
            "nav_history_forward" => self.history_forward(cx),
            "nav_toggle_last_guild_dms" => self.toggle_last_guild(cx),
            "nav_add_guild" => self.open_join_guild(window, cx),
            "misc_search" => self.open_search(window, cx),
            "nav_quick_switcher" => self.toggle_quick_switcher(window, cx),
            "chat_toggle_emoji" => self.toggle_emoji_picker(window, cx),
            "system_toggle_settings" => self.open_settings(window, cx),
            "system_zoom_in" => self.step_ui_zoom(10, cx),
            "system_zoom_out" => self.step_ui_zoom(-10, cx),
            "system_zoom_reset" => self.settings_set_zoom(100, cx),
            "chat_mark_channel_read" => self.mark_channel_read(cx),
            "chat_mark_guild_read" => self.mark_guild_read(cx),
            "voice_toggle_mute" => self.toggle_voice_mute(cx),
            "voice_toggle_deafen" => self.toggle_voice_deaf(cx),
            _ => return false,
        }
        true
    }

    pub(crate) fn intercept_keystroke(
        &mut self,
        keystroke: &gpui_kit::Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Overlay::Settings(modal) = &self.overlay {
            if let Some(recording_id) = modal.keybinds_recording_id.clone() {
                if keystroke.key == "escape"
                    && !keystroke.modifiers.control
                    && !keystroke.modifiers.alt
                    && !keystroke.modifiers.shift
                    && !keystroke.modifiers.platform
                {
                    self.settings_cancel_record_keybind(cx);
                    return true;
                }
                if let Some(combo) = shortcuts::keystroke_to_combo(keystroke) {
                    self.settings_set_custom_keybind_combo(&recording_id, combo, cx);
                    self.settings_cancel_record_keybind(cx);
                }
                return true;
            }
        }

        if self.overlay.is_open() {
            return false;
        }
        let Some(combo) = shortcuts::keystroke_to_combo(keystroke) else {
            return false;
        };
        let Some(action) = shortcuts::matching_custom_action(&self.prefs.data, &combo).map(str::to_owned) else {
            return false;
        };
        self.dispatch_keybind_action(&action, window, cx)
    }

    pub(crate) fn handle_sidebar_resize_key(
        &mut self,
        ev: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let step = if ev.keystroke.modifiers.shift { 48.0 } else { 12.0 };
        let next = match ev.keystroke.key.as_str() {
            "left" => Some((self.effective_sidebar_width() - step).clamp(SIDEBAR_WIDTH_MIN as f32, SIDEBAR_WIDTH_MAX as f32)),
            "right" => Some((self.effective_sidebar_width() + step).clamp(SIDEBAR_WIDTH_MIN as f32, SIDEBAR_WIDTH_MAX as f32)),
            "home" => Some(SIDEBAR_WIDTH_MIN as f32),
            "end" => Some(SIDEBAR_WIDTH_MAX as f32),
            "enter" | "backspace" | "delete" => Some(SIDEBAR_WIDTH_DEFAULT as f32),
            _ => None,
        };
        if let Some(next) = next {
            self.sidebar_width = next;
            self.prefs.set_sidebar_width(next.round() as u16);
            cx.stop_propagation();
            cx.notify();
        }
    }

    pub(crate) fn handle_voice_resize_key(
        &mut self,
        ev: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let max_h = self.max_voice_call_height(window);
        let min_h = VOICE_CALL_HEIGHT_MIN as f32;
        let step = if ev.keystroke.modifiers.shift { 48.0 } else { 16.0 };
        let cur = self.effective_voice_call_height(window);
        let next = match ev.keystroke.key.as_str() {
            "up" => Some((cur - step).clamp(min_h, max_h)),
            "down" => Some((cur + step).clamp(min_h, max_h)),
            "home" => Some(min_h),
            "end" => Some(max_h),
            "enter" | "backspace" | "delete" => Some(VOICE_CALL_HEIGHT_DEFAULT as f32),
            _ => None,
        };
        if let Some(next) = next {
            self.voice_call_height = next;
            self.prefs.set_voice_call_height(next.round() as u16);
            cx.stop_propagation();
            cx.notify();
        }
    }

    pub(crate) fn handle_emoji_resize_key(
        &mut self,
        ev: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (max_w, max_h) = self.max_emoji_picker_bounds(window);
        let min_h = EMOJI_PICKER_HEIGHT_MIN as f32;
        let step_w = if ev.keystroke.modifiers.shift { 96.0 } else { 48.0 };
        let step_h = if ev.keystroke.modifiers.shift { 48.0 } else { 16.0 };
        let next = match ev.keystroke.key.as_str() {
            "left" => Some((snap_emoji_picker_width(self.emoji_picker_width + step_w, max_w), self.emoji_picker_height.round() as u16)),
            "right" => Some((snap_emoji_picker_width(self.emoji_picker_width - step_w, max_w), self.emoji_picker_height.round() as u16)),
            "up" => Some((snap_emoji_picker_width(self.emoji_picker_width, max_w), (self.emoji_picker_height + step_h).clamp(min_h, max_h).round() as u16)),
            "down" => Some((snap_emoji_picker_width(self.emoji_picker_width, max_w), (self.emoji_picker_height - step_h).clamp(min_h, max_h).round() as u16)),
            "home" => Some((EMOJI_PICKER_WIDTH_MIN, EMOJI_PICKER_HEIGHT_MIN)),
            "enter" | "backspace" | "delete" => Some((EMOJI_PICKER_WIDTH_DEFAULT, EMOJI_PICKER_HEIGHT_DEFAULT)),
            _ => None,
        };
        if let Some((w, h)) = next {
            self.apply_emoji_picker_size(w, h, true, cx);
            cx.stop_propagation();
            cx.notify();
        }
    }

    fn render_sidebar_resize_handle(&self, sidebar_w: f32, rail_width: f32, cx: &mut Context<Self>) -> AnyElement {
        let dragging = matches!(self.active_resize, Some(ActiveResize::Sidebar { .. }));
        let handle_bottom = if !matches!(self.voice.state, crate::backend::voice::VoiceConnectionState::Disconnected) {
            261.0
        } else {
            66.0
        };
        let (indicator_w, indicator_opacity) = if dragging {
            (6.0, 1.0)
        } else if self.sidebar_resize_hovered {
            (4.0, 0.92)
        } else {
            (2.0, 0.0)
        };
        div()
            .id("sidebar-resize-handle")
            .debug_selector(|| "sidebar-resize-handle".to_owned())
            .aria_label(l10n::tr("Resize sidebar"))
            .flx("app.sidebar-resize-handle")
            .track_focus(&self.sidebar_resize_focus)
            .absolute()
            .left(px(rail_width + 1. + sidebar_w - 3.))
            .top(px(1.))
            .bottom(px(handle_bottom))
            .w(px(6.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_col_resize()
            .on_hover(cx.listener(|v, hovered, _, cx| v.set_sidebar_resize_hover(*hovered, cx)))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::begin_sidebar_resize))
            .on_key_down(cx.listener(Self::handle_sidebar_resize_key))
            .child(
                div()
                    .debug_selector(|| "sidebar-resize-indicator".to_owned())
                    .flx("app.sidebar-resize-handle.indicator")
                    .w(px(indicator_w))
                    .h_full()
                    .rounded_full()
                    .bg(rgb(0x69686d))
                    .opacity(indicator_opacity),
            )
            .into_any_element()
    }

    pub(crate) fn toggle_user_area_popout(&mut self, cx: &mut Context<Self>) {
        self.user_area_popout_open = !self.user_area_popout_open;
        self.user_area_submenu = None;
        if self.user_area_popout_open {
            self.voice_popout = None;
            self.guild_header_popout_open = false;
        }
        cx.notify();
    }

    pub(crate) fn toggle_guild_header_popout(&mut self, cx: &mut Context<Self>) {
        self.guild_header_popout_open = !self.guild_header_popout_open;
        if self.guild_header_popout_open {
            self.user_area_popout_open = false;
            self.voice_popout = None;
        }
        cx.notify();
    }

    pub(crate) fn toggle_voice_popout(&mut self, kind: VoicePopoutKind, cx: &mut Context<Self>) {
        if self.voice_popout == Some(kind) {
            self.voice_popout = None;
        } else {
            self.voice_popout = Some(kind);
            self.user_area_popout_open = false;
            self.refresh_audio_devices(cx);
        }
        cx.notify();
    }

    pub(crate) fn open_voice_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.voice_popout = None;
        self.user_area_popout_open = false;
        if !matches!(self.overlay, Overlay::Settings(_)) {
            self.open_settings(window, cx);
        }
        self.select_settings_tab(ui::settings::SettingsTab::VoiceVideo, cx);
    }

    pub(crate) fn join_voice_channel(&mut self, channel_id: Snowflake, cx: &mut Context<Self>) {
        if self.voice.channel_id == Some(channel_id)
            && matches!(
                self.voice.state,
                crate::backend::voice::VoiceConnectionState::Connected
                    | crate::backend::voice::VoiceConnectionState::Connecting
                    | crate::backend::voice::VoiceConnectionState::Reconnecting
            )
        {
            self.voice_stage_open = true;
            cx.notify();
            return;
        }
        let target = {
            let state = self.backend.state();
            let Some(ch) = state.channels.get(&channel_id) else { return };
            if ch.kind != fluxer_client_core::state::ChannelKind::GuildVoice && ch.guild_id.is_some() {
                return;
            }
            if ch.guild_id.is_some() {
                let perms = state.channel_permissions(channel_id);
                let required = fluxer_client_core::state::bits::VIEW_CHANNEL
                    | fluxer_client_core::state::bits::CONNECT;
                if (perms & required) != required {
                    self.error = Some(l10n::tr("You do not have permission to connect to this voice channel.").into());
                    cx.notify();
                    return;
                }
            }
            ch.guild_id
        };
        self.error = None;
        self.backend.join_voice(target, channel_id);
        self.voice = self.backend.voice_snapshot();
        self.voice_stage_open = self.voice.channel_id.is_some();
        cx.notify();
    }

    fn start_dm_call(&mut self, channel_id: Snowflake, cx: &mut Context<Self>) {
        let was_active = self.voice.channel_id == Some(channel_id)
            && matches!(
                self.voice.state,
                crate::backend::voice::VoiceConnectionState::Connected
                    | crate::backend::voice::VoiceConnectionState::Connecting
                    | crate::backend::voice::VoiceConnectionState::Reconnecting
            );
        let ring_info = {
            let state = self.backend.state();
            let me = state.current_user_id();
            let Some(ch) = state.channels.get(&channel_id) else { return };
            let can_ring = matches!(
                ch.kind,
                fluxer_client_core::state::ChannelKind::Dm | fluxer_client_core::state::ChannelKind::GroupDm
            );
            let recipients: Vec<Snowflake> = ch
                .recipients
                .iter()
                .map(|u| u.id)
                .filter(|id| Some(*id) != me)
                .collect();
            (can_ring, (!recipients.is_empty()).then_some(recipients))
        };
        self.join_voice_channel(channel_id, cx);
        if !was_active && ring_info.0 && self.voice.channel_id == Some(channel_id) {
            self.backend.ring_dm_call(channel_id, ring_info.1);
        }
    }

    /// DM header voice-call button (`CallButtons.tsx` `VoiceCallButton`): joins and rings the DM call.
    pub(crate) fn start_dm_voice_call(&mut self, cx: &mut Context<Self>) {
        if let Some(channel_id) = self.selected_channel {
            self.start_dm_call(channel_id, cx);
        }
    }

    /// DM header video-call button (`CallButtons.tsx` `VideoCallButton`): joins and rings the DM call.
    /// TODO(backlog): no camera capture/publish path exists in the voice worker yet, so this only
    /// joins and rings without turning the local camera on.
    pub(crate) fn start_dm_video_call(&mut self, cx: &mut Context<Self>) {
        let Some(channel_id) = self.selected_channel else { return };
        self.start_dm_call(channel_id, cx);
    }

    pub(crate) fn leave_voice_channel(&mut self, cx: &mut Context<Self>) {
        self.backend.leave_voice();
        self.voice = self.backend.voice_snapshot();
        self.voice_stage_open = false;
        self.voice_popout = None;
        self.sync_watched_video_frame();
        cx.notify();
    }

    pub(crate) fn toggle_voice_mute(&mut self, cx: &mut Context<Self>) {
        self.backend.toggle_voice_mute();
        self.voice = self.backend.voice_snapshot();
        cx.notify();
    }

    pub(crate) fn toggle_voice_deaf(&mut self, cx: &mut Context<Self>) {
        self.backend.toggle_voice_deaf();
        self.voice = self.backend.voice_snapshot();
        cx.notify();
    }

    pub(crate) fn toggle_screen_share(&mut self, include_audio: bool, cx: &mut Context<Self>) {
        if self.voice.self_stream {
            self.backend.stop_screen_share();
            self.error = None;
        } else {
            match self.backend.start_screen_share(include_audio) {
                Ok(()) => self.error = None,
                Err(err) => self.error = Some(l10n::tr(err).into()),
            }
        }
        self.voice = self.backend.voice_snapshot();
        cx.notify();
    }

    pub(crate) fn toggle_screen_share_audio(&mut self, cx: &mut Context<Self>) {
        if !self.voice.self_stream {
            return;
        }
        match self.backend.start_screen_share(!self.voice.screen_share_audio) {
            Ok(()) => self.error = None,
            Err(err) => self.error = Some(l10n::tr(err).into()),
        }
        self.voice = self.backend.voice_snapshot();
        cx.notify();
    }

    pub(crate) fn toggle_watch_video_track(
        &mut self,
        key: crate::backend::voice::TrackSubscriptionKey,
        cx: &mut Context<Self>,
    ) {
        if self.voice.watched_video_track.as_ref() == Some(&key) {
            self.backend.stop_watching_video();
            self.error = None;
        } else {
            match self.backend.watch_video_track(key) {
                Ok(_) => self.error = None,
                Err(err) => self.error = Some(l10n::tr(err).into()),
            }
        }
        self.voice = self.backend.voice_snapshot();
        self.sync_watched_video_frame();
        cx.notify();
    }

    pub(crate) fn sync_watched_video_frame(&mut self) {
        let Some(track) = self.voice.watched_video_track.as_ref() else {
            self.watched_video_track_sid = None;
            self.watched_video_frame_seq = 0;
            self.watched_video_frame = None;
            return;
        };
        if self.watched_video_track_sid.as_deref() != Some(track.track_sid.as_str()) {
            self.watched_video_track_sid = Some(track.track_sid.clone());
            self.watched_video_frame_seq = 0;
            self.watched_video_frame = None;
        }
        if let Some((gen_id, frame)) = self.backend.latest_video_frame(self.watched_video_frame_seq) {
            self.watched_video_frame_seq = gen_id;
            self.watched_video_frame =
                ui::video_viewer::decoded_frame_to_render_image(&frame).map(|img| (frame.width, frame.height, img));
        }
    }

    pub(crate) fn voice_channel_name(&self, channel_id: Snowflake) -> SharedString {
        let state = self.backend.state();
        state
            .channels
            .get(&channel_id)
            .and_then(|c| c.name.as_deref())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| channel_id.to_string())
            .into()
    }

    pub(crate) fn voice_guild_name(
        &self,
        guild_id: Option<Snowflake>,
        channel_id: Snowflake,
    ) -> Option<SharedString> {
        let state = self.backend.state();
        let resolved_gid = guild_id.or_else(|| state.channels.get(&channel_id).and_then(|c| c.guild_id))?;
        state
            .guilds
            .get(&resolved_gid)
            .map(|g| SharedString::from(g.name.clone()))
    }

    pub(crate) fn voice_member_name(&self, guild_id: Option<Snowflake>, user_id: Snowflake) -> SharedString {
        let state = self.backend.state();
        if let Some(gid) = guild_id
            && let Some(nick) = state
                .members
                .get(&gid)
                .and_then(|m| m.get(&user_id))
                .and_then(|m| m.nick.as_deref())
                .filter(|n| !n.is_empty())
        {
            return nick.to_owned().into();
        }
        if let Some(user) = state.users.get(&user_id) {
            return user.display_name().to_owned().into();
        }
        user_id.to_string().into()
    }

    pub(crate) fn voice_member_avatar_url(
        &self,
        guild_id: Option<Snowflake>,
        user_id: Snowflake,
    ) -> Option<SharedString> {
        let media = self.backend.media()?;
        let state = self.backend.state();
        let user = guild_id
            .and_then(|gid| state.members.get(&gid)?.get(&user_id).map(|m| &m.user))
            .or_else(|| state.users.get(&user_id))
            .or_else(|| state.current_user.as_ref().filter(|u| u.id == user_id))?;
        crate::view_model::avatar_url(media, user).map(Into::into)
    }

    pub(crate) fn select_channel_inner(
        &mut self,
        channel_id: Snowflake,
        auto_load_latest: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let opens_pane = {
            let state = self.backend.state();
            state.can_view_channel(channel_id)
                && state.channels.get(&channel_id).is_some_and(|c| opens_message_pane(c.kind))
        };
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
            self.manual_unread_channel = None;
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
        self.row_groups.borrow_mut().clear();
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
        let oldest = self.rows.borrow().iter().find(|r| r.state == MessageState::Sent).map(|r| r.id);
        let (Some(channel_id), Some(oldest)) = (self.selected_channel, oldest) else {
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
        let can_submit = {
            let state = self.backend.state();
            if editing {
                state.can_view_channel(channel_id)
            } else {
                self.can_send && state.can_send_messages(channel_id)
            }
        };
        if !can_submit {
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
        self.manual_unread_channel = None;
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
        if self.close_floating_popouts(window, cx) {
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

    /// A popout, menu, picker or modal is open.
    fn layer_open(&self) -> bool {
        self.overlay.is_open()
            || self.user_area_popout_open
            || self.guild_header_popout_open
            || self.voice_popout.is_some()
    }

    /// Closes the non-overlay popouts (user area, guild header, voice) and returns
    /// focus to the composer; false when none was open.
    fn close_floating_popouts(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !(self.user_area_popout_open || self.guild_header_popout_open || self.voice_popout.is_some()) {
            return false;
        }
        self.user_area_popout_open = false;
        self.user_area_submenu = None;
        self.guild_header_popout_open = false;
        self.voice_popout = None;
        self.focus_composer(window, cx);
        cx.notify();
        true
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
            MessageAction::Retry => self.retry_failed_message(channel_id, message_id, cx),
            MessageAction::Delete => {
                if target.sent {
                    self.confirm_delete(channel_id, message_id, window, cx);
                } else {
                    self.delete_message(channel_id, message_id, window, cx);
                }
            }
            MessageAction::DeleteNow => self.delete_message(channel_id, message_id, window, cx),
            MessageAction::CopyText | MessageAction::CopyId => {
                message_actions::copy_to_clipboard(action, target, cx);
            }
            MessageAction::SuppressEmbeds => {
                self.backend.toggle_suppress_embeds(channel_id, message_id);
            }
            MessageAction::More => self.open_message_menu(target.clone(), window, cx),
            MessageAction::Forward => self.open_forward_modal(target.clone(), window, cx),
            MessageAction::CreateThread => self.open_create_thread_modal(target.clone(), window, cx),
            MessageAction::Translate => self.translate_message("google_translate", &target.content, window, cx),
            MessageAction::Pin => {
                let pinned = target.pinned;
                let view = cx.weak_entity();
                message_actions::confirm_pin(pinned, target.content.clone(), window, cx, move |_, cx| {
                    let _ = view.update(cx, |v, cx| {
                        v.backend.toggle_pin(channel_id, message_id, !pinned);
                        v.refresh(cx);
                    });
                });
            }
            MessageAction::PinNow => {
                self.backend.toggle_pin(channel_id, message_id, !target.pinned);
                self.refresh(cx);
            }
            MessageAction::Bookmark => {
                self.backend.toggle_bookmark(channel_id, message_id);
                self.refresh(cx);
            }
            MessageAction::MarkUnread => self.mark_message_unread(channel_id, message_id, cx),
            MessageAction::CopyLink => {
                let url = self.backend.message_link(target.guild_id, channel_id, message_id);
                cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(url));
            }
            MessageAction::Speak => {
                if self.tts.is_speaking() {
                    self.tts.stop();
                } else if let Err(err) = self.tts.speak(target.content.to_string()) {
                    self.error = Some(err.into());
                }
                cx.notify();
            }
        }
    }

    pub(crate) fn mark_message_unread(
        &mut self,
        channel_id: Snowflake,
        message_id: Snowflake,
        cx: &mut Context<Self>,
    ) {
        self.cancel_ack();
        let Some(ack_id) = self.backend.mark_message_unread(channel_id, message_id) else {
            return;
        };
        self.manual_unread_channel = Some(channel_id);
        if Some(channel_id) == self.selected_channel {
            self.unread_anchor = Some(ack_id);
        }
        self.refresh(cx);
    }

    pub(crate) fn translate_message(
        &mut self,
        provider_id: &str,
        content: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(url) = message_actions::build_translate_url(provider_id, content) {
            self.launch_external_url(&url, cx);
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

    pub(crate) fn retry_failed_message(&mut self, channel_id: Snowflake, message_id: Snowflake, cx: &mut Context<Self>) {
        if self.backend.retry_failed_message(channel_id, message_id) {
            self.error = None;
            self.refresh(cx);
        }
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
            let opens = opens_message_pane(ch.kind) && state.can_view_channel(channel_id);
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
            self.reveal_jump(channel_id, unread::JumpTarget::Message(message_id), cx);
        } else {
            self.pending_jump = Some((channel_id, unread::JumpTarget::Message(message_id)));
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
            DeepLink::Explore => {
                self.close_overlay(window, cx);
                self.open_explore(cx);
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
            DeepLink::Scene(route) => self.apply_scene_deep_link(route, window, cx),
        }
    }

    pub(crate) fn apply_scene_deep_link(&mut self, route: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let parts: Vec<&str> = route.split('/').filter(|s| !s.is_empty()).collect();
        self.user_area_popout_open = false;
        self.guild_header_popout_open = false;
        self.voice_popout = None;
        if parts.first().copied() != Some("search") && self.search.is_some() {
            self.close_search(window, cx);
        }
        match parts.as_slice() {
            ["settings", tab] => {
                if !matches!(self.overlay, Overlay::Settings(_)) {
                    self.open_settings(window, cx);
                }
                let target_tab = match *tab {
                    "profile" => ui::settings::SettingsTab::Profile,
                    "account" => ui::settings::SettingsTab::Account,
                    "privacy" => ui::settings::SettingsTab::PrivacySafety,
                    "connections" => ui::settings::SettingsTab::Connections,
                    "appearance" => ui::settings::SettingsTab::Appearance,
                    "themes" => ui::settings::SettingsTab::Themes,
                    "notifications" => ui::settings::SettingsTab::Notifications,
                    "chat" => ui::settings::SettingsTab::MessagesMedia,
                    "voice" => ui::settings::SettingsTab::VoiceVideo,
                    "accessibility" => ui::settings::SettingsTab::Accessibility,
                    "language" => ui::settings::SettingsTab::Language,
                    "keybinds" => ui::settings::SettingsTab::Keybinds,
                    "windows" | "desktop" => ui::settings::SettingsTab::WindowsApp,
                    "advanced" => ui::settings::SettingsTab::Advanced,
                    "applications" => ui::settings::SettingsTab::Applications,
                    _ => ui::settings::SettingsTab::Appearance,
                };
                self.select_settings_tab(target_tab, cx);
                true
            }
            ["voice", gid, cid] => {
                let (Some(g), Some(c)) = (Snowflake::parse(gid), Snowflake::parse(cid)) else {
                    return true;
                };
                if !self.backend.state().guilds.contains_key(&g) {
                    return false;
                }
                self.close_overlay(window, cx);
                self.select_place(Place::Guild(g), cx);
                self.voice_stage_expanded = true;
                self.join_voice_channel(c, cx);
                true
            }
            ["voice-split", gid, cid] => {
                let (Some(g), Some(c)) = (Snowflake::parse(gid), Snowflake::parse(cid)) else {
                    return true;
                };
                if !self.backend.state().guilds.contains_key(&g) {
                    return false;
                }
                self.close_overlay(window, cx);
                if self.place != Place::Guild(g) {
                    self.select_place(Place::Guild(g), cx);
                }
                self.join_voice_channel(c, cx);
                self.voice_stage_open = true;
                self.voice_stage_expanded = false;
                cx.notify();
                true
            }
            ["voice-popout", kind] => {
                self.close_overlay(window, cx);
                self.voice_popout = match *kind {
                    "input" => Some(VoicePopoutKind::Input),
                    "output" => Some(VoicePopoutKind::Output),
                    "stream" => Some(VoicePopoutKind::Stream),
                    "details" => Some(VoicePopoutKind::Details),
                    _ => None,
                };
                cx.notify();
                true
            }
            ["voice-leave"] => {
                self.close_overlay(window, cx);
                self.leave_voice_channel(cx);
                true
            }
            ["voice-mute", mode] => {
                let want = *mode == "on";
                if self.voice.self_mute != want {
                    self.toggle_voice_mute(cx);
                }
                true
            }
            ["voice-deafen", mode] => {
                let want = *mode == "on";
                if self.voice.self_deaf != want {
                    self.toggle_voice_deaf(cx);
                }
                true
            }
            ["user-area-popout"] => {
                self.close_overlay(window, cx);
                self.user_area_popout_open = true;
                cx.notify();
                true
            }
            ["profile-self"] => {
                self.close_overlay(window, cx);
                if let Some(uid) = self.user.as_ref().map(|u| u.id) {
                    self.open_profile(uid, self.current_guild_id(), gpui_kit::point(px(340.), px(180.)), window, cx);
                }
                true
            }
            ["guild-header-popout", gid] => {
                let Some(g) = Snowflake::parse(gid) else { return true };
                if !self.backend.state().guilds.contains_key(&g) {
                    return false;
                }
                self.close_overlay(window, cx);
                if self.place != Place::Guild(g) {
                    self.select_place(Place::Guild(g), cx);
                }
                self.guild_header_popout_open = true;
                cx.notify();
                true
            }
            ["channel-settings", gid, cid, tab] => {
                let (Some(g), Some(c)) = (Snowflake::parse(gid), Snowflake::parse(cid)) else {
                    return true;
                };
                if !self.backend.state().guilds.contains_key(&g) {
                    return false;
                }
                self.close_overlay(window, cx);
                if self.place != Place::Guild(g) {
                    self.select_place(Place::Guild(g), cx);
                }
                if *tab == "permissions" {
                    self.open_access(ui::access::Scope::Channel { guild: g, channel: c }, window, cx);
                } else {
                    self.open_edit_channel(c, window, cx);
                }
                true
            }
            ["guild-settings", gid, tab] => {
                let Some(g) = Snowflake::parse(gid) else { return true };
                if !self.backend.state().guilds.contains_key(&g) {
                    return false;
                }
                self.close_overlay(window, cx);
                if self.place != Place::Guild(g) {
                    self.select_place(Place::Guild(g), cx);
                }
                if *tab == "bans" {
                    self.open_guild_bans(window, cx);
                } else {
                    self.open_access(ui::access::Scope::Guild(g), window, cx);
                }
                true
            }
            ["quick-switcher"] => {
                self.open_quick_switcher(window, cx);
                true
            }
            ["emoji-picker"] => {
                self.close_overlay(window, cx);
                self.open_emoji_picker(
                    ui::emoji_picker::sections::PickerTarget::Composer,
                    gpui_kit::point(px(1010.), px(730.)),
                    window,
                    cx,
                );
                true
            }
            ["search"] => {
                self.close_overlay(window, cx);
                if self.search.is_none() {
                    self.toggle_search(window, cx);
                }
                true
            }
            ["theme", kind] => {
                let Some(kind) = ThemeKind::parse(kind) else {
                    return true;
                };
                self.close_overlay(window, cx);
                theme::set_kind(kind, cx);
                theme::apply(kind, window, cx);
                cx.notify();
                true
            }
            ["close-overlays"] => {
                self.close_overlay(window, cx);
                if self.search.is_some() {
                    self.close_search(window, cx);
                }
                cx.notify();
                true
            }
            _ => true,
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

    pub(crate) fn next_moderation_seq(&mut self) -> u64 {
        self.moderation_seq = self.moderation_seq.saturating_add(1);
        self.moderation_seq
    }

    pub(crate) fn next_forward_seq(&mut self) -> u64 {
        self.forward_seq = self.forward_seq.saturating_add(1);
        self.forward_seq
    }

    pub(crate) fn next_thread_seq(&mut self) -> u64 {
        self.thread_seq = self.thread_seq.saturating_add(1);
        self.thread_seq
    }

    pub(crate) fn next_invite_seq(&mut self) -> u64 {
        self.invite_seq = self.invite_seq.saturating_add(1);
        self.invite_seq
    }

    pub(crate) fn next_guild_delete_seq(&mut self) -> u64 {
        self.guild_delete_seq = self.guild_delete_seq.saturating_add(1);
        self.guild_delete_seq
    }

    pub(crate) fn next_purge_notes_seq(&mut self) -> u64 {
        self.purge_notes_seq = self.purge_notes_seq.saturating_add(1);
        self.purge_notes_seq
    }

    pub(crate) fn on_header_update_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match &self.update_status {
            crate::updater::UpdateStatus::Available {
                asset_name: Some(_),
                asset_url: Some(_),
                ..
            } => self.download_available_update(cx),
            crate::updater::UpdateStatus::Available { .. } | crate::updater::UpdateStatus::Downloaded { .. } => {
                if !matches!(self.overlay, Overlay::Settings(_)) {
                    self.open_settings(window, cx);
                }
                self.select_settings_tab(ui::settings::SettingsTab::Advanced, cx);
            }
            _ => {}
        }
    }

    pub(crate) fn toggle_hide_muted_channels(&mut self, guild_id: Snowflake, cx: &mut Context<Self>) {
        let hide = !self.backend.state().guild_hides_muted_channels(guild_id);
        self.backend.set_guild_hide_muted_channels(guild_id, hide);
        self.refresh(cx);
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
        self.row_groups.borrow_mut().clear();
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
        self.sync_watched_video_frame();
        let title = self.title_text();
        if title != self.window_title {
            window.set_window_title(&title);
            self.window_title = title;
        }
        ui::composer::sync_placeholder(self, window, cx);
        // An open layer needs focus inside the layout, otherwise key bindings of the
        // `ClientView` context (Escape) are not on the dispatch path.
        if self.layer_open() && window.focused(cx).is_none() {
            let focus = self.focus.clone();
            window.defer(cx, move |window, cx| window.focus(&focus, cx));
        }
        crate::format::set_time_24h(self.prefs.data.time_format_24h);
        let theme = Theme::current(cx);
        let mut right_panel = if self.search.is_some() {
            Some(ui::search_panel::render(self, &theme, window, cx))
        } else if self.shows_member_list() {
            Some(ui::member_list::render(self, &theme, cx))
        } else if self.group_dm_panel_open && self.group_dm.is_some() {
            Some(ui::group_dm::render_panel(self, &theme, cx))
        } else {
            None
        };
        let handle = cx.weak_entity();
        let composer = self.render_composer(&theme, cx);
        let show_voice_stage = self.voice_stage_open
            && self.voice.channel_id.is_some()
            && self.voice.state != crate::backend::voice::VoiceConnectionState::Disconnected;
        let show_friends = !show_voice_stage && self.place == Place::Home && self.selected_channel.is_none();
        let show_explore = !show_voice_stage && self.place == Place::Explore;
        let sidebar_w = self.effective_sidebar_width();
        let split_height = self.effective_voice_call_height(window);
        let overlay = self.render_overlay(&theme, window, cx);
        let main_pane = self.render_main_pane(
            handle,
            composer,
            &mut right_panel,
            show_voice_stage,
            show_explore,
            show_friends,
            split_height,
            &theme,
            cx,
        );
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
            // Official outline frame leaves 2px of app background at the right edge.
            .pr(px(2.))
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_size(px(theme.sizes.text))
            .capture_action(cx.listener(|v, _: &shortcuts::MarkChannelRead, window, cx| {
                if v.is_recording_keybind() {
                    v.settings_cancel_record_keybind(cx);
                    cx.stop_propagation();
                } else if v.close_floating_popouts(window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_any_mouse_down(cx.listener(|v, _, _, _| {
                v.dismissed_expression_tab = None;
                v.dismissed_channel_popout = None;
            }))
            .on_mouse_move(cx.listener(Self::handle_resize_move))
            .on_mouse_up(MouseButton::Left, cx.listener(|v, _, _, cx| v.finish_resize(cx)))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|v, _, _, cx| v.finish_resize(cx)))
            .child(
                // Rail and sidebar run to the bottom; the user area floats
                // over them (official `user-area-inner`: left 6, bottom 7,
                // 381x58 at default 320px sidebar). Rail 72 + 1px gap + sidebar_w.
                div()
                    .relative()
                    .w(px(theme.sizes.rail_width + 1. + sidebar_w))
                    .h_full()
                    .flex_none()
                    .bg(theme.background_secondary)
                    .flex()
                    .flex_row()
                    .child(ui::guild_rail::render(self, &theme, cx))
                    .child(div().w(px(1.)).h_full().flex_none().bg(theme.background_secondary))
                    // `channel-list-content.bottom-spacer` (73px).
                    .child(
                        // The navbar sits inside the 1px frame border at the top and bottom.
                        div().h_full().flex_none().pt(px(1.)).pb(px(1.)).child(
                            div()
                                .h_full()
                                .when(self.place != Place::Explore, |d| d.pb(px(73.)))
                                .flx("app.guild-sidebar.guild-navbar")
                                .child(ui::channel_list::render(self, &theme, cx)),
                        ),
                    ),
            )
            .child(div().flex().flex_1().min_w_0().h_full().bg(theme.background_secondary_lighter).child(main_pane))
            .children(right_panel)
            // `OutlineFrame.module.css`: 1px `rgba(255,255,255,0.035)` border with a 14px
            // top-left radius around everything right of the 72px rail.
            .child(
                div()
                    .absolute()
                    .top(px(0.))
                    .left(px(theme.sizes.rail_width))
                    .right(px(0.))
                    .bottom(px(0.))
                    .rounded_tl(px(14.))
                    .border_t_1()
                    .border_l_1()
                    .border_color(if theme.background.l < 0.5 {
                        gpui_kit::hsla(0., 0., 1., 0.035)
                    } else {
                        theme.border
                    })
                    .flx("app.outline-frame.frame"),
            )
            // Keep the bottom-left user card and its popout above the outline frame's 1px left border.
            .child(
                div()
                    .absolute()
                    .left(px(6.))
                    .bottom(px(7.))
                    // Height is content-sized so the popout grows upward.
                    .w(px(theme.sizes.rail_width + 1. + sidebar_w - 12.))
                    .child(ui::user_panel::render(self, &theme, cx)),
            )
            .children(ui::user_panel::render_popout(self, &theme, cx))
            .child(self.render_sidebar_resize_handle(sidebar_w, theme.sizes.rail_width, cx))
            .children(overlay)
    }
}

impl ClientView {
    #[inline(never)]
    fn render_overlay(&self, theme: &Theme, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        match &self.overlay {
            Overlay::QuickSwitcher(sw) => Some(ui::quick_switcher::render(sw, theme, cx)),
            Overlay::EmojiPicker(picker) => {
                Some(ui::emoji_picker::render(self, picker, self.backend.media(), window.viewport_size(), theme, cx))
            }
            Overlay::ChannelPopout(pop) => {
                Some(ui::channel_popouts::render(self, pop, window.viewport_size(), theme, cx))
            }
            Overlay::Profile(pop) => Some(ui::profile_popout::render(pop, theme, cx)),
            Overlay::Settings(modal) => Some(ui::settings::render(self, modal, theme, cx)),
            Overlay::AccountSwitcher(modal) => Some(ui::account_switcher::render(self, modal, theme, cx)),
            Overlay::JoinGuild(modal) => Some(ui::guild_modal::render_join(modal, theme, cx)),
            Overlay::LeaveGuild(modal) => Some(ui::guild_modal::render_leave(modal, theme, cx)),
            Overlay::CreateGroupDm(modal) => {
                let query = modal.filter_input.read(cx).value().to_string();
                Some(ui::group_dm::render_create_modal(modal, theme, cx, &query))
            }
            Overlay::LeaveGroupDm(modal) => Some(ui::group_dm::render_leave_modal(modal, theme, cx)),
            Overlay::MediaViewer(modal) => Some(ui::media_viewer::render(modal, theme, window, cx)),
            Overlay::LinkWarning(modal) => Some(ui::link_warning::render(modal, theme, cx)),
            Overlay::CreateChannel(modal) => Some(ui::channel_modal::render_create(modal, theme, cx)),
            Overlay::EditChannel(modal) => Some(ui::channel_modal::render_edit(modal, theme, cx)),
            Overlay::MemberModeration(modal) => Some(ui::moderation_modal::render_action(modal, theme, cx)),
            Overlay::GuildBans(modal) => Some(ui::moderation_modal::render_bans(modal, theme, cx)),
            Overlay::Access(modal) => Some(ui::access::render(self, modal, theme, cx)),
            Overlay::ForwardMessage(modal) => Some(ui::forward_modal::render(self, modal, theme, cx)),
            Overlay::CreateThread(modal) => Some(ui::create_thread_modal::render(modal, theme, cx)),
            Overlay::InviteMembers(modal) => Some(ui::invite_modal::render(self, modal, theme, cx)),
            Overlay::GuildNotifications(modal) => {
                Some(ui::guild_settings_modals::render_notifications(self, modal, theme, cx))
            }
            Overlay::GuildPrivacy(modal) => Some(ui::guild_settings_modals::render_privacy(self, modal, theme, cx)),
            Overlay::GuildProfile(modal) => Some(ui::guild_settings_modals::render_profile(modal, theme, cx)),
            Overlay::DeleteMyGuildMessages(modal) => {
                Some(ui::guild_settings_modals::render_delete_messages(modal, theme, cx))
            }
            Overlay::PurgePersonalNotes(modal) => {
                Some(ui::guild_settings_modals::render_purge_personal_notes(modal, theme, cx))
            }
            Overlay::DiscoveryPreview(modal) => Some(ui::explore::render_preview_modal(self, modal, theme, cx)),
            Overlay::None => None,
        }
    }

    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    fn render_main_pane(
        &mut self,
        handle: gpui_kit::WeakEntity<Self>,
        composer: AnyElement,
        right_panel: &mut Option<AnyElement>,
        show_voice_stage: bool,
        show_explore: bool,
        show_friends: bool,
        split_height: f32,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if show_voice_stage {
            let split_chat = !self.voice_stage_expanded && self.selected_channel.is_some();
            if split_chat {
                div()
                    .debug_selector(|| "voice-stage-split-chat".to_owned())
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .on_drop(cx.listener(|v, paths: &ExternalPaths, _, cx| v.add_paths(paths.paths(), cx)))
                    .child(ui::voice_stage::render(self, theme, split_height, cx))
                    .child(ui::message_list::render(self, handle, theme))
                    .child(composer)
                    .into_any_element()
            } else {
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(ui::voice_stage::render(self, theme, split_height, cx))
                    // Keep composer mounted in a 0px hidden slot so shortcut contexts stay live.
                    .child(div().h(px(0.)).overflow_hidden().child(composer))
                    .into_any_element()
            }
        } else if show_explore {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .h_full()
                .pt(px(1.))
                .pb(px(1.))
                .child(ui::explore::render_page(self, theme, cx))
                // Keep the composer mounted in a 0px hidden slot so shortcut contexts stay live.
                .child(div().h(px(0.)).overflow_hidden().child(composer))
                .into_any_element()
        } else if show_friends {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .h_full()
                .child(ui::friends::render(self, theme, cx))
                // Keep the composer mounted in a 0px hidden slot so the
                // `ClientView > Composer > Input` shortcut context stays live.
                .child(div().h(px(0.)).overflow_hidden().child(composer))
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .h_full()
                // OutlineFrame.module.css: the pane sits inside the 1px frame border.
                .pt(px(1.))
                .pb(px(1.))
                // Files dropped anywhere on the message pane go to the tray.
                .on_drop(cx.listener(|v, paths: &ExternalPaths, _, cx| v.add_paths(paths.paths(), cx)))
                .child(ui::header::render(self, theme, cx))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_1()
                        .min_h_0()
                        .w_full()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .when_some(self.voice.watched_video_track.as_ref(), |d, track| {
                                    let label = ui::video_viewer::participant_display_name(self, track);
                                    d.child(ui::video_viewer::render(
                                        track,
                                        label,
                                        self.watched_video_frame.clone(),
                                        theme,
                                        cx,
                                    ))
                                })
                                .when(!self.guild_unavailable && self.selected_channel.is_some() && self.unread_anchor.is_some(), |d| {
                                    d.child(self.render_unread_bar(theme, cx))
                                })
                                .child(ui::message_list::render(self, handle, theme))
                                .when(self.has_more_after && self.selected_channel.is_some(), |d| {
                                    d.child(self.render_jump_to_present(theme, cx))
                                })
                                .child(composer),
                        )
                        .children(right_panel.take()),
                )
                .into_any_element()
        }
    }

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
mod unread;

impl Drop for ClientView {
    fn drop(&mut self) {
        self.backend.close();
    }
}
