// 일을 worker에 하나씩 맡기는 큐와 강조 결과 기억(src/herdr/offThread.ts·highlightOffThread.ts). herdr-web-ui
// `src/lib/offThread.test.ts`(aa624cb)를 node:test로 옮겼다: bun:test → node:test와 bunExpect.mjs, 형 표기를 지움.
// 「highlightOffThread」 넷은 bun의 진짜 Web Worker를 쓴다. Node에는 없어서 대역(StubWorker: 받은 글을 진짜 worker와 같은
// highlightRuns로 칠해 답하거나, 답을 미루거나, 실패한다)을 두고 같은 것을 본다. 예산(2초)을 넘기는 꼴은 실패로 대신했다(같은
// 길 finish(null, true)). 진짜 worker와 예산은 웹 E2E 15 「파일 보기 문법 강조」가 본다.
import { describe, it } from "node:test";

import { expect } from "./bunExpect.mjs";

import { highlightKnown, highlightOffThread, LinesCache } from "../src/herdr/highlightOffThread.ts";
import { highlightRuns, normalizeCode } from "../src/herdr/highlight.ts";
import { createOffThreadQueue } from "../src/herdr/offThread.ts";

/** A worker that answers only when the test says so, and remembers what it was sent and whether it was ended. */
class FakeWorker {
  onmessage = null;
  onerror = null;
  sent = [];
  terminated = false;
  postMessage(message) { this.sent.push(message); }
  terminate() { this.terminated = true; }
  answer(id, result) {
    this.onmessage?.({ data: { id, result } });
  }
}

/** A queue over fake workers, with every worker it started. */
function fakeQueue(budgetMs = 1_000) {
  const workers = [];
  const run = createOffThreadQueue(() => {
    const worker = new FakeWorker();
    workers.push(worker);
    return worker;
  }, budgetMs);
  return { run, workers };
}

describe("createOffThreadQueue", () => {
  it("starts no worker before the first job, and runs one job at a time", async () => {
    const { run, workers } = fakeQueue();
    expect(workers).toHaveLength(0);
    const a = run("a");
    const b = run("b");
    expect(workers).toHaveLength(1);
    expect(workers[0].sent.map((m) => m.request)).toEqual(["a"]);
    workers[0].answer(workers[0].sent[0].id, "A");
    expect(await a.promise).toBe("A");
    expect(workers[0].sent.map((m) => m.request)).toEqual(["a", "b"]);
    workers[0].answer(workers[0].sent[1].id, "B");
    expect(await b.promise).toBe("B");
  });

  it("ends a worker that runs over its budget and starts a new one for the next job", async () => {
    const { run, workers } = fakeQueue(10);
    const slow = run("slow");
    const next = run("next");
    expect(await slow.promise).toBeNull();
    expect(workers[0].terminated).toBe(true);
    expect(workers).toHaveLength(2);
    expect(workers[1].sent.map((m) => m.request)).toEqual(["next"]);
    // a late answer from the ended worker is not taken for the next job
    workers[0].answer(workers[0].sent[0].id, "late");
    workers[1].answer(workers[1].sent[0].id, "NEXT");
    expect(await next.promise).toBe("NEXT");
  });

  it("drops a cancelled job that has not started, and lets a running one finish", async () => {
    const { run, workers } = fakeQueue();
    const running = run("running");
    const waiting = run("waiting");
    const after = run("after");
    expect(waiting.cancel()).toBe(true);
    expect(running.cancel()).toBe(false);
    expect(await waiting.promise).toBeNull();
    workers[0].answer(workers[0].sent[0].id, "RUNNING");
    expect(await running.promise).toBe("RUNNING");
    expect(workers[0].sent.map((m) => m.request)).toEqual(["running", "after"]);
    workers[0].answer(workers[0].sent[1].id, "AFTER");
    expect(await after.promise).toBe("AFTER");
  });

  it("gives null for a worker that fails, and replaces it", async () => {
    const { run, workers } = fakeQueue();
    const failing = run("x");
    workers[0].onerror?.(new Event("error"));
    expect(await failing.promise).toBeNull();
    expect(workers[0].terminated).toBe(true);
    const next = run("y");
    expect(workers).toHaveLength(2);
    workers[1].answer(workers[1].sent[0].id, "Y");
    expect(await next.promise).toBe("Y");
  });

  it("gives null when no worker can be started", async () => {
    const run = createOffThreadQueue(() => { throw new Error("no workers here"); }, 1_000);
    expect(await run("x").promise).toBeNull();
  });
});

describe("LinesCache", () => {
  const lines = (text) => [[{ text, role: null }]];
  it("keeps texts up to its size in characters, dropping the least recently used", () => {
    const cache = new LinesCache(6);
    cache.set("aaa", "ts", lines("a"));
    cache.set("bbb", "ts", lines("b"));
    expect(cache.get("aaa", "ts")).toEqual(lines("a"));
    cache.set("ccc", "ts", lines("c"));
    expect(cache.get("bbb", "ts")).toBeUndefined();
    expect(cache.get("aaa", "ts")).toEqual(lines("a"));
    expect(cache.get("ccc", "ts")).toEqual(lines("c"));
  });
  it("tells a text by its language too, and remembers one given up on", () => {
    const cache = new LinesCache(100);
    cache.set("x", "ts", null);
    expect(cache.get("x", "ts")).toBeNull();
    expect(cache.get("x", "js")).toBeUndefined();
  });
  it("keeps nothing larger than itself", () => {
    const cache = new LinesCache(2);
    cache.set("abc", "ts", lines("abc"));
    expect(cache.get("abc", "ts")).toBeUndefined();
  });
});

describe("highlightKnown", () => {
  it("highlights short code at once, and asks for the worker past the limit until it answered", () => {
    expect(highlightKnown("const a = 1;", "ts")?.lines[0][0]).toEqual({ text: "const", role: "keyword" });
    expect(highlightKnown(normalizeCode("const b = 2;\n".repeat(400)), "ts")).toBeNull();
  });
});

/** 페이지의 Web Worker 대신(highlight.worker.ts처럼 highlightRuns로 답한다). `mode`: answer 곧 답함, hold 답을 쥠, fail 실패 */
class StubWorker {
  static mode = "answer";
  static made = [];
  onmessage = null;
  onerror = null;
  held = [];
  constructor() {
    StubWorker.made.push(this);
  }
  postMessage({ id, request }) {
    if (StubWorker.mode === "fail") {
      setImmediate(() => this.onerror?.(new Event("error")));
      return;
    }
    const reply = () => this.onmessage?.({ data: { id, result: highlightRuns(request.source, request.language) } });
    if (StubWorker.mode === "hold") this.held.push(reply);
    else setImmediate(reply);
  }
  terminate() {}
}
globalThis.Worker = StubWorker;

describe("highlightOffThread", () => {
  it("highlights in the worker and keeps the result", async () => {
    StubWorker.mode = "answer";
    const source = normalizeCode("const a = 1;\n// done\n".repeat(400));
    const lines = await highlightOffThread(source, "ts").promise;
    expect(lines).not.toBeNull();
    expect(lines[0][0]).toEqual({ text: "const", role: "keyword" });
    expect(lines.length).toBe(801);
    expect(lines.map((line) => line.map((token) => token.text).join("")).join("\n")).toBe("const a = 1;\n// done\n".repeat(400));
    expect(highlightKnown(source, "ts")).toEqual({ lines, tooLong: false });
    expect(highlightKnown(source, "ts").lines).toBe(lines);
  });
  it("remembers giving up on a text whose view left while it ran", async () => {
    // a view that unmounts (or a StrictMode remount) cancels the running job: it still ran, so the
    // next view of the text must not spend another budget on it
    StubWorker.mode = "fail";
    const source = "-".repeat(200_001);
    const job = highlightOffThread(source, "yaml");
    expect(job.cancel()).toBe(false);
    expect(await job.promise).toBeNull();
    expect(highlightKnown(source, "yaml")?.tooLong).toBe(true);
  });
  it("forgets nothing for a text whose job was dropped before it ran", async () => {
    StubWorker.mode = "hold";
    const first = highlightOffThread(normalizeCode("let x = 1;\n".repeat(300)), "ts");
    const source = normalizeCode("let y = 2;\n".repeat(300));
    const dropped = highlightOffThread(source, "ts");
    expect(dropped.cancel()).toBe(true);
    expect(await dropped.promise).toBeNull();
    expect(highlightKnown(source, "ts")).toBeNull();
    StubWorker.made.at(-1).held.shift()();
    await first.promise;
  });
});
