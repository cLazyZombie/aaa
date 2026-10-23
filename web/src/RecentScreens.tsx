import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { recentList, recentScreenChord, RECENT_SCREENS_KEY, standaloneApp, stepRecent, storedRecentScreens, visitRecent, type RecentCycle, type RecentScreen } from "./recentScreenModel.ts";
import "./RecentScreens.css";

/** 현재 화면은 그대로 두고 Ctrl 해제에서만 이동한다. 사건 사이의 연속 누름은 ref로 이어 React commit을 기다리지 않는다. */
export function RecentScreens({ current, resolve, onOpen, enabled }: {
  current: string | null;
  resolve: (key: string) => RecentScreen | null;
  onOpen: (screen: RecentScreen) => void;
  enabled: boolean;
}) {
  const [initial] = useState(storedRecentScreens);
  const history = useRef<string[]>(initial);
  const [cycle, setCycle] = useState<RecentCycle | null>(null);
  const cycling = useRef<RecentCycle | null>(null);
  const latest = useRef({ current, resolve, onOpen, enabled });
  latest.current = { current, resolve, onOpen, enabled };
  const opener = useRef<HTMLElement | null>(null);
  const surface = useRef<HTMLDivElement>(null);
  const list = cycle ? recentList(cycle.keys, resolve) : [];
  useEffect(() => {
    if (cycling.current) { cycling.current = null; setCycle(null); }
    if (current === null) return;
    history.current = visitRecent(history.current, current);
    try { localStorage.setItem(RECENT_SCREENS_KEY, JSON.stringify(history.current)); } catch { /* 사생활 보호 창 */ }
  }, [current]);
  useLayoutEffect(() => {
    if (cycle) surface.current?.focus({ preventScroll: true });
  }, [cycle !== null]);
  useLayoutEffect(() => {
    surface.current?.querySelector<HTMLElement>('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
  }, [cycle?.selected]);
  useEffect(() => {
    const update = (value: RecentCycle | null) => { cycling.current = value; setCycle(value); };
    const finish = (commit: boolean, key = cycling.current?.selected) => {
      if (!cycling.current) return;
      const target = commit && key ? latest.current.resolve(key) : null;
      update(null);
      if (target) latest.current.onOpen(target);
      else if (opener.current?.isConnected) opener.current.focus({ preventScroll: true });
      opener.current = null;
    };
    const consume = (event: KeyboardEvent) => { event.preventDefault(); event.stopImmediatePropagation(); };
    const down = (event: KeyboardEvent) => {
      const state = latest.current;
      if (!standaloneApp()) { finish(false); return; }
      if (recentScreenChord(event)) {
        // 편집·파일 보기·확인창 등 기존 modal 입력은 그 창에 둔다. 설정 본문은 modal이 아니어서 전환할 수 있다.
        if (!state.enabled || (!cycling.current && document.querySelector('[aria-modal="true"], dialog[open], [role="menu"], [data-shortcut-recording="true"]'))) return;
        consume(event);
        const keys = recentList(cycling.current?.keys ?? (state.current ? visitRecent(history.current, state.current) : history.current), state.resolve).map((s) => s.key);
        const next = stepRecent(keys, cycling.current?.selected ?? state.current, event.shiftKey ? -1 : 1);
        if (!next) return;
        if (!cycling.current) opener.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
        update(next);
        return;
      }
      if (!cycling.current) return;
      if (event.isComposing || event.keyCode === 229) { finish(false); return; }
      if (event.key === "Escape") { consume(event); finish(false); }
      else if (event.key === "Enter") { consume(event); finish(true); }
      else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        consume(event);
        const keys = recentList(cycling.current.keys, state.resolve).map((s) => s.key);
        const next = stepRecent(keys, cycling.current.selected, event.key === "ArrowDown" ? 1 : -1);
        if (next) update(next);
      } else if (!event.ctrlKey) finish(false); // 창 전환 중 놓친 Ctrl 해제는 나중의 엉뚱한 입력을 가두지 않는다.
    };
    const up = (event: KeyboardEvent) => {
      if (!cycling.current) return;
      if (event.key === "Control" && !event.ctrlKey) { consume(event); finish(true); }
      else if (recentScreenChord(event)) consume(event);
    };
    const blur = () => finish(false);
    const hidden = () => { if (document.hidden) finish(false); };
    const onClick = (event: MouseEvent) => {
      if (!cycling.current) return;
      const scrim = (event.target as Element).closest(".recent-scrim");
      if (scrim) {
        const option = (event.target as Element).closest<HTMLElement>("[data-recent-key]");
        if (option?.dataset.recentKey) finish(true, option.dataset.recentKey);
        else if (event.target === scrim) finish(false);
      }
    };
    // capture에서 xterm과 다른 앱 키 처리기까지 전파를 막는다.
    window.addEventListener("keydown", down, true);
    window.addEventListener("keyup", up, true);
    window.addEventListener("blur", blur);
    document.addEventListener("visibilitychange", hidden);
    window.addEventListener("pagehide", blur);
    window.addEventListener("click", onClick);
    return () => {
      window.removeEventListener("keydown", down, true);
      window.removeEventListener("keyup", up, true);
      window.removeEventListener("blur", blur);
      document.removeEventListener("visibilitychange", hidden);
      window.removeEventListener("pagehide", blur);
      window.removeEventListener("click", onClick);
    };
  }, []);
  useEffect(() => {
    if (!enabled && cycling.current) {
      cycling.current = null;
      setCycle(null);
    }
  }, [enabled]);
  if (!cycle) return null;
  const selected = list.findIndex((s) => s.key === cycle.selected);
  return (
    <div className="recent-scrim">
      <section className="recent-switcher" role="dialog" aria-modal="true" aria-labelledby="recent-title">
        <header><h2 id="recent-title">최근에 연 화면</h2><p>최근 사용 순서 · 이 기기에서 연 화면</p></header>
        <div ref={surface} className="recent-options" role="listbox" tabIndex={-1} aria-label="최근에 연 화면" aria-activedescendant={selected >= 0 ? `recent-option-${selected}` : undefined}>
          {list.map((screen, i) => (
            <div id={`recent-option-${i}`} role="option" aria-selected={screen.key === cycle.selected} key={screen.key} data-recent-key={screen.key}>
              <span className="recent-mark" aria-hidden="true">{screen.key.startsWith("pane:") ? "›_" : "◇"}</span>
              <span className="recent-label"><strong>{screen.title}</strong><small>{screen.detail}</small></span>
              {screen.key === current && <span className="recent-current">현재 화면</span>}
            </div>
          ))}
        </div>
        {selected < 0 && <p role="status">고른 화면이 닫혔습니다. 다른 화면을 고르세요.</p>}
        <footer><span>Ctrl을 떼면 전환 · Esc 취소</span><span>Shift+Tab 반대 방향</span></footer>
      </section>
    </div>
  );
}
