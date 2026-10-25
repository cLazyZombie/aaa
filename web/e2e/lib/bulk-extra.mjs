import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";
import { DatabaseSync } from "node:sqlite";
import { root, aaa, inList, domIs, pairPage, openSettings, shot, base } from "./harness.mjs";
import { responseGate, responsive, count } from "./bulk-actions.mjs";

let inboxAssets;
async function buildInbox() {
  if (inboxAssets) return inboxAssets;
  const { build } = await import("../../node_modules/vite/dist/node/index.js");
  inboxAssets = `${root}/inbox-ui`;
  const built = await build({ configFile: false, publicDir: false, define: { "process.env.NODE_ENV": '"production"' }, logLevel: "error", root: fileURLToPath(new URL("../..", import.meta.url)),
    build: { outDir: inboxAssets, minify: false, lib: { entry: fileURLToPath(new URL("./bulk-inbox.tsx", import.meta.url)), formats: ["es"], fileName: "entry" } } });
  const output = (Array.isArray(built) ? built[0] : built).output;
  const entry = output.find(o => o.type === "chunk" && o.isEntry).fileName;
  const styles = output.filter(o => o.type === "asset" && o.fileName.endsWith(".css")).map(o => `<link rel="stylesheet" href="${o.fileName}">`).join("");
  fs.writeFileSync(`${inboxAssets}/index.html`, `<!doctype html><html><head><meta name="viewport" content="width=device-width,initial-scale=1">${styles}</head><body><div id="root"></div><script type="module" src="${entry}"></script></body></html>`);
  return inboxAssets;
}
function seedInbox(db, n, prefix) {
  const put = db.prepare("INSERT INTO inbox(id,project,kind,subject,event,done,created_at,data) VALUES(?,'demo','report',?,'report',0,?,?)");
  db.exec("BEGIN IMMEDIATE");
  try {
    for (let i = 0; i < n; i++) {
      const item = { id: `${prefix}-${i}`, project: "demo", item_kind: "report", subject: `t-${String(i + 1).padStart(4, "0")}`, event: "report", summary: `알림 ${i}`, body: "보고 내용", done: false, seen: false, created_at_ms: Date.now(), done_at_ms: null };
      put.run(item.id, item.subject, item.created_at_ms, JSON.stringify(item));
    }
    db.exec("COMMIT");
  } catch (e) { db.exec("ROLLBACK"); throw e; }
}
export async function inboxScenario(engine, label, width) {
  const assets = await buildInbox();
  const db = new DatabaseSync(`${root}/s/host/host.db`);
  db.exec("DELETE FROM inbox");
  seedInbox(db, count, "bulk-inbox");
  const gate = responseGate();
  const paired = await pairPage(engine, aaa("web", "pair").code, { width, height: 900 }, { serviceWorkers: "block", setup: async ctx => {
    await gate.setup(ctx);
    await ctx.route("**/__bulk-inbox/**", route => {
      const name = path.basename(new URL(route.request().url()).pathname);
      const type = name.endsWith(".js") ? "application/javascript" : name.endsWith(".css") ? "text/css" : "text/html";
      return route.fulfill({ body: fs.readFileSync(`${assets}/${name}`), contentType: type });
    });
  } });
  const page = paired.page;
  try {
    await page.goto(`${base}/__bulk-inbox/index.html`);
    assert.deepEqual(paired.errors, [], "inbox 전용 진입 번들 로드");
    await page.getByTestId("inbox-done-all").click();
    const dialog = page.getByRole("alertdialog");
    await dialog.getByText(/500개를 끝낸다/).waitFor();
    seedInbox(db, 1, "late-inbox");
    const held = gate.arm("inbox_done");
    await dialog.getByRole("button", { name: "모두 끝내기", exact: true }).click();
    try { await held.arrived; await responsive(page, dialog); await shot(page, `bulk-inbox-${label}-${width}`); }
    finally { held.release(); }
    await dialog.waitFor({ state: "hidden" });
    await domIs(page, "새 inbox 하나만 남음", () => document.querySelectorAll('[data-testid="inbox-item"]').length === 1);
    assert.equal(gate.calls.length, 1);
    assert.equal(gate.calls[0].ids.length, count);
    assert.equal(gate.calls[0].all, false);
    assert.equal(db.prepare("SELECT count(*) AS n FROM inbox WHERE done=1").get().n, count);
    assert.equal(db.prepare("SELECT count(*) AS n FROM inbox WHERE done=0").get().n, 1);
    assert.deepEqual(paired.errors, []);
  } catch (e) { console.error(label, width, paired.errors, await page.locator("body").innerText()); throw e; }
  finally { await page.context().close(); db.close(); }
}

export async function uploadScenario(engine, label, width) {
  const gate = responseGate();
  let uploads = 0;
  const paired = await pairPage(engine, aaa("web", "pair").code, { width, height: 900 }, { serviceWorkers: "block", setup: async ctx => {
    await gate.setup(ctx);
    ctx.on("request", req => { if (req.method() === "POST" && req.url().includes("/library-uploads/")) uploads++; });
  } });
  const page = paired.page;
  try {
    await page.getByTestId("terminal").waitFor();
    await inList(page, "activity-reports");
    await inList(page, "reports-upload");
    const dialog = page.getByRole("dialog", { name: "보고서 올리기", exact: true });
    const files = Array.from({ length: count }, (_, i) => ({ name: i ? `part-${i}.txt` : "index.html", mimeType: i ? "text/plain" : "text/html", buffer: Buffer.from(i ? `본문 ${i}` : "<h1>대량 업로드</h1>") }));
    await dialog.getByTestId("upload-files").setInputFiles(files);
    await dialog.getByTestId("upload-name").fill(`upload-${label}-${width}`);
    const held = gate.arm("library_upload_publish");
    await dialog.getByTestId("upload-submit").click();
    try {
      await held.arrived;
      await dialog.getByRole("status").getByText("올리는 중 500/500", { exact: true }).waitFor();
      await page.evaluate(() => new Promise(resolve => requestAnimationFrame(resolve)));
      assert.equal(await dialog.getByRole("button", { name: "닫기", exact: true }).isDisabled(), true);
      assert.equal(uploads, count);
      assert.equal(gate.calls.length, 1);
      await shot(page, `bulk-upload-${label}-${width}`);
    } finally { held.release(); }
    await dialog.waitFor({ state: "hidden" });
    await inList(page, "reports-tab-work");
    await page.getByTestId(`report-demo-upload-${label}-${width}`).waitFor();
    assert.equal(fs.readdirSync(`${root}/projects/demo/library/upload-${label}-${width}`).length, count);
    assert.deepEqual(paired.errors, []);
    aaa("library", "delete", "--project", "demo", `upload-${label}-${width}`);
  } finally { await page.context().close(); }
}

export async function sweepScenario(engine, label, width) {
  const db = new DatabaseSync(`${root}/s/host/host.db`);
  const repo = `${root}/sweep-${label}-${width}`;
  const git = (...args) => execFileSync("git", ["-C", repo, "-c", "user.name=fixture", "-c", "user.email=fixture@example.com", ...args], { stdio: "pipe" });
  fs.mkdirSync(repo);
  git("init"); git("commit", "--allow-empty", "-m", "fixture");
  const real = `${repo}-worktree`;
  git("worktree", "add", "--detach", real);
  const machine = aaa("machine", "list").machines.find(m => m.name === "labA").id;
  const put = db.prepare("INSERT INTO threads(project,id,status,data,updated_at) VALUES('demo',?,'resolved',?,?)");
  for (let i = 0; i < count; i++) {
    const id = `t-${20000 + i}`;
    const rec = { project: "demo", id, title: "끝난 자리", machine_id: machine, profile: "fake", kind: "worktree", repo, base: null, branch: null, cwd: i ? `${repo}-gone-${i}` : real,
      status: "resolved", brief: "delivered", leftover: { worktree: true, branch: false }, created_at_ms: Date.now(), updated_at_ms: Date.now() };
    put.run(id, JSON.stringify(rec), rec.updated_at_ms);
  }
  const gate = responseGate();
  const paired = await pairPage(engine, aaa("web", "pair").code, { width, height: 900 }, { serviceWorkers: "block", setup: gate.setup });
  const page = paired.page;
  try {
    await page.getByTestId("terminal").waitFor();
    const settings = await openSettings(page, "이 aaa");
    await settings.getByTestId("sweep-preview").click();
    await settings.getByTestId("sweep-run").waitFor();
    assert.equal(await settings.locator('[data-testid="sweep-notes"] li').count(), count);
    await settings.getByTestId("sweep-run").click();
    const dialog = page.getByRole("alertdialog");
    const held = gate.arm(req => req.op === "project_sweep" && !req.dry_run);
    await dialog.getByRole("button", { name: "치우기", exact: true }).click();
    try { await responsive(page, dialog); await held.arrived; }
    finally { held.release(); }
    await dialog.waitFor({ state: "hidden" });
    await settings.getByTestId("clean-note").getByText(/500개 치움, 0개 남김/).waitFor();
    assert.equal(fs.existsSync(real), false);
    assert.equal(db.prepare("SELECT count(*) AS n FROM threads WHERE json_extract(data,'$.leftover.worktree')=1").get().n, 0);
    await settings.getByTestId("sweep-preview").click();
    await settings.getByTestId("sweep-notes").getByText(/치울 것이 없다/).waitFor();
    assert.deepEqual(paired.errors, []);
    db.exec("DELETE FROM threads WHERE id >= 't-20000' AND id <= 't-20499'");
  } catch (e) { console.error(label, width, paired.errors, await page.locator("body").innerText()); throw e; }
  finally { await page.context().close(); db.close(); }
}
