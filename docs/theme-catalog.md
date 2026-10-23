# 추가 앱 테마

설정 → 모양 → 색상에서 고른다. ‘테마 미리보기’를 펼치면 18개 선택의 색 견본과 이름을 보고 고를 수 있다.
이름·밝음·어두움으로 검색하며 키보드로도 선택한다. 기존 6개에 밝음·어두움 6쌍(12종)을 더했다.
새 테마를 고르면 해당 밝기로 전환하고, 이후 밝기 전환은 같은 계열의 짝을 고른다.
시스템 모드의 라이트·다크 팔레트는 각각 고른다. 공유 범위와 저장 형식은 기존 herdr 방식 그대로다.

## 참고 구조

먼저 herdr `2ecf19a92fc3a7fff75b6ce3655ef04f4625ec01`의 `src/config/theme.rs` 이름·별칭,
`src/app/state.rs`의 `Palette`, herdr-web-ui `aa624cb`의 `src/lib/settings.ts`·`paletteFor.ts`·`src/styles.css`,
herdr-projects의 CLI 구조를 확인했다. 네이티브 herdr는 이름 있는 개별 밝기 테마를 제공하고,
웹은 앱 팔레트에 터미널 색을 묶으며 시스템 밝기의 팔레트를 따로 고른다. herdr-projects에는 별도 색 목록이 없다.

따라서 새 테마도 **앱 팔레트**로 추가한다. ‘앱 팔레트 따름’ 터미널은 그 스킴의 기본색·ANSI 16색·커서·선택 색을 쓴다.
기기별 독립 터미널 선택은 기존 ‘앱 팔레트 따름’·‘GitHub Dark’를 유지한다.
herdr 웹의 세 팔레트만으로는 요청한 목록이 없어서, 네이티브 herdr의 원색 역할을 기존 웹 token으로 옮겼다.

## 목록·출처·라이선스

모든 UI 원색은 위 herdr `Palette`에서 가져왔다(Apache-2.0). 원본 역할 자료와 터미널 원색·출처는
배포 파일 `web/public/theme-catalog/sources.json`에 남긴다. 런타임 자료는 `web/src/themeCatalog.ts`다.
터미널 자료는 [WezTerm 내장 scheme 정본](https://github.com/wezterm/wezterm/blob/e2a98e1b8af156988ad0ac778658ac27b60a55fb/docs/colorschemes/data.json)을 대조했다.
[iTerm2-Color-Schemes](https://github.com/mbadolato/iTerm2-Color-Schemes)와
[VS Code Tokyo Night](https://github.com/enkia/tokyo-night-vscode-theme),
[Dracula](https://github.com/dracula/visual-studio-code), [Nord](https://github.com/nordtheme/visual-studio-code)도 조사했다.
모음의 라이선스만으로 개별 테마를 판단하지 않는다.

| 앱의 밝음 ↔ 어두움 | 원 테마·허가 | 사용한 터미널 자료 |
| --- | --- | --- |
| Tokyo Night Day ↔ Tokyo Night | [tokyonight.nvim](https://github.com/folke/tokyonight.nvim), Apache-2.0 | WezTerm `Tokyo Night Day`·`Tokyo Night` |
| One Light ↔ One Dark | [Atom One Light](https://github.com/atom/one-light-syntax)·[One Dark](https://github.com/atom/one-dark-syntax), MIT | WezTerm `One Light (Gogh)`(Gogh MIT)·[onedark.vim](https://github.com/joshdick/onedark.vim)(MIT)의 16색·기본색·커서·선택색 |
| Solarized Light ↔ Solarized Dark | [Ethan Schoonover Solarized](https://github.com/altercation/solarized), MIT | WezTerm `Builtin Solarized Light`·`Builtin Solarized Dark`; iTerm2 모음 MIT |
| Gruvbox Light ↔ Gruvbox Dark | [morhetz Gruvbox](https://github.com/morhetz/gruvbox), README의 MIT/X11 명시 | WezTerm `GruvboxLight`·`GruvboxDark`; iTerm2 모음 MIT |
| Kanagawa Lotus ↔ Kanagawa Wave | [Kanagawa](https://github.com/rebelot/kanagawa.nvim), MIT | [Lotus Alacritty 정본](https://github.com/rebelot/kanagawa.nvim/blob/master/extras/alacritty/kanagawa_lotus.toml)·WezTerm `Kanagawa (Gogh)`; Gogh MIT |
| Rosé Pine Dawn ↔ Rosé Pine | [Rosé Pine](https://github.com/rose-pine/neovim), MIT | WezTerm `Rosé Pine Dawn (Gogh)`·`Rosé Pine (Gogh)`; Gogh MIT |

WezTerm 구현의 MIT, 원 테마·Gogh·herdr의 라이선스 원문과 변경 고지를 `web/public/licenses/`에 넣었다.
Gruvbox 원본은 별도 LICENSE가 없어 README의 MIT/X11 선언을 `gruvbox-NOTICE.txt`에 보존한다.
기존 Catppuccin의 MIT 고지도 포함했다. **기존 독립 GitHub Dark 원본의 GPL-3.0**과 iTerm2 모음 MIT·Primer MIT도 남긴다.
새 12종에 GPL 자료는 추가하지 않았다. 원본 실행 코드·새 외부 의존성은 추가하지 않는다.

## 원색과 대비 보정

- 바탕·판은 herdr `panel_bg`, 팝업은 `surface0`, hover는 `surface1`, 입력 바탕은 `panel_bg`다.
- 글자는 `text`, 보조 글자는 `subtext0`, 강조는 `accent`, 작업 중은 `blue`, 확인 필요는 `red`, 완료는 `green`, 주의는 `yellow`다.
  구문 색도 herdr의 mauve·green·peach·yellow·teal 역할을 쓴다.
- herdr-web-ui Catppuccin의 ‘legibility first’를 따른다. UI 글자·상태·강조·구문색이 모든 면에서 4.5:1을 만족하도록
  원색을 흰색(어두운 테마)·검정(밝은 테마) 방향으로 최소 혼합한다. 바탕과 원색 자료는 바꾸지 않는다.
  선택·주의·위험 tint는 기존 웹과 같은 12% 혼합이며, 주 단추 글자는 검정/흰색 중 4.5:1을 넘는 것을 쓴다.
- 터미널 원 ANSI 16색은 보정하지 않는다. 일부 black·bright black·밝은 테마의 white·색 글자는 4.5:1 미만이다. Solarized Light의 원 터미널 기본 글자도 4.13:1이다.
  이것은 요청한 UI 대비와 원 터미널 스킴을 분리한 기존 독립 GitHub Dark 계약과 같다.
  WezTerm `One Dark (Gogh)`의 기본 글자는 `#5c6370`으로 어두워, 같은 One Dark 원색을 쓰는 onedark.vim 정본(`bg #282c34`, `fg #abb2bf`)을 선택했다. 원색을 임의로 밝히지 않았다.
  원 스킴에 선택 색이 없으면 herdr `selection_bg`와 원 터미널 글자색, Lotus의 커서는 원 글자/바탕을 쓴다.
  임의 RGB·ANSI 16~255·탭 바 자료는 가져오지 않는다.
- 앱 색 복원은 [GitHub 테마](github-themes.md)에 적었다. 예전 CSS 팔레트로 돌아오면 추가 inline token을 모두 지운다.

## 검증과 다음 묶음

E2E96은 실제 host·agent·PTY, Chromium·WebKit 각각 1280·390px에서 12종 선택·저장·재열기·전체 UI 대비·
원 터미널 ANSI와 변경 frame/초기 연결의 OSC 기본색·OS 전환·기존 팔레트 복귀를 확인한다.
검색·빈 결과·미리보기 선택·카드 겹침·가로 넘침도 검사한다. 각 테마와 갤러리의 화면을 캡처한다.
E2E95는 이전 앱 색 원본(adf056a9)을 넣기 전후 모든 token의 렌더 색과 독립 터미널 보존·대화·질문 카드를 검사한다.

첫 묶음은 원본 밝기 짝이 모두 있는 12종으로 한정했다. 후속 후보는 Dracula, Nord, Vesper(어두움 전용),
Catppuccin Frappé·Macchiato, Tokyo Night Storm·Moon, Rosé Pine Moon, Gruvbox hard·soft다.
어두움 전용 테마에 임의 밝은 짝을 만들지 않는다. 외부 테마 가져오기·편집기는 별도 범위다.
