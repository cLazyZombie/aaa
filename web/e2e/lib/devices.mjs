// 기기 능력 가짜: 소리(AudioContext)·소리 범주(audioSession)·알림(Notification)·push(PushManager)·홈 화면 앱(standalone).
// 실제 기기의 규칙을 따른다. 규칙 없는 가짜(아무 때나 resume되는 AudioContext, 어디에나 있는 Notification)는 iPhone에서만
// 드러난 결함(무음 스위치에 묻힌 소리, 가려졌다 온 뒤 소리 없음, 탭에 없는 알림)을 headless에서 못 잡았다(docs/testing-rules.md 「실기와 headless」).
//
// 기록은 exposeBinding으로 Node에 바로 온다: 시험은 `device.next(…)`로 그 기록을 사건으로 기다린다(page를 다시 묻지 않는다).
//
//   const { page, device } = await pairDevice(browser, aaa("web", "pair").code, "iphone", { standalone: true, silentSwitch: true });
//   const m = device.mark();
//   … 소리를 낼 일 …
//   await device.next("소리", (e) => e === "tone 880", m);
//
// 소리의 시계(AudioContext.currentTime)는 실제로 흐르고, `device.skipAudio()`가 울리는 소리 끝 뒤로 건너뛴다(t3code TestClock.adjust):
// 제품은 한 소리가 울리는 동안 온 알림을 겹쳐 울리지 않으므로, 다음 알림 전에 시간을 자지 않고 건너뛴다.
// `device.visible(false|true)`는 홈 화면으로 나갔다 돌아온 것(visibilitychange)이다.
//
// iOS 규칙(WebKit, iOS 17~18 Safari·홈 화면 앱):
// - AudioContext는 suspended로 태어나고, resume은 사람이 누른 직후(1초 안, WebKit transient activation)에만 running이 된다.
//   그 밖의 resume은 running이 될 때까지 끝나지 않는다. 화면이 가려지면 interrupted가 되고 다시 보여도 누르기 전에는 그대로다.
//   running이 아닐 때 시작한 소리는 들리지 않는다(`silent <Hz>`).
// - 무음 스위치(silentSwitch)를 켠 기기는 소리 범주가 playback·play-and-record가 아니면 소리가 묻힌다(`muted <Hz>`).
// - Safari 탭에는 Notification·PushManager가 없다(Web Push는 홈 화면 앱만). 홈 화면 앱(standalone)은 navigator.standalone이고
//   알림 권한은 누른 직후에만 묻는다(그 밖에는 default 그대로). 시스템 알림은 `system <제목>`으로 남는다.
import { pairPage } from "./harness.mjs";

const IPHONE_UA =
  "Mozilla/5.0 (iPhone; CPU iPhone OS 18_7 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/27.0.1 Mobile/15E148 Safari/604.1";
const IPAD_UA = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/27.2 Safari/605.1.15";

/** 기기 꼴(크기·UA·platform·터치)과 그 기기의 규칙 */
export const PROFILES = {
  iphone: { viewport: { width: 390, height: 844 }, userAgent: IPHONE_UA, platform: "iPhone", deviceScaleFactor: 3, touch: true, rules: "ios" },
  ipad: { viewport: { width: 820, height: 1180 }, userAgent: IPAD_UA, platform: "MacIntel", touch: true, rules: "ios" },
  desktop: { viewport: { width: 1280, height: 800 }, rules: null },
};

/** 페이지 안에서 돈다(addInitScript). 글로 옮겨지므로 바깥 이름을 쓰지 않는다 */
function iosRules({ standalone, silentSwitch, permission }) {
  const queue = [];
  const rec = (e) => {
    queue.push(e);
    const send = window.__aaaDevice;
    if (typeof send === "function") while (queue.length) send(queue.shift());
  };
  // 사람이 누른 때: 제품의 capture 듣기보다 먼저 듣는다(init script가 먼저 돈다)
  let activeUntil = -1;
  const gesture = (e) => {
    if (e.isTrusted) activeUntil = performance.now() + 1000;
  };
  for (const t of ["pointerdown", "pointerup", "touchend", "keydown", "click"]) window.addEventListener(t, gesture, true);
  const active = () => performance.now() <= activeUntil;

  let sessionType = "auto";
  const session = {
    get type() {
      return sessionType;
    },
    set type(v) {
      sessionType = v;
      rec(`session ${v}`);
    },
  };
  Object.defineProperty(navigator, "audioSession", { configurable: true, value: session });

  const contexts = new Set();
  class IosAudioContext {
    state = "suspended";
    destination = {};
    #clock = 0;
    #since = null;
    #waiting = [];
    #lastEnd = 0;
    constructor() {
      contexts.add(this);
      rec("audio new");
    }
    get currentTime() {
      return this.#clock + (this.#since === null ? 0 : (performance.now() - this.#since) / 1000);
    }
    #to(state) {
      if (state === this.state) return;
      if (this.state === "running") this.#clock = this.currentTime;
      this.#since = state === "running" ? performance.now() : null;
      this.state = state;
      rec(`audio ${state}`);
      if (state === "running") for (const w of this.#waiting.splice(0)) w();
    }
    interrupt() {
      if (this.state === "running") this.#to("interrupted");
    }
    /** 시작한 소리가 모두 끝난 뒤로 시계를 옮긴다 */
    skip() {
      const now = this.currentTime;
      if (this.#lastEnd + 0.05 > now) this.#clock += this.#lastEnd + 0.05 - now;
    }
    resume() {
      if (this.state === "running") return Promise.resolve();
      if (active()) {
        this.#to("running");
        return Promise.resolve();
      }
      rec(`audio resume-blocked ${this.state}`);
      return new Promise((r) => this.#waiting.push(r));
    }
    async close() {
      contexts.delete(this);
      this.#to("closed");
    }
    createOscillator() {
      const o = {
        type: "",
        frequency: { value: 0 },
        connect: (g) => g,
        start: (at = this.currentTime) => {
          // 한 음 0.3초(제품 alertSound.ts NOTE_LENGTH_S)
          this.#lastEnd = Math.max(this.#lastEnd, at + 0.3);
          const hz = o.frequency.value;
          if (this.state !== "running") rec(`silent ${hz}`);
          else if (silentSwitch && sessionType !== "playback" && sessionType !== "play-and-record") rec(`muted ${hz}`);
          else rec(`tone ${hz}`);
        },
        stop() {},
      };
      return o;
    }
    createGain() {
      return { gain: { setValueAtTime() {}, exponentialRampToValueAtTime() {} }, connect: (d) => d };
    }
  }
  Object.defineProperty(window, "AudioContext", { configurable: true, value: IosAudioContext });
  Object.defineProperty(window, "webkitAudioContext", { configurable: true, value: IosAudioContext });
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "hidden") for (const c of contexts) c.interrupt();
  });
  Object.defineProperty(window, "__aaaAudioSkip", { value: () => contexts.forEach((c) => c.skip()) });

  if (!standalone) {
    // Safari 탭: 알림·push가 없다
    delete window.Notification;
    delete window.PushManager;
    if (window.ServiceWorkerRegistration)
      Object.defineProperty(ServiceWorkerRegistration.prototype, "pushManager", { configurable: true, get: () => undefined });
    return;
  }
  Object.defineProperty(navigator, "standalone", { configurable: true, get: () => true });
  const mm = window.matchMedia.bind(window);
  window.matchMedia = (q) => (/display-mode:\s*standalone/.test(q) ? { matches: true, media: q, addEventListener() {}, removeEventListener() {} } : mm(q));
  let perm = "default";
  class IosNotification {
    static get permission() {
      return perm;
    }
    static async requestPermission() {
      if (!active()) {
        rec("permission-blocked");
        return perm;
      }
      perm = permission;
      rec(`permission ${perm}`);
      return perm;
    }
    constructor(title) {
      rec(`page-notification ${title}`);
    }
    addEventListener() {}
  }
  Object.defineProperty(window, "Notification", { configurable: true, value: IosNotification });
  if (window.ServiceWorkerRegistration)
    ServiceWorkerRegistration.prototype.showNotification = async function (title) {
      rec(perm === "granted" ? `system ${title}` : `system-blocked ${title}`);
    };
}

/**
 * profile 기기로 pairing한 화면과 그 기기 기록. opts: standalone(홈 화면 앱), silentSwitch(무음 스위치 켬),
 * permission(알림 권한을 물으면 답할 것, 기본 granted). pairPage의 나머지 opts(line·init·colorScheme…)는 그대로 넘긴다.
 */
export async function pairDevice(browser, code, profile, { standalone = false, silentSwitch = false, permission = "granted", ...rest } = {}) {
  const p = PROFILES[profile];
  if (!p) throw new Error(`모르는 기기 ${profile}: ${Object.keys(PROFILES)}`);
  const log = [];
  const waiters = new Set();
  const device = {
    log,
    /** 지금까지 기록 수: `next(…, from)`의 시작점 */
    mark: () => log.length,
    /** from 뒤(기본 처음부터)에 pred에 맞는 기록이 오면 그 기록. ms는 오지 않을 때 멈추는 안전 상한이다 */
    next(what, pred, from = 0, ms = 20_000) {
      const hit = log.slice(from).find(pred);
      if (hit !== undefined) return Promise.resolve(hit);
      return new Promise((resolve, reject) => {
        const w = {
          pred,
          resolve: (e) => (clearTimeout(timer), waiters.delete(w), resolve(e)),
        };
        const timer = setTimeout(() => {
          waiters.delete(w);
          reject(new Error(`${ms}ms 안에 기기 기록 「${what}」이 오지 않았다: ${JSON.stringify(log.slice(from))}`));
        }, ms);
        waiters.add(w);
      });
    },
    /** from 뒤 기록 */
    since: (from) => log.slice(from),
  };
  const setup = async (ctx) => {
    await ctx.exposeBinding("__aaaDevice", (_source, e) => {
      log.push(e);
      for (const w of [...waiters]) if (w.pred(e)) w.resolve(e);
    });
    if (p.rules === "ios") await ctx.addInitScript(iosRules, { standalone, silentSwitch, permission });
  };
  const { viewport, rules: _rules, ...look } = p;
  const paired = await pairPage(browser, code, viewport, { ...look, ...rest, setup });
  /** 울리는 소리의 끝 뒤로 소리 시계를 옮긴다(규칙이 없는 기기에서는 아무것도 하지 않는다) */
  device.skipAudio = () => paired.page.evaluate(() => window.__aaaAudioSkip?.());
  /** 홈 화면으로 나감(false)·돌아옴(true): visibilityState와 그 사건 */
  device.visible = (on) =>
    paired.page.evaluate((on) => {
      Object.defineProperty(document, "visibilityState", { configurable: true, get: () => (on ? "visible" : "hidden") });
      Object.defineProperty(document, "hidden", { configurable: true, get: () => !on });
      document.dispatchEvent(new Event("visibilitychange"));
    }, on);
  return { ...paired, device };
}
