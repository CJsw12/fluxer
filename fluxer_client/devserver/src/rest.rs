//! Discovery document and the `/api/v1` REST routes.

use serde_json::{Value, json};

use crate::Shared;
use crate::fixture::{EMAIL_CAPTCHA, EMAIL_MFA, EMAIL_USER, MFA_CODE, PASSWORD};
use crate::http::{Request, Response};

pub const API_PREFIX: &str = "/api/v1";
pub const GATEWAY_PATH: &str = "/gateway";

pub fn handle(request: &Request, shared: &Shared) -> Response {
    let method = request.method.as_str();
    let path = request.path.as_str();
    if method == "GET" && (path == "/.well-known/fluxer" || path == "/api/.well-known/fluxer") {
        return discovery(request);
    }
    let Some(route) = path.strip_prefix(API_PREFIX) else {
        return Response::error(404, "NOT_FOUND", "404: Not Found");
    };
    let segments: Vec<&str> = route.trim_start_matches('/').split('/').collect();
    match (method, segments.as_slice()) {
        ("POST", ["auth", "login"]) => login(request, shared),
        ("POST", ["auth", "login", "mfa", "totp"]) => login_mfa_totp(request, shared),
        _ => {
            let Some(token) = authorized(request, shared) else {
                return Response::error(401, "UNAUTHORIZED", "401: Unauthorized");
            };
            match (method, segments.as_slice()) {
                ("POST", ["auth", "logout"]) => {
                    lock(shared).tokens.remove(&token);
                    Response::no_content()
                }
                ("GET", ["users", "@me"]) => Response::json(200, lock(shared).me_private()),
                ("GET", ["channels", id, "messages"]) => get_messages(request, shared, id),
                ("POST", ["channels", id, "messages"]) => send_message(request, shared, id),
                ("POST", ["channels", _, "messages", _, "ack"]) => Response::no_content(),
                ("POST", ["read-states", "ack"]) => ack(request, shared),
                ("POST", ["read-states", "ack-bulk"]) => ack_bulk(request, shared),
                _ => Response::error(404, "NOT_FOUND", "404: Not Found"),
            }
        }
    }
}

fn lock(shared: &Shared) -> std::sync::MutexGuard<'_, crate::fixture::State> {
    shared.lock()
}

/// Raw session token in `Authorization` (no `Bearer`), as the client sends it.
fn authorized(request: &Request, shared: &Shared) -> Option<String> {
    let token = request.header("authorization")?.to_owned();
    lock(shared).tokens.contains(&token).then_some(token)
}

/// Endpoints are built from the `Host` the client used, so discovery's
/// same-origin rule holds for both `127.0.0.1` and `localhost`.
fn discovery(request: &Request) -> Response {
    let host = request.header("host").unwrap_or("127.0.0.1");
    let origin = format!("http://{host}");
    let api = format!("{origin}/api");
    Response::json(
        200,
        json!({
            "api_code_version": 1,
            "codename": "tungsten",
            "endpoints": {
                "api": api,
                "api_client": api,
                "api_public": api,
                "gateway": format!("ws://{host}{GATEWAY_PATH}"),
                "media": format!("{origin}/media"),
                "static_cdn": format!("{origin}/static"),
                "marketing": origin,
                "admin": format!("{origin}/admin"),
                "invite": format!("{origin}/invite"),
                "gift": format!("{origin}/gift"),
                "webapp": origin,
            },
            "captcha": { "provider": "altcha" },
            "features": {
                "voice_enabled": false,
                "stripe_enabled": false,
                "premium_enabled": false,
                "self_hosted": true,
                "emails_enabled": false,
                "identifier_first": false,
                "instant_accounts": false,
            },
            "push": { "public_vapid_key": null },
        }),
    )
}

fn invalid_form(errors: Value) -> Response {
    Response::json(400, json!({ "code": "INVALID_FORM_BODY", "message": "Input Validation Error", "errors": errors }))
}

/// `{token, user_id, user}` (`AuthSchemas.ts` login success).
fn token_response(shared: &Shared) -> Response {
    let mut state = lock(shared);
    let token = state.new_token();
    Response::json(
        200,
        json!({ "token": token, "user_id": state.me_id.to_string(), "user": state.me.clone() }),
    )
}

fn login(request: &Request, shared: &Shared) -> Response {
    let body = request.json().unwrap_or(Value::Null);
    let email = body["email"].as_str().unwrap_or_default().trim().to_ascii_lowercase();
    let password = body["password"].as_str().unwrap_or_default();
    let known = [EMAIL_USER, EMAIL_MFA, EMAIL_CAPTCHA].contains(&email.as_str());
    if !known || password != PASSWORD {
        return invalid_form(json!([
            { "path": "email", "code": "INVALID_LOGIN_OR_PASSWORD", "message": "Invalid login or password" },
            { "path": "password", "code": "INVALID_LOGIN_OR_PASSWORD", "message": "Invalid login or password" },
        ]));
    }
    if email == EMAIL_CAPTCHA && request.header("x-captcha-token").is_none_or(str::is_empty) {
        // `CaptchaRequiredError` + `altchaChallengeData`. The token is not
        // verified: any non-empty `X-Captcha-Token` passes.
        return Response::json(
            400,
            json!({
                "code": "CAPTCHA_REQUIRED",
                "message": "Captcha is required",
                "captcha_provider": "altcha",
                "altcha_challenge": {
                    "parameters": {
                        "algorithm": "PBKDF2/SHA-256",
                        "nonce": "00000000000000000000000000000000",
                        "salt": "devserver",
                        "cost": 1000,
                        "keyLength": 32,
                        "keyPrefix": "00",
                        "expiresAt": 4_102_444_800_u64,
                    },
                    "signature": "devserver-unsigned",
                },
            }),
        );
    }
    if email == EMAIL_MFA {
        let ticket = {
            let mut state = lock(shared);
            let ticket = state.secret(48);
            state.mfa_tickets.insert(ticket.clone());
            ticket
        };
        return Response::json(
            200,
            json!({
                "mfa": true,
                "ticket": ticket,
                "allowed_methods": ["totp"],
                "totp": true,
                "webauthn": false,
                "backup_codes": false,
            }),
        );
    }
    token_response(shared)
}

fn login_mfa_totp(request: &Request, shared: &Shared) -> Response {
    let body = request.json().unwrap_or(Value::Null);
    let ticket = body["ticket"].as_str().unwrap_or_default();
    let code = body["code"].as_str().unwrap_or_default();
    if !lock(shared).mfa_tickets.contains(ticket) {
        return invalid_form(json!([{ "path": "ticket", "code": "INVALID_MFA_TICKET", "message": "Invalid or expired MFA ticket" }]));
    }
    if code != MFA_CODE {
        return invalid_form(json!([{ "path": "code", "code": "INVALID_MFA_CODE", "message": "Invalid MFA code" }]));
    }
    lock(shared).mfa_tickets.remove(ticket);
    token_response(shared)
}

fn parse_id(value: &str) -> Option<u64> {
    if value.is_empty() || value.len() > 20 {
        return None;
    }
    value.parse().ok()
}

fn unknown_channel() -> Response {
    Response::error(404, "UNKNOWN_CHANNEL", "Unknown Channel")
}

/// `GET /channels/{id}/messages?limit&before&after&around`, newest first.
fn get_messages(request: &Request, shared: &Shared, channel: &str) -> Response {
    let Some(channel_id) = parse_id(channel) else {
        return unknown_channel();
    };
    let limit = match request.query_param("limit").map(str::parse::<usize>) {
        None => 50,
        Some(Ok(n)) if (1..=100).contains(&n) => n,
        Some(_) => {
            return invalid_form(json!([{ "path": "limit", "code": "NUMBER_TYPE_OUT_OF_RANGE", "message": "Must be between 1 and 100" }]));
        }
    };
    let mut cursor = None;
    for key in ["before", "after", "around"] {
        if let Some(value) = request.query_param(key) {
            let Some(id) = parse_id(value) else {
                return invalid_form(json!([{ "path": key, "code": "SNOWFLAKE_TYPE", "message": "Invalid snowflake" }]));
            };
            cursor.get_or_insert((key, id));
        }
    }
    let state = lock(shared);
    if !state.channels.contains_key(&channel_id) {
        return unknown_channel();
    }
    let empty = Vec::new();
    let all = state.messages.get(&channel_id).unwrap_or(&empty);
    // `all` is ascending; pick an ascending slice, then reverse.
    let slice: &[(u64, Value)] = match cursor {
        None => &all[all.len().saturating_sub(limit)..],
        Some(("before", id)) => {
            let end = all.partition_point(|(m, _)| *m < id);
            &all[end.saturating_sub(limit)..end]
        }
        Some(("after", id)) => {
            let start = all.partition_point(|(m, _)| *m <= id);
            &all[start..(start + limit).min(all.len())]
        }
        Some((_, id)) => {
            // around: the target (if present) plus older and newer halves.
            let pivot = all.partition_point(|(m, _)| *m < id);
            let before = limit / 2;
            let start = pivot.saturating_sub(before);
            &all[start..(start + limit).min(all.len())]
        }
    };
    let list: Vec<Value> = slice.iter().rev().map(|(_, m)| m.clone()).collect();
    Response::json(200, Value::Array(list))
}

/// `POST /channels/{id}/messages` with `{content, nonce}`.
fn send_message(request: &Request, shared: &Shared, channel: &str) -> Response {
    let Some(channel_id) = parse_id(channel) else {
        return unknown_channel();
    };
    let body = request.json().unwrap_or(Value::Null);
    let content = body["content"].as_str().unwrap_or_default().to_owned();
    let nonce = match &body["nonce"] {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    };
    if content.trim().is_empty() {
        return Response::error(400, "CANNOT_SEND_EMPTY_MESSAGE", "Cannot send an empty message");
    }
    if content.chars().count() > 2000 {
        return invalid_form(json!([{ "path": "content", "code": "STRING_LENGTH_INVALID", "message": "Must be between 1 and 2000 in length" }]));
    }
    if nonce.as_ref().is_some_and(|n| n.chars().count() > 32) {
        return invalid_form(json!([{ "path": "nonce", "code": "STRING_LENGTH_INVALID", "message": "Must be between 1 and 32 in length" }]));
    }
    let (message, event) = {
        let mut state = lock(shared);
        if !state.channels.contains_key(&channel_id) {
            return unknown_channel();
        }
        if state.send_denied.contains(&channel_id) {
            return Response::error(403, "MISSING_PERMISSIONS", "Missing Permissions");
        }
        let me = state.me.clone();
        state.post_message(channel_id, &me, &content, Vec::new(), nonce.as_deref())
    };
    shared.dispatch("MESSAGE_CREATE", event);
    Response::json(200, message)
}

/// `POST /read-states/ack` → `{read_states: [...]}` (200, not 204: the real
/// `ReadStateController` returns the authoritative read states).
fn ack(request: &Request, shared: &Shared) -> Response {
    let body = request.json().unwrap_or(Value::Null);
    let Some(entries) = body["read_states"].as_array().filter(|e| !e.is_empty() && e.len() <= 100) else {
        return invalid_form(json!([{ "path": "read_states", "code": "ARRAY_LENGTH_INVALID", "message": "Must contain 1 to 100 entries" }]));
    };
    let mut out = Vec::new();
    let mut events = Vec::new();
    {
        let mut state = lock(shared);
        for entry in entries {
            let (Some(channel_id), Some(message_id)) = (
                entry["channel_id"].as_str().and_then(parse_id),
                entry["message_id"].as_str().and_then(parse_id),
            ) else {
                continue;
            };
            if !state.channels.contains_key(&channel_id) {
                continue;
            }
            let mention_count = entry["mention_count"].as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(0);
            let rs = state.read_states.entry(channel_id).or_default();
            rs.last_message_id = Some(message_id);
            rs.mention_count = mention_count;
            rs.version += 1;
            let version = rs.version;
            events.push(json!({
                "channel_id": channel_id.to_string(),
                "message_id": message_id.to_string(),
                "mention_count": mention_count,
                "version": version.to_string(),
            }));
            out.push(state.read_state_json(channel_id));
        }
    }
    for event in events {
        shared.dispatch("MESSAGE_ACK", event);
    }
    Response::json(200, json!({ "read_states": out }))
}

/// `POST /read-states/ack-bulk` `{read_states: [{channel_id, message_id}]}` → 204.
fn ack_bulk(request: &Request, shared: &Shared) -> Response {
    let body = request.json().unwrap_or(Value::Null);
    let mut state = lock(shared);
    for entry in body["read_states"].as_array().into_iter().flatten() {
        let (Some(channel_id), Some(message_id)) = (
            entry["channel_id"].as_str().and_then(parse_id),
            entry["message_id"].as_str().and_then(parse_id),
        ) else {
            continue;
        };
        let rs = state.read_states.entry(channel_id).or_default();
        rs.last_message_id = Some(message_id);
        rs.mention_count = 0;
        rs.version += 1;
    }
    Response::no_content()
}
