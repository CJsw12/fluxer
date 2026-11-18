//! Text access on the composer's `InputState` for autocomplete: the text up
//! to the cursor, replacing a byte range, and the wire text of the input.
//!
//! Mentions, channels and custom emoji are inline tokens: the input shows
//! `@name` / `#name` / `:name:` and keeps the wire form (`<@id>`, `<#id>`,
//! `<:name:id>`) as the token id, like the web composer's segments
//! (`ComposerSerialization.ts` display vs wire). [`wire_text`] produces what
//! is sent.

use std::ops::Range;

use gpui_kit::component::input::{InlineToken, InputState};
use gpui_kit::{App, Entity, Window};

/// What one autocomplete pick puts into the input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Insert {
    /// Plain text (`@everyone`, unicode emoji).
    Text(String),
    /// Shown as `display`, sent as `wire`.
    Token { display: String, wire: String },
}

/// The input's text up to the cursor (empty if the cursor is not on a char
/// boundary).
pub fn text_before_cursor(state: &InputState) -> String {
    let value = state.value();
    value.get(..state.cursor()).unwrap_or_default().to_owned()
}

/// The space after a pick: one unless the text after it already starts
/// with whitespace (`trailing: charAfter === '' || !/\s/.test(charAfter)`).
pub fn trailing_space(after: &str) -> &'static str {
    if after.chars().next().is_some_and(char::is_whitespace) { "" } else { " " }
}

/// Replaces `range` (UTF-8 bytes of the current text) with `insert` and a
/// trailing space (`createComposerReplacementPlan` adds one for every
/// mention and emoji payload) unless whitespace already follows; the
/// cursor ends after the insert.
pub fn replace_range(input: &Entity<InputState>, range: Range<usize>, insert: &Insert, window: &mut Window, cx: &mut App) {
    input.update(cx, |s, cx| {
        let space = trailing_space(s.value().get(range.end..).unwrap_or_default());
        match insert {
            Insert::Text(text) => {
                s.set_selected_range(range, cx);
                s.replace(format!("{text}{space}"), window, cx);
            }
            Insert::Token { display, wire } => {
                let token = InlineToken::new(wire.clone(), display.clone());
                match s.replace_range_with_token(range.clone(), token, window, cx) {
                    Ok(()) => {
                        let after = range.start + display.len();
                        s.set_selected_range(after..after, cx);
                        if !space.is_empty() {
                            s.insert(space, window, cx);
                        }
                    }
                    Err(err) => {
                        // The text is unchanged; insert the wire form instead
                        // so the pick is not lost.
                        tracing::warn!(?err, "composer token rejected");
                        s.set_selected_range(range, cx);
                        s.replace(format!("{wire}{space}"), window, cx);
                    }
                }
            }
        }
    });
}

/// `text` with every token range replaced by its wire form. `tokens` are
/// (byte range, wire) in any order; ranges outside `text` are ignored.
pub fn to_wire(text: &str, tokens: &[(Range<usize>, &str)]) -> String {
    let mut sorted: Vec<&(Range<usize>, &str)> = tokens.iter().collect();
    sorted.sort_by_key(|(r, _)| r.start);
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for (range, wire) in sorted {
        let (Some(before), Some(_)) = (text.get(at..range.start), text.get(range.clone())) else { continue };
        out.push_str(before);
        out.push_str(wire);
        at = range.end;
    }
    out.push_str(text.get(at..).unwrap_or_default());
    out
}

/// The text to send: the input value with its tokens in wire form.
pub fn wire_text(state: &InputState) -> String {
    let content = state.content();
    let tokens: Vec<(Range<usize>, &str)> =
        content.tokens().iter().map(|span| (span.range(), span.token().id().as_ref())).collect();
    to_wire(content.text(), &tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_replaces_tokens_in_order() {
        let text = "hi @ann and #general :wave: ok";
        let ann = 3..7;
        let general = 12..20;
        let wave = 21..27;
        assert_eq!(&text[ann.clone()], "@ann");
        assert_eq!(&text[general.clone()], "#general");
        assert_eq!(&text[wave.clone()], ":wave:");
        let tokens = [(general, "<#11>"), (ann, "<@5>"), (wave, "<:wave:9>")];
        assert_eq!(to_wire(text, &tokens), "hi <@5> and <#11> <:wave:9> ok");
        assert_eq!(to_wire("plain", &[]), "plain");
        assert_eq!(to_wire("ab", &[(5..9, "x")]), "ab", "out of range ignored");
    }

    #[test]
    fn no_second_space_before_whitespace() {
        assert_eq!(trailing_space(""), " ");
        assert_eq!(trailing_space("abc"), " ");
        assert_eq!(trailing_space(" rest"), "");
        assert_eq!(trailing_space("\nnext"), "");
    }
}
