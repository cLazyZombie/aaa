// 실제 host·agent의 동일 합성 기록으로 처음/왕복을 측정하고 보관 수명을 확인한다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { root, AA, aaa, step, browser, webkit, pairPage, waitOutput, until, domIs, shot, inTime } from "../lib/harness.mjs";
import { frames, shown, go, prepare, holdLatest } from "../lib/chat-retention.mjs";
import { retentionFailure } from "../lib/retention-failure.mjs";
import { retentionProtocol } from "../lib/retention-protocol.mjs";
const baseline = process.env.AAA_CHAT_BASELINE === "1";
const measurements = [];
const diagnosticDir = process.env.AAA_CHAT_DIAGNOSTICS || `${root}/diagnostics`;
fs.mkdirSync(diagnosticDir, { recursive: true });
const retentionLog = event => {
  const line = JSON.stringify({ at: Date.now(), ...event });
  fs.appendFileSync(`${diagnosticDir}/retention-pages.jsonl`, line + "\n");
  console.log("retention-page", line);
};
async function measure(page, pane, row = null) {
  return page.evaluate(({ pane, row }) => new Promise((resolve, reject) => {
    const start = performance.now();
    location.hash = `pane=${pane}`;
    const check = () => {
      const chat = document.querySelector('.term:not([hidden]) [data-testid="chat"]');
      const element = row ? chat?.querySelector(`[data-chat-id="${row.id}"]`) : chat?.querySelector('.chat-row:last-child');
      const ready = element && chat.clientHeight > 0 && chat.getAttribute("aria-busy") === "false"
        && (row ? Math.abs(element.getBoundingClientRect().top - chat.getBoundingClientRect().top - row.offset) <= 1 : element.textContent.includes("기록 2399"));
      if (ready) requestAnimationFrame(() => requestAnimationFrame(() => resolve({ bodyMs: performance.now() - start, connected: document.querySelector('.term:not([hidden]) [data-testid="terminal"]')?.dataset.readonly === "false" })));
      else if (performance.now() - start > 20_000) reject(new Error("본문·읽던 위치 준비 실패"));
      else requestAnimationFrame(check);
    };
    check();
  }), { pane, row });
}
async function check(engine, width, panes, delayed = false) {
  for (const role of delayed ? ["coordinator"] : ["coordinator", "thread"]) {
    for (let trial = 0; trial < 3; trial++) {
      const requests = [];
      const replies = [];
      const ph = await pairPage(engine, aaa("web", "pair").code, { width, height: 844 }, { serviceWorkers: "block", hash: "#tasks", setup: async context => {
        await context.addInitScript(keys => { for (const key of keys) localStorage.setItem(`aaa:view:${key}`, "chat"); }, panes.keys);
        context.on("request", r => { if (r.url().endsWith("/api/v1/call")) { const req = r.postDataJSON(); if (req?.op === "pane_chat") requests.push(req); } });
        context.on("response", r => { if (r.url().endsWith("/api/v1/call") && r.request().postDataJSON()?.op === "pane_chat") replies.push(r); });
      } });
      const { page } = ph;
      const protocol = engine.browserType().name() === "chromium" ? retentionProtocol(page, diagnosticDir) : null;
      const pane = panes[role];
      const first = await measure(page, pane);
      await page.locator('.term:not([hidden]) [data-testid="terminal"][data-readonly="false"]').waitFor({ state: "attached" });
      const composer = page.locator('.term:not([hidden]) textarea[aria-label="메시지"]');
      await composer.fill(`남길 초안 ${role}`);
      await page.evaluate(() => { window.savedChat = document.querySelector('.term:not([hidden]) [data-testid="chat"]'); });
      if (delayed) {
        const cdp = await page.context().newCDPSession(page);
        await cdp.send("Network.enable");
        await cdp.send("Network.emulateNetworkConditions", { offline: false, latency: 80, downloadThroughput: -1, uploadThroughput: -1 });
      }
      await go(page, "tasks"); await frames(page);
      const latest = await measure(page, pane);
      const connectionStart = performance.now();
      await page.locator('.term:not([hidden]) [data-testid="terminal"][data-readonly="false"]').waitFor({ state: "attached" });
      const inputMs = performance.now() - connectionStart + latest.bodyMs;
      if (!baseline) assert.ok(await page.evaluate(() => window.savedChat === document.querySelector('.term:not([hidden]) [data-testid="chat"]')), "같은 본문 DOM");
      assert.equal(await composer.inputValue(), `남길 초안 ${role}`);
      // 최신 150행 외에 여섯 쪽을 받아 둔 과거 위치를 사용한다.
      for (let n = 0; n < 6; n++) {
        const context = { engine: engine.browserType().name(), width, role, trial, pageNo: n, pane,
          phase: "count", before: null, beforeState: null };
        protocol?.at(context);
        retentionLog({ event: "start", ...context });
        try {
          const before = await shown(page).locator('.chat-row').count();
          Object.assign(context, { phase: "lookup-before-state", before });
          retentionLog({ event: "phase", ...context });
          const beforeState = await shown(page).evaluate((el, label) => {
            window.__aaaRetentionEvaluation = { ...label, phase: "entered", at: performance.now() };
            const state = { top: el.scrollTop, busy: el.getAttribute("aria-busy"), rows: el.querySelectorAll(".chat-row").length,
              older: el.querySelector(".chat-older")?.textContent, error: el.querySelector(".banner")?.textContent,
              nodes: el.querySelectorAll("*").length, height: el.clientHeight };
            el.scrollTop = 0;
            el.dispatchEvent(new Event("scroll", { bubbles: true }));
            window.__aaaRetentionEvaluation = { ...label, phase: "returned", at: performance.now() };
            return state;
          }, { role, trial, pageNo: n });
          Object.assign(context, { phase: "commit", beforeState });
          retentionLog({ event: "phase", ...context });
          await domIs(page, "앞 쪽 commit", before => document.querySelector('.term:not([hidden]) [data-testid="chat"]').querySelectorAll('.chat-row').length > before, before);
          retentionLog({ event: "commit", ...context });
        } catch (error) {
          // 최초 오류와 시작 상태는 페이지에 다시 묻기 전에 남긴다. 추가 진단이 실패해도 원래 오류를 던진다.
          await retentionFailure(error, context, requests.slice(-10), retentionLog, () =>
            inTime(page.evaluate(() => {
              const el = document.querySelector('.term:not([hidden]) [data-testid="chat"]');
              return el ? { evaluation: window.__aaaRetentionEvaluation ?? null, top: el.scrollTop, busy: el.getAttribute("aria-busy"), rows: el.querySelectorAll(".chat-row").length,
                older: el.querySelector(".chat-older")?.textContent, error: el.querySelector(".banner")?.textContent,
                nodes: el.querySelectorAll("*").length, height: el.clientHeight } : { absent: true, hash: location.hash };
            }), 5000, "앞 쪽 실패 진단"));
        } finally {
          protocol?.at(null);
        }
      }
      const row = await shown(page).locator('.chat-row').nth(40).evaluate(el => {
        const chat = el.closest('.chat');
        chat.scrollTop += el.getBoundingClientRect().top - chat.getBoundingClientRect().top + 20;
        chat.dispatchEvent(new Event("scroll", { bubbles: true }));
        return el.dataset.chatId;
      });
      await frames(page);
      const anchor = await shown(page).locator(`[data-chat-id="${row}"]`).evaluate(el => ({ id: el.dataset.chatId, offset: el.getBoundingClientRect().top - el.closest('.chat').getBoundingClientRect().top }));
      // 이전 읽기가 남아 있으면 복귀 읽기는 그 응답 뒤에 시작한다. 보관 본문은 그 전에 보여야 한다.
      const controlled = !baseline && !delayed && role === "coordinator" && trial === 0
        && (engine.browserType().name() === "chromium" && width === 1280 || engine.browserType().name() === "webkit" && width === 390);
      const held = controlled ? await holdLatest(page, pane, panes.transcript) : null;
      let old, responsesBeforeBody, from;
      try {
        await go(page, "tasks"); await frames(page);
        from = requests.length;
        const responseFrom = replies.length;
        // 동작 전에 구독한다. DOM/rAF 완료는 별도 전송 lane·Network 사건의 완료가 아니다.
        const newest = page.waitForRequest(r => r.url().endsWith("/api/v1/call")
          && r.postDataJSON()?.op === "pane_chat" && r.postDataJSON()?.pane === pane && !r.postDataJSON()?.before,
        { timeout: 20_000 });
        [old] = await Promise.all([
          (async () => {
            const result = await measure(page, pane, anchor);
            responsesBeforeBody = replies.length - responseFrom;
            if (held) assert.equal(requests.slice(from).filter(r => r.pane === pane).length, 0, "이전 응답 전 본문·과거 위치 복원");
            retentionLog({ event: "return-body", engine: engine.browserType().name(), width, role, trial, delayed,
              controlled, bodyMs: result.bodyMs, requests: requests.slice(from).filter(r => r.pane === pane), responsesBeforeBody });
            held?.release();
            return result;
          })(),
          newest,
        ]);
      } finally { await held?.dispose(); }
      const counts = requests.slice(from).filter(r => r.pane === pane);
      if (!baseline) {
        assert.equal(counts.filter(r => r.before).length, 0, "과거 위치 복원 재요청 0회");
        assert.equal(counts.filter(r => !r.before).length, 1, "복귀 최신 확인 1회");
      }
      retentionLog({ event: "return-requests", engine: engine.browserType().name(), width, role, trial, delayed, controlled, requests: counts });
      measurements.push({ engine: engine.browserType().name(), width, role, trial, delayed, first: first.bodyMs, latest: latest.bodyMs, older: old.bodyMs, inputMs, olderRequests: counts.filter(r => r.before).length, latestRequests: counts.filter(r => !r.before).length, responsesBeforeBody });
      if (!baseline && trial === 0 && !delayed) {
        const label = `retention-${engine.browserType().name()}-${width}-${role}`;
        await shot(page, label);
        if (process.env.AAA_CHAT_SHOTS) {
          fs.mkdirSync(process.env.AAA_CHAT_SHOTS, { recursive: true });
          await page.screenshot({ path: `${process.env.AAA_CHAT_SHOTS}/${label}.png` });
        }
      }
      assert.deepEqual(ph.errors, []);
      await page.context().close();
    }
  }
}
export default async function () {
  let panes, safari;
  try {
  await step("대화 보관: 실제 coordinator·일반 pane 준비", async () => { panes = await prepare(); }, { always: true });
  await step("대화 보관: Chromium 1280 처음·최신·과거 왕복 계측", async () => { await check(browser, 1280, panes); });
  await step("대화 보관: Chromium 390 처음·최신·과거 왕복 계측", async () => { await check(browser, 390, panes); });
  await step("대화 보관: WebKit 1280 처음·최신·과거 왕복 계측", async () => { safari = await webkit.launch({ headless: true }); await check(safari, 1280, panes); });
  await step("대화 보관: WebKit 390 처음·최신·과거 왕복 계측", async () => { await check(safari, 390, panes); });
  await step("대화 보관: 80ms 회선 지연의 coordinator 1280·390 계측", async () => { await check(browser, 1280, panes, true); await check(browser, 390, panes, true); });
  await step("대화 보관: 계측 저장·브라우저 정리", async () => {
    const data = JSON.stringify(measurements, null, 2) + "\n";
    fs.writeFileSync(`${root}/chat-measurements.json`, data);
    if (process.env.AAA_CHAT_MEASUREMENTS) fs.writeFileSync(process.env.AAA_CHAT_MEASUREMENTS, data);
    console.log(data);
    await safari?.close();
  });
  } finally {
    if (safari?.isConnected()) await safari.close();
  }
}
