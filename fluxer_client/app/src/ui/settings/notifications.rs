use super::*;

struct SoundEntry {
    id: &'static str,
    label: &'static str,
}

const COMMON_SOUNDS: &[SoundEntry] = &[
    SoundEntry {
        id: "message",
        label: "Community message notifications",
    },
    SoundEntry {
        id: "direct_message",
        label: "Direct message notifications",
    },
    SoundEntry {
        id: "same_channel_message",
        label: "Current channel message notifications",
    },
    SoundEntry {
        id: "mute",
        label: "Voice mute",
    },
    SoundEntry {
        id: "unmute",
        label: "Voice unmute",
    },
    SoundEntry {
        id: "deaf",
        label: "Voice deafen",
    },
    SoundEntry {
        id: "undeaf",
        label: "Voice undeafen",
    },
    SoundEntry {
        id: "incoming_ring",
        label: "Incoming call",
    },
];

const EXTRA_SOUNDS: &[SoundEntry] = &[
    SoundEntry {
        id: "user_join",
        label: "User joined voice",
    },
    SoundEntry {
        id: "user_leave",
        label: "User left voice",
    },
    SoundEntry {
        id: "stream_start",
        label: "Stream started",
    },
    SoundEntry {
        id: "stream_end",
        label: "Stream ended",
    },
];

fn notif_section(title: String, is_first: bool, content: AnyElement, theme: &Theme) -> AnyElement {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(16.))
        .pb(px(24.))
        .when(!is_first, |d| d.pt(px(24.)).border_t_1().border_color(settings_border(theme)))
        .flx("app.settings-section.section")
        .child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(4.))
                .flx("app.settings-section.section-header")
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .flx("app.settings-section.section-header-text")
                        .child(section_heading(title, theme)),
                ),
        )
        .child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(20.))
                .flx("app.settings-section.section-content")
                .child(content),
        )
        .into_any_element()
}

fn notif_switch_row(
    id: &'static str,
    label: String,
    description: Option<String>,
    checked: bool,
    disabled: bool,
    theme: &Theme,
    on_change: impl Fn(&mut ClientView, &bool, &mut Window, &mut Context<ClientView>) + 'static,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    div()
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(16.))
        .py(px(2.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.foreground)
                        .child(label),
                )
                .when_some(description, |d, desc| {
                    d.child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .text_color(theme.text_secondary)
                            .child(desc),
                    )
                }),
        )
        .child(
            div().flex_none().debug_selector(move || id.to_owned()).child(
                Switch::new(id)
                    .checked(checked)
                    .disabled(disabled)
                    .on_change(cx.listener(on_change)),
            ),
        )
        .into_any_element()
}

fn notif_radio_option(
    id: &'static str,
    title: &'static str,
    desc: &'static str,
    selected: bool,
    theme: &Theme,
    on_click: impl Fn(&mut ClientView, &gpui_kit::ClickEvent, &mut Window, &mut Context<ClientView>) + 'static,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let hover_bg = theme.hover_bg;
    div()
        .id(id)
        .role(gpui_kit::Role::Button)
        .aria_label(l10n::tr(title))
        .aria_selected(selected)
        .w_full()
        .py(px(5.))
        .px(px(8.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(move |s| s.bg(hover_bg))
        .flex()
        .flex_row()
        .items_start()
        .gap(px(12.))
        .on_click(cx.listener(on_click))
        .child(
            div()
                .mt(px(2.))
                .size(px(18.))
                .flex_none()
                .rounded_full()
                .border_2()
                .border_color(if selected { theme.brand } else { settings_border(theme) })
                .when(selected, |d| {
                    d.bg(theme.brand)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(div().size(px(6.)).rounded_full().bg(theme.on_fill))
                }),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.foreground)
                        .child(l10n::tr(title)),
                )
                .child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .text_color(theme.text_secondary)
                        .child(l10n::tr(desc)),
                ),
        )
        .into_any_element()
}

fn render_sound_row(
    entry: &SoundEntry,
    all_disabled: bool,
    enabled: bool,
    is_last: bool,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let sound_id = entry.id;
    let label = l10n::tr(entry.label);
    let hover_bg = theme.hover_bg;
    let switch_id: SharedString = format!("settings-sound-toggle-{sound_id}").into();
    let preview_id: SharedString = format!("settings-sound-preview-{sound_id}").into();
    let upload_id: SharedString = format!("settings-sound-upload-{sound_id}").into();

    div()
        .w_full()
        .h(px(56.))
        .px(px(12.))
        .when(!is_last, |d| d.border_b_1().border_color(settings_border(theme)))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_col()
                .items_start()
                .gap(px(1.))
                .flx("user.notifications-tab.sounds.sound-label")
                .child(
                    div()
                        .max_w_full()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .flx("user.notifications-tab.sounds.sound-label-title")
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.9))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.foreground)
                                .truncate()
                                .flx("user.notifications-tab.sounds.span")
                                .child(label),
                        ),
                )
                .child(
                    div()
                        .id(preview_id)
                        .role(gpui_kit::Role::Button)
                        .when(!all_disabled, |d| d.cursor_pointer().hover(|s| s.underline()))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text_link)
                        .when(all_disabled, |d| d.opacity(0.5))
                        .flx("user.notifications-tab.sounds.preview-link.preview-sound.button")
                        .on_click(cx.listener(move |_this, _, _, cx| {
                            if !all_disabled {
                                // TODO(backlog): play preview audio for `sound_id`
                                cx.notify();
                            }
                        }))
                        .child(l10n::tr("Preview sound")),
                ),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(14.))
                .child(
                    div()
                        .id(upload_id)
                        .role(gpui_kit::Role::Button)
                        .size(px(28.))
                        .rounded(px(4.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(!all_disabled, |d| d.cursor_pointer().hover(move |s| s.bg(hover_bg)))
                        .when(all_disabled, |d| d.opacity(0.5))
                        .flx("user.notifications-tab.sounds.icon-button.upload-click")
                        .on_click(cx.listener(move |_this, _, _, cx| {
                            if !all_disabled {
                                // TODO(backlog): open custom audio upload picker for `sound_id`
                                cx.notify();
                            }
                        }))
                        .child(
                            div()
                                .size(px(16.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .flx("user.notifications-tab.sounds.upload-icon")
                                .child(Icon::new(AppIcon::UploadFill).size(px(16.)).text_color(theme.foreground)),
                        ),
                )
                .child(
                    Switch::new(switch_id)
                        .checked(if all_disabled { false } else { enabled })
                        .disabled(all_disabled)
                        .on_change(cx.listener(move |this, checked: &bool, _, cx| {
                            if let Overlay::Settings(modal) = &mut this.overlay {
                                if *checked {
                                    modal.notif_disabled_sounds.remove(sound_id);
                                } else {
                                    modal.notif_disabled_sounds.insert(sound_id);
                                }
                                // TODO(backlog): persist per-sound enabled state
                                cx.notify();
                            }
                        })),
                ),
        )
        .into_any_element()
}

pub(super) fn render_notifications_tab(view: &ClientView, theme: &Theme, cx: &mut Context<ClientView>) -> AnyElement {
    let (
        push_timeout_mins,
        push_menu_open,
        mention_pref,
        master_volume,
        all_sounds_expanded,
        per_sound_expanded,
        disabled_sounds,
        tts_enabled,
        tts_mode,
    ) = match &view.overlay {
        Overlay::Settings(modal) => (
            modal.notif_push_timeout_mins,
            modal.notif_push_menu_open,
            modal.notif_mention_preference,
            modal.notif_master_volume,
            modal.notif_all_sounds_expanded,
            modal.notif_per_sound_expanded,
            &modal.notif_disabled_sounds,
            modal.notif_tts_enabled,
            modal.notif_tts_mode,
        ),
        _ => return div().into_any_element(),
    };

    let hover_bg = theme.hover_bg;

    // Section 0: General
    let timeout_label = if push_timeout_mins == 1 {
        l10n::tr("1 minute")
    } else {
        format!("{push_timeout_mins} {}", l10n::tr("minutes"))
    };

    let push_settings_row = div()
        .relative()
        .w_full()
        .min_h(px(44.))
        .py(px(4.))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(16.))
        .flx("user.notifications-tab.push-settings.container")
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .items_start()
                .gap(px(4.))
                .flx("user.notifications-tab.push-settings.text")
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.foreground)
                        .flx("user.notifications-tab.push-settings.title")
                        .child(l10n::tr("Push notification inactive timeout")),
                )
                .child(
                    div()
                        .pr(px(16.))
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .text_color(theme.text_secondary)
                        .flx("user.notifications-tab.push-settings.description")
                        .child(l10n::tr(
                            "Fluxer avoids sending push notifications to your mobile devices when you are at your computer. Choose how long you need to be inactive on desktop before you receive push notifications.",
                        )),
                ),
        )
        .child(
            div()
                .w(px(128.))
                .h(px(34.))
                .flex_none()
                .relative()
                .flx("user.notifications-tab.push-settings.select-wrap")
                .child(
                    div()
                        .id("settings-push-timeout-select")
                        .role(gpui_kit::Role::Button)
                        .w_full()
                        .h(px(34.))
                        .px(px(10.))
                        .rounded(px(6.))
                        .border_1()
                        .border_color(settings_border(theme))
                        .bg(theme.background_secondary)
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(4.))
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover_bg))
                        .flx("user.notifications-tab.push-settings.form-select")
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Overlay::Settings(modal) = &mut this.overlay {
                                modal.notif_push_menu_open = !modal.notif_push_menu_open;
                                cx.notify();
                            }
                        }))
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(20.))
                                .text_color(theme.foreground)
                                .truncate()
                                .child(timeout_label),
                        )
                        .child(
                            Icon::new(AppIcon::CaretDown)
                                .size(px(14.))
                                .text_color(theme.text_secondary),
                        ),
                )
                .when(push_menu_open, |d| {
                    d.child(
                        div()
                            .absolute()
                            .top(px(38.))
                            .right_0()
                            .w(px(128.))
                            .rounded(px(6.))
                            .border_1()
                            .border_color(settings_border(theme))
                            .bg(theme.background_secondary)
                            .py(px(4.))
                            .flex()
                            .flex_col()
                            .children((1u8..=10u8).map(|mins| {
                                let label = if mins == 1 {
                                    l10n::tr("1 minute")
                                } else {
                                    format!("{mins} {}", l10n::tr("minutes"))
                                };
                                let selected = mins == push_timeout_mins;
                                let opt_id: SharedString = format!("settings-push-timeout-{mins}").into();
                                div()
                                    .id(opt_id)
                                    .px(px(10.))
                                    .py(px(6.))
                                    .cursor_pointer()
                                    .text_size(px(13.))
                                    .text_color(if selected { theme.foreground } else { theme.text_secondary })
                                    .when(selected, |row| row.bg(theme.background_modifier_accent))
                                    .hover(move |s| s.bg(hover_bg))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Overlay::Settings(modal) = &mut this.overlay {
                                            modal.notif_push_timeout_mins = mins;
                                            modal.notif_push_menu_open = false;
                                            // TODO(backlog): sync afkTimeout via UserSettingsCommands.update
                                            cx.notify();
                                        }
                                    }))
                                    .child(label)
                            })),
                    )
                }),
        );

    let general_content = div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(16.))
        .flx("user.notifications-tab.notifications.container")
        .child(
            div()
                .w_full()
                .min_h(px(132.))
                .flex()
                .flex_col()
                .justify_between()
                .gap(px(16.))
                .flx("user.notifications-tab.notifications.switches-container")
                .child(notif_switch_row(
                    "settings-notifications",
                    l10n::tr("Enable desktop notifications"),
                    Some(l10n::tr(
                        "Uses the OS notification center. For per-channel/per-community controls, right-click a community icon and open notification settings.",
                    )),
                    view.notifier.enabled,
                    false,
                    theme,
                    |this, checked, window, cx| {
                        this.apply_settings_effect(SettingsEffect::NotificationsChanged(*checked), window, cx);
                    },
                    cx,
                ))
                .child(notif_switch_row(
                    "settings-unread-badge",
                    l10n::tr("Enable unread message badge"),
                    None,
                    view.prefs.data.unread_badge_enabled,
                    false,
                    theme,
                    |this, checked, window, cx| {
                        this.apply_settings_effect(SettingsEffect::UnreadBadgeEnabledChanged(*checked), window, cx);
                    },
                    cx,
                )),
        )
        .child(push_settings_row)
        .into_any_element();

    let general_section = notif_section(l10n::tr("General"), true, general_content, theme);

    // Section 1: Mention preference
    let mention_content = div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(notif_radio_option(
            "settings-mention-no-pref",
            "No preference",
            "Respect the sender's intent, with no warning when they toggle the @mention",
            mention_pref == 0,
            theme,
            |this, _, _, cx| {
                if let Overlay::Settings(modal) = &mut this.overlay {
                    modal.notif_mention_preference = 0;
                    // TODO(backlog): sync mention_flags via UserCommands.update
                    cx.notify();
                }
            },
            cx,
        ))
        .child(notif_radio_option(
            "settings-mention-prefer",
            "Prefer @mention",
            "Default replies to @mention you, and warn the sender if they disable it",
            mention_pref == 1,
            theme,
            |this, _, _, cx| {
                if let Overlay::Settings(modal) = &mut this.overlay {
                    modal.notif_mention_preference = 1;
                    // TODO(backlog): sync mention_flags via UserCommands.update
                    cx.notify();
                }
            },
            cx,
        ))
        .child(notif_radio_option(
            "settings-mention-prefer-no",
            "Prefer no @mention",
            "Default replies to omit the @mention, and warn the sender if they enable it",
            mention_pref == 2,
            theme,
            |this, _, _, cx| {
                if let Overlay::Settings(modal) = &mut this.overlay {
                    modal.notif_mention_preference = 2;
                    // TODO(backlog): sync mention_flags via UserCommands.update
                    cx.notify();
                }
            },
            cx,
        ))
        .into_any_element();

    let mention_section = notif_section(l10n::tr("Mention preference"), false, mention_content, theme);

    // Section 2: Sounds
    let all_disabled = !view.notifier.sounds_enabled;
    let master_at_default = master_volume == 100;
    let fill_pct = (f32::from(master_volume) / 200.0).clamp(0.0, 1.0);

    let master_row = div()
        .w_full()
        .min_h(px(138.))
        .p(px(12.))
        .rounded(px(8.))
        .border_1()
        .border_color(settings_border(theme))
        .bg(theme.background_secondary)
        .flex()
        .flex_col()
        .gap(px(8.))
        .flx("user.notifications-tab.sounds.master-row")
        .child(
            div()
                .w_full()
                .h(px(28.))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .flx("user.notifications-tab.sounds.master-header")
                .child(
                    div()
                        .text_size(px(15.))
                        .line_height(px(22.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.foreground)
                        .flx("user.notifications-tab.sounds.master-label")
                        .child(l10n::tr("Master volume")),
                )
                .child(
                    div()
                        .id("settings-master-volume-reset")
                        .role(gpui_kit::Role::Button)
                        .size(px(28.))
                        .rounded(px(4.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(!all_disabled && !master_at_default, |d| {
                            d.cursor_pointer().hover(move |s| s.bg(hover_bg))
                        })
                        .when(all_disabled || master_at_default, |d| d.opacity(0.4))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !all_disabled && !master_at_default {
                                if let Overlay::Settings(modal) = &mut this.overlay {
                                    modal.notif_master_volume = 100;
                                    // TODO(backlog): sync master volume to SoundSettings
                                    cx.notify();
                                }
                            }
                        }))
                        .child(
                            Icon::new(AppIcon::ArrowCounterClockwiseBold)
                                .size(px(16.))
                                .text_color(theme.text_secondary),
                        ),
                ),
        )
        .child(
            div()
                .text_size(px(13.))
                .line_height(px(18.))
                .text_color(theme.text_secondary)
                .flx("user.notifications-tab.sounds.master-description")
                .child(l10n::tr("Sets the level for every sound effect. Per-sound overrides ignore this.")),
        )
        .child(
            div()
                .w_full()
                .h(px(50.))
                .flex()
                .flex_col()
                .justify_between()
                .when(all_disabled, |d| d.opacity(0.5))
                .child(
                    div()
                        .w_full()
                        .h(px(20.))
                        .relative()
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .w_full()
                                .h(px(6.))
                                .rounded_full()
                                .bg(theme.background_modifier_accent)
                                .overflow_hidden()
                                .child(div().w(gpui_kit::relative(fill_pct)).h_full().bg(theme.brand)),
                        )
                        .child(
                            div()
                                .absolute()
                                .left(gpui_kit::relative(fill_pct))
                                .ml(px(-7.))
                                .size(px(14.))
                                .rounded_full()
                                .bg(theme.foreground)
                                .border_2()
                                .border_color(theme.brand),
                        ),
                )
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .children([0u16, 50, 100, 150, 200].into_iter().map(|pct| {
                            let marker_id: SharedString = format!("settings-master-vol-{pct}").into();
                            div()
                                .id(marker_id)
                                .cursor_pointer()
                                .text_size(px(11.))
                                .line_height(px(14.))
                                .text_color(if master_volume == pct {
                                    theme.foreground
                                } else {
                                    theme.text_tertiary
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if !all_disabled {
                                        if let Overlay::Settings(modal) = &mut this.overlay {
                                            modal.notif_master_volume = pct;
                                            // TODO(backlog): sync master volume to SoundSettings
                                            cx.notify();
                                        }
                                    }
                                }))
                                .child(format!("{pct}%"))
                        })),
                ),
        );

    let sound_rows: Vec<AnyElement> = {
        let mut entries: Vec<&SoundEntry> = COMMON_SOUNDS.iter().collect();
        if all_sounds_expanded {
            entries.extend(EXTRA_SOUNDS.iter());
        }
        let total = entries.len();
        entries
            .into_iter()
            .enumerate()
            .map(|(i, entry)| {
                let enabled = !disabled_sounds.contains(entry.id);
                render_sound_row(entry, all_disabled, enabled, i + 1 == total, theme, cx)
            })
            .collect()
    };

    let sound_list_frame = div()
        .relative()
        .w_full()
        .when(!all_sounds_expanded, |d| d.pb(px(4.)))
        .rounded(px(8.))
        .border_1()
        .border_color(settings_border(theme))
        .bg(theme.background_secondary)
        .overflow_hidden()
        .flex()
        .flex_col()
        .flx("user.notifications-tab.sounds.sound-list-frame")
        .children(sound_rows)
        .when(!all_sounds_expanded, |d| {
            d.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(64.))
                    .bg(theme.background_secondary.opacity(0.65))
                    .flx("user.notifications-tab.sounds.fade"),
            )
        });

    let reveal_row = div()
        .w_full()
        .h(px(32.))
        .mt(px(-6.))
        .flex()
        .flex_row()
        .items_center()
        .justify_center()
        .flx("user.notifications-tab.sounds.reveal-row")
        .child(
            div()
                .id("settings-toggle-all-sounds-list")
                .role(gpui_kit::Role::Button)
                .min_h(px(32.))
                .px(px(12.))
                .py(px(5.))
                .rounded_full()
                .border_1()
                .border_color(settings_border(theme))
                .bg(theme.background_secondary_lighter)
                .cursor_pointer()
                .hover(move |s| s.bg(hover_bg))
                .flex()
                .flex_row()
                .items_center()
                .justify_center()
                .gap(px(6.))
                .flx("user.notifications-tab.sounds.reveal-button.toggle-all-sounds.button")
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Overlay::Settings(modal) = &mut this.overlay {
                        modal.notif_all_sounds_expanded = !modal.notif_all_sounds_expanded;
                        cx.notify();
                    }
                }))
                .child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(16.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.foreground)
                        .flx("user.notifications-tab.sounds.reveal-label")
                        .child(if all_sounds_expanded {
                            l10n::tr("Show fewer sound effects")
                        } else {
                            l10n::tr("Show more sound effects")
                        }),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .flx("user.notifications-tab.sounds.reveal-chevron")
                        .child(
                            div()
                                .size(px(16.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .flx("user.notifications-tab.sounds.caret-down-icon")
                                .child(Icon::new(AppIcon::CaretDown).size(px(16.)).text_color(theme.text_secondary)),
                        ),
                ),
        );

    let per_sound_accordion = div()
        .w_full()
        .min_h(px(93.))
        .rounded(px(8.))
        .border_1()
        .border_color(settings_border(theme))
        .bg(theme.background_secondary)
        .flex()
        .flex_col()
        .justify_center()
        .child(
            div()
                .id("settings-per-sound-accordion")
                .role(gpui_kit::Role::Button)
                .w_full()
                .px(px(16.))
                .py(px(14.))
                .cursor_pointer()
                .hover(move |s| s.bg(hover_bg))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Overlay::Settings(modal) = &mut this.overlay {
                        modal.notif_per_sound_expanded = !modal.notif_per_sound_expanded;
                        cx.notify();
                    }
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(20.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.foreground)
                                .child(l10n::tr("Per-sound volume")),
                        )
                        .child(
                            div()
                                .text_size(px(13.))
                                .line_height(px(18.))
                                .text_color(theme.text_secondary)
                                .child(l10n::tr(
                                    "Set custom volumes for individual sounds. Sounds without an override follow the master volume.",
                                )),
                        ),
                )
                .child(
                    Icon::new(if per_sound_expanded {
                        AppIcon::CaretDown
                    } else {
                        AppIcon::CaretRight
                    })
                    .size(px(16.))
                    .text_color(theme.text_secondary),
                ),
        )
        .when(per_sound_expanded, |d| {
            d.child(
                div()
                    .px(px(16.))
                    .pb(px(14.))
                    .pt(px(8.))
                    .border_t_1()
                    .border_color(settings_border(theme))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .children(COMMON_SOUNDS.iter().map(|entry| {
                        div()
                            .w_full()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(theme.foreground)
                                    .child(l10n::tr(entry.label)),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(theme.text_secondary)
                                    .child(format!("Following master • {master_volume}%")),
                            )
                    })),
            )
        });

    let sounds_content = div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(14.))
        .flx("user.notifications-tab.sounds.container")
        .child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(14.))
                .flx("user.notifications-tab.sounds.content")
                .child(master_row)
                .child(
                    div().w_full().min_h(px(52.)).flex().items_center().child(notif_switch_row(
                        "settings-sounds",
                        l10n::tr("Disable all notification sounds"),
                        Some(l10n::tr("Your existing notification sound settings will be preserved.")),
                        all_disabled,
                        false,
                        theme,
                        |this, checked, window, cx| {
                            this.apply_settings_effect(SettingsEffect::SoundsChanged(!*checked), window, cx);
                        },
                        cx,
                    )),
                )
                .child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(19.2))
                        .text_color(theme.text_secondary)
                        .flx("user.notifications-tab.sounds.hint")
                        .child(l10n::tr(
                            "Click the upload icon next to any sound to customize it with your own audio file. Max 2 MB.",
                        )),
                )
                .child(sound_list_frame)
                .child(reveal_row)
                .child(per_sound_accordion),
        )
        .into_any_element();

    let sounds_section = notif_section(l10n::tr("Sounds"), false, sounds_content, theme);

    // Section 3: Text-to-speech notifications
    let tts_content = div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(20.))
        .flx("user.notifications-tab.text-to-speech.container")
        .child(
            div().w_full().min_h(px(72.)).flex().items_center().child(notif_switch_row(
                "settings-tts-playback",
                l10n::tr("Enable /tts speech playback"),
                Some(l10n::tr(
                    "Let /tts read your message aloud. Disabling the setting keeps those commands as regular text.",
                )),
                tts_enabled,
                false,
                theme,
                |this, checked, _, cx| {
                    if let Overlay::Settings(modal) = &mut this.overlay {
                        modal.notif_tts_enabled = *checked;
                        // TODO(backlog): sync enableTTSCommand via AccessibilityCommands.update
                        cx.notify();
                    }
                },
                cx,
            )),
        )
        .child(
            div()
                .h(px(50.))
                .px(px(16.))
                .py(px(12.))
                .rounded(px(6.))
                .border_1()
                .border_color(settings_border(theme))
                .bg(theme.background_modifier_accent)
                .self_start()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.))
                .flx("user.notifications-tab.text-to-speech.helper-callout")
                .child(
                    div()
                        .size(px(16.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .flx("user.notifications-tab.text-to-speech.helper-icon")
                        .child(Icon::new(AppIcon::WarningCircle).size(px(16.)).text_color(theme.text_tertiary)),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .text_size(px(14.))
                        .line_height(px(24.))
                        .text_color(theme.text_secondary)
                        .flx("user.notifications-tab.text-to-speech.helper-text")
                        .child(l10n::tr("Adjust playback speed in "))
                        .child(
                            div()
                                .id("settings-tts-accessibility-link")
                                .role(gpui_kit::Role::Button)
                                .cursor_pointer()
                                .text_size(px(14.))
                                .line_height(px(24.))
                                .text_color(theme.foreground)
                                .underline()
                                .flx("user.notifications-tab.text-to-speech.link-button.accessibility-link-click")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Overlay::Settings(modal) = &mut this.overlay {
                                        modal.active_tab = SettingsTab::Accessibility;
                                        modal.active_section = Some("tts");
                                        cx.notify();
                                    }
                                }))
                                .child(l10n::tr("Accessibility")),
                        )
                        .child("."),
                ),
        )
        .child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(12.))
                .flx("user.notifications-tab.text-to-speech.narration-section")
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .flx("user.notifications-tab.text-to-speech.narration-header")
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(20.8))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.foreground)
                                .flx("user.notifications-tab.text-to-speech.narration-title")
                                .child(l10n::tr("Automatic message narration")),
                        )
                        .child(
                            div()
                                .text_size(px(13.))
                                .line_height(px(18.2))
                                .text_color(theme.text_tertiary)
                                .flx("user.notifications-tab.text-to-speech.narration-description")
                                .child(l10n::tr(
                                    "Converts incoming content to speech, regardless of whether it came from /tts.",
                                )),
                        ),
                )
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .child(notif_radio_option(
                            "settings-tts-every",
                            "Every channel",
                            "Let every incoming message be spoken, regardless of which channel is open.",
                            tts_mode == 0,
                            theme,
                            |this, _, _, cx| {
                                if let Overlay::Settings(modal) = &mut this.overlay {
                                    modal.notif_tts_mode = 0;
                                    // TODO(backlog): sync TTSNotificationMode
                                    cx.notify();
                                }
                            },
                            cx,
                        ))
                        .child(notif_radio_option(
                            "settings-tts-active",
                            "Active channel only",
                            "Narrates only the channel you're viewing. Narration follows you between channels.",
                            tts_mode == 1,
                            theme,
                            |this, _, _, cx| {
                                if let Overlay::Settings(modal) = &mut this.overlay {
                                    modal.notif_tts_mode = 1;
                                    // TODO(backlog): sync TTSNotificationMode
                                    cx.notify();
                                }
                            },
                            cx,
                        ))
                        .child(notif_radio_option(
                            "settings-tts-never",
                            "Never automatically",
                            "Remain silent unless someone runs /tts in the channel you're viewing.",
                            tts_mode == 2,
                            theme,
                            |this, _, _, cx| {
                                if let Overlay::Settings(modal) = &mut this.overlay {
                                    modal.notif_tts_mode = 2;
                                    // TODO(backlog): sync TTSNotificationMode
                                    cx.notify();
                                }
                            },
                            cx,
                        )),
                ),
        )
        .into_any_element();

    let tts_section = notif_section(l10n::tr("Text-to-speech notifications"), false, tts_content, theme);

    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(20.))
        .child(general_section)
        .child(mention_section)
        .child(sounds_section)
        .child(tts_section)
        .into_any_element()
}
