// 실제 lab JSONL → agent → host → 웹. 앞 쪽 도달과 prepend 위치는 브라우저가 소유한다.
import fs from "node:fs";
import { root, aaa, AA, step, inList, until, domIs, shot, pairPage, browser, webkit, inTime } from "../lib/harness.mjs";
import { ChatDiagnostics } from "../lib/chatDiagnostics.mjs";
export const labEnv = { CODEX_HOME: `${root}/codex` };
const line = (harness, n) => JSON.stringify(harness === "claude"
  ? { type: n % 2 ? "assistant" : "user", message: { content: [{ type: "text", text: `기록 ${n} · ${"긴 대화 줄 ".repeat(n % 5 + 1)}` }] } }
  : { type: "response_item", payload: { type: "message", role: n % 2 ? "assistant" : "user", content: [{ type: n % 2 ? "output_text" : "input_text", text: `기록 ${n} · ${"긴 대화 줄 ".repeat(n % 5 + 1)}` }] } }) + "\n";
// 기존 두 frame 대기에서 전이 기록도 가져온다. 진단 때문에 왕복이나 frame을 더 기다리지 않는다.
const frame = (page, diagnostics) => page.evaluate(() => new Promise((resolve) =>
  requestAnimationFrame(() => requestAnimationFrame(() => resolve(window.takeChatStates())))))
  .then((states) => diagnostics.remember(states));
// 같은 두 frame 경계에서 행 ID·좌표·진단을 함께 읽는다. 각 쪽마다 이 값을
// 따로 조회하면 브라우저와 시험 runner의 여러 왕복 사이에도 레이아웃이 진행된다.
const prependPosition = async (chat, diagnostics, id = null) => {
  const result = await chat.evaluate((el, id) => new Promise(resolve => {
    requestAnimationFrame(() => requestAnimationFrame(() => {
      const row = id ? el.querySelector(`[data-chat-id="${id}"]`)
        : el.querySelector('.chat-transcript > [data-chat-id]');
      resolve({ id: row?.getAttribute('data-chat-id'), y: row?.getBoundingClientRect().y,
        states: window.takeChatStates() });
    }));
  }), id);
  diagnostics.remember(result.states);
  return result;
};
const position = row => row.evaluate(row => {
  const el = row.closest('.chat');
  return { rowY: row.getBoundingClientRect().y, y: el.getBoundingClientRect().y,
    top: el.scrollTop, height: el.scrollHeight, client: el.clientHeight,
    banners: [...document.querySelectorAll('.term:not([hidden]) .banner')].map(el => ({ text: el.textContent, y: el.getBoundingClientRect().y, height: el.getBoundingClientRect().height })) };
});
export default async function () {
  // 독립 Chromium 작업은 사용하지 않는 WebKit 프로세스를 띄우지 않는다.
  let safari;
  const safariBrowser = async () => safari ??= await webkit.launch({ headless: true });
  const releases = new Set();
  let diagnostics;
  const failure = { diagnose: (error) => diagnostics?.save(error, inTime) };
  const check = async (engine, width, harness) => {
    const label = `pages-${engine.browserType().name()}-${width}-${harness}`;
    diagnostics = new ChatDiagnostics(root, label);
    const started = Date.now();
    const phase = (name) => console.log(`${label}: ${name} (${Date.now() - started}ms)`);
    const id = "01900000-0000-7000-8000-00000000a045";
    const dir = harness === "claude" ? `${root}/claude/projects/${label}` : `${root}/codex/sessions/2026/10/20`;
    fs.mkdirSync(dir, { recursive: true });
    const tr = harness === "claude" ? `${dir}/s.jsonl` : `${dir}/rollout-${label}-${id}.jsonl`;
    fs.writeFileSync(tr, Array.from({ length: 2400 }, (_, n) => line(harness, n)).join(""));
    const payload = harness === "claude"
      ? { hook_event_name: "SessionStart", session_id: id, source: "startup", transcript_path: tr }
      : { type: "agent-turn-complete", "thread-id": id };
    aaa("tab", "create", "--machine", "labA", "--label", label, "--", "/bin/sh", "-c", '"$0" hook --agent "$1" "$2"; exec cat', AA, harness, JSON.stringify(payload));
    await until("harness 기록 연결", () => aaa("pane", "list").panes.find((p) => p.name === label)?.agent?.harness === harness);
    const paneId = aaa("pane", "list").panes.find(p => p.name === label).id;
    let gate = null;
    let controlGate = null;
    let paneSocket, paneConnections = 0;
    const ph = await pairPage(engine, aaa("web", "pair").code, { width, height: 900 }, {
      serviceWorkers: "block",
      // 보관 복귀의 연결 유지와, 실제 단절 뒤 늦은 제어 응답을 각각 검사한다.
      wsRoute: [/\/api\/v1\/panes\/[^/]+\/ws/, socket => {
        const server = socket.connectToServer();
        if (socket.url().includes(paneId)) { paneSocket = socket; paneConnections++; }
        socket.onMessage(raw => server.send(raw));
        server.onMessage(raw => {
          if (controlGate && socket.url().includes(paneId)) {
            controlGate.messages.push(() => socket.send(raw));
            if (typeof raw === "string" && JSON.parse(raw).t === "snapshot") controlGate.arrive();
          } else socket.send(raw);
        });
      }],
      setup: async (context) => {
        diagnostics.watch(context);
        if (!process.env.AAA_CHAT_FRAME_MS) return;
        await context.addInitScript((ms) => {
          const busy = () => {
            const end = performance.now() + ms;
            while (performance.now() < end) { /* CI처럼 느린 frame */ }
            requestAnimationFrame(busy);
          };
          requestAnimationFrame(busy);
        }, Number(process.env.AAA_CHAT_FRAME_MS));
      },
      // 실제 callback/실측값으로 ResizeObserver가 scroll 사건보다 먼저 오는 순서도 실행한다.
      init: () => {
        const Native = window.ResizeObserver;
        const callbacks = new Map();
        window.ResizeObserver = class extends Native {
          constructor(callback) { super((entries, observer) => {
            if (entries.some((e) => e.target.classList.contains("chat-row"))) callbacks.set(observer, () => callback(entries, observer));
            callback(entries, observer);
          }); }
          disconnect() { callbacks.delete(this); super.disconnect(); }
        };
        window.flushChatResize = () => { for (const callback of callbacks.values()) callback(); };
        // 논리적인 쪽 commit만 센다. 가상 본문 mount·글자 크기 변화는 다시 세거나 실측하지 않는다.
        let chat, transcript, note, previous;
        const states = [];
        window.takeChatStates = () => states.splice(0);
        new MutationObserver((mutations) => {
          const next = chat?.isConnected ? chat : document.querySelector('[data-testid="chat"]');
          const replaced = next !== chat;
          if (!replaced && !mutations.some((m) => m.target === chat || m.target === transcript || note?.contains(m.target))) return;
          chat = next;
          transcript = chat?.querySelector(".chat-transcript");
          note = chat?.querySelector('[data-testid="chat-history-start"]');
          const state = { present: Boolean(chat), note: note?.textContent, busy: chat?.getAttribute("aria-busy"),
            rows: transcript?.querySelectorAll(":scope > [data-chat-id]").length,
            first: transcript?.querySelector("[data-chat-id]")?.getAttribute("data-chat-id") };
          const key = JSON.stringify(state);
          if (key === previous) return;
          previous = key;
          states.push({ at: Date.now(), ...state });
          if (states.length > 256) states.shift();
        }).observe(document, { subtree: true, childList: true, characterData: true, attributes: true, attributeFilter: ["aria-busy"] });
      },
      route: ["**/api/v1/call", async (route) => {
        const request = route.request().postDataJSON();
        const entry = diagnostics.request(route.request(), request);
        if (request?.op === "pane_chat" && request.before && gate) {
          const held = gate; gate = null;
          const response = await route.fetch();
          const body = await response.json();
          diagnostics.reply(entry, body);
          held.arrive(body);
          await held.released;
          await route.fulfill({ response });
        } else await route.continue();
      }],
    });
    const page = ph.page;
    diagnostics.page = page;
    diagnostics.errors = ph.errors;
    // 목록 drawer의 실제 전환·쓸기는 E39 drawerNavigation이 소유한다.
    // 이 owner는 같은 native click·선택·pane 수명으로 대화 위치를 검사한다.
    // drawer 전환만 즉시 바꾸고 대화의 frame·scroll·refresh 시계는 유지한다.
    await page.addStyleTag({ content: '#workspace-drawer { transition: none !important; }' });
    await inList(page, `pane-${label}`);
    const toggle = page.getByTestId("chat-toggle");
    await toggle.waitFor();
    if ((await toggle.getAttribute("aria-pressed")) !== "true") await toggle.click();
    const chat = page.getByTestId("chat");
    phase("최신 줄");
    await chat.getByText("기록 2399 ·", { exact: false }).first().waitFor();
    // 각 prepend를 실제 HTTP 응답으로 기다리고, 기존 첫 줄의 viewport 좌표를 비교한다.
    for (let pageNo = 0; pageNo < 40; pageNo++) {
      if (await chat.getByText("처음이다", { exact: true }).count()) break;
      let arrive, release;
      const arrived = new Promise((resolve) => { arrive = resolve; });
      const released = new Promise((resolve) => { release = resolve; });
      releases.add(release);
      gate = { arrive, released };
      await chat.evaluate((el) => { el.scrollTop = 0; window.flushChatResize(); el.dispatchEvent(new Event("scroll", { bubbles: true })); });
      const body = await inTime(arrived, 20_000, `${label} 이전 쪽 요청`);
      // 응답은 아직 gate 뒤다. 동작 전의 원래 row와 좌표를 잡는다.
      const before = await prependPosition(chat, diagnostics);
      if (!before.id) throw new Error(`${label}: prepend 전 원래 행이 없다`);
      release();
      const added = body.ok?.messages[0]?.id;
      if (!added) throw new Error(`${label}: 메시지 없는 합성 쪽`);
      await chat.locator(`[data-chat-id="${added}"]`).waitFor({ state: "attached" });
      const after = await prependPosition(chat, diagnostics, before.id);
      if (!after.id || Math.abs(after.y - before.y) > 1) throw new Error(`${label}: prepend 위치 ${before.y} → ${after.y}`);
    }
    await chat.evaluate((el) => { el.scrollTop = 0; });
    try { await chat.getByText("기록 0 ·", { exact: false }).first().waitFor(); }
    catch (error) {
      console.log("first-missing", label, await chat.evaluate((el) => ({ top: el.scrollTop, height: el.scrollHeight, note: el.querySelector(".chat-note")?.textContent, first: [...el.querySelectorAll(".chat-row")].slice(0, 3).map((row) => ({ id: row.dataset.chatId, text: row.textContent, y: row.getBoundingClientRect().top })), end: el.querySelector('[data-testid="chat-history-start"]')?.textContent })));
      await page.screenshot({ path: `${root}/first-missing.png` });
      throw error;
    }
    await chat.getByText("처음이다", { exact: true }).waitFor();
    phase("처음 도달");
    if ((await chat.innerText()).includes("앞부분은 생략했다")) throw new Error("처음에서 생략 안내");
    await frame(page, diagnostics);
    const startBox = await chat.getByText("처음이다", { exact: true }).boundingBox();
    const viewBox = await chat.boundingBox();
    if (!startBox || startBox.y < viewBox.y || startBox.y + startBox.height > viewBox.y + viewBox.height) throw new Error(`${label}: 처음 표지가 viewport 밖`);
    const mounted = await chat.locator(".markdown").count();
    if (mounted > 300) throw new Error(`긴 기록 본문 mount 무제한: ${mounted}`);
    await shot(page, `${label}-beginning`);
    const restoreRow = chat.locator(".chat-row").nth(40);
    const controlled = page.getByTestId('terminal').and(page.locator('[data-readonly="false"]'));
    await controlled.waitFor({ state: 'attached' });
    await page.getByTestId('readonly-banner').waitFor({ state: 'hidden' });
    // shell은 항상 있다. 실제 스크롤로 본문이 화면 근처에서 mount되게 한다.
    await restoreRow.evaluate((row) => { const el = row.closest(".chat"); el.scrollTop += row.getBoundingClientRect().top - el.getBoundingClientRect().top + 20; el.dispatchEvent(new Event("scroll", { bubbles: true })); });
    await frame(page, diagnostics);
    const restoreId = await restoreRow.getAttribute("data-chat-id");
    const beforePosition = await position(restoreRow);
    const restoreY = beforePosition.rowY;
    diagnostics.remember([{ at: Date.now(), phase: "before pane switch", restoreId, restoreY,
      layout: beforePosition }]);
    const retainedConnections = paneConnections;
    const terminalNode = await controlled.elementHandle();
    await inList(page, "pane-t1");
    await inList(page, `pane-${label}`);
    await controlled.waitFor({ state: 'attached' });
    await page.getByTestId('readonly-banner').waitFor({ state: 'hidden' });
    await frame(page, diagnostics);
    const retainedPosition = await position(restoreRow);
    if (paneConnections !== retainedConnections) throw new Error(`${label}: 보관 복귀에서 새 연결`);
    if (!await controlled.evaluate((el, previous) => el === previous, terminalNode)) throw new Error(`${label}: 보관 터미널 교체`);
    await terminalNode.dispose();
    if (Math.abs(retainedPosition.rowY - restoreY) > 1) throw new Error(`${label}: 보관 복귀 row40 위치 ${restoreY} → ${retainedPosition.rowY}`);
    diagnostics.remember([{ at: Date.now(), phase: 'retained return', layout: retainedPosition }]);
    // 실제로 끊긴 경우도 기존의 제어 응답 전/후 위치 계약을 유지한다.
    await inList(page, "pane-t1");
    let arriveControl;
    const controlArrived = new Promise(resolve => { arriveControl = resolve; });
    controlGate = { messages: [], arrive: arriveControl };
    const releaseControl = () => {
      const held = controlGate;
      controlGate = null;
      for (const send of held?.messages ?? []) send();
    };
    releases.add(releaseControl);
    paneSocket.close();
    await page.locator(`.term[data-pane="${paneId}"][hidden] .term-box[data-readonly="true"]`).waitFor({ state: 'attached' });
    await inList(page, `pane-${label}`);
    phase("row40 복원");
    await inTime(controlArrived, 20_000, `${label} 복귀 제어 응답`);
    if (paneConnections !== retainedConnections + 1) throw new Error(`${label}: 단절 뒤 새 연결 한 번이 아님`);
    await page.getByTestId('readonly-banner').waitFor();
    await page.getByTestId("chat").and(page.locator('[aria-busy="false"]')).waitFor();
    await page.getByTestId("chat").locator(`[data-chat-id="${restoreId}"] .markdown`).waitFor();
    await frame(page, diagnostics);
    const waitingPosition = await position(restoreRow);
    diagnostics.remember([{ at: Date.now(), phase: 'control pending', layout: waitingPosition }]);
    const beforeOffset = beforePosition.rowY - beforePosition.y;
    const waitingOffset = waitingPosition.rowY - waitingPosition.y;
    if (Math.abs(waitingOffset - beforeOffset) > 1) throw new Error(`${label}: 제어 응답 전 row40 위치 ${beforeOffset} → ${waitingOffset}`);
    releaseControl();
    await controlled.waitFor({ state: 'attached' });
    await page.getByTestId('readonly-banner').waitFor({ state: 'hidden' });
    await frame(page, diagnostics);
    const afterPosition = await position(page.getByTestId("chat").locator(`[data-chat-id="${restoreId}"]`));
    const restoredY = afterPosition.rowY;
    diagnostics.remember([{ at: Date.now(), phase: "after pane switch", restoredY,
      layout: afterPosition }]);
    if (Math.abs(restoredY - restoreY) > 1) throw new Error(`${label}: pane 왕복 row40 위치 ${restoreY} → ${restoredY}`);
    console.log(`${label} restore`, JSON.stringify({ before: beforePosition, controlPending: waitingPosition, after: afterPosition }));
    await chat.evaluate((el) => { el.scrollTop = 0; window.flushChatResize(); el.dispatchEvent(new Event("scroll", { bubbles: true })); });
    await chat.getByText("기록 0 ·", { exact: false }).first().waitFor();
    // 최신 읽기 사이에 두 쪽 이상 자랐어도 중간 400줄을 빠뜨리지 않는다.
    fs.appendFileSync(tr, Array.from({ length: 400 }, (_, n) => line(harness, 2400 + n)).join(""));
    phase("최신 gap");
    await until("최신 gap도 전부 합침", async () => (await chat.locator(".chat-row").count()) === 2800);
    await frame(page, diagnostics);
    const firstAt = await chat.getByText("기록 0 ·", { exact: false }).first().boundingBox();
    if (!firstAt || firstAt.y < (await chat.boundingBox()).y) throw new Error("위를 읽는 중 최신 gap이 자리를 바꿈");
    await page.getByTestId("chat-jump").click();
    phase("최신 따라가기");
    await domIs(page, "끝 따라가기", () => { const e = document.querySelector('[data-testid="chat"]'); return e.scrollHeight - e.scrollTop - e.clientHeight <= 40; });
    fs.appendFileSync(tr, line(harness, 2800));
    try { await chat.getByText("기록 2800 ·", { exact: false }).first().waitFor(); }
    catch (error) {
      console.log("follow-missing", label, await chat.evaluate((el) => ({
        top: el.scrollTop, height: el.scrollHeight, client: el.clientHeight,
        tail: el.style.getPropertyValue("--chat-tail-space"), rows: el.querySelectorAll(".chat-row").length,
        last: [...el.querySelectorAll(".chat-row")].slice(-3).map((row) => ({
          id: row.dataset.chatId, text: row.textContent, y: row.getBoundingClientRect().top, height: row.getBoundingClientRect().height,
        })), error: el.querySelector(".banner")?.textContent,
      })));
      await page.screenshot({ path: `${root}/${label}-follow-missing.png` });
      throw error;
    }
    await domIs(page, "새 기록 따라가기", () => { const e = document.querySelector('[data-testid="chat"]'); return e.scrollHeight - e.scrollTop - e.clientHeight <= 40; });
    // 즉시 끝 이동(End)도 pane별 따라가기 기억에 반영한다. smooth 단추 경로는 기존 E2E30이 소유한다.
    await chat.evaluate((el) => { el.scrollTop = 0; el.dispatchEvent(new Event("scroll", { bubbles: true })); });
    await chat.getByText("기록 0 ·", { exact: false }).first().waitFor();
    await frame(page, diagnostics);
    await chat.focus();
    await page.keyboard.press("End");
    await domIs(page, "End로 즉시 실제 끝", () => { const e = document.querySelector('[data-testid="chat"]'); return e.scrollHeight - e.scrollTop - e.clientHeight <= 40; });
    await inList(page, "pane-t1");
    await inList(page, `pane-${label}`);
    await page.getByTestId("chat").and(page.locator('[aria-busy="false"]')).waitFor();
    await domIs(page, "End 뒤 pane 왕복도 끝 따라가기", () => { const e = document.querySelector('[data-testid="chat"]'); return e.scrollHeight - e.scrollTop - e.clientHeight <= 40; });
    if (width === 390 && harness === "claude") {
      // 최신 쪽이 화면보다 짧아 스크롤이 없어도 앞 쪽을 자동으로 채우며 그 줄을 보존한다.
      let arrive, release;
      const arrived = new Promise((resolve) => { arrive = resolve; });
      const released = new Promise((resolve) => { release = resolve; });
      releases.add(release);
      gate = { arrive, released };
      // 느린 WebKit은 짧은 최신 쪽으로 줄며 생긴 scroll을 자동 앞 쪽 읽기 뒤에 보낼 수 있다.
      // 실제 브라우저 scroll만 잠깐 보류해 그 순서를 재현한다. 응답도 기존 gate 뒤에서 기다린다.
      await chat.evaluate((el) => {
        let arrive;
        const previousTop = el.scrollTop;
        window.shortScroll = new Promise((resolve) => { arrive = resolve; });
        const delay = (event) => {
          if (el.querySelectorAll(".chat-row").length !== 1) return;
          event.stopImmediatePropagation();
          arrive({ previousTop, top: el.scrollTop, height: el.scrollHeight, client: el.clientHeight });
        };
        el.addEventListener("scroll", delay, true);
        window.releaseShortScroll = () => {
          el.removeEventListener("scroll", delay, true);
          el.dispatchEvent(new Event("scroll", { bubbles: true }));
        };
        const busy = () => {
          const end = performance.now() + 25;
          while (performance.now() < end) { /* 느린 CPU의 frame 점유 */ }
          if (!window.shortCpuDone) requestAnimationFrame(busy);
        };
        requestAnimationFrame(busy);
      });
      fs.writeFileSync(`${tr}.replacement`, Array.from({ length: 300 }, (_, n) => line(harness, n)).join("")
        + JSON.stringify({ type: "blob", data: "x".repeat(17 << 20) }) + "\n" + line(harness, 9000));
      fs.renameSync(tr, `${tr}.old`);
      fs.renameSync(`${tr}.replacement`, tr);
      await inTime(arrived, 20_000, `${label} 짧은 최신 쪽 자동 채움`);
      const short = chat.getByText("기록 9000 ·", { exact: false }).first();
      await short.waitFor();
      await frame(page, diagnostics);
      const beforeY = (await short.boundingBox()).y;
      const clamped = await inTime(page.evaluate(() => window.shortScroll), 20_000, "짧은 쪽으로 줄며 생긴 scroll");
      await page.evaluate(() => window.releaseShortScroll());
      release();
      await domIs(page, "짧은 최신 쪽의 이전 쪽 commit", () => document.querySelectorAll('[data-testid="chat"] .chat-row').length === 151);
      await frame(page, diagnostics);
      const afterY = (await short.boundingBox()).y;
      await page.evaluate(() => { window.shortCpuDone = true; });
      const closes = await page.evaluate(() => window.__aaaTermLog.filter((entry) => entry.e === "ws close 1006").length);
      console.log(`${label} short-prepend`, JSON.stringify({ clamped, beforeY, afterY, terminalCloses1006: closes }));
      if (Math.abs(afterY - beforeY) > 1) throw new Error(`${label}: 짧은 쪽 prepend 위치 ${beforeY} → ${afterY}`);
      await shot(page, `${label}-short-prepend`);
      await chat.focus();
      await page.keyboard.press("End");
      await domIs(page, "수동 맨 아래는 실제 메시지 끝", () => {
        const el = document.querySelector('[data-testid="chat"]');
        const last = el.querySelector(".chat-row:last-child");
        return !el.style.getPropertyValue("--chat-tail-space") && last.getBoundingClientRect().bottom <= el.getBoundingClientRect().bottom;
      });
      fs.unlinkSync(`${tr}.old`);
    }
    if (ph.errors.length) throw new Error(ph.errors.join("; "));
    diagnostics.stop();
    await page.context().close();
    aaa("pane", "kill", label);
    fs.unlinkSync(tr);
  };
  try {
    // 독립된 조합마다 같은 상한을 적용한다. literal 이름으로 --list/--grep에도 모두 보인다.
  await step("긴 대화 앞 쪽: Chromium 1280 claude", async () => check(browser, 1280, "claude"), failure);
  await step("긴 대화 앞 쪽: Chromium 1280 codex", async () => check(browser, 1280, "codex"), failure);
  await step("긴 대화 앞 쪽: Chromium 390 claude", async () => check(browser, 390, "claude"), failure);
  await step("긴 대화 앞 쪽: Chromium 390 codex", async () => check(browser, 390, "codex"), failure);
  await step("긴 대화 앞 쪽: Chromium 820 claude", async () => check(browser, 820, "claude"), failure);
  await step("긴 대화 앞 쪽: Chromium 820 codex", async () => check(browser, 820, "codex"), failure);
  await step("긴 대화 앞 쪽: WebKit 1280 claude", async () => check(await safariBrowser(), 1280, "claude"), failure);
  await step("긴 대화 앞 쪽: WebKit 1280 codex", async () => check(await safariBrowser(), 1280, "codex"), failure);
  await step("긴 대화 앞 쪽: WebKit 390 claude", async () => check(await safariBrowser(), 390, "claude"), failure);
  await step("긴 대화 앞 쪽: WebKit 390 codex", async () => check(await safariBrowser(), 390, "codex"), failure);
  await step("긴 대화 앞 쪽: WebKit 820 claude", async () => check(await safariBrowser(), 820, "claude"), failure);
  await step("긴 대화 앞 쪽: WebKit 820 codex", async () => check(await safariBrowser(), 820, "codex"), failure);
  } finally { for (const release of releases) release(); await safari?.close(); }
}
