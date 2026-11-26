//! Replies: the preview line above a reply (`ReplyPreview.tsx`) and the
//! "Replying to X" bar above the composer (`ChannelReplyBar.tsx`).

use std::ops::Range;

use fluxer_client_core::state::{ClientState, Message, ReplyPreview, Snowflake, User};
use crate::icons::AppIcon;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::{
    AnyElement, App, ElementId, FontWeight, HighlightStyle, SharedString, StyledText, Window, div, prelude::*, px,
};

use crate::format::{default_avatar_index, initials};
use crate::parity_probe::FlxExt as _;
use crate::theme::Theme;
use crate::ui::l10n;
use crate::ui::avatar::{AvatarSpec, render_avatar};
use crate::view_model::avatar_url;

/// Longest reply excerpt, in characters, before "…".
pub const EXCERPT_CHARS: usize = 120;
/// Avatar size in the reply line (`ReplyPreview.module.css`: 1rem).
const REPLY_AVATAR: f32 = 16.;

/// What the line above a reply shows.
#[derive(Debug, Clone, PartialEq)]
pub enum ReplyLine {
    Loaded(ReplyAuthorLine),
    /// "Original message was deleted".
    Deleted,
    /// Target neither loaded nor embedded; the caller may fetch it.
    NotLoaded { channel_id: Snowflake, message_id: Snowflake },
}

/// The replied-to message: author and a one-line excerpt.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplyAuthorLine {
    pub channel_id: Snowflake,
    pub message_id: Snowflake,
    pub author_id: Snowflake,
    pub author: SharedString,
    pub mention_author: bool,
    pub initials: SharedString,
    pub avatar_url: Option<SharedString>,
    pub default_avatar: usize,
    /// Content on one line, cut at [`EXCERPT_CHARS`]; empty when the
    /// message has only attachments/embeds.
    pub excerpt: SharedString,
    pub has_attachments: bool,
}

/// The reply line for `message`, or `None` when it is not a reply.
pub fn reply_line(state: &ClientState, message: &Message, media: Option<&str>) -> Option<ReplyLine> {
    match state.reply_preview(message) {
        ReplyPreview::NotReply => None,
        ReplyPreview::Deleted => Some(ReplyLine::Deleted),
        ReplyPreview::NotLoaded { channel_id, message_id } => Some(ReplyLine::NotLoaded { channel_id, message_id }),
        ReplyPreview::Loaded(target) => {
            let guild_id = target.guild_id.or(message.guild_id).or_else(|| {
                state.channels.get(&target.channel_id).and_then(|c| c.guild_id)
            });
            let author = member_name(state, guild_id, &target.author);
            Some(ReplyLine::Loaded(ReplyAuthorLine {
                channel_id: target.channel_id,
                message_id: target.id,
                author_id: target.author.id,
                initials: initials(&author).into(),
                author: author.into(),
                mention_author: message.mentions.iter().any(|u| u.id == target.author.id),
                avatar_url: media.and_then(|media| avatar_url(media, &target.author)).map(Into::into),
                default_avatar: default_avatar_index(target.author.id),
                excerpt: excerpt(&target.content, EXCERPT_CHARS).into(),
                has_attachments: !target.attachments.is_empty() || !target.embeds.is_empty(),
            }))
        }
    }
}

/// Member nick in the guild, else the user's display name.
pub fn member_name(state: &ClientState, guild_id: Option<Snowflake>, user: &User) -> String {
    guild_id
        .and_then(|g| state.members.get(&g))
        .and_then(|members| members.get(&user.id))
        .and_then(|member| member.nick.clone())
        .filter(|nick| !nick.is_empty())
        .unwrap_or_else(|| user.display_name().to_owned())
}

/// `content` on one line (runs of whitespace become one space), cut to
/// `max_chars` characters with a trailing "…".
pub fn excerpt(content: &str, max_chars: usize) -> String {
    let single_line = content.split_whitespace().collect::<Vec<_>>().join(" ");
    if single_line.chars().count() <= max_chars {
        return single_line;
    }
    let mut cut: String = single_line.chars().take(max_chars).collect();
    cut.truncate(cut.trim_end().len());
    cut.push('…');
    cut
}

/// The small line above a reply: a spine, the target's avatar and name and
/// an excerpt. Clicking it calls `on_jump(channel_id, message_id)` with the
/// target (not for a deleted target).
pub fn reply_preview_line(
    id: impl Into<ElementId>,
    line: &ReplyLine,
    theme: &Theme,
    on_jump: impl Fn(Snowflake, Snowflake, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let muted = theme.muted_foreground;
    let (spine_w, spine_ml) = if theme.sizes.compact {
        (12., 4.)
    } else {
        ((theme.sizes.gutter - theme.sizes.avatar / 2. - 4.).max(0.), theme.sizes.avatar / 2.)
    };
    let spine = div()
        .w(px(spine_w))
        .ml(px(spine_ml))
        .mt(px(8.))
        .mr(px(4.))
        .h(px(8.))
        .flex_none()
        .border_l_2()
        .border_t_2()
        .rounded_tl(px(6.))
        .border_color(theme.border);
    let mut inner = div()
        .flex()
        .flex_row()
        .flex_1()
        .items_center()
        // Official `.repliedMessage`: 14px text in an 18px line (`--message-reply-height`).
        .h(px(18.))
        .text_size(px(14.))
        .line_height(px(18.))
        .text_color(muted)
        .min_w_0()
        .flx("channel.reply-preview.replied-message--3");
    let target = match line {
        ReplyLine::Loaded(target) => {
            let mut username_color = theme.foreground;
            username_color.a *= 0.64;
            inner = inner
                .child(
                    div()
                        .flex_none()
                        .mr(px(4.))
                        .cursor_pointer()
                        .flx("channel.reply-preview.replied-avatar")
                        .child(render_avatar(
                            AvatarSpec {
                                user_id: target.author_id,
                                avatar_url: target.avatar_url.clone(),
                                display_name: target.author.clone(),
                                size: REPLY_AVATAR,
                                status: None,
                                status_border_bg: None,
                            },
                            theme,
                        )),
                )
                .child(
                    div()
                        .flex_none()
                        .mr(px(5.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(username_color)
                        .flx("channel.reply-preview.replied-username")
                        .child(if target.mention_author {
                            format!("@{}", target.author)
                        } else {
                            target.author.to_string()
                        }),
                );
            inner = if target.excerpt.is_empty() {
                inner.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h(px(18.))
                        .overflow_hidden()
                        .truncate()
                        .italic()
                        .flx("channel.reply-preview.replied-text-preview.jump-to-replied-message.button")
                        .child(
                            div().flex().flex_row().min_w_0().child(
                                div()
                                    .flx("channel.reply-preview.replied-text-content--2")
                                    .child(if target.has_attachments {
                                        "Click to see attachment"
                                    } else {
                                        "Click to see message"
                                    }),
                            ),
                        ),
                )
            } else {
                inner.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h(px(18.))
                        .overflow_hidden()
                        .truncate()
                        .flx("channel.reply-preview.replied-text-preview.jump-to-replied-message.button")
                        .child(
                            div().flex().flex_row().min_w_0().child(
                                div()
                                    .flx("channel.reply-preview.replied-text-content--2")
                                    .child(target.excerpt.clone()),
                            ),
                        ),
                )
            };
            Some((target.channel_id, target.message_id))
        }
        ReplyLine::Deleted => {
            inner = inner.child(div().italic().child(l10n::tr("Original message was deleted")));
            None
        }
        ReplyLine::NotLoaded { channel_id, message_id } => {
            inner = inner.child(div().italic().child("Loading message…"));
            Some((*channel_id, *message_id))
        }
    };
    let row = div()
        .id(id)
        .flex()
        .flex_row()
        .items_center()
        .min_w_0()
        .child(spine)
        .child(inner);
    match target {
        Some((channel_id, message_id)) => row
            .cursor_pointer()
            .hover(|s| s.text_color(theme.foreground))
            .on_click(move |_, window, cx| on_jump(channel_id, message_id, window, cx))
            .into_any_element(),
        None => row.into_any_element(),
    }
}

/// "Replying to X" with the catalog's `<0>...</0>` markup around the
/// author, which is highlighted; falls back to finding the name.
fn reply_bar_text(author: &str) -> (String, Range<usize>) {
    let full = l10n::tr_args("Replying to <0>{authorNickname}</0>", &[("authorNickname", author)]);
    let mut text = String::with_capacity(full.len());
    let mut range = 0..0;
    let mut rest = full.as_str();
    while let Some(open) = rest.find('<') {
        text.push_str(&rest[..open]);
        let after = &rest[open..];
        if let Some(inner) = after.strip_prefix("<0>")
            && let Some(close) = inner.find("</0>")
        {
            let start = text.len();
            text.push_str(&inner[..close]);
            range = start..text.len();
            rest = &inner[close + 4..];
        } else {
            text.push('<');
            rest = &after[1..];
        }
    }
    text.push_str(rest);
    if range.is_empty()
        && let Some(start) = text.find(author)
    {
        range = start..start + author.len();
    }
    (text, range)
}

/// The message the composer replies to.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplyingTo {
    pub channel_id: Snowflake,
    pub message_id: Snowflake,
    pub author: SharedString,
    /// `allowed_mentions.replied_user`; on by default (TS
    /// `replyingMessage.mentioning ?? true`).
    pub mention: bool,
    /// Off when replying to yourself: the mention toggle is hidden.
    pub can_mention: bool,
}

impl ReplyingTo {
    /// Reply target for `message` (author name resolved like the rows).
    /// You cannot ping yourself, so own messages get `can_mention: false`.
    pub fn of(state: &ClientState, message: &Message) -> Self {
        let guild_id = message.guild_id.or_else(|| state.channels.get(&message.channel_id).and_then(|c| c.guild_id));
        let own = state.current_user_id() == Some(message.author.id);
        Self {
            channel_id: message.channel_id,
            message_id: message.id,
            author: member_name(state, guild_id, &message.author).into(),
            mention: !own,
            can_mention: !own,
        }
    }
}

/// "Replying to **X**" above the composer, with the "@ ON/OFF" mention
/// toggle and a cancel button. Clicking the text jumps to the target.
pub fn replying_to_bar(
    target: &ReplyingTo,
    theme: &Theme,
    on_jump: impl Fn(Snowflake, Snowflake, &mut Window, &mut App) + 'static,
    on_toggle_mention: impl Fn(&mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    let (channel_id, message_id) = (target.channel_id, target.message_id);
    let mention_label = format!("@ {}", l10n::tr(if target.mention { "On" } else { "Off" }));
    let mention_color = if target.mention { theme.mention_fg } else { theme.muted_foreground };
    let (reply_text, author_range) = reply_bar_text(target.author.as_ref());
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px_3()
        .py_1()
        .rounded_t(px(8.))
        .bg(theme.muted)
        .text_sm()
        .text_color(theme.muted_foreground)
        .child(
            div()
                .id("reply-bar-target")
                .flex_1()
                .min_w_0()
                .truncate()
                .cursor_pointer()
                .on_click(move |_, window, cx| on_jump(channel_id, message_id, window, cx))
                .child({
                    let style = HighlightStyle {
                        color: Some(theme.foreground),
                        font_weight: Some(FontWeight::SEMIBOLD),
                        ..Default::default()
                    };
                    let text = StyledText::new(reply_text);
                    if author_range.is_empty() {
                        text.into_any_element()
                    } else {
                        text.with_highlights([(author_range, style)]).into_any_element()
                    }
                }),
        )
        .when(target.can_mention, |bar| {
            bar.child(
                div()
                    .id("reply-bar-mention")
                    .flex_none()
                    .cursor_pointer()
                    .font_weight(FontWeight::BOLD)
                    .text_xs()
                    .text_color(mention_color)
                    .on_click(move |_, window, cx| on_toggle_mention(window, cx))
                    .child(mention_label),
            )
        })
        .child(
            Button::new("reply-bar-cancel")
                .ghost()
                .compact()
                .icon(AppIcon::XCircle)
                .tooltip(l10n::tr("Cancel reply"))
                .on_click(move |_, window, cx| on_cancel(window, cx)),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excerpt_is_one_line_and_cut() {
        assert_eq!(excerpt("hello\n\n  world ", 120), "hello world");
        assert_eq!(excerpt("abcdef ghij", 7), "abcdef…");
        assert_eq!(excerpt("日本語テキスト", 3), "日本語…");
        assert_eq!(excerpt("", 10), "");
    }
}
