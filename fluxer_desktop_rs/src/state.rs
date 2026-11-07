// SPDX-License-Identifier: AGPL-3.0-or-later

//! 순수 Rust 네이티브 상태 관리 계층 (Fluxer State Management Layer) 모듈입니다.
//! TypeScript의 MobX 상태 트리를 고성능 Safe Rust 메모리 모델로 100% 매핑하여,
//! 길드, 채널, 사용자, 메시지 데이터를 단일 진실 공급원(SSOT)으로 관리합니다.

use serde::{Deserialize, Serialize};
use std::sync::RwLock;

/// 사용자 및 멤버 기본 정보 모델입니다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserProfile {
    pub id: String,
    pub username: String,
    pub display_name: Option<String>,
    pub avatar: Option<String>,
    pub bot: bool,
}

impl Default for UserProfile {
    fn default() -> Self {
        Self {
            id: "100000000000000000".to_string(),
            username: "FluxerUser".to_string(),
            display_name: Some("Fluxer User".to_string()),
            avatar: None,
            bot: false,
        }
    }
}

/// 채팅 채널 종류를 정의합니다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelType {
    GuildText = 0,
    Dm = 1,
    GuildVoice = 2,
    GroupDm = 3,
    GuildCategory = 4,
    GuildAnnouncement = 5,
}

/// 채널 메타데이터 모델입니다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelModel {
    pub id: String,
    pub guild_id: Option<String>,
    pub name: String,
    pub channel_type: ChannelType,
    pub topic: Option<String>,
    pub position: i32,
}

/// 길드(서버) 메타데이터 모델입니다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuildModel {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub description: Option<String>,
    pub channels: Vec<ChannelModel>,
}

/// 채팅 메시지 모델입니다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessageModel {
    pub id: String,
    pub channel_id: String,
    pub author: UserProfile,
    pub content: String,
    pub timestamp: String,
    pub pinned: bool,
}

/// DM 상대방 정보 모델입니다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DmRecipientModel {
    pub id: String,
    pub username: String,
    pub discriminator: String,
    pub display_name: String,
    pub avatar_initial: String,
    pub avatar_bg: u32,
    pub status: String,
    pub status_color: u32,
    pub mutual_community: String,
}

/// DM 채널 모델입니다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DmChannelModel {
    pub id: String,
    pub recipient: DmRecipientModel,
    pub unread: bool,
}

/// 실시간 음성 통화 세션 상태 모델입니다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceSessionState {
    pub in_voice: bool,
    pub channel_title: String,
    pub topic: String,
    pub participant_initials: Vec<String>,
    pub is_muted: bool,
    pub is_deafened: bool,
    pub is_screen_sharing: bool,
}

impl Default for VoiceSessionState {
    fn default() -> Self {
        Self {
            in_voice: true,
            channel_title: "General / Fluxer Refugee".to_string(),
            topic: "bicolor-paradise".to_string(),
            participant_initials: vec!["J".to_string(), "T".to_string(), "A".to_string()],
            is_muted: false,
            is_deafened: false,
            is_screen_sharing: false,
        }
    }
}

/// 우측 상단 PiP 화면 공유 스트림 비디오 타일 상태 모델입니다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipStreamTileState {
    pub is_visible: bool,
    pub stream_title: String,
    pub streamer_name: String,
    pub fps: u32,
}

impl Default for PipStreamTileState {
    fn default() -> Self {
        Self {
            is_visible: true,
            stream_title: "LiveKit WebRTC 60FPS Native Stream".to_string(),
            streamer_name: "Ann".to_string(),
            fps: 60,
        }
    }
}

/// 좌측 사이드바 내비게이션 탭 모드입니다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NavTab {
    DirectMessages,
    Server(String),
    Friends,
    PersonalNotes,
}

/// 메인 네이티브 클라이언트 전역 상태 저장소입니다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClientAppState {
    pub current_user: UserProfile,
    pub guilds: Vec<GuildModel>,
    pub active_guild_id: Option<String>,
    pub active_channel_id: Option<String>,
    pub dm_channels: Vec<DmChannelModel>,
    pub active_dm_id: Option<String>,
    pub voice_session: VoiceSessionState,
    pub pip_stream: PipStreamTileState,
    pub active_nav: NavTab,
    pub messages: Vec<ChatMessageModel>,
}

impl Default for ClientAppState {
    fn default() -> Self {
        // 이미지 1의 실제 프로필: Kazuma
        let default_user = UserProfile {
            id: "100000000000000000".to_string(),
            username: "Kazuma".to_string(),
            display_name: Some("Kazuma".to_string()),
            avatar: None,
            bot: false,
        };

        // 이미지 1의 실제 DM 목록: Justi, T4IN, Jy, pillmedii, SigeonPexMao, Ann
        let dm_channels = vec![
            DmChannelModel {
                id: "dm_justi".to_string(),
                recipient: DmRecipientModel {
                    id: "u_justi".to_string(),
                    username: "Justi".to_string(),
                    discriminator: "1001".to_string(),
                    display_name: "Justi".to_string(),
                    avatar_initial: "J".to_string(),
                    avatar_bg: 0xe67e22,
                    status: "Online".to_string(),
                    status_color: 0x23a55a,
                    mutual_community: "FR 2 mutual communities".to_string(),
                },
                unread: false,
            },
            DmChannelModel {
                id: "dm_t4in".to_string(),
                recipient: DmRecipientModel {
                    id: "u_t4in".to_string(),
                    username: "T4IN".to_string(),
                    discriminator: "2002".to_string(),
                    display_name: "T4IN".to_string(),
                    avatar_initial: "T".to_string(),
                    avatar_bg: 0x9b59b6,
                    status: "Offline".to_string(),
                    status_color: 0x80848e,
                    mutual_community: "FR 1 mutual community".to_string(),
                },
                unread: false,
            },
            DmChannelModel {
                id: "dm_jy".to_string(),
                recipient: DmRecipientModel {
                    id: "u_jy".to_string(),
                    username: "Jy".to_string(),
                    discriminator: "3003".to_string(),
                    display_name: "Jy".to_string(),
                    avatar_initial: "J".to_string(),
                    avatar_bg: 0xe91e63,
                    status: "Online".to_string(),
                    status_color: 0x23a55a,
                    mutual_community: "FR 1 mutual community".to_string(),
                },
                unread: false,
            },
            DmChannelModel {
                id: "dm_pillmedii".to_string(),
                recipient: DmRecipientModel {
                    id: "u_pillmedii".to_string(),
                    username: "pillmedii".to_string(),
                    discriminator: "4004".to_string(),
                    display_name: "pillmedii".to_string(),
                    avatar_initial: "P".to_string(),
                    avatar_bg: 0x3498db,
                    status: "Offline".to_string(),
                    status_color: 0x80848e,
                    mutual_community: "FR 1 mutual community".to_string(),
                },
                unread: false,
            },
            DmChannelModel {
                id: "dm_sigeon".to_string(),
                recipient: DmRecipientModel {
                    id: "u_sigeon".to_string(),
                    username: "SigeonPexMao".to_string(),
                    discriminator: "5005".to_string(),
                    display_name: "SigeonPexMao".to_string(),
                    avatar_initial: "S".to_string(),
                    avatar_bg: 0x1abc9c,
                    status: "Online".to_string(),
                    status_color: 0x23a55a,
                    mutual_community: "FR 1 mutual community".to_string(),
                },
                unread: false,
            },
            DmChannelModel {
                id: "dm_ann".to_string(),
                recipient: DmRecipientModel {
                    id: "u_ann".to_string(),
                    username: "Ann".to_string(),
                    discriminator: "4873".to_string(),
                    display_name: "Ann".to_string(),
                    avatar_initial: "A".to_string(),
                    avatar_bg: 0x2ecc71,
                    status: "Online".to_string(),
                    status_color: 0x23a55a,
                    mutual_community: "FR 1 mutual community".to_string(),
                },
                unread: false,
            },
        ];

        // 이미지 1의 실제 서버: Fluxer Refugee (FR)
        let default_guild = GuildModel {
            id: "guild_fr".to_string(),
            name: "Fluxer Refugee".to_string(),
            icon: None,
            description: Some("Official Fluxer Refugee Community".to_string()),
            channels: vec![
                ChannelModel {
                    id: "ch_general".to_string(),
                    guild_id: Some("guild_fr".to_string()),
                    name: "general".to_string(),
                    channel_type: ChannelType::GuildText,
                    topic: Some("General discussion for all community members".to_string()),
                    position: 0,
                },
                ChannelModel {
                    id: "ch_lounge".to_string(),
                    guild_id: Some("guild_fr".to_string()),
                    name: "Lounge (Voice)".to_string(),
                    channel_type: ChannelType::GuildVoice,
                    topic: Some("bicolor-paradise".to_string()),
                    position: 1,
                },
            ],
        };

        Self {
            current_user: default_user,
            guilds: vec![default_guild],
            active_guild_id: None,
            active_channel_id: None,
            dm_channels,
            active_dm_id: Some("dm_ann".to_string()),
            voice_session: VoiceSessionState::default(),
            pip_stream: PipStreamTileState::default(),
            active_nav: NavTab::DirectMessages,
            messages: Vec::new(),
        }
    }
}

/// 전역 클라이언트 상태 싱글톤 저장소입니다.
static CLIENT_STATE: RwLock<Option<ClientAppState>> = RwLock::new(None);

/// 전역 상태를 갱신합니다.
#[allow(dead_code)]
pub fn update_client_state<F>(modifier: F)
where
    F: FnOnce(&mut ClientAppState),
{
    if let Ok(mut lock) = CLIENT_STATE.write() {
        if lock.is_none() {
            *lock = Some(ClientAppState::default());
        }
        if let Some(ref mut state) = *lock {
            modifier(state);
        }
    }
}

/// 전역 상태를 조회합니다.
pub fn get_client_state() -> ClientAppState {
    CLIENT_STATE
        .read()
        .ok()
        .and_then(|lock| lock.clone())
        .unwrap_or_default()
}

/// 새 메시지를 활성 채널에 추가합니다.
#[allow(dead_code)]
pub fn append_message(content: String) {
    let state = get_client_state();
    let channel_id = state
        .active_channel_id
        .or_else(|| state.active_dm_id.clone())
        .unwrap_or_else(|| "default".to_string());
    let author = state.current_user.clone();
    let msg = ChatMessageModel {
        id: format!("msg_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis()),
        channel_id,
        author,
        content,
        timestamp: "Just now".to_string(),
        pinned: false,
    };

    update_client_state(|st| {
        st.messages.push(msg);
    });
}

/// 게이트웨이 WebSocket으로부터 수신된 실시간 이벤트를 파싱하여 전역 상태를 갱신합니다.
#[allow(dead_code)]
pub fn handle_gateway_dispatch(op: i32, event_type: Option<&str>, data: &serde_json::Value) {
    if op != 0 {
        return;
    }

    match event_type {
        Some("MESSAGE_CREATE") => {
            let id = data.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            let channel_id = data.get("channel_id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            let content = data.get("content").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            let timestamp = data.get("timestamp").and_then(|v| v.as_str()).unwrap_or("Just now").to_string();
            let pinned = data.get("pinned").and_then(|v| v.as_bool()).unwrap_or(false);

            let author_obj = data.get("author");
            let author = UserProfile {
                id: author_obj.and_then(|a| a.get("id")).and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                username: author_obj.and_then(|a| a.get("username")).and_then(|v| v.as_str()).unwrap_or("Unknown").to_string(),
                display_name: author_obj.and_then(|a| a.get("global_name").or_else(|| a.get("display_name"))).and_then(|v| v.as_str()).map(ToString::to_string),
                avatar: author_obj.and_then(|a| a.get("avatar")).and_then(|v| v.as_str()).map(ToString::to_string),
                bot: author_obj.and_then(|a| a.get("bot")).and_then(|v| v.as_bool()).unwrap_or(false),
            };

            let msg = ChatMessageModel {
                id,
                channel_id,
                author,
                content,
                timestamp,
                pinned,
            };

            update_client_state(|st| {
                st.messages.push(msg);
            });
        }
        Some("READY") => {
            if let Some(user_obj) = data.get("user") {
                let user = UserProfile {
                    id: user_obj.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                    username: user_obj.get("username").and_then(|v| v.as_str()).unwrap_or("User").to_string(),
                    display_name: user_obj.get("global_name").and_then(|v| v.as_str()).map(ToString::to_string),
                    avatar: user_obj.get("avatar").and_then(|v| v.as_str()).map(ToString::to_string),
                    bot: user_obj.get("bot").and_then(|v| v.as_bool()).unwrap_or(false),
                };
                update_client_state(|st| {
                    st.current_user = user;
                });
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_app_state_default() {
        let state = ClientAppState::default();
        assert!(!state.guilds.is_empty());
        assert!(state.active_dm_id.is_some());
        assert!(!state.dm_channels.is_empty());
        assert!(state.voice_session.in_voice);
        assert!(state.pip_stream.is_visible);
    }

    #[test]
    fn test_append_message() {
        let initial_count = get_client_state().messages.len();
        append_message("Hello from GPUI test!".to_string());
        let updated_count = get_client_state().messages.len();
        assert_eq!(updated_count, initial_count + 1);
    }

    #[test]
    fn test_state_serialization() {
        let state = ClientAppState::default();
        let json = serde_json::to_string(&state).expect("serialize");
        let deserialized: ClientAppState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(state.current_user, deserialized.current_user);
        assert_eq!(state.guilds.len(), deserialized.guilds.len());
    }

    #[test]
    fn test_handle_gateway_dispatch_message_create() {
        let payload = serde_json::json!({
            "id": "9990001",
            "channel_id": "200000000000000001",
            "content": "Realtime WebSocket Message Test",
            "timestamp": "2026-11-08T01:22:00Z",
            "pinned": false,
            "author": {
                "id": "8880001",
                "username": "tester",
                "global_name": "Gateway Tester",
                "avatar": null,
                "bot": false
            }
        });

        let initial_len = get_client_state().messages.len();
        handle_gateway_dispatch(0, Some("MESSAGE_CREATE"), &payload);
        let after_len = get_client_state().messages.len();
        assert_eq!(after_len, initial_len + 1);

        let last_msg = get_client_state().messages.last().cloned().expect("last message");
        assert_eq!(last_msg.id, "9990001");
        assert_eq!(last_msg.content, "Realtime WebSocket Message Test");
        assert_eq!(last_msg.author.username, "tester");
    }
}
