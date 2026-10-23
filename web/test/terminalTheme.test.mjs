import assert from "node:assert/strict";
import fs from "node:fs";
import { it } from "node:test";
import { ANSI_COLOR_KEYS, resolveTerminalTheme, sanitizeTerminalColorTheme, terminalThemeColors, terminalThemeStyle } from "../src/terminalTheme.ts";

const source = JSON.parse(fs.readFileSync(new URL("../public/terminal-themes/github-dark-source.json", import.meta.url))).colors;
const independent = () => { throw new Error("독립 테마가 앱 token을 읽었다"); };
it("WezTerm 정본의 ANSI·커서·선택 색을 보존하고 사용자 bg·fg만 바꾼다", () => {
  const t = resolveTerminalTheme("wezterm-github-dark-user", independent);
  assert.deepEqual(ANSI_COLOR_KEYS.map((key) => t[key]), [...source.ansi, ...source.brights]);
  assert.equal(t.cursor, source.cursor_bg);
  assert.equal(t.cursor, source.cursor_border);
  assert.equal(t.cursorAccent, source.cursor_fg);
  assert.equal(t.selectionBackground, source.selection_bg);
  assert.equal(t.selectionForeground, source.selection_fg);
  assert.equal(t.background, "#24292e");
  assert.equal(t.foreground, "#c9d1d9");
  assert.deepEqual(terminalThemeColors(t), { bg: "24292e", fg: "c9d1d9" });
  t.background = "#000000";
  assert.equal(resolveTerminalTheme("wezterm-github-dark-user", independent).background, "#24292e");
});
it("없거나 모르는 선택은 앱을 따르고 CSS 단축 hex도 OSC 6자리로 보낸다", () => {
  for (const value of [undefined, null, "", "app", "github-dark", {}, 7]) assert.equal(sanitizeTerminalColorTheme(value), "app");
  assert.equal(sanitizeTerminalColorTheme("wezterm-github-dark-user"), "wezterm-github-dark-user");
  const tokens = { "--term-bg": "#ABC", "--term-fg": "#123", "--term-cursor": "#789", "--term-selection": "rgba(1, 2, 3, .4)" };
  const t = resolveTerminalTheme("app", (key) => tokens[key] ?? key);
  assert.deepEqual(terminalThemeColors(t), { bg: "aabbcc", fg: "112233" });
  assert.equal(t.cursorAccent, "#ABC");
  assert.equal(t.selectionBackground, tokens["--term-selection"]);
  assert.deepEqual(ANSI_COLOR_KEYS.map((key) => t[key]), Array.from({ length: 16 }, (_, i) => `--ansi-${i}`));
  assert.equal(terminalThemeStyle("app"), undefined);
  const style = terminalThemeStyle("wezterm-github-dark-user");
  assert.equal(style["--term-bg"], "#24292e");
  assert.equal(style["--term-selection"], source.selection_bg);
});
