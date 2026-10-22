// 실제 긴 JSONL과 pane 연결을 쓴다. 브라우저 시계와 open/message 사건 gate로 499/500ms를 검사한다.
import fs from "node:fs";
import assert from "node:assert/strict";
import { root, AA, aaa, step, until, domIs, shot, pairPage, browser, webkit, inTime } from "../lib/harness.mjs";

function gatePaneConnection() {
  const Native = window.WebSocket;
  window.holdPaneOpen = false;
  window.WebSocket = class extends Native {
    constructor(url, protocols) {
      super(url, protocols);
      if (!String(url).includes("/panes/")) return;
      window.gatedPane = this;
      window.paneOpenHeld = false;
      const queued = [];
      const hold = (event) => {
        if (!window.holdPaneOpen) return;
        event.stopImmediatePropagation();
        // stopImmediatePropagation으로 막은 원본 사건을 재사용하면 전파 금지도 남는다.
        queued.push(event.type === "open" ? new Event("open") : new MessageEvent("message", { data: event.data }));
        if (event.type === "open") {
          window.paneOpenHeld = true;
          window.paneOpenArrived?.();
          window.paneOpenArrived = null;
        }
      };
      this.addEventListener("open", hold);
      this.addEventListener("message", hold);
      window.releasePaneOpen = () => {
        window.holdPaneOpen = false;
        for (const event of queued.splice(0)) this.dispatchEvent(event);
        window.paneOpenHeld = false;
      };
    }
  };
  // 순간 나타났다가 사라지는 띠도 기록한다.
  window.connectionBanners = [];
  new MutationObserver(() => {
    const text = document.querySelector('[data-testid="readonly-banner"]')?.textContent;
    if (text?.includes("연결하는 중")) window.connectionBanners.push(text);
  }).observe(document, { subtree: true, childList: true, characterData: true });
}

export default async function () {
  const dir = `${root}/claude/projects/banner`;
  fs.mkdirSync(dir, { recursive: true });
  const transcript = `${dir}/chat.jsonl`;
  fs.writeFileSync(transcript, Array.from({ length: 2400 }, (_, n) => JSON.stringify({
    type: "assistant", message: { content: [{ type: "text", text: `연결 시험 기록 ${n}` }] },
  }) + "\n").join(""));
  aaa("tab", "create", "--machine", "labA", "--label", "banner-chat", "--", "/bin/sh", "-c",
    '"$0" hook --agent claude "$1"; exec cat', AA,
    JSON.stringify({ hook_event_name: "SessionStart", session_id: "01900000-0000-7000-8000-00000000a084", source: "startup", transcript_path: transcript }));
  const pane = await until("대화 기록 연결", () => aaa("pane", "list").panes.find((p) => p.name === "banner-chat" && p.agent?.harness === "claude"));
  let safari;
  const check = async (engine, width) => {
    const { page, errors } = await pairPage(engine, aaa("web", "pair").code, { width, height: 900 }, {
      init: gatePaneConnection,
    });
    const banner = page.getByTestId("readonly-banner");
    const waitOpen = () => inTime(page.evaluate(() => window.paneOpenHeld || new Promise((resolve) => { window.paneOpenArrived = resolve; })), 20_000, "실제 pane open 사건 보류");
    const select = async (id) => {
      await page.evaluate((id) => { location.hash = `pane=${encodeURIComponent(id)}`; }, id);
      await page.locator(`[data-testid="terminal"][data-pane-name="${id === pane.id ? "banner-chat" : "t1"}"]`).waitFor({ state: "attached" });
    };
    const openHeld = async () => {
      await page.evaluate(() => { window.holdPaneOpen = true; window.connectionBanners = []; });
      await select(pane.id);
      const toggle = page.getByTestId("chat-toggle");
      if (await toggle.getAttribute("aria-pressed") !== "true") await toggle.evaluate((el) => el.click());
      await page.getByTestId("chat").getByText("연결 시험 기록 2399", { exact: true }).waitFor();
      await waitOpen();
    };
    try {
      await domIs(page, "첫 셸 연결 완료", () => document.querySelector('[data-testid="terminal"]')?.getAttribute("data-readonly") === "false");
      await page.clock.pauseAt(new Date());
      await openHeld();
      await page.clock.runFor(499);
      assert.equal(await banner.count(), 0, "499ms까지 연결 띠 없음");
      await page.evaluate(() => window.releasePaneOpen());
      await page.clock.runFor(1);
      await domIs(page, "빠른 연결 입력 가능", () => document.querySelector('[data-testid="terminal"]')?.getAttribute("data-readonly") === "false");
      assert.deepEqual(await page.evaluate(() => window.connectionBanners), [], "빠른 연결은 순간 표시도 없음");
      await shot(page, `84-banner-${width}-fast`);

      const shell = aaa("pane", "list").panes.find((p) => p.name === "t1");
      await select(shell.id);
      await openHeld();
      await page.clock.runFor(499);
      assert.equal(await banner.count(), 0, "새 연결도 지연을 새로 셈");
      await page.clock.runFor(1);
      await banner.getByText("연결하는 중", { exact: true }).waitFor();
      await shot(page, `84-banner-${width}-slow`);
      await page.evaluate(() => window.releasePaneOpen());
      await banner.waitFor({ state: "detached" });
      await domIs(page, "느린 연결 입력 가능", () => document.querySelector('[data-testid="terminal"]')?.getAttribute("data-readonly") === "false");

      await page.evaluate(() => { window.holdPaneOpen = true; window.gatedPane.close(); });
      await domIs(page, "실제 끊김 처리", () => document.querySelector('[data-testid="terminal"]')?.hasAttribute("data-retry"));
      assert.equal(await banner.count(), 0, "끊긴 직후 연결 띠 없음");
      await page.clock.runFor(499);
      assert.equal(await banner.count(), 0, "끊김도 499ms까지 숨김");
      await page.clock.runFor(1);
      await banner.waitFor();
      assert.match(await banner.innerText(), /연결하는 중/);
      await waitOpen();
      assert.equal(await banner.count(), 1, "재시도로 connecting이 되어도 표시 유지");
      await shot(page, `84-banner-${width}-reconnect`);
      await page.evaluate(() => window.releasePaneOpen());
      await banner.waitFor({ state: "detached" });
      await page.clock.runFor(500);
      assert.equal(await banner.count(), 0, "회복 뒤 옛 타이머 표시 없음");
      assert.deepEqual(errors, []);
    } catch (error) {
      console.log("banner-failure", width, await page.evaluate(() => ({
        panes: [...document.querySelectorAll('[data-testid="terminal"]')].map((el) => ({ name: el.dataset.paneName, readonly: el.dataset.readonly })),
        banners: [...document.querySelectorAll(".banner")].map((el) => el.textContent),
        log: window.__aaaTermLog?.slice(-15),
      })), errors);
      throw error;
    } finally { await page.context().close(); }
  };
  try {
  await step("연결 안내 지연: Chromium 1280 긴 기록·499/500ms·끊김·재시도·회복", async () => check(browser, 1280));
  await step("연결 안내 지연: WebKit 390 긴 기록·499/500ms·끊김·재시도·회복", async () => check(safari = await webkit.launch({ headless: true }), 390));
  } finally { await safari?.close(); }
}
