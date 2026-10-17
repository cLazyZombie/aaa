// host 웹 API(crates/aaa-host/src/web.rs). 상태는 host가 갖고, 여기서는 받은 것을 보여 주기만 한다.
import type {
  DeviceRole,
  ErrorBody,
  EventsClientMsg,
  FrontEvent,
  FrontRequest,
  FrontResponse,
  Outcome,
  QuestionView,
  ThreadView,
} from "./gen/proto";
import { watchPageInUse } from "./herdr/presence";

export class ApiError extends Error {
  constructor(public body: ErrorBody) {
    super(body.message);
  }
}

async function post<T>(path: string, body: unknown): Promise<T> {
  const res = await fetch(path, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
    credentials: "same-origin",
  });
  return unwrap<T>(res);
}

async function unwrap<T>(res: Response): Promise<T> {
  let out: Outcome<T>;
  try {
    out = (await res.json()) as Outcome<T>;
  } catch {
    throw new ApiError({ code: "internal", message: `HTTP ${res.status}` });
  }
  if ("err" in out) throw new ApiError(out.err);
  return out.ok;
}

export type Me = { role: DeviceRole; device: string };

export async function me(): Promise<Me | null> {
  const res = await fetch("/api/v1/me", { credentials: "same-origin" });
  if (res.status === 401) return null;
  return unwrap<Me>(res);
}

export const pair = (code: string) => post<{ role: DeviceRole }>("/api/v1/pair", { code });
export const call = (req: FrontRequest) => post<FrontResponse>("/api/v1/call", req);

/** Web Push(§8.26) */
export type PushInfo = { enabled: boolean; key: string | null; subscribed: number; in_use: number; kinds: string[] };
export async function pushInfo(): Promise<PushInfo> {
  return unwrap<PushInfo>(await fetch("/api/v1/push", { credentials: "same-origin" }));
}
export const pushSubscribe = (sub: PushSubscriptionJSON) => post<{ subscribed: boolean }>("/api/v1/push/subscribe", sub);
export const pushUnsubscribe = (endpoint: string) => post<{ removed: boolean }>("/api/v1/push/unsubscribe", { endpoint });
/** 시험 알림: `endpoint`를 주면 그 구독 하나에만. 없으면 `subscription_not_found`, push 서비스가 버렸으면 `subscription_gone` 오류다 */
export const pushTest = (endpoint?: string) =>
  post<{ sent: number; failed: string[] }>("/api/v1/push/test", endpoint ? { endpoint } : {});

export function wsUrl(path: string): string {
  const proto = location.protocol === "https:" ? "wss:" : "ws:";
  return `${proto}//${location.host}${path}`;
}

/** 상태 이벤트 WS. 끊기면 다시 붙고, 붙어 있는지 알린다. */
export type StateEvent = Extract<FrontEvent, { ev: "state" }>;
export type QuestionEvent = Extract<FrontEvent, { ev: "question" }>;

/** 기계·pane 목록과 확인창 질문·thread 사건만 받는다(layout·grid 사건은 TUI용이라 무시한다). */
export function watchEvents(
  onState: (ev: StateEvent) => void,
  onQuestion: (q: QuestionView) => void,
  onLink: (up: boolean) => void,
  onThread: (t: ThreadView) => void,
): () => void {
  let ws: WebSocket | null = null;
  let stopped = false;
  let delay = 500;
  let timer: number | undefined;
  // 이 화면을 지금 쓰는지(herdr-web-ui presence, ws.ts): 바뀔 때와 다시 붙을 때 알린다. 쓰는 화면이 있으면 host는 Web Push를 보내지 않는다
  let active: boolean | null = null;
  const tell = () => {
    if (active !== null && ws?.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ t: "presence", active } satisfies EventsClientMsg));
  };
  const stopPresence = watchPageInUse((now) => {
    if (now === active) return;
    active = now;
    tell();
  });
  const open = () => {
    ws = new WebSocket(wsUrl("/api/v1/events"));
    ws.onopen = () => {
      delay = 500;
      tell();
      onLink(true);
    };
    ws.onmessage = (m) => {
      const ev = JSON.parse(m.data as string) as FrontEvent;
      if (ev.ev === "state") onState(ev);
      else if (ev.ev === "question") {
        const { ev: _tag, ...q } = ev;
        onQuestion(q as QuestionView);
      } else if (ev.ev === "thread") {
        const { ev: _tag, ...t } = ev;
        onThread(t as ThreadView);
      }
    };
    ws.onclose = () => {
      onLink(false);
      if (!stopped) timer = window.setTimeout(open, delay);
      delay = Math.min(delay * 2, 10_000);
    };
  };
  open();
  return () => {
    stopped = true;
    stopPresence();
    window.clearTimeout(timer);
    ws?.close();
  };
}

export function b64decode(s: string): Uint8Array {
  const bin = atob(s);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

export function b64encode(b: Uint8Array): string {
  let bin = "";
  for (let i = 0; i < b.length; i += 0x8000) bin += String.fromCharCode(...b.subarray(i, i + 0x8000));
  return btoa(bin);
}
