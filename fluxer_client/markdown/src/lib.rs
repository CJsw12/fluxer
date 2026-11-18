//! Fluxer message markdown for the GPUI client.
//!
//! Content is parsed by Fluxer's own parser (`fluxer_markdown_parser`, the
//! one the web client runs as WASM), so the syntax and edge cases match the
//! web client, not CommonMark. Two layers:
//!
//! - [`model`]: the AST lowered to blocks of styled spans, with mentions
//!   resolved through [`MentionResolver`], custom emoji URLs and formatted
//!   timestamps. Pure.
//! - [`MessageMarkdown`]: a GPUI element drawing a [`Document`] with
//!   gpui-kit's `TextView` (window-wide drag selection and copy, highlighted
//!   code blocks), spoilers that reveal on click, mention pills and inline
//!   emoji images.
//!
//! ```ignore
//! let time = TimeFormat::local(true);
//! let doc = Arc::new(parse(content, &LowerOptions {
//!     context: MarkdownContext::StandardWithJumbo,
//!     resolver: &my_resolver,
//!     media_endpoint: Some("https://media.example"),
//!     time: &time,
//!     disable_animated_emoji: false,
//! }));
//! MessageMarkdown::new(("msg", id), doc).style(style).on_mention_click(|target, window, cx| ...)
//! ```

pub mod model;
mod source;
pub mod timestamp;
mod view;

pub use model::{
    AlertKind, Alignment, Block, Document, Emoji, LinkRef, LowerOptions, MarkdownContext, Mention, MentionResolver,
    MentionTarget, NoMentions, ResolvedRole, Span, SpanContent, SpanStyle, SpoilerId, Table, Timestamp, TimestampStyle,
    custom_emoji_url, lower, parse,
};
pub use timestamp::{TimeFormat, Zone, format_timestamp};
pub use view::{LinkClickHandler, MarkdownStyle, MentionClickHandler, MessageMarkdown, SpoilerReveals};

#[cfg(test)]
mod model_tests;
#[cfg(test)]
mod source_tests;
#[cfg(test)]
mod view_tests;
