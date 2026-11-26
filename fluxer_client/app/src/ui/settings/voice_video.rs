use super::*;

fn voice_section(title: String, is_first: bool, content: AnyElement, theme: &Theme) -> AnyElement {
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

fn voice_subsection(
    title: String,
    right_action: Option<AnyElement>,
    content: AnyElement,
    theme: &Theme,
) -> AnyElement {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(12.))
        .flx("app.settings-tab-layout.settings-tab-section.subsection")
        .child(
            div()
                .w_full()
                .h(px(22.))
                .flex()
                .flex_col()
                .gap(px(4.))
                .flx("app.settings-tab-layout.settings-tab-section.div")
                .child(
                    div()
                        .w_full()
                        .h(px(22.))
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .flx("app.settings-tab-layout.settings-tab-section.title-row")
                        .child(
                            div()
                                .text_size(px(15.))
                                .line_height(px(19.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.foreground)
                                .flx("app.settings-tab-layout.settings-tab-section.h4")
                                .child(title),
                        )
                        .when_some(right_action, |d, action| d.child(action)),
                ),
        )
        .child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(8.))
                .flx("app.settings-tab-layout.settings-tab-section.div--2")
                .child(content),
        )
        .into_any_element()
}

pub(super) struct VoiceDeviceFlx {
    row: &'static str,
    text: &'static str,
    title_row: &'static str,
    label: &'static str,
    select_wrap: &'static str,
    select: &'static str,
}

const INPUT_DEVICE_FLX: VoiceDeviceFlx = VoiceDeviceFlx {
    row: "user.voice-tab.select.input-device-change.row",
    text: "user.voice-tab.select.input-device-change.text",
    title_row: "user.voice-tab.select.input-device-change.title-row",
    label: "user.voice-tab.select.input-device-change.label",
    select_wrap: "user.voice-tab.select.input-device-change.select-wrap",
    select: "user.voice-tab.select.input-device-change",
};

const OUTPUT_DEVICE_FLX: VoiceDeviceFlx = VoiceDeviceFlx {
    row: "user.voice-tab.select.update.row",
    text: "user.voice-tab.select.update.text",
    title_row: "user.voice-tab.select.update.title-row",
    label: "user.voice-tab.select.update.label",
    select_wrap: "user.voice-tab.select.update.select-wrap",
    select: "user.voice-tab.select.update",
};

pub(super) fn voice_device_row(
    label: &str,
    device: Option<&str>,
    available_devices: &[String],
    id_prefix: &'static str,
    reset_id: &'static str,
    flx: &VoiceDeviceFlx,
    theme: &Theme,
    make_select: impl Fn(Option<String>) -> SettingsEffect + Copy + 'static,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let label_text = l10n::tr(label);
    let default_label = l10n::tr("System default");
    let display_name = device.map(str::to_owned).unwrap_or_else(|| default_label.clone());

    let default_selected = device.is_none();
    let default_sel_id = format!("{id_prefix}-default");
    let mut default_btn = Button::new(default_sel_id.clone())
        .small()
        .label(default_label.clone())
        .accessibility_label(format!("{label_text}: {default_label}"));
    default_btn = if default_selected {
        default_btn.primary()
    } else {
        default_btn.ghost()
    };
    let default_button = div().debug_selector(move || default_sel_id.clone()).child(
        default_btn.on_click(cx.listener(move |this, _, window, cx| {
            this.apply_settings_effect(make_select(None), window, cx);
        })),
    );

    let device_buttons = available_devices.iter().enumerate().map(|(ix, name)| {
        let selected = device == Some(name.as_str());
        let sel_id = format!("{id_prefix}-{ix}");
        let device_name = name.clone();
        let mut btn = Button::new((id_prefix, ix + 1))
            .small()
            .label(name.clone())
            .accessibility_label(format!("{label_text}: {name}"));
        btn = if selected { btn.primary() } else { btn.ghost() };
        div().debug_selector(move || sel_id.clone()).child(btn.on_click(cx.listener(
            move |this, _, window, cx| {
                this.apply_settings_effect(make_select(Some(device_name.clone())), window, cx);
            },
        )))
    });

    div()
        .w_full()
        .h(px(44.))
        .py(px(4.))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(16.))
        .flx(flx.row)
        .child(
            div()
                .w(px(180.))
                .flex_none()
                .flex()
                .flex_col()
                .items_start()
                .gap(px(4.))
                .flx(flx.text)
                .child(
                    div()
                        .h(px(20.))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .flx(flx.title_row)
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(20.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.foreground)
                                .flx(flx.label)
                                .child(label_text.clone()),
                        ),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h(px(34.))
                .flex()
                .flex_col()
                .flx(flx.select_wrap)
                .child(
                    div()
                        .w_full()
                        .h(px(34.))
                        .overflow_hidden()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(4.))
                        .flx(flx.select)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(4.))
                                .child(default_button)
                                .children(device_buttons),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(8.))
                                .child(div().text_xs().text_color(theme.muted_foreground).child(display_name))
                                .when(device.is_some(), |d| {
                                    d.child(
                                        div().debug_selector(move || reset_id.to_owned()).child(
                                            Button::new(reset_id)
                                                .ghost()
                                                .small()
                                                .label(l10n::tr("Reset"))
                                                .on_click(cx.listener(move |this, _, window, cx| {
                                                    this.apply_settings_effect(make_select(None), window, cx);
                                                })),
                                        ),
                                    )
                                }),
                        ),
                ),
        )
        .into_any_element()
}

pub(super) fn voice_volume_row(
    label: &str,
    volume: u16,
    dec_id: &'static str,
    inc_id: &'static str,
    reset_id: &'static str,
    div_flx: &'static str,
    row_flx: &'static str,
    label_flx: &'static str,
    theme: &Theme,
    make_effect: impl Fn(u16) -> SettingsEffect + Copy + 'static,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let clamped = volume.clamp(MIN_VOICE_VOLUME, MAX_VOICE_VOLUME);
    div()
        .w_full()
        .min_h(px(86.))
        .flex()
        .flex_col()
        .flx(div_flx)
        .child(
            div()
                .w_full()
                .h(px(24.))
                .mb(px(12.))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .flx(row_flx)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(16.))
                        .line_height(px(20.8))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.foreground)
                        .flx(label_flx)
                        .child(l10n::tr(label)),
                )
                .child(
                    div().debug_selector(move || reset_id.to_owned()).child(
                        Button::new(reset_id)
                            .ghost()
                            .small()
                            .disabled(clamped == DEFAULT_VOICE_VOLUME)
                            .label(l10n::tr("Reset"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.apply_settings_effect(make_effect(DEFAULT_VOICE_VOLUME), window, cx);
                            })),
                    ),
                ),
        )
        .child(
            div()
                .w_full()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(4.))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(4.))
                        .child(
                            div().debug_selector(move || dec_id.to_owned()).child(
                                Button::new(dec_id)
                                    .ghost()
                                    .small()
                                    .label("-")
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        let next = clamped.saturating_sub(10).max(MIN_VOICE_VOLUME);
                                        this.apply_settings_effect(make_effect(next), window, cx);
                                    })),
                            ),
                        )
                        .child(div().px_2().text_xs().font_weight(FontWeight::SEMIBOLD).child(format!("{clamped}%")))
                        .child(
                            div().debug_selector(move || inc_id.to_owned()).child(
                                Button::new(inc_id)
                                    .ghost()
                                    .small()
                                    .label("+")
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        let next = clamped.saturating_add(10).min(MAX_VOICE_VOLUME);
                                        this.apply_settings_effect(make_effect(next), window, cx);
                                    })),
                            ),
                        ),
                ),
        )
        .into_any_element()
}

pub(super) fn render_voice_tab(
    view: &ClientView,
    active_section: Option<&'static str>,
    theme: &Theme,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let active_in_dev = view
        .prefs
        .data
        .voice_input_device
        .clone()
        .unwrap_or_else(|| l10n::tr("System default"));
    let ptt_enabled = view.prefs.data.voice_push_to_talk;
    let ptt_delay = view.prefs.data.voice_ptt_release_delay_ms;
    let vad_auto = view.prefs.data.voice_vad_auto_threshold;
    let vad_threshold = view.prefs.data.voice_vad_threshold;
    let voice_profile = view.prefs.data.voice_processing_mode.as_str();
    let ns_backend = view.prefs.data.voice_noise_suppression_backend.as_str();
    let echo_cancel = view.prefs.data.voice_echo_cancellation;
    let auto_gain = view.prefs.data.voice_auto_gain_control;
    let mic_testing = view.mic_test_active;
    let cam_res = view.prefs.data.camera_resolution.as_str();
    let mirror_cam = view.prefs.data.mirror_camera;
    let stream_mode = view.prefs.data.streaming_mode.as_str();
    let vid_res = view.prefs.data.video_resolution.as_str();
    let vid_fps = view.prefs.data.video_fps;
    let vid_codec = view.prefs.data.video_codec.as_str();
    let vid_bitrate = view.prefs.data.video_bitrate_kbps;
    let hw_accel = view.prefs.data.video_hardware_acceleration;

    let ptt_delays: &[(u16, &str)] = &[
        (20, "20ms"),
        (100, "100ms"),
        (250, "250ms"),
        (500, "500ms"),
        (1000, "1.0s"),
        (2000, "2.0s"),
    ];
    let profiles: &[(&str, &str)] = &[
        ("voice", "Focused Voice"),
        ("studio", "Direct Input"),
        ("custom", "Custom"),
    ];
    let ns_backends: &[(&str, &str)] = &[
        ("off", "Off"),
        ("standard", "Standard (WebRTC)"),
        ("deepfilternet", "DeepFilterNet"),
    ];
    let camera_resolutions: &[(&str, &str)] = &[("480p", "480p"), ("720p", "720p"), ("1080p", "1080p")];
    let stream_presets: &[(&str, &str)] = &[
        ("screenshare", "Screen Share (1080p · 30 FPS)"),
        ("gaming", "Gaming (1080p · 60 FPS)"),
        ("custom", "Custom"),
    ];
    let video_resolutions: &[(&str, &str)] = &[
        ("480p", "480p"),
        ("720p", "720p"),
        ("1080p", "1080p"),
        ("1440p", "1440p"),
        ("source", "Source"),
    ];
    let video_fps_options: &[(u8, &str)] = &[(15, "15 FPS"), (30, "30 FPS"), (60, "60 FPS")];
    let video_codecs: &[(&str, &str)] = &[
        ("auto", "Auto"),
        ("h265", "H.265 / HEVC"),
        ("h264", "H.264"),
        ("vp9", "VP9"),
        ("vp8", "VP8"),
        ("av1", "AV1"),
    ];
    let bitrate_options: &[(u16, &str)] = &[
        (0, "Automatic"),
        (2000, "2 Mbps"),
        (4000, "4 Mbps"),
        (6000, "6 Mbps"),
        (8000, "8 Mbps"),
        (10000, "10 Mbps"),
        (15000, "15 Mbps"),
    ];

    let meter_fill = if mic_testing {
        (f32::from(view.prefs.data.voice_input_volume.min(200)) / 250.).clamp(0.15, 0.92)
    } else {
        0.04
    };

    let refresh_devices_btn = div().debug_selector(|| "voice-refresh-devices".to_owned()).child(
        Button::new("voice-refresh-devices")
            .ghost()
            .small()
            .label(l10n::tr("Refresh devices"))
            .accessibility_label(l10n::tr("Refresh devices"))
            .on_click(cx.listener(|this, _, _, cx| {
                this.refresh_audio_devices(cx);
            })),
    );

    let devices_subsection = voice_subsection(
        l10n::tr("Input and output"),
        Some(refresh_devices_btn.into_any_element()),
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(voice_device_row(
                "Input device",
                view.prefs.data.voice_input_device.as_deref(),
                &view.audio_devices.input_devices,
                "voice-input-device",
                "voice-input-device-reset",
                &INPUT_DEVICE_FLX,
                theme,
                SettingsEffect::VoiceInputDeviceChanged,
                cx,
            ))
            .child(voice_device_row(
                "Output device",
                view.prefs.data.voice_output_device.as_deref(),
                &view.audio_devices.output_devices,
                "voice-output-device",
                "voice-output-device-reset",
                &OUTPUT_DEVICE_FLX,
                theme,
                SettingsEffect::VoiceOutputDeviceChanged,
                cx,
            ))
            .into_any_element(),
        theme,
    );

    let volume_subsection = voice_subsection(
        l10n::tr("Volume"),
        None,
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(voice_volume_row(
                "Input volume",
                view.prefs.data.voice_input_volume,
                "voice-input-vol-dec",
                "voice-input-vol-inc",
                "voice-input-vol-reset",
                "user.voice-tab.div--3",
                "user.voice-tab.slider-label-row",
                "user.voice-tab.slider-label",
                theme,
                SettingsEffect::VoiceInputVolumeChanged,
                cx,
            ))
            .child(voice_volume_row(
                "Output volume",
                view.prefs.data.voice_output_volume,
                "voice-output-vol-dec",
                "voice-output-vol-inc",
                "voice-output-vol-reset",
                "user.voice-tab.div--4",
                "user.voice-tab.slider-label-row--2",
                "user.voice-tab.slider-label--2",
                theme,
                SettingsEffect::VoiceOutputVolumeChanged,
                cx,
            ))
            .into_any_element(),
        theme,
    );

    let is_custom = voice_profile == "custom";
    let is_compact_voice = voice_profile == "voice" && !ptt_enabled;

    let ptt_row = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_3()
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
                        .child(l10n::tr("Push-to-talk")),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(l10n::tr(
                            "Choose automatic voice activity detection or manual push-to-talk.",
                        )),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .gap_1()
                .child(
                    div().debug_selector(|| "settings-input-mode-vad".to_owned()).child({
                        let vad = !ptt_enabled;
                        let mut btn = Button::new("settings-input-mode-vad")
                            .small()
                            .label(l10n::tr("Voice Activity"));
                        btn = if vad { btn.primary() } else { btn.ghost() };
                        btn.on_click(cx.listener(|this, _, window, cx| {
                            this.apply_settings_effect(
                                SettingsEffect::VoicePushToTalkChanged(false),
                                window,
                                cx,
                            );
                        }))
                    }),
                )
                .child(
                    div().debug_selector(|| "settings-input-mode-ptt".to_owned()).child({
                        let mut btn = Button::new("settings-input-mode-ptt")
                            .small()
                            .label(l10n::tr("Push to Talk"));
                        btn = if ptt_enabled { btn.primary() } else { btn.ghost() };
                        btn.on_click(cx.listener(|this, _, window, cx| {
                            this.apply_settings_effect(
                                SettingsEffect::VoicePushToTalkChanged(true),
                                window,
                                cx,
                            );
                        }))
                    }),
                ),
        );

    let agc_row = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_3()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(l10n::tr("Automatic gain control")),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(l10n::tr(
                            "Evens out your mic volume so you are not too quiet.",
                        )),
                ),
        )
        .child(
            div().debug_selector(|| "settings-auto-gain-control".to_owned()).child(
                Switch::new("settings-auto-gain-control")
                    .checked(auto_gain)
                    .on_change(cx.listener(|this, checked: &bool, window, cx| {
                        this.apply_settings_effect(
                            SettingsEffect::VoiceAutoGainControlChanged(*checked),
                            window,
                            cx,
                        );
                    })),
            ),
        );

    let input_mode_controls = div()
        .debug_selector(|| "settings-voice-input-mode".to_owned())
        .w_full()
        .when(is_compact_voice, |d| d.h(px(125.)))
        .when(!is_compact_voice, |d| d.min_h(px(125.)))
        .mt(px(12.))
        .pt(px(12.))
        .border_t_1()
        .border_color(settings_border(theme))
        .flx("user.voice-tab.profile-sub-section")
        .flex()
        .flex_col()
        .gap(px(if is_compact_voice { 16. } else { 10. }))
        .child(ptt_row)
        .when(ptt_enabled || is_custom, |d| {
            d.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(l10n::tr("Push-to-talk release delay")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(l10n::tr(
                                        "Keep transmitting briefly after releasing the push-to-talk key.",
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap_1()
                            .children(ptt_delays.iter().map(|&(ms, label)| {
                                let selected = ptt_delay == ms;
                                let sel_id = format!("settings-ptt-delay-{ms}");
                                let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                    .small()
                                    .label(label);
                                btn = if selected { btn.primary() } else { btn.ghost() };
                                div().debug_selector(move || sel_id.clone()).child(btn.on_click(
                                    cx.listener(move |this, _, window, cx| {
                                        this.apply_settings_effect(
                                            SettingsEffect::VoicePttReleaseDelayChanged(ms),
                                            window,
                                            cx,
                                        );
                                    }),
                                ))
                            })),
                    ),
            )
        })
        .when(is_custom, |d| {
            d.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(l10n::tr("Auto-set activity threshold")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(l10n::tr(
                                        "Automatically calibrate microphone voice activation sensitivity.",
                                    )),
                            ),
                    )
                    .child(
                        div().debug_selector(|| "settings-vad-auto-threshold".to_owned()).child(
                            Switch::new("settings-vad-auto-threshold")
                                .checked(vad_auto)
                                .on_change(cx.listener(|this, checked: &bool, window, cx| {
                                    this.apply_settings_effect(
                                        SettingsEffect::VoiceVadAutoThresholdChanged(*checked),
                                        window,
                                        cx,
                                    );
                                })),
                        ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(div().text_sm().child(l10n::tr("Activity threshold")))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .child(
                                div().debug_selector(|| "settings-vad-threshold-dec".to_owned()).child(
                                    Button::new("settings-vad-threshold-dec")
                                        .ghost()
                                        .small()
                                        .disabled(vad_auto)
                                        .label("-")
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.apply_settings_effect(
                                                SettingsEffect::VoiceVadThresholdChanged(
                                                    vad_threshold.saturating_sub(5),
                                                ),
                                                window,
                                                cx,
                                            );
                                        })),
                                ),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(format!("{vad_threshold}%")),
                            )
                            .child(
                                div().debug_selector(|| "settings-vad-threshold-inc".to_owned()).child(
                                    Button::new("settings-vad-threshold-inc")
                                        .ghost()
                                        .small()
                                        .disabled(vad_auto)
                                        .label("+")
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.apply_settings_effect(
                                                SettingsEffect::VoiceVadThresholdChanged(
                                                    (vad_threshold + 5).min(100),
                                                ),
                                                window,
                                                cx,
                                            );
                                        })),
                                ),
                            )
                            .child(
                                div().debug_selector(|| "settings-vad-threshold-reset".to_owned()).child(
                                    Button::new("settings-vad-threshold-reset")
                                        .ghost()
                                        .small()
                                        .disabled(vad_auto)
                                        .label(l10n::tr("Reset"))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.apply_settings_effect(
                                                SettingsEffect::VoiceVadThresholdChanged(50),
                                                window,
                                                cx,
                                            );
                                        })),
                                ),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_3()
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
                                    .child(l10n::tr("Noise suppression")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(l10n::tr(
                                        "Remove background keyboard, fan, and room noise from your microphone.",
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .children(ns_backends.iter().map(|&(val, label)| {
                                let selected = ns_backend == val;
                                let sel_id = format!("settings-ns-backend-{val}");
                                let val_owned = val.to_owned();
                                let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                    .small()
                                    .label(l10n::tr(label));
                                btn = if selected { btn.primary() } else { btn.ghost() };
                                div().debug_selector(move || sel_id.clone()).child(btn.on_click(
                                    cx.listener(move |this, _, window, cx| {
                                        this.apply_settings_effect(
                                            SettingsEffect::VoiceNoiseSuppressionBackendChanged(val_owned.clone()),
                                            window,
                                            cx,
                                        );
                                    }),
                                ))
                            }))
                            .child(
                                div().debug_selector(|| "settings-noise-suppression".to_owned()).child(
                                    Switch::new("settings-noise-suppression")
                                        .checked(view.prefs.data.voice_noise_suppression)
                                        .on_change(cx.listener(|this, checked: &bool, window, cx| {
                                            this.apply_settings_effect(
                                                SettingsEffect::VoiceNoiseSuppressionChanged(*checked),
                                                window,
                                                cx,
                                            );
                                        })),
                                ),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(l10n::tr("Echo cancellation")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(l10n::tr(
                                        "Prevent speaker output from feeding back into your microphone.",
                                    )),
                            ),
                    )
                    .child(
                        div().debug_selector(|| "settings-echo-cancellation".to_owned()).child(
                            Switch::new("settings-echo-cancellation")
                                .checked(echo_cancel)
                                .on_change(cx.listener(|this, checked: &bool, window, cx| {
                                    this.apply_settings_effect(
                                        SettingsEffect::VoiceEchoCancellationChanged(*checked),
                                        window,
                                        cx,
                                    );
                                })),
                        ),
                    ),
            )
        })
        .child(agc_row)
        .into_any_element();

    let audio_processing_subsection = voice_subsection(
        l10n::tr("Voice processing"),
        None,
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(16.))
            .flx("user.voice-tab.audio-processing")
            .child(
                div()
                    .debug_selector(|| "settings-voice-processing".to_owned())
                    .w_full()
                    .min_h(px(340.8))
                    .p(px(16.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(settings_border(theme))
                    .flx("user.voice-tab.audio-processing-card")
                    .bg(theme.background_secondary)
                    .flex()
                    .flex_col()
                        .child(
                            div()
                                .w_full()
                                .flex()
                                .flex_col()
                                .gap(px(8.))
                                .children(profiles.iter().map(|&(val, label)| {
                                    let selected = voice_profile == val;
                                    let sel_id = format!("settings-voice-profile-{val}");
                                    let val_owned = val.to_owned();
                                    let desc = match val {
                                        "voice" => "Recommended. Cleans up your mic for clear speech.",
                                        "studio" => "Sends your audio untouched. Best if you're using external audio software.",
                                        _ => "Adjust noise suppression, echo cancellation, and gain yourself.",
                                    };
                                    let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                        .small()
                                        .label(l10n::tr(label));
                                    btn = if selected { btn.primary() } else { btn.ghost() };
                                    div()
                                        .w_full()
                                        .h(px(51.))
                                        .px(px(10.))
                                        .rounded(px(6.))
                                        .border_1()
                                        .border_color(if selected { theme.accent } else { settings_border(theme) })
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .justify_between()
                                        .gap(px(8.))
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
                                                        .child(l10n::tr(label)),
                                                )
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(theme.muted_foreground)
                                                        .child(l10n::tr(desc)),
                                                ),
                                        )
                                        .child(
                                            div().debug_selector(move || sel_id.clone()).child(btn.on_click(
                                                cx.listener(move |this, _, window, cx| {
                                                    this.apply_settings_effect(
                                                        SettingsEffect::VoiceProcessingModeChanged(val_owned.clone()),
                                                        window,
                                                        cx,
                                                    );
                                                }),
                                            )),
                                        )
                                })),
                        )
                        .when(voice_profile != "studio", |d| d.child(input_mode_controls)),
                )
                .into_any_element(),
        theme,
    );

    let mic_test_subsection = voice_subsection(
        l10n::tr("Mic test"),
        None,
        div()
            .debug_selector(|| "settings-voice-mic-test".to_owned())
            .w_full()
            .h(px(66.))
            .p(px(10.))
            .rounded(px(8.))
            .border_1()
            .border_color(settings_border(theme))
            .bg(theme.background_secondary)
            .flex()
            .flex_col()
            .justify_between()
            .gap(px(6.))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_0p5()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(l10n::tr("Input monitor")),
                                        )
                                        .child(
                                            div().text_xs().text_color(theme.muted_foreground).child(format!(
                                                "{}: {active_in_dev} ({}%)",
                                                l10n::tr("Active input"),
                                                view.prefs.data.voice_input_volume
                                            )),
                                        ),
                                )
                                .child(
                                    div().debug_selector(|| "settings-mic-test-btn".to_owned()).child({
                                        let mut btn = Button::new("settings-mic-test-btn").small().label(if mic_testing {
                                            l10n::tr("Stop Mic Test")
                                        } else {
                                            l10n::tr("Test Microphone")
                                        });
                                        btn = if mic_testing { btn.danger() } else { btn.primary() };
                                        btn.on_click(cx.listener(|this, _, window, cx| {
                                            this.apply_settings_effect(SettingsEffect::ToggleMicTest, window, cx);
                                        }))
                                    }),
                                ),
                        )
                        .child(
                            div()
                                .debug_selector(|| "settings-mic-test-meter".to_owned())
                                .w_full()
                                .h(px(8.))
                                .rounded_full()
                                .bg(theme.background)
                                .border_1()
                                .border_color(theme.border)
                                .overflow_hidden()
                                .child(
                                    div()
                                        .h_full()
                                        .bg(if mic_testing {
                                            theme.status_online
                                        } else {
                                            theme.muted_foreground
                                        })
                                        .w(gpui_kit::relative(meter_fill)),
                                ),
                        )
                .into_any_element(),
        theme,
    );

    let entrance_sound_subsection = voice_subsection(
        l10n::tr("Entrance sound"),
        None,
        div()
            .w_full()
            .h(px(461.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .w_full()
                    .h(px(315.))
                    .p(px(10.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(settings_border(theme))
                    .bg(theme.background_secondary)
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.foreground)
                                    .child(l10n::tr("Where it plays")),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .line_height(px(20.))
                                    .text_color(theme.muted_foreground)
                                    .child(l10n::tr(
                                        "Choose where your entrance sound plays when you join a voice channel or call.",
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .p(px(4.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(settings_border(theme))
                            .bg(theme.background)
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .children(
                                [
                                    ("off", "Off", "Never play an entrance sound when joining."),
                                    ("all", "All calls and voice channels", "Play in every call and voice channel you join."),
                                    ("servers", "Specific servers", "Only play in selected servers."),
                                    ("channels", "Specific channels", "Only play in selected voice channels."),
                                ]
                                .iter()
                                .map(|&(id, title, desc)| {
                                    let btn_id = format!("settings-entrance-scope-{id}");
                                    div()
                                        .id(SharedString::from(btn_id))
                                        .w_full()
                                        .h(px(52.))
                                        .px(px(8.))
                                        .py(px(7.))
                                        .rounded(px(6.))
                                        .border_1()
                                        .border_color(settings_border(theme))
                                        .flex()
                                        .flex_col()
                                        .justify_center()
                                        .gap(px(1.))
                                        .cursor_pointer()
                                        .on_click(cx.listener(|_, _, _, _| {
                                            // TODO(backlog): wire entrance sound scope selection
                                        }))
                                        .child(
                                            div()
                                                .text_size(px(13.))
                                                .line_height(px(18.))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(theme.foreground)
                                                .child(l10n::tr(title)),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .line_height(px(17.))
                                                .text_color(theme.muted_foreground)
                                                .child(l10n::tr(desc)),
                                        )
                                }),
                            ),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .flex_1()
                    .p(px(12.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(settings_border(theme))
                    .bg(theme.background_secondary)
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.foreground)
                                    .child(l10n::tr("Sound preview")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(l10n::tr("Default chime")),
                            ),
                    )
                    .child(
                        Button::new("settings-entrance-sound-preview")
                            .ghost()
                            .small()
                            .label(l10n::tr("Play"))
                            .on_click(cx.listener(|_, _, _, _| {
                                // TODO(backlog): wire entrance sound preview playback
                            })),
                    ),
            )
            .into_any_element(),
        theme,
    );

    let voice_channels_subsection = voice_subsection(
        l10n::tr("Voice channels"),
        None,
        div()
            .w_full()
            .h(px(72.))
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.foreground)
                            .child(l10n::tr("Show connection IDs")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(l10n::tr(
                                "Show the ID of each voice connection next to people in calls and in voice channels. With it off, each device shows its type.",
                            )),
                    ),
            )
            .child(
                Switch::new("settings-show-connection-ids")
                    .checked(false)
                    .on_change(cx.listener(|_, _: &bool, _, _| {
                        // TODO(backlog): wire show_voice_connection_id preference
                    })),
            )
            .into_any_element(),
        theme,
    );

    let video_content = div()
        .debug_selector(|| "settings-video-quality".to_owned())
        .w_full()
        .min_h(px(447.5))
        .p(px(8.))
        .rounded(px(8.))
        .border_1()
        .border_color(settings_border(theme))
        .bg(theme.background_secondary)
        .flex()
        .flex_col()
        .gap(px(3.))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_0p5()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(l10n::tr("Camera quality")),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child(l10n::tr("Target capture resolution for your video camera.")),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .flex_wrap()
                                        .gap_1()
                                        .children(camera_resolutions.iter().map(|&(val, label)| {
                                            let selected = cam_res == val;
                                            let sel_id = format!("settings-camera-res-{val}");
                                            let val_owned = val.to_owned();
                                            let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                                .small()
                                                .label(label);
                                            btn = if selected { btn.primary() } else { btn.ghost() };
                                            div().debug_selector(move || sel_id.clone()).child(btn.on_click(
                                                cx.listener(move |this, _, window, cx| {
                                                    this.apply_settings_effect(
                                                        SettingsEffect::CameraResolutionChanged(val_owned.clone()),
                                                        window,
                                                        cx,
                                                    );
                                                }),
                                            ))
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_0p5()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(l10n::tr("Mirror camera")),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child(l10n::tr("Flip your local camera preview horizontally.")),
                                        ),
                                )
                                .child(
                                    div().debug_selector(|| "settings-mirror-camera".to_owned()).child(
                                        Switch::new("settings-mirror-camera")
                                            .checked(mirror_cam)
                                            .on_change(cx.listener(|this, checked: &bool, window, cx| {
                                                this.apply_settings_effect(
                                                    SettingsEffect::MirrorCameraChanged(*checked),
                                                    window,
                                                    cx,
                                                );
                                            })),
                                    ),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_0p5()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(l10n::tr("Screen share preset")),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child(l10n::tr(
                                                    "Optimize screen share for readable text or high-motion gaming.",
                                                )),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .flex_wrap()
                                        .gap_1()
                                        .children(stream_presets.iter().map(|&(val, label)| {
                                            let selected = stream_mode == val;
                                            let sel_id = format!("settings-stream-mode-{val}");
                                            let val_owned = val.to_owned();
                                            let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                                .small()
                                                .label(l10n::tr(label));
                                            btn = if selected { btn.primary() } else { btn.ghost() };
                                            div().debug_selector(move || sel_id.clone()).child(btn.on_click(
                                                cx.listener(move |this, _, window, cx| {
                                                    this.apply_settings_effect(
                                                        SettingsEffect::StreamingModeChanged(val_owned.clone()),
                                                        window,
                                                        cx,
                                                    );
                                                }),
                                            ))
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_0p5()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(l10n::tr("Screen share quality (resolution)")),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child(l10n::tr("Choose 480p, 720p, 1080p, 1440p, or native Source resolution.")),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .flex_wrap()
                                        .gap_1()
                                        .children(video_resolutions.iter().map(|&(val, label)| {
                                            let selected = vid_res == val;
                                            let sel_id = format!("settings-video-res-{val}");
                                            let val_owned = val.to_owned();
                                            let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                                .small()
                                                .label(l10n::tr(label));
                                            btn = if selected { btn.primary() } else { btn.ghost() };
                                            div().debug_selector(move || sel_id.clone()).child(btn.on_click(
                                                cx.listener(move |this, _, window, cx| {
                                                    this.apply_settings_effect(
                                                        SettingsEffect::VideoResolutionChanged(val_owned.clone()),
                                                        window,
                                                        cx,
                                                    );
                                                }),
                                            ))
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_0p5()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(l10n::tr("Frame rate")),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child(l10n::tr("Target frame rate for screen sharing.")),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .flex_wrap()
                                        .gap_1()
                                        .children(video_fps_options.iter().map(|&(fps, label)| {
                                            let selected = vid_fps == fps;
                                            let sel_id = format!("settings-video-fps-{fps}");
                                            let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                                .small()
                                                .label(label);
                                            btn = if selected { btn.primary() } else { btn.ghost() };
                                            div().debug_selector(move || sel_id.clone()).child(btn.on_click(
                                                cx.listener(move |this, _, window, cx| {
                                                    this.apply_settings_effect(
                                                        SettingsEffect::VideoFpsChanged(fps),
                                                        window,
                                                        cx,
                                                    );
                                                }),
                                            ))
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_0p5()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(l10n::tr("Video codec")),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child(l10n::tr(
                                                    "Preferred hardware or software encoder codec for screen sharing and video.",
                                                )),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .flex_wrap()
                                        .gap_1()
                                        .children(video_codecs.iter().map(|&(val, label)| {
                                            let selected = vid_codec == val;
                                            let sel_id = format!("settings-video-codec-{val}");
                                            let val_owned = val.to_owned();
                                            let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                                .small()
                                                .label(label);
                                            btn = if selected { btn.primary() } else { btn.ghost() };
                                            div().debug_selector(move || sel_id.clone()).child(btn.on_click(
                                                cx.listener(move |this, _, window, cx| {
                                                    this.apply_settings_effect(
                                                        SettingsEffect::VideoCodecChanged(val_owned.clone()),
                                                        window,
                                                        cx,
                                                    );
                                                }),
                                            ))
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_0p5()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(l10n::tr("Screen share bitrate")),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child(l10n::tr(
                                                    "Higher bitrates keep fast motion sharper and use more upload bandwidth.",
                                                )),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .flex_wrap()
                                        .gap_1()
                                        .children(bitrate_options.iter().map(|&(kbps, label)| {
                                            let selected = vid_bitrate == kbps;
                                            let sel_id = format!("settings-video-bitrate-{kbps}");
                                            let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                                .small()
                                                .label(l10n::tr(label));
                                            btn = if selected { btn.primary() } else { btn.ghost() };
                                            div().debug_selector(move || sel_id.clone()).child(btn.on_click(
                                                cx.listener(move |this, _, window, cx| {
                                                    this.apply_settings_effect(
                                                        SettingsEffect::VideoBitrateChanged(kbps),
                                                        window,
                                                        cx,
                                                    );
                                                }),
                                            ))
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_0p5()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(l10n::tr("Hardware acceleration")),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child(l10n::tr(
                                                    "Use GPU hardware encoders and decoders (NVENC / AMF / QuickSync / MediaFoundation) when available.",
                                                )),
                                        ),
                                )
                                .child(
                                    div().debug_selector(|| "settings-video-hw-accel".to_owned()).child(
                                        Switch::new("settings-video-hw-accel")
                                            .checked(hw_accel)
                                            .on_change(cx.listener(|this, checked: &bool, window, cx| {
                                                this.apply_settings_effect(
                                                    SettingsEffect::VideoHardwareAccelerationChanged(*checked),
                                                    window,
                                                    cx,
                                                );
                                            })),
                                    ),
                                ),
                        )
                .into_any_element();

    let audio_content = div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(20.))
        .child(devices_subsection)
        .child(volume_subsection)
        .child(audio_processing_subsection)
        .child(mic_test_subsection)
        .child(entrance_sound_subsection)
        .child(voice_channels_subsection)
        .into_any_element();

    let mut sections: Vec<(&'static str, String, AnyElement)> = vec![
        ("audio", l10n::tr("Voice"), audio_content),
        ("video", l10n::tr("Video"), video_content),
    ];
    if let Some(active_id) = active_section
        && let Some(pos) = sections.iter().position(|(id, _, _)| *id == active_id)
        && pos > 0
    {
        let item = sections.remove(pos);
        sections.insert(0, item);
    }

    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(20.))
        .children(
            sections
                .into_iter()
                .enumerate()
                .map(|(ix, (_, title, content))| voice_section(title, ix == 0, content, theme)),
        )
        .into_any_element()
}
