// SPDX-License-Identifier: AGPL-3.0-or-later

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 클라이언트 성능 및 지원 역량 매니페스트 DTO (`packages/desktop_ipc/src/CapabilityManifest.ts`)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "contracts/DesktopCapabilityManifest.ts")]
#[serde(rename_all = "camelCase")]
pub struct DesktopCapabilityManifest {
    pub app_store: bool,
    pub gateway_socket: bool,
}

/// 저장된 인스턴스 기록 DTO (`packages/desktop_ipc/src/KnownInstanceContract.ts`)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "contracts/DesktopKnownInstanceRecord.ts")]
#[serde(rename_all = "camelCase")]
pub struct DesktopKnownInstanceRecord {
    pub instance_key: String,
    pub domain: String,
    pub display_name: String,
    #[ts(type = "number")]
    pub last_used: u64,
}

/// 스토리지 격리 사유 열거형 (`packages/desktop_ipc/src/StorageContract.ts`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "contracts/DesktopStoreQuarantineReason.ts")]
#[serde(rename_all = "camelCase")]
pub enum DesktopStoreQuarantineReason {
    Newer,
    Schema,
    Corrupt,
    Derivation,
}

/// 데스크톱 스토리지 상태 DTO (`packages/desktop_ipc/src/StorageContract.ts`)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "contracts/DesktopStoreStatus.ts")]
#[serde(rename_all = "camelCase")]
pub struct DesktopStoreStatus {
    pub available: bool,
    pub authority_expected: bool,
    pub schema_version: u32,
    pub quarantined: bool,
    pub quarantine_reason: Option<DesktopStoreQuarantineReason>,
    pub unavailable_reason: Option<String>,
}

/// 스토리지 엔트리 DTO (`packages/desktop_ipc/src/StorageContract.ts`)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "contracts/DesktopStorageEntry.ts")]
#[serde(rename_all = "camelCase")]
pub struct DesktopStorageEntry {
    pub key: String,
    pub value: String,
    #[serde(default)]
    #[ts(type = "number")]
    pub updated_at: u64,
}

/// 스토리지 단일 쓰기 요청 DTO (`packages/desktop_ipc/src/StorageContract.ts`)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "contracts/DesktopStorageWrite.ts")]
#[serde(rename_all = "camelCase")]
pub struct DesktopStorageWrite {
    pub scope: String,
    pub key: String,
    pub value: String,
    #[serde(default)]
    #[ts(optional)]
    pub if_absent: Option<bool>,
}

/// 스토리지 마커 DTO (`packages/desktop_ipc/src/AccountContract.ts`)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "contracts/DesktopStoreMarker.ts")]
#[serde(rename_all = "camelCase")]
pub struct DesktopStoreMarker {
    pub key: String,
    pub value: String,
}

/// 계정 레코드 DTO (`packages/desktop_ipc/src/AccountContract.ts`)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "contracts/DesktopAccountRecord.ts")]
#[serde(rename_all = "camelCase")]
pub struct DesktopAccountRecord {
    pub storage_key: String,
    #[ts(type = "Record<string, any>")]
    pub record: serde_json::Value,
}

/// 계정 CAS(Compare and Swap) 요청 DTO (`packages/desktop_ipc/src/AccountContract.ts`)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "contracts/DesktopAccountCompareAndSwapRequest.ts")]
#[serde(rename_all = "camelCase")]
pub struct DesktopAccountCompareAndSwapRequest {
    pub expected: DesktopAccountRecord,
    pub replacement: DesktopAccountRecord,
}

/// 데스크톱 윈도우 동작 설정 DTO (`fluxer_desktop/src/common/Types.ts`)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "contracts/DesktopWindowBehaviorSettings.ts")]
#[serde(rename_all = "camelCase")]
pub struct DesktopWindowBehaviorSettings {
    pub show_tray_icon: bool,
    pub colored_tray_icon: bool,
    pub dark_taskbar_badge: bool,
    pub minimize_to_tray: bool,
    pub close_to_tray: bool,
    pub start_minimized: bool,
    pub use_native_title_bar: bool,
    pub active_use_native_title_bar: bool,
    pub remember_window_state: bool,
    pub allow_transparency: bool,
    pub active_allow_transparency: bool,
    pub smooth_scrolling: bool,
    pub active_smooth_scrolling: bool,
    pub middle_click_autoscroll: bool,
    pub active_middle_click_autoscroll: bool,
}

/// 음성 상태 오버레이 DTO
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "contracts/VoiceOverlayState.ts")]
#[serde(rename_all = "camelCase")]
pub struct VoiceOverlayState {
    pub is_connected: bool,
    pub channel_name: String,
    pub is_muted: bool,
    pub is_deafened: bool,
    pub is_speaking: bool,
    pub active_speakers: Vec<String>,
}

/// 테마 디렉터리 내 단일 CSS 파일 DTO (`packages/desktop_ipc/src/DesktopBridgeTypes.ts`)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "contracts/ThemeDirectoryCssFile.ts")]
#[serde(rename_all = "camelCase")]
pub struct ThemeDirectoryCssFile {
    pub file_name: String,
    pub path: String,
    pub css: String,
}

/// 테마 파일 이름 변경 기록 DTO (`packages/desktop_ipc/src/DesktopBridgeTypes.ts`)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "contracts/ThemeFolderRename.ts")]
#[serde(rename_all = "camelCase")]
pub struct ThemeFolderRename {
    pub from: String,
    pub to: String,
    pub file_name: String,
}

/// 테마 폴더 상태 열거형 (`packages/desktop_ipc/src/DesktopBridgeTypes.ts`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "contracts/ThemeFolderStatus.ts")]
#[serde(rename_all = "camelCase")]
pub enum ThemeFolderStatus {
    Live,
    Missing,
    NotAllowed,
}

/// 테마 디렉터리 변경 이벤트 DTO (`packages/desktop_ipc/src/DesktopBridgeTypes.ts`)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "contracts/ThemeFolderChange.ts")]
#[serde(rename_all = "camelCase")]
pub struct ThemeFolderChange {
    pub folder: String,
    pub status: ThemeFolderStatus,
    pub truncated: bool,
    pub snapshot: bool,
    pub added: Vec<ThemeDirectoryCssFile>,
    pub changed: Vec<ThemeDirectoryCssFile>,
    pub removed: Vec<String>,
    pub renamed: Vec<ThemeFolderRename>,
}

/// 전역 단축키 상태 DTO (`packages/desktop_ipc/src/GlobalShortcutsContract.ts`)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "contracts/GlobalShortcutsStatus.ts")]
#[serde(rename_all = "camelCase")]
pub struct GlobalShortcutsStatus {
    pub supported: bool,
    pub has_access: bool,
    pub direct_input_enabled: bool,
    pub direct_input_available: bool,
    pub is_capturing: bool,
    pub mode: String,
}

/// 전역 키/마우스 훅 등록 옵션 DTO (`packages/desktop_ipc/src/GlobalKeyHookContract.ts`)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "contracts/GlobalKeyHookRegisterOptions.ts")]
#[serde(rename_all = "camelCase")]
pub struct GlobalKeyHookRegisterOptions {
    pub id: String,
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub alt: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub meta: bool,
    #[serde(default)]
    pub key_name: Option<String>,
    #[serde(default)]
    pub physical_key_name: Option<String>,
    #[serde(default)]
    pub mouse_button: Option<u8>,
}

/// AccountEngine 스냅샷 커서 DTO (`packages/desktop_ipc/src/AccountEngineContract.ts`)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "contracts/AccountEngineSnapshotCursor.ts")]
#[serde(rename_all = "camelCase")]
pub struct AccountEngineSnapshotCursor {
    pub session_id: String,
    pub schema_epoch: u32,
    #[ts(type = "number")]
    pub updated_at: u64,
}

/// AccountEngine 계정 요약 DTO (`packages/desktop_ipc/src/AccountEngineContract.ts`)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "contracts/AccountEngineAccountSummary.ts")]
#[serde(rename_all = "camelCase")]
pub struct AccountEngineAccountSummary {
    pub account_key: String,
    pub user_id: String,
    pub username: String,
    pub global_name: Option<String>,
    pub avatar_hash: Option<String>,
    pub unread_count: u32,
    pub mention_count: u32,
    #[ts(type = "number")]
    pub last_active: u64,
}

/// AccountEngine 멘션 DTO (`packages/desktop_ipc/src/AccountEngineContract.ts`)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "contracts/AccountEngineMention.ts")]
#[serde(rename_all = "camelCase")]
pub struct AccountEngineMention {
    pub account_key: String,
    pub message_id: String,
    #[ts(type = "number")]
    pub created_at: u64,
    pub channel_id: String,
    pub channel_type: Option<i32>,
    pub guild_id: Option<String>,
    pub author_id: String,
    pub author_name: String,
    pub content: String,
}

/// AccountEngine 계정 활동 DTO (`packages/desktop_ipc/src/AccountEngineContract.ts`)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "contracts/AccountEngineAccountActivity.ts")]
#[serde(rename_all = "camelCase")]
pub struct AccountEngineAccountActivity {
    pub account_key: String,
    pub unread_channels: Vec<String>,
    pub mentions: Vec<AccountEngineMention>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_export_all_contracts_to_ts() {
        DesktopCapabilityManifest::export().unwrap();
        DesktopKnownInstanceRecord::export().unwrap();
        DesktopStoreQuarantineReason::export().unwrap();
        DesktopStoreStatus::export().unwrap();
        DesktopStorageEntry::export().unwrap();
        DesktopStorageWrite::export().unwrap();
        DesktopStoreMarker::export().unwrap();
        DesktopAccountRecord::export().unwrap();
        DesktopAccountCompareAndSwapRequest::export().unwrap();
        DesktopWindowBehaviorSettings::export().unwrap();
        VoiceOverlayState::export().unwrap();
        ThemeDirectoryCssFile::export().unwrap();
        ThemeFolderRename::export().unwrap();
        ThemeFolderStatus::export().unwrap();
        ThemeFolderChange::export().unwrap();
        GlobalShortcutsStatus::export().unwrap();
        GlobalKeyHookRegisterOptions::export().unwrap();
        AccountEngineSnapshotCursor::export().unwrap();
        AccountEngineAccountSummary::export().unwrap();
        AccountEngineMention::export().unwrap();
        AccountEngineAccountActivity::export().unwrap();

        let base = std::path::Path::new("bindings/contracts");
        assert!(base.join("DesktopCapabilityManifest.ts").exists() || std::path::Path::new("contracts/DesktopCapabilityManifest.ts").exists());
    }

    #[test]
    fn test_known_instance_fixture_roundtrip() {
        let fixture = json!({
            "instanceKey": "https://fluxer.example.com/api",
            "domain": "fluxer.example.com",
            "displayName": "Example Instance",
            "lastUsed": 1700000000000_u64
        });

        let parsed: DesktopKnownInstanceRecord = serde_json::from_value(fixture.clone()).unwrap();
        assert_eq!(parsed.domain, "fluxer.example.com");
        assert_eq!(parsed.display_name, "Example Instance");
        assert_eq!(parsed.last_used, 1700000000000);

        let serialized = serde_json::to_value(&parsed).unwrap();
        assert_eq!(serialized, fixture);
    }

    #[test]
    fn test_window_behavior_defaults_matching() {
        let defaults = DesktopWindowBehaviorSettings {
            show_tray_icon: true,
            colored_tray_icon: false,
            dark_taskbar_badge: false,
            minimize_to_tray: false,
            close_to_tray: false,
            start_minimized: false,
            use_native_title_bar: false,
            active_use_native_title_bar: false,
            remember_window_state: true,
            allow_transparency: false,
            active_allow_transparency: false,
            smooth_scrolling: true,
            active_smooth_scrolling: true,
            middle_click_autoscroll: true,
            active_middle_click_autoscroll: true,
        };

        let val = serde_json::to_value(&defaults).unwrap();
        assert_eq!(val["showTrayIcon"], true);
        assert_eq!(val["coloredTrayIcon"], false);
        assert_eq!(val["minimizeToTray"], false);
        assert_eq!(val["middleClickAutoscroll"], true);
    }

    #[test]
    fn test_store_status_quarantine_serialization() {
        let status = DesktopStoreStatus {
            available: true,
            authority_expected: true,
            schema_version: 1,
            quarantined: false,
            quarantine_reason: None,
            unavailable_reason: None,
        };

        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains(r#""available":true"#));
        assert!(json.contains(r#""authorityExpected":true"#));
        assert!(json.contains(r#""schemaVersion":1"#));
        assert!(json.contains(r#""quarantined":false"#));
    }
}
