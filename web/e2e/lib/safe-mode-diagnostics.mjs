// E2E68: 실패 화면을 움직이기 전에 DOM·요청과 실험실 프로세스/agent 사건을 보관한다.
import fs from "node:fs";
import path from "node:path";

export class SafeModeDiagnostics {
  constructor(root, procs) {
    this.root = root;
    this.procs = procs;
    this.dir = path.join(root, "diagnostics", "safe-mode");
    this.events = [];
    this.pages = new Map();
    this.watched = new Set();
    for (const [tag, proc] of procs) this.watchProcess(tag, proc);
  }
  record(event, detail = {}) {
    this.events.push({ at: new Date().toISOString(), event, ...detail });
    if (this.events.length > 1000) this.events.shift();
  }
  watchProcess(tag, proc) {
    if (this.watched.has(proc)) return;
    this.watched.add(proc);
    this.record("process-watch", { tag, pid: proc.pid });
    proc.once("exit", (code, signal) => {
      this.record("process-exit", { tag, pid: proc.pid, code, signal, killed: proc.killed });
      // 실패 진단 뒤 harness 정리로 생긴 연결 끊김도 같은 시각 기록에 남긴다.
      if (fs.existsSync(this.dir)) this.writeLogs();
    });
  }
  watchPage(label, page) {
    this.pages.set(label, page);
    const requestInfo = (request) => {
      const url = new URL(request.url());
      let op;
      try { op = request.postDataJSON()?.op; } catch { /* form/GET */ }
      // 기기 cookie·짝짓기 code·입력 본문은 저장하지 않는다.
      return { label, path: url.pathname, method: request.method(), navigation: request.isNavigationRequest(), op };
    };
    page.on("request", (r) => this.record("request", requestInfo(r)));
    page.on("response", (r) => this.record("response", { ...requestInfo(r.request()), status: r.status() }));
    page.on("requestfailed", (r) => this.record("request-failed", { ...requestInfo(r), error: r.failure()?.errorText }));
    page.on("framenavigated", (f) => {
      if (f === page.mainFrame()) this.record("navigation", { label, url: f.url().split("?")[0] });
    });
    page.on("pageerror", (e) => this.record("page-error", { label, error: String(e) }));
    page.on("websocket", (ws) => {
      const pathname = new URL(ws.url()).pathname;
      this.record("websocket-open", { label, path: pathname });
      ws.on("close", () => this.record("websocket-close", { label, path: pathname }));
    });
  }
  writeLogs() {
    fs.mkdirSync(this.dir, { recursive: true });
    fs.writeFileSync(path.join(this.dir, "events.json"), JSON.stringify(this.events, null, 2));
    for (const name of fs.readdirSync(this.root).filter((n) => /^(host.*|agent)\.log$/.test(n))) {
      fs.copyFileSync(path.join(this.root, name), path.join(this.dir, name));
    }
  }
  async save(error) {
    this.record("failure", { error: String(error), processes: [...this.procs].map(([tag, p]) =>
      ({ tag, pid: p.pid, exitCode: p.exitCode, signalCode: p.signalCode, killed: p.killed })) });
    this.writeLogs();
    for (const [label, page] of this.pages) {
      try {
        fs.writeFileSync(path.join(this.dir, `${label}.html`), await page.content());
        await page.screenshot({ path: path.join(this.dir, `${label}.png`) });
      } catch (e) { this.record("capture-error", { label, error: String(e) }); }
    }
    this.writeLogs();
    console.log(`안전 모드 실패 진단: ${this.dir}`);
  }
}
