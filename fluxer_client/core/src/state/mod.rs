//! Client state: entity caches built from gateway dispatches, message windows,
//! unread tracking and permission computation.
//!
//! Layout: `client.rs` holds `ClientState`, the dispatch router and the
//! cross-domain queries; `handlers/<domain>.rs` holds each domain's
//! reducers, model types and queries; `model.rs`/`content.rs` hold the wire
//! entities.
//!
//! Spec: `docs/rust_client_core/STATE_MODEL.md`.

mod capabilities;
mod client;
mod content;
mod embed_model;
mod handlers;
mod ids;
mod messages;
mod model;
mod notify;
mod permissions;
mod visibility;

#[cfg(test)]
mod tests;

pub use client::{ChannelTreeNode, ClientState, StateChange, Unread};
pub use content::{
    ATTACHMENT_FLAG_ANIMATED, ATTACHMENT_FLAG_EXPLICIT, ATTACHMENT_FLAG_SPOILER, Attachment, AttachmentSummary,
    Embed, EmbedAuthor, EmbedField, EmbedFooter, EmbedMedia, EmbedProvider, EmbedSummary, MESSAGE_TYPE_REPLY,
    MessageReference, PartialEmoji, Reaction, Sticker, message_flags,
};
pub use embed_model::{
    EMBED_MAX_HEIGHT, EMBED_MAX_WIDTH, EMBED_MEDIA_OUTER_WIDTH, EMBED_TALL_MAX_HEIGHT, EMBED_TEXT_OUTER_WIDTH,
    EMBED_THUMBNAIL_SIZE, EmbedKind, MAX_GALLERY_MEDIA, MIN_USEFUL_MEDIA_SIZE, ProxyOptions, ValidMedia,
    animated_proxy_url, canonicalize_media_url, embed_media_size, fit_media, gallery_images, inline_thumbnail_width,
    is_duplicate, is_useful_media_size, media_proxy_url, normalize_url, spoiler_urls, static_proxy_url,
    strip_proxy_params, toggled_suppress_embeds, url_hostname, valid_media,
};
pub use handlers::emojis::GuildEmoji;
pub use handlers::member_list::{
    ListPresence, MemberList, MemberListEntry, MemberListGroup, MemberListRow, MemberLists,
};
pub use handlers::messages::ReplyPreview;
pub use handlers::presence::{CustomStatus, Presence, Status};
pub use handlers::relationships::{Relationship, RelationshipKind};
pub use handlers::typing::{TYPING_EXPIRY_MS, Typer};
pub use handlers::user_settings::{
    ChannelOverride, MuteConfig, UserGuildSettings, UserSettings, message_notifications,
};
pub use ids::{FLUXER_EPOCH_MS, Snowflake};
pub use messages::{
    LoadMode, MAX_CACHED_CHANNELS, MAX_LOADED_MESSAGES, MessageStore, MessageWindow, PAGE_SIZE,
    TRUNCATED_MESSAGE_VIEW_SIZE,
};
pub use model::{
    Channel, ChannelKind, Guild, Member, Message, MessageState, PermissionOverwrite, ReadState, Role, User,
    parse_timestamp_ms,
};
pub use notify::{NotificationKind, effective_notification_level, should_notify};
pub use visibility::{user_can_view_channel, user_channel_permissions};
pub use permissions::{
    bits, can_manage_target_member, compute_permissions, has, highest_role, is_role_higher, thread_view_permissions,
};
