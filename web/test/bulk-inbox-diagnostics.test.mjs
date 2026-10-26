import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { BulkInboxDiagnostics } from "../e2e/lib/bulk-inbox-diagnostics.mjs";

test("inbox 진단은 초기 앱과 문서 전환의 오류·전체 stack·자산 실패를 구분해 보관한다", async t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "aaa-inbox-diagnostic-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const page = new EventEmitter();
  let current = "http://localhost/?pair=private-fixture-code";
  page.url = () => current;
  page.locator = () => ({ innerText: async () => "알림함 읽는 중" });
  page.screenshot = async ({ path: target }) => fs.writeFileSync(target, "fixture screenshot");
  const diagnostic = new BulkInboxDiagnostics(root, "webkit", 390);
  diagnostic.watch(page);
  const worker = "http://localhost/assets/terminalInflate.worker.js?private=fixture";
  const request = { url: () => worker, isNavigationRequest: () => false, failure: () => ({ errorText: "cancelled" }) };
  page.emit("request", request);
  page.emit("requestfailed", request);
  const error = new Error("worker access control");
  error.stack = [error.toString(), ...Array.from({ length: 8 }, (_, i) => `at frame${i} (index.js:${i}:1)`)].join("\n");
  page.emit("pageerror", error);
  diagnostic.phase = "inbox 진입";
  current = "http://localhost/__bulk-inbox/index.html";
  page.emit("pageerror", new Error("inbox render"));
  assert.equal(diagnostic.terminalWorkers.length, 2);
  await diagnostic.save(error);
  const json = fs.readFileSync(`${diagnostic.dir}/webkit-390.json`, "utf8");
  const saved = JSON.parse(json);
  assert.equal(saved.engine, "webkit");
  assert.equal(saved.width, 390);
  assert.equal(saved.body, "알림함 읽는 중");
  assert.deepEqual(saved.events.map(e => [e.phase, e.event]), [
    ["pairPage", "request"], ["pairPage", "request-failed"], ["pairPage", "pageerror"], ["inbox 진입", "pageerror"],
  ]);
  assert.equal(saved.events[2].path, "/");
  assert.equal(saved.events[2].stack, error.stack);
  assert.equal(saved.events[3].path, "/__bulk-inbox/index.html");
  assert.ok(!json.includes("private-fixture-code") && !json.includes("private=fixture"));
  assert.ok(fs.existsSync(`${diagnostic.dir}/webkit-390.png`));
});

test("본문을 읽지 못해도 원래 실패와 화면을 보존한다", async t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "aaa-inbox-diagnostic-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const page = new EventEmitter();
  page.locator = () => ({ innerText: async () => { throw new Error("document unavailable"); } });
  page.screenshot = async ({ path: target }) => fs.writeFileSync(target, "fixture screenshot");
  const diagnostic = new BulkInboxDiagnostics(root, "chromium", 1280);
  diagnostic.watch(page);
  const cause = new Error("original failure");
  await diagnostic.save(cause);
  const saved = JSON.parse(fs.readFileSync(`${diagnostic.dir}/chromium-1280.json`, "utf8"));
  assert.equal(saved.error, String(cause));
  assert.match(saved.bodyError, /document unavailable/);
  assert.ok(fs.existsSync(`${diagnostic.dir}/chromium-1280.png`));
});
