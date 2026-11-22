//! Message embeds (`ChannelEmbed.tsx` `Embed`, `RichEmbed.tsx`,
//! `EmbedMediaRenderer.tsx`): [`embed_views`] turns a message's embeds into
//! plain view data while the message pane is built, and [`render`] draws
//! each one as bare media ([`media`]) or as a card ([`card`]).
//!
//! Rules (pure, in core `state::embed_model`): duplicates of an earlier
//! embed's URL draw nothing; `hasRichEmbedContent` picks card versus media;
//! invalid media (no url, proxy url or size) is skipped; spoilered URLs
//! cover the whole embed and mature media its media until clicked. The
//! caller drops all embeds when `render_embeds` is off or the message has
//! SUPPRESS_EMBEDS.

mod card;
mod gallery;
mod media;

pub(crate) use media::{image_cache, is_video_proxy_url};

use std::collections::HashSet;
use std::sync::Arc;

use fluxer_client_core::state::{
    EMBED_MAX_WIDTH, EMBED_MEDIA_OUTER_WIDTH, EMBED_TEXT_OUTER_WIDTH, EMBED_THUMBNAIL_SIZE, Embed, EmbedKind, EmbedMedia,
    Message, Snowflake, ValidMedia, embed_media_size, gallery_images, inline_thumbnail_width, is_duplicate,
    spoiler_urls, valid_media,
};
use fluxer_client_markdown::{Block, Document, MarkdownContext};
use gpui_kit::{AnyElement, App, Global, SharedString, WeakEntity, div, prelude::*};

use crate::client_view::ClientView;
use crate::theme::Theme;

/// How a [`MediaView`] is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    /// A still: `?format=webp&width=W&height=H`.
    Image,
    /// GIF or animated image: `?width=W&height=H&animated=true`, the still
    /// when that fails.
    Animated,
    /// Tenor/Klipy gifv: the animated variant of the mp4 is tried, the
    /// thumbnail still shows while it loads or when it is not an image.
    Gifv,
    /// YouTube or another video: the poster with a play button; a click
    /// opens the page (there is no video decoder).
    Video,
    /// Audio: a labelled row that opens the file externally.
    Audio,
}

/// One drawable media item.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaView {
    pub kind: MediaKind,
    /// Proxy URL the size variants are built from (the video's for gifv).
    pub source: Option<SharedString>,
    /// Still proxy URL: gifv thumbnail, video poster.
    pub poster: Option<SharedString>,
    /// Size in css px.
    pub width: f32,
    pub height: f32,
    /// Opened on click: the origin URL, never the proxy.
    pub open_url: Option<SharedString>,
    /// Mature media (`isMediaMatureContent`): covered until revealed.
    pub explicit: bool,
    /// Audio/video title.
    pub label: Option<SharedString>,
}

/// Text of a card part: markdown for rich/bluesky embeds and fields.
#[derive(Debug, Clone, PartialEq)]
pub enum EmbedText {
    Plain(SharedString),
    Markdown(Arc<Document>),
}

/// Text that may link somewhere (provider, author, title).
#[derive(Debug, Clone, PartialEq)]
pub struct LinkText {
    pub text: EmbedText,
    pub url: Option<SharedString>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuthorView {
    pub name: SharedString,
    pub url: Option<SharedString>,
    /// Proxied icon (`proxy_icon_url`).
    pub icon: Option<SharedString>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldView {
    pub name: EmbedText,
    pub value: EmbedText,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FooterView {
    pub text: Option<SharedString>,
    pub icon: Option<SharedString>,
    /// `timestamp` as "Today at 10:20".
    pub time: Option<SharedString>,
}

/// What sits under the card text.
#[derive(Debug, Clone, PartialEq)]
pub enum CardMedia {
    None,
    Single(MediaView),
    /// Images of the embeds sharing this embed's URL (`collectGalleryImages`).
    Gallery(Vec<MediaView>),
}

/// `RichEmbed`: colour bar, provider, author, title, description, fields,
/// media, footer, and an optional 80px thumbnail on the right.
#[derive(Debug, Clone, PartialEq)]
pub struct CardView {
    /// `0xRRGGBB`; `None` (or 0) uses the brand colour.
    pub color: Option<u32>,
    pub provider: Option<LinkText>,
    pub author: Option<AuthorView>,
    pub title: Option<LinkText>,
    pub description: Option<EmbedText>,
    /// Rows of fields: up to 3 inline fields share a row.
    pub fields: Vec<Vec<FieldView>>,
    pub media: CardMedia,
    pub thumbnail: Option<MediaView>,
    pub footer: Option<FooterView>,
    /// 432 with full-width media, else 516 (outer, css px).
    pub max_width: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EmbedBody {
    /// No card text: only the media, no border or colour bar.
    Media(MediaView),
    Card(Box<CardView>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmbedView {
    /// Index in `message.embeds` (reveal state key).
    pub index: usize,
    /// A URL of the embed is inside `||spoiler||` in the content.
    pub spoiler: bool,
    pub body: EmbedBody,
}

/// Which markdown flavour a text is parsed with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextKind {
    /// Titles and field names (`RESTRICTED_INLINE_REPLY`).
    Inline,
    /// Descriptions and field values (`RESTRICTED_EMBED_DESCRIPTION`).
    Description,
}

impl TextKind {
    pub fn context(self) -> MarkdownContext {
        match self {
            Self::Inline => MarkdownContext::InlineReply,
            Self::Description => MarkdownContext::EmbedDescription,
        }
    }
}

/// The message's embeds to draw, in order. `markdown` parses (and may
/// cache) a text; `time` formats an ISO timestamp for footers.
pub fn embed_views(
    message: &Message,
    markdown: &mut dyn FnMut(&str, TextKind) -> Arc<Document>,
    time: &dyn Fn(&str) -> Option<String>,
) -> Vec<EmbedView> {
    let embeds = &message.embeds;
    let mut views = Vec::new();
    for (index, embed) in embeds.iter().enumerate() {
        if is_duplicate(index, embeds) {
            continue;
        }
        let body = if embed.has_rich_content() {
            EmbedBody::Card(Box::new(card_view(embed, index, embeds, markdown, time)))
        } else {
            match media_only_view(embed) {
                Some(media) => EmbedBody::Media(media),
                None => continue,
            }
        };
        let spoiler = !spoiler_urls(embed, &message.content).is_empty();
        views.push(EmbedView { index, spoiler, body });
    }
    views
}

/// `shouldHideContent`: the body is a bare link and every embed is media
/// only, so the media stands in for the URL text.
pub fn hides_bare_url(message: &Message, document: &Document) -> bool {
    !message.embeds.is_empty() && message.embeds.iter().all(Embed::is_media_only) && is_single_link(document)
}

/// One paragraph made only of one link (`astNodes` = `[Link]`).
fn is_single_link(document: &Document) -> bool {
    let [Block::Paragraph(spans)] = document.blocks.as_slice() else { return false };
    let Some(first) = spans.first().and_then(|s| s.style.link.as_ref()) else { return false };
    spans
        .iter()
        .all(|s| s.style.spoiler.is_none() && s.style.link.as_ref().is_some_and(|link| link.id == first.id))
}

fn shared(text: &str) -> SharedString {
    SharedString::from(text.to_owned())
}

fn non_empty(text: Option<&str>) -> Option<&str> {
    text.filter(|t| !t.is_empty())
}

/// Still or animated by the media's type and flags (`resolveEmbedImageSource`,
/// `isGif`).
fn image_kind(media: &EmbedMedia) -> MediaKind {
    if media.is_gif() || media.wants_animation() { MediaKind::Animated } else { MediaKind::Image }
}

/// An image fitted to the embed bounds; a click opens the original.
fn image_view(media: ValidMedia<'_>) -> MediaView {
    let (width, height) = embed_media_size(media.width, media.height, false);
    sized_image(media, width as f32, height as f32)
}

fn sized_image(media: ValidMedia<'_>, width: f32, height: f32) -> MediaView {
    MediaView {
        kind: image_kind(media.media),
        source: Some(shared(media.proxy_url)),
        poster: None,
        width,
        height,
        open_url: Some(shared(media.url)),
        explicit: media.media.is_explicit(),
        label: None,
    }
}

/// `EmbedYouTube` / `EmbedVideo`: the thumbnail as poster. YouTube portrait
/// videos may be 450 tall.
fn video_view(embed: &Embed, video: ValidMedia<'_>) -> MediaView {
    let tall = embed.is_youtube() && video.height > video.width;
    let (width, height) = embed_media_size(video.width, video.height, tall);
    let thumbnail = valid_media(embed.thumbnail.as_ref());
    MediaView {
        kind: MediaKind::Video,
        source: None,
        poster: thumbnail.map(|t| shared(t.proxy_url)),
        width: width as f32,
        height: height as f32,
        open_url: non_empty(embed.url.as_deref()).or(Some(video.url)).map(shared),
        explicit: video.media.is_explicit() || thumbnail.is_some_and(|t| t.media.is_explicit()),
        label: non_empty(embed.title.as_deref()).map(shared),
    }
}

/// `Embed` media-only dispatch, in the web client's order.
fn media_only_view(embed: &Embed) -> Option<MediaView> {
    let kind = embed.embed_kind();
    if kind == EmbedKind::Audio
        && let Some(audio) = embed.audio.as_ref().filter(|a| a.proxy_url.is_some())
    {
        return Some(MediaView {
            kind: MediaKind::Audio,
            source: audio.proxy_url.as_deref().map(shared),
            poster: None,
            width: EMBED_MAX_WIDTH as f32,
            height: 48.,
            open_url: non_empty(embed.url.as_deref()).or(non_empty(Some(&audio.url))).map(shared),
            explicit: false,
            label: non_empty(embed.title.as_deref()).or(non_empty(Some(&audio.url))).map(shared),
        });
    }
    if kind == EmbedKind::Video
        && let Some(video) = valid_media(embed.video.as_ref())
    {
        return Some(video_view(embed, video));
    }
    let thumbnail = valid_media(embed.thumbnail.as_ref());
    if kind == EmbedKind::Image
        && let Some(thumbnail) = thumbnail.filter(|t| t.media.is_animated())
    {
        return Some(MediaView { kind: MediaKind::Animated, ..image_view(thumbnail) });
    }
    if kind == EmbedKind::Gifv
        && let (Some(video), Some(thumbnail), Some(url)) =
            (valid_media(embed.video.as_ref()), thumbnail, non_empty(embed.url.as_deref()))
    {
        let (width, height) = embed_media_size(video.width, video.height, false);
        return Some(MediaView {
            kind: MediaKind::Gifv,
            source: Some(shared(video.proxy_url)),
            poster: Some(shared(thumbnail.proxy_url)),
            width: width as f32,
            height: height as f32,
            open_url: Some(shared(url)),
            explicit: video.media.is_explicit() || thumbnail.media.is_explicit(),
            label: None,
        });
    }
    // The web client draws nothing for a media-only embed with only an
    // `image` (unfurlers add one beside a card, e.g. a second YouTube
    // thumbnail); its image still joins a same-URL card's gallery.
    thumbnail.map(image_view)
}

/// `EmbedMediaRenderer`: video, else image, else thumbnail.
fn card_media_view(embed: &Embed) -> Option<MediaView> {
    if let Some(video) = valid_media(embed.video.as_ref()) {
        return Some(video_view(embed, video));
    }
    valid_media(embed.image.as_ref()).or_else(|| valid_media(embed.thumbnail.as_ref())).map(image_view)
}

/// `groupFields`: inline fields share rows of at most 3; others get a row.
fn field_rows(embed: &Embed, markdown: &mut dyn FnMut(&str, TextKind) -> Arc<Document>) -> Vec<Vec<FieldView>> {
    const MAX_INLINE_PER_ROW: usize = 3;
    let mut rows = Vec::new();
    let mut current = Vec::new();
    for field in &embed.fields {
        let view = FieldView {
            name: EmbedText::Markdown(markdown(&field.name, TextKind::Inline)),
            value: EmbedText::Markdown(markdown(&field.value, TextKind::Description)),
        };
        if field.inline {
            current.push(view);
            if current.len() == MAX_INLINE_PER_ROW {
                rows.push(std::mem::take(&mut current));
            }
        } else {
            if !current.is_empty() {
                rows.push(std::mem::take(&mut current));
            }
            rows.push(vec![view]);
        }
    }
    if !current.is_empty() {
        rows.push(current);
    }
    rows
}

fn card_view(
    embed: &Embed,
    index: usize,
    embeds: &[Embed],
    markdown: &mut dyn FnMut(&str, TextKind) -> Arc<Document>,
    time: &dyn Fn(&str) -> Option<String>,
) -> CardView {
    let kind = embed.embed_kind();
    let rich_text = kind.allows_markdown();
    let has_video = valid_media(embed.video.as_ref()).is_some();
    let has_image = valid_media(embed.image.as_ref()).is_some();
    let thumbnail = valid_media(embed.thumbnail.as_ref());
    let has_any_media = has_video || has_image || thumbnail.is_some();
    let gallery = gallery_images(index, embeds);
    // Divergence: a YouTube card with a video draws the one poster with a
    // play button, never a gallery. The unfurler adds a second same-URL
    // embed with another thumbnail size (hqdefault beside maxresdefault),
    // which the web client is seen to draw as a single card.
    let youtube_video = has_video && embed.is_youtube();
    let show_gallery = !youtube_video && (gallery.len() > 1 || (!has_any_media && !gallery.is_empty()));
    let promotes_thumbnail = matches!(kind, EmbedKind::Article | EmbedKind::Image);
    let inline_thumbnail =
        (!has_video && thumbnail.is_some() && !has_image && !promotes_thumbnail && !show_gallery).then_some(thumbnail).flatten();
    let renders_media = has_any_media || show_gallery;
    let media = if inline_thumbnail.is_some() {
        CardMedia::None
    } else if show_gallery {
        CardMedia::Gallery(gallery::tiles(&gallery, sized_image))
    } else {
        card_media_view(embed).map_or(CardMedia::None, CardMedia::Single)
    };
    let thumbnail = inline_thumbnail.map(|t| {
        sized_image(t, inline_thumbnail_width(t.width, t.height) as f32, EMBED_THUMBNAIL_SIZE as f32)
    });
    let narrow = renders_media && thumbnail.is_none();
    let text = |value: &str, kind: TextKind, markdown: &mut dyn FnMut(&str, TextKind) -> Arc<Document>| {
        if rich_text { EmbedText::Markdown(markdown(value, kind)) } else { EmbedText::Plain(shared(value)) }
    };
    let url = non_empty(embed.url.as_deref()).map(shared);
    let title = non_empty(embed.title.as_deref()).map(|title| LinkText {
        // A linked title is drawn as one link, so as plain text.
        text: if url.is_some() { EmbedText::Plain(shared(title)) } else { text(title, TextKind::Inline, markdown) },
        url: url.clone(),
    });
    let description = if embed.is_youtube() {
        None
    } else {
        non_empty(embed.description.as_deref()).map(|d| text(d, TextKind::Description, markdown))
    };
    let footer = (embed.footer.is_some() || embed.timestamp.is_some()).then(|| FooterView {
        text: embed.footer.as_ref().and_then(|f| non_empty(Some(&f.text))).map(shared),
        icon: embed.footer.as_ref().and_then(|f| non_empty(f.proxy_icon_url.as_deref())).map(shared),
        time: embed.timestamp.as_deref().and_then(time).map(SharedString::from),
    });
    CardView {
        color: embed.color.filter(|c| *c != 0),
        provider: embed.provider.as_ref().filter(|p| !p.name.is_empty()).map(|p| LinkText {
            text: EmbedText::Plain(shared(&p.name)),
            url: non_empty(p.url.as_deref()).map(shared),
        }),
        author: embed.author.as_ref().map(|a| AuthorView {
            name: shared(&a.name),
            url: non_empty(a.url.as_deref()).map(shared),
            icon: non_empty(a.proxy_icon_url.as_deref()).map(shared),
        }),
        title,
        description,
        fields: field_rows(embed, markdown),
        media,
        thumbnail,
        footer,
        max_width: if narrow { EMBED_MEDIA_OUTER_WIDTH } else { EMBED_TEXT_OUTER_WIDTH } as f32,
    }
}

// ------------------------------------------------------------- rendering

/// Spoiler and mature-media reveals, per (message, embed index). App-wide
/// so a reveal survives the row leaving the virtual list.
#[derive(Default)]
struct EmbedReveals(HashSet<(Snowflake, usize)>);

impl Global for EmbedReveals {}

fn is_revealed(message_id: Snowflake, index: usize, cx: &App) -> bool {
    cx.try_global::<EmbedReveals>().is_some_and(|r| r.0.contains(&(message_id, index)))
}

/// What the drawing code of one embed needs.
#[derive(Clone)]
struct EmbedCx {
    theme: Theme,
    /// Window scale factor: proxy sizes are in device px.
    scale: f32,
    message_id: Snowflake,
    index: usize,
    view: WeakEntity<ClientView>,
}

impl EmbedCx {
    fn id(&self, slot: &str) -> SharedString {
        format!("embed-{}-{}-{slot}", self.message_id.0, self.index).into()
    }

    fn revealed(&self, cx: &App) -> bool {
        is_revealed(self.message_id, self.index, cx)
    }

    /// Click handler that reveals this embed's spoiler and mature media.
    fn reveal_handler(&self) -> impl Fn(&gpui_kit::ClickEvent, &mut gpui_kit::Window, &mut App) + 'static {
        let (key, view) = ((self.message_id, self.index), self.view.clone());
        move |_, _, cx| {
            cx.default_global::<EmbedReveals>().0.insert(key);
            let _ = view.update(cx, |v, cx| v.redraw_messages(cx));
        }
    }
}

/// Opens `url` via the trusted-domain guard on `ClientView` (http/https only).
fn open_url(url: &str, view: &WeakEntity<ClientView>, window: &mut gpui_kit::Window, cx: &mut App) {
    if view.update(cx, |v, cx| v.open_external_url(url, window, cx)).is_err() {
        #[cfg(not(test))]
        if crate::trusted_domains::is_safe_non_http_uri(url) {
            cx.open_url(url.trim());
        } else if let Some((parsed, host)) = crate::trusted_domains::parse_external_http_url(url)
            && crate::trusted_domains::is_trusted(&host, &[], &[])
        {
            cx.open_url(&parsed);
        }
    }
}

/// The embeds under a message body. `scale` is the window's scale factor.
pub fn render(
    message_id: Snowflake,
    embeds: &[EmbedView],
    theme: &Theme,
    scale: f32,
    view: &WeakEntity<ClientView>,
    cx: &mut App,
) -> AnyElement {
    let mut column = div().flex().flex_col().gap_1().mt_1();
    for embed in embeds {
        let ecx = EmbedCx { theme: *theme, scale, message_id, index: embed.index, view: view.clone() };
        let hidden = embed.spoiler && !ecx.revealed(cx);
        let element = match &embed.body {
            EmbedBody::Media(media) if hidden => media::spoiler_cover(media.width, media.height, &ecx),
            EmbedBody::Media(media) => media::render(media, &ecx.id("media"), &ecx, cx),
            EmbedBody::Card(card) => card::render(card, hidden, &ecx, cx),
        };
        // A row keeps media-only embeds and cards at their own width.
        column = column.child(div().flex().flex_row().child(element));
    }
    column.into_any_element()
}

#[cfg(test)]
mod tests;
