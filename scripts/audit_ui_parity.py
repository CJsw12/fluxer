#!/usr/bin/env python3
"""Automated Source-to-GPUI UI & State-Machine Parity Auditor (Zero WebView).

Parses the authoritative TypeScript/React + CSS Modules source in `fluxer_app/src`
and verifies that `fluxer_client/app/src` implements matching state-machine rules,
component hierarchies, debug selectors, iconography, and settings sections.
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
            "effective_muted_danger_highlight": r"effective_muted",
            "effective_deafened_danger_highlight": r"effective_deafened",
            "mute_toggle_locked_handling": r"mute_toggle_locked",
            "user_panel_mute_selector": r"user-panel-mute",
            "user_panel_deafen_selector": r"user-panel-deafen",
        },
    )
    all_checks = {**voice_checks, **user_checks}
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


def audit_settings_modal() -> dict[str, Any]:
    official_flx = extract_data_flx("features/app/components/dialogs/components/DesktopSettingsView.tsx")
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
            "voice_input_mode_vad_ptt": r"settings-voice-input-mode",
            "voice_noise_suppression_section": r"settings-voice-processing",
            "voice_mic_test_section": r"settings-voice-mic-test",
            "privacy_dm_and_sensitive_media": r"settings-privacy-dm",
            "chat_inline_media_and_spoilers": r"settings-chat-display",
            "keybinds_reference_table": r"settings-keybinds-list",
        },
    )
    return {
        "official_data_flx_count": len(official_flx),
        "checks": settings_checks,
        "passed": all(settings_checks.values()),
    }


def main() -> int:
    report = {
        "voice_state_machine": audit_voice_state_machine(),
        "bottom_left_user_area": audit_bottom_left_user_area(),
        "voice_call_stage": audit_voice_call_stage(),
        "settings_modal": audit_settings_modal(),
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
