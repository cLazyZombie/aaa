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

## 앱 색 복원과 독립 터미널(t-0899)

사용자 지시로 앱 팔레트는 t-0889 이전(adf056a9)으로 복원했다. 기본·입력·PWA 바탕은 `#0d1117`,
오류·삭제·확인 필요 빨강은 `#f85149`다. `github-themes.css`의 색 정의 전체가 그 기준과 같다.
‘앱 팔레트 따름’ 터미널도 원래 앱 ANSI·커서·선택 색으로 돌아간다.
독립 터미널 선택 `GitHub Dark`는 배경 `#24292e`, 글자 `#c9d1d9`, 원 ANSI 16색과 기존 저장 ID를 유지한다.

앱 주 글자·보조 글자·강조·상태·ANSI는 원래 보정된 Primer 색으로 기본·판·hover에서 4.5:1 이상이다.
독립 GitHub Dark 터미널의 ANSI는 사용자 지정 원색을 보존하므로 black·bright black·cyan은 이 기준 밖이다.
GPL-3.0 원 테마·모음 MIT·Primer MIT 고지를 모두 유지한다. 명시 RGB·ANSI 256색 중 16~255도 그대로다.

## 확인

웹 E2E14는 Chromium·WebKit의 데스크톱·휴대폰에서 세 이름 선택·host 저장·다시 열기·UI 대비를 확인한다.
E2E95는 기존 기기 저장값 복원·이름 유지·앱과 독립 터미널의 색 분리 및 adf056a9의 전체 색 token 일치 및 실제 대화·질문 카드 화면을 확인한다.

TUI 테마 구현은 2026-10-19 삭제했다. 보존 tag `tui-last`와 복구 방법은 [삭제 기록](removed-tui.md)을 본다.
