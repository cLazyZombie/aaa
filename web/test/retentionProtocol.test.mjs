// 필요한 이유: 실패 시점의 원문·미응답 호출을 후속 진단/종료 기록과 혼동하거나 잃지 않게 한다.
import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { it } from "node:test";
import { retentionProtocol } from "../e2e/lib/retention-protocol.mjs";
import { retentionFailure } from "../e2e/lib/retention-failure.mjs";

function lab(t) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "aaa-retention-protocol-"));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const forwarded = [];
  const original = (direction, message) => forwarded.push({ direction, message });
  const connection = { _protocolLogger: original };
  const page = session => {
    const value = new EventEmitter();
    value._connection = { toImpl: () => ({ delegate: { _mainFrameSession: { _client: {
      _connection: connection, _sessionId: session,
    } } } }) };
    return value;
  };
  const emit = (direction, message) => connection._protocolLogger(direction, message);
  const read = () => fs.readFileSync(path.join(directory, "retention-protocol.jsonl"), "utf8")
    .trim().split("\n").map(line => JSON.parse(line));
  return { directory, forwarded, original, connection, page, emit, read };
}

it("DOM/Runtime 원문을 송수신별로 보존하고 실패 순간 pending을 진단 전에 저장한다", t => {
  const { directory, forwarded, page, emit, read } = lab(t);
  const p = page("session-a");
  const observer = retentionProtocol(p, directory);
  const context = { engine: "chromium", width: 1280, role: "coordinator", trial: 2, pageNo: 4, phase: "lookup-before-state" };
  observer.at(context);
  const request = { id: 17, sessionId: "session-a", method: "DOM.resolveNode", params: { backendNodeId: 31, executionContextId: 4 } };
  emit("send", request);
  // 원래 호출을 바꾸지 않고 기록에만 독립된 snapshot을 둔다.
  assert.equal(forwarded[0].message, request);
  assert.deepEqual(forwarded[0].message.params, { backendNodeId: 31, executionContextId: 4 });
  request.params.backendNodeId = 99;
  observer.failed();
  const failure = read();
  assert.equal(failure.find(r => r.event === "failure-boundary").pending, 1);
  const pending = failure.find(r => r.event === "pending-at-failure");
  assert.equal(pending.phase, "lookup-before-state");
  assert.deepEqual(pending.protocol.params, { backendNodeId: 31, executionContextId: 4 });
  assert.ok(pending.ms >= 0);
  // 이후 진단·응답·context 소멸은 최초 실패 snapshot을 덮어쓰지 않는다.
  context.phase = "diagnostic";
  const reply = { id: 17, sessionId: "session-a", result: { object: { type: "object", subtype: "node", objectId: "handle-a" } } };
  emit("receive", reply);
  const diagnostic = { id: 18, sessionId: "session-a", method: "Runtime.callFunctionOn", params: { objectId: "handle-a", arguments: [{ value: "() => 1" }] } };
  emit("send", diagnostic);
  const error = { id: 18, sessionId: "session-a", error: { code: -32000, message: "Cannot find context with specified id", data: "context destroyed" } };
  emit("receive", error);
  const lifecycle = { sessionId: "session-a", method: "Runtime.executionContextDestroyed", params: { executionContextId: 4, executionContextUniqueId: "context-a" } };
  emit("receive", lifecycle);
  observer.at(null);
  const rows = read();
  assert.deepEqual(rows.find(r => r.event === "receive").protocol, reply);
  assert.deepEqual(rows.find(r => r.event === "error").protocol, error);
  assert.deepEqual(rows.find(r => r.event === lifecycle.method).protocol, lifecycle);
  assert.equal(rows.find(r => r.event === "receive").phase, "lookup-before-state");
  assert.equal(rows.find(r => r.event === "error").phase, "diagnostic");
  assert.deepEqual(rows.filter(r => r.event === "pending-at-failure"), [pending]);
  p.emit("close");
  assert.equal(read().some(r => r.event === "pending-at-close"), false);
});

it("페이지별 원문을 격리하고 Network·console·관측 밖 호출을 제외하며 공유 logger를 복구한다", t => {
  const { directory, original, connection, forwarded, page, emit, read } = lab(t);
  const a = page("session-a"), b = page("session-b");
  const first = retentionProtocol(a, directory), second = retentionProtocol(b, directory);
  const logger = connection._protocolLogger;
  first.at({ phase: "lookup-before-state", pageNo: 0 });
  second.at({ phase: "count", pageNo: 1 });
  emit("send", { sessionId: "session-a", id: 1, method: "Network.getCookies", params: {} });
  emit("receive", { sessionId: "session-a", id: 1, result: { cookies: [{ value: "excluded-cookie" }] } });
  emit("receive", { sessionId: "session-a", method: "Runtime.consoleAPICalled", params: { args: [{ value: "excluded-console" }] } });
  emit("send", { sessionId: "unobserved", id: 2, method: "Runtime.evaluate", params: { expression: "excluded-session" } });
  const pending = { sessionId: "session-a", id: 3, method: "DOM.describeNode", params: { objectId: "handle-a" } };
  emit("send", pending);
  a.emit("close");
  assert.equal(connection._protocolLogger, logger);
  const close = read().find(r => r.event === "pending-at-close");
  assert.deepEqual(close.protocol, pending);
  assert.equal(close.pageNo, 0);
  emit("send", { sessionId: "session-b", id: 4, method: "Runtime.evaluate", params: { expression: "1+1" } });
  // 이미 전송된 호출의 응답은 관측 단계를 떠난 뒤에도 보존한다.
  second.at(null);
  emit("send", { sessionId: "session-b", id: 5, method: "Runtime.evaluate", params: { expression: "excluded-outside" } });
  emit("receive", { sessionId: "session-b", id: 4, result: { result: { type: "number", value: 2 } } });
  second.failed();
  assert.equal(read().at(-1).pending, 0);
  b.emit("close");
  assert.equal(connection._protocolLogger, original);
  assert.equal(forwarded.length, 8);
  const rows = read();
  assert.equal(rows.find(r => r.event === "receive").session, "session-b");
  assert.equal(rows.filter(r => r.event === "send").length, 2);
  assert.doesNotMatch(JSON.stringify(rows), /excluded-/);
});

it("실제 원문 파일 쓰기 실패를 남기고 finally·close에서 재시도하거나 최초 오류를 덮지 않는다", async t => {
  const { directory, original, connection, page, emit } = lab(t);
  const p = page("session-a"), observer = retentionProtocol(p, directory);
  observer.at({ phase: "lookup-before-state" });
  emit("send", { id: 1, sessionId: "session-a", method: "DOM.resolveNode", params: { backendNodeId: 31 } });
  fs.mkdirSync(path.join(directory, "retention-protocol.jsonl"));
  const writes = t.mock.method(fs, "appendFileSync", fs.appendFileSync);
  const stderr = t.mock.method(console, "error", () => {});
  const error = new Error("original lookup failure"), records = [];
  await assert.rejects(retentionFailure(error, {}, [], row => records.push(row), async () => ({ responsive: true }), () => observer.failed()), e => e === error);
  assert.equal(records[0].original.stack, error.stack);
  assert.match(records[1].protocolError, /EISDIR/);
  observer.at(null);
  p.emit("close");
  assert.equal(writes.mock.callCount(), 1);
  assert.equal(stderr.mock.callCount(), 1);
  assert.equal(connection._protocolLogger, original);
});
