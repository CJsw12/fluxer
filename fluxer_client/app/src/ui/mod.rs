//! Pieces of the client layout. Each renders from the `ClientView` snapshot;
//! the view owns the state and the actions the listeners call.

pub mod channel_list;
pub mod composer;
pub mod emoji_picker;
pub mod friends;
pub mod guild_modal;
pub mod guild_rail;
pub mod header;
pub mod l10n;
pub mod message_list;
pub mod message_row;
pub mod profile_popout;
pub mod user_panel;
pub mod reactions;
pub mod reply;
pub mod message_actions;
pub mod settings;
pub mod typing;
pub mod member_list;
pub mod attachments;
pub mod autocomplete;
pub mod composer_text;
pub mod overlay;
pub mod quick_switcher;
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
