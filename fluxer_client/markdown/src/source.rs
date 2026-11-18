//! Writes a [`Document`] as the Markdown dialect gpui-kit's `TextView`
//! reads (CommonMark + GFM via markdown-rs, inline `<strong>`/`<em>`/`<u>`/
//! `<s>`/`<mark>` tags), so `TextView` lays out, selects and copies it.
//!
//! The document already follows Fluxer's parser; this writer only has to be
//! unambiguous. Every ASCII punctuation character of user text is
//! backslash-escaped, so no user text becomes markup (or HTML); styles are
//! written as inline HTML tags, which have none of the `*`/`_` flanking
//! rules; line-leading spaces are character references so they never
//! indent. Mentions, emoji, timestamps and hidden spoilers are links to
//! `fluxer-md:<index>` that the view's plugin draws as inline objects; the
//! link text is the object's copy text.

use crate::model::{Alignment, Block, Document, Span, SpanContent, SpanStyle, SpoilerId, blocks_text, spans_text};
use crate::model::{Emoji, Mention, Timestamp};

/// URL scheme of inline-object placeholder links.
pub(crate) const OBJECT_SCHEME: &str = "fluxer-md:";

/// Something drawn as one inline element rather than text.
#[derive(Debug, Clone, PartialEq)]
pub enum InlineObject {
    Mention(Mention),
    Emoji(Emoji),
    Timestamp(Timestamp),
    /// One word (with its trailing whitespace) of a hidden spoiler; words
    /// are separate objects so a long spoiler wraps.
    HiddenSpoiler { spoiler: SpoilerId, text: String },
}

/// One `TextView`: top-level subtext gets its own, drawn small and muted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub subtext: bool,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Lowered {
    pub segments: Vec<Segment>,
    /// Indexed by the `fluxer-md:<index>` links of every segment.
    pub objects: Vec<InlineObject>,
}

/// Writes `doc` with the spoilers `revealed` says are open; the revealed
/// inline spoilers get a `revealed_color` (`#rrggbbaa`) highlight.
pub fn write(doc: &Document, revealed: &dyn Fn(SpoilerId) -> bool, revealed_color: &str) -> Lowered {
    let mut writer = Writer { revealed, revealed_color, objects: Vec::new() };
    let mut segments = Vec::new();
    let mut current: Vec<Vec<String>> = Vec::new();
    for block in &doc.blocks {
        if let Block::Subtext(spans) = block {
            if !current.is_empty() {
                segments.push(Segment { subtext: false, source: join_blocks(std::mem::take(&mut current)) });
            }
            let lines = writer.paragraph(spans);
            if !lines.is_empty() {
                segments.push(Segment { subtext: true, source: lines.join("\n") });
            }
            continue;
        }
        let lines = writer.block(block);
        if !lines.is_empty() {
            current.push(lines);
        }
    }
    if !current.is_empty() {
        segments.push(Segment { subtext: false, source: join_blocks(current) });
    }
    Lowered { segments, objects: writer.objects }
}

fn join_blocks(blocks: Vec<Vec<String>>) -> String {
    blocks.into_iter().map(|lines| lines.join("\n")).collect::<Vec<_>>().join("\n\n")
}

/// How a run of spans is written.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Newlines are hard line breaks.
    Paragraph,
    /// One line (headings): newlines become spaces.
    Line,
    /// A GFM table cell: one line, `|` escaped even in code.
    Cell,
}

struct Writer<'a> {
    revealed: &'a dyn Fn(SpoilerId) -> bool,
    revealed_color: &'a str,
    objects: Vec<InlineObject>,
}

impl Writer<'_> {
    fn block(&mut self, block: &Block) -> Vec<String> {
        match block {
            Block::Paragraph(spans) | Block::Subtext(spans) => self.paragraph(spans),
            Block::Heading { level, spans } => {
                let text = self.inline(&trim_newlines(spans), Mode::Line);
                if text.trim().is_empty() {
                    return Vec::new();
                }
                vec![format!("{} {text}", "#".repeat(usize::from((*level).clamp(1, 6))))]
            }
            Block::CodeBlock { language, code } => code_block(language.as_deref(), code),
            Block::Quote(blocks) => quote(self.container(blocks, true)),
            Block::Alert { kind, blocks } => {
                let mut lines = vec![format!("<strong>{}</strong>", kind.title())];
                let body = self.container(blocks, true);
                if !body.is_empty() {
                    lines.push(String::new());
                    lines.extend(body);
                }
                quote(lines)
            }
            Block::List { ordered, start, items } => {
                let mut lines = Vec::new();
                for (i, item) in items.iter().enumerate() {
                    let marker = if *ordered { format!("{}. ", start.saturating_add(i)) } else { "- ".to_owned() };
                    let indent = " ".repeat(marker.len());
                    let body = self.container(item, false);
                    if body.is_empty() {
                        lines.push(marker.trim_end().to_owned());
                        continue;
                    }
                    for (j, line) in body.into_iter().enumerate() {
                        if j == 0 {
                            lines.push(format!("{marker}{line}"));
                        } else if line.is_empty() {
                            lines.push(line);
                        } else {
                            lines.push(format!("{indent}{line}"));
                        }
                    }
                }
                lines
            }
            Block::Table(table) => {
                let columns = table.header.len().max(1);
                let mut lines = vec![self.table_row(&table.header, columns)];
                let delimiters: Vec<&str> = (0..columns)
                    .map(|i| match table.alignments.get(i).copied().unwrap_or(Alignment::None) {
                        Alignment::None => "---",
                        Alignment::Left => ":--",
                        Alignment::Center => ":-:",
                        Alignment::Right => "--:",
                    })
                    .collect();
                lines.push(format!("| {} |", delimiters.join(" | ")));
                for row in &table.rows {
                    lines.push(self.table_row(row, columns));
                }
                lines
            }
            Block::Spoiler { id, blocks } => {
                if (self.revealed)(*id) {
                    return self.container(blocks, true);
                }
                // Hidden block spoiler: its text, as hidden words.
                let text = blocks_text(blocks);
                let line = self.hidden_spoiler(*id, &text, Mode::Paragraph);
                if line.is_empty() { Vec::new() } else { line.split('\n').map(str::to_owned).collect() }
            }
        }
    }

    /// Blocks inside a quote or list item. `loose` separates every block
    /// with a blank line; list items only where a paragraph would otherwise
    /// continue the block before it, to keep the list tight.
    fn container(&mut self, blocks: &[Block], loose: bool) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        let mut previous: Option<&Block> = None;
        for block in blocks {
            let body = self.block(block);
            if body.is_empty() {
                continue;
            }
            if let Some(previous) = previous {
                let continues = matches!(
                    previous,
                    Block::List { .. } | Block::Quote(_) | Block::Alert { .. } | Block::Spoiler { .. } | Block::Table(_)
                ) && matches!(block, Block::Paragraph(_) | Block::Subtext(_) | Block::Spoiler { .. });
                if loose || continues {
                    lines.push(String::new());
                }
            }
            lines.extend(body);
            previous = Some(block);
        }
        lines
    }

    fn paragraph(&mut self, spans: &[Span]) -> Vec<String> {
        let text = self.inline(&trim_newlines(spans), Mode::Paragraph);
        if text.is_empty() { Vec::new() } else { text.split('\n').map(str::to_owned).collect() }
    }

    fn table_row(&mut self, cells: &[Vec<Span>], columns: usize) -> String {
        let cells: Vec<String> = (0..columns)
            .map(|i| match cells.get(i) {
                Some(spans) => self.inline(&trim_newlines(spans), Mode::Cell),
                None => String::new(),
            })
            .collect();
        format!("| {} |", cells.join(" | "))
    }

    /// Spans as inline Markdown; in `Paragraph` mode a `\n` is written as a
    /// hard break (`\` at the end of the line).
    fn inline(&mut self, spans: &[Span], mode: Mode) -> String {
        let mut out = String::new();
        let mut i = 0;
        while i < spans.len() {
            let span = &spans[i];
            if let Some(id) = span.style.spoiler.filter(|id| !(self.revealed)(*id)) {
                let end = run_end(spans, i, |s| s.style.spoiler == Some(id));
                let text = spans_text(&spans[i..end]);
                out.push_str(&self.hidden_spoiler(id, &text, mode));
                i = end;
                continue;
            }
            if let Some(link) = &span.style.link {
                let end = run_end(spans, i, |s| {
                    s.style.link.as_ref().is_some_and(|l| l.id == link.id)
                        && !s.style.spoiler.is_some_and(|id| !(self.revealed)(id))
                });
                let mut text = String::new();
                for span in &spans[i..end] {
                    self.styled(span, mode, true, &mut text);
                }
                if text.is_empty() {
                    escape_text(&link.url, &mut text);
                }
                out.push('[');
                out.push_str(&text);
                out.push_str("](<");
                escape_destination(&link.url, &mut out);
                out.push_str(">)");
                i = end;
                continue;
            }
            self.styled(span, mode, false, &mut out);
            i += 1;
        }
        out
    }

    /// One span with its style tags. Inside a link (`in_link`) objects are
    /// written as their text: links do not nest.
    fn styled(&mut self, span: &Span, mode: Mode, in_link: bool, out: &mut String) {
        let style = &span.style;
        let mark = style.spoiler.is_some();
        let tags = style_tags(style, mark);
        if tags.is_empty() {
            self.content(span, mode, in_link, out);
            return;
        }
        // Tags close before a hard break and reopen after it, so a line
        // never starts with a dangling tag.
        let opening: String = tags
            .iter()
            .map(|tag| match *tag {
                "mark" => format!("<mark color=\"{}\">", self.revealed_color),
                tag => format!("<{tag}>"),
            })
            .collect();
        let closing: String = tags.iter().rev().map(|tag| format!("</{tag}>")).collect();
        match &span.content {
            SpanContent::Text(text) if mode == Mode::Paragraph && text.contains('\n') => {
                for (n, line) in text.split('\n').enumerate() {
                    if n > 0 {
                        hard_break(out);
                    }
                    if !line.is_empty() {
                        out.push_str(&opening);
                        escape_text(line, out);
                        out.push_str(&closing);
                    }
                }
            }
            _ => {
                out.push_str(&opening);
                self.content(span, mode, in_link, out);
                out.push_str(&closing);
            }
        }
    }

    fn content(&mut self, span: &Span, mode: Mode, in_link: bool, out: &mut String) {
        match &span.content {
            SpanContent::Text(text) => {
                for (n, line) in text.split('\n').enumerate() {
                    if n > 0 {
                        if mode == Mode::Paragraph {
                            hard_break(out);
                        } else {
                            out.push(' ');
                        }
                    }
                    escape_text(line, out);
                }
            }
            SpanContent::Code(code) => code_span(code, mode, out),
            SpanContent::Mention(mention) => self.object(InlineObject::Mention(mention.clone()), in_link, out),
            SpanContent::Emoji(emoji) => self.object(InlineObject::Emoji(emoji.clone()), in_link, out),
            SpanContent::Timestamp(timestamp) => {
                self.object(InlineObject::Timestamp(timestamp.clone()), in_link, out)
            }
        }
    }

    fn object(&mut self, object: InlineObject, in_link: bool, out: &mut String) {
        let text = object_copy_text(&object).to_owned();
        if in_link {
            escape_text(&text, out);
            return;
        }
        let index = self.objects.len();
        self.objects.push(object);
        out.push('[');
        escape_text(&text, out);
        out.push_str("](");
        out.push_str(OBJECT_SCHEME);
        out.push_str(&index.to_string());
        out.push(')');
    }

    /// Hidden spoiler text as one object per word (whitespace kept with the
    /// word before it); newlines are hard breaks.
    fn hidden_spoiler(&mut self, spoiler: SpoilerId, text: &str, mode: Mode) -> String {
        let mut out = String::new();
        for (n, line) in text.trim_end_matches('\n').split('\n').enumerate() {
            if n > 0 {
                if mode == Mode::Paragraph {
                    hard_break(&mut out);
                } else {
                    out.push(' ');
                }
            }
            for word in split_words(line) {
                self.object(InlineObject::HiddenSpoiler { spoiler, text: word.to_owned() }, false, &mut out);
            }
        }
        out
    }
}

/// A hard line break (`\` at the end of the line). After an inline object a
/// space goes first: gpui-base 0.10.0's `TextView` lays such a break out but
/// drops its newline from the copied text.
fn hard_break(out: &mut String) {
    let mut tail = out.as_str();
    while tail.ends_with('>') {
        match tail.rfind("</") {
            Some(at) if !tail[at..].contains(' ') => tail = &tail[..at],
            _ => break,
        }
    }
    if ends_with_object(tail) {
        out.push_str("&#32;");
    }
    out.push_str("\\\n");
}

fn ends_with_object(text: &str) -> bool {
    let Some(body) = text.strip_suffix(')') else { return false };
    let before_index = body.trim_end_matches(|c: char| c.is_ascii_digit());
    before_index.len() < body.len() && before_index.ends_with(&format!("]({OBJECT_SCHEME}"))
}

pub(crate) fn object_copy_text(object: &InlineObject) -> &str {
    match object {
        InlineObject::Mention(mention) => &mention.label,
        InlineObject::Emoji(emoji) => &emoji.copy_text,
        InlineObject::Timestamp(timestamp) => &timestamp.text,
        InlineObject::HiddenSpoiler { text, .. } => text,
    }
}

fn style_tags(style: &SpanStyle, mark: bool) -> Vec<&'static str> {
    let mut tags = Vec::new();
    if mark {
        tags.push("mark");
    }
    if style.bold {
        tags.push("strong");
    }
    if style.italic {
        tags.push("em");
    }
    if style.underline {
        tags.push("u");
    }
    if style.strikethrough {
        tags.push("s");
    }
    tags
}

fn run_end(spans: &[Span], start: usize, same: impl Fn(&Span) -> bool) -> usize {
    let mut end = start + 1;
    while end < spans.len() && same(&spans[end]) {
        end += 1;
    }
    end
}

/// Splits after each whitespace run: `"a  b c"` -> `["a  ", "b ", "c"]`.
fn split_words(line: &str) -> Vec<&str> {
    let mut words = Vec::new();
    let mut start = 0;
    let mut in_space = false;
    for (i, ch) in line.char_indices() {
        if ch.is_whitespace() {
            in_space = true;
        } else if in_space {
            words.push(&line[start..i]);
            start = i;
            in_space = false;
        }
    }
    if start < line.len() {
        words.push(&line[start..]);
    }
    words
}

/// Drops leading newlines of the first text and trailing ones of the last:
/// a paragraph cannot start or end with a hard break.
fn trim_newlines(spans: &[Span]) -> Vec<Span> {
    let mut spans: Vec<Span> = spans.to_vec();
    while let Some(Span { content: SpanContent::Text(text), .. }) = spans.first_mut() {
        let trimmed = text.trim_start_matches('\n');
        if trimmed.is_empty() {
            spans.remove(0);
            continue;
        }
        *text = trimmed.to_owned();
        break;
    }
    while let Some(Span { content: SpanContent::Text(text), .. }) = spans.last_mut() {
        let trimmed = text.trim_end_matches('\n');
        if trimmed.is_empty() {
            spans.pop();
            continue;
        }
        *text = trimmed.to_owned();
        break;
    }
    spans
}

/// User text: ASCII punctuation backslash-escaped; spaces and tabs that
/// start a line written as character references (they would indent or be
/// stripped); control characters dropped.
fn escape_text(text: &str, out: &mut String) {
    let mut line_start = out.is_empty() || out.ends_with('\n');
    for ch in text.chars() {
        match ch {
            ' ' if line_start => out.push_str("&#32;"),
            '\t' if line_start => out.push_str("&#9;"),
            '\t' => out.push('\t'),
            '\n' => out.push(' '),
            c if c.is_ascii_punctuation() => {
                out.push('\\');
                out.push(c);
                line_start = false;
            }
            '\0' => out.push('\u{FFFD}'),
            c if c.is_control() => {}
            c => {
                out.push(c);
                line_start = false;
            }
        }
    }
}

/// A link destination inside `<...>`.
fn escape_destination(url: &str, out: &mut String) {
    for ch in url.chars() {
        match ch {
            '<' | '>' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            '\n' => out.push_str("%0A"),
            '\r' => out.push_str("%0D"),
            c => out.push(c),
        }
    }
}

/// A code span with a fence longer than any backtick run inside.
fn code_span(code: &str, mode: Mode, out: &mut String) {
    let code = code.replace('\n', " ");
    let code = if mode == Mode::Cell { code.replace('|', "\\|") } else { code };
    if code.is_empty() {
        return;
    }
    let fence = "`".repeat(longest_run(&code, '`') + 1);
    let pad = code.starts_with('`')
        || code.ends_with('`')
        || (code.starts_with(' ') && code.ends_with(' ') && !code.trim().is_empty());
    out.push_str(&fence);
    if pad {
        out.push(' ');
    }
    out.push_str(&code);
    if pad {
        out.push(' ');
    }
    out.push_str(&fence);
}

fn code_block(language: Option<&str>, code: &str) -> Vec<String> {
    let fence = "`".repeat((longest_run(code, '`') + 1).max(3));
    let language: String = language
        .unwrap_or_default()
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != '`')
        .collect();
    let mut lines = vec![format!("{fence}{language}")];
    let body = code.strip_suffix('\n').unwrap_or(code);
    if !code.is_empty() {
        lines.extend(body.split('\n').map(|line| line.trim_end_matches('\r').to_owned()));
    }
    lines.push(fence);
    lines
}

fn longest_run(text: &str, ch: char) -> usize {
    let mut longest = 0;
    let mut run = 0;
    for c in text.chars() {
        if c == ch {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    longest
}

fn quote(lines: Vec<String>) -> Vec<String> {
    if lines.is_empty() {
        return vec![">".to_owned()];
    }
    lines.into_iter().map(|line| if line.is_empty() { ">".to_owned() } else { format!("> {line}") }).collect()
}
