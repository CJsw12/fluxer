//! Emoji picker popover (`EmojiPicker.tsx`, `EmojiPickerPopout.tsx`):
//! sections (frequently used, guild custom, unicode categories), search,
//! skin-tone selector and 9-column grid for composer insertion or reactions.

use std::sync::Arc;

use fluxer_client_core::search::emoji::{EmojiChoice, normalize_picker_query, picker_search_choices};
use fluxer_client_core::state::Snowflake;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{
    AppContext as _, Context, Entity, EntityInputHandler as _, Focusable as _, Pixels, Point, ScrollStrategy,
    Subscription, UniformListScrollHandle, Window, point, px,
};

use crate::accounts::now_ms;
use crate::client_view::ClientView;
use crate::ui::composer_text;
use crate::ui::l10n;
use crate::ui::overlay::Overlay;

pub mod sections;
mod view;

use sections::{PickerSection, PickerTarget, build_sections, chunk_rows};
pub use view::render;

/// Key context of the emoji picker popover.
pub const CONTEXT: &str = "EmojiPicker";

/// Active top tab inside the composer expression picker (`ExpressionPickerPopout`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExpressionTab {
    Media,
    Stickers,
    #[default]
    Emojis,
}

/// Computes visible emoji grid column count from panel width (`48px` column track).
pub fn columns_for_width(width: f32) -> usize {
    ((width - 64.0) / 48.0).floor().clamp(5.0, 24.0) as usize
}

pub struct EmojiPicker {
    pub target: PickerTarget,
    pub active_tab: ExpressionTab,
    pub media_filter: &'static str,
    pub anchor: Point<Pixels>,
    pub input: Entity<InputState>,
    /// 0 = default, 1..=5 = skin tone index.
    pub tone: usize,
    pub tone_menu_open: bool,
    pub sections: Vec<PickerSection>,
    pub active_section: usize,
    pub search_results: Option<Vec<EmojiChoice>>,
    /// Cached `columns`-wide rows for the active view (used read-only by `uniform_list`).
    pub rows: Arc<Vec<Vec<EmojiChoice>>>,
    pub columns: usize,
    pub selected_index: usize,
    pub hovered: Option<EmojiChoice>,
    pub scroll: UniformListScrollHandle,
    pub _subscription: Subscription,
}

impl EmojiPicker {
    pub fn active_choices(&self) -> &[EmojiChoice] {
        match &self.search_results {
            Some(results) => results,
            None => self.sections.get(self.active_section).map_or(&[], |s| s.choices.as_slice()),
        }
    }

    pub fn active_title(&self) -> Option<&str> {
        self.search_results
            .is_none()
            .then(|| self.sections.get(self.active_section).map(|s| s.title.as_str()))
            .flatten()
    }

    pub fn rebuild_rows(&mut self) {
        let cols = self.columns.max(1);
        let rows = chunk_rows(self.active_choices(), cols);
        self.rows = Arc::new(rows);
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
    }

    pub fn set_width(&mut self, width: f32) {
        let next = columns_for_width(width);
        if next != self.columns {
            self.columns = next;
            self.rebuild_rows();
        }
    }

    pub fn selected_choice(&self) -> Option<&EmojiChoice> {
        self.active_choices().get(self.selected_index)
    }

    pub fn inspected(&self) -> Option<&EmojiChoice> {
        self.hovered.as_ref().or_else(|| self.selected_choice())
    }

    pub fn move_selection(&mut self, dr: i32, dc: i32) {
        let total = self.active_choices().len();
        let cols = self.columns.max(1);
        self.selected_index = sections::move_grid(self.selected_index, total, cols, dr, dc);
        if total > 0 {
            self.scroll.scroll_to_item(self.selected_index / cols, ScrollStrategy::Nearest);
        }
    }

    pub fn sync_placeholder(&self, window: &mut Window, cx: &mut gpui_kit::App) {
        let placeholder = match self.active_tab {
            ExpressionTab::Media => l10n::tr("Search media"),
            ExpressionTab::Stickers => l10n::tr("Search stickers"),
            ExpressionTab::Emojis => self
                .inspected()
                .map(EmojiChoice::label)
                .unwrap_or_else(|| l10n::tr("Search emoji")),
        };
        self.input.update(cx, |s, cx| {
            if s.presentation().placeholder().as_ref() != placeholder {
                s.set_placeholder(placeholder, window, cx);
            }
        });
    }
}

impl ClientView {
    pub(crate) fn toggle_emoji_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.overlay, Overlay::EmojiPicker(_)) {
            self.close_overlay(window, cx);
        } else {
            let bounds = window.bounds();
            let x = (bounds.size.width - px(270.)).max(px(416.));
            let y = (bounds.size.height - px(70.)).max(px(404.));
            self.open_emoji_picker(PickerTarget::Composer, point(x, y), window, cx);
        }
    }

    pub(crate) fn toggle_expression_tab(
        &mut self,
        tab: ExpressionTab,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dismissed = self.dismissed_expression_tab.take();
        if matches!(
            &self.overlay,
            Overlay::EmojiPicker(picker)
                if picker.target == PickerTarget::Composer && picker.active_tab == tab
        ) || dismissed == Some((PickerTarget::Composer, tab))
        {
            self.close_overlay(window, cx);
            return;
        }
        if matches!(&self.overlay, Overlay::EmojiPicker(picker) if picker.target == PickerTarget::Composer) {
            self.expression_picker_select_tab(tab, window, cx);
            return;
        }
        // Official ResizablePane: right edge 32px past the clicked toggle's centre
        // (toggle right edge + 16), bottom 8px above the toggle; anchored to the trigger rect.
        let trigger = match tab {
            ExpressionTab::Media => "composer-gif-btn",
            ExpressionTab::Stickers => "composer-sticker-btn",
            ExpressionTab::Emojis => "composer-emoji-btn",
        };
        let anchor = crate::ui::channel_popouts::picker_anchor(trigger, anchor);
        self.open_expression_picker(PickerTarget::Composer, tab, anchor, window, cx);
    }

    pub(crate) fn open_reaction_emoji_picker(
        &mut self,
        channel_id: Snowflake,
        message_id: Snowflake,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.reaction_picker = None;
        self.open_emoji_picker(PickerTarget::Reaction { channel_id, message_id }, anchor, window, cx);
    }

    pub(crate) fn open_emoji_picker(
        &mut self,
        target: PickerTarget,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_expression_picker(target, ExpressionTab::Emojis, anchor, window, cx);
    }

    pub(crate) fn open_expression_picker(
        &mut self,
        target: PickerTarget,
        tab: ExpressionTab,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        let active_tab = if target == PickerTarget::Composer { tab } else { ExpressionTab::Emojis };
        let placeholder = match active_tab {
            ExpressionTab::Media => l10n::tr("Search media"),
            ExpressionTab::Stickers => l10n::tr("Search stickers"),
            ExpressionTab::Emojis => l10n::tr("Search emoji"),
        };
        let sections = {
            let state = self.backend.state();
            build_sections(&state, self.selected_channel, &self.frecency.data, now_ms())
        };
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let subscription = cx.subscribe_in(&input, window, |this: &mut Self, _, ev: &InputEvent, _, cx| {
            if let InputEvent::Change = ev {
                this.emoji_picker_query_changed(cx);
            }
        });
        let mut picker = EmojiPicker {
            target,
            active_tab,
            media_filter: "All",
            anchor,
            input: input.clone(),
            tone: 0,
            tone_menu_open: false,
            sections,
            active_section: 0,
            search_results: None,
            rows: Arc::new(Vec::new()),
            columns: columns_for_width(self.emoji_picker_width),
            selected_index: 0,
            hovered: None,
            scroll: UniformListScrollHandle::new(),
            _subscription: subscription,
        };
        picker.rebuild_rows();
        self.overlay = Overlay::EmojiPicker(picker);
        let focus = input.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(crate) fn expression_picker_select_tab(
        &mut self,
        tab: ExpressionTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = {
            let Overlay::EmojiPicker(picker) = &mut self.overlay else { return };
            if picker.target != PickerTarget::Composer {
                return;
            }
            if picker.active_tab != tab {
                picker.active_tab = tab;
                picker.tone_menu_open = false;
                picker.hovered = None;
            }
            picker.sync_placeholder(window, cx);
            picker.input.read(cx).focus_handle(cx)
        };
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(crate) fn expression_picker_set_media_filter(&mut self, filter: &'static str, cx: &mut Context<Self>) {
        if let Overlay::EmojiPicker(picker) = &mut self.overlay
            && picker.media_filter != filter
        {
            picker.media_filter = filter;
            cx.notify();
        }
    }

    fn emoji_picker_composing(&self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Overlay::EmojiPicker(picker) = &self.overlay else { return false };
        picker.input.update(cx, |s, cx| s.marked_text_range(window, cx)).is_some()
    }

    fn emoji_picker_query_changed(&mut self, cx: &mut Context<Self>) {
        let query = {
            let Overlay::EmojiPicker(picker) = &self.overlay else { return };
            if picker.active_tab != ExpressionTab::Emojis {
                cx.notify();
                return;
            }
            picker.input.read(cx).value().to_string()
        };
        let results = if normalize_picker_query(&query).is_empty() {
            None
        } else {
            let state = self.backend.state();
            Some(picker_search_choices(&state, self.selected_channel, &query, &self.frecency.data, now_ms()))
        };
        if let Overlay::EmojiPicker(picker) = &mut self.overlay {
            picker.search_results = results;
            picker.selected_index = 0;
            picker.hovered = None;
            picker.rebuild_rows();
            cx.notify();
        }
    }

    pub(crate) fn emoji_picker_move(&mut self, dr: i32, dc: i32, window: &mut Window, cx: &mut Context<Self>) {
        if self.emoji_picker_composing(window, cx) {
            return;
        }
        if let Overlay::EmojiPicker(picker) = &mut self.overlay {
            if picker.active_tab != ExpressionTab::Emojis {
                return;
            }
            picker.move_selection(dr, dc);
            picker.sync_placeholder(window, cx);
            cx.notify();
        }
    }

    pub(crate) fn emoji_picker_select_section(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Overlay::EmojiPicker(picker) = &mut self.overlay
            && ix < picker.sections.len()
        {
            picker.search_results = None;
            picker.active_section = ix;
            picker.selected_index = 0;
            picker.hovered = None;
            picker.rebuild_rows();
            cx.notify();
        }
    }

    pub(crate) fn emoji_picker_toggle_tone_menu(&mut self, cx: &mut Context<Self>) {
        if let Overlay::EmojiPicker(picker) = &mut self.overlay {
            picker.tone_menu_open = !picker.tone_menu_open;
            cx.notify();
        }
    }

    pub(crate) fn emoji_picker_set_tone(&mut self, tone: usize, cx: &mut Context<Self>) {
        if let Overlay::EmojiPicker(picker) = &mut self.overlay {
            picker.tone = tone.min(sections::SKIN_TONES.len() - 1);
            picker.tone_menu_open = false;
            cx.notify();
        }
    }

    pub(crate) fn emoji_picker_hover(&mut self, choice: EmojiChoice, window: &mut Window, cx: &mut Context<Self>) {
        if let Overlay::EmojiPicker(picker) = &mut self.overlay
            && picker.hovered.as_ref() != Some(&choice)
        {
            picker.hovered = Some(choice);
            picker.sync_placeholder(window, cx);
            cx.notify();
        }
    }

    pub(crate) fn emoji_picker_accept_selected(&mut self, keep_open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.emoji_picker_composing(window, cx) {
            return;
        }
        let choice = {
            let Overlay::EmojiPicker(picker) = &self.overlay else { return };
            if picker.active_tab != ExpressionTab::Emojis {
                return;
            }
            picker.selected_choice().cloned()
        };
        if let Some(choice) = choice {
            self.emoji_picker_pick(choice, keep_open, window, cx);
        }
    }

    pub(crate) fn emoji_picker_pick(
        &mut self,
        choice: EmojiChoice,
        keep_open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (target, tone) = {
            let Overlay::EmojiPicker(picker) = &self.overlay else { return };
            (picker.target, picker.tone)
        };
        self.frecency.record(&choice.frecency_key, now_ms());
        match target {
            PickerTarget::Composer => {
                let insert = if choice.custom_id.is_some() {
                    composer_text::Insert::Token {
                        display: format!(":{}:", choice.name),
                        wire: choice.insert_text(tone),
                    }
                } else {
                    composer_text::Insert::Text(choice.insert_text(tone))
                };
                composer_text::insert_at_cursor(&self.composer, &insert, window, cx);
                self.update_autocomplete(cx);
            }
            PickerTarget::Reaction { channel_id, message_id } => {
                self.toggle_reaction(channel_id, message_id, choice.partial_emoji(tone), cx);
            }
        }
        if !keep_open {
            self.close_overlay(window, cx);
        } else {
            cx.notify();
        }
    }
}
