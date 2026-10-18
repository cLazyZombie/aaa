// 글 파일 앞부분 읽기(src/herdr/filePreviewText.ts). herdr-web-ui `src/lib/filePreviewText.test.ts`(aa624cb)를 node:test로
// 옮겼다: bun:test → node:test와 bunExpect.mjs, 형 표기를 지움. 시한 셋은 참고가 진짜 10ms timer와 겨루던 것을 node:test 흉내
// timer로 바꿨다: 시한은 몸이 멈춘 것(읽는 쪽이 다음 조각을 기다림)을 본 뒤에 test가 돌린다(docs/testing.md 「사건으로 끝낸다」).
import { mock, test } from "node:test";

import { expect } from "./bunExpect.mjs";
import { readFilePreviewText, SOURCE_PREVIEW_BYTES, SOURCE_PREVIEW_LINES } from "../src/herdr/filePreviewText.ts";

const encoder = new TextEncoder();
const chunked = (chunks, onCancel) => new Response(new ReadableStream({
  start(controller) { for (const chunk of chunks) controller.enqueue(chunk); controller.close(); },
  cancel() { onCancel?.(); },
}));
/** `chunk`를 보낸 뒤 멈추는 몸: 읽는 쪽이 다음 조각을 물으면 `waiting`이 풀린다 */
const stalledAfter = (chunk, init) => {
  let asked;
  const waiting = new Promise((resolve) => { asked = resolve; });
  let sent = false;
  const response = new Response(new ReadableStream({
    pull(controller) { if (sent) asked(); else { sent = true; controller.enqueue(chunk); } },
  }, { highWaterMark: 0 }), init);
  return { response, waiting };
};
/** 시한(setTimeout)을 test가 돌린다 */
const withMockedDeadline = async (run) => {
  mock.timers.enable({ apis: ["setTimeout"] });
  try { await run(); } finally { mock.timers.reset(); }
};

test("an HTTP error never becomes preview text, and its body is cancelled", async () => {
  let cancelled = false;
  const response = new Response(new ReadableStream({ cancel() { cancelled = true; } }), { status: 401 });
  await expect(readFilePreviewText(response, 10)).rejects.toThrow("could not be opened");
  expect(cancelled).toBe(true);
});

test("a peer ignoring Range cannot grow the preview beyond the byte budget", async () => {
  let cancelled = false;
  const response = new Response(new ReadableStream({ start(controller) { controller.enqueue(new TextEncoder().encode("abcdef")); }, cancel() { cancelled = true; } }));
  expect(await readFilePreviewText(response, 4)).toEqual({ text: "abcd", truncated: true });
  expect(cancelled).toBe(true);
});

test("UTF-8 splits survive and a budget does not invent a partial final character", async () => {
  const encoded = new TextEncoder().encode("한글");
  const response = () => new Response(new ReadableStream({ start(controller) {
    controller.enqueue(encoded.slice(0, 2)); controller.enqueue(encoded.slice(2)); controller.close();
  } }));
  expect(await readFilePreviewText(response(), 10)).toEqual({ text: "한글", truncated: false });
  expect(await readFilePreviewText(response(), 4)).toEqual({ text: "한", truncated: true });
});

test("cancellation and a stalled body deadline release the reader even if cancellation stalls", async () => {
  let cancelled = 0;
  const response = () => new Response(new ReadableStream({ cancel() { cancelled++; return new Promise(() => {}); } }));
  await withMockedDeadline(async () => {
    // the deadline is armed before the first read: nothing has arrived, and the body never sends
    const timed = readFilePreviewText(response(), 10, { timeoutMs: 10 });
    mock.timers.tick(10);
    await expect(timed).rejects.toThrow("timed out");
  });
  const abort = new AbortController();
  const pending = readFilePreviewText(response(), 10, { signal: abort.signal });
  abort.abort();
  await expect(pending).rejects.toThrow("aborted");
  expect(cancelled).toBe(2);
});

test("partialOnTimeout keeps what arrived before the deadline as a cut prefix", async () => {
  const init = { status: 206, headers: { "content-range": "bytes 0-99/1000" } };
  await withMockedDeadline(async () => {
    const stalled = stalledAfter(encoder.encode("one\ntwo\nthr"), init);
    const read = readFilePreviewText(stalled.response, 100, { timeoutMs: 10, maxLines: 10, partialOnTimeout: true });
    await stalled.waiting;
    mock.timers.tick(10);
    expect(await read).toEqual({ text: "one\ntwo\n", truncated: true, truncatedBy: "time", total: 1000, lines: 2 });
    // nothing arrived: still an error, and without the opt-in the deadline still fails
    const empty = readFilePreviewText(new Response(new ReadableStream()), 100, { timeoutMs: 10, partialOnTimeout: true });
    mock.timers.tick(10);
    await expect(empty).rejects.toThrow("timed out");
    const again = stalledAfter(encoder.encode("one\ntwo\nthr"), init);
    const strict = readFilePreviewText(again.response, 100, { timeoutMs: 10 });
    await again.waiting;
    mock.timers.tick(10);
    await expect(strict).rejects.toThrow("timed out");
  });
});

test("a partial range exposes truncation and rejects malformed range metadata", async () => {
  const partial = new Response("abc", { status: 206, headers: { "content-range": "bytes 0-2/6" } });
  expect(await readFilePreviewText(partial, 10)).toEqual({ text: "abc", truncated: true, total: 6 });
  await expect(readFilePreviewText(new Response("abc", { status: 206 }), 10)).rejects.toThrow("range");
});

test("a 206 cut at the budget names the whole size, and a 200 of exactly the budget is complete", async () => {
  const ranged = new Response("abcd", { status: 206, headers: { "content-range": "bytes 0-3/10" } });
  expect(await readFilePreviewText(ranged, 4)).toEqual({ text: "abcd", truncated: true, total: 10 });
  const exact = new Response(new ReadableStream({ start(controller) { controller.enqueue(new TextEncoder().encode("abcd")); controller.close(); } }));
  expect(await readFilePreviewText(exact, 4)).toEqual({ text: "abcd", truncated: false });
  const longer = new Response(new ReadableStream({ start(controller) { controller.enqueue(new TextEncoder().encode("abcd")); controller.enqueue(new TextEncoder().encode("e")); } }));
  expect(await readFilePreviewText(longer, 4)).toEqual({ text: "abcd", truncated: true });
});

test("an empty range is an empty file, but other 416 responses remain errors", async () => {
  let cancelled = false;
  const empty = new Response(new ReadableStream({ cancel() { cancelled = true; } }), { status: 416, headers: { "content-range": "bytes */0" } });
  expect(await readFilePreviewText(empty, 10))
    .toEqual({ text: "", truncated: false, total: 0 });
  expect(cancelled).toBe(true);
  for (const headers of [new Headers(), new Headers({ "content-range": "bytes */99" })]) {
    await expect(readFilePreviewText(new Response(null, { status: 416, headers }), 10)).rejects.toThrow("could not be opened");
  }
});

test("a partial file ends at its last whole line without trimming a complete file", async () => {
  const part = new Response("a\nb\nhal", { status: 206, headers: { "content-range": "bytes 0-6/100" } });
  expect(await readFilePreviewText(part, 7)).toEqual({ text: "a\nb\n", truncated: true, total: 100 });
  expect(await readFilePreviewText(new Response("a\nb\nhal"), 7)).toEqual({ text: "a\nb\nhal", truncated: false });
  const complete = new Response("one\ntwo", { status: 206, headers: { "content-range": "bytes 0-6/7" } });
  expect(await readFilePreviewText(complete, 10)).toEqual({ text: "one\ntwo", truncated: false, total: 7 });
});

test("a short range retains the body total and drops an incomplete UTF-8 sequence", async () => {
  const part = new Response(new TextEncoder().encode("xä").slice(0, 2), { status: 206, headers: { "content-range": "bytes 0-1/4096" } });
  expect(await readFilePreviewText(part, 10)).toEqual({ text: "x", truncated: true, total: 4096 });
});

test("the source budgets are named and the line budget is opt-in", async () => {
  expect(SOURCE_PREVIEW_BYTES).toBe(8 * 1024 * 1024);
  expect(SOURCE_PREVIEW_LINES).toBe(200_000);
  await expect(readFilePreviewText(new Response("a"), 10, { maxLines: 0 })).rejects.toThrow("Invalid preview limit");
  // no maxLines: the result has no budget fields
  expect(await readFilePreviewText(new Response("a\nb\n"), 10)).toEqual({ text: "a\nb\n", truncated: false });
});

test("line budget edges: exactly N lines is complete, one more is a line truncation", async () => {
  const read = (body, maxLines, size = 2) => {
    const bytes = encoder.encode(body);
    const parts = [];
    for (let at = 0; at < bytes.length; at += size) parts.push(bytes.slice(at, at + size));
    return readFilePreviewText(chunked(parts), 1000, { maxLines });
  };
  expect(await read("a\nb\nc\n", 3)).toEqual({ text: "a\nb\nc\n", truncated: false, lines: 3 });
  expect(await read("a\nb\nc", 3)).toEqual({ text: "a\nb\nc", truncated: false, lines: 3 });
  expect(await read("a\nb\nc\nd", 3)).toEqual({ text: "a\nb\nc\n", truncated: true, truncatedBy: "lines", lines: 3 });
  // a single blank line past the budget is still more source
  expect(await read("a\nb\nc\n\n", 3)).toEqual({ text: "a\nb\nc\n", truncated: true, truncatedBy: "lines", lines: 3 });
  // blank and trailing lines are source lines
  expect(await read("a\n\n\n", 10)).toEqual({ text: "a\n\n\n", truncated: false, lines: 3 });
});

test("byte budget edges report bytes, even with a line budget set", async () => {
  expect(await readFilePreviewText(chunked([encoder.encode("abcd")]), 4, { maxLines: 10 })).toEqual({ text: "abcd", truncated: false, lines: 1 });
  expect(await readFilePreviewText(chunked([encoder.encode("abcde")]), 4, { maxLines: 10 })).toEqual({ text: "abcd", truncated: true, truncatedBy: "bytes", lines: 1 });
  const ranged = new Response("abcd", { status: 206, headers: { "content-range": "bytes 0-3/10" } });
  expect(await readFilePreviewText(ranged, 4, { maxLines: 10 })).toEqual({ text: "abcd", truncated: true, truncatedBy: "bytes", total: 10, lines: 1 });
});

test("a multibyte character split across chunks survives the line budget", async () => {
  const encoded = encoder.encode("한\n글\nx\n");
  // cuts inside both three-byte characters
  const result = await readFilePreviewText(chunked([encoded.slice(0, 2), encoded.slice(2, 6), encoded.slice(6)]), 1000, { maxLines: 2 });
  expect(result).toEqual({ text: "한\n글\n", truncated: true, truncatedBy: "lines", lines: 2 });
});

test("many empty lines stop at the line budget and the excess stream is cancelled", async () => {
  let cancelled = false;
  let pulls = 0;
  const block = encoder.encode("\n".repeat(64 * 1024));
  const endless = new Response(new ReadableStream({
    pull(controller) { pulls++; controller.enqueue(block); },
    cancel() { cancelled = true; },
  }, { highWaterMark: 0 }));
  const result = await readFilePreviewText(endless, SOURCE_PREVIEW_BYTES, { maxLines: SOURCE_PREVIEW_LINES });
  expect(result.truncated).toBe(true);
  expect(result.truncatedBy).toBe("lines");
  expect(result.lines).toBe(SOURCE_PREVIEW_LINES);
  expect(result.text).toBe("\n".repeat(SOURCE_PREVIEW_LINES));
  expect(cancelled).toBe(true);
  expect(pulls).toBeLessThan(10);
});

test("a byte cut keeps an exact prefix of the source", async () => {
  const source = "가\n".repeat(100);
  const bytes = encoder.encode(source);
  const parts = [];
  for (let at = 0; at < bytes.length; at += 7) parts.push(bytes.slice(at, at + 7));
  const result = await readFilePreviewText(chunked(parts), 10, { maxLines: SOURCE_PREVIEW_LINES });
  // 10 bytes cut the third line's character; the last whole line is kept
  expect(result).toEqual({ text: "가\n가\n", truncated: true, truncatedBy: "bytes", lines: 2 });
  expect(source.startsWith(result.text)).toBe(true);
});
