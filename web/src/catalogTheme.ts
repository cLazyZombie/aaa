// herdr의 Palette 역할을 기존 웹 token에 연결한다. 원색 자료는 themeCatalog.ts.
import { THEME_CATALOG, type CatalogPalette } from "./themeCatalog.ts";

export const catalogTheme = (id: string) => THEME_CATALOG.find((theme) => theme.id === id);

function channels(hex: string): number[] {
  return hex.slice(1).match(/../g)!.map((part) => parseInt(part, 16));
}

function luminance(hex: string): number {
  return channels(hex).map((v) => v / 255).map((v) => v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4)
    .reduce((sum, value, i) => sum + value * [0.2126, 0.7152, 0.0722][i], 0);
}

export function contrast(a: string, b: string): number {
  const [x, y] = [luminance(a), luminance(b)];
  return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
}

/** herdr-web-ui Catppuccin의 legibility first 규칙: 원색을 흰색·검정 쪽으로 최소 혼합한다. */
function readable(color: string, backgrounds: string[], mode: "light" | "dark"): string {
  const rgb = channels(color);
  const target = mode === "dark" ? 255 : 0;
  for (let step = 0; step <= 255; step++) {
    const value = "#" + rgb.map((v) => Math.round(v + (target - v) * step / 255).toString(16).padStart(2, "0")).join("");
    if (backgrounds.every((bg) => contrast(value, bg) >= 4.5)) return value;
  }
  throw new Error(`팔레트 대비를 맞출 수 없음: ${color}`);
}

const cachedTokens = new Map<CatalogPalette, Record<string, string>>();

export function catalogTokens(id: CatalogPalette): Record<string, string> {
  const cached = cachedTokens.get(id);
  if (cached) return cached;
  const theme = catalogTheme(id)!;
  const c = theme.colors;
  const surfaces = [c.panel_bg, c.surface0, c.surface1];
  const fg = (color: string) => readable(color, surfaces, theme.mode);
  const tint = (color: string) => `rgba(${channels(color).join(", ")}, 0.12)`;
  const accent = fg(c.accent);
  const tokens: Record<string, string> = {
    "bg": c.panel_bg, "bg-panel": c.panel_bg, "bg-elevated": c.surface0, "bg-hover": c.surface1,
    "bg-input": c.panel_bg, "border": c.surface1, "border-strong": c.overlay0, "bubble-border": c.surface1,
    "text": fg(c.text), "text-dim": fg(c.subtext0), "text-strong": fg(c.text), "line-number": fg(c.overlay1),
    "accent": accent, "accent-tint": tint(accent), "primary": accent, "primary-hover": accent,
    "primary-text": contrast("#000000", accent) >= 4.5 ? "#000000" : "#ffffff", "primary-tint": tint(accent),
    "status-idle": fg(c.subtext0), "status-working": fg(c.blue), "status-blocked": fg(c.red), "status-done": fg(c.green),
    "status-working-tint": tint(c.blue), "status-blocked-tint": tint(c.red), "status-done-tint": tint(c.green),
    "danger-text": fg(c.red), "danger-tint": tint(c.red), "notice": fg(c.yellow),
    "indicator-idle": fg(c.green), "indicator-working": fg(c.yellow), "indicator-blocked": fg(c.red),
    "indicator-done": fg(c.blue), "indicator-unknown": fg(c.subtext0),
    "scrim": theme.mode === "dark" ? "rgba(0, 0, 0, 0.6)" : "rgba(0, 0, 0, 0.3)",
    "syntax-comment": fg(c.overlay1), "syntax-function": accent, "syntax-inserted": fg(c.green), "syntax-deleted": fg(c.red),
    "syntax-keyword": fg(c.mauve), "syntax-string": fg(c.green), "syntax-number": fg(c.peach),
    "syntax-type": fg(c.yellow), "syntax-variable": fg(c.text), "syntax-meta": fg(c.teal),
    "term-bg": theme.terminal.background, "term-fg": theme.terminal.foreground,
    "term-cursor": theme.terminal.cursor_bg, "term-cursor-accent": theme.terminal.cursor_fg,
    "term-selection": theme.terminal.selection_bg, "term-selection-fg": theme.terminal.selection_fg,
  };
  [...theme.terminal.ansi, ...theme.terminal.brights].forEach((color, i) => { tokens[`ansi-${i}`] = color; });
  const result = Object.fromEntries(Object.entries(tokens).map(([key, value]) => [`--${key}`, value]));
  cachedTokens.set(id, result);
  return result;
}
