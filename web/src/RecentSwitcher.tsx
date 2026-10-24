import { useLayoutEffect, useMemo, useRef, useState } from "react";
import type { PaletteTarget } from "./palette.ts";
import { currentRecent, visitScreen } from "./recentHistory.ts";
import { liveRecentTargets, moveRecentCycle, recentScreenChord, startRecentCycle, visitedScreens, type RecentCycle } from "./recentScreens.ts";
import { useTerminalToggle } from "./settings.ts";
import { useFocusTrap } from "./herdr/useFocusTrap.ts";
import "./RecentSwitcher.css";

interface Props {
  standalone: boolean;
  blocked: boolean;
  targets: readonly PaletteTarget[];
  selected: string | null;
  onOpen: (pane: string) => void;
}

/** Ctrl 유지 중에는 본문·주소·MRU를 바꾸지 않는다. ref로 같은 프레임의 Tab→Ctrl 뗌도 처리한다. */
export function RecentSwitcher({ standalone, blocked, targets, selected, onOpen }: Props) {
  const enabled = useTerminalToggle("recentScreens");
  const live = useMemo(() => liveRecentTargets(targets), [targets]);
  const currentKey = targets.find((target) => selected !== null && target.keys.includes(selected))?.key ?? null;
  useLayoutEffect(() => { if (currentKey !== null) visitScreen(currentKey); }, [currentKey]);
  const [cycle, setCycle] = useState<RecentCycle | null>(null);
  const active = useRef<RecentCycle | null>(null);
  const origin = useRef<string | null>(null);
  const restore = useRef(true);
  const surface = useFocusTrap<HTMLElement>(cycle !== null, { shouldRestore: () => restore.current });
  const latest = useRef({ standalone, enabled, blocked, live, selected, currentKey, onOpen });
  latest.current = { standalone, enabled, blocked, live, selected, currentKey, onOpen };
  const finishRef = useRef<(commit: boolean) => void>(() => {});

  useLayoutEffect(() => {
    const consumed = new Set<string>();
    const change = (next: RecentCycle | null) => { active.current = next; setCycle(next); };
    const finish = (commit: boolean, allowRestore = true) => {
      const value = active.current;
      if (value === null) return;
      const now = latest.current;
      const target = commit && now.standalone && now.enabled && !now.blocked && now.selected === origin.current
        ? now.live.find((target) => target.key === value.key) : undefined;
      restore.current = allowRestore && document.hasFocus() && (!target || target.key === now.currentKey);
      change(null);
      if (target?.pane) { visitScreen(target.key); now.onOpen(target.pane.id); }
    };
    finishRef.current = finish;
    const consume = (event: KeyboardEvent) => { event.preventDefault(); event.stopImmediatePropagation(); };
    const onDown = (event: KeyboardEvent) => {
      const now = latest.current;
      if (active.current !== null) {
        consume(event);
        consumed.add(event.code || event.key);
        if (event.isComposing || event.keyCode === 229) return;
        if (event.key === "Escape") return finish(false);
        if (event.key === "Enter") return finish(true);
        const direction = event.key === "ArrowUp" || (event.key === "Tab" && event.shiftKey) ? -1 : 1;
        if (event.key === "Tab" || event.key === "ArrowUp" || event.key === "ArrowDown") {
          const next = moveRecentCycle(active.current, new Set(now.live.map((target) => target.key)), direction);
          if (next) change(next); else finish(false);
        }
        return;
      }
      if (!now.standalone || !now.enabled || now.blocked || !recentScreenChord(event)) return;
      // 다른 창의 입력·설정에서 기록 중인 키를 빼앗지 않는다.
      if (document.querySelector('[aria-modal="true"], dialog[open]')) return;
      consume(event);
      consumed.add(event.code || event.key);
      const next = startRecentCycle(visitedScreens(now.live, currentRecent()).map((target) => target.key), now.currentKey, event.shiftKey ? -1 : 1);
      if (next) {
        restore.current = true;
        origin.current = now.selected;
        change(next);
      }
    };
    const onUp = (event: KeyboardEvent) => {
      const owned = consumed.delete(event.code || event.key);
      if (active.current !== null || owned) consume(event);
      if (event.key === "Control") finish(true);
    };
    const onBlur = () => { finish(false, false); consumed.clear(); };
    const onHidden = () => { if (document.hidden) onBlur(); };
    window.addEventListener("keydown", onDown, true);
    window.addEventListener("keyup", onUp, true);
    window.addEventListener("blur", onBlur);
    document.addEventListener("visibilitychange", onHidden);
    return () => {
      window.removeEventListener("keydown", onDown, true);
      window.removeEventListener("keyup", onUp, true);
      window.removeEventListener("blur", onBlur);
      document.removeEventListener("visibilitychange", onHidden);
    };
  }, []);

  useLayoutEffect(() => {
    if (active.current && (!standalone || !enabled || blocked || selected !== origin.current || !live.some((target) => target.key === active.current?.key))) finishRef.current(false);
  }, [standalone, enabled, blocked, selected, live]);
  useLayoutEffect(() => { surface.current?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" }); }, [cycle, surface]);

  if (!cycle) return null;
  const rows = visitedScreens(live, cycle.keys);
  return <div className="modal-scrim palette-scrim" onMouseDown={(event) => { if (event.target === event.currentTarget) finishRef.current(false); }}>
    <section className="menu recent-switcher" role="dialog" aria-modal="true" aria-label="최근 화면" data-testid="recent-switcher" ref={surface} tabIndex={-1}>
      <header>최근 화면</header>
      <div role="listbox" tabIndex={0} aria-label="방문한 실행 중 화면" aria-activedescendant={`recent-screen-${rows.findIndex((row) => row.key === cycle.key)}`}>
        {rows.map((target, index) => <button key={target.key} id={`recent-screen-${index}`} type="button" role="option" aria-selected={target.key === cycle.key} tabIndex={-1}
          onClick={() => { active.current = { ...cycle, key: target.key }; finishRef.current(true); }}>
          <span>{target.title}</span><small>{[target.machine.name, target.project].filter(Boolean).join(" · ")}</small>
        </button>)}
      </div>
      <footer>Ctrl 놓기: 열기 · Shift+Tab: 반대 · Esc: 취소</footer>
    </section>
  </div>;
}
