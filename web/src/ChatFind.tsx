// 대화 안 찾기 칸과 그 상태(plan §8.308). 참고 t3code `chat/ThreadFindBar.tsx`(칸·개수·위아래·닫기, Enter/Shift+Enter, 조합 중
// Enter 무시), `useThreadFind.ts`(100ms 뒤 찾기, 연 자리부터 시작, 돌아가는 이동), `threadFindHighlights.ts`(CSS Highlight로 칠하고
// MutationObserver로 다시 칠하기), `useThreadFindNavigation.ts`(가상 목록에서 그 줄을 먼저 꺼내고, 찾음이 위아래 96px 안에 오도록
// 굴리고, 한 frame 뒤 다시 재기), `useChatShortcuts.ts`(Esc로 닫기). 참고와 다른 곳은 chatFindText.ts 머리에 있다.
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { ChevronDown, ChevronUp, History, LoaderCircle, X } from "lucide-react";
import type { ChatMessage } from "./gen/proto";
import { unwrapPasted } from "./pastedContent";
import { collectFindRanges, countInSegments, FIND_FOLD_ATTR, FIND_TEXT_ATTR, foldsHiding, formatFindCount, indexOf, matchAt, messageFindSegments,
  startIndex, stepFindIndex, type FindMatch, type FindRange } from "./chatFindText";
import { isMacPlatform } from "./herdr/shortcuts.ts";

const HIGHLIGHT = "aaa-chat-find";
const HIGHLIGHT_ACTIVE = "aaa-chat-find-active";
/** 참고 FIND_MATCH_VIEW_MARGIN: 찾음은 화면 위아래 96px 안쪽에 둔다(위에 뜬 찾기 칸에 가리지 않게) */
const VIEW_MARGIN = 96;
/** 자리를 잡는 동안 다시 재는 한도: 줄 높이가 계속 바뀌어도 끝없이 굴리지 않는다 */
const REVEAL_TRIES = 30;

function setMark(el: Element, name: string, value: string | null) {
  if (el.getAttribute(name) === value) return;
  if (value === null) el.removeAttribute(name);
  else el.setAttribute(name, value);
}

const highlights = () => (typeof CSS !== "undefined" && "highlights" in CSS && typeof Highlight !== "undefined" ? CSS.highlights : null);

/** 사람 말·답만 찾는다(참고: 도구 줄·생각은 찾지 않는다). 사람 말은 그린 글(붙여넣기 표지를 뗀 것)로 */
export function findText(m: ChatMessage): string | null {
  return m.role === "user" ? unwrapPasted(m.text) : m.role === "assistant" ? m.text : null;
}

function useDebounced<T>(value: T, ms: number, enabled: boolean): T {
  const [settled, setSettled] = useState(value);
  useEffect(() => {
    if (!enabled) return;
    const timer = window.setTimeout(() => setSettled(value), ms);
    return () => window.clearTimeout(timer);
  }, [value, ms, enabled]);
  return settled;
}

export interface ChatFindOptions {
  enabled?: boolean;
  /** 대화 굴림 칸(`.chat`) */
  container: React.RefObject<HTMLDivElement | null>;
  messages: readonly ChatMessage[];
  /** messages와 같은 차례의 줄 id(ChatRow `data-chat-id`) */
  rowIds: readonly string[];
  /** 파일 경로를 열 수 있는지(그린 글이 다르다, chatFindText.markdownFindSegments) */
  canOpen: boolean;
  /** 머리 돋보기·팔레트가 올리는 수: 바뀌면 연다 */
  openRequest: number;
  /** 받지 않은 앞 기록이 남았다(chat.before, 앞 쪽을 못 읽는 옛 agent는 truncated): 개수 뒤에 「+」 */
  more: boolean;
  /** 앞 쪽 하나를 받아 붙인다(Chat.tsx pageOlder). 앞 쪽을 읽을 수 없으면(처음이다·옛 agent) null */
  loadEarlier: (() => Promise<OlderOutcome>) | null;
}

/** 앞 쪽 하나를 받은 결과: 붙였다·처음이다·실패·이 대화 읽기가 끝났다(pane 바뀜) */
export type OlderOutcome = "loaded" | "done" | "failed" | "gone";

/** 「앞 기록도 찾기」(plan §8.310): 쉼·받는 중·실패와 이번에 받은 쪽 수 */
export interface EarlierState {
  state: "idle" | "loading" | "failed";
  pages: number;
}

export function useChatFind({ enabled = true, container, messages, rowIds, canOpen, openRequest, more, loadEarlier }: ChatFindOptions) {
  const enabledRef = useRef(enabled);
  enabledRef.current = enabled;
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [focusRequest, setFocusRequest] = useState(0);
  const [active, setActive] = useState<FindMatch | null>(null);
  // 이동마다 하나 오른다: 같은 자리로 다시 가도(한 개뿐일 때 Enter) 다시 보인다
  const [navigation, setNavigation] = useState(0);
  const [revealed, setRevealed] = useState<number | null>(null);
  const restoreFocus = useRef<HTMLElement | null>(null);
  /** 인용 위치로 돌아가기(t-1222): 이 말을 찾으면 처음 찾음을 그 줄에서 고른다 */
  const seeking = useRef<{ query: string; rowId: string | null } | null>(null);
  const trimmed = query.trim();
  const debounced = useDebounced(trimmed, 100, enabled);
  // 참고처럼 친 말이 100ms 머문 뒤에 찾는다. 그 사이에는 앞 결과를 그대로 둔다
  const searched = enabled && open ? debounced : "";

  // 줄마다 센 수는 같은 말·같은 글이면 다시 세지 않는다: 앞 쪽을 이어 받는 동안 새 쪽의 말만 센다(받는 쪽과 세는 쪽을 나눔, plan §8.310)
  const counted = useRef<{ key: string; byId: Map<string, { text: string; count: number }> }>({ key: "", byId: new Map() });
  const rows = useMemo(() => {
    if (searched === "") return [];
    const key = `${canOpen ? 1 : 0}${searched}`;
    if (counted.current.key !== key) counted.current = { key, byId: new Map() };
    const { byId } = counted.current;
    const list: { id: string; count: number }[] = [];
    messages.forEach((m, i) => {
      const text = findText(m);
      const id = rowIds[i];
      if (text === null || id === undefined) return;
      let entry = byId.get(id);
      if (entry === undefined || entry.text !== text) {
        entry = { text, count: countInSegments(messageFindSegments(m, text, canOpen), searched) };
        byId.set(id, entry);
      }
      list.push({ id, count: entry.count });
    });
    return list;
  }, [messages, rowIds, canOpen, searched]);
  const total = useMemo(() => rows.reduce((sum, row) => sum + row.count, 0), [rows]);
  const activeIndex = active === null ? null : indexOf(rows, active);

  /** 화면 맨 위에 걸친 찾을 줄과 그 줄에서 위로 나간 찾음 수(참고 readThreadFindPosition) */
  const firstVisible = useCallback((list: readonly { id: string; count: number }[], q: string) => {
    const el = container.current;
    if (el === null) return null;
    const top = el.getBoundingClientRect().top;
    // 줄은 위에서 아래 차례다: 화면 맨 위에 걸친 줄을 둘로 나눠 찾는다(수만 줄을 받은 뒤 끝을 보면서 말을 바꿔도 굳지 않게, plan §8.310)
    const nodes = new Map<string, Element>();
    for (const node of el.querySelectorAll(".chat-row[data-chat-id]")) nodes.set(node.getAttribute("data-chat-id")!, node);
    const found = list.flatMap((row) => {
      const node = nodes.get(row.id);
      return node === undefined ? [] : [{ id: row.id, node }];
    });
    let lo = 0;
    let hi = found.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (found[mid]!.node.getBoundingClientRect().bottom <= top) lo = mid + 1;
      else hi = mid;
    }
    const first = found[lo];
    if (first === undefined) return null;
    const passed = collectFindRanges(first.node, q).filter(({ range }) => range.getBoundingClientRect().bottom <= top).length;
    return { rowId: first.id, passed };
  }, [container]);

  // 말이 바뀌면(찾기를 연 때 포함) 보이는 자리부터 다시 고른다(참고 onQueryChange·open의 start)
  const [startedFor, setStartedFor] = useState<string | null>(null);
  useLayoutEffect(() => {
    if (searched !== startedFor) {
      setStartedFor(searched);
      if (searched === "" || total === 0) {
        setActive(null);
        return;
      }
      const seek = seeking.current?.query === searched ? seeking.current : null;
      seeking.current = null;
      const row = seek && rows.find((r) => r.id === seek.rowId && r.count > 0);
      setActive(row ? { rowId: row.id, occurrence: 0 } : matchAt(rows, startIndex(rows, firstVisible(rows, searched))));
      setNavigation((n) => n + 1);
      return;
    }
    // 같은 말인데 보던 찾음의 줄이 기록에서 빠졌으면(기록 교체) 처음 찾음으로. 굴리지 않는다. 기록이 늘어 처음 생긴 찾음도 고른다
    if (active !== null ? activeIndex === null : total > 0 && searched !== "") setActive(total > 0 ? matchAt(rows, 0) : null);
  }, [searched, startedFor, rows, total, firstVisible, active, activeIndex]);

  const step = (delta: number) => {
    if (total === 0 || searched !== trimmed) return;
    setActive(matchAt(rows, stepFindIndex(activeIndex ?? 0, total, delta)));
    setNavigation((n) => n + 1);
  };

  const openFind = useCallback(() => {
    if (!enabledRef.current) return;
    if (!open) restoreFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    setOpen(true);
    // 이미 열렸어도 다시 칸에 가고 말을 고른다(참고 focusRequestId). 말이 있으면 보이는 자리부터 다시
    setFocusRequest((n) => n + 1);
    setStartedFor(null);
  }, [open]);
  /** 찾기 칸을 그 말로 열고 그 줄의 처음 찾음으로 간다(인용 위치로 돌아가기, t-1222). 줄이 없으면 보통 찾기처럼 */
  const seek = useCallback((text: string, rowId: string | null) => {
    if (!enabledRef.current) return;
    seeking.current = { query: text.trim(), rowId };
    setOpen(true);
    setQuery(text);
    setStartedFor(null);
  }, []);
  // 「앞 기록도 찾기」(plan §8.310, 참고 t3code useThreadFindNavigation `onLoadEarlier`): 처음까지 앞 쪽을 차례로 받는다. 쪽마다 위
  // rows가 새 말만 세어 개수가 는다. 멈춤·닫기·실패·처음이면 그친다. 받던 쪽 하나는 끝까지 붙인다(그 쪽도 센다)
  const [earlier, setEarlier] = useState<EarlierState>({ state: "idle", pages: 0 });
  const earlierRun = useRef(0);
  const loadEarlierRef = useRef(loadEarlier);
  loadEarlierRef.current = loadEarlier;
  const startEarlier = useCallback(async () => {
    const run = ++earlierRun.current;
    let pages = 0;
    setEarlier({ state: "loading", pages });
    for (;;) {
      const load = loadEarlierRef.current;
      const outcome = load === null ? "done" : await load();
      if (earlierRun.current !== run) return;
      if (outcome === "loaded") {
        setEarlier({ state: "loading", pages: ++pages });
        continue;
      }
      earlierRun.current++;
      setEarlier({ state: outcome === "failed" ? "failed" : "idle", pages });
      return;
    }
  }, []);
  const stopEarlier = useCallback(() => {
    earlierRun.current++;
    setEarlier((now) => (now.state === "idle" ? now : { state: "idle", pages: now.pages }));
  }, []);
  useEffect(() => () => {
    earlierRun.current++;
  }, []);

  const close = useCallback(() => {
    stopEarlier();
    setOpen(false);
    // 참고 CLOSED_FIND: 닫으면 말도 비운다
    setQuery("");
    setActive(null);
    setStartedFor(null);
    setRevealed(null);
    const back = restoreFocus.current;
    restoreFocus.current = null;
    if (enabledRef.current && back?.isConnected) back.focus({ preventScroll: true });
  }, [stopEarlier]);

  useEffect(() => {
    if (enabled) return;
    stopEarlier();
    setOpen(false);
    restoreFocus.current = null;
  }, [enabled, stopEarlier]);

  const seenRequest = useRef(openRequest);
  useEffect(() => {
    if (!enabled || openRequest === seenRequest.current) return;
    seenRequest.current = openRequest;
    openFind();
  }, [enabled, openRequest, openFind]);

  // ⌘F(Mac)·Ctrl+F: 대화를 보는 동안은 브라우저 찾기 대신 이것(참고 keybindings `mod+f` → chat.find, 터미널 focus가 아닐 때).
  // Esc는 열렸을 때 닫는다(참고 useChatShortcuts dismissFind: 조합 중·이미 쓴 Esc·열린 창이면 두고)
  useEffect(() => {
    if (!enabled) return;
    const mac = isMacPlatform();
    const modalOpen = () => document.querySelector("[aria-modal='true'], dialog[open]") !== null;
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (!enabledRef.current || event.isComposing || event.keyCode === 229 || modalOpen()) return;
      const mod = mac ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey;
      if (mod && !event.shiftKey && !event.altKey && (event.code === "KeyF" || event.key.toLowerCase() === "f")) {
        event.preventDefault();
        event.stopPropagation();
        openFind();
      }
    };
    const onEscape = (event: globalThis.KeyboardEvent) => {
      if (!enabledRef.current || event.key !== "Escape" || !open || event.defaultPrevented || event.isComposing || event.keyCode === 229 || modalOpen()) return;
      event.preventDefault();
      close();
    };
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("keydown", onEscape);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("keydown", onEscape);
    };
  }, [enabled, open, openFind, close]);

  // 칠하기와 그 자리로 가기(참고 useThreadFindHighlights·useThreadFindNavigation). 그린 줄의 Range만 칠한다: 가상화로 본문이 없는 줄은
  // 가까이 올 때 mount되고 MutationObserver가 다시 칠한다
  const rangesRef = useRef(new Map<string, FindRange[]>());
  const paintRef = useRef<(() => void) | null>(null);
  const activeRef = useRef<{ match: FindMatch | null; navigation: number }>({ match: active, navigation });
  activeRef.current = { match: active, navigation };
  const countsRef = useRef(new Map<string, number>());
  countsRef.current = new Map(rows.map((row) => [row.id, row.count]));
  const revealState = useRef<{ navigation: number; tries: number; frame: number | null } | null>(null);

  useEffect(() => {
    if (!enabled) return;
    const el = container.current;
    const registry = highlights();
    const clearRows = () => {
      for (const row of el?.querySelectorAll(".chat-row[data-find-count]") ?? []) {
        row.removeAttribute("data-find-count");
        row.removeAttribute("data-find-dom");
      }
    };
    if (el === null || searched === "") {
      registry?.delete(HIGHLIGHT);
      registry?.delete(HIGHLIGHT_ACTIVE);
      rangesRef.current = new Map();
      paintRef.current = null;
      clearRows();
      return;
    }
    const inactive = registry ? new Highlight() : null;
    const current = registry ? new Highlight() : null;
    if (registry && inactive && current) {
      registry.set(HIGHLIGHT, inactive);
      registry.set(HIGHLIGHT_ACTIVE, current);
    }
    const cache = new Map<Element, FindRange[]>();
    const tryReveal = () => {
      const want = activeRef.current;
      const state = revealState.current;
      if (want.match === null || state === null || state.navigation !== want.navigation) return;
      if (state.frame !== null) return;
      if (++state.tries > REVEAL_TRIES) {
        revealState.current = null;
        setRevealed(want.navigation);
        return;
      }
      const row = el.querySelector(`.chat-row[data-chat-id="${CSS.escape(want.match.rowId)}"]`);
      if (row === null) return;
      const view = el.getBoundingClientRect();
      const list = rangesRef.current.get(want.match.rowId) ?? [];
      const target = list[Math.min(want.match.occurrence, list.length - 1)];
      if (target === undefined) {
        // 본문이 아직 없다(화면 밖 가상 줄): 그 줄을 화면 위 여백 자리로 꺼내면 ChatRow가 mount하고 다시 칠할 때 이어 간다.
        // 본문이 있는데 그린 글에 없으면(데이터와 그린 글이 어긋남) 그 줄을 보인 것으로 끝낸다
        const top = row.getBoundingClientRect().top;
        if (Math.abs(top - (view.top + VIEW_MARGIN)) >= 1) el.scrollTop += top - view.top - VIEW_MARGIN;
        if (row.querySelector("[data-chat-find-text]") !== null) {
          revealState.current = null;
          setRevealed(want.navigation);
        }
        return;
      }
      const folds = foldsHiding(target.range, el);
      if (folds.length > 0) {
        // 접힌 곳은 편다(참고 revealFolded의 beforematch). 펼친 뒤 바뀐 DOM으로 다시 칠할 때 이어 간다
        for (const fold of folds) fold.dispatchEvent(new Event("beforematch"));
        return;
      }
      const code = target.range.startContainer.parentElement?.closest("pre");
      if (code && code.scrollWidth > code.clientWidth) {
        const rect = target.range.getBoundingClientRect();
        const box = code.getBoundingClientRect();
        if (rect.left < box.left) code.scrollLeft += rect.left - box.left - 16;
        else if (rect.right > box.right) code.scrollLeft += rect.right - box.right + 16;
      }
      const rect = target.range.getBoundingClientRect();
      const margin = Math.min(VIEW_MARGIN, Math.max(0, (view.height - rect.height) / 2));
      const delta = rect.top < view.top + margin ? rect.top - view.top - margin : rect.bottom > view.bottom - margin ? rect.bottom - view.bottom + margin : 0;
      if (Math.abs(delta) >= 1) el.scrollTop += delta;
      // 주변 줄이 mount되며 높이가 바뀌고 보던 줄 붙잡기(Chat.tsx holdAnchor)가 움직일 수 있다: 한 frame 뒤 다시 재서 그대로면 끝
      const before = { scroll: el.scrollTop, top: rect.top - delta };
      state.frame = requestAnimationFrame(() => {
        state.frame = null;
        if (revealState.current !== state) return;
        const now = target.range.startContainer.isConnected ? target.range.getBoundingClientRect().top : NaN;
        if (Math.abs(delta) < 1 && el.scrollTop === before.scroll && Math.abs(now - before.top) < 1) {
          revealState.current = null;
          setRevealed(want.navigation);
          return;
        }
        tryReveal();
      });
    };
    const repaint = () => {
      // 본문이 그려진 줄만 본다: 가상화로 본문이 없는 줄에는 칠할 글이 없다. 앞 기록을 수만 줄 받아도 다시 칠하기가 화면 근처만큼이다
      const rows = new Set<Element>();
      for (const scope of el.querySelectorAll(`[${FIND_TEXT_ATTR}]`)) {
        const row = scope.closest(".chat-row[data-chat-id]");
        if (row !== null) rows.add(row);
      }
      const byRow = new Map<string, FindRange[]>();
      for (const row of rows) {
        const id = row.getAttribute("data-chat-id")!;
        const count = countsRef.current.get(id);
        if (count === undefined) continue;
        let list = cache.get(row);
        if (list === undefined || list.some(({ range }) => !row.contains(range.startContainer) || !row.contains(range.endContainer))) {
          list = collectFindRanges(row, searched);
          cache.set(row, list);
        }
        // 시험 표지: 데이터로 센 수와 그린 글에서 찾은 수
        setMark(row, "data-find-count", String(count));
        setMark(row, "data-find-dom", String(list.length));
        byRow.set(id, list);
      }
      for (const row of cache.keys()) {
        if (rows.has(row) && countsRef.current.has(row.getAttribute("data-chat-id")!)) continue;
        cache.delete(row);
        setMark(row, "data-find-count", null);
        setMark(row, "data-find-dom", null);
      }
      rangesRef.current = byRow;
      if (inactive && current) {
        inactive.clear();
        current.clear();
        const want = activeRef.current.match;
        for (const [id, list] of byRow) {
          list.forEach(({ range }, n) => {
            if (foldsHiding(range, el).length > 0) return;
            const isActive = want !== null && want.rowId === id && Math.min(want.occurrence, list.length - 1) === n;
            (isActive ? current : inactive).add(range);
          });
        }
      }
      tryReveal();
    };
    paintRef.current = repaint;
    let frame: number | null = null;
    const observer = new MutationObserver((mutations) => {
      // 줄 안 시계·상태 글이 바뀌어도 칠하기는 찾을 글·접힘이 바뀔 때만(참고 affectsSearchableText)
      const relevant = mutations.some((m) => {
        if (m.type === "attributes") return true;
        const target = m.target.nodeType === 1 ? (m.target as Element) : m.target.parentElement;
        if (target?.closest("[data-chat-find-text]")) return true;
        const touches = (node: Node) => node.nodeType === 1 && ((node as Element).matches("[data-chat-find-text], .chat-row") || (node as Element).querySelector("[data-chat-find-text]") !== null);
        return [...m.addedNodes, ...m.removedNodes].some(touches);
      });
      if (!relevant) return;
      // 바뀐 줄은 다시 모은다(본문이 mount된 가상 줄도 여기로 온다)
      for (const m of mutations) {
        const target = m.target.nodeType === 1 ? (m.target as Element) : m.target.parentElement;
        const row = target?.closest(".chat-row");
        if (row) cache.delete(row);
      }
      if (frame !== null) return;
      frame = requestAnimationFrame(() => {
        frame = null;
        repaint();
      });
    });
    observer.observe(el, { subtree: true, childList: true, characterData: true, attributes: true, attributeFilter: ["hidden", FIND_FOLD_ATTR, "class"] });
    repaint();
    return () => {
      observer.disconnect();
      if (frame !== null) cancelAnimationFrame(frame);
      if (revealState.current?.frame != null) cancelAnimationFrame(revealState.current.frame);
      revealState.current = null;
      paintRef.current = null;
      registry?.delete(HIGHLIGHT);
      registry?.delete(HIGHLIGHT_ACTIVE);
      clearRows();
    };
    // rows가 바뀌면(새 말·앞 쪽) 아래 effect가 다시 칠한다
  }, [enabled, container, searched]);

  // 고른 찾음이 바뀌면 다시 칠하고 그 자리로 간다. 기록만 늘었으면(같은 이동) 칠하기만 한다
  useLayoutEffect(() => {
    if (active !== null && revealState.current?.navigation !== navigation && revealed !== navigation) {
      if (revealState.current?.frame != null) cancelAnimationFrame(revealState.current.frame);
      revealState.current = { navigation, tries: 0, frame: null };
    }
    paintRef.current?.();
  }, [active, navigation, rows, revealed]);

  const counting = searched !== trimmed;
  return {
    open,
    openFind,
    seek,
    bar: {
      open,
      query,
      total: counting ? null : total,
      index: activeIndex ?? 0,
      activeKey: active === null || counting ? null : `${active.rowId}:${active.occurrence}`,
      revealed: revealed === navigation && active !== null && !counting,
      more,
      canLoadEarlier: loadEarlier !== null,
      earlier,
      onLoadEarlier: () => void startEarlier(),
      onStopEarlier: stopEarlier,
      focusRequest,
      onQueryChange: setQuery,
      onNext: () => step(1),
      onPrevious: () => step(-1),
      onClose: close,
    },
  };
}

export interface ChatFindBarProps {
  open: boolean;
  query: string;
  /** null이면 아직 세는 중(친 말이 머무는 100ms) */
  total: number | null;
  index: number;
  activeKey: string | null;
  revealed: boolean;
  more: boolean;
  /** 앞 쪽을 더 받을 수 있다(「앞 기록도 찾기」를 보인다) */
  canLoadEarlier: boolean;
  earlier: EarlierState;
  onLoadEarlier: () => void;
  onStopEarlier: () => void;
  focusRequest: number;
  onQueryChange: (query: string) => void;
  onNext: () => void;
  onPrevious: () => void;
  onClose: () => void;
}

/** 찾기 칸(참고 ThreadFindBar): 대화 위 오른쪽에 뜬다. 문구는 참고 「Find in thread」·「Previous match」… 의 한국어 */
export function ChatFindBar(props: ChatFindBarProps) {
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (!props.open) return;
    input.current?.focus();
    input.current?.select();
    // 찾기를 다시 부를 때마다 칸에 가서 말을 고른다
  }, [props.focusRequest, props.open]);
  if (!props.open) return null;
  const hasQuery = props.query.trim().length > 0;
  const none = hasQuery && props.total === 0;
  const label = !hasQuery ? "" : props.total === null ? "…" : `${formatFindCount(props.index, props.total)}${props.more ? "+" : ""}`;
  const disabled = !props.total;
  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229) return;
    if (event.key === "Enter") {
      event.preventDefault();
      event.stopPropagation();
      if (event.shiftKey) props.onPrevious();
      else props.onNext();
    }
  };
  const loading = props.earlier.state === "loading";
  // 앞 기록 줄(칸 아래 작은 줄): 받을 앞이 남았고 찾는 말이 있을 때, 받는 중이면 말을 지워도 남는다. 처음까지 받으면 「+」와 함께 사라진다
  const showEarlier = props.more && props.canLoadEarlier && (hasQuery || loading);
  const moreTitle = !props.more || !hasQuery ? undefined
    : props.canLoadEarlier ? "받아 둔 기록에서만 셌습니다. 「앞 기록도 찾기」로 처음까지 받아 함께 찾습니다"
      : "받아 둔 기록에서만 셌습니다. 앞 기록을 읽으려면 host와 agent를 갱신하세요";
  return (
    <div className="chat-find" role="search" aria-label="대화에서 찾기" data-testid="chat-find"
      data-find-count={props.total ?? undefined} data-find-index={props.total ? props.index : undefined}
      data-find-active={props.activeKey ?? undefined} data-find-revealed={props.revealed || undefined}
      data-find-earlier={props.earlier.state} data-find-pages={props.earlier.pages}>
      <div className="chat-find-bar">
        <input ref={input} type="search" className="chat-find-input" value={props.query} aria-label="대화에서 찾기" placeholder="대화에서 찾기"
          maxLength={200} spellCheck={false} autoComplete="off" enterKeyHint="search" onChange={(e) => props.onQueryChange(e.target.value)} onKeyDown={onKeyDown} />
        <span className={`chat-find-count${none ? " is-none" : ""}`} aria-live="polite" data-testid="chat-find-count" title={moreTitle}>{label}</span>
        <button type="button" className="icon-button chat-find-step" aria-label="이전 찾음" title="이전 찾음 (Shift+Enter)" disabled={disabled}
          onPointerDown={(e) => e.preventDefault()} onClick={props.onPrevious}>
          <ChevronUp aria-hidden="true" />
        </button>
        <button type="button" className="icon-button chat-find-step" aria-label="다음 찾음" title="다음 찾음 (Enter)" disabled={disabled}
          onPointerDown={(e) => e.preventDefault()} onClick={props.onNext}>
          <ChevronDown aria-hidden="true" />
        </button>
        <button type="button" className="icon-button chat-find-step" aria-label="찾기 닫기" title="닫기 (Esc)" onClick={props.onClose}>
          <X aria-hidden="true" />
        </button>
      </div>
      {showEarlier && (
        <div className="chat-find-earlier" data-testid="chat-find-earlier">
          {loading ? (
            <>
              <LoaderCircle className="chat-find-spin" aria-hidden="true" />
              <span className="chat-find-earlier-note" role="status">앞 기록 받는 중{props.earlier.pages > 0 ? ` · ${props.earlier.pages}쪽` : ""}</span>
              <button type="button" className="chat-find-earlier-action" onPointerDown={(e) => e.preventDefault()} onClick={props.onStopEarlier}>멈춤</button>
            </>
          ) : (
            <>
              {props.earlier.state === "failed" && <span className="chat-find-earlier-note is-failed" role="status">앞 기록을 받지 못했습니다</span>}
              <button type="button" className="chat-find-earlier-action" onPointerDown={(e) => e.preventDefault()} onClick={props.onLoadEarlier}>
                <History aria-hidden="true" />
                {props.earlier.state === "failed" ? "다시 받기" : props.earlier.pages > 0 ? "이어 찾기" : "앞 기록도 찾기"}
              </button>
            </>
          )}
        </div>
      )}
    </div>
  );
}
