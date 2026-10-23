// 설정을 거른 뒤 밝기·시스템 모드에서 GitHub 짝을 고르는 계약. DOM·host 동기화는 E2E 14가 맡는다.
import assert from "node:assert/strict";
import { it } from "node:test";
import { paletteFor, sanitizeAppearance } from "../src/theme.ts";
import fs from "node:fs";
import { THEME_CATALOG } from "../src/themeCatalog.ts";
import { catalogTokens } from "../src/catalogTheme.ts";

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
    assert.deepEqual(Array.from({ length: 16 }, (_, i) => tokens[`--ansi-${i}`]), [...theme.terminal.ansi, ...theme.terminal.brights]);
  }
});
