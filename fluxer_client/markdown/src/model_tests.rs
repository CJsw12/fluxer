//! Lowering tests: the parser's own AST for each input, lowered and checked
//! against what the web renderers show.

use chrono::FixedOffset;
use fluxer_markdown_parser::ast::{GuildNavigationType, MentionKind, Node};

use crate::model::*;
use crate::timestamp::{TimeFormat, Zone};

pub(crate) struct Names;

impl MentionResolver for Names {
    fn user(&self, id: &str) -> Option<String> {
        (id == "1").then(|| "Alice".to_owned())
    }
    fn role(&self, id: &str) -> Option<ResolvedRole> {
        match id {
            "20" => Some(ResolvedRole { name: "Mods".to_owned(), color: Some(0x3498db) }),
            "21" => Some(ResolvedRole { name: "Plain".to_owned(), color: Some(0) }),
            _ => None,
        }
    }
    fn channel(&self, id: &str) -> Option<String> {
        (id == "10").then(|| "general".to_owned())
    }
}

/// 2026-11-18T16:05:09Z.
pub(crate) const NOW_S: u64 = 1_795_017_909;

pub(crate) fn utc_time() -> TimeFormat {
    TimeFormat {
        zone: Zone::Fixed(FixedOffset::east_opt(0).expect("utc")),
        hour12: true,
        now_ms: NOW_S as i64 * 1000,
    }
}

pub(crate) fn doc_in(content: &str, context: MarkdownContext) -> Document {
    let time = utc_time();
    parse(
        content,
        &LowerOptions {
            context,
            resolver: &Names,
            media_endpoint: Some("https://media.example/"),
            time: &time,
            disable_animated_emoji: false,
        },
    )
}

pub(crate) fn doc(content: &str) -> Document {
    doc_in(content, MarkdownContext::StandardWithJumbo)
}

fn text(content: &str) -> Span {
    Span { content: SpanContent::Text(content.to_owned()), style: SpanStyle::default() }
}

fn styled(content: &str, style: SpanStyle) -> Span {
    Span { content: SpanContent::Text(content.to_owned()), style }
}

fn paragraph(doc: &Document) -> &[Span] {
    match doc.blocks.as_slice() {
        [Block::Paragraph(spans)] => spans,
        other => panic!("expected one paragraph, got {other:?}"),
    }
}

fn mention(content: &str) -> Mention {
    match &paragraph(&doc(content))[0].content {
        SpanContent::Mention(mention) => mention.clone(),
        other => panic!("{content:?} gave {other:?}"),
    }
}

#[test]
fn nested_formatting_flattens_to_styled_spans() {
    let d = doc("**bold *it* __u__** ~~s~~ `c``d`");
    let bold = SpanStyle { bold: true, ..Default::default() };
    assert_eq!(
        paragraph(&d),
        [
            styled("bold ", bold.clone()),
            styled("it", SpanStyle { italic: true, ..bold.clone() }),
            styled(" ", bold.clone()),
            styled("u", SpanStyle { underline: true, ..bold }),
            text(" "),
            styled("s", SpanStyle { strikethrough: true, ..Default::default() }),
            text(" "),
            Span { content: SpanContent::Code("c``d".to_owned()), style: SpanStyle::default() },
        ]
    );
    assert_eq!(d.plain_text(), "bold it u s c``d");
}

#[test]
fn backslash_escapes_and_literal_punctuation_stay_text() {
    assert_eq!(paragraph(&doc("\\*esc\\* \\_x\\_")), [text("*esc* _x_")]);
    // From the parser's fixture suite: a stray backslash is kept.
    let input = "Nevermind ! It is on C:\\, i thought i moved it";
    assert_eq!(doc(input).plain_text(), input);
}

#[test]
fn user_mentions_show_name_or_id() {
    let known = mention("<@1>");
    assert_eq!(known.label, "@Alice");
    assert!(known.resolved);
    assert_eq!(known.target, MentionTarget::User("1".to_owned()));
    // `genericMention`: `@${name || kind.id}`.
    let unknown = mention("<@1555131954767069184>");
    assert_eq!(unknown.label, "@1555131954767069184");
    assert!(!unknown.resolved);
    assert_eq!(mention("<@!1>").label, "@Alice");
}

#[test]
fn role_channel_and_special_mentions() {
    let role = mention("<@&20>");
    assert_eq!((role.label.as_str(), role.color), ("@Mods", Some(0x3498db)));
    assert_eq!(mention("<@&21>").color, None, "color 0 is uncolored");
    assert_eq!(mention("<@&99>").label, "@Unknown role");
    let channel = mention("<#10>");
    assert_eq!((channel.label.as_str(), channel.resolved), ("general", true));
    assert_eq!(mention("<#11>").label, "unknown-channel");
    assert_eq!(mention("@everyone").label, "@everyone");
    assert_eq!(mention("@here").label, "@here");
    let command = mention("</ban user:42>");
    assert_eq!(command.label, "/ban user");
    assert_eq!(command.target, MentionTarget::Command { id: "42".to_owned(), name: "ban".to_owned() });
    assert_eq!(mention("</mod user ban:42>").label, "/mod user ban");
    assert_eq!(mention("<id:customize>").label, "<id:customize>");
}

#[test]
fn guild_navigation_labels() {
    let time = utc_time();
    let options = LowerOptions {
        context: MarkdownContext::StandardWithJumbo,
        resolver: &NoMentions,
        media_endpoint: None,
        time: &time,
        disable_animated_emoji: false,
    };
    let node = |navigation_type, id: Option<&str>| Node::Mention {
        kind: MentionKind::GuildNavigation { navigation_type, id: id.map(str::to_owned) },
    };
    let d = lower(
        &[
            node(GuildNavigationType::LinkedRoles, Some("7")),
            node(GuildNavigationType::LinkedRoles, None),
            node(GuildNavigationType::Browse, None),
        ],
        &options,
    );
    assert_eq!(d.plain_text(), "<id:linked-roles:7><id:linked-roles><id:browse>");
}

#[test]
fn custom_emoji_get_media_urls_and_copy_text() {
    let d = doc("hi <:blob:42> <a:dance:43>");
    let emoji: Vec<&Emoji> = paragraph(&d)
        .iter()
        .filter_map(|span| match &span.content {
            SpanContent::Emoji(emoji) => Some(emoji),
            _ => None,
        })
        .collect();
    assert_eq!(emoji[0].url.as_deref(), Some("https://media.example/emojis/42.webp?size=128"));
    assert_eq!(emoji[0].copy_text, "<:blob:42>");
    assert_eq!(emoji[0].name, ":blob:");
    assert_eq!(emoji[1].url.as_deref(), Some("https://media.example/emojis/43.webp?animated=true&size=128"));
    assert_eq!(emoji[1].copy_text, "<a:dance:43>");
    assert_eq!(d.plain_text(), "hi <:blob:42> <a:dance:43>");
    assert!(!d.jumbo, "text next to the emoji");

    let time = utc_time();
    let still = parse(
        "<a:dance:43>",
        &LowerOptions {
            context: MarkdownContext::StandardWithJumbo,
            resolver: &NoMentions,
            media_endpoint: Some("https://media.example"),
            time: &time,
            disable_animated_emoji: true,
        },
    );
    match &paragraph(&still)[0].content {
        SpanContent::Emoji(emoji) => {
            assert_eq!(emoji.url.as_deref(), Some("https://media.example/emojis/43.webp?size=128"));
            assert_eq!(emoji.copy_text, "<a:dance:43>", "copy keeps the animated form");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(custom_emoji_url("https://m", "1", false, true), "https://m/emojis/1.webp?size=240");
}

#[test]
fn emoji_only_messages_are_jumbo() {
    assert!(doc("<:a:1> <:b:2>").jumbo);
    assert!(doc(" <:a:1>\n").jumbo);
    assert!(!doc("<:a:1> x").jumbo);
    assert!(!doc("").jumbo);
    assert!(!doc_in("<:a:1>", MarkdownContext::StandardWithoutJumbo).jumbo);
    let many = "<:a:1>".repeat(31);
    assert!(!doc(&many).jumbo, "more than 30 emoji");
    assert!(doc(&"<:a:1>".repeat(30)).jumbo);
}

#[test]
fn timestamps_format_per_style() {
    let at = |content: &str| match &paragraph(&doc(content))[0].content {
        SpanContent::Timestamp(timestamp) => timestamp.clone(),
        other => panic!("{other:?}"),
    };
    let ts = at(&format!("<t:{NOW_S}>"));
    assert_eq!(ts.style, TimestampStyle::ShortDateTime);
    assert_eq!(ts.text, "November 18, 2026 at 4:05 PM");
    assert_eq!(ts.full.as_deref(), Some("Wednesday, November 18, 2026 4:05:09 PM"));
    assert_eq!(at(&format!("<t:{NOW_S}:t>")).text, "4:05 PM");
    assert_eq!(at(&format!("<t:{NOW_S}:F>")).text, "Wednesday, November 18, 2026 at 4:05 PM");
    assert_eq!(at(&format!("<t:{NOW_S}:d>")).text, "11/18/2026");
    assert_eq!(at(&format!("<t:{}:R>", NOW_S - 7200)).text, "2 hours ago");
    // Not a timestamp: unknown style letter.
    assert_eq!(doc(&format!("<t:{NOW_S}:x>")).plain_text(), format!("<t:{NOW_S}:x>"));
}

#[test]
fn links_keep_their_text_and_url() {
    let d = doc("see https://a.b/c and [docs](https://x.y/z)");
    let spans = paragraph(&d);
    let auto = &spans[1];
    assert_eq!(auto.content, SpanContent::Text("https://a.b/c".to_owned()));
    assert_eq!(auto.style.link.as_ref().map(|l| l.url.as_str()), Some("https://a.b/c"));
    let masked = &spans[3];
    assert_eq!(masked.content, SpanContent::Text("docs".to_owned()));
    assert_eq!(masked.style.link.as_ref().map(|l| l.url.as_str()), Some("https://x.y/z"));
    assert_ne!(auto.style.link.as_ref().map(|l| l.id), masked.style.link.as_ref().map(|l| l.id));
}

#[test]
fn inline_spoilers_get_ids_in_order() {
    let d = doc("a ||one|| b ||**two**||");
    let spans = paragraph(&d);
    assert_eq!(spans[1], styled("one", SpanStyle { spoiler: Some(0), ..Default::default() }));
    assert_eq!(spans[3], styled("two", SpanStyle { spoiler: Some(1), bold: true, ..Default::default() }));
    assert_eq!(d.spoiler_count, 2);
    assert_eq!(d.plain_text(), "a one b two", "spoilers copy as their text");
}

#[test]
fn block_spoiler_holds_blocks() {
    let d = doc("||\nblock\n- x\n||");
    match d.blocks.as_slice() {
        [Block::Spoiler { id: 0, blocks }] => {
            assert_eq!(blocks[0], Block::Paragraph(vec![text("block")]));
            assert!(matches!(blocks[1], Block::List { ordered: false, .. }));
        }
        other => panic!("{other:?}"),
    }
    // A reply preview has no blocks: the spoiler stays inline.
    let reply = doc_in("||\nblock\n||", MarkdownContext::InlineReply);
    assert!(matches!(reply.blocks.as_slice(), [Block::Paragraph(_)]), "{reply:?}");
}

#[test]
fn text_and_blocks_split_into_paragraphs() {
    let d = doc("para\n```rs\nfn x(){}\n```\nafter");
    assert_eq!(
        d.blocks,
        [
            Block::Paragraph(vec![text("para")]),
            Block::CodeBlock { language: Some("rs".to_owned()), code: "fn x(){}\n".to_owned() },
            Block::Paragraph(vec![text("after")]),
        ]
    );
    assert_eq!(d.plain_text(), "para\nfn x(){}\nafter");

    // Blank lines inside text are kept; adjacent text nodes merge.
    let d = doc("# Head\ntext\n\n\nmore");
    assert_eq!(
        d.blocks,
        [
            Block::Heading { level: 1, spans: vec![text("Head")] },
            Block::Paragraph(vec![text("text\n\n\nmore")]),
        ]
    );
}

#[test]
fn lists_quotes_subtext_alerts_tables() {
    let d = doc("3. a\n4. b");
    match &d.blocks[0] {
        Block::List { ordered: true, start: 3, items } => assert_eq!(items.len(), 2),
        other => panic!("{other:?}"),
    }
    let d = doc("> quote\nnot");
    assert_eq!(
        d.blocks,
        [Block::Quote(vec![Block::Paragraph(vec![text("quote")])]), Block::Paragraph(vec![text("not")])]
    );
    let d = doc("-# small\nnext");
    assert_eq!(d.blocks, [Block::Subtext(vec![text("small")]), Block::Paragraph(vec![text("next")])]);
    let d = doc("> [!WARNING]\n> careful");
    assert_eq!(
        d.blocks,
        [Block::Alert { kind: AlertKind::Warning, blocks: vec![Block::Paragraph(vec![text("careful")])] }]
    );
    assert_eq!(d.plain_text(), "Warning\ncareful");
    let d = doc("| a | b |\n|---|:-:|\n| 1 | 2 |");
    match &d.blocks[0] {
        Block::Table(table) => {
            assert_eq!(table.alignments, [Alignment::None, Alignment::Center]);
            assert_eq!(table.rows.len(), 1);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(d.plain_text(), "a\tb\n1\t2");
}

#[test]
fn restricted_contexts_follow_their_flags() {
    // Replies: no headings or lists, newlines collapse.
    let d = doc_in("# not a heading\n- nor a list\n\n  end", MarkdownContext::InlineReply);
    assert_eq!(d.plain_text(), "# not a heading - nor a list end");
    // Bios: no code blocks.
    let d = doc_in("```\ncode\n```", MarkdownContext::UserBio);
    assert!(!d.blocks.iter().any(|b| matches!(b, Block::CodeBlock { .. })), "{d:?}");
    // Previews keep headings.
    let d = doc_in("# title", MarkdownContext::InlinePreview);
    assert!(matches!(d.blocks[0], Block::Heading { .. }));
}

#[test]
fn alert_body_keeps_blank_lines_before_lists() {
    // From the parser's block_spacing suite: `["para", "", "- a"]`.
    let d = doc("> [!NOTE]\n> para\n> \n> - a");
    match &d.blocks[0] {
        Block::Alert { blocks, .. } => {
            // The text node is "para\n\n": one newline ends the line, one is a blank line.
            assert_eq!(blocks[0], Block::Paragraph(vec![text("para\n")]));
            assert!(matches!(blocks[1], Block::List { .. }));
        }
        other => panic!("{other:?}"),
    }
}
