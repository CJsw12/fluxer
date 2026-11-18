//! Render model: the Fluxer parser AST lowered into blocks of styled spans.
//!
//! Mirrors the web renderers in
//! `fluxer_app/src/features/messaging/components/markdown/renderers/`:
//! mentions resolve to the labels `MentionRenderer` shows, custom emoji to
//! the media URL `getEmojiRenderUrl` builds, timestamps to the text
//! `formatTimestamp` produces. Everything here is pure.

use fluxer_markdown_parser::ast::{
    AlertType, EmojiKind, GuildNavigationType, ListItem, MentionKind, Node, TableAlignment,
};
use fluxer_markdown_parser::plaintext::{PlaintextOptions, render_ast_to_plaintext};
use fluxer_markdown_parser::{EmojiContext, MarkdownParser, ParserFlags};

pub use fluxer_markdown_parser::ast::TimestampStyle;

use crate::timestamp::{TimeFormat, format_full_date_time, format_timestamp};

/// Where the markdown is shown (`MarkdownContext` in `RendererTypes.ts`);
/// selects the parser flags (`MarkdownParserFlags.ts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkdownContext {
    /// Message content; emoji-only messages render jumbo.
    StandardWithJumbo,
    StandardWithoutJumbo,
    /// Reply preview above a message: inline, newlines collapse to spaces.
    InlineReply,
    UserBio,
    EmbedDescription,
    /// Inline preview (e.g. notifications): like a reply, but with headings and lists.
    InlinePreview,
}

const STANDARD_FLAGS: u32 = ParserFlags::ALLOW_SPOILERS
    | ParserFlags::ALLOW_HEADINGS
    | ParserFlags::ALLOW_LISTS
    | ParserFlags::ALLOW_CODE_BLOCKS
    | ParserFlags::ALLOW_MASKED_LINKS
    | ParserFlags::ALLOW_COMMAND_MENTIONS
    | ParserFlags::ALLOW_GUILD_NAVIGATIONS
    | ParserFlags::ALLOW_USER_MENTIONS
    | ParserFlags::ALLOW_ROLE_MENTIONS
    | ParserFlags::ALLOW_CHANNEL_MENTIONS
    | ParserFlags::ALLOW_EVERYONE_MENTIONS
    | ParserFlags::ALLOW_BLOCKQUOTES
    | ParserFlags::ALLOW_MULTILINE_BLOCKQUOTES
    | ParserFlags::ALLOW_SUBTEXT
    | ParserFlags::ALLOW_TABLES
    | ParserFlags::ALLOW_ALERTS
    | ParserFlags::ALLOW_AUTOLINKS;

impl MarkdownContext {
    /// `getParserFlagsForContext`.
    pub fn parser_flags(self) -> u32 {
        let reply = STANDARD_FLAGS
            & !(ParserFlags::ALLOW_BLOCKQUOTES
                | ParserFlags::ALLOW_MULTILINE_BLOCKQUOTES
                | ParserFlags::ALLOW_TABLES
                | ParserFlags::ALLOW_ALERTS
                | ParserFlags::ALLOW_HEADINGS
                | ParserFlags::ALLOW_LISTS);
        match self {
            Self::StandardWithJumbo | Self::StandardWithoutJumbo => STANDARD_FLAGS,
            Self::InlineReply => reply,
            Self::InlinePreview => reply | ParserFlags::ALLOW_HEADINGS | ParserFlags::ALLOW_LISTS,
            Self::UserBio => {
                STANDARD_FLAGS
                    & !(ParserFlags::ALLOW_HEADINGS
                        | ParserFlags::ALLOW_CODE_BLOCKS
                        | ParserFlags::ALLOW_ROLE_MENTIONS
                        | ParserFlags::ALLOW_EVERYONE_MENTIONS
                        | ParserFlags::ALLOW_SUBTEXT
                        | ParserFlags::ALLOW_TABLES
                        | ParserFlags::ALLOW_ALERTS)
            }
            Self::EmbedDescription => STANDARD_FLAGS & !ParserFlags::ALLOW_TABLES,
        }
    }

    /// `isRestrictedInlineContext`.
    pub fn is_restricted_inline(self) -> bool {
        matches!(self, Self::InlineReply | Self::InlinePreview)
    }
}

/// Names the app knows for mention ids. `None` gives the web client's
/// fallback label (`@<id>`, `@Unknown role`, `unknown-channel`).
pub trait MentionResolver {
    /// Display name (guild nickname or global name) of a user.
    fn user(&self, id: &str) -> Option<String>;
    fn role(&self, id: &str) -> Option<ResolvedRole>;
    /// Name of a mentionable channel (text, announcement, voice, link or thread).
    fn channel(&self, id: &str) -> Option<String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRole {
    pub name: String,
    /// `0xRRGGBB`; `None` (or 0) for an uncolored role.
    pub color: Option<u32>,
}

/// A resolver that knows nothing: every mention gets its fallback label.
pub struct NoMentions;

impl MentionResolver for NoMentions {
    fn user(&self, _id: &str) -> Option<String> {
        None
    }
    fn role(&self, _id: &str) -> Option<ResolvedRole> {
        None
    }
    fn channel(&self, _id: &str) -> Option<String> {
        None
    }
}

/// Inputs of the lowering besides the AST.
pub struct LowerOptions<'a> {
    pub context: MarkdownContext,
    pub resolver: &'a dyn MentionResolver,
    /// `RuntimeConfig.mediaEndpoint` (e.g. `https://fluxerusercontent.com`);
    /// without it custom emoji have no image and show as `:name:`.
    pub media_endpoint: Option<&'a str>,
    pub time: &'a TimeFormat,
    /// User setting: animated custom emoji render as their static image.
    pub disable_animated_emoji: bool,
}

/// A message lowered for rendering.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Document {
    pub blocks: Vec<Block>,
    /// Emoji-only message (`shouldRenderAsJumboEmojis`): emoji render large.
    pub jumbo: bool,
    /// Number of spoilers; their ids are `0..spoiler_count` in source order.
    pub spoiler_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    /// A run of inline content; `\n` inside text is a line break.
    Paragraph(Vec<Span>),
    Heading { level: u8, spans: Vec<Span> },
    /// `-# small text`.
    Subtext(Vec<Span>),
    CodeBlock { language: Option<String>, code: String },
    Quote(Vec<Block>),
    Alert { kind: AlertKind, blocks: Vec<Block> },
    /// `start` is the first item's ordinal (ordered lists only).
    List { ordered: bool, start: usize, items: Vec<Vec<Block>> },
    Table(Table),
    /// `||` on its own lines around block content.
    Spoiler { id: SpoilerId, blocks: Vec<Block> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertKind {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

impl AlertKind {
    /// Title the web client shows above the alert body.
    pub fn title(self) -> &'static str {
        match self {
            Self::Note => "Note",
            Self::Tip => "Tip",
            Self::Important => "Important",
            Self::Warning => "Warning",
            Self::Caution => "Caution",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub header: Vec<Vec<Span>>,
    pub alignments: Vec<Alignment>,
    pub rows: Vec<Vec<Vec<Span>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    None,
    Left,
    Center,
    Right,
}

/// Index of a spoiler in the document, in source order.
pub type SpoilerId = usize;

#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub content: SpanContent,
    pub style: SpanStyle,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SpanStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    /// Outermost spoiler around the span. Hidden until the viewer reveals
    /// that id; revealing it shows everything inside, nested spoilers too.
    pub spoiler: Option<SpoilerId>,
    pub link: Option<LinkRef>,
}

/// A link target; spans of one link share its `id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkRef {
    pub id: usize,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SpanContent {
    Text(String),
    Code(String),
    Mention(Mention),
    Emoji(Emoji),
    Timestamp(Timestamp),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mention {
    pub target: MentionTarget,
    /// Text of the pill: `@Alice`, `@Mods`, `general` (channels show an
    /// icon before the name), `@everyone`, `/cmd sub`.
    pub label: String,
    /// Role color (`0xRRGGBB`) tinting the pill.
    pub color: Option<u32>,
    /// The target is known (a user popout or channel link can open).
    pub resolved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MentionTarget {
    User(String),
    Role(String),
    Channel(String),
    Everyone,
    Here,
    Command { id: String, name: String },
    GuildNavigation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Emoji {
    /// `:name:` (accessible name / tooltip).
    pub name: String,
    /// Custom emoji id; `None` for a Unicode emoji.
    pub id: Option<String>,
    /// Image to draw; `None` draws `text` instead.
    pub url: Option<String>,
    /// Drawn when there is no image: the Unicode emoji, or `:name:`.
    pub text: String,
    /// What a copy of the emoji yields: `<:name:id>`, `<a:name:id>`, or the
    /// raw Unicode emoji (`emojiCopyText`).
    pub copy_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timestamp {
    pub unix: u64,
    pub style: TimestampStyle,
    /// Text shown in the message.
    pub text: String,
    /// Tooltip text (full date with seconds); `None` for an invalid date.
    pub full: Option<String>,
}

const CUSTOM_EMOJI_IMAGE_RUNG: u32 = 128;
const CUSTOM_EMOJI_ENLARGED_IMAGE_RUNG: u32 = 240;
const MAX_JUMBO_EMOJI_COUNT: usize = 30;

/// Parses `content` with Fluxer's parser and lowers it. Unicode emoji are
/// left as text (no emoji database on this side; see `lower` to pass nodes
/// parsed with an `EmojiContext`).
pub fn parse(content: &str, options: &LowerOptions<'_>) -> Document {
    let mut parser = MarkdownParser::new(options.context.parser_flags(), EmojiContext::default());
    match parser.parse(content) {
        Ok(nodes) => lower(&nodes, options),
        // The parser only fails serializing JSON, which `parse` never does;
        // keep the text visible anyway.
        Err(_) => Document {
            blocks: vec![Block::Paragraph(vec![Span {
                content: SpanContent::Text(content.to_owned()),
                style: SpanStyle::default(),
            }])],
            jumbo: false,
            spoiler_count: 0,
        },
    }
}

/// Lowers parsed nodes.
pub fn lower(nodes: &[Node], options: &LowerOptions<'_>) -> Document {
    let mut lowerer = Lowerer { options, next_spoiler: 0, next_link: 0 };
    let blocks = lowerer.blocks(nodes);
    Document {
        blocks,
        jumbo: options.context == MarkdownContext::StandardWithJumbo && is_jumbo(nodes),
        spoiler_count: lowerer.next_spoiler,
    }
}

/// `shouldRenderAsJumboEmojis` (without the compact-mode setting, which
/// the app applies by choosing `StandardWithoutJumbo`).
fn is_jumbo(nodes: &[Node]) -> bool {
    let mut count = 0;
    for node in nodes {
        match node {
            Node::Emoji { .. } => {
                count += 1;
                if count > MAX_JUMBO_EMOJI_COUNT {
                    return false;
                }
            }
            Node::Text { content } if content.trim().is_empty() => {}
            _ => return false,
        }
    }
    count > 0
}

struct Lowerer<'o, 'a> {
    options: &'o LowerOptions<'a>,
    next_spoiler: usize,
    next_link: usize,
}

impl Lowerer<'_, '_> {
    fn is_block(&self, node: &Node) -> bool {
        match node {
            Node::Blockquote { .. }
            | Node::Heading { .. }
            | Node::Subtext { .. }
            | Node::List { .. }
            | Node::CodeBlock { .. }
            | Node::Table { .. }
            | Node::Alert { .. } => true,
            Node::Spoiler { is_block, .. } => {
                *is_block == Some(true) && !self.options.context.is_restricted_inline()
            }
            _ => false,
        }
    }

    fn blocks(&mut self, nodes: &[Node]) -> Vec<Block> {
        let mut blocks = Vec::new();
        let mut spans = Vec::new();
        for node in nodes {
            if self.is_block(node) {
                flush_paragraph(&mut spans, &mut blocks);
                blocks.push(self.block(node));
            } else {
                self.inline(node, &SpanStyle::default(), &mut spans);
            }
        }
        flush_paragraph(&mut spans, &mut blocks);
        blocks
    }

    fn block(&mut self, node: &Node) -> Block {
        match node {
            Node::Blockquote { children, .. } => Block::Quote(self.blocks(children)),
            Node::Alert { alert_type, children } => Block::Alert {
                kind: match alert_type {
                    AlertType::Note => AlertKind::Note,
                    AlertType::Tip => AlertKind::Tip,
                    AlertType::Important => AlertKind::Important,
                    AlertType::Warning => AlertKind::Warning,
                    AlertType::Caution => AlertKind::Caution,
                },
                blocks: self.blocks(children),
            },
            Node::Heading { level, children } => Block::Heading { level: *level, spans: self.spans(children) },
            Node::Subtext { children } => Block::Subtext(self.spans(children)),
            Node::List { ordered, items } => Block::List {
                ordered: *ordered,
                start: items.first().and_then(|item| item.ordinal).unwrap_or(1),
                items: items.iter().map(|ListItem { children, .. }| self.blocks(children)).collect(),
            },
            Node::CodeBlock { language, content } => Block::CodeBlock {
                language: language.clone().filter(|lang| !lang.is_empty()),
                code: content.clone(),
            },
            Node::Table { header, alignments, rows } => Block::Table(Table {
                header: self.row(header),
                alignments: alignments
                    .iter()
                    .map(|alignment| match alignment {
                        TableAlignment::Left => Alignment::Left,
                        TableAlignment::Center => Alignment::Center,
                        TableAlignment::Right => Alignment::Right,
                        TableAlignment::None => Alignment::None,
                    })
                    .collect(),
                rows: rows.iter().map(|row| self.row(row)).collect(),
            }),
            Node::Spoiler { children, .. } => {
                let id = self.next_spoiler;
                self.next_spoiler += 1;
                Block::Spoiler { id, blocks: self.blocks(children) }
            }
            other => {
                let mut spans = Vec::new();
                self.inline(other, &SpanStyle::default(), &mut spans);
                Block::Paragraph(spans)
            }
        }
    }

    fn row(&mut self, row: &Node) -> Vec<Vec<Span>> {
        match row {
            Node::TableRow { cells } => cells
                .iter()
                .map(|cell| match cell {
                    Node::TableCell { children } => self.spans(children),
                    other => self.spans(std::slice::from_ref(other)),
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    fn spans(&mut self, nodes: &[Node]) -> Vec<Span> {
        let mut spans = Vec::new();
        for node in nodes {
            self.inline(node, &SpanStyle::default(), &mut spans);
        }
        spans
    }

    fn inline(&mut self, node: &Node, style: &SpanStyle, out: &mut Vec<Span>) {
        match node {
            Node::Text { content } => {
                let text = if self.options.context.is_restricted_inline() {
                    let text = collapse_whitespace(content);
                    // Collapse across adjacent text nodes too.
                    match out.last() {
                        Some(Span { content: SpanContent::Text(last), .. }) if last.ends_with(' ') => {
                            text.strip_prefix(' ').map(str::to_owned).unwrap_or(text)
                        }
                        _ => text,
                    }
                } else {
                    content.clone()
                };
                push_text(out, text, style);
            }
            Node::Strong { children } => {
                let style = SpanStyle { bold: true, ..style.clone() };
                self.inline_all(children, &style, out);
            }
            Node::Emphasis { children } => {
                let style = SpanStyle { italic: true, ..style.clone() };
                self.inline_all(children, &style, out);
            }
            Node::Underline { children } => {
                let style = SpanStyle { underline: true, ..style.clone() };
                self.inline_all(children, &style, out);
            }
            Node::Strikethrough { children } => {
                let style = SpanStyle { strikethrough: true, ..style.clone() };
                self.inline_all(children, &style, out);
            }
            Node::Spoiler { children, .. } => {
                let id = self.next_spoiler;
                self.next_spoiler += 1;
                let style = SpanStyle { spoiler: Some(style.spoiler.unwrap_or(id)), ..style.clone() };
                self.inline_all(children, &style, out);
            }
            Node::Sequence { children } => self.inline_all(children, style, out),
            Node::InlineCode { content } => {
                out.push(Span { content: SpanContent::Code(content.clone()), style: style.clone() })
            }
            Node::Link { text, url, .. } => {
                let link = LinkRef { id: self.next_link, url: url.clone() };
                self.next_link += 1;
                let style = SpanStyle { link: Some(link), ..style.clone() };
                match text {
                    Some(text) => self.inline(text, &style, out),
                    None => push_text(out, url.clone(), &style),
                }
            }
            Node::Mention { kind } => out.push(Span {
                content: SpanContent::Mention(self.mention(kind)),
                style: style.clone(),
            }),
            Node::Emoji { kind } => out.push(Span {
                content: SpanContent::Emoji(self.emoji(kind)),
                style: style.clone(),
            }),
            Node::Timestamp { timestamp, style: ts_style } => out.push(Span {
                content: SpanContent::Timestamp(Timestamp {
                    unix: *timestamp,
                    style: *ts_style,
                    text: format_timestamp(*timestamp, *ts_style, self.options.time),
                    full: format_full_date_time(*timestamp, self.options.time),
                }),
                style: style.clone(),
            }),
            // A block where only inline content fits (e.g. a list inside an
            // inline spoiler): keep its text.
            block => {
                let text = render_ast_to_plaintext(std::slice::from_ref(block), &PlaintextOptions::default());
                push_text(out, text, style);
            }
        }
    }

    fn inline_all(&mut self, nodes: &[Node], style: &SpanStyle, out: &mut Vec<Span>) {
        for node in nodes {
            self.inline(node, style, out);
        }
    }

    fn mention(&self, kind: &MentionKind) -> Mention {
        let resolver = self.options.resolver;
        match kind {
            MentionKind::User { id } => {
                let name = resolver.user(id);
                Mention {
                    target: MentionTarget::User(id.clone()),
                    label: format!("@{}", name.as_deref().unwrap_or(id)),
                    color: None,
                    resolved: name.is_some(),
                }
            }
            MentionKind::Role { id } => match resolver.role(id) {
                Some(role) => Mention {
                    target: MentionTarget::Role(id.clone()),
                    label: format!("@{}", role.name),
                    color: role.color.filter(|color| *color != 0),
                    resolved: true,
                },
                None => Mention {
                    target: MentionTarget::Role(id.clone()),
                    label: "@Unknown role".to_owned(),
                    color: None,
                    resolved: false,
                },
            },
            MentionKind::Channel { id } => {
                let name = resolver.channel(id);
                Mention {
                    target: MentionTarget::Channel(id.clone()),
                    resolved: name.is_some(),
                    label: name.unwrap_or_else(|| "unknown-channel".to_owned()),
                    color: None,
                }
            }
            MentionKind::Everyone => Mention {
                target: MentionTarget::Everyone,
                label: "@everyone".to_owned(),
                color: None,
                resolved: true,
            },
            MentionKind::Here => Mention {
                target: MentionTarget::Here,
                label: "@here".to_owned(),
                color: None,
                resolved: true,
            },
            MentionKind::Command { name, subcommand_group, subcommand, id } => {
                let mut label = format!("/{name}");
                for part in [subcommand_group, subcommand].into_iter().flatten() {
                    label.push(' ');
                    label.push_str(part);
                }
                Mention {
                    target: MentionTarget::Command { id: id.clone(), name: name.clone() },
                    label,
                    color: None,
                    resolved: true,
                }
            }
            MentionKind::GuildNavigation { navigation_type, id } => {
                let label = match (navigation_type, id) {
                    (GuildNavigationType::Customize, _) => "<id:customize>".to_owned(),
                    (GuildNavigationType::Browse, _) => "<id:browse>".to_owned(),
                    (GuildNavigationType::Guide, _) => "<id:guide>".to_owned(),
                    (GuildNavigationType::LinkedRoles, Some(id)) => format!("<id:linked-roles:{id}>"),
                    (GuildNavigationType::LinkedRoles, None) => "<id:linked-roles>".to_owned(),
                };
                Mention { target: MentionTarget::GuildNavigation, label, color: None, resolved: true }
            }
        }
    }

    fn emoji(&self, kind: &EmojiKind) -> Emoji {
        match kind {
            EmojiKind::Standard { raw, name, .. } => Emoji {
                name: format!(":{name}:"),
                id: None,
                url: None,
                text: raw.clone(),
                copy_text: raw.clone(),
            },
            EmojiKind::Custom { name, id, animated } => {
                let play = *animated && !self.options.disable_animated_emoji;
                Emoji {
                    name: format!(":{name}:"),
                    id: Some(id.clone()),
                    url: self.options.media_endpoint.map(|endpoint| custom_emoji_url(endpoint, id, play, false)),
                    text: format!(":{name}:"),
                    copy_text: format!("<{}:{name}:{id}>", if *animated { "a" } else { "" }),
                }
            }
        }
    }
}

/// `buildCustomEmojiURL`: `{media}/emojis/{id}.webp`, `animated=true` when
/// it plays, then the size rung.
pub fn custom_emoji_url(media_endpoint: &str, id: &str, animated: bool, jumbo: bool) -> String {
    let size = if jumbo { CUSTOM_EMOJI_ENLARGED_IMAGE_RUNG } else { CUSTOM_EMOJI_IMAGE_RUNG };
    let base = media_endpoint.trim_end_matches('/');
    if animated {
        format!("{base}/emojis/{id}.webp?animated=true&size={size}")
    } else {
        format!("{base}/emojis/{id}.webp?size={size}")
    }
}

/// `TextRenderer` in restricted inline contexts: newlines become spaces and
/// whitespace runs collapse.
fn collapse_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !in_space {
                out.push(' ');
            }
            in_space = true;
        } else {
            out.push(ch);
            in_space = false;
        }
    }
    out
}

fn push_text(out: &mut Vec<Span>, text: String, style: &SpanStyle) {
    if text.is_empty() {
        return;
    }
    if let Some(Span { content: SpanContent::Text(last), style: last_style }) = out.last_mut()
        && last_style == style
    {
        last.push_str(&text);
        return;
    }
    out.push(Span { content: SpanContent::Text(text), style: style.clone() });
}

/// Ends a paragraph. Inline text is laid out like the web client's
/// `white-space: pre-wrap`: the newline that ends the last line before a
/// block (or the message end) draws nothing, so one is dropped.
fn flush_paragraph(spans: &mut Vec<Span>, blocks: &mut Vec<Block>) {
    if let Some(Span { content: SpanContent::Text(last), .. }) = spans.last_mut()
        && last.ends_with('\n')
    {
        last.pop();
        if last.is_empty() {
            spans.pop();
        }
    }
    if !spans.is_empty() {
        blocks.push(Block::Paragraph(std::mem::take(spans)));
    }
}

impl Span {
    /// What copying this span yields.
    pub fn copy_text(&self) -> &str {
        match &self.content {
            SpanContent::Text(text) | SpanContent::Code(text) => text,
            SpanContent::Mention(mention) => &mention.label,
            SpanContent::Emoji(emoji) => &emoji.copy_text,
            SpanContent::Timestamp(timestamp) => &timestamp.text,
        }
    }
}

/// Concatenated copy text of spans.
pub fn spans_text(spans: &[Span]) -> String {
    spans.iter().map(Span::copy_text).collect()
}

impl Document {
    /// The whole message as plain text, as copying all of it in the web
    /// client gives: spoilers show their text, mentions their label, custom
    /// emoji `<:name:id>`; blocks on their own lines.
    pub fn plain_text(&self) -> String {
        blocks_text(&self.blocks)
    }
}

/// Plain text of blocks (see [`Document::plain_text`]).
pub(crate) fn blocks_text(blocks: &[Block]) -> String {
    blocks.iter().map(block_text).collect::<Vec<_>>().join("\n")
}

fn block_text(block: &Block) -> String {
    match block {
        Block::Paragraph(spans) | Block::Heading { spans, .. } | Block::Subtext(spans) => spans_text(spans),
        Block::CodeBlock { code, .. } => code.strip_suffix('\n').unwrap_or(code).to_owned(),
        Block::Quote(blocks) | Block::Spoiler { blocks, .. } => blocks_text(blocks),
        Block::Alert { kind, blocks } => format!("{}\n{}", kind.title(), blocks_text(blocks)),
        Block::List { items, .. } => items.iter().map(|item| blocks_text(item)).collect::<Vec<_>>().join("\n"),
        Block::Table(table) => std::iter::once(&table.header)
            .chain(&table.rows)
            .map(|row| row.iter().map(|cell| spans_text(cell)).collect::<Vec<_>>().join("\t"))
            .collect::<Vec<_>>()
            .join("\n"),
    }
}
