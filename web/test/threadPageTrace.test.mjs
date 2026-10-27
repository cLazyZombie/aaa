import assert from "node:assert/strict";
import { webcrypto } from "node:crypto";
import { EventEmitter } from "node:events";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { it } from "node:test";
import vm from "node:vm";
import { threadPageTrace, threadTraceDocument, traceThreadRequests } from "../e2e/lib/thread-page-trace.mjs";

function documentWorld() {
  const requests = [];
  let clockReads = 0;
  const world = vm.createContext({
    document: {}, crypto: webcrypto, Headers,
    performance: { get timeOrigin() { return 1000 + ++clockReads; } },
    WebSocket: class extends EventTarget {},
    fetch: async (_url, init) => { requests.push(init); },
    addEventListener() {}, console: { debug() {} },
  });
  world.window = world;
  const evaluate = (fn) => vm.runInContext(`(${fn.toString()})()`, world);
  return { world, requests, evaluate, clockReads: () => clockReads };
}

it("문서의 시계 값이 달라도 init·HTTP 헤더·완료 판정은 같은 문서 표지를 쓴다", async () => {
  const page = documentWorld();
  page.evaluate(traceThreadRequests);
  const doc = page.evaluate(threadTraceDocument);
  assert.equal(vm.runInContext("performance.timeOrigin === performance.timeOrigin", page.world), false);
  await vm.runInContext('fetch("/api/v1/call", { body: JSON.stringify({ op: "threads" }) })', page.world);
  assert.equal(page.requests[0].headers.get("x-e2e-document"), doc);
  assert.equal(page.evaluate(threadTraceDocument), doc);
  assert.equal(page.clockReads(), 2, "관측기가 문서 키를 얻으려고 시계를 다시 읽지 않는다");
});

it("같은 Document는 표지를 유지하고 새 Document는 같은 시계에서도 다른 표지를 받는다", () => {
  const page = documentWorld();
  const doc = page.evaluate(threadTraceDocument);
  assert.equal(page.evaluate(threadTraceDocument), doc);
  page.world.document = {};
  assert.notEqual(page.evaluate(threadTraceDocument), doc);
  assert.equal(page.clockReads(), 0);
});

it("완료 대기는 현재 문서의 두 실제 본문을 기다리고 이전 문서와 미완료 본문을 세지 않는다", { timeout: 5000 }, async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "aaa-thread-trace-"));
  try {
    const trace = threadPageTrace(directory, "unit");
    const page = new EventEmitter();
    trace.onPage(page);
    const send = (doc, connection, bytes) => {
      const request = {
        url: () => "http://example.test/api/v1/call", postDataJSON: () => ({ op: "threads" }),
        headers: () => ({ "x-e2e-document": doc, "x-e2e-connection": String(connection), "x-e2e-request": String(connection) }),
      };
      page.emit("request", request);
      page.emit("response", { request: () => request, status: () => 200, body: () => bytes });
      page.emit("requestfinished", request);
    };
    const bytes = Buffer.from(JSON.stringify({ ok: { kind: "threads", threads: [], next_after: null } }));
    let reached = false;
    const completed = trace.completed("current", 2).then(() => { reached = true; });
    send("old", 1, Promise.resolve(bytes));
    send("current", 1, Promise.resolve(bytes));
    await trace.drain();
    assert.equal(reached, false);
    const last = Promise.withResolvers();
    send("current", 2, last.promise);
    await Promise.resolve();
    assert.equal(reached, false, "requestfinished만으로 본문 수집 완료를 세지 않는다");
    last.resolve(bytes);
    await completed;
    await trace.drain();
    const reads = await trace.reads([]);
    assert.equal(reads.filter(read => read.doc === "current" && read.next === null).length, 2);
    const rows = fs.readFileSync(path.join(directory, "thread-pages-unit.jsonl"), "utf8").trim().split("\n").map(JSON.parse);
    assert.deepEqual(rows.find(row => row.event === "completion-wait").collected, {});
    assert.deepEqual(rows.find(row => row.event === "completion-reached").collected, { old: 1, current: 2 });
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});
