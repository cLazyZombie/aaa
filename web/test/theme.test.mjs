// 설정을 거른 뒤 밝기·시스템 모드에서 GitHub 짝을 고르는 계약. DOM·host 동기화는 E2E 14가 맡는다.
import assert from "node:assert/strict";
import { it } from "node:test";
import { paletteFor, sanitizeAppearance } from "../src/theme.ts";
import fs from "node:fs";
import { THEME_CATALOG } from "../src/themeCatalog.ts";
import { catalogTokens, contrast } from "../src/catalogTheme.ts";

it("GitHub 색은 저장 후에도 남고, 시스템·수동 밝기의 짝을 고른다", () => {
  for (const palette of ["github-light", "github-dark", "github-dark-dimmed"]) {
    for (const theme of ["light", "dark", "system"]) {
      const a = sanitizeAppearance({ theme, palette, paletteLight: palette, paletteDark: palette });
      assert.equal(a.palette, palette);
      assert.equal(paletteFor(a, "light"), "github-light");
      assert.equal(paletteFor(a, "dark"), palette === "github-dark-dimmed" ? palette : "github-dark");
    }
  }
});

it("새 팔레트는 원본 자료·ANSI를 보존하고 수동·시스템 밝기의 원래 짝을 고른다", () => {
  const source = JSON.parse(fs.readFileSync(new URL("../public/theme-catalog/sources.json", import.meta.url))).themes;
  assert.equal(THEME_CATALOG.length, 12);
  assert.deepEqual(THEME_CATALOG, source);
  for (const theme of THEME_CATALOG) {
    const pair = THEME_CATALOG.find((other) => other.id === theme.pair);
    assert.equal(pair.pair, theme.id);
    assert.notEqual(pair.mode, theme.mode);
    for (const setting of ["light", "dark", "system"]) {
      const a = sanitizeAppearance({ theme: setting, palette: theme.id, paletteLight: theme.id, paletteDark: theme.id });
      assert.equal(a.palette, theme.id);
      for (const mode of ["dark", "light"]) assert.equal(paletteFor(a, mode), theme.mode === mode ? theme.id : pair.id);
    }
    const tokens = catalogTokens(theme.id);
    assert.equal(tokens["--bg"], theme.colors.panel_bg);
    assert.equal(tokens["--term-bg"], theme.terminal.background);
    assert.equal(tokens["--term-fg"], theme.terminal.foreground);
    assert.deepEqual(Array.from({ length: 16 }, (_, i) => tokens[`--ansi-${i}`]), [...theme.terminal.ansi, ...theme.terminal.brights]);
    // 모든 자료의 실제 역할 token이 각 바탕에서 읽혀야 한다. 저장·DOM·재연결은
    // E2E96의 밝음/어두움 대표가 맡아 테마 수만큼 브라우저를 다시 열지 않는다.
    const roles = ["text", "text-dim", "text-strong", "accent", "status-idle", "status-working", "status-blocked", "status-done", "danger-text", "notice", "syntax-comment", "syntax-keyword", "syntax-string", "syntax-function", "syntax-number", "syntax-type", "syntax-variable", "syntax-meta"];
    for (const foreground of roles) for (const background of ["bg", "bg-panel", "bg-elevated", "bg-hover", "bg-input"]) {
      assert.ok(contrast(tokens[`--${foreground}`], tokens[`--${background}`]) >= 4.5, `${theme.id}: ${foreground}/${background}`);
    }
    assert.ok(contrast(tokens["--text-quiet"], tokens["--bg-elevated"]) >= 4.5, `${theme.id}: text-quiet/bg-elevated`);
    for (const background of ["primary", "primary-hover"]) {
      assert.ok(contrast(tokens["--primary-text"], tokens[`--${background}`]) >= 4.5, `${theme.id}: primary-text/${background}`);
    }
  }
});
