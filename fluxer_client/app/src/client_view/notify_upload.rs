//! Notifications (toasts, sounds, window focus) and the attachment tray
//! (picker, drop, paste, send with files).

use std::path::PathBuf;

use fluxer_client_core::state::{MessageState, Snowflake};
use gpui_kit::{ClipboardEntry, ClipboardItem, Context, PathPromptOptions, Window};

use super::ClientView;
use crate::backend::Backend;
use crate::notify::{Notification, Notifier, Outcome};
use crate::ui::attachments::{self, NewFile, PendingAttachment};
use crate::ui::composer::ComposerMode;

impl ClientView {
    /// The OS toaster and sound player; silent in tests.
    pub(super) fn system_notifier(backend: &Backend) -> Notifier {
        #[cfg(not(test))]
        {
            use crate::notify::os::{OsToaster, RodioPlayer};
            Notifier::new(Box::new(OsToaster::new(backend.event_sender())), Box::new(RodioPlayer::default()))
        }
        #[cfg(test)]
        {
            let _ = backend;
            Notifier::new(Box::new(crate::notify::Silent), Box::new(crate::notify::Silent))
        }
    }

    pub(super) fn window_activation_changed(&mut self, active: bool, cx: &mut Context<Self>) {
        if active != self.window_active {
            self.window_active = active;
            self.backend.notify_focus().set_focused(active);
            cx.notify();
        }
    }

    /// A message raised a notification on the gateway side.
    pub(super) fn notify(&mut self, notification: &Notification, cx: &mut Context<Self>) {
        let outcome = self.notifier.handle(notification);
        // `flashFrame` while unfocused, like the desktop client.
        if outcome == Outcome::Toast && !self.window_active {
            let handle = self.window_handle;
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, _| window.request_attention());
            });
        }
    }

    /// A toast was clicked: bring the window forward and open its channel.
    pub(super) fn notification_clicked(
        &mut self,
        guild_id: Option<Snowflake>,
        channel_id: Snowflake,
        cx: &mut Context<Self>,
    ) {
        let handle = self.window_handle;
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, _| window.activate_window());
        });
        if self.backend.state().channels.contains_key(&channel_id) {
            self.open_channel(guild_id, channel_id, cx);
        }
    }

    /// The header's bell: toasts and sounds on/off for this session.
    pub(crate) fn toggle_notifications(&mut self, cx: &mut Context<Self>) {
        self.notifier.enabled = !self.notifier.enabled;
        cx.notify();
    }

    /// Queues files for the open channel; an error replaces the error line.
    /// Without ATTACH_FILES nothing is queued (`UploadManager.tsx`'s
    /// permission modal), so a send cannot fail with 403 after the tray
    /// was emptied.
    pub(crate) fn add_files(&mut self, files: Vec<NewFile>, cx: &mut Context<Self>) {
        let Some(channel_id) = self.selected_channel.filter(|_| self.can_send) else { return };
        if files.is_empty() {
            return;
        }
        if !self.backend.state().can_attach_files(channel_id) {
            self.error = Some("You do not have permission to attach files in this channel.".into());
            cx.notify();
            return;
        }
        match self.attachments.add(channel_id, files) {
            Ok(()) => self.error = None,
            Err(err) => self.error = Some(err.to_string().into()),
        }
        cx.notify();
    }

    /// Dropped or picked paths.
    pub(crate) fn add_paths(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        match attachments::files_from_paths(paths) {
            Ok(files) => self.add_files(files, cx),
            Err(err) => {
                self.error = Some(err.to_string().into());
                cx.notify();
            }
        }
    }

    /// The composer's + button: the system file picker.
    pub(crate) fn pick_files(&mut self, cx: &mut Context<Self>) {
        if self.selected_channel.is_none() || !self.can_send {
            return;
        }
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Upload".into()),
        });
        cx.spawn(async move |this, cx| {
            let paths = match picked.await {
                Ok(Ok(Some(paths))) => paths,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(err)) => {
                    tracing::warn!(%err, "file picker failed");
                    return;
                }
            };
            let _ = this.update(cx, |view, cx| view.add_paths(&paths, cx));
        })
        .detach();
    }

    /// Paste into the composer (`useChannelComposerPaste`): files and
    /// images go to the tray; text over the message length becomes
    /// `message.txt`. True when the paste was taken (the input skips it).
    pub(crate) fn paste(&mut self, item: &ClipboardItem, cx: &mut Context<Self>) -> bool {
        if self.selected_channel.is_none() || !self.can_send {
            return false;
        }
        let mut files = Vec::new();
        let mut paths = Vec::new();
        for entry in item.entries() {
            match entry {
                ClipboardEntry::ExternalPaths(p) => paths.extend(p.paths().iter().cloned()),
                ClipboardEntry::Image(image) => match self.attachments.pasted_image(image) {
                    Ok(file) => files.extend(file),
                    Err(err) => {
                        self.error = Some(err.to_string().into());
                        cx.notify();
                        return true;
                    }
                },
                ClipboardEntry::String(_) => {}
            }
        }
        if !paths.is_empty() {
            match attachments::files_from_paths(&paths) {
                Ok(from_paths) => files.extend(from_paths),
                Err(err) => {
                    self.error = Some(err.to_string().into());
                    cx.notify();
                    return true;
                }
            }
        }
        if !files.is_empty() {
            self.add_files(files, cx);
            return true;
        }
        match item.text() {
            Some(text) if attachments::is_long_text(&text) => {
                self.add_files(vec![attachments::text_file(&text)], cx);
                true
            }
            _ => false,
        }
    }

    pub(crate) fn remove_attachment(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(channel_id) = self.selected_channel
            && self.attachments.remove(channel_id, id)
        {
            cx.notify();
        }
    }

    pub(crate) fn toggle_attachment_spoiler(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(channel_id) = self.selected_channel
            && self.attachments.toggle_spoiler(channel_id, id)
        {
            cx.notify();
        }
    }

    /// Enter with queued files: text (may be empty) plus the files, as a
    /// reply when replying.
    pub(super) fn submit_with_files(
        &mut self,
        channel_id: Snowflake,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let content = text.trim().to_owned();
        if attachments::is_long_text(&content) {
            self.error = Some(
                format!("Messages are limited to {} characters.", attachments::MAX_MESSAGE_LENGTH).into(),
            );
            cx.notify();
            return;
        }
        let reply = match self.composer_mode.mode() {
            ComposerMode::Replying(target) => Some((target.message_id, target.mention)),
            _ => None,
        };
        if self.backend.state().current_user.is_none() {
            self.not_ready(cx);
            return;
        }
        let pending = self.attachments.take(channel_id);
        let files = pending.iter().map(PendingAttachment::upload_file).collect();
        let max_size = self.attachments.max_file_size;
        match self.backend.send_with_files(channel_id, content, files, reply, max_size) {
            Some(nonce) => {
                // Kept until the upload ends: a failed one puts them back.
                self.sent_files.insert(nonce, (channel_id, pending));
                if reply.is_some() {
                    self.composer_mode.channel_changed();
                }
                self.after_send(window, cx);
            }
            None => {
                self.attachments.restore(channel_id, pending);
                self.not_ready(cx);
            }
        }
    }

    /// The upload of `nonce` ended: when it failed (the row is `Failed`)
    /// its files go back to the channel's tray to send again.
    pub(super) fn upload_ended(&mut self, nonce: Snowflake) {
        self.uploads.remove(&nonce);
        let Some((channel_id, files)) = self.sent_files.remove(&nonce) else { return };
        let failed = self
            .backend
            .state()
            .messages
            .message(channel_id, nonce)
            .is_some_and(|m| m.state == MessageState::Failed);
        if failed && self.backend.state().channels.contains_key(&channel_id) {
            self.attachments.restore(channel_id, files);
        }
    }
}
