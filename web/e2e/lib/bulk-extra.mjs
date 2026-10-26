import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";
import { DatabaseSync } from "node:sqlite";
import { root, aaa, inList, domIs, pairPage, openSettings, shot, base } from "./harness.mjs";
import { responseGate, responsive, count } from "./bulk-actions.mjs";
import { seedHost } from "./bulk-fixture.mjs";
import { BulkInboxDiagnostics } from "./bulk-inbox-diagnostics.mjs";

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
  const diagnostic = new BulkInboxDiagnostics(root, label, width);
  try {
    // 인증 뒤 곧 떠날 문서에서 터미널·압축 Worker를 기동하지 않는다. 실제 pairing과 host API는 그대로 쓴다.
    const paired = await pairPage(engine, aaa("web", "pair").code, { width, height: 900 }, { hash: "#tasks", serviceWorkers: "block", onPage: page => diagnostic.watch(page), setup: async ctx => {
      await gate.setup(ctx);
      await ctx.route("**/__bulk-inbox/**", route => {
        const name = path.basename(new URL(route.request().url()).pathname);
        const type = name.endsWith(".js") ? "application/javascript" : name.endsWith(".css") ? "text/css" : "text/html";
        return route.fulfill({ body: fs.readFileSync(`${assets}/${name}`), contentType: type });
      });
    } });
    const page = paired.page;
    assert.deepEqual(paired.errors, [], "inbox 인증용 초기 앱 로드");
    diagnostic.phase = "inbox 진입";
    await page.goto(`${base}/__bulk-inbox/index.html`);
    assert.deepEqual(paired.errors, [], "inbox 전용 진입 번들 로드");
    await page.getByTestId("inbox-done-all").click();
    assert.deepEqual(diagnostic.terminalWorkers, [], "inbox 전용 시험은 터미널 Worker를 열지 않는다");
    diagnostic.phase = "500개 완료";
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
  } catch (e) {
    try { await diagnostic.save(e); }
    catch (captureError) { console.error("inbox 실패 진단 저장 오류", String(captureError)); }
    throw e;
  } finally { await diagnostic.page?.context().close(); db.close(); }
}

export async function uploadScenario(engine, label, width) {
  const gate = responseGate();
  let uploads = 0;
  const requests = new Map(), durations = [];
  const hostStart = fs.readFileSync(`${root}/host.log`, "utf8").length;
  const paired = await pairPage(engine, aaa("web", "pair").code, { width, height: 900 }, { serviceWorkers: "block", setup: async ctx => {
    await gate.setup(ctx);
    ctx.on("request", req => { if (req.method() === "POST" && req.url().includes("/library-uploads/")) { uploads++; requests.set(req, performance.now()); } });
    ctx.on("requestfinished", req => { if (requests.has(req)) { durations.push(performance.now() - requests.get(req)); requests.delete(req); } });
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
    const started = performance.now();
    const held = gate.arm("library_upload_publish");
    await dialog.getByTestId("upload-submit").click();
    try {
      await held.arrived;
      const log = fs.readFileSync(`${root}/host.log`, "utf8").slice(hostStart).replace(/\x1b\[[0-9;]*m/g, "");
      const totals = {};
      for (const line of log.split("\n").filter(line => line.includes("보고서 파일"))) {
        for (const [, key, value] of line.matchAll(/(validated_us|lock_us|scan_us|write_us|elapsed_us|body_us|request_us)=(\d+)/g)) totals[key] = (totals[key] || 0) + Number(value);
      }
      totals.cache_hits = (log.match(/cache_hit=true/g) || []).length;
      console.log("bulk-upload", JSON.stringify({ label, width, total_ms: Math.round(performance.now() - started), requests: uploads, request_sum_ms: Math.round(durations.reduce((a, b) => a + b, 0)), request_max_ms: Math.round(Math.max(...durations)), host_us: totals }));
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
  await seedHost(db => {
    db.exec("DELETE FROM threads WHERE id >= 't-20000' AND id <= 't-20499'");
    const put = db.prepare("INSERT INTO threads(project,id,status,data,updated_at) VALUES('demo',?,'resolved',?,?)");
    for (let i = 0; i < count; i++) {
      const id = `t-${20000 + i}`;
      const rec = { project: "demo", id, title: "끝난 자리", machine_id: machine, profile: "fake", kind: "worktree", repo, base: null, branch: null, cwd: i ? `${repo}-gone-${i}` : real,
        status: "resolved", brief: "delivered", leftover: { worktree: true, branch: false }, created_at_ms: Date.now(), updated_at_ms: Date.now() };
      put.run(id, JSON.stringify(rec), rec.updated_at_ms);
    }
  });
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
    console.log("sweep-host", fs.readFileSync(`${root}/host.log`, "utf8").split("\n").filter(line => line.includes("프로젝트 자리 정리 완료")).at(-1));
    assert.equal(db.prepare("SELECT count(*) AS n FROM threads WHERE json_extract(data,'$.leftover.worktree')=1").get().n, 0);
    await settings.getByTestId("sweep-preview").click();
    await settings.getByTestId("sweep-notes").getByText(/치울 것이 없다/).waitFor();
    assert.deepEqual(paired.errors, []);
  } catch (e) { console.error(label, width, paired.errors, await page.locator("body").innerText()); throw e; }
  finally { await page.context().close(); db.close(); }
}

// 실패 응답과 이미 시작한 성공 요청의 순서를 제어하되 성공 파일은 실제 host에 쓴다.
export async function uploadFailureScenario(engine, label, width) {
  const firstFour = Promise.withResolvers(), release = Promise.withResolvers();
  let failing = true;
  const sent = [];
  let published = 0;
  const paired = await pairPage(engine, aaa("web", "pair").code, { width, height: 900 }, { serviceWorkers: "block", setup: async ctx => {
    ctx.on("request", req => { if (req.url().endsWith("/api/v1/call") && req.postDataJSON()?.op === "library_upload_publish") published++; });
    await ctx.route("**/library-uploads/**", async route => {
      const name = new URL(route.request().url()).searchParams.get("path");
      sent.push(name);
      if (!failing) return route.fallback();
      if (sent.length === 4) firstFour.resolve();
      await firstFour.promise;
      if (name === "file-1.txt") return route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ err: { code: "unavailable", message: "실험실 파일 쓰기 실패" } }) });
      // WebKit File 본문을 Node route.fetch로 재전송하면 비어 버린다.
      // 원래 브라우저 요청을 잠시 보류했다가 그대로 host로 보낸다.
      await release.promise;
      await route.fallback();
    });
  } });
  const page = paired.page;
  try {
    await page.getByTestId("terminal").waitFor();
    await inList(page, "activity-reports");
    await inList(page, "reports-upload");
    const dialog = page.getByRole("dialog", { name: "보고서 올리기", exact: true });
    const files = Array.from({ length: 9 }, (_, i) => ({ name: `file-${i}.txt`, mimeType: "text/plain", buffer: Buffer.from(`본문 ${i}`) }));
    await dialog.getByTestId("upload-files").setInputFiles(files);
    const name = `upload-failure-${label}-${width}`;
    await dialog.getByTestId("upload-name").fill(name);
    const failed = page.waitForResponse(response => response.status() === 503 && response.url().includes("/library-uploads/"));
    await dialog.getByTestId("upload-submit").click();
    await firstFour.promise;
    await failed;
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(resolve)));
    assert.equal(sent.length, 4, "네 요청까지만 보내고 실패 뒤 멈춘다");
    assert.equal(await dialog.getByTestId("upload-submit").isDisabled(), true, "전송 중 성공 응답을 기다린다");
    assert.equal(published, 0);
    release.resolve();
    await dialog.getByRole("alert").getByText(/file-1.txt: 실험실 파일 쓰기 실패/).waitFor();
    await dialog.getByRole("status").getByText("올린 파일 3/9개는 보관했다.", { exact: true }).waitFor();
    assert.equal(sent.length, 4, "자동 재시도하지 않는다");
    failing = false;
    await dialog.getByTestId("upload-submit").click();
    await dialog.waitFor({ state: "hidden" });
    assert.equal(published, 1);
    assert.equal(sent.length, 10);
    for (const file of files) {
      assert.equal(sent.filter(value => value === file.name).length, file.name === "file-1.txt" ? 2 : 1);
      assert.deepEqual(fs.readFileSync(`${root}/projects/demo/library/${name}/${file.name}`), file.buffer);
    }
    assert.deepEqual(paired.errors, []);
    aaa("library", "delete", "--project", "demo", name);
  } finally { release.resolve(); await page.context().close(); }
}
