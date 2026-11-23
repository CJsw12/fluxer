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
    ChatsCircle,
    Check,
    Compass,
    Copy,
    Eye,
    EyeSlash,
    Gear,
    Globe,
    Hash,
    Headphones,
    Image,
    LinkSimple,
    MagnifyingGlass,
    Megaphone,
    Microphone,
    MicrophoneSlash,
    Monitor,
    Palette,
    PaperPlaneRight,
    PencilSimple,
    PersonArmsSpread,
    PhoneDisconnect,
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
    Sticker,
    Trash,
    Tray,
    UserCircle,
    Users,
    WarningCircle,
    Wrench,
    X,
    XCircle,
}

impl AppIcon {
    pub const ALL: &'static [Self] = &[
        Self::ArrowBendUpLeft,
        Self::ArrowClockwise,
        Self::ArrowSquareOut,
        Self::Bell,
        Self::BellSlash,
        Self::CaretDown,
        Self::ChatsCircle,
        Self::Check,
        Self::Compass,
        Self::Copy,
        Self::Eye,
        Self::EyeSlash,
        Self::Gear,
        Self::Globe,
        Self::Hash,
        Self::Headphones,
        Self::Image,
        Self::LinkSimple,
        Self::MagnifyingGlass,
        Self::Megaphone,
        Self::Microphone,
        Self::MicrophoneSlash,
        Self::Monitor,
        Self::Palette,
        Self::PaperPlaneRight,
        Self::PencilSimple,
        Self::PersonArmsSpread,
        Self::PhoneDisconnect,
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
        Self::Sticker,
        Self::Trash,
        Self::Tray,
        Self::UserCircle,
        Self::Users,
        Self::WarningCircle,
        Self::Wrench,
        Self::X,
        Self::XCircle,
    ];

    pub const fn asset_path(self) -> &'static str {
        match self {
            Self::ArrowBendUpLeft => "icons/phosphor/arrow-bend-up-left.svg",
            Self::ArrowClockwise => "icons/phosphor/arrow-clockwise.svg",
            Self::ArrowSquareOut => "icons/phosphor/arrow-square-out.svg",
            Self::Bell => "icons/phosphor/bell.svg",
            Self::BellSlash => "icons/phosphor/bell-slash.svg",
            Self::CaretDown => "icons/phosphor/caret-down.svg",
            Self::ChatsCircle => "icons/phosphor/chats-circle.svg",
            Self::Check => "icons/phosphor/check.svg",
            Self::Compass => "icons/phosphor/compass.svg",
            Self::Copy => "icons/phosphor/copy.svg",
            Self::Eye => "icons/phosphor/eye.svg",
            Self::EyeSlash => "icons/phosphor/eye-slash.svg",
            Self::Gear => "icons/phosphor/gear.svg",
            Self::Globe => "icons/phosphor/globe.svg",
            Self::Hash => "icons/phosphor/hash.svg",
            Self::Headphones => "icons/phosphor/headphones.svg",
            Self::Image => "icons/phosphor/image.svg",
            Self::LinkSimple => "icons/phosphor/link-simple.svg",
            Self::MagnifyingGlass => "icons/phosphor/magnifying-glass.svg",
            Self::Megaphone => "icons/phosphor/megaphone.svg",
            Self::Microphone => "icons/phosphor/microphone.svg",
            Self::MicrophoneSlash => "icons/phosphor/microphone-slash.svg",
            Self::Monitor => "icons/phosphor/monitor.svg",
            Self::Palette => "icons/phosphor/palette.svg",
            Self::PaperPlaneRight => "icons/phosphor/paper-plane-right.svg",
            Self::PencilSimple => "icons/phosphor/pencil-simple.svg",
            Self::PersonArmsSpread => "icons/phosphor/person-arms-spread.svg",
            Self::PhoneDisconnect => "icons/phosphor/phone-disconnect.svg",
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
            Self::Sticker => "icons/phosphor/sticker.svg",
            Self::Trash => "icons/phosphor/trash.svg",
            Self::Tray => "icons/phosphor/tray.svg",
            Self::UserCircle => "icons/phosphor/user-circle.svg",
            Self::Users => "icons/phosphor/users.svg",
            Self::WarningCircle => "icons/phosphor/warning-circle.svg",
            Self::Wrench => "icons/phosphor/wrench.svg",
            Self::X => "icons/phosphor/x.svg",
            Self::XCircle => "icons/phosphor/x-circle.svg",
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
            Self::ChatsCircle => include_bytes!("../assets/icons/chats-circle.svg"),
            Self::Check => include_bytes!("../assets/icons/check.svg"),
            Self::Compass => include_bytes!("../assets/icons/compass.svg"),
            Self::Copy => include_bytes!("../assets/icons/copy.svg"),
            Self::Eye => include_bytes!("../assets/icons/eye.svg"),
            Self::EyeSlash => include_bytes!("../assets/icons/eye-slash.svg"),
            Self::Gear => include_bytes!("../assets/icons/gear.svg"),
            Self::Globe => include_bytes!("../assets/icons/globe.svg"),
            Self::Hash => include_bytes!("../assets/icons/hash.svg"),
            Self::Headphones => include_bytes!("../assets/icons/headphones.svg"),
            Self::Image => include_bytes!("../assets/icons/image.svg"),
            Self::LinkSimple => include_bytes!("../assets/icons/link-simple.svg"),
            Self::MagnifyingGlass => include_bytes!("../assets/icons/magnifying-glass.svg"),
            Self::Megaphone => include_bytes!("../assets/icons/megaphone.svg"),
            Self::Microphone => include_bytes!("../assets/icons/microphone.svg"),
            Self::MicrophoneSlash => include_bytes!("../assets/icons/microphone-slash.svg"),
            Self::Monitor => include_bytes!("../assets/icons/monitor.svg"),
            Self::Palette => include_bytes!("../assets/icons/palette.svg"),
            Self::PaperPlaneRight => include_bytes!("../assets/icons/paper-plane-right.svg"),
            Self::PencilSimple => include_bytes!("../assets/icons/pencil-simple.svg"),
            Self::PersonArmsSpread => include_bytes!("../assets/icons/person-arms-spread.svg"),
            Self::PhoneDisconnect => include_bytes!("../assets/icons/phone-disconnect.svg"),
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
            Self::Sticker => include_bytes!("../assets/icons/sticker.svg"),
            Self::Trash => include_bytes!("../assets/icons/trash.svg"),
            Self::Tray => include_bytes!("../assets/icons/tray.svg"),
            Self::UserCircle => include_bytes!("../assets/icons/user-circle.svg"),
            Self::Users => include_bytes!("../assets/icons/users.svg"),
            Self::WarningCircle => include_bytes!("../assets/icons/warning-circle.svg"),
            Self::Wrench => include_bytes!("../assets/icons/wrench.svg"),
            Self::X => include_bytes!("../assets/icons/x.svg"),
            Self::XCircle => include_bytes!("../assets/icons/x-circle.svg"),
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
            assert!(
                text.contains("viewBox=\"0 0 256 256\""),
                "{icon:?} is not a Phosphor 256x256 SVG"
            );
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
