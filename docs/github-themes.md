# GitHub 테마

웹 설정 → 모양 → 색상에서 고른다. 기본 테마는 amber다.

| 표시 이름 | 저장 이름 | 기본 바탕 |
|---|---|---|
| GitHub Light | `github-light` | `#ffffff` |
| GitHub Dark | `github-dark` | `#24292e` |
| GitHub Dark Dimmed | `github-dark-dimmed` | `#22272e` |

웹에서 고르면 이름의 밝기도 함께 고른다. 이후 밝기 단추·시스템 설정은 기존처럼 동작한다. Light의 어두운 짝은 Dark,
Dark·Dimmed의 밝은 짝은 Light다. 시스템 설정의 밝음·어두움 팔레트도 같은 규칙을 쓴다. host 공유 설정과 기기 저장 형식은 그대로다.

## 출처와 색 역할

- [Primer primitives 7.10.0](https://github.com/primer/primitives/tree/v7.10.0), MIT.
  `dist/json/colors/light.json`·`dark.json`·`dark_dimmed.json`의 `canvas`, `fg`, `border`, `accent`, `success`,
  `attention`, `danger`, `done`, `severe`, `ansi`, `prettylights.syntax`를 쓴다.
  배포된 JSON은 `https://unpkg.com/@primer/primitives@7.10.0/dist/json/colors/`에서도 확인할 수 있다.
- [GitHub 공식 VS Code 테마 colors.js](https://github.com/primer/github-vscode-theme/blob/main/src/colors.js)·
  [package.json](https://github.com/primer/github-vscode-theme/blob/main/package.json)이 같은 Primer 7.10.0과 세 색 파일을 사용한다.
  그 저장소의 임시 override는 적용하지 않는다. 고지는 `web/public/licenses/primer-primitives-LICENSE.txt`에 포함했다.
- herdr 0.9.5 `src/config/theme.rs`(`THEME_NAMES`, `canonical_theme_name`)·`src/app/state.rs`(`Palette`)와
  herdr-web-ui aa624cb `src/lib/settings.ts`(`paletteFor`, `terminalTheme`)·`src/styles.css`의 역할을 유지한다.
  그 두 참고에는 GitHub 팔레트가 없다. t3code `apps/web/src/vscodeThemeImport.ts`는 외부 VS Code 테마의 밝기 짝을 묶으며,
  `vscodeThemeImport.test.ts`의 GitHub 이름·Dimmed 사례를 참고했다. 이번에는 외부 테마 가져오기 기능은 추가하지 않는다.

표면은 canvas.default/subtle, 주 글자는 fg.default, 보조 글자는 fg.muted, 강조·작업 중은 accent,
확인 필요·오류는 danger, 완료는 success, 주의는 attention이다. 웹은 `github-themes.css`를 쓴다.

## 사용자 GitHub Dark 통합(t-0889)

사용자 2026-10-23 지시로 기존 `github-dark`를 터미널의 GitHub Dark에 맞춘다. 같은 이름을 중복 추가하지 않는다.
앱의 기본 바탕·입력 바탕은 `#24292e`, 주 글자는 `#c9d1d9`이고, 터미널의 ANSI 16색·커서·선택 색도
[터미널 색 자료](terminal-themes.md)와 같다. 터미널에 없는 앱의 판·테두리·강조·상태·구문 색은
Primer primitives 7.10.0 `dark.json` 값을 쓴다. 예를 들어 판은 `canvas.subtle` `#161b22`,
테두리는 `border.default` `#30363d`, 강조는 `accent.fg` `#58a6ff`다.
새 배경에서 `danger.fg` `#f85149`의 대비가 4.38:1이므로 오류·삭제 글자와 확인 필요 표지는
같은 Primer `scale.red[3]` `#ff7b72`로 높여 기존 UI 대비 4.5:1을 유지한다.
원 터미널 테마의 GPL-3.0·모음 MIT 고지는 유지하고, Primer MIT 고지는
`web/public/licenses/primer-primitives-LICENSE.txt`에 그대로 배포한다.
[VS Code GitHub Theme](https://github.com/primer/github-vscode-theme/blob/main/src/colors.js)의 Primer 역할 매핑도 확인했다.
새 의존성·설정 구조·host 계약은 추가하지 않는다. 기존 밝기 짝과 시스템 모드 규칙을 유지한다.

주 글자·보조 글자·강조·상태색은 기본·판·hover 바탕 모두에서 대비 4.5:1 이상인
같은 Primer 색 계열의 가장 가까운 밝기로 보정했다. Light·Dimmed의 ANSI 16색도 터미널 바탕과 4.5:1 이상이다.
밝은 테마의 white/bright white는 어두운 회색이다. Dimmed의 보조 글자는 hover 위 대비를 위해 fg.default까지 밝힌다.
이것이 Light·Dimmed에서 원본과 다르게 둔 이유다.
사용자 지정 Dark의 ANSI는 원색을 우선하므로 black·bright black·cyan은 4.5:1 미만일 수 있다.
UI 본문·보조 글자·강조·상태색은 기존 대비 기준을 유지한다. 명시 RGB와 ANSI 256색 중 16~255는 프로그램이 지정한 값을 유지한다.

## 확인

웹 E2E14는 Chromium·WebKit의 데스크톱·휴대폰에서 세 이름 선택·host 저장·다시 열기·UI 대비를 확인한다.
E2E95는 기존 기기 저장값 복원·이름 통일·앱과 독립 터미널 색 일치 및 실제 대화·질문 카드 화면을 확인한다.

TUI 테마 구현은 2026-10-19 삭제했다. 보존 tag `tui-last`와 복구 방법은 [삭제 기록](removed-tui.md)을 본다.
