/**
 * 설정 본문의 브라우저 기록. 휴대폰 목록 → 쪽 → 키 바 편집은 각각 한 칸이며,
 * 넓은 본문의 쪽 전환은 현재 칸을 바꾼다. X·Esc·←가 거두는 칸과 브라우저의
 * 뒤로/앞으로를 구분한다. query는 공유할 설정 쪽, hash는 돌아갈 본문을 가리킨다.
 * 주소가 있는 설정은 새로고침에도 유지하고 옛 모달의 기록은 벗어난다.
 */

const KEY = "aaa:settings";
/** names an opening of Settings on the entry it began from and on each of its entries */
const BASE_KEY = "aaa:settings-base";
/** names the opening an entry another dialog pushed from a Settings entry lies above */
const ABOVE_KEY = "aaa:settings-above";
/** how long to wait before releasing the traversal's busy state, not its ownership */
const LANDING_MS = 1000;

/** One step into Settings: the page shown (null: a phone's list of pages) and the key bar editor over it. */
export interface SettingsLevel {
  page: string | null;
  keyBar: boolean;
}

/** 공유할 설정 주소. hash에는 돌아갈 pane·보고서 주소를 그대로 둔다. */
export function settingsFromUrl(): SettingsLevel | null {
  if (typeof window === "undefined") return null;
  const query = new URLSearchParams(window.location.search);
  const page = query.get("settings");
  if (page === null) return null;
  const valid = ["appearance", "chat", "terminal", "alerts", "usage", "accounts", "safety", "profiles", "shortcuts", "devices", "machines", "deploy", "project", "repos"];
  if (page !== "" && !valid.includes(page)) return null;
  return { page: page || null, keyBar: page === "terminal" && query.get("settings-keybar") === "1" };
}

export interface SettingsEntry extends SettingsLevel {
  /** how many entries of Settings the history holds up to this one */
  depth: number;
}

/**
 * What the dialog shows, as the steps that led there. A phone lists the pages first, so its
 * list is a step of its own; a wider dialog opens on a page, and turning to another replaces it.
 */
export function settingsLevels(narrow: boolean, page: string | null, keyBar: boolean): SettingsLevel[] {
  const levels: SettingsLevel[] = narrow ? [{ page: null, keyBar: false }] : [];
  if (page !== null) levels.push({ page, keyBar: false });
  if (page !== null && keyBar) levels.push({ page, keyBar: true });
  return levels;
}

/** The Settings entry a history state holds, or null: state is whatever another script left there. */
export function settingsEntry(state: unknown): SettingsEntry | null {
  if (state === null || typeof state !== "object") return null;
  const entry = (state as Record<string, unknown>)[KEY];
  if (entry === null || typeof entry !== "object") return null;
  const { page, keyBar, depth } = entry as Record<string, unknown>;
  if ((page !== null && typeof page !== "string") || typeof keyBar !== "boolean") return null;
  if (typeof depth !== "number" || !Number.isInteger(depth) || depth < 1 || depth > 3) return null;
  return { page, keyBar, depth };
}

const fieldOf = (state: unknown, key: string): unknown =>
  state !== null && typeof state === "object" ? (state as Record<string, unknown>)[key] : undefined;

/**
 * The state for an entry another dialog (a file preview) pushes from this one. It is no Settings
 * entry, so it does not copy one: it notes the opening it lies above instead, which a reload on
 * it closes without stepping back over it.
 */
export function outsideSettings(state: unknown): Record<string, unknown> {
  const { [KEY]: _entry, [BASE_KEY]: opening, ...rest } = state !== null && typeof state === "object" ? state as Record<string, unknown> : {};
  return settingsEntry(state) !== null && typeof opening === "string" ? { ...rest, [ABOVE_KEY]: opening } : rest;
}

type Listener = (entry: SettingsEntry | null, own: boolean) => void;
const listeners = new Set<Listener>();
// 이 모듈의 popstate 처리가 App의 듣기보다 먼저 돈다. 같은 주소의 다른 사용자 이동까지
// 내부 복귀로 넘기지 않도록 상태·시한 대신 해당 사건만 표시한다.
const internalRewinds = new WeakSet<PopStateEvent>();
let wanted: readonly SettingsLevel[] = [];
/** the steps this module knows the history holds for Settings, by depth; null for one it did not push and cannot see */
let held: Array<SettingsLevel | null> = [];
const same = (a: SettingsLevel | null, b: SettingsLevel): boolean => a !== null && a.page === b.page && a.keyBar === b.keyBar;
/** traversals asked for here that have not landed yet: nothing is pushed while one is under way */
let rewinding = 0;
let landing: ReturnType<typeof setTimeout> | undefined;
let pending: { from: unknown; depth: number; level: SettingsLevel | null; expired: boolean; reopened: boolean; claim: boolean } | null = null;
let observed: unknown = typeof window === "undefined" ? undefined : window.history.state;
let writing = false;
let loadedOnSettings = false;
/** the opening of Settings the entries held belong to, as written into its base entry */
let opening: unknown;
/** the openings whose entries lie under an entry that is not their base: Back onto them steps over them, each until it has been */
const buried = new Set<unknown>();

function releasePending(): void { pending = null; rewinding = 0; clearTimeout(landing); }

function writeState(method: "pushState" | "replaceState", state: unknown): void {
  writing = true;
  try {
    const url = new URL(window.location.href);
    const entry = settingsEntry(state);
    if (entry) url.searchParams.set("settings", entry.page ?? "");
    else url.searchParams.delete("settings");
    if (entry?.keyBar) url.searchParams.set("settings-keybar", "1");
    else url.searchParams.delete("settings-keybar");
    window.history[method](state, "", url);
  }
  finally { writing = false; observed = window.history.state; }
}

function rewind(by: number): void {
  if (pending !== null) return;
  const depth = (settingsEntry(window.history.state)?.depth ?? 0) - by;
  const request = { from: window.history.state, depth, level: held[depth - 1] ?? null, expired: false, reopened: false, claim: true };
  pending = request;
  rewinding = 1;
  clearTimeout(landing);
  landing = setTimeout(() => {
    if (pending !== request) return;
    rewinding = 0;
    request.expired = true;
    if (request.reopened) request.claim = false;
  }, LANDING_MS);
  window.history.go(-by);
}

function reconcile(): void {
  const state: unknown = window.history.state;
  if (pending !== null && state !== observed) releasePending();
  observed = state;
  if (rewinding > 0) return;
  const current = settingsEntry(state);
  const have = current?.depth ?? 0;
  // entries held under a state that is not the one they began from: a markerless entry (a typed
  // #hash, another script's pushState, a preview) landed above them, and they are buried there
  if (have === 0 && held.length > 0 && fieldOf(state, BASE_KEY) !== opening) buried.add(opening);
  if (have > 0) opening = fieldOf(state, BASE_KEY);
  // what the history holds up to here: a Back landed under what was recorded; the step shown is
  // read from the state; one this module did not push (a reload or a Forward landed on entries
  // of an earlier opening) is unknown, never taken on trust, and rebuilt like a differing one
  held = held.slice(0, have);
  while (held.length < have) held.push(null);
  if (have > 0 && current) held[have - 1] = { page: current.page, keyBar: current.keyBar };
  if (have > wanted.length) { rewind(have - wanted.length); return; }
  // the first step that differs from what is wanted. An earlier one (the window changed width:
  // a phone's list belongs under a page a wider dialog had opened directly, or no longer does)
  // is stepped back to and the steps above it are made anew
  let differs = 0;
  while (differs < have && differs < wanted.length && same(held[differs]!, wanted[differs]!)) differs++;
  if (differs < have - 1) { rewind(have - differs - 1); return; }
  let base = state !== null && typeof state === "object" ? state : {};
  if (have === 0 && wanted.length > 0) {
    // a new opening: its base entry is marked, so a Back onto it is told from a foreign entry
    opening = Math.random().toString(36).slice(2);
    writeState("replaceState", { ...base, [BASE_KEY]: opening });
    base = window.history.state as object;
  }
  if (have > 0 && differs === have - 1) {
    // the step shown is another: a wider dialog turned its page, or the list took a page's place
    writeState("replaceState", { ...base, [KEY]: { ...wanted[have - 1], depth: have } });
    held[have - 1] = wanted[have - 1]!;
  }
  for (let depth = have + 1; depth <= wanted.length; depth++) {
    writeState("pushState", { ...base, [KEY]: { ...wanted[depth - 1], depth } });
    held.push(wanted[depth - 1]!);
  }
}

/** Makes the history hold these steps into Settings, in order; none when the dialog is closed. */
export function recordSettings(levels: readonly SettingsLevel[]): void {
  const reopening = wanted.length === 0 && levels.length > 0;
  wanted = levels;
  if (reopening && pending !== null) { pending.reopened = true; if (pending.expired) pending.claim = false; }
  if (typeof window !== "undefined") reconcile();
}

/**
 * Whether this load began on an entry Settings left, which it steps out of, or on one above them
 * (a preview opened over Settings), which it keeps with Settings closed. Asked rather than
 * read from the history by whoever opens Settings from the address: the step lands when the
 * browser gets to it, before or after the app first draws, and either address may name Settings.
 */
export function loadedOnSettingsEntry(): boolean { return loadedOnSettings; }

/**
 * Tells when the history moved: the Settings entry it landed on (null: under the dialog), and
 * whether the move was this module's own stepping back, which the dialog already shows.
 */
export function onSettingsHistory(listener: Listener): () => void {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

/** 설정 닫기·쪽 변경이 요청한 되감기의 도착인가. 사용자의 Back/Forward와 구분한다. */
export function isSettingsRewind(event: PopStateEvent): boolean {
  return internalRewinds.has(event);
}

if (typeof window !== "undefined") {
  for (const method of ["pushState", "replaceState"] as const) {
    const history = window.history;
    const write = history[method].bind(history);
    history[method] = ((...args: Parameters<History["pushState"]>) => {
      write(...args);
      observed = history.state;
      if (!writing) releasePending();
    }) as History["pushState"];
  }
  window.addEventListener("popstate", (event) => {
    const previous = observed;
    observed = event.state;
    const entry = settingsEntry(event.state);
    // Back from above onto the entries of a buried opening: Settings closed when they were buried,
    // so they are stepped over to the entry they began from instead of opening it again
    if (pending === null && entry !== null && buried.has(fieldOf(event.state, BASE_KEY)) && fieldOf(previous, BASE_KEY) !== fieldOf(event.state, BASE_KEY)) {
      buried.delete(fieldOf(event.state, BASE_KEY));
      rewind(entry.depth);
      return;
    }
    const matches = pending !== null && (entry?.depth ?? 0) === pending.depth
      && (pending.level === null || (entry !== null && same(pending.level, entry)));
    const own = matches && pending?.claim === true;
    if (own) internalRewinds.add(event);
    if (pending !== null && (matches || event.state !== pending.from)) releasePending();
    for (const listener of [...listeners]) listener(entry, own);
    // what was asked for while the traversal was under way is done now
    if (own) reconcile();
  });
  // 새 주소의 설정은 복원하고, 주소 없이 남은 옛 모달 기록만 벗어난다.
  const left = settingsEntry(window.history.state);
  if (left !== null && settingsFromUrl() !== null) {
    // 주소가 있는 설정은 새로고침 뒤에도 유지한다. 목록까지의 기존 칸을 다시 만들지 않는다.
    held = left.depth === 3 ? [{ page: null, keyBar: false }, { page: left.page, keyBar: false }, left]
      : left.depth === 2 ? [left.keyBar ? { page: left.page, keyBar: false } : { page: null, keyBar: false }, left] : [left];
    opening = fieldOf(window.history.state, BASE_KEY);
  }
  else if (left !== null) { loadedOnSettings = true; rewind(left.depth); }
  else if (typeof fieldOf(window.history.state, ABOVE_KEY) === "string") {
    // an entry above Settings (a preview opened over it) stays, and Settings stays closed: Back
    // from it steps over the entries beneath
    loadedOnSettings = true;
    buried.add(fieldOf(window.history.state, ABOVE_KEY));
  }
}
