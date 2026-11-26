use super::*;

pub(super) fn voice_device_row(
    label: &str,
    device: Option<&str>,
    available_devices: &[String],
    id_prefix: &'static str,
    reset_id: &'static str,
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
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_2()
                .child(div().text_sm().child(label_text.clone()))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
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
        )
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap_1()
                .child(default_button)
                .children(device_buttons),
        )
        .into_any_element()
}

pub(super) fn voice_volume_row(
    label: &str,
    volume: u16,
    dec_id: &'static str,
    inc_id: &'static str,
    reset_id: &'static str,
    make_effect: impl Fn(u16) -> SettingsEffect + Copy + 'static,
    cx: &mut Context<ClientView>,
) -> AnyElement {
    let clamped = volume.clamp(MIN_VOICE_VOLUME, MAX_VOICE_VOLUME);
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(div().text_sm().child(l10n::tr(label)))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
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
                )
                .child(
                    div().debug_selector(move || reset_id.to_owned()).child(
                        Button::new(reset_id)
                            .ghost()
                            .small()
                            .label(l10n::tr("Reset"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.apply_settings_effect(make_effect(DEFAULT_VOICE_VOLUME), window, cx);
                            })),
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

    let devices_section = div()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(section_heading(l10n::tr("Voice & Audio"), theme))
                .child(
                    div().debug_selector(|| "voice-refresh-devices".to_owned()).child(
                        Button::new("voice-refresh-devices")
                            .ghost()
                            .small()
                            .label(l10n::tr("Refresh devices"))
                            .accessibility_label(l10n::tr("Refresh devices"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.refresh_audio_devices(cx);
                            })),
                    ),
                ),
        )
        .child(voice_device_row(
            "Input device",
            view.prefs.data.voice_input_device.as_deref(),
            &view.audio_devices.input_devices,
            "voice-input-device",
            "voice-input-device-reset",
            theme,
            SettingsEffect::VoiceInputDeviceChanged,
            cx,
        ))
        .child(voice_volume_row(
            "Input volume",
            view.prefs.data.voice_input_volume,
            "voice-input-vol-dec",
            "voice-input-vol-inc",
            "voice-input-vol-reset",
            SettingsEffect::VoiceInputVolumeChanged,
            cx,
        ))
        .child(voice_device_row(
            "Output device",
            view.prefs.data.voice_output_device.as_deref(),
            &view.audio_devices.output_devices,
            "voice-output-device",
            "voice-output-device-reset",
            theme,
            SettingsEffect::VoiceOutputDeviceChanged,
            cx,
        ))
        .child(voice_volume_row(
            "Output volume",
            view.prefs.data.voice_output_volume,
            "voice-output-vol-dec",
            "voice-output-vol-inc",
            "voice-output-vol-reset",
            SettingsEffect::VoiceOutputVolumeChanged,
            cx,
        ))
        .into_any_element();

    let input_mode_section = div()
        .debug_selector(|| "settings-voice-input-mode".to_owned())
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Input mode"), theme))
                .child(
                    div()
                        .p_3()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background_secondary)
                        .flex()
                        .flex_col()
                        .gap_3()
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
                                                .child(l10n::tr("Voice transmission mode")),
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
                                            div()
                                                .debug_selector(|| "settings-vad-threshold-dec".to_owned())
                                                .child(
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
                                            div()
                                                .debug_selector(|| "settings-vad-threshold-inc".to_owned())
                                                .child(
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
                                            div()
                                                .debug_selector(|| "settings-vad-threshold-reset".to_owned())
                                                .child(
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
                        ),
                )
                .into_any_element();

    let audio_processing_section = div()
        .debug_selector(|| "settings-voice-processing".to_owned())
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Audio processing"), theme))
                .child(
                    div()
                        .p_3()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background_secondary)
                        .flex()
                        .flex_col()
                        .gap_3()
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
                                                .child(l10n::tr("Voice processing profile")),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child(l10n::tr(
                                                    "Focused Voice isolates speech; Direct Input passes studio audio untouched.",
                                                )),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .flex_wrap()
                                        .gap_1()
                                        .children(profiles.iter().map(|&(val, label)| {
                                            let selected = voice_profile == val;
                                            let sel_id = format!("settings-voice-profile-{val}");
                                            let val_owned = val.to_owned();
                                            let mut btn = Button::new(SharedString::from(sel_id.clone()))
                                                .small()
                                                .label(l10n::tr(label));
                                            btn = if selected { btn.primary() } else { btn.ghost() };
                                            div().debug_selector(move || sel_id.clone()).child(btn.on_click(
                                                cx.listener(move |this, _, window, cx| {
                                                    this.apply_settings_effect(
                                                        SettingsEffect::VoiceProcessingModeChanged(val_owned.clone()),
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
                                                        SettingsEffect::VoiceNoiseSuppressionBackendChanged(
                                                            val_owned.clone(),
                                                        ),
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
                                                .child(l10n::tr("Automatic gain control")),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child(l10n::tr(
                                                    "Automatically normalize microphone volume for consistent speech levels.",
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
                                ),
                        ),
                )
                .into_any_element();

    let mic_test_section = div()
        .debug_selector(|| "settings-voice-mic-test".to_owned())
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Microphone test"), theme))
                .child(
                    div()
                        .p_3()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background_secondary)
                        .flex()
                        .flex_col()
                        .gap_2p5()
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
                        ),
                )
                .into_any_element();

    let video_quality_section = div()
        .debug_selector(|| "settings-video-quality".to_owned())
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading(l10n::tr("Video & screen share"), theme))
                .child(
                    div()
                        .p_3()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background_secondary)
                        .flex()
                        .flex_col()
                        .gap_3()
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
                        ),
                )
                .into_any_element();

    let audio_section = div()
        .flex()
        .flex_col()
        .gap_5()
        .child(devices_section)
        .child(input_mode_section)
        .child(audio_processing_section)
        .child(mic_test_section)
        .into_any_element();

    let mut sections: Vec<(&'static str, AnyElement)> = vec![
        ("audio", audio_section),
        ("video", video_quality_section),
    ];
    if let Some(active_id) = active_section
        && let Some(pos) = sections.iter().position(|(id, _)| *id == active_id)
        && pos > 0
    {
        let item = sections.remove(pos);
        sections.insert(0, item);
    }

    div()
        .flex()
        .flex_col()
        .gap_5()
        .children(sections.into_iter().map(|(_, el)| el))
        .into_any_element()
}
