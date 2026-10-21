// 휴대폰 목록 drawer 쓸기(web/src/herdr/edgeSwipe.ts). herdr-web-ui(aa624cb) src/lib/edgeSwipe.test.ts를 node:test로
// 옮겼다(bun:test의 describe·test·expect를 아래 작은 expect로, 고지는 web/src/herdr/README.md). 여는 쓸기는 왼쪽 끝이 아니라 화면 안쪽에서
// 시작한다(plan §8.397): 닫힌 drawer를 여는 사례는 x 40부터, 왼쪽 끝 쓸기는 브라우저 뒤로 몫이다.
import { afterEach, beforeEach, describe, test } from "node:test";
import assert from "node:assert/strict";
import { drawerSettles, EDGE_PX, FLING_SPEED, SWIPE_PX, swipeVerdict, watchDrawerSwipe } from "../src/herdr/edgeSwipe.ts";

const expect = (actual) => ({
  toBe: (want) => assert.equal(actual, want),
  toEqual: (want) => assert.deepEqual(actual, want),
  toContain: (want) => assert.ok(actual.includes(want), `${JSON.stringify(actual)}에 ${JSON.stringify(want)}이 없다`),
});

describe("swipeVerdict", () => {
  test("a swipe to the right from inside the screen opens the closed drawer", () => {
    expect(swipeVerdict(false, EDGE_PX + 1, 20, 2)).toBe("claim");
    expect(swipeVerdict(false, EDGE_PX + 1, SWIPE_PX, 5)).toBe("open");
    expect(swipeVerdict(false, 200, SWIPE_PX, 5)).toBe("open");
  });

  test("a stroke from the left edge is the browser's back swipe, not the drawer's", () => {
    expect(swipeVerdict(false, 4, SWIPE_PX * 2, 0)).toBe("ignore");
    expect(swipeVerdict(false, EDGE_PX, SWIPE_PX * 2, 0)).toBe("ignore");
  });

  test("a vertical stroke still scrolls", () => {
    expect(swipeVerdict(false, 40, 12, 40)).toBe("ignore");
  });

  test("a swipe to the left over the closed drawer is the page's", () => {
    expect(swipeVerdict(false, 200, -SWIPE_PX, 3)).toBe("ignore");
  });

  test("a tiny movement is not a direction yet", () => {
    expect(swipeVerdict(false, 40, 3, 2)).toBe("pending");
  });

  test("a swipe to the left anywhere closes the open drawer", () => {
    expect(swipeVerdict(true, 200, -20, 3)).toBe("claim");
    expect(swipeVerdict(true, 200, -SWIPE_PX, 3)).toBe("close");
    expect(swipeVerdict(true, 200, SWIPE_PX, 3)).toBe("ignore");
  });
});

describe("drawerSettles", () => {
  test("let go slowly, the drawer stays open when more than half of it shows", () => {
    expect(drawerSettles(170, 300, 0)).toBe(true);
    expect(drawerSettles(130, 300, 0)).toBe(false);
  });

  test("a flick settles it the way it went, wherever it is let go", () => {
    expect(drawerSettles(40, 300, FLING_SPEED)).toBe(true);
    expect(drawerSettles(260, 300, -FLING_SPEED)).toBe(false);
  });
});

/**
 * The listener itself, on a stand-in document: a claimed stroke must keep every later move
 * from the page (the terminal's one-finger scroll listens below it) until the finger lifts.
 */
describe("watchDrawerSwipe", () => {
  let listeners;
  let saved;
  let narrow;
  let modal;
  let selection;

  beforeEach(() => {
    listeners = new Map();
    narrow = { matches: true };
    modal = null;
    selection = { isCollapsed: true };
    saved = { window: globalThis["window"], document: globalThis["document"] };
    globalThis["window"] = { matchMedia: () => narrow };
    globalThis["document"] = {
      addEventListener: (type, handler) => listeners.set(type, handler),
      removeEventListener: (type) => listeners.delete(type),
      querySelector: () => modal,
      getSelection: () => selection,
    };
  });
  afterEach(() => {
    globalThis["window"] = saved.window;
    globalThis["document"] = saved.document;
  });

  /** One stroke through the listener; how many moves reached the page, and what the drawer did. */
  const stroke = (points, open = false, target = null, midway) => {
    const calls = [];
    const stop = watchDrawerSwipe(() => open, (next) => calls.push(next));
    let reached = 0;
    const event = (x, y) => {
      let blocked = false;
      return { touches: [{ clientX: x, clientY: y }], preventDefault: () => { blocked = true; }, stopPropagation: () => { blocked = true; }, get blocked() { return blocked; } };
    };
    const [first, ...rest] = points;
    listeners.get("touchstart")({ ...event(first[0], first[1]), target });
    for (const [index, [x, y]] of rest.entries()) {
      if (index === 1) midway?.();
      const move = event(x, y);
      listeners.get("touchmove")(move);
      if (!move.blocked) reached++;
    }
    listeners.get("touchend")({ touches: [] });
    stop();
    return { reached, calls };
  };

  test("a claimed stroke that comes back within the slop keeps every move from the page", () => {
    expect(stroke([[40, 400], [60, 400], [42, 400], [41, 400]])).toEqual({ reached: 0, calls: [] });
  });

  test("a claimed stroke that comes back and goes on opens the drawer once, and keeps its moves", () => {
    expect(stroke([[40, 400], [60, 400], [42, 400], [106, 400], [126, 460], [156, 520]])).toEqual({ reached: 0, calls: [true] });
  });

  test("a stroke from the left edge leaves every move to the page and the browser", () => {
    expect(stroke([[4, 400], [24, 400], [90, 400], [200, 400]])).toEqual({ reached: 3, calls: [] });
  });

  test("a vertical stroke from mid-screen belongs to the page", () => {
    expect(stroke([[200, 300], [200, 330], [200, 360], [200, 400]])).toEqual({ reached: 3, calls: [] });
  });

  test("a swipe to the left closes the open drawer", () => {
    expect(stroke([[300, 400], [270, 400], [200, 400]], true)).toEqual({ reached: 0, calls: [false] });
  });

  test("a stroke that turns to scrolling before the drawer moved goes back to the page", () => {
    expect(stroke([[40, 300], [51, 302], [51, 360], [51, 430]])).toEqual({ reached: 2, calls: [] });
  });

  test("a swipe over a dialog, the palette or a sheet leaves the drawer under it alone", () => {
    modal = {};
    expect(stroke([[40, 400], [60, 400], [126, 400]])).toEqual({ reached: 2, calls: [] });
  });

  test("a swipe on a code block scrolled sideways scrolls it back instead", () => {
    const code = { scrollLeft: 40, scrollWidth: 900, clientWidth: 350, parentElement: null };
    const text = { scrollLeft: 0, scrollWidth: 0, clientWidth: 0, parentElement: code };
    expect(stroke([[40, 400], [60, 400], [126, 400]], false, text)).toEqual({ reached: 2, calls: [] });
  });

  test("a stroke that goes on after the screen turned wide leaves the drawer alone", () => {
    expect(stroke([[40, 400], [60, 400], [126, 400]], false, null, () => { narrow.matches = false; })).toEqual({ reached: 1, calls: [] });
  });

  test("a stroke on a text field edits it, the drawer open or closed", () => {
    const input = { tagName: "TEXTAREA", scrollLeft: 0, scrollWidth: 0, clientWidth: 0, parentElement: null };
    expect(stroke([[40, 400], [60, 400], [126, 400]], false, input)).toEqual({ reached: 2, calls: [] });
    expect(stroke([[300, 400], [270, 400], [200, 400]], true, input)).toEqual({ reached: 2, calls: [] });
    const block = { isContentEditable: true, tagName: "DIV", scrollLeft: 0, scrollWidth: 0, clientWidth: 0, parentElement: null };
    const inner = { tagName: "SPAN", scrollLeft: 0, scrollWidth: 0, clientWidth: 0, parentElement: block };
    expect(stroke([[40, 400], [60, 400], [126, 400]], false, inner)).toEqual({ reached: 2, calls: [] });
  });

  test("a stroke while text is selected drags the selection, not the drawer", () => {
    selection = { isCollapsed: false };
    expect(stroke([[40, 400], [60, 400], [126, 400]])).toEqual({ reached: 2, calls: [] });
    expect(stroke([[300, 400], [270, 400], [200, 400]], true)).toEqual({ reached: 2, calls: [] });
  });

  test("a selection begun mid-stroke, before the drawer moved, hands the stroke back", () => {
    expect(stroke([[40, 400], [60, 400], [76, 400], [126, 400]], false, null, () => { selection = { isCollapsed: false }; })).toEqual({ reached: 2, calls: [] });
  });

  test("the drawer is under the finger while it moves, and settles where it is let go", () => {
    const style = {};
    const drawer = { style, getBoundingClientRect: () => ({ width: 300 }) };
    const doc = globalThis["document"];
    doc["getElementById"] = (id) => (id === "workspace-drawer" ? drawer : null);
    const seen = [];
    const watch = new Proxy(style, { set: (target, key, value) => { target[key] = value; if (key === "transform") seen.push(value); return true; } });
    drawer.style = watch;
    // past the swipe it opens, the finger comes back to a third of the drawer, and is let go slowly: it closes
    const timed = stroke([[40, 400], [60, 400], [136, 400], [136, 400], [136, 400]]);
    expect(seen).toContain("translateX(-204px)");
    expect(timed.calls).toEqual([true, false]);
    expect(style["transform"]).toBe("");
    expect(style["visibility"]).toBe("");
    delete doc["getElementById"];
  });

  test("a stroke whose end never came lets the drawer go at the next touch", () => {
    const drawer = { style: {}, getBoundingClientRect: () => ({ width: 300 }) };
    const scrim = { style: {} };
    const doc = globalThis["document"];
    doc["getElementById"] = (id) => (id === "workspace-drawer" ? drawer : null);
    doc["querySelector"] = (selector) => (selector === ".scrim" ? scrim : null);
    const calls = [];
    const stop = watchDrawerSwipe(() => true, (next) => calls.push(next));
    const event = (x, timeStamp) => ({ touches: [{ clientX: x, clientY: 400 }], target: null, timeStamp, preventDefault: () => {}, stopPropagation: () => {} });
    // a swipe from the scrim closes the drawer, and its touchend never reaches the document
    listeners.get("touchstart")(event(360, 0));
    listeners.get("touchmove")(event(330, 16));
    listeners.get("touchmove")(event(290, 32));
    expect(calls).toEqual([false]);
    expect(drawer.style["transform"]).toBe("translateX(-70px)");
    expect(scrim.style["transition"]).toBe("none");
    // the next touch finds it held, and lets it go where it was: most of it showed, so it stays open
    listeners.get("touchstart")(event(100, 5000));
    expect(calls).toEqual([false, true]);
    expect(drawer.style["transform"]).toBe("");
    expect(drawer.style["transition"]).toBe("");
    expect(drawer.style["visibility"]).toBe("");
    expect(scrim.style["transition"]).toBe("");
    expect(scrim.style["opacity"]).toBe("");
    stop();
    delete doc["getElementById"];
  });

  test("a flick that lifts after one move settles by its speed from the touch down", () => {
    const drawer = { style: {}, getBoundingClientRect: () => ({ width: 300 }) };
    const doc = globalThis["document"];
    doc["getElementById"] = (id) => (id === "workspace-drawer" ? drawer : null);
    const calls = [];
    const stop = watchDrawerSwipe(() => false, (next) => calls.push(next));
    const event = (x, timeStamp) => ({ touches: [{ clientX: x, clientY: 400 }], target: null, timeStamp, preventDefault: () => {}, stopPropagation: () => {} });
    // 36 px of 300 in 20 ms: far short of half, but fast enough to open
    listeners.get("touchstart")(event(40, 0));
    listeners.get("touchmove")(event(76, 20));
    listeners.get("touchend")({ touches: [], timeStamp: 30 });
    expect(calls).toEqual([true]);
    stop();
    delete doc["getElementById"];
  });

  test("a selection after the drawer moved leaves the stroke with it", () => {
    expect(stroke([[40, 400], [106, 400], [126, 400], [136, 400]], false, null, () => { selection = { isCollapsed: false }; })).toEqual({ reached: 0, calls: [true] });
  });
});
