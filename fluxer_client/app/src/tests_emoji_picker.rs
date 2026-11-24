//! Headless UI and unit tests for the emoji picker popover.

use fluxer_client_core::Dispatch;
use fluxer_client_core::search::emoji::{EmojiChoice, normalize_picker_query, unicode_by_name};
use fluxer_client_core::state::{Message, Snowflake, bits};
use gpui_kit::{Modifiers, TestAppContext, VisualTestContext, point, px};
use serde_json::json;

use crate::backend::Backend;
use crate::client_view::ClientView;
use crate::tests::{composer_text, draw, focus_composer, msg_id, offline_rest, open_client, runtime};
use crate::ui::composer_text::wire_text;
use crate::ui::overlay::Overlay;
use crate::view_model::Place;

const GUILD: Snowflake = Snowflake(1);
const GENERAL: Snowflake = Snowflake(11);

fn emoji_ready() -> Dispatch {
    let perms = bits::VIEW_CHANNEL | bits::SEND_MESSAGES | bits::READ_MESSAGE_HISTORY | bits::USE_EXTERNAL_EMOJIS;
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
                "members": [{ "user": { "id": "100", "username": "me" }, "roles": [] }],
                "emojis": [{ "id": "9002", "name": "pepe", "animated": false }]
            }],
            "private_channels": [],
            "read_states": []
        }),
    }
}

fn open_emoji_channel(cx: &mut TestAppContext) -> (tokio::runtime::Runtime, Backend, gpui_kit::Entity<ClientView>, &mut VisualTestContext) {
    let rt = runtime();
    let (backend, events) = Backend::new(rt.handle().clone(), offline_rest(), None);
    {
        let mut state = backend.state();
        state.apply(&emoji_ready());
        let msg: Message = serde_json::from_value(json!({
            "id": msg_id(1).to_string(),
            "channel_id": GENERAL.to_string(),
            "author": { "id": "201", "username": "alice" },
            "content": "hello",
            "timestamp": "2026-11-18T10:20:30+00:00"
        }))
        .expect("message");
        state.messages.load(GENERAL, vec![msg], fluxer_client_core::state::LoadMode::Replace);
    }
    let (view, cx) = open_client(cx, backend.clone(), events);
    view.update(cx, |v, cx| v.select_place(Place::Guild(GUILD), cx));
    cx.run_until_parked();
    draw(cx);
    (rt, backend, view, cx)
}

#[test]
fn pure_picker_query_and_tone() {
    assert_eq!(normalize_picker_query(":smile:"), "smile");
    assert_eq!(normalize_picker_query("hello world"), "hello_world");
    assert_eq!(normalize_picker_query(":+1"), "+1");
    assert_eq!(normalize_picker_query("<3"), "heart");
    let wave = unicode_by_name("wave").expect("wave");
    let choice = EmojiChoice::from_unicode(wave);
    assert_eq!(choice.with_tone(0).glyph, wave.surrogates);
    assert_eq!(choice.with_tone(3).glyph, wave.with_skin_tone(3));
}

#[gpui_kit::test]
fn ctrl_e_opens_and_esc_closes_emoji_picker(cx: &mut TestAppContext) {
    let (rt, _backend, view, cx) = open_emoji_channel(cx);
    focus_composer(&view, cx);

    cx.simulate_keystrokes("ctrl-e");
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::EmojiPicker(_))));

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));
    drop(rt);
}

#[gpui_kit::test]
fn picking_unicode_and_custom_emoji_inserts_into_composer_and_shift_keeps_open(cx: &mut TestAppContext) {
    let (rt, _backend, view, cx) = open_emoji_channel(cx);
    let composer = focus_composer(&view, cx);

    cx.simulate_keystrokes("ctrl-e");
    cx.run_until_parked();
    draw(cx);

    // Pick toned unicode emoji with Shift (stays open).
    let wave = unicode_by_name("wave").expect("wave");
    let wave_choice = EmojiChoice::from_unicode(wave);
    let expected_toned = wave.with_skin_tone(3).to_owned();
    view.update_in(cx, |v, window, cx| {
        v.emoji_picker_set_tone(3, cx);
        v.emoji_picker_pick(wave_choice.clone(), true, window, cx);
    });
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::EmojiPicker(_))), "keep_open stays open");
    assert_eq!(composer_text(&composer, cx), format!("{expected_toned} "));

    // Pick custom emoji without Shift (inserts :pepe: display and <:pepe:9002> wire, then closes).
    let custom_choice = view.read_with(cx, |v, _| {
        let Overlay::EmojiPicker(p) = &v.overlay else { panic!("picker closed") };
        p.sections
            .iter()
            .flat_map(|s| &s.choices)
            .find(|c| c.custom_id == Some(Snowflake(9002)))
            .cloned()
            .expect("custom emoji")
    });
    view.update_in(cx, |v, window, cx| v.emoji_picker_pick(custom_choice, false, window, cx));
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));
    assert_eq!(composer_text(&composer, cx), format!("{expected_toned} :pepe: "));
    let wire = composer.read_with(cx, |s, _| wire_text(s));
    assert_eq!(wire, format!("{expected_toned} <:pepe:9002> "));
    drop(rt);
}

#[gpui_kit::test]
fn reaction_target_adds_optimistic_reaction(cx: &mut TestAppContext) {
    let (rt, _backend, view, cx) = open_emoji_channel(cx);
    let msg_id_1 = msg_id(1);
    view.update_in(cx, |v, window, cx| {
        v.open_reaction_emoji_picker(GENERAL, msg_id_1, point(px(200.), px(200.)), window, cx);
    });
    cx.run_until_parked();
    draw(cx);

    let heart = EmojiChoice::from_unicode(unicode_by_name("heart").expect("heart"));
    let heart_glyph = heart.glyph.clone();
    view.update_in(cx, |v, window, cx| v.emoji_picker_pick(heart, false, window, cx));
    cx.run_until_parked();
    draw(cx);

    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));
    let added = view.read_with(cx, |v, _| {
        v.rows.borrow()[0].reactions.iter().any(|r| r.emoji.name == heart_glyph && r.me && r.count == 1)
    });
    assert!(added, "reaction target inserts optimistic reaction");
    drop(rt);
}

#[gpui_kit::test]
fn empty_search_shows_empty_state_and_click_cell_works(cx: &mut TestAppContext) {
    let (rt, _backend, view, cx) = open_emoji_channel(cx);
    focus_composer(&view, cx);

    cx.simulate_keystrokes("ctrl-e");
    cx.run_until_parked();
    draw(cx);

    // Section 0 is the guild custom emojis (:pepe:); click it via its drawn bounds.
    let pepe_bounds = cx.debug_bounds("emoji-cell-pepe").expect("pepe cell drawn");
    cx.simulate_click(pepe_bounds.center(), Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));
    let composer = view.read_with(cx, |v, _| v.composer.clone());
    assert_eq!(composer_text(&composer, cx), ":pepe: ");

    // Reopen (now Frequently used is section 0 because :pepe: was recorded),
    // switch to the People category, and click grinning.
    cx.simulate_keystrokes("ctrl-e");
    cx.run_until_parked();
    view.update(cx, |v, cx| {
        let Overlay::EmojiPicker(p) = &v.overlay else { panic!("picker closed") };
        let people_ix = p
            .sections
            .iter()
            .position(|s| {
                matches!(
                    s.kind,
                    crate::ui::emoji_picker::sections::SectionKind::Category(
                        fluxer_client_core::search::emoji::EmojiCategory::People
                    )
                )
            })
            .expect("People section");
        v.emoji_picker_select_section(people_ix, cx);
    });
    cx.run_until_parked();
    draw(cx);
    let grinning_bounds = cx.debug_bounds("emoji-cell-grinning").expect("grinning cell drawn");
    cx.simulate_click(grinning_bounds.center(), Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));
    assert!(composer_text(&composer, cx).contains("😀"));

    // Colon-only query stays in section view (search_results is None).
    cx.simulate_keystrokes("ctrl-e");
    cx.run_until_parked();
    cx.simulate_input(":");
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| {
        let Overlay::EmojiPicker(p) = &v.overlay else { return false };
        p.search_results.is_none()
    }));

    // Empty search
    cx.simulate_input("zzzzzz_no_such_emoji");
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| {
        let Overlay::EmojiPicker(p) = &v.overlay else { return false };
        p.active_choices().is_empty()
    }));
    drop(rt);
}

#[gpui_kit::test]
fn composer_expression_tabs_switch_and_toggle_close(cx: &mut TestAppContext) {
    use crate::ui::emoji_picker::ExpressionTab;

    let (rt, _backend, view, cx) = open_emoji_channel(cx);
    let media_pos = cx.debug_bounds("composer-gif").expect("media btn").center();
    let sticker_pos = cx.debug_bounds("composer-sticker").expect("sticker btn").center();
    let emoji_pos = cx.debug_bounds("composer-emoji").expect("emoji btn").center();

    // Media button opens Media tab (not file dialog).
    cx.simulate_click(media_pos, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| {
        matches!(&v.overlay, Overlay::EmojiPicker(p) if p.active_tab == ExpressionTab::Media)
    }));
    assert!(cx.debug_bounds("media-picker-panel").is_some());
    let images_pill = cx.debug_bounds("media-filter-Images").expect("Images filter pill").center();
    cx.simulate_click(images_pill, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| {
        matches!(&v.overlay, Overlay::EmojiPicker(p) if p.media_filter == "Images")
    }));

    // Clicking Stickers directly switches tab without needing Escape.
    cx.simulate_click(sticker_pos, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| {
        matches!(&v.overlay, Overlay::EmojiPicker(p) if p.active_tab == ExpressionTab::Stickers)
    }));
    assert!(cx.debug_bounds("sticker-picker-panel").is_some());
    assert!(cx.debug_bounds("media-picker-panel").is_none());
    assert!(cx.debug_bounds("emoji-picker-grid").is_none());

    // Clicking Emojis directly switches to Emojis grid.
    cx.simulate_click(emoji_pos, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| {
        matches!(&v.overlay, Overlay::EmojiPicker(p) if p.active_tab == ExpressionTab::Emojis)
    }));
    assert!(cx.debug_bounds("emoji-cell-pepe").is_some());

    // In-popout tab bar switches cleanly back to Stickers, then Escape closes.
    let tab_stickers = cx.debug_bounds("expression-tab-Stickers").expect("Stickers tab").center();
    cx.simulate_click(tab_stickers, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| {
        matches!(&v.overlay, Overlay::EmojiPicker(p) if p.active_tab == ExpressionTab::Stickers)
    }));

    // Repeat-clicking Stickers composer button toggles popout closed.
    cx.simulate_click(sticker_pos, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));

    // Dismissing on outside click still lets the next composer click open normally.
    cx.simulate_click(emoji_pos, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::EmojiPicker(_))));
    cx.simulate_click(point(px(200.), px(200.)), Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.overlay, Overlay::None)));
    cx.simulate_click(emoji_pos, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |v, _| {
        matches!(&v.overlay, Overlay::EmojiPicker(p) if p.active_tab == ExpressionTab::Emojis)
    }));
    drop(rt);
}
