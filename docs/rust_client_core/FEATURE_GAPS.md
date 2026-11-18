# Rust client feature gaps vs web client (2026-11-18)

Scope: `fluxer_app/src/features/*` (TS/React) vs `fluxer_client/` (core: gateway/state/rest; app: GPUI; devserver for tests).
Evidence is from reading code; nothing here was executed. TS line counts are approximate (`wc -l` of the folder).
Sizes: S = under 1 day, M = 1-3 days, L = a week or more.

## 0. What exists in Rust today

Core (`fluxer_client/core/src`)
- Gateway: connect, zstd, heartbeat, IDENTIFY/RESUME, reconnect (`gateway/connection.rs`, 688 lines). Outgoing helpers: `send_raw`, `lazy_request` (op 14), `update_presence` (op 3) at `gateway/mod.rs:137-150`. No VOICE_STATE_UPDATE helper.
- REST: client with token and installation id (`rest/client.rs`), login incl. ALTCHA auto-solve and TOTP (`rest/auth.rs:73-133`; webauthn/passkey are stubs, `:119`, `:124`), instance discovery (`rest/discovery.rs`). Only 5 API calls: `get_me`, `get_messages`, `send_message`, `ack_message(s)` (`rest/api.rs:35-82`). Everything else needs new endpoints.
- State dispatch handles only: READY, GUILD_CREATE/SYNC/UPDATE/DELETE, GUILD_ROLE_*, GUILD_MEMBER_ADD/UPDATE/REMOVE, CHANNEL_CREATE/UPDATE/UPDATE_BULK/DELETE, MESSAGE_CREATE/UPDATE/DELETE/DELETE_BULK/ACK, USER_UPDATE, PASSIVE_UPDATES (`state/client.rs:143-168`). Everything else is dropped (no TYPING_START, PRESENCE_UPDATE, reactions, relationships, user settings, guild settings, voice).
- State: guilds, channels, roles, overwrites, members, permissions (`state/permissions.rs`), read states and unread/anchor (`state/client.rs:734-784`), per-channel message windows with pending/failed sends (`state/messages.rs`). Message model is a subset: no reactions, stickers, embeds beyond title/description/url (`state/model.rs:397-406`), reference without referenced message body (`:409`).

App (`fluxer_client/app/src`, GPUI + gpui-kit, ~3.2k lines)
- Login/instance picker/TOTP (`login.rs`; captcha-required fallback message at `:294`), guild rail and DM home, channel tree, message list with grouping, markdown via `TextView::markdown` (`client_view.rs:568`), basic mention rewriting (`format.rs:31`), system messages (`format.rs:293`), image attachment sizing (`format.rs:188`), unread badges, ack, load older, plain-text send with optimistic pending (`backend.rs:193`), composer (`client_view.rs:63`).
- Nothing else: no settings, no profiles, no members pane, no voice, no search.

## 1. Feature-area inventory

### 1.1 Messaging composer
- Plain send: DONE (`backend.rs:193`, `rest/api.rs:61`, `state/messages.rs:202`).
- Replies: MISSING. `send_message` has no `message_reference` (`rest/api.rs:61`). TS: `messaging/commands/MessageCommands.tsx` (1258), `channel/components/channel_textarea/*`, `messaging/hooks/useMessageSubmission.tsx`, `useTextareaSubmit.ts`. Core needs reply param and referenced_message in model. S-M.
- Edit / delete own message: MISSING. TS `MessageCommands.tsx`, `channel/components/ChannelEditBar.tsx`, `useTextareaEditing.ts`. REST PATCH/DELETE missing; state already applies MESSAGE_UPDATE/DELETE. S-M.
- Attachment upload: MISSING. TS `messaging/upload/CloudUpload.ts` (603), `ChunkedAttachmentUploader.ts` (195), `AttachmentUploadPlan.ts` (247), `channel/components/ChannelAttachmentArea.tsx`. Needs REST (attachment slots, chunked PUT, send with attachments) and file picker/drag-drop in GPUI. L.
- Emoji picker and custom emoji: MISSING. TS `features/expressions/components` (3.7k), `emoji/` (2.8k), `useTextareaExpressionPicker.tsx`. Needs guild emoji in state (GUILD_EMOJIS_UPDATE not handled) and unicode emoji data. M-L.
- Mention autocomplete (@user/@role/#channel/:emoji:): MISSING. TS `channel/components/Autocomplete*.tsx` (about 15 files), `search/state/Autocomplete.ts`. State has members/roles/channels; member lazy list needed for big guilds. M.
- Slash commands: MISSING (low priority for self-host chat). TS `AutocompleteCommand*.tsx`, `channel/components/commands`. M-L.
- Drafts, typing send, markdown keybinds: MISSING. TS `messaging/commands/DraftCommands.ts`, `useTextareaDraftAndTyping.ts`, `useMarkdownKeybinds.ts`. Typing send needs `POST /channels/{id}/typing`. S.
- Stickers/GIF picker: MISSING. TS `expressions/`, `AutocompleteGif.tsx`, `AutocompleteSticker.tsx`. M, optional.

### 1.2 Message rendering
- Markdown: PARTIAL. Uses gpui-kit markdown, not the Fluxer parser. TS `messaging/components/markdown` (3.2k; `renderers/*`: Emoji, Link, Mention, Timestamp, etc.). Missing: spoilers, timestamps, custom emoji, channel/role mention chips, jump links. `format.rs:31` rewrites only mentions to links. M-L. See Q1.
- Code highlighting: MISSING. TS `features/code_highlighting` (1.8k). Rust: syntect or tree-sitter. S-M.
- Embeds / link previews: MISSING (title/description/url parsed, never rendered). TS `messaging/components/embeds/embed_card`, `ActiveIframeEmbed.ts`. Needs full embed model (author, fields, image, thumbnail, video, footer, color). M.
- Attachments display: PARTIAL (image fit only, `format.rs:188`). Missing video/audio/file chips, download, media viewer, spoiler/NSFW. TS `messaging/hooks/useMedia*`, `useImageRecovery.ts`. M.
- Reactions: MISSING (no model, no events). TS `messaging/commands/ReactionCommands.tsx` (362), `events/MessageReaction*.ts`, `useMessageReactionsState.ts`. Core: 5 reaction events and REST PUT/DELETE. M.
- Reply preview, pins, forwards, message actions (copy, pin, copy link, mark unread): MISSING. TS `MessageCommands.tsx`, `messaging/components/popouts`. M.
- Blocked-message collapse: MISSING. TS `channel/components/BlockedMessageGroups.tsx`. S (needs relationships).

### 1.3 Navigation
- Channel/guild nav, unread badges: DONE basic (`view_model.rs`, `state/client.rs:734-784`). Missing: unread jump bar, jump to first unread, jump to message (windows not ported, `state/messages.rs:3`), category collapse, guild folders, ordering by user settings, mute state. TS `navigation/*` (1.5k), `read_state/state/ReadStates.ts` (1114). M.
- Quick switcher (Ctrl+K): MISSING. TS `search/components/quick_switcher/QuickSwitcherModal.tsx` (656), `search/state/QuickSwitcher*.ts` (9 files). Client-side over state; no new core. M.
- Deep links (`fluxer://`, invite URLs): MISSING. TS `navigation/utils/DeepLinkUtils.ts` (377). S.

### 1.4 Member list and presence
- Member list pane: MISSING (members stored, `state/model.rs:263`; no UI). TS `channel/components/MemberListContainer.tsx`, `MemberListGroupHeader.tsx`, `member/*` (5k), `presence/state/MemberPresenceSubscription.ts` (217). Needs LAZY_REQUEST member-list ranges (op 14 helper exists, `gateway/mod.rs:144`; GUILD_MEMBER_LIST_UPDATE not handled). M-L.
- Presence (status dots, custom status): MISSING. No PRESENCE_UPDATE handling; READY presences not parsed. TS `presence/state/Presence.ts` (658), `LocalPresence.ts` (212). Own status: `update_presence` exists in gateway. M.

### 1.5 Typing indicators
- MISSING both directions. TS `typing/rolling/*` (about 450), `channel/components/TypingUsers.tsx` (147), `messaging/events/TypingStart.ts`. Core: TYPING_START with timeout map and REST typing POST. S.

### 1.6 Notifications and sounds
- MISSING entirely: no desktop notifications, no sounds, no mute/notification settings (user guild settings not parsed from READY). TS `notification/*` (3.1k: `NotificationUtils.ts` 407, `SoundUtils.ts` 585, `MessageNotificationPreview.ts`, `MentionFeed.ts`), `platform/notifications`. Core: user_guild_settings in state and notify-level logic. M-L (OS toast, audio via rodio).

### 1.7 User settings
- MISSING. TS `features/user` (62k), `user/components/modals/UserSettingsModal.tsx`, `settings_utils` (9.8k), `theme_settings` (10k), `theme` (9k), `accessibility` (2.6k), `streamer_mode`. Core needs USER_SETTINGS_UPDATE, `GET/PATCH /users/@me/settings`, READY `user_settings`. Slices: (a) settings store and REST in core, (b) appearance/theme (`theme/styles`, `theme/variables`) and density/font size, (c) notification settings, (d) privacy/account (`user/components/modals/*`, `auth` 38k). Daily-driver minimum: theme, notification, sign out. M per slice.

### 1.8 Profiles and friends
- MISSING. No relationships in state; READY relationships ignored. TS `relationship/*` (1.5k; `RelationshipActionUtils.tsx` 355, `FriendsListUtils.tsx` 275), `user/components/popouts`, `user/components/profile`, `user/commands/UserProfileCommands.tsx`, `UserNoteCommands.ts`. Core: RELATIONSHIP_ADD/UPDATE/REMOVE, REST friend/block endpoints, `GET /users/{id}/profile`. Friends list M, profile popout M.

### 1.9 DMs and group DMs
- PARTIAL. DM channels listed (`state/client.rs:716`); messaging works as in any channel. Missing: open/create DM (`POST /users/@me/channels`), group DM create/add/remove, DM close, group names/avatars, call UI. TS `channel/components/channel_view/DMChannelView.tsx`, `channel/commands`, `channel/engine`. M.

### 1.10 Guild management
- MISSING (read-only mirroring only). TS `guild/*` (26k), `channel/*` (76k, mostly UI), `invite/*` (3.5k), `moderation/*` (5.6k), `permissions/*` (2.9k), `webhook`, `threads` (4.4k), `forum` (4.3k). Needs: join via invite (`POST /invites/{code}`) S-M, leave guild S, create/edit channels M, role editor M-L, kick/ban/timeout M, threads/forum L (channel kinds already parsed, `model.rs:132-147`). Daily-driver minimum: join invite, leave guild, create invite, mute channel. Admin UI can stay in the web client.

### 1.11 Voice and screen share
- MISSING. TS `features/voice` (98k, LiveKit), `connection/`. Rust precedent: `fluxer_desktop_rs` and `tools/h265_livekit_probe`. Core needs VOICE_STATE_UPDATE (op 4), VOICE_STATE/VOICE_SERVER events, `GET /channels/{id}/call`, LiveKit client, audio device I/O. L-XL, separate track.

### 1.12 Search
- MISSING. TS `search/components/search/*`, `search/state/SearchEngine.ts`, `channel/components/channel_search_results`, `channel/components/channel_view/useChannelSearchState.ts`. REST guild and DM message search. Result rendering reuses message row. M.

### 1.13 i18n
- MISSING; UI strings are hard-coded English. TS uses Lingui; 44 locales at `fluxer_app/src/features/i18n/locales/<xx>/messages.po`. Rust options: parse `.po` at build time or use `fluent`. Recommend a `t!()` wrapper now so new UI strings are keyed, then load `.po`. Lingui ICU plurals need an ICU-capable formatter. M, best started before more UI lands.

### 1.14 Accessibility
- MISSING/unknown. TS `features/accessibility` (2.6k: reduced motion, high contrast, font scaling, saturation). Screen-reader labels and focus order depend on gpui-kit/AccessKit support. M, research first.

### 1.15 Keyboard shortcuts
- MISSING beyond composer enter-to-send. TS `input/*` (4.6k), `useMessageListKeyboardNavigation.ts`, Ctrl+K, Alt+Up/Down channel nav, Esc to mark read. S-M (GPUI actions and keymap).

### 1.16 Desktop integration
- MISSING. TS `platform/*` (20k), `updater/*` (543), `window/*` (358). `fluxer_desktop_rs/src` already has tray, autostart, deep links, updater logic for the Electron replacement; the new client has none. Needs tray and autostart crates, single instance, `fluxer://` registration, update check. M each, Windows first.

### 1.17 Misc / lower priority
- Premium, gift, inbox/mention feed (`notification/state/MentionFeed.ts`), saved messages, public guild discovery (`discovery` 2.2k), trusted-domain link warning (`trusted_domain` 112, S, security relevant), devtools, experiments, terms. Defer.

## 2. Core gaps that block many UI tasks (do first, one owner each)

All in `fluxer_client/core/src`:
1. `rest/api.rs` is one file; split into `rest/api/` modules (messages, channels, guilds, users, relationships, invites, search) so workers add endpoints without conflicts.
2. `state/model.rs` message model: add reactions, referenced_message, stickers, full embed, emoji; add Presence, Relationship, UserSettings, UserGuildSettings, Emoji structs (serialised before other state work).
3. `state/client.rs` dispatch match (`:143`) is the single hot file; move to one handler per module (`state/typing.rs`, `presence.rs`, `relationships.rs`, `reactions.rs`, `settings.rs`) with a one-line registration each.
4. Gateway: VOICE_STATE_UPDATE helper and GUILD_MEMBER_LIST_UPDATE parsing.
5. devserver fixtures (`devserver/src/rest.rs`, `gateway.rs`, `fixture.rs`) must grow with each endpoint so `devserver/tests/e2e.rs` stays valid; one owner per file.

## 3. Suggested worker split (file-disjoint)

| Worker | Owns | Depends on |
|---|---|---|
| W1 core-REST | `core/src/rest/api/*` split plus edit/delete/reply/typing/reaction/invite/DM-create/relationship endpoints, devserver `rest.rs` | none |
| W2 core-state | `core/src/state/` model, typing, presence, relationships, reactions, settings, emoji modules; dispatch registration in `client.rs`; devserver `gateway.rs`, `fixture.rs` | none (agree type names with W1) |
| W3 app-composer | `app/src/composer/*`: reply bar, edit bar, drafts, typing send, mention autocomplete | W1, W2 |
| W4 app-message-render | `app/src/message/*`: embeds, attachments, reactions, spoilers, code highlight, reply preview (extract from `render_row`, `client_view.rs:546`) | W2 |
| W5 app-nav | `app/src/quick_switcher.rs`, `shortcuts.rs`, unread-jump bar, deep links | none |
| W6 app-members | `app/src/members.rs` (list pane, presence dots), typing line | W2, gateway lazy list |
| W7 app-settings | `app/src/settings/*` (theme, notifications, account) | W1, W2 |
| W8 app-social | `app/src/friends.rs`, `profile.rs`, DM create | W1, W2 |
| W9 platform | `app/src/platform/*`: tray, autostart, notify, sound, update | none |
| W10 i18n | new crate `fluxer_client/i18n`, `.po` loader, `t!` macro | none |
| W11 search | `app/src/search/*`, `core/src/rest/api/search.rs` | W1 |
| W12 voice | new crate `fluxer_client/voice` | gateway op 4 |

Single-owner (lead) files: `app/src/client_view.rs` (670 lines; split into panes first), `app/src/main.rs`, `core/src/state/client.rs`, `core/src/lib.rs`.

## 4. Top-15 backlog for a usable daily driver

1. Refactor `client_view.rs` into pane modules; split `rest/api.rs` and `state/client.rs` dispatch (unblocks parallel work). S-M, lead.
2. Typing indicators (TYPING_START, REST typing, line under composer). S.
3. Message edit/delete own and reply (composer bars, REST). M.
4. Reactions (state events, REST, render, quick add). M.
5. Member list pane with presence (lazy list op 14, PRESENCE_UPDATE, own status). M-L.
6. Mention/emoji autocomplete and custom emoji rendering. M.
7. Attachment upload (picker, drag-drop, progress) and non-image attachment display. L.
8. Embeds/link previews and trusted-domain link warning. M.
9. Notifications: mute/notify levels from user guild settings, mention badges, OS toast, sounds. M-L.
10. Quick switcher (Ctrl+K) and keyboard shortcuts. M.
11. DM create/open, friends list, profile popout, block. M.
12. User settings core, theme (light/dark), sign-out/account basics. M.
13. Invite join/create, leave guild, deep links, channel mute. M.
14. Desktop integration: tray, autostart, single instance, update check (Windows first). M.
15. Search (guild and DM) and jump-to-message (needs around-load in `state/messages.rs`, noted at `:3`). M-L.

Deferred: voice/screen share (L-XL track), i18n full locale load (start `t!` early, cheap), accessibility research, threads/forum, guild admin UIs, premium/gift, slash commands, stickers/GIFs, passkey/webauthn login (`rest/auth.rs:119,124`).

## 5. Open questions
- Q1 Markdown: keep gpui-kit `TextView::markdown` (limits spoilers, custom emoji, timestamps, chips) or port the Fluxer parser (`messaging/components/markdown`; `lexical` 14k is editor-side only)? Decide before W4.
- Q2 Are API shapes (settings, relationships, upload slots, search) stable on the target self-hosted version? Verify against the server routes, not only TS callers.
- Q3 Does gpui-kit expose a rich-text input (mention chips, inline emoji) or only plain `InputState`? Sets W3 scope.
- Q4 i18n source of truth: reuse Lingui `.po` msgids or new Rust keys.
- Q5 Voice: reuse `fluxer_desktop_rs` LiveKit code or standalone.
