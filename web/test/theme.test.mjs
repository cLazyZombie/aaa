// 설정을 거른 뒤 밝기·시스템 모드에서 GitHub 짝을 고르는 계약. DOM·host 동기화는 E2E 14가 맡는다.
import assert from "node:assert/strict";
import { it } from "node:test";
import { paletteFor, sanitizeAppearance } from "../src/theme.ts";

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
