// SPDX-License-Identifier: AGPL-3.0-or-later

#![allow(dead_code)]

pub const DXGI_FORMAT_R16G16B16A16_FLOAT: u32 = 10;
pub const DXGI_FORMAT_R16G16B16A16_UNORM: u32 = 11;
pub const DXGI_FORMAT_R10G10B10A2_UNORM: u32 = 24;
pub const DXGI_FORMAT_R8G8B8A8_UNORM: u32 = 28;
pub const DXGI_FORMAT_R8G8B8A8_UNORM_SRGB: u32 = 29;
pub const DXGI_FORMAT_B8G8R8A8_UNORM: u32 = 87;
pub const DXGI_FORMAT_B8G8R8X8_UNORM: u32 = 88;
pub const DXGI_FORMAT_R10G10B10_XR_BIAS_A2_UNORM: u32 = 89;
pub const DXGI_FORMAT_B8G8R8A8_UNORM_SRGB: u32 = 91;
pub const DXGI_FORMAT_B8G8R8X8_UNORM_SRGB: u32 = 93;
pub const DXGI_FORMAT_P010: u32 = 104;
pub const DXGI_FORMAT_P016: u32 = 105;
pub const DXGI_FORMAT_420_OPAQUE: u32 = 106;
pub const DXGI_FORMAT_YUY2: u32 = 107;
pub const DXGI_FORMAT_R32G32B32A32_FLOAT: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFormat {
    Bgra8,
    Rgba8,
    R10G10B10A2 { hdr: bool },
    Rgba16Float { hdr: bool },
}

impl SourceFormat {
    pub fn classify(dxgi_format: u32, capture_flags: u32) -> Option<SourceFormat> {
        let hdr = capture_flags & crate::game_capture_abi::GAME_CAPTURE_FLAG_HDR != 0;
        match dxgi_format {
            DXGI_FORMAT_B8G8R8A8_UNORM
            | DXGI_FORMAT_B8G8R8A8_UNORM_SRGB
            | DXGI_FORMAT_B8G8R8X8_UNORM
            | DXGI_FORMAT_B8G8R8X8_UNORM_SRGB => Some(SourceFormat::Bgra8),
            DXGI_FORMAT_R8G8B8A8_UNORM | DXGI_FORMAT_R8G8B8A8_UNORM_SRGB => {
                Some(SourceFormat::Rgba8)
            }
            DXGI_FORMAT_R10G10B10A2_UNORM => Some(SourceFormat::R10G10B10A2 { hdr }),
            DXGI_FORMAT_R16G16B16A16_FLOAT => Some(SourceFormat::Rgba16Float { hdr }),
            _ => None,
        }
    }

    pub fn is_8bit(self) -> bool {
        matches!(self, SourceFormat::Bgra8 | SourceFormat::Rgba8)
    }

    pub fn is_hdr(self) -> bool {
        matches!(
            self,
            SourceFormat::R10G10B10A2 { hdr: true } | SourceFormat::Rgba16Float { hdr: true }
        )
    }

    pub fn bytes_per_pixel(self) -> usize {
        match self {
            SourceFormat::Bgra8 | SourceFormat::Rgba8 | SourceFormat::R10G10B10A2 { .. } => 4,
            SourceFormat::Rgba16Float { .. } => 8,
        }
    }
}

pub fn r10g10b10a2_row_to_bgra(src_row: &[u8], width: usize, dst_row: &mut [u8], hdr: bool) {
    for x in 0..width {
        let so = x * 4;
        let dofs = x * 4;
        if so + 4 > src_row.len() || dofs + 4 > dst_row.len() {
            break;
        }
        let packed = u32::from_le_bytes([
            src_row[so],
            src_row[so + 1],
            src_row[so + 2],
            src_row[so + 3],
        ]);
        let r10 = (packed & 0x3FF) as u16;
        let g10 = ((packed >> 10) & 0x3FF) as u16;
        let b10 = ((packed >> 20) & 0x3FF) as u16;
        let a2 = ((packed >> 30) & 0x3) as u8;
        let (r, g, b) = if hdr {
            tonemap_rec2020_pq_to_srgb8(r10, g10, b10)
        } else {
            (scale10_to_8(r10), scale10_to_8(g10), scale10_to_8(b10))
        };
        dst_row[dofs] = b;
        dst_row[dofs + 1] = g;
        dst_row[dofs + 2] = r;
        dst_row[dofs + 3] = (a2 as u16 * 255 / 3) as u8;
    }
}

pub fn rgba16f_row_to_bgra(src_row: &[u8], width: usize, dst_row: &mut [u8], hdr: bool) {
    for x in 0..width {
        let so = x * 8;
        let dofs = x * 4;
        if so + 8 > src_row.len() || dofs + 4 > dst_row.len() {
            break;
        }
        let r = f16_to_f32(u16::from_le_bytes([src_row[so], src_row[so + 1]]));
        let g = f16_to_f32(u16::from_le_bytes([src_row[so + 2], src_row[so + 3]]));
        let b = f16_to_f32(u16::from_le_bytes([src_row[so + 4], src_row[so + 5]]));
        let a = f16_to_f32(u16::from_le_bytes([src_row[so + 6], src_row[so + 7]]));
        let (lr, lg, lb) = if hdr {
            let [r, g, b] = tone_map_scrgb_to_sdr([r, g, b], 1.0);
            (r, g, b)
        } else {
            (r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0))
        };
        dst_row[dofs] = linear_to_srgb8(lb);
        dst_row[dofs + 1] = linear_to_srgb8(lg);
        dst_row[dofs + 2] = linear_to_srgb8(lr);
        dst_row[dofs + 3] = (a.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    }
}

fn scale10_to_8(v10: u16) -> u8 {
    ((v10 as u32 * 255 + 511) / 1023) as u8
}

pub fn tone_map_scrgb_to_sdr(rgb: [f32; 3], source_white: f32) -> [f32; 3] {
    tone_map_scrgb_to_sdr_with_highlights(rgb, source_white, 0.0)
}

pub fn tone_map_scrgb_to_sdr_with_highlights(
    rgb: [f32; 3],
    source_white: f32,
    highlight_strength: f32,
) -> [f32; 3] {
    let source_white = if source_white.is_finite() && source_white > 0.0 {
        source_white
    } else {
        1.0
    };
    let rgb = rgb.map(|channel| if channel.is_finite() { channel } else { 0.0 });
    let peak = rgb[0].max(rgb[1]).max(rgb[2]).max(0.0);
    if peak <= 0.0 {
        return [0.0; 3];
    }

    // Only HDR-containing frames need headroom. SDR-only reshares retain identity.
    // ponytail: fixed 0.75 linear knee; content-aware exposure is deferred until
    // scene brightness changes show this shoulder needs temporal adaptation.
    let base_scale = 1.0 / source_white.max(peak);
    let strength = if highlight_strength.is_finite() {
        highlight_strength.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let scale = if strength > 0.0 && peak > 0.75 * source_white {
        let mapped_peak = 0.75 + 0.25 * (peak - 0.75 * source_white) / (peak - 0.5 * source_white);
        base_scale + (mapped_peak / peak - base_scale) * strength
    } else {
        base_scale
    };
    compress_rec709_to_sdr_gamut(rgb.map(|channel| channel * scale))
}

fn compress_rec709_to_sdr_gamut(rgb: [f32; 3]) -> [f32; 3] {
    if rgb.iter().all(|channel| (0.0..=1.0).contains(channel)) {
        return rgb;
    }

    let luma = (0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]).clamp(0.0, 1.0);
    let chroma = rgb.map(|channel| channel - luma);
    let bound = |delta: f32| {
        if delta < 0.0 {
            luma / -delta
        } else if delta > 0.0 {
            (1.0 - luma) / delta
        } else {
            1.0
        }
    };
    let amount = bound(chroma[0])
        .min(bound(chroma[1]))
        .min(bound(chroma[2]))
        .clamp(0.0, 1.0);
    chroma.map(|channel| (luma + channel * amount).clamp(0.0, 1.0))
}

fn tonemap_rec2020_pq_to_srgb8(r10: u16, g10: u16, b10: u16) -> (u8, u8, u8) {
    let r = pq_eotf(r10 as f32 / 1023.0) * (10000.0 / 80.0);
    let g = pq_eotf(g10 as f32 / 1023.0) * (10000.0 / 80.0);
    let b = pq_eotf(b10 as f32 / 1023.0) * (10000.0 / 80.0);
    let (r, g, b) = (
        1.660_491_0 * r - 0.587_641_1 * g - 0.072_849_9 * b,
        -0.124_550_5 * r + 1.132_899_9 * g - 0.008_349_4 * b,
        -0.018_150_8 * r - 0.100_578_9 * g + 1.118_729_7 * b,
    );
    let [r, g, b] = tone_map_scrgb_to_sdr([r, g, b], 1.0);
    (linear_to_srgb8(r), linear_to_srgb8(g), linear_to_srgb8(b))
}

fn pq_eotf(e: f32) -> f32 {
    const M1: f64 = 0.1593017578125;
    const M2: f64 = 78.84375;
    const C1: f64 = 0.8359375;
    const C2: f64 = 18.8515625;
    const C3: f64 = 18.6875;
    let e = (e.clamp(0.0, 1.0)) as f64;
    let ep = e.powf(1.0 / M2);
    let num = (ep - C1).max(0.0);
    let den = C2 - C3 * ep;
    if den <= 0.0 {
        return 0.0;
    }
    (num / den).powf(1.0 / M1) as f32
}

fn linear_to_srgb8(linear: f32) -> u8 {
    let l = linear.clamp(0.0, 1.0);
    let srgb = if l <= 0.0031308 {
        l * 12.92
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    };
    (srgb.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

pub fn f16_to_f32(h: u16) -> f32 {
    let sign = (h >> 15) & 0x1;
    let exp = (h >> 10) & 0x1F;
    let mant = h & 0x3FF;
    let sign_f = if sign == 1 { -1.0f32 } else { 1.0f32 };
    if exp == 0 {
        sign_f * (mant as f32) * 2f32.powi(-24)
    } else if exp == 0x1F {
        if mant == 0 { sign_f * 65504.0 } else { 0.0 }
    } else {
        sign_f * (1.0 + (mant as f32) / 1024.0) * 2f32.powi(exp as i32 - 15)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_capture_abi::{GAME_CAPTURE_FLAG_HDR, GAME_CAPTURE_FLAG_TEN_BIT};

    #[test]
    fn classifies_8bit_formats() {
        assert_eq!(
            SourceFormat::classify(DXGI_FORMAT_B8G8R8A8_UNORM, 0),
            Some(SourceFormat::Bgra8)
        );
        assert_eq!(
            SourceFormat::classify(DXGI_FORMAT_R8G8B8A8_UNORM, 0),
            Some(SourceFormat::Rgba8)
        );
        assert!(
            SourceFormat::classify(DXGI_FORMAT_B8G8R8A8_UNORM, 0)
                .unwrap()
                .is_8bit()
        );
    }

    #[test]
    fn classifies_obs_8bit_alias_set() {
        for format in [
            DXGI_FORMAT_B8G8R8A8_UNORM,
            DXGI_FORMAT_B8G8R8A8_UNORM_SRGB,
            DXGI_FORMAT_B8G8R8X8_UNORM,
            DXGI_FORMAT_B8G8R8X8_UNORM_SRGB,
        ] {
            assert_eq!(SourceFormat::classify(format, 0), Some(SourceFormat::Bgra8));
            assert_eq!(
                SourceFormat::classify(format, GAME_CAPTURE_FLAG_HDR),
                Some(SourceFormat::Bgra8),
                "8-bit format {format} must not become HDR just because the flag is set"
            );
        }

        for format in [DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM_SRGB] {
            assert_eq!(SourceFormat::classify(format, 0), Some(SourceFormat::Rgba8));
            assert_eq!(
                SourceFormat::classify(format, GAME_CAPTURE_FLAG_HDR),
                Some(SourceFormat::Rgba8),
                "8-bit format {format} must not become HDR just because the flag is set"
            );
        }
    }

    #[test]
    fn classifies_10bit_and_hdr_flag() {
        assert_eq!(
            SourceFormat::classify(DXGI_FORMAT_R10G10B10A2_UNORM, GAME_CAPTURE_FLAG_TEN_BIT),
            Some(SourceFormat::R10G10B10A2 { hdr: false })
        );
        assert_eq!(
            SourceFormat::classify(
                DXGI_FORMAT_R10G10B10A2_UNORM,
                GAME_CAPTURE_FLAG_TEN_BIT | GAME_CAPTURE_FLAG_HDR
            ),
            Some(SourceFormat::R10G10B10A2 { hdr: true })
        );
        assert!(
            SourceFormat::classify(DXGI_FORMAT_R10G10B10A2_UNORM, GAME_CAPTURE_FLAG_HDR)
                .unwrap()
                .is_hdr()
        );
    }

    #[test]
    fn high_precision_formats_have_expected_byte_widths() {
        assert_eq!(SourceFormat::Bgra8.bytes_per_pixel(), 4);
        assert_eq!(SourceFormat::Rgba8.bytes_per_pixel(), 4);
        assert_eq!(
            SourceFormat::R10G10B10A2 { hdr: false }.bytes_per_pixel(),
            4
        );
        assert_eq!(SourceFormat::R10G10B10A2 { hdr: true }.bytes_per_pixel(), 4);
        assert_eq!(
            SourceFormat::Rgba16Float { hdr: false }.bytes_per_pixel(),
            8
        );
        assert_eq!(SourceFormat::Rgba16Float { hdr: true }.bytes_per_pixel(), 8);
    }

    #[test]
    fn classifies_fp16_scrgb() {
        assert_eq!(
            SourceFormat::classify(DXGI_FORMAT_R16G16B16A16_FLOAT, GAME_CAPTURE_FLAG_HDR),
            Some(SourceFormat::Rgba16Float { hdr: true })
        );
        assert_eq!(
            SourceFormat::classify(DXGI_FORMAT_R16G16B16A16_FLOAT, 0),
            Some(SourceFormat::Rgba16Float { hdr: false })
        );
        assert_eq!(
            SourceFormat::Rgba16Float { hdr: false }.bytes_per_pixel(),
            8
        );
    }

    #[test]
    fn unsupported_format_is_none() {
        assert_eq!(SourceFormat::classify(104, 0), None);
        assert_eq!(SourceFormat::classify(0, 0), None);
    }

    #[test]
    fn video_and_opaque_formats_stay_unsupported_even_with_hdr_flags() {
        for format in [
            DXGI_FORMAT_P010,
            DXGI_FORMAT_P016,
            DXGI_FORMAT_420_OPAQUE,
            DXGI_FORMAT_YUY2,
        ] {
            assert_eq!(
                SourceFormat::classify(format, GAME_CAPTURE_FLAG_TEN_BIT | GAME_CAPTURE_FLAG_HDR),
                None,
                "DXGI video/opaque format {format} is not a directly readable game backbuffer"
            );
        }
    }

    #[test]
    fn obs_high_precision_formats_without_reader_conversion_stay_unsupported() {
        assert_eq!(
            SourceFormat::classify(DXGI_FORMAT_R16G16B16A16_UNORM, GAME_CAPTURE_FLAG_HDR),
            None,
            "OBS maps RGBA16 UNORM, but Fluxer needs a row converter before claiming support"
        );
        assert_eq!(
            SourceFormat::classify(DXGI_FORMAT_R32G32B32A32_FLOAT, GAME_CAPTURE_FLAG_HDR),
            None,
            "OBS maps RGBA32F, but Fluxer has no reader-side conversion for it yet"
        );
        assert_eq!(
            SourceFormat::classify(
                DXGI_FORMAT_R10G10B10_XR_BIAS_A2_UNORM,
                GAME_CAPTURE_FLAG_TEN_BIT | GAME_CAPTURE_FLAG_HDR
            ),
            None,
            "XR-bias 10A2 is not equivalent to regular R10G10B10A2 UNORM"
        );
    }

    #[test]
    fn f16_round_trips_known_values() {
        assert_eq!(f16_to_f32(0x0000), 0.0);
        assert_eq!(f16_to_f32(0x3C00), 1.0);
        assert_eq!(f16_to_f32(0x4000), 2.0);
        assert_eq!(f16_to_f32(0x3800), 0.5);
        assert!((f16_to_f32(0xC000) + 2.0).abs() < 1e-6);
    }

    #[test]
    fn f16_special_values_are_bounded_for_sdr_fallback() {
        assert_eq!(f16_to_f32(0x7C00), 65504.0);
        assert_eq!(f16_to_f32(0xFC00), -65504.0);
        assert_eq!(f16_to_f32(0x7E00), 0.0);
    }

    #[test]
    fn r10_sdr_unpack_scales_extremes() {
        let packed: u32 = 0x3FF | (0x3FF << 10) | (0x3FF << 20) | (0x3 << 30);
        let src = packed.to_le_bytes();
        let mut dst = [0u8; 4];
        r10g10b10a2_row_to_bgra(&src, 1, &mut dst, false);
        assert_eq!(dst, [255, 255, 255, 255]);

        let src0 = 0u32.to_le_bytes();
        let mut dst0 = [0u8; 4];
        r10g10b10a2_row_to_bgra(&src0, 1, &mut dst0, false);
        assert_eq!(dst0, [0, 0, 0, 0]);
    }

    #[test]
    fn r10_sdr_unpack_scales_midpoints_and_alpha() {
        let packed: u32 = 0x200 | (0x100 << 10) | (0x080 << 20) | (0x2 << 30);
        let src = packed.to_le_bytes();
        let mut dst = [0u8; 4];
        r10g10b10a2_row_to_bgra(&src, 1, &mut dst, false);
        assert!((31..=33).contains(&dst[0]), "B was {}", dst[0]);
        assert!((63..=65).contains(&dst[1]), "G was {}", dst[1]);
        assert!((127..=129).contains(&dst[2]), "R was {}", dst[2]);
        assert_eq!(dst[3], 170);
    }

    #[test]
    fn r10_unpack_respects_short_rows() {
        let src = [0xFF, 0xFF, 0xFF];
        let mut dst = [9u8; 8];
        r10g10b10a2_row_to_bgra(&src, 2, &mut dst, false);
        assert_eq!(dst, [9u8; 8]);
    }

    #[test]
    fn r10_unpack_channel_order_is_bgra() {
        let packed: u32 = 0x3FF;
        let src = packed.to_le_bytes();
        let mut dst = [0u8; 4];
        r10g10b10a2_row_to_bgra(&src, 1, &mut dst, false);
        assert_eq!(dst[0], 0, "B should be 0");
        assert_eq!(dst[1], 0, "G should be 0");
        assert_eq!(dst[2], 255, "R should be 255");
    }

    #[test]
    fn r10_hdr_unpack_is_bounded_and_nonzero() {
        let packed: u32 = 0x3FF | (0x200 << 10) | (0x100 << 20) | (0x3 << 30);
        let src = packed.to_le_bytes();
        let mut dst = [0u8; 4];
        r10g10b10a2_row_to_bgra(&src, 1, &mut dst, true);
        assert!(dst[2] > 0, "bright red PQ should map to a visible value");
    }

    #[test]
    fn r10_hdr_unpack_is_monotonic_after_tonemap() {
        let pack = |v: u32| -> [u8; 4] { (v | (v << 10) | (v << 20) | (0x3 << 30)).to_le_bytes() };
        let mut low = [0u8; 4];
        let mut mid = [0u8; 4];
        let mut high = [0u8; 4];
        r10g10b10a2_row_to_bgra(&pack(0x100), 1, &mut low, true);
        r10g10b10a2_row_to_bgra(&pack(0x200), 1, &mut mid, true);
        r10g10b10a2_row_to_bgra(&pack(0x3FF), 1, &mut high, true);
        assert!(low[2] <= mid[2] && mid[2] <= high[2]);
        assert_eq!(high[3], 255);
    }

    #[test]
    fn hdr_frame_keeps_shadow_values_and_separates_neutral_highlights() {
        let shadow = tone_map_scrgb_to_sdr_with_highlights([0.3; 3], 3.0, 1.0);
        assert!((shadow[0] - 0.1).abs() < 1e-6);
        let values = [3.0, 4.0, 6.0, 12.0]
            .map(|x| tone_map_scrgb_to_sdr_with_highlights([x; 3], 3.0, 1.0)[0]);
        assert!(values.windows(2).all(|p| p[0] < p[1]));
        assert!(values[0] < 1.0 && values[3] < 1.0);
        let halfway_white = tone_map_scrgb_to_sdr_with_highlights([3.0; 3], 3.0, 0.5);
        assert!((halfway_white[0] - 0.9375).abs() < 1e-6);
        for peak in [0.8, 1.0] {
            let result = tone_map_scrgb_to_sdr_with_highlights([peak * 3.0; 3], 3.0, 0.0);
            assert!(result.iter().all(|value| (value - peak).abs() < 1e-6));
        }
        let color = tone_map_scrgb_to_sdr_with_highlights([12.0, 6.0, 3.0], 3.0, 1.0);
        assert!((color[0] / color[1] - 2.0).abs() < 1e-6);
        assert!((color[1] / color[2] - 2.0).abs() < 1e-6);
        let reshared = tone_map_scrgb_to_sdr(color.map(|x| x * 3.0), 3.0);
        for (actual, expected) in reshared.into_iter().zip(color) {
            assert!((actual - expected).abs() < 1e-6);
        }
    }

    #[test]
    fn sdr_content_on_hdr_preserves_its_linear_values_across_reshares() {
        let original = [0.01_f32, 0.18, 0.7];
        let mut current = original;
        for source_white in [1.0, 3.0, 4.0, 3.0, 1.0] {
            current =
                tone_map_scrgb_to_sdr(current.map(|channel| channel * source_white), source_white);
            for (actual, expected) in current.into_iter().zip(original) {
                assert!(
                    (actual - expected).abs() < 1e-6,
                    "SDR content must retain its value after HDR composition and resharing: {actual} != {expected}"
                );
            }
        }
    }

    #[test]
    fn scrgb_tonemap_keeps_reference_white_and_compresses_highlights() {
        let white = tone_map_scrgb_to_sdr([1.0; 3], 1.0);
        let highlight = tone_map_scrgb_to_sdr([12.5; 3], 1.0);
        let extreme = tone_map_scrgb_to_sdr([125.0; 3], 1.0);
        assert!(white.iter().all(|channel| (*channel - 1.0).abs() < 1e-6));
        assert_eq!(highlight, white);
        assert_eq!(extreme, white);
    }

    #[test]
    fn scrgb_sdr_white_at_three_reference_units_maps_to_sdr_white() {
        let white = tone_map_scrgb_to_sdr([3.0; 3], 3.0);
        assert!(white.iter().all(|channel| (*channel - 1.0).abs() < 1e-6));
        let color = tone_map_scrgb_to_sdr([1.5, 0.75, 0.375], 3.0);
        let expected = tone_map_scrgb_to_sdr([0.5, 0.25, 0.125], 1.0);
        for (actual, expected) in color.into_iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-6);
        }
    }

    #[test]
    fn scrgb_tonemap_preserves_highlight_chroma() {
        let mapped = tone_map_scrgb_to_sdr([12.5, 6.25, 3.125], 1.0);
        assert!(mapped[0] <= 1.0);
        assert!((mapped[0] / mapped[1] - 2.0).abs() < 1e-6);
        assert!((mapped[1] / mapped[2] - 2.0).abs() < 1e-6);
    }

    #[test]
    fn scrgb_tonemap_compresses_negative_gamut_without_changing_luma() {
        let source = [-0.5_f32, 1.0, 0.0];
        let mapped = tone_map_scrgb_to_sdr(source, 1.0);
        let luma = |rgb: [f32; 3]| 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
        let expected_luma = luma(source);
        assert!(mapped.iter().all(|channel| (0.0..=1.0).contains(channel)));
        assert!((luma(mapped) - expected_luma).abs() < 1e-6);
    }

    #[test]
    fn fp16_sdr_clamps_and_encodes_srgb() {
        let mut src = [0u8; 8];
        src[0..2].copy_from_slice(&0x3C00u16.to_le_bytes());
        src[6..8].copy_from_slice(&0x3C00u16.to_le_bytes());
        let mut dst = [0u8; 4];
        rgba16f_row_to_bgra(&src, 1, &mut dst, false);
        assert_eq!(dst[0], 0, "B");
        assert_eq!(dst[1], 0, "G");
        assert_eq!(dst[2], 255, "R linear 1.0 -> sRGB 255");
        assert_eq!(dst[3], 255, "A");
    }

    #[test]
    fn fp16_sdr_clamps_negative_and_above_one_channels() {
        let mut src = [0u8; 8];
        src[0..2].copy_from_slice(&0xBC00u16.to_le_bytes());
        src[2..4].copy_from_slice(&0x4000u16.to_le_bytes());
        src[4..6].copy_from_slice(&0x3800u16.to_le_bytes());
        src[6..8].copy_from_slice(&0x4000u16.to_le_bytes());
        let mut dst = [0u8; 4];
        rgba16f_row_to_bgra(&src, 1, &mut dst, false);
        assert!((180..=196).contains(&dst[0]), "B linear 0.5 was {}", dst[0]);
        assert_eq!(dst[1], 255, "G linear 2.0 clamps to white");
        assert_eq!(dst[2], 0, "R negative clamps to black");
        assert_eq!(dst[3], 255, "A above 1.0 clamps opaque");
    }

    #[test]
    fn fp16_unpack_respects_short_rows() {
        let src = [0x00, 0x3C, 0x00, 0x3C];
        let mut dst = [7u8; 8];
        rgba16f_row_to_bgra(&src, 2, &mut dst, false);
        assert_eq!(dst, [7u8; 8]);
    }

    #[test]
    fn fp16_hdr_above_one_is_tonemapped_not_clipped_to_garbage() {
        let mut src = [0u8; 8];
        src[0..2].copy_from_slice(&0x4400u16.to_le_bytes());
        src[6..8].copy_from_slice(&0x3C00u16.to_le_bytes());
        let mut dst = [0u8; 4];
        rgba16f_row_to_bgra(&src, 1, &mut dst, true);
        assert!(dst[2] > 100, "HDR highlight should be bright after tonemap");
    }

    #[test]
    fn srgb_encode_endpoints() {
        assert_eq!(linear_to_srgb8(0.0), 0);
        assert_eq!(linear_to_srgb8(1.0), 255);
        let mid = linear_to_srgb8(0.5);
        assert!((180..=196).contains(&mid), "sRGB(0.5) was {mid}");
    }
}
