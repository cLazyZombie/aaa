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

/**
 * 이 브라우저의 기기가 취소됐거나 로그아웃했다: host가 cookie를 모른다(401) 또는 열린 WebSocket을 4401로 닫았다.
 * App이 듣고 pairing 화면으로 간다(herdr-web-ui는 health 검사가 gate를 다시 알린다).
 */
export const REVOKED_CLOSE_CODE = 4401;
const signedOutListeners = new Set<() => void>();
export function onSignedOut(listener: () => void): () => void {
  signedOutListeners.add(listener);
  return () => signedOutListeners.delete(listener);
}
export function reportSignedOut(): void {
  for (const listener of signedOutListeners) listener();
}

async function unwrap<T>(res: Response): Promise<T> {
  // pairing 자체(/api/v1/pair)의 401은 코드가 틀린 것이다. 그 밖의 401은 이 기기를 host가 모른다는 뜻이다
  if (res.status === 401 && !res.url.endsWith("/api/v1/pair")) reportSignedOut();
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

/** 코드 하나로 이 브라우저를 기기로 연결한다. `label`은 설정 → 기기 목록에 보이는 이름이다(herdr-web-ui `pairDevice`) */
export const pair = (code: string, label: string) => post<{ role: DeviceRole }>("/api/v1/pair", { code, label });

/** 이 브라우저의 로그인을 잊는다(herdr-web-ui `signOut`, DELETE /api/auth). 기기 기록은 host에 남는다 */
export async function signOut(): Promise<void> {
  const res = await fetch("/api/v1/me", { method: "DELETE", credentials: "same-origin" });
  if (!res.ok) throw new ApiError({ code: "internal", message: `HTTP ${res.status}` });
}
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
      } else if (ev.ev === "preferences") {
        const { ev: _tag, ...p } = ev;
        onPreferences(p);
      }
    };
    ws.onclose = (event) => {
      onLink(false);
      // 기기가 취소됐다: 다시 붙지 않는다
      if (event.code === REVOKED_CLOSE_CODE) {
        stopped = true;
        reportSignedOut();
        return;
      }
      // 다시 붙기 전에 host가 아직 이 기기를 아는지 본다. 끊긴 동안 취소되면 upgrade는 401이라 WebSocket이 그 까닭을 알려 주지 않는다
      if (!stopped)
        timer = window.setTimeout(() => {
          if (stopped) return;
          void me()
            .then((m) => {
              if (stopped) return;
              if (m === null) {
                stopped = true;
                reportSignedOut();
              } else open();
            })
            .catch(() => {
              if (!stopped) open();
            });
        }, delay);
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
