# Rust 클라이언트용 REST 계층 및 로그인 흐름 명세

- 기준: 저장소 루트, 브랜치 `desktop-rs`, HEAD `d1124083`(작성 시점 working tree 기준, 읽기 전용 조사).
- 표기: 경로는 저장소 루트 기준이며 `경로:줄`이 근거다. "추정"은 코드에서 직접 확인하지 못한 항목이다.
- 이 문서에는 토큰·비밀값을 적지 않았다. 토큰 형식은 서버 정규식만 인용한다.
- 게이트웨이(WebSocket) 프로토콜은 범위 밖이다. 첫 화면 데이터가 게이트웨이로 오는지 여부만 4절에 적었다.

## 1. 인스턴스 discovery

### 1.1 절차 (웹 원본, TypeScript)

1. 입력 정규화: `resolveDiscoveryApiEndpoint(input)`가 `normalizeInstanceEndpoint(input, API)`로 정규화한다. 상대 경로이거나 http(s)가 아니면 오류다. `fluxer_app/src/features/app/state/InstanceSnapshotStore.ts:176-198`
2. 공식 인스턴스 별칭: 공식 호스트의 `https://<호스트>/` 형태이면 릴리스 채널에 따라 `https://web.fluxer.app/api`(stable) 또는 `https://web.canary.fluxer.app/api`(canary)로 치환한다. `packages/instance_bootstrap/src/OfficialInstance.ts:17-20`, `InstanceSnapshotStore.ts:184-197`
3. 후보 URL을 순서대로 GET한다. `packages/instance_bootstrap/src/Discovery.ts:261-281`
   - `<origin>/.well-known/fluxer` (상수 `Discovery.ts:49`)
   - `<origin><입력에 포함된 경로 또는 알려진 경로>/.well-known/fluxer`
   - `<origin>/api/.well-known/fluxer` (`Discovery.ts:50`)
   - 먼저 성공한 후보를 채택하고, 모두 실패하면 `InstanceDiscoveryUnreachableError`다(`Discovery.ts:282-300`).
4. 요청 조건: `Accept: application/json`, 리디렉션 거부(`redirect: error`), `credentials: omit`, `cache: no-store`, 타임아웃 15초, 본문 최대 1 MiB, 응답 `Content-Type`은 JSON이어야 한다. 304이면 캐시 재사용(ETag/Last-Modified 조건부 요청). `Discovery.ts:51-53,330-362,455-495`
5. origin 검증: 문서의 `endpoints.api`/`endpoints.api_client` origin이 문서를 서빙한 origin과 같아야 한다. 다르면 `InstanceDiscoveryOriginMismatchError`. `Discovery.ts:433-446`
6. 캐시: 스냅샷 TTL 1시간, 최대 256개. `InstanceSnapshotStore.ts:34-35`
7. 웹 전용: `meta name=fluxer-api-origin content=self`가 있는 문서는 같은 origin의 `/api`를 쓴다(`InstanceSnapshotStore.ts:147-174`). Rust 클라이언트에는 해당 없다.

### 1.2 discovery 문서에서 얻는 값

- 서버는 `GET /.well-known/fluxer`를 제공한다(`fluxer_api/src/api/instance/InstanceController.ts:252`). 응답의 `endpoints`는 `api, api_client, api_public, api_transport(선택), gateway, media, upload_relay, static_cdn, marketing, admin, invite, gift, webapp`이다(`InstanceController.ts:109-123`).
- 파서가 필수로 요구하는 값: `api_code_version`(정수 1 이상), `endpoints.api`(http/https), `endpoints.gateway`(ws/wss). 나머지 엔드포인트는 null 허용이지만 클라이언트 투영 단계에서 `api_client, api_public, media, static_cdn, marketing, admin, invite, gift, webapp`을 필수로 요구한다. `Discovery.ts:302-325,535-564`, `packages/instance_bootstrap/src/ClientRuntimeDiscovery.ts:75-118`
- 클라이언트가 실제로 쓰는 매핑: REST base = `endpoints.api_client`, gateway = `endpoints.gateway`, media = `endpoints.media`. `InstanceSnapshotStore.ts:93-111`, `ClientRuntimeDiscovery.ts:100-118`
- API 버전: `api_code_version`이 URL의 `/v{N}` 접두에 쓰인다. 서버 상수는 `API_CODE_VERSION = 1`(`packages/constants/src/AppConstants.ts:3`). 서버는 `/v1/*`와 `/*` 양쪽을 모두 마운트한다(`fluxer_api/src/api/App.ts:76-77`).
- `api_transport`는 공식 인스턴스의 허용 목록과 일치할 때만 채택한다(`ClientRuntimeDiscovery.ts:121-125`). 셀프호스트에서는 항상 무시된다.
- discovery 문서에는 captcha 설정, SSO, 가입, 한도, 로그인 방식(`identifier_first`) 등도 있다. 로그인 UI 분기에 필요하면 `Discovery.ts:302-325`의 필드를 추가로 파싱해야 한다.

### 1.3 데스크톱(Electron) 차이

- Electron 렌더러는 `desktopRuntimeResolve` IPC로 discovery를 메인 프로세스에 위임한다(`InstanceSnapshotStore.ts:521-524,551-593`). 렌더러의 REST는 로컬 프로토콜 프록시를 거친다(`fluxer_app/src/features/platform/transport/InstanceHTTP.ts:91-100`, `fluxer_app/src/features/platform/DesktopLocalAppRuntime.ts:38-41`). 프록시 정책은 `fluxer_desktop/src/main/LocalAppProxyHTTPPolicy.ts:27-80`이다. Rust 네이티브 클라이언트는 이 프록시가 필요 없고 직접 연결하면 된다(추정).

### 1.4 구 `fluxer_desktop_rs` discovery 구현의 현황과 차이 (`fluxer_desktop_rs/src/ipc.rs:415-495`)

- 후보는 `https://{d}/.well-known/fluxer`, `https://{d}/api/.well-known/fluxer`, 그 뒤 `http://` 두 개다. 첫 성공을 채택한다.
- 차이점(원본 대비 누락): 리디렉션 거부, 15초 대신 8초 타임아웃, 1 MiB 제한, Content-Type 검사, origin 일치 검증, `api_code_version`/필수 필드 검증, 공식 인스턴스 별칭 매핑이 없다.
- 보안 주의: `danger_accept_invalid_certs(true)`로 TLS 인증서 검증을 끈다(`ipc.rs:429-433`). `fetchProxy`도 동일하다(`ipc.rs:529-533`). 새 코어는 TLS 인증서 검증을 끄지 않는다.
- 기존 구현은 `instanceKey`/`apiEndpoint`를 `endpoints.api`로 채우고 `endpoints.api_client`를 쓰지 않는다. 원본은 `api_client`를 쓴다(`ClientRuntimeDiscovery.ts:104`). 서버 기본값에서는 두 값이 같다(`InstanceController.ts:111-112`).

## 2. 요청 공통 규약

### 2.1 URL 구성

- `<api_client 끝 슬래시 제거>/v<api_code_version><path>`, 쿼리는 URLSearchParams로 붙인다. null/undefined 값은 생략한다. `fluxer_app/src/features/platform/transport/RestTransport.ts:495-516`(핵심은 504행)
- 경로 상수 목록: `fluxer_app/src/features/app/constants/Endpoints.ts`.

### 2.2 헤더

| 헤더 | 값/조건 | 근거 |
|---|---|---|
| `Authorization` | 로그인 응답의 `token` 원문. Bearer 접두 없음. 서버는 접두 없는 값을 session 토큰으로 처리하고 Bearer 접두가 붙은 session 토큰도 인식한다. 세션 토큰 정규식은 `^flx_[A-Za-z0-9]{36}$` | `RestTransport.ts:613-616`, `fluxer_api/src/api/middleware/UserMiddleware.ts:37-76` |
| `Content-Type` | JSON 본문이면 `application/json`, URLSearchParams는 `application/x-www-form-urlencoded;charset=UTF-8`, FormData는 생략 | `RestTransport.ts:627-638` |
| `X-Fluxer-Features` | `view_channel_members_permission,channel_threads` | `RestTransport.ts:606` |
| `X-Fluxer-Capabilities` | `guild_visits` | `RestTransport.ts:607` |
| `X-Fluxer-Client-Installation-ID` | 설치별 영속 ID(저장 키 `ClientInstallationId`). 형식은 미확인(추정: 랜덤 ID) | `RestTransport.ts:608`, `fluxer_app/src/features/platform/state/ClientInstallationId.ts:5,27-33` |
| `Accept-Language` | 로그인·가입 계열 호출에서만 명시 | `AuthenticationCommands.ts:34-37` |
| `X-Audit-Log-Reason` | URL 인코딩한 사유(선택) | `RestTransport.ts:612` |
| `X-Captcha-Token` | 캡차 해결 후 재시도 시(3.4절) | `fluxer_app/src/features/auth/altcha/CaptchaInterceptor.ts:65` |
| `X-Fluxer-Sudo-Mode-JWT` | sudo 모드 토큰이 있을 때(소문자 `x-fluxer-sudo-mode-jwt`) | `RestTransport.ts:48,617-618` |

- 위 `X-Fluxer-*`, `Authorization`은 "같은 origin"일 때만 붙는다(`RestTransport.ts:463,605,613,617`). 브라우저 보안 모델이며, `InstanceHTTP`는 cross-instance 요청에서 `auth: none`으로 바꾸고 호출자가 `Authorization`을 직접 넣는다(`InstanceHTTP.ts:113-123`, 예: `fluxer_app/src/features/auth/state/AccountAccess.ts:355-364`). 새 코어는 대상 인스턴스에 이 헤더를 항상 붙인다(`rest/client.rs`).
- `User-Agent`: 웹 앱은 브라우저 값을 쓰며 별도로 설정하지 않는다. 서버는 `Fluxer` 뒤에 `Android, iOS, Linux, Desktop, Client` 중 하나가 오는 UA를 "네이티브 클라이언트"로 인식하고(`fluxer_api/src/api/utils/SessionClientIdentity.ts:48,83-85`), 그 경우에만 `X-Fluxer-Client-Properties`(base64 JSON, 최대 4096자, 필드 `os, client_platform, client_version, release_channel, build_number, beta_number`)를 읽어 세션의 OS·앱 정보로 기록한다(`fluxer_api/src/api/auth/AuthSession.ts:73-80,171-173`, `SessionClientIdentity.ts:57-125`). 새 코어는 `Fluxer Desktop/<버전>` UA와 base64 JSON `{os, client_platform: "desktop", client_version, release_channel}` 형식의 `X-Fluxer-Client-Properties`를 보낸다(`rest/client.rs`, 서버 필수는 아님).
- CORS 허용 헤더 목록에는 `X-Fluxer-Platform`, `X-Request-ID`, `X-Captcha-Id`도 있으나 웹 앱은 보내지 않는다. 응답 노출 헤더는 `X-Fluxer-Version`, `ETag`. `fluxer_api/src/api/app/MiddlewarePipeline.ts:72-86`

### 2.3 오류 응답 JSON

- 형식: `{code: string, message: string, errors?: [{path, code?, message}], ...추가 필드}`. 추가 필드는 오류별 데이터이며 최상위에 펼쳐진다. `packages/schema/src/domains/common/ErrorSchemas.ts:12-21`, `packages/errors/src/FluxerError.ts:46-52`
- 인증 실패는 401 `UNAUTHORIZED`(`packages/errors/src/domains/core/UnauthorizedError.ts:6`, `fluxer_api/src/api/middleware/AuthMiddleware.ts:17-21`).
- 429 본문: `{code: RATE_LIMITED, message, retry_after(초, 소수), global(bool)}` (`ErrorSchemas.ts:23-31`, `packages/errors/src/domains/core/RateLimitError.ts:35-75`).

### 2.4 429 처리와 rate limit 헤더

- 서버 제한 헤더(성공 응답에는 봇·웹훅에게만 붙고 일반 사용자 세션에는 붙지 않는다): `X-RateLimit-Limit`, `X-RateLimit-Remaining`, `X-RateLimit-Reset`(epoch 초), `X-RateLimit-Reset-After`(초, 소수), `X-RateLimit-Bucket`(버킷 해시 16자). `fluxer_api/src/api/middleware/RateLimitMiddleware.ts:55-57,142-146`
- 429 응답에는 사용자 요청에도 위 헤더와 `X-RateLimit-Scope`가 붙고, 전역 제한이면 `X-RateLimit-Global: true`만 붙는다(`RateLimitError.ts:61-80`). `Retry-After`는 부모 클래스에서 설정되는 것으로 보인다(추정). `Retry-After`가 없을 때를 대비해 본문 `retry_after`도 읽어야 한다.
- 버킷은 라우트별 "클라이언트 식별자:버킷명(경로 파라미터 치환)"이며 클라이언트가 버킷 ID를 사전 학습할 방법은 없다. 원본 클라이언트는 "METHOD 경로" 문자열을 키로 쓴다. `RateLimitMiddleware.ts:98-104`, `RestTransport.ts:480`
- 전역 제한: 사용자당 초당 50건(플래그 `HIGH_GLOBAL_RATE_LIMIT`이면 1200). 창은 1000 ms. `RateLimitMiddleware.ts:95-100`, `fluxer_api/pkgs/rate_limit/src/RateLimitService.ts:63`
- 중요 위험: 전역 제한을 초과하면 서버가 해당 session 토큰을 폐기(revoke)한다. `RateLimitMiddleware.ts:159-169,201-204`. 새 코어(`rest/client.rs`)는 서버 전역 한도를 `SERVER_GLOBAL_LIMIT_PER_SECOND = 50`으로 두고, 클라이언트가 시작하는 요청은 기본 25건/초(`DEFAULT_REQUESTS_PER_SECOND`), 최대 40건/초(`MAX_REQUESTS_PER_SECOND`)로 제한하며 동시 요청은 8개(`MAX_IN_FLIGHT_REQUESTS`)로 묶는다.
- 원본 클라이언트의 처리 (`RestTransport.ts:863-865,900-917`, `RestTransport.ts:640-692`):
  1. 429 수신 시 `Retry-After`(초 또는 HTTP 날짜), `X-RateLimit-Reset-After`(초에 1000을 곱한 ms)를 읽는다. 둘 다 없거나 0 이하면 1000 ms. 둘이 있으면 `Reset-After`가 우선한다.
  2. 키 "METHOD 경로"에 만료 시각(현재 + ms)을 기록하고, 이후 같은 키 요청은 만료까지 서버에 보내지 않고 합성 429(`retry-after` 헤더, 본문 `retry_after`, `global: false`)를 반환하거나 대기한다. `/reports/` 경로는 페이싱 제외.
  3. 모드: `strict`(기본, 즉시 오류), `silent`(429 응답을 그대로 반환), `auto-retry`(만료까지 대기 후 재시도). 기본 모드는 `strict`(`RestTransport.ts:486`).
- 메시지 전송은 별도 정책: `retry_after`가 자동 재시도 한도 이하이면 자동 재시도하고 그렇지 않으면 사용자에게 오류를 노출한다. `fluxer_app/src/features/messaging/state/MessageQueue.tsx:180-190,1033-1050`. 한도 상수 값은 확인하지 못했다.

### 2.5 재시도 정책

- 기본 재시도 0회, 기본 타임아웃 0(무제한)이다(`RestTransport.ts:202-203`). 호출자가 `retries`를 지정한 경우만 재시도한다. 예: 메시지 목록 `retries: 2`(`fluxer_app/src/features/messaging/commands/MessageCommands.tsx:292-296`), 세션 목록 `retries: 2`(`fluxer_app/src/features/auth/commands/AuthSessionCommands.ts:18`), 토큰 검증·로그아웃 `retries: 0`.
- 재시도 대상: 전송 오류(네트워크), 상태 502, 504, 507, 522, 523, 524, 598, 599 (`RestTransport.ts:47,859-873`). 429는 2.4절 규칙을 따르며 `strict`에서는 재시도하지 않는다.
- 백오프(`backoff` 모드): `min(1000 * 2^n, 30000)` ms에 ±25% 지터, n은 0부터(최대 16). `RestTransport.ts:43-46,1036-1042`
- 재시도 시작 전 `navigator.onLine`이 거짓이면 온라인 복귀까지 대기한다(`RestTransport.ts:410,436-438`). 새 코어는 아직 네트워크 오류·502/504 계열 자동 재시도를 하지 않으며, 429만 설정된 횟수와 대기 한도 안에서 재시도한다.
- sudo 모드: 오류 코드가 sudo 필요/검증 실패이면 프롬프트 후 같은 요청을 sudo 토큰·코드와 함께 재시도한다(`RestTransport.ts:311-395`). 첫 화면과 로그인에는 필요 없다.
- 403 + 콘텐츠 차단 코드, `network_blocked` 제한은 UI 모달을 띄우는 처리이며 새 코어는 오류 코드와 상태를 `RestError::Api`로 전달한다. `RestTransport.ts:878-889`

## 3. 로그인 흐름

인증 호출은 토큰이 필요 없고(OpenAPI security 빈 배열) LocalAuthMiddleware를 거친다. 요청 경로는 `<api_client>/v1/auth/...`이다.

### 3.1 이메일/비밀번호 로그인

- `POST /auth/login` (`fluxer_app/src/features/app/constants/Endpoints.ts:11`, `fluxer_api/src/api/auth/AuthController.ts:197-218`)
- 본문: `{email, password}` 또는 `{login, password}`(username 인스턴스 또는 `login` 방식), 선택 `invite_code`. `fluxer_app/src/features/auth/commands/AuthenticationCommands.ts:218-228`, `packages/schema/src/domains/auth/AuthSchemas.ts:78-101`
- 제한: 10초당 10회(`fluxer_api/src/api/rate_limit_configs/AuthRateLimitConfig.ts:11-14`). CaptchaMiddleware가 적용된다.
- 성공 응답(토큰): `{token, user_id, user: UserPartial, device_proof?}`. `AuthSchemas.ts:208-220`. 클라이언트 타입은 `AuthenticationCommands.ts:56-62`.
- 응답 구분: `mfa === true`이면 MFA 응답, 아니면 토큰 응답이다(`AuthenticationCommands.ts:64-77`).

### 3.2 MFA 분기

- MFA 응답: `{mfa: true, ticket, allowed_methods: string[], totp: bool, webauthn: bool, backup_codes: bool}` (`AuthSchemas.ts:226-233`).
- TOTP: `POST /auth/login/mfa/totp` 본문 `{code, ticket, invite_code?}` → 토큰 응답(`AuthenticationCommands.ts:322-338`, `AuthController.ts:222`, `AuthSchemas.ts:412-415`). 제한 분당 5회(`AuthRateLimitConfig.ts:23-26`). 백업 코드가 같은 `code` 필드로 처리되는지는 미확인(추정: 서버가 TOTP와 백업 코드를 모두 허용).
- WebAuthn(보안 키):
  1. `POST /auth/login/mfa/webauthn/authentication-options` 본문 `{ticket}` → PublicKeyCredentialRequestOptionsJSON(`AuthenticationCommands.ts:364-383`, `AuthController.ts:604`).
  2. 인증자 서명 후 `POST /auth/login/mfa/webauthn` 본문 `{response, challenge, ticket, invite_code?}` → 토큰 응답(`AuthenticationCommands.ts:340-362`, `AuthController.ts:631`). `challenge`의 출처(옵션 응답 필드 여부)는 `fluxer_app/src/features/auth/utils/WebAuthnUtils.ts`에서 추가 확인이 필요하다.
  3. Rust 클라이언트에서 WebAuthn 인증자 호출은 OS API가 필요하다. 저장소에 `fluxer_desktop/native/webauthn`이 있으나 이번 조사에서 내용은 읽지 않았다.
- 패스키 로그인(비밀번호 없음): `POST /auth/webauthn/authentication-options`(선택 쿼리 `rp_id`) → `POST /auth/webauthn/authenticate` 본문 `{response, challenge, invite_code?}` → 토큰 응답(`AuthenticationCommands.ts:385-429`, `Endpoints.ts:15-16`).

### 3.3 IP 인증 요구

- 새 IP에서 로그인하면 HTTP 403, 코드 `IP_AUTHORIZATION_REQUIRED`, 본문에 `ticket`, `email`, `resend_available_in`이 포함된다. `AuthenticationCommands.ts:276-285`, `fluxer_app/src/features/platform/utils/ResponseInspection.ts:74-90`
- 이후: `POST /auth/authorize-ip`(메일 링크 토큰), `POST /auth/ip-authorization/resend`, `GET /auth/ip-authorization/poll?ticket=...`(완료 시 토큰 발급 형태는 미확인). `Endpoints.ts:43-49`, `AuthenticationCommands.ts:644-694`

### 3.4 캡차(ALTCHA)

- 서버가 캡차를 요구하면 HTTP 400, `code`가 `CAPTCHA_REQUIRED` 또는 `INVALID_CAPTCHA`이고 본문에 `captcha_provider: altcha`, `altcha_challenge: {parameters, signature}`가 들어 있다. `fluxer_app/src/features/auth/altcha/CaptchaInterceptor.ts:26-31`, `fluxer_app/src/features/auth/altcha/AltchaSolver.ts:21-27`, `fluxer_api/src/api/middleware/CaptchaMiddleware.ts:55-93`, `packages/errors/src/CaptchaErrors.ts`
- 캡차가 꺼진 인스턴스에서는 검증을 통과한다(`CaptchaMiddleware.ts:77-82`). 요구 여부는 로그인 시도 전에는 알 수 없고 400 응답으로 안다. 적용 대상: `/auth/login`, 가입, 비밀번호 찾기, 복구 키 재설정(`AuthController.ts` 설명 문구).
- 해결 절차: PBKDF2 계열 작업증명(알고리즘 `PBKDF2/SHA-256`, `PBKDF2/SHA-384`, `PBKDF2/SHA-512`, `cost`, `keyLength` 기본 32)을 풀어 `solution`을 얻는다. 헤더 `X-Captcha-Token` 값은 `{challenge: {parameters, signature}, solution}` JSON을 base64로 인코딩한 문자열이다. 같은 요청을 이 헤더와 함께 재전송한다. 최대 2회 해결 시도(`MAX_SOLVES = 2`), 풀이 제한 시간 60초. `CaptchaInterceptor.ts:22-23,57-77`, `AltchaSolver.ts:29-40`, `fluxer_app/src/features/auth/altcha/AltchaKeyDerivation.ts:8-20`
- 새 코어는 ALTCHA v2 PBKDF2 풀이를 구현했다(`rest/altcha.rs`). `PBKDF2(password = nonce || u32_be(counter), salt, cost, keyLength)`를 계산해 `keyPrefix`와 일치하는 `counter`를 찾고, `X-Captcha-Token`에 base64 JSON `{challenge: {parameters, signature}, solution: {counter, derivedKey, time}}`을 담는다. `parameters`는 서버 HMAC 검증 때문에 원문 그대로 되돌려 보낸다. 알고리즘은 `PBKDF2/SHA-256`, `PBKDF2/SHA-384`, `PBKDF2/SHA-512`를 지원하고, 요청 계층이 한 호출당 최대 2회(`MAX_CAPTCHA_SOLVES`) 자동으로 풀어 재전송한다.
- 비밀번호 오류 연속 시 서버가 난이도를 올린다(`escalatedCaptchaCost`, 최대 6단계). `CaptchaMiddleware.ts:40,50-53`

### 3.5 토큰 발급 후 처리

- `completeLogin`은 계정 레코드(`userId`, `token`, `userData`, 인스턴스 스냅샷)를 저장하고 해당 계정으로 전환한다. `AuthenticationCommands.ts:822-834`
- 토큰 검증은 `GET /users/@me`에 `Authorization` 헤더로 토큰을 보내 `id`가 저장된 `userId`와 같은지 확인한다. 401이면 무효, 그 외 오류는 "확인 불가"로 취급한다. `fluxer_app/src/features/platform/state/AuthSession.ts:525-536`
- `device_proof`는 인증 토큰이 아니며 이후 로그인에 보낼 값이다. 계정·인스턴스별 저장 키 접두 `fluxer.device_proof.`(`fluxer_app/src/features/auth/state/AccountEntryStorage.ts:11,41`). 사용처는 계정 진입(account entry) 흐름이며(`AccountEntryCommands.ts:60,70,190`) 일반 이메일 로그인 본문에는 쓰이지 않는다.

### 3.6 로그아웃

- `POST /auth/logout`, `Authorization` 헤더에 토큰, 본문 없음, 응답 204. 5초 타임아웃, 재시도 0. `fluxer_app/src/features/platform/state/auth_session/AuthSessionDependencies.ts:169-181`, `fluxer_api/src/api/auth/AuthController.ts:243-262`
- 서버는 해당 세션 토큰을 폐기한다. 이메일·비밀번호·패스키가 없는 계정의 마지막 세션이면 24시간 뒤 삭제 예약된다(`AuthController.ts:243-262` 설명과 `scheduleAbandonedOnLastLogout`). 봇 토큰은 204만 반환하고 토큰은 유지된다.
- 클라이언트 측: `Accounts.logout({notifyServer})`가 서버 호출 여부를 선택한다(`AuthenticationCommands.ts:627-631`). 제한 10초당 20회(`AuthRateLimitConfig.ts:105-108`).
- 다른 세션 종료: `GET /auth/sessions`, `POST /auth/sessions/logout`(`Endpoints.ts:25-26`, `AuthSessionCommands.ts`).

### 3.7 데스크톱 전용 차이와 기타 로그인 경로

- 브라우저 로그인 핸드오프: 데스크톱 앱이 `POST /auth/handoff/initiate`(인증 없음)로 `{code, expires_at, poll_secret?, return_method?}`를 받고, 사용자가 브라우저에서 승인하면 `GET /auth/handoff/{code}/status`를 폴링해 `{status: pending|completed|denied|expired, token?, user_id?, user?}`를 받는다. 승인 측은 `/auth/handoff/complete`, `/auth/handoff/{code}/info`, `/auth/handoff/{code}/deny`. `AuthenticationCommands.ts:134-163,696-800`, `Endpoints.ts:27-31`. 폴링 요청에 `poll_secret`을 어떻게 싣는지는 `AuthenticationCommands.ts:719-742`에서 확인해야 한다(이번에는 미확인).
- SSO: `POST /auth/sso/start` → `{authorization_url, state, redirect_uri, client_binding}`, 이후 `POST /auth/sso/complete`(code, state, client_binding). `AuthSchemas.ts:195-205`, `Endpoints.ts:51-52`
- 이메일 링크 로그인: `POST /auth/sign-in-link` 본문 `{token}`(`AuthenticationCommands.ts:633-642`).

## 4. 첫 화면에 필요한 REST 엔드포인트

원본 웹 앱의 첫 화면 데이터는 게이트웨이 READY 이벤트로 온다. REST는 보조·복구·페이지네이션용이다.

- READY 페이로드(게이트웨이): `session_id, guilds[](각 길드의 id, properties, channels, members, roles, emojis 등), user, private_channels, relationships, user_settings, user_guild_settings, read_states, presences` 등. `fluxer_app/src/features/gateway/events/GatewayReady.ts:64-84`, `fluxer_app/src/features/gateway/types/GatewayGuildTypes.ts:12-24`. 게이트웨이 IDENTIFY는 `{token, properties, presence?, flags, initial_guild_id?}`(`fluxer_app/src/features/gateway/transport/GatewaySocket.ts:1135-1146`).
- 따라서 길드·채널·DM 목록은 게이트웨이에서 오고, REST 대체 경로는 아래와 같다.

| 용도 | 메서드·경로 | 비고·응답 | 제한(근거: `fluxer_api/src/api/rate_limit_configs/`) |
|---|---|---|---|
| 현재 사용자 | `GET /users/@me` | `UserPrivateResponse`(`id`, 이메일, 플래그 등, `packages/schema/src/domains/user/UserResponseSchemas.ts:78-90`). 컨트롤러 `fluxer_api/src/api/user/controllers/UserAccountController.ts:112-125` | 10초당 40 (`UserRateLimitConfig.ts:115-118`) |
| 길드 목록 | `GET /users/@me/guilds` (쿼리 `before, after, limit, with_counts`) | `GuildResponse[]`, `limit` 1-200 기본 200. `fluxer_api/src/api/guild/controllers/GuildBaseController.ts:88-110`, `packages/schema/src/domains/guild/GuildRequestSchemas.ts:337-342` | 10초당 40 (`GuildRateLimitConfig.ts:11-14`) |
| 길드 채널 | `GET /guilds/{guild_id}/channels` | `ChannelListResponse`. `fluxer_api/src/api/guild/controllers/GuildChannelController.ts:30-45` | 10초당 60 (`GuildRateLimitConfig.ts:59-62`) |
| DM 채널 | `GET /users/@me/channels` | 열린 비공개 채널 목록. `fluxer_api/src/api/user/controllers/UserChannelController.ts:17-30` | 10초당 40 (`UserRateLimitConfig.ts:127-130`) |
| 메시지 목록 | `GET /channels/{channel_id}/messages` (쿼리 `limit, before, after, around`) | 4.1 | 10초당 100 (`ChannelRateLimitConfig.ts:23-26`) |
| 메시지 전송 | `POST /channels/{channel_id}/messages` | 4.2 | 10초당 20 (`ChannelRateLimitConfig.ts:35-38`) |
| 읽음 ack(권장) | `POST /read-states/ack` | 4.3 | 10초당 20 (`MiscRateLimitConfig.ts:39-42`) |
| 읽음 ack(단건) | `POST /channels/{channel_id}/messages/{message_id}/ack` | 본문 `{mention_count?, manual?}`, 204. 일반 사용자 전용 | 10초당 100 (`ChannelRateLimitConfig.ts:55-58`) |

### 4.1 메시지 목록

- 쿼리: `limit` 1-100(기본 50), `before`, `after`, `around`(스노플레이크, 선택). `packages/schema/src/domains/message/MessageRequestSchemas.ts:402-408`
- 응답: 최신순(newest first) 메시지 배열(`MessageListResponse = MessageResponse[]`, `packages/schema/src/domains/message/MessageResponseSchemas.ts:237`). 주요 필드: `id, channel_id, author, type, flags, content, timestamp, edited_timestamp, pinned, mention_everyone, tts, mentions, mention_roles, embeds, attachments, stickers, reactions, message_reference, nonce, referenced_message`. `MessageResponseSchemas.ts:100-176`
- 서버 핸들러: `fluxer_api/src/api/channel/controllers/MessageController.ts:52-82`
- 원본 호출: 쿼리 `before, after, limit, around`를 넘기고 `retries: 2`. 페이지 끝 판정은 반환 개수와 `limit` 비교로 한다(`fluxer_app/src/features/messaging/commands/MessageCommands.tsx:285-320`의 `resolveMessagePageState`; 세부 규칙 미확인).
- 여러 채널 일괄: `POST /channels/messages/bulk` 본문 `{requests: [{channel_id, limit(1-50), before?, after?, around?}]}`(최대 25개 채널, 합계 250개). `MessageRequestSchemas.ts:411-432`, `MessageController.ts:84-90`

### 4.2 메시지 전송

- JSON 본문(모든 필드 선택, partial 스키마): `content`, `embeds`, `attachments`, `message_reference`, `allowed_mentions`, `flags`(기본 0), `nonce`, `favorite_meme_id`, `sticker_ids`(최대 3), `tts`. `MessageRequestSchemas.ts:338-358`
- `nonce`: 문자열 1-32자 또는 음이 아닌 정수(문자열로 변환). 원본 클라이언트는 `SnowflakeUtils.fromTimestamp(Date.now())` 같은 스노플레이크형 값을 쓴다. `MessageRequestSchemas.ts:318-328`, `MessageCommands.tsx:1152`. 서버 응답의 `nonce`는 낙관적 UI 대조용이다(`MessageResponseSchemas.ts` `nonce` 필드).
- 첨부가 있으면 multipart/form-data: `payload_json`(JSON 문자열)과 `files[0]`, `files[1]` 등의 파트. 미리 서명된 업로드 방식은 `POST /channels/{id}/attachments`. `fluxer_app/src/features/messaging/state/MessageQueue.tsx:958-1000`, `MessageController.ts:182-200,225-235`. 새 코어는 텍스트/JSON 전송과 multipart 첨부 전송을 모두 구현했고, 사전 서명 업로드는 아직 구현하지 않았다.
- 응답: `MessageResponse` 하나(200).

### 4.3 읽음 ack

- `POST /read-states/ack` 본문 `{read_states: [{channel_id, message_id, mention_count?, manual?}]}`(1-100개), 응답 `{read_states: [권위 있는 읽음 상태]}`. `packages/schema/src/domains/channel/ChannelRequestSchemas.ts:444-465`, `fluxer_api/src/api/read_state/ReadStateController.ts:17-40`
- 원본은 이 경로를 우선 쓰고 404/405이면 구형 `POST /read-states/ack-bulk`(자동 ack)와 단건 ack(수동)로 대체한다. `fluxer_app/src/features/read_state/state/ReadStates.ts:1050-1090`

## 5. 토큰 저장 위치

| 환경 | 저장소 | 암호화 | 근거 |
|---|---|---|---|
| 웹 브라우저 | IndexedDB DB `FluxerAccounts`(v2), 스토어 `accounts`, keyPath `userId` | 암호화 없음(추정, 코드에 암호화 호출 없음) | `fluxer_app/src/features/auth/state/BrowserAccountStorageRepository.ts:8-10,424` |
| 웹(IndexedDB 불가 시) | localStorage 키 `fluxer:accounts:fallback`에 JSON | 없음 | `fluxer_app/src/features/auth/state/BrowserAccountFallbackStore.ts:8,90` |
| Electron | 메인 프로세스 SQLite `desktop-app-store.sqlite3`의 `accounts` 테이블(`storage_key, user_id, last_active, record_json`). 레코드에 `token` 포함 | 이번 조사에서 암호화 코드를 찾지 못함(추정: 평문 JSON) | `fluxer_desktop/src/main/DesktopAppStorage.ts:32,261`, `fluxer_desktop/native/app-store/src/model.rs:85`, `fluxer_desktop/src/main/AccountEngineAccounts.ts:53` |
| Electron safeStorage | 부팅 스냅샷 캐시의 데이터 키 래핑에만 사용 | 토큰 저장과 무관 | `fluxer_desktop/src/main/BootState.ts:150-181` |
| 구 `fluxer_desktop_rs` | APPDATA 아래 `fluxer_desktop_rs/storage/desktop_accounts.json`에 `storageKey`와 `record` 목록을 평문 JSON으로 기록 | 없음 | `fluxer_desktop_rs/src/storage.rs:35-40,53-60,122-125`, `fluxer_desktop_rs/src/config.rs:9-17` |
| 새 Rust 코어(`fluxer_client/core`) | `%APPDATA%\fluxer_client\sessions.json`(`sessions.lock` 배타 잠금, 버전 2) | Windows DPAPI(current-user). 레코드의 account key, user id, API endpoint를 묶은 entropy를 사용한다. 비Windows는 `CryptoError::Unsupported`이며 평문 대체가 없다 | `session/mod.rs`, `session/dpapi.rs` |

- 계정 키 형식: `<instanceKey>::<userId>`(구분자 `::`, 최대 1024 바이트). `fluxer_app/src/features/auth/state/AccountStorageKey.ts:8-9,22-34`
- 저장 레코드 필드: `userId, token, userData(username, discriminator, globalName, email, avatar), presenceIntent, localStorageData, managedStorageData, lastActive, isValid, switcherPosition, instance(스냅샷), storageKey`. `fluxer_app/src/features/auth/state/AccountStorageContract.ts:15-60`
- Electron IPC 채널: `desktop-accounts:get-all`, `get`, `upsert`, `compare-and-swap`, `delete`, `import`, `prune`(`packages/desktop_ipc/src/AccountContract.ts:3-11`). 구 코어 `storage.rs`는 같은 계약을 구현한다(`storage.rs:243-277`).
- 새 Rust 코어는 Windows DPAPI(current-user)로 토큰을 암호화해 `sessions.json`에 저장하고, SQLite 캐시에는 토큰을 넣지 않는다. entropy는 레코드의 account key, user id, API endpoint를 정규 JSON으로 묶어 만들므로, 다른 레코드로 옮기거나 endpoint를 고친 레코드의 토큰은 복호화되지 않는다. 버전 1 파일은 열 때 고정 entropy 토큰을 복호화해 레코드 결합 entropy로 다시 봉인한다. 구 `desktop_accounts.json`과의 형식 호환은 없으며, 평문 파일을 읽어 들이는 경로도 없다.

### 5.1 구 `fluxer_desktop_rs` account_engine / sqlite_store의 범위

- `fluxer_desktop_rs/src/account_engine.rs`(234줄)는 렌더러 IPC 요청(`configure`, `claimForeground`, `getSummaries`, `getActivity`, `beginPromotion`, `completePromotion`, `abortPromotion`, `demote`, `ackChannel`, `reconnect`, `setUnreadBadgeCustomization`)을 처리하는 라우터다. `account_engine.rs:30-180`
- `completePromotion`, `abortPromotion`, `reconnect`, `setUnreadBadgeCustomization`은 Null만 반환하는 스텁이다(`account_engine.rs:121-125,169-174`). `beginPromotion`은 커서가 있어도 `entries`를 빈 객체로 반환한다(`account_engine.rs:110-118`).
- `ackChannel`은 로컬 SQLite의 멘션 행만 삭제하며 서버로 ack를 보내지 않는다(`account_engine.rs:154-168`).
- `sqlite_store.rs`는 `account-engine-cache.sqlite3`(WAL)에 테이블 `meta, rows, cursors, summaries, mentions`를 둔다(`fluxer_desktop_rs/src/sqlite_store.rs:13-20`). 토큰·REST·게이트웨이 로직은 없는 로컬 캐시다.
- 결론: 구 `fluxer_desktop_rs`에는 REST 클라이언트, 로그인, 토큰 갱신 코드가 없다. 구 코어의 REST 관련 코드는 `ipc.rs`의 `desktopRuntimeResolve`(discovery)와 `fetchProxy`(단순 프록시)뿐이고, 게이트웨이는 `gateway.rs`에 WebSocket 전송 계층만 있다(`fluxer_desktop_rs/src/gateway.rs:1-40`). `bridge.rs:440-475`는 웹 UI의 XHR에 `Authorization`과 `X-Fluxer-Features`를 localStorage에서 보강하는 JS 주입이며 새 코어에서는 쓰지 않는다. 새 코어 `fluxer_client/core`에는 discovery, REST 클라이언트, 로그인/MFA/로그아웃, 메시지·읽음·리액션·타이핑 API, 첨부 업로드, DPAPI 세션 저장이 구현되어 있다(아래 구현 맵). 토큰 갱신과 5xx 자동 재시도는 아직 없다.

## 6. Rust 클라이언트 구현 체크리스트 (제안)

이 절은 구현 전 제안이며, 현재 구현 현황은 마지막 “Rust implementation map” 절에 있다.

1. discovery: 1.1 절차 그대로. TLS 검증 켜기, 리디렉션 거부, 15초, 1 MiB, JSON 검사, origin 일치 검증.
2. HTTP 계층: base = `api_client`, 경로 `/v{api_code_version}`, `Authorization` 원문 토큰, 2.2 헤더, 오류 JSON 파서, 429(`retry_after`, `Retry-After`, `X-RateLimit-Reset-After`). 클라이언트 페이서는 기본 25건/초, 최대 40건/초이고 동시 요청은 8개로 제한한다(서버 전역 한도 50건/초). 502/504 계열 백오프는 아직 없다.
3. 로그인: `/auth/login` → `mfa` 분기 → TOTP(`/auth/login/mfa/totp`) → 토큰 저장. 400 `CAPTCHA_REQUIRED`는 ALTCHA 풀이 후 `X-Captcha-Token`으로 재시도. 403 `IP_AUTHORIZATION_REQUIRED`는 별도 상태로 노출.
4. 토큰 검증: `GET /users/@me`(401만 무효로 처리).
5. 첫 화면: 게이트웨이 READY를 우선 사용하고, 필요 시 REST 대체 경로와 메시지 목록·전송·ack를 4절대로 구현.
6. 로그아웃: `POST /auth/logout` 후 저장소에서 토큰 삭제.

## 7. 불확실·미확인 항목

- `X-Fluxer-Client-Installation-ID`의 값 형식(`ClientInstallationId.ts`의 생성 로직 미확인).
- 429 응답의 `Retry-After` 헤더 설정 위치(`RateLimitError` 부모 클래스)는 코드로 확인하지 못했다.
- ALTCHA `solution` 구조와 해시 입력은 `rest/altcha.rs` 구현으로 확인했다. `nonce || u32_be(counter)`를 PBKDF2 password로 쓰고 `{counter, derivedKey, time}`을 보낸다.
- WebAuthn 로그인 `challenge`가 옵션 응답의 어느 필드인지, 데스크톱 `fluxer_desktop/native/webauthn` 연동 방식.
- 핸드오프 `poll_secret` 전달 방식, IP 인증 poll 완료 응답의 형태, 메시지 전송 자동 재시도 한도 상수.
- 토큰 평문 저장 여부(Electron app-store, 웹)는 암호화 호출이 없다는 부정 증거 기반이다(추정).
- 서버 라우트는 소스 정의만 확인했고 실제 서버 실행이나 응답 캡처로 검증하지 않았다. 스키마 필드는 zod 정의 기준이다.
- 데스크톱 로컬 프로토콜 프록시(`LocalAppProxyHTTPPolicy.ts`)가 요청 헤더를 어떻게 변형하는지 세부(허용 목록 전체)는 읽지 않았다.

## Rust implementation map (2026-11-18)

- 기준: `fluxer_client/core/src/rest`의 `mod.rs`, `client.rs`, `discovery.rs`, `auth.rs`, `altcha.rs`, `upload.rs`, `secret.rs`, `error.rs`, `api/*.rs`와 `fluxer_client/core/src/session/mod.rs`, `session/dpapi.rs`를 직접 읽었다. 테스트 모듈은 선언만 확인했고 실행하지 않았다.
- 이 절은 TS 명세와 Rust 구현의 대응을 기록한다. 위 절과 다르면 이 절의 “미구현·차이”가 우선한다.

### 모듈 경로

| 경로 | 역할 |
|---|---|
| `fluxer_client/core/src/rest/mod.rs` | REST·인증 모듈 선언과 공개 타입 재수출 |
| `fluxer_client/core/src/rest/client.rs` | URL·헤더·오류·429·ALTCHA·페이서·동시 요청 제한을 담당하는 HTTP 계층 |
| `fluxer_client/core/src/rest/discovery.rs` | `/.well-known/fluxer` discovery와 endpoint 정규화·검증 |
| `fluxer_client/core/src/rest/auth.rs` | 이메일/비밀번호 로그인, TOTP MFA, 로그아웃 |
| `fluxer_client/core/src/rest/altcha.rs` | ALTCHA v2 PBKDF2 작업증명 풀이 |
| `fluxer_client/core/src/rest/upload.rs` | 첨부 파일 multipart 업로드 |
| `fluxer_client/core/src/rest/secret.rs` | 토큰·비밀번호 래퍼와 JSON redaction |
| `fluxer_client/core/src/rest/error.rs` | `RestError`, `ApiError`, `DiscoveryError` |
| `fluxer_client/core/src/rest/api/mod.rs` | API 도메인 모듈 선언과 snowflake 검증 |
| `fluxer_client/core/src/rest/api/messages.rs` | 메시지 목록·전송·조회·편집·삭제·핀 |
| `fluxer_client/core/src/rest/api/guilds.rs` | 길드 멤버 목록 |
| `fluxer_client/core/src/rest/api/users.rs` | `@me`, 프로필, DM 채널 |
| `fluxer_client/core/src/rest/api/read_states.rs` | 읽음 ack와 구형 경로 대체 |
| `fluxer_client/core/src/rest/api/reactions.rs` | 리액션 추가·제거·사용자 목록 |
| `fluxer_client/core/src/rest/api/typing.rs` | 타이핑 전송과 클라이언트 스로틀 |
| `fluxer_client/core/src/session/mod.rs` | 다중 계정 세션 저장소 |
| `fluxer_client/core/src/session/dpapi.rs` | Windows DPAPI 암복호화 |

### 공개 타입과 함수

| 항목 | 한 줄 목적 |
|---|---|
| `RestClient`, `RestConfig`, `ClientIdentity` | HTTP 클라이언트, base URL·정책·타임아웃·속도 설정, 앱 버전·설치 id |
| `RestClient::new`, `set_token`, `has_token`, `installation_id` | 클라이언트 생성, 세션 토큰 교체·보유 확인, 설치 id 조회 |
| `RestClient::login`, `login_with_captcha`, `login_mfa_totp`, `logout` | 비밀번호 로그인, 호출자가 푼 캡차 토큰으로 재시도, TOTP MFA, 로그아웃 |
| `RestClient::login_mfa_webauthn`, `login_passkey` | WebAuthn MFA와 패스키 로그인 자리이며 현재 `RestError::Unsupported`를 반환한다. |
| `LoginOutcome`, `LoginSuccess`, `MfaMethods` | 토큰·MFA·캡차·IP 인증 결과, 성공 토큰과 사용자, MFA 수단 |
| `Endpoints`, `DiscoveryOptions`, `ReleaseChannel`, `TransportPolicy` | REST·게이트웨이·미디어 endpoint, discovery 설정, 릴리스 채널, https/wss 정책 |
| `discover`, `discover_with`, `discovery_candidates`, `resolve_instance_input` | 기본/옵션 discovery, 후보 URL 생성, 사용자 입력 정규화와 공식 인스턴스 별칭 매핑 |
| `DISCOVERY_PATH`, `DISCOVERY_TIMEOUT`, `DISCOVERY_MAX_BYTES` | `/.well-known/fluxer`, 15초, 1 MiB 상수 |
| `RestError`, `ApiError`, `FieldError`, `DiscoveryError`, `DiscoveryAttempt` | 요청·API·필드·discovery 오류와 실패 후보 기록 |
| `Secret`, `SecretToken`, `redact_json` | 값을 출력하지 않고 drop 시 0으로 지우는 비밀 래퍼, 오류 본문 비밀 키 마스킹 |
| `SERVER_GLOBAL_LIMIT_PER_SECOND` | 서버 전역 한도 50건/초(참고 상수) |
| `DEFAULT_REQUESTS_PER_SECOND`, `MAX_REQUESTS_PER_SECOND`, `MAX_IN_FLIGHT_REQUESTS` | 클라이언트 기본 25건/초, 설정 상한 40건/초, 동시 요청 8개 |
| `MAX_RESPONSE_BYTES` | REST 응답 본문 상한 8 MiB |
| `new_installation_id` | 16바이트 난수를 32자 소문자 hex로 만든 설치 id |
| `solve_altcha_challenge`, `ALTCHA_SOLVE_TIMEOUT`, `ALTCHA_MAX_COUNTER` | ALTCHA challenge 풀이, 60초 제한, counter 상한 100,000 |
| `MessageQuery`, `SendMessageRequest`, `MessageReference`, `EditMessageRequest`, `PinsQuery` | 메시지 목록·전송·답장/전달 참조·편집·핀 요청 본문 |
| `RestClient::get_messages`, `send_message`, `send_message_request` | 메시지 페이지 조회, `{content, nonce}` 전송, 타입화된 전송 |
| `RestClient::get_message`, `edit_message`, `delete_message`, `get_pins`, `pin_message`, `unpin_message` | 단건 조회·편집·삭제와 핀 목록·고정·해제 |
| `UploadFile`, `UploadOptions`, `UploadSource`, `UploadProgress` | 첨부 파일 원본(경로/메모리), 파일 크기·진행 콜백, 업로드 진행 함수 |
| `RestClient::send_message_with_files` | 첨부 1..=10개를 `payload_json`과 `files[n]` multipart로 전송 |
| `MAX_ATTACHMENTS_PER_MESSAGE`, `DEFAULT_MAX_ATTACHMENT_FILE_SIZE` | 메시지당 첨부 10개, 기본 파일 상한 25 MiB |
| `MemberQuery`, `RestClient::list_guild_members` | 길드 멤버 목록 커서 조회 |
| `ProfileQuery`, `RestClient::get_me`, `get_user_profile`, `get_private_channels`, `open_dm` | `@me`, 프로필, DM 목록, DM 열기 |
| `AckRequest`, `RestClient::ack_message`, `ack_messages` | 읽음 ack 1건·1..=100건 전송과 404/405 구형 경로 대체 |
| `ReactionEmoji`, `ReactionUsersQuery`, `RestClient::add_reaction`, `remove_own_reaction`, `get_reaction_users` | 리액션 이모지 경로, 추가·제거·사용자 목록 |
| `encode_uri_component` | JavaScript `encodeURIComponent`와 같은 퍼센트 인코딩 |
| `TypingCooldown`, `TypingThrottle`, `TypingPlan`, `TYPING_SEND_DELAY`, `TYPING_THROTTLE`, `TYPING_IMMEDIATE_SEND_IDLE`, `RestClient::trigger_typing` | 타이핑 요청과 1.5초 지연·8초 스로틀·16초 즉시 전송 규칙 |
| `SessionStore`, `NewAccount`, `AccountSummary`, `InstanceRecord`, `CachedEndpoints`, `DisplayInfo` | 세션 저장소, 저장 입력, 토큰을 뺀 계정 요약, 인스턴스·캐시 endpoint·표시 정보 |
| `SessionStore::open`, `open_with`, `save_account`, `load_token`, `remove_account`, `set_active`, `active_account`, `list_accounts`, `issues` | 저장소 열기, 계정 저장·토큰 복호화·삭제·활성 전환·목록·로드 문제 조회 |
| `SessionError`, `LoadIssue`, `CryptoError`, `instance_key`, `account_key`, `default_dir` | 세션 저장 오류, 로드 문제, DPAPI 오류, instance key·계정 키 생성, 기본 저장 경로 |
| `dpapi::protect`, `dpapi::unprotect` | Windows current-user DPAPI 암복호화(모듈 내부) |

### 구현됨

- discovery: 후보는 origin root, 입력 경로, `/api` 순서이며 첫 성공을 채택한다. 공식 호스트 별칭, `api_code_version >= 1`, `endpoints.api`/`endpoints.gateway` 필수, `api_client` 우선, endpoint origin 일치, JSON Content-Type, 리디렉션 거부, 15초, 1 MiB, codename `tungsten` 검사를 구현했다. ETag/304 캐시는 없다.
- HTTP: `<api>/v<N>` URL, `Authorization` 원문 토큰, `X-Fluxer-Features`, `X-Fluxer-Capabilities`, 설치 id, client properties, JSON Content-Type을 보낸다. 응답은 8 MiB에서 중단하고, 204/빈 본문은 `Value::Null`로 처리한다. 오류 본문은 `code`, `message`, `errors`로 파싱하고 비밀 키를 마스킹한다.
- 429: `X-RateLimit-Reset-After`, `Retry-After`(초 또는 HTTP 날짜), 본문 `retry_after` 순으로 대기 시간을 정하고, `X-RateLimit-Global` 또는 본문 `global`을 반영한다. 버킷별·전역 만료를 기록하며, 기본 2회까지 10초 이하 대기 후 재시도한다.
- 속도 제한: 1초 슬라이딩 윈도우 페이서(기본 25건/초, 최대 40건/초)와 동시 요청 8개 세마포어를 적용한다. 같은 origin 리디렉션만 따르고 TLS 검증은 끄지 않는다.
- ALTCHA: 400 `CAPTCHA_REQUIRED`/`INVALID_CAPTCHA`의 `altcha_challenge`를 v2 PBKDF2로 풀어 `X-Captcha-Token`으로 최대 2회 자동 재시도한다. 알고리즘·cost·keyLength·keyPrefix·만료·토큰 크기를 검증하고, 8개 이하 스레드로 counter를 탐색하며 60초에 중단한다.
- 인증: `POST /auth/login` `{email, password}`, TOTP MFA, 로그아웃(5초 타임아웃, 429 재시도 0)을 구현했다. 로그인 토큰은 자동 저장하지 않고 호출자가 `set_token`으로 설정한다. 로그아웃은 서버 호출이 실패해도 해당 세대의 로컬 토큰을 지운다.
- API: 메시지 목록·전송·단건 조회·편집·삭제·핀, 첨부 multipart 전송, 길드 멤버 목록, `@me`·프로필·DM 목록·DM 열기, 읽음 ack(구형 경로 대체 포함), 리액션, 타이핑을 구현했다.
- 세션 저장: `%APPDATA%\fluxer_client\sessions.json`에 버전 2 JSON으로 저장한다. 임시 파일에 쓰고 fsync한 뒤 rename하며, `sessions.lock` 배타 잠금, 손상 파일 격리, 사용할 수 없는 레코드 보존, 버전 1 마이그레이션을 구현했다. 토큰은 Windows DPAPI(current-user)로 암호화하고 entropy를 account key·user id·API endpoint에 결합한다.

### 미구현·차이

- 토큰 갱신과 세션 연장 로직이 없다. 로그인 토큰을 그대로 저장하고 `set_token`으로 교체한다.
- 네트워크 오류와 502/504/507/522/523/524/598/599 자동 재시도·백오프가 없다. 429만 재시도한다.
- WebAuthn MFA와 패스키 로그인은 `RestError::Unsupported`이다. OS 인증자 연동이 없다.
- IP 인증의 `authorize-ip`, 재전송, poll을 구현하지 않았다. 403 `IP_AUTHORIZATION_REQUIRED`는 `LoginOutcome::IpAuthorizationRequired`로 상태만 노출한다.
- 브라우저 핸드오프, SSO, 이메일 링크 로그인, 다른 세션 목록·종료를 구현하지 않았다.
- `GET /users/@me/guilds`, `GET /guilds/{guild_id}/channels`, `POST /channels/messages/bulk`, 사전 서명 첨부 업로드(`POST /channels/{id}/attachments`)를 구현하지 않았다. 길드·채널·DM 목록은 게이트웨이 READY를 사용한다.
- 로그인은 `{email, password}`만 보낸다. `login` 식별자 방식과 `invite_code`를 보내지 않는다. `Accept-Language`, `X-Audit-Log-Reason`, sudo 모드 JWT도 보내지 않는다.
- discovery의 ETag/304 캐시와 웹 전용 “serving origin 채택” 규칙을 구현하지 않았다.
- 세션 토큰 암호화는 Windows DPAPI 전용이다. 다른 플랫폼은 `CryptoError::Unsupported`이며 평문 저장으로 대체하지 않는다. DPAPI 재봉인 외의 OS Credential Manager/keyring 연동은 없다.
