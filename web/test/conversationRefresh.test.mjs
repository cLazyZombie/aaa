import assert from "node:assert/strict";
import { test } from "node:test";
import { ConversationRefresh } from "../src/herdr/conversationRefresh.ts";

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
