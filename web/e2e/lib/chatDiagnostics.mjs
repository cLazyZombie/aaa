// E2E37·93의 대화 요청/실패 상태. 통과 경로에는 파일 쓰기·출력·추가 대기가 없다.
import fs from "node:fs";
import path from "node:path";

const LIMIT = 256;
export class ChatDiagnostics {
  constructor(root, label) {
    this.root = root;
    this.label = label;
    this.started = Date.now();
    this.network = [];
    this.states = [];
    this.checkpoints = [];
    this.requests = new WeakMap();
    this.pending = new Set();
    this.active = true;
  }
  remember(states = []) {
    this.checkpoints.push(...states.filter(state => state.phase));
    this.states.push(...states);
    if (this.states.length > LIMIT) this.states.splice(0, this.states.length - LIMIT);
  }
  request(request, call) {
    if (call?.op !== "pane_chat") return null;
    const entry = { at: Date.now(), pane: call.pane, before: call.before ?? null };
    this.requests.set(request, entry);
    this.network.push(entry);
    if (this.network.length > LIMIT) this.network.shift();
    return entry;
  }
  reply(entry, body) {
    const chat = body.ok;
    entry.reply = { at: Date.now(), before: chat?.before, history: chat?.history,
      first: chat?.messages?.[0]?.id, last: chat?.messages?.at(-1)?.id,
      count: chat?.messages?.length, error: body.err };
  }
  watch(context) {
    this.context = context;
    if (process.env.AAA_CHAT_TRACE) this.tracing = context.tracing.start({ screenshots: true, snapshots: true });
    context.on("page", (page) => { this.page = page; });
    context.on("response", (response) => {
      const entry = this.requests.get(response.request());
      if (!entry || !this.active) return;
      entry.received = Date.now();
      entry.status = response.status();
      // gate가 이미 읽은 응답은 다시 읽지 않는다. 나머지도 시험 흐름과 따로 관측한다.
      if (entry.reply) return;
      const read = response.json().then((body) => {
        if (this.active) this.reply(entry, body);
      }).catch((error) => { if (this.active) entry.readError = String(error); })
        .finally(() => this.pending.delete(read));
      this.pending.add(read);
    });
  }
  stop() { this.active = false; }
  async save(error, inTime) {
    const dir = path.join(this.root, "diagnostics", this.label);
    fs.mkdirSync(dir, { recursive: true });
    const write = (name, value) => fs.writeFileSync(path.join(dir, name), value);
    const summary = () => write("failure.json", JSON.stringify({ label: this.label,
      started: new Date(this.started).toISOString(), failed: new Date().toISOString(),
      error: String(error), errors: this.errors ?? [], network: this.network, states: this.states, checkpoints: this.checkpoints }, null, 2));
    // renderer가 응답하지 않아도 이미 받은 HTTP·DOM 전이와 로그는 남는다.
    summary();
    for (const role of ["host", "agent"]) {
      const lines = fs.readFileSync(path.join(this.root, `${role}.log`), "utf8").split("\n");
      const at = lines.findIndex((line) => Date.parse(line.split(" ")[0]) >= this.started);
      write(`${role}.log`, lines.slice(at < 0 ? -5 : Math.max(0, at - 5)).join("\n")
        .replace(/\bfp=[0-9a-f]+\b/g, "fp=<redacted>"));
    }
    if (this.page) {
      await Promise.all([
        inTime(this.page.evaluate(() => {
          const el = document.querySelector('[data-testid="chat"]');
          return { states: [...(window.takeChatStates?.() ?? []), ...(window.takeScrollStates?.() ?? [])], reset: { before: window.shellResetLayout, writes: window.shellResetWrites }, html: el?.outerHTML,
            composer: document.querySelector('[data-testid="chat-composer"]')?.outerHTML,
            draft: document.querySelector('[aria-label="메시지"]')?.value,
            focus: document.activeElement?.outerHTML,
            url: location.hash, top: el?.scrollTop, height: el?.scrollHeight, client: el?.clientHeight,
            noteBox: el?.querySelector('[data-testid="chat-history-start"]')?.getBoundingClientRect().toJSON() };
        }), 5000, "실패 DOM 수집").then((dom) => {
          this.remember(dom.states);
          write("dom.json", JSON.stringify(dom, null, 2));
        }).catch((error) => write("dom-error.txt", String(error))),
        this.page.screenshot({ path: path.join(dir, "screen.png"), timeout: 5000 })
          .catch((error) => write("screen-error.txt", String(error))),
        inTime(Promise.allSettled([...this.pending]), 5000, "실패 HTTP 수집").catch(() => {}),
        this.tracing?.then(() => this.context.tracing.stop({ path: path.join(dir, "trace.zip") }))
          .catch((error) => write("trace-error.txt", String(error))),
      ]);
    }
    this.stop();
    summary();
    console.log(`대화 실패 진단: ${dir}`);
  }
}
