// public/sw.js의 화면 이동 답(t-2150): 앱 첫 화면은 캐시한 셸로 바로 열고 새 셸은 뒤에서 둔다. 「앱 다시 열기」(?fresh)·다른 주소·
// 셸이 아닌 캐시는 네트워크 먼저다. 내보내는 sw.js를 가짜 self·caches·fetch로 그대로 돌린다(E2E 01 알림 시험과 같은 방법).
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const ORIGIN = "https://aaa.example.ts.net";
const src = readFileSync(new URL("../public/sw.js", import.meta.url), "utf8");
const shellHtml = (tag) => `<!doctype html><html><head><script type="module" crossorigin src="/assets/index-${tag}.js"></script><link rel="stylesheet" crossorigin href="/assets/index-${tag}.css"></head><body><div id="root"></div></body></html>`;

function basic(body, type) {
  const response = new Response(body, { headers: { "content-type": type } });
  Object.defineProperty(response, "type", { value: "basic" });
  return response;
}

function lab() {
  const stores = new Map();
  const key = (r) => new URL(typeof r === "string" ? r : r.url, ORIGIN).pathname;
  const cache = () => {
    const m = new Map();
    return {
      m,
      async match(r) { return m.get(key(r))?.clone(); },
      async put(r, res) { m.set(key(r), res); },
      async add() {},
      async keys() { return [...m.keys()].map((k) => ({ url: ORIGIN + k })); },
    };
  };
  const caches = {
    async open(name) { if (!stores.has(name)) stores.set(name, cache()); return stores.get(name); },
    async keys() { return [...stores.keys()]; },
    async delete(name) { return stores.delete(name); },
    async match(r) { for (const c of stores.values()) { const v = await c.match(r); if (v) return v; } return undefined; },
  };
  const pending = [];
  // 화면 이동 fetch는 시험이 놓을 때까지 붙잡는다. 빌드 파일은 바로 답한다
  const fetch = (input) => {
    const path = key(input);
    if (path.startsWith("/assets/")) return Promise.resolve(basic("/* file */", path.endsWith(".css") ? "text/css" : "text/javascript"));
    return new Promise((resolve, reject) => pending.push({ path, resolve, reject }));
  };
  const handlers = {};
  const self = { location: { origin: ORIGIN }, addEventListener: (name, fn) => { handlers[name] = fn; }, clients: {}, registration: {} };
  new Function("self", "caches", "fetch", "Response", src)(self, caches, fetch, Response);
  const navigate = (path) => {
    let answer;
    const waits = [];
    handlers.fetch({
      request: { method: "GET", url: ORIGIN + path, mode: "navigate" },
      respondWith: (p) => { answer = Promise.resolve(p); },
      waitUntil: (p) => waits.push(p),
    });
    return { answer, settled: async () => { for (let i = 0; i < waits.length; i++) await waits[i]; } };
  };
  return { caches, stores, pending, navigate };
}

async function seedShell(l, html) {
  const c = await l.caches.open("aaa-web-v3");
  await c.put("/", basic(html, "text/html; charset=utf-8"));
}

test("캐시한 셸이 있으면 네트워크를 기다리지 않고 답하고, 받은 새 셸은 뒤에서 둔다", async () => {
  const l = lab();
  await seedShell(l, shellHtml("old"));
  const nav = l.navigate("/");
  let answered = false;
  void nav.answer.then(() => { answered = true; });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(answered, true, "네트워크 답 전에 캐시한 셸로 답한다");
  assert.equal(await (await nav.answer).text(), shellHtml("old"));
  assert.equal(l.pending.length, 1, "새 셸은 계속 받는다");
  l.pending[0].resolve(basic(shellHtml("new"), "text/html; charset=utf-8"));
  await nav.settled();
  assert.equal(await (await l.caches.match("/")).text(), shellHtml("new"));
  assert.ok(await l.caches.match("/assets/index-new.js"), "새 셸이 여는 파일도 둔다");
});

test("「앱 다시 열기」(?fresh)·다른 화면 이동·셸이 아닌 캐시는 네트워크 답이다", async () => {
  for (const [path, cached] of [["/?fresh=1#pane=p1", shellHtml("old")], ["/f/cap1/note.html", shellHtml("old")], ["/", "<!doctype html><p>file</p>"]]) {
    const l = lab();
    await seedShell(l, cached);
    const nav = l.navigate(path);
    let answered = false;
    void nav.answer.then(() => { answered = true; });
    await new Promise((resolve) => setImmediate(resolve));
    assert.equal(answered, false, `${path}: 네트워크 전에 답하지 않는다`);
    l.pending[0].resolve(basic(path.startsWith("/f/") ? "<p>file view</p>" : shellHtml("new"), "text/html"));
    const body = await (await nav.answer).text();
    await nav.settled();
    assert.notEqual(body, cached, path);
    // 파일 보기의 답은 셸 자리에 두지 않는다(두면 다음 첫 화면이 그것을 연다)
    if (path.startsWith("/f/")) assert.equal(await (await l.caches.match("/")).text(), cached);
  }
});

test("끊겼으면 ?fresh도 캐시한 셸, 셸이 없으면 offline.html", async () => {
  const l = lab();
  await seedShell(l, shellHtml("old"));
  const nav = l.navigate("/?fresh=1");
  l.pending[0].reject(new TypeError("Load failed"));
  assert.equal(await (await nav.answer).text(), shellHtml("old"));

  const empty = lab();
  await (await empty.caches.open("aaa-files-v2")).put("/offline.html", basic("offline", "text/html"));
  const first = empty.navigate("/");
  empty.pending[0].reject(new TypeError("Load failed"));
  assert.equal(await (await first.answer).text(), "offline");
});
