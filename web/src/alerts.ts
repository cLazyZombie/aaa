// 이 기기의 알림(plan §8.70, herdr-web-ui App.tsx 알림 부분·lib/notifications.ts·Droplet·alertSound).
// host가 Web Push로 보내는 것과 같은 사건에 띄운다: 질문이 열림(입력 필요), thread의 새 보고(작업 완료).
// - 앱 안(Droplet): 앱이 보일 때 위에서 내려온다. 그 자리를 이미 보고 있으면 띄우지 않는다.
// - 소리: 앱 탭이 열려 있으면(가려져도) 울린다. 탭이 여럿이면 한 탭만 울린다(alertTurns).
// - 탭 알림: 이 기기에 push 구독이 없고 탭이 가려졌을 때만 브라우저 알림으로(구독이 있으면 host가 보낸다).
import { useEffect, useSyncExternalStore } from "react";
import { canPlayAlertSound, playAlertSound, unlockAlertSound, type AlertSoundKind } from "./herdr/alertSound";
import { createAlertTurnPlayer } from "./herdr/alertTurns";
import { showDroplet, type DropletKind } from "./herdr/droplet";
import { pushTest } from "./api";
import { ensurePushSubscription, pushSupported, removePushSubscription } from "./push";
import { alertSetting, setAlertSetting, useAlertSetting } from "./settings";
import { beforeAlertSound } from "./silentMode";

export type NotificationState = "unsupported" | "default" | "granted" | "denied";

export function notificationState(): NotificationState {
  if (typeof globalThis.Notification === "undefined") return "unsupported";
  return globalThis.Notification.permission;
}

export async function requestNotificationPermission(): Promise<NotificationState> {
  if (typeof globalThis.Notification === "undefined") return "unsupported";
  const permission = await globalThis.Notification.requestPermission();
  return permission === "granted" ? "granted" : permission === "denied" ? "denied" : "default";
}

// 권한과 이 기기의 push 구독(참고 App.tsx `notifications`·`pushOn`). 알림 줄기와 메뉴가 함께 읽는다.
let state = { notifications: notificationState(), pushOn: false };
const subs = new Set<() => void>();
function put(next: Partial<typeof state>) {
  state = { ...state, ...next };
  subs.forEach((f) => f());
}
function subscribe(f: () => void) {
  subs.add(f);
  return () => void subs.delete(f);
}

/** 알림 켜기(참고 enableNotifications): 앱 안 알림은 권한이 필요 없어 늘 켜지고, push는 권한을 받으면 구독해 시험 알림으로 확인한다 */
export async function enableAlerts(): Promise<boolean> {
  setAlertSetting("alertsOn", true);
  const current = notificationState();
  const next = current === "default" ? await requestNotificationPermission() : current;
  put({ notifications: next });
  if (next !== "granted") return false;
  try {
    const endpoint = await ensurePushSubscription();
    put({ pushOn: endpoint !== null });
    // the confirmation push proves the whole path (server -> push service -> this device)
    if (endpoint) await pushTest(endpoint);
    return endpoint !== null;
  } catch (err) {
    console.warn("web push unavailable, alerts stay tab-only", err);
    return false;
  }
}

/** 알림 끄기(참고 disableNotifications): 브라우저 권한은 페이지가 거둘 수 없어, 이 기기 구독을 지우고(host도 잊는다) 탭 알림도 끈다 */
export async function disableAlerts(): Promise<void> {
  setAlertSetting("alertsOn", false);
  put({ pushOn: false });
  await removePushSubscription().catch((err) => console.warn("could not drop the push subscription", err));
}

/** 「알림」 항목이 말하는 이 기기의 상태(참고 App.tsx `bell`, 문구는 참고 i18n.ko.ts) */
export interface Bell {
  state: string;
  title: string;
  on: boolean;
  run: () => Promise<unknown>;
}

export function useAlerts(): { bell: Bell; visible: boolean; offDot: boolean; notifications: NotificationState; pushOn: boolean } {
  const s = useSyncExternalStore(subscribe, () => state);
  const alertsOn = useAlertSetting("alertsOn");
  const alertInApp = useAlertSetting("alertInApp");
  // a device that already allowed alerts re-registers on every load: idempotent, and it
  // brings the device back if the server lost its subscriptions
  useEffect(() => {
    if (s.notifications !== "granted" || !alertsOn || !pushSupported()) return;
    let cancelled = false;
    ensurePushSubscription()
      .then((endpoint) => {
        if (!cancelled) put({ pushOn: endpoint !== null });
      })
      .catch(() => {
        if (!cancelled) put({ pushOn: false });
      });
    return () => {
      cancelled = true;
    };
  }, [s.notifications, alertsOn]);
  // The Alerts item says what this device does, whatever the browser's permission: in-app alerts
  // need none, so they count as on. A device that has not answered the permission question is
  // asked by the item; one that has answered gets a plain switch.
  const bell: Bell = !alertsOn
    ? { state: "이 기기에서 꺼짐", title: "이 기기에서 알림이 꺼져 있습니다. 누르면 켜집니다", on: false, run: enableAlerts }
    : s.notifications !== "granted"
      ? !alertInApp
        ? { state: "이 기기에서 꺼짐", title: "패널이 입력을 기다리거나 끝나면 알려줍니다", on: false, run: enableAlerts }
        : s.notifications === "default"
          ? { state: "앱 안에서 켜짐", title: "앱에서 알림 켜짐. 누르면 앱을 닫았을 때도 받도록 허용할 수 있습니다", on: true, run: enableAlerts }
          : { state: "앱 안에서 켜짐", title: "앱에서 알림 켜짐. 누르면 꺼집니다", on: true, run: disableAlerts }
      : s.pushOn
        ? { state: "켜짐, 이 기기로 푸시", title: "알림 켜짐, 이 기기로 푸시됩니다. 앱을 닫아도 옵니다. 누르면 꺼집니다", on: true, run: disableAlerts }
        : {
            state: "이 탭에서 켜짐",
            // turning them off and on again retries the push subscription
            title: pushSupported()
              ? "이 탭이 열려 있는 동안 알림이 옵니다. 누르면 꺼집니다"
              : "이 탭이 열려 있는 동안 알림이 옵니다 (앱을 닫은 뒤의 알림은 https가 필요하고, iPhone에서는 홈 화면 앱이어야 합니다). 누르면 꺼집니다",
            on: true,
            run: disableAlerts,
          };
  // hidden only where it could do nothing: no system notifications and in-app alerts off
  const visible = s.notifications === "default" || s.notifications === "granted" || alertInApp;
  // the menu's button carries a dot while alerts are off here: the one state of the menu worth a glance
  return { bell, visible, offDot: visible && !bell.on, notifications: s.notifications, pushOn: s.pushOn };
}

/** 소리를 켠 동안, 탭·키를 누를 때마다 다음 소리를 울릴 수 있게 한다(참고 App.tsx: 페이지는 사람이 누른 뒤에만 소리를 낸다) */
export function useAlertSoundUnlock() {
  const on = useAlertSetting("alertSound");
  useEffect(() => {
    if (!on) return;
    const unlock = () => void unlockAlertSound();
    const events = ["pointerdown", "pointerup", "keydown"] as const;
    for (const event of events) window.addEventListener(event, unlock, true);
    return () => {
      for (const event of events) window.removeEventListener(event, unlock, true);
    };
  }, [on]);
}

let turns: ReturnType<typeof createAlertTurnPlayer> | null = null;
function player() {
  turns ??= createAlertTurnPlayer({
    play: (kind) => {
      if (!alertSetting("alertsOn") || !alertSetting("alertSound")) return false;
      if (canPlayAlertSound()) beforeAlertSound();
      return playAlertSound(kind);
    },
  });
  return turns;
}

/** 알림 하나 */
export interface Alert {
  kind: Exclude<DropletKind, "ended">;
  /** 누르면 갈 주소 hash(`pane=…`·`thread=…`) */
  target: string;
  /** 지금 이 화면이 그 자리를 보고 있다 */
  showing: boolean;
  title: string;
  machine: string | null;
  agent: string | null;
  /** 탭 알림(브라우저 알림)의 제목·본문·자리(host push와 같은 자리) */
  notice: { title: string; body: string; tag: string };
}

/** 사건 하나를 이 기기에 알린다(켜 둔 길로만). 참고 App.tsx dropIn·chime·showPaneStatusNotification */
export function raiseAlert(a: Alert) {
  if (!alertSetting("alertsOn")) return;
  const visible = document.visibilityState === "visible";
  // In-app alerts: only while the app is on screen, never for what is already in front of the user
  if (alertSetting("alertInApp") && visible && !a.showing) {
    showDroplet({ target: a.target, agent: a.agent, title: a.title, machine: a.machine, kind: a.kind });
  }
  // The alert sound: heard also while the tab is hidden; never for what is open in front of the user
  if (alertSetting("alertSound") && canPlayAlertSound() && !(visible && a.showing)) {
    player().chime(JSON.stringify([a.target, a.kind]), a.kind as AlertSoundKind);
  }
  // Tab alerts: a device with a push subscription gets them from the host instead
  if (!state.pushOn && document.hidden) void showTabNotification(a.notice.title, a.notice.body, a.notice.tag, `/#${a.target}`);
}

/**
 * Shown through the service worker when there is one: Android Chrome has no
 * `new Notification()`, and the worker's notificationclick (public/sw.js) opens the place.
 * The constructor is the fallback for a page without a worker. (참고 lib/notifications.ts show)
 */
async function showTabNotification(title: string, body: string, tag: string, url: string): Promise<void> {
  if (typeof globalThis.Notification === "undefined") return;
  if (globalThis.Notification.permission !== "granted") return;
  if (!document.hidden) return; // visible tab: the UI already shows it
  const options: NotificationOptions = { body, tag, data: { url }, icon: "/icon-192.png?v=2" };
  try {
    const registration = "serviceWorker" in navigator ? await navigator.serviceWorker.getRegistration() : undefined;
    if (registration?.active) {
      await registration.showNotification(title, options);
      return;
    }
    const notification = new globalThis.Notification(title, options);
    notification.addEventListener("click", () => {
      window.focus();
      location.hash = url.slice(2);
    });
  } catch {
    /* no way to show it here: the list still carries the change */
  }
}
