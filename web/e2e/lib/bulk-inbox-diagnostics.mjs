// pairPage의 초기 앱과 전용 InboxPanel 문서를 구분한다. context를 닫기 전에 실패 자료를 보관한다.
import fs from "node:fs";
import path from "node:path";

export class BulkInboxDiagnostics {
  constructor(root, engine, width) {
    this.dir = path.join(root, "diagnostics", "bulk-inbox");
    this.engine = engine;
    this.width = width;
    this.phase = "pairPage";
    this.events = [];
  }
  record(event, detail = {}) {
    this.events.push({ at: new Date().toISOString(), phase: this.phase, event, ...detail });
  }
  watch(page) {
    this.page = page;
    const pathname = url => new URL(url).pathname;
    const relevant = request => request.isNavigationRequest() || /\.(?:js|css)$/.test(pathname(request.url()));
    page.on("request", request => {
      if (relevant(request)) this.record("request", { path: pathname(request.url()) });
    });
    page.on("response", response => {
      if (relevant(response.request())) this.record("response", { path: pathname(response.url()), status: response.status() });
    });
    page.on("requestfailed", request => {
      if (relevant(request)) this.record("request-failed", { path: pathname(request.url()), error: request.failure()?.errorText });
    });
    page.on("framenavigated", frame => {
      if (frame === page.mainFrame()) this.record("navigation", { path: pathname(frame.url()) });
    });
    page.on("pageerror", error => this.record("pageerror", { path: pathname(page.url()), error: String(error), stack: error.stack }));
    page.on("worker", worker => this.record("worker", { path: pathname(worker.url()) }));
  }
  get terminalWorkers() {
    return this.events.filter(event => /terminalInflate\.worker/.test(event.path ?? ""));
  }
  async save(error) {
    fs.mkdirSync(this.dir, { recursive: true });
    const stem = path.join(this.dir, `${this.engine}-${this.width}`);
    const result = { engine: this.engine, width: this.width, phase: this.phase, error: String(error), stack: error.stack, events: this.events };
    try { result.body = await this.page.locator("body").innerText(); }
    catch (captureError) { result.bodyError = String(captureError); }
    fs.writeFileSync(`${stem}.json`, JSON.stringify(result, null, 2));
    try { await this.page.screenshot({ path: `${stem}.png`, timeout: 5000 }); }
    catch (captureError) { console.error("inbox 실패 화면 저장 오류", String(captureError)); }
    console.error("bulk-inbox-failure", JSON.stringify(result));
  }
}
