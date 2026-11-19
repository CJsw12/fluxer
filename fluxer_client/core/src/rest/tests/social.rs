//! Fixture tests for relationships, invites, settings and leaving a guild.

use super::*;

const USER: &str = "423456789012345678";
const GUILD: &str = "523456789012345678";

fn ok_fixture() -> impl Fn(&Req, usize) -> Resp + Send + Sync + 'static {
    |req, _| match req.method.as_str() {
        "DELETE" => Resp::empty(204),
        _ => Resp::json(200, json!({ "ok": true })),
    }
}

fn lines(fixture: &Fixture) -> Vec<(String, String)> {
    fixture.requests().iter().map(|r| (r.method.clone(), r.target.clone())).collect()
}

fn l(method: &str, target: String) -> (String, String) {
    (method.to_owned(), target)
}

#[tokio::test]
async fn relationship_routes() {
    let fixture = Fixture::start(ok_fixture()).await;
    let client = authed(&fixture).await;
    client.list_relationships().await.unwrap();
    client.send_friend_request(USER, false).await.unwrap();
    client.send_friend_request(USER, true).await.unwrap();
    client.send_friend_request_by_tag("alice", Some("0042")).await.unwrap();
    client.send_friend_request_by_tag("bob", None).await.unwrap();
    client.accept_friend_request(USER).await.unwrap();
    client.remove_relationship(USER).await.unwrap();
    client.block_user(USER).await.unwrap();
    client.unblock_user(USER).await.unwrap();
    client.set_relationship_nickname(USER, Some("Al")).await.unwrap();
    client.set_relationship_nickname(USER, None).await.unwrap();

    let rel = format!("/api/v1/users/@me/relationships/{USER}");
    assert_eq!(
        lines(&fixture),
        vec![
            l("GET", "/api/v1/users/@me/relationships".into()),
            l("POST", rel.clone()),
            l("POST", rel.clone()),
            l("POST", "/api/v1/users/@me/relationships".into()),
            l("POST", "/api/v1/users/@me/relationships".into()),
            l("PUT", rel.clone()),
            l("DELETE", rel.clone()),
            l("PUT", rel.clone()),
            l("DELETE", rel.clone()),
            l("PATCH", rel.clone()),
            l("PATCH", rel),
        ]
    );
    let reqs = fixture.requests();
    assert_eq!(reqs[1].json(), json!({}));
    assert_eq!(reqs[2].json(), json!({ "staff_force_accept": true }));
    assert_eq!(reqs[3].json(), json!({ "username": "alice", "discriminator": "0042" }));
    assert_eq!(reqs[4].json(), json!({ "username": "bob" }));
    assert!(reqs[5].body.is_empty());
    assert_eq!(reqs[7].json(), json!({ "type": 2 }));
    assert_eq!(reqs[9].json(), json!({ "nickname": "Al" }));
    assert_eq!(reqs[10].json(), json!({ "nickname": null }));
    for req in &reqs {
        assert_eq!(req.header("authorization"), Some(FAKE_TOKEN));
    }

    assert!(matches!(client.block_user("1/../2").await, Err(RestError::InvalidArgument(_))));
    assert!(matches!(client.send_friend_request_by_tag("a", Some("12345")).await, Err(RestError::InvalidArgument(_))));
    assert_eq!(fixture.requests().len(), 11);
}

#[test]
fn user_tag_parsing() {
    assert_eq!(parse_user_tag(" alice#0042 ").unwrap(), ("alice".into(), Some("0042".into())));
    assert_eq!(parse_user_tag("bob").unwrap(), ("bob".into(), None));
    assert!(parse_user_tag("#1234").is_err());
    assert!(parse_user_tag("a#12x").is_err());
    assert!(parse_user_tag("").is_err());
}

#[tokio::test]
async fn invite_routes() {
    let fixture = Fixture::start(ok_fixture()).await;
    let client = authed(&fixture).await;
    client.get_invite("abcDEF12").await.unwrap();
    client.accept_invite("abcDEF12").await.unwrap();
    let request = CreateInviteRequest { max_age: Some(3600), max_uses: Some(5), temporary: Some(true), unique: None };
    client.create_channel_invite(CHANNEL, &request).await.unwrap();
    client.create_channel_invite(CHANNEL, &CreateInviteRequest::default()).await.unwrap();
    client.delete_invite("abcDEF12").await.unwrap();
    client.list_guild_invites(GUILD).await.unwrap();
    client.list_channel_invites(CHANNEL).await.unwrap();

    assert_eq!(
        lines(&fixture),
        vec![
            l("GET", "/api/v1/invites/abcDEF12".into()),
            l("POST", "/api/v1/invites/abcDEF12".into()),
            l("POST", format!("/api/v1/channels/{CHANNEL}/invites")),
            l("POST", format!("/api/v1/channels/{CHANNEL}/invites")),
            l("DELETE", "/api/v1/invites/abcDEF12".into()),
            l("GET", format!("/api/v1/guilds/{GUILD}/invites")),
            l("GET", format!("/api/v1/channels/{CHANNEL}/invites")),
        ]
    );
    let reqs = fixture.requests();
    assert!(reqs[1].body.is_empty());
    assert_eq!(reqs[2].json(), json!({ "max_age": 3600, "max_uses": 5, "temporary": true }));
    assert_eq!(reqs[3].json(), json!({}));

    assert!(matches!(client.get_invite("").await, Err(RestError::InvalidArgument(_))));
    assert!(matches!(client.list_guild_invites("x").await, Err(RestError::InvalidArgument(_))));
    // Codes cannot alter the path.
    client.get_invite("a/b").await.unwrap();
    assert_eq!(fixture.requests()[7].target, "/api/v1/invites/a%2Fb");
}

#[tokio::test]
async fn settings_routes() {
    let fixture = Fixture::start(ok_fixture()).await;
    let client = authed(&fixture).await;
    let patch = UserSettingsPatch {
        status: Some("dnd".into()),
        custom_status: Some(Some(CustomStatus { text: Some("hi".into()), ..CustomStatus::default() })),
        theme: Some("coal".into()),
        locale: Some("en-US".into()),
        guild_positions: Some(vec![GUILD.into()]),
        ..UserSettingsPatch::default()
    };
    client.update_user_settings(&patch).await.unwrap();
    let clear = UserSettingsPatch { custom_status: Some(None), ..UserSettingsPatch::default() };
    client.update_user_settings(&clear).await.unwrap();

    let mut overrides = std::collections::BTreeMap::new();
    overrides.insert(
        CHANNEL.to_owned(),
        ChannelOverride { muted: Some(true), message_notifications: Some(1), ..ChannelOverride::default() },
    );
    let guild_patch = GuildSettingsPatch {
        muted: Some(true),
        mute_config: Some(Some(MuteConfig {
            end_time: Some("2030-01-01T00:00:00.000Z".into()),
            selected_time_window: 3600,
        })),
        message_notifications: Some(1),
        suppress_everyone: Some(true),
        channel_overrides: Some(Some(overrides)),
        ..GuildSettingsPatch::default()
    };
    client.update_guild_settings(GUILD, &guild_patch).await.unwrap();
    let dm = GuildSettingsPatch { muted: Some(false), ..GuildSettingsPatch::default() };
    client.update_dm_settings(&dm).await.unwrap();

    assert_eq!(
        lines(&fixture),
        vec![
            l("PATCH", "/api/v1/users/@me/settings".into()),
            l("PATCH", "/api/v1/users/@me/settings".into()),
            l("PATCH", format!("/api/v1/users/@me/guilds/{GUILD}/settings")),
            l("PATCH", "/api/v1/users/@me/guilds/@me/settings".into()),
        ]
    );
    let reqs = fixture.requests();
    assert_eq!(
        reqs[0].json(),
        json!({
            "status": "dnd", "custom_status": { "text": "hi" }, "theme": "coal",
            "locale": "en-US", "guild_positions": [GUILD]
        })
    );
    assert_eq!(reqs[1].json(), json!({ "custom_status": null }));
    assert_eq!(
        reqs[2].json(),
        json!({
            "message_notifications": 1, "muted": true,
            "mute_config": { "end_time": "2030-01-01T00:00:00.000Z", "selected_time_window": 3600 },
            "suppress_everyone": true,
            "channel_overrides": { CHANNEL: { "message_notifications": 1, "muted": true } }
        })
    );
    assert_eq!(reqs[3].json(), json!({ "muted": false }));

    let bad = UserSettingsPatch { guild_positions: Some(vec!["nope".into()]), ..UserSettingsPatch::default() };
    assert!(matches!(client.update_user_settings(&bad).await, Err(RestError::InvalidArgument(_))));
    assert!(matches!(
        client.update_guild_settings("x", &GuildSettingsPatch::default()).await,
        Err(RestError::InvalidArgument(_))
    ));
    assert_eq!(fixture.requests().len(), 4);
}

#[tokio::test]
async fn leave_guild_route() {
    let fixture = Fixture::start(ok_fixture()).await;
    let client = authed(&fixture).await;
    client.leave_guild(GUILD, false).await.unwrap();
    client.leave_guild(GUILD, true).await.unwrap();
    assert_eq!(
        lines(&fixture),
        vec![
            l("DELETE", format!("/api/v1/users/@me/guilds/{GUILD}")),
            l("DELETE", format!("/api/v1/users/@me/guilds/{GUILD}?delete_messages=true")),
        ]
    );
    assert!(matches!(client.leave_guild("1/../2", false).await, Err(RestError::InvalidArgument(_))));
}

#[tokio::test]
async fn search_messages_route() {
    let fixture = Fixture::start(|req, idx| {
        if req.target == "/api/v1/search/messages" && idx == 1 {
            Resp::json(200, json!({ "indexing": true }))
        } else {
            Resp::json(
                200,
                json!({
                    "messages": [{
                        "id": "900000000000000001",
                        "channel_id": CHANNEL,
                        "author": { "id": USER, "username": "alice" },
                        "content": "found me",
                        "timestamp": "2026-11-19T00:00:00.000Z"
                    }],
                    "channels": [{
                        "id": CHANNEL,
                        "guild_id": GUILD,
                        "name": "general",
                        "type": 0
                    }],
                    "total": 1,
                    "hits_per_page": 25,
                    "page": 1
                }),
            )
        }
    })
    .await;
    let client = authed(&fixture).await;

    let req = MessageSearchRequest {
        content: Some("found".into()),
        scope: Some(MessageSearchScope::AllGuilds),
        context_guild_id: Some(GUILD.into()),
        has: Some(vec![MessageContentType::Image]),
        sort_by: Some(MessageSearchSortBy::Relevance),
        sort_order: Some(MessageSearchSortOrder::Desc),
        ..Default::default()
    };
    let first = client.search_messages(&req).await.unwrap();
    let MessageSearchOutcome::Results(page) = first else { panic!("expected results") };
    assert_eq!(page.messages.len(), 1);
    assert_eq!(page.messages[0].content, "found me");
    assert_eq!(page.total, 1);

    let second = client.search_messages(&req).await.unwrap();
    assert_eq!(second, MessageSearchOutcome::Indexing);

    let reqs = fixture.requests();
    assert_eq!(reqs.len(), 2);
    assert_eq!(reqs[0].method, "POST");
    assert_eq!(reqs[0].target, "/api/v1/search/messages");
    assert_eq!(reqs[0].header("authorization"), Some(FAKE_TOKEN));
    assert_eq!(
        reqs[0].json(),
        json!({
            "content": "found",
            "has": ["image"],
            "sort_by": "relevance",
            "sort_order": "desc",
            "scope": "all_guilds",
            "context_guild_id": GUILD,
        })
    );

    let invalid = MessageSearchRequest { context_guild_id: Some("1/../2".into()), ..Default::default() };
    assert!(matches!(client.search_messages(&invalid).await, Err(RestError::InvalidArgument(_))));
    assert_eq!(fixture.requests().len(), 2);
}
