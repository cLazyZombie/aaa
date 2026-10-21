import { test } from "node:test";
import assert from "node:assert/strict";

for (const event of ["beforeunload", "pagehide"]) {
  test(`${event}부터 일반 요청을 취소하고 명시 keepalive·pageshow의 새 요청을 유지한다`, async () => {
    const oldWindow = globalThis.window;
    const oldFetch = globalThis.fetch;
    const calls = [];
    globalThis.window = new EventTarget();
    globalThis.fetch = async (...args) => {
      calls.push(args);
      return new Response("{}");
    };
    try {
      const { apiFetch } = await import(`../src/apiRequest.ts?lifecycle=${event}`);
      await apiFetch("/api/v1/call");
      window.dispatchEvent(new Event(event));
      await assert.rejects(apiFetch("/api/v1/call"), { name: "AbortError" });
      await apiFetch("/api/v1/call", { keepalive: true });
      assert.equal(calls.length, 2, "취소한 요청은 fetch 자체를 호출하지 않는다");
      window.dispatchEvent(new Event("pageshow"));
      assert.equal(calls.length, 2, "복귀는 취소한 요청을 재생하지 않는다");
      await apiFetch("/api/v1/call");
      assert.equal(calls.length, 3, "복귀 뒤 새 요청");
    } finally {
      if (oldWindow === undefined) delete globalThis.window;
      else globalThis.window = oldWindow;
      globalThis.fetch = oldFetch;
    }
  });
}
