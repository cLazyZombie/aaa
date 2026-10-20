import assert from "node:assert/strict";
import { it } from "node:test";
import { compactionStep, MessageQueueStore, TAB_ID } from "../src/herdr/messageQueue.ts";

const memory = () => {
  const data = new Map();
  return { getItem: (k) => data.get(k) ?? null, setItem: (k, v) => data.set(k, v), removeItem: (k) => data.delete(k) };
};

it("정리 뒤 보낼 글은 넣은 탭과 함께 저장되고, 다시 읽어도 남는다(plan §8.306)", () => {
  const storage = memory();
  const a = new MessageQueueStore(() => storage);
  a.add("p", "보통");
  a.add("p", "정리 뒤", "compaction");
  const read = new MessageQueueStore(() => storage).read("p");
  assert.deepEqual(read.map((m) => [m.text, m.after, m.by]), [["보통", undefined, undefined], ["정리 뒤", "compaction", TAB_ID]]);
});

it("표지 떼기는 정리 뒤 보낼 글을 모두 보통 붙잡은 글로 바꾸고, 넘겨받기는 그 탭의 글만 바꾼다", () => {
  const storage = memory();
  storage.setItem("herdr-web-ui:queue:p", JSON.stringify({ version: 1, messages: [
    { id: "1", text: "닫힌 탭", after: "compaction", by: "gone" },
    { id: "2", text: "다른 탭", after: "compaction", by: "other" },
    { id: "3", text: "모르는 표지", after: "later", by: "gone" },
  ] }));
  const q = new MessageQueueStore(() => storage);
  assert.deepEqual(q.read("p").map((m) => [m.id, m.after, m.by]), [["1", "compaction", "gone"], ["2", "compaction", "other"], ["3", undefined, undefined]]);
  q.adopt("p", "gone", "me");
  assert.deepEqual(new MessageQueueStore(() => storage).read("p").map((m) => m.by), ["me", "other", undefined]);
  q.release("p");
  assert.deepEqual(new MessageQueueStore(() => storage).read("p").map((m) => [m.id, m.after, m.by]), [["1", undefined, undefined], ["2", undefined, undefined], ["3", undefined, undefined]]);
});

it("정리 뒤 차례: 넣은 탭이 첫 글부터 보내고, 남의 글은 넘겨받기를 묻고, 앞에 보통 글이 있거나 붙잡을 설정이면 표지를 뗀다", () => {
  const held = (id, by, after = "compaction") => ({ id, text: id, ...(after ? { after, by } : {}) });
  const none = () => false;
  assert.equal(compactionStep([], none, "me", false), null);
  assert.equal(compactionStep([held("a", undefined, null)], none, "me", false), null, "보통 붙잡은 글은 저절로 가지 않는다");
  assert.deepEqual(compactionStep([held("a", "me"), held("b", "me")], none, "me", false), { send: held("a", "me") });
  assert.equal(compactionStep([held("a", "me"), held("b", "me")], (id) => id === "a", "me", false), null, "앞 글이 가는 중이면 기다린다");
  assert.deepEqual(compactionStep([held("a", "other")], none, "me", false), { adopt: "other" });
  assert.deepEqual(compactionStep([held("a", undefined)], none, "me", false), { adopt: undefined });
  assert.equal(compactionStep([held("x", undefined, null), held("a", "me")], none, "me", false), "release", "앞의 보통 글을 앞지르지 않는다");
  assert.equal(compactionStep([held("a", "me")], none, "me", true), "release", "작업 중이고 자동 전송을 껐다");
});
