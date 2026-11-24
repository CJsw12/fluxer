//! Bottom of the message pane: the last error, the reply/edit bar, the
//! message input, the typing row and the connection status line.
//!
//! [`ComposerController`] is the composer's mode machine (Normal /
//! Replying / Editing) without any GPUI state, so the owner of the input
//! (`ClientView`) drives it from the input's events and [`key_hooks`].

use fluxer_client_core::state::{ClientState, MESSAGE_TYPE_REPLY, Message, MessageState, Snowflake};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Enter, Escape, IndentInline, Input, InputContent, MoveDown, MoveUp};
use gpui_kit::{AnyElement, App, Context, Div, SharedString, Window, div, prelude::*, px, rgb};

use crate::client_view::ClientView;
use crate::icons::AppIcon;
use crate::parity_probe::FlxExt as _;
use crate::theme::Theme;
use crate::ui::emoji_picker::ExpressionTab;
use crate::ui::l10n;
use crate::ui::reply::ReplyingTo;

/// The last error, the autocomplete popup (`ui::autocomplete::render`)
/// and the mode bar above the input (from [`mode_bar`]), the input with
/// the key hooks around it, the typing row under it
/// (`ui::typing::typing_indicator`) and the connection status line.
pub fn render_with(
    view: &ClientView,
    theme: &Theme,
    popup: Option<AnyElement>,
    top_bar: Option<AnyElement>,
    typing_row: AnyElement,
    keys: ComposerKeys<ClientView>,
    cx: &mut Context<ClientView>,
) -> impl IntoElement {
    let paste_view = cx.weak_entity();
    let field = Input::new(&view.composer)
        .appearance(false)
        // Official composer text and placeholder: 16px / 22px line (`flx-channel-textarea-composer`).
        .text_size(px(16.))
        .line_height(px(22.))
        .disabled(!view.can_send)
        .on_paste(move |item, _, cx| paste_view.update(cx, |v, cx| v.paste(item, cx)).unwrap_or(false));
    let tray = view
        .selected_channel
        .and_then(|channel_id| crate::ui::attachments::tray::render(view.attachments.files(channel_id), theme, cx));
    let upload = view.can_send.then(|| {
        div()
            .flex_none()
            .debug_selector(|| "composer-upload".to_owned())
            .flx("channel.lexical-channel-textarea-content.plus-button-above-backdrop.clear-slash-command")
            .child(
                Button::new("composer-upload-btn")
                    .ghost()
                    .with_size(px(32.))
                    .rounded(px(6.))
                    .icon(composer_icon(AppIcon::Plus, theme))
                    .tooltip(l10n::tr("Open menu"))
                    .accessibility_label(l10n::tr("Open menu"))
                    .on_click(cx.listener(|v, ev: &gpui_kit::ClickEvent, window, cx| {
                        v.toggle_composer_plus_menu(ev.position(), window, cx);
                    })),
            )
    });
    let gif_btn = view.can_send.then(|| {
        div()
            .debug_selector(|| "composer-gif".to_owned())
            .flx("channel.textarea.textarea-buttons.textarea-button.expression-picker-toggle--2")
            .child(
                Button::new("composer-gif-btn")
                    .ghost()
                    .with_size(px(32.))
                    .rounded(px(6.))
                    .icon(composer_icon(AppIcon::Image, theme))
                    .tooltip(l10n::tr("Media"))
                    .accessibility_label(l10n::tr("Media"))
                    .on_click(cx.listener(|v, ev: &gpui_kit::ClickEvent, window, cx| {
                        v.toggle_expression_tab(ExpressionTab::Media, ev.position(), window, cx);
                    })),
            )
    });
    let sticker_btn = view.can_send.then(|| {
        div()
            .debug_selector(|| "composer-sticker".to_owned())
            .flx("channel.textarea.textarea-buttons.textarea-button.expression-picker-toggle--3")
            .child(
                Button::new("composer-sticker-btn")
                    .ghost()
                    .with_size(px(32.))
                    .rounded(px(6.))
                    .icon(composer_icon(AppIcon::Sticker, theme))
                    .tooltip(l10n::tr("Stickers"))
                    .accessibility_label(l10n::tr("Stickers"))
                    .on_click(cx.listener(|v, ev: &gpui_kit::ClickEvent, window, cx| {
                        v.toggle_expression_tab(ExpressionTab::Stickers, ev.position(), window, cx);
                    })),
            )
    });
    let emoji = view.can_send.then(|| {
        div()
            .debug_selector(|| "composer-emoji".to_owned())
            .flx("channel.textarea.textarea-buttons.textarea-button.expression-picker-toggle--4")
            .child(
                Button::new("composer-emoji-btn")
                    .ghost()
                    .with_size(px(32.))
                    .rounded(px(6.))
                    .icon(composer_icon(AppIcon::Smiley, theme))
                    .tooltip(l10n::tr("Emojis"))
                    .accessibility_label(l10n::tr("Emojis"))
                    .on_click(cx.listener(|v, ev: &gpui_kit::ClickEvent, window, cx| {
                        v.toggle_expression_tab(ExpressionTab::Emojis, ev.position(), window, cx);
                    })),
            )
    });
    let input = div()
        .flex()
        .flex_col()
        .children(popup)
        .children(tray)
        .children(top_bar)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(16.))
                .min_h(px(theme.sizes.composer_height))
                .rounded(px(theme.sizes.composer_radius))
                .bg(rgb(0x242229))
                .pl(px(14.))
                .pr(px(10.))
                .py(px(13.))
                .flx("channel.lexical-channel-textarea-content.textarea-outer")
                .children(upload)
                .child(div().flex_1().line_height(px(22.)).child(field))
                .when(view.can_send, |d| {
                    d.child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(4.))
                            .px(px(4.))
                            .flx("channel.textarea.textarea-buttons.button-container-dense")
                            .children(gif_btn)
                            .children(sticker_btn)
                            .children(emoji),
                    )
                }),
        );
    div()
        .flex()
        .flex_col()
        .children(view.error.clone().map(|e| div().px_4().text_color(theme.danger).child(e)))
        // Official: the textarea area is flush with the window bottom; the
        // typing row and connection status overlay the chat above it.
        .child(
            div().relative().h(px(0.)).w_full().child(
                div()
                    .absolute()
                    .bottom(px(0.))
                    .left(px(0.))
                    .w_full()
                    .child(typing_row)
                    .when(view.gateway_status != crate::backend::GatewayStatus::Ready, |d| {
                        d.child(div().px_4().pb_1().text_xs().text_color(theme.muted_foreground).child(view.status.clone()))
                    }),
            ),
        )
        .child(div().px(px(6.)).pb(px(6.)).flx("channel.channel-chat-layout.textarea-area").child(key_hooks(input, keys, cx)))
}

/// Placeholder for the current channel and send permission.
pub fn placeholder(view: &ClientView) -> SharedString {
    if view.selected_channel.is_none() {
        l10n::tr("Select a channel").into()
    } else if view.can_send {
        let name = view.channel_title.as_ref();
        match view.channel_glyph {
            "#" | "📢" | "🧵" | "💬" => l10n::tr_args("Message #{channelName}", &[("channelName", name)]),
            "@" => l10n::tr_args("Message @{userName}", &[("userName", name)]),
            _ => l10n::tr_args("Message {groupName}", &[("groupName", name)]),
        }
        .into()
    } else {
        l10n::tr("You can't send messages in this channel.").into()
    }
}

/// Updates the input's placeholder when it changed.
pub fn sync_placeholder(view: &ClientView, window: &mut Window, cx: &mut Context<ClientView>) {
    let placeholder = placeholder(view);
    view.composer.update(cx, |s, cx| {
        if s.presentation().placeholder() != &placeholder {
            s.set_placeholder(placeholder, window, cx);
        }
    });
}

/// The message being edited in the composer.
#[derive(Debug, Clone, PartialEq)]
pub struct EditTarget {
    pub channel_id: Snowflake,
    pub message_id: Snowflake,
    /// Content when editing started; the input is filled with it.
    pub original: String,
}

impl EditTarget {
    pub fn of(message: &Message) -> Self {
        Self { channel_id: message.channel_id, message_id: message.id, original: message.content.clone() }
    }
}

/// The newest sent message of the current user in `channel_id`, for Up
/// arrow editing (`MessageEditUtils.getLastEditableMessage`: own, not a
/// system message, not pending).
pub fn last_own_message(state: &ClientState, channel_id: Snowflake) -> Option<EditTarget> {
    let me = state.current_user_id()?;
    let window = state.messages.window(channel_id)?;
    window
        .messages
        .iter()
        .rev()
        .find(|m| m.author.id == me && m.state == MessageState::Sent && matches!(m.kind, 0 | MESSAGE_TYPE_REPLY))
        .map(EditTarget::of)
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum ComposerMode {
    #[default]
    Normal,
    Replying(ReplyingTo),
    Editing(EditTarget),
}

/// What Enter should do with the composer text.
#[derive(Debug, Clone, PartialEq)]
pub enum Submit {
    /// Empty text outside editing: nothing to send.
    Nothing,
    Send { content: String },
    /// `Backend::send_reply`; the composer is back to Normal.
    Reply { content: String, target: ReplyingTo },
    /// `Backend::edit_message`; put `restore` (the draft from before the
    /// edit) back into the input.
    Edit { channel_id: Snowflake, message_id: Snowflake, content: String, restore: InputContent },
    /// Edit left unchanged: just leave editing and restore the draft.
    EditUnchanged { restore: InputContent },
    /// Edit emptied: ask to delete the message (`message_actions::
    /// confirm_delete`); the composer stays in Editing until
    /// the delete ([`ComposerController::message_deleted`]) or Esc.
    ConfirmDelete { channel_id: Snowflake, message_id: Snowflake },
}

/// What Esc did.
#[derive(Debug, Clone, PartialEq)]
pub enum EscapeOutcome {
    /// Normal mode: let the input handle Escape.
    Ignored,
    CancelledReply,
    /// Put `restore` back into the input.
    CancelledEdit { restore: InputContent },
}

/// Typing signal for an input change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypingSignal {
    /// Text present: `Backend::typing_started`.
    Started,
    /// Input cleared: `TypingSender::stopped`.
    Stopped,
    /// Editing: no typing indicator (the web client sends none either).
    None,
}

/// The composer's mode machine.
#[derive(Debug, Clone)]
pub struct ComposerController {
    mode: ComposerMode,
    /// Input content from before editing started, restored afterwards.
    /// Kept with its tokens: a mention stashed as display text alone would
    /// be sent as `@name` and ping nobody.
    stashed_draft: InputContent,
}

impl Default for ComposerController {
    fn default() -> Self {
        Self { mode: ComposerMode::default(), stashed_draft: InputContent::new("") }
    }
}

impl ComposerController {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mode(&self) -> &ComposerMode {
        &self.mode
    }

    /// Reply to a message. While editing, the edit is dropped and the
    /// returned draft goes back into the input first.
    pub fn start_reply(&mut self, target: ReplyingTo) -> Option<InputContent> {
        let restore = self.leave_edit();
        self.mode = ComposerMode::Replying(target);
        restore
    }

    /// Edit a message: returns the text to put into the input. The current
    /// input text is kept and restored when editing ends (unless already
    /// editing another message, whose draft stays stashed).
    pub fn start_edit(&mut self, target: EditTarget, current_input: impl Into<InputContent>) -> String {
        if !matches!(self.mode, ComposerMode::Editing(_)) {
            self.stashed_draft = current_input.into();
        }
        let text = target.original.clone();
        self.mode = ComposerMode::Editing(target);
        text
    }

    /// Flip the reply's `replied_user` mention.
    pub fn toggle_mention(&mut self) {
        if let ComposerMode::Replying(target) = &mut self.mode
            && target.can_mention
        {
            target.mention = !target.mention;
        }
    }

    pub fn escape(&mut self) -> EscapeOutcome {
        match std::mem::take(&mut self.mode) {
            ComposerMode::Normal => EscapeOutcome::Ignored,
            ComposerMode::Replying(_) => EscapeOutcome::CancelledReply,
            ComposerMode::Editing(_) => EscapeOutcome::CancelledEdit { restore: self.take_draft() },
        }
    }

    /// Up arrow: with an empty input and not already editing, start editing
    /// the message `last_own` finds. Returns the text for the input when
    /// editing started; `None` lets the input move its cursor as usual.
    pub fn arrow_up(&mut self, input_empty: bool, last_own: impl FnOnce() -> Option<EditTarget>) -> Option<String> {
        if !input_empty || matches!(self.mode, ComposerMode::Editing(_)) {
            return None;
        }
        let target = last_own()?;
        Some(self.start_edit(target, ""))
    }

    /// Enter with `text` in the input.
    pub fn submit(&mut self, text: &str) -> Submit {
        let content = text.trim();
        match &self.mode {
            ComposerMode::Editing(target) => {
                if content.is_empty() {
                    return Submit::ConfirmDelete { channel_id: target.channel_id, message_id: target.message_id };
                }
                let (channel_id, message_id) = (target.channel_id, target.message_id);
                let unchanged = content == target.original.trim();
                let restore = self.leave_edit().unwrap_or_else(|| InputContent::new(""));
                if unchanged {
                    Submit::EditUnchanged { restore }
                } else {
                    Submit::Edit { channel_id, message_id, content: content.to_owned(), restore }
                }
            }
            _ if content.is_empty() => Submit::Nothing,
            ComposerMode::Replying(target) => {
                let target = target.clone();
                self.mode = ComposerMode::Normal;
                Submit::Reply { content: content.to_owned(), target }
            }
            ComposerMode::Normal => Submit::Send { content: content.to_owned() },
        }
    }

    /// The open channel changed: replies and edits belong to the old one.
    /// Returns the draft to restore when an edit was dropped.
    pub fn channel_changed(&mut self) -> Option<InputContent> {
        let restore = self.leave_edit();
        self.mode = ComposerMode::Normal;
        restore
    }

    /// The target message was deleted: drop a reply or edit pointing at it.
    pub fn message_deleted(&mut self, message_id: Snowflake) -> Option<InputContent> {
        match &self.mode {
            ComposerMode::Replying(t) if t.message_id == message_id => {
                self.mode = ComposerMode::Normal;
                None
            }
            ComposerMode::Editing(t) if t.message_id == message_id => self.leave_edit(),
            _ => None,
        }
    }

    pub fn typing_signal(&self, text: &str) -> TypingSignal {
        if matches!(self.mode, ComposerMode::Editing(_)) {
            TypingSignal::None
        } else if text.trim().is_empty() {
            TypingSignal::Stopped
        } else {
            TypingSignal::Started
        }
    }

    fn leave_edit(&mut self) -> Option<InputContent> {
        if !matches!(self.mode, ComposerMode::Editing(_)) {
            return None;
        }
        self.mode = ComposerMode::Normal;
        Some(self.take_draft())
    }

    fn take_draft(&mut self) -> InputContent {
        std::mem::replace(&mut self.stashed_draft, InputContent::new(""))
    }
}

/// Key handlers of the composer owner. Each returns true when it handled
/// the key; the input then does not see it.
pub struct ComposerKeys<V> {
    pub on_escape: fn(&mut V, &mut Window, &mut Context<V>) -> bool,
    pub on_up: fn(&mut V, &mut Window, &mut Context<V>) -> bool,
    pub on_down: fn(&mut V, &mut Window, &mut Context<V>) -> bool,
    /// Tab and Enter (autocomplete pick).
    pub on_accept: fn(&mut V, &mut Window, &mut Context<V>) -> bool,
}

/// Key context around the composer input; the layout's navigation
/// shortcuts are bound for `ClientView > Composer > Input` so they do not
/// fire in other inputs (the quick switcher's).
pub const CONTEXT: &str = "Composer";

/// Runs the handlers before the focused input inside `element` handles
/// Escape / MoveUp / MoveDown / Tab / Enter (capture phase).
pub fn key_hooks<V: 'static>(element: Div, keys: ComposerKeys<V>, cx: &mut Context<V>) -> Div {
    let ComposerKeys { on_escape, on_up, on_down, on_accept } = keys;
    element
        .key_context(CONTEXT)
        .capture_action(cx.listener(move |view, _: &Escape, window, cx| {
            if on_escape(view, window, cx) {
                cx.stop_propagation();
            }
        }))
        .capture_action(cx.listener(move |view, _: &MoveUp, window, cx| {
            if on_up(view, window, cx) {
                cx.stop_propagation();
            }
        }))
        .capture_action(cx.listener(move |view, _: &MoveDown, window, cx| {
            if on_down(view, window, cx) {
                cx.stop_propagation();
            }
        }))
        .capture_action(cx.listener(move |view, _: &IndentInline, window, cx| {
            if on_accept(view, window, cx) {
                cx.stop_propagation();
            }
        }))
        .capture_action(cx.listener(move |view, _: &Enter, window, cx| {
            if on_accept(view, window, cx) {
                cx.stop_propagation();
            }
        }))
}

/// The bar above the input for the mode: "Replying to X" or "Editing
/// message"; `None` in Normal mode.
pub fn mode_bar(
    mode: &ComposerMode,
    theme: &Theme,
    on_jump: impl Fn(Snowflake, Snowflake, &mut Window, &mut App) + 'static,
    on_toggle_mention: impl Fn(&mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&mut Window, &mut App) + 'static,
) -> Option<AnyElement> {
    match mode {
        ComposerMode::Normal => None,
        ComposerMode::Replying(target) => {
            Some(crate::ui::reply::replying_to_bar(target, theme, on_jump, on_toggle_mention, on_cancel))
        }
        ComposerMode::Editing(_) => Some(editing_bar(theme, on_cancel)),
    }
}

/// "Editing message · escape to cancel · enter to save" (`ChannelEditBar.tsx`).
pub fn editing_bar(theme: &Theme, on_cancel: impl Fn(&mut Window, &mut App) + 'static) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px_3()
        .py_1()
        .rounded_t(px(8.))
        .bg(theme.muted)
        .text_sm()
        .text_color(theme.muted_foreground)
        .child(div().flex_1().min_w_0().truncate().child(l10n::tr("Editing message")))
        .child(
            Button::new("edit-bar-cancel")
                .ghost()
                .compact()
                .icon(AppIcon::XCircle)
                .tooltip(l10n::tr("Cancel editing message"))
                .on_click(move |_, window, cx| on_cancel(window, cx)),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use gpui_kit::component::input::InputState;
    use gpui_kit::{Entity, Focusable as _, Render, TestAppContext};

    use super::*;

    fn reply_target(mention: bool) -> ReplyingTo {
        ReplyingTo {
            channel_id: Snowflake(11),
            message_id: Snowflake(5),
            author: "Ann".into(),
            mention,
            can_mention: true,
        }
    }

    fn edit_target() -> EditTarget {
        EditTarget { channel_id: Snowflake(11), message_id: Snowflake(7), original: "old text".to_owned() }
    }

    #[test]
    fn reply_mode_sends_a_reply_then_returns_to_normal() {
        let mut c = ComposerController::new();
        assert_eq!(c.submit("  hi "), Submit::Send { content: "hi".to_owned() });
        assert_eq!(c.start_reply(reply_target(true)), None);
        c.toggle_mention();
        assert_eq!(c.submit("   "), Submit::Nothing, "empty reply keeps the mode");
        assert!(matches!(c.mode(), ComposerMode::Replying(_)));
        assert_eq!(c.submit("yo"), Submit::Reply { content: "yo".to_owned(), target: reply_target(false) });
        assert_eq!(c.mode(), &ComposerMode::Normal);
    }

    #[test]
    fn escape_cancels_reply_and_edit() {
        let mut c = ComposerController::new();
        assert_eq!(c.escape(), EscapeOutcome::Ignored);
        c.start_reply(reply_target(true));
        assert_eq!(c.escape(), EscapeOutcome::CancelledReply);
        assert_eq!(c.mode(), &ComposerMode::Normal);
        assert_eq!(c.start_edit(edit_target(), "my draft"), "old text");
        assert_eq!(c.escape(), EscapeOutcome::CancelledEdit { restore: "my draft".into() });
        assert_eq!(c.mode(), &ComposerMode::Normal);
    }

    #[test]
    fn edit_submit_outcomes() {
        let mut c = ComposerController::new();
        c.start_edit(edit_target(), "draft");
        assert_eq!(
            c.submit(""),
            Submit::ConfirmDelete { channel_id: Snowflake(11), message_id: Snowflake(7) },
            "emptied edit asks to delete"
        );
        assert!(matches!(c.mode(), ComposerMode::Editing(_)));
        assert_eq!(c.submit("old text "), Submit::EditUnchanged { restore: "draft".into() });
        c.start_edit(edit_target(), "");
        assert_eq!(
            c.submit("new text"),
            Submit::Edit {
                channel_id: Snowflake(11),
                message_id: Snowflake(7),
                content: "new text".to_owned(),
                restore: "".into()
            }
        );
        assert_eq!(c.mode(), &ComposerMode::Normal);
    }

    #[test]
    fn arrow_up_edits_only_from_an_empty_input() {
        let mut c = ComposerController::new();
        assert_eq!(c.arrow_up(false, || Some(edit_target())), None, "text in the input: cursor movement");
        assert_eq!(c.arrow_up(true, || None), None, "no own message");
        assert_eq!(c.arrow_up(true, || Some(edit_target())).as_deref(), Some("old text"));
        assert_eq!(c.arrow_up(true, || panic!("already editing")), None);
        assert_eq!(c.typing_signal("x"), TypingSignal::None);
        assert_eq!(c.channel_changed(), Some("".into()));
        assert_eq!(c.typing_signal("x"), TypingSignal::Started);
        assert_eq!(c.typing_signal(" "), TypingSignal::Stopped);
    }

    #[test]
    fn reply_while_editing_restores_the_draft() {
        let mut c = ComposerController::new();
        c.start_edit(edit_target(), "keep me");
        assert_eq!(c.start_reply(reply_target(true)), Some("keep me".into()));
        assert_eq!(c.message_deleted(Snowflake(5)), None);
        assert_eq!(c.mode(), &ComposerMode::Normal);
        c.start_edit(edit_target(), "again");
        assert_eq!(c.channel_changed(), Some("again".into()));
    }

    /// The stashed draft keeps its mention token, so restoring it after an
    /// edit still sends `<@id>` (not the display text `@Ann`).
    #[test]
    fn stashed_draft_keeps_tokens() {
        use gpui_kit::component::input::InlineToken;
        let draft = InputContent::new("hi @Ann ")
            .with_token(3..7, InlineToken::new("<@5>", "@Ann"))
            .expect("token over its text");
        let mut c = ComposerController::new();
        c.start_edit(edit_target(), draft.clone());
        let EscapeOutcome::CancelledEdit { restore } = c.escape() else { panic!("edit cancelled") };
        assert_eq!(restore, draft);
        assert_eq!(restore.tokens().len(), 1);
    }

    /// The composer owner in miniature: an input inside [`key_hooks`].
    struct Harness {
        input: Entity<InputState>,
        controller: ComposerController,
        last_own: Option<EditTarget>,
    }

    impl Harness {
        fn on_escape(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
            match self.controller.escape() {
                EscapeOutcome::Ignored => false,
                EscapeOutcome::CancelledReply => true,
                EscapeOutcome::CancelledEdit { restore } => {
                    self.input.update(cx, |s, cx| s.set_value(restore, window, cx));
                    true
                }
            }
        }

        fn on_up(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
            let empty = self.input.read(cx).value().is_empty();
            let last_own = self.last_own.clone();
            match self.controller.arrow_up(empty, || last_own) {
                Some(text) => {
                    self.input.update(cx, |s, cx| s.set_value(text, window, cx));
                    true
                }
                None => false,
            }
        }
    }

    impl Render for Harness {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let keys = ComposerKeys {
                on_escape: Harness::on_escape,
                on_up: Harness::on_up,
                on_down: |_, _, _| false,
                on_accept: |_, _, _| false,
            };
            key_hooks(div().size_full().child(Input::new(&self.input)), keys, cx)
        }
    }

    #[gpui_kit::test]
    fn up_arrow_edits_and_escape_restores_through_key_hooks(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let mut harness = None;
        let (_, cx) = cx.add_window_view(|window, cx| {
            let input = cx.new(|cx| InputState::new(window, cx));
            let view = cx.new(|_| Harness { input, controller: ComposerController::new(), last_own: Some(edit_target()) });
            harness = Some(view.clone());
            gpui_kit::base::Root::new(view, window, cx)
        });
        let harness = harness.expect("window built its content");
        cx.run_until_parked();
        cx.update(|window, cx| {
            let handle = harness.read(cx).input.focus_handle(cx);
            handle.focus(window, cx);
            let _ = window.draw(cx);
        });
        let value = |cx: &mut gpui_kit::VisualTestContext| {
            cx.update(|_, cx| harness.read(cx).input.read(cx).value().to_string())
        };

        cx.simulate_keystrokes("up");
        cx.run_until_parked();
        assert_eq!(value(cx), "old text", "Up on an empty input edits the last own message");
        assert!(cx.update(|_, cx| matches!(harness.read(cx).controller.mode(), ComposerMode::Editing(_))));

        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert_eq!(value(cx), "", "Esc cancels the edit and restores the (empty) draft");
        assert!(cx.update(|_, cx| harness.read(cx).controller.mode() == &ComposerMode::Normal));

        // With text in the input, Up is left to the input.
        cx.simulate_input("draft");
        cx.simulate_keystrokes("up");
        cx.run_until_parked();
        assert_eq!(value(cx), "draft");
        assert!(cx.update(|_, cx| harness.read(cx).controller.mode() == &ComposerMode::Normal));
    }
}

/// Official textarea buttons draw 22px icons in `--text-primary-muted` (rgb(206,203,212) dark);
/// gpui-component's Button would otherwise recolour them with its own foreground.
fn composer_icon(icon: AppIcon, theme: &Theme) -> gpui_kit::component::Icon {
    gpui_kit::component::Icon::new(icon).with_size(px(22.)).text_color(theme.muted_foreground)
}
