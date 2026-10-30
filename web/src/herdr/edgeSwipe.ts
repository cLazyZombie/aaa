// herdr-web-ui(aa624cb) src/lib/edgeSwipe.ts를 옮겼다. 고지는 README.md. 여는 쓸기의 자리만 바꿨다: 참고는 왼쪽 끝 24px에서
// 시작한 쓸기로 열지만, 그 자리는 iPhone Safari의 뒤로 쓸기 자리라 두 몸짓이 함께 돌았다. aaa는 왼쪽 끝을 브라우저에 두고
// 화면 안쪽에서 오른쪽으로 쓸면 연다(plan §8.408, 사용자 1A).
import { drawerTrace } from "./drawerTrace.ts";
/*
 * The workspace drawer follows a phone's finger: a swipe to the right from inside the screen
 * opens it, as the menu button does, and a swipe to the left closes it again. The left edge
 * is the browser's: Safari's (and Android's) back swipe begins there, and a page is told
 * nothing when it does, so the drawer leaves strokes from the edge alone. Only at the drawer's
 * width (<=768px, styles.css) and only for a mostly horizontal stroke, so the terminal's
 * one-finger scrolling and the transcript's vertical scrolling keep working.
 *
 * While the finger is down the drawer is under it, as an iPhone's is: it moves with the
 * stroke rather than playing a slide of its own, and where it is let go, and how fast, decide
 * whether it stays open (`drawerSettles`). The stylesheet's slide then carries it the rest
 * of the way from there.
 */

/** A stroke that starts this close to the left edge is the browser's back swipe, not the drawer's. */
export const EDGE_PX = 24;
/** How far a finger travels sideways before the drawer opens or closes. */
export const SWIPE_PX = 56;
/** Movement below this is not a direction yet. */
const SLOP_PX = 10;
/** A flick at least this fast (px per ms) settles the drawer the way it went, wherever it is let go. */
export const FLING_SPEED = 0.35;
/** A finger that rests this long before it lifts has stopped: it lets go with no flick. */
const REST_MS = 100;

export type SwipeVerdict = "open" | "close" | "claim" | "pending" | "ignore";

/**
 * What a stroke from (startX) that has moved (dx, dy) means. "claim" is a recognised
 * swipe still short of SWIPE_PX: the caller keeps it from scrolling what lies under it.
 */
export function swipeVerdict(drawerOpen: boolean, startX: number, dx: number, dy: number): SwipeVerdict {
  if (!drawerOpen && startX <= EDGE_PX) return "ignore";
  if (Math.abs(dx) < SLOP_PX && Math.abs(dy) < SLOP_PX) return "pending";
  if (Math.abs(dy) >= Math.abs(dx)) return "ignore";
  if (!drawerOpen && dx > 0) return dx >= SWIPE_PX ? "open" : "claim";
  if (drawerOpen && dx < 0) return -dx >= SWIPE_PX ? "close" : "claim";
  return "ignore";
}

/**
 * Whether a drawer let go with `shown` px of its `width` on screen, moving at `speed` px/ms
 * (rightward positive), stays open: a flick decides, else whether more than half of it shows.
 */
export function drawerSettles(shown: number, width: number, speed: number): boolean {
  if (speed >= FLING_SPEED) return true;
  if (speed <= -FLING_SPEED) return false;
  return shown > width / 2;
}

/** A dialog, the palette or a sheet is up: a swipe there is theirs, not the drawer's under them. */
export const MODAL = "[aria-modal='true'], dialog[open]";

/** 쓸기를 스스로 쓰는 화면(아이디어 대화 쪽: 오른쪽 쓸기가 노트로 간다, t-1260): 그 안에서 시작한 쓸기는 drawer 것이 아니다 */
function ownSwipe(target: EventTarget | null): boolean {
  const node = target as { closest?: (selector: string) => unknown } | null;
  return typeof node?.closest === "function" && node.closest("[data-drawer-swipe='off']") !== null;
}

/** Something under the finger scrolled sideways, a code block or a table: a swipe right scrolls it back. */
export function scrolledAside(target: EventTarget | null): boolean {
  for (let node = target as HTMLElement | null; node; node = node.parentElement) {
    if (node.scrollLeft > 0 && node.scrollWidth > node.clientWidth) return true;
  }
  return false;
}

/** A finger on a text field edits its text: a sideways stroke there moves the caret or the selection. */
export function editable(target: EventTarget | null): boolean {
  for (let node = target as HTMLElement | null; node; node = node.parentElement) {
    if (node.isContentEditable === true || /^(?:INPUT|TEXTAREA|SELECT)$/.test(node.tagName ?? "")) return true;
  }
  return false;
}

/** Text is selected (the transcript's, a long press in the gutter): a stroke drags its handles. */
export function selecting(): boolean {
  const selection = document.getSelection?.() ?? null;
  return selection !== null && !selection.isCollapsed;
}

/** Listens on the whole document; returns the cleanup. */
export function watchDrawerSwipe(isOpen: () => boolean, setOpen: (open: boolean) => void, internalNavigation: (event: PopStateEvent) => boolean = () => false): (() => void) & { beforeNavigation: () => void } {
  const narrow = window.matchMedia("(max-width: 768px)");
  let start: { x: number; y: number; open: boolean; at: number } | null = null;
  // a recognised swipe owns the stroke until the finger lifts, after the drawer moved too
  let claimed = false;
  let done = false;
  // what the drawer is set to now, and where the finger has it: its width on screen, and how fast it moved
  let state = false;
  let follow: { drawer: HTMLElement; scrim: HTMLElement | null; width: number; shown: number; x: number; at: number; speed: number } | null = null;
  const onStart = (event: TouchEvent): void => {
    const touch = event.touches[0];
    // the last stroke still holds the drawer: a second finger, or its end never came (the element
    // it began on left the page and took its touches with it). It lets go where it was first
    const open = follow !== null ? onEnd() : isOpen();
    start = event.touches.length === 1 && touch && narrow.matches && document.querySelector(MODAL) === null && (open || !scrolledAside(event.target))
      && !editable(event.target) && !selecting() && (open || !ownSwipe(event.target))
      ? { x: touch.clientX, y: touch.clientY, open, at: event.timeStamp } : null;
    claimed = false;
    done = false;
    state = open;
    follow = null;
    drawerTrace("swipe-start", { eligible: start !== null, x: touch?.clientX ?? null, y: touch?.clientY ?? null, wasOpen: open });
  };
  const onMove = (event: TouchEvent): void => {
    const touch = event.touches[0];
    if (start === null || !touch) return;
    // 가장자리 누름·세로 스크롤은 메뉴를 유지한다. 오른쪽 쓸기가 확인될 때만
    // 뒤로 미리보기에 대비해 닫고, 몸짓 자체는 브라우저에 그대로 둔다.
    if (start.x <= EDGE_PX) {
      const dx = touch.clientX - start.x;
      const dy = touch.clientY - start.y;
      if (Math.abs(dx) < SLOP_PX && Math.abs(dy) < SLOP_PX) return;
      if (narrow.matches && start.open && dx >= SLOP_PX && dx > Math.abs(dy)) cancel("edge-swipe");
      else onEnd();
      return;
    }
    // 시스템이 소유한 touchmove로 메뉴를 끌거나 preventDefault를 시도하지 않는다.
    if (event.cancelable === false) { cancel("noncancelable-move"); return; }
    // the screen turned wide mid-stroke: there is no drawer to swipe any more
    if (!narrow.matches) { onEnd(); return; }
    // a selection begun mid-stroke is the finger's: until the drawer moved, the stroke goes back
    if (!done && selecting()) { onEnd(); return; }
    const verdict = done ? "claim" : swipeVerdict(start.open, start.x, touch.clientX - start.x, touch.clientY - start.y);
    drawerTrace("swipe-move", { verdict, dx: touch.clientX - start.x, dy: touch.clientY - start.y });
    // a stroke that was already the drawer's stays so when it comes back near where it started;
    // one that never was waits for a direction. Until the drawer moves, a stroke that turns to
    // scrolling goes back to the page, so the terminal's one-finger scroll is not held up
    if (!claimed && verdict === "pending") return;
    if (verdict === "ignore") { onEnd(); return; }
    claimed = true;
    // a recognised swipe belongs to the drawer, not to the terminal or list under the finger
    event.preventDefault();
    event.stopPropagation();
    if (!done && (verdict === "open" || verdict === "close")) {
      state = verdict === "open";
      drawerTrace("swipe-set", { value: state, reason: "distance" });
      setOpen(state);
      done = true;
    }
    track(touch.clientX - start.x, touch.clientX, event.timeStamp);
  };
  /** Puts the drawer under the finger, `dx` from where the stroke began. */
  const track = (dx: number, x: number, at: number): void => {
    if (start === null) return;
    if (follow === null) {
      const drawer = document.getElementById?.("workspace-drawer") ?? null;
      if (drawer === null) return;
      const scrim = document.querySelector<HTMLElement>(".scrim");
      // a flick may lift after one move: its speed so far is measured from where the finger came down
      const speed = at > start.at ? (x - start.x) / (at - start.at) : 0;
      follow = { drawer, scrim, width: drawer.getBoundingClientRect().width, shown: 0, x, at, speed };
      // the stroke draws the drawer now: no slide of the stylesheet's, and shown even while it is closing
      drawer.style.transition = "none";
      drawer.style.visibility = "visible";
      if (scrim !== null) scrim.style.transition = "none";
    }
    const shown = Math.min(follow.width, Math.max(0, (start.open ? follow.width : 0) + dx));
    if (at > follow.at) follow.speed = (x - follow.x) / (at - follow.at);
    follow.shown = shown;
    follow.x = x;
    follow.at = at;
    follow.drawer.style.transform = `translateX(${shown - follow.width}px)`;
    if (follow.scrim !== null) follow.scrim.style.opacity = String(shown / Math.max(1, follow.width));
  };
  /** Ends the stroke; returns what the drawer is set to now. */
  const onEnd = (event?: TouchEvent): boolean => {
    const held = follow;
    start = null;
    claimed = false;
    done = false;
    follow = null;
    if (held === null) return state;
    // a finger that rested before it lifted, or a stroke handed back mid-way, flicks nothing
    const rested = event === undefined || event.timeStamp - held.at > REST_MS;
    const open = drawerSettles(held.shown, held.width, rested ? 0 : held.speed);
    drawerTrace("swipe-settle", { value: open, shown: held.shown, speed: held.speed, rested });
    if (open !== state) setOpen(open);
    state = open;
    // the stylesheet's slide takes it from where the finger left it to where it settles
    held.drawer.style.transition = "";
    held.drawer.style.visibility = "";
    held.drawer.style.transform = "";
    if (held.scrim !== null) { held.scrim.style.transition = ""; held.scrim.style.opacity = ""; }
    return open;
  };
  /** 취소·이동은 손을 뗀 것이 아니다. 속도로 열림을 확정하지 않고 늦은 사건도 버린다. */
  const cancel = (reason: string): void => {
    const held = follow;
    start = null; follow = null; claimed = false; done = false; state = false;
    if (held !== null) {
      held.drawer.style.transition = ""; held.drawer.style.visibility = ""; held.drawer.style.transform = "";
      if (held.scrim !== null) { held.scrim.style.transition = ""; held.scrim.style.opacity = ""; }
    }
    drawerTrace("swipe-cancel", { reason, speed: held?.speed ?? 0, shown: held?.shown ?? 0 });
    setOpen(false);
  };
  // 기록·복원 경계에서는 진행 중인 몸짓을 버리고 React 상태로 닫는다.
  // class·layout을 강제해도 Safari의 이전 화면 스냅숏은 바뀌지 않아 제거했다.
  const cancelNavigation = (reason: string): void => { if (narrow.matches) cancel(reason); };
  const onCancel = (): void => { if (start !== null || follow !== null) cancel("touchcancel"); };
  const onPop = (event: PopStateEvent): void => {
    const internal = internalNavigation(event);
    drawerTrace("swipe-popstate", { internal });
    if (!internal) cancelNavigation("popstate");
  };
  const onHide = (): void => cancelNavigation("pagehide");
  const onShow = (event: PageTransitionEvent): void => { if (event.persisted) cancelNavigation("pageshow"); };
  document.addEventListener("touchstart", onStart, { capture: true, passive: true });
  document.addEventListener("touchmove", onMove, { capture: true, passive: false });
  document.addEventListener("touchend", onEnd, { capture: true, passive: true });
  document.addEventListener("touchcancel", onCancel, { capture: true, passive: true });
  window.addEventListener("popstate", onPop);
  window.addEventListener("pagehide", onHide);
  window.addEventListener("pageshow", onShow);
  const cleanup = () => {
    document.removeEventListener("touchstart", onStart, { capture: true });
    document.removeEventListener("touchmove", onMove, { capture: true });
    document.removeEventListener("touchend", onEnd, { capture: true });
    document.removeEventListener("touchcancel", onCancel, { capture: true });
    window.removeEventListener("popstate", onPop);
    window.removeEventListener("pagehide", onHide);
    window.removeEventListener("pageshow", onShow);
    if (follow !== null) cancel("cleanup");
  };
  return Object.assign(cleanup, { beforeNavigation: () => cancelNavigation("history-write") });
}
