# Fluxer Windows 스트리밍 포크

Fluxer 원본 기준 커밋 [c68d62b8](https://github.com/fluxerapp/fluxer/commit/c68d62b8a00e7a1cc2899773a03d54dadea9cdef)에 Windows 데스크톱 화면공유와 관련 설정·진단 변경을 적용한 포크입니다.

## 코드 변경 전후

[기준 커밋부터 변경 코드까지 전체 비교](https://github.com/CJsw12/fluxer/compare/c68d62b8a00e7a1cc2899773a03d54dadea9cdef...85917100a5cd7a64bd1b17a82276836f5043abc2?diff=split)

| 변경 | 기준 코드 | 수정 코드 |
| --- | --- | --- |
| Windows WGC 캡처와 HDR→SDR 변환 | [wgc_capture.rs](https://github.com/CJsw12/fluxer/blob/c68d62b8a00e7a1cc2899773a03d54dadea9cdef/fluxer_desktop/native/win-game-capture/src/wgc_capture.rs) · [hdr.rs](https://github.com/CJsw12/fluxer/blob/c68d62b8a00e7a1cc2899773a03d54dadea9cdef/fluxer_desktop/native/win-game-capture/src/hdr.rs) | [wgc_capture.rs](https://github.com/CJsw12/fluxer/blob/0f2f59a13e05fed01e3d31c55f21970ee9e5e25e/fluxer_desktop/native/win-game-capture/src/wgc_capture.rs) · [hdr.rs](https://github.com/CJsw12/fluxer/blob/0f2f59a13e05fed01e3d31c55f21970ee9e5e25e/fluxer_desktop/native/win-game-capture/src/hdr.rs) · [nv12_gpu.rs](https://github.com/CJsw12/fluxer/blob/0f2f59a13e05fed01e3d31c55f21970ee9e5e25e/fluxer_desktop/native/win-game-capture/src/nv12_gpu.rs) |
| 고주사율 프레임 캡처와 전달 | [NativeScreenCapture.ts](https://github.com/CJsw12/fluxer/blob/c68d62b8a00e7a1cc2899773a03d54dadea9cdef/fluxer_desktop/src/main/NativeScreenCapture.ts) | [NativeScreenCapture.ts](https://github.com/CJsw12/fluxer/blob/0f2f59a13e05fed01e3d31c55f21970ee9e5e25e/fluxer_desktop/src/main/NativeScreenCapture.ts) · [CpuFrameRateFilter.ts](https://github.com/CJsw12/fluxer/blob/0f2f59a13e05fed01e3d31c55f21970ee9e5e25e/packages/voice_engine_v2/src/bridge/CpuFrameRateFilter.ts) · [NativeDisplayMediaCapture.ts](https://github.com/CJsw12/fluxer/blob/0f2f59a13e05fed01e3d31c55f21970ee9e5e25e/fluxer_app/src/features/voice/utils/NativeDisplayMediaCapture.ts) |
| FPS·코덱·비트레이트 설정과 송신 정책 | [StreamSettingsMenuContent.tsx](https://github.com/CJsw12/fluxer/blob/c68d62b8a00e7a1cc2899773a03d54dadea9cdef/fluxer_app/src/features/voice/components/StreamSettingsMenuContent.tsx) · [VoiceSettings.ts](https://github.com/CJsw12/fluxer/blob/c68d62b8a00e7a1cc2899773a03d54dadea9cdef/fluxer_app/src/features/voice/state/VoiceSettings.ts) | [StreamSettingsMenuContent.tsx](https://github.com/CJsw12/fluxer/blob/aeb95d9b9/fluxer_app/src/features/voice/components/StreamSettingsMenuContent.tsx) · [VoiceSettings.ts](https://github.com/CJsw12/fluxer/blob/aeb95d9b9/fluxer_app/src/features/voice/state/VoiceSettings.ts) · [ScreenShareOptions.ts](https://github.com/CJsw12/fluxer/blob/aeb95d9b9/fluxer_app/src/features/voice/utils/ScreenShareOptions.ts) |
| 화면공유 캡처 경로 연결과 앱 오디오 범위 처리 | [NativeAudioCaptureBridge.ts](https://github.com/CJsw12/fluxer/blob/c68d62b8a00e7a1cc2899773a03d54dadea9cdef/fluxer_app/src/features/voice/utils/NativeAudioCaptureBridge.ts) | [NativeAudioCaptureBridge.ts](https://github.com/CJsw12/fluxer/blob/fbc4769a4/fluxer_app/src/features/voice/utils/NativeAudioCaptureBridge.ts) · [ScreenShareStartFlow.ts](https://github.com/CJsw12/fluxer/blob/cb1a6d75b/fluxer_app/src/features/voice/utils/ScreenShareStartFlow.ts) |
| 캡처·송신 정책 진단 정보 | [StatsForNerdsCopy.ts](https://github.com/CJsw12/fluxer/blob/c68d62b8a00e7a1cc2899773a03d54dadea9cdef/fluxer_app/src/features/voice/utils/StatsForNerdsCopy.ts) | [StatsForNerdsCopy.ts](https://github.com/CJsw12/fluxer/blob/4a16140f2/fluxer_app/src/features/voice/utils/StatsForNerdsCopy.ts) |
| 패키지 실행 오류와 Windows 개발 도구 빌드 | [electron-builder.config.cjs](https://github.com/CJsw12/fluxer/blob/c68d62b8a00e7a1cc2899773a03d54dadea9cdef/fluxer_desktop/electron-builder.config.cjs) · [tunnel.rs](https://github.com/CJsw12/fluxer/blob/c68d62b8a00e7a1cc2899773a03d54dadea9cdef/tools/dev/src/tunnel.rs) | [electron-builder.config.cjs](https://github.com/CJsw12/fluxer/blob/82baa7ba4/fluxer_desktop/electron-builder.config.cjs) · [tunnel.rs](https://github.com/CJsw12/fluxer/blob/85917100a/tools/dev/src/tunnel.rs) |

## 구현 요약

- Windows Graphics Capture 프레임을 native 경로로 전달하고, HDR 입력은 GPU에서 SDR NV12로 변환합니다.
- 화면공유 설정에서 비트레이트를 Automatic 또는 1–20 Mbps로 고를 수 있습니다. 기본 FPS는 60이며, 자동 코덱 선택은 로컬 인코더와 수신 측에서 지원될 때 H.265를 우선합니다.
- 90/120 FPS 공유에서는 송신 측 프레임 유지 정책을 사용합니다. 실제 수신 FPS는 원본 화면 갱신, 인코더, 네트워크, 수신 디코더의 영향을 받으므로 120 FPS를 보장하지는 않습니다.
- 통계 내보내기에 native 캡처와 송신 인코딩 설정 정보를 추가했습니다.

## 오류와 해결

| 증상 | 수정 |
| --- | --- |
| 패키징된 앱 실행 시 ERR_MODULE_NOT_FOUND: electron-log | Electron 패키지 파일 목록에 electron-log를 포함했습니다. [electron-builder.config.cjs](https://github.com/CJsw12/fluxer/blob/82baa7ba4/fluxer_desktop/electron-builder.config.cjs) |
| Windows에서 개발 도구의 Unix 소켓 코드가 컴파일되지 않음 | Unix 빌드에서만 UnixStream 검사를 컴파일하고, 다른 플랫폼에서는 false를 반환하도록 분리했습니다. [tunnel.rs](https://github.com/CJsw12/fluxer/blob/85917100a/tools/dev/src/tunnel.rs) |
| HDR 화면공유에서 어두운 영역이 뭉치거나 밝은 부분이 날아감 | 캡처 단계에서 GPU 기반 HDR→SDR 변환 경로를 적용했습니다. 수신 화면의 결과는 캡처·톤매핑·코덱·디스플레이에 따라 달라질 수 있습니다. [hdr.rs](https://github.com/CJsw12/fluxer/blob/0f2f59a13e05fed01e3d31c55f21970ee9e5e25e/fluxer_desktop/native/win-game-capture/src/hdr.rs) · [nv12_gpu.rs](https://github.com/CJsw12/fluxer/blob/0f2f59a13e05fed01e3d31c55f21970ee9e5e25e/fluxer_desktop/native/win-game-capture/src/nv12_gpu.rs) |

기반 프로젝트: [Fluxer](https://github.com/fluxerapp/fluxer) · 소스 라이선스: [AGPL-3.0-or-later](LICENSE).

Fluxer 브랜딩, 아이콘, 기본 아바타, 배지, 스크린샷과 홍보 이미지는 [fluxer_static/LICENSE](fluxer_static/LICENSE)에 별도 조건이 적용됩니다. 저장소 공개만으로 상표·브랜드 사용이나 공식 보증 권한이 부여되지는 않습니다. 제3자 자료는 [THIRD_PARTY_LICENSES.md](fluxer_static/THIRD_PARTY_LICENSES.md)에 기재된 각 라이선스를 따릅니다.
