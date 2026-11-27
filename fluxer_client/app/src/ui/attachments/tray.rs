//! The strip of queued files above the composer input: a preview (image
//! thumbnail, or a file glyph), the name and size, a spoiler toggle and a
//! remove button per file (`ChannelAttachmentArea.tsx`, simplified).

use std::{cell::Cell, rc::Rc};

use crate::icons::AppIcon;
use fluxer_client_core::state::Snowflake;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::{
    AnyElement, AppContext as _, Context, Entity, Focusable as _, ObjectFit, Render, SharedString, StyledImage as _,
    Subscription, Window, div, img, prelude::*, px,
};

use super::{AttachmentEdit, DropPosition, EditError, MAX_ALT_TEXT_LENGTH, PendingAttachment, Preview, format_size};
use crate::client_view::ClientView;
use crate::parity_probe::FlxExt as _;
use crate::theme::Theme;
use crate::ui::l10n;
use crate::ui::overlay::Overlay;

const CHIP_WIDTH: f32 = 148.;
const THUMB_HEIGHT: f32 = 84.;

/// `None` when nothing is queued.
pub fn render(files: &[PendingAttachment], theme: &Theme, cx: &mut Context<ClientView>) -> Option<AnyElement> {
    if files.is_empty() {
        return None;
    }
    let chips: Vec<AnyElement> = files.iter().map(|file| chip(file, theme, cx)).collect();
    Some(
        div()
            .id("attachment-tray")
            .flex()
            .flex_row()
            .gap_2()
            .p_2()
            .rounded_t(px(8.))
            .bg(theme.muted)
            .overflow_x_scroll()
            .children(chips)
            .into_any_element(),
    )
}

fn chip(file: &PendingAttachment, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let id = file.id;
    let drag_position = Rc::new(Cell::new(DropPosition::After));
    let position = drag_position.clone();
    let placeholder = theme.image_placeholder;
    let thumb = match &file.file.preview {
        Preview::Path(path) => Some(img(path.clone())),
        Preview::Image(image) => Some(img(image.clone())),
        Preview::None => None,
    };
    let thumb = match thumb {
        Some(image) => image
            .size_full()
            .object_fit(ObjectFit::Cover)
            .with_loading(move || div().size_full().bg(placeholder).into_any_element())
            .with_fallback(move || div().size_full().bg(placeholder).into_any_element())
            .into_any_element(),
        None => div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(file_glyph(&file.file.name))
            .into_any_element(),
    };
    let spoiler_cover = file.spoiler.then(|| {
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.background)
            .opacity(0.9)
            .text_xs()
            .font_weight(gpui_kit::FontWeight::BOLD)
            .text_color(theme.foreground)
            .child(l10n::tr("Spoiler"))
    });
    let spoiler = Button::new(("attachment-spoiler", id))
        .ghost()
        .xsmall()
        .icon(if file.spoiler { AppIcon::EyeSlash } else { AppIcon::Eye })
        .tooltip(if file.spoiler {
            l10n::tr("Remove spoiler")
        } else {
            l10n::tr("Mark as spoiler")
        })
        .on_click(cx.listener(move |view, _, _, cx| view.toggle_attachment_spoiler(id, cx)));
    let remove = Button::new(("attachment-remove", id))
        .ghost()
        .xsmall()
        .icon(AppIcon::X)
        .tooltip(l10n::tr("Remove attachment"))
        .on_click(cx.listener(move |view, _, _, cx| view.remove_attachment(id, cx)));
    let edit = Button::new(("attachment-edit", id))
        .ghost()
        .xsmall()
        .icon(AppIcon::PencilSimple)
        .tooltip(l10n::tr("Edit attachment"))
        .on_click(cx.listener(move |view, _, window, cx| view.open_attachment_edit(id, window, cx)));
    div()
        .id(("queued-attachment", id))
        .flex()
        .flex_col()
        .flex_none()
        .w(px(CHIP_WIDTH))
        .p_1()
        .gap_1()
        .rounded(px(6.))
        .bg(theme.background)
        .flx("channel.channel-attachment-area.sortable-attachment-item")
        .on_drag(
            AttachmentDrag {
                id,
                name: file.file.name.clone().into(),
            },
            |drag, _, _, cx| cx.new(|_| AttachmentDragPreview(drag.name.clone())),
        )
        .on_drag_move::<AttachmentDrag>(move |event, _, _| {
            position.set(DropPosition::from_offset(
                f32::from(event.event.position.x - event.bounds.origin.x),
                f32::from(event.bounds.size.width),
            ));
        })
        .on_drop(cx.listener(move |view, drag: &AttachmentDrag, _, cx| {
            if let Some(channel_id) = view.selected_channel
                && view.attachments.reorder(channel_id, drag.id, id, drag_position.get())
            {
                cx.notify();
            }
        }))
        .child(
            div()
                .relative()
                .w_full()
                .h(px(THUMB_HEIGHT))
                .rounded(px(4.))
                .overflow_hidden()
                .child(thumb)
                .children(spoiler_cover),
        )
        .child(
            div()
                .id(("attachment-name", id))
                .text_xs()
                .truncate()
                .flx("channel.channel-attachment-area.sortable-attachment-item.tooltip")
                .tooltip({
                    let name = file.file.name.clone();
                    move |window, cx| Tooltip::new(name.clone()).build(window, cx)
                })
                .child(file.file.name.clone()),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(format_size(file.file.size)),
                )
                .child(
                    div()
                        .flx("channel.channel-attachment-area.attachment-action-bar-button.tooltip")
                        .child(edit),
                )
                .child(
                    div()
                        .flx("channel.channel-attachment-area.attachment-action-bar-button.tooltip")
                        .child(spoiler),
                )
                .child(
                    div()
                        .flx("channel.channel-attachment-area.attachment-action-bar-button.tooltip")
                        .child(remove),
                ),
        )
        .into_any_element()
}

struct AttachmentDrag {
    id: u64,
    name: SharedString,
}

struct AttachmentDragPreview(SharedString);

impl Render for AttachmentDragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().px_2().py_1().text_xs().child(self.0.clone())
    }
}

pub struct EditAttachmentModal {
    channel_id: Snowflake,
    id: u64,
    filename: Entity<InputState>,
    description: Option<Entity<TextareaState>>,
    spoiler: bool,
    error: bool,
    _description_subscription: Option<Subscription>,
}

impl ClientView {
    fn open_attachment_edit(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(channel_id) = self.selected_channel else {
            return;
        };
        let Some(file) = self.attachments.files(channel_id).iter().find(|file| file.id == id) else {
            return;
        };
        let initial = AttachmentEdit::from_pending(file);
        let filename = cx.new(|cx| {
            let mut input = InputState::new(window, cx);
            input.set_value(initial.filename, window, cx);
            input
        });
        let description = file.alt_text_supported().then(|| {
            cx.new(|cx| {
                let mut input = TextareaState::new(window, cx)
                    .rows(3)
                    .placeholder(l10n::tr("Describe this media for screen readers"));
                input.set_value(initial.description, window, cx);
                input
            })
        });
        let description_subscription = description.as_ref().map(|input| {
            cx.subscribe_in(input, window, |_, input, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    let value = input.read(cx).value();
                    if let Some((end, _)) = value.char_indices().nth(MAX_ALT_TEXT_LENGTH) {
                        let limited = value[..end].to_owned();
                        input.update(cx, |input, cx| input.set_value(limited, window, cx));
                    }
                }
            })
        });
        self.prepare_overlay_replace(cx);
        let focus = filename.read(cx).focus_handle(cx);
        self.overlay = Overlay::EditAttachment(EditAttachmentModal {
            channel_id,
            id,
            filename,
            description,
            spoiler: initial.spoiler,
            error: false,
            _description_subscription: description_subscription,
        });
        window.focus(&focus, cx);
        cx.notify();
    }

    fn save_attachment_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Overlay::EditAttachment(modal) = &mut self.overlay else {
            return;
        };
        let edit = AttachmentEdit {
            filename: modal.filename.read(cx).value().to_string(),
            description: modal
                .description
                .as_ref()
                .map(|input| input.read(cx).value().to_string())
                .unwrap_or_default(),
            spoiler: modal.spoiler,
        };
        if self.attachments.edit(modal.channel_id, modal.id, &edit) == Err(EditError::BadFilename) {
            modal.error = true;
            cx.notify();
        } else {
            self.close_overlay(window, cx);
        }
    }
}

pub fn render_edit(modal: &EditAttachmentModal, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let close = cx.listener(|view, _: &gpui_kit::ClickEvent, window, cx| view.close_overlay(window, cx));
    let header = div()
        .flex()
        .items_center()
        .justify_between()
        .h(px(64.))
        .px_4()
        .flex_none()
        .flx("messaging.attachment-edit-modal.modal-header")
        .child(
            div()
                .text_lg()
                .font_weight(gpui_kit::FontWeight::BOLD)
                .child(l10n::tr("Edit attachment")),
        )
        .child(
            Button::new("edit-attachment-close")
                .ghost()
                .small()
                .icon(AppIcon::X)
                .tooltip(l10n::tr("Close"))
                .on_click(cx.listener(|view, _, window, cx| view.close_overlay(window, cx))),
        );
    let fields = div()
        .flex()
        .flex_col()
        .gap_4()
        .px_4()
        .flex_1()
        .flx("messaging.attachment-edit-modal.modal-content")
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().line_height(px(19.)).child(l10n::tr("Filename")))
                .child(
                    div()
                        .flx("messaging.attachment-edit-modal.input.text")
                        .child(Input::new(&modal.filename).large().aria_label(l10n::tr("Filename"))),
                ),
        )
        .children(modal.description.as_ref().map(|input| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().line_height(px(19.)).child(l10n::tr("Alt text description")))
                .child(
                    div().mt(px(2.)).flx("messaging.attachment-edit-modal.textarea").child(
                        Textarea::new(input)
                            .h(px(82.))
                            .aria_label(l10n::tr("Alt text description")),
                    ),
                )
        }))
        .children(modal.error.then(|| div().child(l10n::tr("Invalid filename."))))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .py_2()
                .child(l10n::tr("Mark as spoiler"))
                .child(
                    div().flx("messaging.attachment-edit-modal.switch.set-value").child(
                        Switch::new("edit-attachment-spoiler")
                            .checked(modal.spoiler)
                            .on_change(cx.listener(|view, checked: &bool, _, cx| {
                                if let Overlay::EditAttachment(modal) = &mut view.overlay {
                                    modal.spoiler = *checked;
                                    cx.notify();
                                }
                            })),
                    ),
                ),
        );
    let footer = div()
        .flex()
        .items_center()
        .justify_end()
        .gap_2()
        .px_4()
        .h(px(68.))
        .flex_none()
        .flx("messaging.attachment-edit-modal.modal-footer")
        .child(
            div().flx("messaging.attachment-edit-modal.button.pop").child(
                Button::new("edit-attachment-cancel")
                    .w(px(65.))
                    .h(px(36.))
                    .label(l10n::tr("Cancel"))
                    .on_click(close),
            ),
        )
        .child(
            div().flx("messaging.attachment-edit-modal.button.submit").child(
                Button::new("edit-attachment-save")
                    .primary()
                    .w(px(53.))
                    .h(px(36.))
                    .label(l10n::tr("Save"))
                    .on_click(cx.listener(|view, _, window, cx| view.save_attachment_edit(window, cx))),
            ),
        );
    div()
        .id("attachment-edit-backdrop")
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .occlude()
        .bg(theme.background.opacity(0.75))
        .on_mouse_down(
            gpui_kit::MouseButton::Left,
            cx.listener(|view, _, window, cx| view.close_overlay(window, cx)),
        )
        .child(
            div()
                .w(px(440.))
                .max_w_full()
                .min_h(px(if modal.description.is_some() { 400. } else { 248. }))
                .flex()
                .flex_col()
                .rounded(px(8.))
                .border_1()
                .border_color(theme.background_secondary.blend(theme.border))
                .bg(theme.background_secondary)
                .text_color(theme.foreground)
                .shadow_lg()
                .flx("messaging.attachment-edit-modal.modal-root")
                .on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(header)
                .child(fields)
                .child(footer),
        )
        .into_any_element()
}

/// Upper-case extension ("PDF"), or "FILE".
fn file_glyph(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_uppercase())
        .filter(|ext| !ext.is_empty() && ext.len() <= 5)
        .unwrap_or_else(|| l10n::tr("File"))
}
