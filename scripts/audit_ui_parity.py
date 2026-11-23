#!/usr/bin/env python3
"""Automated Source-to-GPUI UI, Control Inventory & State-Machine Parity Auditor v2 (Zero WebView).

Parses the authoritative TypeScript/React + CSS Modules source in `fluxer_app/src`
and verifies that `fluxer_client/app/src` implements:
1. Matching state-machine rules (`LocalVoiceStateMachine.ts`, `UserAreaState.ts`, `GuildListIndicator.ts`).
2. Matching component hierarchies, debug selectors, and hover morphing (`GuildsLayout.module.css`, `GuildListDMItem.tsx`).
3. Full interactive control inventory across all 12 Settings tabs (`VoiceSettings.ts`, `UserVoiceTab.tsx`,
   `UserVideoTab.tsx`, `UserVideoTabState.ts`, `DisplayTab.tsx`, `ChatSettingsTabInputTab.tsx`,
   `PrivacySafetyTabInline.tsx`, `NotificationsTab.tsx`), strictly forbidding static read-only
   `info_card_row` badge substitutions for interactive user preferences.
"""

from __future__ import annotations

import json
import pathlib
import re
import sys
from typing import Any

sys.stdout.reconfigure(encoding="utf-8")

ROOT = pathlib.Path(__file__).resolve().parent.parent
FLUXER_APP = ROOT / "fluxer_app" / "src"
FLUXER_CLIENT = ROOT / "fluxer_client" / "app" / "src"
OUT_JSON = ROOT / "Logs" / "ui-parity-audit.json"


def read_text(path: pathlib.Path) -> str:
    return path.read_text(encoding="utf-8")


def extract_data_flx(rel_path: str) -> list[str]:
    text = read_text(FLUXER_APP / rel_path)
    return re.findall(r'data-flx="([^"]+)"', text)


def check_patterns(rel_path: str, patterns: dict[str, str]) -> dict[str, bool]:
    text = read_text(FLUXER_CLIENT / rel_path)
    return {name: bool(re.search(pat, text, re.MULTILINE)) for name, pat in patterns.items()}


def audit_voice_state_machine() -> dict[str, Any]:
    ts_machine = read_text(FLUXER_APP / "features/voice/state/LocalVoiceStateMachine.ts")
    ts_user_area_state = read_text(FLUXER_APP / "features/app/components/layout/UserAreaState.ts")
    assert "shouldUnmuteOnUndeafen" in ts_machine
    assert "muteToggleLocked" in ts_user_area_state

    checks = check_patterns(
        "backend/voice.rs",
        {
            "should_unmute_on_undeafen_field": r"should_unmute_on_undeafen:\s*bool",
            "effective_muted_method": r"pub fn effective_muted\(&self\)\s*->\s*bool",
            "effective_deafened_method": r"pub fn effective_deafened\(&self\)\s*->\s*bool",
            "mute_toggle_locked_method": r"pub fn mute_toggle_locked\(&self\)\s*->\s*bool",
            "toggle_self_mute_couples_deaf": r"fn toggle_self_mute\(&mut self\)",
            "toggle_self_deaf_forces_mute": r"fn toggle_self_deaf\(&mut self\)",
            "deaf_sets_self_mute_true": r"self\.self_deaf\s*=\s*true;\s*self\.self_mute\s*=\s*true;",
        },
    )
    return {
        "official_source": [
            "fluxer_app/src/features/voice/state/LocalVoiceStateMachine.ts",
            "fluxer_app/src/features/app/components/layout/UserAreaState.ts",
        ],
        "checks": checks,
        "passed": all(checks.values()),
    }


def audit_guild_rail_hover_and_indicator_state_machine() -> dict[str, Any]:
    ts_indicator = read_text(FLUXER_APP / "features/app/components/layout/sidebar_nav/GuildListIndicator.ts")
    css_guilds = read_text(FLUXER_APP / "features/app/components/layout/GuildsLayout.module.css")
    ts_dm_item = read_text(FLUXER_APP / "features/app/components/layout/sidebar_nav/GuildListDMItem.tsx")
    assert "resolveGuildListIndicatorHeight" in ts_indicator
    assert "return 40;" in ts_indicator and "return 20;" in ts_indicator and "return 8;" in ts_indicator
    assert "border-radius: 30%" in css_guilds
    assert "--brand-primary" in css_guilds
    assert "resolveGuildListIndicatorBarTarget" in ts_dm_item

    rail_checks = check_patterns(
        "ui/guild_rail.rs",
        {
            "indicator_height_state_machine_fn": r"pub\(crate\) fn resolve_rail_indicator_height\(",
            "indicator_height_selected_40px": r"if selected \{\s*Some\(40\.\)",
            "indicator_height_hovered_20px": r"else if hovered \{\s*Some\(20\.\)",
            "indicator_height_unread_8px": r"else if unread \{\s*Some\(8\.\)",
            "rail_pill_selector": r"rail-pill-",
            "home_wrapped_in_rail_item": r'rail_item\(\s*"home"',
            "favorites_wrapped_in_rail_item": r'rail_item\(\s*"favorites"',
            "home_on_hover_listener": r"RailHoverTarget::Home",
            "favorites_on_hover_listener": r"RailHoverTarget::Favorites",
            "guild_on_hover_listener": r"RailHoverTarget::Guild\(id\)",
            "discover_on_hover_listener": r"RailHoverTarget::Discover",
            "join_on_hover_listener": r"RailHoverTarget::Join",
            "active_or_hovered_morphs_to_12px": r"if active \{\s*12\.",
            "active_or_hovered_uses_brand_bg": r"theme\.brand",
        },
    )
    client_checks = check_patterns(
        "client_view.rs",
        {
            "rail_hover_target_enum": r"pub\(crate\) enum RailHoverTarget",
            "hovered_rail_item_field": r"pub\(crate\) hovered_rail_item:\s*Option<RailHoverTarget>",
            "favorites_selected_field": r"pub\(crate\) favorites_selected:\s*bool",
        },
    )
    all_checks = {**rail_checks, **client_checks}
    return {
        "official_source": [
            "fluxer_app/src/features/app/components/layout/sidebar_nav/GuildListIndicator.ts",
            "fluxer_app/src/features/app/components/layout/sidebar_nav/GuildListDMItem.tsx",
            "fluxer_app/src/features/app/components/layout/GuildsLayout.module.css",
        ],
        "checks": all_checks,
        "passed": all(all_checks.values()),
    }


def audit_bottom_left_user_area() -> dict[str, Any]:
    official_flx = (
        extract_data_flx("features/app/components/layout/UserArea.tsx")
        + extract_data_flx("features/voice/components/voice_connection_status/VoiceConnectionStatusInner.tsx")
        + extract_data_flx("features/voice/components/voice_connection_status/LocalParticipantControls.tsx")
    )
    voice_checks = check_patterns(
        "ui/voice_panel.rs",
        {
            "status_row_signal_icon": r"voice-signal-icon",
            "status_button_open_stage": r"voice-panel-open-stage",
            "noise_suppression_button": r"voice-noise-suppression-btn",
            "disconnect_icon_button": r"AppIcon::PhoneDisconnect",
            "channel_guild_source_row": r"voice-channel-source-link",
            "channel_avatar_stack": r"voice-channel-avatar-stack",
            "local_participant_camera_button": r"voice-camera-btn",
            "local_participant_screen_share_button": r"voice-share-btn",
        },
    )
    user_checks = check_patterns(
        "ui/user_panel.rs",
        {
            "unified_user_area_wrapper": r"user-area-wrapper",
            "user_area_popout_menu": r"user-area-popout",
            "user_area_status_selector": r"user-area-status-",
            "user_area_edit_profile_action": r"user-area-edit-profile",
            "effective_muted_danger_highlight": r"effective_muted",
            "effective_deafened_danger_highlight": r"effective_deafened",
            "mute_toggle_locked_handling": r"mute_toggle_locked",
            "user_panel_mute_selector": r"user-panel-mute",
            "user_panel_deafen_selector": r"user-panel-deafen",
        },
    )
    voice_popout_checks = check_patterns(
        "ui/voice_panel.rs",
        {
            "inline_voice_details_popout": r"voice-details-popout",
            "inline_noise_suppression_switch": r"voice-popout-noise-suppression",
        },
    )
    all_checks = {**voice_checks, **user_checks, **voice_popout_checks}
    return {
        "official_data_flx_count": len(official_flx),
        "checks": all_checks,
        "passed": all(all_checks.values()),
    }


def audit_voice_call_stage() -> dict[str, Any]:
    official_flx = (
        extract_data_flx("features/voice/components/compact_voice_call_view/CompactVoiceCallViewInner.tsx")
        + extract_data_flx("features/voice/components/VoiceControlBar.tsx")
    )
    stage_checks = check_patterns(
        "ui/voice_stage.rs",
        {
            "split_or_expand_toggle": r"voice-stage-expand-btn",
            "audio_only_wrapped_avatar_list": r"voice-stage-avatar-list",
            "grouped_mic_dropdown": r"voice-stage-input-settings-btn",
            "grouped_speaker_dropdown": r"voice-stage-output-settings-btn",
            "speaker_high_icon_in_control_bar": r"AppIcon::SpeakerHigh",
            "grouped_camera_button": r"voice-stage-camera-btn",
            "grouped_screen_share_dropdown": r"voice-stage-share-settings-btn",
            "more_options_button": r"voice-stage-more-btn",
            "control_bar_separator": r"voice-stage-control-separator",
            "disconnect_button": r"voice-stage-disconnect-btn",
            "inline_voice_control_popout": r"voice-control-popout",
            "stream_popout_dynamic_resolution_fps_codec": r"prefs\.data\.video_resolution",
        },
    )
    client_checks = check_patterns(
        "client_view.rs",
        {
            "voice_stage_split_chat_support": r"voice_stage_expanded",
        },
    )
    all_checks = {**stage_checks, **client_checks}
    return {
        "official_data_flx_count": len(official_flx),
        "checks": all_checks,
        "passed": all(all_checks.values()),
    }


def audit_settings_modal_and_interactive_controls() -> dict[str, Any]:
    official_flx = (
        extract_data_flx("features/app/components/dialogs/components/DesktopSettingsView.tsx")
        + extract_data_flx("features/user/components/modals/tabs/UserVoiceTab.tsx")
        + extract_data_flx("features/user/components/modals/tabs/UserVideoTab.tsx")
        + extract_data_flx("features/user/components/modals/tabs/chat_settings_tab/DisplayTab.tsx")
    )
    ts_voice_settings = read_text(FLUXER_APP / "features/voice/state/VoiceSettings.ts")
    for field in [
        "voiceProcessingMode",
        "echoCancellation",
        "autoGainControl",
        "noiseSuppressionBackend",
        "vadThreshold",
        "vadAutoSensitivity",
        "cameraResolution",
        "mirrorCamera",
        "screenshareResolution",
        "videoFrameRate",
        "streamingMode",
        "preferredVideoCodec",
        "screenShareBitrate",
    ]:
        assert field in ts_voice_settings, f"Missing {field} in VoiceSettings.ts"

    prefs_checks = check_patterns(
        "prefs.rs",
        {
            "pref_voice_processing_mode": r"pub voice_processing_mode:\s*String",
            "pref_voice_echo_cancellation": r"pub voice_echo_cancellation:\s*bool",
            "pref_voice_noise_suppression_backend": r"pub voice_noise_suppression_backend:\s*String",
            "pref_voice_auto_gain_control": r"pub voice_auto_gain_control:\s*bool",
            "pref_voice_vad_auto_threshold": r"pub voice_vad_auto_threshold:\s*bool",
            "pref_voice_vad_threshold": r"pub voice_vad_threshold:\s*u8",
            "pref_voice_ptt_release_delay_ms": r"pub voice_ptt_release_delay_ms:\s*u16",
            "pref_camera_resolution": r"pub camera_resolution:\s*String",
            "pref_mirror_camera": r"pub mirror_camera:\s*bool",
            "pref_streaming_mode": r"pub streaming_mode:\s*String",
            "pref_video_resolution": r"pub video_resolution:\s*String",
            "pref_video_fps": r"pub video_fps:\s*u8",
            "pref_video_codec": r"pub video_codec:\s*String",
            "pref_video_bitrate_kbps": r"pub video_bitrate_kbps:\s*u16",
            "pref_video_hardware_acceleration": r"pub video_hardware_acceleration:\s*bool",
            "pref_friend_request_access": r"pub friend_request_access:\s*String",
            "pref_spoiler_display": r"pub spoiler_display:\s*String",
            "pref_render_embeds": r"pub render_embeds:\s*bool",
            "pref_render_reactions": r"pub render_reactions:\s*bool",
            "pref_convert_emoticons": r"pub convert_emoticons:\s*bool",
            "pref_unread_badge_enabled": r"pub unread_badge_enabled:\s*bool",
            "pref_time_format_24h": r"pub time_format_24h:\s*bool",
        },
    )

    settings_checks = check_patterns(
        "ui/settings.rs",
        {
            "clickable_profile_card_tab": r"settings-profile-card",
            "tab_profile": r"SettingsTab::Profile",
            "tab_account_security": r"settings-tab-account",
            "tab_privacy_safety": r"settings-tab-privacy",
            "tab_appearance_look_and_feel": r"settings-tab-appearance",
            "tab_themes": r"settings-tab-themes",
            "tab_notifications": r"settings-tab-notifications",
            "tab_messages_media": r"settings-tab-chat",
            "tab_voice_video": r"settings-tab-voice",
            "tab_accessibility": r"settings-tab-accessibility",
            "tab_language_time": r"settings-tab-language",
            "tab_keybinds": r"settings-tab-keybinds",
            "tab_windows_advanced": r"settings-tab-advanced",
            "expandable_sidebar_subsections": r"settings-subitem-",
            # Voice & Audio interactive controls
            "voice_input_mode_vad_ptt": r"settings-voice-input-mode",
            "interactive_input_mode_vad": r"settings-input-mode-vad",
            "interactive_input_mode_ptt": r"settings-input-mode-ptt",
            "interactive_ptt_release_delay": r"settings-ptt-delay-",
            "interactive_vad_auto_threshold_switch": r"settings-vad-auto-threshold",
            "interactive_vad_threshold_stepper": r"settings-vad-threshold-",
            "voice_processing_profile_selector": r"settings-voice-profile-",
            "interactive_echo_cancellation_switch": r"settings-echo-cancellation",
            "interactive_noise_suppression_switch": r"settings-noise-suppression",
            "interactive_noise_suppression_backend": r"settings-ns-backend-",
            "interactive_auto_gain_control_switch": r"settings-auto-gain-control",
            "interactive_mic_test_toggle_button": r"settings-mic-test-btn",
            "interactive_mic_test_level_meter": r"settings-mic-test-meter",
            # Video & Screen Share interactive controls
            "interactive_camera_resolution": r"settings-camera-res-",
            "interactive_mirror_camera_switch": r"settings-mirror-camera",
            "interactive_streaming_preset": r"settings-stream-mode-",
            "interactive_video_resolution": r"settings-video-res-",
            "interactive_video_fps": r"settings-video-fps-",
            "interactive_video_codec": r"settings-video-codec-",
            "interactive_video_bitrate": r"settings-video-bitrate-",
            "interactive_video_hw_accel_switch": r"settings-video-hw-accel",
            # Privacy, Messages & Media, Notifications, Language interactive controls
            "interactive_allow_guild_dms_switch": r"settings-allow-guild-dms",
            "interactive_friend_request_access": r"settings-friend-req-",
            "interactive_spoiler_display_mode": r"settings-spoiler-mode-",
            "interactive_inline_media_switch": r"settings-inline-media",
            "interactive_render_embeds_switch": r"settings-render-embeds",
            "interactive_render_reactions_switch": r"settings-render-reactions",
            "interactive_convert_emoticons_switch": r"settings-convert-emoticons",
            "interactive_unread_badge_switch": r"settings-unread-badge",
            "interactive_time_format_selector": r"settings-time-format-",
            "keybinds_reference_table": r"settings-keybinds-list",
        },
    )

    # Anti-pattern guard: ensure `render_voice_tab`, `render_privacy_tab`,
    # `render_messages_media_tab`, `render_notifications_tab`, and `render_language_tab`
    # do NOT contain any static `info_card_row(` placeholders!
    settings_src = read_text(FLUXER_CLIENT / "ui/settings.rs")
    no_static_placeholders: dict[str, bool] = {}
    for fn_name in [
        "render_voice_tab",
        "render_privacy_tab",
        "render_messages_media_tab",
        "render_notifications_tab",
        "render_language_tab",
    ]:
        start = settings_src.find(f"fn {fn_name}(")
        nxt = settings_src.find("\nfn ", start + 1)
        body = settings_src[start:nxt] if start != -1 else ""
        no_static_placeholders[f"no_static_info_card_in_{fn_name}"] = (
            bool(body) and "info_card_row(" not in body
        )

    all_checks = {**prefs_checks, **settings_checks, **no_static_placeholders}
    return {
        "official_data_flx_count": len(official_flx),
        "checks": all_checks,
        "passed": all(all_checks.values()),
    }


def audit_m5_navigation_and_rows() -> dict[str, Any]:
    channel_list_checks = check_patterns(
        "ui/channel_list.rs",
        {
            "voice_member_live_badge": r"voice-member-live-",
            "voice_member_muted_icon": r"voice-member-muted-",
            "voice_member_deaf_icon": r"voice-member-deaf-",
        },
    )
    rail_checks = check_patterns(
        "ui/guild_rail.rs",
        {
            "home_brand_icon": r"rail-home-icon",
            "guild_icon_image": r"guild-rail-icon-",
        },
    )
    channel_modal_checks = check_patterns(
        "ui/channel_modal.rs",
        {
            "channel_settings_tab_overview": r"channel-settings-tab-overview",
            "channel_settings_tab_permissions": r"channel-settings-tab-permissions",
        },
    )
    access_checks = check_patterns(
        "ui/access.rs",
        {
            "access_channel_tab_overview": r"channel-settings-tab-overview",
            "access_guild_tab_roles": r"guild-settings-tab-roles",
            "access_guild_tab_bans": r"guild-settings-tab-bans",
        },
    )
    moderation_checks = check_patterns(
        "ui/moderation_modal.rs",
        {
            "bans_guild_tab_roles": r"guild-settings-tab-roles",
            "bans_guild_tab_bans": r"guild-settings-tab-bans",
        },
    )
    profile_checks = check_patterns(
        "ui/profile_popout.rs",
        {
            "profile_moderation_subcard": r"profile-moderation-card",
        },
    )
    all_checks = {
        **channel_list_checks,
        **rail_checks,
        **channel_modal_checks,
        **access_checks,
        **moderation_checks,
        **profile_checks,
    }
    return {
        "checks": all_checks,
        "passed": all(all_checks.values()),
    }


def audit_m6_live_parity_and_scenes() -> dict[str, Any]:
    rail_m6 = check_patterns(
        "ui/guild_rail.rs",
        {
            "rail_favorites_button": r"rail-favorites-btn",
            "rail_top_separator": r"rail-separator",
            "rail_discover_button": r"rail-discover-btn",
        },
    )
    channel_list_m6 = check_patterns(
        "ui/channel_list.rs",
        {
            "guild_header_caret_menu_btn": r"guild-header-menu-btn",
            "guild_header_popout_menu": r"guild-header-popout",
        },
    )
    header_m6 = check_patterns(
        "ui/header.rs",
        {
            "header_threads_button": r"header-threads-btn",
            "header_pins_button": r"header-pins-btn",
            "header_favorite_button": r"header-favorite-btn",
            "header_inbox_button": r"header-inbox-btn",
        },
    )
    composer_m6 = check_patterns(
        "ui/composer.rs",
        {
            "composer_gif_button": r"composer-gif",
            "composer_sticker_button": r"composer-sticker",
        },
    )
    message_m6 = check_patterns(
        "ui/message_row.rs",
        {
            "self_mention_row_highlight": r"msg-mentioned-",
        },
    )
    embed_m6 = check_patterns(
        "ui/embed.rs",
        {
            "youtube_unproxied_video_poster_support": r"youtube_or_proxied_video_view",
        },
    )
    deeplink_m6 = check_patterns(
        "client_view.rs",
        {
            "live_scene_deeplink_driver": r"apply_scene_deep_link",
        },
    )
    all_checks = {
        **rail_m6,
        **channel_list_m6,
        **header_m6,
        **composer_m6,
        **message_m6,
        **embed_m6,
        **deeplink_m6,
    }
    return {
        "checks": all_checks,
        "passed": all(all_checks.values()),
    }


def audit_m8_screen_layout_and_icon_parity() -> dict[str, Any]:
    icons_m8 = check_patterns(
        "icons.rs",
        {
            "phosphor_star_icon": r"Self::Star\s*=>\s*\"icons/phosphor/star\.svg\"",
            "phosphor_compass_icon": r"Self::Compass\s*=>\s*\"icons/phosphor/compass\.svg\"",
            "phosphor_tray_icon": r"Self::Tray\s*=>\s*\"icons/phosphor/tray\.svg\"",
            "phosphor_image_icon": r"Self::Image\s*=>\s*\"icons/phosphor/image\.svg\"",
            "phosphor_sticker_icon": r"Self::Sticker\s*=>\s*\"icons/phosphor/sticker\.svg\"",
        },
    )
    layout_m8 = check_patterns(
        "client_view.rs",
        {
            "unified_left_column_rail_plus_sidebar_width": r"theme\.sizes\.rail_width\s*\+\s*theme\.sizes\.channel_list_width",
            "profile_self_scene_deeplink": r'\["profile-self"\]',
        },
    )
    rail_m8 = check_patterns(
        "ui/guild_rail.rs",
        {
            "rail_background_secondary": r"\.bg\(theme\.background_secondary\)",
            "rail_favorites_star_icon": r"AppIcon::Star",
            "rail_discover_compass_icon": r"AppIcon::Compass",
        },
    )
    channel_list_m8 = check_patterns(
        "ui/channel_list.rs",
        {
            "sidebar_background_secondary": r"\.bg\(theme\.background_secondary\)",
            "header_height_56px_alignment": r"\.h\(px\(theme\.sizes\.header_height\)\)",
            "channel_row_32px_min_height": r"\.min_h\(px\(32\.\)\)",
            "channel_row_16px_small_icon": r"Icon::new\(icon\)\.small\(\)",
        },
    )
    header_m8 = check_patterns(
        "ui/header.rs",
        {
            "channel_title_never_prematurely_truncated": r"\.flex_none\(\)\s*\.font_weight\(FontWeight::SEMIBOLD\)",
            "header_favorite_star_icon": r"AppIcon::Star",
            "header_inbox_tray_icon": r"AppIcon::Tray",
        },
    )
    composer_m8 = check_patterns(
        "ui/composer.rs",
        {
            "composer_gif_image_icon": r"AppIcon::Image",
            "composer_sticker_icon": r"AppIcon::Sticker",
        },
    )
    friends_m8 = check_patterns(
        "ui/friends.rs",
        {
            "friends_header_height_56px": r"\.h\(px\(theme\.sizes\.header_height\)\)",
            "friends_header_vertical_divider": r"\.w\(px\(1\.\)\)\.h\(px\(24\.\)\)",
            "friends_add_tab_brand_primary": r"tab == FriendsTab::AddFriend[\s\S]{0,160}\.bg\(theme\.brand\)[\s\S]{0,80}\.text_color\(theme\.on_fill\)",
            "friends_active_tab_accent_surface": r"else if active[\s\S]{0,120}\.bg\(accent\)\.text_color\(fg\)",
        },
    )
    user_panel_m8 = check_patterns(
        "ui/user_panel.rs",
        {
            "user_panel_switch_accounts_hover_reveal": r"\.group\(panel_group\.clone\(\)\)[\s\S]*?switch-accounts[\s\S]{0,300}\.opacity\(0\.\)[\s\S]{0,160}\.group_hover\(panel_group",
        },
    )
    settings_m8 = check_patterns(
        "ui/settings.rs",
        {
            "profile_tab_banner_cutout_card": r"fn render_profile_tab[\s\S]{0,2400}settings-profile-banner[\s\S]{0,600}\.p\(px\(6\.\)\)",
        },
    )
    voice_stage_m8 = check_patterns(
        "ui/voice_stage.rs",
        {
            "voice_stage_pure_black_canvas": r"\.bg\(rgb\(0x000000\)\)",
            "voice_control_44px_button": r"\.size\(px\(44\.\)\)",
            "voice_control_16px_radius": r"\.rounded\(px\(16\.\)\)",
            "voice_control_17px_corner_caret_badge": r"\.size\(px\(17\.\)\)",
        },
    )
    all_checks = {
        **icons_m8,
        **layout_m8,
        **rail_m8,
        **channel_list_m8,
        **header_m8,
        **composer_m8,
        **friends_m8,
        **user_panel_m8,
        **settings_m8,
        **voice_stage_m8,
    }
    return {
        "checks": all_checks,
        "passed": all(all_checks.values()),
    }


def main() -> int:
    report = {
        "voice_state_machine": audit_voice_state_machine(),
        "guild_rail_hover_and_indicator": audit_guild_rail_hover_and_indicator_state_machine(),
        "bottom_left_user_area": audit_bottom_left_user_area(),
        "voice_call_stage": audit_voice_call_stage(),
        "settings_modal_and_interactive_controls": audit_settings_modal_and_interactive_controls(),
        "m5_navigation_and_rows": audit_m5_navigation_and_rows(),
        "m6_live_parity_and_scenes": audit_m6_live_parity_and_scenes(),
        "m8_screen_layout_and_icon_parity": audit_m8_screen_layout_and_icon_parity(),
    }
    OUT_JSON.parent.mkdir(parents=True, exist_ok=True)
    OUT_JSON.write_text(json.dumps(report, indent=2, ensure_ascii=False), encoding="utf-8")

    total_checks = 0
    passed_checks = 0
    for area, data in report.items():
        checks = data["checks"]
        area_total = len(checks)
        area_passed = sum(1 for v in checks.values() if v)
        total_checks += area_total
        passed_checks += area_passed
        status = "PASS" if data["passed"] else "FAIL"
        print(f"[{status}] {area}: {area_passed}/{area_total}")
        for name, ok in checks.items():
            if not ok:
                print(f"   - MISSING: {name}")

    print(f"\nTotal UI parity contract checks: {passed_checks}/{total_checks}")
    return 0 if passed_checks == total_checks else 1


if __name__ == "__main__":
    raise SystemExit(main())
