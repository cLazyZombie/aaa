import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import { setAppBadge } from "../src/appBadge.ts";

test("host 수 그대로 표시하고 0은 지우며, 미지원·거부·잘못된 수는 화면을 막지 않는다", async () => {
  const saved = Object.getOwnPropertyDescriptor(globalThis, "navigator");
  const calls = [];
  try {
    Object.defineProperty(globalThis, "navigator", { configurable: true, value: {
      setAppBadge: async (n) => calls.push(n), clearAppBadge: async () => calls.push(0),
    } });
    for (const n of [1, 120, 0, -1, NaN, 0.5]) await setAppBadge(n);
    assert.deepEqual(calls, [1, 120, 0]);
    Object.defineProperty(globalThis, "navigator", { configurable: true, value: {} });
    await setAppBadge(1);
    await setAppBadge(0);
    Object.defineProperty(globalThis, "navigator", { configurable: true, value: {
      setAppBadge: async () => { throw Error("denied"); }, clearAppBadge: async () => { throw Error("denied"); },
    } });
    await setAppBadge(1);
    await setAppBadge(0);
  } finally {
    if (saved) Object.defineProperty(globalThis, "navigator", saved);
    else delete globalThis.navigator;
  }
});

const source = readFileSync(new URL("../public/sw.js", import.meta.url), "utf8");
async function push({ payload = { app_badge: 5 }, latest = 2, status = 200, offline = false, supported = true, denied = false } = {}) {
  const events = new Map(), badges = [], notifications = [], requests = [];
  const context = vm.createContext({
    URL, console,
    fetch: async (url, opts) => {
      requests.push({ url, opts });
      if (offline) throw Error("offline");
      return { ok: status === 200, status, json: async () => ({ ok: latest }) };
    },
    self: {
      navigator: supported ? {
        setAppBadge: async (n) => { badges.push(n); if (denied) throw Error("denied"); },
        clearAppBadge: async () => badges.push(0),
      } : {},
      addEventListener: (name, fn) => events.set(name, fn),
      clients: { matchAll: async () => [] },
      registration: { showNotification: async (...args) => notifications.push(args) },
    },
  });
  vm.runInContext(source, context);
  let done;
  events.get("push")({ data: { json: () => payload }, waitUntil: (p) => { done = p; } });
  await done;
  return { badges, notifications, requests };
}

test("창 없는 push는 host 최신 수를 쓰고 offline이면 payload, 0·로그아웃은 지운다", async () => {
  for (const [opts, expected] of [
    [{}, [2]], [{ latest: 0 }, [0]], [{ status: 401 }, [0]],
    [{ offline: true }, [5]], [{ offline: true, payload: {} }, []],
    [{ latest: -1 }, []], [{ supported: false }, []], [{ denied: true }, [2]],
  ]) {
    const result = await push(opts);
    assert.deepEqual(result.badges, expected);
    assert.equal(result.notifications.length, 1, "배지 실패·미지원이어도 알림");
    if (opts.supported !== false) {
      assert.equal(result.requests[0].url, "/api/v1/app-badge");
      assert.equal(result.requests[0].opts.cache, "no-store");
      assert.equal(result.requests[0].opts.credentials, "same-origin");
    }
  }
});
