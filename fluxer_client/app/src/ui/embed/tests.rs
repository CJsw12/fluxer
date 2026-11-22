//! Embed classification against the web client's rules, the proxy URLs
//! drawn, and a headless render of a card and an image.

use chrono::{DateTime, FixedOffset};
use fluxer_client_core::Dispatch;
use fluxer_client_core::state::{ClientState, LoadMode, Message, Snowflake, message_flags};
use gpui_kit::{
    Context, IntoElement, Modifiers, Render, TestAppContext, VisualTestContext, WeakEntity, Window, div,
    px,
};
use serde_json::{Value, json};

use super::media::{still_url, variant_url};
use super::*;
use crate::ui::message_row::{self, RowBar};
use crate::view_model::{MessageRow, message_pane_at};

const CHANNEL: u64 = 11;

fn state() -> ClientState {
    let mut state = ClientState::new();
    state.apply(&Dispatch {
        t: "READY".to_owned(),
        s: 1,
        d: json!({
            "session_id": "s",
            "user": { "id": "100", "username": "me" },
            "guilds": [{
                "id": "1",
                "properties": { "name": "G", "owner_id": "999" },
                "roles": [{ "id": "1", "name": "@everyone", "permissions": "0" }],
                "channels": [{ "id": "11", "type": 0, "name": "general", "position": 0 }],
                "members": []
            }],
            "private_channels": [],
            "read_states": []
        }),
    });
    state
}

fn message(id: u64, content: &str, embeds: Value, flags: u64) -> Message {
    serde_json::from_value(json!({
        "id": id.to_string(), "channel_id": CHANNEL.to_string(), "content": content,
        "timestamp": "2026-11-18T10:00:00+00:00", "flags": flags,
        "author": { "id": "200", "username": "alice" }, "embeds": embeds
    }))
    .expect("message")
}

/// Rows of `messages` as the pane builds them (UTC, "now" = 12:00).
fn rows_of(state: &mut ClientState, messages: Vec<Message>) -> Vec<MessageRow> {
    state.messages.load(Snowflake(CHANNEL), messages, LoadMode::Replace);
    let utc = FixedOffset::east_opt(0).expect("offset");
    let now = DateTime::parse_from_rfc3339("2026-11-18T12:00:00+00:00").expect("now").with_timezone(&utc);
    message_pane_at(state, Snowflake(CHANNEL), None, None, &now).rows
}

fn media(url: &str, w: u32, h: u32) -> Value {
    json!({ "url": url, "proxy_url": format!("https://media.example/external/{}", url.trim_start_matches("https://")),
            "width": w, "height": h })
}

fn rich_card_embed() -> Value {
    json!({
        "type": "rich", "url": "https://bot.example/r", "title": "Release", "description": "**Big** news",
        "color": 0x00ff00, "timestamp": "2026-11-18T10:00:00+00:00",
        "author": { "name": "Bot", "url": "https://bot.example", "proxy_icon_url": "https://media.example/i.png" },
        "footer": { "text": "v1.0" },
        "fields": [
            { "name": "a", "value": "1", "inline": true }, { "name": "b", "value": "2", "inline": true },
            { "name": "c", "value": "3", "inline": true }, { "name": "d", "value": "4", "inline": true },
            { "name": "e", "value": "5", "inline": false }
        ]
    })
}

fn sample_embeds() -> Value {
    json!([
        // 0: tenor gifv -> media only.
        { "type": "gifv", "url": "https://tenor.com/view/x", "provider": { "name": "Tenor" },
          "video": media("https://media.tenor.com/x.mp4", 498, 280), "thumbnail": media("https://media.tenor.com/x.png", 498, 280) },
        // 1: YouTube article -> card with the video poster, no description.
        { "type": "article", "url": "https://www.youtube.com/watch?v=abc", "title": "Video", "description": "long text",
          "provider": { "name": "YouTube", "url": "https://www.youtube.com" },
          "video": media("https://www.youtube.com/embed/abc", 1280, 720), "thumbnail": media("https://i.ytimg.com/abc.jpg", 1280, 720) },
        // 2: the same URL again (rich image) -> dropped.
        { "type": "rich", "url": "https://www.youtube.com/watch?v=abc", "image": media("https://i.ytimg.com/abc.jpg", 1280, 720) },
        // 3: rich card with fields.
        rich_card_embed(),
        // 4: article with a thumbnail -> promoted to full-width media.
        { "type": "article", "url": "https://news.example/a", "title": "News", "thumbnail": media("https://news.example/t.jpg", 800, 600) },
        // 5: link with a thumbnail -> 80px inline thumbnail.
        { "type": "link", "url": "https://site.example/", "title": "Site", "thumbnail": media("https://site.example/i.png", 200, 100) },
        // 6: animated image -> animated variant.
        { "type": "image", "url": "https://gif.example/a.gif",
          "thumbnail": { "url": "https://gif.example/a.gif", "proxy_url": "https://media.example/external/a.gif",
                         "width": 220, "height": 160, "content_type": "image/gif", "flags": 32 } },
        // 7: media without a size -> skipped.
        { "type": "image", "url": "https://bad.example/x.png", "thumbnail": { "url": "https://bad.example/x.png" } }
    ])
}

#[test]
fn classifies_like_the_web_client() {
    let mut state = state();
    let rows = rows_of(&mut state, vec![message(1, "links", sample_embeds(), 0)]);
    let embeds = &rows[0].embeds;
    assert_eq!(embeds.iter().map(|e| e.index).collect::<Vec<_>>(), [0, 1, 3, 4, 5, 6], "duplicate and invalid dropped");

    let EmbedBody::Media(gifv) = &embeds[0].body else { panic!("gifv is media only") };
    assert_eq!(gifv.kind, MediaKind::Gifv);
    assert_eq!((gifv.width, gifv.height), (400., 225.));
    assert_eq!(gifv.source, None);
    assert_eq!(gifv.poster.as_deref(), Some("https://media.example/external/media.tenor.com/x.png"));
    assert_eq!(gifv.open_url.as_deref(), Some("https://tenor.com/view/x"));

    let EmbedBody::Card(youtube) = &embeds[1].body else { panic!("YouTube is a card") };
    assert!(youtube.description.is_none(), "YouTube cards hide the description");
    let CardMedia::Single(video) = &youtube.media else { panic!("video media") };
    assert_eq!(video.kind, MediaKind::Video);
    assert_eq!((video.width, video.height), (400., 225.));
    assert_eq!(video.poster.as_deref(), Some("https://media.example/external/i.ytimg.com/abc.jpg"));
    assert_eq!(video.open_url.as_deref(), Some("https://www.youtube.com/watch?v=abc"));
    assert_eq!(youtube.max_width, 432.);
    assert_eq!(youtube.title.as_ref().and_then(|t| t.url.as_deref()), Some("https://www.youtube.com/watch?v=abc"));

    let EmbedBody::Card(rich) = &embeds[2].body else { panic!("rich card") };
    assert_eq!(rich.color, Some(0x00ff00));
    assert_eq!(rich.fields.iter().map(Vec::len).collect::<Vec<_>>(), [3, 1, 1], "3 inline per row, then own rows");
    assert!(matches!(rich.description, Some(EmbedText::Markdown(_))), "rich descriptions are markdown");
    assert_eq!(rich.max_width, 516.);
    let footer = rich.footer.as_ref().expect("footer");
    assert_eq!(footer.text.as_deref(), Some("v1.0"));
    assert_eq!(footer.time.as_deref(), Some("Today at 10:00"));
    assert_eq!(rich.author.as_ref().and_then(|a| a.icon.as_deref()), Some("https://media.example/i.png"));

    let EmbedBody::Card(article) = &embeds[3].body else { panic!("article card") };
    assert!(article.thumbnail.is_none());
    let CardMedia::Single(promoted) = &article.media else { panic!("promoted thumbnail") };
    assert_eq!((promoted.kind, promoted.width, promoted.height), (MediaKind::Image, 400., 300.));
    assert_eq!(article.max_width, 432.);

    let EmbedBody::Card(link) = &embeds[4].body else { panic!("link card") };
    assert_eq!(link.media, CardMedia::None);
    let thumb = link.thumbnail.as_ref().expect("inline thumbnail");
    assert_eq!((thumb.width, thumb.height), (80., 80.));
    assert_eq!(link.max_width, 516., "an inline thumbnail keeps the text width");
    assert!(matches!(link.title.as_ref().map(|t| &t.text), Some(EmbedText::Plain(_))), "linked titles are plain");

    let EmbedBody::Media(gif) = &embeds[5].body else { panic!("gif is media only") };
    assert_eq!((gif.kind, gif.width, gif.height), (MediaKind::Animated, 220., 160.));
}

#[test]
fn proxy_variants_use_device_pixels() {
    let mut state = state();
    let rows = rows_of(&mut state, vec![message(1, "links", sample_embeds(), 0)]);
    let embeds = &rows[0].embeds;
    let media_of = |i: usize| match &embeds[i].body {
        EmbedBody::Media(m) => m.clone(),
        EmbedBody::Card(c) => match &c.media {
            CardMedia::Single(m) => m.clone(),
            _ => panic!("no single media"),
        },
    };
    let gif = media_of(5);
    assert_eq!(
        variant_url(&gif, 1.5).as_deref(),
        Some("https://media.example/external/a.gif?width=330&height=240&animated=true")
    );
    assert_eq!(
        still_url(&gif, 1.5).as_deref(),
        Some("https://media.example/external/a.gif?format=webp&width=330&height=240")
    );
    let gifv = media_of(0);
    assert_eq!(
        variant_url(&gifv, 1.).as_deref(),
        Some("https://media.example/external/media.tenor.com/x.png?width=400&height=225&animated=true")
    );
    assert_eq!(
        still_url(&gifv, 1.).as_deref(),
        Some("https://media.example/external/media.tenor.com/x.png?format=webp&width=400&height=225")
    );
    let video = media_of(1);
    assert_eq!(
        variant_url(&video, 2.).as_deref(),
        Some("https://media.example/external/i.ytimg.com/abc.jpg?format=webp&width=800&height=450")
    );
    let promoted = media_of(3);
    assert_eq!(
        variant_url(&promoted, 1.).as_deref(),
        Some("https://media.example/external/news.example/t.jpg?format=webp&width=400&height=300")
    );
}

#[test]
fn bare_media_link_hides_the_url_and_suppress_hides_embeds() {
    let gifv = json!([{ "type": "gifv", "url": "https://tenor.com/view/x", "provider": { "name": "Tenor" },
        "video": media("https://media.tenor.com/x.mp4", 498, 280), "thumbnail": media("https://media.tenor.com/x.png", 498, 280) }]);
    let mut state = state();
    let rows = rows_of(
        &mut state,
        vec![
            message(1, "https://tenor.com/view/x", gifv.clone(), 0),
            message(2, "look https://tenor.com/view/x", gifv.clone(), 0),
            message(3, "https://tenor.com/view/x", gifv.clone(), message_flags::SUPPRESS_EMBEDS),
            message(4, "https://bot.example/r", json!([rich_card_embed()]), 0),
            message(5, "||https://tenor.com/view/x||", gifv, 0),
        ],
    );
    assert!(rows[0].document.is_none() && rows[0].embeds.len() == 1, "the gif replaces the bare URL");
    assert!(rows[1].document.is_some(), "text around the URL stays");
    assert!(rows[2].embeds.is_empty() && rows[2].document.is_some(), "suppressed: no embeds, URL shown");
    assert!(rows[3].document.is_some(), "a card does not hide the URL");
    assert!(rows[4].document.is_some(), "a spoilered URL is not a bare link");
    assert!(rows[4].embeds[0].spoiler, "the embed of a spoilered URL is a spoiler");
    assert!(!rows[0].embeds[0].spoiler);
}

#[test]
fn suppress_action_for_own_messages_with_embeds() {
    use crate::ui::message_actions::{MessageAction, available_actions};
    let mut own = message(1, "https://tenor.com/view/x", sample_embeds(), 0);
    own.author = serde_json::from_value(json!({ "id": "100", "username": "me" })).expect("user");
    let mut own_suppressed = message(2, "x", json!([]), message_flags::SUPPRESS_EMBEDS);
    own_suppressed.author = own.author.clone();
    let mut own_plain = message(3, "x", json!([]), 0);
    own_plain.author = own.author.clone();
    let other = message(4, "x", sample_embeds(), 0);
    let mut state = state();
    let rows = rows_of(&mut state, vec![own, own_suppressed, own_plain, other]);
    let offers = |row: &MessageRow| available_actions(&row.actions).contains(&MessageAction::SuppressEmbeds);
    assert!(offers(&rows[0]));
    assert!(offers(&rows[1]) && rows[1].actions.embeds_suppressed, "suppressed: offered to undo");
    assert!(!offers(&rows[2]), "nothing to suppress");
    assert!(!offers(&rows[3]), "someone else's message without Manage Messages");
}

#[test]
fn render_embeds_setting_turns_embeds_off() {
    let mut state = state();
    state.user_settings.raw = json!({ "render_embeds": false });
    let rows = rows_of(&mut state, vec![message(1, "https://tenor.com/view/x", sample_embeds(), 0)]);
    assert!(rows[0].embeds.is_empty());
    assert!(rows[0].document.is_some());
}

#[test]
fn gallery_tiles() {
    assert_eq!(gallery::tile_side(2), (2, 198.));
    assert_eq!(gallery::tile_side(3), (3, 130.));
    assert_eq!(gallery::tile_side(4), (2, 198.));
    assert_eq!(gallery::tile_side(1).0, 1);
    // Two images of one URL make a gallery in the first embed's card.
    let embeds = json!([
        { "type": "rich", "url": "https://g.example/p", "title": "Pics", "image": media("https://g.example/1.png", 800, 600) },
        { "type": "rich", "url": "https://g.example/p", "image": media("https://g.example/2.png", 800, 600) }
    ]);
    let mut state = state();
    let rows = rows_of(&mut state, vec![message(1, "x", embeds, 0)]);
    assert_eq!(rows[0].embeds.len(), 1);
    let EmbedBody::Card(card) = &rows[0].embeds[0].body else { panic!("card") };
    let CardMedia::Gallery(tiles) = &card.media else { panic!("gallery") };
    assert_eq!(tiles.iter().map(|t| (t.width, t.height)).collect::<Vec<_>>(), [(198., 198.); 2]);
}

/// A YouTube link unfurls into an article (video, 1280x720 thumbnail) and a
/// rich embed with only a 480x360 image for the same URL: one card with the
/// video poster, no gallery of both thumbnails.
#[test]
fn youtube_alternate_thumbnail_is_not_a_gallery() {
    let url = "https://www.youtube.com/watch?v=abc";
    let embeds = json!([
        { "type": "article", "url": url, "title": "Video",
          "provider": { "name": "YouTube", "url": "https://www.youtube.com" },
          "video": media("https://www.youtube.com/embed/abc", 1280, 720),
          "thumbnail": media("https://i.ytimg.com/vi/abc/maxresdefault.jpg", 1280, 720) },
        { "type": "rich", "url": url, "image": media("https://i.ytimg.com/vi/abc/hqdefault.jpg", 480, 360) }
    ]);
    let mut state = state();
    let rows = rows_of(&mut state, vec![message(1, url, embeds, 0)]);
    assert_eq!(rows[0].embeds.len(), 1, "the second embed of the URL is merged");
    let EmbedBody::Card(card) = &rows[0].embeds[0].body else { panic!("card") };
    let CardMedia::Single(video) = &card.media else { panic!("single video poster, not a gallery") };
    assert_eq!(video.kind, MediaKind::Video);
    assert_eq!(video.poster.as_deref(), Some("https://media.example/external/i.ytimg.com/vi/abc/maxresdefault.jpg"));
}

/// A media-only embed with only an `image` draws nothing (web client), so
/// an alternate thumbnail under another URL does not show beside the card.
#[test]
fn image_only_media_embed_is_not_drawn() {
    let embeds = json!([
        { "type": "rich", "url": "https://youtu.be/abc", "image": media("https://i.ytimg.com/vi/abc/hqdefault.jpg", 480, 360) }
    ]);
    let mut state = state();
    let rows = rows_of(&mut state, vec![message(1, "x", embeds, 0)]);
    assert!(rows[0].embeds.is_empty());
}

/// GPUI animates a multi-frame image only for an `img` with an element id.
#[gpui_kit::test]
fn media_images_have_an_id_so_gifs_animate(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let cache = media::image_cache(cx);
        let image = media::media_img("media", "https://media.example/a.gif?animated=true", &cache);
        assert!(gpui_kit::Element::id(&image).is_some());
    });
}

/// Failed or loading entries hold no pixels; past the entry cap the ones
/// not drawn recently are dropped so broken URLs do not pile up.
#[gpui_kit::test]
fn image_cache_forgets_pixelless_entries_past_the_cap(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let cache = media::image_cache(cx);
        cache.update(cx, |cache, cx| {
            let now = std::time::Instant::now();
            for i in 0..=media::MAX_ENTRIES {
                let resource = gpui_kit::Resource::Uri(format!("https://broken.example/{i}.png").into());
                cache.insert_loading(&resource, now, cx);
            }
            assert_eq!(cache.len(), media::MAX_ENTRIES + 1);
            cache.prune_empty(now);
            assert_eq!(cache.len(), media::MAX_ENTRIES + 1, "drawn just now: kept");
            cache.prune_empty(now + std::time::Duration::from_secs(3));
            assert_eq!(cache.len(), 0);
        });
    });
}

struct Host {
    rows: Vec<MessageRow>,
}

impl Render for Host {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = crate::theme::Theme::current(cx);
        let view = WeakEntity::new_invalid();
        div()
            .w(px(900.))
            .flex()
            .flex_col()
            .children(self.rows.iter().map(|row| message_row::render(row, &theme, &view, RowBar::None, 1., cx)))
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
}

/// A rich card (bar, author, title, description, fields, footer) and an
/// image embed drawn through the real message row.
#[gpui_kit::test]
fn renders_rich_card_and_image(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let image = json!({ "type": "image", "url": "https://img.example/p.png", "thumbnail": media("https://img.example/p.png", 1280, 720) });
    let mut state = state();
    let rows = rows_of(&mut state, vec![message(1, "", json!([rich_card_embed()]), 0), message(2, "", json!([image]), 0)]);
    let (_, cx) = cx.add_window_view(|window, cx| {
        let host = cx.new(|_| Host { rows });
        gpui_kit::base::Root::new(host, window, cx)
    });
    draw(cx);
    let card = cx.debug_bounds("embed-card").expect("card drawn");
    assert!(card.size.width <= px(516.) && card.size.width > px(100.), "card width {:?}", card.size.width);
    let bar = cx.debug_bounds("embed-color-bar").expect("colour bar");
    assert_eq!(bar.size.width, px(4.));
    assert_eq!(bar.size.height, card.size.height - px(2.), "bar spans the card inside its border");
    for part in ["embed-author", "embed-title", "embed-description", "embed-field", "embed-footer"] {
        let bounds = cx.debug_bounds(part).unwrap_or_else(|| panic!("{part} not drawn"));
        assert!(card.contains(&bounds.origin), "{part} inside the card");
        assert!(bounds.size.height > px(0.), "{part} has height");
    }
    let title = cx.debug_bounds("embed-title").expect("title");
    let description = cx.debug_bounds("embed-description").expect("description");
    let footer = cx.debug_bounds("embed-footer").expect("footer");
    assert!(title.origin.y < description.origin.y && description.origin.y < footer.origin.y, "card part order");
    let media = cx.debug_bounds("embed-media").expect("image embed drawn");
    assert_eq!((media.size.width, media.size.height), (px(400.), px(225.)));
    assert!(media.origin.y > card.origin.y + card.size.height, "the image row is below the card row");
}

/// A spoilered media embed shows a cover until clicked; the reveal is kept
/// app-wide for the message.
#[gpui_kit::test]
fn spoiler_cover_reveals_on_click(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let image = json!({ "type": "image", "url": "https://img.example/s.png", "thumbnail": media("https://img.example/s.png", 300, 200) });
    let mut state = state();
    let rows = rows_of(&mut state, vec![message(77, "||https://img.example/s.png||", json!([image]), 0)]);
    assert!(rows[0].embeds[0].spoiler);
    let (_, cx) = cx.add_window_view(|window, cx| {
        let host = cx.new(|_| Host { rows });
        gpui_kit::base::Root::new(host, window, cx)
    });
    draw(cx);
    let cover = cx.debug_bounds("embed-cover").expect("spoiler cover drawn");
    assert_eq!((cover.size.width, cover.size.height), (px(300.), px(200.)));
    assert!(cx.debug_bounds("embed-media").is_none(), "nothing loads behind the cover");
    cx.simulate_click(cover.center(), Modifiers::default());
    cx.update(|window, _| window.refresh());
    draw(cx);
    assert!(cx.update(|_, cx| is_revealed(Snowflake(77), 0, cx)));
    assert!(cx.debug_bounds("embed-cover").is_none(), "revealed");
    assert!(cx.debug_bounds("embed-media").is_some());
}

/// Mature (NSFW) media shows an NSFW cover (`media.rs:283-286`) until clicked.
#[gpui_kit::test]
fn nsfw_cover_reveals_on_click(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let image = json!({
        "type": "image",
        "url": "https://img.example/nsfw.png",
        "thumbnail": {
            "url": "https://img.example/nsfw.png",
            "proxy_url": "https://media.example/external/img.example/nsfw.png",
            "width": 320,
            "height": 200,
            "nsfw": true
        }
    });
    let mut state = state();
    let rows = rows_of(&mut state, vec![message(88, "", json!([image]), 0)]);
    assert!(!rows[0].embeds[0].spoiler, "not a spoiler, only explicit/NSFW");
    let EmbedBody::Media(media) = &rows[0].embeds[0].body else { panic!("media embed") };
    assert!(media.explicit);
    let (_, cx) = cx.add_window_view(|window, cx| {
        let host = cx.new(|_| Host { rows });
        gpui_kit::base::Root::new(host, window, cx)
    });
    draw(cx);
    let cover = cx.debug_bounds("embed-cover").expect("NSFW cover drawn");
    assert_eq!((cover.size.width, cover.size.height), (px(320.), px(200.)));
    assert!(cx.debug_bounds("embed-media").is_none(), "nothing loads behind the NSFW cover");
    cx.simulate_click(cover.center(), Modifiers::default());
    cx.update(|window, _| window.refresh());
    draw(cx);
    assert!(cx.update(|_, cx| is_revealed(Snowflake(88), 0, cx)));
    assert!(cx.debug_bounds("embed-cover").is_none(), "NSFW cover revealed");
    assert!(cx.debug_bounds("embed-media").is_some());
}

/// A video embed with a poster thumbnail and video URL classifies as
/// `MediaKind::Video` and renders its poster and play overlay without panic.
#[gpui_kit::test]
fn video_embed_renders_poster_and_play_overlay(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let video_embed = json!({
        "type": "article",
        "url": "https://www.youtube.com/watch?v=vid123",
        "title": "Demo Video",
        "provider": { "name": "YouTube", "url": "https://www.youtube.com" },
        "video": media("https://www.youtube.com/embed/vid123", 1280, 720),
        "thumbnail": media("https://i.ytimg.com/vid123.jpg", 1280, 720)
    });
    let mut state = state();
    let rows = rows_of(&mut state, vec![message(99, "", json!([video_embed]), 0)]);
    assert_eq!(rows[0].embeds.len(), 1);
    let EmbedBody::Card(card) = &rows[0].embeds[0].body else { panic!("video card") };
    let CardMedia::Single(video) = &card.media else { panic!("single video media") };
    assert_eq!(video.kind, MediaKind::Video);
    assert_eq!(video.poster.as_deref(), Some("https://media.example/external/i.ytimg.com/vid123.jpg"));
    assert_eq!(video.open_url.as_deref(), Some("https://www.youtube.com/watch?v=vid123"));
    assert_eq!(
        variant_url(video, 1.).as_deref(),
        Some("https://media.example/external/i.ytimg.com/vid123.jpg?format=webp&width=400&height=225")
    );

    let (_, cx) = cx.add_window_view(|window, cx| {
        let host = cx.new(|_| Host { rows });
        gpui_kit::base::Root::new(host, window, cx)
    });
    draw(cx);

    let card_bounds = cx.debug_bounds("embed-card").expect("video card drawn");
    let media_bounds = cx.debug_bounds("embed-media").expect("video poster + play overlay drawn");
    assert_eq!((media_bounds.size.width, media_bounds.size.height), (px(400.), px(225.)));
    assert!(card_bounds.contains(&media_bounds.origin), "video preview inside card");
}

#[test]
fn gifv_image_source_and_video_proxy_url_detection() {
    assert!(media::is_video_proxy_url("https://media.example/x.mp4"));
    assert!(media::is_video_proxy_url("https://media.example/x.WEBM?foo=1"));
    assert!(media::is_video_proxy_url("https://media.example/x.mov#frag"));
    assert!(media::is_video_proxy_url("https://media.example/x.M4V?a=1#b"));
    assert!(!media::is_video_proxy_url("https://media.example/x.gif"));
    assert!(!media::is_video_proxy_url("https://media.example/x.webp?animated=true"));

    let gif_source_gifv = MediaView {
        kind: MediaKind::Gifv,
        source: Some("https://media.example/external/tenor.com/x.gif".into()),
        poster: Some("https://media.example/external/tenor.com/x.png".into()),
        open_url: Some("https://tenor.com/view/x".into()),
        width: 400.,
        height: 225.,
        label: None,
        explicit: false,
    };
    assert_eq!(
        variant_url(&gif_source_gifv, 1.).as_deref(),
        Some("https://media.example/external/tenor.com/x.gif?width=400&height=225&animated=true")
    );
}

#[test]
fn gifv_opaque_video_proxy_with_video_content_type_uses_poster() {
    let embeds = json!([{
        "type": "gifv",
        "url": "https://tenor.com/view/opaque",
        "provider": { "name": "Tenor" },
        "video": {
            "url": "https://media.tenor.com/opaque-video",
            "proxy_url": "https://media.example/external/sig123/v2/b3BhcXVl",
            "width": 498,
            "height": 280,
            "content_type": "video/mp4"
        },
        "thumbnail": {
            "url": "https://media.tenor.com/opaque-poster",
            "proxy_url": "https://media.example/external/sig456/v2/cG9zdGVy",
            "width": 498,
            "height": 280
        }
    }]);
    let mut state = state();
    let rows = rows_of(&mut state, vec![message(1, "https://tenor.com/view/opaque", embeds, 0)]);
    let EmbedBody::Media(gifv) = &rows[0].embeds[0].body else { panic!("gifv is media only") };
    assert_eq!(gifv.kind, MediaKind::Gifv);
    assert_eq!(gifv.source, None);
    assert_eq!(
        variant_url(gifv, 1.).as_deref(),
        Some("https://media.example/external/sig456/v2/cG9zdGVy?width=400&height=225&animated=true")
    );
}
