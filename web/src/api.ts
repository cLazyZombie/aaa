// host 웹 API(crates/aa-host/src/web.rs). 상태는 host가 갖고, 여기서는 받은 것을 보여 주기만 한다.
import type { DeviceRole, ErrorBody, FrontEvent, FrontRequest, FrontResponse, Outcome } from "./gen/proto";

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

export function wsUrl(path: string): string {
  const proto = location.protocol === "https:" ? "wss:" : "ws:";
  return `${proto}//${location.host}${path}`;
}

/** 상태 이벤트 WS. 끊기면 다시 붙고, 붙어 있는지 알린다. */
export type StateEvent = Extract<FrontEvent, { ev: "state" }>;

/** 기계·pane 목록 사건만 받는다(layout·grid 사건은 TUI용이라 무시한다). */
export function watchState(onState: (ev: StateEvent) => void, onLink: (up: boolean) => void): () => void {
  let ws: WebSocket | null = null;
  let stopped = false;
  let delay = 500;
  let timer: number | undefined;
  const open = () => {
    ws = new WebSocket(wsUrl("/api/v1/events"));
    ws.onopen = () => {
      delay = 500;
      onLink(true);
    };
    ws.onmessage = (m) => {
      const ev = JSON.parse(m.data as string) as FrontEvent;
      if (ev.ev === "state") onState(ev);
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
