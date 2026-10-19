// 이 기기의 알림(plan §8.70, herdr-web-ui App.tsx 알림 부분·lib/notifications.ts·Droplet·alertSound).
// 앱 안에는 질문이 열림(입력 필요)·thread의 새 보고(작업 완료)를 계속 띄운다. 외부 Web Push는 coordinator 사용자 post만이다.
// - 앱 안(Droplet): 앱이 보일 때 위에서 내려온다. 그 자리를 이미 보고 있으면 띄우지 않는다.
// - 소리: 앱 탭이 열려 있으면(가려져도) 울린다. 탭이 여럿이면 한 탭만 울린다(alertTurns).
// - 탭 알림: 이 기기에 push 구독이 없고 탭이 가려졌을 때만 브라우저 알림으로(구독이 있으면 host가 보낸다).
import { useEffect, useSyncExternalStore } from "react";
import { canPlayAlertSound, playAlertSound, unlockAlertSound, type AlertSoundKind } from "./herdr/alertSound";
import { createAlertTurnPlayer } from "./herdr/alertTurns";
import { showDroplet, type DropletKind } from "./herdr/droplet";
import { pushTest } from "./api";
import { ensurePushSubscription, pushSupported, removePushSubscription } from "./push";
import { alertPrefs, alertSetting, setAlertSetting, useAlertSetting } from "./settings";
import { beforeAlertSound } from "./silentMode";
import { noteAlert } from "./faviconBadge";

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
    const endpoint = await ensurePushSubscription(alertPrefs());
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
  const alertInput = useAlertSetting("alertInput");
  const alertDone = useAlertSetting("alertDone");
  // a device that already allowed alerts re-registers on every load: idempotent, and it
  // brings the device back if the server lost its subscriptions. Its alert choices go
  // the same way (참고 App.tsx 652~668줄: `alerts`가 바뀌면 다시 등록)
  useEffect(() => {
    if (s.notifications !== "granted" || !alertsOn || !pushSupported()) return;
    let cancelled = false;
    ensurePushSubscription({ input: alertInput, done: alertDone })
      .then((endpoint) => {
        if (!cancelled) put({ pushOn: endpoint !== null });
      })
      .catch(() => {
        if (!cancelled) put({ pushOn: false });
      });
    return () => {
      cancelled = true;
    };
  }, [s.notifications, alertsOn, alertInput, alertDone]);
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

/**
 * 늦은 알림: 멈췄던 페이지가 깨어난 바로 뒤에 받은 사건은 멈춘 동안 쌓였다가 온 것이다(iOS가 멈춘 앱의 소켓 사건은 다시
 * 앞으로 올 때 한꺼번에 온다). 멈춤은 1초 timer가 3초 넘게 돌지 않은 것으로 안다. 깨어남은 그 뒤 처음 보인 때(다시 보임·
 * pageshow·Page Lifecycle resume)나, 보이는 동안 멈췄다 돈 timer다. 잠깐 가렸다 다시 보인 것(멈추지 않음)은 아니다.
 * host 시각(thread `updated_at_ms`)은 쓰지 않는다: 보고가 검토 대기가 되는 때는 agent가 쉴 때라 저장 시각보다 늦을 수 있다.
 */
const WAKE_LATE_MS = 2_000;
const WAKE_TICK_MS = 1_000;
let wokeAt = 0;
/** 마지막으로 멈춘 때(멈추기 전 마지막 timer) */
let sleptAt = 0;
/** 가려진 때(보이는 동안 null), 마지막으로 가려졌다 다시 보인 동안의 시작과 다시 보인 때 */
let hiddenAt: number | null = null;
let reshown: { from: number; at: number } | null = null;
let checkWake = () => {};
if (typeof window !== "undefined" && typeof document !== "undefined") {
  let stalled = false;
  let tick = Date.now();
  // 아직 돌지 못한 timer도 멈춤이다: 깨어날 때 쌓인 소켓 사건이 timer보다 먼저 돌 수 있다
  const stalledNow = () => stalled || Date.now() - tick > WAKE_TICK_MS * 3;
  const shown = () => {
    if (document.visibilityState !== "visible" || !stalledNow()) return;
    stalled = false;
    sleptAt = tick;
    tick = Date.now();
    wokeAt = tick;
  };
  checkWake = shown;
  // 가려진 동안: 알림을 띄우지 못한 때의 시작(다시 붙어 놓친 사건을 읽을 때 그 뒤 이 기기에 간 push를 본다)
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState !== "visible") hiddenAt ??= Date.now();
    else if (hiddenAt !== null) {
      reshown = { from: hiddenAt, at: Date.now() };
      hiddenAt = null;
    }
  });
  document.addEventListener("visibilitychange", shown);
  window.addEventListener("pageshow", shown);
  document.addEventListener("resume", () => {
    stalled = true;
    shown();
  });
  window.setInterval(() => {
    if (Date.now() - tick > WAKE_TICK_MS * 3) stalled = true;
    shown();
    tick = Date.now();
  }, WAKE_TICK_MS);
}

/**
 * 이 페이지가 알림을 띄우지 못한 동안의 시작: 지금 가려져 있으면 가려진 때, 방금 다시 보였으면 가려졌던 때, 방금 멈췄다
 * 깼으면 멈춘 때(둘 다면 이른 쪽). 그 뒤 이 기기에 간 push는 이미 받은 알림이다. 그런 동안이 아니면 null.
 * 다시 붙어 놓친 사건을 읽을 때도 끊긴 때가 아니라 이때부터 본다: 10초 넘게 가려졌다 보이면 웹은 다시 보인 순간에 끊고
 * 새로 붙으므로, 끊긴 때부터 보면 가려진 동안 간 push를 놓친다(실기 10-18 10:22 iPhone, plan §8.150)
 */
export function unableSince(now = Date.now()): number | null {
  const from: number[] = [];
  if (hiddenAt !== null) from.push(hiddenAt);
  else if (reshown !== null && now - reshown.at < WAKE_LATE_MS) from.push(reshown.from);
  if (justWoke(now)) from.push(sleptAt);
  return from.length ? Math.min(...from) : null;
}

/** 방금 깨어났다: 지금 받은 사건은 그 일이 일어난 때 알리지 못한 늦은 알림이다 */
export function justWoke(now = Date.now()): boolean {
  // 사건이 깨어남 알림(timer·resume)보다 먼저 돌 수도 있다
  checkWake();
  return now - wokeAt < WAKE_LATE_MS;
}

/** 휴대폰·태블릿(손가락이 주 입력) */
function touchDevice(): boolean {
  return typeof matchMedia === "function" && matchMedia("(pointer: coarse)").matches;
}

/** 알림 하나 */
export interface Alert {
  /** 새 보고의 공개 링크. host의 LibraryView에서 조작 기기에만 준다. */
  libraryUrl?: string | null;
  kind: Exclude<DropletKind, "ended">;
  /** 누르면 갈 주소 hash(`pane=…`·`thread=…`) */
  target: string;
  /** 지금 이 화면이 그 자리를 보고 있다 */
  showing: boolean;
  /** 그 일이 일어난 때 알리지 못했다(다시 붙어 읽은 놓친 사건, 멈췄던 페이지가 깨어 받은 사건): 소리를 내지 않는다 */
  late: boolean;
  title: string;
  machine: string | null;
  agent: string | null;
  /** 탭 알림(브라우저 알림)의 제목·본문·자리(host push와 같은 자리) */
  notice: { title: string; body: string; tag: string };
}

/**
 * 사건 하나를 이 기기에 알린다(켜 둔 길로만). 참고 App.tsx dropIn·chime·showPaneStatusNotification.
 * 어떻게 했는지 한 줄로 돌려준다(host가 push 판정 근거로 남긴다, 10-18 실기)
 */
export function raiseAlert(a: Alert): string {
  if (!alertSetting("alertsOn")) return "알림 꺼짐";
  // 이 기기가 고른 종류만(참고 notify-policy.ts `alertsAllow`, 차이 목록 N3)
  if (!alertSetting(a.kind === "blocked" ? "alertInput" : "alertDone")) return "종류 꺼짐";
  const visible = document.visibilityState === "visible";
  const did: string[] = [];
  // 창에 focus가 없는 동안 알린 자리는 탭 아이콘 수로 센다(faviconBadge.ts, t3code onNotification)
  noteAlert(a.notice.tag);
  // In-app alerts: only while the app is on screen, never for what is already in front of the user
  if (!alertSetting("alertInApp")) did.push("앱 안 꺼짐");
  else if (!visible) did.push("가려짐");
  else if (a.showing) did.push("보는 중");
  else {
    showDroplet({ target: a.target, agent: a.agent, title: a.title, machine: a.machine, kind: a.kind, libraryUrl: a.libraryUrl });
    did.push("띄움");
  }
  did.push(document.hasFocus() ? "focus" : "focus 없음");
  // The alert sound: heard also while the tab is hidden; never for what is open in front of the user
  if (!alertSetting("alertSound")) did.push("소리 꺼짐");
  else if (visible && a.showing) did.push("소리 안 냄");
  // 늦은 소리는 내지 않는다: 알림 순간에 못 냈으면 버린다(coordinator 실기 10-18 07:45, 무음 모드 iPhone에서 앱으로 돌아올 때 울림)
  else if (a.late) did.push("늦어 소리 버림");
  // 가려진 휴대폰 페이지의 소리는 그때 나지 않고 앱으로 돌아올 때 난다(iOS는 뒤로 간 앱의 소리를 멈춘다). 그 기기에는 host push가 간다
  else if (!visible && touchDevice()) did.push("가려진 휴대폰이라 소리 안 냄");
  else if (!canPlayAlertSound()) did.push("소리 못 냄(소리 문맥 멈춤)");
  else {
    player().chime(JSON.stringify([a.target, a.kind]), (a.kind === "blocked" ? "blocked" : "done") satisfies AlertSoundKind);
    did.push("소리");
  }
  // Tab alerts: a device with a push subscription gets them from the host instead
  if (!state.pushOn && document.hidden) {
    void showTabNotification(a.notice.title, a.notice.body, a.notice.tag, `/#${a.target}`);
    did.push("탭 알림");
  }
  return did.join(" · ");
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
