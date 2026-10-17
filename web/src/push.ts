// service worker 등록(PWA)과 이 기기의 Web Push 구독(§8.26). herdr-web-ui src/lib/push.ts를 옮겼다(plan §8.70).
// 구독은 브라우저가 만들고 host에 endpoint·키를 맡긴다. host는 질문·보고 준비 같은 알림을 암호화해 보낸다.
// service worker와 PushManager는 secure context(https·localhost)에만 있고, iPhone은 홈 화면 앱에만 있다.
import { ApiError, pushInfo, pushSubscribe, pushTest, pushUnsubscribe } from "./api";

/** 앱을 열 때 한 번 등록한다. 등록에 실패해도 화면은 그대로 쓴다. */
export function registerServiceWorker() {
  if (!("serviceWorker" in navigator) || !window.isSecureContext) return;
  navigator.serviceWorker.register("/sw.js").catch((e) => console.warn("service worker 등록 실패", e));
  // 알림을 누르면 service worker가 열린 창에 보낼 곳을 알린다
  navigator.serviceWorker.addEventListener("message", (e) => {
    const d = e.data as { type?: string; hash?: string } | null;
    if (d?.type === "open" && typeof d.hash === "string") location.hash = d.hash;
  });
}

/** How long to wait for the service worker before giving up on push for this page load. */
const WORKER_READY_TIMEOUT_MS = 10_000;

export function pushSupported(): boolean {
  return typeof navigator !== "undefined" && "serviceWorker" in navigator && typeof globalThis.PushManager !== "undefined";
}

/** Test the existing subscription without repairing it first, so stale registrations stay visible. */
export async function testDevicePush(): Promise<"sent" | "unsupported" | "permission" | "missing"> {
  if (!pushSupported()) return "unsupported";
  if (globalThis.Notification?.permission !== "granted") return "permission";
  const registration = await navigator.serviceWorker.getRegistration();
  const subscription = await registration?.pushManager.getSubscription();
  if (!subscription) return "missing";
  await pushTest(subscription.endpoint);
  return "sent";
}

function keyBytes(base64url: string): Uint8Array<ArrayBuffer> {
  const base64 = base64url.replace(/-/g, "+").replace(/_/g, "/").padEnd(Math.ceil(base64url.length / 4) * 4, "=");
  const binary = atob(base64);
  const bytes = new Uint8Array(new ArrayBuffer(binary.length));
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
  return bytes;
}

function sameKey(current: ArrayBuffer | null, expected: Uint8Array): boolean {
  if (!current || current.byteLength !== expected.length) return false;
  const bytes = new Uint8Array(current);
  return bytes.every((byte, index) => byte === expected[index]);
}

/** navigator.serviceWorker.ready never settles when registration failed; bound it. */
async function workerRegistration(): Promise<ServiceWorkerRegistration | null> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<null>((resolve) => {
    timer = setTimeout(() => resolve(null), WORKER_READY_TIMEOUT_MS);
  });
  try {
    return await Promise.race([navigator.serviceWorker.ready, timeout]);
  } finally {
    clearTimeout(timer);
  }
}

/** The subscription being made right now, shared by every caller that arrives meanwhile. */
let pending: Promise<string | null> | null = null;
// Preserve click order across registration/removal, even when a prior operation fails.
let operations: Promise<unknown> = Promise.resolve();

/**
 * Subscribes this device (reusing a live subscription) and registers it with the server.
 * Idempotent, so it also runs on every load: a server that lost its state gets the device
 * back without a click. Resolves the endpoint, or null when push is not available here.
 *
 * Single-flight: the bell's first tap grants permission, which also starts App's
 * load-time registration. Two overlapping `subscribe()` calls make Chrome issue TWO
 * subscriptions and keep only the later one, so the confirmation push could go to a
 * dead endpoint. Concurrent callers therefore share one attempt.
 */
export function ensurePushSubscription(): Promise<string | null> {
  if (pending) return pending;
  const request = operations
    .catch(() => undefined)
    .then(() => subscribeDevice())
    .finally(() => {
      if (pending === request) pending = null;
    });
  pending = request;
  operations = request;
  return request;
}

// aaa: host가 꺼 두었으면(`notify.web_push`) 구독하지 않는다. 참고의 「버려진 endpoint」 거절(#1038, push-retired.json)은
// aaa host에 없어 그 되살리기도 옮기지 않았다.
async function subscribeDevice(): Promise<string | null> {
  if (!pushSupported() || globalThis.Notification?.permission !== "granted") return null;
  const registration = await workerRegistration();
  if (!registration) return null;
  const info = await pushInfo();
  if (!info.enabled || !info.key) return null;
  const key = keyBytes(info.key);
  let subscription = await registration.pushManager.getSubscription();
  // made for another server key (the server's state dir was reset): it can never receive our pushes
  if (subscription && !sameKey(subscription.options.applicationServerKey, key)) {
    await subscription.unsubscribe();
    subscription = null;
  }
  subscription ??= await registration.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: key });
  await pushSubscribe(subscription.toJSON());
  return subscription.endpoint;
}

/** Stops pushes to this device: the server forgets it first, then the browser drops it. */
export function removePushSubscription(): Promise<void> {
  // A later enable must enqueue a new registration after this removal, not join the old one.
  pending = null;
  const request = operations.catch(() => undefined).then(unsubscribeDevice);
  operations = request;
  return request;
}

async function unsubscribeDevice(): Promise<void> {
  if (!pushSupported()) return;
  const registration = await navigator.serviceWorker.getRegistration();
  const subscription = await registration?.pushManager.getSubscription();
  if (!subscription) return;
  try {
    await pushUnsubscribe(subscription.endpoint);
  } finally {
    await subscription.unsubscribe();
  }
}

/** 시험 알림 결과의 오류 코드(`subscription_not_found`·`subscription_gone`), 아니면 null */
export function pushErrorCode(error: unknown): string | null {
  return error instanceof ApiError ? error.body.code : null;
}
