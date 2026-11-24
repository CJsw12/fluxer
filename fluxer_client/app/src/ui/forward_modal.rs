//! Forward message modal (`ForwardModal.tsx`).

use fluxer_client_core::state::{ClientState, Snowflake};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::{Disableable as _, Icon, Sizable as _};
use gpui_kit::{
    AnyElement, AppContext as _, Context, Entity, Focusable as _, FontWeight, Role, SharedString,
    Subscription, Window, div, prelude::*, px,
};

use crate::client_view::ClientView;
use crate::icons::AppIcon;
use crate::theme::Theme;
use crate::ui::message_actions::ActionTarget;
use crate::ui::overlay::Overlay;
use crate::ui::{l10n, overlay};

pub const CONTEXT: &str = "ForwardMessageModal";
pub const MAX_FORWARD_DESTINATIONS: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardDestination {
    pub channel_id: Snowflake,
    pub label: SharedString,
    pub subtitle: Option<SharedString>,
    pub is_dm: bool,
}

pub struct ForwardMessageModal {
    pub seq: u64,
    pub target: ActionTarget,
    pub query: Entity<InputState>,
    pub comment: Entity<InputState>,
    pub selected: Vec<Snowflake>,
    pub submitting: bool,
    pub error: Option<String>,
    pub _subscriptions: Vec<Subscription>,
}

/// Lists every channel or DM the current user can send messages into, ordered
/// with DMs/group DMs first and guild text/thread channels grouped by guild name.
pub fn candidate_destinations(state: &ClientState) -> Vec<ForwardDestination> {
    let mut dms: Vec<ForwardDestination> = state
        .dm_channels()
        .into_iter()
        .filter(|c| c.kind.has_messages())
        .map(|c| {
            let label = crate::view_model::channel_name(c);
            ForwardDestination {
                channel_id: c.id,
                label: label.into(),
                subtitle: Some(l10n::tr("Direct Messages").into()),
                is_dm: true,
            }
        })
        .collect();

    let mut guild_channels: Vec<(String, i32, Snowflake, ForwardDestination)> = Vec::new();
    for c in state.channels.values() {
        if c.kind.is_private() || !c.kind.has_messages() || !state.can_send_messages(c.id) {
            continue;
        }
        let guild_name = c
            .guild_id
            .and_then(|gid| state.guilds.get(&gid))
            .map(|g| g.name.clone())
            .unwrap_or_default();
        let name = c.name.as_deref().unwrap_or("channel");
        let dest = ForwardDestination {
            channel_id: c.id,
            label: format!("#{name}").into(),
            subtitle: (!guild_name.is_empty()).then(|| guild_name.clone().into()),
            is_dm: false,
        };
        guild_channels.push((guild_name.to_lowercase(), c.position, c.id, dest));
    }
    guild_channels.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));

    dms.extend(guild_channels.into_iter().map(|(_, _, _, d)| d));
    dms
}

impl ClientView {
    pub(crate) fn open_forward_modal(&mut self, target: ActionTarget, window: &mut Window, cx: &mut Context<Self>) {
        if !target.sent {
            return;
        }
        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        let query = cx.new(|cx| InputState::new(window, cx).placeholder(l10n::tr("Search")));
        let comment = cx.new(|cx| InputState::new(window, cx).placeholder(l10n::tr("Add a comment (optional)")));
        let sub_q = cx.subscribe_in(&query, window, |this: &mut Self, _, ev: &InputEvent, _, cx| {
            if matches!(ev, InputEvent::Change) && matches!(this.overlay, Overlay::ForwardMessage(_)) {
                cx.notify();
            }
        });
        let sub_c = cx.subscribe_in(&comment, window, |this: &mut Self, _, ev: &InputEvent, _, cx| match ev {
            InputEvent::PressEnter { shift: false, .. } => this.submit_forward_message(cx),
            InputEvent::Change if matches!(this.overlay, Overlay::ForwardMessage(_)) => cx.notify(),
            _ => {}
        });
        let seq = self.next_forward_seq();
        let focus = query.read(cx).focus_handle(cx);
        self.overlay = Overlay::ForwardMessage(ForwardMessageModal {
            seq,
            target,
            query,
            comment,
            selected: Vec::new(),
            submitting: false,
            error: None,
            _subscriptions: vec![sub_q, sub_c],
        });
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(crate) fn toggle_forward_destination(&mut self, channel_id: Snowflake, cx: &mut Context<Self>) {
        let Overlay::ForwardMessage(modal) = &mut self.overlay else { return };
        if modal.submitting {
            return;
        }
        if let Some(pos) = modal.selected.iter().position(|id| *id == channel_id) {
            modal.selected.remove(pos);
        } else if modal.selected.len() < MAX_FORWARD_DESTINATIONS {
            modal.selected.push(channel_id);
        }
        modal.error = None;
        cx.notify();
    }

    pub(crate) fn submit_forward_message(&mut self, cx: &mut Context<Self>) {
        let (seq, target, targets, comment) = {
            let Overlay::ForwardMessage(modal) = &mut self.overlay else { return };
            if modal.submitting || modal.selected.is_empty() {
                return;
            }
            modal.submitting = true;
            modal.error = None;
            let comment = modal.comment.read(cx).value().to_string();
            (modal.seq, modal.target.clone(), modal.selected.clone(), Some(comment))
        };
        self.backend.forward_message(
            seq,
            target.channel_id,
            target.guild_id,
            target.message_id,
            targets,
            comment,
        );
        cx.notify();
    }
}

pub fn render(
    view: &ClientView,
    modal: &ForwardMessageModal,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let q = modal.query.read(cx).value().trim().to_lowercase();
    let destinations: Vec<ForwardDestination> = {
        let state = view.backend.state();
        candidate_destinations(&state)
            .into_iter()
            .filter(|d| {
                if q.is_empty() {
                    return true;
                }
                d.label.to_lowercase().contains(&q)
                    || d.subtitle
                        .as_deref()
                        .is_some_and(|s| s.to_lowercase().contains(&q))
            })
            .collect()
    };

    let mut list = div()
        .id("forward-destination-list")
        .max_h(px(220.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_1();

    if destinations.is_empty() {
        list = list.child(
            div()
                .py_4()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(l10n::tr("No matching channels found")),
        );
    } else {
        for dest in destinations {
            let picked = modal.selected.contains(&dest.channel_id);
            let at_cap = modal.selected.len() >= MAX_FORWARD_DESTINATIONS && !picked;
            let cid = dest.channel_id;
            list = list.child(
                div()
                    .id(SharedString::from(format!("forward-dest-{}", cid.0)))
                    .debug_selector(move || format!("forward-dest-{}", cid.0))
                    .role(Role::Button)
                    .px_2p5()
                    .py_1p5()
                    .rounded(px(6.))
                    .when(!at_cap, |d| d.cursor_pointer().hover(|s| s.bg(theme.hover_bg)))
                    .when(picked, |d| d.bg(theme.accent))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_forward_destination(cid, cx)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().truncate().text_sm().font_weight(FontWeight::MEDIUM).child(dest.label))
                            .when_some(dest.subtitle, |d, sub| {
                                d.child(div().truncate().text_xs().text_color(theme.muted_foreground).child(sub))
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(if picked { theme.brand } else { theme.muted_foreground })
                            .child(if picked { "✓" } else { "+" }),
                    ),
            );
        }
    }

    let preview_text: SharedString = if modal.target.content.trim().is_empty() {
        l10n::tr("(attachment)").into()
    } else {
        modal.target.content.clone()
    };
    let selected_count = modal.selected.len();
    let send_label = if modal.submitting {
        l10n::tr("Loading…")
    } else if selected_count > 0 {
        format!("{} ({selected_count}/{MAX_FORWARD_DESTINATIONS})", l10n::tr("Send"))
    } else {
        l10n::tr("Send")
    };

    let content = div()
        .id("forward-message-modal")
        .debug_selector(|| "forward-message-modal".to_owned())
        .key_context(CONTEXT)
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .capture_action(cx.listener(|this, _: &Escape, window, cx| {
            cx.stop_propagation();
            this.close_overlay(window, cx);
        }))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(div().text_lg().font_weight(FontWeight::BOLD).child(l10n::tr("Forward message")))
                .child(
                    Button::new("forward-close-btn")
                        .ghost()
                        .small()
                        .icon(AppIcon::X)
                        .accessibility_label(l10n::tr("Close"))
                        .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
                ),
        )
        .child(Input::new(&modal.query).small())
        .child(list)
        .child(
            div()
                .p_2p5()
                .rounded(px(6.))
                .border_1()
                .border_color(theme.border)
                .bg(theme.background)
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1p5()
                        .child(Icon::new(AppIcon::ArrowBendUpRight).with_size(px(14.)).text_color(theme.muted_foreground))
                        .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).child(modal.target.author.clone())),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .line_clamp(2)
                        .child(preview_text),
                ),
        )
        .child(Input::new(&modal.comment).small())
        .when_some(modal.error.clone(), |d, err| {
            d.child(div().text_xs().text_color(theme.danger).child(err))
        })
        .child(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("forward-cancel-btn")
                        .ghost()
                        .small()
                        .label(l10n::tr("Cancel"))
                        .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
                )
                .child(
                    div().debug_selector(|| "forward-submit-btn".to_owned()).child(
                        Button::new("forward-submit-btn")
                            .primary()
                            .small()
                            .disabled(modal.submitting || modal.selected.is_empty())
                            .label(send_label)
                            .on_click(cx.listener(|this, _, _, cx| this.submit_forward_message(cx))),
                    ),
                ),
        );

    let view = cx.weak_entity();
    overlay::modal(content, theme, move |window, cx| {
        let _ = view.update(cx, |v, cx| v.close_overlay(window, cx));
    })
}
