// 시험용 셸 → hook 없는 harness → PID/기록 → host → 웹의 고유 연결 계약.
// 같은 pane의 직접 실행→재개는 한 수명으로 잇고, 일반 화면·왕복은 E2E07/30/37/103/104/109가 맡는다.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
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

async function resumed(engine, width, harness, { page, pane, id, name, mark }) {
  const state = () => aaa("pane", "list").panes.find((p) => p.id === pane);
  const toggle = page.getByTestId("chat-toggle"), chat = page.getByTestId("chat");
  const terminal = async () => {
    await toggle.waitFor();
    if (await toggle.getAttribute("aria-pressed") === "true") await toggle.click();
    await until("재개 화면 그리기", async () => (await screenText(page)).includes("foreground-harness-ready"));
  };
  const quit = async () => {
    aaa("pane", "run", pane, "quit");
    await waitOutput(pane, "foreground-harness-exited");
    await until("재개 전 셸 복귀", () => !state().agent);
  };
  const modes = harness === "codex" ? ["resume"] : ["--resume", "--continue"];
  for (const mode of modes) {
    mark(`${mode}:terminal`);
    aaa("pane", "run", pane, `${root}/shellbin/${harness} ${root} ${mode}${mode === "--continue" ? "" : ` ${id}`} --mouse`);
    await waitOutput(pane, "foreground-harness-ready");
    await until(`${mode}로 같은 대화 연결`, () => state().agent?.harness_session === id);
    await terminal();
    const footer = page.locator(".xterm-rows > div").filter({ hasText: "fixed-composer>" });
    const fixed = await footer.boundingBox();
    // 복귀 반복은 E2E109가 맡고, 여기서는 실제 셸 프로그램의 입력을 한 번 확인한다.
    {
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
      mark(`${mode}:terminal:return`);
      await inList(page, "pane-t1");
      await inList(page, `pane-${name}`);
      await terminal();
      await until("alternate 화면 사본 복원", async () => (await screenText(page)).includes("app-history-") && (await screenText(page)).includes("fixed-composer>"));
    }
    // VT200은 휠뿐 아니라 클릭 누름·뗌도 보낸다. 복귀한 터미널을 실제로 누른 뒤
    // 대화·agent 재시작을 거쳐 quit해도 시험 harness의 입력 해석이 막히면 안 된다.
    mark(`${mode}:terminal:click`);
    await page.locator(".xterm-screen").click();
    await toggle.waitFor();
    const newest = page.waitForResponse((r) => r.url().endsWith("/api/v1/call") && r.request().postDataJSON()?.op === "pane_chat" && !r.request().postDataJSON()?.before);
    if (await toggle.getAttribute("aria-pressed") !== "true") await toggle.click();
    mark(`${mode}:chat`);
    const reply = await (await newest).json();
    assert.ok(reply.ok?.messages?.some((message) => message.text.startsWith(`${id} 기록 359 ·`)), "재개한 최신 기록이 없다");
    // 같은 pane의 일반 위치·보기 복귀는 E2E30/37/103/104가 검사한다.
    // 새 cursor의 전체 재독은 harness마다 마지막 mode에서 한 번만 검사한다.
    if (mode === modes.at(-1)) {
      // 보관된 옛 「처음이다」가 재시작 뒤 새 cursor 검사를 건너뛰게 해서는 안 된다.
      await firstMessage(page, id);
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
      const revalidated = page.waitForResponse((r) => r.url().endsWith("/api/v1/call") && r.request().postDataJSON()?.op === "pane_chat" && r.request().postDataJSON()?.pane === pane && !r.request().postDataJSON()?.before);
      await inList(page, `pane-${name}`);
      // 보관 본문은 API보다 먼저 보인다. 재시작의 새 cursor 검사는 이번 재검증의 commit 뒤에 시작한다.
      const latestHistory = (await (await revalidated).json()).ok.history;
      await domIs(page, "재시작 재검증 commit", history => document.querySelector('[data-testid="chat"] [data-chat-id]')?.getAttribute("data-chat-id")?.startsWith(`${history}:`), latestHistory);
      // 재시작은 기존 byte cursor를 무효화한다(chat_pages). 새 cursor로 이전 대화 전체를 다시 읽을 수 있어야 한다.
      const composer = page.getByTestId("chat-composer"), before = await composer.boundingBox();
      await firstMessage(page, id);
      assert.deepEqual(await composer.boundingBox(), before, "재개한 이전 대화를 굴리며 입력창이 움직였다");
      await inList(page, "pane-t1");
      await inList(page, `pane-${name}`);
      assert.equal(await toggle.getAttribute("aria-pressed"), "true", "재개 뒤 보기 선택 소실");
      await chat.getByText(`${id} 기록 0 ·`, { exact: false }).first().waitFor();
    }
    await shot(page, `${name}-${mode}-${width}`);
    mark(`${mode}:quit`);
    await quit();
    const trace = fs.readdirSync(`${root}/shell-input`).map((file) => JSON.parse(fs.readFileSync(`${root}/shell-input/${file}`, "utf8")))
      .find((trace) => trace.session === id && trace.harness === harness && trace.mode === mode);
    for (const final of ["M", "m"]) {
      assert.ok(trace?.events.some((event) => event.kind === "control" && event.button === 0 && event.final === final), "실제 클릭 보고를 받지 않았다");
    }
    assert.ok(trace.events.some((event) => event.kind === "command" && event.command === "quit"), "quit을 명령으로 읽지 않았다");
    assert.equal(trace.events.at(-1).kind, "exited", "시험 harness의 출력·정리가 끝나지 않았다");
  }
  mark("done");
}

function diagnoseShell(error) {
  const diagnostics = shellDiagnostics;
  if (!diagnostics) return;
  if (!diagnostics.saving) {
    const current = diagnostics.states.findLast((state) => state.phase);
    diagnostics.errors = [{ ...current, error: String(error) }];
    diagnostics.saving = Promise.resolve().then(async () => {
      // 브라우저 진단만으로 PTY 전달과 시험 입력 해석을 구별할 수 없다. 바깥 단계의
      // 시한에도 같은 진단을 남기며 수집 오류가 최초 시험 오류를 덮지 않게 한다.
      const dir = `${root}/diagnostics/${diagnostics.label}`;
      fs.mkdirSync(dir, { recursive: true });
      const capture = (name, fn) => {
        try { fn(); }
        catch (captureError) { fs.writeFileSync(`${dir}/${name}-error.txt`, String(captureError)); }
      };
      capture("pane", () => fs.writeFileSync(`${dir}/pane.json`, JSON.stringify({
        load: os.loadavg(), pane: aaa("pane", "list").panes.find((pane) => pane.id === diagnostics.pane),
        recent: aaa("pane", "read", diagnostics.pane, "--source", "recent", "--lines", "100").text,
      }, null, 2)));
      capture("session", () => fs.copyFileSync(`${root}/s/agent/sessions/${diagnostics.pane}/session-host.log`, `${dir}/session-host.log`));
      capture("input", () => fs.cpSync(`${root}/shell-input`, `${dir}/shell-input`, { recursive: true }));
      await diagnostics.save(error, inTime);
    }).catch((captureError) => { console.error(`셸 실패 진단 저장 오류: ${captureError}`); });
  }
  return diagnostics.saving;
}

async function harnessCase(harness, kind, width, run) {
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
    // 표시 사본과 실제 xterm의 복귀 경합을 관측한다. 휠·준비 순서를 바꾸지 않는다.
    await page.evaluate(() => {
      window.shellTerminalWheels = [];
      document.addEventListener('wheel', event => {
        const box = document.querySelector('[data-testid="terminal"]');
        if (!box?.contains(event.target)) return;
        window.shellTerminalWheels.push({ at: performance.now(), target: event.target.className,
          readonly: box.dataset.readonly, preview: Boolean(box.querySelector('.terminal-preview')),
          terminals: [...box.querySelectorAll('.xterm')].map(el => ({ preview: el.classList.contains('terminal-preview'), visibility: getComputedStyle(el).visibility })),
          focus: document.activeElement?.className });
      }, { capture: true, passive: true });
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
      const pane = aaa("tab", "create", "--machine", "labA", "--label", name, "--", "/bin/sh").tabs.at(-1).root.pane;
      diagnostics.pane = pane;
      await inList(page, `pane-${name}`);
      aaa("pane", "run", pane, `${root}/shellbin/${harness} ${root} ${id} --inline`);
      await waitOutput(pane, "foreground-harness-ready");
      await until("직접 실행한 harness 감지", () => aaa("pane", "list").panes.find((p) => p.id === pane)?.agent?.harness === harness);
      const state = () => aaa("pane", "list").panes.find((p) => p.id === pane);
      assert.equal(state().agent.harness_session, null, "최근 기록을 추측해 연결했다");
      const toggle = page.getByTestId("chat-toggle");
      await toggle.waitFor();
      if ((await toggle.getAttribute("aria-pressed")) === "true") await toggle.click();
      await until("연결 전 터미널 화면", async () => (await screenText(page)).includes("foreground-harness-ready"));
      const read = () => aaa("pane", "read", pane, "--source", "recent", "--lines", "1000").text;
      assert.ok(read().includes("terminal-history-0"), "서버 VT가 입력줄 위에서 밀린 기록을 잃었다");
      await inList(page, "pane-t1");
      await inList(page, `pane-${name}`);
      await until("inline 복귀 화면", async () => (await screenText(page)).includes("foreground-harness-ready"));
      await domIs(page, "inline 사본의 기록·화면 읽기 완료", () => Number(document.querySelector('[data-testid="terminal"]')?.dataset.viewBase) > 0);
      await page.getByTestId("terminal").evaluate((el) => el.querySelector(".xterm").dispatchEvent(new WheelEvent("wheel", { bubbles: true, cancelable: true, deltaY: -10000, deltaMode: WheelEvent.DOM_DELTA_LINE })));
      await until("연결 전 inline 기록 복원", async () => (await screenText(page)).includes("terminal-history-0"));
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
      mark("scroll:header");
      // 앞 안내의 높이가 늦게 재측정되는 경계. 맨 위에서 첫 row의 위치만 붙들면 1px 아래로 옮긴다.
      const headerShift = await chat.evaluate((c) => {
        const note = c.querySelector(".chat-note");
        const top = c.scrollTop, before = note.getBoundingClientRect().height;
        note.style.paddingBottom = "1px";
        return new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(() => done({ top, after: c.scrollTop, growth: note.getBoundingClientRect().height - before }))));
      });
      assert.equal(headerShift.growth, 1, "안내 높이 변화 준비");
      assert.equal(headerShift.after, 0, `맨 위에서 안내 재측정: ${JSON.stringify(headerShift)}`);
      mark("scroll:pixel-anchor");
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
      mark("scroll:input");
      // 데스크톱 휠과 Chromium 휴대폰의 실제 터치를 확인한다. 앞 쪽 갱신은 읽던 위치를 지킨다.
      const top = await chat.evaluate((c) => c.scrollTop);
      const box = await chat.boundingBox();
      if (width < 500 && engine === browser) {
        await chat.evaluate((c) => {
          window.shellScrollEnd = new Promise((done) => c.addEventListener("scrollend", () => done(), { once: true }));
        });
        const cdp = await page.context().newCDPSession(page);
        const x = Math.round(box.x + box.width / 2), y = Math.round(box.y + box.height - 50);
        try {
          // 개별 CDP move의 송신 간격은 부하에 따라 달라져 반대 방향 fling도 만든다.
          // 브라우저가 실제 touch를 일정한 속도로 보내고 손을 멈춘 뒤 떼게 한다.
          await inTime(cdp.send("Input.synthesizeScrollGesture", {
            x, y, yDistance: -200, gestureSourceType: "touch", preventFling: true,
          }), 20_000, "실제 터치로 대화 스크롤");
          await inTime(page.evaluate(() => window.shellScrollEnd), 20_000, "대화 터치 스크롤 종료");
        } finally { await cdp.detach(); }
      } else if (width < 500) {
        // Playwright의 mobile WebKit은 wheel 입력을 제공하지 않는다. 같은 브라우저 scroll 경로를 쓴다.
        await chat.evaluate((c) => c.scrollBy(0, 300));
      } else {
        await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
        await page.mouse.wheel(0, 300);
      }
      await page.waitForFunction((before) => document.querySelector('[data-testid="chat"]').scrollTop > before, top);
      mark("scroll:return");
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
      mark("append");
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
      const atEnd = () => {
        const c = document.querySelector('[data-testid="chat"]');
        return c && Math.abs(c.scrollHeight - c.clientHeight - c.scrollTop) <= 1;
      };
      // 파일 교체의 웹 위치 규칙은 공통이다. Codex는 보는 중 FD 교체,
      // Claude는 떠난 동안 PID 등록 교체·쓰는 중 파일 복귀를 한 번씩 지킨다.
      if (harness === "codex") {
        mark("switch");
        aaa("pane", "run", pane, `switch ${next}`);
        await until("같은 프로세스의 새 대화 연결", () => state().agent?.harness_session === next);
      } else {
        mark("switch-away");
        await inList(page, "pane-t1");
        aaa("pane", "run", pane, `switch-paused ${next}`);
        await until("떠난 동안 새 대화 연결", () => state().agent?.harness_session === next);
        await inList(page, `pane-${name}`);
        assert.equal(await toggle.getAttribute("aria-pressed"), "true", "돌아오며 대화 보기 선택이 바뀌었다");
        await chat.getByText(`${next} 기록 299 ·`, { exact: false }).first().waitFor();
        await domIs(page, "쓰는 중 새 파일 복귀의 최신 끝 표시", atEnd);
        aaa("pane", "run", pane, "finish-history");
      }
      await chat.getByText(`${next} 기록 359 ·`, { exact: false }).first().waitFor();
      await domIs(page, "새 대화의 최신 끝 표시", atEnd);
      assert.equal(await chat.getByText(`${id} 기록`, { exact: false }).count(), 0, "이전 세션을 섞었다");
      aaa("pane", "run", pane, "quit");
      await waitOutput(pane, "foreground-harness-exited");
      await until("셸 복귀 때 연결 해제", () => !state().agent);
      await page.getByTestId("chat-toggle").waitFor({ state: "detached" });
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
      await resumed(engine, width, harness, { page, pane, id: next, name, mark });
    } catch (error) {
      await diagnoseShell(error);
      fs.mkdirSync(`${root}/diagnostics`, { recursive: true });
      const view = await inTime(page.evaluate(() => ({
        terminal: { ...document.querySelector('[data-testid="terminal"]')?.dataset },
        screen: document.querySelector(".xterm-rows")?.textContent,
        viewport: [...document.querySelectorAll(".xterm-viewport")].map((el) => ({ top: el.scrollTop, height: el.scrollHeight, client: el.clientHeight })),
        wheels: window.shellTerminalWheels,
        log: window.__aaaTermLog?.slice(-80),
        chat: document.querySelector('[data-testid="chat"]')?.textContent?.slice(0, 2000),
      })), 5000, "재개 실패 화면 수집").catch((error) => ({ captureError: String(error) }));
      fs.writeFileSync(`${root}/diagnostics/${name}.json`, JSON.stringify({ phase, width, engine: engine.browserType().name(), view }, null, 2));
      throw error;
    } finally {
      const phases = diagnostics.states.filter((state) => state.phase);
      const closes1006 = await page.evaluate(() => window.__aaaTermLog.filter((entry) => entry.e === "ws close 1006").length).catch(() => null);
      console.log("셸 진행", JSON.stringify({ name, width, engine: engine.browserType().name(), phase,
        phases: phases.map((state) => [state.phase, state.at - diagnostics.started]),
        connections: connection.length, closes1006,
        samePaneReconnects: connection.filter((entry, i) => i > 0 && entry.pane === connection[i - 1].pane).length }));
      diagnostics.stop();
      shellDiagnostics = null;
      await page.context().close();
    }
  };
  // 두 harness는 별도 실험실이다. 각 수명 안에서는 같은 pane/기록을 직접 실행→재개로 잇는다.
  await step("셸 Codex 직접 연결·교체·resume·PID 복구 Chromium 390", async () => {
    await harnessCase("codex", "chromium", 390, check);
  }, { diagnose: diagnoseShell });
  await step("셸 Claude 직접 연결·숨김 교체·resume·continue·PID 복구 WebKit 390", async () => {
    await harnessCase("claude", "webkit", 390, check);
  }, { diagnose: diagnoseShell });
}
