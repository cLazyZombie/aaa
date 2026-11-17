// 실패 화면을 context.close와 termDiag의 viewport 변경보다 먼저 보존한다.
import fs from "node:fs";
import path from "node:path";
import { inspect } from "node:util";

export function writeFailure(directory, name, error) {
  fs.mkdirSync(directory, { recursive: true });
  fs.writeFileSync(path.join(directory, `${name}.txt`), inspect(error, { depth: 8, colors: false, maxStringLength: null }));
}

const bounded = async operation => {
  let timer;
  try {
    return await Promise.race([operation, new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error("진단 응답 5초 상한")), 5000);
    })]);
  } finally { clearTimeout(timer); }
};

export async function failureDiagnostics(context, directory, name) {
  fs.mkdirSync(directory, { recursive: true });
  const prefix = path.join(directory, name);
  const events = [];
  const record = (event, detail) => {
    events.push({ at: performance.now(), event, ...detail });
    if (events.length > 2000) events.shift();
  };
  await context.tracing.start({ screenshots: true, snapshots: true, sources: false });
  await context.addInitScript(() => {
    const events = window.__aaaFailureEvents = [];
    const record = (event, detail = {}) => {
      events.push({ at: performance.now(), event, hash: location.hash, sheet: history.state?.["aaa:sheet"], ...detail });
      if (events.length > 300) events.shift();
    };
    for (const method of ["pushState", "replaceState", "back", "forward", "go"]) {
      const original = history[method].bind(history);
      history[method] = (...args) => { record(`history.${method}`); return original(...args); };
    }
    for (const event of ["popstate", "hashchange", "pagehide", "pageshow", "focusin", "pointerdown", "pointerup", "click", "keydown"]) {
      addEventListener(event, e => record(event, {
        target: e.target instanceof Element ? { tag: e.target.tagName, testid: e.target.getAttribute("data-testid"), label: e.target.getAttribute("aria-label") } : null,
        ...(event === "keydown" ? { key: e.key === "Escape" ? "Escape" : "other" } : {}),
      }), true);
    }
  });
  const watch = page => {
    page.on("framenavigated", frame => { if (frame === page.mainFrame()) record("navigation", { url: frame.url() }); });
    page.on("requestfailed", request => record("requestfailed", { url: request.url(), navigation: request.isNavigationRequest(), error: request.failure() }));
    page.on("pageerror", error => record("pageerror", { error: error.stack ?? String(error) }));
    page.on("crash", () => record("crash", {}));
    page.on("close", () => record("close", {}));
  };
  let sequence = 0, finishing;
  const capture = async () => {
    const part = `${prefix}-${++sequence}`;
    const results = await Promise.allSettled(context.pages().map(async (page, index) => {
      if (page.isClosed()) return;
      const file = `${part}-${index}`;
      // 서로 독립적이다. 화면 저장 실패가 DOM·사건·원래 오류를 가리지 않는다.
      const results = await Promise.allSettled([
        page.screenshot({ path: `${file}.png`, timeout: 5000 }),
        bounded(page.content()).then(html => fs.writeFileSync(`${file}.html`, html)),
        bounded(page.evaluate(() => ({ events: window.__aaaFailureEvents, hash: location.hash, state: history.state,
          active: document.activeElement?.outerHTML.slice(0, 500),
          drawer: document.getElementById("workspace-drawer")?.outerHTML.slice(0, 1000) })))
          .then(state => fs.writeFileSync(`${file}.json`, JSON.stringify(state, null, 2))),
      ]);
      record("capture", { index, errors: results.filter(r => r.status === "rejected").map(r => String(r.reason)) });
    }));
    for (const result of results) if (result.status === "rejected") record("capture-error", { error: String(result.reason) });
    fs.writeFileSync(`${prefix}-events.json`, JSON.stringify(events, null, 2));
  };
  const finish = () => finishing ??= (async () => {
    try { await capture(); } catch (error) { console.log(`실패 화면 저장 오류: ${error}`); }
    try { await bounded(context.tracing.stop({ path: `${prefix}-trace.zip` })); }
    catch (error) { console.log(`실패 trace 저장 오류: ${error}`); }
  })();
  const close = context.close.bind(context);
  context.close = async (...args) => {
    await finish();
    record("context-close-start", {});
    try { return await close(...args); }
    finally {
      record("context-close-end", {});
      try { fs.writeFileSync(`${prefix}-events.json`, JSON.stringify(events, null, 2)); }
      catch (error) { console.log(`context 종료 사건 저장 오류: ${error}`); }
    }
  };
  return { watch, finish };
}
