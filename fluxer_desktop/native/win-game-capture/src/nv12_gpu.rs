// SPDX-License-Identifier: AGPL-3.0-or-later

use windows::Win32::Foundation::FreeLibrary;
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE,
    D3D11_BIND_UNORDERED_ACCESS, D3D11_BUFFER_DESC, D3D11_CPU_ACCESS_READ, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_RESOURCE_MISC_SHARED, D3D11_SUBRESOURCE_DATA,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING,
    D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE, D3D11_VIDEO_PROCESSOR_CONTENT_DESC,
    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0,
    D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0,
    D3D11_VIDEO_PROCESSOR_STREAM, D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
    D3D11_VPIV_DIMENSION_TEXTURE2D, D3D11_VPOV_DIMENSION_TEXTURE2D, ID3D11Buffer,
    ID3D11ComputeShader, ID3D11Device, ID3D11DeviceContext, ID3D11ShaderResourceView,
    ID3D11Texture2D, ID3D11UnorderedAccessView, ID3D11VideoContext, ID3D11VideoContext1,
    ID3D11VideoDevice, ID3D11VideoProcessor, ID3D11VideoProcessorEnumerator,
    ID3D11VideoProcessorInputView, ID3D11VideoProcessorOutputView,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709, DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020,
    DXGI_COLOR_SPACE_TYPE, DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709, DXGI_FORMAT_NV12,
    DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R32_UINT, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::IDXGIResource;
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::core::{HRESULT, Interface, PCSTR, w};

use crate::hdr;

fn vlog(msg: &str) {
    if crate::game_capture_abi::env_flag_enabled(crate::game_capture_abi::ENV_VERBOSE) {
        use std::io::Write;
        let _ = writeln!(std::io::stderr(), "[fluxer-nv12] {msg}");
    }
}

pub const NV12_OUTPUT_SLOT_COUNT: usize = 3;

struct Nv12OutputSlot {
    _texture: ID3D11Texture2D,
    view: ID3D11VideoProcessorOutputView,
    handle: u64,
}

pub struct Nv12GpuConverter {
    _video_device: ID3D11VideoDevice,
    video_context: ID3D11VideoContext,
    processor: ID3D11VideoProcessor,
    _enumerator: ID3D11VideoProcessorEnumerator,
    input_view: ID3D11VideoProcessorInputView,
    output_slots: [Nv12OutputSlot; NV12_OUTPUT_SLOT_COUNT],
    tone_map: Option<ScrgbToneMapStage>,
    readback: ID3D11Texture2D,
    slot_cursor: usize,
    context: ID3D11DeviceContext,
    out_width: u32,
    out_height: u32,
}

pub struct Nv12SharedTextureFrame {
    pub handle: u64,
    pub width: u32,
    pub height: u32,
    pub dxgi_format: u32,
}

pub struct CpuFrameConversionTimings {
    pub readback_map: std::time::Duration,
    pub cpu_pack: std::time::Duration,
}

struct ScrgbToneMapStage {
    output: ID3D11Texture2D,
    source_view: ID3D11ShaderResourceView,
    output_view: ID3D11UnorderedAccessView,
    frame_peak_view: ID3D11UnorderedAccessView,
    frame_peak_source_view: ID3D11ShaderResourceView,
    hdr_scan_shader: ID3D11ComputeShader,
    shader: ID3D11ComputeShader,
    constants: ID3D11Buffer,
    width: u32,
    height: u32,
    source_white: f32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ToneMapConstants {
    source_white: f32,
    _padding: [f32; 3],
}

fn normalized_scrgb_sdr_white(source_white: f32) -> f32 {
    if source_white.is_finite() && source_white > 0.0 {
        source_white
    } else {
        1.0
    }
}

impl ToneMapConstants {
    fn for_source_white(source_white: f32) -> Self {
        Self {
            source_white: normalized_scrgb_sdr_white(source_white),
            _padding: [0.0; 3],
        }
    }
}

impl ScrgbToneMapStage {
    fn new(
        device: &ID3D11Device,
        input: &ID3D11Texture2D,
        width: u32,
        height: u32,
        source_white: f32,
    ) -> Result<Self, String> {
        let output_desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS).0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut output = None;
        unsafe { device.CreateTexture2D(&output_desc, None, Some(&mut output)) }
            .map_err(|e| format!("CreateTexture2D scRGB tone-map output: {e}"))?;
        let output = output.ok_or("CreateTexture2D scRGB tone-map output returned null")?;

        let mut source_view = None;
        unsafe { device.CreateShaderResourceView(input, None, Some(&mut source_view)) }
            .map_err(|e| format!("CreateShaderResourceView scRGB input: {e}"))?;
        let source_view =
            source_view.ok_or("CreateShaderResourceView scRGB input returned null")?;

        let mut output_view = None;
        unsafe { device.CreateUnorderedAccessView(&output, None, Some(&mut output_view)) }
            .map_err(|e| format!("CreateUnorderedAccessView scRGB output: {e}"))?;
        let output_view =
            output_view.ok_or("CreateUnorderedAccessView scRGB output returned null")?;

        let flag_desc = D3D11_TEXTURE2D_DESC {
            Width: 1,
            Height: 1,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R32_UINT,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS).0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut frame_peak = None;
        unsafe { device.CreateTexture2D(&flag_desc, None, Some(&mut frame_peak)) }
            .map_err(|e| format!("CreateTexture2D scRGB frame peak: {e}"))?;
        let frame_peak = frame_peak.ok_or("CreateTexture2D scRGB frame peak returned null")?;
        let mut frame_peak_view = None;
        unsafe { device.CreateUnorderedAccessView(&frame_peak, None, Some(&mut frame_peak_view)) }
            .map_err(|e| format!("CreateUnorderedAccessView scRGB frame peak: {e}"))?;
        let frame_peak_view =
            frame_peak_view.ok_or("CreateUnorderedAccessView scRGB frame peak returned null")?;
        let mut frame_peak_source_view = None;
        unsafe {
            device.CreateShaderResourceView(&frame_peak, None, Some(&mut frame_peak_source_view))
        }
        .map_err(|e| format!("CreateShaderResourceView scRGB frame peak: {e}"))?;
        let frame_peak_source_view = frame_peak_source_view
            .ok_or("CreateShaderResourceView scRGB frame peak returned null")?;

        let bytecode = scrgb_tone_map_shader_bytecode()?;
        let mut shader = None;
        unsafe {
            device.CreateComputeShader(
                bytecode,
                None::<&windows::Win32::Graphics::Direct3D11::ID3D11ClassLinkage>,
                Some(&mut shader),
            )
        }
        .map_err(|e| format!("CreateComputeShader scRGB tone map: {e}"))?;
        let shader = shader.ok_or("CreateComputeShader scRGB tone map returned null")?;

        let scan_bytecode = scrgb_hdr_scan_shader_bytecode()?;
        let mut hdr_scan_shader = None;
        unsafe {
            device.CreateComputeShader(
                scan_bytecode,
                None::<&windows::Win32::Graphics::Direct3D11::ID3D11ClassLinkage>,
                Some(&mut hdr_scan_shader),
            )
        }
        .map_err(|e| format!("CreateComputeShader scRGB HDR scan: {e}"))?;
        let hdr_scan_shader =
            hdr_scan_shader.ok_or("CreateComputeShader scRGB HDR scan returned null")?;

        let values = ToneMapConstants::for_source_white(source_white);
        let constants_desc = D3D11_BUFFER_DESC {
            ByteWidth: std::mem::size_of::<ToneMapConstants>() as u32,
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
            StructureByteStride: 0,
        };
        let initial_data = D3D11_SUBRESOURCE_DATA {
            pSysMem: (&values as *const ToneMapConstants).cast(),
            SysMemPitch: 0,
            SysMemSlicePitch: 0,
        };
        let mut constants = None;
        unsafe { device.CreateBuffer(&constants_desc, Some(&initial_data), Some(&mut constants)) }
            .map_err(|e| format!("CreateBuffer scRGB tone-map constants: {e}"))?;
        let constants = constants.ok_or("CreateBuffer scRGB tone-map constants returned null")?;

        Ok(Self {
            output,
            source_view,
            output_view,
            frame_peak_view,
            frame_peak_source_view,
            hdr_scan_shader,
            shader,
            constants,
            width,
            height,
            source_white: values.source_white,
        })
    }

    fn update_source_white(&mut self, context: &ID3D11DeviceContext, source_white: f32) -> bool {
        let values = ToneMapConstants::for_source_white(source_white);
        if self.source_white.to_bits() == values.source_white.to_bits() {
            return false;
        }
        unsafe {
            context.UpdateSubresource(
                &self.constants,
                0,
                None,
                (&values as *const ToneMapConstants).cast(),
                0,
                0,
            );
        }
        self.source_white = values.source_white;
        true
    }

    fn run(&self, context: &ID3D11DeviceContext) {
        let source = [Some(self.source_view.clone())];
        let source_with_peak = [
            Some(self.source_view.clone()),
            Some(self.frame_peak_source_view.clone()),
        ];
        let output = [Some(self.output_view.clone())];
        let no_source: [Option<ID3D11ShaderResourceView>; 2] = [None, None];
        let no_output: [Option<ID3D11UnorderedAccessView>; 1] = [None];
        let no_constants: [Option<ID3D11Buffer>; 1] = [None];
        let frame_peak = [Some(self.frame_peak_view.clone())];
        let no_frame_peak: [Option<ID3D11UnorderedAccessView>; 1] = [None];
        let constants = [Some(self.constants.clone())];
        let clear_peak = [0_u32; 4];
        unsafe {
            context.ClearUnorderedAccessViewUint(&self.frame_peak_view, &clear_peak);
            context.CSSetShader(Some(&self.hdr_scan_shader), None);
            context.CSSetConstantBuffers(0, Some(&constants));
            context.CSSetShaderResources(0, Some(&source));
            context.CSSetUnorderedAccessViews(1, 1, Some(frame_peak.as_ptr()), None);
            context.Dispatch(self.width.div_ceil(16), self.height.div_ceil(16), 1);
            context.CSSetShaderResources(0, Some(&no_source[..1]));
            context.CSSetUnorderedAccessViews(1, 1, Some(no_frame_peak.as_ptr()), None);

            context.CSSetShader(Some(&self.shader), None);
            context.CSSetConstantBuffers(0, Some(&constants));
            context.CSSetShaderResources(0, Some(&source_with_peak));
            context.CSSetUnorderedAccessViews(0, 1, Some(output.as_ptr()), None);
            context.Dispatch(self.width.div_ceil(16), self.height.div_ceil(16), 1);
            context.CSSetShaderResources(0, Some(&no_source));
            context.CSSetUnorderedAccessViews(0, 1, Some(no_output.as_ptr()), None);
            context.CSSetUnorderedAccessViews(1, 1, Some(no_frame_peak.as_ptr()), None);
            context.CSSetConstantBuffers(0, Some(&no_constants));
            context.CSSetShader(None::<&ID3D11ComputeShader>, None);
        }
    }
}

const SCRGB_TONE_MAP_HLSL: &str = r#"
Texture2D<float4> sourceTexture : register(t0);
Texture2D<uint> framePeakTexture : register(t1);
RWTexture2D<float4> outputTexture : register(u0);
RWTexture2D<uint> framePeakOutput : register(u1);

cbuffer ToneMapConstants : register(b0) {
    float sourceWhite;
    float3 _padding;
}

float gamutBound(float neutral, float delta) {
    if (delta < 0.0) return neutral / -delta;
    if (delta > 0.0) return (1.0 - neutral) / delta;
    return 1.0;
}

float3 compressRec709Gamut(float3 rgb) {
    if (all(rgb >= 0.0) && all(rgb <= 1.0)) return rgb;
    float neutral = saturate(dot(rgb, float3(0.2126, 0.7152, 0.0722)));
    float3 chroma = rgb - neutral;
    float amount = saturate(min(gamutBound(neutral, chroma.r),
        min(gamutBound(neutral, chroma.g), gamutBound(neutral, chroma.b))));
    return saturate(neutral + chroma * amount);
}

float srgbOetf(float linearValue) {
    return linearValue <= 0.0031308 ? 12.92 * linearValue : 1.055 * pow(linearValue, 1.0 / 2.4) - 0.055;
}

groupshared uint groupPeakBits;

float3 finiteRgb(float3 source) {
    return float3(
        isfinite(source.r) ? source.r : 0.0,
        isfinite(source.g) ? source.g : 0.0,
        isfinite(source.b) ? source.b : 0.0);
}

[numthreads(16, 16, 1)]
void detectHdr(uint3 id : SV_DispatchThreadID, uint3 groupThreadId : SV_GroupThreadID) {
    if (all(groupThreadId.xy == 0)) groupPeakBits = 0;
    GroupMemoryBarrierWithGroupSync();

    uint width, height;
    sourceTexture.GetDimensions(width, height);
    if (id.x < width && id.y < height) {
        float3 rgb = finiteRgb(sourceTexture.Load(int3(id.xy, 0)).rgb);
        float peak = max(0.0, max(rgb.r, max(rgb.g, rgb.b)));
        if (peak / sourceWhite > 1.01) {
            InterlockedMax(groupPeakBits, asuint(peak));
        }
    }

    GroupMemoryBarrierWithGroupSync();
    if (all(groupThreadId.xy == 0) && groupPeakBits != 0) {
        InterlockedMax(framePeakOutput[uint2(0, 0)], groupPeakBits);
    }
}

[numthreads(16, 16, 1)]
void mapToSdr(uint3 id : SV_DispatchThreadID) {
    uint width, height;
    sourceTexture.GetDimensions(width, height);
    if (id.x >= width || id.y >= height) return;

    float3 rgb = finiteRgb(sourceTexture.Load(int3(id.xy, 0)).rgb);
    float peak = max(0.0, max(rgb.r, max(rgb.g, rgb.b)));
    float scale = 1.0 / max(sourceWhite, peak);
    float framePeak = asfloat(framePeakTexture.Load(int3(0, 0, 0)));
    float transition = saturate((framePeak - sourceWhite * 1.01) / (sourceWhite * 0.24));
    float highlightStrength = transition * transition * (3.0 - 2.0 * transition);
    if (highlightStrength > 0.0) {
        float shoulderScale = 1.0 / sourceWhite;
        if (peak > sourceWhite * 0.75) {
            float mappedPeak = 0.75 + 0.25 * (peak - sourceWhite * 0.75) / (peak - sourceWhite * 0.5);
            shoulderScale = mappedPeak / peak;
        }
        scale = lerp(scale, shoulderScale, highlightStrength);
    }
    float3 sdrLinear = compressRec709Gamut(rgb * scale);
    // The video processor declares RGB_FULL_G22_NONE_P709 (sRGB) for this texture.
    float3 sdrSrgb = float3(
        srgbOetf(sdrLinear.r),
        srgbOetf(sdrLinear.g),
        srgbOetf(sdrLinear.b));
    outputTexture[id.xy] = float4(sdrSrgb, 1.0);
}
"#;

fn scrgb_tone_map_shader_bytecode() -> Result<&'static [u8], String> {
    static BYTECODE: std::sync::OnceLock<Result<Vec<u8>, String>> = std::sync::OnceLock::new();
    BYTECODE
        .get_or_init(|| compile_scrgb_shader(b"mapToSdr\0"))
        .as_deref()
        .map_err(Clone::clone)
}

fn scrgb_hdr_scan_shader_bytecode() -> Result<&'static [u8], String> {
    static BYTECODE: std::sync::OnceLock<Result<Vec<u8>, String>> = std::sync::OnceLock::new();
    BYTECODE
        .get_or_init(|| compile_scrgb_shader(b"detectHdr\0"))
        .as_deref()
        .map_err(Clone::clone)
}

fn compile_scrgb_shader(entry_point: &'static [u8]) -> Result<Vec<u8>, String> {
    type D3DCompileFn = unsafe extern "system" fn(
        *const std::ffi::c_void,
        usize,
        PCSTR,
        *const std::ffi::c_void,
        *mut std::ffi::c_void,
        PCSTR,
        PCSTR,
        u32,
        u32,
        *mut *mut std::ffi::c_void,
        *mut *mut std::ffi::c_void,
    ) -> HRESULT;

    let module =
        unsafe { LoadLibraryExW(w!("d3dcompiler_47.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) }
            .map_err(|e| format!("LoadLibraryExW d3dcompiler_47.dll: {e}"))?;
    let result = (|| {
        let compile = unsafe { GetProcAddress(module, PCSTR(b"D3DCompile\0".as_ptr())) }
            .ok_or("D3DCompile not exported by d3dcompiler_47.dll")?;
        let compile: D3DCompileFn = unsafe { std::mem::transmute(compile) };
        let mut code = std::ptr::null_mut();
        let mut errors = std::ptr::null_mut();
        let status = unsafe {
            compile(
                SCRGB_TONE_MAP_HLSL.as_ptr().cast(),
                SCRGB_TONE_MAP_HLSL.len(),
                PCSTR::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                PCSTR(entry_point.as_ptr()),
                PCSTR(b"cs_5_0\0".as_ptr()),
                0,
                0,
                &mut code,
                &mut errors,
            )
        };
        let errors = (!errors.is_null()).then(|| unsafe { ID3DBlob::from_raw(errors.cast()) });
        if status.is_err() {
            let message = errors
                .map(|blob| unsafe {
                    let bytes = std::slice::from_raw_parts(
                        blob.GetBufferPointer().cast::<u8>(),
                        blob.GetBufferSize(),
                    );
                    String::from_utf8_lossy(bytes).into_owned()
                })
                .unwrap_or_else(|| format!("D3DCompile failed: {status:?}"));
            return Err(format!("Compile scRGB compute shader: {message}"));
        }
        if code.is_null() {
            return Err("D3DCompile returned no scRGB compute shader bytecode".into());
        }
        let code = unsafe { ID3DBlob::from_raw(code.cast()) };
        let bytes = unsafe {
            std::slice::from_raw_parts(code.GetBufferPointer().cast::<u8>(), code.GetBufferSize())
        };
        Ok(bytes.to_vec())
    })();
    unsafe {
        let _ = FreeLibrary(module);
    }
    result
}

impl Nv12GpuConverter {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        input: &ID3D11Texture2D,
        in_width: u32,
        in_height: u32,
        out_width: u32,
        out_height: u32,
        source_format: hdr::SourceFormat,
        scrgb_sdr_white: f32,
    ) -> Option<Self> {
        let out_width = (out_width & !1).max(2);
        let out_height = (out_height & !1).max(2);
        let video_device = device
            .cast::<ID3D11VideoDevice>()
            .inspect_err(|e| vlog(&format!("cast ID3D11VideoDevice failed: {e:?}")))
            .ok()?;
        let video_context = context
            .cast::<ID3D11VideoContext>()
            .inspect_err(|e| vlog(&format!("cast ID3D11VideoContext failed: {e:?}")))
            .ok()?;

        let content_desc = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
            InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            InputFrameRate: DXGI_RATIONAL {
                Numerator: 60,
                Denominator: 1,
            },
            InputWidth: in_width,
            InputHeight: in_height,
            OutputFrameRate: DXGI_RATIONAL {
                Numerator: 60,
                Denominator: 1,
            },
            OutputWidth: out_width,
            OutputHeight: out_height,
            Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
        };
        let enumerator = unsafe { video_device.CreateVideoProcessorEnumerator(&content_desc) }
            .inspect_err(|e| vlog(&format!("CreateVideoProcessorEnumerator: {e:?}")))
            .ok()?;
        let processor = unsafe { video_device.CreateVideoProcessor(&enumerator, 0) }
            .inspect_err(|e| vlog(&format!("CreateVideoProcessor: {e:?}")))
            .ok()?;

        if let Ok(vctx1) = video_context.cast::<ID3D11VideoContext1>() {
            let input_cs = processor_input_colour_space(source_format);
            unsafe {
                vctx1.VideoProcessorSetStreamColorSpace1(&processor, 0, input_cs);
                vctx1.VideoProcessorSetOutputColorSpace1(
                    &processor,
                    DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
                );
            }
            vlog(&format!(
                "video processor colour space set: input={} -> output=YCbCr studio Rec.709",
                input_cs.0
            ));
        } else {
            vlog("ID3D11VideoContext1 unavailable; using default SDR Rec.709 colour space");
        }

        let output_desc = D3D11_TEXTURE2D_DESC {
            Width: out_width,
            Height: out_height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_NV12,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: D3D11_RESOURCE_MISC_SHARED.0 as u32,
        };
        let mut output_slots = Vec::with_capacity(NV12_OUTPUT_SLOT_COUNT);
        for _ in 0..NV12_OUTPUT_SLOT_COUNT {
            output_slots.push(create_output_slot(
                device,
                &video_device,
                &enumerator,
                &output_desc,
            )?);
        }
        assert_eq!(
            output_slots.len(),
            NV12_OUTPUT_SLOT_COUNT,
            "all NV12 output slots created"
        );
        let Ok(output_slots) = <[Nv12OutputSlot; NV12_OUTPUT_SLOT_COUNT]>::try_from(output_slots)
        else {
            vlog("NV12 output slot count mismatch");
            return None;
        };

        let readback_desc = D3D11_TEXTURE2D_DESC {
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
            ..output_desc
        };
        let mut readback = None;
        unsafe { device.CreateTexture2D(&readback_desc, None, Some(&mut readback)) }
            .inspect_err(|e| vlog(&format!("CreateTexture2D NV12 readback: {e:?}")))
            .ok()?;
        let readback = readback?;

        let tone_map = if matches!(source_format, hdr::SourceFormat::Rgba16Float { hdr: true }) {
            Some(ScrgbToneMapStage::new(device, input, in_width, in_height, scrgb_sdr_white).ok()?)
        } else {
            None
        };
        let processor_input = tone_map
            .as_ref()
            .map(|stage| &stage.output)
            .unwrap_or(input);

        let input_view_desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
            FourCC: 0,
            ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                Texture2D: windows::Win32::Graphics::Direct3D11::D3D11_TEX2D_VPIV {
                    MipSlice: 0,
                    ArraySlice: 0,
                },
            },
        };
        let mut input_view = None;
        unsafe {
            video_device.CreateVideoProcessorInputView(
                processor_input,
                &enumerator,
                &input_view_desc,
                Some(&mut input_view),
            )
        }
        .inspect_err(|e| vlog(&format!("CreateVideoProcessorInputView: {e:?}")))
        .ok()?;
        let input_view = input_view?;
        vlog(&format!(
            "NV12 converter built OK ({in_width}x{in_height} -> {out_width}x{out_height})"
        ));

        Some(Self {
            _video_device: video_device,
            video_context,
            processor,
            _enumerator: enumerator,
            input_view,
            output_slots,
            tone_map,
            readback,
            slot_cursor: 0,
            context: context.clone(),
            out_width,
            out_height,
        })
    }

    pub fn dxgi_format(&self) -> u32 {
        DXGI_FORMAT_NV12.0 as u32
    }

    pub fn update_scrgb_sdr_white(&mut self, source_white: f32) -> bool {
        self.tone_map
            .as_mut()
            .is_some_and(|stage| stage.update_source_white(&self.context, source_white))
    }

    pub fn convert_shared_texture(&mut self) -> Result<Nv12SharedTextureFrame, String> {
        assert!(
            self.slot_cursor < NV12_OUTPUT_SLOT_COUNT,
            "slot cursor in range"
        );
        assert!(self.out_width >= 2, "output width at least 2");
        let slot_index = self.slot_cursor;
        self.slot_cursor = (slot_index + 1) % NV12_OUTPUT_SLOT_COUNT;
        self.run_video_processor(slot_index)?;
        unsafe {
            self.context.Flush();
        }
        Ok(Nv12SharedTextureFrame {
            handle: self.output_slots[slot_index].handle,
            width: self.out_width,
            height: self.out_height,
            dxgi_format: self.dxgi_format(),
        })
    }

    pub fn convert_cpu_frame(
        &mut self,
    ) -> Result<(Nv12SharedTextureFrame, Vec<u8>, CpuFrameConversionTimings), String> {
        assert!(
            self.slot_cursor < NV12_OUTPUT_SLOT_COUNT,
            "slot cursor in range"
        );
        let slot_index = self.slot_cursor;
        self.slot_cursor = (slot_index + 1) % NV12_OUTPUT_SLOT_COUNT;
        self.run_video_processor(slot_index)?;
        let slot = &self.output_slots[slot_index];
        unsafe {
            self.context.CopyResource(&self.readback, &slot._texture);
            self.context.Flush();
        }
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        let readback_started = std::time::Instant::now();
        unsafe {
            self.context
                .Map(&self.readback, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
        }
        .map_err(|e| format!("Map NV12 readback: {e}"))?;
        let readback_map = readback_started.elapsed();
        let pack_started = std::time::Instant::now();
        let result = copy_packed_nv12(mapped, self.out_width, self.out_height);
        let cpu_pack = pack_started.elapsed();
        unsafe { self.context.Unmap(&self.readback, 0) };
        let data = result?;
        Ok((
            Nv12SharedTextureFrame {
                handle: slot.handle,
                width: self.out_width,
                height: self.out_height,
                dxgi_format: self.dxgi_format(),
            },
            data,
            CpuFrameConversionTimings {
                readback_map,
                cpu_pack,
            },
        ))
    }

    fn run_video_processor(&self, slot_index: usize) -> Result<(), String> {
        assert!(slot_index < NV12_OUTPUT_SLOT_COUNT, "slot index in range");
        if let Some(tone_map) = &self.tone_map {
            tone_map.run(&self.context);
        }
        let mut stream = D3D11_VIDEO_PROCESSOR_STREAM {
            Enable: windows::core::BOOL(1),
            OutputIndex: 0,
            InputFrameOrField: 0,
            PastFrames: 0,
            FutureFrames: 0,
            ppPastSurfaces: std::ptr::null_mut(),
            pInputSurface: std::mem::ManuallyDrop::new(Some(self.input_view.clone())),
            ppFutureSurfaces: std::ptr::null_mut(),
            ppPastSurfacesRight: std::ptr::null_mut(),
            pInputSurfaceRight: std::mem::ManuallyDrop::new(None),
            ppFutureSurfacesRight: std::ptr::null_mut(),
        };
        let blt = unsafe {
            self.video_context.VideoProcessorBlt(
                &self.processor,
                &self.output_slots[slot_index].view,
                0,
                std::slice::from_ref(&stream),
            )
        };
        unsafe {
            std::mem::ManuallyDrop::drop(&mut stream.pInputSurface);
        }
        blt.inspect_err(|e| vlog(&format!("VideoProcessorBlt RGB->NV12: {e:?}")))
            .map_err(|e| format!("VideoProcessorBlt RGB->NV12: {e}"))
    }
}

fn copy_packed_nv12(
    mapped: D3D11_MAPPED_SUBRESOURCE,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, String> {
    let row_pitch = mapped.RowPitch as usize;
    let width = width as usize;
    let height = height as usize;
    if row_pitch < width || width % 2 != 0 || height % 2 != 0 || mapped.pData.is_null() {
        return Err("Invalid NV12 readback dimensions or row pitch".into());
    }
    let mut packed = vec![0; width * height * 3 / 2];
    let src = mapped.pData.cast::<u8>();
    for row in 0..height {
        unsafe {
            std::ptr::copy_nonoverlapping(
                src.add(row * row_pitch),
                packed.as_mut_ptr().add(row * width),
                width,
            );
        }
    }
    let chroma_src = unsafe { src.add(row_pitch * height) };
    let chroma_dst = &mut packed[width * height..];
    for row in 0..height / 2 {
        unsafe {
            std::ptr::copy_nonoverlapping(
                chroma_src.add(row * row_pitch),
                chroma_dst.as_mut_ptr().add(row * width),
                width,
            );
        }
    }
    Ok(packed)
}

#[cfg(test)]
mod readback_tests {
    use super::*;

    #[test]
    fn strips_row_padding_from_both_nv12_planes() {
        let source = [10u8, 11, 0, 0, 20, 21, 0, 0, 30, 31, 0, 0];
        let mapped = D3D11_MAPPED_SUBRESOURCE {
            pData: source.as_ptr().cast_mut().cast(),
            RowPitch: 4,
            DepthPitch: source.len() as u32,
        };
        assert_eq!(
            copy_packed_nv12(mapped, 2, 2).unwrap(),
            [10, 11, 20, 21, 30, 31]
        );
    }
}

fn create_output_slot(
    device: &ID3D11Device,
    video_device: &ID3D11VideoDevice,
    enumerator: &ID3D11VideoProcessorEnumerator,
    output_desc: &D3D11_TEXTURE2D_DESC,
) -> Option<Nv12OutputSlot> {
    assert!(output_desc.Width >= 2, "output width at least 2");
    assert!(output_desc.Height >= 2, "output height at least 2");
    let mut output_texture = None;
    unsafe { device.CreateTexture2D(output_desc, None, Some(&mut output_texture)) }
        .inspect_err(|e| vlog(&format!("CreateTexture2D NV12 output: {e:?}")))
        .ok()?;
    let output_texture = output_texture?;
    let resource: IDXGIResource = output_texture
        .cast()
        .inspect_err(|e| {
            vlog(&format!(
                "QueryInterface IDXGIResource for NV12 output: {e:?}"
            ))
        })
        .ok()?;
    let shared_handle = unsafe { resource.GetSharedHandle() }
        .inspect_err(|e| vlog(&format!("GetSharedHandle NV12 output: {e:?}")))
        .ok()?;
    if shared_handle.is_invalid() {
        vlog("GetSharedHandle NV12 output returned an invalid handle");
        return None;
    }
    let shared_handle = shared_handle.0 as usize as u64;

    let output_view_desc = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
        ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
        Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
            Texture2D: windows::Win32::Graphics::Direct3D11::D3D11_TEX2D_VPOV { MipSlice: 0 },
        },
    };
    let mut output_view = None;
    unsafe {
        video_device.CreateVideoProcessorOutputView(
            &output_texture,
            enumerator,
            &output_view_desc,
            Some(&mut output_view),
        )
    }
    .inspect_err(|e| vlog(&format!("CreateVideoProcessorOutputView: {e:?}")))
    .ok()?;
    let output_view = output_view?;

    Some(Nv12OutputSlot {
        _texture: output_texture,
        view: output_view,
        handle: shared_handle,
    })
}

fn processor_input_colour_space(source_format: hdr::SourceFormat) -> DXGI_COLOR_SPACE_TYPE {
    match source_format {
        hdr::SourceFormat::R10G10B10A2 { hdr: true } => DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020,
        _ => DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709,
    }
}

unsafe impl Send for Nv12GpuConverter {}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_WARP;
    use windows::Win32::Graphics::Direct3D11::{
        D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
        D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_USAGE_STAGING, D3D11CreateDevice,
    };
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R16G16B16A16_FLOAT;

    fn cs_value(source_format: hdr::SourceFormat) -> i32 {
        processor_input_colour_space(source_format).0
    }

    #[test]
    fn eight_bit_sources_use_sdr_rec709_colour_space() {
        assert_eq!(
            cs_value(hdr::SourceFormat::Bgra8),
            DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709.0
        );
        assert_eq!(
            cs_value(hdr::SourceFormat::Rgba8),
            DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709.0
        );
    }

    #[test]
    fn unflagged_high_precision_sources_stay_sdr_rec709() {
        assert_eq!(
            cs_value(hdr::SourceFormat::R10G10B10A2 { hdr: false }),
            DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709.0
        );
        assert_eq!(
            cs_value(hdr::SourceFormat::Rgba16Float { hdr: false }),
            DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709.0
        );
    }

    #[test]
    fn ten_bit_hdr_uses_pq_rec2020_input_space() {
        assert_eq!(
            cs_value(hdr::SourceFormat::R10G10B10A2 { hdr: true }),
            DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020.0
        );
    }

    #[test]
    fn fp16_hdr_tone_map_outputs_sdr_rec709_input_space() {
        assert_eq!(
            cs_value(hdr::SourceFormat::Rgba16Float { hdr: true }),
            DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709.0
        );
    }

    #[test]
    fn scrgb_compute_shader_tonemaps_fp16_and_preserves_highlight_chroma() {
        let (device, context) = match crate::d3d11_device::create_shared_texture_device(None) {
            Ok(device) => device,
            Err(hardware_error) => {
                eprintln!("hardware D3D11 unavailable ({hardware_error}); exercising WARP");
                let mut device = None;
                let mut context = None;
                unsafe {
                    D3D11CreateDevice(
                        None,
                        D3D_DRIVER_TYPE_WARP,
                        HMODULE::default(),
                        D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                        None,
                        D3D11_SDK_VERSION,
                        Some(&mut device),
                        None,
                        Some(&mut context),
                    )
                    .expect("WARP D3D11 device must execute the tone-map check");
                }
                (
                    device.expect("WARP D3D11 device returned null"),
                    context.expect("WARP D3D11 context returned null"),
                )
            }
        };
        let width = 8;
        let height = 2;
        let input_desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R16G16B16A16_FLOAT,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut input = None;
        unsafe { device.CreateTexture2D(&input_desc, None, Some(&mut input)) }.unwrap();
        let input = input.unwrap();
        let half_pixels: [u16; 64] = [
            0x2266, 0x2266, 0x2266, 0x3C00, // scRGB 0.0125 -> SDR linear 0.00333 at white=3.
            0x3000, 0x3000, 0x3000, 0x3C00, // scRGB 0.125 -> SDR linear 0.0417 at white=3.
            0x4200, 0x4200, 0x4200, 0x3C00, // SDR reference white 3.0 -> 1.0.
            0x4400, 0x4400, 0x4400, 0x3C00, // HDR white 4.
            0x4600, 0x4600, 0x4600, 0x3C00, // Brighter HDR white 6.
            0x7C00, 0x3C00, 0x3C00, 0x3C00, // Positive infinity is scrubbed.
            0x4A40, 0x4640, 0x4240, 0x3C00, // Colored HDR highlight, 4:2:1.
            0x4A40, 0x4640, 0x4240, 0x3C00, // Repeat to fill an NV12 2x2 chroma block.
            0x2266, 0x2266, 0x2266, 0x3C00, 0x3000, 0x3000, 0x3000, 0x3C00, 0x3E00, 0x3A00, 0x3600,
            0x3C00, // In-gamut SDR color scaled from white=1 to white=3.
            0x4A40, 0x4A40, 0x4A40, 0x3C00, 0xB800, 0x3C00, 0x0000,
            0x3C00, // Negative red scRGB patch exercises hue-preserving gamut squeeze.
            0x7E00, 0x3C00, 0x3C00, 0x3C00, // NaN is scrubbed.
            0x4A40, 0x4640, 0x4240, 0x3C00, 0x4A40, 0x4640, 0x4240, 0x3C00,
        ];
        unsafe {
            context.UpdateSubresource(
                &input,
                0,
                None,
                half_pixels.as_ptr().cast(),
                (width * 8) as u32,
                (width * 8) as u32,
            );
        }

        let mut stage = ScrgbToneMapStage::new(&device, &input, width, height, 3.0).unwrap();
        stage.run(&context);
        let staging_desc = D3D11_TEXTURE2D_DESC {
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
            // VideoProcessor G22 input is unsupported for FP16 on some GPUs; feed it
            // an 8-bit SDR texture after the shader has applied the sRGB OETF.
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            ..input_desc
        };
        let mut staging = None;
        unsafe { device.CreateTexture2D(&staging_desc, None, Some(&mut staging)) }.unwrap();
        let staging = staging.unwrap();
        unsafe {
            context.CopyResource(&staging, &stage.output);
            context.Flush();
        }
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .unwrap();
        }
        let source = mapped.pData.cast::<u8>();
        let read = |pixel: usize, channel: usize| unsafe {
            let offset = (pixel / width as usize) * mapped.RowPitch as usize
                + (pixel % width as usize) * 4
                + channel;
            *source.add(offset) as f32 / 255.0
        };
        let low_srgb = read(0, 0);
        let mid_srgb = read(1, 0);
        let white_srgb = read(2, 0);
        let highlight_srgb = read(3, 0);
        let extreme_srgb = read(4, 0);
        let very_bright_srgb = read(11, 0);
        // SDR gray composed on HDR at white=3 must return to the same sRGB code.
        // This assertion is independent of the tone-map implementation's output.
        let original_srgb_gray = 1.055_f32 * (0.125_f32 / 3.0).powf(1.0 / 2.4) - 0.055;
        assert!(
            (mid_srgb - original_srgb_gray).abs() <= 1.0 / 255.0,
            "resharing SDR gray must preserve its sRGB code: got {mid_srgb}, expected {original_srgb_gray}"
        );
        assert_eq!(read(5, 0), 0.0, "positive infinity must map to zero");
        assert_eq!(read(13, 0), 0.0, "NaN must map to zero");
        let srgb_eotf = |encoded: f32| {
            if encoded <= 0.04045 {
                encoded / 12.92
            } else {
                ((encoded + 0.055) / 1.055).powf(2.4)
            }
        };
        assert!(
            (srgb_eotf(low_srgb) - 0.0125 / 3.0).abs() < 0.002,
            "sRGB low-segment output should decode to the scaled scRGB value, got {} from {low_srgb}",
            srgb_eotf(low_srgb)
        );
        assert!(
            (srgb_eotf(mid_srgb) - 0.125 / 3.0).abs() < 0.003,
            "sRGB midtone output should decode to the scaled scRGB value, got {} from {mid_srgb}",
            srgb_eotf(mid_srgb)
        );
        let white_linear = srgb_eotf(white_srgb);
        let expected_white = hdr::tone_map_scrgb_to_sdr_with_highlights([3.0; 3], 3.0, 1.0)[0];
        assert!(
            (white_linear - expected_white).abs() < 0.004,
            "sRGB-decoded white in an HDR frame should be {expected_white}, got {white_linear} from {white_srgb}"
        );
        assert!(
            white_srgb < highlight_srgb
                && highlight_srgb < extreme_srgb
                && extreme_srgb < very_bright_srgb,
            "white and HDR highlights must remain distinct: {white_srgb}, {highlight_srgb}, {extreme_srgb}, {very_bright_srgb}"
        );
        let colored_srgb = [read(6, 0), read(6, 1), read(6, 2)];
        let red = colored_srgb[0];
        let green = colored_srgb[1];
        let blue = colored_srgb[2];
        assert!(red <= 1.0);
        let expected = hdr::tone_map_scrgb_to_sdr_with_highlights([12.5, 6.25, 3.125], 3.0, 1.0);
        for (channel, expected_linear) in [red, green, blue].into_iter().zip(expected) {
            let expected_srgb = if expected_linear <= 0.0031308 {
                12.92 * expected_linear
            } else {
                1.055 * expected_linear.powf(1.0 / 2.4) - 0.055
            };
            assert!((channel - expected_srgb).abs() < 0.003);
        }
        let in_gamut_gpu = [read(10, 0), read(10, 1), read(10, 2)].map(srgb_eotf);
        let in_gamut_cpu = hdr::tone_map_scrgb_to_sdr([1.5, 0.75, 0.375], 3.0);
        for (actual, expected) in in_gamut_gpu.into_iter().zip(in_gamut_cpu) {
            assert!((actual - expected).abs() < 0.004);
        }
        let negative_gamut_gpu = [read(12, 0), read(12, 1), read(12, 2)].map(srgb_eotf);
        let negative_gamut_cpu = hdr::tone_map_scrgb_to_sdr([-0.5, 1.0, 0.0], 3.0);
        for (actual, expected) in negative_gamut_gpu.into_iter().zip(negative_gamut_cpu) {
            assert!((actual - expected).abs() < 0.004);
        }
        unsafe { context.Unmap(&staging, 0) };

        let transition_peak = hdr::f16_to_f32(0x4266);
        let mut transition_pixels = [0x4200_u16; 64];
        transition_pixels[12..15].fill(0x4266);
        unsafe {
            context.UpdateSubresource(
                &input,
                0,
                None,
                transition_pixels.as_ptr().cast(),
                (width * 8) as u32,
                (width * 8) as u32,
            );
        }
        stage.run(&context);
        unsafe {
            context.CopyResource(&staging, &stage.output);
            context.Flush();
        }
        let mut transition_mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut transition_mapped))
                .unwrap();
        }
        let transition_source = transition_mapped.pData.cast::<u8>();
        let transition_white_srgb = unsafe { *transition_source.add(2 * 4) as f32 / 255.0 };
        let relative_peak = transition_peak / 3.0;
        let t = ((relative_peak - 1.01) / 0.24).clamp(0.0, 1.0);
        let strength = t * t * (3.0 - 2.0 * t);
        let expected_transition_white =
            hdr::tone_map_scrgb_to_sdr_with_highlights([3.0; 3], 3.0, strength)[0];
        assert!(
            (srgb_eotf(transition_white_srgb) - expected_transition_white).abs() < 0.004,
            "frame-peak shoulder activation should be smooth: expected {expected_transition_white}, got {}",
            srgb_eotf(transition_white_srgb)
        );
        unsafe { context.Unmap(&staging, 0) };

        let sdr_only_pixels = [0x4200_u16; 64];
        unsafe {
            context.UpdateSubresource(
                &input,
                0,
                None,
                sdr_only_pixels.as_ptr().cast(),
                (width * 8) as u32,
                (width * 8) as u32,
            );
        }
        stage.run(&context);
        unsafe {
            context.CopyResource(&staging, &stage.output);
            context.Flush();
        }
        let mut sdr_mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut sdr_mapped))
                .unwrap();
        }
        let sdr_source = sdr_mapped.pData.cast::<u8>();
        let sdr_white = unsafe { *sdr_source.add(2 * 4) as f32 / 255.0 };
        assert_eq!(
            sdr_white, 1.0,
            "an SDR-only frame must clear the prior HDR peak"
        );
        unsafe { context.Unmap(&staging, 0) };

        unsafe {
            context.UpdateSubresource(
                &input,
                0,
                None,
                half_pixels.as_ptr().cast(),
                (width * 8) as u32,
                (width * 8) as u32,
            );
        }

        assert!(stage.update_source_white(&context, 4.0));
        assert!(!stage.update_source_white(&context, 4.0));
        stage.run(&context);
        unsafe {
            context.CopyResource(&staging, &stage.output);
            context.Flush();
        }
        let mut updated_mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut updated_mapped))
                .unwrap();
        }
        let updated_source = updated_mapped.pData.cast::<u8>();
        let updated_white_offset = 2 * 4;
        let updated_white = unsafe { *updated_source.add(updated_white_offset) as f32 / 255.0 };
        let updated_hdr_highlight = unsafe { *updated_source.add(3 * 4) as f32 / 255.0 };
        let expected_hdr_highlight =
            hdr::tone_map_scrgb_to_sdr_with_highlights([4.0; 3], 4.0, 1.0)[0];
        assert!(
            (srgb_eotf(updated_white) - 0.75).abs() < 0.004,
            "changing the SDR-white constant from 3 to 4 should rescale the 3-unit patch to 0.75"
        );
        assert!(
            (srgb_eotf(updated_hdr_highlight) - expected_hdr_highlight).abs() < 0.004,
            "HDR highlights should re-enable the shoulder after an SDR-only frame"
        );
        unsafe { context.Unmap(&staging, 0) };

        let mut converter = Nv12GpuConverter::new(
            &device,
            &context,
            &input,
            width,
            height,
            width,
            height,
            hdr::SourceFormat::Rgba16Float { hdr: true },
            3.0,
        )
        .expect("FP16 tone-map plus video-processor NV12 pipeline should initialize");
        let vctx1 = converter
            .video_context
            .cast::<ID3D11VideoContext1>()
            .expect("VideoContext1 should be available for color-space checks");
        assert_eq!(
            unsafe { vctx1.VideoProcessorGetStreamColorSpace1(&converter.processor, 0) }.0,
            DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709.0
        );
        assert_eq!(
            unsafe { vctx1.VideoProcessorGetOutputColorSpace1(&converter.processor) }.0,
            DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709.0
        );
        let enumerator1 = converter
            ._enumerator
            .cast::<windows::Win32::Graphics::Direct3D11::ID3D11VideoProcessorEnumerator1>()
            .expect("VideoProcessorEnumerator1 should be available for format checks");
        let conversion_supported = unsafe {
            enumerator1.CheckVideoProcessorFormatConversion(
                DXGI_FORMAT_R8G8B8A8_UNORM,
                DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709,
                DXGI_FORMAT_NV12,
                DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
            )
        }
        .map(|supported| supported.as_bool())
        .expect("video processor color-space support query should work");
        assert!(
            conversion_supported,
            "RGBA8 G22 to NV12 G22 must be supported"
        );
        let (_, nv12, _) = converter
            .convert_cpu_frame()
            .expect("HDR FP16 should convert to packed NV12");
        assert_eq!(nv12.len(), (width * height * 3 / 2) as usize);
        let expected_limited_y = |linear: f64| {
            let encoded = if linear <= 0.0031308 {
                12.92 * linear
            } else {
                1.055 * linear.powf(1.0 / 2.4) - 0.055
            };
            (16.0 + 219.0 * encoded).round() as u8
        };
        let first_chroma = (width * height) as usize;
        assert!(
            nv12[first_chroma].abs_diff(128) <= 3 && nv12[first_chroma + 1].abs_diff(128) <= 3,
            "neutral SDR reference white should have neutral studio-range UV near 128, got U={} V={}",
            nv12[first_chroma],
            nv12[first_chroma + 1]
        );
        for (pixel, source_peak) in [
            (0, 0.0125_f32),
            (1, 0.125),
            (2, 3.0),
            (3, 4.0),
            (4, 6.0),
            (11, 12.5),
        ] {
            let mapped_linear =
                hdr::tone_map_scrgb_to_sdr_with_highlights([source_peak; 3], 3.0, 1.0)[0] as f64;
            let expected_y = expected_limited_y(mapped_linear);
            assert!(
                nv12[pixel].abs_diff(expected_y) <= 3,
                "FP16 input peak {source_peak} maps to linear {mapped_linear}, then sRGB/studio Rec.709 Y={expected_y}, got Y={}",
                nv12[pixel]
            );
        }
        assert!(
            nv12[0] < nv12[1]
                && nv12[1] < nv12[2]
                && nv12[2] < nv12[3]
                && nv12[3] < nv12[4]
                && nv12[4] < nv12[11],
            "NV12 luma should preserve white/highlight ordering: {:?}",
            [&nv12[0], &nv12[1], &nv12[2], &nv12[3], &nv12[4], &nv12[11]]
        );
        // The last two columns form a uniform colored 2x2 block: NV12 chroma
        // subsampling must not average this highlight with a neighboring gray.
        let [r, g, b] = colored_srgb.map(f64::from);
        let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        let expected_y = (16.0 + 219.0 * y).round() as u8;
        let expected_u = (128.0 + 224.0 * (b - y) / (2.0 * (1.0 - 0.0722))).round() as u8;
        let expected_v = (128.0 + 224.0 * (r - y) / (2.0 * (1.0 - 0.2126))).round() as u8;
        assert!(nv12[6].abs_diff(expected_y) <= 3);
        assert!(nv12[first_chroma + 6].abs_diff(expected_u) <= 3);
        assert!(nv12[first_chroma + 7].abs_diff(expected_v) <= 3);
    }

    #[test]
    #[ignore = "manual hardware-only throughput probe; excludes WGC acquisition and app delivery"]
    fn benchmark_synthetic_hdr_1080p_to_nv12_cpu_readback() {
        use std::time::Instant;

        const WIDTH: u32 = 1920;
        const HEIGHT: u32 = 1080;
        const WARMUP_FRAMES: usize = 10;
        const MEASURED_FRAMES: usize = 120;
        const BUDGET_MS: f64 = 1000.0 / 120.0;

        let (device, context) = crate::d3d11_device::create_shared_texture_device(None)
            .expect("benchmark requires a hardware D3D11 device; WARP is not representative");
        let desc = D3D11_TEXTURE2D_DESC {
            Width: WIDTH,
            Height: HEIGHT,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R16G16B16A16_FLOAT,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut source = None;
        let mut input = None;
        unsafe {
            device
                .CreateTexture2D(&desc, None, Some(&mut source))
                .unwrap();
            device
                .CreateTexture2D(&desc, None, Some(&mut input))
                .unwrap();
        }
        let source = source.unwrap();
        let input = input.unwrap();
        let hdr_pixels = [0x4600_u16, 0x4400, 0x4200, 0x3C00].repeat((WIDTH * HEIGHT) as usize);
        unsafe {
            context.UpdateSubresource(
                &source,
                0,
                None,
                hdr_pixels.as_ptr().cast(),
                WIDTH * 8,
                WIDTH * HEIGHT * 8,
            );
        }
        let mut converter = Nv12GpuConverter::new(
            &device,
            &context,
            &input,
            WIDTH,
            HEIGHT,
            WIDTH,
            HEIGHT,
            hdr::SourceFormat::Rgba16Float { hdr: true },
            3.0,
        )
        .expect("hardware video processor must support synthetic FP16 HDR to NV12");
        let expected_bytes = (WIDTH * HEIGHT * 3 / 2) as usize;

        for _ in 0..WARMUP_FRAMES {
            unsafe { context.CopyResource(&input, &source) };
            let (_, data, _) = converter.convert_cpu_frame().expect("HDR NV12 warmup");
            assert_eq!(data.len(), expected_bytes);
        }

        let mut samples_ms = Vec::with_capacity(MEASURED_FRAMES);
        for _ in 0..MEASURED_FRAMES {
            let started = Instant::now();
            unsafe { context.CopyResource(&input, &source) };
            let (_, data, _) = converter.convert_cpu_frame().expect("HDR NV12 readback");
            assert_eq!(data.len(), expected_bytes);
            samples_ms.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        let mean_ms = samples_ms.iter().sum::<f64>() / samples_ms.len() as f64;
        samples_ms.sort_by(f64::total_cmp);
        let p95_ms = samples_ms[(samples_ms.len() * 95).div_ceil(100) - 1];
        eprintln!(
            "synthetic HDR 1080p -> 2-pass tone-map -> NV12 -> CPU readback: frames={MEASURED_FRAMES}, mean={mean_ms:.2}ms, p95={p95_ms:.2}ms, 120fps_budget={BUDGET_MS:.2}ms, mean_budget={:.1}%, p95_budget={:.1}% (excludes WGC acquire and app delivery)",
            mean_ms / BUDGET_MS * 100.0,
            p95_ms / BUDGET_MS * 100.0,
        );
    }
}
