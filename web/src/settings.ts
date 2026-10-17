// 이 기기의 설정(herdr-web-ui lib/settings.ts의 「device」 범위 값). 설정 화면은 아직 없어 키 막대 ⌨ 단추가 바꾼다.
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
  if (e.key !== KEY) return;
  mode = read();
  listeners.forEach((f) => f());
});

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
