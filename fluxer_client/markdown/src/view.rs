//! GPUI rendering of a [`Document`] through gpui-kit's `TextView`.
//!
//! The document is written as `TextView` Markdown (see `source.rs`); a
//! `MarkdownPlugin` draws mentions, emoji, timestamps and hidden spoiler
//! words as inline elements. `TextView` brings the window-scoped text
//! selection (drag across messages, copy) and syntax-highlighted code
//! blocks (gpui-component's syntect highlighter, installed with the theme).

use std::collections::HashSet;
use std::sync::Arc;

use gpui_kit::component::text::{
    InlineElement, InlineRenderContext, MarkdownExtensions, MarkdownNode, MarkdownParseContext, MarkdownPlugin,
    TextView, markdown_ast,
};
use gpui_kit::{
    App, ElementId, Entity, FontWeight, Hsla, IntoElement, ObjectFit, Pixels, RenderOnce, SharedString, Window, div,
    hsla, img, prelude::*, px, rgb,
};

use gpui_kit::base::TextSelection;

use crate::model::{Document, MentionTarget, SpoilerId};
use crate::source::{InlineObject, OBJECT_SCHEME, write};

/// Size of a jumbo emoji (`3rem`).
const JUMBO_EMOJI_SIZE: Pixels = px(48.);

/// Name of the inline-object Markdown node.
const OBJECT_NODE: &str = "fluxer-md-object";

/// Spoilers the viewer opened in one message.
#[derive(Debug, Default)]
pub struct SpoilerReveals {
    revealed: HashSet<SpoilerId>,
}

impl SpoilerReveals {
    pub fn is_revealed(&self, id: SpoilerId) -> bool {
        self.revealed.contains(&id)
    }

    pub fn reveal(&mut self, id: SpoilerId) {
        self.revealed.insert(id);
    }
}

/// Colors of the Fluxer-specific inline elements.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MarkdownStyle {
    pub mention_background: Hsla,
    pub mention_foreground: Hsla,
    /// Cover of a hidden spoiler.
    pub spoiler_hidden: Hsla,
    /// Background behind a revealed inline spoiler.
    pub spoiler_revealed: Hsla,
    pub timestamp_background: Hsla,
    pub subtext: Hsla,
}

impl Default for MarkdownStyle {
    /// Fluxer's dark theme (`Markup.module.css` mention / spoiler tokens).
    fn default() -> Self {
        Self {
            mention_background: hsla(235. / 360., 0.86, 0.65, 0.3),
            mention_foreground: hsla(235. / 360., 0.86, 0.85, 1.),
            spoiler_hidden: hsla(220. / 360., 0.08, 0.12, 1.),
            spoiler_revealed: hsla(0., 0., 0.5, 0.15),
            timestamp_background: hsla(0., 0., 0.5, 0.12),
            subtext: hsla(220. / 360., 0.05, 0.6, 1.),
        }
    }
}

/// Called with a clicked mention whose target is known.
pub type MentionClickHandler = Arc<dyn Fn(&MentionTarget, &mut Window, &mut App) + Send + Sync>;
/// Called with the URL of a clicked link.
pub type LinkClickHandler = Arc<dyn Fn(&SharedString, &mut Window, &mut App) + Send + Sync>;

/// A message body: selectable rich text with Fluxer's inline elements.
#[derive(IntoElement)]
pub struct MessageMarkdown {
    id: ElementId,
    document: Arc<Document>,
    style: MarkdownStyle,
    on_link_click: Option<LinkClickHandler>,
    on_mention_click: Option<MentionClickHandler>,
    spoilers: Option<Entity<SpoilerReveals>>,
}

impl MessageMarkdown {
    /// `id` must be unique among its siblings (e.g. the message id); the
    /// revealed spoilers are kept under it while the element is drawn.
    pub fn new(id: impl Into<ElementId>, document: Arc<Document>) -> Self {
        Self {
            id: id.into(),
            document,
            style: MarkdownStyle::default(),
            on_link_click: None,
            on_mention_click: None,
            spoilers: None,
        }
    }

    pub fn style(mut self, style: MarkdownStyle) -> Self {
        self.style = style;
        self
    }

    /// Without a handler links open with `App::open_url`.
    pub fn on_link_click(mut self, handler: impl Fn(&SharedString, &mut Window, &mut App) + Send + Sync + 'static) -> Self {
        self.on_link_click = Some(Arc::new(handler));
        self
    }

    /// Keeps the revealed spoilers in `state` instead of element state, so
    /// they survive the message scrolling out of a virtual list.
    pub fn spoiler_state(mut self, state: Entity<SpoilerReveals>) -> Self {
        self.spoilers = Some(state);
        self
    }

    pub fn on_mention_click(
        mut self,
        handler: impl Fn(&MentionTarget, &mut Window, &mut App) + Send + Sync + 'static,
    ) -> Self {
        self.on_mention_click = Some(Arc::new(handler));
        self
    }
}

impl RenderOnce for MessageMarkdown {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let reveals = match self.spoilers {
            Some(state) => state,
            None => {
                let state_id = ElementId::NamedChild(Arc::new(self.id.clone()), "spoilers".into());
                window.use_keyed_state(state_id, cx, |_, _| SpoilerReveals::default())
            }
        };
        let lowered = {
            let state = reveals.read(cx);
            write(&self.document, &|id| state.is_revealed(id), &hex_color(self.style.spoiler_revealed))
        };
        let plugin = ObjectPlugin {
            objects: Arc::new(lowered.objects),
            reveals,
            style: self.style,
            jumbo: self.document.jumbo,
            on_mention_click: self.on_mention_click,
        };
        let extensions = MarkdownExtensions::default().plugin(plugin);
        let subtext = self.style.subtext;
        let on_link_click = self.on_link_click;
        div().id(self.id).flex().flex_col().min_w_0().children(lowered.segments.into_iter().enumerate().map(
            move |(i, segment)| {
                let mut view = TextView::markdown(ElementId::named_usize("fluxer-md-segment", i), segment.source)
                    .markdown_extensions(extensions.clone())
                    .selectable(true);
                if let Some(handler) = on_link_click.clone() {
                    view = view.on_link_click(move |url, _, window, cx| handler(url, window, cx));
                }
                if segment.subtext {
                    div().text_xs().text_color(subtext).child(view).into_any_element()
                } else {
                    view.into_any_element()
                }
            },
        ))
    }
}

/// `#rrggbbaa`, the form `<mark color>` takes.
fn hex_color(color: Hsla) -> String {
    let rgba = color.to_rgb();
    let byte = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
    format!("#{:02x}{:02x}{:02x}{:02x}", byte(rgba.r), byte(rgba.g), byte(rgba.b), byte(rgba.a))
}

/// Draws the `fluxer-md:<index>` links of one message.
struct ObjectPlugin {
    objects: Arc<Vec<InlineObject>>,
    reveals: Entity<SpoilerReveals>,
    style: MarkdownStyle,
    jumbo: bool,
    on_mention_click: Option<MentionClickHandler>,
}

impl MarkdownPlugin for ObjectPlugin {
    fn name(&self) -> &str {
        OBJECT_NODE
    }

    fn parse(&self, node: &markdown_ast::Node, _cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let markdown_ast::Node::Link(link) = node else { return None };
        let index: usize = link.url.strip_prefix(OBJECT_SCHEME)?.parse().ok()?;
        Some(MarkdownNode::new(OBJECT_NODE, index).text(node.to_string()))
    }

    fn render_inline(
        &self,
        node: &MarkdownNode,
        context: &InlineRenderContext,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<InlineElement> {
        let index = *node.data::<usize>()?;
        let object = self.objects.get(index)?;
        let font_size = context.font_size();
        let line_height = context.line_height();
        let element = match object {
            InlineObject::Mention(mention) => {
                let (background, foreground) = match mention.color {
                    Some(color) => {
                        let color: Hsla = rgb(color).into();
                        (color.opacity(0.1), color)
                    }
                    None => (self.style.mention_background, self.style.mention_foreground),
                };
                let label = match mention.target {
                    MentionTarget::Channel(_) => format!("#{}", mention.label),
                    _ => mention.label.clone(),
                };
                let mut pill = div()
                    .id(ElementId::named_usize("fluxer-md-mention", index))
                    .px(px(2.))
                    .rounded(px(3.))
                    .bg(background)
                    .text_color(foreground)
                    .font_weight(FontWeight::MEDIUM)
                    .text_size(font_size)
                    .line_height(line_height)
                    .child(label);
                if let Some(handler) = self.on_mention_click.clone().filter(|_| mention.resolved) {
                    let target = mention.target.clone();
                    pill = pill.cursor_pointer().on_click(move |_, window, cx| handler(&target, window, cx));
                }
                pill.into_any_element()
            }
            InlineObject::Emoji(emoji) => {
                // `.emoji` is 1.375em; jumbo emoji 3rem.
                let size = if self.jumbo { JUMBO_EMOJI_SIZE } else { font_size * 1.375 };
                match &emoji.url {
                    Some(url) => {
                        let fallback = emoji.text.clone();
                        img(SharedString::from(url.clone()))
                            .size(size)
                            .object_fit(ObjectFit::Contain)
                            .with_fallback(move || div().child(fallback.clone()).into_any_element())
                            .into_any_element()
                    }
                    None => div()
                        .text_size(if self.jumbo { size } else { font_size })
                        .line_height(if self.jumbo { size } else { line_height })
                        .child(emoji.text.clone())
                        .into_any_element(),
                }
            }
            InlineObject::Timestamp(timestamp) => div()
                .px(px(2.))
                .rounded(px(3.))
                .bg(self.style.timestamp_background)
                .text_size(font_size)
                .line_height(line_height)
                .child(timestamp.text.clone())
                .into_any_element(),
            InlineObject::HiddenSpoiler { spoiler, text } => {
                let spoiler = *spoiler;
                let reveals = self.reveals.clone();
                div()
                    .id(ElementId::named_usize("fluxer-md-spoiler", index))
                    .bg(self.style.spoiler_hidden)
                    .text_color(self.style.spoiler_hidden)
                    .text_size(font_size)
                    .line_height(line_height)
                    .cursor_pointer()
                    .on_click(move |_, window, cx| {
                        reveals.update(cx, |state, cx| {
                            state.reveal(spoiler);
                            cx.notify();
                        });
                        // The text is re-laid out: a selection begun by this
                        // click would point into the old layout (and the next
                        // drag would select nothing).
                        TextSelection::clear(window, cx);
                        // An app-owned state is not observed by the view.
                        window.refresh();
                    })
                    .child(text.clone())
                    .into_any_element()
            }
        };
        Some(InlineElement::new(element))
    }
}
