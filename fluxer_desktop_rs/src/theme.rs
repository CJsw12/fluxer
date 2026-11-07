// SPDX-License-Identifier: AGPL-3.0-or-later

#![allow(dead_code)]

//! Fluxer 공식 디자인 시스템 (ColorSystemPalette & Typography) 1:1 역이식 모듈입니다.
//! 원본 `fluxer_app/src/features/theme/utils/ColorSystemPalette.ts`의
//! 모든 색상 토큰과 레이아웃 치수를 GPUI 렌더러와 100% 일치하도록 정량화합니다.

use gpui::Rgba;

/// RGB 16진수 코드를 컴파일 타임에 GPUI Rgba 구조체로 변환합니다.
const fn rgb(hex: u32) -> Rgba {
    let r = ((hex >> 16) & 0xFF) as f32 / 255.0;
    let g = ((hex >> 8) & 0xFF) as f32 / 255.0;
    let b = (hex & 0xFF) as f32 / 255.0;
    Rgba { r, g, b, a: 1.0 }
}

/// RGBA 16진수 코드를 컴파일 타임에 GPUI Rgba 구조체로 변환합니다.
const fn rgba(hex: u32) -> Rgba {
    let r = ((hex >> 24) & 0xFF) as f32 / 255.0;
    let g = ((hex >> 16) & 0xFF) as f32 / 255.0;
    let b = ((hex >> 8) & 0xFF) as f32 / 255.0;
    let a = (hex & 0xFF) as f32 / 255.0;
    Rgba { r, g, b, a }
}

// ==========================================
// 1. 레이아웃 규격 치수 (Layout Dimensions)
// ==========================================

pub const GUILD_RAIL_WIDTH: f32 = 72.0;
pub const CHANNEL_SIDEBAR_WIDTH: f32 = 240.0;
pub const HEADER_HEIGHT: f32 = 48.0;
pub const PROFILE_DOCK_HEIGHT: f32 = 52.0;
pub const MEMBER_LIST_WIDTH: f32 = 240.0;
pub const INPUT_BAR_HEIGHT: f32 = 44.0;

// ==========================================
// 2. Fluxer 다크 테마 표면 색상 (Surface Colors)
// ==========================================

/// 최좌측 서버(길드) 목록 레일 배경색 (`--background-tertiary`)
pub const BG_TERTIARY: Rgba = rgb(0x1e1f22);

/// 채널 목록 및 멤버 목록 사이드바 배경색 (`--background-secondary`)
pub const BG_SECONDARY: Rgba = rgb(0x2b2d31);

/// 메인 채팅 뷰포트 배경색 (`--background-primary`)
pub const BG_PRIMARY: Rgba = rgb(0x313338);

/// 하단 사용자 프로필 미니 독 배경색 (`--background-secondary-alt`)
pub const BG_SECONDARY_ALT: Rgba = rgb(0x232428);

/// 상단 채널 헤더 바 배경색 (`--background-channel-header`)
pub const BG_CHANNEL_HEADER: Rgba = rgb(0x313338);

/// 활성화(선택)된 채널 항목 배경색
pub const BG_CHANNEL_SELECTED: Rgba = rgb(0x35373c);

/// 채널 항목 마우스 호버 배경색
pub const BG_CHANNEL_HOVER: Rgba = rgb(0x2e3035);

/// 채팅 메시지 입력창 배경색 (`--background-textarea`)
pub const BG_TEXTAREA: Rgba = rgb(0x383a40);

/// 메시지 행 마우스 호버 시 배경색
pub const BG_MESSAGE_HOVER: Rgba = rgb(0x2e3035);

/// 팝오버 및 툴팁 배경색
pub const BG_FLOATING: Rgba = rgb(0x111214);

// ==========================================
// 3. 브랜드 및 액센트 색상 (Brand & Accents)
// ==========================================

/// Fluxer 시그니처 브랜드 강조색 (`--brand`)
pub const BRAND_ACCENT: Rgba = rgb(0x5865f2);

/// 브랜드 강조색 마우스 호버 상태
pub const BRAND_ACCENT_HOVER: Rgba = rgb(0x4752c4);

/// 링크 텍스트 색상 (`--text-link`)
pub const TEXT_LINK: Rgba = rgb(0x00a8fc);

/// 구분선 및 테두리 색상 (`--border-color`)
pub const BORDER_DIVIDER: Rgba = rgb(0x35363c);

/// 반투명 테두리 색상
pub const BORDER_SUBTLE: Rgba = rgba(0x35363c88);

// ==========================================
// 4. 타이포그래피 텍스트 색상 (Typography Colors)
// ==========================================

/// 제목, 닉네임, 강조 텍스트 (`--text-normal` / `--header-primary`)
pub const TEXT_PRIMARY: Rgba = rgb(0xf2f3f5);

/// 일반 본문 텍스트 (`--text-secondary`)
pub const TEXT_BODY: Rgba = rgb(0xdbdee1);

/// 채널 아이콘, 타임스탬프, 보조 설명 (`--text-muted`)
pub const TEXT_MUTED: Rgba = rgb(0x949ba4);

/// 입력창 플레이스홀더 및 비활성 텍스트
pub const TEXT_PLACEHOLDER: Rgba = rgb(0x80848e);

/// 봇(BOT) 배지 텍스트
pub const TEXT_BOT_BADGE: Rgba = rgb(0xffffff);

// ==========================================
// 5. 온라인 상태 인디케이터 (Status Indicators)
// ==========================================

/// 온라인 상태 (초록색)
pub const STATUS_ONLINE: Rgba = rgb(0x23a55a);

/// 자리 비움 상태 (노란색)
pub const STATUS_IDLE: Rgba = rgb(0xf0b232);

/// 방해 금지 상태 (빨간색)
pub const STATUS_DND: Rgba = rgb(0xf23f43);

/// 오프라인 상태 (회색)
pub const STATUS_OFFLINE: Rgba = rgb(0x80848e);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme_colors_constants() {
        assert_eq!(BG_PRIMARY, rgb(0x313338));
        assert_eq!(BG_SECONDARY, rgb(0x2b2d31));
        assert_eq!(BG_TERTIARY, rgb(0x1e1f22));
        assert_eq!(BRAND_ACCENT, rgb(0x5865f2));
        assert_eq!(STATUS_ONLINE, rgb(0x23a55a));
    }
}
