// 이 기기의 설정(herdr-web-ui lib/settings.ts). 설정 창(SettingsDialog.tsx)과 키 막대 ⌨ 단추가 바꾼다.
// 참고는 「shared」 범위 값(구독 사용량 설정)을 server에 두어 기기끼리 나눈다. aaa에는 아직 설정 저장소가 없어
// 모두 이 기기(localStorage)에 둔다(plan §8.45).
import { useCallback, useSyncExternalStore } from "react";

/** 터미널 입력 방식. auto는 바로 입력이다(herdr-web-ui settings.ts `terminalInputMode`, PaneTerminal 358줄). */
export type TerminalInputMode = "auto" | "line" | "direct";

const KEY = "aaa:terminal-input-mode";
const listeners = new Set<() => void>();

function read(): TerminalInputMode {
  try {
    const v = localStorage.getItem(KEY);
    return v === "line" || v === "direct" ? v : "auto";
  } catch {
    return "auto";
  }
}

let mode = read();

function subscribe(f: () => void): () => void {
  listeners.add(f);
  return () => listeners.delete(f);
}

// 같은 기기의 다른 탭이 바꾼 값도 따른다
window.addEventListener("storage", (e) => {
  if (e.key !== KEY && e.key !== FONT_KEY) return;
  mode = read();
  fontSize = readFontSize();
  listeners.forEach((f) => f());
});

/** 터미널 글자 크기 px(herdr-web-ui settings.ts `terminalFontSize`: 기기마다, 기본 13, 10~22). 핀치도 이것을 바꾼다. */
export const TERMINAL_FONT_MIN = 10;
export const TERMINAL_FONT_MAX = 22;
const DEFAULT_FONT_SIZE = 13;
const FONT_KEY = "aaa:terminal-font-size";

function clampFont(size: number): number {
  return Math.min(TERMINAL_FONT_MAX, Math.max(TERMINAL_FONT_MIN, Math.round(size)));
}

function readFontSize(): number {
  try {
    const raw = localStorage.getItem(FONT_KEY);
    const v = raw === null ? NaN : Number(raw);
    return Number.isFinite(v) ? clampFont(v) : DEFAULT_FONT_SIZE;
  } catch {
    return DEFAULT_FONT_SIZE;
  }
}

let fontSize = readFontSize();

export function setTerminalFontSize(size: number) {
  fontSize = clampFont(size);
  try {
    if (fontSize === DEFAULT_FONT_SIZE) localStorage.removeItem(FONT_KEY);
    else localStorage.setItem(FONT_KEY, String(fontSize));
  } catch {
    // 저장이 막혀도 이 화면 동안은 바꾼 대로 둔다
  }
  listeners.forEach((f) => f());
}

export function useTerminalFontSize(): number {
  return useSyncExternalStore(subscribe, () => fontSize);
}

export function setTerminalInputMode(next: TerminalInputMode) {
  mode = next;
  try {
    if (next === "auto") localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, next);
  } catch {
    // 저장이 막혀도 이 화면 동안은 바꾼 대로 둔다
  }
  listeners.forEach((f) => f());
}

/** media query가 맞는지(바뀌면 다시 그린다). 키 막대 ⌨ 단추는 터치 기기(`pointer: coarse`)에만 둔다. */
export function useMedia(query: string): boolean {
  const subscribeMedia = useCallback(
    (f: () => void) => {
      const m = window.matchMedia(query);
      m.addEventListener("change", f);
      return () => m.removeEventListener("change", f);
    },
    [query],
  );
  return useSyncExternalStore(subscribeMedia, () => window.matchMedia(query).matches);
}

export function useTerminalInputMode(): TerminalInputMode {
  return useSyncExternalStore(subscribe, () => mode);
}

/** 막대가 보이는 것: 쓴 몫, 남은 몫 */
export type UsageCount = "used" | "left";
/** 칩이 보이는 한도: 주간, 짧은 세션 */
export type UsageGlance = "week" | "session";

export interface UsageSettings {
  showUsage: boolean;
  usageCount: UsageCount;
  usageGlance: UsageGlance;
  /** 계정 key 순서(사용자가 정함) */
  usageOrder: string[];
  /** 숨긴 계정 key */
  usageHidden: string[];
}

/** 참고 DEFAULT_SETTINGS와 같다. 다만 showUsage는 켬: 사용자가 이 막대를 보이라고 했다(10-17, 참고는 끔). */
export const DEFAULT_USAGE: UsageSettings = { showUsage: true, usageCount: "used", usageGlance: "week", usageOrder: [], usageHidden: [] };

const USAGE_KEY = "aaa:usage-settings";
const usageListeners = new Set<() => void>();
const strings = (v: unknown): string[] => (Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : []);

/** 잘못된 저장값은 기본값으로(참고 settings.ts 465줄과 같은 거르기) */
function readUsage(): UsageSettings {
  let r: Record<string, unknown> = {};
  try {
    const v = JSON.parse(localStorage.getItem(USAGE_KEY) ?? "{}");
    if (v && typeof v === "object" && !Array.isArray(v)) r = v;
  } catch {
    // 막혔거나 깨진 값: 기본값
  }
  return {
    showUsage: typeof r.showUsage === "boolean" ? r.showUsage : DEFAULT_USAGE.showUsage,
    usageCount: r.usageCount === "used" || r.usageCount === "left" ? r.usageCount : DEFAULT_USAGE.usageCount,
    usageGlance: r.usageGlance === "week" || r.usageGlance === "session" ? r.usageGlance : DEFAULT_USAGE.usageGlance,
    usageOrder: strings(r.usageOrder),
    usageHidden: strings(r.usageHidden),
  };
}

let usage = readUsage();

window.addEventListener("storage", (e) => {
  if (e.key !== USAGE_KEY) return;
  usage = readUsage();
  usageListeners.forEach((f) => f());
});

export function updateUsageSettings(patch: Partial<UsageSettings>) {
  usage = { ...usage, ...patch };
  try {
    localStorage.setItem(USAGE_KEY, JSON.stringify(usage));
  } catch {
    // 저장이 막혀도 이 화면 동안은 바꾼 대로 둔다
  }
  usageListeners.forEach((f) => f());
}

function subscribeUsage(f: () => void): () => void {
  usageListeners.add(f);
  return () => usageListeners.delete(f);
}

export function useUsageSettings(): UsageSettings {
  return useSyncExternalStore(subscribeUsage, () => usage);
}
