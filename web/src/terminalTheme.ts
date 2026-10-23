// 터미널 색은 앱 팔레트와 따로 고른다. 저장·동기화는 settings.ts, 화면과 OSC 10/11은 같은 ITheme을 쓴다.
import type { ITheme } from "@xterm/xterm";
import type { CSSProperties } from "react";

export type TerminalColorTheme = "app" | "wezterm-github-dark-user";
export const TERMINAL_COLOR_THEMES: readonly { value: TerminalColorTheme; label: string }[] = [
  { value: "app", label: "앱 팔레트 따름" },
  // 저장 ID는 기존 기기의 선택을 이어받는다.
  { value: "wezterm-github-dark-user", label: "GitHub Dark" },
];

export const ANSI_COLOR_KEYS = [
  "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
  "brightBlack", "brightRed", "brightGreen", "brightYellow", "brightBlue", "brightMagenta", "brightCyan", "brightWhite",
] as const;

// WezTerm e2a98e1b8af156988ad0ac778658ac27b60a55fb config/src/scheme_data.rs의 "GitHub Dark".
// 색 자료: iTerm2-Color-Schemes 모음(MIT), 원 테마 Al Duncanson(GPL-3.0).
// 원본·색 자료·고지는 /terminal-themes/와 /licenses/github-dark-GPL-3.0.txt에 함께 넣는다.
// 사용자 설정에 따라 background·foreground만 바꾼다. cursorAccent는 원본 cursor_fg 그대로다.
const WEZTERM_GITHUB_DARK_USER: Readonly<ITheme> = {
  background: "#24292e", foreground: "#c9d1d9",
  cursor: "#c9d1d9", cursorAccent: "#101216",
  selectionBackground: "#3b5070", selectionForeground: "#ffffff",
  black: "#000000", red: "#f78166", green: "#56d364", yellow: "#e3b341",
  blue: "#6ca4f8", magenta: "#db61a2", cyan: "#2b7489", white: "#ffffff",
  brightBlack: "#4d4d4d", brightRed: "#f78166", brightGreen: "#56d364", brightYellow: "#e3b341",
  brightBlue: "#6ca4f8", brightMagenta: "#db61a2", brightCyan: "#2b7489", brightWhite: "#ffffff",
};

export function sanitizeTerminalColorTheme(value: unknown): TerminalColorTheme {
  return value === "wezterm-github-dark-user" ? value : "app";
}

/** app은 지금 팔레트의 token을 매번 읽는다. 고른 테마는 앱 밝기·팔레트와 무관하다. */
export function resolveTerminalTheme(id: TerminalColorTheme, token: (name: string) => string): ITheme {
  if (id === "wezterm-github-dark-user") return { ...WEZTERM_GITHUB_DARK_USER };
  const theme: ITheme = {
    background: token("--term-bg"), foreground: token("--term-fg"), cursor: token("--term-cursor"),
    cursorAccent: token("--term-cursor-accent") || token("--term-bg"), selectionBackground: token("--term-selection"),
    selectionForeground: token("--term-selection-fg") || undefined,
  };
  ANSI_COLOR_KEYS.forEach((key, n) => { theme[key] = token(`--ansi-${n}`); });
  return theme;
}

/** CSS 최적화가 #ffffff를 #fff로 줄여도 wire의 rrggbb 계약을 지킨다. */
export function terminalThemeColors(theme: ITheme): { fg: string; bg: string } {
  const hex = (color: string | undefined) => (color ?? "").replace(/^#/, "")
    .replace(/^([0-9a-f])([0-9a-f])([0-9a-f])$/i, "$1$1$2$2$3$3").toLowerCase();
  return { fg: hex(theme.foreground), bg: hex(theme.background) };
}

/** 바깥 여백·앞 기록 덮개도 터미널 색이다. 앱 token은 바꾸지 않고 해당 pane 안에서만 덮는다. */
export function terminalThemeStyle(id: TerminalColorTheme): CSSProperties | undefined {
  if (id === "app") return undefined;
  const theme = WEZTERM_GITHUB_DARK_USER;
  const vars: Record<string, string | undefined> = {
    "--term-bg": theme.background, "--term-fg": theme.foreground,
    "--term-cursor": theme.cursor, "--term-selection": theme.selectionBackground,
  };
  ANSI_COLOR_KEYS.forEach((key, n) => { vars[`--ansi-${n}`] = theme[key]; });
  return vars as CSSProperties;
}
