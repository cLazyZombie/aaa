import assert from "node:assert/strict";
import { webcrypto } from "node:crypto";
import { EventEmitter } from "node:events";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { it } from "node:test";
import vm from "node:vm";
import { threadPageTrace, threadTraceDocument, traceThreadRequests, disconnectThreadEvents } from "../e2e/lib/thread-page-trace.mjs";

function documentWorld() {
  const requests = [];
  let clockReads = 0;
  const world = vm.createContext({
    document: new EventTarget(), crypto: webcrypto, Headers, Event, queueMicrotask,
    performance: { get timeOrigin() { return 1000 + ++clockReads; } },
    WebSocket: class extends EventTarget {
      static OPEN = 1;
      readyState = 0;
      closeCalls = 0;
      open() { this.readyState = 1; this.dispatchEvent(new Event("open")); }
      close() { this.closeCalls++; this.readyState = 2; }
      finishClose() { this.readyState = 3; this.dispatchEvent(new Event("close")); }
    },
    fetch: async (_url, init) => { requests.push(init); },
    addEventListener() {}, console: { debug() {} },
  });
  world.window = world;
  const evaluate = (fn, ...args) => vm.runInContext(`(${fn.toString()})(${args.map(a => JSON.stringify(a)).join(",")})`, world);
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

// 응답 헤더 전에 실패한 threads 요청 하나와 그 page. evaluate는 page의 지금 문서 표지를 돌려준다.
function failedBeforeHeaders(doc) {
  const page = new EventEmitter();
  page.current = doc;
  page.evaluate = async () => page.current;
  const request = {
    url: () => "http://example.test/api/v1/call", postDataJSON: () => ({ op: "threads" }),
    headers: () => ({ "x-e2e-document": doc, "x-e2e-connection": "2", "x-e2e-request": "32" }),
    failure: () => ({ errorText: "cancelled" }),
  };
  const traced = row => ({ text: () => "THREAD_TRACE " + JSON.stringify({ connection: 0, ...row }) });
  return { page, request, traced };
}

it("응답 전 실패가 새 문서 표지보다 먼저 와도 실제 문서 교체면 navigation 취소다", async () => {
  // t-1225 착지 1회차 WebKit 기록 순서: 요청 → beforeunload → request-failed(cancelled) → pagehide → 새 문서
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "aaa-thread-trace-"));
  try {
    const trace = threadPageTrace(directory, "unit");
    const { page, request, traced } = failedBeforeHeaders("old");
    trace.onPage(page);
    page.emit("request", request);
    page.emit("console", traced({ doc: "old", event: "beforeunload" }));
    page.emit("requestfailed", request);
    page.emit("console", traced({ doc: "old", event: "pagehide" }));
    page.current = "new";
    page.emit("console", traced({ doc: "new", event: "document" }));
    const reads = await trace.reads([]);
    assert.deepEqual(reads.map(read => [read.doc, read.pages, read.next]), [["old", 0, "aborted-before-body"]]);
    const rows = fs.readFileSync(path.join(directory, "thread-pages-unit.jsonl"), "utf8").trim().split("\n").map(JSON.parse);
    assert.deepEqual(rows.find(row => row.event === "navigation-aborted")?.failed, "cancelled");
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});

it("응답 전 실패 뒤 문서 표지 사건 없이 읽어도 지금 문서가 바뀌었으면 취소, 같은 문서면 실패 그대로다", async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "aaa-thread-trace-"));
  try {
    const replaced = failedBeforeHeaders("old");
    const moved = threadPageTrace(directory, "moved");
    moved.onPage(replaced.page);
    replaced.page.emit("request", replaced.request);
    replaced.page.emit("requestfailed", replaced.request);
    replaced.page.current = "blank";
    assert.equal((await moved.reads([]))[0].next, "aborted-before-body");

    const same = failedBeforeHeaders("current");
    const stayed = threadPageTrace(directory, "stayed");
    stayed.onPage(same.page);
    same.page.emit("request", same.request);
    same.page.emit("requestfailed", same.request);
    await assert.rejects(stayed.reads([]), /HTTP 본문 읽기 실패: cancelled/);

    const closed = failedBeforeHeaders("closing");
    const ended = threadPageTrace(directory, "closed");
    ended.onPage(closed.page);
    closed.page.emit("request", closed.request);
    closed.page.emit("requestfailed", closed.request);
    closed.page.emit("close");
    await assert.rejects(ended.reads([]), /HTTP 본문 읽기 실패: cancelled/, "닫힌 page도 원래 실패 문구로 남긴다");
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});

it("새 문서의 CONNECTING events는 실제 open 뒤에만 닫고 close 사건으로 끝난다", async () => {
  const page = documentWorld();
  page.evaluate(traceThreadRequests);
  const doc = page.evaluate(threadTraceDocument);
  const socket = vm.runInContext('new WebSocket("ws://example.test/api/v1/events")', page.world);
  const disconnected = page.evaluate(disconnectThreadEvents, doc);
  let done = false;
  disconnected.then(() => { done = true; });
  assert.equal(socket.closeCalls, 0);
  socket.addEventListener("open", () => assert.equal(socket.closeCalls, 0, "앱 onopen이 끝나기 전에는 닫지 않는다"));
  socket.open();
  await Promise.resolve();
  assert.equal(socket.closeCalls, 1);
  assert.equal(done, false, "close 메서드 반환은 close 사건이 아니다");
  socket.finishClose();
  await disconnected;
  assert.equal(done, true);
});
