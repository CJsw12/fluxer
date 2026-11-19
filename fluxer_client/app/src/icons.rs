//! The SVG icons the app draws. gpui-kit's `Icon` only names a path; the
//! bytes come from the application's asset source, so without one every
//! `IconName` renders blank. [`AppAssets`] serves the gpui-kit component
//! bundle (the icons its widgets use) plus the extra Lucide icons below.

use std::borrow::Cow;

use anyhow::Result;
use gpui_kit::assets::{Assets, icon_assets};
use gpui_kit::{AssetSource, SharedString};

// Icons outside gpui-kit's default component bundle.
icon_assets!(ExtraIcons, [BellOff, FaceSlightlySmilingPlus, Hash, Pencil, Reply, Search, Trash, Users]);

/// Register with `Application::with_assets`.
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match ExtraIcons.load(path)? {
            Some(bytes) => Ok(Some(bytes)),
            None => Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = ExtraIcons.list(path)?;
        paths.extend(Assets.list(path)?);
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::assets::IconName;
    use gpui_kit::AssetSource as _;

    use super::AppAssets;

    /// Every icon the app names has bytes in the registered source.
    #[test]
    fn app_icons_load() {
        let used = [
            IconName::Bell,
            IconName::BellOff,
            IconName::Close,
            IconName::Eye,
            IconName::EyeOff,
            IconName::Plus,
            IconName::CircleX,
            IconName::Copy,
            IconName::FaceSlightlySmilingPlus,
            IconName::Hash,
            IconName::Pencil,
            IconName::Reply,
            IconName::Search,
            IconName::Settings,
            IconName::Trash,
            IconName::Users,
        ];
        for icon in used {
            let bytes = AppAssets.load(&icon.path()).ok().flatten();
            assert!(bytes.is_some_and(|b| b.starts_with(b"<svg")), "{icon:?} has no SVG");
        }
        assert!(AppAssets.load("icons/no-such-icon.svg").is_err(), "unknown paths still fail");
    }
}
