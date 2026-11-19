//! One user message (reply line, avatar, author, time, markdown body,
//! images, reactions, hover actions) and the shared avatar element.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use fluxer_client_core::state::{MessageState, Snowflake};
use fluxer_client_markdown::{MarkdownStyle, MessageMarkdown, SpoilerReveals};
use gpui_kit::{
    AnyElement, App, Entity, FontWeight, Global, Hsla, Image, ImageFormat, ObjectFit, SharedString,
    StyledImage as _, WeakEntity, div, img, prelude::*, px, rgb,
};

use crate::client_view::ClientView;
use crate::format::DEFAULT_AVATAR_COLORS;
use crate::theme::Theme;
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

fn spoiler_state(id: Snowflake, cx: &mut App) -> Entity<SpoilerReveals> {
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

/// Mention pills in the theme's colors; the rest of the crate's dark style.
pub(crate) fn markdown_style(theme: &Theme) -> MarkdownStyle {
    MarkdownStyle {
        mention_background: theme.mention_bg,
        mention_foreground: theme.mention_fg,
        ..MarkdownStyle::default()
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
    let author_id = row.author_id;
    let compact = sizes.compact;
    let mut content = div().flex().flex_col().flex_1().min_w_0();
    if compact || row.group_start {
        let view_author = view.clone();
        let author_btn = div()
            .id(("msg-author", row.id.0))
            .cursor_pointer()
            .text_color(fg)
            .font_weight(FontWeight::SEMIBOLD)
            .on_click(move |ev, window, cx| {
                if !ev.modifiers().shift {
                    let pos = ev.position();
                    let _ = view_author.update(cx, |v, cx| {
                        v.open_profile(author_id, v.current_guild_id(), pos, window, cx);
                    });
                }
            })
            .child(row.author.clone());
        let header = if compact {
            div()
                .flex()
                .flex_row()
                .items_baseline()
                .gap_2()
                .child(div().text_color(muted_fg).text_xs().child(row.time.clone()))
                .child(author_btn)
                .children(state_label.map(|(label, color)| div().text_color(color).text_xs().child(label)))
        } else {
            div()
                .flex()
                .flex_row()
                .items_baseline()
                .gap_2()
                .child(author_btn)
                .child(div().text_color(muted_fg).text_xs().child(row.time.clone()))
                .children(state_label.map(|(label, color)| div().text_color(color).text_xs().child(label)))
        };
        content = content.child(header);
    } else if let Some((label, color)) = state_label {
        content = content.child(div().text_color(color).text_xs().child(label));
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
        content = content.child(body);
    }
    if let (MessageState::Sending, Some(percent)) = (row.state, row.upload_percent) {
        content = content.child(upload_bar(percent, theme));
    }
    let placeholder = theme.image_placeholder;
    if !row.images.is_empty() {
        // The budgeted embed cache (decoded images are dropped when off
        // screen); the id lets GPUI animate GIFs.
        let cache = embed::image_cache(cx);
        let viewer_items: Arc<Vec<MediaViewerItem>> = Arc::new(
            row.images
                .iter()
                .map(|image| media_viewer::from_attachment(image, scale))
                .collect(),
        );
        for (ix, image) in row.images.iter().enumerate() {
            let url = image.url.clone();
            let view_img = view.clone();
            let items_for_click = viewer_items.clone();
            let thumb = img(image.source(scale))
                .image_cache(&cache)
                .id(SharedString::from(format!("attachment-{}-{ix}", row.id.0)))
                .w(px(image.width))
                .h(px(image.height))
                .rounded(px(4.))
                .object_fit(ObjectFit::Contain)
                .with_loading(move || div().size_full().bg(placeholder).into_any_element())
                .with_fallback(move || div().text_xs().text_color(muted_fg).child(url.clone()).into_any_element());
            content = content.child(
                div()
                    .id(SharedString::from(format!("attachment-open-{}-{ix}", row.id.0)))
                    .debug_selector(move || format!("attachment-open-{}-{ix}", row.id.0))
                    .mt_1()
                    .w(px(image.width))
                    .h(px(image.height))
                    .cursor_pointer()
                    .on_click(move |_, window, cx| {
                        let items = (*items_for_click).clone();
                        let _ = view_img.update(cx, |v, cx| v.open_media_viewer(items, ix, window, cx));
                    })
                    .child(thumb),
            );
        }
    }
    if !row.embeds.is_empty() {
        content = content.child(embed::render(row.id, &row.embeds, theme, scale, view, cx));
    }
    let (channel_id, message_id) = (row.actions.channel_id, row.id);
    if !row.reactions.is_empty() {
        let view = view.clone();
        content = content.child(reactions::reaction_bar(row.id.0, &row.reactions, theme, move |emoji, _, cx| {
            let emoji = emoji.clone();
            let _ = view.update(cx, |v, cx| v.toggle_reaction(channel_id, message_id, emoji, cx));
        }));
    }
    let mut main = div()
        .flex()
        .flex_row()
        .w_full()
        .px_4()
        .pb(px(if compact { 1. } else { 2. }))
        .when(row.state == MessageState::Sending, |d| d.opacity(0.6));
    if !compact {
        let gutter = if row.group_start {
            let view_avatar = view.clone();
            div()
                .id(("msg-avatar", row.id.0))
                .cursor_pointer()
                .on_click(move |ev, window, cx| {
                    if !ev.modifiers().shift {
                        let pos = ev.position();
                        let _ = view_avatar.update(cx, |v, cx| {
                            v.open_profile(author_id, v.current_guild_id(), pos, window, cx);
                        });
                    }
                })
                .child(avatar(row.avatar_url.clone(), row.default_avatar, row.initials.clone(), sizes.avatar, theme))
                .into_any_element()
        } else {
            div().into_any_element()
        };
        main = main.child(div().w(px(sizes.gutter)).flex_none().child(gutter));
    }
    let main = main.child(content);
    let reply_line = row.reply.as_ref().map(|line| {
        let view = view.clone();
        let id = SharedString::from(format!("reply-{}", row.id.0));
        div().px_4().child(reply::reply_preview_line(id, line, theme, move |channel_id, message_id, _, cx| {
            let _ = view.update(cx, |v, cx| v.jump_to_message(channel_id, message_id, cx));
        }))
    });
    let column = div()
        .flex()
        .flex_col()
        .w_full()
        .when(row.group_start, |d| d.pt(px(sizes.group_gap)))
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
            let view = view.clone();
            let target = row.actions.clone();
            Some(message_actions::action_bar(&row.actions, theme, move |action, window, cx| {
                let _ = view.update(cx, |v, cx| v.message_action(action, &target, window, cx));
            }))
        }
    };
    let view = view.clone();
    let id = SharedString::from(format!("message-{}", row.id.0));
    // A message still being sent has no actions yet.
    let hover_target = (row.state != MessageState::Sending).then_some(message_id);
    message_actions::with_hover_actions(id, column, bar, move |_, cx| {
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

/// Avatar image, else the default avatar for the user id
/// (`AvatarUtils.getDefaultAvatarURL`), else a colored circle.
pub fn avatar(
    url: Option<SharedString>,
    default_index: usize,
    initials: SharedString,
    size: f32,
    theme: &Theme,
) -> AnyElement {
    let color = default_avatar_color(default_index);
    let text_color = theme.on_fill;
    let circle = move || {
        div()
            .size(px(size))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(color)
            .text_color(text_color)
            .text_sm()
            .font_weight(FontWeight::SEMIBOLD)
            .child(initials.clone())
            .into_any_element()
    };
    let default_image = default_avatar(default_index);
    let fallback = {
        let circle = circle.clone();
        move || match default_image.clone() {
            Some(image) => img(image).size(px(size)).rounded_full().into_any_element(),
            None => circle(),
        }
    };
    match url {
        Some(url) => img(url)
            .size(px(size))
            .rounded_full()
            .with_loading(circle)
            .with_fallback(fallback)
            .into_any_element(),
        None => fallback(),
    }
}

fn default_avatar_color(index: usize) -> Hsla {
    rgb(DEFAULT_AVATAR_COLORS.get(index).copied().unwrap_or(DEFAULT_AVATAR_COLORS[0])).into()
}

/// The web client's default avatars (`fluxer_app/src/media/images/avatars`),
/// decoded once.
fn default_avatar(index: usize) -> Option<Arc<Image>> {
    static IMAGES: OnceLock<Vec<Arc<Image>>> = OnceLock::new();
    const PNGS: [&[u8]; 6] = [
        include_bytes!("../../assets/avatars/0.png"),
        include_bytes!("../../assets/avatars/1.png"),
        include_bytes!("../../assets/avatars/2.png"),
        include_bytes!("../../assets/avatars/3.png"),
        include_bytes!("../../assets/avatars/4.png"),
        include_bytes!("../../assets/avatars/5.png"),
    ];
    IMAGES
        .get_or_init(|| PNGS.iter().map(|png| Arc::new(Image::from_bytes(ImageFormat::Png, png.to_vec()))).collect())
        .get(index)
        .cloned()
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
}
