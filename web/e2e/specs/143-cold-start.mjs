// 다시 연 앱의 첫 화면(t-2150): 마지막으로 확인한 기기면 기기 확인(me) 응답 전에 상태 연결을 열고, 기억한 coordinator pane은
// thread 목록 응답을 기다리지 않고 연다. 두 응답을 route로 붙잡은 채 WebSocket 요청과 대화 DOM이 나오는지 보고, 놓은 뒤에도
// 같은 연결을 쓰는지 본다. 취소된 기기는 기억이 있어도 확인 응답으로 연결 화면이 되고 기억을 지운다. 고정 sleep은 없다.
import assert from "node:assert/strict";
import { aaa, step, webkit, pairPage, inTime } from "../lib/harness.mjs";
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
      const start = new URL("/", page.url()).href;
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
      await again.goto(start);
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
      await revoked.goto(start);
      await revoked.getByLabel("페어링 코드").waitFor();
      assert.equal(await revoked.evaluate(() => localStorage.getItem("aaa:signed-in")), null);
      await revoked.close();
    });
  } finally {
    await safari?.close();
  }
}
