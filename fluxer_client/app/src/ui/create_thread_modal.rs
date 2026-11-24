//! Create thread from a message modal (`CreateThreadModal.tsx`).

use fluxer_client_core::rest::{
    CreateThreadFromMessageRequest, THREAD_AUTO_ARCHIVE_DURATIONS, validate_thread_name,
};
use fluxer_client_core::state::Snowflake;
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

pub const CONTEXT: &str = "CreateThreadModal";

pub struct CreateThreadModal {
    pub seq: u64,
    pub channel_id: Snowflake,
    pub message_id: Snowflake,
    pub author: SharedString,
    pub preview: SharedString,
    pub name_input: Entity<InputState>,
    pub auto_archive_duration: u32,
    pub submitting: bool,
    pub error: Option<String>,
    pub _subscription: Subscription,
}

fn duration_label(minutes: u32) -> String {
    match minutes {
        60 => l10n::tr("1 Hour"),
        1440 => l10n::tr("24 Hours"),
        4320 => l10n::tr("3 Days"),
        10080 => l10n::tr("1 Week"),
        other => format!("{other}m"),
    }
}

impl ClientView {
    pub(crate) fn open_create_thread_modal(
        &mut self,
        target: ActionTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !target.sent || !target.can_create_thread {
            return;
        }
        self.prepare_overlay_replace(cx);
        self.autocomplete = None;
        let name_input = cx.new(|cx| InputState::new(window, cx).placeholder(l10n::tr("New thread")));
        let subscription = cx.subscribe_in(&name_input, window, |this: &mut Self, _, ev: &InputEvent, _, cx| match ev {
            InputEvent::PressEnter { shift: false, .. } => this.submit_create_thread(cx),
            InputEvent::Change => {
                if let Overlay::CreateThread(modal) = &mut this.overlay {
                    modal.error = None;
                    cx.notify();
                }
            }
            _ => {}
        });
        let seq = self.next_thread_seq();
        let focus = name_input.read(cx).focus_handle(cx);
        self.overlay = Overlay::CreateThread(CreateThreadModal {
            seq,
            channel_id: target.channel_id,
            message_id: target.message_id,
            author: target.author,
            preview: target.content,
            name_input,
            auto_archive_duration: 1440,
            submitting: false,
            error: None,
            _subscription: subscription,
        });
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(crate) fn set_create_thread_duration(&mut self, minutes: u32, cx: &mut Context<Self>) {
        let Overlay::CreateThread(modal) = &mut self.overlay else { return };
        if modal.submitting || !THREAD_AUTO_ARCHIVE_DURATIONS.contains(&minutes) {
            return;
        }
        modal.auto_archive_duration = minutes;
        cx.notify();
    }

    pub(crate) fn submit_create_thread(&mut self, cx: &mut Context<Self>) {
        let (seq, channel_id, message_id, request) = {
            let Overlay::CreateThread(modal) = &mut self.overlay else { return };
            if modal.submitting {
                return;
            }
            let raw = modal.name_input.read(cx).value().to_string();
            let name = match validate_thread_name(&raw) {
                Ok(name) => name,
                Err(_) => {
                    modal.error = Some(l10n::tr("Thread name cannot be empty."));
                    cx.notify();
                    return;
                }
            };
            modal.submitting = true;
            modal.error = None;
            (
                modal.seq,
                modal.channel_id,
                modal.message_id,
                CreateThreadFromMessageRequest {
                    name,
                    auto_archive_duration: Some(modal.auto_archive_duration),
                },
            )
        };
        self.backend
            .create_thread_from_message(seq, channel_id, message_id, request);
        cx.notify();
    }
}

pub fn render(modal: &CreateThreadModal, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let preview_text: SharedString = if modal.preview.trim().is_empty() {
        l10n::tr("(attachment)").into()
    } else {
        modal.preview.clone()
    };

    let duration_row = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap_1p5()
        .children(THREAD_AUTO_ARCHIVE_DURATIONS.iter().copied().map(|minutes| {
            let active = modal.auto_archive_duration == minutes;
            div()
                .id(SharedString::from(format!("thread-archive-{minutes}")))
                .debug_selector(move || format!("thread-archive-{minutes}"))
                .role(Role::Button)
                .px_2p5()
                .py_1()
                .rounded(px(6.))
                .border_1()
                .border_color(if active { theme.brand } else { theme.border })
                .when(active, |d| d.bg(theme.accent))
                .cursor_pointer()
                .text_xs()
                .font_weight(if active { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM })
                .text_color(if active { theme.foreground } else { theme.muted_foreground })
                .child(duration_label(minutes))
                .on_click(cx.listener(move |this, _, _, cx| this.set_create_thread_duration(minutes, cx)))
        }));

    let content = div()
        .id("create-thread-modal")
        .debug_selector(|| "create-thread-modal".to_owned())
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
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(Icon::new(AppIcon::Chats).with_size(px(18.)).text_color(theme.foreground))
                        .child(div().text_lg().font_weight(FontWeight::BOLD).child(l10n::tr("Create thread"))),
                )
                .child(
                    Button::new("create-thread-close-btn")
                        .ghost()
                        .small()
                        .icon(AppIcon::X)
                        .accessibility_label(l10n::tr("Close"))
                        .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
                ),
        )
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
                .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).child(modal.author.clone()))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .line_clamp(2)
                        .child(preview_text),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted_foreground)
                        .child(l10n::tr("Thread name")),
                )
                .child(Input::new(&modal.name_input).small()),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted_foreground)
                        .child(l10n::tr("Hide after inactivity")),
                )
                .child(duration_row),
        )
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
                    Button::new("create-thread-cancel-btn")
                        .ghost()
                        .small()
                        .label(l10n::tr("Cancel"))
                        .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
                )
                .child(
                    div().debug_selector(|| "create-thread-submit-btn".to_owned()).child(
                        Button::new("create-thread-submit-btn")
                            .primary()
                            .small()
                            .disabled(modal.submitting)
                            .label(if modal.submitting {
                                l10n::tr("Loading…")
                            } else {
                                l10n::tr("Create thread")
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.submit_create_thread(cx))),
                    ),
                ),
        );

    let view = cx.weak_entity();
    overlay::modal(content, theme, move |window, cx| {
        let _ = view.update(cx, |v, cx| v.close_overlay(window, cx));
    })
}
