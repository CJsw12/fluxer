//! Official Phosphor SVG icons embedded at compile time from MIT-licensed
//! `@phosphor-icons/core` (`assets/icons/`).
//!
//! [`AppAssets`] serves every [`AppIcon`] path (`icons/phosphor/*.svg`) and also
//! overrides matching `gpui_kit::assets::IconName` paths with Phosphor SVGs so
//! both app UI and `gpui-kit` widgets share consistent Fluxer iconography with
//! zero external file dependencies at runtime.

use std::borrow::Cow;

use anyhow::Result;
use gpui_kit::assets::{Assets, IconName, IconNamed};
use gpui_kit::{AssetSource, SharedString};

/// Application icon set backed by embedded Phosphor SVGs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppIcon {
    ArrowBendUpLeft,
    ArrowClockwise,
    ArrowSquareOut,
    Bell,
    BellSlash,
    CaretDown,
    CaretRight,
    ChatCircleFill,
    Check,
    Compass,
    Copy,
    Eye,
    EyeSlash,
    Gear,
    Hash,
    Headphones,
    Image,
    LinkSimple,
    MagnifyingGlass,
    Megaphone,
    Microphone,
    MicrophoneSlash,
    Monitor,
    PaperPlaneRight,
    PencilSimple,
    PhoneX,
    PhoneFill,
    VideoCameraFill,
    CameraSlash,
    MonitorPlay,
    Waveform,
    DotsThreeBold,
    Plus,
    Prohibit,
    PushPin,
    Screencast,
    SignOut,
    SlidersHorizontal,
    Smiley,
    SpeakerHigh,
    SpeakerSlash,
    Star,
    StarBold,
    Sticker,
    Trash,
    Tray,
    UserCircle,
    Users,
    UsersThree,
    UsersThreeFill,
    NotePencilFill,
    IdentificationBadge,
    WarningCircle,
    X,
    XCircle,
    AnnouncementChannel,
    Chats,
    ForumChannel,
    Inbox,
    LinkChannel,
    TextChannel,
    VoiceChannel,
    Shield,
    PaintBrush,
    Swatches,
    PersonSimpleCircle,
    Keyboard,
    Translate,
    Code,
    PencilBold,
    SignOutFill,
    CrownFill,
    PlusCircleFill,
    FolderPlusFill,
    UserPlusFill,
    SmileyRegular,
    Snowflake,
    ArrowBendUpRight,
    BookmarkSimple,
    EnvelopeOpen,
    LinkBold,
    AtBold,
    SparkleFill,
    UploadSimpleFill,
    PaperclipBold,
    MicrophoneBold,
    StopCircleFill,
    BookOpenFill,
    AppWindowFill,
    AppWindowBold,
    UserListFill,
    UserFill,
    DesktopFill,
    WarningFill,
    DotsThreeVerticalBold,
    UploadFill,
    ArrowCounterClockwiseBold,
    PaletteBold,
    ChatTeardropFill,
    SmileySadFill,
    ArrowLeftBold,
}

impl AppIcon {
    pub const ALL: &'static [Self] = &[
        Self::ArrowBendUpLeft,
        Self::ArrowClockwise,
        Self::ArrowSquareOut,
        Self::Bell,
        Self::BellSlash,
        Self::CaretDown,
        Self::CaretRight,
        Self::ChatCircleFill,
        Self::Check,
        Self::Compass,
        Self::Copy,
        Self::Eye,
        Self::EyeSlash,
        Self::Gear,
        Self::Hash,
        Self::Headphones,
        Self::Image,
        Self::LinkSimple,
        Self::MagnifyingGlass,
        Self::Megaphone,
        Self::Microphone,
        Self::MicrophoneSlash,
        Self::Monitor,
        Self::PaperPlaneRight,
        Self::PencilSimple,
        Self::PhoneX,
        Self::PhoneFill,
        Self::VideoCameraFill,
        Self::CameraSlash,
        Self::MonitorPlay,
        Self::Waveform,
        Self::DotsThreeBold,
        Self::Plus,
        Self::Prohibit,
        Self::PushPin,
        Self::Screencast,
        Self::SignOut,
        Self::SlidersHorizontal,
        Self::Smiley,
        Self::SpeakerHigh,
        Self::SpeakerSlash,
        Self::Star,
        Self::StarBold,
        Self::Sticker,
        Self::Trash,
        Self::Tray,
        Self::UserCircle,
        Self::Users,
        Self::UsersThree,
        Self::UsersThreeFill,
        Self::NotePencilFill,
        Self::IdentificationBadge,
        Self::WarningCircle,
        Self::X,
        Self::XCircle,
        Self::AnnouncementChannel,
        Self::Chats,
        Self::ForumChannel,
        Self::Inbox,
        Self::LinkChannel,
        Self::TextChannel,
        Self::VoiceChannel,
        Self::Shield,
        Self::PaintBrush,
        Self::Swatches,
        Self::PersonSimpleCircle,
        Self::Keyboard,
        Self::Translate,
        Self::Code,
        Self::PencilBold,
        Self::SignOutFill,
        Self::CrownFill,
        Self::PlusCircleFill,
        Self::FolderPlusFill,
        Self::UserPlusFill,
        Self::SmileyRegular,
        Self::Snowflake,
        Self::ArrowBendUpRight,
        Self::BookmarkSimple,
        Self::EnvelopeOpen,
        Self::LinkBold,
        Self::AtBold,
        Self::SparkleFill,
        Self::UploadSimpleFill,
        Self::PaperclipBold,
        Self::MicrophoneBold,
        Self::StopCircleFill,
        Self::BookOpenFill,
        Self::AppWindowFill,
        Self::AppWindowBold,
        Self::UserListFill,
        Self::UserFill,
        Self::DesktopFill,
        Self::WarningFill,
        Self::DotsThreeVerticalBold,
        Self::UploadFill,
        Self::ArrowCounterClockwiseBold,
        Self::PaletteBold,
        Self::ChatTeardropFill,
        Self::SmileySadFill,
        Self::ArrowLeftBold,
    ];

    pub const fn asset_path(self) -> &'static str {
        match self {
            Self::ArrowBendUpLeft => "icons/phosphor/arrow-bend-up-left.svg",
            Self::ArrowClockwise => "icons/phosphor/arrow-clockwise.svg",
            Self::ArrowSquareOut => "icons/phosphor/arrow-square-out.svg",
            Self::Bell => "icons/phosphor/bell.svg",
            Self::BellSlash => "icons/phosphor/bell-slash.svg",
            Self::CaretDown => "icons/phosphor/caret-down.svg",
            Self::CaretRight => "icons/phosphor/caret-right.svg",
            Self::ChatCircleFill => "icons/phosphor/chat-circle-fill.svg",
            Self::Check => "icons/phosphor/check.svg",
            Self::Compass => "icons/phosphor/compass.svg",
            Self::Copy => "icons/phosphor/copy.svg",
            Self::Eye => "icons/phosphor/eye.svg",
            Self::EyeSlash => "icons/phosphor/eye-slash.svg",
            Self::Gear => "icons/phosphor/gear.svg",
            Self::Hash => "icons/phosphor/hash.svg",
            Self::Headphones => "icons/phosphor/headphones.svg",
            Self::Image => "icons/phosphor/image.svg",
            Self::LinkSimple => "icons/phosphor/link-simple.svg",
            Self::MagnifyingGlass => "icons/phosphor/magnifying-glass.svg",
            Self::Megaphone => "icons/phosphor/megaphone.svg",
            Self::Microphone => "icons/phosphor/microphone.svg",
            Self::MicrophoneSlash => "icons/phosphor/microphone-slash.svg",
            Self::Monitor => "icons/phosphor/monitor.svg",
            Self::PaperPlaneRight => "icons/phosphor/paper-plane-right.svg",
            Self::PencilSimple => "icons/phosphor/pencil-simple.svg",
            Self::PhoneX => "icons/phosphor/phone-x.svg",
            Self::PhoneFill => "icons/phosphor/phone.svg",
            Self::VideoCameraFill => "icons/phosphor/video-camera.svg",
            Self::CameraSlash => "icons/phosphor/camera-slash.svg",
            Self::MonitorPlay => "icons/phosphor/monitor-play.svg",
            Self::Waveform => "icons/phosphor/waveform.svg",
            Self::DotsThreeBold => "icons/phosphor/dots-three-bold.svg",
            Self::Plus => "icons/phosphor/plus.svg",
            Self::Prohibit => "icons/phosphor/prohibit.svg",
            Self::PushPin => "icons/phosphor/push-pin.svg",
            Self::Screencast => "icons/phosphor/screencast.svg",
            Self::SignOut => "icons/phosphor/sign-out.svg",
            Self::SlidersHorizontal => "icons/phosphor/sliders-horizontal.svg",
            Self::Smiley => "icons/phosphor/smiley.svg",
            Self::SpeakerHigh => "icons/phosphor/speaker-high.svg",
            Self::SpeakerSlash => "icons/phosphor/speaker-slash.svg",
            Self::Star => "icons/phosphor/star.svg",
            Self::StarBold => "icons/phosphor/star-bold.svg",
            Self::Sticker => "icons/phosphor/sticker.svg",
            Self::Trash => "icons/phosphor/trash.svg",
            Self::Tray => "icons/phosphor/tray.svg",
            Self::UserCircle => "icons/phosphor/user-circle.svg",
            Self::Users => "icons/phosphor/users.svg",
            Self::UsersThree => "icons/phosphor/users-three.svg",
            Self::UsersThreeFill => "icons/phosphor/users-three-fill.svg",
            Self::NotePencilFill => "icons/phosphor/note-pencil-fill.svg",
            Self::IdentificationBadge => "icons/phosphor/identification-badge.svg",
            Self::WarningCircle => "icons/phosphor/warning-circle.svg",
            Self::X => "icons/phosphor/x.svg",
            Self::XCircle => "icons/phosphor/x-circle.svg",
            Self::AnnouncementChannel => "icons/phosphor/announcement-channel.svg",
            Self::Chats => "icons/phosphor/chats.svg",
            Self::ForumChannel => "icons/phosphor/forum-channel.svg",
            Self::Inbox => "icons/phosphor/inbox.svg",
            Self::LinkChannel => "icons/phosphor/link-channel.svg",
            Self::TextChannel => "icons/phosphor/text-channel.svg",
            Self::VoiceChannel => "icons/phosphor/voice-channel.svg",
            Self::Shield => "icons/phosphor/shield.svg",
            Self::PaintBrush => "icons/phosphor/paint-brush.svg",
            Self::Swatches => "icons/phosphor/swatches.svg",
            Self::PersonSimpleCircle => "icons/phosphor/person-simple-circle.svg",
            Self::Keyboard => "icons/phosphor/keyboard.svg",
            Self::Translate => "icons/phosphor/translate-bold.svg",
            Self::Code => "icons/phosphor/code-bold.svg",
            Self::PencilBold => "icons/phosphor/pencil-bold.svg",
            Self::SignOutFill => "icons/phosphor/sign-out-fill.svg",
            Self::CrownFill => "icons/phosphor/crown-fill.svg",
            Self::PlusCircleFill => "icons/phosphor/plus-circle-fill.svg",
            Self::FolderPlusFill => "icons/phosphor/folder-plus-fill.svg",
            Self::UserPlusFill => "icons/phosphor/user-plus-fill.svg",
            Self::SmileyRegular => "icons/phosphor/smiley-regular.svg",
            Self::Snowflake => "icons/phosphor/snowflake.svg",
            Self::ArrowBendUpRight => "icons/phosphor/arrow-bend-up-right.svg",
            Self::BookmarkSimple => "icons/phosphor/bookmark-simple.svg",
            Self::EnvelopeOpen => "icons/phosphor/envelope-open.svg",
            Self::LinkBold => "icons/phosphor/link-bold.svg",
            Self::AtBold => "icons/phosphor/at-bold.svg",
            Self::SparkleFill => "icons/phosphor/sparkle-fill.svg",
            Self::UploadSimpleFill => "icons/phosphor/upload-simple-fill.svg",
            Self::PaperclipBold => "icons/phosphor/paperclip-bold.svg",
            Self::MicrophoneBold => "icons/phosphor/microphone-bold.svg",
            Self::StopCircleFill => "icons/phosphor/stop-circle-fill.svg",
            Self::BookOpenFill => "icons/phosphor/book-open-fill.svg",
            Self::AppWindowFill => "icons/phosphor/app-window-fill.svg",
            Self::AppWindowBold => "icons/phosphor/app-window-bold.svg",
            Self::UserListFill => "icons/phosphor/user-list-fill.svg",
            Self::UserFill => "icons/phosphor/user-fill.svg",
            Self::DesktopFill => "icons/phosphor/desktop-fill.svg",
            Self::WarningFill => "icons/phosphor/warning-fill.svg",
            Self::DotsThreeVerticalBold => "icons/phosphor/dots-three-vertical-bold.svg",
            Self::UploadFill => "icons/phosphor/upload-fill.svg",
            Self::ArrowCounterClockwiseBold => "icons/phosphor/arrow-counter-clockwise-bold.svg",
            Self::PaletteBold => "icons/phosphor/palette-bold.svg",
            Self::ChatTeardropFill => "icons/phosphor/chat-teardrop-fill.svg",
            Self::SmileySadFill => "icons/phosphor/smiley-sad-fill.svg",
            Self::ArrowLeftBold => "icons/phosphor/arrow-left-bold.svg",
        }
    }

    pub const fn svg_bytes(self) -> &'static [u8] {
        match self {
            Self::ArrowBendUpLeft => include_bytes!("../assets/icons/arrow-bend-up-left.svg"),
            Self::ArrowClockwise => include_bytes!("../assets/icons/arrow-clockwise.svg"),
            Self::ArrowSquareOut => include_bytes!("../assets/icons/arrow-square-out.svg"),
            Self::Bell => include_bytes!("../assets/icons/bell.svg"),
            Self::BellSlash => include_bytes!("../assets/icons/bell-slash.svg"),
            Self::CaretDown => include_bytes!("../assets/icons/caret-down.svg"),
            Self::CaretRight => include_bytes!("../assets/icons/caret-right.svg"),
            Self::ChatCircleFill => include_bytes!("../assets/icons/chat-circle-fill.svg"),
            Self::Check => include_bytes!("../assets/icons/check.svg"),
            Self::Compass => include_bytes!("../assets/icons/compass.svg"),
            Self::Copy => include_bytes!("../assets/icons/copy.svg"),
            Self::Eye => include_bytes!("../assets/icons/eye.svg"),
            Self::EyeSlash => include_bytes!("../assets/icons/eye-slash.svg"),
            Self::Gear => include_bytes!("../assets/icons/gear.svg"),
            Self::Hash => include_bytes!("../assets/icons/hash.svg"),
            Self::Headphones => include_bytes!("../assets/icons/headphones.svg"),
            Self::Image => include_bytes!("../assets/icons/image.svg"),
            Self::LinkSimple => include_bytes!("../assets/icons/link-simple.svg"),
            Self::MagnifyingGlass => include_bytes!("../assets/icons/magnifying-glass.svg"),
            Self::Megaphone => include_bytes!("../assets/icons/megaphone.svg"),
            Self::Microphone => include_bytes!("../assets/icons/microphone.svg"),
            Self::MicrophoneSlash => include_bytes!("../assets/icons/microphone-slash.svg"),
            Self::Monitor => include_bytes!("../assets/icons/monitor.svg"),
            Self::PaperPlaneRight => include_bytes!("../assets/icons/paper-plane-right.svg"),
            Self::PencilSimple => include_bytes!("../assets/icons/pencil-simple.svg"),
            Self::PhoneX => include_bytes!("../assets/icons/phone-x.svg"),
            Self::PhoneFill => include_bytes!("../assets/icons/phone.svg"),
            Self::VideoCameraFill => include_bytes!("../assets/icons/video-camera.svg"),
            Self::CameraSlash => include_bytes!("../assets/icons/camera-slash.svg"),
            Self::MonitorPlay => include_bytes!("../assets/icons/monitor-play.svg"),
            Self::Waveform => include_bytes!("../assets/icons/waveform.svg"),
            Self::DotsThreeBold => include_bytes!("../assets/icons/dots-three-bold.svg"),
            Self::Plus => include_bytes!("../assets/icons/plus.svg"),
            Self::Prohibit => include_bytes!("../assets/icons/prohibit.svg"),
            Self::PushPin => include_bytes!("../assets/icons/push-pin.svg"),
            Self::Screencast => include_bytes!("../assets/icons/screencast.svg"),
            Self::SignOut => include_bytes!("../assets/icons/sign-out.svg"),
            Self::SlidersHorizontal => include_bytes!("../assets/icons/sliders-horizontal.svg"),
            Self::Smiley => include_bytes!("../assets/icons/smiley.svg"),
            Self::SpeakerHigh => include_bytes!("../assets/icons/speaker-high.svg"),
            Self::SpeakerSlash => include_bytes!("../assets/icons/speaker-slash.svg"),
            Self::Star => include_bytes!("../assets/icons/star.svg"),
            Self::StarBold => include_bytes!("../assets/icons/star-bold.svg"),
            Self::Sticker => include_bytes!("../assets/icons/sticker.svg"),
            Self::Trash => include_bytes!("../assets/icons/trash.svg"),
            Self::Tray => include_bytes!("../assets/icons/tray.svg"),
            Self::UserCircle => include_bytes!("../assets/icons/user-circle.svg"),
            Self::Users => include_bytes!("../assets/icons/users.svg"),
            Self::UsersThree => include_bytes!("../assets/icons/users-three.svg"),
            Self::UsersThreeFill => include_bytes!("../assets/icons/users-three-fill.svg"),
            Self::NotePencilFill => include_bytes!("../assets/icons/note-pencil-fill.svg"),
            Self::IdentificationBadge => include_bytes!("../assets/icons/identification-badge.svg"),
            Self::WarningCircle => include_bytes!("../assets/icons/warning-circle.svg"),
            Self::X => include_bytes!("../assets/icons/x.svg"),
            Self::XCircle => include_bytes!("../assets/icons/x-circle.svg"),
            Self::AnnouncementChannel => include_bytes!("../assets/icons/announcement-channel.svg"),
            Self::Chats => include_bytes!("../assets/icons/chats.svg"),
            Self::ForumChannel => include_bytes!("../assets/icons/forum-channel.svg"),
            Self::Inbox => include_bytes!("../assets/icons/inbox.svg"),
            Self::LinkChannel => include_bytes!("../assets/icons/link-channel.svg"),
            Self::TextChannel => include_bytes!("../assets/icons/text-channel.svg"),
            Self::VoiceChannel => include_bytes!("../assets/icons/voice-channel.svg"),
            Self::Shield => include_bytes!("../assets/icons/shield.svg"),
            Self::PaintBrush => include_bytes!("../assets/icons/paint-brush.svg"),
            Self::Swatches => include_bytes!("../assets/icons/swatches.svg"),
            Self::PersonSimpleCircle => include_bytes!("../assets/icons/person-simple-circle.svg"),
            Self::Keyboard => include_bytes!("../assets/icons/keyboard.svg"),
            Self::Translate => include_bytes!("../assets/icons/translate-bold.svg"),
            Self::Code => include_bytes!("../assets/icons/code-bold.svg"),
            Self::PencilBold => include_bytes!("../assets/icons/pencil-bold.svg"),
            Self::SignOutFill => include_bytes!("../assets/icons/sign-out-fill.svg"),
            Self::CrownFill => include_bytes!("../assets/icons/crown-fill.svg"),
            Self::PlusCircleFill => include_bytes!("../assets/icons/plus-circle-fill.svg"),
            Self::FolderPlusFill => include_bytes!("../assets/icons/folder-plus-fill.svg"),
            Self::UserPlusFill => include_bytes!("../assets/icons/user-plus-fill.svg"),
            Self::SmileyRegular => include_bytes!("../assets/icons/smiley-regular.svg"),
            Self::Snowflake => include_bytes!("../assets/icons/snowflake.svg"),
            Self::ArrowBendUpRight => include_bytes!("../assets/icons/arrow-bend-up-right.svg"),
            Self::BookmarkSimple => include_bytes!("../assets/icons/bookmark-simple.svg"),
            Self::EnvelopeOpen => include_bytes!("../assets/icons/envelope-open.svg"),
            Self::LinkBold => include_bytes!("../assets/icons/link-bold.svg"),
            Self::AtBold => include_bytes!("../assets/icons/at-bold.svg"),
            Self::SparkleFill => include_bytes!("../assets/icons/sparkle-fill.svg"),
            Self::UploadSimpleFill => include_bytes!("../assets/icons/upload-simple-fill.svg"),
            Self::PaperclipBold => include_bytes!("../assets/icons/paperclip-bold.svg"),
            Self::MicrophoneBold => include_bytes!("../assets/icons/microphone-bold.svg"),
            Self::StopCircleFill => include_bytes!("../assets/icons/stop-circle-fill.svg"),
            Self::BookOpenFill => include_bytes!("../assets/icons/book-open-fill.svg"),
            Self::AppWindowFill => include_bytes!("../assets/icons/app-window-fill.svg"),
            Self::AppWindowBold => include_bytes!("../assets/icons/app-window-bold.svg"),
            Self::UserListFill => include_bytes!("../assets/icons/user-list-fill.svg"),
            Self::UserFill => include_bytes!("../assets/icons/user-fill.svg"),
            Self::DesktopFill => include_bytes!("../assets/icons/desktop-fill.svg"),
            Self::WarningFill => include_bytes!("../assets/icons/warning-fill.svg"),
            Self::DotsThreeVerticalBold => include_bytes!("../assets/icons/dots-three-vertical-bold.svg"),
            Self::UploadFill => include_bytes!("../assets/icons/upload-fill.svg"),
            Self::ArrowCounterClockwiseBold => include_bytes!("../assets/icons/arrow-counter-clockwise-bold.svg"),
            Self::PaletteBold => include_bytes!("../assets/icons/palette-bold.svg"),
            Self::ChatTeardropFill => include_bytes!("../assets/icons/chat-teardrop-fill.svg"),
            Self::SmileySadFill => include_bytes!("../assets/icons/smiley-sad-fill.svg"),
            Self::ArrowLeftBold => include_bytes!("../assets/icons/arrow-left-bold.svg"),
        }
    }
}

impl IconNamed for AppIcon {
    fn path(self) -> SharedString {
        SharedString::new_static(self.asset_path())
    }
}

fn load_phosphor(path: &str) -> Option<&'static [u8]> {
    for &icon in AppIcon::ALL {
        if path == icon.asset_path() {
            return Some(icon.svg_bytes());
        }
    }
    // Override Lucide `IconName` paths used by app/gpui-kit widgets with Phosphor SVGs.
    let mapped = if path == IconName::Bell.path().as_ref() {
        Some(AppIcon::Bell)
    } else if path == IconName::BellOff.path().as_ref() {
        Some(AppIcon::BellSlash)
    } else if path == IconName::Close.path().as_ref() {
        Some(AppIcon::X)
    } else if path == IconName::Eye.path().as_ref() {
        Some(AppIcon::Eye)
    } else if path == IconName::EyeOff.path().as_ref() {
        Some(AppIcon::EyeSlash)
    } else if path == IconName::Plus.path().as_ref() {
        Some(AppIcon::Plus)
    } else if path == IconName::CircleX.path().as_ref() {
        Some(AppIcon::XCircle)
    } else if path == IconName::Copy.path().as_ref() {
        Some(AppIcon::Copy)
    } else if path == IconName::ExternalLink.path().as_ref() {
        Some(AppIcon::ArrowSquareOut)
    } else if path == IconName::FaceSlightlySmilingPlus.path().as_ref() {
        Some(AppIcon::Smiley)
    } else if path == IconName::Hash.path().as_ref() {
        Some(AppIcon::Hash)
    } else if path == IconName::Pencil.path().as_ref() {
        Some(AppIcon::PencilSimple)
    } else if path == IconName::Reply.path().as_ref() {
        Some(AppIcon::ArrowBendUpLeft)
    } else if path == IconName::RotateCw.path().as_ref() {
        Some(AppIcon::ArrowClockwise)
    } else if path == IconName::Search.path().as_ref() {
        Some(AppIcon::MagnifyingGlass)
    } else if path == IconName::Settings.path().as_ref() {
        Some(AppIcon::Gear)
    } else if path == IconName::Settings2.path().as_ref() {
        Some(AppIcon::SlidersHorizontal)
    } else if path == IconName::Trash.path().as_ref() {
        Some(AppIcon::Trash)
    } else if path == IconName::Users.path().as_ref() {
        Some(AppIcon::Users)
    } else if path == IconName::Volume2.path().as_ref() {
        Some(AppIcon::SpeakerHigh)
    } else if path == IconName::VolumeX.path().as_ref() {
        Some(AppIcon::SpeakerSlash)
    } else if path == IconName::Check.path().as_ref() {
        Some(AppIcon::Check)
    } else if path == IconName::ChevronDown.path().as_ref() {
        Some(AppIcon::CaretDown)
    } else {
        None
    };
    mapped.map(AppIcon::svg_bytes)
}

/// Register with `Application::with_assets`.
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = load_phosphor(path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths: Vec<SharedString> = AppIcon::ALL
            .iter()
            .map(|icon| icon.asset_path())
            .filter(|name| name.starts_with(path))
            .map(SharedString::new_static)
            .collect();
        paths.extend(Assets.list(path)?);
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::AssetSource as _;
    use gpui_kit::assets::{IconName, IconNamed as _};

    use super::{AppAssets, AppIcon};

    /// Every embedded Phosphor `AppIcon` and overridden `IconName` loads valid
    /// Phosphor SVG bytes (`viewBox="0 0 256 256"`), with unknown paths failing.
    #[test]
    fn app_icons_load() {
        for &icon in AppIcon::ALL {
            let path = icon.path();
            let bytes = AppAssets.load(path.as_ref()).ok().flatten();
            let Some(bytes) = bytes else {
                panic!("{icon:?} ({path}) has no SVG");
            };
            let text = std::str::from_utf8(&bytes).expect("valid utf-8 svg");
            assert!(text.starts_with("<svg"), "{icon:?} does not start with <svg");
            // Fluxer's own Inbox glyph uses a 24x24 viewBox.
            let expected = if icon == AppIcon::Inbox { "viewBox=\"0 0 24 24\"" } else { "viewBox=\"0 0 256 256\"" };
            assert!(text.contains(expected), "{icon:?} has an unexpected viewBox");
            assert!(text.contains("<path"), "{icon:?} has no <path element");
            let has_nonempty_d = text.split(" d=\"").skip(1).any(|rest| {
                rest.split('"')
                    .next()
                    .is_some_and(|d| !d.trim().is_empty())
            });
            assert!(has_nonempty_d, "{icon:?} has no non-empty <path d=...> attribute");
        }

        // Verify every `rust_asset` referenced in tools/parity_lab/icon_manifest.json
        // matches an embedded `AppIcon` asset path.
        let manifest_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/parity_lab/icon_manifest.json");
        if let Ok(manifest_text) = std::fs::read_to_string(&manifest_path) {
            let embedded_assets: std::collections::HashSet<String> = AppIcon::ALL
                .iter()
                .map(|icon| {
                    let file = icon.asset_path().rsplit('/').next().unwrap_or("");
                    format!("fluxer_client/app/assets/icons/{file}")
                })
                .collect();
            for line in manifest_text.lines() {
                let trimmed = line.trim();
                if let Some(rest) = trimmed.strip_prefix("\"rust_asset\": \"") {
                    let asset = rest.trim_end_matches(',').trim_end_matches('"');
                    assert!(
                        embedded_assets.contains(asset),
                        "manifest rust_asset {asset} is not in AppIcon::ALL"
                    );
                }
            }
        }

        let used = [
            IconName::Bell,
            IconName::BellOff,
            IconName::Close,
            IconName::Eye,
            IconName::EyeOff,
            IconName::Plus,
            IconName::CircleX,
            IconName::Copy,
            IconName::ExternalLink,
            IconName::FaceSlightlySmilingPlus,
            IconName::Hash,
            IconName::Pencil,
            IconName::Reply,
            IconName::RotateCw,
            IconName::Search,
            IconName::Settings,
            IconName::Settings2,
            IconName::Trash,
            IconName::Users,
            IconName::Volume2,
            IconName::VolumeX,
        ];
        for icon in used {
            let bytes = AppAssets.load(&icon.path()).ok().flatten();
            let Some(bytes) = bytes else {
                panic!("{icon:?} has no SVG");
            };
            let text = std::str::from_utf8(&bytes).expect("valid utf-8 svg");
            assert!(text.starts_with("<svg"), "{icon:?} has no SVG");
            assert!(
                text.contains("viewBox=\"0 0 256 256\""),
                "{icon:?} should resolve to Phosphor SVG override"
            );
        }

        assert!(AppAssets.load("icons/no-such-icon.svg").is_err(), "unknown paths still fail");
    }
}
