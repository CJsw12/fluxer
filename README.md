# Fluxer Rust 데스크톱 이주

이 프로젝트의 목표는 [Fluxer](https://github.com/fluxerapp/fluxer)의 Windows 데스크톱 클라이언트를 UI, 음성·영상, 화면 공유와 운영체제 연동까지 Rust로 이주하는 것입니다.

## Rust 클라이언트 진행도

[![Rust 클라이언트 전체](docs/progress/badge-overall.svg)](docs/progress/progress.json) [![일상 사용 기준](docs/progress/badge-daily-driver.svg)](docs/progress/progress.json)

**최근 업데이트 · 2026-11-22**

네이티브 LiveKit `voice_worker`와 opcode 4 음성 채널 통합, 화면 공유 송수신(`DesktopCapturer`·`NativeVideoStream`), `cpal` 오디오 장치·볼륨 선택, 로케일별 날짜·숫자 현지화, 첨부 스포일러·NSFW 커버·비이미지 파일 카드, 역할·권한 CRUD, 접근성 설정·AccessKit 라벨, 업데이트 확인기를 구현하고 Codex 교차 계열 보안 검토 2회를 거쳤습니다. 총 **669건**의 자동 테스트(워크스페이스 659건 + `voice_worker` 10건)가 통과했으며, 실서버 2계정 음성·화면 공유 및 실기 GUI 검증은 남아 있습니다. [상세 진행 기록](PROGRESS.md)

[![진행 지도](docs/progress/progress.svg)](docs/progress/progress.json)

<!-- progress:start -->
**기준일 2026-11-28** · 전체 **80%** · 일상 사용(서버 관리, 검색, 다국어·접근성, 음성·화면 공유 제외) **84%** · 실기 완료 55 · 구현 완료 0 · 미검증 38 · 부분 구현 0 · 미착수 0

<details>
<summary>영역별 상세 항목 보기 (93개)</summary>

| 영역 | 진행률 | 항목 |
| --- | --- | --- |
| 코어(게이트웨이·상태·REST·세션) | 88% | 🟢 게이트웨이 연결·하트비트, zstd 스트림 수신, 상태 디스패치·모델 확장, REST 클라이언트·레이트리밋, 세션 저장(DPAPI·잠금), op14 멤버 목록<br>🟣 op3 프레즌스 갱신, 재연결·장시간 안정성 |
| 로그인·계정 | 71% | 🟢 인스턴스 로그인, 캡차 자동 풀이, 로그인 유지·자동 로그인<br>🟣 TOTP 2단계 인증, 로그아웃, 패스키·보안 키, 다중 계정 전환 |
| 채널·메시지 표시 | 100% | 🟢 길드·채널 트리, 안읽음 배지·pill, 메시지 그룹핑·구분선, 마크다운 렌더, 코드 블록 강조, 멘션 pill·시스템 메시지, 멤버 목록 사이드바, 이전 메시지 페이지, 인라인 이미지 표시, 반응 pill 표시 |
| 메시지 작성·상호작용 | 86% | 🟢 텍스트 전송·낙관적 표시, 메시지 수정·저장, 멘션 자동완성, 이모지 단축코드, 답장·미리보기, 입력 중 표시, 첨부 업로드, 드래프트 보존<br>🟣 삭제 확인, 반응 추가·토글, 이모지 피커 |
| 탐색·단축키 | 92% | 🟢 Ctrl+K 빠른 전환, 단축키 등록·처리, 길드·채널 순회, 딥링크(fluxer://), 방문 기록·앞뒤 이동<br>🟣 안읽음·멘션 점프 |
| 알림·첨부 | 58% | 🟢 음소거·알림 레벨 로직<br>🟣 데스크톱 토스트, 알림 소리, 첨부 업로드 UI, 비이미지 첨부 표시, 클립보드·드래그 업로드 |
| 임베드·미디어 | 92% | 🟢 임베드 모델(core), 임베드 카드 렌더, GIF·동영상 미리보기, 미디어 뷰어, 스포일러·NSFW 가림<br>🟣 신뢰 도메인 경고 |
| 설정·개인화 | 100% | 🟢 다크 테마, 라이트 테마, 사용자 설정 모델·REST, 사용자 패널, 설정 화면, 글꼴·밀도·확대 |
| 소셜(친구·DM·프로필) | 58% | 🟢 DM 목록·대화<br>🟣 DM 열기·생성, 친구 목록, 프로필 팝업, 차단·관계 관리, 그룹 DM |
| 서버 관리 | 50% | 🟣 초대 참가, 길드 탈퇴, 채널 생성·편집, 역할·권한 편집, 멤버 관리(킥·밴), 채널 음소거 |
| 검색 | 67% | 🟢 메시지 검색(REST)<br>🟣 검색 결과 화면, 메시지로 점프 |
| 다국어·접근성 | 92% | 🟢 i18n 크레이트·카탈로그, 날짜·숫자 현지화, 앱 UI 번역 적용, 언어 전환 UI, 스크린리더 라벨<br>🟣 접근성 설정 |
| 데스크톱 통합·배포 | 79% | 🟢 단일 인스턴스, 릴리스 exe·아이콘, 콘솔 창 제거, 파일 로그<br>🟣 시작 메뉴·AUMID, 트레이·자동 시작, 자동 업데이트 |
| 음성·화면 공유 | 50% | 🟣 음성 채널 참가, 화면 공유 송신, 화면 공유 수신, 오디오 장치·볼륨, 음성 UI |

</details>
<!-- progress:end -->

<sub>초록 = 실기 완료(1.0), 파랑 = 구현 완료·자동 테스트 통과(0.75), 보라 = 미검증(0.5, 실기·자동 시험 미확인), 노랑 = 부분 구현(0.5), 회색 = 미착수(0.0). 퍼센트는 항목 수 가중 평균이며 작업량 비율이 아닙니다. 데이터: [`docs/progress/progress.json`](docs/progress/progress.json), 생성: `python -I scripts/progress_svg.py`. 상세 기록: [`PROGRESS.md`](PROGRESS.md).</sub>

### 네이티브 Rust 클라이언트 (`fluxer_client/`)

웹뷰 없이 [GPUI](https://github.com/zed-industries/zed) + [gpui-kit](https://github.com/longbridge/gpui-component)로 만든 새 클라이언트입니다. 자체 호스팅 Fluxer 서버에서 공식 앱과 나란히 띄워 1:1로 비교하며 개발합니다.

| 크레이트 | 내용 |
| --- | --- |
| `core` | 게이트웨이(zstd-stream, 재개), 상태 모델·권한, REST(ALTCHA 캡차 풀이, 요청 속도 제한), DPAPI 세션 저장, 알림 판단, 빠른 전환·이모지 검색 |
| `app` | GPUI 앱: 로그인, 서버·채널·DM, 메시지(마크다운·임베드·반응·답장·수정), 멤버 목록, 단축키·Ctrl+K, 자동완성, 알림, 첨부, 다크 테마 |
| `markdown` | 공식 Rust 마크다운 파서 기반 렌더(드래그 선택 유지) |
| `i18n` | 공식 Lingui 번역 44개 언어 내장(zstd 압축) |
| `voice_worker` | 별도 프로세스 LiveKit WebRTC·화면 공유 캡처/디코드·`PlatformAudio` 오디오 워커 |
| `devserver` | 로컬 테스트용 픽스처 서버 |

```sh
cd fluxer_client
cargo test --workspace -- --test-threads=2
cargo run -p fluxer_client_app
```

## 미디어·Rust 셸 실험

기준일: 2026-11-11. 아래 미디어 결과는 별도 실험이며 실제 Fluxer 음성 채널에서의 성공을 뜻하지 않습니다.

- **Rust 셸:** Windows 데스크톱 셸과 창·트레이·입력·오디오·IPC 관련 이주 작업이 진행 중입니다. 구현된 기능과 기존 Electron 기능의 동등성은 항목별로 실제 앱에서 확인해야 합니다.
- **H.265 송신 실험:** 자체 테스트 창을 Windows Graphics Capture로 캡처하고 Media Foundation으로 H.265를 인코딩해 로컬 LiveKit 서버를 거쳐 Chrome에서 수신했습니다. GPU 텍스처를 활용한 별도 실험에서도 화면 변화와 디코딩을 확인했습니다. 키프레임 요청 및 비트레이트 피드백을 연결하고 재구독 후 화면 복구도 실험했습니다.
- **H.265 Rust 수신 실험:** Windows 시스템 디코더를 Rust에서 직접 구동해 H.265 60프레임을 디코딩했습니다. 출력 지연 처리와 저지연 설정을 보완한 뒤, 로컬 LiveKit/WebRTC를 통한 기록 영상 및 실시간 테스트 창 수신도 각각 10프레임·서로 다른 화면 10개로 확인했습니다. Rust UI 표시와 실제 Fluxer 채널 통합은 아직 남아 있습니다.
- **성능:** 짧은 캡처·인코딩 실험에서 GPU 경로가 비교 경로보다 낮은 CPU와 프로세스 메모리를 보였습니다. 프로세스 구성도 달랐으며 전체 앱, GPU 메모리와 장시간 성능은 비교하지 않았습니다. 따라서 전체 앱의 RAM 절감은 아직 확인되지 않았습니다.
- **실제 제품 통합:** 실제 Fluxer 채널의 H.265 송수신, 화면 오디오, 암호화 및 Rust UI 영상 표시는 아직 검증되지 않았습니다.

상세 실행 결과와 제약은 개발 작업 공간의 `PROGRESS.md`에 기록하고 있습니다. 미디어 실험은 `tools/h265_livekit_probe`에서 진행 중이며 아직 이 브랜치에 공개되지 않았습니다.

## 이주 계획

일상 사용 기능 검증을 우선합니다. 현재 개발 작업 순서는 [진행 기록](PROGRESS.md#next-steps)에서 확인할 수 있습니다.

| 단계 | 작업과 완료 기준 | 상태 |
| --- | --- | --- |
| 1. 기준선 | 실제 앱의 기능과 프로세스 메모리, GPU, CPU, 영상 지연 및 배포 크기를 재현 가능한 조건에서 측정합니다. | 전체 앱 기준선 미완료 |
| 2. H.265 네이티브 미디어 | 화면 캡처, 인코딩, WebRTC 송수신, 디코딩, 복구와 호환 기기를 검증합니다. | 로컬 Rust 수신·재구독 복구·반복 Room 종료 확인. 별도 기기 검증 필요 |
| 3. Fluxer 음성 채널 통합 | 실제 참가자·트랙 소유권, 화면 오디오, 재연결, 암호화와 코덱 대체 동작을 연결하고 검증합니다. | 코어·워커·UI 구현 및 교차 검토 완료; 2계정 실서버 SFU 검증 필요 |
| 4. Rust UI와 앱 기능 | 로그인, 채널·메시지, 첨부 파일, 설정, 알림과 음성·영상 UI를 실제 서비스에 연결합니다. 한글 IME, 접근성과 긴 목록도 검증합니다. | GPUI 클라이언트 진행 중(위 진행도) |
| 5. 네이티브 셸 안정화 | 입력, 트레이, 알림, 파일, 저장, 권한과 오디오의 정상·실패·종료 동작을 기존 기능과 비교합니다. | 일부 구현 중; 기능별 실기 검증 필요 |
| 6. 최종 전환 | 실제 계정의 핵심 기능과 H.265 공유를 검증한 뒤 웹 런타임을 제거하고, 동일 조건에서 성능과 회귀를 비교합니다. | 미완료 |

전환기에는 기존 웹 UI를 유지하면서 Rust 기능을 단계적으로 검증합니다. UI 프레임워크는 2026-11-17에 GPUI + gpui-kit으로 확정했습니다(가변 높이 가상 목록·Markdown 선택 성능 측정, Windows 한글 IME 실기 확인). 실시간 영상 표시는 아직 검증하지 않았습니다.

더 자세한 단계별 작업과 수락 기준은 개발 작업 공간의 `RUST_FULL_MIGRATION_PLAN.md`와 `RUST_DESKTOP_EXECUTION_PLAN.md`에 정리하고 있으며 아직 이 브랜치에 공개되지 않았습니다.

## 빌드

Windows와 Rust 툴체인이 필요합니다. 주력 GPUI 클라이언트는 위 `fluxer_client/` 명령으로 빌드·실행하며, 릴리스 패키지는 루트의 `scripts/package_fluxer_client.ps1`로 만듭니다. 아래 명령은 별도 WebView2 전환기 셸을 대상으로 합니다.

```powershell
cargo test --manifest-path fluxer_desktop_rs/Cargo.toml
cargo build --release --manifest-path fluxer_desktop_rs/Cargo.toml --target-dir target
```

빌드 결과물은 `target/release/fluxer.exe`입니다. 기존 배포본은 [GitHub Actions](https://github.com/CJsw12/fluxer/actions)에서 확인할 수 있으며, 각 아티팩트가 현재 작업 내용과 일치하는지는 해당 실행의 커밋을 확인해야 합니다.

## 라이선스

원본 프로젝트는 [Fluxer](https://github.com/fluxerapp/fluxer)입니다. 저장소의 라이선스는 [AGPL-3.0-or-later](LICENSE)를 참조하세요. 별도 라이선스가 적용되는 서드파티 구성 요소는 해당 구성 요소의 고지를 따릅니다.
