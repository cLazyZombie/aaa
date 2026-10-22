import { test } from "node:test";
import assert from "node:assert/strict";
import { drawerTrace, drawerTraceText, startDrawerTrace } from "../src/herdr/drawerTrace.ts";

test("쓸기 진단은 명시한 탭에서만 수집하고 최근 사건과 총 빈도를 분리한다", () => {
  const names = ["location", "window", "document", "innerWidth", "innerHeight", "history", "MutationObserver"];
  const saved = new Map(names.map((name) => [name, Object.getOwnPropertyDescriptor(globalThis, name)]));
  const handlers = new Map();
  try {
    globalThis.location = { search: "?pair=private-pair&settings=chat", href: "private-url" };
    globalThis.document = { getElementById() { throw new Error("진단 꺼진 탭에서 DOM을 읽었다"); } };
    startDrawerTrace()();
    drawerTrace("disabled");
    globalThis.location.search = "?drawer-debug=1&pair=private-pair";
    globalThis.innerWidth = 390; globalThis.innerHeight = 844;
    globalThis.history = { length: 3, state: { private: "private-state" } };
    globalThis.window = {
      addEventListener(type, fn) { handlers.set(type, fn); },
      removeEventListener(type) { handlers.delete(type); },
    };
    globalThis.document = { documentElement: {}, getElementById: () => ({
      classList: { contains: () => false }, style: { transform: "" },
      getBoundingClientRect: () => ({ right: 0 }), textContent: "private-text",
    }) };
    let disconnected = false;
    globalThis.MutationObserver = class { observe() {} disconnect() { disconnected = true; } };
    const stop = startDrawerTrace();
    for (let i = 0; i < 805; i++) drawerTrace("result:normal");
    const text = drawerTraceText();
    const trace = JSON.parse(text);
    assert.equal(trace.entries.length, 800);
    assert.equal(trace.dropped, 6);
    assert.equal(trace.counts["result:normal"], 805);
    assert.equal(trace.counts.disabled, undefined);
    assert.equal(trace.entries.at(-1).seq, 806);
    assert.doesNotMatch(text, /private-(?:pair|url|state|text)/);
    stop();
    assert.equal(handlers.size, 0);
    assert.equal(disconnected, true);
    drawerTrace("after-stop");
    assert.equal(JSON.parse(drawerTraceText()).counts["after-stop"], undefined);
  } finally {
    for (const [name, descriptor] of saved) {
      if (descriptor) Object.defineProperty(globalThis, name, descriptor); else delete globalThis[name];
    }
  }
});
