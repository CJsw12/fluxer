//! Gateway dispatch routing for voice (`VOICE_SERVER_UPDATE` / `VOICE_STATE_UPDATE`).

use super::*;

pub(super) fn parse_optional_snowflake(value: Option<&Value>) -> Result<Option<Snowflake>, &'static str> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(raw)) => Snowflake::parse(raw)
            .map(Some)
            .ok_or("invalid snowflake"),
        Some(_) => Err("snowflake field must be a string or null"),
    }
}

const MAX_VOICE_TOKEN_BYTES: usize = 65_536;
const MAX_CONNECTION_ID_BYTES: usize = 256;

pub(super) fn parse_voice_server_update(payload: &Value) -> Result<ParsedVoiceServerUpdate, &'static str> {
    let guild_id = parse_optional_snowflake(payload.get("guild_id"))?;
    let channel_id = parse_optional_snowflake(payload.get("channel_id"))?;
    // Any non-null e2ee_key, `p2p: true`, or non-null ice_servers (including empty array `[]`,
    // which is truthy in the web client) must be rejected before requiring SFU credentials.
    if payload.get("e2ee_key").is_some_and(|v| !v.is_null()) {
        return Ok(ParsedVoiceServerUpdate::UnsupportedE2ee { guild_id, channel_id });
    }
    let is_p2p = payload.get("p2p").and_then(Value::as_bool) == Some(true);
    let has_ice_servers = payload.get("ice_servers").is_some_and(|v| !v.is_null());
    if is_p2p || has_ice_servers {
        return Ok(ParsedVoiceServerUpdate::UnsupportedP2pMesh { guild_id, channel_id });
    }

    let token = payload
        .get("token")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty() && s.len() <= MAX_VOICE_TOKEN_BYTES)
        .ok_or("missing or oversized token")?;
    let endpoint = payload
        .get("endpoint")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or("missing endpoint")?;
    let mut parsed_url = Url::parse(endpoint).map_err(|_| "invalid endpoint url")?;
    let is_loopback = parsed_url.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    match parsed_url.scheme() {
        "wss" => {}
        "https" => {
            parsed_url.set_scheme("wss").map_err(|_| "invalid endpoint url")?;
        }
        "ws" if is_loopback => {}
        "http" if is_loopback => {
            parsed_url.set_scheme("ws").map_err(|_| "invalid endpoint url")?;
        }
        _ => return Err("voice endpoint requires wss/https (or loopback ws/http)"),
    }
    if parsed_url.host().is_none()
        || !parsed_url.username().is_empty()
        || parsed_url.password().is_some()
        || parsed_url.query().is_some()
        || parsed_url.fragment().is_some()
    {
        return Err("voice endpoint contains unsupported URL fields");
    }
    let normalized_endpoint = parsed_url.to_string();
    let connection_id = payload
        .get("connection_id")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty() && s.len() <= MAX_CONNECTION_ID_BYTES)
        .ok_or("missing or oversized connection_id")?;

    Ok(ParsedVoiceServerUpdate::Sfu {
        guild_id,
        channel_id,
        endpoint: normalized_endpoint,
        token: SecretToken::new(token),
        connection_id: connection_id.to_owned(),
    })
}

pub(super) fn parse_voice_state_update(payload: &Value) -> Option<ParsedVoiceStateUpdate> {
    let user_id = payload.get("user_id").and_then(Value::as_str).and_then(Snowflake::parse)?;
    let guild_id = parse_optional_snowflake(payload.get("guild_id")).ok()?;
    let channel_id = parse_optional_snowflake(payload.get("channel_id")).ok()?;
    let session_id = payload.get("session_id").and_then(Value::as_str).unwrap_or_default().to_owned();
    let connection_id = payload
        .get("connection_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let flag = |name: &str| payload.get(name).and_then(Value::as_bool).unwrap_or(false);
    Some(ParsedVoiceStateUpdate {
        user_id,
        guild_id,
        channel_id,
        session_id,
        connection_id,
        self_mute: flag("self_mute"),
        self_deaf: flag("self_deaf"),
        self_video: flag("self_video"),
        self_stream: flag("self_stream"),
        mute: flag("mute"),
        deaf: flag("deaf"),
        suppress: flag("suppress"),
    })
}

pub(super) fn recheck_active_voice_permissions(
    state: &Mutex<ClientState>,
    voice: &Arc<Mutex<VoiceManager>>,
    gateway: Option<&GatewayHandle>,
    events: &mpsc::UnboundedSender<UiEvent>,
    #[cfg(test)] sent_voice_states: Option<&Arc<Mutex<Vec<VoiceStateUpdate>>>>,
) {
    let record_sent = |payload: &VoiceStateUpdate| {
        #[cfg(test)]
        if let Some(log) = sent_voice_states {
            lock(log).push(payload.clone());
        }
        let _ = payload;
    };
    let active = {
        let mgr = lock(voice);
        match (mgr.guild_id, mgr.channel_id) {
            (Some(g), Some(c)) => Some((mgr.attempt_id, g, c)),
            _ => None,
        }
    };
    let Some((expected_attempt_id, guild_id, channel_id)) = active else {
        return;
    };
    let perms = {
        let st = lock(state);
        (st.guilds.contains_key(&guild_id) && st.channels.contains_key(&channel_id))
            .then(|| st.channel_permissions(channel_id))
    };
    let Some(perms) = perms.filter(|&p| {
        (p & (bits::VIEW_CHANNEL | bits::CONNECT)) == (bits::VIEW_CHANNEL | bits::CONNECT)
    }) else {
        let (payload, _handles) = lock(voice).begin_leave_if_attempt(expected_attempt_id);
        if let Some(payload) = payload {
            record_sent(&payload);
            let _ = send_voice_gateway_update(gateway, &payload);
            let _ = events.send(UiEvent::VoiceChanged);
        }
        return;
    };

    let stream_allowed = (perms & bits::STREAM) != 0;
    let speak_denied = (perms & bits::SPEAK) == 0;
    let mut changed = false;
    let mut stream_stop_payload = None;
    {
        let mut mgr = lock(voice);
        if mgr.attempt_id == expected_attempt_id
            && mgr.guild_id == Some(guild_id)
            && mgr.channel_id == Some(channel_id)
        {
            if !stream_allowed && (mgr.self_stream || mgr.screen_share_audio) {
                let _handles = mgr.stop_screen_share_media();
                stream_stop_payload = mgr.current_state_update_payload();
                changed = true;
            }
            if mgr.speak_denied != speak_denied {
                mgr.speak_denied = speak_denied;
                mgr.apply_audio_mute_flags();
                changed = true;
            }
        }
    }
    if let Some(payload) = stream_stop_payload {
        record_sent(&payload);
        let _ = send_voice_gateway_update(gateway, &payload);
    }
    if changed {
        let _ = events.send(UiEvent::VoiceChanged);
    }
}

pub(crate) fn on_gateway_dispatch(
    dispatch: &fluxer_client_core::Dispatch,
    state: &Mutex<ClientState>,
    voice: &Arc<Mutex<VoiceManager>>,
    gateway: Option<&GatewayHandle>,
    runtime: &Handle,
    events: &mpsc::UnboundedSender<UiEvent>,
) {
    on_gateway_dispatch_inner(
        dispatch,
        state,
        voice,
        gateway,
        runtime,
        events,
        #[cfg(test)]
        None,
    );
}

pub(super) fn on_gateway_dispatch_inner(
    dispatch: &fluxer_client_core::Dispatch,
    state: &Mutex<ClientState>,
    voice: &Arc<Mutex<VoiceManager>>,
    gateway: Option<&GatewayHandle>,
    runtime: &Handle,
    events: &mpsc::UnboundedSender<UiEvent>,
    #[cfg(test)] sent_voice_states: Option<&Arc<Mutex<Vec<VoiceStateUpdate>>>>,
) {
    match dispatch.t.as_str() {
        "READY" => {
            if let Some(guilds) = dispatch.d.get("guilds").and_then(Value::as_array) {
                let mut mgr = lock(voice);
                for guild in guilds {
                    let Some(guild_id) = guild.get("id").and_then(Value::as_str).and_then(Snowflake::parse) else {
                        continue;
                    };
                    if let Some(voice_states) = guild.get("voice_states").and_then(Value::as_array) {
                        mgr.replace_guild_voice_states(guild_id, voice_states);
                    }
                }
            }
        }
        "GUILD_CREATE" | "GUILD_SYNC" => {
            if let Some(guild_id) = dispatch.d.get("id").and_then(Value::as_str).and_then(Snowflake::parse)
                && let Some(voice_states) = dispatch.d.get("voice_states").and_then(Value::as_array)
            {
                lock(voice).replace_guild_voice_states(guild_id, voice_states);
                let _ = events.send(UiEvent::VoiceChanged);
            }
            recheck_active_voice_permissions(
                state,
                voice,
                gateway,
                events,
                #[cfg(test)]
                sent_voice_states,
            );
        }
        "GUILD_DELETE" => {
            if let Some(guild_id) = dispatch.d.get("id").and_then(Value::as_str).and_then(Snowflake::parse) {
                let mut mgr = lock(voice);
                mgr.channel_members.retain(|(g, _), _| *g != Some(guild_id));
                if mgr.guild_id == Some(guild_id) && mgr.channel_id.is_some() {
                    let (payload, _handles) = mgr.begin_leave();
                    drop(mgr);
                    if let Some(payload) = payload {
                        let _ = send_voice_gateway_update(gateway, &payload);
                    }
                    let _ = events.send(UiEvent::VoiceChanged);
                }
            }
        }
        "CHANNEL_DELETE" => {
            if let Some(channel_id) = dispatch.d.get("id").and_then(Value::as_str).and_then(Snowflake::parse) {
                let mut mgr = lock(voice);
                mgr.channel_members.retain(|_, m| m.channel_id != channel_id);
                if mgr.channel_id == Some(channel_id) {
                    let (payload, _handles) = mgr.begin_leave();
                    drop(mgr);
                    if let Some(payload) = payload {
                        let _ = send_voice_gateway_update(gateway, &payload);
                    }
                    let _ = events.send(UiEvent::VoiceChanged);
                }
            }
        }
        "GUILD_UPDATE"
        | "GUILD_ROLE_CREATE"
        | "GUILD_ROLE_UPDATE"
        | "GUILD_ROLE_UPDATE_BULK"
        | "GUILD_ROLE_DELETE"
        | "GUILD_MEMBER_ADD"
        | "GUILD_MEMBER_UPDATE"
        | "GUILD_MEMBER_REMOVE"
        | "CHANNEL_CREATE"
        | "CHANNEL_UPDATE"
        | "CHANNEL_UPDATE_BULK" => {
            recheck_active_voice_permissions(
                state,
                voice,
                gateway,
                events,
                #[cfg(test)]
                sent_voice_states,
            );
        }
        "VOICE_STATE_UPDATE" => {
            let current_user_id = lock(state).current_user.as_ref().map(|u| u.id);
            let changed = lock(voice).handle_voice_state_dispatch(&dispatch.d, current_user_id);
            if changed {
                let _ = events.send(UiEvent::VoiceChanged);
            }
        }
        "VOICE_SERVER_UPDATE" => {
            let decision = {
                let mut mgr = lock(voice);
                match mgr.accept_voice_server_dispatch(&dispatch.d) {
                    Ok(Some(creds)) => Some((mgr.transport.clone(), creds)),
                    Ok(None) => None,
                    Err(_) => {
                        let _ = events.send(UiEvent::VoiceChanged);
                        None
                    }
                }
            };
            if let Some((transport, creds)) = decision {
                let attempt_id = creds.attempt_id;
                let guild_id = creds.guild_id;
                let channel_id = creds.channel_id;
                let connection_id = creds.connection_id.clone();
                let voice_clone = voice.clone();
                let events_clone = events.clone();
                let conn_id_check = creds.connection_id.clone();
                let task = runtime.spawn(async move {
                    let result = transport.connect(creds).await;
                    let watcher = {
                        let mut mgr = lock(&voice_clone);
                        let changed = mgr.finish_connect(
                            attempt_id,
                            guild_id,
                            channel_id,
                            &connection_id,
                            result,
                        );
                        if !changed {
                            None
                        } else if mgr.state == VoiceConnectionState::Connected
                            && let Some(event_rx) = mgr
                                .media
                                .as_ref()
                                .and_then(|media| media.room.take_event_rx())
                        {
                            let mut rx = event_rx;
                            let voice_for_events = voice_clone.clone();
                            let events_for_watcher = events_clone.clone();
                            let conn_id_for_watcher = connection_id.clone();
                            let handle = tokio::spawn(async move {
                                while let Some(ev) = rx.recv().await {
                                    match ev {
                                        VoiceRoomEvent::TracksChanged => {
                                            let mut mgr = lock(&voice_for_events);
                                            if mgr.attempt_id != attempt_id {
                                                break;
                                            }
                                            if mgr.handle_room_event(VoiceRoomEvent::TracksChanged) {
                                                let _ = events_for_watcher.send(UiEvent::VoiceChanged);
                                            }
                                        }
                                        VoiceRoomEvent::VideoFrameReady => {
                                            let mut mgr = lock(&voice_for_events);
                                            if mgr.attempt_id != attempt_id {
                                                break;
                                            }
                                            if mgr.handle_room_event(VoiceRoomEvent::VideoFrameReady) {
                                                let _ = events_for_watcher.send(UiEvent::VoiceChanged);
                                            }
                                        }
                                        ev @ (VoiceRoomEvent::ScreenSources(_)
                                        | VoiceRoomEvent::ActiveSpeakers(_)) => {
                                            let mut mgr = lock(&voice_for_events);
                                            if mgr.attempt_id != attempt_id {
                                                break;
                                            }
                                            if mgr.handle_room_event(ev) {
                                                let _ = events_for_watcher.send(UiEvent::VoiceChanged);
                                            }
                                        }
                                        #[cfg(test)]
                                        ev @ VoiceRoomEvent::VideoFrame { .. } => {
                                            let mut mgr = lock(&voice_for_events);
                                            if mgr.attempt_id != attempt_id {
                                                break;
                                            }
                                            if mgr.handle_room_event(ev) {
                                                let _ = events_for_watcher.send(UiEvent::VoiceChanged);
                                            }
                                        }
                                        VoiceRoomEvent::Disconnected(reason) => {
                                            let changed = lock(&voice_for_events)
                                                .handle_room_disconnected(
                                                    attempt_id,
                                                    &conn_id_for_watcher,
                                                    reason,
                                                );
                                            if changed {
                                                let _ = events_for_watcher.send(UiEvent::VoiceChanged);
                                            }
                                            break;
                                        }
                                    }
                                }
                            });
                            if let Some(media) = &mut mgr.media {
                                media.set_event_task(handle);
                            } else {
                                handle.abort();
                            }
                            Some(())
                        } else {
                            Some(())
                        }
                    };
                    if watcher.is_some() {
                        let _ = events_clone.send(UiEvent::VoiceChanged);
                    }
                });
                let mut mgr = lock(voice);
                if mgr.attempt_id == attempt_id && mgr.connection_id.as_deref() == Some(conn_id_check.as_str()) {
                    if let Some(prev) = mgr.connect_task.replace(task) {
                        prev.abort();
                    }
                } else {
                    task.abort();
                }
                let _ = events.send(UiEvent::VoiceChanged);
            }
        }
        _ => {}
    }
}

pub(crate) fn on_gateway_ready(
    session_id: &str,
    voice: &Mutex<VoiceManager>,
    gateway: Option<&GatewayHandle>,
    events: &mpsc::UnboundedSender<UiEvent>,
) {
    let (changed, recovery_payload, _handles) = lock(voice).handle_gateway_ready(session_id.to_owned());
    if let Some(payload) = recovery_payload {
        let _ = send_voice_gateway_update(gateway, &payload);
    }
    if changed {
        let _ = events.send(UiEvent::VoiceChanged);
    }
}

pub(crate) fn on_gateway_reset(
    voice: &Mutex<VoiceManager>,
    events: &mpsc::UnboundedSender<UiEvent>,
) {
    let (changed, _handles) = lock(voice).handle_gateway_reset();
    if changed {
        let _ = events.send(UiEvent::VoiceChanged);
    }
}

pub(super) fn send_voice_gateway_update(gateway: Option<&GatewayHandle>, payload: &VoiceStateUpdate) -> bool {
    let Some(gateway) = gateway else {
        return false;
    };
    if let Err(err) = gateway.update_voice_state(payload) {
        tracing::warn!(%err, "failed to send gateway VOICE_STATE_UPDATE");
        return false;
    }
    true
}
