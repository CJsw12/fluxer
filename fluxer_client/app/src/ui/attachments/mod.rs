//! Files waiting to be sent with the next message, per channel
//! (`UploadManager.tsx`, `FileUploadUtils.ts`, `ClipboardFilePasteUtils.ts`,
//! `CloudUpload.ts`). Pure state; `tray` draws it above the composer.
//!
//! Rules copied from the web client: at most 10 files per message counting
//! those already queued; every file is checked against the size limit
//! before it is queued; pasted text over the message length becomes
//! `message.txt`; a `SPOILER_` file name starts out as a spoiler; a
//! spoiler is sent as the `IS_SPOILER` flag with the file name unchanged
//! (`CloudUpload.ts`). The ATTACH_FILES gate is the caller's
//! (`ClientState::can_attach_files`). Not ported: alt text, reordering,
//! the per-user `max_attachment_file_size` (25 MiB default is used).

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bytes::Bytes;
use fluxer_client_core::rest::{DEFAULT_MAX_ATTACHMENT_FILE_SIZE, MAX_ATTACHMENTS_PER_MESSAGE, UploadFile, UploadSource};
use fluxer_client_core::state::{ATTACHMENT_FLAG_SPOILER, Snowflake};
use gpui_kit::Image;

use crate::ui::l10n;

pub mod tray;

/// `MAX_MESSAGE_LENGTH_NON_PREMIUM`: longer pasted text becomes a file.
pub const MAX_MESSAGE_LENGTH: usize = 2000;
const SPOILER_PREFIX: &str = "SPOILER_";

/// How the tray previews a file.
#[derive(Debug, Clone, PartialEq)]
pub enum Preview {
    /// An image file on disk.
    Path(PathBuf),
    /// A pasted image.
    Image(Arc<Image>),
    /// Not an image: name and size only.
    None,
}

/// A file to queue.
#[derive(Debug, Clone, PartialEq)]
pub struct NewFile {
    pub source: UploadSource,
    pub name: String,
    pub mime: Option<String>,
    pub size: u64,
    pub preview: Preview,
}

/// A queued file.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingAttachment {
    pub id: u64,
    pub file: NewFile,
    pub spoiler: bool,
}

impl PendingAttachment {
    /// The upload part: the name as picked, with the spoiler flag when
    /// marked as a spoiler (the web client never renames the file).
    pub fn upload_file(&self) -> UploadFile {
        UploadFile {
            source: self.file.source.clone(),
            filename: self.file.name.clone(),
            content_type: self.file.mime.clone(),
            description: None,
            flags: if self.spoiler { ATTACHMENT_FLAG_SPOILER } else { 0 },
        }
    }
}

/// Why files were not queued (the whole batch is rejected).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachError {
    TooMany { max: usize },
    TooLarge { name: String, size: u64, limit: u64 },
    Unreadable { name: String, error: String },
}

impl fmt::Display for AttachError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooMany { max } => write!(f, "You can only attach up to {max} files per message."),
            Self::TooLarge { name, size, limit } => write!(
                f,
                "{}",
                l10n::tr_args_or_en(
                    "One or more files you're trying to upload exceed the maximum size limit of {maxSizeFormatted} per file.",
                    &format!("{name} is too large ({}; the limit is {}).", format_size(*size), format_size(*limit)),
                    &[("maxSizeFormatted", &format_size(*limit))],
                )
            ),
            Self::Unreadable { name, error } => write!(f, "Could not read {name}: {error}"),
        }
    }
}

/// `validateFiles` for a batch: count first, then each file's size.
pub fn validate_add(existing: usize, new: &[NewFile], max_count: usize, max_size: u64) -> Result<(), AttachError> {
    if existing + new.len() > max_count {
        return Err(AttachError::TooMany { max: max_count });
    }
    match new.iter().find(|f| f.size > max_size) {
        Some(f) => Err(AttachError::TooLarge { name: f.name.clone(), size: f.size, limit: max_size }),
        None => Ok(()),
    }
}

/// The queued files of every channel.
#[derive(Debug)]
pub struct AttachmentTray {
    by_channel: BTreeMap<Snowflake, Vec<PendingAttachment>>,
    next_id: u64,
    /// Counter for `clipboard-image-N` names.
    pasted_images: u32,
    pub max_count: usize,
    pub max_file_size: u64,
}

impl Default for AttachmentTray {
    fn default() -> Self {
        Self {
            by_channel: BTreeMap::new(),
            next_id: 1,
            pasted_images: 0,
            max_count: MAX_ATTACHMENTS_PER_MESSAGE,
            max_file_size: DEFAULT_MAX_ATTACHMENT_FILE_SIZE,
        }
    }
}

impl AttachmentTray {
    pub fn files(&self, channel_id: Snowflake) -> &[PendingAttachment] {
        self.by_channel.get(&channel_id).map_or(&[], Vec::as_slice)
    }

    /// Queues `new` for `channel_id`, or none of them.
    pub fn add(&mut self, channel_id: Snowflake, new: Vec<NewFile>) -> Result<(), AttachError> {
        let queued = self.files(channel_id).len();
        validate_add(queued, &new, self.max_count, self.max_file_size)?;
        let list = self.by_channel.entry(channel_id).or_default();
        for file in new {
            let spoiler = file.name.starts_with(SPOILER_PREFIX);
            list.push(PendingAttachment { id: self.next_id, file, spoiler });
            self.next_id += 1;
        }
        Ok(())
    }

    pub fn remove(&mut self, channel_id: Snowflake, id: u64) -> bool {
        let Some(list) = self.by_channel.get_mut(&channel_id) else { return false };
        let before = list.len();
        list.retain(|a| a.id != id);
        let removed = list.len() != before;
        if list.is_empty() {
            self.by_channel.remove(&channel_id);
        }
        removed
    }

    pub fn toggle_spoiler(&mut self, channel_id: Snowflake, id: u64) -> bool {
        let item = self.by_channel.get_mut(&channel_id).and_then(|l| l.iter_mut().find(|a| a.id == id));
        match item {
            Some(item) => {
                item.spoiler = !item.spoiler;
                true
            }
            None => false,
        }
    }

    /// Empties the channel's tray (Enter); [`PendingAttachment::upload_file`]
    /// makes the upload parts. Keep them until the send is confirmed.
    pub fn take(&mut self, channel_id: Snowflake) -> Vec<PendingAttachment> {
        self.by_channel.remove(&channel_id).unwrap_or_default()
    }

    /// Puts files of a send that failed back in front of the channel's
    /// tray. Files queued meanwhile stay in the tray even if it exceeds the send limit.
    pub fn restore(&mut self, channel_id: Snowflake, mut files: Vec<PendingAttachment>) {
        if files.is_empty() {
            return;
        }
        let list = self.by_channel.entry(channel_id).or_default();
        files.append(list);
        *list = files;
    }

    /// Drops the trays of channels that no longer exist (their bytes would
    /// stay until exit otherwise).
    pub fn retain_channels(&mut self, exists: impl Fn(Snowflake) -> bool) {
        self.by_channel.retain(|id, _| exists(*id));
    }

    /// A pasted image as `clipboard-image-N.ext` (empty images are skipped).
    /// One over the size limit is rejected before its bytes are copied.
    pub fn pasted_image(&mut self, image: &Image) -> Result<Option<NewFile>, AttachError> {
        if image.bytes.is_empty() {
            return Ok(None);
        }
        let size = u64::try_from(image.bytes.len()).unwrap_or(u64::MAX);
        if size > self.max_file_size {
            let name = format!("clipboard-image.{}", image.format.extension());
            return Err(AttachError::TooLarge { name, size, limit: self.max_file_size });
        }
        self.pasted_images += 1;
        let format = image.format;
        Ok(Some(NewFile {
            source: UploadSource::Bytes(Bytes::copy_from_slice(&image.bytes)),
            name: format!("clipboard-image-{}.{}", self.pasted_images, format.extension()),
            mime: Some(format.mime_type().to_owned()),
            size,
            preview: Preview::Image(Arc::new(Image::from_bytes(format, image.bytes.clone()))),
        }))
    }
}

/// A file on disk (picker, drop, pasted file entry).
pub fn file_from_path(path: &Path) -> Result<NewFile, AttachError> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".to_owned());
    let meta = std::fs::metadata(path).map_err(|e| AttachError::Unreadable { name: name.clone(), error: e.to_string() })?;
    if !meta.is_file() {
        return Err(AttachError::Unreadable { name, error: "not a file".to_owned() });
    }
    let mime = mime_for(&name);
    let preview = match mime {
        Some(m) if m.starts_with("image/") => Preview::Path(path.to_path_buf()),
        _ => Preview::None,
    };
    Ok(NewFile { source: UploadSource::Path(path.to_path_buf()), name, mime: mime.map(str::to_owned), size: meta.len(), preview })
}

/// Paths of a drop or a file picker; the first unreadable one fails all.
pub fn files_from_paths(paths: &[PathBuf]) -> Result<Vec<NewFile>, AttachError> {
    paths.iter().map(|p| file_from_path(p)).collect()
}

/// `convertTextToFile`: long pasted text as `message.txt`.
pub fn text_file(text: &str) -> NewFile {
    NewFile {
        source: UploadSource::Bytes(Bytes::copy_from_slice(text.as_bytes())),
        name: "message.txt".to_owned(),
        mime: Some("text/plain".to_owned()),
        size: u64::try_from(text.len()).unwrap_or(u64::MAX),
        preview: Preview::None,
    }
}

/// Text too long for one message. The server (and the web client's
/// `text.length`) counts UTF-16 code units: an emoji counts twice.
pub fn is_long_text(text: &str) -> bool {
    text.encode_utf16().count() > MAX_MESSAGE_LENGTH
}

/// MIME type by extension for the common types; `None` sends
/// `application/octet-stream`.
pub fn mime_for(name: &str) -> Option<&'static str> {
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "avif" => "image/avif",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "txt" | "log" => "text/plain",
        "md" => "text/markdown",
        "json" => "application/json",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        _ => return None,
    })
}

/// "512 B", "1.5 KB", "25.0 MB" (1024-based like the web `formatFileSize`).
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, size: u64) -> NewFile {
        NewFile { source: UploadSource::Bytes(Bytes::new()), name: name.to_owned(), mime: None, size, preview: Preview::None }
    }

    #[test]
    fn validate_counts_queued_files_then_sizes() {
        let two = [file("a", 1), file("b", 1)];
        assert_eq!(validate_add(8, &two, 10, 5), Ok(()));
        assert_eq!(validate_add(9, &two, 10, 5), Err(AttachError::TooMany { max: 10 }));
        let big = [file("a", 1), file("huge.bin", 6)];
        assert_eq!(validate_add(0, &big, 10, 5), Err(AttachError::TooLarge { name: "huge.bin".into(), size: 6, limit: 5 }));
    }

    /// A spoiler is the flag only; the file keeps its name (web client).
    #[test]
    fn spoiler_flag_keeps_the_name() {
        let mut tray = AttachmentTray::default();
        let ch = Snowflake(1);
        tray.add(ch, vec![file("a.png", 1), file("SPOILER_b.png", 1)]).expect("added");
        let first = tray.files(ch)[0].id;
        assert!(tray.toggle_spoiler(ch, first));
        let parts: Vec<UploadFile> = tray.take(ch).iter().map(PendingAttachment::upload_file).collect();
        let summary: Vec<(&str, u32)> = parts.iter().map(|p| (p.filename.as_str(), p.flags)).collect();
        assert_eq!(summary, [("a.png", ATTACHMENT_FLAG_SPOILER), ("SPOILER_b.png", ATTACHMENT_FLAG_SPOILER)]);
        assert!(tray.files(ch).is_empty());
    }

    /// Files of a failed send come back first without dropping newly queued files.
    #[test]
    fn restore_puts_failed_files_back() {
        let mut tray = AttachmentTray::default();
        let ch = Snowflake(1);
        tray.add(ch, vec![file("a", 1), file("b", 1)]).expect("added");
        let taken = tray.take(ch);
        tray.add(ch, (0..9).map(|i| file(&format!("new{i}"), 1)).collect()).expect("added");
        tray.restore(ch, taken);
        let names: Vec<&str> = tray.files(ch).iter().map(|f| f.file.name.as_str()).collect();
        assert_eq!(names.len(), 11);
        assert_eq!(&names[..3], ["a", "b", "new0"]);
        assert_eq!(names.last(), Some(&"new8"));
        tray.retain_channels(|id| id != ch);
        assert!(tray.files(ch).is_empty());
    }

    #[test]
    fn text_threshold_mime_and_sizes() {
        assert!(!is_long_text(&"é".repeat(2000)), "counted in UTF-16 units");
        assert!(is_long_text(&"é".repeat(2001)));
        assert!(is_long_text(&"😀".repeat(1001)), "an emoji is two units: 2002");
        assert!(!is_long_text(&"😀".repeat(1000)));
        let t = text_file("hello");
        assert_eq!((t.name.as_str(), t.mime.as_deref(), t.size), ("message.txt", Some("text/plain"), 5));
        assert_eq!(mime_for("Photo.JPG"), Some("image/jpeg"));
        assert_eq!(mime_for("archive.7z"), None);
        assert_eq!(mime_for("noext"), None);
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(25 * 1024 * 1024), "25.0 MB");
    }

    #[test]
    fn pasted_images_are_numbered_and_empty_ones_skipped() {
        let mut tray = AttachmentTray::default();
        assert_eq!(tray.pasted_image(&Image::empty()), Ok(None));
        let png = Image::from_bytes(gpui_kit::ImageFormat::Png, vec![1, 2, 3]);
        let f = tray.pasted_image(&png).expect("within the limit").expect("image");
        assert_eq!((f.name.as_str(), f.mime.as_deref(), f.size), ("clipboard-image-1.png", Some("image/png"), 3));
        tray.max_file_size = 2;
        assert!(matches!(tray.pasted_image(&png), Err(AttachError::TooLarge { size: 3, limit: 2, .. })));
    }
}
