//! Right-hand message search panel (Ctrl+F / header magnifying glass):
//! parses inline filters (`has:*`, `is:pinned`), queries `POST /search/messages`,
//! paginates results and jumps into historical message windows.

use std::collections::BTreeSet;
use std::sync::Arc;

use chrono::Local;
use fluxer_client_core::rest::{
    MessageContentType, MessageSearchOutcome, MessageSearchPage, MessageSearchRequest, MessageSearchScope,
    MessageSearchSortBy, MessageSearchSortOrder,
};
use fluxer_client_core::state::{
    ATTACHMENT_FLAG_ANIMATED, Channel, ChannelKind, ClientState, Message, Snowflake,
};
use fluxer_client_markdown::{Document, LowerOptions, MarkdownContext, MessageMarkdown, TimeFormat, parse};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::{Selectable as _, Sizable as _};
use gpui_kit::{
    AnyElement, AppContext as _, Context, Entity, EntityInputHandler as _, Focusable as _, FontWeight, Hsla,
    ObjectFit, ScrollHandle, SharedString, StyledImage as _, Subscription, Window, div, img, prelude::*, px, rgb,
};

use crate::client_view::ClientView;
use crate::icons::AppIcon;
use crate::format::{default_avatar_index, fit_image, format_message_time, initials, message_time};
use crate::theme::Theme;
use crate::ui::avatar::{AvatarSpec, render_avatar};
use crate::ui::embed::{self, CardMedia, EmbedBody, EmbedView, MediaView};
use crate::ui::l10n;
use crate::ui::media_viewer::{self, MediaViewerItem};
use crate::ui::message_row::{markdown_style, spoiler_state};
use crate::ui::reply::{excerpt, member_name};
use crate::view_model::{ImagePreview, StateMentions, avatar_url, channel_name, opens_message_pane, render_embeds};

pub const CONTEXT: &str = "SearchPanel";
pub const PANEL_WIDTH: f32 = 340.;
const MAX_HAS_TOKENS: usize = 20;
const MAX_SEARCH_PAGE: u32 = 400;
const EXCERPT_LIMIT: usize = 140;
const SEARCH_AVATAR_SIZE: f32 = 28.;
const SEARCH_CARD_MAX_WIDTH: f32 = 288.;
const SEARCH_EMBED_MEDIA_MAX_WIDTH: f32 = 256.;
const SEARCH_EMBED_MEDIA_MAX_HEIGHT: f32 = 180.;
const SEARCH_IMAGE_MAX_WIDTH: f32 = 256.;
const SEARCH_IMAGE_MAX_HEIGHT: f32 = 180.;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchScopeChoice {
    CurrentChannel,
    CurrentGuild,
    All,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedQuery {
    pub content: Option<String>,
    pub has: Vec<MessageContentType>,
    pub pinned: Option<bool>,
}

fn parse_has_kind(token: &str) -> Option<MessageContentType> {
    match token.to_ascii_lowercase().as_str() {
        "image" => Some(MessageContentType::Image),
        "sound" => Some(MessageContentType::Sound),
        "video" => Some(MessageContentType::Video),
        "file" => Some(MessageContentType::File),
        "sticker" => Some(MessageContentType::Sticker),
        "embed" => Some(MessageContentType::Embed),
        "link" => Some(MessageContentType::Link),
        "poll" => Some(MessageContentType::Poll),
        "snapshot" => Some(MessageContentType::Snapshot),
        _ => None,
    }
}

/// Splits user search input into structured `has:` / `is:pinned` / `pinned:`
/// filters and remaining plain text content.
pub fn parse_search_query(raw: &str) -> ParsedQuery {
    let mut terms = Vec::new();
    let mut has = Vec::new();
    let mut pinned = None;
    for token in raw.split_whitespace() {
        let lower = token.to_ascii_lowercase();
        if let Some(kind_str) = lower.strip_prefix("has:")
            && let Some(kind) = parse_has_kind(kind_str)
        {
            if !has.contains(&kind) && has.len() < MAX_HAS_TOKENS {
                has.push(kind);
            }
            continue;
        }
        match lower.as_str() {
            "is:pinned" | "pinned:true" => {
                pinned = Some(true);
            }
            "pinned:false" => {
                pinned = Some(false);
            }
            _ => terms.push(token),
        }
    }
    let content = (!terms.is_empty()).then(|| terms.join(" "));
    ParsedQuery { content, has, pinned }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub message_id: Snowflake,
    pub channel_id: Snowflake,
    pub guild_id: Option<Snowflake>,
    pub user_id: Snowflake,
    pub channel_label: SharedString,
    pub author: SharedString,
    pub initials: SharedString,
    pub avatar_url: Option<SharedString>,
    pub default_avatar: usize,
    pub time: SharedString,
    pub excerpt: SharedString,
    pub document: Option<Arc<Document>>,
    pub images: Vec<ImagePreview>,
    pub embeds: Vec<EmbedView>,
    pub has_attachments: bool,
    pub jumpable: bool,
}

pub struct SearchPanelState {
    pub input: Entity<InputState>,
    pub scope: SearchScopeChoice,
    pub sort_by: MessageSearchSortBy,
    pub loading: bool,
    pub indexing: bool,
    pub has_more: bool,
    pub error: Option<SharedString>,
    pub hits: Vec<SearchHit>,
    pub total: u64,
    pub page: u32,
    pub hits_per_page: u32,
    pub cursor: Option<Vec<String>>,
    pub last_request: Option<MessageSearchRequest>,
    pub request_seq: u64,
    pub has_searched: bool,
    pub scroll: ScrollHandle,
    pub _subscription: Subscription,
}

fn channel_label_and_guild(
    state: &ClientState,
    extra_channels: &[Channel],
    threads: &[Channel],
    channel_id: Snowflake,
    fallback_guild: Option<Snowflake>,
) -> (SharedString, Option<Snowflake>, bool) {
    let local = state.channels.get(&channel_id);
    let jumpable = local.is_some_and(|ch| opens_message_pane(ch.kind));
    let channel = local
        .or_else(|| extra_channels.iter().find(|c| c.id == channel_id))
        .or_else(|| threads.iter().find(|t| t.id == channel_id));
    match channel {
        Some(ch) => {
            let name = channel_name(ch);
            let label = match ch.kind {
                ChannelKind::Dm | ChannelKind::GroupDm | ChannelKind::DmPersonalNotes => name,
                _ => format!("#{name}"),
            };
            (label.into(), ch.guild_id.or(fallback_guild), jumpable)
        }
        None => (format!("#{channel_id}").into(), fallback_guild, false),
    }
}

fn clamp_media_size(width: f32, height: f32, max_w: f32, max_h: f32) -> (f32, f32) {
    if !width.is_finite() || !height.is_finite() || width <= 0. || height <= 0. {
        return (max_w, max_h);
    }
    let scale = (max_w / width).min(max_h / height).min(1.);
    ((width * scale).round().max(1.), (height * scale).round().max(1.))
}

fn clamp_media_view(media: &mut MediaView, max_w: f32, max_h: f32) {
    if media.kind == embed::MediaKind::Audio {
        media.width = media.width.min(max_w);
        return;
    }
    let (w, h) = clamp_media_size(media.width, media.height, max_w, max_h);
    media.width = w;
    media.height = h;
}

fn clamp_embed_for_search(embed: &mut EmbedView) {
    match &mut embed.body {
        EmbedBody::Media(media) => {
            clamp_media_view(media, SEARCH_EMBED_MEDIA_MAX_WIDTH, SEARCH_EMBED_MEDIA_MAX_HEIGHT);
        }
        EmbedBody::Card(card) => {
            card.max_width = card.max_width.min(SEARCH_CARD_MAX_WIDTH);
            let inner_max_w = (SEARCH_CARD_MAX_WIDTH - 32.).max(160.);
            match &mut card.media {
                CardMedia::None => {}
                CardMedia::Single(media) => {
                    clamp_media_view(media, inner_max_w, SEARCH_EMBED_MEDIA_MAX_HEIGHT);
                }
                CardMedia::Gallery(items) => {
                    let tile_max = (inner_max_w / 2. - 4.).max(80.);
                    for item in items {
                        clamp_media_view(item, tile_max, tile_max);
                    }
                }
            }
            if let Some(thumb) = &mut card.thumbnail {
                clamp_media_view(thumb, 64., 64.);
            }
        }
    }
}

fn build_search_images(msg: &Message) -> Vec<ImagePreview> {
    let mut images = Vec::new();
    for a in &msg.attachments {
        let is_image = a.content_type.as_deref().is_some_and(|t| t.starts_with("image/"));
        if is_image && let Some(url) = &a.url {
            let (fit_w, fit_h) = fit_image(a.width, a.height);
            let (width, height) = clamp_media_size(fit_w, fit_h, SEARCH_IMAGE_MAX_WIDTH, SEARCH_IMAGE_MAX_HEIGHT);
            let animated =
                a.content_type.as_deref() == Some("image/gif") || a.flags & ATTACHMENT_FLAG_ANIMATED != 0;
            images.push(ImagePreview {
                url: url.clone().into(),
                proxy_url: a.proxy_url.clone().map(Into::into),
                animated,
                width,
                height,
            });
        }
    }
    images
}

fn build_hits(state: &ClientState, media: Option<&str>, page: &MessageSearchPage) -> Vec<SearchHit> {
    let now = Local::now();
    let time_fmt = TimeFormat { now_ms: now.timestamp_millis(), ..TimeFormat::local(false) };
    let show_embeds_setting = render_embeds(state);
    page.messages
        .iter()
        .map(|msg| {
            let (channel_label, guild_id, jumpable) =
                channel_label_and_guild(state, &page.channels, &page.threads, msg.channel_id, msg.guild_id);
            let author_str = member_name(state, guild_id, &msg.author);
            let author_initials = initials(&author_str);
            let user_id = msg.author.id;
            let avatar = media.and_then(|m| avatar_url(m, &msg.author)).map(Into::into);
            let default_avatar = default_avatar_index(user_id);
            let time = message_time(msg, &Local).map(|at| format_message_time(&at, &now)).unwrap_or_default().into();
            let images = build_search_images(msg);
            let resolver = StateMentions { state, guild_id, message: msg };
            let mut document = (!msg.content.trim().is_empty()).then(|| {
                Arc::new(parse(
                    &msg.content,
                    &LowerOptions {
                        context: MarkdownContext::StandardWithoutJumbo,
                        resolver: &resolver,
                        media_endpoint: media,
                        time: &time_fmt,
                        disable_animated_emoji: false,
                    },
                ))
            });
            let show_embeds = show_embeds_setting && !msg.suppresses_embeds();
            let mut embeds = if show_embeds {
                let footer_time = |iso: &str| {
                    let at = chrono::DateTime::parse_from_rfc3339(iso).ok()?.with_timezone(&Local);
                    Some(format_message_time(&at, &now))
                };
                embed::embed_views(
                    msg,
                    &mut |text, kind| {
                        Arc::new(parse(
                            text,
                            &LowerOptions {
                                context: kind.context(),
                                resolver: &resolver,
                                media_endpoint: media,
                                time: &time_fmt,
                                disable_animated_emoji: false,
                            },
                        ))
                    },
                    &footer_time,
                )
            } else {
                Vec::new()
            };
            for ev in &mut embeds {
                clamp_embed_for_search(ev);
            }
            if show_embeds && !embeds.is_empty() && document.as_deref().is_some_and(|d| embed::hides_bare_url(msg, d)) {
                document = None;
            }
            let has_attachments = !msg.attachments.is_empty() || !msg.embeds.is_empty();
            let body_excerpt = if msg.content.trim().is_empty() && has_attachments {
                l10n::tr("Attachment")
            } else {
                excerpt(&msg.content, EXCERPT_LIMIT)
            };
            SearchHit {
                message_id: msg.id,
                channel_id: msg.channel_id,
                guild_id,
                user_id,
                channel_label,
                author: author_str.into(),
                initials: author_initials.into(),
                avatar_url: avatar,
                default_avatar,
                time,
                excerpt: body_excerpt.into(),
                document,
                images,
                embeds,
                has_attachments,
                jumpable,
            }
        })
        .collect()
}

impl ClientView {
    pub(crate) fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = &self.search {
            let focus = panel.input.read(cx).focus_handle(cx);
            window.focus(&focus, cx);
            cx.notify();
            return;
        }
        let default_scope = if self.selected_channel.is_some() {
            SearchScopeChoice::CurrentChannel
        } else if self.current_guild_id().is_some() {
            SearchScopeChoice::CurrentGuild
        } else {
            SearchScopeChoice::All
        };
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(l10n::tr("Search messages")));
        let subscription =
            cx.subscribe_in(&input, window, |this: &mut Self, input_entity, ev: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { shift: false, .. } = ev {
                    if input_entity.update(cx, |s, cx| s.marked_text_range(window, cx)).is_some() {
                        return;
                    }
                    this.search_submit(cx);
                }
            });
        let focus = input.read(cx).focus_handle(cx);
        let request_seq = self.search_seq;
        self.search = Some(SearchPanelState {
            input,
            scope: default_scope,
            sort_by: MessageSearchSortBy::Timestamp,
            loading: false,
            indexing: false,
            has_more: false,
            error: None,
            hits: Vec::new(),
            total: 0,
            page: 1,
            hits_per_page: 25,
            cursor: None,
            last_request: None,
            request_seq,
            has_searched: false,
            scroll: ScrollHandle::new(),
            _subscription: subscription,
        });
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(crate) fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search.is_some() {
            self.close_search(window, cx);
        } else {
            self.open_search(window, cx);
        }
    }

    pub(crate) fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search.take().is_some() {
            self.focus_composer(window, cx);
            cx.notify();
        }
    }

    pub(crate) fn search_set_scope(&mut self, scope: SearchScopeChoice, cx: &mut Context<Self>) {
        let should_rerun = {
            let Some(panel) = &mut self.search else { return };
            if panel.scope == scope {
                return;
            }
            panel.scope = scope;
            panel.has_searched || !panel.input.read(cx).value().trim().is_empty()
        };
        if should_rerun {
            self.search_submit(cx);
        } else {
            cx.notify();
        }
    }

    pub(crate) fn search_set_sort(&mut self, sort_by: MessageSearchSortBy, cx: &mut Context<Self>) {
        let should_rerun = {
            let Some(panel) = &mut self.search else { return };
            if panel.sort_by == sort_by {
                return;
            }
            panel.sort_by = sort_by;
            panel.has_searched || !panel.input.read(cx).value().trim().is_empty()
        };
        if should_rerun {
            self.search_submit(cx);
        } else {
            cx.notify();
        }
    }

    pub(crate) fn search_submit(&mut self, cx: &mut Context<Self>) {
        let raw = {
            let Some(panel) = &self.search else { return };
            panel.input.read(cx).value().to_string()
        };
        let parsed = parse_search_query(&raw);
        if parsed.content.is_none() && parsed.has.is_empty() && parsed.pinned.is_none() {
            let seq = self.next_search_seq();
            if let Some(panel) = &mut self.search {
                panel.request_seq = seq;
                panel.loading = false;
                panel.indexing = false;
                panel.has_more = false;
                panel.error = None;
                panel.has_searched = false;
                panel.hits.clear();
                panel.total = 0;
                panel.page = 1;
                panel.cursor = None;
                panel.last_request = None;
                panel.scroll = ScrollHandle::new();
            }
            cx.notify();
            return;
        }

        let context_channel_id = self.selected_channel;
        let context_guild_id = self.current_guild_id();
        let (scope, sort_by) = {
            let Some(panel) = &self.search else { return };
            (panel.scope, panel.sort_by)
        };
        let (api_scope, channel_id) = match scope {
            SearchScopeChoice::CurrentChannel => match context_channel_id {
                Some(ch) => (MessageSearchScope::Current, Some(vec![ch.to_string()])),
                None => (MessageSearchScope::All, None),
            },
            SearchScopeChoice::CurrentGuild => {
                if context_guild_id.is_some() {
                    (MessageSearchScope::Current, None)
                } else {
                    (MessageSearchScope::All, None)
                }
            }
            SearchScopeChoice::All => (MessageSearchScope::All, None),
        };
        let request = MessageSearchRequest {
            hits_per_page: Some(25),
            page: Some(1),
            cursor: None,
            content: parsed.content,
            channel_id,
            channel_ids: None,
            author_id: None,
            pinned: parsed.pinned,
            has: (!parsed.has.is_empty()).then_some(parsed.has),
            sort_by: Some(sort_by),
            sort_order: Some(MessageSearchSortOrder::Desc),
            include_nsfw: None,
            scope: Some(api_scope),
            context_channel_id: context_channel_id.map(|id| id.to_string()),
            context_guild_id: context_guild_id.map(|id| id.to_string()),
        };

        let seq = self.next_search_seq();
        if let Some(panel) = &mut self.search {
            panel.loading = true;
            panel.indexing = false;
            panel.has_more = false;
            panel.error = None;
            panel.has_searched = true;
            panel.hits.clear();
            panel.total = 0;
            panel.page = 1;
            panel.cursor = None;
            panel.last_request = Some(request.clone());
            panel.request_seq = seq;
            panel.scroll = ScrollHandle::new();
        } else {
            return;
        }
        self.backend.search_messages(seq, request);
        cx.notify();
    }

    pub(crate) fn search_load_more(&mut self, cx: &mut Context<Self>) {
        let Some(request) = self.search.as_ref().and_then(|panel| {
            if panel.loading || panel.indexing || !panel.has_more {
                return None;
            }
            let next_page = panel.page.saturating_add(1);
            let page_u16 = u16::try_from(next_page).ok().filter(|p| u32::from(*p) <= MAX_SEARCH_PAGE)?;
            let mut req = panel.last_request.clone()?;
            req.page = Some(page_u16);
            req.cursor = panel.cursor.clone();
            Some(req)
        }) else {
            return;
        };
        let seq = self.next_search_seq();
        if let Some(panel) = &mut self.search {
            panel.loading = true;
            panel.error = None;
            panel.request_seq = seq;
        } else {
            return;
        }
        self.backend.search_messages(seq, request);
        cx.notify();
    }

    pub(crate) fn search_loaded(
        &mut self,
        seq: u64,
        result: Result<MessageSearchOutcome, String>,
        cx: &mut Context<Self>,
    ) {
        let matches_seq = self.search.as_ref().is_some_and(|panel| panel.request_seq == seq);
        if !matches_seq {
            return;
        }
        let next_hits = match &result {
            Ok(MessageSearchOutcome::Results(page)) => {
                let state = self.backend.state();
                Some(build_hits(&state, self.backend.media(), page))
            }
            _ => None,
        };
        let Some(panel) = &mut self.search else { return };
        panel.loading = false;
        match result {
            Ok(MessageSearchOutcome::Indexing) => {
                panel.indexing = true;
                panel.has_more = false;
                panel.error = None;
            }
            Ok(MessageSearchOutcome::Results(page)) => {
                panel.indexing = false;
                panel.error = None;
                panel.total = page.total;
                panel.page = page.page;
                panel.hits_per_page = page.hits_per_page;
                panel.cursor = page.cursor.clone();
                let hits = next_hits.unwrap_or_default();
                let appended = if page.page > 1 {
                    let mut existing: BTreeSet<Snowflake> = panel.hits.iter().map(|h| h.message_id).collect();
                    let mut added = 0usize;
                    for hit in hits {
                        if existing.insert(hit.message_id) {
                            panel.hits.push(hit);
                            added += 1;
                        }
                    }
                    added
                } else {
                    let count = hits.len();
                    panel.hits = hits;
                    count
                };
                panel.has_more = !page.messages.is_empty()
                    && (page.page <= 1 || appended > 0)
                    && (panel.hits.len() as u64) < page.total
                    && page.page < MAX_SEARCH_PAGE
                    && (page.cursor.is_some() || (page.messages.len() as u32) >= page.hits_per_page);
            }
            Err(err) => {
                panel.indexing = false;
                panel.error = Some(err.into());
            }
        }
        cx.notify();
    }
}

pub fn render(view: &ClientView, theme: &Theme, window: &Window, cx: &mut Context<ClientView>) -> AnyElement {
    let Some(panel) = &view.search else {
        return div().into_any_element();
    };
    let in_guild = view.current_guild_id().is_some();
    let has_channel = view.selected_channel.is_some();
    let can_load_more = !panel.loading && !panel.indexing && panel.has_more;
    let scale = window.scale_factor();
    let weak_view = cx.weak_entity();

    let mut scope_row = div().flex().flex_row().flex_wrap().gap_1().when(has_channel, |d| {
        d.child(
            Button::new("search-scope-channel")
                .ghost()
                .small()
                .selected(panel.scope == SearchScopeChoice::CurrentChannel)
                .label(l10n::tr("Channel"))
                .on_click(cx.listener(|v, _, _, cx| v.search_set_scope(SearchScopeChoice::CurrentChannel, cx))),
        )
    });
    if in_guild {
        scope_row = scope_row.child(
            Button::new("search-scope-guild")
                .ghost()
                .small()
                .selected(panel.scope == SearchScopeChoice::CurrentGuild)
                .label(l10n::tr("Community"))
                .on_click(cx.listener(|v, _, _, cx| v.search_set_scope(SearchScopeChoice::CurrentGuild, cx))),
        );
    }
    scope_row = scope_row.child(
        Button::new("search-scope-all")
            .ghost()
            .small()
            .selected(panel.scope == SearchScopeChoice::All)
            .label(l10n::tr("All"))
            .on_click(cx.listener(|v, _, _, cx| v.search_set_scope(SearchScopeChoice::All, cx))),
    );

    let sort_row = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .child(
            Button::new("search-sort-newest")
                .ghost()
                .small()
                .selected(panel.sort_by == MessageSearchSortBy::Timestamp)
                .label(l10n::tr("Newest"))
                .on_click(cx.listener(|v, _, _, cx| v.search_set_sort(MessageSearchSortBy::Timestamp, cx))),
        )
        .child(
            Button::new("search-sort-relevant")
                .ghost()
                .small()
                .selected(panel.sort_by == MessageSearchSortBy::Relevance)
                .label(l10n::tr("Relevance"))
                .on_click(cx.listener(|v, _, _, cx| v.search_set_sort(MessageSearchSortBy::Relevance, cx))),
        );

    let mut body = div()
        .id("search-results")
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&panel.scroll)
        .p_3()
        .gap_2();

    if let Some(err) = panel.error.clone() {
        body = body.child(
            div()
                .debug_selector(|| "search-error".to_owned())
                .p_2()
                .rounded(px(6.))
                .border_1()
                .border_color(theme.danger)
                .text_xs()
                .text_color(theme.danger)
                .child(err),
        );
    }

    if panel.indexing {
        body = body.child(
            div()
                .p_3()
                .rounded(px(6.))
                .border_1()
                .border_color(theme.border)
                .bg(theme.background)
                .flex()
                .flex_col()
                .gap_2()
                .child(div().text_xs().text_color(theme.muted_foreground).child(l10n::tr("Indexing")))
                .child(
                    Button::new("search-retry")
                        .ghost()
                        .small()
                        .label(l10n::tr("Retry"))
                        .on_click(cx.listener(|v, _, _, cx| v.search_submit(cx))),
                ),
        );
    } else if panel.loading && panel.hits.is_empty() {
        body = body.child(div().text_xs().text_color(theme.muted_foreground).child(l10n::tr("Loading…")));
    } else if panel.has_searched && panel.hits.is_empty() && panel.error.is_none() {
        body = body.child(div().text_xs().text_color(theme.muted_foreground).child(l10n::tr("No results")));
    } else if !panel.hits.is_empty() {
        let total_str = panel.total.to_string();
        let total_label =
            l10n::tr_args("{total, plural, one {# result} other {# results}}", &[("total", &total_str)]);
        body = body.child(
            div()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.muted_foreground)
                .child(total_label),
        );
        for hit in &panel.hits {
            let guild_id = hit.guild_id;
            let channel_id = hit.channel_id;
            let message_id = hit.message_id;
            let jumpable = hit.jumpable;

            let header_row = div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.mention_fg)
                        .child(hit.channel_label.clone()),
                )
                .when(jumpable, |d| {
                    d.child(
                        Button::new(("search-jump", message_id.0))
                            .ghost()
                            .small()
                            .label(l10n::tr("Jump"))
                            .on_click(cx.listener(move |v, _, _, cx| {
                                v.jump_to_channel_message(guild_id, channel_id, message_id, cx);
                            })),
                    )
                });

            let avatar_el = div()
                .debug_selector(|| "search-hit-avatar".to_owned())
                .flex_none()
                .child(render_avatar(
                    AvatarSpec {
                        user_id: hit.user_id,
                        avatar_url: hit.avatar_url.clone(),
                        display_name: hit.author.clone(),
                        size: SEARCH_AVATAR_SIZE,
                        status: None,
                        status_border_bg: None,
                    },
                    theme,
                ));

            let mut text_col = div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_baseline()
                        .gap_2()
                        .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).child(hit.author.clone()))
                        .child(div().text_xs().text_color(theme.muted_foreground).child(hit.time.clone())),
                );

            if let Some(doc) = &hit.document {
                let view_link = weak_view.clone();
                let mut md = MessageMarkdown::new(("search-msg", message_id.0), doc.clone())
                    .style(markdown_style(theme))
                    .on_link_click(move |url, window, cx| {
                        let _ = view_link.update(cx, |v, cx| v.open_external_url(url, window, cx));
                    });
                if doc.spoiler_count > 0 {
                    md = md.spoiler_state(spoiler_state(message_id, cx));
                }
                text_col = text_col.child(
                    div()
                        .debug_selector(|| "search-hit-markdown".to_owned())
                        .w_full()
                        .min_w_0()
                        .text_xs()
                        .child(md),
                );
            } else if hit.images.is_empty() && hit.embeds.is_empty() && !hit.excerpt.is_empty() {
                text_col = text_col.child(div().text_xs().text_color(theme.foreground).child(hit.excerpt.clone()));
            }

            let mut card_body = div()
                .id(("search-hit-body", message_id.0))
                .flex()
                .flex_row()
                .items_start()
                .gap_2()
                .child(avatar_el)
                .child(text_col);

            if jumpable {
                card_body = card_body.cursor_pointer().on_click(cx.listener(move |v, _, _, cx| {
                    v.jump_to_channel_message(guild_id, channel_id, message_id, cx);
                }));
            }

            let mut card = div()
                .id(("search-hit", message_id.0))
                .debug_selector(|| "search-hit".to_owned())
                .p_2()
                .rounded(px(6.))
                .border_1()
                .border_color(theme.border)
                .bg(theme.background)
                .flex()
                .flex_col()
                .gap_1()
                .child(header_row)
                .child(card_body);

            if !hit.images.is_empty() {
                let cache = embed::image_cache(cx);
                let placeholder = theme.image_placeholder;
                let muted_fg = theme.muted_foreground;
                let viewer_items: Arc<Vec<MediaViewerItem>> = Arc::new(
                    hit.images.iter().map(|image| media_viewer::from_attachment(image, scale)).collect(),
                );
                let mut images_grid = div().flex().flex_row().flex_wrap().gap_1().mt_1();
                for (ix, image) in hit.images.iter().enumerate() {
                    let url = image.url.clone();
                    let view_img = weak_view.clone();
                    let items_for_click = viewer_items.clone();
                    let thumb = img(image.source(scale))
                        .image_cache(&cache)
                        .id(SharedString::from(format!("search-attachment-{}-{ix}", message_id.0)))
                        .size_full()
                        .object_fit(ObjectFit::Cover)
                        .with_loading(move || div().size_full().bg(placeholder).into_any_element())
                        .with_fallback(move || {
                            div().text_xs().text_color(muted_fg).child(url.clone()).into_any_element()
                        });
                    let mut badge_bg: Hsla = rgb(0x000000).into();
                    badge_bg.a = 0.65;
                    images_grid = images_grid.child(
                        div()
                            .id(SharedString::from(format!("search-attachment-open-{}-{ix}", message_id.0)))
                            .debug_selector(|| "search-hit-image".to_owned())
                            .relative()
                            .w(px(image.width))
                            .h(px(image.height))
                            .flex_none()
                            .rounded(px(4.))
                            .overflow_hidden()
                            .bg(placeholder)
                            .cursor_pointer()
                            .on_click(move |_, window, cx| {
                                let items = (*items_for_click).clone();
                                let _ = view_img.update(cx, |v, cx| v.open_media_viewer(items, ix, window, cx));
                            })
                            .child(thumb)
                            .when(image.animated, |d| {
                                d.child(
                                    div()
                                        .debug_selector(|| "search-hit-gif-badge".to_owned())
                                        .absolute()
                                        .top_1()
                                        .left_1()
                                        .px_1()
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
                card = card.child(images_grid);
            }

            if !hit.embeds.is_empty() {
                card = card.child(
                    div()
                        .id(("search-hit-embeds", message_id.0))
                        .debug_selector(|| "search-hit-embeds".to_owned())
                        .min_w_0()
                        .overflow_hidden()
                        .child(embed::render(message_id, &hit.embeds, theme, scale, &weak_view, cx)),
                );
            }

            body = body.child(card);
        }
        if can_load_more {
            body = body.child(
                div().debug_selector(|| "search-load-more".to_owned()).child(
                    Button::new("search-load-more")
                        .ghost()
                        .small()
                        .label(l10n::tr("Load more"))
                        .on_click(cx.listener(|v, _, _, cx| v.search_load_more(cx))),
                ),
            );
        }
    } else if panel.error.is_none() {
        body = body.child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child("has:image · has:file · has:link · is:pinned"),
        );
    }

    div()
        .id("search-panel")
        .debug_selector(|| "search-panel".to_owned())
        .key_context(CONTEXT)
        .capture_action(cx.listener(|v, _: &Escape, window, cx| {
            cx.stop_propagation();
            v.close_search(window, cx);
        }))
        .flex()
        .flex_col()
        .flex_none()
        .w(px(PANEL_WIDTH))
        .h_full()
        .border_l_1()
        .border_color(theme.border)
        .bg(theme.background_secondary)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .px_3()
                .py_2()
                .border_b_1()
                .border_color(theme.border)
                .child(div().text_sm().font_weight(FontWeight::BOLD).child(l10n::tr("Search")))
                .child(
                    Button::new("close-search")
                        .ghost()
                        .small()
                        .icon(AppIcon::X)
                        .tooltip(l10n::tr("Close"))
                        .on_click(cx.listener(|v, _, window, cx| v.close_search(window, cx))),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .p_3()
                .border_b_1()
                .border_color(theme.border)
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .child(div().flex_1().min_w_0().child(Input::new(&panel.input).small()))
                        .child(
                            Button::new("search-submit")
                                .primary()
                                .small()
                                .icon(AppIcon::MagnifyingGlass)
                                .tooltip(l10n::tr("Search"))
                                .on_click(cx.listener(|v, _, _, cx| v.search_submit(cx))),
                        ),
                )
                .child(div().flex().flex_col().gap_1().child(scope_row).child(sort_row)),
        )
        .child(body)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_inline_search_filters() {
        let plain = parse_search_query("hello world");
        assert_eq!(plain.content.as_deref(), Some("hello world"));
        assert!(plain.has.is_empty());
        assert_eq!(plain.pinned, None);

        let filtered = parse_search_query("HAS:IMAGE has:file is:pinned report has:image has:unknown");
        assert_eq!(filtered.content.as_deref(), Some("report has:unknown"));
        assert_eq!(filtered.has, vec![MessageContentType::Image, MessageContentType::File]);
        assert_eq!(filtered.pinned, Some(true));

        let unpinned = parse_search_query("pinned:false");
        assert_eq!(unpinned.content, None);
        assert_eq!(unpinned.pinned, Some(false));

        let empty = parse_search_query("   \t  ");
        assert_eq!(empty, ParsedQuery::default());
    }

    #[test]
    fn build_hits_preserves_avatar_url_embeds_images_and_hides_bare_media_url() {
        let state = ClientState::new();
        let msg: Message = serde_json::from_value(serde_json::json!({
            "id": "5001",
            "channel_id": "11",
            "guild_id": "1",
            "author": { "id": "201", "username": "alice", "avatar": "a_hash123" },
            "content": "https://cdn.example/cat.gif",
            "timestamp": "2026-11-18T10:20:30.000000+00:00",
            "attachments": [{
                "id": "7001",
                "filename": "photo.png",
                "size": 1024,
                "content_type": "image/png",
                "url": "https://cdn.example/photo.png",
                "proxy_url": "https://media.example/proxy/photo.png",
                "width": 1200,
                "height": 800
            }],
            "embeds": [{
                "type": "image",
                "url": "https://cdn.example/cat.gif",
                "thumbnail": {
                    "url": "https://cdn.example/cat.gif",
                    "proxy_url": "https://media.example/proxy/cat.gif",
                    "width": 400,
                    "height": 300,
                    "flags": 1
                }
            }]
        }))
        .expect("message fixture");
        let page = MessageSearchPage {
            messages: vec![msg],
            channels: Vec::new(),
            threads: Vec::new(),
            total: 1,
            hits_per_page: 25,
            page: 1,
            cursor: None,
        };
        let hits = build_hits(&state, Some("https://media.example"), &page);
        assert_eq!(hits.len(), 1);
        let hit = &hits[0];
        assert_eq!(hit.user_id, Snowflake(201));
        assert_eq!(hit.avatar_url.as_deref(), Some("https://media.example/avatars/201/hash123.webp?size=80"));
        assert_eq!(hit.images.len(), 1);
        assert!(hit.images[0].width <= SEARCH_IMAGE_MAX_WIDTH);
        assert_eq!(hit.embeds.len(), 1);
        assert!(hit.document.is_none(), "bare URL with media-only embed hides redundant link text");
    }
}
