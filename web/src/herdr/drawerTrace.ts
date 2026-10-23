// ?drawer-debug=1에서만 기록한다. 주소·history.state·본문·인증 정보는 수집하지 않는다.
type Fields = Record<string, string | number | boolean | null>;
type Entry = Fields & { seq: number; ms: number; event: string };
const LIMIT = 800;
let enabled = false;
let seq = 0;
let since = 0;
const entries: Entry[] = [];
const counts: Record<string, number> = {};
// 원문 주소 대신 탭 안에서만 뜻이 있는 번호로 같은 목적지 복귀를 구분한다.
const routes = new Map<string, number>();
let routeSequence = 0;

function routeNumber(): number {
  const query = new URLSearchParams(location.search);
  const key = `${location.hash ?? ""}|${query.get("settings") ?? ""}|${query.get("settings-keybar") ?? ""}`;
  const known = routes.get(key);
  if (known !== undefined) return known;
  const number = ++routeSequence;
  routes.set(key, number);
  if (routes.size > 100) routes.delete(routes.keys().next().value!);
  return number;
}

export function drawerTraceEnabled(): boolean {
  return typeof location !== "undefined" && new URLSearchParams(location.search).get("drawer-debug") === "1";
}

export function drawerTrace(event: string, fields: Fields = {}): void {
  if (!enabled) return;
  counts[event] = (counts[event] ?? 0) + 1;
  const drawer = document.getElementById("workspace-drawer");
  const box = drawer?.getBoundingClientRect();
  entries.push({ ...fields, seq: ++seq, ms: Math.round(performance.now() - since), event,
    open: drawer?.classList.contains("is-open") ?? false,
    right: box ? Math.round(box.right) : null,
    inline: drawer?.style.transform ?? "", narrow: innerWidth <= 768, historyLength: history.length, route: routeNumber() });
  if (entries.length > LIMIT) entries.shift();
}

export function drawerTraceText(): string {
  return JSON.stringify({ version: 2, viewport: [innerWidth, innerHeight], counts, dropped: Math.max(0, seq - entries.length), entries }, null, 2);
}

export function startDrawerTrace(): () => void {
  if (!drawerTraceEnabled()) return () => {};
  enabled = true;
  since = performance.now();
  drawerTrace("start");
  const touch = (event: TouchEvent) => {
    const point = event.touches[0] ?? event.changedTouches[0];
    drawerTrace(event.type, { x: point ? Math.round(point.clientX) : null, y: point ? Math.round(point.clientY) : null,
      fingers: event.touches.length, cancelable: event.cancelable, trusted: event.isTrusted });
  };
  const page = (event: Event) => {
    drawerTrace(event.type, { persisted: "persisted" in event && event.persisted === true, visible: document.visibilityState === "visible" });
    // microtask가 제품 듣기보다 먼저 돌 수도 있다. 완료 순서로 해석하지 않는다.
    queueMicrotask(() => drawerTrace(`${event.type}:microtask`));
  };
  const touches = ["touchstart", "touchmove", "touchend", "touchcancel"] as const;
  const pages = ["popstate", "hashchange", "pagehide", "pageshow", "visibilitychange"] as const;
  for (const type of touches) window.addEventListener(type, touch, { capture: true, passive: true });
  for (const type of pages) window.addEventListener(type, page, true);
  const observer = new MutationObserver((mutations) => {
    if (mutations.some(({ target }) => target instanceof Element && (target.id === "workspace-drawer" || target.classList.contains("scrim")))) drawerTrace("drawer-dom");
  });
  observer.observe(document.documentElement, { subtree: true, attributes: true, attributeFilter: ["class", "style"] });
  return () => {
    for (const type of touches) window.removeEventListener(type, touch, true);
    for (const type of pages) window.removeEventListener(type, page, true);
    observer.disconnect();
    enabled = false;
  };
}
