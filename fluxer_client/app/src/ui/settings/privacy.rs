use super::*;

pub(super) fn render_privacy_tab(
    view: &ClientView,
    modal: &SettingsModal,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    const CALL_EVERYONE: u32 = 1 << 2;
    const CALL_FRIENDS_ONLY: u32 = 1 << 3;
    const CALL_NOBODY: u32 = 1 << 4;
    const CALL_SILENT: u32 = 1 << 5;
    const GDM_EVERYONE: u32 = 1 << 2;
    const GDM_FRIENDS_ONLY: u32 = 1 << 3;
    const GDM_NOBODY: u32 = 1 << 4;

    let trusted_count = view.effective_trusted_domains().len();
    let active_friend_req = view.prefs.data.friend_request_access.as_str();
    let active_spoiler = view.prefs.data.spoiler_display.as_str();

    let (
        profile_privacy,
        default_guilds_restricted,
        bot_default_guilds_restricted,
        incoming_call_flags,
        group_dm_flags,
        default_share_voice_activity,
        friend_dm_filter,
        non_friend_dm_filter,
        guild_filter,
    ) = {
        let state = view.backend.state();
        let raw = &state.user_settings.raw;
        (
            raw.get("profile_privacy").and_then(|v| v.as_u64()).unwrap_or(0) as u8,
            raw.get("default_guilds_restricted")
                .and_then(|v| v.as_bool())
                .unwrap_or(!view.prefs.data.allow_guild_dms),
            raw.get("bot_default_guilds_restricted")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            raw.get("incoming_call_flags")
                .and_then(|v| v.as_u64())
                .unwrap_or(u64::from(CALL_FRIENDS_ONLY)) as u32,
            raw.get("group_dm_add_permission_flags")
                .and_then(|v| v.as_u64())
                .unwrap_or(u64::from(GDM_FRIENDS_ONLY)) as u32,
            raw.get("default_share_voice_activity")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            raw.get("sensitive_content_friend_dm_filter")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u8,
            raw.get("sensitive_content_non_friend_dm_filter")
                .and_then(|v| v.as_u64())
                .unwrap_or(1) as u8,
            raw.get("sensitive_content_guild_filter")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u8,
        )
    };

    let profile_privacy_options: &[(u8, &str, &str)] = &[
        (
            0,
            "Friends and all communities",
            "Your full profile is visible to friends and to anyone in your communities",
        ),
        (
            1,
            "Friends and small communities only",
            "Your full profile is visible to friends and members of your communities with 200 or fewer members",
        ),
        (2, "Friends only", "Your full profile is only visible to your friends"),
    ];
    let friend_req_options: &[(&str, &str, u32)] = &[
        ("everyone", "Everyone", 1 << 2),
        ("friends_of_friends", "Friends of Friends", 1 << 0),
        ("no_one", "No one", 0),
    ];
    let call_options: &[(u32, &str)] = &[
        (CALL_NOBODY, "Block all"),
        (CALL_FRIENDS_ONLY, "Friends only"),
        (CALL_EVERYONE, "Everyone"),
    ];
    let group_dm_options: &[(u32, &str)] = &[
        (GDM_NOBODY, "No one"),
        (GDM_FRIENDS_ONLY, "Friends only"),
        (GDM_EVERYONE, "Everyone"),
    ];
    let sensitive_dm_options: &[(u8, &str)] = &[(0, "Show"), (1, "Blur"), (2, "Block")];
    let sensitive_guild_options: &[(u8, &str)] = &[(0, "Show"), (1, "Blur")];
    let spoiler_options: &[(&str, &str)] = &[
        ("on_click", "On click"),
        ("moderator", "In channels I moderate"),
        ("always", "Always"),
    ];

    let active_call_base = if (incoming_call_flags & CALL_NOBODY) != 0 {
        CALL_NOBODY
    } else if (incoming_call_flags & CALL_EVERYONE) != 0 {
        CALL_EVERYONE
    } else {
        CALL_FRIENDS_ONLY
    };
    let silent_calls = (incoming_call_flags & CALL_SILENT) != 0;
    let active_gdm_base = if (group_dm_flags & GDM_NOBODY) != 0 {
        GDM_NOBODY
    } else if (group_dm_flags & GDM_EVERYONE) != 0 {
        GDM_EVERYONE
    } else {
        GDM_FRIENDS_ONLY
    };

    let profile_privacy_section = settings_section_block(
        l10n::tr("Profile privacy"),
        Some(l10n::tr("Who can see your full profile")),
        div()
            .flex()
            .flex_col()
            .gap_2()
            .children(profile_privacy_options.iter().map(|&(level, label, desc)| {
                let selected = profile_privacy == level;
                let sel_id = format!("settings-profile-privacy-{level}");
                div()
                    .id(SharedString::from(sel_id.clone()))
                    .debug_selector(move || sel_id.clone())
                    .p_3()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(if selected { theme.brand } else { settings_border(theme) })
                    .bg(settings_card_bg(theme))
                    .cursor_pointer()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.settings_update_privacy_patch(
                            UserSettingsPatch {
                                profile_privacy: Some(level),
                                ..Default::default()
                            },
                            cx,
                        );
                    }))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .min_w_0()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(l10n::tr(label)))
                            .child(div().text_xs().text_color(theme.muted_foreground).child(l10n::tr(desc))),
                    )
                    .when(selected, |d| {
                        d.child(Icon::new(AppIcon::Check).with_size(px(16.)).text_color(theme.brand))
                    })
            }))
            .into_any_element(),
        theme,
    );

    let friends_dms_section = settings_section_block(
        l10n::tr("Friends & direct messages"),
        None,
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .py_2p5()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .min_w_0()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(l10n::tr("Allow direct messages from community members")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(l10n::tr(
                                        "Applies to communities you join from now on. You can also change it for each community.",
                                    )),
                            ),
                    )
                    .child(
                        div().debug_selector(|| "settings-allow-guild-dms".to_owned()).child(
                            Switch::new("settings-allow-guild-dms")
                                .checked(!default_guilds_restricted)
                                .on_change(cx.listener(|this, checked: &bool, window, cx| {
                                    this.apply_settings_effect(
                                        SettingsEffect::AllowGuildDmsChanged(*checked),
                                        window,
                                        cx,
                                    );
                                    this.settings_update_privacy_patch(
                                        UserSettingsPatch {
                                            default_guilds_restricted: Some(!*checked),
                                            ..Default::default()
                                        },
                                        cx,
                                    );
                                })),
                        ),
                    ),
            )
            .child(
                div()
                    .py_2p5()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .min_w_0()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(l10n::tr("Allow direct messages from community bots")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(l10n::tr(
                                        "Applies to bots in communities you join from now on. You can also change it for each community.",
                                    )),
                            ),
                    )
                    .child(
                        div().debug_selector(|| "settings-allow-bot-dms".to_owned()).child(
                            Switch::new("settings-allow-bot-dms")
                                .checked(!bot_default_guilds_restricted)
                                .on_change(cx.listener(|this, checked: &bool, _, cx| {
                                    this.settings_update_privacy_patch(
                                        UserSettingsPatch {
                                            bot_default_guilds_restricted: Some(!*checked),
                                            ..Default::default()
                                        },
                                        cx,
                                    );
                                })),
                        ),
                    ),
            )
            .child(
                div()
                    .py_2p5()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .min_w_0()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(l10n::tr("Friend requests")))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(l10n::tr("Choose who can send you a friend request.")),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap_1()
                            .children(friend_req_options.iter().map(|&(val, label, flag_bits)| {
                                let selected = active_friend_req == val;
                                let sel_id = format!("settings-friend-req-{val}");
                                let val_owned = val.to_owned();
                                let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                    .small()
                                    .label(l10n::tr(label));
                                btn = if selected { btn.primary() } else { btn.ghost() };
                                div().debug_selector(move || sel_id.clone()).child(btn.on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.apply_settings_effect(
                                            SettingsEffect::FriendRequestAccessChanged(val_owned.clone()),
                                            window,
                                            cx,
                                        );
                                        this.settings_update_privacy_patch(
                                            UserSettingsPatch {
                                                friend_source_flags: Some(flag_bits),
                                                ..Default::default()
                                            },
                                            cx,
                                        );
                                    },
                                )))
                            })),
                    ),
            )
            .into_any_element(),
        theme,
    );

    let communication_section = settings_section_block(
        l10n::tr("Communication"),
        None,
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .py_2p5()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .flx("user.privacy-safety-tab.communication-tab.communication-tab-content.select.incoming-call-base-change.row")
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .min_w_0()
                            .flx("user.privacy-safety-tab.communication-tab.communication-tab-content.select.incoming-call-base-change.text")
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .flx("user.privacy-safety-tab.communication-tab.communication-tab-content.select.incoming-call-base-change.label")
                                    .child(l10n::tr("Allowed callers")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(l10n::tr("Choose who can start a call with you.")),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap_1()
                            .flx("user.privacy-safety-tab.communication-tab.communication-tab-content.select.incoming-call-base-change")
                            .children(call_options.iter().map(|&(bits, label)| {
                                let selected = active_call_base == bits;
                                let sel_id = format!("settings-incoming-call-{bits}");
                                let next_flags = if bits == CALL_NOBODY {
                                    CALL_NOBODY
                                } else if silent_calls {
                                    bits | CALL_SILENT
                                } else {
                                    bits
                                };
                                let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                    .small()
                                    .label(l10n::tr(label));
                                btn = if selected { btn.primary() } else { btn.ghost() };
                                div().debug_selector(move || sel_id.clone()).child(btn.on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.settings_update_privacy_patch(
                                            UserSettingsPatch {
                                                incoming_call_flags: Some(next_flags),
                                                ..Default::default()
                                            },
                                            cx,
                                        );
                                    },
                                )))
                            })),
                    ),
            )
            .when(active_call_base != CALL_NOBODY, |d| {
                d.child(
                    div()
                        .py_2p5()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap_4()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_0p5()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(l10n::tr("Receive calls silently")),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(l10n::tr("Calls you allow still come through, but they don't ring.")),
                                ),
                        )
                        .child(
                            Switch::new("settings-silent-calls")
                                .checked(silent_calls)
                                .on_change(cx.listener(move |this, checked: &bool, _, cx| {
                                    let next = if *checked {
                                        active_call_base | CALL_SILENT
                                    } else {
                                        active_call_base & !CALL_SILENT
                                    };
                                    this.settings_update_privacy_patch(
                                        UserSettingsPatch {
                                            incoming_call_flags: Some(next),
                                            ..Default::default()
                                        },
                                        cx,
                                    );
                                })),
                        ),
                )
            })
            .child(
                div()
                    .py_2p5()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .flx("user.privacy-safety-tab.communication-tab.communication-tab-content.select.group-dm-add-base-change.row")
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .min_w_0()
                            .flx("user.privacy-safety-tab.communication-tab.communication-tab-content.select.group-dm-add-base-change.text")
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .flx("user.privacy-safety-tab.communication-tab.communication-tab-content.select.group-dm-add-base-change.label")
                                    .child(l10n::tr("Allowed group chat invites")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(l10n::tr("Choose who can add you to a group chat without asking first.")),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap_1()
                            .flx("user.privacy-safety-tab.communication-tab.communication-tab-content.select.group-dm-add-base-change")
                            .children(group_dm_options.iter().map(|&(bits, label)| {
                                let selected = active_gdm_base == bits;
                                let sel_id = format!("settings-group-dm-add-{bits}");
                                let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                    .small()
                                    .label(l10n::tr(label));
                                btn = if selected { btn.primary() } else { btn.ghost() };
                                div().debug_selector(move || sel_id.clone()).child(btn.on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.settings_update_privacy_patch(
                                            UserSettingsPatch {
                                                group_dm_add_permission_flags: Some(bits),
                                                ..Default::default()
                                            },
                                            cx,
                                        );
                                    },
                                )))
                            })),
                    ),
            )
            .into_any_element(),
        theme,
    );

    let activity_sharing_section = settings_section_block(
        l10n::tr("Activity sharing"),
        None,
        info_card_row(
            l10n::tr("Share your voice activity with friends"),
            l10n::tr("Friends see which voice channel you're in on their Active now list. You can change this once every 24 hours."),
            Some(if default_share_voice_activity {
                l10n::tr("On")
            } else {
                l10n::tr("Off")
            }),
            theme,
        ),
        theme,
    );

    let sensitive_content_section = settings_section_block(
        l10n::tr("Sensitive content"),
        Some(l10n::tr(
            "Choose what happens to images and videos flagged as sensitive. Show displays them as usual, Blur hides them until you click, and Block hides them completely.",
        )),
        div()
            .flex()
            .flex_col()
            .gap_2()
            .flx("user.privacy-safety-tab.sensitive-content-tab.sensitive-content-tab-content.rows")
            .child(
                div()
                    .py_2p5()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .flx("user.privacy-safety-tab.sensitive-content-tab.sensitive-content-tab-content.select.set-friend-dm-filter.row")
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .min_w_0()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(l10n::tr("Direct messages from friends")))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(l10n::tr("Media your friends send you in direct messages.")),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_1()
                            .flx("user.privacy-safety-tab.sensitive-content-tab.sensitive-content-tab-content.select.set-friend-dm-filter")
                            .children(sensitive_dm_options.iter().map(|&(lvl, label)| {
                                let selected = friend_dm_filter == lvl;
                                let sel_id = format!("settings-sensitive-friend-{lvl}");
                                let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                    .small()
                                    .label(l10n::tr(label));
                                btn = if selected { btn.primary() } else { btn.ghost() };
                                div().debug_selector(move || sel_id.clone()).child(btn.on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.settings_update_privacy_patch(
                                            UserSettingsPatch {
                                                sensitive_content_friend_dm_filter: Some(lvl),
                                                ..Default::default()
                                            },
                                            cx,
                                        );
                                    },
                                )))
                            })),
                    ),
            )
            .child(
                div()
                    .py_2p5()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .flx("user.privacy-safety-tab.sensitive-content-tab.sensitive-content-tab-content.select.set-non-friend-dm-filter.row")
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .min_w_0()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(l10n::tr("Direct messages from others")))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(l10n::tr("Media in group chats and in direct messages from people who aren't your friends.")),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_1()
                            .flx("user.privacy-safety-tab.sensitive-content-tab.sensitive-content-tab-content.select.set-non-friend-dm-filter")
                            .children(sensitive_dm_options.iter().map(|&(lvl, label)| {
                                let selected = non_friend_dm_filter == lvl;
                                let sel_id = format!("settings-sensitive-other-{lvl}");
                                let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                    .small()
                                    .label(l10n::tr(label));
                                btn = if selected { btn.primary() } else { btn.ghost() };
                                div().debug_selector(move || sel_id.clone()).child(btn.on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.settings_update_privacy_patch(
                                            UserSettingsPatch {
                                                sensitive_content_non_friend_dm_filter: Some(lvl),
                                                ..Default::default()
                                            },
                                            cx,
                                        );
                                    },
                                )))
                            })),
                    ),
            )
            .child(
                div()
                    .py_2p5()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .flx("user.privacy-safety-tab.sensitive-content-tab.sensitive-content-tab-content.select.set-guild-filter.row")
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .min_w_0()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(l10n::tr("Messages in community channels")))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(l10n::tr("Media in channels that are not marked 18+.")),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_1()
                            .flx("user.privacy-safety-tab.sensitive-content-tab.sensitive-content-tab-content.select.set-guild-filter")
                            .children(sensitive_guild_options.iter().map(|&(lvl, label)| {
                                let selected = guild_filter == lvl;
                                let sel_id = format!("settings-sensitive-guild-{lvl}");
                                let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                    .small()
                                    .label(l10n::tr(label));
                                btn = if selected { btn.primary() } else { btn.ghost() };
                                div().debug_selector(move || sel_id.clone()).child(btn.on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.settings_update_privacy_patch(
                                            UserSettingsPatch {
                                                sensitive_content_guild_filter: Some(lvl),
                                                ..Default::default()
                                            },
                                            cx,
                                        );
                                    },
                                )))
                            })),
                    ),
            )
            .child(
                div()
                    .py_2p5()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .min_w_0()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(l10n::tr("Spoilers & sensitive media")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(l10n::tr("Choose when spoiler-tagged text and media are revealed.")),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap_1()
                            .children(spoiler_options.iter().map(|&(val, label)| {
                                let selected = active_spoiler == val;
                                let sel_id = format!("settings-spoiler-mode-{val}");
                                let val_owned = val.to_owned();
                                let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                    .small()
                                    .label(l10n::tr(label));
                                btn = if selected { btn.primary() } else { btn.ghost() };
                                div().debug_selector(move || sel_id.clone()).child(btn.on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.apply_settings_effect(
                                            SettingsEffect::SpoilerDisplayChanged(val_owned.clone()),
                                            window,
                                            cx,
                                        );
                                    },
                                )))
                            })),
                    ),
            )
            .into_any_element(),
        theme,
    );

    let link_protection_section = settings_section_block(
        l10n::tr("Link protection"),
        None,
        div()
            .py_2p5()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap_4()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(format!("{} ({trusted_count})", l10n::tr("Trusted external domains"))),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(l10n::tr("Untrusted external links show a confirmation dialog before opening.")),
                    ),
            )
            .when(trusted_count > 0, |d| {
                d.child(
                    Button::new("settings-privacy-clear-trusted")
                        .ghost()
                        .small()
                        .label(l10n::tr("Clear"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.apply_settings_effect(SettingsEffect::ClearTrustedDomains, window, cx);
                        })),
                )
            })
            .into_any_element(),
        theme,
    );

    let data_export_section = settings_section_block(
        l10n::tr("Data export"),
        None,
        div()
            .flex()
            .flex_col()
            .gap_3()
            .flx("user.privacy-safety-tab.data-export-tab.data-export-tab-content.delete-section")
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .flx("user.privacy-safety-tab.data-export-tab.data-export-tab-content.warning-text")
                    .child(l10n::tr(
                        "Request a copy of your account data. Export execution requires verified confirmation.",
                    )),
            )
            .child(
                div()
                    .debug_selector(|| "settings-privacy-data-export".to_owned())
                    .flx("user.privacy-safety-tab.data-export-tab.data-export-tab-content.button.open-modal")
                    .child(
                        Button::new("settings-privacy-data-export")
                            .primary()
                            .small()
                            .label(l10n::tr("Export my data"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                // TODO(backlog, T3): Connect POST /users/@me/harvest data export flow.
                                this.settings_set_privacy_notice(
                                    l10n::tr("Data export requests are managed in the web client for now."),
                                    cx,
                                );
                            })),
                    ),
            )
            .into_any_element(),
        theme,
    );

    let data_deletion_section = settings_section_block(
        l10n::tr("Data deletion"),
        None,
        div()
            .flex()
            .flex_col()
            .gap_3()
            .flx("user.privacy-safety-tab.data-deletion-tab.data-deletion-tab-content.delete-section")
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .flx("user.privacy-safety-tab.data-deletion-tab.data-deletion-tab-content.warning-text")
                    .child(l10n::tr(
                        "Permanently delete your messages or account data. This action cannot be undone.",
                    )),
            )
            .child(
                div()
                    .debug_selector(|| "settings-privacy-data-deletion".to_owned())
                    .flx("user.privacy-safety-tab.data-deletion-tab.data-deletion-tab-content.button.open-modal")
                    .child(
                        Button::new("settings-privacy-data-deletion")
                            .danger()
                            .small()
                            .label(l10n::tr("Delete my messages"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                // TODO(backlog, T3): Connect verified message/account data deletion flow.
                                this.settings_set_privacy_notice(
                                    l10n::tr("Bulk data deletion requires verified confirmation in the web client."),
                                    cx,
                                );
                            })),
                    ),
            )
            .into_any_element(),
        theme,
    );

    div()
        .debug_selector(|| "settings-privacy-dm".to_owned())
        .flex()
        .flex_col()
        .gap_5()
        .flx("user.privacy-safety-tab.privacy-dashboard-content.container")
        .when_some(modal.privacy_notice.clone(), |d, notice| {
            d.child(
                div()
                    .debug_selector(|| "settings-privacy-notice".to_owned())
                    .p_3()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(settings_border(theme))
                    .bg(settings_card_bg(theme))
                    .text_xs()
                    .text_color(theme.foreground)
                    .child(notice),
            )
        })
        .child(profile_privacy_section)
        .child(settings_divider(theme))
        .child(friends_dms_section)
        .child(settings_divider(theme))
        .child(communication_section)
        .child(settings_divider(theme))
        .child(activity_sharing_section)
        .child(settings_divider(theme))
        .child(sensitive_content_section)
        .child(settings_divider(theme))
        .child(link_protection_section)
        .child(settings_divider(theme))
        .child(data_export_section)
        .child(settings_divider(theme))
        .child(data_deletion_section)
        .into_any_element()
}
