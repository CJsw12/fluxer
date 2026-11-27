# 하위 에이전트(워커) 공통 규칙

루트가 맡긴 UI 1:1 작업을 시작하기 전에 이 파일을 읽는다. 브리프와 충돌하면 브리프가 우선한다.

## 판정·구현
- 판정 기준은 공식 앱 실측 수치(명세 JSON·`spec_delta` 결과)뿐이다. 소스나 기억으로 추측하지 않는다.
- 색은 하드코딩하지 않고 `Theme` 필드를 쓴다(다크·라이트 모두). 측정 태그 `.flx()`는 `.border_*()` 뒤에 둔다.
- 공식 앱에 없는 숨은 호버 버튼(`opacity(0).group_hover(...)`)을 만들지 않고, 다른 기능 버튼에 같은 핸들러를 복붙하지 않는다. 기능이 없는 항목은 고유 핸들러(닫기만) + `TODO(backlog)`.

## 컨텍스트 절약
- 300줄이 넘는 파일은 `rg -n`으로 위치를 찾은 뒤 **해당 함수·섹션 전체(150~300줄)를 1턴에 병렬로** 읽는다(`read_file`에 `StartLine`/`EndLine` 필수). 30~50줄씩 여러 턴 나눠 읽지 않고, 고친 뒤 확인은 `rg -n`으로 바꾼 줄만 보며 이미 읽은 구간을 다시 읽지 않는다.
- 명령 출력이나 임시 파일이 8KB를 넘으면 통째로 읽지 말고, 필터(`--flx`, `--scene`, `--limit`, `--fields`)를 좁혀 다시 실행하거나 `rg`/`Select-Object`로 필요한 줄만 본다.
- 검색은 좁게: `rg -l`(파일만)·`rg -c`(개수)로 먼저 좁히고, 본문 검색은 파일·경로를 지정하고 `-m 3` 이하·`-A/-B` 2줄 이하로 한다. 출력이 6,000자를 넘을 것 같으면 나눠서 실행한다.
- 명세·래칫·차이 JSON은 즉석 파이썬 대신 `python -I tools/parity_lab/spec_query.py`(`baseline` · `spec` · `delta`)로 조회한다.
- 도구 소스코드(`tools/parity_lab/*.py`, `*.mjs`)는 읽지 않는다. 사용법은 `--help`로 본다.

## 빌드·실기
- `cargo check`/`cargo test`는 루트에서 `--manifest-path fluxer_client/Cargo.toml -p fluxer_client_app -j 4`를 쓴다(`CARGO_TARGET_DIR=<repo>/target/fc-root`, `jobs=4`). 병렬 `swarm` 구현 워커는 브리프에 별도 지시가 없으면 `cargo`를 생략하고 `check_colors.py`·`check_icons.py`만 돌린다(동시 락·형제 미완성 파일 충돌 방지).
- 수정 후 실기 재측정이 필요하면 `python -I tools/parity_lab/cycle.py --skip-tests`로 한다. 클라이언트는 `cycle.py`·`env.py`가 띄우므로 직접 실행 파일을 띄우지 않는다.
- 기존 UI 행 수·칩 인덱스·표시용 콤보(`default_shortcut_rows` 등)는 명시적 요청 없이 바꾸지 않고, 수정으로 대상 장면의 `deltas`/`missing`이 늘면 즉시 원복한다.
- 실기 테스트 후 반드시 원래 길드 채널·다크 테마·음성 퇴장·마이크 켜짐(`Mute microphone`) 상태로 복원한다. 아이콘 줄 번호가 바뀌면 `check_icons.py` 결과를 보고한다.
- 실기 클라이언트를 조작하는 에이전트는 동시에 하나뿐이다. 모든 장시간 명령에 시간 제한을 둔다.

## 금지
- `Logs/`, `../Logs`, `%APPDATA%`, 키·자격 증명 접근, 커밋·push, 사용자 개인 공식 앱 조작, `SSL_CERT_FILE` 설정.

## 보고
- `swarm` 워커는 종료 시 반드시 `@coordinator` 멘션으로 25줄 이내 보고한다: 수정 전후 표, 남은 차이와 이유, 검사 합계, 끝 상태(클라이언트·복원 여부), **실수·실패·낭비(Friction, 0~3줄: 실패한 명령·재시도·시간/토큰 낭비 지점·다음 워커 주의점)**.
