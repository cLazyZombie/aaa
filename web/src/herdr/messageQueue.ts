/** Held messages are explicitly sent, never dispatched by reconnects or status changes.
 * aaa의 예외(plan §8.309): 대화 정리 중에 보낸 글(`after: "compaction"`)은 정리가 끝나면 입력창이 차례대로 보낸다. 보내다 실패하면
 * 표지를 떼어 보통 붙잡은 글이 된다(다시 저절로 보내지 않는다). 보관함은 같은 브라우저의 탭이 함께 보므로 글을 넣은 탭(`by`)만
 * 보낸다. 그 pane 입력창을 연 탭이 그 탭이 아니면(닫힘·새로고침·다른 pane으로 감) 남은 탭이 넘겨받는다(`compactionStep`). */
export interface HeldMessage { id: string; text: string; after?: "compaction"; by?: string }
type QueueStorage = Pick<Storage, "getItem" | "setItem" | "removeItem">;
const newId = () => globalThis.crypto?.randomUUID?.() ?? `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
/** 이 탭(페이지를 열 때마다 새로). 정리 뒤 보낼 글의 주인이다 */
export const TAB_ID = newId();

/** 정리가 끝난 뒤 입력창이 할 일. `sending`: 이 탭이 보내는 중인 글, `hold`: 작업 중이고 「대기 메시지 자동 전송」을 껐다 */
export type CompactionStep = { send: HeldMessage } | { adopt: string | undefined } | "release" | null;
export function compactionStep(queue: readonly HeldMessage[], sending: (id: string) => boolean, me: string, hold: boolean): CompactionStep {
  const i = queue.findIndex((m) => m.after === "compaction");
  if (i < 0) return null;
  const first = queue[i]!;
  if (sending(first.id)) return null;
  // 앞에 보통 붙잡은 글이 있으면(앞 정리 뒤 보내다 실패했거나 사람이 붙잡았다) 앞질러 가지 않는다: 모두 사람이 보낸다
  if (hold || i > 0) return "release";
  return first.by === me ? { send: first } : { adopt: first.by };
}

/** Target-scoped cache also lets an ACK remove its own item after the user switches panes. */
export class MessageQueueStore {
  private saved = new Map<string, string | null>();
  private unsaved = new Set<string>();
  isUnsaved(owner: string): boolean { return this.unsaved.has(owner); }
  private queues = new Map<string, HeldMessage[]>();
  private listeners = new Set<() => void>();
  private pending = new Set<string>();
  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => { this.listeners.delete(listener); };
  };
  isSending(id: string): boolean { return this.pending.has(id); }
  beginSend(owner: string, id: string): boolean {
    if (this.pending.has(id)) return false;
    this.pending.add(id);
    this.queues.set(owner, [...this.read(owner)]);
    for (const listener of this.listeners) listener();
    return true;
  }
  endSend(owner: string, id: string): void {
    this.pending.delete(id);
    this.queues.set(owner, [...this.read(owner)]);
    for (const listener of this.listeners) listener();
  }
  private storage: () => QueueStorage;
  constructor(storage: () => QueueStorage = () => window.localStorage) { this.storage = storage; }

  /** Reconcile storage before mutation; another tab may have written since our last render. */
  refresh(owner: string): void {
    if (this.unsaved.has(owner)) return;
    try {
      const raw = this.storage().getItem(`herdr-web-ui:queue:${owner}`);
      if (this.saved.has(owner) && raw === this.saved.get(owner)) return;
      this.queues.delete(owner);
      this.read(owner);
      for (const listener of this.listeners) listener();
    } catch { /* keep in-memory messages when storage cannot be read */ }
  }

  read(owner: string): HeldMessage[] {
    const cached = this.queues.get(owner);
    if (cached) return cached;
    let raw: string | null = null;
    try { raw = this.storage().getItem(`herdr-web-ui:queue:${owner}`); } catch { /* private mode */ }
    let messages: HeldMessage[] = raw ? [{ id: newId(), text: raw }] : [];
    try {
      const data = JSON.parse(raw ?? "null");
      if (data?.version === 1 && Array.isArray(data.messages)) {
        const ids = new Set<string>();
        messages = data.messages.filter((item: unknown): item is HeldMessage => {
          if (!item || typeof item !== "object") return false;
          const value = item as HeldMessage;
          if (typeof value.id !== "string" || typeof value.text !== "string" || ids.has(value.id)) return false;
          if (value.after !== undefined && value.after !== "compaction") delete value.after;
          if (value.after === undefined || typeof value.by !== "string") delete value.by;
          ids.add(value.id); return true;
        });
      }
    } catch { /* previous versions stored a single plain-text message */ }
    this.saved.set(owner, raw);
    this.queues.set(owner, messages);
    return messages;
  }

  private write(owner: string, messages: HeldMessage[]): void {
    this.queues.set(owner, messages);
    try {
      const key = `herdr-web-ui:queue:${owner}`;
      const raw = messages.length ? JSON.stringify({ version: 1, messages }) : null;
      if (raw !== null) this.storage().setItem(key, raw);
      else this.storage().removeItem(key);
      this.saved.set(owner, raw);
      this.unsaved.delete(owner);
    } catch { this.unsaved.add(owner); }
    for (const listener of this.listeners) listener();
  }

  add(owner: string, text: string, after?: HeldMessage["after"]): void {
    this.refresh(owner);
    this.write(owner, [...this.read(owner), { id: newId(), text, ...(after ? { after, by: TAB_ID } : {}) }]);
  }
  /** 저절로 보낼 표지를 모두 뗀다: 이제 사람이 보낸다 */
  release(owner: string): void {
    this.refresh(owner);
    this.write(owner, this.read(owner).map((message) => {
      if (message.after === undefined) return message;
      const { after: _after, by: _by, ...held } = message;
      return held;
    }));
  }
  /** `from` 탭이 넣은 정리 뒤 보낼 글을 이 탭(`me`)이 넘겨받는다 */
  adopt(owner: string, from: string | undefined, me = TAB_ID): void {
    this.refresh(owner);
    this.write(owner, this.read(owner).map((message) => message.after !== undefined && message.by === from ? { ...message, by: me } : message));
  }
  edit(owner: string, id: string, text: string): void {
    this.refresh(owner);
    this.write(owner, this.read(owner).map((message) => message.id === id ? { ...message, text } : message));
  }
  remove(owner: string, id: string): void {
    this.refresh(owner);
    this.write(owner, this.read(owner).filter((message) => message.id !== id));
  }
}

// Machine switches remount the terminal; outstanding sends must share the same owner cache.
export const messageQueues = new MessageQueueStore();

if (typeof window !== "undefined") {
  window.addEventListener("storage", (event) => {
    const prefix = "herdr-web-ui:queue:";
    if (event.key?.startsWith(prefix)) messageQueues.refresh(event.key.slice(prefix.length));
  });
}

/** 이 탭에서 그 pane 입력창이 열려 있다(정리 뒤 보낼 글을 이 탭이 보낼 수 있다). 다른 탭의 물음에 답한다 */
const composers = new Map<string, number>();
export function openComposer(owner: string): () => void {
  channel();
  composers.set(owner, (composers.get(owner) ?? 0) + 1);
  return () => {
    const n = (composers.get(owner) ?? 1) - 1;
    if (n > 0) composers.set(owner, n);
    else composers.delete(owner);
  };
}
type TabMessage = { ask: string; owner: string; asker: string } | { alive: string; owner: string; asker: string };
let tabs: BroadcastChannel | null | undefined;
const replies = new Set<(m: TabMessage) => void>();
/** 탭 사이 물음(참고 alertTurns처럼 BroadcastChannel: LAN HTTP에서도 된다). 처음 쓸 때 연다 */
function channel(): BroadcastChannel | null {
  if (tabs !== undefined) return tabs;
  tabs = typeof BroadcastChannel === "undefined" ? null : new BroadcastChannel("aaa:held-composers");
  if (tabs) tabs.onmessage = (event: MessageEvent<TabMessage>) => {
    const m = event.data;
    if (!m || typeof m !== "object" || typeof m.owner !== "string") return;
    if ("ask" in m && m.ask === TAB_ID && composers.has(m.owner)) tabs?.postMessage({ alive: TAB_ID, owner: m.owner, asker: m.asker });
    else if ("alive" in m && m.asker === TAB_ID) for (const reply of replies) reply(m);
  };
  return tabs;
}
/** `tab`이 그 pane 입력창을 열어 두었는가(`wait` ms 안에 답이 오면). 탭 사이 통신이 없으면(옛 브라우저) 없다고 본다 */
export function composerOpenIn(tab: string | undefined, owner: string, wait = 500): Promise<boolean> {
  const bus = channel();
  if (!bus || tab === undefined) return Promise.resolve(false);
  return new Promise((resolve) => {
    const reply = (m: TabMessage) => {
      if ("alive" in m && m.alive === tab && m.owner === owner) done(true);
    };
    const done = (alive: boolean) => {
      clearTimeout(timer);
      replies.delete(reply);
      resolve(alive);
    };
    const timer = setTimeout(() => done(false), wait);
    replies.add(reply);
    bus.postMessage({ ask: tab, owner, asker: TAB_ID });
  });
}
