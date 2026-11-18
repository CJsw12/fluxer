# gpui_spike

GPUI 0.2.2로 Fluxer 메인 UI를 만들 수 있는지 판단하기 위한 최소 시제품입니다. 독립 Cargo 워크스페이스이며 루트 워크스페이스와 무관합니다.

- 위쪽: 가변 높이 메시지 2만 개(한글·영문·이모지, 1~6줄)를 `list`/`ListState`(Bottom 정렬)로 표시합니다.
- 아래쪽: `EntityInputHandler` 기반 한 줄 입력입니다. IME 조합 문자열은 밑줄로 표시하고, `bounds_for_range`로 후보창 위치를 제공합니다. Enter를 누르면 목록 끝에 메시지를 추가하고 맨 아래로 스크롤합니다.

## 빌드와 실행

```
cd tools/gpui_spike
set CARGO_TARGET_DIR=%REPO%\target\gpui_spike
cargo build --release
..\..\target\gpui_spike\release\gpui_spike.exe            :: 수동 확인용
..\..\target\gpui_spike\release\gpui_spike.exe --bench    :: 자동 스크롤 후 종료
..\..\target\gpui_spike\release\gpui_spike.exe --bench --step=500   :: 프레임당 스크롤 px 지정(기본 1500)
```

`--bench`는 맨 위로 이동한 뒤 프레임마다 `step` px씩 끝까지 스크롤하고, 경과 시간, 렌더 프레임 수(루트 뷰 render 호출 수), PeakWorkingSetSize와 PrivateUsage를 stdout에 출력합니다. 100초 안에 끝나지 않으면 timeout으로 종료 코드가 1입니다.

## 한글 IME 수동 확인 체크리스트 (미검증)

Microsoft 한글 IME를 켠 상태로 위 실행 파일(`--bench` 없이)을 띄워 하단 입력창에 입력합니다.

1. 조합 중 밑줄: `ㅎ`, `ㅏ`, `ㄴ`을 차례로 눌러 조합 중인 글자에만 밑줄이 표시되고, 확정하면 밑줄이 사라지는지 확인합니다.
2. 후보창 위치: 조합 중 한자 키나 이모지 패널(Win+.)을 열어 후보창이 입력창의 현재 커서 바로 아래에 뜨는지, 글자가 늘어나면 따라 이동하는지 확인합니다.
3. 백스페이스 자모 단위 삭제: 조합 중 `한` 상태에서 백스페이스를 누르면 `하`, `ㅎ`, 빈 상태 순으로 줄어드는지 확인합니다.
4. 조합 중 Enter: 조합 중인 글자가 있을 때 Enter를 누르면 글자만 확정되고 메시지는 전송되지 않는지, 한 번 더 Enter를 누르면 전송되어 목록 맨 아래에 추가되는지 확인합니다.
5. 마우스 클릭으로 커서 이동: 긴 문장 중간을 클릭하면 커서가 그 위치로 이동하고, 이후 입력이 그 위치에 삽입되는지 확인합니다. 조합 중 클릭했을 때 조합이 깨지지 않는지도 봅니다.
6. 보너스: 입력 후 목록이 맨 아래로 이동하는지, 마우스 휠로 위아래 스크롤이 부드러운지, 창 크기 변경 시 행 높이가 다시 계산되는지 확인합니다.
