# GitHub 테마

웹 설정 → 모양 → 색상에서 고른다. 기본 테마는 amber다.

| 표시 이름 | 저장 이름 | 기본 바탕 |
|---|---|---|
| GitHub Light | `github-light` | `#ffffff` |
| GitHub Dark | `github-dark` | `#0d1117` |
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

작은 글자의 가독성을 위해 주 글자·보조 글자·강조·상태색은 기본·판·hover 바탕 모두에서 대비 4.5:1 이상인
같은 Primer 색 계열의 가장 가까운 밝기로 보정했다. ANSI 16색도 기본 터미널 바탕과 4.5:1 이상으로 보정했다.
밝은 테마의 white/bright white는 어두운 회색이다. Dimmed의 보조 글자는 hover 위 대비를 위해 fg.default까지 밝힌다.
이것이 원본과 다르게 둔 이유다. 명시 RGB와 ANSI 256색 중 16~255는 프로그램이 지정한 값을 유지한다.

## 확인

웹 `14-shared-preferences` E2E에서 세 이름 선택·host 저장·다시 열기와 휴대폰·데스크톱 표시를 확인한다.

TUI 테마 구현은 2026-10-19 삭제했다. 보존 tag `tui-last`와 복구 방법은 [삭제 기록](removed-tui.md)을 본다.
