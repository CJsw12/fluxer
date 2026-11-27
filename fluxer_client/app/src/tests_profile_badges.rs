use fluxer_client_core::rest::UserProfileFull;
use fluxer_client_core::state::{ClientState, Snowflake};
use serde_json::json;

use crate::profile::{build_profile_card, profile_badge_image, profile_badge_tooltip};

#[test]
fn badge_assets_details_and_display_order_follow_the_response() {
    let response: UserProfileFull = serde_json::from_value(json!({
        "profile_badges": [
            {"key":"future","kind":"new_kind","label":"New badge","asset":"0123456789abcdef",
             "asset_url":"https://cdn.example/custom.webp","caption":"#42"},
            {"key":"staff","kind":"staff","label":"Staff","asset":"0123456789abcdef",
             "small_asset":"fedcba9876543210","details":{"title":"Staff member","lines":["Since 2026"]}},
            {"key":"number","kind":"visionary_number","label":"Visionary number","caption":"#23"}
        ]
    })).unwrap();
    let card = build_profile_card(&ClientState::new(), None, Snowflake(42), None, Some(&response));
    let mut badges = card.profile_badges.iter();
    let future = badges.next().unwrap();
    assert_eq!(future.kind, "new_kind");
    assert_eq!(future.caption.as_deref(), Some("#42"));
    let static_cdn = Some("https://static.example/");
    assert_eq!(profile_badge_image(future, static_cdn).as_deref(), Some("https://cdn.example/custom.webp"));
    assert_eq!(profile_badge_tooltip(future), "New badge");
    let staff = badges.next().unwrap();
    assert_eq!(staff.kind, "staff");
    assert_eq!(
        profile_badge_image(staff, static_cdn).as_deref(),
        Some("https://static.example/badges/fedcba9876543210.webp")
    );
    assert_eq!(profile_badge_tooltip(staff), "Staff member\nSince 2026");
    let number = badges.next().unwrap();
    assert_eq!(number.caption.as_deref(), Some("#23"));
    assert_eq!(profile_badge_image(number, static_cdn), None);
}

#[test]
fn limited_profile_omits_premium_family_and_connections() {
    let mut response: UserProfileFull = serde_json::from_value(json!({
        "profile_limited": true,
        "profile_badges": [
            {"key":"premium","kind":"premium","label":"Premium"},
            {"key":"operator_pass","kind":"operator_pass","label":"Operator"},
            {"key":"visionary_number","kind":"visionary_number","label":"Visionary Number"},
            {"key":"staff","kind":"staff","label":"Staff"}
        ],
        "connected_accounts": [{"id":"a","type":"bsky","name":"user.example","verified":true}]
    })).unwrap();
    let state = ClientState::new();
    let card = build_profile_card(&state, None, Snowflake(42), None, Some(&response));
    assert_eq!(card.profile_badges.len(), 1);
    assert_eq!(card.profile_badges[0].kind, "staff");
    assert!(card.connected_accounts.is_empty());
    response.profile_limited = false;
    let card = build_profile_card(&state, None, Snowflake(42), None, Some(&response));
    assert_eq!(card.profile_badges.len(), 4);
    assert!(card.connected_accounts[0].verified);
}

#[test]
fn missing_null_and_empty_sections_are_empty() {
    let state = ClientState::new();
    for payload in [
        "{}",
        r#"{"profile_badges":null,"connected_accounts":null,"profile_limited":null}"#,
        r#"{"profile_badges":[],"connected_accounts":[]}"#,
    ] {
        let response: UserProfileFull = serde_json::from_str(payload).unwrap();
        let card = build_profile_card(&state, None, Snowflake(42), None, Some(&response));
        assert!(card.profile_badges.is_empty());
        assert!(card.connected_accounts.is_empty());
    }
}
