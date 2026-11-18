# Fluxer 게이트웨이 프로토콜 명세 (Rust 클라이언트 재구현용)

기준: 저장소 루트, 브랜치 `desktop-rs`, HEAD `d1124083`. 읽기 전용 소스 분석 결과이며 실행 검증은 하지 않았다.
표기: 경로는 저장소 루트 기준. `TS/` = `fluxer_app/src/features/gateway/transport/`, `ERL/` = `fluxer_gateway/src/`. "추정"은 코드에서 직접 확인하지 못한 항목이다.

## 1. 접속 URL, 인코딩, 압축

- 게이트웨이 URL은 인스턴스 discovery 문서의 `endpoints.gateway`(ws/wss URL)에서 얻는다. 경로는 `/.well-known/fluxer`(`packages/instance_bootstrap/src/Discovery.ts:49`)이고 웹앱은 `/api/.well-known/fluxer`를 쓴다(`fluxer_app/src/features/platform/service_worker/Worker.ts:478`). 스키마는 `packages/schema/src/domains/instance/InstanceSchemas.ts:210`. 클라이언트는 `RuntimeConfig.gatewayEndpoint`를 그대로 사용한다(`TS/PooledGatewayConnection.ts:689`).
- 서버는 cowboy 라우트 `/` 하나에서 WebSocket을 받는다(`ERL/gateway/fluxer_gateway_app.erl:37`). discovery가 가리키는 URL의 경로는 그대로 쓰면 된다.
- 쿼리 파라미터(`TS/GatewaySocket.ts:1443-1453`, 서버 파싱 `ERL/gateway/gateway_handler.erl:56-63`):
  - `v=1` 필수. 서버는 1만 허용하고 아니면 4012로 닫는다(`gateway_handler.erl:parse_version`, `websocket_init`). 상수는 `packages/constants/src/AppConstants.ts:4`.
  - `encoding=json`. 서버는 값을 무시하고 항상 JSON이다(`ERL/gateway/gateway_codec.erl:17-18`). ETF 등은 없다.
  - `compress=zstd-stream&stream=1`. 서버는 두 파라미터가 모두 있을 때만 zstd 스트림을 켠다(`gateway_compress.erl:61-75`). `compress=none`이거나 `stream`이 없으면 비압축이다.
- 비압축 모드: 서버는 JSON 텍스트 프레임을 보낸다. 클라이언트도 텍스트로 보낸다(`TS/GatewaySocket.ts:1475`, `gateway_handler_encode.erl:make_frame`).
- zstd 스트림 모드:
  - 서버 송신은 항상 바이너리 프레임이고, 클라이언트 송신도 바이너리여야 한다(`TS/GatewaySocket.ts:1473`).
  - 연결당 방향별로 zstd 컨텍스트 하나를 끝까지 유지한다. 사전(dictionary)은 없다. 첫 메시지에서 프레임이 시작되고 이후 메시지는 같은 프레임의 연속이다.
  - 메시지 하나가 WebSocket 바이너리 메시지 하나에 대응한다. 클라이언트 인코더는 메시지마다 flush(`ZSTD_e_flush`)한다(`fluxer_app/rust/libfluxcore/src/zstd_stream.rs:62`). 디코더는 입력을 모두 소비해도 출력이 비면 다음 청크를 기다린다(`TS/GatewaySocket.ts:897-903`).
  - 클라이언트 인코더 파라미터: level 3, windowLog 17, hashLog/chainLog 15(`zstd_stream.rs:8-10`, `TS/GatewayCompression.ts:19`). 서버 인코더는 level 3, windowLog 기본 17(설정으로 10~21), hash/chain 15(`gateway_compress.erl:21-26,190-203`). 클라이언트 디코더는 windowLogMax 21을 허용해야 한다(`zstd_stream.rs:10`).
  - 한도: 수신 압축 메시지와 압축 해제 결과 모두 16384바이트(`ERL/utils/constants.erl:max_payload_size`, `gateway_handler.erl:handle_incoming_data`). 초과하면 4002로 닫는다. 서버가 보내는 메시지의 크기 제한은 확인하지 못했다.
  - 복구 규칙: 클라이언트는 압축 해제 실패 시 `compress=none`으로 낮춰 즉시 재접속한다(`TS/GatewaySocket.ts:916-934`). Rust 클라이언트도 이 폴백을 두는 편이 안전하다.
- WebSocket 확장(permessage-deflate)은 서버 옵션에 없다(`gateway_handler.erl:66-74`). 연결은 `idle_timeout => infinity`, `max_frame_size => 16384`.
- 기존 네이티브 전송 `fluxer_desktop/native/gateway-socket/src/lib.rs`는 `sendText`/`sendBinary`/`close`만 제공하는 순수 전송이다(`lib.rs:418-436`). zstd, JSON, 하트비트는 JS가 담당한다. Rust 재구현은 `tokio-tungstenite`(rustls)와 `zstd` 또는 `zstd-safe` 크레이트면 충분하다(추정).

## 2. Opcode 표

번호 출처: `packages/constants/src/GatewayConstants.ts:5-24`, `ERL/utils/constants.erl:34-73`. 모든 프레임은 `{"op":N,"d":...,"s":seq?,"t":"EVENT"?}`이며 `s`와 `t`는 op 0에서만 쓴다(`TS/GatewaySocket.ts:166-183`).

| op | 이름 | 방향 | d 페이로드 |
|---|---|---|---|
| 0 | DISPATCH | S→C | `t`(이벤트명), `s`(시퀀스), `d`(이벤트 데이터) |
| 1 | HEARTBEAT | 양방향 | C→S: `d`=마지막 시퀀스(정수 또는 null). S→C: `d:null`(즉시 하트비트 요청) |
| 2 | IDENTIFY | C→S | 3.3절 참조 |
| 3 | PRESENCE_UPDATE | C→S | `{status, afk?, mobile?, custom_status?}`. `status` 필수(`gateway_handler_dispatch.erl:validate_presence_data`) |
| 4 | VOICE_STATE_UPDATE | C→S | `{guild_id, channel_id, self_mute, self_deaf, self_video, self_stream, viewer_stream_keys?, connection_id, p2p?, is_mobile, latitude?, longitude?}`(`TS/GatewaySocket.ts:629-642`) |
| 5 | VOICE_SERVER_PING | - | 서버가 인증 후 처리하지 않는다. 보내면 4001로 닫힌다(`gateway_handler_dispatch.erl:102-103`) |
| 6 | RESUME | C→S | `{token, session_id, seq}` 세 필드 모두 필수(`gateway_handler_resume.erl:validate_resume_data`) |
| 7 | RECONNECT | S→C | `d` 없음. 직후 서버가 코드 4000으로 닫는다(`gateway_handler_dispatch.erl:handle_session_reconnect`) |
| 8 | REQUEST_GUILD_MEMBERS | C→S | `{guild_id 또는 guild_ids[], query?, limit?, user_ids[]?, presences?, nonce?}`(`TS/GatewaySocket.ts:659-690`). 응답은 `GUILD_MEMBERS_CHUNK` |
| 9 | INVALID_SESSION | S→C | `d:false`(서버는 항상 false 전송. 클라이언트는 bool로 해석) |
| 10 | HELLO | S→C | `{heartbeat_interval: 41250}`(ms) |
| 11 | HEARTBEAT_ACK | S→C | `d` 없음 |
| 12 | GATEWAY_ERROR | S→C | `{code: string, message: string}`. 코드 목록은 `GatewayConstants.ts:49-76`(VOICE_* 중심) |
| 13 | VOICE_SIGNAL | C→S | `{guild_id, to, data}` 류(`TS/GatewaySocket.ts:118-122`). JSON 16000바이트 초과 금지(`:124,647-655`). 첫 화면에는 불필요 |
| 14 | LAZY_REQUEST | C→S | `{subscriptions: {<guildId>: {...}}}`. 4절 참조 |
| 15 | REQUEST_GUILD_COUNTS | C→S | `{guild_ids: string[]}` |
| 16 | REQUEST_CHANNEL_MEMBER_COUNTS | C→S | `{guild_id, channel_ids[], nonce?}` |
| 28 | REQUEST_FORUM_UNREADS | C→S | `{guild_id, channel_id, threads:[{thread_id, ack_message_id}]}` |

번호 17~27은 정의가 없다. 서버는 모르는 op를 4001로 닫는다.

## 3. 연결 수명

### 3.1 서버 동작 (HELLO와 하트비트)

- 소켓 오픈 즉시 HELLO를 보내고 하트비트 점검 타이머를 시작한다(`gateway_handler.erl:do_websocket_init`). `heartbeat_interval`은 41250ms(`constants.erl:heartbeat_interval`).
- 서버 점검 주기는 interval/3(약 13.75초)이다. 마지막 ACK 후 경과가 interval의 90% 이상이면 서버가 `op 1, d:null`을 보내고 `waiting_for_ack`를 켠다. 그 상태에서 마지막 ACK로부터 45000ms를 넘기면 4009로 닫는다(`gateway_handler_heartbeat.erl:evaluate_heartbeat`, `constants.erl:heartbeat_timeout`).
- 클라이언트 HEARTBEAT(`d`=seq)를 받으면 ACK(op 11)를 보낸다. 세션이 있고 `d`가 정수이면 서버 세션이 시퀀스를 검증하고 틀리면 4007로 닫는다. `d`가 null이거나 세션이 없으면 통과한다(`gateway_handler_heartbeat.erl:verify_heartbeat_ack`).
- 서버가 보낸 `op 1`에는 클라이언트가 즉시 op 1(`d`=seq)로 답해야 한다(`TS/GatewaySocket.ts:984-987`).

### 3.2 클라이언트 동작 (`TS/GatewaySocket.ts`)

- 타임아웃 상수(`GatewaySocket.ts:46-58`): HELLO 대기 20000ms, 하트비트 ACK 15000ms, RESUME 응답 15000ms, 재개 가능 창 180000ms, 최소 재접속 1000, 최대 10000(복원 화면 중 3000, 디스패치 연속 실패 시 60000), 바닥 500, 스프레드 2000, INVALID_SESSION 후 재식별 분산 1000.
- 하트비트 주기: `base = max(1000, 0.8*interval)`, 지터 `rand(0..min(1500, 0.05*interval))`. 첫 전송과 이후 전송에 같은 식을 쓴다(`GatewaySocket.ts:1195-1202`). interval은 HELLO 값이다.
- ACK를 기다리는 중 새 틱이 오면 `0.75*interval`(최소 500, 최대 15000)까지 전송을 미룬다. 15000ms 안에 ACK가 없으면 연결을 끊고 즉시 재접속 플래그를 켠다(`GatewaySocket.ts:1219-1276`).
- 데스크톱 엔진의 별도 클라이언트는 더 단순하다. 첫 하트비트를 `interval*random()` 뒤에 보내고 ACK 기한은 `interval+5000ms`이다(`fluxer_desktop/src/engine/GatewayClient.ts:322,342-353`). 최소 구현은 이 단순형을 따라도 서버 요건(45초 내 ACK 왕복)을 만족한다.
- HELLO를 받으면 하트비트를 시작하고 재개 가능하면 RESUME, 아니면 IDENTIFY를 보낸다(`GatewaySocket.ts:1043-1056`). 재개 가능 조건은 `session_id`가 있고 마지막 활동(메시지 수신, 하트비트 송수신) 후 180초 이내인 경우다(`:1369-1385`). 서버는 소켓 끊김 후 세션을 60초만 유지하므로(`ERL/utils/constants.erl:resume_timeout`, `ERL/session/session_monitor.erl:72-80`) 실제 재개 성공 창은 약 60초이다. 클라이언트의 180초는 느슨한 상한이다.

### 3.3 IDENTIFY 페이로드

클라이언트 송신(`TS/GatewaySocket.ts:1136-1150`), 서버 검증(`ERL/gateway/gateway_handler_identify.erl:198-330`).

| 필드 | 필수 | 내용 |
|---|---|---|
| `token` | 필수 | 문자열, 256바이트 이하, 공백/제어문자만 있으면 안 됨. 사용자 인증 토큰 그대로 |
| `properties` | 필수 | 객체. 서버가 강제하는 것은 `os`, `browser`, `device`가 문자열인 것뿐. 웹 클라이언트는 `os, os_version, browser, browser_version, device, system_locale, locale, user_agent, build_version, desktop_app_version?, desktop_app_channel?, desktop_arch?, desktop_os?, e2ee_capable?, latitude?, longitude?`를 보낸다(`fluxer_app/src/features/platform/utils/ClientInfo.ts:327-347`) |
| `presence` | 선택 | `{status, afk, mobile, custom_status?}`. 데스크톱 엔진은 `{status:"invisible", since:0, afk:false, activities:[]}`를 보낸다 |
| `flags` | 선택(기본 0) | 0 이상의 정수 비트마스크. 아래 참조 |
| `initial_guild_id` | 선택 | 스노플레이크 문자열. 첫 화면에서 보는 길드 ID(효과는 8절 불확실 항목) |
| `ignored_events` | 선택 | 문자열 배열(256개 이하). 웹 클라이언트는 보내지 않음 |
| `shard` | 선택 | 샤딩용. 웹 클라이언트는 보내지 않음 |

`flags`(`packages/constants/src/GatewayConstants.ts:30-34`): `DEBOUNCE_MESSAGE_REACTIONS=1<<1`, `CHANNEL_THREADS=1<<2`, `GUILD_VISITS=1<<3`. 웹은 셋 모두 켠다(`TS/PooledGatewayConnection.ts:459-466`). 비트 0은 정의가 없다. 데스크톱 엔진 클라이언트는 `config.identifyFlags`를 그대로 전달한다.

IDENTIFY 서버 제한: IP당 분당 300회(`ERL/gateway/session_abuse_protection.erl:16`). 토큰별 제한을 넘으면 서버가 처리를 보류하고 재시도한다(`gateway_handler_identify.erl:hold_identify`, 재시도 1000ms+지터 1000ms). IP당 연결 256개(`gateway_ip_connections.erl:11`), 사용자당 세션 100개(`session_abuse_protection.erl:19`).

### 3.4 READY (op 0, t=READY)

서버는 소유한 모든 길드가 연결되고 presence가 준비된 뒤에 READY를 보낸다(`ERL/session/session_ready.erl:check_readiness`). 구조는 `fluxer_app/src/features/gateway/events/GatewayReady.ts:64-85`와 서버 조립부 `ERL/session/session_ready_dispatch.erl:134-157`에서 확인했다.

`session_id`, `user`(UserPrivate), `guilds[]`(GuildReadyData 또는 `{id, unavailable:true}`), `private_channels[]`(DM 채널), `read_states[]`(`{id=channel_id, mention_count?, last_message_id?, last_pin_timestamp?, version?}`), `relationships[]`, `users[]`(참조용 사용자), `presences[]`, `user_settings`, `user_guild_settings[]`, `notes`, `pinned_dms`, `favorite_memes`, `rtc_regions`, `webauthn_credentials`, `auth_session_id_hash`, `country_code`, `latitude`, `longitude`, `version`, `sessions[]`, 스태프에게만 `_timings_gw`. 클라이언트가 필수로 취급하는 필드는 `user`, `guilds`, `session_id`이다.

READY를 받으면 클라이언트는 구독 ledger와 하트비트 이력을 초기화하고 백오프를 리셋하며 상태를 CONNECTED로 바꾼다(`TS/GatewaySocket.ts:1015-1031`). 봇 계정은 길드가 READY 뒤에 개별 `GUILD_CREATE`로 온다(`session_ready_dispatch.erl:dispatch_bot_guild_creates`).

### 3.5 RESUME, INVALID_SESSION, RECONNECT

- RESUME `d={token, session_id, seq}` 송신 후 15초 안에 `RESUMED`(t=RESUMED)가 없으면 세션을 버리고 재접속한다(`TS/GatewaySocket.ts:1425-1434`).
- 서버는 놓친 이벤트를 원래 `s`로 재전송한 뒤 `RESUMED`를 현재 시퀀스로 보낸다(`gateway_handler_resume.erl:finalize_resume`). 클라이언트는 RESUMED 때 presence를 다시 보낸다(`GatewaySocket.ts:1037`).
- 서버 응답: 세션 없음 또는 재개 불가는 `op 9 d:false`(연결 유지). 시퀀스 불일치는 4007. 토큰 불일치는 4004. 세션 프로세스 장애는 4000.
- INVALID_SESSION 수신(`GatewaySocket.ts:1090-1121`): RESUME 대기 중이고 `d`가 false이면 같은 연결에서 세션을 지우고 0~1000ms 뒤 IDENTIFY를 보낸다. 그 외에는 2500~3500ms 후 `d`가 false이면 세션을 지우고 코드 4000으로 끊어 즉시 재접속한다.
- RECONNECT 수신: 즉시 재접속 플래그를 켜고 4000으로 끊는다. 세션은 유지하므로 RESUME을 시도한다.

### 3.6 Close code 정책

코드 정의: `GatewayConstants.ts:35-48`, `ERL/utils/constants.erl:75-87`.

| 코드 | 의미 | 서버가 보내는 상황 | 클라이언트 정책 |
|---|---|---|---|
| 4000 | UNKNOWN_ERROR | RECONNECT 직전, 세션 장애 | 재접속(재개 시도) |
| 4001 | UNKNOWN_OPCODE | 모르는 op, 미지원 op | 재접속 |
| 4002 | DECODE_ERROR | 크기 초과, JSON/압축 오류, 잘못된 identify | 압축 오류면 비압축으로 재접속, 그 외는 치명적 오류로 처리 |
| 4003 | NOT_AUTHENTICATED | 인증 전 요청 | 재접속 |
| 4004 | AUTHENTICATION_FAILED | 토큰 무효 | 재접속하지 않음. 상태를 Disconnected로 두고 소유자에게 맡김(`GatewaySocket.ts:953-958`) |
| 4005 | ALREADY_AUTHENTICATED | 중복 IDENTIFY/RESUME | 재접속 |
| 4007 | INVALID_SEQ | 시퀀스 불일치 | 세션 폐기 후 재식별(`:959-966`) |
| 4008 | RATE_LIMITED | 속도 제한, 연결 수 초과 | 재접속(백오프 적용) |
| 4009 | SESSION_TIMEOUT | 하트비트 45초 초과 | 세션 폐기 후 재식별 |
| 4010~4012 | 샤드/버전 오류 | invalid_api_version(4012) 등 | 웹 클라이언트는 별도 처리 없음. 데스크톱 엔진은 재개 불가 코드로 취급(`fluxer_desktop/src/engine/GatewayClient.ts:35-42`) |

표에서 재접속이라고 적은 코드는 `handleSocketClose`가 사용자 종료가 아닐 때 `handleConnectionFailure`로 재접속을 예약하는 동작을 요약한 것이다(`GatewaySocket.ts:967-972`). 데스크톱 엔진은 자체 코드 ZOMBIE 4900, HANDOFF 4901을 정의한다(`GatewayClient.ts:28-29`). 일반 종료 1000/1001은 서버가 세션을 즉시 정리한다(`gateway_handler.erl:exit_on_client_close`).

### 3.7 재접속 백오프 (`GatewaySocket.ts:1305-1338`)

- 즉시 재접속 플래그(RECONNECT, 하트비트 실패, INVALID_SESSION, 압축 폴백)가 켜져 있으면 `rand(0..2000)ms` 후 재접속하고 백오프 카운터를 건드리지 않는다.
- 그렇지 않으면: 직전 시도 후 1000ms 미만이면 `1000 + rand(0..2000)`. 아니면 시도 횟수 n을 올리고 `ceiling = min(1000 * 2^(n-1), cap)`, 지연 = `500 + rand * (ceiling - 500)`. cap은 기본 10000ms이다. READY/RESUMED 수신 시 n을 0으로 리셋한다.
- 서버 속도 제한: 일반 메시지 분당 600건, presence 20초당 5건, forum unreads 5초당 5건, voice signal 10초당 240건(`ERL/gateway/gateway_handler_rate_limit.erl:12-19`). 일반 한도 초과는 4008로 닫고, op별 한도 초과는 해당 메시지를 조용히 버린다.

## 4. 길드 구독 (LAZY_REQUEST, op 14)

서버 정책: 멤버 250명을 초과하는 대형 길드에서 세션이 활성화하지 않은 길드는 passive 상태이다(`ERL/session/session_passive.erl:is_small_guild`). passive 길드에서는 CHANNEL_CREATE/UPDATE/DELETE, GUILD_UPDATE, 역할 변경, 자신에 대한 GUILD_MEMBER_*, GUILD_DELETE, 자신을 멘션하거나 자신이 쓴 MESSAGE_CREATE, 자신을 멘션한 MESSAGE_UPDATE, `PASSIVE_UPDATES`만 받는다(`session_passive.erl:66-140`). 멤버 250명 이하 길드는 항상 활성처럼 동작하고 봇은 항상 활성이다.

요청 형식(`TS/GatewaySocket.ts:695-698`, 타입 `TS/GatewaySubscriptionLedger.ts:3-14`):

```json
{"op":14,"d":{"subscriptions":{"<guild_id>":{
  "active":true, "sync":true,
  "member_list_channels":{"<channel_id>":[[0,99]]},
  "typing":true, "members":["<user_id>"],
  "threads":true, "thread_member_lists":["<thread_id>"],
  "focused_channel_id":"<channel_id 또는 null>"}}}}
```

- 첫 화면 최소 요청은 선택한 길드에 `{active:true, sync:true}`이다(`TS/GatewayGuildSubscriptionSynchronizer.ts:41-50`). 서버는 `active` 전환 시 passive를 해제하고 `send_guild_sync`(GUILD_SYNC 디스패치)를 실행한다. `sync:true`는 강제 재동기화이다(`ERL/guild/guild_unified_subscriptions.erl:process_sync_flag`).
- 멤버 목록 범위는 폭 100 이하(`MEMBER_LIST_RANGE_MAX_SPAN=99`), 끝 값 100000 이하, 채널당 범위 10개 이하, `members` 1000개 이하, `thread_member_lists` 10개 이하이다(`guild_unified_subscriptions.erl:15-20`, `GatewayConstants.ts:25-29`).
- 서버는 구독 상태를 세션에 저장하고 재개 시 다시 적용한다(`maybe_store_subscription_state`). 새 세션(READY)에서는 클라이언트가 구독을 다시 보내야 한다(클라이언트는 READY에서 ledger를 비운다, `GatewaySocket.ts:1020`).
- 요청 처리 워커는 연결당 4개까지이며 초과 요청은 버려진다(`gateway_handler_dispatch.erl:28-29,540-562`). 요청 하나는 10초 제한이다.
- 방문자(guild visit) 구독 길드는 `focused_channel_id`만 처리한다(`guild_unified_subscriptions.erl:process_visitor_focus`).
- `PASSIVE_UPDATES` d: `{guild_id, channels:{<channel_id>:<last_message_id>}, voice_states?:[...]}`. passive 길드의 채널별 `last_message_id` 변화만 담는다(`ERL/guild/guild_passive_sync.erl:225-290`). 안읽음 표시용이다.

## 5. DISPATCH 이벤트

이벤트 이름 목록은 클라이언트 라우터 `fluxer_app/src/features/gateway/events/EventRouter.ts:105-184`의 등록 이름이다. 서버가 보내는 이름과 완전히 일치하는지는 대조하지 않았다.

- 세션/계정: `READY`, `RESUMED`(라우터 핸들러 없이 전송 계층이 처리), `SESSIONS_REPLACE`, `AUTH_SESSION_CHANGE`, `USER_UPDATE`, `USER_SETTINGS_UPDATE`, `USER_GUILD_SETTINGS_UPDATE`, `USER_PINNED_DMS_UPDATE`, `USER_NOTE_UPDATE`, `USER_CONNECTIONS_UPDATE`, `WEBAUTHN_CREDENTIALS_UPDATE`, `OPERATOR_PASS_UPDATE`.
- 길드: `GUILD_CREATE`, `GUILD_UPDATE`, `GUILD_DELETE`, `GUILD_SYNC`, `GUILD_BAN_ADD/REMOVE`, `GUILD_EMOJIS_UPDATE`, `GUILD_STICKERS_UPDATE`, `GUILD_MEMBER_ADD/UPDATE/REMOVE`, `GUILD_MEMBERS_CHUNK`, `GUILD_MEMBER_LIST_UPDATE`, `GUILD_COUNTS_UPDATE`, `GUILD_HEALTH_UPDATE`, `GUILD_ROLE_CREATE/UPDATE/DELETE/UPDATE_BULK`, `CHANNEL_MEMBER_COUNTS_UPDATE`, `PASSIVE_UPDATES`.
- 채널/스레드: `CHANNEL_CREATE/UPDATE/UPDATE_BULK/DELETE`, `CHANNEL_PINS_UPDATE/ACK`, `CHANNEL_RECIPIENT_ADD/REMOVE`, `THREAD_CREATE/UPDATE/DELETE`, `THREAD_LIST_SYNC`, `THREAD_MEMBER_UPDATE`, `THREAD_MEMBERS_UPDATE`, `THREAD_MEMBER_LIST_UPDATE`, `FORUM_UNREADS`.
- 메시지: `MESSAGE_CREATE/UPDATE/DELETE/DELETE_BULK`, `MESSAGE_ACK`, `MESSAGE_REACTION_ADD/ADD_MANY/REMOVE/REMOVE_ALL/REMOVE_EMOJI`, `TYPING_START`, `RECENT_MENTION_DELETE`, `SAVED_MESSAGE_CREATE/DELETE`.
- 기타: `PRESENCE_UPDATE`, `PRESENCE_UPDATE_BULK`, `VOICE_STATE_UPDATE`, `VOICE_SERVER_UPDATE`, `VOICE_SIGNAL`, `CALL_CREATE/UPDATE/DELETE`, `ENTRANCE_SOUND_PLAY`, `INVITE_CREATE/DELETE`, `RELATIONSHIP_ADD/UPDATE/REMOVE`, `WEBHOOKS_UPDATE`, `FAVORITE_MEME_CREATE/UPDATE/DELETE`.

첫 화면(채널 목록, 메시지 읽기)에 필요한 이벤트와 핵심 필드:

| 이벤트 | 핵심 필드 | 근거 |
|---|---|---|
| READY | 3.4절 | `GatewayReady.ts:64-85`, `:115-196` |
| GUILD_CREATE | READY의 길드와 같은 GuildReadyData: `id, properties(이름, 아이콘 등), channels[], threads?, roles[], members[], member_count, online_count?, presences?, voice_states?, emojis[], stickers?, joined_at, unavailable?, visiting?, degraded?` | `fluxer_app/src/features/gateway/types/GatewayGuildTypes.ts:12-30` |
| GUILD_DELETE | `{id, unavailable?, unavailable_hidden?}` | `session_ready.erl:build_unavailable_state` |
| CHANNEL_CREATE/UPDATE/DELETE | 채널 객체: `id, guild_id, name, topic, type, position, parent_id, last_message_id, last_pin_timestamp, permission_overwrites[], recipients[], nsfw, rate_limit_per_user, bitrate, user_limit` 등 | `packages/schema/src/domains/channel/ChannelSchemas.ts:51-112` |
| MESSAGE_CREATE | 메시지 객체: `id, channel_id, guild_id?, author, content, timestamp, edited_timestamp, type, flags, pinned, mention_everyone, mentions[], mention_roles[], attachments?, embeds?, stickers?, reactions?, message_reference?, nonce?, call?, thread?, member?` | `packages/schema/src/domains/message/MessageResponseSchemas.ts:120-170`, `fluxer_app/src/features/messaging/events/MessageCreate.ts:19-47` |
| MESSAGE_UPDATE | `id`, `channel_id?` + 변경된 메시지 필드(부분 객체일 수 있음. 병합 규칙은 미확인) | `messaging/events/MessageUpdate.ts:16-18` |
| MESSAGE_DELETE | `{id, channel_id}` | `messaging/events/MessageDelete.ts:17-19` |
| MESSAGE_ACK | `{channel_id, message_id, mention_count, manual?, version?}`. 다른 기기가 채널을 읽었을 때 오는 읽음 동기화 | `messaging/events/MessageAck.ts:7-14` |
| PASSIVE_UPDATES | 4절 | `guild_passive_sync.erl` |

읽음 상태: READY의 `read_states[].last_message_id`는 사용자가 마지막으로 확인한 위치이고 채널 객체의 `last_message_id`는 채널의 최신 메시지로 보이며, 둘을 비교해 안읽음을 계산한다고 추정한다(계산 로직 `read_state/state/ReadStates.ts`는 정독하지 않았다). 메시지 히스토리는 게이트웨이가 아니라 REST `GET /channels/{id}/messages`로 받는다(`fluxer_app/src/features/app/constants/Endpoints.ts:74`). ack 전송은 REST `POST /channels/{id}/messages/{id}/ack`와 `POST /read-states/ack`, `/read-states/ack-bulk`이다(`Endpoints.ts:79,185-186`). 따라서 게이트웨이만으로는 메시지 목록을 채울 수 없다.

디스패치 처리 규칙: `s`가 현재 값보다 클 때만 시퀀스를 갱신한다(`GatewaySocket.ts:871-877`). 하트비트의 `d`에는 마지막 시퀀스를 넣는다. 웹 클라이언트는 디스패치 처리에 실패하면 세션을 버리고 재접속한다(`:1062-1072`). 이 동작은 선택 사항이다.

## 6. 세션 풀과 다중 연결

- 구조: `GatewaySessionPool`은 `PooledGatewayConnection` 하나(`foregroundConnection`)를 소유하고(`TS/GatewaySessionPool.ts:34`), 그 안에 `GatewaySocket`이 하나 있다(`TS/PooledGatewayConnection.ts:474`). 풀이라는 이름이지만 웹앱 안에서는 포그라운드 연결 하나뿐이다.
- 존재 이유(코드 근거): 다중 계정 전환 시 포그라운드를 내리고 올리는 인수인계(`beginForegroundDemotionHandover`, `retireForAccountSwitch`, `startResumedSession`)와 부팅 전 소켓 승계(`adoptPrebootSocket`, `PrebootNetworkHandoff`)를 위해서다. 백그라운드 계정은 Electron 엔진 프로세스의 `GatewayClient`가 별도 연결로 받으며, 이 연결은 IDENTIFY에 invisible presence를 넣고 사용하는 op는 IDENTIFY, RESUME, HEARTBEAT, LAZY_REQUEST뿐이다(`fluxer_desktop/src/engine/GatewayClient.ts:7-17,330-340`).
- 최소 구현에서는 생략 가능하다. 계정당 연결 하나와 HELLO, IDENTIFY/RESUME, 하트비트, 디스패치 루프만으로 서버 프로토콜은 충족된다. 풀, 핸드오프, 스냅샷(`snapshot/` 디스크 캐시), 디스패치 버퍼링(`MAX_DEFERRED_GATEWAY_EMITS=1024`, 16MB)은 UI 성능과 다중 계정 최적화이다.
- 다중 계정을 지원한다면 서버 제약을 지킨다: 사용자당 세션 100개, IP당 연결 256개, IP당 IDENTIFY 분당 300회.

## 7. 최소 구현 체크리스트

1. discovery 문서에서 `endpoints.gateway`를 읽고 `?v=1&encoding=json&compress=zstd-stream&stream=1`을 붙여 접속한다. 압축 문제가 있으면 `compress=none`으로 시작해도 된다.
2. HELLO를 받으면 `0.8*interval + jitter`마다 `{op:1,d:<마지막 s 또는 null>}`를 보내고, 서버의 op 1에는 즉시 응답하며, ACK가 15초 넘게 없으면 재접속한다.
3. 처음에는 IDENTIFY `{token, properties{os,browser,device,...}, presence?, flags, initial_guild_id?}`를 보낸다. 재접속이고 끊김 후 60초 안이면 RESUME `{token, session_id, seq}`를 보낸다.
4. READY에서 `guilds`, `private_channels`, `read_states`, `users`를 읽어 채널 목록을 만든다. 선택한 길드에 LAZY_REQUEST `{active:true, sync:true}`를 보낸다.
5. 메시지 본문은 REST로 가져오고, 이후 MESSAGE_CREATE/UPDATE/DELETE/ACK와 CHANNEL_*를 반영한다.
6. 4004는 재시도하지 않고, 4007과 4009는 세션을 버려 재식별하며, 나머지는 3.7절 백오프로 재접속한다.

## 8. 불확실 항목

- `initial_guild_id`가 READY 전에 서버에서 어떤 효과(해당 길드 활성/동기화)를 내는지 끝까지 따라가지 않았다. 클라이언트가 보낸 뒤 `markInitialGuildCompleted`로 완료 처리하는 것(`TS/GatewayGuildSubscriptionSynchronizer.ts:111-119`)에서 추정했다.
- `flags` 중 `DEBOUNCE_MESSAGE_REACTIONS`, `GUILD_VISITS`의 서버 측 영향은 확인하지 않았다. CHANNEL_THREADS만 `session_manager_shard_drain:thread_channels_capable` 호출을 확인했다.
- zstd 스트림에서 서버 `ezstd:compress_streaming`이 메시지마다 flush하는지는 라이브러리 소스로 확인하지 못했다. 클라이언트가 메시지 단위로 디코드하고 빈 출력을 추가 청크 대기로 처리하는 점(`TS/GatewaySocket.ts:897-903`)에서 추정했다. 실제 서버와의 수신 테스트가 필요하다.
- READY와 GUILD_CREATE의 세부 필드 스키마는 TS 타입과 Zod 스키마에서 요약했다. 길드 `properties` 내부 등 선택 필드는 `packages/schema`의 Guild/User 스키마를 따로 읽어야 한다.
- 서버가 보내는 전체 `t` 이름 목록은 서버의 `event_atoms` 모듈과 대조하지 않았다.
- 읽음/안읽음 계산 규칙과 MESSAGE_UPDATE 병합 규칙은 `ReadStates.ts`, `MessagingMessages.ts`를 읽지 않았다.
- 서버의 RESUME 보존 시간 60초와 `RESUME_OFFLINE_GRACE_MS`(값 미확인)의 관계를 확인하지 못했다.
- 서버가 클라이언트로 보내는 메시지의 크기 상한과 단편화 여부는 확인하지 못했다.
