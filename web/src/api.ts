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
  SharedPreferences,
  ThreadView,
} from "./gen/proto";
import type { PreferencesChange } from "./herdr/preferences.ts";
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

/** 이 host의 웹 화면들이 함께 쓰는 설정과 고정(plan §8.73, herdr-web-ui api.ts `fetchPreferences`·`savePreferences`) */
export async function loadPreferences(): Promise<SharedPreferences> {
  const r = await call({ op: "preferences" });
  if (r.kind !== "preferences") throw new ApiError({ code: "internal", message: `뜻밖의 답 ${r.kind}` });
  const { kind: _kind, ...doc } = r;
  return doc;
}
export async function savePreferences(change: PreferencesChange): Promise<SharedPreferences> {
  const r = await call({ op: "preferences_set", settings: change.settings, pins: change.pins ?? null });
  if (r.kind !== "preferences") throw new ApiError({ code: "internal", message: `뜻밖의 답 ${r.kind}` });
  const { kind: _kind, ...doc } = r;
  return doc;
}

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

/**
 * 살아 있는지(herdr-web-ui src/lib/ws.ts 「Liveness」를 상태 사건 연결에 옮겼다): 휴대폰이 잠들었다 깨거나 망이 바뀌면
 * 소켓이 OPEN인 채 몇 분씩 아무것도 오지 않는다. 그동안 이 화면은 질문·보고 사건(앱 안 알림)을 받지 못하고, host는 이
 * 연결의 「쓰는 화면」 때문에 push도 보내지 않는다(10-18 iPhone 실기). 페이지가 다시 보이거나(visibilitychange·pageshow)
 * 망이 바뀌거나(online), 보이는 동안 IDLE_PROBE_MS 넘게 아무것도 오지 않으면 ping을 보낸다. 그 ping의 pong만 답이다.
 * PROBE_TIMEOUT_MS 안에 없으면 한 번 더, 그것도 없으면 끊고 바로 다시 붙는다. 끊을 때마다 다음 기한은 두 배(최대
 * PROBE_TIMEOUT_MAX_MS)다. 기한이 PROBE_OVERDUE_MS 넘게 늦게 돌면 페이지가 얼어 있던 것이라 처음부터 다시 묻는다.
 */
const PROBE_TIMEOUT_MS = 3_000;
const PROBE_TIMEOUT_MAX_MS = 12_000;
const PROBE_OVERDUE_MS = 2_000;
const IDLE_PROBE_MS = 20_000;
const IDLE_CHECK_MS = 5_000;
/**
 * 이만큼 넘게 가려졌다가 다시 보이면 묻지 않고 바로 새로 붙는다. 휴대폰 OS는 멈춘 앱의 소켓을 close 사건 없이 죽이는
 * 일이 잦아, 묻고 기다리는(3초+3초) 동안 화면이 사건 없이 멈춰 있게 된다. t3code(MIT)
 * apps/mobile/src/connection/app-state-wakeups.ts·packages/client-runtime/src/connection/supervisor.ts 522~526줄
 * 「application-active-reconnect」(10초)를 따랐다. herdr-web-ui ws.ts는 이때도 probe한다(plan §8.82).
 */
const RECONNECT_AFTER_HIDDEN_MS = 10_000;

/** 기계·pane 목록과 확인창 질문·thread·공유 설정 사건만 받는다(layout·grid 사건은 TUI용이라 무시한다). */
export function watchEvents(
  onState: (ev: StateEvent) => void,
  onQuestion: (q: QuestionView) => void,
  onLink: (up: boolean) => void,
  onThread: (t: ThreadView) => void,
  onPreferences: (p: SharedPreferences) => void,
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
  let heard = Date.now();
  let nextPing = 1;
  const probeIds = new Set<number>();
  let probeTimer: number | undefined;
  let probeDrops = 0;
  const endProbe = () => {
    window.clearTimeout(probeTimer);
    probeTimer = undefined;
    probeIds.clear();
  };
  const ping = (socket: WebSocket, attempt: 1 | 2) => {
    const deadline = Math.min(PROBE_TIMEOUT_MAX_MS, PROBE_TIMEOUT_MS * 2 ** probeDrops);
    const armedAt = Date.now();
    probeTimer = window.setTimeout(() => {
      probeTimer = undefined;
      if (ws !== socket) return;
      // 페이지가 얼어 기한을 넘겼다: 그동안 pong을 읽을 수 없었으니 처음부터 다시 묻는다
      if (Date.now() - armedAt > deadline + PROBE_OVERDUE_MS) {
        probeIds.clear();
        ping(socket, 1);
      } else if (attempt === 1) ping(socket, 2);
      else drop(socket);
    }, deadline);
    const id = nextPing++;
    probeIds.add(id);
    // 보내기가 던지면 기한이 끊는다
    try {
      socket.send(JSON.stringify({ t: "ping", id } satisfies EventsClientMsg));
    } catch {
      /* 위와 같다 */
    }
  };
  const probe = () => {
    if (!ws || ws.readyState !== WebSocket.OPEN || probeTimer !== undefined) return;
    ping(ws, 1);
  };
  // 답하지 않는 연결은 close 사건을 기다리지 않고 끝내고 바로 새로 붙는다
  const drop = (socket: WebSocket) => {
    probeDrops += 1;
    endProbe();
    socket.onclose = null;
    socket.onmessage = null;
    socket.close();
    onLink(false);
    window.clearTimeout(timer);
    delay = 500;
    open();
  };
  let hiddenAt: number | null = document.visibilityState === "visible" ? null : Date.now();
  const onShow = () => {
    if (document.visibilityState !== "visible") {
      // 가려진 동안은 기한을 멈춘다(얼어 있는 동안 끊지 않는다). 다시 보이면 새로 묻는다
      hiddenAt ??= Date.now();
      endProbe();
      return;
    }
    const away = hiddenAt === null ? 0 : Date.now() - hiddenAt;
    hiddenAt = null;
    if (away >= RECONNECT_AFTER_HIDDEN_MS && ws?.readyState === WebSocket.OPEN) {
      drop(ws);
      probeDrops = 0;
      return;
    }
    probe();
  };
  const idle = window.setInterval(() => {
    if (document.visibilityState === "visible" && Date.now() - heard >= IDLE_PROBE_MS) probe();
  }, IDLE_CHECK_MS);
  document.addEventListener("visibilitychange", onShow);
  window.addEventListener("pageshow", onShow);
  window.addEventListener("online", probe);
  const open = () => {
    const socket = new WebSocket(wsUrl("/api/v1/events"));
    ws = socket;
    heard = Date.now();
    socket.onopen = () => {
      delay = 500;
      heard = Date.now();
      tell();
      onLink(true);
    };
    socket.onmessage = (m) => {
      heard = Date.now();
      const ev = JSON.parse(m.data as string) as FrontEvent;
      if (ev.ev === "state") onState(ev);
      else if (ev.ev === "question") {
        const { ev: _tag, ...q } = ev;
        onQuestion(q as QuestionView);
      } else if (ev.ev === "thread") {
        const { ev: _tag, ...t } = ev;
        onThread(t as ThreadView);
      } else if (ev.ev === "preferences") {
        const { ev: _tag, ...p } = ev;
        onPreferences(p);
      } else if (ev.ev === "pong" && probeIds.has(ev.id)) {
        probeDrops = 0;
        endProbe();
      }
    };
    socket.onclose = () => {
      if (ws !== socket) return;
      endProbe();
      onLink(false);
      if (!stopped) timer = window.setTimeout(open, delay);
      delay = Math.min(delay * 2, 10_000);
    };
  };
  open();
  return () => {
    stopped = true;
    stopPresence();
    endProbe();
    window.clearTimeout(timer);
    window.clearInterval(idle);
    document.removeEventListener("visibilitychange", onShow);
    window.removeEventListener("pageshow", onShow);
    window.removeEventListener("online", probe);
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
