# 에이전트 작업 안내서 (시작할 때 이것만 읽는다)

`docs/PARITY_LAB.md`는 배경·근거·과거 기록이 담긴 참고서다. 시작할 때 전체를 읽지 않고, 이 안내서에 없는 것이 필요할 때만 해당 절을 찾아 읽는다.

## 1. 측정 전 점검 (매번)
`python -I tools/parity_lab/verify_all.py --only precheck` 한 줄로 확인한다(Rust `Online`·멤버 목록 `Online—2`·양쪽 마이크·스피커 켜짐·CDP 9334). 실패하면 측정하지 말고 환경부터 고친다.
- 공식 앱 게이트웨이가 끊겨 상대 계정이 오프라인 → CDP `Page.reload`. 터널이 끊겼다 돌아왔으면 Rust 화면의 오류 상태(탐색 등)를 다시 시도한 뒤 측정.
- 공식 비교 앱은 항상 `FLUXER_DISABLE_MODULE_UPDATES=1`로 띄운다(버전이 바뀌면 전체 명세 재캡처).
- Antigravity 명령은 숨은 데스크톱에서 돈다. 거기서 띄운 클라이언트는 사용자 화면에 없으므로, 끝낼 때 "클라이언트 재기동 필요"를 보고한다.

## 2. 규칙
- **판정 기준:** 공식 앱 실측 수치만. 소스·기억으로 추측하지 않는다. 공식 앱은 tester2, Rust는 tester1이라 본인 시점·상대 이름·색 차이는 결함이 아니다.
- **루트는 구현·디버깅하지 않는다.** 기능 1개 = 새 하위 에이전트 1개. 실패하면 원인을 담아 브리프를 좁혀 다시 맡기고, 2회 실패하면 건너뛰고 기록한다.
- **위임 등급:** 여러 시스템(REST·상태·UI) 설계 = Neon급(Claude는 Sonnet), 명세 수치 맞추기·태그·스크립트 = Hydrogen급, 전략·T3 검토 = 상위 모델. Antigravity에서 T0 검색·로그 확인은 Flash 실행기가 맡고, DeepSeek은 병렬 독립 작업에만.
- **T3(데이터 삭제·인증·권한·저장 형식):** 구현 전 멈추고 다른 계열 교차 검토. 실기에서 삭제를 실제로 실행하지 않는다(확인 모달까지).
- **시간 제한:** 모든 백그라운드 명령·스크립트에 `timeout`. 같은 대기가 15분을 넘으면 중단.
- **실기 클라이언트는 하나:** 클라이언트를 조작하는 하위 에이전트는 동시에 하나. 동시 빌드 최대 2개, `target/fc-root`만.
- **공식 UX:** 숨은 호버 버튼 금지, 버튼마다 고유 핸들러, 기능 없는 항목은 고유 핸들러(닫기만) + `TODO(backlog)`. 일시적 UI(업데이트 아이콘·알림 막대)는 흉내 내지 말고 조건부로만.
- **색:** 하드코딩 금지, `Theme` 필드(공식 `ThemeVariableManifest.ts`·`ColorSystemPalette.ts` 기준, 다크·라이트 모두). `check_colors.py`가 막는다.
- **GPUI:** `uniform_list` 콜백에서 상태 변경·`cx.notify()` 금지. `.flx()`는 `.border_*()` 뒤에.

## 3. 컨텍스트 예산 (토큰 낭비 방지)
- **도구 소스코드를 읽지 않는다.** 사용법은 아래 4절 표와 `--help`로 충분하다.
- **큰 파일은 범위만:** 300줄 넘는 파일은 `rg -n`으로 위치를 찾은 뒤 그 범위만 읽는다. 같은 파일을 여러 번 통째로 읽지 않는다(특히 `ui/settings.rs`, `client_view.rs`, `backend/voice.rs`, `theme.rs`, `theme_tokens_generated.rs`).
- **검색은 좁게:** `rg -l`(파일만)·`rg -c`(개수)로 먼저 좁히고, 본문 검색은 `-g` 경로 지정과 `-m` 상한을 쓴다.
- **출력은 요약만:** 게이트는 `verify_all.py` 표만 읽고, 실패한 단계의 로그에서 필요한 줄만 `tail`/`grep`. `git diff --stat`, JSON 원문 출력 금지(파이썬 한 줄로 필요한 필드만), 방금 쓴 파일 재독 금지.
- 하위 에이전트는 이 안내서를 읽지 않으므로 브리프에 2·3절의 해당 규칙을 그대로 넣는다.

## 4. 도구 한 줄 사용법 (저장소 루트에서, Python은 `python -I`)
| 목적 | 명령 |
|---|---|
| 커밋 전 전체 검증 | `verify_all.py --build [--worker]` (`--only step,...`로 일부) |
| 개발 루프(빌드·재실행) | `cycle.py [--release] [--worker] [--skip-tests]` (`voice_worker` 변경 시 `--worker`) |
| Rust 조작 | `rust_probe.py resize 1280 800` · `link fluxer://channels/<g>/<c>` · `click <flx> [i]` · `hover <flx>` · `key escape` · `inspect` · `shot out.png` (딥링크 목록 `fluxer_client/app/src/deep_links.rs`) |
| 공식 명세 | `node tools/parity_lab/spec_official.mjs --attach 9334 --viewport 1280x800 --out S.json` / `--scenes <scenes.json> --out-dir D` |
| 수치 차이 | `spec_delta.py S.json --live [--scope <flx>]` / `--scenes <scenes.json> --spec-dir D --live --out-dir O` |
| 누락 분류 | `coverage.py <spec> <delta>` / `--scenes-dir D` |
| 래칫 | `spec_gate.py --live --all` · 하향 `--update` · 상향 `--allow-raise --reason "<사유>"` (+20%·+10 초과는 `--confirm-large`) |
| 아이콘 | `check_icons.py` (호출 위치를 옮기면 `--sync`) |
| 색 | `check_colors.py [--base REF]` |
| 화면 공유 양방향 | `share_check.py --guild G --channel C --out O` (릴리스+워커) |
| 공식 마이크 상태 | `node tools/parity_lab/mic_state.mjs` |
| 테스트 | `cd fluxer_client; CARGO_TARGET_DIR=<repo>/target/fc-root cargo test --workspace -j 4 -- --test-threads=2` |

장면 파일: `tools/parity_lab/spec_scenes.json`(팝오버 8)·`spec_scenes_more.json`(DM·탐색·설정 탭)·`spec_scenes_extra.json`(음성·친구·스레드·라이트)·`spec_scenes_msg.json`(메시지 바). 공식 명세는 `target/parity-tmp/` 아래(래칫 기준 경로는 `spec_baseline.json`).

## 5. 작업 흐름
1. **명세 루프:** 공식 명세 → `spec_delta` 차이 목록 → 하위 에이전트가 고치고 `cycle.py --skip-tests`로 직접 재측정("범위 안 차이 0줄") → 루트가 `verify_all.py --build` → 통과하면 커밋·push → 래칫이 줄었으면 `--update`.
2. **브리프 양식:** 목표 1줄 / 입력 경로(`target/parity-tmp/`) / 루트가 판정한 비결함 / 실제 결함(공식 수치·CSS 규칙·Rust 위치) / 루프 명령 / 회귀 기준 / 소유·금지 파일 / 2·3절 규칙 / 보고 형식(25줄 이내: 수정 전후 표, 남은 차이와 이유, 테스트 합계, 끝 상태).
3. **자동 진행(오케스트레이터):** 다음 기능 선택 → 브리프 → 하위 에이전트 1개 → 보고 → `verify_all.py --build` → 커밋·push → `OPERATIONS.md` 스냅샷 갱신 → 다음 기능. 멈춤 조건: 백로그 소진, 같은 게이트 2회 연속 실패, T3 도달, 외부 승인 필요.
4. **끝낼 때:** 멈춘 이유, 커밋 목록, 기능별 배정 모델표(작업/등급/모델/호출 방식), 남은 백로그, 클라이언트 재기동 필요 여부를 보고하고 `OPERATIONS.md` 스냅샷을 **덮어써서** 최신 상태만 남긴다(지난 기록은 `Logs/handoff/archive/`).
