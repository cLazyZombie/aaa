# 터미널 색 테마

설정 → 터미널 → 「터미널 색 테마」에서 고른다. 이 기기에만 저장하며 같은 브라우저의 다른 탭은 변경을 따라간다.

| 선택 | 저장 이름 | 동작 |
| --- | --- | --- |
| 앱 팔레트 따름 | `app`(기본, 저장 키 제거) | 기존처럼 앱 밝기·팔레트의 터미널 색을 쓴다 |
| GitHub Dark | `wezterm-github-dark-user` | 앱 팔레트와 별개로 고정된 터미널 색을 쓴다 |

이 선택 자체는 앱의 머리·목록·대화·입력창 색을 바꾸지 않는다. 앱 설정 → 모양에서도 같은 이름의
[GitHub Dark](github-themes.md)를 고를 수 있다. 터미널의 기존 저장 ID는 호환성을 위해 유지한다. 터미널 안 글자·ANSI 16색·커서·선택·여백·앞 기록 덮개는 고른 터미널 색을 쓴다.
프로그램이 지정한 truecolor와 ANSI 256색의 16~255는 바꾸지 않는다. OSC 10/11의 기본 글자·배경색 응답도 그 화면이 고른 색이다.
여러 웹 화면이 같은 pane을 보면 기존처럼 마지막으로 색을 알린 화면의 기본색으로 session host가 답한다.

## WezTerm 사용자 색

WezTerm 정본 `e2a98e1b8af156988ad0ac778658ac27b60a55fb`의
[`config/src/scheme_data.rs`](https://github.com/wezterm/wezterm/blob/e2a98e1b8af156988ad0ac778658ac27b60a55fb/config/src/scheme_data.rs)와
[`docs/colorschemes/data.json`](https://github.com/wezterm/wezterm/blob/e2a98e1b8af156988ad0ac778658ac27b60a55fb/docs/colorschemes/data.json)의 `GitHub Dark`를 쓴다.
사용자 지시대로 배경만 `#24292e`, 기본 글자만 `#C9D1D9`로 바꾼다.

| 역할 | 값 |
| --- | --- |
| ANSI 0~7 | `#000000 #f78166 #56d364 #e3b341 #6ca4f8 #db61a2 #2b7489 #ffffff` |
| ANSI 8~15 | `#4d4d4d #f78166 #56d364 #e3b341 #6ca4f8 #db61a2 #2b7489 #ffffff` |
| 커서·커서 테두리 | `#c9d1d9` |
| 커서 아래 글자 | `#101216`(원본 유지) |
| 선택 배경·글자 | `#3b5070`, `#ffffff` |

iTerm2-Color-Schemes **모음**의 라이선스는 MIT지만, 이 개별 테마의 원저자는 Al Duncanson이고 원 저장소는 GPL-3.0이다.
원본 iTerm 색 파일·WezTerm 색 수치·출처·변경 내역을 `web/public/terminal-themes/`에, 두 라이선스 원문을 `web/public/licenses/`에 함께 둔다.
색 자료의 원 고지를 보존하며 외부 실행 코드는 가져오지 않았다. 이를 모음의 MIT만으로 고지하지 않는다.

VS Code의 [통합 터미널 색 설정](https://code.visualstudio.com/api/references/theme-color#integrated-terminal-colors)처럼 앱 모양과 터미널 기본/ANSI 색을 분리한다.
xterm.js의 [`ITheme`](https://xtermjs.org/docs/api/terminal/interfaces/itheme/)과
[5.5 demo의 theme 사용](https://github.com/xtermjs/xterm.js/blob/5.5.0/demo/client.ts)을 참고해, renderer 변경 없이 같은 테마 값을 그리기와 OSC 응답에 사용한다.
VS Code·xterm.js·WezTerm의 구현 라이선스는 MIT다. [추가 앱 테마 12종](theme-catalog.md)은 herdr처럼 앱·터미널을 묶고, ‘앱 팔레트 따름’에서 해당 원본 스킴을 쓴다. 독립 터미널 선택은 기존 두 개를 유지한다. 스킴 편집·가져오기는 추가하지 않는다.
