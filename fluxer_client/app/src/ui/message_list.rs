//! Message list: the virtual scroller (or a placeholder), and per row the
//! channel-start header, the date / "NEW" dividers and system messages.

use gpui_kit::component::message_scroller::MessageScroller;
use gpui_kit::{
    AnyElement, App, FontWeight, HighlightStyle, Hsla, SharedString, StyleRefinement, StyledText, WeakEntity, div,
    prelude::*, px,
};

use super::message_row::{self, RowBar};
use crate::client_view::ClientView;
use crate::format::SystemIcon;
use crate::theme::Theme;
use crate::view_model::{MessageRow, SystemRow};

/// `handle` is `view`'s own entity: row clicks (reactions, reply jumps,
/// the action bar) go to it.
pub fn render(view: &ClientView, handle: WeakEntity<ClientView>, theme: &Theme) -> AnyElement {
    let placeholder = if view.selected_channel.is_none() {
        Some("Select a channel")
    } else if view.loading_history && view.rows.borrow().is_empty() {
        Some("Loading messages…")
    } else {
        None
    };
    if let Some(text) = placeholder {
        return div().flex_1().p_4().text_color(theme.muted_foreground).child(text).into_any_element();
    }
    let rows = view.rows.clone();
    let leave = handle.clone();
    let (picker, hovered) = (view.reaction_picker, view.hovered_message);
    let theme = *theme;
    // Only reads the rows: the scroller calls this during layout.
    let list = MessageScroller::new("messages", view.scroller.clone(), move |ix, window, cx| {
        let rows = rows.borrow();
        match rows.get(ix) {
            Some(row) => {
                let bar = if picker == Some(row.id) {
                    RowBar::QuickReactions
                } else if hovered == Some(row.id) {
                    RowBar::Actions
                } else {
                    RowBar::None
                };
                render_row(row, &theme, &handle, bar, window.scale_factor(), cx)
            }
            None => div().into_any_element(),
        }
    })
    // The scroller's default row wrapper adds 32px below and 12px beside
    // every row; rows carry their own spacing.
    .with_row_style(StyleRefinement::default().pb(px(0.)).px(px(0.)));
    // Rows set the hovered message on mouse moves; leaving the list clears it.
    div()
        .id("message-pane")
        .flex_1()
        .min_h_0()
        .on_hover(move |hovered, _, cx| {
            if !*hovered {
                let _ = leave.update(cx, |v, cx| v.hover_message(None, cx));
            }
        })
        .child(list)
        .into_any_element()
}

/// One scroller row: the message (or system message) with the channel-start
/// header, the date separator and the "NEW" divider above it when they apply.
fn render_row(
    row: &MessageRow,
    theme: &Theme,
    handle: &WeakEntity<ClientView>,
    bar: RowBar,
    scale: f32,
    cx: &mut App,
) -> AnyElement {
    // The parts above a message, and system messages, have no action bar:
    // the pointer over them hovers no message.
    let no_message = |part: AnyElement| {
        let handle = handle.clone();
        div()
            .w_full()
            .on_mouse_move(move |_, _, cx| {
                let _ = handle.update(cx, |v, cx| v.hover_message(None, cx));
            })
            .child(part)
    };
    let mut column = div().flex().flex_col().w_full();
    if let Some(name) = &row.channel_start {
        column = column.child(no_message(channel_start(name, theme)));
    }
    if row.day_divider.is_some() || row.new_divider {
        column = column.child(no_message(divider(row.day_divider.clone(), row.new_divider, theme)));
    }
    let message = match &row.system {
        Some(system) => no_message(render_system(row, system, theme)).into_any_element(),
        None => message_row::render(row, theme, handle, bar, scale, cx),
    };
    column.child(message).into_any_element()
}

/// `SystemMessage`: icon in the avatar column, text with names in bold,
/// then the time.
fn render_system(row: &MessageRow, system: &SystemRow, theme: &Theme) -> AnyElement {
    let (fg, muted_fg, sizes) = (theme.foreground, theme.muted_foreground, theme.sizes);
    let icon_color: Hsla = match system.icon {
        SystemIcon::Join => theme.join_icon,
        SystemIcon::Leave => theme.leave_icon,
        _ => muted_fg,
    };
    let bold = HighlightStyle { color: Some(fg), font_weight: Some(FontWeight::SEMIBOLD), ..Default::default() };
    let text = StyledText::new(system.text.clone()).with_highlights(system.bold.iter().map(|r| (r.clone(), bold)));
    div()
        .flex()
        .flex_row()
        .w_full()
        .px_4()
        .when(row.group_start, |d| d.pt(px(sizes.group_gap)))
        .pb(px(2.))
        .child(
            div()
                .w(px(sizes.gutter))
                .flex_none()
                .flex()
                .justify_center()
                .pr(px(sizes.gutter - sizes.avatar))
                .text_color(icon_color)
                .font_weight(FontWeight::BOLD)
                .child(system.icon.glyph()),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_baseline()
                .gap_2()
                .flex_1()
                .min_w_0()
                .child(div().text_color(muted_fg).child(text))
                .child(div().text_color(muted_fg).text_xs().child(row.time.clone())),
        )
        .into_any_element()
}

/// `ChannelWelcomeSection`: "Welcome to #name" / "This is the start of #name."
fn channel_start(name: &SharedString, theme: &Theme) -> AnyElement {
    let glyph: String = name.chars().take(1).collect();
    div()
        .flex()
        .flex_col()
        .gap_2()
        .px_4()
        .pt_4()
        .pb_2()
        .child(
            div()
                .size(px(theme.sizes.channel_start_icon))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.border)
                .text_color(theme.foreground)
                .text_size(px(36.))
                .child(glyph),
        )
        .child(
            div()
                .text_color(theme.foreground)
                .text_size(px(28.))
                .font_weight(FontWeight::BOLD)
                .child(format!("Welcome to {name}")),
        )
        .child(div().text_color(theme.muted_foreground).child(format!("This is the start of {name}.")))
        .into_any_element()
}

/// Date separator (`ChannelDivider`) and/or the red "NEW" divider.
fn divider(label: Option<SharedString>, new: bool, theme: &Theme) -> AnyElement {
    let line_color = if new { theme.danger } else { theme.border };
    let line = move || div().flex_1().h(px(1.)).bg(line_color);
    let mut row = div().flex().flex_row().items_center().gap_2().mx_4().my(px(8.)).h(px(16.)).child(line());
    if let Some(label) = label {
        row = row
            .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(label))
            .child(line());
    }
    if new {
        row = row.child(
            div()
                .px_1()
                .rounded(px(4.))
                .bg(theme.danger)
                .text_color(theme.on_fill)
                .text_size(px(10.))
                .font_weight(FontWeight::BOLD)
                .child("NEW"),
        );
    }
    row.into_any_element()
}
