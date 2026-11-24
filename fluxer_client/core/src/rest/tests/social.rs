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
async fn role_and_channel_access_routes_preserve_wire_bitfields() {
    use crate::state::{PermissionOverwrite, Snowflake};

    let guild = Snowflake(120);
    let role = Snowflake(121);
    let channel = Snowflake(122);
    let parent = Snowflake(123);
    let role_body = json!({ "id": role, "name": "Helpers", "permissions": "9007199254740992" });
    let fixture = Fixture::start(move |req, _| match (req.method.as_str(), req.target.as_str()) {
        ("GET", "/api/v1/guilds/120/roles") => Resp::json(200, json!([role_body])),
        ("POST", "/api/v1/guilds/120/roles") | ("PATCH", "/api/v1/guilds/120/roles/121") => {
            Resp::json(200, role_body.clone())
        }
        ("GET", "/api/v1/channels/122") => Resp::json(200, json!({ "id": channel, "permission_overwrites": [] })),
        _ => Resp::empty(204),
    }).await;
    let client = authed(&fixture).await;

    assert_eq!(client.list_guild_roles(guild).await.unwrap()[0].id, role);
    assert_eq!(client.create_guild_role(guild, "  Helpers ").await.unwrap().id, role);
    let patch = RolePatch { permissions: Some(1 << 53), mentionable: Some(false), ..RolePatch::default() };
    assert_eq!(client.update_guild_role(guild, role, &patch).await.unwrap().permissions, 1 << 53);
    client.reorder_guild_roles(guild, &[(role, 2)]).await.unwrap();
    client.delete_guild_role(guild, role).await.unwrap();
    assert!(client.delete_guild_role(guild, guild).await.is_err());
    let overwrite = PermissionOverwrite { id: role, kind: 0, allow: 1 << 53, deny: 1 << 11 };
    client.set_channel_overwrite(channel, &overwrite).await.unwrap();
    assert!(client.set_channel_overwrite(channel, &PermissionOverwrite { deny: 1 << 53, ..overwrite }).await.is_err());
    client.delete_channel_overwrite(channel, role).await.unwrap();
    client.move_channel_and_sync(guild, channel, parent).await.unwrap();
    assert_eq!(client.get_guild_channel(channel).await.unwrap()["id"], "122");

    let expected = [
        ("GET", "/api/v1/guilds/120/roles"),
        ("POST", "/api/v1/guilds/120/roles"),
        ("PATCH", "/api/v1/guilds/120/roles/121"),
        ("PATCH", "/api/v1/guilds/120/roles"),
        ("DELETE", "/api/v1/guilds/120/roles/121"),
        ("PUT", "/api/v1/channels/122/permissions/121"),
        ("DELETE", "/api/v1/channels/122/permissions/121"),
        ("PATCH", "/api/v1/guilds/120/channels"),
        ("GET", "/api/v1/channels/122"),
    ].map(|(method, path)| l(method, path.to_owned()));
    assert_eq!(lines(&fixture), expected);
    let reqs = fixture.requests();
    assert_eq!(reqs[1].json(), json!({ "name": "Helpers" }));
    assert_eq!(reqs[2].json(), json!({ "permissions": "9007199254740992", "mentionable": false }));
    assert_eq!(reqs[3].json(), json!([{ "id": "121", "position": 2 }]));
    assert_eq!(reqs[5].json(), json!({ "type": 0, "allow": "9007199254740992", "deny": "2048" }));
    assert_eq!(reqs[7].json(), json!([{ "id": "122", "parent_id": "123", "lock_permissions": true }]));
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

#[tokio::test]
async fn group_dm_routes() {
    let fixture = Fixture::start(ok_fixture()).await;
    let client = authed(&fixture).await;
    let u2 = "423456789012345679";

    client.create_group_dm(&[USER.into(), u2.into()]).await.unwrap();
    client
        .update_group_dm(CHANNEL, &GroupDmPatch { name: Some(Some("Squad".into())), owner_id: Some(USER.into()) })
        .await
        .unwrap();
    client
        .update_group_dm(CHANNEL, &GroupDmPatch { name: Some(None), owner_id: None })
        .await
        .unwrap();
    client.add_group_dm_recipient(CHANNEL, u2).await.unwrap();
    client.remove_group_dm_recipient(CHANNEL, u2, false).await.unwrap();
    client.leave_channel(CHANNEL, true).await.unwrap();

    assert_eq!(
        lines(&fixture),
        vec![
            l("POST", "/api/v1/users/@me/channels".into()),
            l("PATCH", format!("/api/v1/channels/{CHANNEL}")),
            l("PATCH", format!("/api/v1/channels/{CHANNEL}")),
            l("PUT", format!("/api/v1/channels/{CHANNEL}/recipients/{u2}")),
            l("DELETE", format!("/api/v1/channels/{CHANNEL}/recipients/{u2}")),
            l("DELETE", format!("/api/v1/channels/{CHANNEL}?silent=true")),
        ]
    );
    let reqs = fixture.requests();
    assert_eq!(reqs[0].json(), json!({ "recipients": [USER, u2] }));
    assert_eq!(reqs[1].json(), json!({ "name": "Squad", "owner_id": USER }));
    assert_eq!(reqs[2].json(), json!({ "name": null }));
    assert!(reqs[3].body.is_empty());

    assert!(matches!(client.create_group_dm(&[]).await, Err(RestError::InvalidArgument("recipients"))));
    assert!(matches!(
        client.create_group_dm(&[USER.into(), USER.into()]).await,
        Err(RestError::InvalidArgument("recipients"))
    ));
    let over: Vec<String> = (1..=50).map(|n| n.to_string()).collect();
    assert!(matches!(client.create_group_dm(&over).await, Err(RestError::InvalidArgument("recipients"))));
    assert!(matches!(
        client.update_group_dm(CHANNEL, &GroupDmPatch { name: Some(Some("😀".repeat(51))), owner_id: None }).await,
        Err(RestError::InvalidArgument("name"))
    ));
    assert!(matches!(client.add_group_dm_recipient("bad", u2).await, Err(RestError::InvalidArgument(_))));
    assert_eq!(fixture.requests().len(), 6);
}

#[tokio::test]
async fn guild_channel_crud_routes() {
    let fixture = Fixture::start(ok_fixture()).await;
    let client = authed(&fixture).await;
    let cat = "623456789012345678";

    client
        .create_guild_channel(
            GUILD,
            &CreateGuildChannelRequest {
                name: "  voice-chat  ".into(),
                kind: crate::state::ChannelKind::GuildVoice,
                parent_id: Some(cat.into()),
                topic: None,
                nsfw: Some(false),
                rate_limit_per_user: None,
                bitrate: Some(VOICE_CHANNEL_BITRATE_DEFAULT),
                user_limit: Some(0),
                voice_connection_limit: Some(VOICE_CHANNEL_CONNECTION_LIMIT_DEFAULT),
            },
        )
        .await
        .unwrap();
    client
        .update_guild_channel(
            CHANNEL,
            &GuildChannelPatch {
                name: Some("  news  ".into()),
                kind: Some(crate::state::ChannelKind::GuildAnnouncement),
                topic: Some(None),
                parent_id: Some(None),
                nsfw: None,
                nsfw_override: Some(Some(false)),
                rate_limit_per_user: Some(15),
            },
        )
        .await
        .unwrap();
    client.delete_channel(CHANNEL).await.unwrap();

    assert_eq!(
        lines(&fixture),
        vec![
            l("POST", format!("/api/v1/guilds/{GUILD}/channels")),
            l("PATCH", format!("/api/v1/channels/{CHANNEL}")),
            l("DELETE", format!("/api/v1/channels/{CHANNEL}")),
        ]
    );
    let reqs = fixture.requests();
    assert_eq!(
        reqs[0].json(),
        json!({
            "name": "voice-chat",
            "type": 2,
            "parent_id": cat,
            "nsfw": false,
            "bitrate": VOICE_CHANNEL_BITRATE_DEFAULT,
            "user_limit": 0,
            "voice_connection_limit": VOICE_CHANNEL_CONNECTION_LIMIT_DEFAULT,
        })
    );
    assert_eq!(
        reqs[1].json(),
        json!({
            "name": "news",
            "type": 5,
            "topic": null,
            "parent_id": null,
            "nsfw_override": false,
            "rate_limit_per_user": 15,
        })
    );
    for req in &reqs {
        assert_eq!(req.header("authorization"), Some(FAKE_TOKEN));
    }

    assert!(matches!(
        client
            .create_guild_channel("1/../2", &CreateGuildChannelRequest {
                name: "x".into(),
                kind: crate::state::ChannelKind::GuildText,
                parent_id: None,
                topic: None,
                nsfw: None,
                rate_limit_per_user: None,
                bitrate: None,
                user_limit: None,
                voice_connection_limit: None,
            })
            .await,
        Err(RestError::InvalidArgument("guild_id"))
    ));
    assert!(matches!(
        client.update_guild_channel(CHANNEL, &GuildChannelPatch::default()).await,
        Err(RestError::InvalidArgument("patch"))
    ));
    assert_eq!(fixture.requests().len(), 3);
}

#[tokio::test]
async fn guild_member_moderation_routes() {
    let fixture = Fixture::start(|req, _| match (req.method.as_str(), req.target.as_str()) {
        ("GET", t) if t.ends_with("/bans") => Resp::json(
            200,
            json!([{
                "user": { "id": USER, "username": "alice", "global_name": "Alice" },
                "reason": "spam",
                "banned_at": "2026-11-19T00:00:00Z"
            }]),
        ),
        ("DELETE", _) => Resp::empty(204),
        ("PUT", _) => Resp::empty(204),
        _ => Resp::json(200, json!({ "user": { "id": USER, "username": "alice" }, "roles": [] })),
    })
    .await;
    let client = authed(&fixture).await;

    client.kick_guild_member(GUILD, USER, Some("규칙 위반")).await.unwrap();
    client
        .ban_guild_member(
            GUILD,
            USER,
            &BanMemberRequest {
                delete_message_seconds: Some(3600),
                reason: Some("  스팸 도배  ".into()),
                ban_duration_seconds: Some(86_400),
            },
        )
        .await
        .unwrap();
    let bans = client.list_guild_bans(GUILD).await.unwrap();
    assert_eq!(bans.len(), 1);
    assert_eq!(bans[0].user.username, "alice");
    assert_eq!(bans[0].reason.as_deref(), Some("spam"));
    client.unban_guild_member(GUILD, USER, Some("해제")).await.unwrap();
    client
        .update_guild_member(
            GUILD,
            USER,
            &GuildMemberPatch {
                nick: Some(None),
                communication_disabled_until: Some(None),
                timeout_reason: Some("사유".into()),
                ..Default::default()
            },
            Some("  정정  "),
        )
        .await
        .unwrap();
    client
        .update_own_guild_member(
            GUILD,
            &GuildMemberPatch { nick: Some(Some("  MyNick  ".into())), ..Default::default() },
        )
        .await
        .unwrap();

    assert_eq!(
        lines(&fixture),
        vec![
            l("DELETE", format!("/api/v1/guilds/{GUILD}/members/{USER}")),
            l("PUT", format!("/api/v1/guilds/{GUILD}/bans/{USER}")),
            l("GET", format!("/api/v1/guilds/{GUILD}/bans")),
            l("DELETE", format!("/api/v1/guilds/{GUILD}/bans/{USER}")),
            l("PATCH", format!("/api/v1/guilds/{GUILD}/members/{USER}")),
            l("PATCH", format!("/api/v1/guilds/{GUILD}/members/@me")),
        ]
    );
    let reqs = fixture.requests();
    assert_eq!(reqs[0].header("x-audit-log-reason"), Some(encode_uri_component("규칙 위반").as_str()));
    assert_eq!(reqs[1].header("x-audit-log-reason"), Some(encode_uri_component("스팸 도배").as_str()));
    assert_eq!(
        reqs[1].json(),
        json!({ "delete_message_seconds": 3600, "reason": "스팸 도배", "ban_duration_seconds": 86_400 })
    );
    assert_eq!(reqs[3].header("x-audit-log-reason"), Some(encode_uri_component("해제").as_str()));
    assert_eq!(reqs[4].header("x-audit-log-reason"), Some(encode_uri_component("정정").as_str()));
    assert_eq!(
        reqs[4].json(),
        json!({ "nick": null, "communication_disabled_until": null, "timeout_reason": "사유" })
    );
    assert_eq!(reqs[5].header("x-audit-log-reason"), None);
    assert_eq!(reqs[5].json(), json!({ "nick": "MyNick" }));

    assert!(matches!(
        client
            .update_own_guild_member(
                GUILD,
                &GuildMemberPatch {
                    communication_disabled_until: Some(Some("2026-12-01T00:00:00Z".into())),
                    ..Default::default()
                },
            )
            .await,
        Err(RestError::InvalidArgument("communication_disabled_until"))
    ));
    assert!(matches!(
        client.kick_guild_member(GUILD, "@me", None).await,
        Err(RestError::InvalidArgument("user_id"))
    ));
    assert_eq!(fixture.requests().len(), 6);
}

#[tokio::test]
async fn guild_privacy_profile_and_bulk_delete_my_messages_endpoints() {
    let fixture = Fixture::start(|req, _| {
        if req.target.contains("bulk-delete-mine") {
            Resp::empty(202)
        } else {
            Resp::json(200, json!({ "ok": true }))
        }
    })
    .await;
    let client = authed(&fixture).await;

    let privacy = UserSettingsPatch {
        restricted_guilds: Some(vec![GUILD.to_owned()]),
        bot_restricted_guilds: Some(vec![]),
        ..Default::default()
    };
    client.update_user_settings(&privacy).await.unwrap();
    client
        .update_own_guild_member(
            GUILD,
            &GuildMemberPatch {
                bio: Some(Some("  hello world  ".into())),
                pronouns: Some(Some("  they/them  ".into())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    client.bulk_delete_my_guild_messages(GUILD, None).await.unwrap();
    client.bulk_delete_my_guild_messages(GUILD, Some("secret")).await.unwrap();

    assert_eq!(
        lines(&fixture),
        vec![
            l("PATCH", "/api/v1/users/@me/settings".into()),
            l("PATCH", format!("/api/v1/guilds/{GUILD}/members/@me")),
            l("POST", format!("/api/v1/users/@me/guilds/{GUILD}/messages/bulk-delete-mine")),
            l("POST", format!("/api/v1/users/@me/guilds/{GUILD}/messages/bulk-delete-mine")),
        ]
    );
    let reqs = fixture.requests();
    assert_eq!(reqs[0].json(), json!({ "restricted_guilds": [GUILD], "bot_restricted_guilds": [] }));
    assert_eq!(reqs[1].json(), json!({ "bio": "hello world", "pronouns": "they/them" }));
    assert_eq!(reqs[2].json(), json!({}));
    assert_eq!(reqs[3].json(), json!({ "password": "secret" }));

    assert!(matches!(
        client
            .update_user_settings(&UserSettingsPatch {
                restricted_guilds: Some(vec!["bad".into()]),
                ..Default::default()
            })
            .await,
        Err(RestError::InvalidArgument("restricted_guilds"))
    ));
    assert!(matches!(
        client
            .update_own_guild_member(
                GUILD,
                &GuildMemberPatch { bio: Some(Some("x".repeat(MAX_MEMBER_BIO_UTF16 + 1))), ..Default::default() },
            )
            .await,
        Err(RestError::InvalidArgument("bio"))
    ));
    assert!(matches!(
        client
            .update_own_guild_member(
                GUILD,
                &GuildMemberPatch { pronouns: Some(Some("   ".into())), ..Default::default() },
            )
            .await,
        Err(RestError::InvalidArgument("pronouns"))
    ));
}

#[tokio::test]
async fn purge_personal_notes_and_call_ring_routes() {
    let fx = Fixture::start(|req, _| {
        if req.method == "POST" && req.target == format!("/api/v1/channels/{CHANNEL}/messages/purge") {
            return Resp::json(200, json!({ "deleted_count": 7 }));
        }
        Resp::empty(204)
    })
    .await;
    let client = authed(&fx).await;

    let deleted = client.purge_personal_notes_messages(CHANNEL).await.unwrap();
    assert_eq!(deleted, 7);

    let targets = vec![USER.to_owned()];
    client.ring_call_recipients(CHANNEL, None).await.unwrap();
    client.ring_call_recipients(CHANNEL, Some(&targets)).await.unwrap();
    client.stop_ringing_call_recipients(CHANNEL, Some(&targets)).await.unwrap();

    assert_eq!(
        lines(&fx),
        vec![
            l("POST", format!("/api/v1/channels/{CHANNEL}/messages/purge")),
            l("POST", format!("/api/v1/channels/{CHANNEL}/call/ring")),
            l("POST", format!("/api/v1/channels/{CHANNEL}/call/ring")),
            l("POST", format!("/api/v1/channels/{CHANNEL}/call/stop-ringing")),
        ]
    );
    let reqs = fx.requests();
    assert_eq!(reqs[1].json(), json!({}));
    assert_eq!(reqs[2].json(), json!({ "recipients": [USER] }));
    assert_eq!(reqs[3].json(), json!({ "recipients": [USER] }));

    assert!(matches!(
        client.purge_personal_notes_messages("bad").await,
        Err(RestError::InvalidArgument("channel_id"))
    ));
    assert!(matches!(
        client.ring_call_recipients(CHANNEL, Some(&["bad".to_owned()])).await,
        Err(RestError::InvalidArgument("recipients"))
    ));
}
