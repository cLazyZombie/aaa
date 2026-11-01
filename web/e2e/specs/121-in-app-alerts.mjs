// 종류·한 줄 배치·원문 보존·누름을 실제 컴포넌트와 두 브라우저에서 본다.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { root, aaa, step, browser, webkit, pairPage, domIs, base, hashIs } from "../lib/harness.mjs";

const web = fileURLToPath(new URL("../..", import.meta.url));
const original = "알림의 긴 원문을 확인한다. ".repeat(30) + "\n둘째 줄도 원문에 남는다.";
const items = [
  ["thread-state", "new report", "t-0001", "reports"],
  ["thread-state", "blocked on a prompt", "t-0002", "asks"],
  ["task-followup", "task needs check", "d-0001", "tasks"],
  ["idea", "updated", "n-0001", "ideas"],
].map(([item_kind, event, subject, topic], i) => ({ id: `fixture-${i}`, short_id: `i-000${i + 1}`, project: "demo", item_kind, event, subject,
  summary: i === 3 ? "화면 폭 때문에 잘릴 수 있는 사십 자보다 짧은 아이디어 원문을 보존한다" : original,
  body: i === 3 ? "" : "보존한 본문", done: false, seen: false, created_at_ms: Date.now() + i, done_at_ms: null, topic }));
let assets;
async function buildUI() {
  const entry = `${root}/notification-ui.mjs`;
  const module = (p) => JSON.stringify(`${web}/${p}`);
  fs.writeFileSync(entry, `
import { createElement as h, Fragment, useEffect } from ${module("node_modules/react/index.js")};
import { createRoot } from ${module("node_modules/react-dom/client.js")};
import { ActivityBar } from ${module("src/ActivityBar.tsx")};
import { InboxPanel } from ${module("src/InboxPanel.tsx")};
import { Droplet } from ${module("src/herdr/Droplet.tsx")};
import { showDroplet } from ${module("src/herdr/droplet.ts")};
import ${module("src/herdr/tokens.css")};
import ${module("src/style.css")};
window.showNotice = showDroplet;
function UI() {
  useEffect(() => { document.body.dataset.ready = "true"; }, []);
  return h(Fragment, null,
    h(ActivityBar, { coordinator: null, panel: "asks", shown: true, onPanel() {}, badges: {}, reports: false, onReports() {}, unseenReports: 0,
      tasks: false, onTasks() {}, taskBadge: 0, up: true, reader: false, settings: null }),
    h("main", { style: { marginLeft: 54, padding: 12, width: "calc(100% - 54px)", height: "100vh" } }, h(InboxPanel, { canControl: false })),
    h(Droplet, { onOpen: target => { location.hash = target; } }));
}
createRoot(document.getElementById("root")).render(h(UI));
`);
  assets = `${root}/notification-ui`;
  const { build } = await import("../../node_modules/vite/dist/node/index.js");
  const built = await build({ configFile: false, publicDir: false, root: web, logLevel: "error", define: { "process.env.NODE_ENV": '"production"' },
    build: { outDir: assets, minify: false, lib: { entry, formats: ["es"], fileName: "entry" } } });
  const output = (Array.isArray(built) ? built[0] : built).output;
  const script = output.find(o => o.type === "chunk" && o.isEntry).fileName;
  const styles = output.filter(o => o.type === "asset" && o.fileName.endsWith(".css")).map(o => `<link rel="stylesheet" href="${o.fileName}">`).join("");
  fs.writeFileSync(`${assets}/index.html`, `<!doctype html><html><head><meta name="viewport" content="width=device-width,initial-scale=1">${styles}</head><body><div id="root"></div><script type="module" src="${script}"></script></body></html>`);
}

async function scenario(engine, width) {
  const client = await pairPage(engine, aaa("web", "pair").code, { width, height: 844 }, { hash: "#tasks", serviceWorkers: "block", setup: async ctx => {
    await ctx.route("**/__notification-ui/**", route => {
      const file = path.basename(new URL(route.request().url()).pathname);
      return route.fulfill({ body: fs.readFileSync(`${assets}/${file}`), contentType: file.endsWith(".js") ? "application/javascript" : file.endsWith(".css") ? "text/css" : "text/html" });
    });
    await ctx.route("**/api/v1/call", route => {
      if (route.request().postDataJSON()?.op !== "inbox") return route.fallback();
      return route.fulfill({ json: { ok: { kind: "inbox", items, omitted_open: 0, omitted_done: 0 } } });
    });
  } });
  const page = client.page;
  try {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto(`${base}/__notification-ui/index.html`);
    await domIs(page, "알림 컴포넌트 구독 준비", () => document.body.dataset.ready === "true");
    for (const item of items) {
      const row = page.getByTestId("inbox-item").filter({ hasText: item.short_id });
      await row.waitFor();
      assert.equal(await row.getAttribute("data-topic"), item.topic);
      const icon = await page.getByTestId(`activity-${item.topic}`).locator("svg").first().innerHTML();
      assert.equal(await row.locator(".inbox-icon").innerHTML(), icon);
      const summary = await row.locator(".inbox-summary").innerText();
      assert.ok(summary.length <= 48);
      assert.equal(summary.endsWith("…"), item.summary === original);
      await row.locator("summary").click();
      assert.equal(await row.locator("details p").innerText(), item.summary);
      if (item.body) assert.equal(await row.locator("details pre").innerText(), item.body);
      await row.locator("summary").click();
    }
    const notices = [
      ["asks", "blocked", "질문", "pane=question"], ["reports", "done", "완료 보고", "thread=demo%2Ft-0001"],
      ["work", "done", "작업 완료", "pane=finished"], ["ideas", "done", "아이디어 갱신", "idea=n-0001"],
      ["tasks", "blocked", "할 일 확인", "tasks"],
    ];
    for (const [topic, kind, label, target] of notices) {
      await page.evaluate(notice => window.showNotice(notice), { topic, kind, label, target, title: original, machine: null, agent: "codex", libraryUrl: "https://example.com/v/very-long-public-url/" });
      const toast = page.locator(`.droplet[data-topic="${topic}"][data-phase="in"]`);
      const card = toast.locator(".droplet-card");
      await card.waitFor();
      // 실제 애니메이션 완료로 배치가 보이는 때를 정한다.
      await toast.evaluate(async el => { await Promise.all(el.getAnimations({ subtree: true }).map(a => a.finished)); });
      assert.equal(await toast.locator(".droplet-mark svg").innerHTML(), await page.getByTestId(`activity-${topic}`).locator("svg").first().innerHTML());
      assert.equal(await toast.locator(".droplet-title").innerText(), label);
      assert.ok((await card.getAttribute("title")).includes(original));
      assert.ok((await card.getAttribute("aria-label")).length < 100, "읽어 주는 알림도 짧게 유지한다");
      assert.ok(!(await card.innerText()).includes("https://"), "공개 URL이 한 줄 내용을 밀어내지 않는다");
      const fit = await card.evaluate(el => {
        const box = el.getBoundingClientRect(), label = el.querySelector(".droplet-title").getBoundingClientRect(), text = el.querySelector(".droplet-detail").getBoundingClientRect();
        const close = el.parentElement.querySelector(".droplet-close").getBoundingClientRect();
        return { left: box.left, right: box.right, screen: innerWidth, sameLine: Math.abs(label.top - text.top) < 2, overlap: text.right > close.left, height: box.height };
      });
      assert.ok(fit.left >= 0 && fit.right <= fit.screen && fit.sameLine && !fit.overlap && fit.height <= 48, JSON.stringify(fit));
      await card.click();
      await hashIs(page, `#${target}`);
      await toast.waitFor({ state: "detached" });
    }
    assert.deepEqual(client.errors, []);
  } finally { await page.context().close(); }
}

export default async function () {
  await step("앱 안 알림 컴포넌트 준비", buildUI, { always: true });
  await step("종류 아이콘·한 줄·원문·이동: Chromium 데스크톱", () => scenario(browser, 1280));
  await step("종류 아이콘·한 줄·원문·이동: Chromium 휴대폰", () => scenario(browser, 390));
  const safari = await webkit.launch({ headless: true });
  try {
  await step("종류 아이콘·한 줄·원문·이동: WebKit 데스크톱", () => scenario(safari, 1280));
  await step("종류 아이콘·한 줄·원문·이동: WebKit 휴대폰", () => scenario(safari, 390));
  } finally { await safari.close(); }
}
