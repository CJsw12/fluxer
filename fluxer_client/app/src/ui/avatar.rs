//! Shared user avatar renderer (`StatusAwareAvatar.tsx` / `BaseAvatar.tsx`):
//! loads `avatar_url` when available, falls back to the deterministic Fluxer
//! default avatar asset for `user_id` (or an initials circle), and optionally
//! places a presence status indicator dot at the bottom-right corner.

use std::sync::{Arc, OnceLock};

use fluxer_client_core::state::{Snowflake, Status};
use gpui_kit::{
    AnyElement, FontWeight, Hsla, Image, ImageFormat, ObjectFit, SharedString, StyledImage as _, div, img,
    prelude::*, px, rgb,
};

use super::member_list::status_color;
use crate::format::{DEFAULT_AVATAR_COLORS, default_avatar_index, initials};
use crate::theme::Theme;

/// Input specification for [`render_avatar`].
#[derive(Debug, Clone)]
pub struct AvatarSpec {
    pub user_id: Snowflake,
    pub avatar_url: Option<SharedString>,
    pub display_name: SharedString,
    pub size: f32,
    pub status: Option<Status>,
    pub status_border_bg: Option<Hsla>,
}

/// Renders a circular user avatar with optional bottom-right presence dot.
pub fn render_avatar(spec: AvatarSpec, theme: &Theme) -> AnyElement {
    let size = spec.size.max(12.);
    let default_index = default_avatar_index(spec.user_id);
    let user_initials: SharedString = initials(&spec.display_name).into();
    let color = default_avatar_color(default_index);
    let text_color = theme.on_fill;
    let font_px = (size * 0.4).clamp(10., 28.);

    let circle = move || {
        div()
            .size(px(size))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(color)
            .text_color(text_color)
            .text_size(px(font_px))
            .font_weight(FontWeight::SEMIBOLD)
            .child(user_initials.clone())
            .into_any_element()
    };

    let default_image = default_avatar(default_index);
    let fallback = {
        let circle = circle.clone();
        move || match default_image.clone() {
            Some(image) => img(image).size(px(size)).rounded_full().object_fit(ObjectFit::Cover).into_any_element(),
            None => circle(),
        }
    };

    let picture = match spec.avatar_url.filter(|url| !url.is_empty()) {
        Some(url) => div()
            .size(px(size))
            .flex_none()
            .rounded_full()
            .overflow_hidden()
            .debug_selector(|| "avatar-image".to_owned())
            .child(
                img(url)
                    .size(px(size))
                    .rounded_full()
                    .object_fit(ObjectFit::Cover)
                    .with_loading(circle)
                    .with_fallback(fallback),
            )
            .into_any_element(),
        None => div()
            .size(px(size))
            .flex_none()
            .rounded_full()
            .overflow_hidden()
            .debug_selector(|| "avatar-default".to_owned())
            .child(fallback())
            .into_any_element(),
    };

    let Some(status) = spec.status else {
        return picture;
    };

    let dot_size = status_dot_size(size, theme);
    let offset = if size >= 48. { 0. } else { -2. };
    let border_bg = spec.status_border_bg.unwrap_or(theme.background);
    let dot = div()
        .debug_selector(|| "avatar-status-dot".to_owned())
        .absolute()
        .right(px(offset))
        .bottom(px(offset))
        .size(px(dot_size))
        .rounded_full()
        .border_2()
        .border_color(border_bg)
        .bg(status_color(status, theme));

    div()
        .relative()
        .flex_none()
        .size(px(size))
        .child(picture)
        .child(dot)
        .into_any_element()
}

fn status_dot_size(avatar_size: f32, theme: &Theme) -> f32 {
    if avatar_size <= 24. {
        8.
    } else if avatar_size <= 44. {
        theme.sizes.status_dot + 2.
    } else {
        (avatar_size * 0.24).clamp(14., 22.)
    }
}

fn default_avatar_color(index: usize) -> Hsla {
    rgb(DEFAULT_AVATAR_COLORS.get(index).copied().unwrap_or(DEFAULT_AVATAR_COLORS[0])).into()
}

/// The web client's default avatars (`fluxer_app/src/media/images/avatars`),
/// decoded once.
fn default_avatar(index: usize) -> Option<Arc<Image>> {
    static IMAGES: OnceLock<Vec<Arc<Image>>> = OnceLock::new();
    const PNGS: [&[u8]; 6] = [
        include_bytes!("../../assets/avatars/0.png"),
        include_bytes!("../../assets/avatars/1.png"),
        include_bytes!("../../assets/avatars/2.png"),
        include_bytes!("../../assets/avatars/3.png"),
        include_bytes!("../../assets/avatars/4.png"),
        include_bytes!("../../assets/avatars/5.png"),
    ];
    IMAGES
        .get_or_init(|| PNGS.iter().map(|png| Arc::new(Image::from_bytes(ImageFormat::Png, png.to_vec()))).collect())
        .get(index)
        .cloned()
}

#[cfg(test)]
mod tests {
    use fluxer_client_core::Dispatch;
    use fluxer_client_core::rest::{ClientIdentity, Endpoints, MessageSearchOutcome, MessageSearchPage, RestClient, RestConfig};
    use fluxer_client_core::state::{LoadMode, Message, Snowflake, Status, bits};
    use gpui_kit::{Context, Render, TestAppContext, VisualTestContext, Window, div};
    use serde_json::json;

    use super::*;
    use crate::backend::{Backend, UiEvent};
    use crate::client_view::ClientView;
    use crate::view_model::Place;

    struct AvatarHost {
        spec: AvatarSpec,
    }

    impl Render for AvatarHost {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let theme = Theme::current(cx);
            div().p_4().child(render_avatar(self.spec.clone(), &theme))
        }
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
    }

    #[gpui_kit::test]
    fn renders_image_when_avatar_url_present_and_default_when_absent(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (_, vcx) = cx.add_window_view(|window, cx| {
            let host = cx.new(|_| AvatarHost {
                spec: AvatarSpec {
                    user_id: Snowflake(42),
                    avatar_url: Some("https://media.example/avatars/42/abc.webp?size=80".into()),
                    display_name: "Alice".into(),
                    size: 40.,
                    status: Some(Status::Online),
                    status_border_bg: None,
                },
            });
            gpui_kit::base::Root::new(host, window, cx)
        });
        draw(vcx);
        assert!(vcx.debug_bounds("avatar-image").is_some(), "avatar_url renders image element");
        assert!(vcx.debug_bounds("avatar-status-dot").is_some(), "status renders status dot");
        assert!(vcx.debug_bounds("avatar-default").is_none());

        let (_, vcx2) = cx.add_window_view(|window, cx| {
            let host = cx.new(|_| AvatarHost {
                spec: AvatarSpec {
                    user_id: Snowflake(43),
                    avatar_url: None,
                    display_name: "Bob".into(),
                    size: 32.,
                    status: None,
                    status_border_bg: None,
                },
            });
            gpui_kit::base::Root::new(host, window, cx)
        });
        draw(vcx2);
        assert!(vcx2.debug_bounds("avatar-default").is_some(), "missing url renders default fallback");
        assert!(vcx2.debug_bounds("avatar-image").is_none());
        assert!(vcx2.debug_bounds("avatar-status-dot").is_none());
    }

    #[gpui_kit::test]
    fn avatar_urls_render_across_dm_row_dm_banner_settings_search_and_members(cx: &mut TestAppContext) {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("rt");
        let endpoints = Endpoints {
            api_code_version: 1,
            api: "https://fluxer.invalid/api".to_owned(),
            gateway: "wss://fluxer.invalid/gateway".to_owned(),
            media: Some("https://media.example".to_owned()),
            static_cdn: None,
            upload_relay: None,
            invite: None,
            webapp: None,
            discovery_url: "https://fluxer.invalid/.well-known/fluxer".to_owned(),
            document: json!({}),
        };
        let rest = Arc::new(RestClient::new(RestConfig::new(&endpoints, ClientIdentity::default())).expect("rest"));
        let (mut backend, events) = Backend::new(rt.handle().clone(), rest, None);
        backend.set_media_for_test(Some("https://media.example".to_owned()));
        let perms = (bits::VIEW_CHANNEL | bits::SEND_MESSAGES | bits::READ_MESSAGE_HISTORY).to_string();
        {
            let mut state = backend.state();
            state.apply(&Dispatch {
                t: "READY".to_owned(),
                s: 1,
                d: json!({
                    "session_id": "s1",
                    "user": { "id": "100", "username": "me", "global_name": "Me", "avatar": "mehash" },
                    "guilds": [{
                        "id": "1",
                        "properties": { "name": "Guild", "owner_id": "100" },
                        "roles": [{ "id": "1", "name": "@everyone", "permissions": perms }],
                        "channels": [{ "id": "11", "type": 0, "name": "general", "position": 0 }],
                        "members": [
                            { "user": { "id": "100", "username": "me", "avatar": "mehash" }, "roles": [] },
                            { "user": { "id": "201", "username": "alice", "global_name": "Alice", "avatar": "alicehash" }, "roles": [] }
                        ]
                    }],
                    "private_channels": [{
                        "id": "30",
                        "type": 1,
                        "recipients": [{ "id": "201", "username": "alice", "global_name": "Alice", "avatar": "alicehash" }]
                    }],
                    "read_states": []
                }),
            });
            state.apply(&Dispatch {
                t: "GUILD_MEMBER_LIST_UPDATE".to_owned(),
                s: 2,
                d: json!({
                    "guild_id": "1",
                    "id": "everyone",
                    "channel_id": "11",
                    "member_count": 1,
                    "online_count": 1,
                    "groups": [{ "id": "online", "count": 1 }],
                    "ops": [{
                        "op": "SYNC",
                        "range": [0, 1],
                        "items": [
                            { "group": { "id": "online", "count": 1 } },
                            { "member": {
                                "user": { "id": "201", "username": "alice", "global_name": "Alice", "avatar": "alicehash" },
                                "roles": [],
                                "presence": { "status": "online" }
                            } }
                        ]
                    }]
                }),
            });
            state.messages.load(Snowflake(30), Vec::new(), LoadMode::Replace);
        }

        cx.update(gpui_kit::init);
        cx.update(crate::shortcuts::bind_keys);
        let mut content = None;
        let (_, vcx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| ClientView::new(backend, events, window, cx));
            content = Some(view.clone());
            gpui_kit::base::Root::new(view, window, cx)
        });
        let view = content.expect("view");

        // 1. DM row + empty DM welcome banner both render avatar image
        view.update(vcx, |v, cx| {
            v.select_place(Place::Home, cx);
            v.select_channel(Snowflake(30), cx);
        });
        vcx.run_until_parked();
        draw(vcx);
        view.read_with(vcx, |v, _| {
            let dm = v.channels.iter().find(|c| c.id == Snowflake(30)).expect("dm channel");
            assert_eq!(dm.dm_user_id, Some(Snowflake(201)));
            assert_eq!(
                dm.dm_avatar_url.as_deref(),
                Some("https://media.example/avatars/201/alicehash.webp?size=80")
            );
        });
        assert!(vcx.debug_bounds("dm-row-avatar").is_some(), "DM row renders shared avatar");
        assert!(vcx.debug_bounds("dm-banner-avatar").is_some(), "DM welcome banner renders shared avatar");
        assert!(vcx.debug_bounds("avatar-image").is_some(), "avatar image rendered for DM");

        // 2. Settings sidebar profile renders avatar image
        view.update_in(vcx, |v, window, cx| v.open_settings(window, cx));
        vcx.run_until_parked();
        draw(vcx);
        assert!(vcx.debug_bounds("settings-profile-avatar").is_some(), "settings profile renders shared avatar");
        assert!(vcx.debug_bounds("avatar-image").is_some(), "settings profile avatar image rendered");
        view.update_in(vcx, |v, window, cx| v.close_overlay(window, cx));
        vcx.run_until_parked();

        // 3. Guild member list renders member avatar image
        view.update(vcx, |v, cx| v.select_place(Place::Guild(Snowflake(1)), cx));
        vcx.run_until_parked();
        draw(vcx);
        assert!(vcx.debug_bounds("member-panel").is_some());
        assert!(vcx.debug_bounds("member-row-1").is_some());
        assert!(vcx.debug_bounds("avatar-image").is_some(), "member list avatar image rendered");

        // 4. Search result renders author avatar image
        view.update_in(vcx, |v, window, cx| {
            v.open_search(window, cx);
            if let Some(panel) = &v.search {
                panel.input.update(cx, |s, cx| s.set_value("hello", window, cx));
            }
            v.search_submit(cx);
        });
        vcx.run_until_parked();
        let seq = view.read_with(vcx, |v, _| v.search.as_ref().map(|s| s.request_seq)).unwrap();
        let msg: Message = serde_json::from_value(json!({
            "id": "555",
            "channel_id": "11",
            "guild_id": "1",
            "author": { "id": "201", "username": "alice", "global_name": "Alice", "avatar": "alicehash" },
            "content": "hello world",
            "timestamp": "2026-11-18T10:20:30.000000+00:00"
        }))
        .expect("msg");
        view.update(vcx, |v, cx| {
            v.handle_events(
                vec![UiEvent::SearchLoaded {
                    seq,
                    result: Ok(MessageSearchOutcome::Results(MessageSearchPage {
                        messages: vec![msg],
                        channels: Vec::new(),
                        threads: Vec::new(),
                        total: 1,
                        hits_per_page: 25,
                        page: 1,
                        cursor: None,
                    })),
                }],
                cx,
            );
        });
        vcx.run_until_parked();
        draw(vcx);
        view.read_with(vcx, |v, _| {
            let hit = &v.search.as_ref().unwrap().hits[0];
            assert_eq!(
                hit.avatar_url.as_deref(),
                Some("https://media.example/avatars/201/alicehash.webp?size=80")
            );
        });
        assert!(vcx.debug_bounds("search-hit-avatar").is_some(), "search hit renders shared avatar");
        assert!(vcx.debug_bounds("avatar-image").is_some(), "search hit avatar image rendered");

        drop(rt);
    }
}
