import assert from "node:assert/strict";
import { test } from "node:test";

test("저장소가 막혀도 방문 순서를 유지하고 다른 창의 기록·삭제를 반영한다", async () => {
  let storage;
  const savedWindow = globalThis.window;
  globalThis.window = {
    localStorage: { getItem() { throw new Error("시험: 읽기 거부"); }, setItem() { throw new Error("시험: 쓰기 거부"); } },
    addEventListener(type, listener) { if (type === "storage") storage = listener; },
  };
  try {
    const { currentRecent, visitScreen } = await import("../src/recentHistory.ts");
    visitScreen("pane:a"); visitScreen("pane:b"); visitScreen("pane:a");
    assert.deepEqual(currentRecent(), ["pane:a", "pane:b"]);
    const snapshot = currentRecent();
    visitScreen("pane:a");
    assert.equal(currentRecent(), snapshot);
    storage({ key: "aaa:palette-recent", newValue: '["pane:c","pane:b"]' });
    assert.deepEqual(currentRecent(), ["pane:c", "pane:b"]);
    storage({ key: "aaa:unrelated", newValue: '[]' });
    assert.deepEqual(currentRecent(), ["pane:c", "pane:b"]);
    storage({ key: null, newValue: null });
    assert.deepEqual(currentRecent(), []);
  } finally {
    if (savedWindow === undefined) delete globalThis.window;
    else globalThis.window = savedWindow;
  }
});
