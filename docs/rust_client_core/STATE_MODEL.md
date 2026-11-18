# Fluxer 클라이언트 상태 모델 명세 (Rust 코어 재구현용)

- 기준: 저장소 루트, 브랜치 `desktop-rs`, HEAD `d1124083`. dirty 파일은 읽기만 했다.
- 경로 약어: `GW` = `fluxer_app/src/features/gateway`, `SNAP` = `GW/snapshot`, `MSG` = `fluxer_app/src/features/messaging`, `PKG` = `packages`.
- 표기: 관찰 = 소스에서 직접 확인, 추정 = 코드 구조에서 유추(검증 안 함). 실행과 빌드는 하지 않았으므로 모든 항목은 소스 정독 근거이다.

## 0. 구조 요약 (가장 중요한 발견)

1. 포그라운드 UI의 진실 공급원은 MobX 스토어이며, 스냅샷 리듀서는 디스크 영속과 백그라운드 계정용 그림자 사본이다. 디스패치마다 핸들러(MobX)를 먼저 실행하고 그 뒤에 `SnapshotSync.applyDispatch`를 호출한다. `GW/transport/ForegroundGatewayDispatchProcessor.ts:60-97`.
2. 메시지 본문은 스냅샷에 들어가지 않는다. `SnapshotDispatchEvent`에는 `MESSAGE_CREATE`와 `MESSAGE_ACK`만 있고 이 둘도 채널 `last_message_id`와 read_state만 갱신한다 (`SNAP/SnapshotDispatchEvent.ts:137-187`, `SNAP/SnapshotChannelReducer.ts:354-387`). 메시지 윈도우는 `MSG/state/ChannelMessages.ts`(메모리)와 별도 boot_state 바이너리(`fluxer_app/src/features/boot_state/BootStateCodec.ts`)가 담당한다.
3. Rust 코어는 "스냅샷 리듀서(행 연산 방출)"와 "메시지 윈도우 캐시"를 별도 모듈로 만들고 같은 이벤트 스트림을 소비하게 하면 TS 구조와 1:1로 대응한다.
4. 구 `fluxer_desktop_rs`의 `account_engine.rs`는 리듀서가 아니라 IPC 요청 라우터 겸 SQLite 래퍼이다(6절). 상태 모델은 새 코어 `fluxer_client/core/src/state`에 구현되어 있다(마지막 “Rust implementation map” 절).

## 1. 엔티티 목록과 핵심 필드

### 1.1 영속 행(스냅샷 엔티티) 16종, `SNAP/SnapshotEntities.ts:215-253`

| 엔티티 | 키 | 값 타입 | 근거 |
|---|---|---|---|
| `guild` | guildId | `{id, properties(Guild에서 roles 제외), joined_at, member_count, online_count?, threads_active?}` | `:172-179` |
| `unavailable_guild` | guildId | `{id, unavailable:true}` | `:181-184` |
| `guild_role` | `guildId:roleId` | `GuildRole` | `:77-81` |
| `channel` | channelId | wire `Channel` 그대로 | `:219` |
| `read_state` | channelId | `{ackMessageId, ackPinTimestamp(ms), mentionCount, serverVersion, readStateKnown, lastMessageId, guildId}` | `:186-194` |
| `user_settings` | `@me` | `UserSettingsResponse` | `:221` |
| `account_metadata` | `@me` | `{authSessionIdHash, staticClientSessionId, countryCode, pinnedDmIds, favoriteMemes, rtcRegions, webAuthnCredentials, notes}` | `:204-213` |
| `user_guild_settings` | guildId 또는 `@me` | `GatewayGuildSettings` | `:93-95` |
| `guild_emoji`, `guild_sticker` | guildId | 배열 | `:102-110` |
| `guild_member` | `guildId:userId` | `GuildMemberData`(guild_id 제거) | `:112-116`, `SNAP/SnapshotReducer.ts:594` |
| `relationship` | userId | `RelationshipWire` | `:118` |
| `user` | userId | `User`, `UserPrivate`, `UserPartial` 중 하나 | `:170` |
| `user_connections` | `@me` | `ConnectionListResponse` | `:229` |
| `presence` | userId | `{status, customStatus, afk, mobile, guildIds[]}`(`guildIds`에 `ME` 센티넬 포함) | `:196-202` |
| `voice_state` | guildId | `VoiceState[]` | `:231` |

키 규칙: 복합 키는 콜론 구분이며 각 부분에 콜론과 빈 문자열을 금지한다 (`:58-66`, `:155-166`). 행 값은 JSON 문자열이다 (`parseSnapshotEntity :311-324`).

### 1.2 User (wire)
- `UserPartial`: `id, username, discriminator, global_name, avatar, avatar_color, bot?, system?, flags, mention_flags?` (`PKG/schema/src/domains/user/UserResponseSchemas.ts:54-74`).
- `UserPrivate`: Partial에 `email, bio, pronouns, accent_color, banner, banner_color, mfa_enabled, authenticator_types, is_staff, acls, traits, timezone` 등을 확장한다 (`:78-103`). 셀프 사용자 판별은 `email` 필드 존재 여부이다 (`SNAP/SnapshotHydration.ts:143-160`).

### 1.3 Guild, Role, 권한 비트
- Guild 속성: `id, name, icon, banner, splash, owner_id, system_channel_id, rules_channel_id, afk_*, features, verification_level, mfa_level, nsfw*, explicit_content_filter, default_message_notifications` 등 (`PKG/schema/src/domains/guild/GuildResponseSchemas.ts:143-191`).
- READY 길드 페이로드 `GuildReadyData`: `id, properties, channels, threads?, emojis, stickers?, members, member_count, online_count?, presences?, voice_states?, roles, joined_at, unavailable?, visiting?, degraded?` (`GW/types/GatewayGuildTypes.ts`).
- Role: `id, name, color, position, hoist_position, permissions(문자열 비트필드), hoist, mentionable, icon, unicode_emoji` (`PKG/schema/src/domains/guild/GuildRoleSchemas.ts:9-20`). @everyone 역할 id는 guildId와 같다 (`fluxer_app/src/features/permissions/utils/PermissionUtils.ts:150-151`).
- 권한 비트(전체 목록 `PKG/constants/src/ChannelConstants.ts:172-210`, BigInt): `CREATE_INSTANT_INVITE 1<<0, KICK_MEMBERS 1<<1, BAN_MEMBERS 1<<2, ADMINISTRATOR 1<<3, MANAGE_CHANNELS 1<<4, MANAGE_GUILD 1<<5, ADD_REACTIONS 1<<6, VIEW_CHANNEL 1<<10, SEND_MESSAGES 1<<11, MANAGE_MESSAGES 1<<13, EMBED_LINKS 1<<14, ATTACH_FILES 1<<15, READ_MESSAGE_HISTORY 1<<16, MENTION_EVERYONE 1<<17, MODERATE_MEMBERS 1<<40, PIN_MESSAGES 1<<51, VIEW_CHANNEL_MEMBERS 1<<54`. 명시된 최대 비트는 54이므로 `u64`로 충분해 보인다. 다만 스레드 전용 비트(`PKG/constants/src/ThreadPermissionUtils.ts`)는 읽지 않았으므로 확인이 필요하다.
- 기본 권한 `DEFAULT_PERMISSIONS`(`:251-266`)는 VIEW_CHANNEL, SEND_MESSAGES, READ_MESSAGE_HISTORY, ADD_REACTIONS, EMBED_LINKS, ATTACH_FILES 등을 포함한다.

### 1.4 Channel (종류 enum), `PKG/constants/src/ChannelConstants.ts:5-19`
`GUILD_TEXT 0, DM 1, GUILD_VOICE 2, GROUP_DM 3, GUILD_CATEGORY 4, GUILD_ANNOUNCEMENT 5, ANNOUNCEMENT_THREAD 10, PUBLIC_THREAD 11, PRIVATE_THREAD 12, GUILD_FORUM 15, GUILD_MEDIA 16, GUILD_LINK 998, DM_PERSONAL_NOTES 999`.
- `TEXT_BASED_CHANNEL_TYPES`는 {0, 2, 5, 1, 999, 3}이며 음성 채널도 텍스트 기반으로 취급한다 (`:23-33`). 스레드 집합은 {10, 11, 12}, 스레드 전용 부모는 {15, 16}이다 (`PKG/constants/src/ThreadConstants.ts:5-17`).
- 필드(`PKG/schema/src/domains/channel/ChannelSchemas.ts:51-113`): `id, guild_id?, name, topic, icon, owner_id, type, position, parent_id, bitrate, user_limit, rtc_region, last_message_id, last_pin_timestamp, permission_overwrites[{id,type 0 또는 1,allow,deny}], recipients[UserPartial], nsfw, rate_limit_per_user, nicks`. 스레드 필드는 `thread_metadata, applied_tags, message_count, member_count, member` (`ThreadSchemas.ts:62-79`).
- READY에서 채널은 `private_channels`, 각 길드의 `channels`(guild_id 주입), `threads`(guild_id 주입)를 평탄화한다 (`GW/events/GatewayReady.ts:127-141`, `SNAP/SnapshotChannelReducer.ts:125-144`).

### 1.5 Member
`GuildMemberData`: `user(UserPartial), nick, avatar, banner, accent_color, roles[], joined_at, mute, deaf, communication_disabled_until, profile_flags, mention_flags` (`PKG/schema/src/domains/guild/GuildMemberSchemas.ts`). 타임아웃 판정은 `communication_disabled_until > now`이다 (`fluxer_app/src/features/member/models/GuildMember.ts:133-138`).
READY의 `members`는 일부 구성원만 담고 음성 상태의 member를 병합한다 (`SNAP/SnapshotReducer.ts:580-592`). 전체 목록은 `GUILD_MEMBERS_CHUNK`와 `GUILD_MEMBER_LIST_UPDATE`로 온다(후자의 스냅샷 처리는 카운트 갱신뿐).

### 1.6 Message (wire, `MessageResponseSchema`), `PKG/schema/src/domains/message/MessageResponseSchemas.ts:122-180`
- 본문: `id, channel_id, author(UserPartial), webhook_id?, type, flags, content, timestamp, edited_timestamp, pinned, mention_everyone, tts, mentions[UserPartial], mention_roles[], mention_channels, nonce, call?, thread?, referenced_message?`.
- 첨부 `attachments[]`: `id, filename, title, description, content_type, size, url, proxy_url, width, height, placeholder, flags, duration, waveform, expires_at, expired` (`:23-46`).
- 임베드 `embeds[]`(`EmbedSchemas.ts`), 스티커 `stickers[{id,name,animated}]`, 전달 `message_snapshots[]`, 답장 `message_reference{channel_id,message_id,guild_id,type}`.
- 리액션 요약 `reactions[{emoji{id,name,animated}, count, me}]` (`:61-70`). 개별 사용자 목록은 별도 API이다.
- 타입 enum (`ChannelConstants.ts:57-71`): `DEFAULT 0, RECIPIENT_ADD 1, RECIPIENT_REMOVE 2, CALL 3, CHANNEL_NAME_CHANGE 4, CHANNEL_ICON_CHANGE 5, CHANNEL_PINNED_MESSAGE 6, USER_JOIN 7, CHANNEL_FOLLOW_ADD 12, THREAD_CREATED 18, REPLY 19, THREAD_STARTER_MESSAGE 21, CLIENT_SYSTEM 99`(클라이언트 전용).
- 클라이언트 전용 상태 `MessageStates = SENT, SENDING, EDITING, FAILED` (`ChannelConstants.ts:162-167`). wire에 없고 낙관적 메시지에만 붙는다.

### 1.7 ReadState
- wire(READY `read_states[]`): `{id(channelId), mention_count?, last_message_id?(ack 위치), last_pin_timestamp?, version?}` (`fluxer_app/src/features/read_state/state/read_states/shared.ts:5-12`).
- 스냅샷 행은 1.1의 `SnapshotReadStateRow`이다. `lastMessageId`는 read_state가 아니라 채널에서 온 값이다 (`SNAP/SnapshotChannelReducer.ts:389-427`).
- 메시지 id 비교는 스노우플레이크 비교(`shared.ts:60-69`)이고 `version`도 스노우플레이크 비교이며 한쪽이 null이면 0(동등)으로 처리해 갱신을 허용한다 (`:76-83`).

## 2. 이벤트와 리듀서 매핑

### 2.1 READY
- MobX 처리 순서: 신규 세션이면 메시지, 멤버 사이드바, 카운트 초기화(`GW/events/GatewayReady.ts:115-124`) → 채널 평탄화 → 미가용 길드, 노트, 핀 DM, 관계, 즐겨찾기 밈, RTC 리전, `Users.handleGatewayReady`, `Users.cacheUsers(data.users)` → Guilds, GuildList, GuildMembers, ThreadGuilds, Channels, ReadStates, Presence, UserGuildSettings 등 (`:142-191`) → `setReady()` 후 `Messages.handleGatewayReady()` (`:192-196`).
- 신원 검증: `data.user.id !== expectedUserId`이면 오류이다 (`:105-108`).
- 스냅샷 `applyReady`(`SNAP/SnapshotReducer.ts:168-251`)는 메모리 맵을 비우고 `replaceEntity` 연산을 이 순서로 방출한다: `unavailable_guild, guild, guild_role, guild_emoji, guild_sticker, guild_member, voice_state, channel, read_state, user_settings, account_metadata, user_connections(빈 목록), user_guild_settings, relationship, user, presence`. 반환 커서는 `{sessionId, schemaEpoch=4, updatedAt=now}`이다.
- `user` 행에는 `shellUserIds`(본인, 관계, DM 수신자, 길드 소유자, READY 구성원)만 저장한다 (`SNAP/SnapshotUserReducer.ts:253-274`, `:114-120`).
- `read_state` 초기화: READY 항목은 `readStateKnown:true`, 항목이 없는 텍스트 기반 채널은 빈 read_state에 채널 정보만 채운다 (`SNAP/SnapshotChannelReducer.ts:389-427`).

### 2.2 DISPATCH (스냅샷 리듀서가 처리하는 이벤트, `SNAP/SnapshotReducer.ts:253-432`)

| 이벤트 | 갱신 대상 | 핵심 동작 |
|---|---|---|
| GUILD_CREATE, GUILD_SYNC | guild, role, emoji, sticker, member, voice, channel, read_state | unavailable이면 GUILD_DELETE(unavailable=true)로 위임한다. 아니면 역할과 멤버를 접두사 삭제 후 재삽입하고 누락 채널을 삭제한다 (`:438-466`) |
| GUILD_UPDATE | guild | `member_count`가 payload와 기존 행 모두에 없으면 무시한다 (`:70-84`, `:468-475`) |
| GUILD_DELETE | guild, role, emoji, sticker, member, voice, channel, presence | `unavailable:true`면 `unavailable_guild`를 upsert하고 user_guild_settings는 유지한다 (`:499-522`) |
| GUILD_ROLE_CREATE, UPDATE, DELETE, UPDATE_BULK | guild_role | 단순 upsert, delete (`:265-282`) |
| GUILD_EMOJIS_UPDATE, GUILD_STICKERS_UPDATE | guild_emoji, guild_sticker | 길드 전체 배열 교체 (`:283-292`) |
| GUILD_COUNTS_UPDATE, GUILD_MEMBER_LIST_UPDATE | guild | member_count, online_count만 갱신 (`:477-497`) |
| GUILD_MEMBER_ADD, UPDATE, REMOVE | guild_member, user, presence.guildIds | (`:524-536`) |
| CHANNEL_CREATE | channel, read_state | 통째로 upsert (`SnapshotChannelReducer.ts:255-259`) |
| CHANNEL_UPDATE, CHANNEL_UPDATE_BULK | channel | 기존 행이 없으면 무시하고 얕은 병합 `{...existing, ...data}` (`:261-270`) |
| CHANNEL_DELETE | channel, read_state | 자식 스레드까지 삭제 (`:272-279`) |
| CHANNEL_PINS_UPDATE, CHANNEL_PINS_ACK | channel.last_pin_timestamp, read_state.ackPinTimestamp | (`:281-300`) |
| CHANNEL_RECIPIENT_ADD, REMOVE | channel.recipients, user | 본인이 제거되면 채널 삭제 (`:302-330`) |
| PASSIVE_UPDATES | channel.last_message_id, read_state, voice_state | 더 새로운 id일 때만 반영 (`:332-352`) |
| MESSAGE_CREATE | channel.last_message_id, read_state | 본인 작성이면 ack를 해당 id로, mentionCount를 0으로 (`:354-373`, `:480-508`) |
| MESSAGE_ACK | read_state | `compareReadStateVersions(data.version, current.serverVersion) < 0`이면 무시하고 `message_id === "0"`은 null로 (`:375-387`) |
| THREAD_CREATE, UPDATE, DELETE, LIST_SYNC, MEMBER_UPDATE, MEMBERS_UPDATE | channel(스레드) | 해당 길드가 `threads_active`일 때만 처리 (`SnapshotReducer.ts:434-436`, `:346-371`) |
| VOICE_STATE_UPDATE | voice_state | connection_id 필수, 같은 connection 또는 세션 항목 교체 (`:553-568`) |
| USER_UPDATE | user | `shellUserIds`에 있고 기존 행이 있을 때만 병합 (`SnapshotUserReducer.ts:186-197`) |
| PRESENCE_UPDATE, PRESENCE_UPDATE_BULK | presence, user | shell 사용자만. undefined 필드는 이전 값 유지, 결정 불가면 무시 (`:39-52`, `:199-218`) |
| RELATIONSHIP_ADD, UPDATE, REMOVE | relationship, user | (`SnapshotReducer.ts:538-541`) |
| USER_SETTINGS_UPDATE, USER_GUILD_SETTINGS_UPDATE, USER_NOTE_UPDATE, USER_PINNED_DMS_UPDATE, USER_CONNECTIONS_UPDATE, WEBAUTHN_CREDENTIALS_UPDATE, AUTH_SESSION_CHANGE, FAVORITE_MEME_CREATE, UPDATE, DELETE | user_settings, account_metadata, user_connections, user_guild_settings | `SnapshotAccountReducer.ts:93-192`. READY 이전에 호출되면 예외가 나고 스냅샷이 무효화된다 |

### 2.3 스냅샷이 처리하지 않고 MobX 핸들러만 처리하는 이벤트 (`GW/events/EventRouter.ts:103-185`)
`MESSAGE_UPDATE, MESSAGE_DELETE, MESSAGE_DELETE_BULK, MESSAGE_REACTION_ADD, ADD_MANY, REMOVE, REMOVE_ALL, REMOVE_EMOJI, TYPING_START, RECENT_MENTION_DELETE, SAVED_MESSAGE_*, GUILD_MEMBERS_CHUNK, GUILD_BAN_*, GUILD_HEALTH_UPDATE, CHANNEL_MEMBER_COUNTS_UPDATE, INVITE_*, WEBHOOKS_UPDATE, VOICE_SERVER_UPDATE, VOICE_SIGNAL, CALL_*, ENTRANCE_SOUND_PLAY, OPERATOR_PASS_UPDATE, FORUM_UNREADS, THREAD_MEMBER_LIST_UPDATE`, 그리고 무동작 `SESSIONS_REPLACE`. 메시지 계열 핸들러는 `MSG/events/*.ts`에 있다.

### 2.4 MESSAGE 핸들러 부수효과 (Rust 메시지 모듈이 따라야 할 동작)
- CREATE (`MSG/events/MessageCreate.ts`): 길드 구성원 `hydrateIfMissing`(author, mentions의 member) → `Messages.handleIncomingMessage` → `Channels.handleMessageCreate`(last_message_id) → `ReadStates.handleIncomingMessage` → `MentionFeed` → 알림.
- UPDATE: payload가 부분일 수 있어 `withUpdates`로 병합하고, 편집 중이던 메시지는 `SENT`로 복귀한다 (`MSG/state/MessagingMessages.ts:1180-1200`).
- DELETE: 윈도우에서 제거하고 `ReadStates.handleMessageDelete`가 채널 read_state를 재계산한다 (`MSG/events/MessageDelete.ts`).
- REACTION_ADD, REMOVE: `MessageReactions` 스토어가 카운트를 보유하고 `Messages.handleReaction`은 `withUpdates({})`로 재빌드만 한다 (`MessagingMessages.ts:1296-1313`).

## 3. 디스크 스냅샷 구조

### 3.1 TS 구현 (실제 영속 경로는 데스크톱 엔진)
- 스키마 에포크 `SNAPSHOT_SCHEMA_EPOCH = 4` (`SNAP/SnapshotSchemaEpoch.ts:3`). 하이드레이션 시 커서가 없거나 `cursor.schemaEpoch !== 4`이면 스냅샷을 폐기한다 (`SNAP/SnapshotHydration.ts`의 `runSnapshotScopeHydration`).
- 데스크톱 SQLite는 `fluxer_desktop/src/engine/CacheDatabase.ts`이고 파일명은 `account-engine-cache.sqlite3`이다 (`fluxer_desktop/src/main/AccountEngineIpc.ts:53`). 테이블 5종: `meta(key,value)`, `rows(account_key,entity,key,value)` WITHOUT ROWID 기본키 `(account_key,entity,key)`, `cursors(account_key PK, session_id, schema_epoch, updated_at)`, `summaries(account_key PK, json, updated_at)`, `mentions(account_key,message_id,created_at,json)` 기본키 `(account_key,message_id)`와 인덱스 `mentions_by_time` (`CacheDatabase.ts:23-31`).
- DB 에포크 문자열은 `1 * 1000 + snapshotEpoch`, 즉 `"1004"`이며 `meta.schema_epoch`에 저장한다. 불일치하면 DB 파일을 삭제하고 재생성한다(`reset=epoch`). 열기 실패나 `quick_check` 실패는 `reset=corrupt`로 재생성하고, 그래도 실패하면 메모리 DB로 대체한다 (`CacheDatabase.ts:132-145`, `:389-437`, `PKG/desktop_ipc/src/AccountEngineCoreContract.ts:75`).
- 쓰기 정책: 250ms 또는 4000연산마다 커밋(`:14-15`), 계정당 행 2,000,000개 제한(`:16`). PRAGMA는 WAL, synchronous NORMAL, busy_timeout 2000, secure_delete ON이다.
- 메모리 스토어 제한: 계정 1개, 행 100,000개, 128MiB, 값 256KiB, 이벤트당 연산 100,000개 (`SNAP/InMemorySnapshotStore.ts:11-22`, `SNAP/DiskBackedSnapshotReducer.ts:437-449`).

### 3.2 DiskBacked 리듀서: 작업 집합과 부재 키 로딩
- 무거운 8개 엔티티(`guild, unavailable_guild, guild_member, channel, read_state, presence, user, voice_state`)만 작업 집합 맵으로 둔다 (`SNAP/DiskBackedSnapshotReducer.ts:30-43`). 나머지(role, emoji, user_settings 등)는 쓰기 전용이다.
- 디스패치 처리: 리듀서를 실행해 읽기에 실패한 키를 수집하고, 있으면 롤백(`restore`) 후 SQLite에서 해당 키만 로드해 재실행한다. 최대 8라운드 후에는 전체를 로드한다 (`:544-564`). 성공한 패스의 연산을 `SnapshotWriter`에 큐잉한다 (`:566-570`).
- `SnapshotWriter`는 쓰기 실패 시 `store.evict(account)`로 계정 전체를 무효화한다 (`SNAP/SnapshotWriter.ts:66-73`). 부분 복구 없이 전체 폐기가 정책이다.
- 셸 상태(self id, shellUserIds, user_settings, account_metadata)는 항상 메모리에 있고 재시작 시 저장된 행에서 재구성한다 (`fluxer_app/src/features/account_engine/core/AccountEngineCore.ts:74-100`, `:332-358`).
- 포그라운드 경로의 `SnapshotSync`는 `InMemorySnapshotStore`를 쓰고 데스크톱 엔진 경로가 `DiskBackedSnapshotReducer`를 쓴다 (`SNAP/SnapshotSync.ts:107`, `AccountEngineCore.ts:308-315`). 두 경로가 같은 `SnapshotReducer`를 공유한다.

### 3.3 영속 대상과 메모리 전용
- 영속: 1.1의 16개 엔티티, 커서, 요약, mentions(백그라운드 알림), boot_state 파일(메시지 창 채널당 100개 최대 25채널, 멤버 리스트 4개 각 300행; `BootStateCodec.ts:5-8`).
- 메모리 전용: 메시지 윈도우(`ChannelMessages`), 타이핑, 입력 초안, 리액션 상세, 점프 상태, 전송 큐(`MessageQueue`), 읽음 ack 큐(`ReadStates.ackQueue`), 멤버 사이드바.

## 4. 메시지 목록 캐시 정책 (`MSG/state/ChannelMessages.ts`, `MessagingMessages.ts`)

- 상수 (`PKG/constants/src/LimitConstants.ts:107-111`): 페이지 크기 `MAX_MESSAGES_PER_CHANNEL=50`, 윈도우 상한 `MAX_LOADED_MESSAGES=200`, 트림 후 크기 `TRUNCATED_MESSAGE_VIEW_SIZE=100`(모바일은 50), `MAX_MESSAGE_CACHE_SIZE=250`.
- 채널 캐시: 메모리에 최대 50채널을 두고 접근 순서(LRU)로 제거하며 선택 채널과 retain된 채널은 제외한다 (`ChannelMessages.ts:256-258`, `:340-378`).
- 윈도우 상태: `ready, hasMoreBefore(초기 true), hasMoreAfter, loadingMore, cached(재연결 후 신뢰 불가), error, version, jump*`와 양끝 버퍼 `beforeBuffer, afterBuffer` (`:259-283`).
- 로딩 API: `GET /channels/{id}/messages?before&after&limit&around` (`MSG/commands/MessageCommands.tsx:287-295`, `Endpoints.CHANNEL_MESSAGES`). 응답은 최신순이며 적용 시 뒤집는다 (`ChannelMessages.ts:793`).
- 페이지 상태 (`MSG/commands/MessagePageStateMachine.ts`의 `getBasePageState`): `isBefore = before != null`, `isAfter = after != null`, 초기 로딩과 교체는 둘 다 false이다. `hasMoreBefore = (응답 수 == limit) && (isBefore 또는 교체)`, `hasMoreAfter = isAfter && 응답 수 == limit`. around 요청은 `MessagePaginationUtils`가 별도로 계산한다.
- 적용 모드 (`MSG/state/ChannelMessagesLoadStateMachine.ts`): 교체 조건은 jump가 있거나, 윈도우가 ready가 아니거나, before와 after가 모두 없을 때이다. 그 외 before는 앞쪽 병합(상단에 붙이고 하단을 트림, hasMoreAfter 유지), after는 뒤쪽 병합(하단에 붙이고 상단을 트림, hasMoreBefore 유지)이다. 교체이거나 `isAfter && !hasMoreAfter`이면 `cached` 신뢰를 회복한다.
- 교체 시 서버 응답에 없는 미전송 메시지(`SENDING`, `FAILED`)는 보존해 다시 병합한다 (`ChannelMessages.ts` `collectUnsentMessages`, `applyLoadedWindow :786-846`).
- 캐시 적중: `ready && !cached`이고 요청한 before나 after가 버퍼로 충족되면 네트워크 없이 `drainBufferedSide`로 처리한다 (`MessageCommands.tsx:456-495`).
- 수신 메시지 결정표 (`MSG/state/ChannelIncomingMessageStateMachine.ts`): nonce가 윈도우에 있고 작성자가 같으면 `replaceNonceMessage`(업로드 자리표시자면 제거 후 끝에 추가), 아니면 `hasMoreAfter`일 때 무시(`ignorePastVisibleWindow`), 아니면 끝에 추가하고 상단을 트림한다 (`ChannelMessages.ts:730-769`). 윈도우가 `ready`가 아니거나 시드 창이면 무시한다 (`MessagingMessages.ts:1102-1120`).
- 낙관적 전송: 클라이언트가 `nonce = SnowflakeUtils.fromTimestamp(Date.now())`를 만들고 `id = nonce`, `state = SENDING`, `nonce`를 가진 임시 메시지를 `createOptimistic`으로 삽입한다 (`MSG/hooks/useMessageSubmission.tsx:127-160`, `:201-225`, `MessageCommands.tsx:1123-1126`). `POST /channels/{id}/messages` 본문에 같은 `nonce`를 담고 (`MSG/state/MessageQueue.tsx:958`), HTTP 응답과 게이트웨이 `MESSAGE_CREATE`가 모두 `handleIncomingMessage`로 들어와 nonce로 병합된다 (`MessageCommands.tsx:709-722`). 실패하면 `FAILED`, 재시도하면 `SENDING`이다 (`MessagingMessages.ts:1120-1137`).
- 로컬 전송 제한: 채널당 2초에 5회이고 초과 시 3초 차단 (`MessageQueue.tsx:104-106`), 예약 TTL 30초, 큐 기본 크기 5, 429 자동 재시도는 최대 2회에 30초 이하 대기 (`:102-113`).
- 세션 변경(READY의 session_id가 이전과 다름) 시 모든 채널 윈도우를 비우고 선택 채널을 다시 로드한다 (`MessagingMessages.ts:676-702`). 같은 세션 재연결은 윈도우를 `ready:false`로 두고 선택 채널만 즉시 다시 가져온다 (`:703-755`).
- 보조: 오래된 `cached` 창은 10초 간격으로 재조회하고 (`:30`), 부팅 복원 창은 최대 4쪽을 따라잡는다 (`:32-33`).

## 5. 권한 계산 (최소 로직 위치)

- 계산 본체는 `fluxer_app/src/features/permissions/utils/PermissionUtils.ts:105-201`의 `computePermissions`이다. 알고리즘은 다음과 같다.
  1. 길드가 없으면 0을 반환한다. 방문자(visiting)면 @everyone 권한에서 ADMINISTRATOR를 빼고 `VIEW_CHANNEL`과 `READ_MESSAGE_HISTORY`로 마스크한다 (`:131-142`, `PKG/constants/src/GuildVisitConstants.ts:6`).
  2. 소유자는 `ALL_PERMISSIONS`이다 (`:143-145`).
  3. 기본값은 @everyone 역할 권한(없으면 `DEFAULT_PERMISSIONS`)이고 멤버 역할 권한을 OR한다 (`:146-157`).
  4. ADMINISTRATOR가 있으면 전부 허용한다. 아니면 오버라이트를 @everyone(deny 제거 후 allow 추가), 멤버 역할 오버라이트(allow와 deny를 각각 합산해 적용), 멤버 개인 오버라이트 순으로 적용한다 (`:158-189`).
  5. MFA 승격이 필요한 길드에서 본인이 소유자가 아니고 MFA를 쓰지 않으면 `ElevatedPermissions`를 제거한다 (`:90-103`, `ChannelConstants.ts:267-278`).
- 캐시: `fluxer_app/src/features/permissions/state/Permission.ts`가 채널과 길드별 비트를 보관하고 (`:34-69`), 스레드는 부모 채널 권한에서 `threadViewPermissions`로 파생한다 (`:117-140`). 갱신 트리거는 `handleGuildMemberUpdate`, `handleGuildRole`, `handleChannelUpdate` 등이다.
- 채널 보기는 `VIEW_CHANNEL`, 메시지 보내기는 `SEND_MESSAGES` (`fluxer_app/src/features/channel/components/ChannelTextarea.tsx:27`의 `Permission.can(Permissions.SEND_MESSAGES, channel)`), 기록 읽기는 `READ_MESSAGE_HISTORY`이다.
- 비트 외 제약: 타임아웃 멤버 `isTimedOut()`, 슬로우모드 `rate_limit_per_user`와 `ChannelSlowmodeStateResponse`(`next_send_allowed_at`, `can_bypass`; `ChannelSchemas.ts:30-42`), 스레드 잠금과 보관이 있다. 전송을 막는 UI 분기의 전체 위치는 확인하지 못했다(불확실).
- DM 채널(guild_id 없음)은 `computePermissions`에서 길드가 없어 0을 반환할 가능성이 있다. DM 전송 허용이 별도 분기인지는 확인하지 못했다(불확실).

## 6. 구 `fluxer_desktop_rs` 자산과 TS 대응

| Rust | TS 대응 | 상태 |
|---|---|---|
| `fluxer_desktop_rs/src/sqlite_store.rs:12-21` SCHEMA | `fluxer_desktop/src/engine/CacheDatabase.ts:23-31` | 5개 테이블과 인덱스가 동일하다 (관찰) |
| `SqliteStore` 의 `upsert_row, delete_row, delete_rows_by_prefix, rows_by_prefix, replace_entity, delete_account_rows` (`:106-203`) | `upsertRow, deleteRow, deleteRowsByPrefix, rowsByPrefix, replaceEntity` | 기능은 동등하다. 접두사 삭제는 구 코어가 `LIKE prefix%`(`:138-146`), TS가 범위 질의(`prefixUpperBound`)이다. ID가 숫자이면 결과가 같다(추정) |
| `read_cursor, write_cursor` (`:206-235`) | `readCursor, writeCursor` | 구 코어 커서는 `session_id: String`, `schema_epoch: u32`로 널을 허용하지 않는다 (`contracts.rs:262-269`). TS 커서는 `sessionId: string 또는 null`, `schemaEpoch: number 또는 null`이다 (`SNAP/SnapshotTypes.ts:5-9`) |
| `set_summary, all_summaries, insert_mention, list_mentions, delete_mention, delete_mentions` (`:248-330`) | `writeSummary, readSummaries, insertMention, listMentions` | TS `insertMention`의 보존 개수(`keep`)와 prune이 구 코어에 없다 (`CacheDatabase.ts:299-310`) |
| 없음 | 에포크 불일치 시 DB 재생성, quick_check, 250ms 배치 커밋, 행 수와 크기 제한, 파일 권한 | 구 코어에 미구현이다. `meta` 읽기와 쓰기는 있으나 에포크 비교 로직이 없고 테스트에서만 쓴다 (`sqlite_store.rs:81-103`, `:343`) |
| `account_engine.rs:30-181` `handle_request` | `AccountEngineIpc.ts`와 `AccountEngineCore.ts` | 요청 라우터이며 리듀서가 아니다. `beginPromotion`은 항상 `entries: {}`를 반환하고 (`:129-143`), `completePromotion, abortPromotion, reconnect`는 무동작이다 (`:145-173`). `getActivity`는 존재하지 않는 `channel_states` 엔티티를 조회한다 (`:78-85`, 저장소에서 이 문자열은 Rust 한 곳뿐이다). `demote`만 엔트리를 `replace_entity`로 커밋한다 (`:147-170`) |

- 구 `fluxer_desktop_rs`에는 `SnapshotReducer`, `DiskBackedSnapshotReducer`, 메시지 윈도우, 권한 계산, READY와 DISPATCH 처리 코드가 없다. 새 코어 `fluxer_client/core/src/state`는 READY와 주요 DISPATCH를 메모리 상태로 처리하지만, 스냅샷 행 연산과 SQLite 영속은 아직 구현하지 않았다(마지막 구현 맵). `fluxer_desktop_rs/src/gateway.rs`는 이번 조사에서 읽지 않았다(불확실).
- 위험(추정): 구 `fluxer_desktop_rs`와 TS 데스크톱이 같은 파일명 `account-engine-cache.sqlite3`를 쓰지만 구 Rust 코어는 `%APPDATA%/fluxer_desktop_rs/storage`를 쓰는 것으로 보인다 (`sqlite_store.rs:30-37`). 같은 파일을 공유한다면 구 Rust 코어가 에포크 문자열 `"1004"`를 쓰지 않아 상호 오해석 위험이 있다.

## 7. 첫 마일스톤(로그인, 길드와 채널 목록, 메시지 읽기와 보내기) 최소 부분집합 제안

### 7.1 포함 (구현 순서)
1. 식별 타입: 스노우플레이크 문자열을 `u64`로 파싱해 숫자 비교하고, `ChannelType` enum, `Permissions` 비트(`u64`), `MessageType`을 정의한다.
2. `ReadyPayload` 역직렬화는 필요한 필드만 타입화하고 나머지는 `serde_json::Value`로 보존한다: `session_id, user, guilds[properties, channels, roles, members, member_count, joined_at, unavailable], private_channels, read_states, users`.
3. 상태 컨테이너 `ClientState`: `users, guilds, unavailable_guilds, roles(길드별), channels, members(길드별, 본인 멤버 필수), read_states, self_user_id, session_id`. TS 스냅샷의 키 규칙을 그대로 쓰면 `rows` 테이블과 호환된다.
4. 리듀서 함수 `apply_ready`, `apply_dispatch`는 입력 이벤트에서 행 연산(`Upsert, Delete, DeleteByPrefix, ReplaceEntity`)을 방출하고, SQLite 영속과 UI 갱신이 같은 연산 목록을 소비하게 한다 (`SNAP/SnapshotRowStore.ts` 인터페이스에 대응).
   - 필수: `GUILD_CREATE, GUILD_UPDATE, GUILD_DELETE, GUILD_ROLE_*, GUILD_MEMBER_ADD, UPDATE, REMOVE(본인 멤버의 역할 반영), CHANNEL_CREATE, UPDATE, DELETE, MESSAGE_CREATE(last_message_id와 read_state), MESSAGE_ACK, USER_UPDATE`.
   - 있으면 좋음: `GUILD_COUNTS_UPDATE, PASSIVE_UPDATES, PRESENCE_UPDATE`.
   - 보류: 스레드 계열, 음성, 이모지와 스티커, 관계, 즐겨찾기 밈, 계정 메타데이터. 사용자 설정은 필요하면 `user_settings` 원본 JSON만 보존한다.
5. 권한 계산: 5절 1~4단계를 순수 함수로 이식하고 `can_view_channel`, `can_send_messages`를 노출한다. MFA 승격, 방문자 모드, 스레드 권한은 보류한다.
6. 메시지 윈도우 모듈(채널별): `Vec<Message>`, `has_more_before`, `has_more_after`, `ready`, `cached`와 연산 `apply_loaded_window(replace, before, after)`, `apply_incoming`(nonce 병합 포함), `update`, `remove`, `trim`. 첫 마일스톤은 최신 50개 로드, `before`로 이전 페이지, 수신 `MESSAGE_CREATE`, 낙관적 전송과 성공, 실패까지로 제한하고 `around`, 점프, 버퍼 드레인은 보류한다. 상수는 4절 값을 그대로 쓴다.
7. 보내기: `POST /channels/{id}/messages` 본문 `{content, nonce}`만 구현한다. 로컬 속도 제한(2초 5회)과 429 재시도는 보류할 수 있으나 nonce 병합은 필수이다.
8. 읽음: `MESSAGE_ACK` 반영과 채널을 열 때 `POST /channels/{id}/messages/{messageId}/ack` (`Endpoints.CHANNEL_MESSAGE_ACK`). 멘션 증가는 `ReadStates.ts`의 `recordUnread` 분기(`:~655-665`)이며 첫 마일스톤에서는 서버가 준 `mention_count`만 표시한다.
9. 영속: 기존 `SqliteStore`를 재사용해 위 엔티티를 `rows`에 JSON으로 저장하고 커서를 `schemaEpoch`와 함께 기록한다. 에포크 불일치 시 계정 행을 삭제하는 로직(최소한 `delete_account_rows` 호출)을 추가한다.

### 7.2 첫 마일스톤에서 제외 (근거)
- 메시지 본문의 디스크 영속과 boot_state: 성능 최적화이며 기능 필수가 아니다.
- 백그라운드 알림용 `BackgroundReadStateReducer`와 mentions, summaries 테이블 사용.
- `DiskBackedSnapshotReducer`의 작업 집합과 부재 키 로딩: 첫 단계는 전체를 메모리에 올리는 resident 백킹(`SNAP/SnapshotReducerBacking.ts:34-43`)에 대응시키고, 계정 규모가 커질 때 도입한다.

### 7.3 검증 제안
- TS 리듀서를 오라클로 삼아 같은 READY와 디스패치 JSON을 TS와 Rust에 입력하고 방출된 `SnapshotRowOp` 목록을 `insta` 스냅샷으로 비교한다. TS 리듀서의 기존 테스트 유무는 확인하지 않았다(불확실).

## 8. 불확실 항목

1. DM 채널과 스레드의 `can_send_messages` 경로, 슬로우모드, 타임아웃, 잠긴 스레드가 어디서 전송을 막는지 확인하지 못했다.
2. 스레드 전용 권한 비트(`ThreadPermissionUtils.ts`)의 값 범위를 읽지 않았다.
3. Rust `gateway.rs`와 IDENTIFY, 재연결, 시퀀스 처리(전송 계층)는 범위 밖으로 두어 읽지 않았다.
4. 구 `fluxer_desktop_rs`와 TS 데스크톱이 같은 SQLite 파일을 공유하는지, 에포크 문자열 호환이 필요한지는 경로만 보고 추정했다.
5. `ChannelMessages`의 버퍼 세그먼트 경계 처리(`isBoundary`)와 `reconcileSeededWindow`의 세부 동작은 요약만 했다.
6. 미읽음 증가(`unreadCount`, `oldestUnreadMessageId`)와 ack 큐(`ReadStateAckQueue`, 3초 배치 `ACK_BATCH_DELAY_MS`)의 전체 상태 기계는 요약 수준이다.
7. 이 문서는 소스 정독만 근거이며 실행, 빌드, 테스트는 하지 않았다.

## Rust implementation map (2026-11-18)

- 기준: `fluxer_client/core/src/state`의 `mod.rs`, `client.rs`, `model.rs`, `content.rs`, `messages.rs`, `permissions.rs`, `ids.rs`, `handlers/*.rs`를 직접 읽었다. 테스트 파일(`state/tests.rs`, `state/tests/handlers.rs`)은 존재만 확인했고 실행하지 않았다.
- 이 절은 TS 명세와 Rust 구현의 대응을 기록한다. 위 절의 TS 설명과 다르면 이 절의 “미구현·차이”가 우선한다.

### 모듈 경로

| 경로 | 역할 |
|---|---|
| `fluxer_client/core/src/state/mod.rs` | 하위 모듈 선언과 공개 타입 재수출 |
| `fluxer_client/core/src/state/client.rs` | `ClientState`, 디스패치 라우터, 사이드바·읽음·권한 조회 |
| `fluxer_client/core/src/state/model.rs` | `User`, `Guild`, `Channel`, `Member`, `Message`, `ReadState`, `Role` 등 wire 엔티티와 timestamp 파서 |
| `fluxer_client/core/src/state/content.rs` | 첨부·임베드·스티커·리액션·답장 참조 타입과 플래그 상수 |
| `fluxer_client/core/src/state/messages.rs` | 채널별 메시지 윈도우 `MessageStore`/`MessageWindow` |
| `fluxer_client/core/src/state/permissions.rs` | 권한 비트, `compute_permissions`, 스레드 권한 파생 |
| `fluxer_client/core/src/state/ids.rs` | `Snowflake`, `FLUXER_EPOCH_MS`, 비트필드 문자열 serde |
| `fluxer_client/core/src/state/handlers/ready.rs` | READY 처리 |
| `fluxer_client/core/src/state/handlers/guilds.rs` | 길드·역할 이벤트 |
| `fluxer_client/core/src/state/handlers/channels.rs` | 채널 이벤트와 `PASSIVE_UPDATES` |
| `fluxer_client/core/src/state/handlers/messages.rs` | 메시지 이벤트, 수신 메시지의 읽음 반영, 답장 미리보기 |
| `fluxer_client/core/src/state/handlers/reactions.rs` | 리액션 이벤트와 낙관적 토글 |
| `fluxer_client/core/src/state/handlers/read_states.rs` | READY 읽음 상태, `MESSAGE_ACK`, 로컬 ack |
| `fluxer_client/core/src/state/handlers/members.rs` | 길드 멤버 추가·수정·삭제와 멤버 hydration |
| `fluxer_client/core/src/state/handlers/member_list.rs` | 지연 멤버 목록(`GUILD_MEMBER_LIST_UPDATE`) |
| `fluxer_client/core/src/state/handlers/presence.rs` | 프레즌스와 사용자 상태 |
| `fluxer_client/core/src/state/handlers/relationships.rs` | 친구·차단·요청 관계 |
| `fluxer_client/core/src/state/handlers/typing.rs` | 타이핑 표시와 만료 |
| `fluxer_client/core/src/state/handlers/user_settings.rs` | 사용자 설정, 길드별 알림 설정, 음소거 판정 |
| `fluxer_client/core/src/state/handlers/users.rs` | `USER_UPDATE` |
| `fluxer_client/core/src/state/handlers/emojis.rs` | 길드 커스텀 이모지 |

### 공개 타입과 함수

| 항목 | 한 줄 목적 |
|---|---|
| `ClientState` | 한 계정의 엔티티 캐시와 메시지 윈도우를 보유하고 디스패치를 적용한다. |
| `ClientState::new`, `apply`, `apply_at` | 빈 상태 생성, 현재 시각 또는 지정 시각으로 `Dispatch` 하나를 적용하고 `Vec<StateChange>`를 반환한다. |
| `ClientState::sorted_guilds`, `channel_tree`, `dm_channels` | 폴더 순서 길드 목록, 카테고리 포함 채널 트리, 최근 활동순 DM 목록을 만든다. |
| `ClientState::channel_unread`, `guild_unread`, `unread_anchor` | 채널·길드 읽음 여부와 멘션 수, “NEW” 구분선 기준 id를 계산한다. |
| `ClientState::channel_permissions`, `can_view_channel`, `can_send_messages`, `can_send_messages_at` | 채널 권한 계산과 보기·전송 가능 여부를 판정한다. |
| `StateChange` | UI가 다시 그릴 대상(Ready, Guild*, Channel*, MessagesChanged, ReadStateChanged 등)을 나타낸다. |
| `Unread`, `ChannelTreeNode` | 읽음 표시와 카테고리·자식 채널 묶음. |
| `Snowflake`, `FLUXER_EPOCH_MS` | 스노우플레이크 id와 생성 시각 변환, Fluxer epoch 상수. |
| `User`, `Role`, `PermissionOverwrite`, `Guild`, `Channel`, `ChannelKind`, `Member` | READY·길드·채널·멤버 wire 엔티티. |
| `Message`, `MessageState`, `ReadState` | 메시지, 클라이언트 전용 전송 상태, 채널별 읽음 상태. |
| `parse_timestamp_ms` | RFC 3339/ISO 8601 문자열을 Unix ms로 파싱한다. |
| `Attachment`, `Embed`, `EmbedAuthor`, `EmbedFooter`, `EmbedProvider`, `EmbedMedia`, `EmbedField`, `Sticker`, `Reaction`, `PartialEmoji`, `MessageReference` | 메시지 본문 구성 요소 타입. `AttachmentSummary`와 `EmbedSummary`는 각각 `Attachment`/`Embed`의 옛 이름 별칭이다. |
| `ATTACHMENT_FLAG_*`, `MESSAGE_TYPE_REPLY`, `message_flags` | 첨부·메시지 플래그 비트와 답장 타입 상수. |
| `MessageStore`, `MessageWindow`, `LoadMode` | 채널별 윈도우 저장소, 윈도우 상태, REST 페이지 적용 모드. |
| `MessageStore::load`, `apply_incoming`, `insert_pending`, `mark_failed`, `mark_sending`, `remove_pending` | 페이지 병합·교체, 수신 메시지 nonce 병합, 낙관적 전송과 실패·재시도 상태 전환. |
| `MessageStore::select`, `set_pinned_to_end`, `message` | 선택 채널 고정, 끝 고정 여부 기록, 로드된 메시지 조회. |
| `PAGE_SIZE`, `MAX_LOADED_MESSAGES`, `TRUNCATED_MESSAGE_VIEW_SIZE`, `MAX_CACHED_CHANNELS` | 50, 200, 100, 50 상수. |
| `bits`, `compute_permissions`, `thread_view_permissions`, `has` | 권한 비트 정의, guild/member/channel 권한 계산, 스레드 파생, 비트 포함 검사. |
| `GuildEmoji` | 길드 커스텀 이모지. |
| `MemberList`, `MemberListEntry`, `MemberListGroup`, `MemberListRow`, `MemberLists`, `ListPresence` | 지연 멤버 목록의 행·그룹·프레즌스와 목록 저장소. |
| `Presence`, `Status`, `CustomStatus` | 다른 사용자의 프레즌스, 상태 enum, 커스텀 상태. |
| `Relationship`, `RelationshipKind` | 관계 종류와 관계 엔티티(상대 user는 `users`에 저장). |
| `Typer`, `TYPING_EXPIRY_MS` | 타이핑 사용자와 10초 만료 상수. |
| `UserSettings`, `UserGuildSettings`, `ChannelOverride`, `MuteConfig`, `message_notifications` | 사용자 설정, 길드·DM 알림 설정, 채널 오버라이드, 음소거 설정과 알림 상수. |
| `ReplyPreview` | 답장 헤더가 보여 줄 대상(로드됨, 삭제됨, 미로드)을 나타낸다. |

핸들러의 이벤트 메서드는 `pub(in crate::state)`이므로 외부 공개 API는 `ClientState::apply`/`apply_at`과 위 재수출 타입이다. 주요 메서드는 `apply_ready`, `insert_guild_payload`, `apply_guild_create/update/delete`, `apply_role_upsert/bulk/delete`, `apply_channel_create/update/update_bulk/delete`, `apply_passive_updates`, `apply_message_create/update/delete/delete_bulk`, `apply_reaction_add/add_many/remove/remove_all/remove_emoji`, `apply_message_ack`, `local_ack`, `apply_member_upsert/remove`, `hydrate_member`, `apply_member_list_update`, `apply_presence_update/bulk`, `apply_relationship_upsert/remove`, `apply_typing_start`, `expire_typing`, `apply_user_settings_update`, `apply_user_guild_settings_update`, `apply_user_update`, `apply_guild_emojis_update`이다.

### 구현됨

- READY: `session_id`가 같으면 메시지 윈도우를 무효화하고, 다르면 비운 뒤 사용자·길드·비공개 채널·읽음 상태·관계·프레즌스·설정을 다시 채운다.
- 디스패치: `USER_UPDATE`, `GUILD_CREATE/SYNC/UPDATE/DELETE`, `GUILD_ROLE_CREATE/UPDATE/UPDATE_BULK/DELETE`, `GUILD_EMOJIS_UPDATE`, `GUILD_MEMBER_ADD/UPDATE/REMOVE`, `GUILD_MEMBER_LIST_UPDATE`, `CHANNEL_CREATE/UPDATE/UPDATE_BULK/DELETE`, `PASSIVE_UPDATES`, `MESSAGE_CREATE/UPDATE/DELETE/DELETE_BULK`, `MESSAGE_REACTION_ADD/ADD_MANY/REMOVE/REMOVE_ALL/REMOVE_EMOJI`, `MESSAGE_ACK`, `TYPING_START`, `PRESENCE_UPDATE/BULK`, `RELATIONSHIP_ADD/UPDATE/REMOVE`, `USER_SETTINGS_UPDATE`, `USER_GUILD_SETTINGS_UPDATE`를 처리한다.
- 메시지: 최신 50개 페이지 교체, `before` 앞 병합, `after` 뒤 병합, 200개 초과 시 100개로 트림, 채널 50개 LRU, nonce 기반 낙관적 전송과 서버 메시지 병합, 실패·재시도, 삭제·편집 반영, 답장 미리보기를 구현했다.
- 권한: 방문자 마스크, 소유자 전체 권한, @everyone과 멤버 역할 OR, ADMINISTRATOR, 채널 오버라이트(@everyone, 역할, 멤버), 스레드 부모 파생을 구현했다.
- 읽음·알림: 서버 `read_states`, `MESSAGE_ACK` 버전 비교, 본인 메시지 자동 ack, 멘션 카운트(사용자 멘션, @everyone, 내 역할, DM), 로컬 ack를 구현했다.
- 부가 상태: 타이핑 만료, 프레즌스, 관계, 사용자·길드 설정과 음소거 판정, 커스텀 이모지, 지연 멤버 목록(SYNC/INSERT/UPDATE/DELETE/INVALIDATE)을 구현했다.

### 미구현·차이

- 이 모듈에는 디스크 스냅샷 영속이 없다. `SnapshotReducer`의 행 연산(`Upsert/Delete/DeleteByPrefix/ReplaceEntity`), `DiskBackedSnapshotReducer`의 작업 집합·부재 키 로딩, SQLite `rows/cursors/summaries/mentions`, boot_state 파일을 구현하지 않았다. `ClientState`는 메모리 상태만 보유한다.
- 음성 상태(`VOICE_STATE_UPDATE`, `VOICE_SERVER_UPDATE`, `VOICE_SIGNAL`)와 통화 이벤트를 처리하지 않는다. `PASSIVE_UPDATES`도 채널 `last_message_id`와 읽음 변경만 반영하고 `voice_state`는 갱신하지 않는다.
- `GUILD_STICKERS_UPDATE`, `GUILD_COUNTS_UPDATE`, `GUILD_MEMBERS_CHUNK`, `CHANNEL_PINS_UPDATE/ACK`, `CHANNEL_RECIPIENT_ADD/REMOVE`, `THREAD_CREATE/UPDATE/DELETE/LIST_SYNC/MEMBER_UPDATE/MEMBERS_UPDATE`, `RECENT_MENTION_DELETE`, `SAVED_MESSAGE_*`를 처리하지 않는다. 스레드는 READY·`GUILD_CREATE`·`CHANNEL_CREATE`로만 들어온다.
- 메시지 윈도우의 `around` 로드, 점프, before/after 사이드 버퍼 드레인, `cached` 신뢰 플래그, 세션 재연결 후 선택 채널 자동 재조회를 구현하지 않았다. 트림된 메시지는 다시 가져와야 한다. `MessageState`도 `Sending/Sent/Failed`만 쓰고 편집 중 상태(`EDITING`)는 사용하지 않는다.
- `can_send_messages_at`은 슬로우모드를 검사하지 않는다. 비공개 채널은 항상 전송 가능으로 판정하고, 타임아웃만 검사한다.
- 멘션 계산은 차단·음소거·`suppress_everyone`·`suppress_roles` 설정을 적용하지 않는다. 리액션 중복 제거도 현재 사용자의 `me` 플래그만 사용한다.
- 프레즌스는 오프라인이 되면 항목을 제거하며 길드별 온라인 수를 유지하지 않는다. 멤버 목록의 1초 배칭, boot-state 시딩, 목록별 커스텀 상태 부기는 이식하지 않았다.
- 권한 계산에서 MFA 승격 제거(`calculateElevatedPermissions`)를 구현하지 않았다. DM 전송 허용은 별도 권한 계산 대신 비공개 채널 분기로 처리한다.
