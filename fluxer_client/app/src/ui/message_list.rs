//! Message list: the virtual scroller (or a placeholder), and per row the
//! channel-start header, the date / "NEW" dividers and system messages.

use std::ops::Range;

use gpui_kit::component::message_scroller::MessageScroller;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, App, FontWeight, HighlightStyle, Hsla, MouseButton, SharedString, StyleRefinement, StyledText,
    WeakEntity, div, prelude::*, px,
};

use super::avatar::{AvatarSpec, render_avatar};
use super::message_row::{self, RowBar};
use crate::client_view::ClientView;
use crate::parity_probe::FlxExt as _;
use crate::format::SystemIcon;
use crate::icons::AppIcon;
use crate::theme::Theme;
use crate::ui::l10n;
use crate::view_model::{MessageRow, SystemRow};

/// Groups consecutive flat [`MessageRow`]s into scroller items matching the
/// official `channel.message-group.group` structure. Dividers, channel-start
/// banners, and system messages always start their own group.
pub(crate) fn compute_row_groups(rows: &[MessageRow]) -> Vec<Range<usize>> {
    let mut groups: Vec<Range<usize>> = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let starts_group = i == 0
            || row.group_start
            || row.channel_start.is_some()
            || row.day_divider.is_some()
            || row.new_divider
            || row.system.is_some()
            || rows[i - 1].system.is_some();
        if starts_group {
            groups.push(i..i + 1);
        } else if let Some(last) = groups.last_mut() {
            last.end = i + 1;
        }
    }
    groups
}

/// `handle` is `view`'s own entity: row clicks (reactions, reply jumps,
/// the action bar) go to it.
pub fn render(view: &ClientView, handle: WeakEntity<ClientView>, theme: &Theme) -> AnyElement {
    let placeholder = if view.guild_unavailable {
        Some(l10n::tr("Community temporarily unavailable"))
    } else if view.selected_channel.is_none() {
        Some(l10n::tr("Select a channel"))
    } else if view.loading_history && view.rows.borrow().is_empty() {
        Some(l10n::tr("Loading…"))
    } else {
        None
    };
    if let Some(text) = placeholder {
        return div().flex_1().p_4().text_color(theme.muted_foreground).child(text).into_any_element();
    }
    if view.rows.borrow().is_empty()
        && view.selected_channel.is_some()
        && !view.channel_title.is_empty()
    {
        let banner = if view.place == crate::view_model::Place::Home {
            let dm_item = view
                .selected_channel
                .and_then(|id| view.channels.iter().find(|c| c.id == id));
            let dm_user_id = dm_item
                .and_then(|c| c.dm_user_id.or(Some(c.id)))
                .or(view.selected_channel)
                .unwrap_or(fluxer_client_core::state::Snowflake(0));
            let dm_avatar_url = dm_item.and_then(|c| c.dm_avatar_url.clone());
            dm_start(&view.channel_title, dm_user_id, dm_avatar_url, theme)
        } else {
            let display: SharedString = format!("{}{}", view.channel_glyph, view.channel_title).into();
            channel_start(&display, theme)
        };
        return div()
            .id("message-pane")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .justify_end()
            .child(banner)
            .into_any_element();
    }
    let rows = view.rows.clone();
    let row_groups = view.row_groups.clone();
    let leave = handle.clone();
    let (picker, hovered) = (view.reaction_picker, view.hovered_message);
    let highlighted_id = view
        .highlighted_message
        .filter(|(ch, _)| Some(*ch) == view.selected_channel)
        .map(|(_, id)| id)
        .or(view.keyboard_message);
    let theme = *theme;
    // Only reads the rows and groups: the scroller calls this during layout.
    let list = MessageScroller::new("messages", view.scroller.clone(), move |ix, window, cx| {
        let rows = rows.borrow();
        let groups = row_groups.borrow();
        let is_first_group = ix == 0;
        let has_next_group = ix + 1 < groups.len();
        let Some(range) = groups.get(ix).cloned() else {
            return div().into_any_element();
        };
        let Some(group_rows) = rows.get(range) else {
            return div().into_any_element();
        };
        render_group(
            group_rows,
            is_first_group,
            has_next_group,
            &theme,
            &handle,
            picker,
            hovered,
            highlighted_id,
            window.scale_factor(),
            cx,
        )
    })
    // The scroller's default row wrapper adds 32px below and 12px beside
    // every row; rows carry their own spacing.
    .with_row_style(StyleRefinement::default().pb(px(0.)).px(px(0.)));
    // Rows set the hovered message on mouse moves; leaving the list clears it.
    let (wheel, drag) = (leave.clone(), leave.clone());
    div()
        .id("message-pane")
        .flex_1()
        .min_h_0()
        // Official stream bottom space: last message ends 25px above the composer (2px row padding + 23px).
        .pb(px(16.))
        .flx("channel.messages.messages-wrapper")
        .flx("channel.messages.scroller-container")
        .flx("channel.messages.scroller")
        .on_hover(move |hovered, _, cx| {
            if !*hovered {
                let _ = leave.update(cx, |v, cx| v.hover_message(None, cx));
            }
        })
        .on_scroll(move |_, _, cx| {
            let _ = wheel.update(cx, |v, _| v.suppress_jump_scroll = false);
        })
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let _ = drag.update(cx, |v, _| v.suppress_jump_scroll = false);
        })
        .child(list)
        .into_any_element()
}

/// One scroller item: a group of consecutive user messages (or a single system
/// message) with the channel-start header, the date separator and the "NEW"
/// divider above it when they apply.
#[allow(clippy::too_many_arguments)]
fn render_group(
    group_rows: &[MessageRow],
    is_first_group: bool,
    has_next_group: bool,
    theme: &Theme,
    handle: &WeakEntity<ClientView>,
    picker: Option<fluxer_client_core::state::Snowflake>,
    hovered: Option<fluxer_client_core::state::Snowflake>,
    highlighted_id: Option<fluxer_client_core::state::Snowflake>,
    scale: f32,
    cx: &mut App,
) -> AnyElement {
    let Some(first) = group_rows.first() else {
        return div().into_any_element();
    };
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
    if let Some(name) = &first.channel_start {
        column = column.child(no_message(channel_start(name, theme)));
    }
    if first.day_divider.is_some() || first.new_divider {
        column = column.child(no_message(divider(first.day_divider.clone(), first.new_divider, theme)));
    }
    if let Some(system) = &first.system {
        let mut message = no_message(render_system(first, system, theme)).into_any_element();
        if highlighted_id == Some(first.id) {
            message = highlight_wrapper(message, theme);
        }
        return column.child(message).into_any_element();
    }
    if is_first_group && first.group_start {
        let leave_view = handle.clone();
        column = column.child(
            div()
                .mx(px(16.))
                .h(px(theme.sizes.group_gap))
                .on_mouse_move(move |_, _, cx| {
                    let _ = leave_view.update(cx, |v, cx| v.hover_message(None, cx));
                }),
        );
    }
    // Official `channel.message-group.group`: normal-flow container inset 16px on
    // each side; each child article bleeds -16px to full chat width.
    let mut group_el = div()
        .flex()
        .flex_col()
        .mx(px(16.))
        .flx("channel.message-group.group");
    for row in group_rows {
        let bar = if picker == Some(row.id) {
            RowBar::QuickReactions
        } else if hovered == Some(row.id) {
            RowBar::Actions
        } else {
            RowBar::None
        };
        let mut message = message_row::render(row, theme, handle, bar, scale, cx);
        if highlighted_id == Some(row.id) {
            message = highlight_wrapper(message, theme);
        }
        group_el = group_el.child(div().mx(px(-16.)).child(message));
    }
    column = column.child(group_el);
    if has_next_group {
        let leave_view = handle.clone();
        column = column.child(
            div()
                .mx(px(16.))
                .h(px(theme.sizes.group_gap))
                .flx("channel.channel-message-stream.push-spacer-if-needed.div")
                .on_mouse_move(move |_, _, cx| {
                    let _ = leave_view.update(cx, |v, cx| v.hover_message(None, cx));
                }),
        );
    }
    column.into_any_element()
}

fn highlight_wrapper(message: AnyElement, theme: &Theme) -> AnyElement {
    div()
        .relative()
        .w_full()
        .bg(theme.mention_bg)
        .debug_selector(|| "highlighted-message".to_owned())
        .child(div().absolute().left_0().top_0().bottom_0().w(px(3.)).bg(theme.brand))
        .child(message)
        .into_any_element()
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
    let (icon_w, icon_pr) = if sizes.compact {
        (20., 6.)
    } else {
        (sizes.gutter, sizes.gutter - sizes.avatar)
    };
    div()
        .flex()
        .flex_row()
        .w_full()
        .px_4()
        .when(row.group_start, |d| d.pt(px(sizes.group_gap)))
        .pb(px(if sizes.compact { 1. } else { 2. }))
        .child(
            div()
                .w(px(icon_w))
                .flex_none()
                .flex()
                .justify_center()
                .pr(px(icon_pr))
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
    // `ChannelWelcomeSection.module.css`: container margin 1rem inside the 1rem list gutter,
    // 5rem icon box (`--guild-list-foreground`), heading margin-top 0.75rem, gap 0.25rem.
    div()
        .flex()
        .flex_col()
        .gap_1()
        .px(px(32.))
        .pt_4()
        // Measured against the official layout: the description ends 32px above the divider.
        .pb(px(47.))
        .child(
            div()
                .size(px(80.))
                .flex_none()
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.channel_welcome_icon_bg)
                .text_color(theme.foreground)
                .text_size(px(20.))
                .flx("channel.channel-welcome-section.pointer-events-none")
                .child(
                    div()
                        .size(px(48.))
                        .flex_none()
                        .flx("channel.channel-utils.get-icon.text-channel-icon")
                        .child(Icon::new(AppIcon::TextChannel).with_size(px(48.)).text_color(theme.foreground)),
                ),
        )
        .child(
            div()
                .mt(px(12.))
                .text_color(theme.foreground)
                .text_size(px(30.))
                .line_height(px(45.))
                .font_weight(FontWeight::SEMIBOLD)
                .flx("channel.channel-welcome-section.heading")
                .child(l10n::tr_args("Welcome to {channelDisplayName}", &[("channelDisplayName", name.as_ref())])),
        )
        .child(
            div()
                .text_color(theme.muted_foreground)
                .text_size(px(18.))
                .line_height(px(27.))
                .flx("channel.channel-welcome-section.description")
                .child(l10n::tr_args("This is the start of {channelDisplayName}.", &[("channelDisplayName", name.as_ref())])),
        )
        .into_any_element()
}

/// DM empty-state welcome banner: circular avatar + bold title +
/// `Say hi to {channel_title}. Your DM starts here.`
fn dm_start(
    name: &SharedString,
    user_id: fluxer_client_core::state::Snowflake,
    avatar_url: Option<SharedString>,
    theme: &Theme,
) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .px_4()
        .pt_4()
        .pb_4()
        .child(
            div()
                .debug_selector(|| "dm-banner-avatar".to_owned())
                .child(render_avatar(
                    AvatarSpec {
                        user_id,
                        avatar_url,
                        display_name: name.clone(),
                        size: theme.sizes.channel_start_icon,
                        status: None,
                        status_border_bg: None,
                    },
                    theme,
                )),
        )
        .child(
            div()
                .text_color(theme.foreground)
                .text_size(px(28.))
                .font_weight(FontWeight::BOLD)
                .child(name.clone()),
        )
        .child(
            div()
                .text_color(theme.muted_foreground)
                .child(format!("Say hi to {name}. Your DM starts here.")),
        )
        .into_any_element()
}

/// Date separator (`ChannelDivider`) and/or the red "NEW" divider.
fn divider(label: Option<SharedString>, new: bool, theme: &Theme) -> AnyElement {
    let line_color = if new { theme.danger } else { theme.background_modifier_accent };
    let line = move |id: &'static str| div().flex_1().h(px(2.)).bg(line_color).flx(id);
    // `ChannelDivider.container`: 20px tall, 0 1.25rem padding, text padding 0 0.75rem.
    let mut row = div()
        .flex()
        .flex_row()
        .items_center()
        .mx(px(16.))
        .px(px(20.))
        .h(px(20.))
        .flx("channel.channel-message-stream.render-channel-stream.divider")
        .child(line("channel.divider.line"));
    if let Some(label) = label {
        row = row
            .child(
                div()
                    .px(px(12.))
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_tertiary)
                    .flx_text("channel.divider.text", label.to_string())
                    .child(label),
            )
            .child(line("channel.divider.line--2"));
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
                .child(l10n::tr("New")),
        );
    }
    row.into_any_element()
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, FixedOffset};
    use fluxer_client_core::Dispatch;
    use fluxer_client_core::state::{ClientState, LoadMode, Message, Snowflake};
    use serde_json::json;

    use super::compute_row_groups;
    use crate::view_model::message_pane_at;

    #[test]
    fn compute_row_groups_merges_consecutive_same_group_messages() {
        let mut state = ClientState::new();
        state.apply(&Dispatch {
            t: "READY".to_owned(),
            s: 1,
            d: json!({
                "session_id": "s",
                "user": { "id": "100", "username": "me" },
                "guilds": [{
                    "id": "1",
                    "properties": { "name": "G", "owner_id": "100" },
                    "roles": [{ "id": "1", "name": "@everyone", "permissions": "0" }],
                    "channels": [{ "id": "11", "type": 0, "name": "general", "position": 0 }],
                    "members": []
                }],
                "private_channels": [],
                "read_states": []
            }),
        });
        let msgs: Vec<Message> = vec![
            serde_json::from_value(json!({
                "id": "1003", "channel_id": "11", "content": "third",
                "timestamp": "2026-11-18T10:02:00+00:00",
                "author": { "id": "201", "username": "bob" }
            })).expect("msg3"),
            serde_json::from_value(json!({
                "id": "1002", "channel_id": "11", "content": "second",
                "timestamp": "2026-11-18T10:01:00+00:00",
                "author": { "id": "200", "username": "alice" }
            })).expect("msg2"),
            serde_json::from_value(json!({
                "id": "1001", "channel_id": "11", "content": "first",
                "timestamp": "2026-11-18T10:00:00+00:00",
                "author": { "id": "200", "username": "alice" }
            })).expect("msg1"),
        ];
        state.messages.load(Snowflake(11), msgs, LoadMode::Replace);
        let utc = FixedOffset::east_opt(0).expect("offset");
        let now = DateTime::parse_from_rfc3339("2026-11-18T12:00:00+00:00").expect("now").with_timezone(&utc);
        let rows = message_pane_at(&state, Snowflake(11), None, None, &now).rows;
        assert_eq!(rows.len(), 3);
        assert_eq!(compute_row_groups(&rows), vec![0..2, 2..3]);
    }
}
