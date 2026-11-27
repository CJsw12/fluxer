//! Shared media right-click context menu (`MediaContextMenu.tsx`,
//! `MediaMenuData.tsx`). Non-T3 media actions only: favorites (backlog),
//! copy media (backlog), download/save, copy link, open link.

use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    Anchor, AnyElement, App, ClipboardItem, FontWeight, MouseButton, Pixels, Point, Role,
    SharedString, Size, Window, anchored, div, prelude::*, px,
};

use crate::client_view::ClientView;
use crate::icons::AppIcon;
use crate::parity_probe::FlxExt as _;
use crate::theme::Theme;
use crate::ui::l10n;

/// Shared media right-click context menu (`MediaContextMenu.tsx`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaContextMenu {
    pub target: MediaContextMenuTarget,
}

/// Media category matching official `MediaType` in `MediaContextMenu.tsx`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaMenuKind {
    Image,
    Gif,
    Gifv,
    Video,
    Audio,
    File,
}

/// Target metadata for opening [`MediaContextMenu`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaContextMenuTarget {
    pub kind: MediaMenuKind,
    pub url: SharedString,
    pub proxy_url: Option<SharedString>,
    pub default_name: Option<SharedString>,
}

impl MediaContextMenuTarget {
    pub fn new(kind: MediaMenuKind, url: impl Into<SharedString>) -> Self {
        Self {
            kind,
            url: url.into(),
            proxy_url: None,
            default_name: None,
        }
    }

    pub fn with_proxy(mut self, proxy_url: Option<SharedString>) -> Self {
        self.proxy_url = proxy_url;
        self
    }

    pub fn with_name(mut self, name: Option<SharedString>) -> Self {
        self.default_name = name;
        self
    }

    /// Best URL for saving/downloading (`proxy_url` if present, else `url`).
    pub fn download_url(&self) -> &str {
        self.proxy_url
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| self.url.as_ref())
    }
}

/// Non-T3 actions exposed by `MediaContextMenu` (`MediaMenuData.tsx`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaMenuAction {
    AddToFavorites,
    CopyMedia,
    DownloadMedia,
    CopyLink,
    OpenLink,
}

/// Menu groups in official order (`MediaMenuData.tsx` `groups`).
pub fn menu_groups(kind: MediaMenuKind) -> Vec<Vec<MediaMenuAction>> {
    let mut media_group = vec![MediaMenuAction::CopyMedia, MediaMenuAction::DownloadMedia];
    if kind != MediaMenuKind::File {
        media_group.push(MediaMenuAction::CopyLink);
    }
    media_group.push(MediaMenuAction::OpenLink);
    vec![vec![MediaMenuAction::AddToFavorites], media_group]
}

/// Localized label for `action` specialized by `kind` (`MediaMenuData.tsx`).
pub fn action_label(action: MediaMenuAction, kind: MediaMenuKind) -> String {
    match action {
        MediaMenuAction::AddToFavorites => l10n::tr("Add to favorites"),
        MediaMenuAction::CopyMedia => match kind {
            MediaMenuKind::Image => l10n::tr("Copy image"),
            MediaMenuKind::Gif | MediaMenuKind::Gifv => l10n::tr("Copy GIF"),
            MediaMenuKind::Video => l10n::tr("Copy video"),
            MediaMenuKind::Audio => l10n::tr("Copy audio"),
            MediaMenuKind::File => l10n::tr("Copy media"),
        },
        MediaMenuAction::DownloadMedia => match kind {
            MediaMenuKind::Image => l10n::tr("Download image"),
            MediaMenuKind::Gif | MediaMenuKind::Gifv => l10n::tr("Download GIF"),
            MediaMenuKind::Video => l10n::tr("Download video"),
            MediaMenuKind::Audio => l10n::tr("Download audio"),
            MediaMenuKind::File => l10n::tr("Download file"),
        },
        MediaMenuAction::CopyLink => match kind {
            MediaMenuKind::Image => l10n::tr("Copy image link"),
            MediaMenuKind::Gif | MediaMenuKind::Gifv => l10n::tr("Copy GIF link"),
            MediaMenuKind::Video => l10n::tr("Copy video link"),
            MediaMenuKind::Audio => l10n::tr("Copy audio link"),
            MediaMenuKind::File => l10n::tr("Copy file link"),
        },
        MediaMenuAction::OpenLink => match kind {
            MediaMenuKind::Image => l10n::tr("Open image link"),
            MediaMenuKind::Gif | MediaMenuKind::Gifv => l10n::tr("Open GIF link"),
            MediaMenuKind::Video => l10n::tr("Open video link"),
            MediaMenuKind::Audio => l10n::tr("Open audio link"),
            MediaMenuKind::File => l10n::tr("Open file link"),
        },
    }
}

pub fn action_icon(action: MediaMenuAction) -> AppIcon {
    match action {
        MediaMenuAction::AddToFavorites => AppIcon::Star,
        MediaMenuAction::CopyMedia => AppIcon::Copy,
        MediaMenuAction::DownloadMedia => AppIcon::Image,
        MediaMenuAction::CopyLink => AppIcon::LinkSimple,
        MediaMenuAction::OpenLink => AppIcon::ArrowSquareOut,
    }
}

pub fn action_flx(action: MediaMenuAction) -> &'static str {
    match action {
        MediaMenuAction::AddToFavorites => "ui.action-menu.items.media-menu-data.groups.favorite-icon--2",
        MediaMenuAction::CopyMedia => "ui.action-menu.items.media-menu-data.groups.copy-media-icon",
        MediaMenuAction::DownloadMedia => "ui.action-menu.items.media-menu-data.groups.download-media-icon",
        MediaMenuAction::CopyLink => "ui.action-menu.items.media-menu-data.groups.copy-link-icon",
        MediaMenuAction::OpenLink => "ui.action-menu.items.media-menu-data.groups.open-media-link-icon",
    }
}

pub fn menu_height(kind: MediaMenuKind) -> f32 {
    let groups = menu_groups(kind);
    let item_count: usize = groups.iter().map(Vec::len).sum();
    14. + (item_count as f32) * 32. + 9. * groups.len().saturating_sub(1) as f32
}

/// Executes a `MediaContextMenu` action. Each item has its own dedicated handler;
/// items without backend support yet close the menu and are marked `TODO(backlog)`.
pub fn execute_action(
    view: &mut ClientView,
    action: MediaMenuAction,
    target: &MediaContextMenuTarget,
    window: &mut Window,
    cx: &mut gpui_kit::Context<ClientView>,
) {
    match action {
        MediaMenuAction::AddToFavorites => handle_add_to_favorites(),
        MediaMenuAction::CopyMedia => handle_copy_media(),
        MediaMenuAction::DownloadMedia => handle_download_media(view, target, window, cx),
        MediaMenuAction::CopyLink => handle_copy_link(target, cx),
        MediaMenuAction::OpenLink => handle_open_link(view, target, window, cx),
    }
}

fn handle_add_to_favorites() {
    // TODO(backlog): wire favorite meme / GIF persistence once backend support lands.
}

fn handle_copy_media() {
    // TODO(backlog): encode image bitmap / media file to clipboard once GPUI supports binary clipboard items.
}

fn handle_download_media(
    view: &mut ClientView,
    target: &MediaContextMenuTarget,
    window: &mut Window,
    cx: &mut gpui_kit::Context<ClientView>,
) {
    let url = target.download_url().to_owned();
    if !url.is_empty() {
        view.open_external_url(&url, window, cx);
    }
}

fn handle_copy_link(target: &MediaContextMenuTarget, cx: &mut App) {
    if !target.url.is_empty() {
        cx.write_to_clipboard(ClipboardItem::new_string(target.url.to_string()));
    }
}

fn handle_open_link(
    view: &mut ClientView,
    target: &MediaContextMenuTarget,
    window: &mut Window,
    cx: &mut gpui_kit::Context<ClientView>,
) {
    if !target.url.is_empty() {
        view.open_external_url(target.url.as_ref(), window, cx);
    }
}

/// Renders the `MediaContextMenu` body inside a popout or floating container.
pub fn render_menu_body(
    target: &MediaContextMenuTarget,
    theme: &Theme,
    on_select: impl Fn(&MediaMenuAction, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let on_select = std::rc::Rc::new(on_select);
    let hover_bg = theme.hover_bg;
    let border_color = theme.border;
    let separator = move || {
        div()
            .mx(px(2.))
            .my(px(4.))
            .h(px(1.))
            .w(px(190.))
            .flex_none()
            .bg(border_color)
            .flx("ui.action-menu.context-menu.separator")
    };
    let mut body = div()
        .flex()
        .flex_col()
        .flx("ui.action-menu.context-menu.menu-scroller");
    for (gi, group) in menu_groups(target.kind).into_iter().enumerate() {
        if gi > 0 {
            body = body.child(separator());
        }
        let mut col = div()
            .flex()
            .flex_col()
            .flx("ui.action-menu.context-menu.menu-group.group");
        for action in group {
            let label = action_label(action, target.kind);
            let icon = action_icon(action);
            let flx_icon = action_flx(action);
            let on_select = on_select.clone();
            let id = format!("media-menu-{action:?}");
            let selector = id.clone();
            let row = div()
                .id(SharedString::from(id))
                .debug_selector(move || selector.clone())
                .role(Role::Button)
                .min_h(px(32.))
                .px(px(8.))
                .py(px(5.))
                .border_1()
                .border_color(gpui_kit::Hsla::transparent_black())
                .rounded(px(6.))
                .cursor_pointer()
                .hover(move |s| s.bg(hover_bg))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(8.))
                .text_size(px(13.))
                .line_height(px(18.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.foreground)
                .on_click(move |_, window, cx| on_select(&action, window, cx))
                .flx_text("ui.action-menu.context-menu.menu-item.item", label.clone())
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h(px(18.))
                        .flx("ui.action-menu.context-menu.menu-item.item-label")
                        .child(label),
                )
                .child(
                    div()
                        .size(px(18.))
                        .flex_none()
                        .flx("ui.action-menu.context-menu.menu-item.item-icon")
                        .child(
                            div()
                                .size(px(18.))
                                .flx(flx_icon)
                                .child(Icon::new(icon).with_size(px(18.)).text_color(theme.muted_foreground)),
                        ),
                );
            col = col.child(row);
        }
        body = body.child(col);
    }
    div().p(px(6.)).child(body).into_any_element()
}

/// Renders `MediaContextMenu` as a standalone floating popup (used inside `MediaViewerModal`).
pub fn render_floating(
    target: &MediaContextMenuTarget,
    anchor: Point<Pixels>,
    viewport: Size<Pixels>,
    theme: &Theme,
    on_select: impl Fn(&MediaMenuAction, &mut Window, &mut App) + 'static,
    on_dismiss: impl Fn(&gpui_kit::MouseDownEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let width = 208.0_f32;
    let height = menu_height(target.kind);
    let vw = f32::from(viewport.width);
    let vh = f32::from(viewport.height);
    let left = f32::from(anchor.x).clamp(12., (vw - width - 12.).max(12.));
    let top = f32::from(anchor.y).clamp(12., (vh - height - 12.).max(12.));

    let panel = div()
        .id("media-context-menu-floating")
        .debug_selector(|| "media-context-menu".to_owned())
        .w(px(width))
        .min_h(px(height))
        .rounded(px(8.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.background_secondary)
        .shadow_lg()
        .occlude()
        .flex()
        .flex_col()
        .overflow_hidden()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_down_out(on_dismiss)
        .flx("ui.action-menu.context-menu.popup")
        .child(render_menu_body(target, theme, on_select));

    div()
        .absolute()
        .top_0()
        .left_0()
        .child(anchored().position(gpui_kit::point(px(left), px(top))).anchor(Anchor::TopLeft).child(panel))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_groups_exclude_copy_link_for_files_and_include_for_media() {
        let img_groups = menu_groups(MediaMenuKind::Image);
        assert_eq!(img_groups.len(), 2);
        assert_eq!(img_groups[0], vec![MediaMenuAction::AddToFavorites]);
        assert_eq!(
            img_groups[1],
            vec![
                MediaMenuAction::CopyMedia,
                MediaMenuAction::DownloadMedia,
                MediaMenuAction::CopyLink,
                MediaMenuAction::OpenLink,
            ]
        );

        let file_groups = menu_groups(MediaMenuKind::File);
        assert_eq!(file_groups.len(), 2);
        assert_eq!(
            file_groups[1],
            vec![
                MediaMenuAction::CopyMedia,
                MediaMenuAction::DownloadMedia,
                MediaMenuAction::OpenLink,
            ]
        );
    }

    #[test]
    fn action_labels_match_media_kind() {
        assert_eq!(action_label(MediaMenuAction::CopyMedia, MediaMenuKind::Image), "Copy image");
        assert_eq!(action_label(MediaMenuAction::CopyMedia, MediaMenuKind::Gifv), "Copy GIF");
        assert_eq!(action_label(MediaMenuAction::DownloadMedia, MediaMenuKind::Video), "Download video");
        assert_eq!(action_label(MediaMenuAction::DownloadMedia, MediaMenuKind::File), "Download file");
        assert_eq!(action_label(MediaMenuAction::OpenLink, MediaMenuKind::Audio), "Open audio link");
    }

    #[test]
    fn target_download_url_prefers_proxy_when_present() {
        let target = MediaContextMenuTarget::new(MediaMenuKind::Image, "https://cdn.example/a.png")
            .with_proxy(Some("https://media.example/a.png".into()));
        assert_eq!(target.download_url(), "https://media.example/a.png");

        let no_proxy = MediaContextMenuTarget::new(MediaMenuKind::File, "https://cdn.example/doc.pdf");
        assert_eq!(no_proxy.download_url(), "https://cdn.example/doc.pdf");
    }
}
