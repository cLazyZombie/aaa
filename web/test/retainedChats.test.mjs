import assert from "node:assert/strict";
import { test } from "node:test";
import { CHAT_BYTES, CHAT_IDLE_MS, chatExpiry, chatKey, retainChats } from "../src/retainedChats.ts";
const pane = (id, session = id, harness = "claude") => ({ id, machine_id: "mac-a", agent: { harness, harness_session: session } });
const panes = ["a", "b", "c", "d"].map(id => pane(id));
const key = id => `mac-a:${id}`;
const visit = (entries, id, at = 0, live = panes) => retainChats(entries, live, id && key(id), at);
test("세 대화 LRU·화면 이탈·가장 가까운 숨김 만료", () => {
  let entries = visit([], "a");
  assert.equal(visit(entries, "a"), entries);
  entries = visit(entries, "b", 10); entries = visit(entries, "c", 20);
  entries = visit(entries, "a", 30); entries = visit(entries, "d", 40);
  assert.deepEqual(entries.map(e => e.key), [key("d"), key("a"), key("c")]);
  assert.equal(chatExpiry(entries), CHAT_IDLE_MS + 30);
  entries = visit(entries, null, 50);
  assert.equal(entries[0].hiddenAt, 50);
  entries = visit(entries, null, CHAT_IDLE_MS + 30);
  assert.deepEqual(entries.map(e => e.key), [key("d"), key("a")]);
});
test("UTF-8 데이터 예산은 비활성 LRU부터 제거하고 활성 단독 초과는 떠날 때 버린다", () => {
  let entries = visit(visit(visit([], "a"), "b", 1), "c", 2).map(e => ({ ...e, bytes: CHAT_BYTES / 2 }));
  entries = visit(entries, "c", 3);
  assert.deepEqual(entries.map(e => e.key), [key("c"), key("b")]);
  entries = entries.map(e => ({ ...e, bytes: e.key === key("c") ? CHAT_BYTES + 1 : e.bytes }));
  entries = visit(entries, "c", 4); assert.equal(entries.length, 1);
  entries = visit(entries, null, 5); assert.equal(entries.length, 0);
});
test("삭제·확인된 session 교체·chat 없는 pane은 보관하지 않는다", () => {
  const held = visit([], "a");
  assert.deepEqual(visit(held, null, 1, []), []);
  assert.deepEqual(visit(held, null, 1, [pane("a", "new")]), []);
  assert.deepEqual(visit(held, null, 1, [pane("a", "a", "shell")]), []);
  const unknown = visit(held, null, 1, [pane("a", null)]);
  assert.equal(unknown[0].session, "a");
  assert.equal(chatExpiry(visit([], "a")), null);
  assert.equal(chatKey(panes[0]), key("a"));
});
