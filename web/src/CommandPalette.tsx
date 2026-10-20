// 명령 팔레트(herdr-web-ui aa624cb src/components/CommandPalette.tsx를 옮김, 모양 herdr/CommandPalette.css 그대로, 문구는 참고
// i18n.ko.ts). Mod+Shift+K나 목록 머리 돋보기로 열고, 목록 줄(thread·pane)을 찾아 열거나 동작을 부른다. 찾지 않으면 최근 연 줄 셋이
// 앞에 선다. `>`는 동작만, 상태 칩(목록 밖에서 a·b·w·i·d 키)은 줄만 거른다. 설정 쪽은 찾을 때만 나온다. ←·→는 묶음을 건넌다.
// 다른 곳: 참고의 묶음은 workspace, aaa는 머신 안 프로젝트와 pane 묶음이다(palette.ts). 참고의 탭·나누기·확대·사이드바·PC 추가·
// 알림 켜기·파일·폴더 찾아보기·plugin 동작·worktree branch는 aaa에 그 화면이나 API가 없어 뺐다. plan §8.132.
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ComponentType, type FocusEvent as ReactFocusEvent, type KeyboardEvent as ReactKeyboardEvent, type MouseEvent as ReactMouseEvent } from "react";
import { FileText, ListChecks, ListTodo, MessageSquarePlus, PanelLeft, Settings, SunMoon, SwitchCamera, TextSearch, X } from "lucide-react";

import "./herdr/CommandPalette.css";

import type { MachineView, PaneView, ThreadView } from "./gen/proto";
import { AgentMark } from "./herdr/AgentMark.tsx";
import { formatKeys, shortcutDisplayKeys, type ShortcutId } from "./herdr/shortcuts.ts";
import { useFocusTrap } from "./herdr/useFocusTrap.ts";
import { useSheetSwipe } from "./herdr/sheets.ts";
import { usePhoneSheetBack } from "./herdr/sheetHistory.ts";
import { useMediaQuery } from "./herdr/useMediaQuery.ts";
import {
  ACTION_ALIASES,
  FILTER_KEYS,
  groupTargets,
  loadRecent,
  parseQuery,
  rankByTitle,
  rankTargets,
  RECENT_KEY,
  recentTargets,
  rememberTarget,
  requestSettingsPage,
  SETTINGS_PAGE_ENTRIES,
  paletteTargets,
  STATUS_FILTERS,
  statusCounts,
  type PaletteStatusFilter,
  type PaletteTarget,
  type SettingsPageId,
} from "./palette";
import { PAGES } from "./SettingsDialog";
import { StatusBadge } from "./Sidebar";
import { useShortcutOverrides, useSidebarShared } from "./settings";

const RECENT_SHOWN = 3;

export interface PaletteActions {
  /** 보기 권한 기기는 띄울 수 없다: null */
  openNewSession: (() => void) | null;
  /** 고른 pane이 있을 때만 */
  toggleView: (() => void) | null;
  view: "chat" | "terminal" | null;
  /** 대화 보기일 때만(참고 「Find in current thread」, plan §8.308) */
  openFind: (() => void) | null;
  toggleTheme: () => void;
  openSettings: (page?: SettingsPageId) => void;
  /** 모든 thread(끝난 것까지, `#threads`) */
  openThreads: () => void;
  /** 본문 보고서 화면(활동 막대 「보고서」, plan §8.240) */
  openReports: () => void;
  /** 본문 할 일 화면(활동 막대 「할 일」, plan §8.306) */
  openTasks: () => void;
  /** 사이드바 접기·펴기(휴대폰은 목록 drawer, 참고 CommandPalette `sidebar`) */
  toggleSidebar: () => void;
}

interface CommandPaletteProps {
  open: boolean;
  onClose: () => void;
  machines: readonly MachineView[];
  panes: readonly PaneView[];
  threads: readonly ThreadView[];
  /** 목록의 선택 값(`pane:…`·`thread:…`) */
  selected: string | null;
  agentOf: (p: PaneView | undefined, t?: ThreadView) => string | null;
  onOpen: (open: PaletteTarget["open"]) => void;
  actions: PaletteActions;
}

interface PaletteAction {
  id: string;
  label: string;
  icon: ComponentType;
  shortcut?: ShortcutId;
  run: () => void;
}

interface TargetRow { kind: "pane"; target: PaletteTarget }
interface ActionRow { kind: "action"; action: PaletteAction }
type PaletteRow = TargetRow | ActionRow;

interface PaletteSectionView {
  id: string;
  heading: string;
  machineName?: string;
  rows: PaletteRow[];
}

/**
 * The picked row, by what it is (its section and its pane or action) and by where it stood. A
 * roster change that moves the row (a pane before it left the filter) carries the pick with it
 * and tells its place again; one that takes the row away leaves the pick on the row now at its
 * place, among the rows that stayed, so the keyboard loses nothing.
 */
interface PaletteSelection {
  key: string | null;
  index: number;
}

const NO_SELECTION: PaletteSelection = { key: null, index: 0 };

function rowKey(sectionId: string, row: PaletteRow): string {
  return `${sectionId}:${row.kind === "pane" ? row.target.key : row.action.id}`;
}

function storeRecent(recent: string[]): string[] {
  try { window.localStorage.setItem(RECENT_KEY, JSON.stringify(recent)); } catch { /* private mode: keep this page's history */ }
  return recent;
}

const readRecent = (): string[] => loadRecent((key) => { try { return window.localStorage.getItem(key); } catch { return null; } });

function ShortcutHint({ shortcutId }: { shortcutId?: ShortcutId }) {
  const overrides = useShortcutOverrides();
  if (!shortcutId) return null;
  const keys = formatKeys(shortcutDisplayKeys(shortcutId, overrides));
  if (keys.length === 0) return null;
  return <span className="palette-shortcut" aria-label={keys.join(" + ")}>{keys.map((key) => <kbd className="kbd" key={key}>{key}</kbd>)}</span>;
}

export function CommandPalette({ open, onClose, machines, panes, threads, selected, agentOf, onOpen, actions }: CommandPaletteProps) {
  const touch = useMediaQuery("(pointer: coarse)");
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<PaletteStatusFilter>("all");
  const [selection, setSelection] = useState<PaletteSelection>(NO_SELECTION);
  const [recentKeys, setRecentKeys] = useState<string[]>(readRecent);
  const inputRef = useRef<HTMLInputElement>(null);
  // a pane picked here is where the keyboard goes: the focus is not given back to the opener
  const navigated = useRef(false);
  useLayoutEffect(() => { if (open) navigated.current = false; }, [open]);
  const surface = useFocusTrap<HTMLElement>(open, { initialFocus: inputRef, shouldRestore: () => !navigated.current });
  useSheetSwipe(surface, onClose);
  usePhoneSheetBack(() => { onClose(); }, open);
  const resultsRef = useRef<HTMLDivElement>(null);
  // a filter letter pressed over the list keeps the keyboard in the list: the first row takes the
  // focus once the filter is drawn, also when the letter named the filter already shown
  const [rowFocusRequest, setRowFocusRequest] = useState(0);
  // the row the focus is on, told from one the focus left, for a roster change that takes it out of the list
  const focusedRow = useRef<HTMLElement | null>(null);
  // the rows as the last roster listed them: where a row that left stood among the rows that stayed
  const previousRowKeys = useRef<readonly string[]>([]);

  // Terminal attachment can move focus after the palette opens. Escape belongs to
  // this modal even then, and must not leak through to the underlying terminal.
  useLayoutEffect(() => {
    if (!open) return;
    const dismiss = (event: KeyboardEvent): void => {
      if (event.key !== "Escape" || event.isComposing || event.keyCode === 229) return;
      event.preventDefault();
      event.stopPropagation();
      onClose();
    };
    window.addEventListener("keydown", dismiss, true);
    return () => window.removeEventListener("keydown", dismiss, true);
  }, [open, onClose]);

  // In the opening commit: a passive effect leaves a render of the last search's results until a later
  // task, where a quick Tab focuses a row at an index the full list then gives to another row.
  useLayoutEffect(() => {
    if (!open) return;
    setQuery("");
    setFilter("all");
    setSelection(NO_SELECTION);
    focusedRow.current = null;
    requestSettingsPage(null);
    setRecentKeys(readRecent());
  }, [open]);

  const { paneOrder } = useSidebarShared();
  const targets = useMemo(() => paletteTargets(machines, panes, threads, paneOrder), [machines, panes, threads, paneOrder]);
  // 목록에서 연 줄도 최근이다(참고: 고른 pane이 바뀌면 기억한다)
  const selectedTarget = selected === null ? undefined : targets.find((target) => target.keys.includes(selected));
  const selectedKey = selectedTarget?.key ?? null;
  useEffect(() => {
    if (selectedKey === null) return;
    setRecentKeys((current) => (current[0] === selectedKey ? current : storeRecent(rememberTarget(selectedKey, readRecent()))));
  }, [selectedKey]);

  const paletteActions = useMemo<PaletteAction[]>(() => [
    ...(actions.openNewSession ? [{ id: "new", label: "새 pane", icon: MessageSquarePlus, shortcut: "new-session", run: actions.openNewSession }] satisfies PaletteAction[] : []),
    ...(actions.toggleView ? [{ id: "view", label: actions.view === "chat" ? "터미널로 전환" : "채팅으로 전환", icon: SwitchCamera, shortcut: "toggle-view", run: actions.toggleView }] satisfies PaletteAction[] : []),
    ...(actions.openFind ? [{ id: "find", label: "대화에서 찾기", icon: TextSearch, run: actions.openFind }] satisfies PaletteAction[] : []),
    { id: "theme", label: "테마 전환", icon: SunMoon, run: actions.toggleTheme },
    { id: "settings", label: "설정", icon: Settings, shortcut: "settings", run: () => actions.openSettings() },
    // 참고 「Toggle sidebar」(i18n.ko.ts 「사이드바 토글」)
    { id: "sidebar", label: "사이드바 토글", icon: PanelLeft, shortcut: "toggle-sidebar", run: actions.toggleSidebar },
    { id: "reports", label: "보고서", icon: FileText, run: actions.openReports },
    { id: "tasks", label: "할 일 열기", icon: ListTodo, run: actions.openTasks },
    // aaa 머리 「더 보기 → 모든 thread」(끝난 thread까지)
    { id: "threads", label: "모든 thread", icon: ListChecks, run: actions.openThreads },
  ], [actions]);

  // Settings pages are found by a query only, and open on the page itself: the dialog reads the request as it mounts
  const settingsActions = useMemo(() => {
    const icons = new Map(PAGES.map(({ id, icon }) => [id, icon]));
    return SETTINGS_PAGE_ENTRIES.map((entry) => ({ action: { id: `settings-${entry.id}`, label: entry.label, icon: icons.get(entry.id) ?? Settings, run: () => { requestSettingsPage(entry.id); actions.openSettings(entry.id); } } satisfies PaletteAction, aliases: entry.aliases }));
  }, [actions]);

  const multiMachine = machines.length > 1;
  const counts = useMemo(() => statusCounts(targets), [targets]);
  const { actionsOnly, text } = parseQuery(query);

  // herdr's Goto picker: a row per pane under its workspace; `>` keeps the actions alone (T3 Code's
  // palette); a status filter is about panes, so it leaves the actions out; the rows went to last
  // lead an unsearched, unfiltered list
  const sections = useMemo<PaletteSectionView[]>(() => {
    const list: PaletteSectionView[] = [];
    if (!actionsOnly) {
      const ranked = rankTargets(text, targets, filter);
      if (text === "" && filter === "all") {
        const recent = recentTargets(ranked, recentKeys, selected, RECENT_SHOWN);
        if (recent.length > 0) list.push({ id: "recent", heading: "최근", rows: recent.map((target) => ({ kind: "pane", target })) });
      }
      for (const group of groupTargets(ranked)) {
        list.push({ id: group.id, heading: group.project ?? "pane", machineName: multiMachine || group.project === null ? group.machine.name : undefined, rows: group.targets.map((target) => ({ kind: "pane", target })) });
      }
    }
    if (actionsOnly || filter === "all") {
      // a name found only through an alias comes after the titles that hold the query
      const visible = rankByTitle(text, paletteActions.map((action) => ({ item: action, title: action.label, aliases: ACTION_ALIASES[action.id] })));
      if (visible.length > 0) list.push({ id: "actions", heading: "동작", rows: visible.map((action) => ({ kind: "action", action })) });
      const pages = text === "" ? [] : rankByTitle(text, settingsActions.map(({ action, aliases }) => ({ item: action, title: action.label, aliases })));
      if (pages.length > 0) list.push({ id: "settings-pages", heading: "설정", rows: pages.map((action) => ({ kind: "action", action })) });
    }
    return list;
  }, [actionsOnly, text, filter, targets, multiMachine, recentKeys, selected, paletteActions, settingsActions]);

  const rows = useMemo(() => sections.flatMap((section) => section.rows), [sections]);
  const rowKeys = useMemo(() => sections.flatMap((section) => section.rows.map((row) => rowKey(section.id, row))), [sections]);
  const sectionStarts = useMemo(() => {
    const starts: number[] = [];
    let index = 0;
    for (const section of sections) { starts.push(index); index += section.rows.length; }
    return starts;
  }, [sections]);
  const itemCount = rows.length;
  // the picked row where it stands now, else the row at the pick's place (the last one when the list shrank under it)
  const pickedAt = selection.key === null ? -1 : rowKeys.indexOf(selection.key);
  const activeIndex = pickedAt >= 0 ? pickedAt : Math.min(selection.index, Math.max(0, itemCount - 1));

  // Arrow navigation keeps focus in the search field: aria-activedescendant alone does not
  // scroll its option into view, including when an arrow wraps to the other end of the list.
  // Rows take hover from mousemove, not mouseenter: a scroll under a resting pointer sends the
  // row now under it a mouseenter, which would replace the option the arrow picked.
  useLayoutEffect(() => {
    if (!open) return;
    resultsRef.current?.querySelector<HTMLElement>('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
  }, [open, activeIndex, query, itemCount]);

  // The first row's focus event tells the pick only when the focus moves: a letter pressed on the
  // first row itself fires none. So the pick is told here.
  useLayoutEffect(() => {
    if (!open || rowFocusRequest === 0) return;
    const first = document.getElementById("palette-item-0");
    if (!first) {
      inputRef.current?.focus();
      return;
    }
    const key = rowKeys[0] ?? null;
    setSelection((current) => (current.key === key && current.index === 0 ? current : { key, index: 0 }));
    first.focus();
  }, [rowFocusRequest]); // eslint-disable-line react-hooks/exhaustive-deps

  // The roster changed under the pick. A picked row that stayed tells its place again; one that
  // left hands the pick to the row now at its place, counted over the rows before it that stayed.
  // A row that leaves the list while the focus is on it would leave the focus on the page body,
  // where no palette key reaches: the row at the pick's place takes it, the search when none is left.
  useLayoutEffect(() => {
    const before = previousRowKeys.current;
    previousRowKeys.current = rowKeys;
    if (!open) return;
    let index = activeIndex;
    if (selection.key !== null) {
      const wasAt = pickedAt < 0 ? before.indexOf(selection.key) : -1;
      if (wasAt >= 0) {
        const stayed = new Set(rowKeys);
        index = Math.min(before.slice(0, wasAt).filter((key) => stayed.has(key)).length, Math.max(0, itemCount - 1));
      }
      const key = rowKeys[index] ?? null;
      if (key !== selection.key || index !== selection.index) setSelection({ key, index });
    }
    const row = focusedRow.current;
    if (!row || row.isConnected) return;
    focusedRow.current = null;
    (document.getElementById(`palette-item-${index}`) ?? inputRef.current)?.focus();
  }, [rowKeys]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!open) return null;

  const select = (index: number): void => {
    const key = rowKeys[index] ?? null;
    setSelection((current) => (current.key === key && current.index === index ? current : { key, index }));
  };
  const runTarget = (target: PaletteTarget): void => {
    setRecentKeys((current) => storeRecent(rememberTarget(target.key, current)));
    navigated.current = true;
    onOpen(target.open);
    onClose();
  };
  const runAction = (action: PaletteAction): void => {
    onClose();
    action.run();
  };
  const activate = (index: number): void => {
    const row = rows[index];
    if (!row) return;
    if (row.kind === "pane") runTarget(row.target);
    else runAction(row.action);
  };
  const adjacentSectionStart = (index: number, direction: -1 | 1): number => {
    let current = 0;
    for (let section = 0; section < sectionStarts.length; section += 1) if (sectionStarts[section]! <= index) current = section;
    return sectionStarts[(current + direction + sectionStarts.length) % sectionStarts.length] ?? 0;
  };
  const applyFilter = (next: PaletteStatusFilter): void => {
    setFilter(next);
    setSelection(NO_SELECTION);
  };
  const onInputKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>): void => {
    // Candidate navigation and the committing Enter belong to the IME. WebKit can report
    // the latter after compositionend with isComposing false and key code 229.
    if (event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229) return;
    if (event.key === "ArrowDown" && itemCount > 0) {
      event.preventDefault();
      select((activeIndex + 1) % itemCount);
    } else if (event.key === "ArrowUp" && itemCount > 0) {
      event.preventDefault();
      select((activeIndex - 1 + itemCount) % itemCount);
    } else if ((event.key === "ArrowLeft" || event.key === "ArrowRight") && sectionStarts.length > 1) {
      // a caret inside the text keeps its own Left and Right; at the text's edge they walk the sections
      const input = event.currentTarget;
      const collapsed = input.selectionStart === input.selectionEnd;
      const atEdge = event.key === "ArrowLeft" ? input.selectionStart === 0 : input.selectionEnd === input.value.length;
      if (!collapsed || !atEdge) return;
      event.preventDefault();
      select(adjacentSectionStart(activeIndex, event.key === "ArrowLeft" ? -1 : 1));
    } else if (event.key === "Enter" && itemCount > 0) {
      event.preventDefault();
      activate(activeIndex);
    }
  };
  // herdr's picker keys, over the list and the chips but never in the search field: b, w, i, d and a
  // set the filter, `/` goes back to the search; arrows walk the rows and the sections by focus
  const onSurfaceKeyDown = (event: ReactKeyboardEvent<HTMLElement>): void => {
    const target = event.target as HTMLElement;
    if (event.defaultPrevented || target === inputRef.current || event.ctrlKey || event.metaKey || event.altKey) return;
    const filterKey = FILTER_KEYS[event.key];
    if (filterKey) {
      event.preventDefault();
      applyFilter(filterKey);
      setRowFocusRequest((count) => count + 1);
      return;
    }
    if (event.key === "/") {
      event.preventDefault();
      inputRef.current?.focus();
      return;
    }
    const row = target.closest<HTMLElement>("[role=\"option\"]");
    if (!row || itemCount === 0) return;
    const index = Number(row.id.slice("palette-item-".length));
    const next = event.key === "ArrowDown" ? (index + 1) % itemCount
      : event.key === "ArrowUp" ? (index - 1 + itemCount) % itemCount
      : event.key === "ArrowRight" ? adjacentSectionStart(index, 1)
      : event.key === "ArrowLeft" ? adjacentSectionStart(index, -1)
      : null;
    if (next === null) return;
    event.preventDefault();
    document.getElementById(`palette-item-${next}`)?.focus();
  };
  const onChipsKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>): void => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    const at = STATUS_FILTERS.indexOf(filter);
    const next = STATUS_FILTERS[(at + (event.key === "ArrowRight" ? 1 : -1) + STATUS_FILTERS.length) % STATUS_FILTERS.length]!;
    applyFilter(next);
    event.currentTarget.querySelector<HTMLElement>(`[data-status="${next}"]`)?.focus();
  };
  const onChipClick = (event: ReactMouseEvent<HTMLButtonElement>, next: PaletteStatusFilter): void => {
    applyFilter(next);
    // a mouse goes back to typing; a finger would open the keyboard
    if (event.detail > 0 && !touch) inputRef.current?.focus();
  };

  const activeRow = rows[activeIndex];
  const footerPlace = activeRow?.kind === "pane"
    ? [multiMachine || activeRow.target.project === null ? activeRow.target.machine.name : null, activeRow.target.project].filter(Boolean).join(" › ")
    : activeRow?.kind === "action" ? activeRow.action.label : "";
  const footerPath = activeRow?.kind === "pane" ? activeRow.target.path : "";

  const onSurfaceFocus = (event: ReactFocusEvent<HTMLElement>): void => {
    focusedRow.current = (event.target as HTMLElement).closest<HTMLElement>("[role=\"option\"]");
  };

  let rowIndex = 0;
  return (
    <div className="modal-scrim palette-scrim" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <section ref={surface} className="menu command-palette" role="dialog" aria-modal="true" aria-label="명령 팔레트" data-testid="command-palette" onKeyDown={onSurfaceKeyDown} onFocus={onSurfaceFocus}>
        <div className="palette-search">
          <input ref={inputRef} className="input" type="search" value={query} placeholder="패널과 동작 검색…" aria-label="패널과 동작 검색" aria-controls="palette-results" aria-activedescendant={itemCount ? `palette-item-${activeIndex}` : undefined} onKeyDown={onInputKeyDown} onChange={(event) => { setQuery(event.target.value); setSelection(NO_SELECTION); }} />
          <button type="button" className="icon-button" aria-label="명령 팔레트 닫기" onClick={onClose}><X /></button>
        </div>
        {!actionsOnly && (
          <div className="palette-filters" role="radiogroup" aria-label="상태별 패널 필터" onKeyDown={onChipsKeyDown}>
            {STATUS_FILTERS.map((status) => (
              <button key={status} type="button" role="radio" className="palette-filter" data-status={status} aria-checked={filter === status} tabIndex={filter === status ? 0 : -1} data-empty={counts[status] === 0 || undefined} onClick={(event) => onChipClick(event, status)}>
                <span className="palette-filter-word">{status === "all" ? "전체" : STATUS_KO[status]}</span>
                <span className="palette-filter-count">{counts[status]}</span>
              </button>
            ))}
          </div>
        )}
        <div ref={resultsRef} className="palette-results" id="palette-results" role="listbox">
          {sections.map((section) => (
            <div key={section.id} className="palette-section" data-section={section.id}>
              <div className="menu-heading palette-section-heading">
                {section.machineName && <span className="palette-section-machine">{section.machineName}</span>}
                <span className="palette-section-name">{section.heading}</span>
                <span className="palette-section-count">{section.rows.length}</span>
              </div>
              {section.rows.map((row) => {
                const index = rowIndex;
                rowIndex += 1;
                if (row.kind === "action") {
                  const Icon = row.action.icon;
                  return <button key={row.action.id} id={`palette-item-${index}`} type="button" role="option" className="menu-item" aria-selected={activeIndex === index} onFocus={() => select(index)} onMouseMove={() => select(index)} onClick={() => runAction(row.action)}><Icon /><span className="menu-item-main">{row.action.label}</span><ShortcutHint shortcutId={row.action.shortcut} /></button>;
                }
                const { target } = row;
                const place = section.id === "recent" ? [multiMachine ? target.machine.name : null, target.project, target.place].filter(Boolean).join(" › ") : target.place;
                const isSelected = selected !== null && target.keys.includes(selected);
                return (
                  <button key={target.key} data-target={target.key} id={`palette-item-${index}`} type="button" role="option" className="menu-item palette-pane" aria-selected={activeIndex === index} aria-current={isSelected ? "true" : undefined} onFocus={() => select(index)} onMouseMove={() => select(index)} onClick={() => runTarget(target)}>
                    <span className="palette-mark"><AgentMark agent={agentOf(target.pane, target.thread) ?? "shell"} /></span>
                    <span className="menu-item-main"><span className="palette-row-title">{target.title}{isSelected && <span className="palette-selected">선택됨</span>}</span>{place && <span className="palette-row-subtitle">{place}</span>}</span>
                    <StatusBadge status={target.status === "unknown" ? undefined : target.status} />
                  </button>
                );
              })}
            </div>
          ))}
          {itemCount === 0 && <p className="palette-empty" role="status">일치하는 패널이나 동작이 없습니다</p>}
        </div>
        <footer className="palette-footer">
          <div className="palette-footer-where" aria-live="polite">
            {footerPlace && <span className="palette-footer-place">{footerPlace}</span>}
            {footerPath && <span className="palette-footer-path" title={footerPath}>{footerPath}</span>}
          </div>
          <div className="palette-footer-keys" aria-hidden="true">
            <span className="palette-footer-key"><kbd className="kbd">←</kbd><kbd className="kbd">→</kbd>묶음</span>
            <span className="palette-footer-key"><kbd className="kbd">&gt;</kbd>동작</span>
          </div>
        </footer>
      </section>
    </div>
  );
}

/** 칩 말: 참고 STATUS_WORD(READY·RUN·INPUT·DONE·BG)의 i18n.ko.ts 번역 */
const STATUS_KO: Record<Exclude<PaletteStatusFilter, "all">, string> = {
  idle: "대기",
  working: "실행",
  blocked: "입력",
  done: "완료",
  waiting: "백그라운드",
};
