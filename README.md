# Fluxer (Rust Native Desktop)

공식 [Fluxer](https://github.com/fluxerapp/fluxer) 웹/백엔드와 100% 호환되는 Windows용 초경량 Safe Rust 네이티브 데스크톱 클라이언트입니다.

---

## 주요 특징

- **0ms 창 드래그 레이턴시**: 마우스 이벤트 직렬화 병목 없이 Win32 네이티브 `drag_window` 파이프라인 직결
- **초경량 바이너리 (5.19 MB)**: Node.js 메인 런타임을 완전히 제거하여 메모리 점유 최소화 및 즉시 부팅
- **완전한 호환성**: 공식 인스턴스 및 셀프호스트 인스턴스 선택/전환, 안전한 뒤로가기 탐색 완벽 지원
- **공식 1:1 트레이 메뉴**: 숨기기/보이기, 설정, 상태(온라인/자리비움/방해금지/오프라인), 업데이트 확인, 재시작, 종료
- **Rust Red 테마**: 투명 마스크가 보존된 32-bit DIB Rust Red(`#CE412B`) 아이콘 적용

---

## 실행 방법

1. [Actions](https://github.com/CJsw12/fluxer/actions) 탭의 최신 빌드에서 `Fluxer-Windows-x64` 아티팩트를 다운로드합니다.
2. 실행:
   - 기본 실행 (인스턴스 선택 화면): `Fluxer.exe`
   - 특정 인스턴스 직접 실행: `Fluxer.exe https://your-fluxer-instance.org`

---

## 빌드 방법

Windows 환경에서 Rust 툴체인이 설치되어 있어야 합니다.

```sh
# 단위 테스트
cargo test -p fluxer_desktop_rs

# 릴리스 빌드
cargo build --release -p fluxer_desktop_rs
```

빌드 결과물은 `target/release/Fluxer.exe`로 생성됩니다.

---

## 라이선스

- 원본 프로젝트: [Fluxer](https://github.com/fluxerapp/fluxer)
- 본 저장소의 코드는 원본과 동일하게 [AGPL-3.0-or-later](./LICENSE) 라이선스를 따릅니다.
