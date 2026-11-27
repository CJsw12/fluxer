//! Upload tray edit modal and reordering (`AttachmentEditModal.tsx`,
//! `ChannelAttachmentArea.tsx`): alt text limits, spoiler, stable ids.

use bytes::Bytes;
use fluxer_client_core::rest::UploadSource;
use fluxer_client_core::state::{ATTACHMENT_FLAG_SPOILER, Snowflake};

use crate::ui::attachments::{
    AttachmentEdit, AttachmentTray, DropPosition, EditError, MAX_ALT_TEXT_LENGTH, NewFile, PendingAttachment, Preview,
    normalize_description,
};

fn file(name: &str, mime: Option<&str>) -> NewFile {
    NewFile { source: UploadSource::Bytes(Bytes::new()), name: name.to_owned(), mime: mime.map(str::to_owned), size: 1, preview: Preview::None }
}

fn tray_with(names: &[&str]) -> (AttachmentTray, Snowflake, Vec<u64>) {
    let mut tray = AttachmentTray::default();
    let ch = Snowflake(7);
    tray.add(ch, names.iter().map(|n| file(n, Some("image/png"))).collect()).expect("added");
    let ids = tray.files(ch).iter().map(|a| a.id).collect();
    (tray, ch, ids)
}

fn names(tray: &AttachmentTray, ch: Snowflake) -> Vec<&str> {
    tray.files(ch).iter().map(|a| a.file.name.as_str()).collect()
}

fn edit(a: &PendingAttachment, description: &str) -> AttachmentEdit {
    AttachmentEdit { description: description.to_owned(), ..AttachmentEdit::from_pending(a) }
}

#[test]
fn description_is_trimmed_empty_is_none_and_capped() {
    assert_eq!(normalize_description("  hello \n"), Some("hello".to_owned()));
    assert_eq!(normalize_description(" \t\n "), None);
    assert_eq!(normalize_description(""), None);
    let exact = "a".repeat(MAX_ALT_TEXT_LENGTH);
    assert_eq!(normalize_description(&exact).as_deref(), Some(exact.as_str()));
    let over = "é".repeat(MAX_ALT_TEXT_LENGTH + 5);
    assert_eq!(normalize_description(&over).map(|d| d.chars().count()), Some(MAX_ALT_TEXT_LENGTH));
}

#[test]
fn edit_sets_description_spoiler_and_name_and_blank_clears() {
    let (mut tray, ch, ids) = tray_with(&["a.png"]);
    let e = AttachmentEdit { filename: "b.png".into(), spoiler: true, description: "  a cat  ".into() };
    assert_eq!(tray.edit(ch, ids[0], &e), Ok(()));
    let a = &tray.files(ch)[0];
    assert_eq!((a.file.name.as_str(), a.spoiler, a.description.as_deref()), ("b.png", true, Some("a cat")));
    let blank = edit(a, "   ");
    tray.edit(ch, ids[0], &blank).expect("edited");
    assert_eq!(tray.files(ch)[0].description, None, "whitespace only is None");
    // The modal prefills from the current state.
    tray.edit(ch, ids[0], &edit(&tray.files(ch)[0].clone(), "x")).expect("edited");
    assert_eq!(AttachmentEdit::from_pending(&tray.files(ch)[0]).description, "x");
}

#[test]
fn alt_text_only_for_image_and_video_files() {
    let mut tray = AttachmentTray::default();
    let ch = Snowflake(1);
    tray.add(ch, vec![file("a.txt", Some("text/plain")), file("b.bin", None), file("c.mp4", Some("Video/MP4"))]).expect("added");
    for a in tray.files(ch).to_vec() {
        tray.edit(ch, a.id, &edit(&a, "desc")).expect("edited");
    }
    let got: Vec<Option<&str>> = tray.files(ch).iter().map(|a| a.description.as_deref()).collect();
    assert_eq!(got, [None, None, Some("desc")]);
}

#[test]
fn bad_filename_or_unknown_id_changes_nothing() {
    let (mut tray, ch, ids) = tray_with(&["a.png"]);
    for bad in ["", "a/b.png", "a\\b.png", "a\"b", "a\nb", &"x".repeat(256)] {
        let e = AttachmentEdit { filename: bad.to_owned(), spoiler: true, description: "d".into() };
        assert_eq!(tray.edit(ch, ids[0], &e), Err(EditError::BadFilename), "{bad:?}");
    }
    let a = &tray.files(ch)[0];
    assert_eq!((a.file.name.as_str(), a.spoiler, a.description.as_ref()), ("a.png", false, None));
    let ok = AttachmentEdit::from_pending(a);
    assert_eq!(tray.edit(ch, 999, &ok), Err(EditError::NotFound));
    assert_eq!(tray.edit(Snowflake(8), ids[0], &ok), Err(EditError::NotFound));
}

#[test]
fn spoiler_edit_and_toggle_agree_and_payload_carries_them() {
    let (mut tray, ch, ids) = tray_with(&["a.png", "b.png"]);
    let e = AttachmentEdit { filename: "a2.png".into(), spoiler: true, description: " alt ".into() };
    tray.edit(ch, ids[0], &e).expect("edited");
    assert!(tray.toggle_spoiler(ch, ids[1]));
    assert!(tray.toggle_spoiler(ch, ids[1]), "second toggle turns it off again");
    let parts: Vec<_> = tray.take(ch).iter().map(PendingAttachment::upload_file).collect();
    assert_eq!(parts[0].filename, "a2.png");
    assert_eq!(parts[0].description.as_deref(), Some("alt"));
    assert_eq!(parts[0].flags, ATTACHMENT_FLAG_SPOILER);
    assert_eq!((parts[1].description.as_deref(), parts[1].flags), (None, 0));
    // A spoiler switched off in the modal clears the flag.
    let (mut tray, ch, ids) = tray_with(&["SPOILER_c.png"]);
    let e = AttachmentEdit { spoiler: false, ..AttachmentEdit::from_pending(&tray.files(ch)[0]) };
    tray.edit(ch, ids[0], &e).expect("edited");
    assert_eq!(tray.files(ch)[0].upload_file().flags, 0);
}

#[test]
fn drop_position_uses_the_left_half_as_before() {
    assert_eq!(DropPosition::from_offset(10.0, 100.0), DropPosition::Before);
    assert_eq!(DropPosition::from_offset(50.0, 100.0), DropPosition::After);
    assert_eq!(DropPosition::from_offset(90.0, 100.0), DropPosition::After);
}

#[test]
fn reorder_moves_before_or_after_and_keeps_ids_stable() {
    use DropPosition::{After, Before};
    let (mut tray, ch, ids) = tray_with(&["a", "b", "c", "d"]);
    assert!(tray.reorder(ch, ids[0], ids[2], After));
    assert_eq!(names(&tray, ch), ["b", "c", "a", "d"]);
    assert!(tray.reorder(ch, ids[3], ids[1], Before));
    assert_eq!(names(&tray, ch), ["d", "b", "c", "a"]);
    assert!(tray.reorder(ch, ids[0], ids[3], Before), "a just before d: moving left");
    assert_eq!(names(&tray, ch), ["a", "d", "b", "c"]);
    // Official quirk kept: moving right with Before is not decremented, so it lands after the target.
    assert!(tray.reorder(ch, ids[0], ids[2], Before));
    assert_eq!(names(&tray, ch), ["d", "b", "c", "a"]);
    let after: Vec<u64> = tray.files(ch).iter().map(|a| a.id).collect();
    assert_eq!(after, [ids[3], ids[1], ids[2], ids[0]], "ids follow their files");
}

#[test]
fn reorder_rejects_same_or_unknown_ids_without_changes() {
    let (mut tray, ch, ids) = tray_with(&["a", "b"]);
    assert!(!tray.reorder(ch, ids[0], ids[0], DropPosition::After));
    assert!(!tray.reorder(ch, ids[0], 999, DropPosition::After));
    assert!(!tray.reorder(ch, 999, ids[0], DropPosition::Before));
    assert!(!tray.reorder(Snowflake(99), ids[0], ids[1], DropPosition::Before));
    assert_eq!(names(&tray, ch), ["a", "b"]);
}

#[test]
fn reorder_survives_removal_and_the_payload_follows_the_order() {
    let (mut tray, ch, ids) = tray_with(&["a", "b", "c"]);
    assert!(tray.remove(ch, ids[1]));
    assert!(!tray.reorder(ch, ids[0], ids[1], DropPosition::After), "the target was removed");
    assert!(tray.reorder(ch, ids[2], ids[0], DropPosition::Before));
    let order: Vec<String> = tray.take(ch).iter().map(|a| a.upload_file().filename).collect();
    assert_eq!(order, ["c", "a"]);
}

#[test]
fn edits_and_order_are_per_channel_and_retain_channels_drops_them() {
    let mut tray = AttachmentTray::default();
    let (c1, c2) = (Snowflake(1), Snowflake(2));
    tray.add(c1, vec![file("a.png", Some("image/png")), file("b.png", Some("image/png"))]).expect("added");
    tray.add(c2, vec![file("x.png", Some("image/png"))]).expect("added");
    let ids: Vec<u64> = tray.files(c1).iter().map(|a| a.id).collect();
    let other = tray.files(c2)[0].id;
    assert_eq!(tray.edit(c1, other, &AttachmentEdit { filename: "n.png".into(), spoiler: false, description: String::new() }), Err(EditError::NotFound));
    assert!(!tray.reorder(c1, ids[0], other, DropPosition::After), "ids of another channel are unknown");
    assert!(tray.reorder(c1, ids[0], ids[1], DropPosition::After));
    tray.retain_channels(|id| id == c1);
    assert_eq!(names(&tray, c1), ["b.png", "a.png"]);
    assert!(tray.files(c2).is_empty());
}
