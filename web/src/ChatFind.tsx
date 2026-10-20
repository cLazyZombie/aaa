// 대화 안 찾기 칸과 그 상태(plan §8.308). 참고 t3code `chat/ThreadFindBar.tsx`(칸·개수·위아래·닫기, Enter/Shift+Enter, 조합 중
// Enter 무시), `useThreadFind.ts`(100ms 뒤 찾기, 연 자리부터 시작, 돌아가는 이동), `threadFindHighlights.ts`(CSS Highlight로 칠하고
// MutationObserver로 다시 칠하기), `useThreadFindNavigation.ts`(가상 목록에서 그 줄을 먼저 꺼내고, 찾음이 위아래 96px 안에 오도록
// 굴리고, 한 frame 뒤 다시 재기), `useChatShortcuts.ts`(Esc로 닫기). 참고와 다른 곳은 chatFindText.ts 머리에 있다.
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { ChevronDown, ChevronUp, X } from "lucide-react";
import type { ChatMessage } from "./gen/proto";
import { unwrapPasted } from "./pastedContent";
import { collectFindRanges, countInSegments, FIND_FOLD_ATTR, foldsHiding, formatFindCount, indexOf, matchAt, messageFindSegments,
  startIndex, stepFindIndex, type FindMatch, type FindRange } from "./chatFindText";
import { isMacPlatform } from "./herdr/shortcuts.ts";

const HIGHLIGHT = "aaa-chat-find";
const HIGHLIGHT_ACTIVE = "aaa-chat-find-active";
/** 참고 FIND_MATCH_VIEW_MARGIN: 찾음은 화면 위아래 96px 안쪽에 둔다(위에 뜬 찾기 칸에 가리지 않게) */
const VIEW_MARGIN = 96;
/** 자리를 잡는 동안 다시 재는 한도: 줄 높이가 계속 바뀌어도 끝없이 굴리지 않는다 */
const REVEAL_TRIES = 30;

const highlights = () => (typeof CSS !== "undefined" && "highlights" in CSS && typeof Highlight !== "undefined" ? CSS.highlights : null);

/** 사람 말·답만 찾는다(참고: 도구 줄·생각은 찾지 않는다). 사람 말은 그린 글(붙여넣기 표지를 뗀 것)로 */
function findText(m: ChatMessage): string | null {
  return m.role === "user" ? unwrapPasted(m.text) : m.role === "assistant" ? m.text : null;
}

function useDebounced<T>(value: T, ms: number): T {
  const [settled, setSettled] = useState(value);
  useEffect(() => {
    const timer = window.setTimeout(() => setSettled(value), ms);
    return () => window.clearTimeout(timer);
  }, [value, ms]);
  return settled;
}

export interface ChatFindOptions {
  /** 대화 굴림 칸(`.chat`) */
  container: React.RefObject<HTMLDivElement | null>;
  messages: readonly ChatMessage[];
  /** messages와 같은 차례의 줄 id(ChatRow `data-chat-id`) */
  rowIds: readonly string[];
  /** 파일 경로를 열 수 있는지(그린 글이 다르다, chatFindText.markdownFindSegments) */
  canOpen: boolean;
  /** 머리 돋보기·팔레트가 올리는 수: 바뀌면 연다 */
  openRequest: number;
  /** 받지 않은 앞 기록이 남았다(chat.before): 개수 뒤에 「+」, 1단계는 받은 범위만 찾는다 */
  more: boolean;
}

export function useChatFind({ container, messages, rowIds, canOpen, openRequest, more }: ChatFindOptions) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [focusRequest, setFocusRequest] = useState(0);
  const [active, setActive] = useState<FindMatch | null>(null);
  // 이동마다 하나 오른다: 같은 자리로 다시 가도(한 개뿐일 때 Enter) 다시 보인다
  const [navigation, setNavigation] = useState(0);
  const [revealed, setRevealed] = useState<number | null>(null);
  const restoreFocus = useRef<HTMLElement | null>(null);
  const trimmed = query.trim();
  const debounced = useDebounced(trimmed, 100);
  // 참고처럼 친 말이 100ms 머문 뒤에 찾는다. 그 사이에는 앞 결과를 그대로 둔다
  const searched = open ? debounced : "";

  const rows = useMemo(() => {
    if (searched === "") return [];
    const list: { id: string; count: number }[] = [];
    messages.forEach((m, i) => {
      const text = findText(m);
      const id = rowIds[i];
      if (text === null || id === undefined) return;
      list.push({ id, count: countInSegments(messageFindSegments(text, canOpen), searched) });
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
    for (const row of list) {
      const node = el.querySelector(`.chat-row[data-chat-id="${CSS.escape(row.id)}"]`);
      if (node === null || node.getBoundingClientRect().bottom <= top) continue;
      const passed = collectFindRanges(node, q).filter(({ range }) => range.getBoundingClientRect().bottom <= top).length;
      return { rowId: row.id, passed };
    }
    return null;
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
      setActive(matchAt(rows, startIndex(rows, firstVisible(rows, searched))));
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
    if (!open) restoreFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    setOpen(true);
    // 이미 열렸어도 다시 칸에 가고 말을 고른다(참고 focusRequestId). 말이 있으면 보이는 자리부터 다시
    setFocusRequest((n) => n + 1);
    setStartedFor(null);
  }, [open]);
  const close = useCallback(() => {
    setOpen(false);
    // 참고 CLOSED_FIND: 닫으면 말도 비운다
    setQuery("");
    setActive(null);
    setStartedFor(null);
    setRevealed(null);
    const back = restoreFocus.current;
    restoreFocus.current = null;
    if (back?.isConnected) back.focus({ preventScroll: true });
  }, []);

  const seenRequest = useRef(openRequest);
  useEffect(() => {
    if (openRequest === seenRequest.current) return;
    seenRequest.current = openRequest;
    openFind();
  }, [openRequest, openFind]);

  // ⌘F(Mac)·Ctrl+F: 대화를 보는 동안은 브라우저 찾기 대신 이것(참고 keybindings `mod+f` → chat.find, 터미널 focus가 아닐 때).
  // Esc는 열렸을 때 닫는다(참고 useChatShortcuts dismissFind: 조합 중·이미 쓴 Esc·열린 창이면 두고)
  useEffect(() => {
    const mac = isMacPlatform();
    const modalOpen = () => document.querySelector("[aria-modal='true'], dialog[open]") !== null;
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (event.isComposing || event.keyCode === 229 || modalOpen()) return;
      const mod = mac ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey;
      if (mod && !event.shiftKey && !event.altKey && (event.code === "KeyF" || event.key.toLowerCase() === "f")) {
        event.preventDefault();
        event.stopPropagation();
        openFind();
      }
    };
    const onEscape = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "Escape" || !open || event.defaultPrevented || event.isComposing || event.keyCode === 229 || modalOpen()) return;
      event.preventDefault();
      close();
    };
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("keydown", onEscape);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("keydown", onEscape);
    };
  }, [open, openFind, close]);

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
      const rows = new Set(el.querySelectorAll(".chat-row[data-chat-id]"));
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
        // 시험 표지: 데이터로 센 수와 그린 글에서 찾은 수(mount된 줄만 dom이 있다)
        row.setAttribute("data-find-count", String(count));
        if (row.querySelector("[data-chat-find-text]")) row.setAttribute("data-find-dom", String(list.length));
        else row.removeAttribute("data-find-dom");
        byRow.set(id, list);
      }
      for (const row of cache.keys()) if (!rows.has(row)) cache.delete(row);
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
  }, [container, searched]);

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
    bar: {
      open,
      query,
      total: counting ? null : total,
      index: activeIndex ?? 0,
      activeKey: active === null || counting ? null : `${active.rowId}:${active.occurrence}`,
      revealed: revealed === navigation && active !== null && !counting,
      more,
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
  return (
    <div className="chat-find" role="search" aria-label="대화에서 찾기" data-testid="chat-find"
      data-find-count={props.total ?? undefined} data-find-index={props.total ? props.index : undefined}
      data-find-active={props.activeKey ?? undefined} data-find-revealed={props.revealed || undefined}>
      <input ref={input} type="search" className="chat-find-input" value={props.query} aria-label="대화에서 찾기" placeholder="대화에서 찾기"
        maxLength={200} spellCheck={false} autoComplete="off" enterKeyHint="search" onChange={(e) => props.onQueryChange(e.target.value)} onKeyDown={onKeyDown} />
      <span className={`chat-find-count${none ? " is-none" : ""}`} aria-live="polite" data-testid="chat-find-count"
        title={props.more && hasQuery ? "받아 둔 기록에서만 찾았습니다. 앞 기록은 위로 굴려 더 불러오면 함께 찾습니다" : undefined}>{label}</span>
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
  );
}
