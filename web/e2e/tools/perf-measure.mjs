// 첫 접속과 질문 답의 시간을 실제 host·agent·브라우저로 잰다(t-1767). 시험이 아니다: 단언 없이 숫자를 JSON으로 남긴다.
// 고치기 전후를 같은 자료·같은 순서로 재서 비교한다. 쓰는 법과 읽는 법은 docs/testing-web-response-time-t-1767.md.
//
// 실행(저장소 루트, 웹 빌드와 e2e 실행 파일이 지금 소스로 만들어진 뒤):
//   NODE_PATH=<playwright 1.64.1> AAA_PERF_OUT=<결과.json> node web/e2e/lib/child.mjs web/e2e/tools/perf-measure.mjs
//   AAA_PERF_RUNS=5(기본) 한 조건을 몇 번 재는가(느린 망은 3번까지), AAA_PERF_ANSWERS=5(기본) 엔진마다 답을 몇 번 재는가
//   AAA_PERF_SHOTS=<폴더> 답 직후 화면을 찍는다(「답할 것」 다시 읽기를 붙잡은 채). AAA_BIN=<실행 파일>로 고치기 전 판을 잰다
import fs from "node:fs";
import { createHash } from "node:crypto";
import { aaa, step, browser, webkit, pairPage, domIs, until, root, base, waitOutput, coordinatorPost } from "../lib/harness.mjs";
import { seedHost } from "../lib/bulk-fixture.mjs";

const RUNS = Number(process.env.AAA_PERF_RUNS ?? 5);
const ANSWERS = Number(process.env.AAA_PERF_ANSWERS ?? 5);
const OUT = process.env.AAA_PERF_OUT ?? `${root}/perf.json`;
// 운영 host(11-12)의 규모: thread 1,766개(열린 것 12), 답한 것 1,004건(한 건 중간 2.1KB)
const OLD_THREADS = 1754;
const DONE_ASKS = 1004;
// Chromium DevTools 망 흉내(지연은 요청마다, 속도는 응답 본문의 전송 바이트에 걸린다). WebSocket에는 걸리지 않는다.
const LINKS = {
  loopback: null,
  lte: { latency: 60, downloadThroughput: (10_000_000 / 8) | 0, uploadThroughput: (5_000_000 / 8) | 0 },
  slow: { latency: 150, downloadThroughput: (1_600_000 / 8) | 0, uploadThroughput: (750_000 / 8) | 0 },
};

/** 페이지 안 기록: API 요청(시작·머리·본문 끝·푼 크기·압축), events 연결, 화면 단계, 긴 작업, 답 카드 변화 */
function perfInit() {
  const P = (window.__perf = { calls: [], ws: [], marks: {}, long: [], clicks: [], card: [], threadEvents: [] });
  const mark = (name) => { if (!(name in P.marks)) P.marks[name] = performance.now(); };
  try { performance.setResourceTimingBufferSize(4000); } catch {}
  try {
    new PerformanceObserver((list) => { for (const e of list.getEntries()) P.long.push([Math.round(e.startTime), Math.round(e.duration)]); })
      .observe({ type: "longtask", buffered: true });
  } catch {}
  const realFetch = window.fetch;
  window.fetch = function (input, init) {
    const url = typeof input === "string" ? input : input.url;
    let op = null;
    if (url.endsWith("/api/v1/call")) { try { op = JSON.parse(init.body).op; } catch {} }
    else if (url.includes("/api/v1/")) op = `GET ${url.slice(url.indexOf("/api/v1/") + 8)}`;
    const promise = realFetch.call(this, input, init);
    if (!op) return promise;
    const row = { op, start: performance.now() };
    P.calls.push(row);
    promise.then((res) => {
      row.headers = performance.now();
      row.status = res.status;
      row.encoding = res.headers.get("content-encoding");
      res.clone().arrayBuffer().then((b) => { row.end = performance.now(); row.bytes = b.byteLength; }, () => { row.end = performance.now(); });
    }, () => { row.end = performance.now(); row.failed = true; });
    return promise;
  };
  const WS = window.WebSocket;
  window.WebSocket = class extends WS {
    constructor(...a) {
      super(...a);
      if (!String(a[0]).endsWith("/api/v1/events")) return;
      const row = { created: performance.now(), msgs: 0, chars: 0, first: {} };
      P.ws.push(row);
      this.addEventListener("open", () => { row.open = performance.now(); });
      this.addEventListener("message", (m) => {
        if (typeof m.data !== "string") return;
        row.msgs += 1;
        row.chars += m.data.length;
        const ev = /^\{"ev":"([a-z_]+)"/.exec(m.data)?.[1];
        if (ev && !(ev in row.first)) row.first[ev] = performance.now();
        if (ev === "thread" || ev === "asks") {
          const id = ev === "thread" ? /"id":"(t-\d+)"/.exec(m.data)?.[1] : null;
          P.threadEvents.push({ at: performance.now(), ev, id, answered: ev === "thread" && /"answer":\{/.test(m.data) });
          if (P.threadEvents.length > 400) P.threadEvents.shift();
        }
      });
    }
  };
  let before = "";
  const look = () => {
    const app = document.querySelector(".app");
    if (app) mark("shell");
    if (document.querySelector('[data-testid^="machine-"]')) mark("machines");
    if (app?.dataset.threadsReady === "1") mark("threadsReady");
    if (app && app.dataset.threadsSync !== "0" && app.dataset.threadsSync !== undefined) mark("threadsSync");
    const card = document.querySelector(".thread-ask-card[data-state='open']");
    if (card) mark("askCard");
    const done = document.querySelector('[data-testid="ask-done-card"]');
    const state = { card: card?.dataset.testid ?? null, busy: card?.getAttribute("aria-busy") === "true", done: !!done,
      missing: !!document.querySelector('[data-testid="asks-missing"]'), empty: !!document.querySelector('[data-testid="asks-empty"]') };
    const key = JSON.stringify(state);
    if (key !== before) { before = key; P.card.push({ at: performance.now(), ...state }); if (P.card.length > 400) P.card.shift(); }
  };
  new MutationObserver(look).observe(document, { subtree: true, childList: true, attributes: true });
  document.addEventListener("click", (e) => {
    if (e.target instanceof Element && e.target.closest('[data-testid="thread-ask-send"]')) P.clicks.push({ at: performance.now(), wall: Date.now() });
  }, true);
}

const median = (xs) => { const s = xs.filter((x) => x != null).sort((a, b) => a - b); return s.length ? s[s.length >> 1] : null; };

/** 그 페이지의 기록을 모은다: 화면 단계(ms, 탐색 시작부터), 자산·API 요청 표 */
async function collect(page) {
  return page.evaluate(() => {
    const P = window.__perf;
    const nav = performance.getEntriesByType("navigation")[0];
    const res = performance.getEntriesByType("resource");
    const asset = (r) => ({ name: new URL(r.name).pathname.replace(/-[\w-]{8}\./, "-*."), start: Math.round(r.startTime), end: Math.round(r.responseEnd),
      wire: r.encodedBodySize, raw: r.decodedBodySize, cached: r.transferSize === 0 && r.decodedBodySize > 0 });
    // 같은 주소(/api/v1/call)의 전송 크기는 시작 차례로 맞춘다
    const wires = res.filter((r) => r.name.endsWith("/api/v1/call")).sort((a, b) => a.startTime - b.startTime);
    const posts = P.calls.filter((c) => !c.op.startsWith("GET ")).sort((a, b) => a.start - b.start);
    posts.forEach((c, i) => { c.wire = wires.length === posts.length ? wires[i].encodedBodySize : null; });
    return {
      marks: Object.fromEntries(Object.entries(P.marks).map(([k, v]) => [k, Math.round(v)])),
      nav: nav ? { html: Math.round(nav.responseEnd), dom: Math.round(nav.domContentLoadedEventEnd), load: Math.round(nav.loadEventEnd), wire: nav.encodedBodySize } : null,
      assets: res.filter((r) => /\/assets\/|\.woff2|manifest|icon|favicon/.test(r.name)).map(asset),
      ws: P.ws.map((w) => ({ created: Math.round(w.created), open: Math.round(w.open ?? -1), msgs: w.msgs, chars: w.chars,
        first: Object.fromEntries(Object.entries(w.first).map(([k, v]) => [k, Math.round(v)])) })),
      calls: P.calls.map((c) => ({ op: c.op, start: Math.round(c.start), headers: Math.round(c.headers ?? -1), end: Math.round(c.end ?? -1),
        bytes: c.bytes ?? null, wire: c.wire ?? null, encoding: c.encoding ?? null, status: c.status ?? null })),
      long: P.long,
    };
  });
}

/** 한 조건의 여러 번을 한 줄로: 단계 중간값과 API 합계 */
function summarize(runs) {
  const marks = {};
  for (const name of ["shell", "machines", "threadsReady", "askCard", "threadsSync"]) marks[name] = median(runs.map((r) => r.marks[name]));
  const sum = (r, f) => r.calls.reduce((n, c) => n + (f(c) ?? 0), 0);
  return {
    marks,
    html: median(runs.map((r) => r.nav?.html)),
    eventsOpen: median(runs.map((r) => r.ws[0]?.open)),
    firstAsks: median(runs.map((r) => r.calls.find((c) => c.op === "asks")?.end)),
    settled: median(runs.map((r) => Math.max(...r.calls.map((c) => c.end)))),
    apiCalls: median(runs.map((r) => r.calls.length)),
    apiBytes: median(runs.map((r) => sum(r, (c) => c.bytes))),
    apiWire: median(runs.map((r) => (r.calls.every((c) => c.op.startsWith("GET ") || c.wire != null) ? sum(r, (c) => c.wire) : null))),
    assetWire: median(runs.map((r) => r.assets.reduce((n, a) => n + (a.cached ? 0 : a.wire), 0))),
    longTasks: median(runs.map((r) => r.long.length)),
    longTotal: median(runs.map((r) => r.long.reduce((n, [, d]) => n + d, 0))),
    longMax: median(runs.map((r) => Math.max(0, ...r.long.map(([, d]) => d)))),
  };
}

async function settle(page, wantCard) {
  await domIs(page, "목록 전체와 답할 것 읽기", (wantCard) => {
    const P = window.__perf;
    const app = document.querySelector(".app");
    return app && app.dataset.threadsSync !== "0" && P.calls.some((c) => c.op === "asks" && c.end) && P.calls.every((c) => c.end)
      && (!wantCard || !!document.querySelector(".thread-ask-card[data-state='open']"));
  }, wantCard, 120_000);
  // 마지막 응답을 화면에 반영한 frame까지
  await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));
}

async function question(title) {
  fs.writeFileSync(`${root}/perf-task.md`, "응답 시간 측정\n");
  const t = aaa("thread", "start", "demo", "--title", title, "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/perf-task.md`);
  await until("첫 보고", () => aaa("thread", "show", t.id).report_hash);
  const body = `[결정 필요] 질문 1개: ${title}\n\n## Questions\n### 1. ${title}\n- A 그대로 (추천)\n- B 고친다\n`;
  fs.writeFileSync(`${t.work_dir}/report.md`, body);
  const hash = createHash("sha256").update(body).digest("hex");
  await until("질문 카드", () => { const v = aaa("thread", "show", t.id); return v.report_hash === hash && v.questions; });
  return t;
}

export default async function () {
  const result = { at: new Date().toISOString(), runs: RUNS, answers: ANSWERS, links: LINKS, scale: {}, first: {}, answer: {}, request: {} };
  let asked;

  await step("운영 규모 자료", async () => {
    aaa("project", "create", "demo");
    asked = await question("먼저 답할 질문");
    await seedHost((db) => {
      const original = JSON.parse(db.prepare("SELECT data FROM threads WHERE id=?").get(asked.id).data);
      const put = db.prepare("INSERT INTO threads(project,id,status,data,updated_at) VALUES('demo',?,?,?,1)");
      for (let i = 0; i < OLD_THREADS; i++) {
        const id = `t-${3000 + i}`;
        const record = { ...original, id, title: `이전 작업 ${i} 응답 시간을 재는 자료`, status: "resolved",
          pane: null, questions: [], question_answer: null, question_draft: null, report_hash: null, report_copy_hash: null, acked_hash: null,
          work_dir: `${root}/work/demo/${id}`, cwd: `${root}/work/demo/${id}`, launch: null, resolve_pending: null, park: null,
          next: ["정리: worktree와 branch를 지운다"], error: null, task_clean: { ...original.task_clean, followups: [], followup_revisions: [] } };
        put.run(id, "resolved", JSON.stringify(record));
      }
      const ask = db.prepare("INSERT INTO asks(project,id,kind,subject,done_ms,data) VALUES('demo',?,'request',?,?,?)");
      const now = Date.now();
      for (let i = 0; i < DONE_ASKS; i++) {
        const record = { id: `history-${i}`, question_id: `q-${7000 + i}`, kind: "request", subject: `fixture-${i}`, title: `답한 질문 ${i}`,
          asked_ms: now - i * 60_000, done_ms: now - i * 60_000, by: "측정", summary: "1A", items: [], body: `원문 ${i} ` + "측정용 답 본문. ".repeat(190) };
        ask.run(record.id, record.subject, record.done_ms, JSON.stringify(record));
      }
    });
    result.scale = { threads: OLD_THREADS + 1, doneAsks: DONE_ASKS };
  }, { always: true });

  const wk = await webkit.launch({ headless: true });
  try {
    // ── 첫 접속 ──
    // 짝지은 기기의 cookie만 새 context에 옮긴다: 첫 방문은 캐시가 빈 접속(cold), 같은 context의 둘째 방문은 캐시가 있는 접속(warm)이다.
    const viewport = { width: 390, height: 844 };
    let cookies;
    await step("측정 기기 짝짓기", async () => {
      let context;
      const { page } = await pairPage(browser, aaa("web", "pair").code, viewport, { serviceWorkers: "block", setup: (ctx) => { context = ctx; } });
      cookies = await context.cookies();
      await page.close();
      await context.close();
    }, { always: true });
    for (const [engine, name] of [[browser, "chromium"], [wk, "webkit"]]) {
      for (const link of name === "chromium" ? Object.keys(LINKS) : ["loopback"]) for (const route of ["home", "question"]) {
        await step(`첫 접속 ${name} ${link} ${route}`, async () => {
          const runs = { cold: [], warm: [] };
          for (let i = 0; i < (link === "slow" ? Math.min(RUNS, 3) : RUNS); i++) {
            const context = await engine.newContext({ viewport, isMobile: true, hasTouch: true, serviceWorkers: "block" });
            try {
              await context.addCookies(cookies);
              await context.addInitScript(perfInit);
              for (const cache of ["cold", "warm"]) {
                const page = await context.newPage();
                if (LINKS[link]) {
                  const cdp = await context.newCDPSession(page);
                  await cdp.send("Network.enable");
                  await cdp.send("Network.emulateNetworkConditions", { offline: false, ...LINKS[link] });
                }
                await page.goto(`${base}${route === "question" ? `#question=${asked.id}` : ""}`);
                await settle(page, route === "question");
                runs[cache].push(await collect(page));
                await page.close();
              }
            } finally { await context.close(); }
          }
          for (const cache of ["cold", "warm"]) {
            const key = `${name}/${link}/${cache}/${route}`;
            result.first[key] = { summary: summarize(runs[cache]), sample: runs[cache][runs[cache].length >> 1] };
            console.log(`perf first ${key} ${JSON.stringify(result.first[key].summary)}`);
          }
        });
      }
    }

    // ── thread 질문 카드 답 ──
    for (const [engine, name] of [[browser, "chromium"], [wk, "webkit"]]) {
      await step(`질문 답 ${name}`, async () => {
        let context;
        const { page } = await pairPage(engine, aaa("web", "pair").code, { width: 390, height: 844 }, { serviceWorkers: "block", init: perfInit, setup: (ctx) => { context = ctx; } });
        try {
          await settle(page, false);
          const rows = [];
          for (let i = 0; i < ANSWERS; i++) {
            const t = i === 0 && name === "chromium" ? asked : await question(`답 시간 ${name} ${i}`);
            const seq = Math.max(0, ...aaa("events", "demo").events.map((e) => e.seq));
            await page.evaluate((id) => { location.hash = `question=${id}`; }, t.id);
            const card = page.getByTestId(`thread-ask-${t.id}`);
            await card.getByTestId("thread-ask-pick-1A").click();
            await page.evaluate(() => { const P = window.__perf; P.clicks.length = 0; P.card.length = 0; P.threadEvents.length = 0; P.calls.length = 0; });
            await card.getByTestId("thread-ask-send").click();
            await domIs(page, "답한 카드", () => !!document.querySelector('[data-testid="ask-done-card"]') && !document.querySelector(".thread-ask-card[data-state='open']"), null, 60_000);
            await waitOutput(t.pane, "보고의 질문에 대한 사용자 답");
            const events = await until("전달 사건", () => {
              const ev = aaa("events", "demo", "--after", String(seq)).events.filter((e) => e.thread === t.id);
              return ev.some((e) => e.kind === "message_accepted" || e.kind === "message_unknown") && ev.some((e) => e.kind === "question_answered") && ev;
            });
            await domIs(page, "답 뒤 다시 읽기 끝", () => window.__perf.calls.every((c) => c.end));
            const p = await page.evaluate(() => {
              const P = window.__perf;
              return { click: P.clicks[0], card: P.card, events: P.threadEvents, calls: P.calls };
            });
            const at = (x) => (x == null ? null : Math.round(x - p.click.at));
            const host = (kind) => { const e = events.find((e) => e.kind === kind); return e ? e.at_ms - p.click.wall : null; };
            const answerCall = p.calls.find((c) => c.op === "thread_answer");
            const reread = p.calls.find((c) => c.op === "asks");
            rows.push({
              busy: at(p.card.find((c) => c.busy)?.at),
              threadEvent: at(p.events.find((e) => e.ev === "thread" && e.id === t.id && e.answered)?.at),
              cardGone: at(p.card.find((c) => c.card !== `thread-ask-${t.id}`)?.at),
              // 질문 카드가 사라진 뒤 「찾을 수 없다」·「답할 것이 없다」 안내가 보인 시간
              notice: (() => { const a = p.card.find((c) => c.missing || c.empty); const b = a && p.card.find((c) => c.at > a.at && !c.missing && !c.empty); return a ? Math.round((b?.at ?? a.at) - a.at) : 0; })(),
              response: at(answerCall?.end), responseBytes: answerCall?.bytes ?? null,
              doneCard: at(p.card.find((c) => c.done)?.at),
              asksReread: at(reread?.end), asksBytes: reread?.bytes ?? null,
              hostQueued: host("message_queued"), hostAccepted: host("message_accepted") ?? host("message_unknown"), hostAnswered: host("question_answered"),
            });
            aaa("thread", "resolve", t.id);
          }
          const keys = Object.keys(rows[0]);
          result.answer[name] = { median: Object.fromEntries(keys.map((k) => [k, median(rows.map((r) => r[k]))])), rows };
          console.log(`perf answer ${name} ${JSON.stringify(result.answer[name].median)}`);
        } finally { await context.close(); }
      });
    }

    // ── coordinator 확인 카드 답(ask_answer) ──
    await step("확인 카드 답 chromium", async () => {
      let context;
      const { page } = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { serviceWorkers: "block", init: perfInit, setup: (ctx) => { context = ctx; } });
      try {
        await settle(page, false);
        const rows = [];
        for (let i = 0; i < Math.min(3, ANSWERS); i++) {
          const post = await coordinatorPost({ request: true, title: `확인 ${i}`, body: `## Questions\n### 1. 확인 ${i}\n- A 진행 (추천)\n- B 멈춤\n` });
          const req = await until("확인 카드", async () => {
            const r = await page.evaluate(async () => (await (await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ op: "asks", paged: true }) })).json()).ok.requests);
            return post && r.find((x) => x.questions && x.title === `확인 ${i}`);
          });
          await page.evaluate((id) => { location.hash = `question=${id}`; }, req.question_id ?? req.questions.items[0].id);
          const card = page.getByTestId(`request-ask-${req.id}`);
          await card.getByTestId("thread-ask-pick-1A").click();
          await domIs(page, "앞 읽기 끝", () => window.__perf.calls.every((c) => c.end));
          await page.evaluate(() => { const P = window.__perf; P.clicks.length = 0; P.card.length = 0; P.threadEvents.length = 0; P.calls.length = 0; });
          await card.getByTestId("thread-ask-send").click();
          await domIs(page, "답한 카드", () => !!document.querySelector('[data-testid="ask-done-card"]') && !document.querySelector(".thread-ask-card[data-state='open']"), null, 60_000);
          await domIs(page, "답 뒤 다시 읽기 끝", () => window.__perf.calls.every((c) => c.end));
          const p = await page.evaluate(() => { const P = window.__perf; return { click: P.clicks[0], card: P.card, calls: P.calls }; });
          const at = (x) => (x == null ? null : Math.round(x - p.click.at));
          const answerCall = p.calls.find((c) => c.op === "ask_answer");
          const reread = p.calls.find((c) => c.op === "asks");
          rows.push({ busy: at(p.card.find((c) => c.busy)?.at), response: at(answerCall?.end), responseBytes: answerCall?.bytes ?? null,
            encoding: answerCall?.encoding ?? null, doneCard: at(p.card.find((c) => c.done)?.at), asksReread: at(reread?.end), asksBytes: reread?.bytes ?? null });
        }
        const keys = Object.keys(rows[0]);
        result.request.chromium = { median: Object.fromEntries(keys.map((k) => [k, typeof rows[0][k] === "string" || rows[0][k] === null ? rows[0][k] : median(rows.map((r) => r[k]))])), rows };
        console.log(`perf request chromium ${JSON.stringify(result.request.chromium.median)}`);
      } finally { await context.close(); }
    });
    // ── 답 직후 화면(촬영만, AAA_PERF_SHOTS=<폴더>) ──
    if (process.env.AAA_PERF_SHOTS) await step("답 직후 화면", async () => {
      fs.mkdirSync(process.env.AAA_PERF_SHOTS, { recursive: true });
      const hold = Promise.withResolvers();
      let context, armed = false;
      const { page } = await pairPage(wk, aaa("web", "pair").code, { width: 390, height: 844 }, { serviceWorkers: "block", init: perfInit, setup: async (ctx) => {
        context = ctx;
        // 누른 뒤의 「답할 것」 다시 읽기만 붙잡아 그 사이 화면을 찍는다
        await ctx.route("**/api/v1/call", async (route) => {
          if (armed && route.request().postDataJSON()?.op === "asks") await hold.promise;
          await route.continue().catch(() => {});
        });
      } });
      try {
        await settle(page, false);
        const t = await question("답 직후 화면");
        await page.evaluate((id) => { location.hash = `question=${id}`; }, t.id);
        const card = page.getByTestId(`thread-ask-${t.id}`);
        await card.getByTestId("thread-ask-pick-1A").click();
        armed = true;
        await card.getByTestId("thread-ask-send").click();
        await domIs(page, "질문 카드 닫힘", () => !document.querySelector(".thread-ask-card[data-state='open']"), null, 60_000);
        await waitOutput(t.pane, "보고의 질문에 대한 사용자 답");
        await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));
        await page.screenshot({ path: `${process.env.AAA_PERF_SHOTS}/after-send.png` });
        result.afterSend = await page.evaluate(() => ({ done: !!document.querySelector('[data-testid="ask-done-card"]'), missing: !!document.querySelector('[data-testid="asks-missing"]'), empty: !!document.querySelector('[data-testid="asks-empty"]') }));
        console.log(`perf afterSend ${JSON.stringify(result.afterSend)}`);
        aaa("thread", "resolve", t.id);
      } finally { hold.resolve(); await context.unrouteAll({ behavior: "ignoreErrors" }); await context.close(); }
    });
  } finally {
    await wk.close();
    fs.writeFileSync(OUT, `${JSON.stringify(result, null, 1)}\n`);
    console.log(`perf result ${OUT}`);
  }
}
