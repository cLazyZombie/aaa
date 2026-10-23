// 기본 셸 → hook 없는 harness → 실제 프로세스/기록 연결 → host → 대화 페이지/스크롤.
import assert from "node:assert/strict";
import fs from "node:fs";
import { watchScroll } from "../lib/scrollDiagnostics.mjs";
import { ChatDiagnostics } from "../lib/chatDiagnostics.mjs";
import {
  root, aaa, step, until, waitOutput, inList, domIs, shot, pairPage, browser, webkit, inTime, screenText, start, stop,
} from "../lib/harness.mjs";

export const labEnv = { CODEX_HOME: `${root}/codex` };
fs.mkdirSync(`${root}/shellbin`, { recursive: true });
for (const name of ["codex", "claude"]) {
  fs.copyFileSync(new URL("../lib/fake-shell-harness.py", import.meta.url), `${root}/shellbin/${name}`);
  fs.chmodSync(`${root}/shellbin/${name}`, 0o755);
}
const sid = (n) => `01900000-0000-7000-8000-${n.toString(16).padStart(12, "0")}`;
let serial = 0;
let shellDiagnostics = null;

async function firstMessage(page, id) {
  const chat = page.getByTestId("chat");
  await chat.locator("[data-chat-id]").first().waitFor({ state: "attached" });
  for (let part = 0; part < 5; part++) {
    if (await chat.getByText("처음이다", { exact: true }).count()) break;
    const response = page.waitForResponse((r) => r.url().endsWith("/api/v1/call") && r.request().postDataJSON()?.op === "pane_chat" && Boolean(r.request().postDataJSON()?.before));
    await chat.evaluate((c) => { c.scrollTop = 0; c.dispatchEvent(new Event("scroll", { bubbles: true })); });
    await response;
    await domIs(page, "재개한 앞 기록 읽기 완료", () => !document.querySelector('[data-testid="chat"] button.chat-older:disabled'));
  }
  await chat.evaluate((c) => { c.scrollTop = 0; });
  await chat.getByText(`${id} 기록 0 ·`, { exact: false }).first().waitFor();
}

async function resumed(engine, width, harness) {
  const name = `resume-${harness}-${++serial}`, id = sid(serial * 3);
  const page = (await pairPage(engine, aaa("web", "pair").code, { width, height: 844 }, { touch: width < 500 })).page;
  const pane = aaa("tab", "create", "--machine", "labA", "--label", name).tabs.at(-1).root.pane;
  const state = () => aaa("pane", "list").panes.find((p) => p.id === pane);
  const toggle = page.getByTestId("chat-toggle"), chat = page.getByTestId("chat");
  const diagnostics = new ChatDiagnostics(root, name);
  shellDiagnostics = diagnostics;
  diagnostics.page = page;
  diagnostics.watch(page.context());
  page.on("request", (request) => {
    if (request.url().endsWith("/api/v1/call")) diagnostics.request(request, request.postDataJSON());
  });
  const connection = [];
  page.on("websocket", (ws) => {
    if (!ws.url().includes("/panes/")) return;
    const entry = { pane: new URL(ws.url()).pathname.split("/").at(-2), at: Date.now() };
    connection.push(entry);
    diagnostics.remember([{ connection: entry }]);
    ws.on("close", () => { entry.closed = Date.now(); });
  });
  let phase;
  const mark = (next) => {
    phase = next;
    diagnostics.remember([{ at: Date.now(), phase, width, engine: engine.browserType().name(), harness }]);
  };
  mark("inline");
  const terminal = async () => {
    await toggle.waitFor();
    if (await toggle.getAttribute("aria-pressed") === "true") await toggle.click();
    await until("inline 화면 그리기", async () => (await screenText(page)).includes("foreground-harness-ready"));
  };
  const quit = async () => {
    aaa("pane", "run", pane, "quit");
    await waitOutput(pane, "foreground-harness-exited");
    await until("재개 전 셸 복귀", () => !state().agent);
  };
  try {
    await inList(page, `pane-${name}`);
    aaa("pane", "run", pane, `${root}/shellbin/${harness} ${root} ${id} --inline`);
    await waitOutput(pane, "foreground-harness-ready");
    await terminal();
    const read = () => aaa("pane", "read", pane, "--source", "recent", "--lines", "1000").text;
    assert.ok(read().includes("terminal-history-0"), "서버 VT가 입력줄 위에서 밀린 기록을 잃었다");
    await inList(page, "pane-t1");
    await inList(page, `pane-${name}`);
    await terminal();
    await domIs(page, "inline 사본의 기록·화면 읽기 완료", () => Number(document.querySelector('[data-testid="terminal"]')?.dataset.viewBase) > 0);
    await page.getByTestId("terminal").evaluate((el) => el.querySelector(".xterm").dispatchEvent(new WheelEvent("wheel", { bubbles: true, cancelable: true, deltaY: -10000, deltaMode: WheelEvent.DOM_DELTA_LINE })));
    await until("연결 전 inline 기록 복원", async () => (await screenText(page)).includes("terminal-history-0"));
    aaa("pane", "run", pane, "open");
    await until("재개 전 원래 기록 연결", () => state().agent?.harness_session === id);
    await quit();
    for (const mode of harness === "codex" ? ["resume"] : ["--resume", "--continue"]) {
      mark(`${mode}:terminal`);
      aaa("pane", "run", pane, `${root}/shellbin/${harness} ${root} ${mode}${mode === "--continue" ? "" : ` ${id}`} --mouse`);
      await waitOutput(pane, "foreground-harness-ready");
      await until(`${mode}로 같은 대화 연결`, () => state().agent?.harness_session === id);
      await terminal();
      const footer = page.locator(".xterm-rows > div").filter({ hasText: "fixed-composer>" });
      const fixed = await footer.boundingBox();
      for (let turn = 0; turn < 2; turn++) {
        const previous = await screenText(page);
        const area = await page.getByTestId("terminal").boundingBox();
        if (width < 500 && engine === browser) {
          const cdp = await page.context().newCDPSession(page);
          const x = Math.round(area.x + area.width / 2), y = Math.round(area.y + area.height / 2);
          await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x, y, id: 1 }] });
          await cdp.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x, y: y + 100, id: 1 }] });
          await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
          await cdp.detach();
        } else if (width < 500) {
          // WebKit의 Playwright는 이동 touch API가 없어 TermView 사건 경로만 확인한다.
          await page.getByTestId("terminal").evaluate((el) => {
            const rect = el.getBoundingClientRect(), x = rect.left + rect.width / 2, y = rect.top + rect.height / 2;
            const dispatch = (type, at, end = false) => {
              const touch = document.createTouch(window, el, 1, x, at, x, at);
              const touches = document.createTouchList(...(end ? [] : [touch]));
              el.dispatchEvent(new TouchEvent(type, { bubbles: true, cancelable: true, touches, targetTouches: touches, changedTouches: document.createTouchList(touch) }));
            };
            dispatch("touchstart", y);
            dispatch("touchmove", y + 100);
            dispatch("touchend", y + 100, true);
          });
        } else {
          await page.mouse.move(area.x + area.width / 2, area.y + area.height / 2);
          await page.mouse.wheel(0, -100);
        }
        await until("프로그램 자체 대화 스크롤", async () => (await screenText(page)) !== previous);
        assert.deepEqual(await footer.boundingBox(), fixed, "프로그램 휠이 입력줄까지 움직였다");
        mark(`${mode}:terminal:return-${turn + 1}`);
        await inList(page, "pane-t1");
        await inList(page, `pane-${name}`);
        await terminal();
        await until("alternate 화면 사본 복원", async () => (await screenText(page)).includes("app-history-") && (await screenText(page)).includes("fixed-composer>"));
      }
      await toggle.waitFor();
      const newest = page.waitForResponse((r) => r.url().endsWith("/api/v1/call") && r.request().postDataJSON()?.op === "pane_chat" && !r.request().postDataJSON()?.before);
      if (await toggle.getAttribute("aria-pressed") !== "true") await toggle.click();
      mark(`${mode}:chat`);
      const reply = await (await newest).json();
      assert.ok(reply.ok?.messages?.some((message) => message.text.startsWith(`${id} 기록 359 ·`)), "재개한 최신 기록이 없다");
      // continue는 같은 기록의 읽던 맨 위를 복원한다. 화면 밖 최신 본문은 가상화되어 mount되지 않을 수 있다.
      await chat.locator("[data-chat-id]").first().waitFor({ state: "attached" });
      const composer = page.getByTestId("chat-composer"), before = await composer.boundingBox();
      await firstMessage(page, id);
      assert.deepEqual(await composer.boundingBox(), before, "재개한 이전 대화를 굴리며 입력창이 움직였다");
      await inList(page, "pane-t1");
      await inList(page, `pane-${name}`);
      assert.equal(await toggle.getAttribute("aria-pressed"), "true", "재개 뒤 보기 선택 소실");
      await chat.getByText(`${id} 기록 0 ·`, { exact: false }).first().waitFor();
      // 자기 실험실 agent만 재시작한다. 저장된 연결을 없애 PID 증거로 다시 찾는지 확인한다.
      await stop("agent");
      mark(`${mode}:restart`);
      const saved = `${root}/s/agent/sessions/${pane}/harness.json`;
      fs.rmSync(saved);
      start("agent", "agent", "--host", "local", "--name", "labA");
      await until("재시작 뒤 PID로 기록 다시 연결", () => {
        const events = JSON.parse(fs.readFileSync(saved, "utf8"));
        return events.some((e) => e.harness_session === id && e.transcript?.endsWith(".jsonl"));
      });
      await until("재시작 뒤 host 연결", () => state().connection === "connected" && state().agent?.harness_session === id);
      await inList(page, "pane-t1");
      await inList(page, `pane-${name}`);
      // 재시작은 기존 byte cursor를 무효화한다(chat_pages). 새 cursor로 이전 대화 전체를 다시 읽을 수 있어야 한다.
      await firstMessage(page, id);
      await inList(page, "pane-t1");
      await inList(page, `pane-${name}`);
      await chat.getByText(`${id} 기록 0 ·`, { exact: false }).first().waitFor();
      await shot(page, `${name}-${mode}-${width}`);
      mark(`${mode}:quit`);
      await quit();
    }
    mark("done");
  } catch (error) {
    await diagnoseShell(error);
    fs.mkdirSync(`${root}/diagnostics`, { recursive: true });
    const view = await inTime(page.evaluate(() => ({
      terminal: { ...document.querySelector('[data-testid="terminal"]')?.dataset },
      screen: document.querySelector(".xterm-rows")?.textContent,
      viewport: [...document.querySelectorAll(".xterm-viewport")].map((el) => ({ top: el.scrollTop, height: el.scrollHeight, client: el.clientHeight })),
      chat: document.querySelector('[data-testid="chat"]')?.textContent?.slice(0, 2000),
    })), 5000, "재개 실패 화면 수집").catch((error) => ({ captureError: String(error) }));
    fs.writeFileSync(`${root}/diagnostics/${name}.json`, JSON.stringify({ phase, width, engine: engine.browserType().name(), view }, null, 2));
    throw error;
  } finally {
    const phases = diagnostics.states.filter((state) => state.phase);
    const closes1006 = await page.evaluate(() => window.__aaaTermLog.filter((entry) => entry.e === "ws close 1006").length).catch(() => null);
    console.log("재개 진행", JSON.stringify({ name, width, engine: engine.browserType().name(), phase,
      phases: phases.map((state) => [state.phase, state.at - diagnostics.started]),
      connections: connection.length, closes1006,
      samePaneReconnects: connection.filter((entry, i) => i > 0 && entry.pane === connection[i - 1].pane).length }));
    diagnostics.stop();
    shellDiagnostics = null;
    await page.context().close();
  }
}

function diagnoseShell(error) {
  const diagnostics = shellDiagnostics;
  if (!diagnostics) return;
  if (!diagnostics.saving) {
    const current = diagnostics.states.findLast((state) => state.phase);
    diagnostics.errors = [{ ...current, error: String(error) }];
    diagnostics.saving = diagnostics.save(error, inTime);
  }
  return diagnostics.saving;
}

async function harnessCase(harness, kind, width, run = resumed) {
  const engine = kind === "chromium" ? browser : await webkit.launch({ headless: true });
  try { await run(engine, width, harness); }
  finally { if (engine !== browser) await engine.close(); }
}

export default async function () {
  const check = async (engine, width, harness) => {
    const n = ++serial;
    const name = `shell-${harness}-${n}`;
    const id = sid(n * 3), next = sid(n * 3 + 1), decoy = sid(n * 3 + 2);
    const page = (await pairPage(engine, aaa("web", "pair").code, { width, height: 844 }, { touch: width < 500 })).page;
    const diagnostics = new ChatDiagnostics(root, name);
    shellDiagnostics = diagnostics;
    diagnostics.page = page;
    diagnostics.watch(page.context());
    page.on("request", (request) => {
      if (request.url().endsWith("/api/v1/call")) diagnostics.request(request, request.postDataJSON());
    });
    let phase;
    const mark = (next) => {
      phase = next;
      diagnostics.remember([{ at: Date.now(), phase, width, engine: engine.browserType().name(), harness }]);
    };
    mark("connect");
    try {
      // 본문과 cwd가 비슷한 더 새 기록도 다른 PID의 것이면 고르지 않는다.
      const dir = harness === "codex" ? `${root}/codex/sessions/2026/10/23` : `${root}/claude/projects/shell`;
      fs.mkdirSync(dir, { recursive: true });
      fs.writeFileSync(`${dir}/${harness === "codex" ? "rollout-decoy-" : ""}${decoy}.jsonl`, "{}\n");
      if (harness === "codex") {
        const backup = `${root}/codex/sessions/2026/10/22`;
        fs.mkdirSync(backup, { recursive: true });
        fs.writeFileSync(`${backup}/rollout-backup-${id}.jsonl`, JSON.stringify({ type: "session_meta", payload: { id, source: "cli" } }) + "\n");
      }
      const pane = aaa("tab", "create", "--machine", "labA", "--label", name).tabs.at(-1).root.pane;
      await inList(page, `pane-${name}`);
      aaa("pane", "run", pane, `${root}/shellbin/${harness} ${root} ${id}`);
      await waitOutput(pane, "foreground-harness-ready");
      await until("직접 실행한 harness 감지", () => aaa("pane", "list").panes.find((p) => p.id === pane)?.agent?.harness === harness);
      const state = () => aaa("pane", "list").panes.find((p) => p.id === pane);
      assert.equal(state().agent.harness_session, null, "최근 기록을 추측해 연결했다");
      const toggle = page.getByTestId("chat-toggle");
      await toggle.waitFor();
      if ((await toggle.getAttribute("aria-pressed")) === "true") await toggle.click();
      await until("연결 전 터미널 화면", async () => (await screenText(page)).includes("foreground-harness-ready"));
      await inList(page, "pane-t1");
      await inList(page, `pane-${name}`);
      await until("다른 대화 복귀 때 연결 전 화면 복원", async () => (await screenText(page)).includes("foreground-harness-ready"));
      if ((await toggle.getAttribute("aria-pressed")) !== "true") await toggle.click();
      const chat = page.getByTestId("chat");
      await chat.locator(".banner.warn").waitFor();
      // 첫 기록이 늦게 생겨도 사용자 새로 고침 없이 연결하고 push로 갱신한다.
      aaa("pane", "run", pane, "open");
      await until("실제 PID의 기록 연결", () => state().agent?.harness_session === id);
      await chat.getByText(`${id} 기록 359 ·`, { exact: false }).first().waitFor();
      await domIs(page, "대화 스크롤 범위", () => {
        const c = document.querySelector('[data-testid="chat"]');
        return c && c.scrollHeight > c.clientHeight;
      });
      // 두 쪽 이상을 실제 host에서 읽고 처음까지 도달한다. scroll 사건이 앞 쪽 요청을 끝낸다.
      for (let part = 0; part < 5; part++) {
        if (await chat.getByText("처음이다", { exact: true }).count()) break;
        const response = page.waitForResponse((r) => {
          if (!r.url().endsWith("/api/v1/call")) return false;
          const b = r.request().postDataJSON();
          return b?.op === "pane_chat" && Boolean(b.before);
        });
        await chat.evaluate((c) => { c.scrollTop = 0; c.dispatchEvent(new Event("scroll", { bubbles: true })); });
        await response;
        await domIs(page, "앞 기록 읽기 완료", () => !document.querySelector('[data-testid="chat"] [data-testid="chat-older-error"]') && !document.querySelector('[data-testid="chat"] button.chat-older:disabled'));
      }
      await chat.evaluate((c) => { c.scrollTop = 0; });
      await chat.getByText(`${id} 기록 0 ·`, { exact: false }).first().waitFor();
      await chat.getByText("처음이다", { exact: true }).waitFor();
      await watchScroll(page, '[data-testid="chat"]');
      // 앞 안내의 높이가 늦게 재측정되는 경계. 맨 위에서 첫 row의 위치만 붙들면 1px 아래로 옮긴다.
      const headerShift = await chat.evaluate((c) => {
        const note = c.querySelector(".chat-note");
        const top = c.scrollTop, before = note.getBoundingClientRect().height;
        note.style.paddingBottom = "1px";
        return new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(() => done({ top, after: c.scrollTop, growth: note.getBoundingClientRect().height - before }))));
      });
      assert.equal(headerShift.growth, 1, "안내 높이 변화 준비");
      assert.equal(headerShift.after, 0, `맨 위에서 안내 재측정: ${JSON.stringify(headerShift)}`);
      // 실제 scroll 사건의 1px도 읽던 자리다. 끝난 입력 뒤 안내가 재측정돼도 그 줄을 지킨다.
      const pixelAnchor = await inTime(chat.evaluate(async (c) => {
        const trusted = await new Promise((done) => {
          c.addEventListener("scroll", (event) => done(event.isTrusted), { once: true });
          c.scrollBy(0, 1);
        });
        const top = c.scrollTop, first = c.querySelector(".chat-row").getBoundingClientRect().top;
        const note = c.querySelector(".chat-note"), before = note.getBoundingClientRect().height;
        note.style.paddingBottom = "2px";
        await new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)));
        return { trusted, top, after: c.scrollTop, first, afterFirst: c.querySelector(".chat-row").getBoundingClientRect().top,
          growth: note.getBoundingClientRect().height - before };
      }), 20_000, "1px 이동 뒤 안내 높이 측정");
      assert.equal(pixelAnchor.trusted, true, "브라우저가 보낸 실제 scroll 사건");
      assert.equal(pixelAnchor.top, 1, "실제 1px 스크롤 준비");
      assert.equal(pixelAnchor.growth, 1, "1px 입력 뒤 안내 재측정 준비");
      assert.equal(pixelAnchor.after, 2, `1px 입력 뒤 읽던 자리: ${JSON.stringify(pixelAnchor)}`);
      assert.equal(pixelAnchor.afterFirst, pixelAnchor.first, "안내 재측정이 읽던 줄을 옮겼다");
      // 이 회귀의 2→0 이동이 낸 scrollend를 다음 손가락의 종료로 받지 않는다.
      await inTime(chat.evaluate((c) => new Promise((done) => {
        c.addEventListener("scrollend", () => done(), { once: true });
        c.scrollTop = 0;
      })), 20_000, "1px 회귀 뒤 맨 위 복귀 완료");
      await shot(page, `shell-chat-${harness}-${engine.browserType().name()}-${width}`);
      // 데스크톱 휠과 Chromium 휴대폰의 실제 터치를 확인한다. 앞 쪽 갱신은 읽던 위치를 지킨다.
      const top = await chat.evaluate((c) => c.scrollTop);
      const box = await chat.boundingBox();
      if (width < 500 && engine === browser) {
        await chat.evaluate((c) => {
          window.shellScrollEnd = new Promise((done) => c.addEventListener("scrollend", () => done(), { once: true }));
        });
        const cdp = await page.context().newCDPSession(page);
        const x = Math.round(box.x + box.width / 2), y = Math.round(box.y + box.height - 50);
        await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x, y, id: 1 }] });
        for (let i = 1; i <= 8; i++) await cdp.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x, y: y - i * 25, id: 1 }] });
        await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
        await inTime(page.evaluate(() => window.shellScrollEnd), 20_000, "손을 뗀 뒤 관성 스크롤 종료");
        await cdp.detach();
      } else if (width < 500) {
        // Playwright의 mobile WebKit은 wheel 입력을 제공하지 않는다. 같은 브라우저 scroll 경로를 쓴다.
        await chat.evaluate((c) => c.scrollBy(0, 300));
      } else {
        await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
        await page.mouse.wheel(0, 300);
      }
      await page.waitForFunction((before) => document.querySelector('[data-testid="chat"]').scrollTop > before, top);
      await chat.evaluate((c) => {
        const native = Object.getOwnPropertyDescriptor(Element.prototype, "scrollTop");
        window.shellResetWrites = [];
        Object.defineProperty(c, "scrollTop", { configurable: true, get() { return native.get.call(this); }, set(value) {
          const before = native.get.call(this); native.set.call(this, value);
          window.shellResetWrites.push({ before, value, after: native.get.call(this), stack: new Error().stack });
        } });
        window.shellResetLayout = { top: c.scrollTop, note: c.querySelector(".chat-note")?.getBoundingClientRect().height, first: c.querySelector(".chat-row")?.getBoundingClientRect().top };
      });
      if (width < 500 && engine === browser) {
        // Chromium의 소수 터치 위치를 scrollTop=0으로 덮으면 scrollend 뒤에도
        // compositor 반올림 잔량(-1, +1)이 온다. 사용자가 처음을 읽듯 실제 터치로 돌아간다.
        const before = await chat.evaluate((c) => {
          window.shellScrollEnd = new Promise((done) => c.addEventListener("scrollend", () => done(), { once: true }));
          return c.scrollTop;
        });
        const cdp = await page.context().newCDPSession(page);
        try {
          await inTime(cdp.send("Input.synthesizeScrollGesture", {
            x: Math.round(box.x + box.width / 2), y: Math.round(box.y + box.height / 2),
            yDistance: before + box.height, gestureSourceType: "touch", preventFling: true,
          }), 20_000, "실제 터치로 맨 위 복귀");
          await inTime(page.evaluate(() => window.shellScrollEnd), 20_000, "맨 위 터치 스크롤 종료");
        } finally { await cdp.detach(); }
      } else {
        await chat.evaluate((c) => { c.scrollTop = 0; c.dispatchEvent(new Event("scroll", { bubbles: true })); });
      }
      const settled = await chat.evaluate((c) => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(() => done({ top: c.scrollTop, first: c.querySelector(".chat-row")?.getBoundingClientRect().top, note: c.querySelector(".chat-note")?.getBoundingClientRect().height, before: window.shellResetLayout, writes: window.shellResetWrites })))));
      if (settled.top !== 0) {
        fs.mkdirSync(`${root}/diagnostics`, { recursive: true });
        fs.writeFileSync(`${root}/diagnostics/shell-scroll-reset.json`, JSON.stringify(settled, null, 2));
      }
      // 실제 두 frame에서 가상 row의 위치 측정을 끝낸 뒤 새 답변을 쓴다.
      assert.equal(settled.top, 0, `${name}: 새 답변을 쓰기 전 자리`);
      const appended = page.waitForResponse(async (r) => {
        if (!r.url().endsWith("/api/v1/call") || r.request().postDataJSON()?.op !== "pane_chat") return false;
        return (await r.json()).ok?.messages?.some((m) => m.text === `${id} 새 답변`);
      });
      aaa("pane", "run", pane, "append");
      const reply = await (await appended).json();
      const added = reply.ok.messages.find((m) => m.text === `${id} 새 답변`).id;
      // 화면 밖 본문은 가상화된다. 새 row의 commit으로 갱신을 확인한다.
      await chat.locator(`[data-chat-id="${added}"]`).waitFor({ state: "attached" });
      assert.equal(await chat.evaluate((c) => c.scrollTop), 0, "갱신이 읽던 위치를 옮겼다");
      await inList(page, "pane-t1");
      await inList(page, `pane-${name}`);
      assert.equal(await toggle.getAttribute("aria-pressed"), "true", "돌아오며 대화 보기 선택이 바뀌었다");
      await chat.getByText(`${id} 기록 0 ·`, { exact: false }).first().waitFor();
      mark("switch");
      // 옛 파일의 맨 위를 읽던 상태를 확정한다. 새 파일은 이 위치를 이어받으면 안 된다.
      await chat.evaluate((c) => { c.scrollTop = 0; c.dispatchEvent(new Event("scroll", { bubbles: true })); });
      diagnostics.remember([{ phase, top: await chat.evaluate((c) => c.scrollTop) }]);
      aaa("pane", "run", pane, `switch ${next}`);
      await until("같은 프로세스의 새 대화 연결", () => state().agent?.harness_session === next);
      await chat.getByText(`${next} 기록 359 ·`, { exact: false }).first().waitFor();
      // 본문이 mount돼도 화면 밖일 수 있다. 새 대화에서는 최신 끝이 실제로 보여야 한다.
      const atEnd = () => {
        const c = document.querySelector('[data-testid="chat"]');
        return c && Math.abs(c.scrollHeight - c.clientHeight - c.scrollTop) <= 1;
      };
      await domIs(page, "새 대화의 최신 끝 표시", atEnd);
      assert.equal(await chat.getByText(`${id} 기록`, { exact: false }).count(), 0, "이전 세션을 섞었다");
      if (width < 500) {
        mark("switch-away");
        await firstMessage(page, next);
        await inList(page, "pane-t1");
        // 기록 파일이 쓰이는 중 감지되는 순서도 고정한다. 첫 300줄 뒤 실제 명령으로 나머지를 쓴다.
        aaa("pane", "run", pane, `switch-paused ${decoy}`);
        await until("떠난 동안 새 대화 연결", () => state().agent?.harness_session === decoy);
        await inList(page, `pane-${name}`);
        await chat.getByText(`${decoy} 기록 299 ·`, { exact: false }).first().waitFor();
        await domIs(page, "쓰는 중 새 파일 복귀의 최신 끝 표시", atEnd);
        aaa("pane", "run", pane, "finish-history");
        await chat.getByText(`${decoy} 기록 359 ·`, { exact: false }).first().waitFor();
        await domIs(page, "새 파일 복귀의 최신 끝 표시", atEnd);
        assert.equal(await chat.getByText(`${next} 기록`, { exact: false }).count(), 0, "옛 파일의 복귀 위치를 섞었다");
      }
      aaa("pane", "run", pane, "quit");
      await waitOutput(pane, "foreground-harness-exited");
      await until("셸 복귀 때 연결 해제", () => !state().agent);
      await page.getByTestId("chat-toggle").waitFor({ state: "detached" });
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
    } catch (error) {
      await diagnoseShell(error);
      throw error;
    } finally {
      diagnostics.stop();
      shellDiagnostics = null;
      await page.context().close();
    }
  };
  await step("셸에서 직접 연 Codex의 대화 연결·스크롤 Chromium 1280", async () => {
    await harnessCase("codex", "chromium", 1280, check);
  }, { diagnose: diagnoseShell });
  await step("셸에서 직접 연 Codex의 대화 연결·스크롤 Chromium 390", async () => {
    await harnessCase("codex", "chromium", 390, check);
  }, { diagnose: diagnoseShell });
  await step("셸에서 직접 연 Codex의 대화 연결·스크롤 WebKit 1280", async () => {
    await harnessCase("codex", "webkit", 1280, check);
  }, { diagnose: diagnoseShell });
  await step("셸에서 직접 연 Codex의 대화 연결·스크롤 WebKit 390", async () => {
    await harnessCase("codex", "webkit", 390, check);
  }, { diagnose: diagnoseShell });
  await step("셸에서 직접 연 Claude의 대화 연결·스크롤 Chromium 1280", async () => {
    await harnessCase("claude", "chromium", 1280, check);
  }, { diagnose: diagnoseShell });
  await step("셸에서 직접 연 Claude의 대화 연결·스크롤 Chromium 390", async () => {
    await harnessCase("claude", "chromium", 390, check);
  }, { diagnose: diagnoseShell });
  await step("셸에서 직접 연 Claude의 대화 연결·스크롤 WebKit 1280", async () => {
    await harnessCase("claude", "webkit", 1280, check);
  }, { diagnose: diagnoseShell });
  await step("셸에서 직접 연 Claude의 대화 연결·스크롤 WebKit 390", async () => {
    await harnessCase("claude", "webkit", 390, check);
  }, { diagnose: diagnoseShell });
  // 조합마다 새 pane·대화·기기를 만들며 앞 조합의 상태를 쓰지 않는다.
  // agent 재시작이 다른 조합에 영향을 주지 않게 병렬 입구는 각 단계를 별도 실험실에 둔다.
  await step("Codex resume 이전 대화·inline 복원·재시작 Chromium 1280", async () => {
    await harnessCase("codex", "chromium", 1280);
  }, { diagnose: diagnoseShell });
  await step("Codex resume 이전 대화·inline 복원·재시작 Chromium 390", async () => {
    await harnessCase("codex", "chromium", 390);
  }, { diagnose: diagnoseShell });
  await step("Codex resume 이전 대화·inline 복원·재시작 WebKit 1280", async () => {
    await harnessCase("codex", "webkit", 1280);
  }, { diagnose: diagnoseShell });
  await step("Codex resume 이전 대화·inline 복원·재시작 WebKit 390", async () => {
    await harnessCase("codex", "webkit", 390);
  }, { diagnose: diagnoseShell });
  await step("Claude resume·continue 이전 대화·복귀·재시작 Chromium 1280", async () => {
    await harnessCase("claude", "chromium", 1280);
  }, { diagnose: diagnoseShell });
  await step("Claude resume·continue 이전 대화·복귀·재시작 Chromium 390", async () => {
    await harnessCase("claude", "chromium", 390);
  }, { diagnose: diagnoseShell });
  await step("Claude resume·continue 이전 대화·복귀·재시작 WebKit 1280", async () => {
    await harnessCase("claude", "webkit", 1280);
  }, { diagnose: diagnoseShell });
  await step("Claude resume·continue 이전 대화·복귀·재시작 WebKit 390", async () => {
    await harnessCase("claude", "webkit", 390);
  }, { diagnose: diagnoseShell });
}
