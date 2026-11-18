//! Channel sidebar: heading, the channels of the selected place, and the
//! user panel at the bottom.

use gpui_kit::{AnyElement, Context, FontWeight, SharedString, div, prelude::*, px};

use super::{badge, user_panel};
use crate::client_view::ClientView;
use crate::theme::Theme;
use crate::view_model::{ChannelItem, Place};

pub fn render(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> impl IntoElement {
    let heading: SharedString = match view.place {
        Place::Home => "Direct Messages".into(),
        Place::Guild(id) => view.guilds.iter().find(|g| g.id == id).map(|g| g.name.clone()).unwrap_or_default(),
    };
    let mut list = div()
        .id("channel-list")
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .child(div().px_3().py_2().font_weight(FontWeight::BOLD).child(heading));
    for c in &view.channels {
        list = list.child(render_channel(view, c, theme, cx));
    }
    div()
        .flex()
        .flex_col()
        .w(px(theme.sizes.channel_list_width))
        .h_full()
        .flex_none()
        .border_r_1()
        .border_color(theme.border)
        .child(list)
        .child(user_panel::render(view, theme, cx))
}

fn render_channel(view: &ClientView, c: &ChannelItem, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let (muted_fg, fg) = (theme.muted_foreground, theme.foreground);
    if c.is_category {
        return div()
            .px_3()
            .pt_3()
            .text_xs()
            .text_color(muted_fg)
            .font_weight(FontWeight::MEDIUM)
            .child(c.name.clone())
            .into_any_element();
    }
    let selected = view.selected_channel == Some(c.id);
    let mut row = div()
        .id(("channel", c.id.0))
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .px_3()
        .py_1()
        .when(c.nested, |d| d.pl_5())
        .text_color(if (c.unread.unread || selected) && c.has_messages { fg } else { muted_fg })
        .when(c.unread.unread, |d| d.font_weight(FontWeight::BOLD))
        .when(selected, |d| d.bg(theme.accent))
        .child(div().w(px(18.)).flex_none().text_color(muted_fg).child(c.icon.glyph()))
        .child(div().flex_1().truncate().child(c.name.clone()));
    if c.unread.mention_count > 0 {
        row = row.child(badge(c.unread.mention_count, theme));
    } else if c.unread.unread {
        row = row.child(div().text_color(fg).child("•"));
    }
    if c.has_messages {
        let id = c.id;
        row = row.on_click(cx.listener(move |this, _, _, cx| this.select_channel(id, cx)));
    }
    row.into_any_element()
}
