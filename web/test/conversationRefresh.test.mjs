import assert from "node:assert/strict";
import { test } from "node:test";
import { ConversationRefresh } from "../src/herdr/conversationRefresh.ts";

test("복귀 읽기 중 같은 기록 알림의 재전달은 두 번째 최신 요청을 예약하지 않는다", { timeout: 5000 }, async t => {
  const previous = globalThis.window, timers = new Map();
  let next = 0, now = 0, reads = 0;
  globalThis.window = {
    setTimeout: (callback, delay) => { const id = ++next; timers.set(id, { callback, delay }); return id; },
    clearTimeout: id => timers.delete(id),
  };
  t.after(() => {
    refresh.stop();
    if (previous === undefined) delete globalThis.window; else globalThis.window = previous;
  });
  t.mock.method(performance, "now", () => now);
  const refresh = new ConversationRefresh();
  const initial = Promise.withResolvers();
  refresh.setRead(() => { reads++; return initial.promise; }, "pane-a");
  refresh.setPushes(true);
  refresh.refresh();
  refresh.invalidate("revision-a");
  initial.resolve(); await initial.promise;
  refresh.stop();

  // 실제 E103의 재구독 순서: 복귀 read 시작 → 같은 signature 재전달 → 2초 경계 → read 완료.
  const returned = Promise.withResolvers();
  refresh.setRead(() => { reads++; return returned.promise; }, "pane-a");
  refresh.refresh();
  refresh.invalidate("revision-a");
  now = 2000;
  returned.resolve(); await returned.promise;
  assert.equal(reads, 2, "처음 1회와 복귀 최신 확인 1회");
  assert.deepEqual([...timers.values()].map(t => t.delay), [10_000], "같은 알림의 trailing read가 없다");

  // 실제 변경과 다른 pane의 같은 signature는 계속 읽어야 한다.
  const changed = Promise.withResolvers();
  refresh.setRead(() => { reads++; return changed.promise; }, "pane-a");
  refresh.invalidate("revision-b");
  assert.equal(reads, 3);
  refresh.invalidate("revision-c");
  refresh.invalidate("revision-c");
  now = 4000;
  changed.resolve(); await changed.promise;
  assert.equal(reads, 4, "진짜 새 변경은 읽는 중에도 다음 최신 확인을 남긴다");
  refresh.stop();
  const different = Promise.withResolvers();
  refresh.setRead(async () => { reads++; different.resolve(); }, "pane-b");
  refresh.invalidate("revision-c");
  await different.promise;
  assert.equal(reads, 5, "다른 pane의 기록 알림을 이전 pane으로 거르지 않는다");
});

test("앞 쪽을 읽은 뒤 숨겨도 최신 확인 타이머가 남지 않는다", async t => {
  const previous = globalThis.window, timers = new Map();
  let next = 0, reads = 0, finish;
  globalThis.window = {
    setTimeout: callback => { const id = ++next; timers.set(id, callback); return id; },
    clearTimeout: id => timers.delete(id),
  };
  t.after(() => { if (previous === undefined) delete globalThis.window; else globalThis.window = previous; });
  const finished = new Promise(resolve => { finish = resolve; });
  const refresh = new ConversationRefresh();
  refresh.setRead(() => { reads++; return finished; });
  refresh.refresh(); finish(); await finished;
  await refresh.page(async () => "앞 쪽 하나");
  await refresh.page(async () => "앞 쪽 둘");
  refresh.stop();
  assert.equal(timers.size, 0, "화면을 떠나면 확인 타이머를 모두 취소한다");
  assert.equal(reads, 1, "앞 쪽을 읽는 것은 새 최신 확인이 아니다");
});
