// 웹 E2E 인증·PWA·앱 아이콘·service worker 알림.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 01-http-pwa(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.

import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import { repo, aaa, until, shot, pairPage, base, step, webPort, browser, webkit } from "../lib/harness.mjs";

export default async function ({ ctl, page }) {
  await step("인증 없는 요청·다른 Origin 거부", async () => {
    const r1 = await fetch(`${base}api/v1/call`, {
      method: "POST",
      headers: { "content-type": "application/json", origin: base.slice(0, -1) },
      body: JSON.stringify({ op: "panes" }),
    });
    if (r1.status !== 401) throw new Error(`인증 없는 call이 ${r1.status}`);
    const r2 = await fetch(`${base}api/v1/pair`, {
      method: "POST",
      headers: { "content-type": "application/json", origin: "https://evil.example" },
      body: JSON.stringify({ code: "000000" }),
    });
    if (r2.status !== 403) throw new Error(`다른 Origin pair가 ${r2.status}`);
    // fetch는 Host 헤더를 바꾸지 못한다(DNS rebinding 흉내)
    const r3 = await new Promise((resolve, reject) =>
      http.get({ host: "127.0.0.1", port: webPort, path: "/", headers: { host: "evil.example" } }, resolve).on("error", reject),
    );
    r3.resume();
    if (r3.statusCode !== 421) throw new Error(`다른 Host가 ${r3.statusCode}`);
  });

  await step("PWA: manifest·아이콘을 인증 없이 받고 브라우저가 읽는다", async () => {
    const m = await fetch(`${base}manifest.webmanifest`);
    if (m.status !== 200 || !m.headers.get("content-type").startsWith("application/manifest+json")) {
      throw new Error(`manifest ${m.status} ${m.headers.get("content-type")}`);
    }
    const man = await m.json();
    if (man.id !== "/" || man.name !== "aaa" || man.display !== "standalone" || man.start_url !== "/") throw new Error(JSON.stringify(man));
    // 참고(herdr-web-ui manifest)처럼 일반·maskable을 192·512 모두(차이 목록 M4)
    const kinds = man.icons.map((i) => `${i.purpose} ${i.sizes}`).sort();
    if (JSON.stringify(kinds) !== JSON.stringify(["any 192x192", "any 512x512", "maskable 192x192", "maskable 512x512"])) throw new Error(`manifest 아이콘 ${kinds}`);
    for (const icon of [...man.icons, { src: "/apple-touch-icon.png" }, { src: "/favicon-32.png" }]) {
      const r = await fetch(new URL(icon.src, base));
      const buf = Buffer.from(await r.arrayBuffer());
      // PNG 서명(89 50 4E 47), 그리고 IHDR의 폭·높이가 적은 크기와 같다
      if (r.status !== 200 || r.headers.get("content-type") !== "image/png" || buf.readUInt32BE(0) !== 0x89504e47) {
        throw new Error(`아이콘 ${icon.src}: ${r.status} ${r.headers.get("content-type")}`);
      }
      if (icon.sizes && icon.sizes !== `${buf.readUInt32BE(16)}x${buf.readUInt32BE(20)}`) throw new Error(`아이콘 ${icon.src} 크기 ${buf.readUInt32BE(16)}x${buf.readUInt32BE(20)} ≠ ${icon.sizes}`);
    }
    // 관례 터치 아이콘 주소는 같은 PNG, 없는 파일은 화면(HTML)이 아니라 404, 확장자 없는 주소만 화면(plan §8.61)
    const touch = Buffer.from(await (await fetch(`${base}apple-touch-icon.png`)).arrayBuffer());
    for (const alias of ["apple-touch-icon-precomposed.png", "apple-touch-icon-180x180.png", "apple-touch-icon-120x120-precomposed.png"]) {
      const r = await fetch(`${base}${alias}`);
      if (r.status !== 200 || r.headers.get("content-type") !== "image/png" || !Buffer.from(await r.arrayBuffer()).equals(touch)) throw new Error(`${alias}: ${r.status} ${r.headers.get("content-type")}`);
    }
    for (const missing of ["favicon.ico", "nope.png", "assets/nope.js"]) {
      const r = await fetch(`${base}${missing}`);
      if (r.status !== 404) throw new Error(`${missing}: ${r.status} ${r.headers.get("content-type")}`);
    }
    const route = await fetch(`${base}thread/t-0001`);
    if (route.status !== 200 || !route.headers.get("content-type").startsWith("text/html")) throw new Error(`화면 경로 ${route.status}`);
    const ctx = await browser.newContext();
    const page = await ctx.newPage();
    await page.goto(base);
    const cdp = await ctx.newCDPSession(page);
    const got = await cdp.send("Page.getAppManifest");
    // 상태 막대 style은 두지 않는다(참고 index.html: 홈 화면 앱이 상태 막대 아래에 그리지 않는다)
    if ((await page.locator('meta[name="apple-mobile-web-app-status-bar-style"]').count()) !== 0) throw new Error("apple-mobile-web-app-status-bar-style meta가 있다");
    if (got.errors.length || !got.url.endsWith("/manifest.webmanifest?v=2")) throw new Error(`${got.url} ${JSON.stringify(got.errors)}`);
    // service worker(§8.26·§8.115)가 이 화면을 맡고, 브라우저가 설치할 수 있다고 본다
    const sw = await fetch(`${base}sw.js`);
    if (sw.status !== 200 || !sw.headers.get("content-type").startsWith("text/javascript")) throw new Error(`sw.js ${sw.status}`);
    if ((await fetch(`${base}offline.html`)).status !== 200) throw new Error("offline.html");
    await until("service worker가 화면을 맡음", () => page.evaluate(() => !!navigator.serviceWorker.controller));
    const inst = await cdp.send("Page.getInstallabilityErrors");
    if (inst.installabilityErrors.length) throw new Error(`설치할 수 없다: ${JSON.stringify(inst.installabilityErrors)}`);
    // 아이콘을 바꾼 판(v=2, F3 a 블록 커서): 모든 아이콘 주소에 v가 붙고 service worker cache 이름도 올라가,
    // 이미 설치된 앱·열린 탭도 옛 파일 대신 새 파일을 받는다
    const links = await page.evaluate(() => [...document.querySelectorAll('link[rel="icon"], link[rel="apple-touch-icon"], link[rel="manifest"]')].map((l) => l.getAttribute("href")));
    const sws = await sw.text();
    const old = [...man.icons.map((i) => i.src), ...links].filter((u) => !u.endsWith("?v=2"));
    if (links.length !== 3 || old.length) throw new Error(`v 없는 아이콘 주소 ${JSON.stringify(old)} (link ${links.length}개)`);
    if (!sws.includes('"aaa-files-v2"') || !sws.includes('"/icon-192.png?v=2"') || !sws.includes('"/favicon-32.png?v=2"')) throw new Error("sw.js 파일 cache 이름·알림 아이콘이 v2가 아니다");
    // 빌드마다 worker가 따로다(herdr-web-ui scripts/service-worker-build.ts, 차이 목록 M5, plan §8.115): 이 빌드의 이름과 파일이 찍혀 있다
    const build = /const BUILD = "([0-9a-f]{12})";/.exec(sws)?.[1];
    const listed = /const BUILD_FILES = (\[[^\]]*\]);/.exec(sws)?.[1];
    if (!build || !listed || !JSON.parse(listed).some((f) => /^\/assets\/index-.*\.js$/.test(f))) throw new Error(`sw.js 빌드 도장 ${build} ${listed}`);
    // 네트워크 먼저인 화면은 받은 셸을 이 빌드의 cache에 둔다. 그 셸이 여는 입구 script·stylesheet도 함께.
    // 첫 방문의 화면 이동은 worker가 맡기 전이라(참고도 같다) worker가 맡은 뒤의 화면 이동부터 둔다
    await page.reload();
    const cacheName = `aaa-web-v3-${build}`;
    await until("셸 cache", () =>
      page.evaluate(async (name) => {
        const c = await caches.open(name);
        const shell = await c.match("/");
        if (!shell) return false;
        const html = await shell.text();
        const starts = [...html.matchAll(/(?:src|href)="(\/assets\/[^"]+\.(?:js|css))"/g)].map((m) => m[1]);
        return starts.length >= 2 && (await Promise.all(starts.map((f) => c.match(f)))).every(Boolean);
      }, cacheName),
    );
    // 끊긴 채 새로 고치면 「연결 없음」 화면이 아니라 캐시한 셸로 앱이 뜬다(연결 화면·오류는 앱이 그린다)
    await ctx.setOffline(true);
    await page.reload();
    // 앱은 host에 닿지 못했다고 말하고(offline.html의 「다시 열기」 링크가 아니라 앱의 카드), 네트워크가 돌아오면 스스로 다시 묻는다
    await page.getByTestId("unreachable").waitFor();
    if (await page.getByRole("link", { name: "다시 열기" }).count()) throw new Error("셸 대신 offline.html이 떴다");
    await shot(page, "pwa-offline-shell");
    await ctx.setOffline(false);
    await page.getByTestId("token-gate").waitFor();
    // 셸을 갖기 전의 처음 실행이 끊겼다면: 미리 둔 offline.html(aaa 것)
    if (!(await page.evaluate(() => caches.open("aaa-files-v2").then((c) => c.match("/offline.html")).then(Boolean)))) throw new Error("offline.html이 미리 놓이지 않았다");
    await ctx.close();
  });

  await step("앱 아이콘(사용자 요청 10-17 「모두 바꿔」): 머리·연결 화면·빈 화면의 표시는 icon.svg(F3 a 블록 커서), 옛 「aaa」 글자 배지 없음", async () => {
    const logos = (p, sel = ".logo") =>
      p.locator(sel).evaluateAll((els) => els.map((e) => `${e.tagName} ${e.getAttribute("src")} ${e.naturalWidth > 0} ${Math.round(e.getBoundingClientRect().width)} ${e.textContent}`));
    const bad = [];
    // 연결(pairing) 화면: 휴대폰
    const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
    const pp = await ctx.newPage();
    await pp.goto(base);
    await pp.getByLabel("페어링 코드").waitFor();
    // 연결 화면은 herdr-web-ui AccessGate(.access-gate-mark, 44px)
    const gate = (p) => logos(p, ".logo, .access-gate-mark");
    await until("연결 화면 아이콘", async () => (await gate(pp)).join() === "IMG /icon.svg true 44 ").catch(async (e) => bad.push(`${e.message}: ${await gate(pp)}`));
    await shot(pp, "phone-pair-icon");
    await ctx.close();
    // iPhone WebKit 390×844(QA t-0337 Q2): 옛 .pair는 입력칸 고유 폭(20px mono·자간)이 grid 항목 카드를 47px 밀어 오른쪽이 잘렸다.
    // AccessGate는 카드 폭이 min(100%, …)이고 입력칸이 100%라 카드·입력칸·단추·설명·틀린 코드 말이 모두 화면 안이다
    const wk = await webkit.launch();
    try {
      const wctx = await wk.newContext({
        viewport: { width: 390, height: 844 },
        isMobile: true,
        hasTouch: true,
        userAgent: "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1",
      });
      const wp = await wctx.newPage();
      await wp.goto(base);
      const input = wp.getByLabel("페어링 코드");
      await input.waitFor();
      await input.fill("000000");
      await wp.getByRole("button", { name: "이 기기 페어링" }).click();
      await wp.getByRole("alert").waitFor();
      const fit = await wp.evaluate(() => {
        const right = (sel) => [...document.querySelectorAll(sel)].map((e) => Math.round(e.getBoundingClientRect().right));
        const card = document.querySelector(".access-gate").getBoundingClientRect();
        return {
          page: document.documentElement.scrollWidth,
          view: window.innerWidth,
          card: [Math.round(card.left), Math.round(card.right)],
          parts: right(".access-gate-input, .access-gate-submit, .access-gate-copy, .access-gate-error"),
        };
      });
      if (fit.page > fit.view || fit.card[0] < 0 || fit.card[1] > fit.view || fit.parts.some((r) => r > fit.card[1]))
        bad.push(`iPhone WebKit 연결 화면이 넘친다: ${JSON.stringify(fit)}`);
      await shot(wp, "phone-webkit-pair-gate");
      await wctx.close();
    } finally {
      await wk.close();
    }
    // 머리(목록 왼쪽 위)와 보일 pane이 없는 빈 화면(주소가 가리킨 pane이 없다. 그 밖에는 첫 화면이 pane을 연다)
    const dp = (await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 760 }, { hash: "#pane=gone" })).page;
    await until("머리·빈 화면 아이콘", async () => (await logos(dp)).join("|") === "IMG /icon.svg true 22 |IMG /icon.svg true 44 ").catch(async (e) => bad.push(`${e.message}: ${await logos(dp)}`));
    if ((await dp.locator(".list-head .logo").getAttribute("alt")) !== "aaa") bad.push("머리 아이콘에 이름(alt)이 없다");
    await shot(dp, "desktop-head-icon");
    await dp.context().close();
    if (bad.length) throw new Error(bad.join("; "));
  });

  await step("service worker 알림(herdr-web-ui sw.js·pwa-notification.test.ts 사례, 차이 목록 N5·N7): 보이는 창이 있으면 소리 없이, 누르면 focus 전에 자리를 알리고, focus가 안 되면 새 창, 마지막 누름만, 파일 탭은 고르지 않음", async () => {
    // 내보내는 sw.js를 그대로 돌린다(가짜 self). 참고 시험과 같은 방법이다.
    const src = fs.readFileSync(path.join(repo, "web/public/sw.js"), "utf8");
    const APP = "https://aaa.example/";
    const deferred = () => {
      let resolve, reject;
      const promise = new Promise((y, n) => ((resolve = y), (reject = n)));
      return { promise, resolve, reject };
    };
    const worker = (windows, matchAll = async () => windows, openWindow = async () => null) => {
      const on = new Map();
      const opened = [];
      const shown = [];
      const self = {
        location: new URL(APP),
        addEventListener: (type, f) => on.set(type, f),
        clients: { matchAll, openWindow: async (u) => (opened.push(u), openWindow(u)), claim: async () => {} },
        registration: { showNotification: async (title, options) => void shown.push({ title, options }) },
        skipWaiting: async () => {},
      };
      const { isAppWindow } = new Function("self", "caches", `${src}\nreturn { isAppWindow };`)(self, {});
      const run = (type, ev) => {
        let done;
        on.get(type)({ ...ev, waitUntil: (p) => (done = p) });
        return done;
      };
      const push = (payload) => run("push", { data: { json: () => payload, text: () => JSON.stringify(payload) } });
      let closed = 0;
      const click = (url = "/#thread=demo%2Ft-0001") => run("notificationclick", { notification: { data: { url }, close: () => closed++ } });
      return { isAppWindow, push, click, opened, shown, closed: () => closed };
    };
    const client = (focus = async () => {}, focused = false, url = APP, visibilityState = "visible") => {
      const got = [];
      return { url, focused, focus, visibilityState, got, postMessage: (m) => got.push(m.hash) };
    };
    const eq = (what, a, b) => {
      if (JSON.stringify(a) !== JSON.stringify(b)) throw new Error(`${what}: ${JSON.stringify(a)} ≠ ${JSON.stringify(b)}`);
    };
    const A = "#thread=demo%2Ft-0001";
    const B = "#thread=demo%2Ft-0002";
    // 앱 창: host API가 연 파일·결과물 탭과 보내는 파일은 아니다
    const { isAppWindow } = worker([]);
    eq(
      "앱 창",
      [APP, `${APP}#pane=p1`, `${APP}thread/t-0001`, `${APP}index.html`, `${APP}api/v1/push`, `${APP}v/cap1/index.html`, `${APP}v/cap1/sub/`, `${APP}icon-192.png`, `${APP}manifest.webmanifest`, `${APP}sw.js`, `${APP}offline.html`, "not a url"].map(isAppWindow),
      [true, true, true, true, false, false, false, false, false, false, false, false],
    );
    // 보이는 창이 있으면 소리·진동 없이, 없으면 다시 울린다. 누를 때 쓸 주소를 단다
    const seen = worker([client()]);
    await seen.push({ title: "demo · 웹", body: "b", url: `/${A}`, tag: "aaa-demo/t-0001" });
    const hidden = worker([client(undefined, false, APP, "hidden")]);
    await hidden.push({ title: "t", body: "b" });
    eq("보는 중 알림", [seen.shown[0].title, seen.shown[0].options.silent, seen.shown[0].options.renotify, seen.shown[0].options.tag, seen.shown[0].options.data.url], ["demo · 웹", true, false, "aaa-demo/t-0001", `/${A}`]);
    eq("안 보는 중 알림", [hidden.shown[0].options.silent, hidden.shown[0].options.renotify, hidden.shown[0].options.tag, hidden.shown[0].options.data.url], [false, true, "aaa", "/"]);
    // focus가 늦어도 먼저 알리고, 끝나면 다시 알린다. 새 창은 열지 않는다
    {
      const d = deferred();
      const t = client(() => d.promise);
      const w = worker([t]);
      const done = w.click();
      await Promise.resolve();
      eq("focus 전에 알림", t.got, [A]);
      d.resolve();
      await done;
      eq("focus 뒤 다시", [t.got, w.opened, w.closed()], [[A, A], [], 1]);
    }
    // focus를 거절하면 그 자리로 새 창을 한 번 연다
    {
      const t = client(async () => {
        throw new Error("NotAllowedError");
      });
      const w = worker([t]);
      await w.click();
      eq("거절 뒤 새 창", [t.got, w.opened], [[A], [`/${A}`]]);
    }
    // focus된 창을 고른다. 창이 없으면 그 자리로 열고, 자리가 없는 알림은 맨 앞으로
    {
      const bg = client();
      const fg = client(undefined, true);
      const w = worker([bg, fg]);
      await w.click();
      eq("focus된 창", [bg.got.length, fg.got.length, w.opened], [0, 2, []]);
      const cold = worker([]);
      await cold.click();
      await cold.click("/");
      eq("창 없음", cold.opened, [`/${A}`, "/"]);
      let focused = 0;
      const g = client(async () => void focused++);
      const gw = worker([g]);
      await gw.click("/");
      eq("자리 없는 알림", [focused, g.got, gw.opened], [1, [], []]);
    }
    // 파일 탭(결과물)은 고르지 않는다: 앱 창이 받고, 앱 창이 없으면 연다
    {
      const file = client(undefined, true, `${APP}v/cap1/index.html`);
      const app = client();
      const w = worker([file, app]);
      await w.click();
      eq("파일 탭", [file.got, app.got.length, w.opened], [[], 2, []]);
      let focused = 0;
      const lone = client(async () => void focused++, true, `${APP}icon-192.png`);
      const cold = worker([lone]);
      await cold.click(`/${B}`);
      eq("파일 탭만", [focused, lone.got, cold.opened], [0, [], [`/${B}`]]);
    }
    // 늦게 끝난 앞 누름의 focus가 새 누름 자리를 바꾸지 못한다
    {
      const d = deferred();
      let n = 0;
      const t = client(() => (++n === 1 ? d.promise : Promise.resolve()));
      const w = worker([t]);
      const first = w.click(`/${A}`);
      await Promise.resolve();
      await w.click(`/${B}`);
      d.resolve();
      await first;
      eq("늦은 focus", [t.got, w.opened], [[A, B, B, B], []]);
    }
    // 밀려난 앞 누름의 focus 거절이 옛 자리로 새 창을 열지 못한다
    {
      const d = deferred();
      let n = 0;
      const t = client(() => (++n === 1 ? d.promise : Promise.resolve()));
      const w = worker([t]);
      const first = w.click(`/${A}`);
      await Promise.resolve();
      await w.click(`/${B}`);
      d.reject(new Error("NotAllowedError"));
      await first;
      eq("밀려난 거절", [t.got, w.opened], [[A, B, B], []]);
    }
    // 창 찾기가 느린 앞 누름은 새 누름 뒤에 옛 자리를 알리지 못한다
    {
      const d = deferred();
      let n = 0;
      const t = client();
      const w = worker([t], async () => {
        if (++n === 1) await d.promise;
        return [t];
      });
      const first = w.click(`/${A}`);
      await w.click(`/${B}`);
      d.resolve();
      await first;
      eq("느린 창 찾기", [t.got, w.opened], [[B, B], []]);
    }
    // 늦게 열린 새 창(거절 뒤·처음 열기)도 마지막 누름 자리를 받는다
    for (const fallback of [false, true]) {
      const opening = deferred();
      const ready = deferred();
      const refused = client(async () => {
        throw new Error("NotAllowedError");
      });
      const newer = client(undefined, true);
      const opened = client();
      let n = 0;
      const w = worker(
        [],
        async () => (++n > 1 ? [newer] : fallback ? [refused] : []),
        async () => {
          opening.resolve();
          await ready.promise;
          return opened;
        },
      );
      const first = w.click(`/${A}`);
      await opening.promise;
      await w.click(`/${B}`);
      ready.resolve();
      await first;
      eq(`늦게 열린 창(${fallback ? "거절 뒤" : "처음"})`, [opened.got, newer.got, w.opened], [[B], [B, B], [`/${A}`]]);
    }
  });
}
