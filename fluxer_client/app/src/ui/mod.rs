//! Pieces of the client layout. Each renders from the `ClientView` snapshot;
//! the view owns the state and the actions the listeners call.

pub mod access;
pub mod account_switcher;
pub mod avatar;
pub mod channel_list;
pub mod channel_modal;
pub mod channel_popouts;
pub mod composer;
pub mod create_thread_modal;
pub mod emoji_picker;
pub mod explore;
pub mod forward_modal;
pub mod friends;
pub mod group_dm;
pub mod guild_modal;
pub mod guild_settings_modals;
pub mod guild_rail;
pub mod header;
pub mod invite_modal;
pub mod l10n;
pub mod link_warning;
pub mod media_viewer;
pub mod message_list;
pub mod message_row;
pub mod moderation_modal;
pub mod profile_popout;
pub mod user_panel;
pub(crate) mod video_viewer;
pub mod voice_panel;
pub mod voice_stage;
pub mod reactions;
pub mod reply;
pub mod search_panel;
pub mod message_actions;
pub mod settings;
pub mod titlebar;
pub mod typing;
pub mod member_list;
pub mod attachments;
pub mod autocomplete;
pub mod composer_text;
pub mod overlay;
pub mod quick_switcher;
pub mod rail_tooltip;
pub mod status_indicator;
pub mod embed;

use gpui_kit::{div, prelude::*, px};

use crate::theme::Theme;

/// Red mention count badge.
pub fn badge(count: u32, theme: &Theme) -> impl IntoElement {
    div()
        .px_1()
        .rounded(px(8.))
        .bg(theme.danger)
        .text_xs()
        .text_color(theme.on_fill)
        .child(count.to_string())
}
