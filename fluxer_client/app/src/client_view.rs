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
    /// Recent frame arrival timestamps (2s sliding window) for live FPS calculation.
    pub(crate) watched_video_frame_times: std::collections::VecDeque<std::time::Instant>,
    /// Computed live FPS of the currently watched remote video stream.
    pub(crate) watched_video_fps: u32,
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
            watched_video_frame_times: std::collections::VecDeque::new(),
            watched_video_fps: 0,
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
                | Overlay::ScreenSharePicker(_)
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
                    .border_color(theme.user_area_divider)
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

mod ack_updates;
mod deep_links;
mod events;
mod keybinds;
mod keyboard;
mod messages;
mod navigation;
mod notify_upload;
mod render;
mod resize;
mod unread;
mod voice;

impl Drop for ClientView {
    fn drop(&mut self) {
        self.backend.close();
    }
}
