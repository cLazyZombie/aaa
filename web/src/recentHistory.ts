// 팔레트와 PWA 전환이 함께 쓰는 이 기기의 방문 기록. host/API 상태는 저장하지 않는다.
import { useSyncExternalStore } from "react";
import { loadRecent, RECENT_KEY, rememberTarget } from "./palette.ts";
import { logIgnoredFailure } from "./errorDiagnostic.ts";

let recent = loadRecent((key) => window.localStorage.getItem(key));
const listeners = new Set<() => void>();
const subscribe = (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; };
export const currentRecent = (): readonly string[] => recent;
export const useRecentHistory = (): readonly string[] => useSyncExternalStore(subscribe, currentRecent);

export function visitScreen(key: string): void {
  if (recent[0] === key) return;
  recent = rememberTarget(key, recent);
  try { window.localStorage.setItem(RECENT_KEY, JSON.stringify(recent)); }
  catch (error) { logIgnoredFailure(error, "최근 화면 기록 저장"); }
  listeners.forEach((listener) => listener());
}

window.addEventListener("storage", (event) => {
  if (event.key !== RECENT_KEY && event.key !== null) return;
  recent = loadRecent(() => event.newValue);
  listeners.forEach((listener) => listener());
});
