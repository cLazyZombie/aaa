// 모양 설정: 테마·팔레트·상태 표시·목록 줄(herdr-web-ui lib/settings.ts `theme`·`palette`·`paletteLight`·`paletteDark`·
// `statusIndicators`·`sidebarRows`, lib/paletteFor.ts, applyToDocument). 기본값도 참고와 같다: 어두움, amber, classic, 두 줄.
// <html data-theme>·data-palette를 늘 달고(참고 applyToDocument), 실제 색은 herdr/tokens.css에 있다.
// 참고는 theme·palette·sidebarRows를 「shared」(서버 저장)로 둔다. aaa는 아직 이 기기(localStorage)에 둔다(plan §8.70).
import { useSyncExternalStore } from "react";
import type { StatusIndicators } from "./herdr/status.ts";

export type ThemeSetting = "dark" | "light" | "system";
export type ResolvedTheme = "dark" | "light";
/** amber: herdr 모양(기본), charcoal: 무채색 Ghostty식 어두움, catppuccin: 어두우면 Mocha, 밝으면 Latte */
export type Palette = "amber" | "charcoal" | "catppuccin";
/** 한 줄은 이름, 두 줄은 하는 일과 그 아래 이름(참고 `SidebarRows`) */
export type SidebarRows = "one" | "two";

export interface Appearance {
  theme: ThemeSetting;
  palette: Palette;
  /** 시스템 테마일 때만: OS가 밝을 때와 어두울 때 팔레트(참고 paletteFor) */
  paletteLight: Palette;
  paletteDark: Palette;
  statusIndicators: StatusIndicators;
  sidebarRows: SidebarRows;
}

export const PALETTES: readonly Palette[] = ["amber", "charcoal", "catppuccin"];
const DEFAULT: Appearance = { theme: "dark", palette: "amber", paletteLight: "amber", paletteDark: "amber", statusIndicators: "classic", sidebarRows: "two" };
const KEY = "aaa:appearance";
/** 전에 쓰던 열쇠(밝음·어두움만 남았다). 처음 한 번 옮긴다 */
const OLD_KEY = "aaa:theme";

function pick<T extends string>(v: unknown, all: readonly T[], fallback: T): T {
  return all.find((x) => x === v) ?? fallback;
}

/** 저장된 값을 거른다(참고 settings.ts `sanitizeSettings`의 해당 줄): 모르는 값은 기본, 짝이 없던 기록은 한 팔레트에서 시작 */
export function sanitizeAppearance(raw: unknown): Appearance {
  const r = raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
  const palette = pick(r.palette, PALETTES, DEFAULT.palette);
  return {
    theme: pick(r.theme, ["dark", "light", "system"] as const, DEFAULT.theme),
    palette,
    paletteLight: pick(r.paletteLight, PALETTES, palette),
    paletteDark: pick(r.paletteDark, PALETTES, palette),
    statusIndicators: pick(r.statusIndicators, ["classic", "dots", "symbols"] as const, DEFAULT.statusIndicators),
    sidebarRows: pick(r.sidebarRows, ["one", "two"] as const, DEFAULT.sidebarRows),
  };
}

function read(): Appearance {
  try {
    const v = localStorage.getItem(KEY);
    if (v !== null) return sanitizeAppearance(JSON.parse(v));
    const old = localStorage.getItem(OLD_KEY);
    if (old === "light" || old === "dark") return { ...DEFAULT, theme: old };
  } catch {
    // 저장소를 못 쓰는 창(사생활 보호 등)은 기본값
  }
  return DEFAULT;
}

const DARK_QUERY = "(prefers-color-scheme: dark)";
const media = () => window.matchMedia(DARK_QUERY);

/** 시스템이면 OS 설정을 따른다. 알 수 없으면 어두움(참고 resolveTheme) */
export function resolveTheme(setting: ThemeSetting): ResolvedTheme {
  if (setting !== "system") return setting;
  return media().matches === false ? "light" : "dark";
}

/** 그릴 팔레트: 시스템이면 밝음·어두움 짝 가운데 하나(참고 paletteFor) */
export function paletteFor(a: Pick<Appearance, "theme" | "palette" | "paletteLight" | "paletteDark">, resolved: ResolvedTheme): Palette {
  if (a.theme !== "system") return a.palette;
  return resolved === "dark" ? a.paletteDark : a.paletteLight;
}

/** PWA 제목 막대 색은 판 바탕을 따른다(참고 settings.ts THEME_COLOR) */
const THEME_COLOR: Record<Palette, Record<ResolvedTheme, string>> = {
  amber: { dark: "#181613", light: "#faf8f3" },
  charcoal: { dark: "#171717", light: "#fafaf9" },
  catppuccin: { dark: "#181825", light: "#e6e9ef" },
};

const subs = new Set<() => void>();
let current: Appearance = read();
let resolved: ResolvedTheme = resolveTheme(current.theme);

function apply() {
  resolved = resolveTheme(current.theme);
  const palette = paletteFor(current, resolved);
  const root = document.documentElement;
  root.dataset["theme"] = resolved;
  root.dataset["palette"] = palette;
  root.style.colorScheme = resolved;
  document.querySelector('meta[name="theme-color"]')?.setAttribute("content", THEME_COLOR[palette][resolved]);
  subs.forEach((f) => f());
}

export function initTheme() {
  apply();
  media().addEventListener("change", () => current.theme === "system" && apply());
  // 같은 기기의 다른 탭이 바꾼 것도 따른다
  window.addEventListener("storage", (e) => {
    if (e.key !== KEY) return;
    current = read();
    apply();
  });
}

export function updateAppearance(patch: Partial<Appearance>) {
  current = sanitizeAppearance({ ...current, ...patch });
  try {
    localStorage.setItem(KEY, JSON.stringify(current));
    localStorage.removeItem(OLD_KEY);
  } catch {
    // 남기지 못해도 이번 창에서는 바뀐다
  }
  apply();
}

function subscribe(f: () => void): () => void {
  subs.add(f);
  return () => subs.delete(f);
}

let snapshot: { appearance: Appearance; resolvedTheme: ResolvedTheme; resolvedPalette: Palette } | null = null;
function getSnapshot() {
  const palette = paletteFor(current, resolved);
  if (!snapshot || snapshot.appearance !== current || snapshot.resolvedTheme !== resolved || snapshot.resolvedPalette !== palette)
    snapshot = { appearance: current, resolvedTheme: resolved, resolvedPalette: palette };
  return snapshot;
}

/** 지금 모양 설정과 실제로 그리는 테마·팔레트 */
export function useAppearance() {
  return useSyncExternalStore(subscribe, getSnapshot);
}

/** 테마가 바뀔 때 부른다(React 밖: xterm 색 갱신) */
export function onThemeChange(f: () => void): () => void {
  return subscribe(f);
}

/** CSS token 값 */
export const token = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();
