// service worker 등록(PWA)과 Web Push 알림 켜기·끄기(§8.26).
// 구독은 브라우저가 만들고 host에 endpoint·키를 맡긴다. host는 질문·보고 준비 같은 알림을 암호화해 보낸다.
import { useCallback, useEffect, useState } from "react";
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

const supported = () =>
  window.isSecureContext && "serviceWorker" in navigator && "PushManager" in window && "Notification" in window;

/** iPhone·iPad Safari는 홈 화면 앱에서만 알림을 켤 수 있다 */
export const iosNeedsInstall = () =>
  /iPhone|iPad|iPod/.test(navigator.userAgent) &&
  !(navigator as Navigator & { standalone?: boolean }).standalone &&
  !("PushManager" in window);

function keyBytes(b64url: string): Uint8Array<ArrayBuffer> {
  const b64 = (b64url + "===".slice((b64url.length + 3) % 4)).replace(/-/g, "+").replace(/_/g, "/");
  const raw = atob(b64);
  const out = new Uint8Array(new ArrayBuffer(raw.length));
  for (let i = 0; i < raw.length; i++) out[i] = raw.charCodeAt(i);
  return out;
}

function sameKey(sub: PushSubscription, key: Uint8Array): boolean {
  const k = sub.options.applicationServerKey;
  if (!k) return false;
  const a = new Uint8Array(k);
  return a.length === key.length && a.every((v, i) => v === key[i]);
}

export type PushState = "hidden" | "off" | "on" | "denied" | "busy";

/** 알림 단추 상태와 켜기·끄기. 브라우저가 못 하거나 host에서 꺼져 있으면 hidden. */
export function usePush(): { state: PushState; msg: string; toggle: () => Promise<void> } {
  const [state, setState] = useState<PushState>("hidden");
  const [msg, setMsg] = useState("");
  const [key, setKey] = useState<string | null>(null);

  useEffect(() => {
    if (!supported()) return;
    let alive = true;
    (async () => {
      const info = await pushInfo();
      if (!alive || !info.enabled || !info.key) return;
      setKey(info.key);
      if (Notification.permission === "denied") return setState("denied");
      const reg = await navigator.serviceWorker.ready;
      const sub = await reg.pushManager.getSubscription();
      if (alive) setState(sub && info.subscribed > 0 && sameKey(sub, keyBytes(info.key)) ? "on" : "off");
    })().catch(() => undefined);
    return () => {
      alive = false;
    };
  }, []);

  const toggle = useCallback(async () => {
    if (!key) return;
    const before = state;
    setState("busy");
    setMsg("");
    try {
      const reg = await navigator.serviceWorker.ready;
      let sub = await reg.pushManager.getSubscription();
      if (before === "on") {
        if (sub) {
          await pushUnsubscribe(sub.endpoint);
          await sub.unsubscribe();
        }
        setState("off");
        return;
      }
      if ((await Notification.requestPermission()) !== "granted") {
        setState("denied");
        return;
      }
      const k = keyBytes(key);
      // 다른 host 키로 만든 구독이면 새로 만든다
      if (sub && !sameKey(sub, k)) {
        await sub.unsubscribe();
        sub = null;
      }
      sub ??= await reg.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: k });
      await pushSubscribe(sub.toJSON());
      setState("on");
      const t = await pushTest();
      if (t.failed.length) setMsg(`시험 알림 실패: ${t.failed[0]}`);
    } catch (e) {
      setState(before === "on" ? "on" : "off");
      setMsg(e instanceof ApiError ? e.message : `알림을 켜지 못했다: ${String(e)}`);
    }
  }, [key, state]);

  return { state, msg, toggle };
}
