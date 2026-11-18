//! Quick switcher (Ctrl+K): a search box over channels, DMs, people and
//! communities (`QuickSwitcherModal.tsx`). Ranking is core's
//! `search::switcher`; this file holds the modal state, the keyboard
//! selection and the view.

use fluxer_client_core::search::switcher::{QueryMode, SwitcherHeader, SwitcherItem, SwitcherTarget};
use gpui_kit::component::input::{Enter, Escape, Input, InputState, MoveDown, MoveUp};
use gpui_kit::{AnyElement, Context, Entity, FontWeight, ScrollHandle, Subscription, div, prelude::*, px};

use crate::client_view::ClientView;
use crate::shortcuts::{SwitcherNext, SwitcherPrev};
use crate::theme::Theme;
use crate::ui::l10n;
use crate::ui::overlay;

/// Key context of the modal (Ctrl+N / Ctrl+P bindings).
pub const CONTEXT: &str = "QuickSwitcher";

pub struct QuickSwitcher {
    pub input: Entity<InputState>,
    pub items: Vec<SwitcherItem>,
    /// Index into `items`; never a header.
    pub selected: Option<usize>,
    pub scroll: ScrollHandle,
    pub _subscription: Subscription,
}

impl QuickSwitcher {
    /// New results for the query: the first result is selected.
    pub fn set_items(&mut self, items: Vec<SwitcherItem>) {
        self.selected = first_selectable(&items);
        self.items = items;
        self.scroll_to_selected();
    }

    pub fn move_selection(&mut self, delta: i32) {
        self.selected = next_selectable(&self.items, self.selected, delta);
        self.scroll_to_selected();
    }

    pub fn selected_target(&self) -> Option<&SwitcherTarget> {
        match self.items.get(self.selected?)? {
            SwitcherItem::Result(r) => Some(&r.target),
            SwitcherItem::Header(_) => None,
        }
    }

    fn scroll_to_selected(&self) {
        if let Some(ix) = self.selected {
            self.scroll.scroll_to_item(ix);
        }
    }
}

fn is_result(item: &SwitcherItem) -> bool {
    matches!(item, SwitcherItem::Result(_))
}

pub fn first_selectable(items: &[SwitcherItem]) -> Option<usize> {
    items.iter().position(is_result)
}

/// `findNextSelectableIndex`: step with wrap-around, skipping headers; the
/// current index when nothing else is selectable.
pub fn next_selectable(items: &[SwitcherItem], current: Option<usize>, delta: i32) -> Option<usize> {
    let len = i64::try_from(items.len()).ok().filter(|l| *l > 0)?;
    let step = if delta < 0 { -1 } else { 1 };
    let mut index = current.and_then(|c| i64::try_from(c).ok()).unwrap_or(if step > 0 { -1 } else { len });
    for _ in 0..len {
        index = (index + step).rem_euclid(len);
        let ix = usize::try_from(index).ok()?;
        if items.get(ix).is_some_and(is_result) {
            return Some(ix);
        }
    }
    current
}

fn header_text(header: SwitcherHeader) -> String {
    match header {
        SwitcherHeader::Previous => l10n::tr("Previous channels"),
        SwitcherHeader::Mentions => l10n::tr("Mentions"),
        SwitcherHeader::Unread => l10n::tr("Unread channels"),
        SwitcherHeader::Searching(QueryMode::Users { .. }) => l10n::tr("Searching all users"),
        SwitcherHeader::Searching(QueryMode::TextChannels) => l10n::tr("Searching text channels"),
        SwitcherHeader::Searching(QueryMode::VoiceChannels) => l10n::tr("Searching voice channels"),
        SwitcherHeader::Searching(QueryMode::Guilds) => l10n::tr("Searching communities"),
    }
}

fn glyph(target: &SwitcherTarget) -> &'static str {
    match target {
        SwitcherTarget::User { .. } | SwitcherTarget::GroupDm { .. } => "@",
        SwitcherTarget::TextChannel { .. } => "#",
        SwitcherTarget::VoiceChannel { .. } => "🔊",
        SwitcherTarget::Guild { .. } => "*",
    }
}

/// The modal: search box, then headers and results. Up/Down (and Ctrl+N /
/// Ctrl+P) move, Enter opens, Esc or a press outside closes, hover selects.
pub fn render(sw: &QuickSwitcher, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let mut list = div().id("quick-switcher-results").flex().flex_col().max_h(px(420.)).overflow_y_scroll().pb_2();
    list = list.track_scroll(&sw.scroll);
    for (ix, item) in sw.items.iter().enumerate() {
        let row = match item {
            SwitcherItem::Header(h) => div()
                .px_4()
                .pt_3()
                .pb_1()
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(theme.muted_foreground)
                .child(header_text(*h))
                .into_any_element(),
            SwitcherItem::Result(r) => div()
                .id(("quick-switcher-row", ix))
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .mx_2()
                .px_2()
                .py_1()
                .rounded(px(4.))
                .when(sw.selected == Some(ix), |d| d.bg(theme.accent))
                .child(div().w(px(20.)).flex_none().text_color(theme.muted_foreground).child(glyph(&r.target)))
                .child(div().flex_1().min_w_0().truncate().child(r.title.clone()))
                .children(
                    r.subtitle.clone().map(|s| div().text_xs().truncate().text_color(theme.muted_foreground).child(s)),
                )
                .on_mouse_move(cx.listener(move |this, _, _, cx| this.quick_switcher_hover(ix, cx)))
                .on_click(cx.listener(move |this, _, window, cx| this.quick_switcher_accept(Some(ix), window, cx)))
                .into_any_element(),
        };
        list = list.child(row);
    }
    let empty = sw.items.is_empty().then(|| {
        div().px_4().py_3().text_color(theme.muted_foreground).child(l10n::tr("No results"))
    });
    let content = div()
        .key_context(CONTEXT)
        .flex()
        .flex_col()
        .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
            this.quick_switcher_move(-1, cx);
            cx.stop_propagation();
        }))
        .capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
            this.quick_switcher_move(1, cx);
            cx.stop_propagation();
        }))
        .on_action(cx.listener(|this, _: &SwitcherPrev, _, cx| this.quick_switcher_move(-1, cx)))
        .on_action(cx.listener(|this, _: &SwitcherNext, _, cx| this.quick_switcher_move(1, cx)))
        .capture_action(cx.listener(|this, _: &Enter, window, cx| {
            cx.stop_propagation();
            this.quick_switcher_accept(None, window, cx);
        }))
        .capture_action(cx.listener(|this, _: &Escape, window, cx| {
            cx.stop_propagation();
            this.close_quick_switcher(window, cx);
        }))
        .child(div().p_3().child(Input::new(&sw.input)))
        .children(empty)
        .child(list);
    let view = cx.weak_entity();
    overlay::modal(content, theme, move |window, cx| {
        let _ = view.update(cx, |v, cx| v.close_quick_switcher(window, cx));
    })
}

#[cfg(test)]
mod tests {
    use fluxer_client_core::search::switcher::SwitcherResult;
    use fluxer_client_core::state::Snowflake;

    use super::*;

    fn result(n: u64) -> SwitcherItem {
        SwitcherItem::Result(SwitcherResult {
            target: SwitcherTarget::Guild { guild_id: Snowflake(n) },
            title: n.to_string(),
            subtitle: None,
            score: 0,
            matched_text: String::new(),
            sort_weight: 0,
        })
    }

    #[test]
    fn selection_skips_headers_and_wraps() {
        let items = vec![
            SwitcherItem::Header(SwitcherHeader::Previous),
            result(1),
            SwitcherItem::Header(SwitcherHeader::Unread),
            result(2),
        ];
        assert_eq!(first_selectable(&items), Some(1));
        assert_eq!(next_selectable(&items, Some(1), 1), Some(3));
        assert_eq!(next_selectable(&items, Some(3), 1), Some(1), "wraps past the end");
        assert_eq!(next_selectable(&items, Some(1), -1), Some(3), "wraps before the start");
        assert_eq!(next_selectable(&items, None, -1), Some(3));
        assert_eq!(next_selectable(&[], None, 1), None);
        let only_headers = vec![SwitcherItem::Header(SwitcherHeader::Mentions)];
        assert_eq!(first_selectable(&only_headers), None);
        assert_eq!(next_selectable(&only_headers, None, 1), None);
    }
}
