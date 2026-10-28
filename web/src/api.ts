import { logIgnoredFailure } from "./errorDiagnostic.ts";
import { apiFetch } from "./apiRequest";
import { ApiError, readApiResponse } from "./apiOutcome";
export { ApiError } from "./apiOutcome";
// host 웹 API(crates/aaa-host/src/web.rs). 상태는 host가 갖고, 여기서는 받은 것을 보여 주기만 한다.
import type {
  DeviceRole,
  EventsClientMsg,
  FrontEvent,
  FrontRequest,
  FrontResponse,
  NoticeView,
  Outcome,
  PasskeyChallenge,
  PasskeyCreation,
  PasskeyState,
  QuestionView,
  RolloutCard,
  SharedPreferences,
  ThreadView,
} from "./gen/proto";
import type { PreferencesChange } from "./herdr/preferences.ts";
import { pageInUse } from "./herdr/presence";
import { createPasskey, signPasskey, withGesture } from "./passkey";

async function post<T>(path: string, body: unknown, keepalive = false): Promise<T> {
  const res = await apiFetch(path, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
    credentials: "same-origin",
    keepalive,
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

/** host `tasks` 사건(TASKS.md가 바뀜, FRONT_API 1.60): 할 일 화면과 배지가 다시 읽는다 */
const workspaceMetricsListeners = new Set<() => void>();
export function onWorkspaceMetricsChanged(listener: () => void): () => void {
  workspaceMetricsListeners.add(listener);
  return () => workspaceMetricsListeners.delete(listener);
}
const ideasListeners = new Set<() => void>();
/** 아이디어 목록·노트가 바뀌었다(웹 events `ideas`, FRONT_API 1.146) */
export function onIdeasChanged(listener: () => void): () => void {
  ideasListeners.add(listener);
  return () => ideasListeners.delete(listener);
}
const tasksListeners = new Set<(sha256: string) => void>();
export function onTasksChanged(listener: (sha256: string) => void): () => void {
  tasksListeners.add(listener);
  return () => tasksListeners.delete(listener);
}

/** host `asks` 사건(확인 필요 post·답한 것 기록이 바뀜, FRONT_API 1.78, plan §8.345): 「답할 것」이 다시 읽는다 */
const asksListeners = new Set<() => void>();
export function onAsksChanged(listener: () => void): () => void {
  asksListeners.add(listener);
  return () => asksListeners.delete(listener);
}

/** host `enroll` 사건(머신 등록 창·대기 목록이 바뀜, FRONT_API 1.80): 설정 「머신」이 다시 읽는다. 연결을 열 때도 한 번 온다 */
const enrollListeners = new Set<() => void>();
export function onEnrollChanged(listener: () => void): () => void {
  enrollListeners.add(listener);
  return () => enrollListeners.delete(listener);
}

/** host `rollout` 사건(배포 기록·개별 배포 작업이 바뀜): 설정 「배포」가 기록을 다시 읽는다. 연결을 열 때도 한 번 온다(관리 GUI W6a) */
const rolloutListeners = new Set<() => void>();
export function onRolloutChanged(listener: () => void): () => void {
  rolloutListeners.add(listener);
  return () => rolloutListeners.delete(listener);
}

/** host `accounts` 사건(계정이 바뀜: 로그인 진행·끝, 기본, 메일·상태, FRONT_API 1.79): 설정 「계정」이 다시 읽는다. 연결을 열 때도 한 번 온다 */
const accountsListeners = new Set<() => void>();
export function onAccountsChanged(listener: () => void): () => void {
  accountsListeners.add(listener);
  return () => accountsListeners.delete(listener);
}

/** host `inbox` 사건(알림함이 바뀜, FRONT_API 1.77): 알림함 배지와 패널이 다시 읽는다. 연결을 열 때도 한 번 온다 */
const inboxListeners = new Set<() => void>();
export function onInboxChanged(listener: () => void): () => void {
  inboxListeners.add(listener);
  return () => inboxListeners.delete(listener);
}

async function unwrap<T>(res: Response): Promise<T> {
  // pairing 자체(/api/v1/pair)의 401은 코드가 틀린 것이다. 그 밖의 401은 이 기기를 host가 모른다는 뜻이다
  if (res.status === 401 && !res.url.endsWith("/api/v1/pair")) reportSignedOut();
  return readApiResponse<T>(res);
}

/** `front_api`: host의 FRONT_API(1.58부터, 옛 host는 없다). `passkey`: 공개 주소로 연 화면의 passkey 상태(관리 GUI W7),
 * loopback 화면·옛 host는 없다 */
export type Me = { role: DeviceRole; device: string; front_api?: string; passkey?: PasskeyState };

export async function me(): Promise<Me | null> {
  const res = await apiFetch("/api/v1/me", { credentials: "same-origin" });
  if (res.status === 401) return null;
  return unwrap<Me>(res);
}

/** 코드 하나로 이 브라우저를 기기로 연결한다. `label`은 설정 → 기기 목록에 보이는 이름이다(herdr-web-ui `pairDevice`) */
export const pair = (code: string, label: string) => post<{ role: DeviceRole }>("/api/v1/pair", { code, label });

/** 이 브라우저의 로그인을 잊는다(herdr-web-ui `signOut`, DELETE /api/auth). 기기 기록은 host에 남는다 */
export async function signOut(): Promise<void> {
  const res = await apiFetch("/api/v1/me", { method: "DELETE", credentials: "same-origin" });
  if (!res.ok) await readApiResponse<unknown>(res);
}
/** 공개 주소로 연 조작 기기의 승인 필요 동작(관리 GUI W7): host가 `passkey_required`로 답하면 이 기기 passkey의 사용자
 * 확인(UV)으로 다시 보낸다. host는 begin 때 받은 요청만 실행한다. passkey가 없으면 begin이 등록 안내로 거부한다 */
export async function call(req: FrontRequest, keepalive = false): Promise<FrontResponse> {
  try {
    return await post<FrontResponse>("/api/v1/call", req, keepalive);
  } catch (e) {
    if (!(e instanceof ApiError) || e.body.code !== "passkey_required") throw e;
    const challenge = await post<PasskeyChallenge>("/api/v1/passkey/approve/begin", req);
    return post<FrontResponse>("/api/v1/passkey/approve", await withGesture("승인", () => signPasskey(challenge)));
  }
}

/** 이 기기에 passkey를 등록한다(짝지은 뒤 10분 안, 공개 주소의 조작 기기만) */
export async function registerPasskey(): Promise<void> {
  const creation = await post<PasskeyCreation>("/api/v1/passkey/register/begin", {});
  await post<null>("/api/v1/passkey/register", await withGesture("등록", () => createPasskey(creation)));
}

/** herdr-web-ui uploadPaneImage: 저장된 경로는 초안에 넣고, 보내기 전에는 pane에 입력하지 않는다. */
export async function uploadChatImage(pane: string, image: File): Promise<string> {
  if (image.size === 0 || image.size > 16 * 1024 * 1024) throw new Error("그림은 1바이트부터 16MiB까지 붙여 넣습니다.");
  const response = await apiFetch(`/api/v1/panes/${encodeURIComponent(pane)}/image`, {
    method: "POST", body: image, credentials: "same-origin",
  });
  return (await unwrap<{ path: string }>(response)).path;
}

/** task 첨부(plan §8.327): 프로젝트 uploads/에 쓰고 메모에 적을 `uploads/<이름>`을 받는다. `kind`는 문서(md·html), 없으면 그림.
 * 메모 줄은 `task_edit`로 쓴다 */
export async function uploadTaskFile(project: string, file: File, kind?: "md" | "html"): Promise<string> {
  const response = await apiFetch(`/api/v1/projects/${encodeURIComponent(project)}/uploads${kind ? `?kind=${kind}` : ""}`, {
    method: "POST", body: file, credentials: "same-origin",
  });
  return (await unwrap<{ path: string }>(response)).path;
}

/** 아이디어 노트의 그림·영상 올리기(t-1260): 그 아이디어 폴더에 내용 hash 이름으로 쓰고 노트에 적을 이름을 돌려준다 */
export async function uploadIdeaFile(project: string, idea: string, file: Blob): Promise<string> {
  const response = await apiFetch(`/api/v1/projects/${encodeURIComponent(project)}/ideas/${encodeURIComponent(idea)}/files`, {
    method: "POST", body: file, credentials: "same-origin",
  });
  return (await unwrap<{ name: string }>(response)).name;
}

/** 웹 「보고서 올리기」(FRONT_API 1.80, plan §8.355): 파일 하나를 올리기 폴더의 `path`에 쓴다. 다 올리면 `library_upload_publish`로 등록한다 */
export async function uploadReportFile(project: string, upload: string, path: string, file: Blob): Promise<{ files: number; total: number }> {
  const response = await apiFetch(`/api/v1/projects/${encodeURIComponent(project)}/library-uploads/${upload}?path=${encodeURIComponent(path)}`, {
    method: "POST", body: file, credentials: "same-origin",
  });
  return unwrap<{ files: number; total: number }>(response);
}

/** 이 host의 웹 화면들이 함께 쓰는 설정과 고정(plan §8.79, herdr-web-ui api.ts `fetchPreferences`·`savePreferences`) */
export async function loadPreferences(): Promise<SharedPreferences> {
  const r = await call({ op: "preferences" });
  if (r.kind !== "preferences") throw new ApiError({ code: "internal", message: `뜻밖의 답 ${r.kind}` });
  const { kind: _kind, ...doc } = r;
  return doc;
}
/** `keepalive`: 화면이 닫혀도 브라우저가 요청을 마저 보낸다(떠날 때 보내기, QA t-0337 Q11) */
export async function savePreferences(change: PreferencesChange, keepalive = false): Promise<SharedPreferences> {
  const r = await call({ op: "preferences_set", settings: change.settings, pins: change.pins ?? null }, keepalive);
  if (r.kind !== "preferences") throw new ApiError({ code: "internal", message: `뜻밖의 답 ${r.kind}` });
  const { kind: _kind, ...doc } = r;
  return doc;
}

/** Web Push(§8.26) */
/** `user`는 쓰는 기기(앞 7자), `you`는 이 기기가 쓰는 기기인지(plan §8.105) */
/** `alerts`는 이 기기 구독이 고른 알림 종류(차이 목록 N3, 구독이 없으면 null) */
export type AlertPrefs = { input: boolean; done: boolean };
export type PushInfo = {
  enabled: boolean;
  key: string | null;
  subscribed: number;
  in_use: number;
  user: string | null;
  you: boolean;
  kinds: string[];
  alerts: AlertPrefs | null;
  /** 이 기기에 최근 push로 보낸 항목(옛 host는 없다) */
  pushed?: PushedItem[];
};
export type PushedItem = { project: string; subject: string; at_ms: number };
export async function pushInfo(): Promise<PushInfo> {
  return unwrap<PushInfo>(await apiFetch("/api/v1/push", { credentials: "same-origin" }));
}
/** host와 이 기기의 시계 차이를 덮는다 */
const PUSHED_SKEW_MS = 5_000;
let pushedAsk: { at: number; list: Promise<PushedItem[]> } | null = null;
/**
 * 이 기기에 `since`(이 기기 시계) 뒤 push로 보낸 항목. 깨어나 한꺼번에 온 사건들은 1초 안이면 한 번만 묻는다
 * (사용자 결정 10-18 「A」, plan §8.141)
 */
export function pushedSince(since: number): Promise<PushedItem[]> {
  const now = Date.now();
  if (!pushedAsk || now - pushedAsk.at > 1_000) pushedAsk = { at: now, list: pushInfo().then((i) => i.pushed ?? []) };
  return pushedAsk.list.then((all) => all.filter((p) => p.at_ms >= since - PUSHED_SKEW_MS));
}
/** `alerts`: 이 기기가 받을 알림. host가 이 기기로 보내는 push에 적용한다(참고 registerPushSubscription). 없으면 전에 고른 것을 둔다 */
export const pushSubscribe = (sub: PushSubscriptionJSON, alerts?: AlertPrefs) =>
  post<{ subscribed: boolean }>("/api/v1/push/subscribe", alerts === undefined ? sub : { ...sub, alerts });
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
 * 「application-active-reconnect」(10초)를 따랐다. herdr-web-ui ws.ts는 이때도 probe한다(plan §8.84).
 */
const RECONNECT_AFTER_HIDDEN_MS = 10_000;
/**
 * 쓰는 동안 presence를 이만큼마다 다시 알린다. host는 그 연결에서 앱 글이 끊기면 lease가 끝나 쓰는 화면에서 뺀다.
 * 멈춘 페이지(iOS가 잠재운 앱)는 timer가 돌지 않아 알리지 못한다. WebSocket pong은 브라우저 네트워크 층이 답하므로
 * 살아 있다는 증거가 아니다(10-18 iPhone 실기, plan §8.133).
 */
const PRESENCE_EVERY_MS = 10_000;

/** 이 웹 빌드: 주 묶음 파일 이름(예: `index-BsC_6E06`). host가 판정 근거에 남겨 기기가 어느 웹을 돌리는지 본다.
 * host가 events 연결마다 알리는 빌드(`web` 사건)와 다르면 「앱 다시 열기」 줄을 띄운다(UpdateNotice.tsx, plan §8.166) */
export const BUILD = (() => {
  try {
    return new URL(import.meta.url).pathname.split("/").pop()?.replace(/\.[jt]sx?$/, "") || "?";
  } catch (ignoredError) { logIgnoredFailure(ignoredError, "api.ts:262");
    return "?";
  }
})();

/** 지금 열린 상태 사건 연결(앱 안 알림 결과를 host에 알릴 때 쓴다) */
let eventsSocket: WebSocket | null = null;

/** 사건 하나를 앱 안 알림으로 어떻게 했는지 host에 알린다(host는 push 판정 근거로 흐름 사건에 남긴다) */
export function tellAlert(m: Omit<Extract<EventsClientMsg, { t: "alert" }>, "t">) {
  if (eventsSocket?.readyState !== WebSocket.OPEN) return;
  try {
    eventsSocket.send(JSON.stringify({ t: "alert", ...m } satisfies EventsClientMsg));
  } catch (ignoredError) { logIgnoredFailure(ignoredError, "api.ts:275");
    /* 기록일 뿐이다 */
  }
}

/** 대화 보기가 청한 pane의 알림: 기록이 바뀌었다(`changed`), 또는 host가 그 pane의 바뀜을 지금 알릴 수 있는가(`live`) */
export type ChatNote = { changed: true; signature: string } | { live: boolean };
/** pane마다 대화를 보는 화면의 받는 곳(차이 목록 C3, herdr-web-ui ws.ts `conversationWanted`). 연결이 열리면 모두 다시 청한다 */
const chatWanted = new Map<string, Set<(note: ChatNote) => void>>();

function sendChatWatch(socket: WebSocket | null, pane: string, enabled: boolean) {
  if (socket?.readyState !== WebSocket.OPEN) return;
  try {
    socket.send(JSON.stringify({ t: "chat_watch", pane, enabled } satisfies EventsClientMsg));
  } catch (ignoredError) { logIgnoredFailure(ignoredError, "api.ts:289");
    /* 연결이 끊기면 다시 붙으며 청한다 */
  }
}

function tellChat(pane: string, note: ChatNote) {
  for (const listener of chatWanted.get(pane) ?? []) listener(note);
}

/** 그 pane의 대화 기록 변화를 받는다. 처음에는 push가 안 되는 것으로 두고, host의 `chat_watching`이 알려 준다 */
export function watchChat(pane: string, onNote: (note: ChatNote) => void): () => void {
  let listeners = chatWanted.get(pane);
  if (listeners === undefined) {
    listeners = new Set();
    chatWanted.set(pane, listeners);
    sendChatWatch(eventsSocket, pane, true);
  }
  listeners.add(onNote);
  return () => {
    const current = chatWanted.get(pane);
    if (current === undefined || !current.delete(onNote) || current.size > 0) return;
    chatWanted.delete(pane);
    sendChatWatch(eventsSocket, pane, false);
  };
}

/** 기계·pane 목록과 확인창 질문·thread·공유 설정 사건만 받는다(layout·grid 사건은 TUI용이라 무시한다). */
export function watchEvents(
  onState: (ev: StateEvent) => void,
  onQuestion: (q: QuestionView) => void,
  onLink: (up: boolean) => void,
  onThread: (t: ThreadView) => void,
  onPreferences: (p: SharedPreferences) => void,
  onWeb: (build: string) => void,
  onNotice: (n: NoticeView) => void = () => {},
  onReports: (unseen: number) => void = () => {},
  onAppBadge: (count: number) => void = () => {},
  onRollout: (result: Outcome<RolloutCard | null>) => void = () => {},
): () => void {
  let ws: WebSocket | null = null;
  let stopped = false;
  let delay = 500;
  let timer: number | undefined;
  // 이 화면을 지금 쓰는지(herdr-web-ui presence, ws.ts): 바뀔 때와 다시 붙을 때, 쓰는 동안은 PRESENCE_EVERY_MS마다 알린다.
  // 쓰는 화면이 있으면 host는 Web Push를 보내지 않는다. 까닭(사건 이름)과 그때 화면 값은 host가 판정 근거로 남긴다
  let active: boolean | null = null;
  let why = "open";
  const tell = (cause = why) => {
    if (active === null || ws?.readyState !== WebSocket.OPEN) return;
    const visible = document.visibilityState === "visible";
    ws.send(JSON.stringify({ t: "presence", active, why: cause, visible, focus: document.hasFocus(), build: BUILD } satisfies EventsClientMsg));
  };
  const beat = window.setInterval(() => {
    if (active) tell("heartbeat");
  }, PRESENCE_EVERY_MS);
  // 쓰는 기기(사용자 결정 10-18, plan §8.105): 사람이 이 기기에서 누르거나 치거나 화면을 켜면(앱을 열거나 보이고 focus가
  // 되면) 알린다. host는 마지막으로 알린 기기가 쓰는 화면일 때만 push를 건너뛴다. 다시 붙을 때는 알리지 않는다(누른 것이
  // 아니다). 붙기 전·끊긴 동안 누른 것은 붙으면 보낸다
  let touched = false;
  const touch = () => {
    if (!active) return;
    if (ws?.readyState !== WebSocket.OPEN) return void (touched = true);
    touched = false;
    ws.send(JSON.stringify({ t: "touch" } satisfies EventsClientMsg));
  };
  const onKey = (event: KeyboardEvent) => {
    if (!event.repeat) touch();
  };
  window.addEventListener("pointerdown", touch, true);
  window.addEventListener("keydown", onKey, true);
  // herdr-web-ui watchPageInUse와 같은 사건에 pageshow·Page Lifecycle resume을 더했다. freeze(Chrome·Android)는 쓰지 않는
  // 것이다: 얼어 있는 페이지는 알림을 띄우지도, 가려졌다고 알리지도 못한다
  const use = (now: boolean, cause: string) => {
    if (now === active) return;
    active = now;
    why = cause;
    tell();
    if (now) touch();
  };
  const onUse = (event: Event) => use(pageInUse(), event.type);
  const onFreeze = () => use(false, "freeze");
  const useEvents: [EventTarget, string][] = [
    [window, "focus"],
    [window, "blur"],
    [window, "pagehide"],
    [window, "pageshow"],
    [document, "visibilitychange"],
    [document, "resume"],
  ];
  for (const [target, name] of useEvents) target.addEventListener(name, onUse);
  document.addEventListener("freeze", onFreeze);
  use(pageInUse(), "open");
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
    } catch (ignoredError) { logIgnoredFailure(ignoredError, "api.ts:409");
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
    for (const pane of chatWanted.keys()) tellChat(pane, { live: false });
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
    eventsSocket = socket;
    heard = Date.now();
    socket.onopen = () => {
      delay = 500;
      heard = Date.now();
      tell("open");
      if (touched) touch();
      for (const pane of chatWanted.keys()) sendChatWatch(socket, pane, true);
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
      } else if (ev.ev === "web") onWeb(ev.build);
      else if (ev.ev === "notice") {
        const { ev: _tag, ...n } = ev;
        onNotice(n);
      }
      else if (ev.ev === "reports") onReports(ev.unseen);
      else if (ev.ev === "workspace_metrics") for (const listener of workspaceMetricsListeners) listener();
      else if (ev.ev === "tasks") for (const listener of tasksListeners) listener(ev.sha256);
      else if (ev.ev === "ideas") for (const listener of ideasListeners) listener();
      else if (ev.ev === "asks") for (const listener of asksListeners) listener();
      else if (ev.ev === "inbox") for (const listener of inboxListeners) listener();
      else if (ev.ev === "accounts") for (const listener of accountsListeners) listener();
      else if (ev.ev === "enroll") for (const listener of enrollListeners) listener();
      else if (ev.ev === "app_badge") onAppBadge(ev.count);
      else if (ev.ev === "rollout") {
        onRollout(ev.result);
        for (const listener of rolloutListeners) listener();
      }
      else if (ev.ev === "chat_changed") tellChat(ev.pane, { changed: true, signature: ev.signature });
      else if (ev.ev === "chat_watching") tellChat(ev.pane, { live: ev.live });
      else if (ev.ev === "pong" && probeIds.has(ev.id)) {
        probeDrops = 0;
        endProbe();
      }
    };
    socket.onclose = (event) => {
      if (ws !== socket) return;
      endProbe();
      onLink(false);
      for (const pane of chatWanted.keys()) tellChat(pane, { live: false });
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
            .catch((e: unknown) => {
              if (stopped) return;
              // host가 설정을 읽지 못해 안전 모드로 떴다(503 unavailable, plan §8.370): 다시 붙지 않고 App이 다시 물어 그 글을 보인다
              if (e instanceof ApiError && e.body.code === "unavailable") {
                stopped = true;
                reportSignedOut();
              } else open();
            });
        }, delay);
      delay = Math.min(delay * 2, 10_000);
    };
  };
  open();
  return () => {
    stopped = true;
    for (const [target, name] of useEvents) target.removeEventListener(name, onUse);
    document.removeEventListener("freeze", onFreeze);
    window.clearInterval(beat);
    if (eventsSocket === ws) eventsSocket = null;
    endProbe();
    window.clearTimeout(timer);
    window.clearInterval(idle);
    document.removeEventListener("visibilitychange", onShow);
    window.removeEventListener("pageshow", onShow);
    window.removeEventListener("online", probe);
    window.removeEventListener("pointerdown", touch, true);
    window.removeEventListener("keydown", onKey, true);
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
