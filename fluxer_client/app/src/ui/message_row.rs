//! One user message (reply line, avatar, author, time, markdown body,
//! images, reactions, hover actions) and the shared avatar element.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use fluxer_client_core::state::{MessageState, Snowflake};
use fluxer_client_markdown::{MarkdownStyle, MessageMarkdown, SpoilerReveals};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::{
    AnyElement, App, Entity, FontWeight, Global, Hsla, ObjectFit, SharedString,
    StyledImage as _, WeakEntity, div, img, prelude::*, px, rgb,
};

use crate::client_view::ClientView;
use crate::parity_probe::FlxExt as _;
use crate::theme::Theme;
use crate::ui::avatar::{AvatarSpec, render_avatar_tagged};
use crate::ui::media_viewer::{self, MediaViewerItem};
use crate::ui::{embed, l10n, message_actions, reactions, reply};
use crate::view_model::MessageRow;

/// Revealed spoilers per message. Kept app-wide so a reveal survives the
/// row leaving the virtual list; only messages with spoilers get an entry.
#[derive(Default)]
struct SpoilerStates(HashMap<Snowflake, Entity<SpoilerReveals>>);

impl Global for SpoilerStates {}

/// Spoiler states kept; past this all are forgotten (older reveals hide
/// again) so hours of browsing do not keep one entity per message.
const MAX_SPOILER_STATES: usize = 512;

pub(crate) fn spoiler_state(id: Snowflake, cx: &mut App) -> Entity<SpoilerReveals> {
    if let Some(state) = cx.default_global::<SpoilerStates>().0.get(&id) {
        return state.clone();
    }
    let state = cx.new(|_| SpoilerReveals::default());
    let states = &mut cx.default_global::<SpoilerStates>().0;
    if states.len() >= MAX_SPOILER_STATES {
        states.clear();
    }
    states.insert(id, state.clone());
    state
}

/// Revealed image-attachment spoiler/NSFW covers per `(message_id, index)`.
#[derive(Default)]
struct AttachmentReveals(HashSet<(Snowflake, usize)>);

impl Global for AttachmentReveals {}

fn is_attachment_revealed(message_id: Snowflake, index: usize, cx: &App) -> bool {
    cx.try_global::<AttachmentReveals>().is_some_and(|r| r.0.contains(&(message_id, index)))
}

fn reveal_attachment(message_id: Snowflake, index: usize, cx: &mut App) {
    let reveals = &mut cx.default_global::<AttachmentReveals>().0;
    if reveals.len() >= MAX_SPOILER_STATES {
        reveals.clear();
    }
    reveals.insert((message_id, index));
}

fn file_extension_badge(filename: &str) -> String {
    filename
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_uppercase())
        .filter(|ext| !ext.is_empty() && ext.len() <= 5)
        .unwrap_or_else(|| l10n::tr("File"))
}

/// `ExpiryFootnote.module.css .footnote`: inline-flex inside `.mosaicContainerWrapper`'s
/// 24px line box, with 0.65rem tertiary text positioned 8px below the mosaic tiles.
fn expiry_footnote(label: SharedString, theme: &Theme) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .h(px(24.))
        .pt(px(8.))
        .child(
            div()
                .text_size(px(10.4))
                .line_height(px(12.48))
                .text_color(theme.text_tertiary)
                .child(label)
                .flx("app.expiry-footnote.inline-footnote.text"),
        )
        .into_any_element()
}

/// Mention pills and hidden spoilers in the active theme's colors.
pub(crate) fn markdown_style(theme: &Theme) -> MarkdownStyle {
    let default_style = MarkdownStyle::default();
    // `--markup-spoiler-hidden-bg`: currentColor at 16%.
    let spoiler_hidden = theme.text_chat.opacity(0.16);
    MarkdownStyle {
        mention_background: theme.mention_bg,
        mention_foreground: theme.mention_fg,
        spoiler_hidden,
        // `--bg-code-block`: the secondary background darkened 16%.
        // `--code-block-bg`: dark mixes primary 92% with 5% lightness 8%; light mixes
        // 98.5% 88% with 92.84% 12%.
        code_background: Hsla {
            l: if theme.background.l < 0.5 { theme.background.l * 0.92 + 0.05 * 0.08 } else { 0.985 * 0.88 + 0.9284 * 0.12 },
            ..theme.background
        },
        code_border: theme.border,
        quote_text: theme.muted_foreground,
        list_marker: theme.text_chat,
        ..default_style
    }
}

/// The progress bar under a pending message with files.
fn upload_bar(percent: u8, theme: &Theme) -> impl IntoElement {
    let fraction = f32::from(percent.min(100)) / 100.;
    div()
        .mt_1()
        .w(px(240.))
        .h(px(4.))
        .rounded(px(2.))
        .bg(theme.image_placeholder)
        .child(div().h_full().rounded(px(2.)).bg(theme.mention_fg).w(gpui_kit::relative(fraction)))
}

/// A group start has the avatar, author and time; the rest of the group is
/// only the body, aligned with the first one. A reply has its preview line
/// above; embeds and reactions go under the body. Clicks go to `view`.
/// `scale` is the window's scale factor (embed media load at device size).
pub fn render(
    row: &MessageRow,
    theme: &Theme,
    view: &WeakEntity<ClientView>,
    bar: RowBar,
    scale: f32,
    cx: &mut App,
) -> AnyElement {
    let (fg, muted_fg) = (theme.foreground, theme.muted_foreground);
    let sizes = theme.sizes;
    let state_label: Option<(SharedString, Hsla)> = match (row.state, row.upload_percent) {
        (MessageState::Sent, _) => None,
        (MessageState::Sending, Some(percent)) => Some((format!("Uploading… {percent}%").into(), muted_fg)),
        (MessageState::Sending, None) => Some(("sending…".into(), muted_fg)),
        (MessageState::Failed, _) => Some((l10n::tr("Failed to send message.").into(), theme.danger)),
    };
    let (channel_id, message_id) = (row.actions.channel_id, row.id);
    let can_retry = row.actions.can_retry;
    let render_state_label = |label: SharedString, color: Hsla, view: &WeakEntity<ClientView>| -> AnyElement {
        let mut el = div()
            .flex()
            .flex_row()
            .items_baseline()
            .gap_1()
            .text_color(color)
            .text_xs()
            .child(label);
        if can_retry {
            let view_retry = view.clone();
            el = el.child(
                div()
                    .id(SharedString::from(format!("retry-msg-{}", message_id.0)))
                    .debug_selector(move || format!("retry-msg-{}", message_id.0))
                    .cursor_pointer()
                    .font_weight(FontWeight::SEMIBOLD)
                    .hover(|s| s.underline())
                    .on_click(move |_, _, cx| {
                        let _ = view_retry.update(cx, |v, cx| v.retry_failed_message(channel_id, message_id, cx));
                    })
                    .child(l10n::tr("Try again")),
            );
        }
        el.into_any_element()
    };
    let author_id = row.author_id;
    let compact = sizes.compact;
    let mut content = div().flex().flex_col().flex_1().min_w_0();
    // The official wrapper holds the author line and the body.
    content = content.flx("channel.user-message.message-content");
    if compact || row.group_start {
        let view_author = view.clone();
        let author_btn = div()
            .id(("msg-author", row.id.0))
            .cursor_pointer()
            .text_color(fg)
            .text_size(px(16.))
            .line_height(px(22.))
            .font_weight(FontWeight::MEDIUM)
            .on_click(move |ev, window, cx| {
                if !ev.modifiers().shift {
                    let pos = ev.position();
                    let _ = view_author.update(cx, |v, cx| {
                        v.open_profile(author_id, v.current_guild_id(), pos, window, cx);
                    });
                }
            })
            .flx_text("channel.user-message.message-author-name", row.author.to_string())
            .flx("channel.user-message.message-author-part--2")
            .flx("channel.message-username.context-menu-underline.key-down")
            .child(row.author.clone());
        // Official `TimestampWithTooltip`: a 22px container holding a 16px `<time>`.
        let time_el = |time: SharedString| {
            div()
                .flex_none()
                .h(px(22.))
                .flx("channel.timestamp-with-tooltip.container")
                .child(
                    div()
                        .mt(px(3.))
                        .h(px(16.))
                        .text_color(muted_fg)
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .flx("channel.timestamp-with-tooltip.render-time-element.time")
                        .child(time),
                )
        };
        let header = if compact {
            div()
                .flex()
                .flex_row()
                .items_baseline()
                .gap_2()
                .child(div().text_color(muted_fg).text_size(px(12.)).line_height(px(22.)).child(row.time.clone()))
                .child(author_btn)
                .children(state_label.map(|(label, color)| render_state_label(label, color, view)))
        } else {
            div()
                .flex()
                .flex_row()
                .items_baseline()
                .gap_2()
                .h(px(23.))
                .flx("channel.user-message.message-author-info")
                .flx("channel.user-message.message-author-info--2")
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_baseline()
                        .gap_x(px(2.4)).gap_y(px(0.))
                        .h(px(23.))
                        .flx("channel.user-message.message-author-row--2")
                        .child(author_btn)
                        .child(time_el(row.time.clone())),
                )
                .children(state_label.map(|(label, color)| render_state_label(label, color, view)))
        };
        content = content.child(header);
    } else if let Some((label, color)) = state_label {
        content = content.child(render_state_label(label, color, view));
    }
    if let Some(document) = &row.document {
        let view_link = view.clone();
        let mut body = MessageMarkdown::new(("msg", row.id.0), document.clone())
            .style(markdown_style(theme))
            .on_link_click(move |url, window, cx| {
                let _ = view_link.update(cx, |v, cx| v.open_external_url(url, window, cx));
            });
        if document.spoiler_count > 0 {
            body = body.spoiler_state(spoiler_state(row.id, cx));
        }
        content = content.child(
            div()
                .w_full()
                .max_w(px(760.))
                .text_size(px(16.))
                .line_height(px(22.))
                // `.messageText` bottom padding.
                .pb(px(1.))
                .text_color(theme.text_chat)
                .flx("channel.user-message.message-text")
                .flx("channel.user-message.message-text--2")
                // Official `.markup` renders at 24px line height inside `.messageText` (22px).
                .child(div().line_height(px(24.)).child(body)),
        );
    }
    if let (MessageState::Sending, Some(percent)) = (row.state, row.upload_percent) {
        content = content.child(upload_bar(percent, theme));
    }
    let mut accessories = div().flex().flex_col().py(px(2.));
    let mut has_accessories = false;
    let placeholder = theme.image_placeholder;
    if !row.images.is_empty() {
        has_accessories = true;
        // The budgeted embed cache (decoded images are dropped when off
        // screen); the id lets GPUI animate GIFs.
        let cache = embed::image_cache(cx);
        let viewer_items: Arc<Vec<MediaViewerItem>> = Arc::new(
            row.images
                .iter()
                .map(|image| media_viewer::from_attachment(image, scale))
                .collect(),
        );
        // Official `AttachmentMosaic`: a block at most 550px wide; one media keeps its fitted
        // size, two media share square 273px tiles (`getMosaicTileBox`, 4px gap).
        let image_count = row.images.len();
        let mut images_grid = div().flex().flex_row().when(image_count != 2, |d| d.flex_wrap()).gap(px(if image_count == 2 { 4. } else { 8. }));
        if image_count == 2 {
            images_grid = images_grid.flx("channel.embeds.attachments.attachment-layout-grid.div");
        }
        for (ix, image) in row.images.iter().enumerate() {
            let cover = row.image_covers.get(ix).copied().unwrap_or_default();
            let (tile_w, tile_h) = if image_count == 2 { (273., 273.) } else { (image.width, image.height) };
            if (cover.spoiler || cover.nsfw) && !is_attachment_revealed(row.id, ix, cx) {
                let label = if cover.nsfw {
                    format!("NSFW · {}", l10n::tr("Click to reveal"))
                } else {
                    l10n::tr("Spoiler")
                };
                let msg_id = row.id;
                let view_reveal = view.clone();
                images_grid = images_grid.child(
                    div()
                        .id(SharedString::from(format!("attachment-cover-{}-{ix}", row.id.0)))
                        .debug_selector(move || format!("attachment-cover-{}-{ix}", row.id.0))
                        .w(px(tile_w))
                        .h(px(tile_h))
                        .flex_none()
                        .rounded(px(4.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(theme.background_tertiary)
                        .text_color(theme.muted_foreground)
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .cursor_pointer()
                        .on_click(move |_, _, cx| {
                            reveal_attachment(msg_id, ix, cx);
                            let _ = view_reveal.update(cx, |v, cx| v.redraw_messages(cx));
                        })
                        .child(label),
                );
                continue;
            }
            let url = image.url.clone();
            let view_img = view.clone();
            let items_for_click = viewer_items.clone();
            let thumb = img(image.source(scale))
                .image_cache(&cache)
                .id(SharedString::from(format!("attachment-{}-{ix}", row.id.0)))
                .size_full()
                .object_fit(ObjectFit::Cover)
                .with_loading(move || div().size_full().bg(placeholder).into_any_element())
                .with_fallback(move || div().text_xs().text_color(muted_fg).child(url.clone()).into_any_element());
            let mut badge_bg: Hsla = rgb(0x000000).into();
            badge_bg.a = 0.65;
            images_grid = images_grid.child(
                div()
                    .id(SharedString::from(format!("attachment-open-{}-{ix}", row.id.0)))
                    .debug_selector(move || format!("attachment-open-{}-{ix}", row.id.0))
                    .relative()
                    .w(px(tile_w))
                    .h(px(tile_h))
                    .flex_none()
                    .rounded(px(4.))
                    .overflow_hidden()
                    .bg(placeholder)
                    .when(image_count == 2, |d| {
                        d.flx("channel.embeds.attachments.attachment-grid-item.media-container")
                    })
                    .when(image_count == 1, |d| d.flx("channel.embeds.media.embed-image.image-element"))
                    .cursor_pointer()
                    .on_click(move |_, window, cx| {
                        let items = (*items_for_click).clone();
                        let _ = view_img.update(cx, |v, cx| v.open_media_viewer(items, ix, window, cx));
                    })
                    .child(thumb)
                    .when(image.animated, |d| {
                        d.child(
                            div()
                                .debug_selector(move || format!("attachment-gif-badge-{}-{ix}", row.id.0))
                                .when(image_count == 2, |d| {
                                    d.flx("channel.embeds.attachments.attachment-grid-item.gif-indicator")
                                })
                                .absolute()
                                .top_2()
                                .left_2()
                                .px(px(8.))
                                .py_0p5()
                                .rounded(px(4.))
                                .bg(badge_bg)
                                .text_color(rgb(0xffffff))
                                .text_xs()
                                .font_weight(FontWeight::BOLD)
                                .child("GIF"),
                        )
                    }),
            );
        }
        let container = div()
            .w_full()
            .max_w(px(550.))
            .overflow_hidden()
            .flx("channel.embeds.attachments.attachment-mosaic.attachment-mosaic-component.mosaic-container")
            .child(images_grid);
        // `.mosaicContainerWrapper` sits inside `.container` (`padding-block: 2px`).
        accessories = accessories.child(
            div()
                .relative()
                .w_full()
                .max_w(px(550.))
                .overflow_hidden()
                .flx("channel.embeds.attachments.attachment-mosaic.attachment-mosaic-component.mosaic-container-wrapper")
                .child(container)
                .children(row.image_expiry.clone().map(|label| expiry_footnote(label, theme))),
        );
    }
    if !row.file_attachments.is_empty() {
        has_accessories = true;
        let mut list = div().flex().flex_col().gap_1().mt_1();
        for (ix, file) in row.file_attachments.iter().enumerate() {
            let ext = file_extension_badge(&file.filename);
            let a11y = format!("{} ({})", file.filename, file.size_label);
            let card_id = SharedString::from(format!("file-attachment-{}-{ix}", row.id.0));
            let debug_id = card_id.clone();
            let url = file.url.clone();
            let view_file = view.clone();
            let mut card = Button::new(card_id)
                .ghost()
                .accessibility_label(a11y)
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .w(px(320.))
                        .px_3()
                        .py_2()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background_secondary)
                        .child(
                            div()
                                .px_1p5()
                                .py_0p5()
                                .rounded(px(4.))
                                .bg(theme.background_tertiary)
                                .text_xs()
                                .font_weight(FontWeight::BOLD)
                                .text_color(muted_fg)
                                .child(ext),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .truncate()
                                        .text_sm()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(if url.is_empty() { fg } else { theme.text_link })
                                        .child(file.filename.clone()),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .gap_2()
                                        .text_xs()
                                        .text_color(muted_fg)
                                        .child(file.size_label.clone())
                                        .when(file.spoiler, |d| d.child(l10n::tr("Spoiler"))),
                                ),
                        ),
                );
            if !url.is_empty() {
                card = card.on_click(move |_, window, cx| {
                    let _ = view_file.update(cx, |v, cx| v.open_external_url(&url, window, cx));
                });
            }
            list = list.child(
                div()
                    .debug_selector(move || debug_id.to_string())
                    .child(card)
                    .children(file.expiry.clone().map(|label| expiry_footnote(label, theme))),
            );
        }
        accessories = accessories.child(list);
    }
    if !row.embeds.is_empty() {
        has_accessories = true;
        accessories = accessories.child(embed::render(row.id, &row.embeds, theme, scale, view, cx));
    }
    let (channel_id, message_id) = (row.actions.channel_id, row.id);
    if !row.reactions.is_empty() {
        has_accessories = true;
        let view_toggle = view.clone();
        let view_add = view.clone();
        accessories = accessories.child(reactions::reaction_bar(
            row.id.0,
            &row.reactions,
            theme,
            move |emoji, _, cx| {
                let emoji = emoji.clone();
                let _ = view_toggle.update(cx, |v, cx| v.toggle_reaction(channel_id, message_id, emoji, cx));
            },
            move |pos, window, cx| {
                let _ = view_add.update(cx, |v, cx| v.open_reaction_emoji_picker(channel_id, message_id, pos, window, cx));
            },
        ));
    }
    if has_accessories {
        content = content.child(accessories);
    }
    let is_hovered = !matches!(bar, RowBar::None);
    let mut main = div()
        .flex()
        .flex_row()
        .w_full()
        // Official `padding-inline-end` leaves 64px to the chat area edge.
        .pl_4()
        .pr(px(64.))
        .py(px(sizes.message_padding_y))
        .when(row.state == MessageState::Sending, |d| d.opacity(0.6));
    if !compact {
        let gutter = if row.group_start {
            let view_avatar = view.clone();
            div()
                .id(("msg-avatar", row.id.0))
                // Official `.messageAvatar` margin: spacing + avatar align offset (6px vs content 2px).
                .mt(px(4.))
                .cursor_pointer()
                .on_click(move |ev, window, cx| {
                    if !ev.modifiers().shift {
                        let pos = ev.position();
                        let _ = view_avatar.update(cx, |v, cx| {
                            v.open_profile(author_id, v.current_guild_id(), pos, window, cx);
                        });
                    }
                })
                .child(render_avatar_tagged(
                    AvatarSpec {
                        user_id: row.author_id,
                        avatar_url: row.avatar_url.clone(),
                        display_name: row.author.clone(),
                        size: sizes.avatar,
                        status: None,
                        status_border_bg: None,
                    },
                    theme,
                    &["channel.message-avatar.avatar", "ui.base-avatar.image"],
                ))
                .into_any_element()
        } else {
            div()
                .ml(px(8.))
                .w(px(40.))
                .h(px(22.))
                .flx("channel.timestamp-with-tooltip.container")
                .child(
                    div()
                        // Right-aligned inside the 40px container: 40 - 50.78 = -10.78 (x = 406.22).
                        .ml(px(-10.78))
                        .w(px(50.78))
                        .h(px(22.))
                        .flex_none()
                        .flx("channel.timestamp-with-tooltip.render-time-element.time")
                        .when(is_hovered && !row.short_time.is_empty(), |d| {
                            d.debug_selector(move || format!("msg-hover-time-{}", message_id.0))
                                .flex()
                                .items_center()
                                .justify_end()
                                .text_size(px(11.))
                                .line_height(px(22.))
                                .text_color(muted_fg)
                                .whitespace_nowrap()
                                .child(row.short_time.clone())
                        }),
                )
                .into_any_element()
        };
        main = main.child(div().w(px(sizes.gutter)).flex_none().child(gutter));
    }
    // Probe-only overlays: the official message wrapper and group exclude the 16px chat gutters.
    let main = main
        .child(content)
        .child(div().absolute().top_0().bottom_0().left(px(16.)).right(px(16.)).flx("channel.message-group.rendered-messages.div"));
    let reply_line = row.reply.as_ref().map(|line| {
        let view = view.clone();
        let id = SharedString::from(format!("reply-{}", row.id.0));
        div().px_4().child(reply::reply_preview_line(id, line, theme, move |channel_id, message_id, _, cx| {
            let _ = view.update(cx, |v, cx| v.jump_to_message(channel_id, message_id, cx));
        }))
    });
    let mentioned_sel = format!("msg-mentioned-{}", row.id.0);
    let mut mention_bg: Hsla = rgb(0xfaa61a).into();
    mention_bg.a = 0.10;
    let mention_border: Hsla = rgb(0xfaa61a).into();
    let rclick_view = view.clone();
    let rclick_target = row.actions.clone();
    let article = div()
        .flex()
        .flex_col()
        .w_full()
        .flx("channel.message.article.alt-click")
        .when(row.mentioned, |d| {
            d.debug_selector(move || mentioned_sel.clone())
                .bg(mention_bg)
                .border_l_2()
                .border_color(mention_border)
        })
        .when(is_hovered && !row.mentioned, |d| d.bg(theme.hover_bg))
        .when(!row.mentioned, |d| d.hover(|s| s.bg(theme.hover_bg)))
        .when(row.actions.sent, |d| {
            d.on_mouse_down(gpui_kit::MouseButton::Right, move |ev, window, cx| {
                cx.stop_propagation();
                let pos = ev.position;
                let target = rclick_target.clone();
                let _ = rclick_view.update(cx, |v, cx| v.open_message_menu_at(target, pos, window, cx));
            })
        })
        .children(reply_line)
        .child(main);
    let bar = match bar {
        RowBar::None => None,
        RowBar::QuickReactions => {
            let view_pick = view.clone();
            let view_more = view.clone();
            Some(message_actions::quick_reactions(
                row.id.0,
                theme,
                move |emoji, _, cx| {
                    let _ = view_pick.update(cx, |v, cx| v.pick_reaction(channel_id, message_id, emoji, cx));
                },
                move |pos, window, cx| {
                    let _ = view_more.update(cx, |v, cx| {
                        v.open_reaction_emoji_picker(channel_id, message_id, pos, window, cx);
                    });
                },
            ))
        }
        RowBar::Actions => {
            let view_react = view.clone();
            let view = view.clone();
            let target = row.actions.clone();
            Some(message_actions::action_bar(
                &row.actions,
                theme,
                move |emoji, _, cx| {
                    let _ = view_react.update(cx, |v, cx| v.pick_reaction(channel_id, message_id, emoji, cx));
                },
                move |action, window, cx| {
                    let _ = view.update(cx, |v, cx| v.message_action(action, &target, window, cx));
                },
            ))
        }
    };
    let view = view.clone();
    let id = SharedString::from(format!("message-{}", row.id.0));
    // A message still being sent has no actions yet.
    let hover_target = (row.state != MessageState::Sending).then_some(message_id);
    message_actions::with_hover_actions(id, article, bar, move |_, cx| {
        let _ = view.update(cx, |v, cx| v.hover_message(hover_target, cx));
    })
}

/// What floats at the top right of a message row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowBar {
    None,
    /// The hovered row's action bar.
    Actions,
    /// The quick reactions opened by the action bar's React.
    QuickReactions,
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, FixedOffset};
    use fluxer_client_core::Dispatch;
    use fluxer_client_core::state::{ClientState, LoadMode, Message};
    use gpui_kit::base::TextSelection;
    use gpui_kit::{
        Context, Modifiers, MouseButton, Pixels, Point, Render, TestAppContext, VisualTestContext,
        Window, point,
    };
    use serde_json::json;

    use super::*;
    use crate::view_model::message_pane_at;

    const CONTENT: &str = "hey <@200> in <#11>\n||secret|| end\n```rs\nfn main() {}\n```";

    /// One spoiler state per message with spoilers, forgotten past the cap.
    #[gpui_kit::test]
    fn spoiler_states_are_capped(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let first = spoiler_state(Snowflake(1), cx);
            assert_eq!(spoiler_state(Snowflake(1), cx), first, "reused while kept");
            for id in 2..=MAX_SPOILER_STATES as u64 {
                spoiler_state(Snowflake(id), cx);
            }
            assert_eq!(cx.global::<SpoilerStates>().0.len(), MAX_SPOILER_STATES);
            spoiler_state(Snowflake(10_000), cx);
            assert_eq!(cx.global::<SpoilerStates>().0.len(), 1);
        });
    }

    /// The fixture message as a real row (the view model parses its body).
    fn fixture_row() -> MessageRow {
        let mut state = ClientState::new();
        state.apply(&Dispatch {
            t: "READY".to_owned(),
            s: 1,
            d: json!({
                "session_id": "s",
                "user": { "id": "100", "username": "me" },
                "guilds": [{
                    "id": "1",
                    "properties": { "name": "G", "owner_id": "100" },
                    "roles": [{ "id": "1", "name": "@everyone", "permissions": "0" }],
                    "channels": [{ "id": "11", "type": 0, "name": "general", "position": 0 }],
                    "members": [{ "user": { "id": "200", "username": "alice" }, "nick": "Ali", "roles": [] }]
                }],
                "private_channels": [],
                "read_states": []
            }),
        });
        let message: Message = serde_json::from_value(json!({
            "id": "1001", "channel_id": "11", "content": CONTENT, "timestamp": "2026-11-18T10:00:00+00:00",
            "author": { "id": "200", "username": "alice" }
        }))
        .expect("message");
        state.messages.load(Snowflake(11), vec![message], LoadMode::Replace);
        let utc = FixedOffset::east_opt(0).expect("offset");
        let now = DateTime::parse_from_rfc3339("2026-11-18T12:00:00+00:00").expect("now").with_timezone(&utc);
        let mut rows = message_pane_at(&state, Snowflake(11), None, None, &now).rows;
        assert_eq!(rows.len(), 1);
        rows.remove(0)
    }

    struct Host {
        row: MessageRow,
    }

    impl Render for Host {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let theme = Theme::current(cx);
            let view = WeakEntity::new_invalid();
            div().w(px(700.)).flex().flex_col().child(render(&self.row, &theme, &view, RowBar::None, 1., cx))
        }
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
    }

    /// Topmost and bottommost points (at `x`) with selectable text.
    fn selectable_rows(cx: &mut VisualTestContext, x: Pixels) -> (Pixels, Pixels) {
        let (mut first, mut last) = (None, None);
        let mut y = px(1.);
        while y < px(400.) {
            let p = point(x, y);
            cx.simulate_mouse_move(p, None, Modifiers::default());
            draw(cx);
            if cx.update(|window, cx| TextSelection::is_selectable_at(p, window, cx)) {
                first.get_or_insert(y);
                last = Some(y);
            }
            y += px(2.);
        }
        (first.expect("selectable text"), last.expect("selectable text"))
    }

    fn drag(cx: &mut VisualTestContext, from: Point<Pixels>, to: Point<Pixels>) -> String {
        cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
        draw(cx);
        for i in 1..=8 {
            let t = i as f32 / 8.;
            let p = point(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t);
            cx.simulate_mouse_move(p, Some(MouseButton::Left), Modifiers::default());
            draw(cx);
        }
        cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::default());
        draw(cx);
        cx.update(|window, cx| TextSelection::selected_text(window, cx))
    }

    /// Mention, spoiler and code block through the real row: the text is
    /// selectable as plain text, and a revealed spoiler stays revealed in
    /// the app-wide state the next row element for the message reads.
    #[gpui_kit::test]
    fn row_renders_mentions_spoilers_and_code(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let row = fixture_row();
        let document = row.document.clone().expect("parsed body");
        assert_eq!(document.spoiler_count, 1);
        let (_, cx) = cx.add_window_view(|window, cx| {
            let host = cx.new(|_| Host { row });
            gpui_kit::base::Root::new(host, window, cx)
        });
        cx.run_until_parked();
        draw(cx);

        // Row padding 16 + avatar gutter.
        let gutter = cx.update(|_, cx| Theme::current(cx).sizes.gutter);
        let x = px(16. + gutter + 3.);
        let (top, _) = selectable_rows(cx, x);
        // The code block's text is indented by its padding.
        let (_, bottom) = selectable_rows(cx, x + px(30.));
        let selected = drag(cx, point(x - px(2.), top), point(px(690.), bottom));
        let mut rest = selected.as_str();
        for piece in ["hey @Ali in general", "secret end", "fn main() {}"] {
            let at = rest.find(piece).unwrap_or_else(|| panic!("{piece:?} not in order in {selected:?}"));
            rest = &rest[at + piece.len()..];
        }
        for markup in ["<@200>", "||", "```", "fluxer-md"] {
            assert!(!selected.contains(markup), "{markup:?} leaked into {selected:?}");
        }

        let state = cx.update(|_, cx| spoiler_state(Snowflake(1001), cx));
        assert!(!state.read_with(cx, |s, _| s.is_revealed(0)));
        state.update(cx, |s, cx| {
            s.reveal(0);
            cx.notify();
        });
        let again = cx.update(|_, cx| spoiler_state(Snowflake(1001), cx));
        assert_eq!(again, state, "one state entity per message");
        assert!(again.read_with(cx, |s, _| s.is_revealed(0)));
    }

    #[gpui_kit::test]
    fn image_attachment_covers_and_file_cards_render_and_reveal(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let mut row = fixture_row();
        row.id = Snowflake(4242);
        row.images = vec![
            crate::view_model::ImagePreview {
                url: "https://cdn/s.png".into(),
                proxy_url: None,
                animated: false,
                width: 240.,
                height: 160.,
            },
            crate::view_model::ImagePreview {
                url: "https://cdn/n.png".into(),
                proxy_url: None,
                animated: false,
                width: 240.,
                height: 160.,
            },
        ];
        row.image_covers = vec![
            crate::view_model::ImageCover { spoiler: true, nsfw: false },
            crate::view_model::ImageCover { spoiler: false, nsfw: true },
        ];
        row.file_attachments = vec![crate::view_model::FileAttachmentRow {
            id: "99".to_owned(),
            filename: "archive.zip".to_owned(),
            size_label: "12.4 KB".to_owned(),
            url: "https://cdn/archive.zip".to_owned(),
            spoiler: false,
            expiry: None,
        }];
        let (_, cx) = cx.add_window_view(|window, cx| {
            let host = cx.new(|_| Host { row });
            gpui_kit::base::Root::new(host, window, cx)
        });
        draw(cx);
        let spoiler_cover = cx.debug_bounds("attachment-cover-4242-0").expect("spoiler cover");
        let nsfw_cover = cx.debug_bounds("attachment-cover-4242-1").expect("nsfw cover");
        assert!(cx.debug_bounds("attachment-open-4242-0").is_none());
        assert!(cx.debug_bounds("attachment-open-4242-1").is_none());
        assert!(cx.debug_bounds("file-attachment-4242-0").is_some());

        cx.simulate_click(spoiler_cover.center(), Modifiers::default());
        cx.simulate_click(nsfw_cover.center(), Modifiers::default());
        cx.update(|window, _| window.refresh());
        draw(cx);
        assert!(cx.debug_bounds("attachment-cover-4242-0").is_none());
        assert!(cx.debug_bounds("attachment-cover-4242-1").is_none());
        assert!(cx.debug_bounds("attachment-open-4242-0").is_some());
        assert!(cx.debug_bounds("attachment-open-4242-1").is_some());
    }

    #[gpui_kit::test]
    fn multiple_images_lay_out_horizontally_and_animated_shows_gif_badge(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let mut row = fixture_row();
        row.id = Snowflake(4243);
        row.images = vec![
            crate::view_model::ImagePreview {
                url: "https://cdn/a.gif".into(),
                proxy_url: None,
                animated: true,
                width: 120.,
                height: 100.,
            },
            crate::view_model::ImagePreview {
                url: "https://cdn/b.png".into(),
                proxy_url: None,
                animated: false,
                width: 120.,
                height: 100.,
            },
        ];
        row.image_covers = vec![Default::default(), Default::default()];
        let (_, cx) = cx.add_window_view(|window, cx| {
            let host = cx.new(|_| Host { row });
            gpui_kit::base::Root::new(host, window, cx)
        });
        draw(cx);
        let first = cx.debug_bounds("attachment-open-4243-0").expect("first image");
        let second = cx.debug_bounds("attachment-open-4243-1").expect("second image");
        assert!(second.origin.x > first.origin.x, "images should lay out horizontally");
        assert_eq!(second.origin.y, first.origin.y, "images in first row share y");
        assert!(cx.debug_bounds("attachment-gif-badge-4243-0").is_some(), "animated image shows GIF badge");
        assert!(cx.debug_bounds("attachment-gif-badge-4243-1").is_none(), "static image has no GIF badge");
    }
}
