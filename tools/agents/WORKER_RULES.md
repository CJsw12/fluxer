# 하위 에이전트(워커) 공통 규칙

루트가 맡긴 UI 1:1 작업을 시작하기 전에 이 파일을 읽는다. 브리프와 충돌하면 브리프가 우선한다.

## 판정·구현
- 판정 기준은 공식 앱 실측 수치(명세 JSON·`spec_delta` 결과)뿐이다. 소스나 기억으로 추측하지 않는다.
- 색은 하드코딩하지 않고 `Theme` 필드를 쓴다(다크·라이트 모두). 측정 태그 `.flx()`는 `.border_*()` 뒤에 둔다.
- 공식 앱에 없는 숨은 호버 버튼(`opacity(0).group_hover(...)`)을 만들지 않고, 다른 기능 버튼에 같은 핸들러를 복붙하지 않는다. 기능이 없는 항목은 고유 핸들러(닫기만) + `TODO(backlog)`.

## 컨텍스트 절약
- 300줄이 넘는 파일은 `rg -n`으로 위치를 찾은 뒤 그 범위만 읽는다(`read_file`에 `StartLine`/`EndLine` 필수). 같은 파일을 여러 번 통째로 읽지 않는다.
- 명령 출력이 잘리면 출력 파일(`.system_generated/steps/*/output.txt`, 게이트 로그)을 통째로 읽지 말고, 필터(`--flx`, `--scene`, `--limit`, `--fields`)를 좁혀 다시 실행하거나 `rg`/`Select-Object -Last`로 필요한 줄만 본다.
- 명세·래칫·차이 JSON은 즉석 파이썬 대신 `python -I tools/parity_lab/spec_query.py`(`baseline` · `spec` · `delta`)로 조회한다.
- 도구 소스코드(`tools/parity_lab/*.py`, `*.mjs`)는 읽지 않는다. 사용법은 `--help`로 본다.

## 빌드·실기
- 수정 후 재측정은 `python -I tools/parity_lab/cycle.py --skip-tests`로 한다(빌드 디렉터리는 `target/fc-root` 하나, `jobs=4`). 클라이언트는 `cycle.py`·`env.py`가 사용자 화면에 띄우므로 직접 실행 파일을 띄우지 않는다.
- 실기 클라이언트를 조작하는 에이전트는 동시에 하나뿐이다. 모든 장시간 명령에 시간 제한을 둔다.

## 금지
- `Logs/`, `../Logs`, `%APPDATA%`, 키·자격 증명 접근, 커밋·push, 사용자 개인 공식 앱 조작, `SSL_CERT_FILE` 설정.

## 보고
- 25줄 이내: 수정 전후 표, 남은 차이와 이유, 테스트 합계, 끝 상태(클라이언트 실행 여부).
