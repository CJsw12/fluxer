// SPDX-License-Identifier: AGPL-3.0-or-later

use std::sync::atomic::{AtomicI32, Ordering};

static LAST_COUNT: AtomicI32 = AtomicI32::new(-1);

// 배지 아이콘 바이너리 임베드 (1~10: 숫자, 11: 10+ 또는 점)
static BADGES_LIGHT: [&[u8]; 11] = [
    include_bytes!("../../fluxer_desktop/build_resources/icons-canary/badges/badge-1.ico"),
    include_bytes!("../../fluxer_desktop/build_resources/icons-canary/badges/badge-2.ico"),
    include_bytes!("../../fluxer_desktop/build_resources/icons-canary/badges/badge-3.ico"),
    include_bytes!("../../fluxer_desktop/build_resources/icons-canary/badges/badge-4.ico"),
    include_bytes!("../../fluxer_desktop/build_resources/icons-canary/badges/badge-5.ico"),
    include_bytes!("../../fluxer_desktop/build_resources/icons-canary/badges/badge-6.ico"),
    include_bytes!("../../fluxer_desktop/build_resources/icons-canary/badges/badge-7.ico"),
    include_bytes!("../../fluxer_desktop/build_resources/icons-canary/badges/badge-8.ico"),
    include_bytes!("../../fluxer_desktop/build_resources/icons-canary/badges/badge-9.ico"),
    include_bytes!("../../fluxer_desktop/build_resources/icons-canary/badges/badge-10.ico"),
    include_bytes!("../../fluxer_desktop/build_resources/icons-canary/badges/badge-11.ico"),
];

static BADGES_DARK: [&[u8]; 11] = [
    include_bytes!("../../fluxer_desktop/build_resources/icons-canary/badges/badge-dark-1.ico"),
    include_bytes!("../../fluxer_desktop/build_resources/icons-canary/badges/badge-dark-2.ico"),
    include_bytes!("../../fluxer_desktop/build_resources/icons-canary/badges/badge-dark-3.ico"),
    include_bytes!("../../fluxer_desktop/build_resources/icons-canary/badges/badge-dark-4.ico"),
    include_bytes!("../../fluxer_desktop/build_resources/icons-canary/badges/badge-dark-5.ico"),
    include_bytes!("../../fluxer_desktop/build_resources/icons-canary/badges/badge-dark-6.ico"),
    include_bytes!("../../fluxer_desktop/build_resources/icons-canary/badges/badge-dark-7.ico"),
    include_bytes!("../../fluxer_desktop/build_resources/icons-canary/badges/badge-dark-8.ico"),
    include_bytes!("../../fluxer_desktop/build_resources/icons-canary/badges/badge-dark-9.ico"),
    include_bytes!("../../fluxer_desktop/build_resources/icons-canary/badges/badge-dark-10.ico"),
    include_bytes!("../../fluxer_desktop/build_resources/icons-canary/badges/badge-dark-11.ico"),
];

/// ICO 바이너리 버퍼에서 가장 적합한 16x16 PNG/BMP 아이콘 리소스 슬라이스를 추출합니다.
pub fn extract_icon_frame(data: &[u8]) -> Option<&[u8]> {
    if data.len() < 6 || u16::from_le_bytes([data[2], data[3]]) != 1 {
        return None;
    }
    let count = u16::from_le_bytes([data[4], data[5]]) as usize;
    let mut best_offset = 0;
    let mut best_length = 0;

    for i in 0..count {
        let entry = 6 + i * 16;
        if entry + 16 > data.len() {
            break;
        }
        let length = u32::from_le_bytes([
            data[entry + 8],
            data[entry + 9],
            data[entry + 10],
            data[entry + 11],
        ]) as usize;
        let offset = u32::from_le_bytes([
            data[entry + 12],
            data[entry + 13],
            data[entry + 14],
            data[entry + 15],
        ]) as usize;

        if offset + length <= data.len() {
            best_offset = offset;
            best_length = length;
            // 16x16 우선 선택
            if data[entry] == 16 || data[entry] == 0 {
                break;
            }
        }
    }

    if best_length > 0 {
        Some(&data[best_offset..best_offset + best_length])
    } else {
        None
    }
}

/// Windows 작업표시줄 오버레이 뱃지를 설정합니다.
pub fn set_taskbar_badge(hwnd: isize, count: i32, is_dark: bool) {
    if hwnd == 0 {
        return;
    }

    let prev_count = LAST_COUNT.swap(count, Ordering::Relaxed);
    if prev_count == count {
        return;
    }

    #[cfg(windows)]
    {
        if count <= 0 {
            let _ = fluxer_win_shell::set_taskbar_overlay_icon_sync(hwnd, None, "");
            return;
        }

        let index = if count > 10 {
            10
        } else {
            (count - 1).clamp(0, 10) as usize
        };

        let badge_data = if is_dark {
            BADGES_DARK[index]
        } else {
            BADGES_LIGHT[index]
        };

        if let Some(frame) = extract_icon_frame(badge_data) {
            let desc = format!("{count} unread notifications");
            let _ = fluxer_win_shell::set_taskbar_overlay_icon_sync(hwnd, Some(frame), &desc);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_icon_frame_validity() {
        for (i, ico) in BADGES_LIGHT.iter().enumerate() {
            let frame = extract_icon_frame(ico);
            assert!(frame.is_some(), "Light badge {} frame extraction failed", i + 1);
            let bytes = frame.unwrap();
            assert!(!bytes.is_empty());
        }

        for (i, ico) in BADGES_DARK.iter().enumerate() {
            let frame = extract_icon_frame(ico);
            assert!(frame.is_some(), "Dark badge {} frame extraction failed", i + 1);
            let bytes = frame.unwrap();
            assert!(!bytes.is_empty());
        }
    }

    #[test]
    fn test_set_taskbar_badge_zero_hwnd() {
        set_taskbar_badge(0, 5, false);
    }
}
