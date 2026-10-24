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

test("coordinator 한 자리와 일반 세 자리는 따로, 만료·용량 예산은 함께", () => {
  const live = [...panes, { ...pane("coord"), role: "coordinator" }, { ...pane("coord2"), role: "coordinator" }];
  const go = (entries, id, at = 0) => visit(entries, id, at, live);
  let entries = go([], "coord");
  for (const id of ["a", "b", "c"]) entries = go(entries, id, 1);
  assert.equal(entries.length, 4);
  entries = go(entries, "d", 2);
  assert.deepEqual(entries.map(e => e.key), [key("d"), key("c"), key("b"), key("coord")]);
  entries = go(entries, "coord2", 3);
  assert.deepEqual(entries.map(e => e.key), [key("coord2"), key("d"), key("c"), key("b")]);
  entries = go(entries, "b", 4);
  entries = go(entries, "b", CHAT_IDLE_MS + 3);
  assert.deepEqual(entries.map(e => e.key), [key("b"), key("coord2")]);
  entries = go(entries, "b", CHAT_IDLE_MS + 4);
  assert.deepEqual(entries.map(e => e.key), [key("b")]);
  entries = go(go([], "coord"), "a", 1).map(e => ({ ...e, bytes: CHAT_BYTES }));
  assert.deepEqual(go(entries, "a", 2).map(e => e.key), [key("a")]);
});
test("일반 셸의 활성 연결도 일반 세 자리 안에 둔다", () => {
  const live = [...panes, { ...pane("coord"), role: "coordinator" }, pane("shell", "shell", "shell")];
  let entries = [];
  for (const id of ["coord", "a", "b", "c", "shell"]) entries = visit(entries, id, 0, live);
  assert.deepEqual(entries.map(e => e.key), [key("c"), key("b"), key("coord")]);
});
