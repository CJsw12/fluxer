//! Source writer tests: exact output for the constructs, and a round trip
//! through markdown-rs (the parser `TextView` runs) proving user text never
//! turns into markup.

use markdown::mdast::Node as Md;

use crate::model::Document;
use crate::model_tests::doc;
use crate::source::{InlineObject, Lowered, OBJECT_SCHEME, write};

const MARK: &str = "#80808026";

fn hidden(doc: &Document) -> Lowered {
    write(doc, &|_| false, MARK)
}

fn revealed(doc: &Document) -> Lowered {
    write(doc, &|_| true, MARK)
}

fn source(content: &str) -> String {
    let lowered = hidden(&doc(content));
    lowered.segments.iter().map(|s| s.source.as_str()).collect::<Vec<_>>().join("\n---segment---\n")
}

/// Text a reader of the mdast sees: objects as their link text, style tags
/// dropped. Panics on any construct the writer never emits on purpose
/// (emphasis from `*`, images, footnotes, math, raw HTML other than its tags).
fn visible(source: &str, objects: &[InlineObject]) -> String {
    let root = markdown::to_mdast(source, &markdown::ParseOptions::gfm()).expect("markdown parses");
    let mut out = String::new();
    block_text(&root, objects, &mut out);
    out
}

fn block_text(node: &Md, objects: &[InlineObject], out: &mut String) {
    match node {
        Md::Root(root) => join_blocks(&root.children, objects, out),
        Md::Blockquote(quote) => join_blocks(&quote.children, objects, out),
        Md::List(list) => join_blocks(&list.children, objects, out),
        Md::ListItem(item) => join_blocks(&item.children, objects, out),
        Md::Paragraph(p) => inline_text(&p.children, objects, out),
        Md::Heading(h) => inline_text(&h.children, objects, out),
        Md::Code(code) => out.push_str(&code.value),
        Md::Table(table) => {
            for (i, row) in table.children.iter().enumerate() {
                if i > 0 {
                    out.push('\n');
                }
                let Md::TableRow(row) = row else { panic!("table child {row:?}") };
                for (j, cell) in row.children.iter().enumerate() {
                    if j > 0 {
                        out.push('\t');
                    }
                    let Md::TableCell(cell) = cell else { panic!("row child {cell:?}") };
                    inline_text(&cell.children, objects, out);
                }
            }
        }
        other => panic!("unexpected block {other:?}"),
    }
}

fn join_blocks(children: &[Md], objects: &[InlineObject], out: &mut String) {
    for (i, child) in children.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        block_text(child, objects, out);
    }
}

fn inline_text(children: &[Md], objects: &[InlineObject], out: &mut String) {
    for child in children {
        match child {
            Md::Text(text) => out.push_str(&text.value),
            Md::InlineCode(code) => out.push_str(&code.value),
            Md::Break(_) => out.push('\n'),
            Md::Html(html) => {
                let tag = html.value.trim_start_matches("</").trim_start_matches('<');
                let ok = ["strong>", "em>", "u>", "s>", "mark>"].contains(&tag)
                    || tag == format!("mark color=\"{MARK}\">");
                assert!(ok, "unexpected html {:?}", html.value);
            }
            Md::Link(link) => {
                if let Some(index) = link.url.strip_prefix(OBJECT_SCHEME) {
                    let index: usize = index.parse().expect("object index");
                    assert!(index < objects.len(), "object {index} of {}", objects.len());
                }
                inline_text(&link.children, objects, out);
            }
            other => panic!("unexpected inline {other:?}"),
        }
    }
}

fn round_trip(lowered: &Lowered) -> String {
    lowered
        .segments
        .iter()
        .map(|s| visible(&s.source, &lowered.objects))
        .collect::<Vec<_>>()
        .join("\n")
        // The space `hard_break` puts after an object.
        .replace(" \n", "\n")
}

#[test]
fn break_after_an_object_gets_a_space() {
    assert_eq!(source("<@1>\nnext"), "[\\@Alice](fluxer-md:0)&#32;\\\nnext");
    assert_eq!(source("**<@1>**\nnext"), "<strong>[\\@Alice](fluxer-md:0)</strong>&#32;\\\nnext");
    assert_eq!(source("||a b||\nnext"), "[a ](fluxer-md:0)[b](fluxer-md:1)&#32;\\\nnext");
    assert_eq!(source("text\nnext"), "text\\\nnext");
}

#[test]
fn user_punctuation_never_becomes_markup() {
    let inputs = [
        "\\*not em\\* \\_nor\\_ \\~\\~strike\\~\\~ <b>html</b> &amp; $x$ [^1] ![img](u)",
        "\\# not a heading",
        "1\\. not a list",
        "\\> not a quote",
        "\\- dash",
        "a | b | c",
        "www.example.com and me@example.com",
        "trailing backslash \\\\",
        "    indented four",
        "line\n    indented continuation\n\ttab",
        "`code with ``ticks`` inside`",
        "``` `",
        "<https://a.b/c?q=<x>>",
        "[label *with* stars](https://x.y/(paren))",
        "**bold (punct)**word and *it.*x",
        "=====\n-----",
    ];
    for input in inputs {
        let d = doc(input);
        assert_eq!(round_trip(&hidden(&d)), d.plain_text(), "{input:?}\nsource: {}", source(input));
    }
}

#[test]
fn every_construct_round_trips() {
    let inputs = [
        "**bold *it* __u__** ~~s~~ `c``d`",
        "hi <@1> in <#10> for <@&20> and @everyone <:blob:42> <a:dance:43>",
        "para\n```rs\nfn x(){}\n```\nafter",
        "# Head\ntext\n\n\nmore",
        "a\n- one\n  - nested\n- two\nb",
        "1. a\n2. b\n   continued",
        "> quote\n> more\nnot",
        "> [!NOTE]\n> para\n>\n> - a",
        "| a | b `x|y` |\n|---|:-:|\n| 1 | **2** |",
        "x ||**b** _i_|| y",
        "||\nblock\n- x\n||",
        "- item\n  ```py\n  print(1)\n  ```\n  after code",
        "```\n````\n```",
        "<t:1795017909:R> <t:1795017909:F>",
    ];
    for input in inputs {
        let d = doc(input);
        for lowered in [hidden(&d), revealed(&d)] {
            assert_eq!(round_trip(&lowered), d.plain_text(), "{input:?}\nsource: {:?}", lowered.segments);
        }
    }
}

#[test]
fn styles_are_inline_tags() {
    assert_eq!(
        source("**bold *it* __u__** ~~s~~"),
        "<strong>bold </strong><strong><em>it</em></strong><strong> </strong><strong><u>u</u></strong> <s>s</s>"
    );
}

#[test]
fn text_is_escaped_and_line_starts_kept() {
    // Fluxer's parser takes `*b*` as emphasis even inside a word.
    assert_eq!(source("a*b* # <x> &"), "a<em>b</em> \\# \\<x\\> \\&");
    assert_eq!(source("\\*a\\* 1\\. x"), "\\*a\\* 1\\. x");
    assert_eq!(source("a\n  b\n\nc"), "a\\\n&#32;&#32;b\\\n\\\nc");
}

#[test]
fn objects_are_placeholder_links() {
    let lowered = hidden(&doc("hi <@1> <:blob:42>"));
    assert_eq!(lowered.segments[0].source, "hi [\\@Alice](fluxer-md:0) [\\<\\:blob\\:42\\>](fluxer-md:1)");
    assert!(matches!(&lowered.objects[0], InlineObject::Mention(m) if m.label == "@Alice"));
    assert!(matches!(&lowered.objects[1], InlineObject::Emoji(e) if e.copy_text == "<:blob:42>"));
}

#[test]
fn hidden_spoilers_are_words_revealed_ones_marked() {
    let d = doc("a ||two words|| b");
    let lowered = hidden(&d);
    assert_eq!(lowered.segments[0].source, "a [two ](fluxer-md:0)[words](fluxer-md:1) b");
    assert_eq!(
        lowered.objects,
        [
            InlineObject::HiddenSpoiler { spoiler: 0, text: "two ".to_owned(), start: true, end: false },
            InlineObject::HiddenSpoiler { spoiler: 0, text: "words".to_owned(), start: false, end: true },
        ]
    );
    let shown = revealed(&d);
    assert_eq!(shown.segments[0].source, format!("a <mark color=\"{MARK}\">two words</mark> b"));
    assert!(shown.objects.is_empty());
    // Only the opened spoiler shows.
    let d = doc("||one|| ||two||");
    let one_open = write(&d, &|id| id == 0, MARK);
    assert_eq!(one_open.segments[0].source, format!("<mark color=\"{MARK}\">one</mark> [two](fluxer-md:0)"));
}

#[test]
fn links_and_code() {
    assert_eq!(source("[docs](https://x.y/a b)"), "[docs](<https://x.y/a b>)");
    assert_eq!(source("`a``b`"), "```a``b```");
    // Fluxer keeps the inner spaces; padding one more on each side keeps
    // them through CommonMark's one-space strip.
    assert_eq!(source("`` `x` ``"), "``  `x`  ``");
    assert_eq!(source("```rs\nlet a = 1;\n```"), "```rs\nlet a = 1;\n```");
    assert_eq!(source("```\n```inner\n```"), "````\n```inner\n````");
}

#[test]
fn blocks_have_prefixes() {
    assert_eq!(source("> a\n> b"), "> a\\\n> b");
    assert_eq!(source("- a\n  - b\n- c"), "- a\n  - b\n- c");
    assert_eq!(source("3. x\n4. y"), "3. x\n4. y");
    assert_eq!(source("> [!TIP]\n> hi"), "> <strong>Tip</strong>\n>\n> hi");
    assert_eq!(source("| a | b |\n|:-|-:|\n| 1 | 2 |"), "| a | b |\n| :-- | --: |\n| 1 | 2 |");
    assert_eq!(source("# Title\nbody"), "# Title\n\nbody");
}

#[test]
fn top_level_subtext_is_its_own_segment() {
    let lowered = hidden(&doc("before\n-# small *print*\nafter"));
    let segments: Vec<(bool, &str)> = lowered.segments.iter().map(|s| (s.subtext, s.source.as_str())).collect();
    assert_eq!(segments, [(false, "before"), (true, "small <em>print</em>"), (false, "after")]);
}
