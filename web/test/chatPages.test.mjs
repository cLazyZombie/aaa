import assert from "node:assert/strict";
import test from "node:test";
import { mergeChatPage } from "../src/chatPages.ts";

const message = (id, text, pending = false) => ({ id, role: "user", text, pending, receipt: null });
const page = (messages, before = "older", history = "file") => ({ messages, before, history, truncated: before !== null });

test("ID 있는 큐가 흡수되면 옛 pending을 버리고 보통 말 한 개만 남긴다", () => {
  const current = page([message("001", "시작"), message("002:queue", "후속", true)]);
  const incoming = page([message("001", "시작"), message("003", "후속")]);
  const merged = mergeChatPage(current, incoming);
  assert.deepEqual(merged.messages, incoming.messages);
  assert.equal(merged.messages.filter((m) => m.text === "후속").length, 1);
  assert.equal(current.messages[1].pending, true, "이전 snapshot을 바꾸지 않는다");
});

test("같은 본문 일부만 흡수됐을 때 최신 큐만 남기고 본문으로 합치지 않는다", () => {
  const current = page([message("001:queue", "동일 본문", true), message("002:queue", "동일 본문", true)]);
  const incoming = page([message("002:queue", "동일 본문", true), message("003", "동일 본문")]);
  const merged = mergeChatPage(current, incoming);
  assert.deepEqual(merged.messages, [incoming.messages[1], incoming.messages[0]]);
  assert.equal(merged.messages.filter((m) => m.pending).length, 1);
  assert.equal(merged.before, "older");
});

test("늦은 앞 페이지는 현재 큐를 보존하고 최신 응답은 큐 제거를 반영한다", () => {
  const current = page([message("003", "현재 기록"), message("004:queue", "현재 큐", true)]);
  const earlier = page([message("001", "앞 기록"), message("002:queue", "이미 소비한 큐", true)], null);
  const merged = mergeChatPage(current, earlier, true);
  assert.deepEqual(merged.messages, [earlier.messages[0], current.messages[0], current.messages[1]]);
  assert.equal(merged.before, null);
  const settled = message("005", "현재 큐");
  const latest = mergeChatPage(merged, page([settled]));
  assert.deepEqual(latest.messages, [earlier.messages[0], current.messages[0], settled]);
  assert.equal(latest.before, null, "이미 도달한 처음 cursor는 되돌리지 않는다");
});

test("ID 없는 옛 큐도 최신 응답만 쓰고 파일 교체 때 이전 기록을 버린다", () => {
  const current = page([message("001", "기록"), message(null, "옛 큐", true)]);
  const incoming = page([message("002", "옛 큐")]);
  assert.deepEqual(mergeChatPage(current, incoming).messages, [current.messages[0], incoming.messages[0]]);
  const changed = page([message("001:queue", "다른 파일 큐", true)], null, "other");
  assert.deepEqual(mergeChatPage(current, changed), changed);
});
