//! Pure embed rules of the web client: which embeds draw, as media or as a
//! card, at what size and from which proxy URL.
//!
//! Sources: `fluxer_app/src/features/channel/components/embeds/
//! {EmbedRenderUtils.ts, channel_embed/ChannelEmbedShared.ts}`,
//! `messaging/utils/{MediaProxyUtils.ts, MediaDimensionConfig.ts,
//! SpoilerUtils.ts, AttachmentCdnUrl.ts}`, `ui/utils/DimensionUtils.ts`.

use std::collections::{BTreeSet, HashMap, HashSet};

use url::Url;

use super::content::{ATTACHMENT_FLAG_ANIMATED, ATTACHMENT_FLAG_EXPLICIT, Embed, EmbedMedia, message_flags};

/// Embed media fits in 400x300 css px (`MediaDimensionConfig.ts`).
pub const EMBED_MAX_WIDTH: u32 = 400;
pub const EMBED_MAX_HEIGHT: u32 = 300;
/// Portrait videos may be 450 tall.
pub const EMBED_TALL_MAX_HEIGHT: u32 = 450;
/// Smaller fitted media is not worth showing.
pub const MIN_USEFUL_MEDIA_SIZE: u32 = 40;
/// Outer max width of a card with text only (`EMBED_TEXT_OUTER_WIDTH`).
pub const EMBED_TEXT_OUTER_WIDTH: u32 = 516;
/// Outer max width of a card with full-width media (`EMBED_MEDIA_MAX_WIDTH`).
pub const EMBED_MEDIA_OUTER_WIDTH: u32 = 432;
/// Inline right-hand thumbnail of a card (`THUMBNAIL_SIZE`).
pub const EMBED_THUMBNAIL_SIZE: u32 = 80;
/// At most this many images in one gallery (`MAX_GALLERY_MEDIA`).
pub const MAX_GALLERY_MEDIA: usize = 10;
/// `provider.url` host of YouTube embeds.
pub const YOUTUBE_PROVIDER_HOST: &str = "www.youtube.com";

/// `MessageEmbedTypes` (`ChannelConstants.ts:152`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbedKind {
    Rich,
    Article,
    Link,
    Image,
    Video,
    Audio,
    Gifv,
    Bluesky,
    /// Missing or unknown `type`.
    Other,
}

impl EmbedKind {
    pub fn of(kind: Option<&str>) -> Self {
        match kind {
            Some("rich") => Self::Rich,
            Some("article") => Self::Article,
            Some("link") => Self::Link,
            Some("image") => Self::Image,
            Some("video") => Self::Video,
            Some("audio") => Self::Audio,
            Some("gifv") => Self::Gifv,
            Some("bluesky") => Self::Bluesky,
            _ => Self::Other,
        }
    }

    /// `embedAllowsMarkdown`: title and description are markdown.
    pub fn allows_markdown(self) -> bool {
        matches!(self, Self::Rich | Self::Bluesky)
    }
}

impl Embed {
    pub fn embed_kind(&self) -> EmbedKind {
        EmbedKind::of(self.kind.as_deref())
    }

    /// `hasRichEmbedContent`: drawn as a card rather than bare media.
    pub fn has_rich_content(&self) -> bool {
        self.title.is_some()
            || self.description.as_deref().is_some_and(|d| !d.is_empty())
            || self.author.is_some()
            || self.footer.is_some()
            || !self.fields.is_empty()
            || (self.provider.is_some() && self.embed_kind() != EmbedKind::Gifv)
    }

    /// `isMediaOnlyEmbed`.
    pub fn is_media_only(&self) -> bool {
        !self.has_rich_content()
            && (self.image.is_some() || self.thumbnail.is_some() || self.video.is_some() || self.audio.is_some())
    }

    /// The provider is YouTube (`getUrlHostname(provider.url) === 'www.youtube.com'` or provider name `"YouTube"`).
    pub fn is_youtube(&self) -> bool {
        self.provider.as_ref().is_some_and(|p| {
            p.name.eq_ignore_ascii_case("YouTube")
                || p.url
                    .as_deref()
                    .and_then(url_hostname)
                    .is_some_and(|host| {
                        host.eq_ignore_ascii_case(YOUTUBE_PROVIDER_HOST)
                            || host.eq_ignore_ascii_case("youtube.com")
                            || host.eq_ignore_ascii_case("m.youtube.com")
                            || host.eq_ignore_ascii_case("youtu.be")
                    })
        })
    }
}

impl EmbedMedia {
    /// `IS_ANIMATED` flag.
    pub fn is_animated(&self) -> bool {
        self.flags & ATTACHMENT_FLAG_ANIMATED != 0
    }

    /// `isMediaMatureContent`: `nsfw` or the explicit flag.
    pub fn is_explicit(&self) -> bool {
        self.nsfw || self.flags & ATTACHMENT_FLAG_EXPLICIT != 0
    }

    /// A GIF by content type or file name (`isGif` in `ChannelEmbed.tsx`).
    pub fn is_gif(&self) -> bool {
        self.content_type.as_deref() == Some("image/gif") || self.url.to_ascii_lowercase().ends_with(".gif")
    }

    /// `isAnimatedEmbedMedia`: request the animated proxy variant.
    pub fn wants_animation(&self) -> bool {
        self.content_type.as_deref() == Some("image/gif") || self.is_animated()
    }
}

/// Media with everything needed to draw it (`isValidMedia`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ValidMedia<'a> {
    pub media: &'a EmbedMedia,
    pub url: &'a str,
    pub proxy_url: &'a str,
    pub width: u32,
    pub height: u32,
}

/// `isValidMedia`: url, proxy url, width and height all present.
pub fn valid_media(media: Option<&EmbedMedia>) -> Option<ValidMedia<'_>> {
    let media = media?;
    if media.url.is_empty() {
        return None;
    }
    Some(ValidMedia {
        media,
        url: &media.url,
        proxy_url: media.proxy_url.as_deref()?,
        width: media.width?,
        height: media.height?,
    })
}

/// `fitMediaWithinBounds`: scale down (never up) to fit, rounding like JS.
pub fn fit_media(width: u32, height: u32, max_width: u32, max_height: u32) -> (u32, u32) {
    let (width, height) = (width.max(1), height.max(1));
    if width == max_width && height == max_height {
        return (width, height);
    }
    let (w, h) = (f64::from(width), f64::from(height));
    let width_ratio = if w > f64::from(max_width) { f64::from(max_width) / w } else { 1. };
    let fitted_w = (w * width_ratio).round();
    let fitted_h = (h * width_ratio).round();
    let height_ratio = if fitted_h > f64::from(max_height) { f64::from(max_height) / fitted_h } else { 1. };
    (to_u32((fitted_w * height_ratio).round()), to_u32((fitted_h * height_ratio).round()))
}

fn to_u32(value: f64) -> u32 {
    // Values here are already rounded and within the input range.
    value.clamp(0., f64::from(u32::MAX)) as u32
}

/// `calculateMediaDimensions`: an embed image in 400x300 (or 400x450 for
/// tall video).
pub fn embed_media_size(width: u32, height: u32, tall: bool) -> (u32, u32) {
    let max_height = if tall { EMBED_TALL_MAX_HEIGHT } else { EMBED_MAX_HEIGHT };
    fit_media(width, height, EMBED_MAX_WIDTH, max_height)
}

/// `isUsefulVisualMediaSize` against the embed bounds.
pub fn is_useful_media_size(width: u32, height: u32) -> bool {
    if width == 0 || height == 0 {
        return false;
    }
    let (w, h) = embed_media_size(width, height, false);
    w >= MIN_USEFUL_MEDIA_SIZE && h >= MIN_USEFUL_MEDIA_SIZE
}

/// Width of the 80px-tall inline thumbnail (`InlineThumbnailRenderer`).
pub fn inline_thumbnail_width(width: u32, height: u32) -> u32 {
    let ratio = f64::from(width) / f64::from(height.max(1));
    EMBED_THUMBNAIL_SIZE.min(to_u32((f64::from(EMBED_THUMBNAIL_SIZE) * ratio).round()))
}

/// `normalizeUrl`: parsed href without one trailing slash.
pub fn normalize_url(url: &str) -> Option<String> {
    let href = Url::parse(url).ok()?.to_string();
    Some(href.strip_suffix('/').map(str::to_owned).unwrap_or(href))
}

/// `getUrlHostname`.
pub fn url_hostname(url: &str) -> Option<String> {
    Url::parse(url).ok()?.host_str().map(str::to_owned)
}

/// First index of each normalized embed URL.
fn first_indexes(embeds: &[Embed]) -> (Vec<Option<String>>, HashMap<String, usize>) {
    let normalized: Vec<Option<String>> =
        embeds.iter().map(|e| e.url.as_deref().and_then(normalize_url)).collect();
    let mut first = HashMap::new();
    for (index, url) in normalized.iter().enumerate() {
        if let Some(url) = url {
            first.entry(url.clone()).or_insert(index);
        }
    }
    (normalized, first)
}

/// `isDuplicateEmbedAtIndex`: an earlier embed has the same URL.
pub fn is_duplicate(index: usize, embeds: &[Embed]) -> bool {
    let (normalized, first) = first_indexes(embeds);
    let Some(Some(url)) = normalized.get(index) else { return false };
    first.get(url).is_some_and(|first| *first < index)
}

/// `collectGalleryImages`: for the first embed of a URL, the image (else
/// thumbnail) of every embed sharing that URL, deduped by content hash or
/// URL, at most [`MAX_GALLERY_MEDIA`].
pub fn gallery_images(index: usize, embeds: &[Embed]) -> Vec<ValidMedia<'_>> {
    let (normalized, first) = first_indexes(embeds);
    let mut images = Vec::new();
    let mut seen = HashSet::new();
    for (i, embed) in embeds.iter().enumerate() {
        let Some(Some(url)) = normalized.get(i) else { continue };
        if first.get(url) != Some(&index) {
            continue;
        }
        let Some(media) = valid_media(embed.image.as_ref()).or_else(|| valid_media(embed.thumbnail.as_ref())) else {
            continue;
        };
        if images.len() >= MAX_GALLERY_MEDIA {
            break;
        }
        let key = media.media.content_hash.clone().filter(|h| !h.is_empty()).unwrap_or_else(|| media.url.to_owned());
        if seen.insert(key) {
            images.push(media);
        }
    }
    images
}

/// `MessageFlags` after the suppress-embeds toggle.
pub fn toggled_suppress_embeds(flags: u64) -> u64 {
    flags ^ message_flags::SUPPRESS_EMBEDS
}

// ------------------------------------------------------------- spoilers

const SIGNATURE_FIELDS: [&str; 4] = ["ex", "is", "hm", "uc"];

/// Bodies of `||...||` spans, non-greedy, left to right.
fn spoiler_spans(content: &str) -> Vec<std::ops::Range<usize>> {
    let mut spans = Vec::new();
    let mut rest = 0;
    while let Some(open) = content[rest..].find("||") {
        let start = rest + open + 2;
        let Some(close) = content[start..].find("||") else { break };
        let end = start + close;
        spans.push(start..end);
        rest = end + 2;
    }
    spans
}

/// `URL_REGEX` (`/https?:\/\/[^\s<>"']+/gi`) matches in `text`.
fn find_urls(text: &str) -> Vec<&str> {
    let lower = text.to_ascii_lowercase();
    let mut urls = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let next = ["http://", "https://"].iter().filter_map(|p| lower[at..].find(p)).min();
        let Some(offset) = next else { break };
        let start = at + offset;
        let end = text[start..]
            .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\''))
            .map_or(text.len(), |e| start + e);
        urls.push(&text[start..end]);
        at = end.max(start + 1);
    }
    urls
}

fn youtube_video_id(url: &Url) -> Option<String> {
    let host = url.host_str()?;
    let path = url.path();
    let first_segment = |prefix: &str| {
        path.strip_prefix(prefix).and_then(|rest| rest.split('/').next()).filter(|id| !id.is_empty()).map(str::to_owned)
    };
    match host {
        "youtu.be" => first_segment("/"),
        "www.youtube.com" | "youtube.com" | "m.youtube.com" | "music.youtube.com" => {
            if path == "/watch" {
                url.query_pairs().find(|(k, _)| k == "v").map(|(_, v)| v.into_owned())
            } else {
                first_segment("/shorts/").or_else(|| first_segment("/v/")).or_else(|| first_segment("/embed/"))
            }
        }
        _ => None,
    }
}

/// `stripAttachmentSignature`: drops the `ex`/`is`/`hm`/`uc` query fields.
fn strip_signature(href: &str) -> String {
    let (head, fragment) = href.split_at(href.find('#').unwrap_or(href.len()));
    let (base, query) = match head.find('?') {
        Some(i) => (&head[..i], &head[i + 1..]),
        None => (head, ""),
    };
    let kept: Vec<&str> = query
        .split('&')
        .filter(|field| !field.is_empty() && *field != "=")
        .filter(|field| {
            let name = field.split('=').next().unwrap_or_default();
            !SIGNATURE_FIELDS.contains(&name)
        })
        .collect();
    if kept.is_empty() { format!("{base}{fragment}") } else { format!("{base}?{}{fragment}", kept.join("&")) }
}

/// `canonicalizeMediaUrl`: `youtube:<id>` for YouTube videos, else the URL
/// without signature fields or a trailing slash.
pub fn canonicalize_media_url(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    if let Some(id) = youtube_video_id(&parsed) {
        return Some(format!("youtube:{id}"));
    }
    let stripped = strip_signature(parsed.as_str());
    Some(stripped.strip_suffix('/').map(str::to_owned).unwrap_or(stripped))
}

/// `getEmbedSpoilerUrls`: the embed's URLs that the message content puts
/// inside `||spoilers||`. Non-rich embeds are also spoilers when the content
/// has spoilered URLs and no URL outside a spoiler.
pub fn spoiler_urls(embed: &Embed, content: &str) -> Vec<String> {
    let spans = spoiler_spans(content);
    let spoilered: BTreeSet<String> = spans
        .iter()
        .flat_map(|span| find_urls(&content[span.clone()]))
        .filter_map(canonicalize_media_url)
        .collect();
    if spoilered.is_empty() {
        return Vec::new();
    }
    let media = [&embed.image, &embed.thumbnail, &embed.video, &embed.audio];
    let candidates = [embed.url.as_deref(), embed.provider.as_ref().and_then(|p| p.url.as_deref())]
        .into_iter()
        .chain(media.iter().flat_map(|m| {
            let m = m.as_ref();
            [m.map(|m| m.url.as_str()), m.and_then(|m| m.proxy_url.as_deref())]
        }));
    let matches: Vec<String> = candidates
        .flatten()
        .filter_map(canonicalize_media_url)
        .filter(|canonical| spoilered.contains(canonical))
        .collect();
    if !matches.is_empty() || embed.embed_kind() == EmbedKind::Rich {
        return matches;
    }
    // Content with every spoiler span blanked out.
    let mut outside = String::with_capacity(content.len());
    let mut last = 0;
    for span in &spans {
        outside.push_str(&content[last..span.start - 2]);
        outside.push(' ');
        last = span.end + 2;
    }
    outside.push_str(&content[last.min(content.len())..]);
    if find_urls(&outside).is_empty() { spoilered.into_iter().collect() } else { Vec::new() }
}

// ----------------------------------------------------------- proxy URLs

/// `MediaProxyOptions`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProxyOptions {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub format: Option<&'static str>,
    /// `high`, `low` or `lossless`.
    pub quality: Option<&'static str>,
    pub animated: bool,
}

const PROXY_PARAMS: [&str; 5] = ["width", "height", "format", "quality", "animated"];

/// `URLSearchParams.set`: replaces the first `key` in place, drops the
/// others, or appends.
fn set_param(pairs: &mut Vec<(String, String)>, key: &str, value: String) {
    match pairs.iter().position(|(k, _)| k == key) {
        Some(first) => {
            pairs[first].1 = value;
            let mut index = 0;
            pairs.retain(|(k, _)| {
                let keep = k != key || index == first;
                index += 1;
                keep
            });
        }
        None => pairs.push((key.to_owned(), value)),
    }
}

fn write_pairs(url: &mut Url, pairs: &[(String, String)]) {
    if pairs.is_empty() {
        url.set_query(None);
    } else {
        url.query_pairs_mut().clear().extend_pairs(pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    }
}

fn skips_params(url: &Url) -> bool {
    matches!(url.scheme(), "data" | "blob") || url.path().to_ascii_lowercase().ends_with(".svg")
}

/// `buildMediaProxyURL`: `base` with the proxy params of `options` set
/// (`animated` removed unless asked for). SVG, `data:` and `blob:` URLs and
/// unparsable input come back unchanged.
pub fn media_proxy_url(base: &str, options: &ProxyOptions) -> String {
    let Ok(mut url) = Url::parse(base) else { return base.to_owned() };
    if skips_params(&url) {
        return base.to_owned();
    }
    let mut pairs: Vec<(String, String)> = url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
    if let Some(format) = options.format {
        set_param(&mut pairs, "format", format.to_owned());
    }
    if let Some(width) = options.width.filter(|w| *w > 0) {
        set_param(&mut pairs, "width", width.to_string());
    }
    if let Some(height) = options.height.filter(|h| *h > 0) {
        set_param(&mut pairs, "height", height.to_string());
    }
    if let Some(quality) = options.quality {
        set_param(&mut pairs, "quality", quality.to_owned());
    }
    if options.animated {
        set_param(&mut pairs, "animated", "true".to_owned());
    } else {
        pairs.retain(|(k, _)| k != "animated");
    }
    write_pairs(&mut url, &pairs);
    url.to_string()
}

/// `stripMediaProxyParams`.
pub fn strip_proxy_params(proxy_url: &str) -> String {
    let Ok(mut url) = Url::parse(proxy_url) else { return proxy_url.to_owned() };
    let pairs: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| !PROXY_PARAMS.contains(&k.as_ref()))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    write_pairs(&mut url, &pairs);
    url.to_string()
}

fn proxy_dimension(url: &Url, key: &str) -> Option<u32> {
    let raw = url.query_pairs().find(|(k, _)| k == key)?.1;
    let value: f64 = raw.parse().ok()?;
    (value.is_finite() && value > 0.).then(|| to_u32(value.ceil()))
}

/// `getOptimizedMediaURL`: a static webp at `width`x`height` device px.
pub fn static_proxy_url(proxy_url: &str, width: u32, height: u32) -> String {
    let options = ProxyOptions { width: Some(width), height: Some(height), format: Some("webp"), ..ProxyOptions::default() };
    media_proxy_url(proxy_url, &options)
}

/// `buildAnimatedImageProxyURL`: params stripped, then the size (an
/// existing one in the URL wins) and `animated=true`, no format, so the
/// proxy keeps GIFs as GIF and makes animated WebP otherwise.
pub fn animated_proxy_url(proxy_url: &str, width: u32, height: u32) -> String {
    let existing = Url::parse(proxy_url).ok().map(|url| (proxy_dimension(&url, "width"), proxy_dimension(&url, "height")));
    let (width, height) = match existing {
        Some((w, h)) if w.is_some() || h.is_some() => (w, h),
        _ => (Some(width), Some(height)),
    };
    let base = strip_proxy_params(proxy_url);
    media_proxy_url(&base, &ProxyOptions { width, height, animated: true, ..ProxyOptions::default() })
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn embed(value: Value) -> Embed {
        serde_json::from_value(value).expect("embed")
    }

    fn media(url: &str, w: u32, h: u32) -> Value {
        json!({ "url": url, "proxy_url": format!("https://media.example/external/{w}x{h}"), "width": w, "height": h })
    }

    #[test]
    fn kinds_and_null_tolerant_media() {
        assert_eq!(EmbedKind::of(Some("gifv")), EmbedKind::Gifv);
        assert_eq!(EmbedKind::of(Some("bluesky")), EmbedKind::Bluesky);
        assert_eq!(EmbedKind::of(Some("poll")), EmbedKind::Other);
        assert_eq!(EmbedKind::of(None), EmbedKind::Other);
        let e = embed(json!({
            "type": "image",
            "thumbnail": { "url": "https://x/a.gif", "proxy_url": "https://p/a.gif", "width": 10, "height": 20,
                           "content_type": "image/gif", "flags": 32, "nsfw": null, "content_hash": "h1" },
            "image": { "url": null, "proxy_url": null, "width": null, "height": null, "flags": null }
        }));
        let thumb = e.thumbnail.as_ref().expect("thumbnail");
        assert!(thumb.is_animated() && thumb.is_gif() && thumb.wants_animation() && !thumb.is_explicit());
        assert_eq!(thumb.content_hash.as_deref(), Some("h1"));
        assert!(valid_media(e.thumbnail.as_ref()).is_some());
        assert!(valid_media(e.image.as_ref()).is_none(), "missing url/size is skipped");
        let explicit: EmbedMedia = serde_json::from_value(json!({ "url": "u", "flags": 16 })).expect("media");
        assert!(explicit.is_explicit());
        let nsfw: EmbedMedia = serde_json::from_value(json!({ "url": "u", "nsfw": true })).expect("media");
        assert!(nsfw.is_explicit());
    }

    #[test]
    fn media_only_versus_rich() {
        // Tenor gifv: provider set but gifv, no text -> media only.
        let gifv = embed(json!({ "type": "gifv", "url": "https://tenor.com/view/x", "provider": { "name": "Tenor" },
            "video": media("https://media.tenor.com/x.mp4", 498, 280), "thumbnail": media("https://media.tenor.com/x.png", 498, 280) }));
        assert!(!gifv.has_rich_content() && gifv.is_media_only());
        // YouTube article: title + provider -> card.
        let yt = embed(json!({ "type": "article", "url": "https://www.youtube.com/watch?v=abc", "title": "T",
            "provider": { "name": "YouTube", "url": "https://www.youtube.com" }, "thumbnail": media("https://i.ytimg.com/a.jpg", 1280, 720) }));
        assert!(yt.has_rich_content() && !yt.is_media_only() && yt.is_youtube());
        // An empty title still counts (`title != null`); empty description does not.
        assert!(embed(json!({ "type": "rich", "title": "" })).has_rich_content());
        assert!(!embed(json!({ "type": "rich", "description": "" })).has_rich_content());
        // Link with only a thumbnail is media only; with no media it is neither.
        assert!(embed(json!({ "type": "link", "thumbnail": media("https://x/t.png", 10, 10) })).is_media_only());
        assert!(!embed(json!({ "type": "link" })).is_media_only());
    }

    #[test]
    fn fit_matches_fit_media_within_bounds() {
        assert_eq!(embed_media_size(1280, 720, false), (400, 225));
        assert_eq!(embed_media_size(720, 1280, false), (169, 300));
        assert_eq!(embed_media_size(720, 1280, true), (253, 450));
        assert_eq!(embed_media_size(200, 100, false), (200, 100), "never upscaled");
        assert_eq!(embed_media_size(400, 300, false), (400, 300));
        assert_eq!(embed_media_size(0, 0, false), (1, 1));
        assert!(is_useful_media_size(1280, 720));
        assert!(!is_useful_media_size(4000, 100), "fits to 400x10");
        assert!(!is_useful_media_size(30, 30));
        assert!(!is_useful_media_size(0, 50));
        assert_eq!(inline_thumbnail_width(1280, 720), 80);
        assert_eq!(inline_thumbnail_width(400, 800), 40);
    }

    #[test]
    fn proxy_urls() {
        let base = "https://media.example/external/abc/x.gif";
        assert_eq!(static_proxy_url(base, 600, 338), format!("{base}?format=webp&width=600&height=338"));
        assert_eq!(animated_proxy_url(base, 600, 338), format!("{base}?width=600&height=338&animated=true"));
        // Existing params are replaced in place; `animated` is removed for stills.
        let with = format!("{base}?width=10&animated=true&ex=1");
        assert_eq!(static_proxy_url(&with, 600, 338), format!("{base}?width=600&ex=1&format=webp&height=338"));
        // The animated variant keeps a size already in the URL, drops format.
        let sized = format!("{base}?format=webp&width=320&height=180");
        assert_eq!(animated_proxy_url(&sized, 600, 338), format!("{base}?width=320&height=180&animated=true"));
        assert_eq!(strip_proxy_params(&format!("{base}?quality=low&format=webp&ex=1")), format!("{base}?ex=1"));
        assert_eq!(strip_proxy_params(&format!("{base}?quality=low")), base);
        // SVG, data: and unparsable URLs are left alone.
        assert_eq!(static_proxy_url("https://m/x.SVG", 10, 10), "https://m/x.SVG");
        assert_eq!(static_proxy_url("data:image/png;base64,AA", 10, 10), "data:image/png;base64,AA");
        assert_eq!(static_proxy_url("not a url", 10, 10), "not a url");
        let quality = ProxyOptions { quality: Some("lossless"), ..ProxyOptions::default() };
        assert_eq!(media_proxy_url(base, &quality), format!("{base}?quality=lossless"));
    }

    #[test]
    fn duplicates_and_gallery() {
        let embeds: Vec<Embed> = serde_json::from_value(json!([
            { "type": "article", "url": "https://www.youtube.com/watch?v=abc", "title": "T",
              "thumbnail": media("https://i.ytimg.com/a.jpg", 1280, 720) },
            { "type": "rich", "url": "https://www.youtube.com/watch?v=abc", "image": media("https://i.ytimg.com/a.jpg", 1280, 720) },
            { "type": "rich", "url": "https://www.youtube.com/watch?v=abc/", "image": media("https://i.ytimg.com/b.jpg", 640, 360) },
            { "type": "link", "url": "https://other.example/" },
            { "type": "image", "image": media("https://x/no-url.png", 5, 5) }
        ]))
        .expect("embeds");
        assert!(!is_duplicate(0, &embeds));
        assert!(is_duplicate(1, &embeds), "same URL as the article");
        assert!(!is_duplicate(3, &embeds));
        assert!(!is_duplicate(4, &embeds), "no URL is never a duplicate");
        assert!(!is_duplicate(9, &embeds));
        let gallery: Vec<&str> = gallery_images(0, &embeds).iter().map(|m| m.url).collect();
        // a.jpg twice (deduped by URL); the third embed's URL only differs by
        // a trailing slash, so its image joins the gallery.
        assert!(is_duplicate(2, &embeds));
        assert_eq!(gallery, ["https://i.ytimg.com/a.jpg", "https://i.ytimg.com/b.jpg"]);
        assert!(gallery_images(1, &embeds).is_empty(), "only the first embed of a URL collects");
        let many: Vec<Embed> = (0..12)
            .map(|i| embed(json!({ "url": "https://g.example/p", "image": media(&format!("https://g/{i}.png"), 10, 10) })))
            .collect();
        assert_eq!(gallery_images(0, &many).len(), MAX_GALLERY_MEDIA);
        assert!(is_duplicate(11, &many));
        // A trailing slash on the path is ignored.
        let slash: Vec<Embed> =
            vec![embed(json!({ "url": "https://s.example/p/" })), embed(json!({ "url": "https://s.example/p" }))];
        assert!(is_duplicate(1, &slash));
    }

    #[test]
    fn spoiler_urls_follow_content() {
        let image = embed(json!({ "type": "image", "url": "https://x.example/a.png?ex=1&hm=2",
            "thumbnail": { "url": "https://x.example/a.png", "proxy_url": "https://p.example/a.png" } }));
        assert_eq!(spoiler_urls(&image, "look ||https://x.example/a.png||"), ["https://x.example/a.png"; 2]);
        assert!(spoiler_urls(&image, "https://x.example/a.png").is_empty());
        // Non-rich: spoilered URL text that matches nothing still hides the
        // embed when no URL is outside a spoiler...
        let other = embed(json!({ "type": "link", "url": "https://y.example/" }));
        assert_eq!(spoiler_urls(&other, "||https://z.example/||"), ["https://z.example"]);
        // ...but not when another URL is visible, nor for rich embeds.
        assert!(spoiler_urls(&other, "||https://z.example/|| https://y.example/").is_empty());
        let rich = embed(json!({ "type": "rich", "url": "https://y.example/" }));
        assert!(spoiler_urls(&rich, "||https://z.example/||").is_empty());
        // YouTube URLs match by video id.
        let yt = embed(json!({ "type": "video", "url": "https://www.youtube.com/watch?v=abc" }));
        assert_eq!(spoiler_urls(&yt, "||https://youtu.be/abc||"), ["youtube:abc"]);
        assert_eq!(canonicalize_media_url("https://www.youtube.com/shorts/xyz/"), Some("youtube:xyz".to_owned()));
        assert_eq!(canonicalize_media_url("HTTPS://A.example/p/?is=3&k=v#f"), Some("https://a.example/p/?k=v#f".to_owned()));
        assert_eq!(canonicalize_media_url("nope"), None);
        assert!(spoiler_urls(&image, "|| ||").is_empty());
    }

    #[test]
    fn suppress_toggle() {
        assert_eq!(toggled_suppress_embeds(0), message_flags::SUPPRESS_EMBEDS);
        assert_eq!(toggled_suppress_embeds(message_flags::SUPPRESS_EMBEDS | 1), 1);
    }
}

