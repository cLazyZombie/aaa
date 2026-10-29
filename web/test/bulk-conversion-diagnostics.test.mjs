import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";
import test from "node:test";
import { BulkConversionDiagnostics } from "../e2e/lib/bulk-conversion-diagnostics.mjs";

function lab(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "aaa-conversion-diag-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  return root;
}

test("변환 실패는 renderer 조회 전에 최초 오류·host 파일/journal 상태를 보존한다", async t => {
  const root = lab(t);
  const prefix = "legacy-webkit-1280-0";
  for (const dir of ["routines/.converted", "automations"]) fs.mkdirSync(path.join(root, "projects/demo", dir), { recursive: true });
  fs.writeFileSync(path.join(root, "projects/demo/routines/.converted", `${prefix}.md`), "보존한 원문");
  fs.writeFileSync(path.join(root, "projects/demo/automations", `${prefix}.toml`), "변환한 규칙");
  fs.writeFileSync(path.join(root, "host.log"), '이전 단계\n자동화 변환 단계 비용 scope="resume" elapsed_ms=6126\n');
  fs.mkdirSync(path.join(root, "s/host"), { recursive: true });
  const db = new DatabaseSync(path.join(root, "s/host/host.db"));
  db.exec("CREATE TABLE meta(key TEXT, value TEXT)");
  db.prepare("INSERT INTO meta VALUES('automation_migration:demo',?)").run("private-journal-content");
  db.close();
  const diagnostic = new BulkConversionDiagnostics(root, "webkit", 1280, Buffer.byteLength("이전 단계\n"));
  const page = new EventEmitter();
  const browser = new EventEmitter();
  diagnostic.watch(page, browser);
  diagnostic.phase = "convert";
  const primary = new Error("host 일괄 응답 제한");
  const saved = () => JSON.parse(fs.readFileSync(diagnostic.file, "utf8"));
  page.locator = () => ({ innerText: async () => {
    assert.equal(saved().primary.stack, primary.stack, "renderer에 묻기 전에 디스크에 원래 오류를 확정한다");
    throw new Error("renderer unavailable");
  } });
  page.screenshot = async () => { throw new Error("screenshot unavailable"); };
  diagnostic.record("gate.arm", { gate: 1 });
  diagnostic.record("gate.fetch.start", { gate: 1, op: "automation_convert" });
  await diagnostic.capture(primary);
  await diagnostic.close(async () => { page.emit("close"); throw new Error("context disposed"); });
  const result = saved();
  assert.equal(result.primary.stack, primary.stack);
  assert.deepEqual(result.snapshot.files, { routines: 0, "routines/.converted": 1, automations: 1 });
  assert.deepEqual(result.snapshot.journals, [{ key: "automation_migration:demo", bytes: 23 }]);
  assert.deepEqual(result.snapshot.timings, ['자동화 변환 단계 비용 scope="resume" elapsed_ms=6126']);
  assert.deepEqual(result.events.map(e => e.event), ["gate.arm", "gate.fetch.start", "failure", "body.error", "screenshot.error", "cleanup.start", "page.close", "cleanup.error"]);
  assert.ok(!fs.readFileSync(diagnostic.file, "utf8").includes("private-journal-content"));
  assert.equal(page.eventNames().length, 0);
  assert.equal(browser.listenerCount("disconnected"), 0);
});

test("변환 진단은 실제 클릭·요청·응답·종료 사건을 구분하며 요청 비밀값을 기록하지 않는다", async t => {
  const diagnostic = new BulkConversionDiagnostics(lab(t), "chromium", 390);
  const page = new EventEmitter();
  const browser = new EventEmitter();
  diagnostic.watch(page, browser);
  const request = { url: () => "http://localhost/api/v1/call?secret=private-query",
    postDataJSON: () => ({ op: "automation_convert", dry_run: false, token: "private-token", plan_sha256: "private-plan" }),
    failure: () => ({ errorText: "net::ERR_ABORTED" }) };
  page.emit("console", { text: () => "aaa:e114:convert-click" });
  page.emit("request", request);
  page.emit("response", { request: () => request, status: () => 200 });
  page.emit("requestfinished", request);
  page.emit("requestfailed", request);
  page.emit("crash");
  browser.emit("disconnected");
  await diagnostic.close(async () => page.emit("close"));
  const raw = fs.readFileSync(diagnostic.file, "utf8"), result = JSON.parse(raw);
  assert.deepEqual(result.events.map(e => e.event), ["click.dispatched", "request", "response", "request.finished", "request.failed", "page.crash", "browser.disconnected", "cleanup.start", "page.close", "cleanup.end"]);
  assert.deepEqual(result.events[1].op, "automation_convert");
  assert.equal(result.events[1].dryRun, false);
  assert.ok(result.events.every(e => Number.isFinite(e.ms) && e.at));
  for (const secret of ["private-query", "private-token", "private-plan"]) assert.ok(!raw.includes(secret));
  const original = new Error("cleanup failure");
  await assert.rejects(diagnostic.close(async () => { throw original; }), error => error === original);
});
