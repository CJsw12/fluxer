//! Headless UI tests for Media Viewer lightbox, Trusted Domain Link Warning
//! modal, and Font/Density/UI Zoom display settings.

use fluxer_client_core::Dispatch;
use fluxer_client_core::rest::UserSettingsPatch;
use fluxer_client_core::state::{LoadMode, Message, Snowflake, bits};
use gpui_kit::{Focusable as _, Modifiers, TestAppContext, VisualTestContext};
use serde_json::json;

use crate::backend::{Backend, lock_sent};
use crate::client_view::ClientView;
use crate::tests::{draw, focus_composer, msg_id, offline_rest, open_client, runtime};
use crate::theme::Theme;
use crate::ui::overlay::Overlay;
use crate::view_model::Place;

const GUILD: Snowflake = Snowflake(1);
const GENERAL: Snowflake = Snowflake(11);

fn media_ready() -> Dispatch {
    let perms = bits::VIEW_CHANNEL | bits::SEND_MESSAGES | bits::READ_MESSAGE_HISTORY;
    Dispatch {
        t: "READY".to_owned(),
        s: 1,
        d: json!({
            "session_id": "s",
            "user": { "id": "100", "username": "me" },
            "guilds": [{
                "id": GUILD.to_string(),
                "properties": { "name": "Rust Port", "owner_id": "999" },
                "roles": [{ "id": GUILD.to_string(), "name": "@everyone", "permissions": perms.to_string() }],
                "channels": [{ "id": GENERAL.to_string(), "type": 0, "name": "general", "position": 0 }],
                "members": [{ "user": { "id": "100", "username": "me" }, "roles": [] }]
            }],
            "private_channels": [],
            "read_states": []
        }),
    }
}

fn open_media_fixture(
    cx: &mut TestAppContext,
    instance_hosts: Vec<String>,
) -> (tokio::runtime::Runtime, Backend, gpui_kit::Entity<ClientView>, &mut VisualTestContext) {
    let rt = runtime();
    let (mut backend, events) = Backend::new(rt.handle().clone(), offline_rest(), None);
    if !instance_hosts.is_empty() {
        backend.set_instance_hosts_for_test(instance_hosts);
    }
    {
        let mut state = backend.state();
        state.apply(&media_ready());
        let attachments_msg: Message = serde_json::from_value(json!({
            "id": msg_id(1).to_string(),
            "channel_id": GENERAL.to_string(),
            "author": { "id": "201", "username": "alice", "global_name": "Alice" },
            "content": "photos",
            "timestamp": "2026-11-19T08:00:00+00:00",
            "attachments": [
                {
                    "id": "5001",
                    "filename": "trusted.png",
                    "url": "https://cdn.fluxer.app/media/1.png",
                    "proxy_url": "https://media.fluxer.app/media/1.png",
                    "width": 800,
                    "height": 600,
                    "content_type": "image/png"
                },
                {
                    "id": "5002",
                    "filename": "external.png",
                    "url": "https://untrusted.example/img2.png",
                    "proxy_url": "https://media.fluxer.app/media/2.png",
                    "width": 640,
                    "height": 480,
                    "content_type": "image/png"
                }
            ]
        }))
        .expect("attachments message");
        let embed_msg: Message = serde_json::from_value(json!({
            "id": msg_id(2).to_string(),
            "channel_id": GENERAL.to_string(),
            "author": { "id": "201", "username": "alice", "global_name": "Alice" },
            "content": "",
            "timestamp": "2026-11-19T08:01:00+00:00",
            "embeds": [{
                "type": "image",
                "url": "https://img.example/embed.png",
                "thumbnail": {
                    "url": "https://img.example/embed.png",
                    "proxy_url": "https://media.fluxer.app/embed.png",
                    "width": 1200,
                    "height": 900
                }
            }]
        }))
        .expect("embed message");
        let reply_msg: Message = serde_json::from_value(json!({
            "id": msg_id(3).to_string(),
            "channel_id": GENERAL.to_string(),
            "author": { "id": "201", "username": "alice", "global_name": "Alice" },
            "content": "reply body",
            "timestamp": "2026-11-19T08:02:00+00:00",
            "referenced_message": {
                "id": msg_id(1).to_string(),
                "channel_id": GENERAL.to_string(),
                "author": { "id": "201", "username": "alice", "global_name": "Alice" },
                "content": "photos",
                "timestamp": "2026-11-19T08:00:00+00:00"
            }
        }))
        .expect("reply message");
        let sys_msg: Message = serde_json::from_value(json!({
            "id": msg_id(4).to_string(),
            "channel_id": GENERAL.to_string(),
            "author": { "id": "201", "username": "alice", "global_name": "Alice" },
            "content": "",
            "type": 7,
            "timestamp": "2026-11-19T08:03:00+00:00"
        }))
        .expect("system join message");
        state.messages.load(GENERAL, vec![attachments_msg, embed_msg, reply_msg, sys_msg], LoadMode::Replace);
    }
    let (view, cx) = open_client(cx, backend.clone(), events);
    view.update(cx, |v, cx| v.select_place(Place::Guild(GUILD), cx));
    cx.run_until_parked();
    draw(cx);
    (rt, backend, view, cx)
}

#[gpui_kit::test]
fn media_viewer_navigation_zoom_open_and_escape(cx: &mut TestAppContext) {
    let (rt, _backend, view, cx) = open_media_fixture(cx, Vec::new());

    let first_sel: &'static str = Box::leak(format!("attachment-open-{}-0", msg_id(1).0).into_boxed_str());
    let first_bounds = cx.debug_bounds(first_sel).expect("first attachment rendered").center();
    cx.simulate_click(first_bounds, Modifiers::default());
    cx.run_until_parked();
    draw(cx);

    view.read_with(cx, |v, _| {
        let Overlay::MediaViewer(modal) = &v.overlay else { panic!("media viewer did not open") };
        assert_eq!(modal.items.len(), 2);
        assert_eq!(modal.index, 0);
        assert!(!modal.zoomed);
    });

    // Keyboard navigation and zoom controls
    cx.simulate_keystrokes("right");
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(&v.overlay, Overlay::MediaViewer(m) if m.index == 1)));

    cx.simulate_keystrokes("right");
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(&v.overlay, Overlay::MediaViewer(m) if m.index == 1)), "clamped at end");

    cx.simulate_keystrokes("left");
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(&v.overlay, Overlay::MediaViewer(m) if m.index == 0)));

    cx.simulate_keystrokes("=");
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(&v.overlay, Overlay::MediaViewer(m) if m.zoomed)));

    cx.simulate_keystrokes("-");
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(&v.overlay, Overlay::MediaViewer(m) if !m.zoomed)));

    // Toolbar Open button on trusted domain opens directly without LinkWarning
    draw(cx);
    let open_btn = cx.debug_bounds("media-viewer-open").expect("open button drawn").center();
    cx.simulate_click(open_btn, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, _| v.opened_urls.clone()),
        vec!["https://cdn.fluxer.app/media/1.png".to_owned()]
    );

    // Step to untrusted attachment and click Open -> opens LinkWarning
    let next_btn = cx.debug_bounds("media-viewer-next").expect("next button drawn").center();
    cx.simulate_click(next_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    let open_btn = cx.debug_bounds("media-viewer-open").expect("open button drawn").center();
    cx.simulate_click(open_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    view.read_with(cx, |v, _| {
        let Overlay::LinkWarning(modal) = &v.overlay else { panic!("expected link warning for untrusted domain") };
        assert_eq!(modal.host, "untrusted.example");
    });

    // Escape closes overlay and restores composer focus
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));
    let composer_focused = cx.update(|window, cx| {
        view.read_with(cx, |v, cx| v.composer.read(cx).focus_handle(cx).is_focused(window))
    });
    assert!(composer_focused);

    // Clicking an image embed also opens MediaViewer
    draw(cx);
    let embed_bounds = cx.debug_bounds("embed-media").expect("embed media drawn").center();
    cx.simulate_click(embed_bounds, Modifiers::default());
    cx.run_until_parked();
    view.read_with(cx, |v, _| {
        let Overlay::MediaViewer(modal) = &v.overlay else { panic!("embed did not open media viewer") };
        assert_eq!(modal.items[0].url, "https://img.example/embed.png");
    });
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    drop(rt);
}

#[gpui_kit::test]
fn trusted_domain_link_warning_persists_and_blocks_bad_schemes(cx: &mut TestAppContext) {
    let (rt, backend, view, cx) = open_media_fixture(cx, vec!["chat.selfhost.example".to_owned()]);

    // Built-in, configured instance host, and userinfo-to-trusted URLs open without prompting
    view.update_in(cx, |v, window, cx| {
        v.open_external_url("https://fluxer.app/docs", window, cx);
        v.open_external_url("https://chat.selfhost.example/invite/abc", window, cx);
        v.open_external_url("mailto:support@fluxer.app", window, cx);
        v.open_external_url("javascript:alert(1)", window, cx);
        v.open_external_url("file:///etc/passwd", window, cx);
    });
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));
    assert_eq!(
        view.read_with(cx, |v, _| v.opened_urls.clone()),
        vec![
            "https://fluxer.app/docs".to_owned(),
            "https://chat.selfhost.example/invite/abc".to_owned(),
            "mailto:support@fluxer.app".to_owned(),
        ]
    );

    // Credential-spoofed URL shows true parsed host ("evil.example") and Escape cancels
    view.update_in(cx, |v, window, cx| {
        v.open_external_url("https://fluxer.app@evil.example/login", window, cx);
    });
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("link-warning-host").is_some());
    view.read_with(cx, |v, _| {
        let Overlay::LinkWarning(modal) = &v.overlay else { panic!("expected link warning") };
        assert_eq!(modal.host, "evil.example");
        assert!(!modal.always_trust);
    });
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));
    assert_eq!(view.read_with(cx, |v, _| v.opened_urls.len()), 3);

    // Always-trust + Continue saves to prefs + server settings and skips future prompts
    view.update_in(cx, |v, window, cx| {
        v.open_external_url("https://example.org/first", window, cx);
    });
    cx.run_until_parked();
    view.update(cx, |v, cx| v.link_warning_set_trust(true, cx));
    cx.run_until_parked();
    draw(cx);
    let continue_btn = cx.debug_bounds("link-warning-continue").expect("continue button drawn").center();
    cx.simulate_click(continue_btn, Modifiers::default());
    cx.run_until_parked();

    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));
    assert_eq!(view.read_with(cx, |v, _| v.prefs.data.trusted_domains.clone()), vec!["example.org".to_owned()]);
    assert_eq!(
        view.read_with(cx, |v, _| v.opened_urls.last().cloned()),
        Some("https://example.org/first".to_owned())
    );
    assert_eq!(
        *lock_sent(&backend.sent_settings),
        vec![UserSettingsPatch {
            trusted_domains: Some(vec!["example.org".to_owned()]),
            ..Default::default()
        }]
    );

    // Subsequent click on example.org bypasses warning
    view.update_in(cx, |v, window, cx| {
        v.open_external_url("https://example.org/second", window, cx);
    });
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));
    assert_eq!(
        view.read_with(cx, |v, _| v.opened_urls.last().cloned()),
        Some("https://example.org/second".to_owned())
    );

    // Clearing trusted domains revokes trust and restores prompt
    view.update(cx, |v, cx| v.settings_clear_trusted_domains(cx));
    cx.run_until_parked();
    assert_eq!(
        lock_sent(&backend.sent_settings).last(),
        Some(&UserSettingsPatch {
            trusted_domains: Some(Vec::new()),
            ..Default::default()
        })
    );
    view.update_in(cx, |v, window, cx| {
        v.open_external_url("https://example.org/third", window, cx);
    });
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| matches!(&v.overlay, Overlay::LinkWarning(m) if m.host == "example.org")));
    drop(rt);
}

#[gpui_kit::test]
fn display_settings_zoom_font_and_compact_density(cx: &mut TestAppContext) {
    let (rt, backend, view, cx) = open_media_fixture(cx, Vec::new());

    // Keyboard zoom shortcuts adjust ui_zoom and scaled Theme sizes
    focus_composer(&view, cx);
    cx.simulate_keystrokes("ctrl-=");
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |v, _| v.prefs.data.ui_zoom), 110);
    assert_eq!(cx.update(|_, cx| Theme::current(cx).sizes.text), 15.);

    cx.simulate_keystrokes("ctrl-- ctrl--");
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |v, _| v.prefs.data.ui_zoom), 90);
    assert_eq!(cx.update(|_, cx| Theme::current(cx).sizes.text), 13.);

    cx.simulate_keystrokes("ctrl-0");
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |v, _| v.prefs.data.ui_zoom), 100);
    assert_eq!(cx.update(|_, cx| Theme::current(cx).sizes.text), 14.);

    // Settings modal: font size 18px + Compact mode + back to Cozy
    cx.simulate_keystrokes("ctrl-,");
    cx.run_until_parked();
    draw(cx);

    let font_18 = cx.debug_bounds("settings-font-18").expect("font-18 button drawn").center();
    cx.simulate_click(font_18, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert_eq!(view.read_with(cx, |v, _| v.prefs.data.font_size), 18);
    assert_eq!(cx.update(|_, cx| Theme::current(cx).sizes.text), 18.);

    let compact_btn = cx.debug_bounds("settings-density-compact").expect("compact button drawn").center();
    cx.simulate_click(compact_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    let compact_sizes = cx.update(|_, cx| Theme::current(cx).sizes);
    assert!(compact_sizes.compact);
    assert_eq!(compact_sizes.avatar, 0.);
    assert_eq!(compact_sizes.gutter, 0.);
    assert_eq!(compact_sizes.group_gap, 4.);
    assert_eq!(
        *lock_sent(&backend.sent_settings),
        vec![UserSettingsPatch {
            message_display_compact: Some(true),
            ..Default::default()
        }]
    );

    let cozy_btn = cx.debug_bounds("settings-density-cozy").expect("cozy button drawn").center();
    cx.simulate_click(cozy_btn, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(!cx.update(|_, cx| Theme::current(cx).sizes.compact));
    assert_eq!(cx.update(|_, cx| Theme::current(cx).sizes.gutter), 56.);

    // Server-side USER_SETTINGS_UPDATE syncs message_display_compact & rollback cleanly
    backend.state().apply(&Dispatch {
        t: "USER_SETTINGS_UPDATE".to_owned(),
        s: 2,
        d: json!({ "message_display_compact": true }),
    });
    view.update(cx, |v, cx| v.handle_events(vec![crate::backend::UiEvent::StateChanged], cx));
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| v.prefs.data.compact));
    assert!(cx.update(|_, cx| Theme::current(cx).sizes.compact));

    // Media viewer scale factor: 1x uses 800x600 max device size while 2x uses 1600x1200
    let preview = crate::view_model::ImagePreview {
        url: "https://cdn.fluxer.app/a.png".into(),
        proxy_url: Some("https://media.fluxer.app/a.png".into()),
        animated: false,
        width: 400.,
        height: 300.,
    };
    let item_1x = crate::ui::media_viewer::from_attachment(&preview, 1.0);
    let item_2x = crate::ui::media_viewer::from_attachment(&preview, 2.0);
    assert!(item_1x.preview_url.contains("width=800&height=600"));
    assert!(item_2x.preview_url.contains("width=1600&height=1200"));
    drop(rt);
}
