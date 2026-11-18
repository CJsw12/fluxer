//! Emoji picker view: search bar, skin-tone menu, category rail,
//! 9-column `uniform_list` grid and hovered-emoji inspector.

use fluxer_client_core::search::emoji::{EmojiCategory, EmojiChoice};
use gpui_kit::component::input::{Enter, Escape, Input, MoveDown, MoveLeft, MoveRight, MoveUp};
use gpui_kit::{
    AnyElement, Context, FontWeight, ObjectFit, SharedString, div, img, prelude::*, px, uniform_list,
};

use super::sections::{COLUMNS, SKIN_TONES, SectionKind};
use super::{CONTEXT, EmojiPicker};
use crate::client_view::ClientView;
use crate::format::initials;
use crate::theme::Theme;
use crate::ui::{l10n, overlay};

const PANEL_WIDTH: f32 = 500.;
const RAIL_WIDTH: f32 = 46.;
const ROW_HEIGHT: f32 = 48.;
const CELL_SIZE: f32 = 42.;
const GRID_HEIGHT: f32 = 288.;

fn rail_icon(kind: SectionKind, title: &str) -> SharedString {
    match kind {
        SectionKind::FrequentlyUsed => SharedString::new_static("🕒"),
        SectionKind::Guild(_) => SharedString::from(initials(title)),
        SectionKind::Category(cat) => SharedString::new_static(match cat {
            EmojiCategory::People => "😀",
            EmojiCategory::Nature => "🌿",
            EmojiCategory::Food => "🍔",
            EmojiCategory::Activity => "⚽",
            EmojiCategory::Travel => "🚗",
            EmojiCategory::Objects => "💡",
            EmojiCategory::Symbols => "🔣",
            EmojiCategory::Flags => "🚩",
        }),
    }
}

fn render_emoji_cell(
    choice: &EmojiChoice,
    tone: usize,
    selected: bool,
    media: Option<&str>,
    theme: &Theme,
    view: &gpui_kit::WeakEntity<ClientView>,
    ix: usize,
) -> AnyElement {
    let toned = choice.with_tone(tone);
    let content = if let Some(id) = choice.custom_id
        && let Some(base) = media
    {
        let url = format!("{}/emojis/{id}.webp?size=44", base.trim_end_matches('/'));
        let fallback_name = format!(":{}:", choice.name);
        let fallback_color = theme.muted_foreground;
        img(url)
            .id(SharedString::from(format!("picker-custom-emoji-{id}")))
            .size(px(26.))
            .object_fit(ObjectFit::Contain)
            .with_fallback(move || div().text_xs().text_color(fallback_color).child(fallback_name.clone()).into_any_element())
            .into_any_element()
    } else if choice.custom_id.is_some() {
        div().text_xs().truncate().child(format!(":{}:", choice.name)).into_any_element()
    } else {
        div().text_size(px(22.)).child(toned.glyph.clone()).into_any_element()
    };

    let hover_choice = choice.clone();
    let pick_choice = choice.clone();
    let view_hover = view.clone();
    let view_pick = view.clone();
    div()
        .id(("emoji-cell", ix))
        .debug_selector(|| format!("emoji-cell-{}", pick_choice.name))
        .size(px(CELL_SIZE))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .cursor_pointer()
        .when(selected, |d| d.bg(theme.accent))
        .hover(|s| s.bg(theme.muted))
        .on_mouse_move(move |_, window, cx| {
            let _ = view_hover.update(cx, |v, cx| v.emoji_picker_hover(hover_choice.clone(), window, cx));
        })
        .on_click(move |ev, window, cx| {
            let keep_open = ev.modifiers().shift;
            let _ = view_pick.update(cx, |v, cx| v.emoji_picker_pick(pick_choice.clone(), keep_open, window, cx));
        })
        .child(content)
        .into_any_element()
}

pub fn render(picker: &EmojiPicker, media: Option<&str>, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let tone = picker.tone;
    let searching = picker.search_results.is_some();
    let rows = picker.rows.clone();
    let selected_index = picker.selected_index;
    let empty = picker.active_choices().is_empty();
    let weak = cx.weak_entity();
    let theme_copy = *theme;
    let media_owned = media.map(str::to_owned);

    let search_placeholder = picker
        .inspected()
        .map(EmojiChoice::label)
        .unwrap_or_else(|| l10n::tr("Search emoji"));

    let tone_button = div()
        .id("emoji-tone-toggle")
        .w(px(32.))
        .h(px(32.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .border_1()
        .border_color(theme.border)
        .cursor_pointer()
        .hover(|s| s.bg(theme.muted))
        .child(SKIN_TONES[tone])
        .on_click(cx.listener(|this, _, _, cx| this.emoji_picker_toggle_tone_menu(cx)));

    let tone_strip = picker.tone_menu_open.then(|| {
        let mut order: Vec<usize> = vec![tone];
        for i in 0..SKIN_TONES.len() {
            if i != tone {
                order.push(i);
            }
        }
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .px_2()
            .pb_2()
            .children(order.into_iter().map(|i| {
                div()
                    .id(("emoji-tone", i))
                    .px_2()
                    .py_1()
                    .rounded(px(4.))
                    .cursor_pointer()
                    .when(i == tone, |d| d.bg(theme.accent))
                    .hover(|s| s.bg(theme.muted))
                    .child(SKIN_TONES[i])
                    .on_click(cx.listener(move |this, _, _, cx| this.emoji_picker_set_tone(i, cx)))
            }))
    });

    let rail = (!searching).then(|| {
        let buttons = picker.sections.iter().enumerate().map(|(ix, section)| {
            let active = ix == picker.active_section;
            div()
                .id(("emoji-rail", ix))
                .w(px(32.))
                .h(px(30.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .cursor_pointer()
                .text_sm()
                .when(active, |d| d.bg(theme.accent))
                .hover(|s| s.bg(theme.muted))
                .child(rail_icon(section.kind, &section.title))
                .on_click(cx.listener(move |this, _, _, cx| this.emoji_picker_select_section(ix, cx)))
        });
        div()
            .id("emoji-category-rail")
            .w(px(RAIL_WIDTH))
            .h(px(GRID_HEIGHT))
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .py_1()
            .border_r_1()
            .border_color(theme.border)
            .overflow_y_scroll()
            .children(buttons)
    });

    let grid = if empty {
        div()
            .flex_1()
            .h(px(GRID_HEIGHT))
            .flex()
            .items_center()
            .justify_center()
            .text_sm()
            .text_color(theme.muted_foreground)
            .child(l10n::tr("No emojis match that search"))
            .into_any_element()
    } else {
        let count = rows.len();
        let list = uniform_list("emoji-picker-grid", count, move |range, _window, _cx| {
            range
                .map(|row_ix| {
                    let row = rows.get(row_ix);
                    let mut cells = Vec::new();
                    if let Some(row) = row {
                        for (col_ix, choice) in row.iter().enumerate() {
                            let idx = row_ix * COLUMNS + col_ix;
                            cells.push(render_emoji_cell(
                                choice,
                                tone,
                                idx == selected_index,
                                media_owned.as_deref(),
                                &theme_copy,
                                &weak,
                                idx,
                            ));
                        }
                    }
                    div()
                        .h(px(ROW_HEIGHT))
                        .w_full()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(2.))
                        .px_2()
                        .children(cells)
                        .into_any_element()
                })
                .collect()
        })
        .track_scroll(&picker.scroll)
        .w_full()
        .h(px(GRID_HEIGHT));
        list.into_any_element()
    };

    let section_header = picker.active_title().map(|title| {
        div()
            .px_2()
            .pt_1()
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme.muted_foreground)
            .child(title.to_owned())
    });

    let inspector = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px_3()
        .py_2()
        .border_t_1()
        .border_color(theme.border)
        .text_sm()
        .child(match picker.inspected() {
            Some(choice) => {
                let preview = if choice.custom_id.is_some() {
                    format!(":{}:", choice.name)
                } else {
                    choice.with_tone(tone).glyph
                };
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .child(div().text_base().child(preview))
                    .child(div().truncate().text_color(theme.muted_foreground).child(choice.label()))
                    .into_any_element()
            }
            None => div().text_color(theme.muted_foreground).child(search_placeholder).into_any_element(),
        });

    let content = div()
        .id("emoji-picker-popover")
        .key_context(CONTEXT)
        .w(px(PANEL_WIDTH))
        .flex()
        .flex_col()
        .capture_action(cx.listener(|this, _: &MoveUp, window, cx| {
            this.emoji_picker_move(-1, 0, window, cx);
            cx.stop_propagation();
        }))
        .capture_action(cx.listener(|this, _: &MoveDown, window, cx| {
            this.emoji_picker_move(1, 0, window, cx);
            cx.stop_propagation();
        }))
        .capture_action(cx.listener(|this, _: &MoveLeft, window, cx| {
            this.emoji_picker_move(0, -1, window, cx);
            cx.stop_propagation();
        }))
        .capture_action(cx.listener(|this, _: &MoveRight, window, cx| {
            this.emoji_picker_move(0, 1, window, cx);
            cx.stop_propagation();
        }))
        .capture_action(cx.listener(|this, ev: &Enter, window, cx| {
            cx.stop_propagation();
            this.emoji_picker_accept_selected(ev.shift, window, cx);
        }))
        .capture_action(cx.listener(|this, _: &Escape, window, cx| {
            cx.stop_propagation();
            this.close_overlay(window, cx);
        }))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .p_2()
                .child(div().flex_1().child(Input::new(&picker.input)))
                .child(tone_button),
        )
        .children(tone_strip)
        .child(
            div()
                .flex()
                .flex_row()
                .border_t_1()
                .border_color(theme.border)
                .children(rail)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .children(section_header)
                        .child(grid),
                ),
        )
        .child(inspector);

    let view = cx.weak_entity();
    overlay::popover(content, picker.anchor, theme, move |window, cx| {
        let _ = view.update(cx, |v, cx| v.close_overlay(window, cx));
    })
}
