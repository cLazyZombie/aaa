// E2E28: 어느 hover 대기인지, 포인터·scroll·요약 응답·카드 표시 순서를 실패할 때만 저장한다.
import fs from "node:fs";
import path from "node:path";

export class SummaryDiagnostics {
  constructor(root, label) {
    this.dir = path.join(root, "diagnostics", label);
    this.network = [];
  }
  async watch(page) {
    this.page = page;
    const record = (event, request, status) => {
      let call;
      try { call = request.postDataJSON(); } catch { return; }
      if (call?.op !== "thread_summary") return;
      this.network.push({ at: Date.now(), event, thread: call.thread, status });
      if (this.network.length > 128) this.network.shift();
    };
    page.on("request", (request) => record("request", request));
    page.on("response", (response) => record("response", response.request(), response.status()));
    page.on("requestfailed", (request) => record("failed", request));
    const install = function install() {
      if (!document.body) {
        document.addEventListener('DOMContentLoaded', install, { once: true });
        return;
      }
      window.summaryHoverEvents = [];
      const rowOf = (node) => node instanceof Element ? node.closest('.pane-select[data-testid^="thread-"]') : null;
      const nameOf = (node) => node instanceof Element ? node.getAttribute("data-testid") ?? node.className : node === document ? "document" : "window";
      const record = (event, target, extra = {}) => {
        const row = rowOf(target) ?? document.querySelector('.pane-select[data-testid^="thread-"]:hover');
        const card = document.querySelector('[data-testid="summary-hover"]');
        window.summaryHoverEvents.push({ at: performance.now(), event, target: nameOf(target),
          row: row?.getAttribute("data-testid"), rect: row?.getBoundingClientRect().toJSON(),
          ancestor: target instanceof Node && row ? target.contains(row) : false,
          focus: nameOf(document.activeElement), card: Boolean(card), visibility: card?.style.visibility,
          theme: document.documentElement.dataset.theme, ...extra });
        if (window.summaryHoverEvents.length > 256) window.summaryHoverEvents.shift();
      };
      for (const event of ["mouseover", "mouseout", "mousedown", "scroll", "resize"]) {
        window.addEventListener(event, (e) => {
          if (event.startsWith("mouse") && (!rowOf(e.target) || rowOf(e.target) === rowOf(e.relatedTarget))) return;
          record(event, e.target, { x: e.clientX, y: e.clientY, related: nameOf(e.relatedTarget),
            scrollTop: e.target?.scrollTop });
        }, true);
      }
      let state = "";
      new MutationObserver(() => {
        const card = document.querySelector('[data-testid="summary-hover"]');
        const next = `${Boolean(card)}:${card?.style.visibility}`;
        if (state !== next) { state = next; record("card", card); }
      }).observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ["style"] });
      window.summaryHoverRecord = (event) => record(event, null);
    };
    await page.addInitScript(install);
    await page.evaluate(install);
  }
  async mark(event) {
    await this.page.evaluate((event) => window.summaryHoverRecord(event), event);
  }
  async visible(what) {
    try { await this.page.getByTestId("summary-hover").waitFor(); }
    catch (error) { throw new Error(`${what}: ${error.message}`, { cause: error }); }
  }
  async save(error, inTime) {
    fs.mkdirSync(this.dir, { recursive: true });
    const failure = { error: String(error), stack: error.stack, network: this.network };
    fs.writeFileSync(path.join(this.dir, "failure.json"), JSON.stringify(failure, null, 2));
    await inTime(this.page.evaluate(() => ({
      events: window.summaryHoverEvents, card: document.querySelector('[data-testid="summary-hover"]')?.outerHTML,
      rows: [...document.querySelectorAll('.pane-select[data-testid^="thread-"]')].map((el) => ({ id: el.dataset.testid,
        rect: el.getBoundingClientRect().toJSON(), hover: el.matches(":hover"), connected: el.isConnected })),
    })), 5000, "hover 실패 DOM 수집").then((dom) => {
      fs.writeFileSync(path.join(this.dir, "dom.json"), JSON.stringify(dom, null, 2));
    }).catch((error) => { fs.writeFileSync(path.join(this.dir, "dom-error.txt"), String(error)); });
    await this.page.screenshot({ path: path.join(this.dir, "screen.png"), timeout: 5000 }).catch(() => {});
    console.log(`hover 실패 진단: ${this.dir}`);
  }
}
