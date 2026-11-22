// 다시 연 앱의 첫 화면(t-2150): 마지막으로 확인한 기기면 기기 확인(me) 응답 전에 상태 연결을 열고, 기억한 coordinator pane은
// thread 목록 응답을 기다리지 않고 연다. 두 응답을 route로 붙잡은 채 WebSocket 요청과 대화 DOM이 나오는지 보고, 놓은 뒤에도
// 같은 연결을 쓰는지 본다. 취소된 기기는 기억이 있어도 확인 응답으로 연결 화면이 되고 기억을 지운다. 첫 화면 셸은 service
// worker가 캐시로 답하고 새 셸은 뒤에서 두며, 「앱 다시 열기」는 host의 셸을 여는지 캐시 셸에 심은 표지로 본다. 안전 모드 host면 캐시한 앱이
// host의 셸로 한 번 다시 열어 안전 모드 화면에 닿는다. 고정 sleep은 없다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { aaa, step, webkit, pairPage, inTime, until, root, start, stop } from "../lib/harness.mjs";
import { prepare } from "../lib/chat-retention.mjs";

const IPHONE = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_7 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.7 Mobile/15E148 Safari/604.1";
const chatRows = page => page.locator('.term:not([hidden]) [data-testid="chat"] .chat-transcript > :not(.chat-note):not(.banner)');

/** 붙잡은 응답: 놓기 전까지 route를 끝내지 않는다 */
function holder() {
  const held = [];
  let open = false, release;
  const released = new Promise(resolve => { release = resolve; });
  return {
    held,
    get open() { return open; },
    async hold(route) {
      held.push(route.request().url());
      await released;
      await route.continue();
    },
    release() { open = true; release(); },
  };
}

export default async function () {
  let fixture, safari;
  await step("다시 연 앱: coordinator 대화 준비", async () => { fixture = await prepare(); }, { always: true });
  try {
    await step("다시 연 앱: 기기 확인·thread 목록 응답 전에 상태 연결과 기억한 coordinator 대화", async () => {
      safari = await webkit.launch({ headless: true });
      const { page } = await pairPage(safari, aaa("web", "pair").code, { width: 390, height: 844 }, {
        userAgent: IPHONE, touch: true, serviceWorkers: "block", hash: `#pane=${fixture.coordinator}`,
      });
      await chatRows(page).first().waitFor();
      const ctx = page.context();
      const home = new URL("/", page.url()).href;
      const remembered = await page.evaluate(() => JSON.parse(localStorage.getItem("aaa:signed-in")));
      assert.equal(remembered.role, "controller");
      await page.close();

      const me = holder(), threads = holder();
      await ctx.route("**/api/v1/me", route => route.request().method() === "GET" ? me.hold(route) : route.continue());
      await ctx.route("**/api/v1/call", route => {
        const body = route.request().postDataJSON();
        return body?.op === "threads" && body.open ? threads.hold(route) : route.continue();
      });
      const again = await ctx.newPage();
      const sockets = [];
      again.on("websocket", ws => sockets.push(ws.url()));
      const events = again.waitForEvent("websocket", ws => ws.url().includes("/api/v1/events"));
      // 홈 화면 앱은 주소 없이 start_url로 연다: 고를 것은 기억한 선택이다
      await again.goto(home);
      await inTime(events, 10_000, "기기 확인 응답 전 상태 연결");
      await chatRows(again).first().waitFor();
      assert.equal(me.open, false, "기기 확인 응답을 놓기 전에 열려야 한다");
      assert.equal(threads.open, false, "thread 목록 응답을 놓기 전에 열려야 한다");
      assert.ok(me.held.length >= 1 && threads.held.length >= 1, `붙잡은 요청 me=${me.held.length} threads=${threads.held.length}`);
      assert.equal(new URL(again.url()).hash, `#pane=${encodeURIComponent(fixture.coordinator)}`);

      // 확인 응답이 기억과 같으면 화면을 다시 만들지 않는다: 상태 연결은 하나 그대로다
      me.release();
      threads.release();
      await again.waitForFunction(() => document.querySelector("[data-threads-sync]")?.getAttribute("data-threads-sync") !== "0");
      await chatRows(again).first().waitFor();
      assert.deepEqual(sockets.filter(url => url.includes("/api/v1/events")).length, 1, sockets.join("\n"));
      await ctx.unrouteAll({ behavior: "wait" });
      await again.close();

      // 취소된 기기: 기억이 있어도 확인 응답(401)으로 연결 화면이 되고 기억을 지운다
      aaa("web", "revoke", remembered.device);
      const revoked = await ctx.newPage();
      await revoked.goto(home);
      await revoked.getByLabel("페어링 코드").waitFor();
      assert.equal(await revoked.evaluate(() => localStorage.getItem("aaa:signed-in")), null);
      await revoked.close();
    });
    await step("다시 연 앱: 첫 화면은 캐시한 셸로 바로 열고 새 셸은 뒤에서 두며, 「앱 다시 열기」는 host의 지금 셸을 연다", async () => {
      const { page } = await pairPage(safari, aaa("web", "pair").code, { width: 390, height: 844 }, { userAgent: IPHONE, touch: true });
      const ctx = page.context();
      const home = new URL("/", page.url()).href;
      const shellHas = (p, mark) => p.evaluate(async mark => (await (await caches.match("/"))?.text() ?? "").includes(mark), mark);
      // 캐시한 셸에 표지를 심는다: 표지가 있는 화면이 열리면 worker가 캐시로 답한 것이다
      const markShell = p => p.evaluate(async () => {
        for (const name of await caches.keys()) {
          const cache = await caches.open(name);
          const shell = await cache.match("/");
          if (!shell) continue;
          const html = (await shell.text()).replace("<head>", '<head><meta name="aaa-shell" content="cached">');
          await cache.put("/", new Response(html, { headers: shell.headers }));
        }
      });
      // 셸은 worker가 맡은 화면 이동에서 둔다: worker가 화면을 맡은 뒤 한 번 다시 연다
      await until("service worker가 화면을 맡음", () => page.evaluate(() => !!navigator.serviceWorker.controller));
      await page.reload();
      await until("셸 cache", () => shellHas(page, "/assets/index-"));
      await markShell(page);
      assert.equal(await shellHas(page, "aaa-shell"), true);
      await page.close();

      const reopened = await ctx.newPage();
      await reopened.goto(home);
      assert.equal(await reopened.locator('meta[name="aaa-shell"]').count(), 1, "캐시한 셸로 열려야 한다");
      await reopened.getByTestId("machine-labA").waitFor({ state: "attached" });
      // 그사이 받은 host의 셸이 캐시 자리를 바꾼다(다음 실행은 새 빌드)
      await until("뒤에서 받은 셸", async () => !(await shellHas(reopened, "aaa-shell")) && shellHas(reopened, "/assets/index-"));

      // 배포 뒤 첫 실행: host가 다른 웹을 알리면 줄이 뜨고, 단추는 캐시한 셸이 아니라 host의 셸을 연다.
      // 뒤 갱신이 셸 자리를 바꾼 뒤에 다시 심는다(그 전에 심으면 갱신이 덮는다)
      await markShell(reopened);
      await ctx.routeWebSocket(/\/api\/v1\/events/, ws => {
        const server = ws.connectToServer();
        server.onMessage(raw => ws.send(typeof raw === "string" && raw.includes('"ev":"web"') ? raw.replace(/"build":"[^"]*"/, '"build":"index-deployed"') : raw));
      });
      await reopened.close();
      const stale = await ctx.newPage();
      await stale.goto(`${home}#pane=${encodeURIComponent(fixture.coordinator)}`);
      assert.equal(await stale.locator('meta[name="aaa-shell"]').count(), 1, "배포 뒤 첫 실행은 캐시한 셸");
      const line = stale.getByTestId("web-update");
      await line.waitFor();
      // 뒤 갱신이 끝난 뒤 캐시를 다시 옛 셸로 둔다(새 셸을 아직 받지 못한 경우): 그냥 새로 고치면 이 셸이 다시 열린다
      await until("뒤에서 받은 셸", async () => !(await shellHas(stale, "aaa-shell")) && shellHas(stale, "/assets/index-"));
      await markShell(stale);
      await Promise.all([stale.waitForEvent("load"), line.getByRole("button", { name: "앱 다시 열기" }).click()]);
      assert.equal(await stale.locator('meta[name="aaa-shell"]').count(), 0, "앱 다시 열기는 host의 셸이어야 한다");
      await stale.waitForFunction(() => !new URL(location.href).searchParams.has("fresh"));
      assert.equal(new URL(stale.url()).hash, `#pane=${encodeURIComponent(fixture.coordinator)}`);
      await ctx.unrouteAll({ behavior: "wait" });
      await stale.close();

      // 안전 모드 host(설정을 못 읽음)는 첫 화면 주소에 안전 모드 화면을 준다: 캐시한 셸로 연 앱은 host의 셸로 한 번 다시 열어 그 화면에 닿는다
      const toml = `${root}/c/host.toml`;
      const good = fs.readFileSync(toml, "utf8");
      await stop("host");
      fs.writeFileSync(toml, `${good}\n[[[ 깨진 설정\n`);
      start("host", "host");
      try {
        await until("안전 모드 host", async () => (await fetch(home)).headers.get("x-aaa-mode") !== null);
        const safe = await ctx.newPage();
        await safe.goto(home);
        await safe.waitForFunction(() => document.title === "aaa 안전 모드");
        // 캐시한 앱이 표지를 붙여 다시 연 화면이다(안전 모드 화면은 표지를 지우지 않는다)
        assert.equal(new URL(safe.url()).searchParams.get("fresh"), "1", safe.url());
        await safe.close();
      } finally {
        await stop("host");
        fs.writeFileSync(toml, good);
        start("host", "host");
        await until("host 복귀", () => aaa("status").web_listen);
      }
    });
  } finally {
    await safari?.close();
  }
}
