//! Emoji picker view: search bar, skin-tone menu, category rail,
//! resizable `uniform_list` grid, and hovered-emoji inspector.

use fluxer_client_core::search::emoji::{EmojiCategory, EmojiChoice};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Enter, Escape, Input, MoveDown, MoveLeft, MoveRight, MoveUp};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, Context, FontWeight, KeyDownEvent, MouseButton, MouseDownEvent, ObjectFit, Pixels, Role, SharedString,
    Size, div, img, prelude::*, px, rgba, uniform_list,
};

use super::sections::{PickerTarget, SKIN_TONES, SectionKind};
use super::{CONTEXT, EmojiPicker, ExpressionTab};
use crate::client_view::{ClientView, ResizeEdge};
use crate::format::initials;
use crate::icons::AppIcon;
use crate::parity_probe::FlxExt as _;
use crate::prefs::{
    EMOJI_PICKER_HEIGHT_MAX, EMOJI_PICKER_HEIGHT_MIN, EMOJI_PICKER_WIDTH_MAX, EMOJI_PICKER_WIDTH_MIN,
    snap_emoji_picker_width,
};
use crate::theme::Theme;
use crate::ui::{l10n, overlay};

const RAIL_WIDTH: f32 = 46.;
const ROW_HEIGHT: f32 = 48.;
const CELL_SIZE: f32 = 44.;

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
            .with_fallback(move || {
                div().text_xs().text_color(fallback_color).child(fallback_name.clone()).into_any_element()
            })
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
        .hover(|s| s.bg(theme.hover_bg))
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

pub fn render(
    view: &ClientView,
    picker: &EmojiPicker,
    media: Option<&str>,
    viewport: Size<Pixels>,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let tone = picker.tone;
    let searching = picker.search_results.is_some();
    let rows = picker.rows.clone();
    let cols = picker.columns.max(1);
    let selected_index = picker.selected_index;
    let empty = picker.active_choices().is_empty();
    let weak = cx.weak_entity();
    let theme_copy = *theme;
    let media_owned = media.map(str::to_owned);

    let vw = f32::from(viewport.width);
    let vh = f32::from(viewport.height);
    let min_top = if vh >= 700. { 44. } else { 16. };
    let eff_right = f32::from(picker.anchor.x).clamp((EMOJI_PICKER_WIDTH_MIN as f32 + 16.).min(vw - 16.), (vw - 16.).max(EMOJI_PICKER_WIDTH_MIN as f32 + 16.));
    let eff_bottom = f32::from(picker.anchor.y).clamp((min_top + EMOJI_PICKER_HEIGHT_MIN as f32).min(vh - 16.), (vh - 16.).max(min_top + EMOJI_PICKER_HEIGHT_MIN as f32));
    let max_w = (eff_right - 16.).clamp(EMOJI_PICKER_WIDTH_MIN as f32, EMOJI_PICKER_WIDTH_MAX as f32);
    let max_h = (eff_bottom - min_top).clamp(EMOJI_PICKER_HEIGHT_MIN as f32, EMOJI_PICKER_HEIGHT_MAX as f32);
    let panel_w = snap_emoji_picker_width(view.emoji_picker_width, max_w) as f32;
    let panel_h = view.emoji_picker_height.clamp(EMOJI_PICKER_HEIGHT_MIN as f32, max_h);

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
        .hover(|s| s.bg(theme.hover_bg))
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
                    .hover(|s| s.bg(theme.hover_bg))
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
                .hover(|s| s.bg(theme.hover_bg))
                .child(rail_icon(section.kind, &section.title))
                .on_click(cx.listener(move |this, _, _, cx| this.emoji_picker_select_section(ix, cx)))
        });
        div()
            .id("emoji-category-rail")
            .w(px(RAIL_WIDTH))
            .h_full()
            .min_h_0()
            .flex_none()
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
            .min_h_0()
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
                            let idx = row_ix * cols + col_ix;
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
                        .gap(px(4.))
                        .px_2()
                        .children(cells)
                        .into_any_element()
                })
                .collect()
        })
        .track_scroll(&picker.scroll)
        .w_full()
        .flex_1()
        .min_h_0();
        list.into_any_element()
    };

    let section_header = picker.active_title().map(|title| {
        div()
            .px_2()
            .pt_1()
            .flex_none()
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme.muted_foreground)
            .child(title.to_owned())
    });

    let inspector = div()
        .flex_none()
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

    let make_handle = |id: &'static str, edge: ResizeEdge, aria: String| {
        div()
            .id(id)
            .debug_selector(move || id.to_owned())
            .aria_label(aria)
            .absolute()
            .track_focus(&view.emoji_resize_focus)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.begin_emoji_picker_resize(edge, ev, window, cx);
                }),
            )
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                this.handle_emoji_resize_key(ev, window, cx);
            }))
            .flx("ui.resizable-pane.resizable-pane-handles.handle.button")
    };

    let top_handle = make_handle(
        "emoji-resize-top",
        ResizeEdge::Top,
        l10n::tr("Resize expression picker from the top edge"),
    )
    .top_0()
    .left_0()
    .right_0()
    .h(px(8.))
    .cursor_n_resize();

    let left_handle = make_handle(
        "emoji-resize-left",
        ResizeEdge::Left,
        l10n::tr("Resize expression picker from the left edge"),
    )
    .top_0()
    .bottom_0()
    .left_0()
    .w(px(8.))
    .cursor_w_resize();

    let corner_handle = make_handle(
        "emoji-resize-top-left",
        ResizeEdge::TopLeft,
        l10n::tr("Resize expression picker from top left"),
    )
    .top_0()
    .left_0()
    .size(px(20.))
    .cursor_nwse_resize();

    let dark = theme.background.l < 0.5;
    let tab_active_bg: gpui_kit::Hsla = if dark { rgba(0xffffff1a).into() } else { theme.accent };
    let tab_hover_bg: gpui_kit::Hsla = if dark { rgba(0xffffff12).into() } else { theme.hover_bg };

    let tab_bar = (picker.target == PickerTarget::Composer).then(|| {
        let tabs = [
            (ExpressionTab::Media, "Media"),
            (ExpressionTab::Stickers, "Stickers"),
            (ExpressionTab::Emojis, "Emojis"),
        ];
        div()
            .flex_none()
            .h(px(40.))
            .pt(px(12.))
            .px_3()
            .flex()
            .flex_row()
            .items_start()
            .gap(px(8.))
            .children(tabs.into_iter().map(|(tab, label)| {
                let active = picker.active_tab == tab;
                let text = l10n::tr(label);
                div()
                    .id(SharedString::from(format!("expression-tab-{label}")))
                    .debug_selector(move || format!("expression-tab-{label}"))
                    .role(Role::Button)
                    .aria_selected(active)
                    .px_2()
                    .py_1()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .text_sm()
                    .line_height(px(20.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if active { theme.foreground } else { theme.muted_foreground })
                    .when(active, |d| d.bg(tab_active_bg))
                    .when(!active, |d| d.hover(move |s| s.bg(tab_hover_bg)))
                    .flx_text("expressions.expression-picker-popout.tab.button", text.clone())
                    .child(text)
                    .on_click(cx.listener(move |this, _, window, cx| this.expression_picker_select_tab(tab, window, cx)))
            }))
    });

    let body: AnyElement = match picker.active_tab {
        ExpressionTab::Emojis => div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .flx("expressions.emoji-picker.container")
            .child(
                div()
                    .flex_none()
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
                    .flex_1()
                    .min_h_0()
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
                            .h_full()
                            .min_h_0()
                            .children(section_header)
                            .child(grid),
                    ),
            )
            .child(inspector)
            .into_any_element(),
        ExpressionTab::Stickers => {
            let empty_title = l10n::tr("No stickers yet");
            div()
                .id("sticker-picker-panel")
                .debug_selector(|| "sticker-picker-panel".to_owned())
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(div().flex_none().p_2().border_b_1().border_color(theme.border).child(Input::new(&picker.input)))
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .px_6()
                        .child(Icon::new(AppIcon::Sticker).with_size(px(36.)).text_color(theme.muted_foreground))
                        .child(
                            div()
                                .text_base()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.foreground)
                                .flx_text("expressions.sticker-picker.empty", empty_title.clone())
                                .child(empty_title),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .child(l10n::tr("Stickers added to your communities show up here.")),
                        ),
                )
                .into_any_element()
        }
        ExpressionTab::Media => {
            let empty_title = l10n::tr("No saved media");
            let pills = ["All", "Images", "Videos", "Audio", "GIFs"];
            div()
                .id("media-picker-panel")
                .debug_selector(|| "media-picker-panel".to_owned())
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(div().flex_none().p_2().child(Input::new(&picker.input)))
                .child(
                    div()
                        .flex_none()
                        .px_2()
                        .pb_2()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .border_b_1()
                        .border_color(theme.border)
                        .children(pills.into_iter().map(|pill| {
                            let active = picker.media_filter == pill;
                            div()
                                .id(SharedString::from(format!("media-filter-{pill}")))
                                .debug_selector(move || format!("media-filter-{pill}"))
                                .role(Role::Button)
                                .aria_selected(active)
                                .px_2()
                                .py_1()
                                .rounded(px(6.))
                                .cursor_pointer()
                                .text_xs()
                                .font_weight(if active { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM })
                                .text_color(if active { theme.foreground } else { theme.muted_foreground })
                                .when(active, |d| d.bg(tab_active_bg))
                                .when(!active, |d| d.bg(theme.background).hover(move |s| s.bg(tab_hover_bg)))
                                .child(l10n::tr(pill))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.expression_picker_set_media_filter(pill, cx);
                                }))
                        })),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .px_6()
                        .child(Icon::new(AppIcon::Image).with_size(px(36.)).text_color(theme.muted_foreground))
                        .child(
                            div()
                                .text_base()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.foreground)
                                .flx_text("expressions.media-picker.empty", empty_title.clone())
                                .child(empty_title),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .child(l10n::tr("Save some media from messages to get started!")),
                        )
                        .child(
                            div().mt_2().child(
                                Button::new("media-picker-upload")
                                    .primary()
                                    .small()
                                    .label(l10n::tr("Upload a file"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.close_overlay(window, cx);
                                        this.pick_files(cx);
                                    })),
                            ),
                        ),
                )
                .into_any_element()
        }
    };

    let content = div()
        .id("emoji-picker-popover")
        .key_context(CONTEXT)
        .relative()
        .size_full()
        .flex()
        .flex_col()
        .capture_action(cx.listener(|this, _: &MoveUp, window, cx| {
            if this.emoji_resize_focus.is_focused(window) {
                return;
            }
            this.emoji_picker_move(-1, 0, window, cx);
            cx.stop_propagation();
        }))
        .capture_action(cx.listener(|this, _: &MoveDown, window, cx| {
            if this.emoji_resize_focus.is_focused(window) {
                return;
            }
            this.emoji_picker_move(1, 0, window, cx);
            cx.stop_propagation();
        }))
        .capture_action(cx.listener(|this, _: &MoveLeft, window, cx| {
            if this.emoji_resize_focus.is_focused(window) {
                return;
            }
            this.emoji_picker_move(0, -1, window, cx);
            cx.stop_propagation();
        }))
        .capture_action(cx.listener(|this, _: &MoveRight, window, cx| {
            if this.emoji_resize_focus.is_focused(window) {
                return;
            }
            this.emoji_picker_move(0, 1, window, cx);
            cx.stop_propagation();
        }))
        .capture_action(cx.listener(|this, ev: &Enter, window, cx| {
            if this.emoji_resize_focus.is_focused(window) {
                return;
            }
            cx.stop_propagation();
            this.emoji_picker_accept_selected(ev.shift, window, cx);
        }))
        .capture_action(cx.listener(|this, _: &Escape, window, cx| {
            cx.stop_propagation();
            this.close_overlay(window, cx);
        }))
        .children(tab_bar)
        .child(body)
        .child(top_handle)
        .child(left_handle)
        .child(corner_handle);

    let view_dismiss = cx.weak_entity();
    let view_move = cx.weak_entity();
    let view_up = cx.weak_entity();
    overlay::resizable_popover(
        "emoji-picker-container",
        "expressions.expression-picker-popout.container",
        content,
        picker.anchor,
        panel_w,
        panel_h,
        viewport,
        theme,
        move |window, cx| {
            let _ = view_dismiss.update(cx, |v, cx| {
                if let overlay::Overlay::EmojiPicker(p) = &v.overlay {
                    v.dismissed_expression_tab = Some((p.target, p.active_tab));
                }
                v.close_overlay(window, cx);
            });
        },
        move |ev, window, cx| {
            let _ = view_move.update(cx, |v, cx| v.handle_resize_move(ev, window, cx));
        },
        move |_, _, cx| {
            let _ = view_up.update(cx, |v, cx| v.finish_resize(cx));
        },
    )
}
