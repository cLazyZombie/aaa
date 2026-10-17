// 이 기기의 설정(herdr-web-ui lib/settings.ts). 설정 창(SettingsDialog.tsx)과 키 막대 ⌨ 단추가 바꾼다.
// 참고처럼 「shared」 범위 값(구독 사용량 설정)은 host에 두어 기기끼리 나눈다(preferencesSync.ts, plan §8.73).
// 이 파일은 그 값의 이 기기 사본도 localStorage에 둔다. 나머지는 이 기기 것이다(plan §8.45).
import { useCallback, useSyncExternalStore } from "react";
import { DEFAULT_KEY_BAR_ITEMS, keyBarItemId, sanitizeKeyBarItems, type KeyBarItem } from "./herdr/keyBar.ts";
import { sanitizeFontFamily } from "./herdr/fontFamily.ts";
import { sanitizeUsage, type UsageSettings } from "./preferences.ts";

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
  if (e.key !== KEY && e.key !== FONT_KEY && e.key !== WHEEL_KEY && e.key !== KEY_BAR_KEY && e.key !== FONT_FAMILY_KEY && e.key !== KEEP_SCREEN_KEY) return;
  mode = read();
  fontSize = readFontSize();
  wheelSpeed = readWheelSpeed();
  keyBarItems = readKeyBarItems();
  fontFamily = readFontFamily();
  keepScreenOn = readKeepScreenOn();
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

/** 휠 한 칸에 보내는 mouse 보고 수(herdr-web-ui settings.ts `terminalWheelSpeed`: 기기마다, 기본 1, 1~10) */
export const TERMINAL_WHEEL_SPEED_MIN = 1;
export const TERMINAL_WHEEL_SPEED_MAX = 10;
const WHEEL_KEY = "aaa:terminal-wheel-speed";

function clampWheel(speed: number): number {
  return Math.min(TERMINAL_WHEEL_SPEED_MAX, Math.max(TERMINAL_WHEEL_SPEED_MIN, Math.round(speed)));
}

function readWheelSpeed(): number {
  try {
    const raw = localStorage.getItem(WHEEL_KEY);
    const v = raw === null ? NaN : Number(raw);
    return Number.isFinite(v) ? clampWheel(v) : TERMINAL_WHEEL_SPEED_MIN;
  } catch {
    return TERMINAL_WHEEL_SPEED_MIN;
  }
}

let wheelSpeed = readWheelSpeed();

export function getTerminalWheelSpeed(): number {
  return wheelSpeed;
}

export function setTerminalWheelSpeed(speed: number) {
  wheelSpeed = clampWheel(speed);
  try {
    if (wheelSpeed === TERMINAL_WHEEL_SPEED_MIN) localStorage.removeItem(WHEEL_KEY);
    else localStorage.setItem(WHEEL_KEY, String(wheelSpeed));
  } catch {
    // 저장이 막혀도 이 화면 동안은 바꾼 대로 둔다
  }
  listeners.forEach((f) => f());
}

export function useTerminalWheelSpeed(): number {
  return useSyncExternalStore(subscribe, () => wheelSpeed);
}

/** 키 막대 항목(herdr-web-ui settings.ts `keyBarItems`: 기기마다, 기본 DEFAULT_KEY_BAR_ITEMS, 잘못된 저장값은 거른다) */
const KEY_BAR_KEY = "aaa:keybar-items";

function readKeyBarItems(): KeyBarItem[] {
  try {
    const raw = localStorage.getItem(KEY_BAR_KEY);
    return sanitizeKeyBarItems(raw === null ? undefined : JSON.parse(raw));
  } catch {
    return DEFAULT_KEY_BAR_ITEMS;
  }
}

let keyBarItems = readKeyBarItems();

export function setKeyBarItems(items: readonly KeyBarItem[]) {
  keyBarItems = sanitizeKeyBarItems(items);
  const same = (a: readonly KeyBarItem[], b: readonly KeyBarItem[]) =>
    a.length === b.length && a.every((item, i) => keyBarItemId(item) === keyBarItemId(b[i]!));
  try {
    if (same(keyBarItems, DEFAULT_KEY_BAR_ITEMS)) localStorage.removeItem(KEY_BAR_KEY);
    else localStorage.setItem(KEY_BAR_KEY, JSON.stringify(keyBarItems));
  } catch {
    // 저장이 막혀도 이 화면 동안은 바꾼 대로 둔다
  }
  listeners.forEach((f) => f());
}

export function useKeyBarItems(): readonly KeyBarItem[] {
  return useSyncExternalStore(subscribe, () => keyBarItems);
}

/**
 * 터미널 글꼴(herdr-web-ui settings.ts `terminalFontFamily`: 기기마다, 기본 ""). 쉼표로 적은 목록이고
 * 기본 글꼴 목록 앞에 붙는다(Term.tsx). 읽을 때도 거른다.
 */
const FONT_FAMILY_KEY = "aaa:terminal-font-family";

function readFontFamily(): string {
  try {
    return sanitizeFontFamily(localStorage.getItem(FONT_FAMILY_KEY));
  } catch {
    return "";
  }
}

let fontFamily = readFontFamily();

export function setTerminalFontFamily(family: string) {
  fontFamily = sanitizeFontFamily(family);
  try {
    if (fontFamily === "") localStorage.removeItem(FONT_FAMILY_KEY);
    else localStorage.setItem(FONT_FAMILY_KEY, fontFamily);
  } catch {
    // 저장이 막혀도 이 화면 동안은 바꾼 대로 둔다
  }
  listeners.forEach((f) => f());
}

export function useTerminalFontFamily(): string {
  return useSyncExternalStore(subscribe, () => fontFamily);
}

/** 화면 켜짐 유지(herdr-web-ui settings.ts `keepScreenOn`: 기기마다, 기본 꺼짐). pane을 보는 동안 wake lock을 건다 */
const KEEP_SCREEN_KEY = "aaa:keep-screen-on";

function readKeepScreenOn(): boolean {
  try {
    return localStorage.getItem(KEEP_SCREEN_KEY) === "1";
  } catch {
    return false;
  }
}

let keepScreenOn = readKeepScreenOn();

export function setKeepScreenOn(on: boolean) {
  keepScreenOn = on;
  try {
    if (on) localStorage.setItem(KEEP_SCREEN_KEY, "1");
    else localStorage.removeItem(KEEP_SCREEN_KEY);
  } catch {
    // 저장이 막혀도 이 화면 동안은 바꾼 대로 둔다
  }
  listeners.forEach((f) => f());
}

export function useKeepScreenOn(): boolean {
  return useSyncExternalStore(subscribe, () => keepScreenOn);
}

/**
 * 켜고 끄는 터미널 설정(herdr-web-ui settings.ts: 둘 다 `device` 범위).
 * - paneClipboard(기본 켬): pane 프로그램의 클립보드 쓰기(OSC 52)를 이 기기 클립보드에 넣는다.
 * - releasePaneAway(기본 꺼짐): 다른 창으로 옮기고 1초 뒤 이 탭은 pane을 보기만 하고 크기를 놓는다(터미널의 aaa TUI와 같이 쓸 때).
 * 기본값이면 지운다. 값은 "1"·"0".
 */
export type TerminalToggle = "paneClipboard" | "releasePaneAway";
const TOGGLE_KEYS: Record<TerminalToggle, string> = {
  paneClipboard: "aaa:pane-clipboard",
  releasePaneAway: "aaa:release-pane-away",
};
const TOGGLE_DEFAULTS: Record<TerminalToggle, boolean> = { paneClipboard: true, releasePaneAway: false };

function readToggle(name: TerminalToggle): boolean {
  try {
    const v = localStorage.getItem(TOGGLE_KEYS[name]);
    return v === "1" ? true : v === "0" ? false : TOGGLE_DEFAULTS[name];
  } catch {
    return TOGGLE_DEFAULTS[name];
  }
}

const toggles: Record<TerminalToggle, boolean> = {
  paneClipboard: readToggle("paneClipboard"),
  releasePaneAway: readToggle("releasePaneAway"),
};

window.addEventListener("storage", (e) => {
  const name = (Object.keys(TOGGLE_KEYS) as TerminalToggle[]).find((k) => TOGGLE_KEYS[k] === e.key);
  if (!name) return;
  toggles[name] = readToggle(name);
  listeners.forEach((f) => f());
});

export function setTerminalToggle(name: TerminalToggle, on: boolean) {
  toggles[name] = on;
  try {
    if (on === TOGGLE_DEFAULTS[name]) localStorage.removeItem(TOGGLE_KEYS[name]);
    else localStorage.setItem(TOGGLE_KEYS[name], on ? "1" : "0");
  } catch {
    // 저장이 막혀도 이 화면 동안은 바꾼 대로 둔다
  }
  listeners.forEach((f) => f());
}

/** 사건 처리기에서 읽는 지금 값 */
export function terminalToggle(name: TerminalToggle): boolean {
  return toggles[name];
}

export function useTerminalToggle(name: TerminalToggle): boolean {
  return useSyncExternalStore(subscribe, () => toggles[name]);
}

/**
 * 알림(herdr-web-ui settings.ts `alertsOn`·`alertInApp`·`alertSound`: 기기마다).
 * - 켜짐(기본 켬): 이 기기의 알림 전체. 끄면 push 구독도 지운다.
 * - 앱 안(기본 켬): 앱을 보는 동안 위에서 내려오는 알림(Droplet).
 * - 소리(기본 끔): 앱 탭이 열려 있으면 알림마다 소리.
 * 기본값이면 지운다. 값은 "1"·"0".
 */
export type AlertSetting = "alertsOn" | "alertInApp" | "alertSound";
const ALERT_KEYS: Record<AlertSetting, string> = {
  alertsOn: "aaa:alerts-on",
  alertInApp: "aaa:alert-in-app",
  alertSound: "aaa:alert-sound",
};
const ALERT_DEFAULTS: Record<AlertSetting, boolean> = { alertsOn: true, alertInApp: true, alertSound: false };

function readAlert(name: AlertSetting): boolean {
  try {
    const v = localStorage.getItem(ALERT_KEYS[name]);
    return v === "1" ? true : v === "0" ? false : ALERT_DEFAULTS[name];
  } catch {
    return ALERT_DEFAULTS[name];
  }
}

const alerts: Record<AlertSetting, boolean> = {
  alertsOn: readAlert("alertsOn"),
  alertInApp: readAlert("alertInApp"),
  alertSound: readAlert("alertSound"),
};

export function setAlertSetting(name: AlertSetting, on: boolean) {
  alerts[name] = on;
  try {
    if (on === ALERT_DEFAULTS[name]) localStorage.removeItem(ALERT_KEYS[name]);
    else localStorage.setItem(ALERT_KEYS[name], on ? "1" : "0");
  } catch {
    // 저장이 막혀도 이 화면 동안은 바꾼 대로 둔다
  }
  listeners.forEach((f) => f());
}

export function alertSetting(name: AlertSetting): boolean {
  return alerts[name];
}

export function useAlertSetting(name: AlertSetting): boolean {
  return useSyncExternalStore(subscribe, () => alerts[name]);
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

// 사용량 막대 설정은 host 공유 설정이라 그 형·기본값·거르기는 preferences.ts에 있다(plan §8.73)
export { DEFAULT_USAGE, type UsageCount, type UsageGlance, type UsageSettings } from "./preferences.ts";

const USAGE_KEY = "aaa:usage-settings";
const usageListeners = new Set<() => void>();

/** 잘못된 저장값은 기본값으로(참고 settings.ts 465줄과 같은 거르기) */
function readUsage(): UsageSettings {
  try {
    return sanitizeUsage(JSON.parse(localStorage.getItem(USAGE_KEY) ?? "{}"));
  } catch {
    // 막혔거나 깨진 값: 기본값
    return sanitizeUsage({});
  }
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
