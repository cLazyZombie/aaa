// 합성 E54의 저장만 관측한다. 본문·쿠키·URL은 기록하지 않는다.
import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";

const taskSha = text => crypto.createHash("sha256").update(text).digest("hex");

export async function taskSaveTrace(page, file, directory) {
  fs.mkdirSync(directory, { recursive: true });
  const output = path.join(directory, `task-save-${process.pid}.jsonl`);
  const start = performance.now();
  let call = 0;
  const record = (event, data = {}) => fs.appendFileSync(output, `${JSON.stringify({ at: performance.now() - start, call, event, ...data })}\n`);
  const fileState = () => ({ fileSha: taskSha(fs.readFileSync(file, "utf8")) });
  const watch = fs.watch(path.dirname(file), (_, name) => {
    if (String(name) === path.basename(file)) record("file", fileState());
  });
  const requests = new Map();
  let nextRequest = 0;
  const onRequest = request => {
    if (!request.url().endsWith("/api/v1/call")) return;
    const op = request.postDataJSON()?.op;
    if (op !== "task_edit" && op !== "tasks") return;
    const id = ++nextRequest;
    requests.set(request, { id, op, call });
    record("request", { id, op, ...fileState() });
  };
  const pending = new Set();
  const onResponse = response => {
    const request = requests.get(response.request());
    if (!request) return;
    record("response", { ...request, status: response.status(), ...fileState() });
    const done = response.json().then(body => {
      const value = body.ok ?? body;
      record("response-body", { ...request, status: response.status(), kind: value.kind, expectedSha: (value.board ?? value).sha256, conflict: value.conflict != null, error: body.err ?? null, ...fileState() });
    }).catch(error => record("response-error", { ...request, error: String(error) })).finally(() => pending.delete(done));
    pending.add(done);
  };
  page.on("request", onRequest);
  page.on("response", onResponse);
  // Network 도메인은 이미 열린 사건 소켓에도 수신 frame을 보낸다. 전송·응답은 건드리지 않는다.
  const session = await page.context().newCDPSession(page);
  await session.send("Network.enable");
  session.on("Network.webSocketFrameReceived", ({ response }) => {
    if (response.opcode !== 1) return;
    let event;
    try { event = JSON.parse(response.payloadData); } catch { return; }
    if (event.ev === "tasks") record("tasks", { expectedSha: event.sha256, ...fileState() });
  });
  await page.exposeFunction("__aaaTaskSaveRecord", data => record("dom", data));
  await page.evaluate(() => {
    const ids = new WeakMap();
    let next = 0, last;
    const snapshot = () => {
      const form = document.querySelector('[data-testid="task-form"]');
      if (form && !ids.has(form)) ids.set(form, ++next);
      return {
        browserAt: performance.now(),
        form: form ? ids.get(form) : null,
        domSha: document.querySelector('[data-testid="tasks-view"]')?.dataset.tasksSha ?? null,
        saving: form?.dataset.taskSaving ?? null,
      };
    };
    window.__aaaTaskSaveSnapshot = snapshot;
    const observer = new MutationObserver(() => {
      const state = snapshot(), key = JSON.stringify([state.form, state.domSha, state.saving]);
      if (key === last) return;
      last = key;
      void window.__aaaTaskSaveRecord(state);
    });
    observer.observe(document, { subtree: true, attributes: true, childList: true });
    window.__aaaTaskSaveObserver = observer;
  });
  return {
    async mark(event, data = {}) {
      if (event === "before-select") call++;
      record(event, { ...data, ...fileState(), ...await page.evaluate(() => window.__aaaTaskSaveSnapshot()) });
    },
    async close() {
      await page.evaluate(() => window.__aaaTaskSaveObserver.disconnect());
      page.off("request", onRequest);
      page.off("response", onResponse);
      watch.close();
      await Promise.all(pending);
      await session.detach();
      console.log(`저장 순서: ${output}`);
    },
  };
}
