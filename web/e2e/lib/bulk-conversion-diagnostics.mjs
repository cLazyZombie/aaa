// E114 변환 대기의 실제 사건을 남긴다. 시한·요청·응답 보류 정책은 바꾸지 않는다.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";

const describe = error => ({ name: error.name, message: error.message, stack: error.stack });
const call = request => {
  if (new URL(request.url()).pathname !== "/api/v1/call") return null;
  try {
    const body = request.postDataJSON();
    return { op: body.op, ...(body.op === "automation_convert" ? { dryRun: body.dry_run === true } : {}) };
  } catch { return { op: "unreadable" }; }
};

export class BulkConversionDiagnostics {
  constructor(root, engine, width, hostOffset = 0) {
    this.root = root;
    this.engine = engine;
    this.width = width;
    this.hostOffset = hostOffset;
    this.started = performance.now();
    this.events = [];
    this.phase = "pairPage";
    this.file = path.join(root, "diagnostics", "bulk-conversion", `${engine}-${width}.json`);
    this.listeners = [];
  }
  record = (event, detail = {}) => {
    this.events.push({ at: new Date().toISOString(), ms: Math.round(performance.now() - this.started), phase: this.phase, event, ...detail });
    this.write();
  };
  write() {
    try {
      fs.mkdirSync(path.dirname(this.file), { recursive: true });
      const result = { engine: this.engine, width: this.width, phase: this.phase, primary: this.primary, snapshot: this.snapshot, events: this.events };
      fs.writeFileSync(`${this.file}.tmp`, JSON.stringify(result, null, 2));
      fs.renameSync(`${this.file}.tmp`, this.file);
    } catch (error) {
      if (!this.writeFailed) console.error("변환 진단 저장 오류", String(error));
      this.writeFailed = true;
    }
  }
  watch(page, browser) {
    this.page = page;
    const listen = (target, event, fn) => {
      target.on(event, fn);
      this.listeners.push(() => target.off(event, fn));
    };
    listen(page, "request", request => { const body = call(request); if (body) this.record("request", body); });
    listen(page, "response", response => { const body = call(response.request()); if (body) this.record("response", { ...body, status: response.status() }); });
    listen(page, "requestfinished", request => { const body = call(request); if (body) this.record("request.finished", body); });
    listen(page, "requestfailed", request => { const body = call(request); if (body) this.record("request.failed", { ...body, error: request.failure()?.errorText }); });
    listen(page, "pageerror", error => this.record("page.error", { error: describe(error) }));
    listen(page, "crash", () => this.record("page.crash"));
    listen(page, "close", () => this.record("page.close"));
    listen(browser, "disconnected", () => this.record("browser.disconnected"));
    listen(page, "console", message => {
      if (message.text() === "aaa:e114:convert-click") this.record("click.dispatched");
    });
  }
  async observeClick() {
    await this.page.evaluate(() => document.addEventListener("click", event => {
      const button = event.target instanceof Element ? event.target.closest("button") : null;
      if (button?.closest('[data-testid="automation-convert"]') && button.textContent?.trim() === "변환")
        console.debug("aaa:e114:convert-click");
    }, true));
  }
  hostSnapshot() {
    const result = { load: os.loadavg() };
    try {
      const log = fs.readFileSync(path.join(this.root, "host.log"));
      result.hostLogBytes = log.length;
      result.timings = log.subarray(this.hostOffset).toString().split("\n").filter(line => line.includes("자동화 변환 단계 비용"));
    } catch (error) { result.hostLogError = describe(error); }
    try {
      const prefix = `legacy-${this.engine}-${this.width}-`;
      result.files = Object.fromEntries(["routines", "routines/.converted", "automations"].map(dir =>
        [dir, fs.readdirSync(path.join(this.root, "projects", "demo", dir)).filter(name => name.startsWith(prefix)).length]));
    } catch (error) { result.filesError = describe(error); }
    let db;
    try {
      db = new DatabaseSync(path.join(this.root, "s", "host", "host.db"), { readOnly: true });
      result.journals = db.prepare("SELECT key, length(value) AS bytes FROM meta WHERE key IN ('automation_migration:demo', 'mission_conversion:demo')").all();
    } catch (error) { result.dbError = describe(error); }
    finally {
      try { db?.close(); }
      catch (error) { result.dbCloseError = describe(error); }
    }
    return result;
  }
  async capture(error) {
    this.primary ??= describe(error);
    // renderer 조회·정리보다 먼저 원래 오류와 Node/host 상태를 디스크에 확정한다.
    this.record("failure", { load: os.loadavg() });
    this.snapshot = this.hostSnapshot();
    this.write();
    try { this.record("body", { text: (await this.page.locator("body").innerText({ timeout: 1000 })).slice(0, 4000) }); }
    catch (captureError) { this.record("body.error", { error: describe(captureError) }); }
    try { await this.page.screenshot({ path: this.file.replace(/\.json$/, ".png"), timeout: 1000 }); }
    catch (captureError) { this.record("screenshot.error", { error: describe(captureError) }); }
  }
  async close(close) {
    this.record("cleanup.start");
    try { await close(); this.record("cleanup.end"); }
    catch (error) {
      this.record("cleanup.error", { error: describe(error) });
      if (!this.primary) throw error;
    } finally {
      for (const off of this.listeners) off();
      this.listeners.length = 0;
      console.log("bulk-conversion-events", JSON.stringify({ engine: this.engine, width: this.width, primary: this.primary,
        events: this.events.filter(e => e.event.startsWith("gate.") || e.event.startsWith("click.") || e.event.startsWith("cleanup.")) }));
    }
  }
}
