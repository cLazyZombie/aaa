import { test } from "node:test";
import assert from "node:assert/strict";
import { watchDrawerNavigation } from "../src/herdr/drawerNavigation.ts";

function fixture(run) {
  const saved = { window: globalThis.window, document: globalThis.document };
  const frames = new Map(), listeners = new Map();
  let id = 0, open = true;
  const order = [];
  globalThis.window = {
    requestAnimationFrame(fn) { frames.set(++id, fn); return id; },
    cancelAnimationFrame(id) { frames.delete(id); },
    addEventListener(type, fn) { listeners.set(type, fn); },
    removeEventListener(type) { listeners.delete(type); },
  };
  globalThis.document = { visibilityState: "visible",
    addEventListener(type, fn) { listeners.set(type, fn); },
    removeEventListener(type) { listeners.delete(type); },
  };
  const navigation = watchDrawerNavigation(() => open, () => { order.push("close"); open = false; }, (event) => event.internal === true);
  const frame = () => { const next = [...frames.values()]; frames.clear(); order.push("frame"); next.forEach((fn) => fn()); };
  try { run({ ...navigation, order, frames, listeners, frame, setOpen: (value) => { open = value; } }); }
  finally { navigation.dispose(); assert.equal(frames.size, 0); assert.equal(listeners.size, 0); Object.assign(globalThis, saved); }
}

test("메뉴 닫기와 이전 본문 frame 뒤에만 기록·본문을 함께 바꾼다", () => fixture(({ navigate, order, frame }) => {
  navigate(() => order.push("history+body"));
  assert.deepEqual(order, ["close"]);
  frame(); assert.deepEqual(order, ["close", "frame"]);
  frame(); assert.deepEqual(order, ["close", "frame", "frame", "history+body"]);
}));
test("빠른 재선택은 마지막 목적지 하나만 남긴다", () => fixture(({ navigate, order, frame }) => {
  navigate(() => order.push("old")); frame();
  navigate(() => order.push("new")); frame();
  assert.ok(!order.includes("new")); frame();
  assert.ok(!order.includes("old")); assert.equal(order.at(-1), "new");
}));
for (const event of ["popstate", "hashchange", "pagehide", "visibilitychange"]) {
  test(`${event} 뒤에는 대기하던 선택이 덮어쓰지 않는다`, () => fixture(({ navigate, order, frame, listeners }) => {
    navigate(() => order.push("stale")); frame();
    document.visibilityState = "hidden";
    listeners.get(event)({}); frame();
    assert.ok(!order.includes("stale"));
  }));
}
test("설정 내부 복귀는 선택 대기를 취소하지 않는다", () => fixture(({ navigate, order, frame, listeners }) => {
  navigate(() => order.push("selected")); listeners.get("popstate")({ internal: true });
  frame(); frame(); assert.equal(order.at(-1), "selected");
}));
test("이미 닫힌 메뉴와 넓은 화면은 바로 이동한다", () => fixture(({ navigate, order, setOpen, frames }) => {
  setOpen(false); navigate(() => order.push("selected"));
  assert.deepEqual(order, ["close", "selected"]); assert.equal(frames.size, 0);
}));
test("그리기를 기다리는 사이 메뉴를 다시 열면 선택을 취소한다", () => fixture(({ navigate, order, setOpen, frame }) => {
  navigate(() => order.push("stale")); frame(); setOpen(true); frame(); assert.ok(!order.includes("stale"));
}));
test("정리 때 예약 frame을 취소한다", () => fixture(({ navigate, dispose, frame, order }) => {
  navigate(() => order.push("stale")); dispose(); frame(); frame(); assert.ok(!order.includes("stale"));
}));
