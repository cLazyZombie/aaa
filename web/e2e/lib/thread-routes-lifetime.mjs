// 보류한 실제 응답 두 개가 정리 중 자동 통과·중복 fulfill되지 않아야 한다.
import assert from "node:assert/strict";
import http from "node:http";
import { once } from "node:events";
import { ThreadRoutes } from "./thread-routes.mjs";

export async function threadRoutesLifetime(browser) {
  const hits = [], errors = [], releases = [Promise.withResolvers(), Promise.withResolvers()];
  const fetched = [Promise.withResolvers(), Promise.withResolvers()];
  const server = http.createServer((req, res) => {
    if (!req.url.startsWith("/held/")) return res.end("<!doctype html><p>fixture</p>");
    hits.push(req.url);
    res.end(req.url);
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const context = await browser.newContext();
  const routes = new ThreadRoutes();
  let unregisterStarted = false;
  const unroute = context.unrouteAll.bind(context);
  context.unrouteAll = options => { unregisterStarted = true; return unroute(options); };
  let closing;
  try {
    await context.route("**/held/*", async route => {
      try {
        await routes.run(route, async () => {
          const i = Number(new URL(route.request().url()).pathname.split("/").pop());
          const response = await route.fetch();
          fetched[i].resolve();
          await releases[i].promise;
          await route.fulfill({ response });
        });
      } catch (error) { errors.push(String(error)); }
    });
    const page = await context.newPage();
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    const received = page.evaluate(() => Promise.all([0, 1].map(i => fetch(`/held/${i}`).then(r => r.text()))));
    received.catch(() => {}); // 실패 정리의 page.close가 최초 단언을 가리지 않는다.
    await Promise.all(fetched.map(g => g.promise));
    closing = routes.close(context);
    // 정리 중 새 요청은 보류 handler를 다시 열지 않고 원래 서버로 한 번 통과한다.
    assert.equal(await page.evaluate(() => fetch("/held/2").then(r => r.text())), "/held/2");
    const first = page.waitForEvent("requestfinished", request => request.url().endsWith("/held/0"));
    releases[0].resolve();
    await first;
    assert.equal(unregisterStarted, false, "다른 handler의 마지막 fulfill 전에 등록을 해제했다");
    releases[1].resolve();
    assert.deepEqual(await received, ["/held/0", "/held/1"]);
    await closing;
    assert.equal(unregisterStarted, true);
    assert.deepEqual(errors, []);
    assert.deepEqual(hits.sort(), ["/held/0", "/held/1", "/held/2"], "자동 continue로 실제 HTTP를 중복 전송하지 않는다");
  } finally {
    releases.forEach(g => g.resolve());
    await context.close();
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
    if (errors.length) console.log(errors.join("\n"));
  }
}
