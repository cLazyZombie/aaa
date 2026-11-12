// Codex 판 3·기존 thread·결과 불명 정리·늦은 조회 응답을 실제 host로 지킨다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { execFileSync } from "node:child_process";
import { token } from "../../../scripts/fixture-secret.mjs";
import { root, aaa, until, step, browser, webkit, pairPage, openSettings, waitOutput } from "../lib/harness.mjs";

const config = `${root}/c/host.toml`;
fs.writeFileSync(config, fs.readFileSync(config, "utf8").replace('allow_profiles = ["fake"]', 'allow_profiles = ["fake", "fake-codex"]')
  + `\n[profiles.fake-codex]\nagent = "codex"\ncommand = ["/bin/sh", "${root}/fake.sh"]\n`);
const auth = `${root}/codex-auth.json`, task = `${root}/codex-task.md`;
fs.writeFileSync(auth, JSON.stringify({ tokens: { access_token: token("image-codex-access"), refresh_token: token("image-codex-refresh") } }));
fs.writeFileSync(task, "사람을 기다림\n");
let nextId = 0;
const readJob = id => JSON.parse(execFileSync("python3", ["-c", "import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); print(c.execute('SELECT value FROM meta WHERE key=?',('image-job:'+sys.argv[2],)).fetchone()[0])", `${root}/s/host/host.db`, id], { encoding: "utf8" }));
function saveJob(job) {
  execFileSync("python3", ["-c", "import sqlite3,json,sys; c=sqlite3.connect(sys.argv[1]); j=json.loads(sys.argv[2]); c.execute('INSERT OR REPLACE INTO meta(key,value) VALUES(?,?)',('image-job:'+j['begin']['id'],json.dumps(j))); c.commit()", `${root}/s/host/host.db`, JSON.stringify(job)]);
}
function seed(name, state) {
  const id = String(++nextId).padStart(26, "0");
  const sealed = JSON.parse(fs.readFileSync(`${root}/s/host/accounts/${name}.json`, "utf8"));
  const job = { begin: { id, prompt: "private fixture", profile: "fake-codex", account: name, transparent: false, inputs: [] }, machine: "labA", session: "fixture", project: "demo", owner: "thread:t-0001", model: "model-a", effort: "high", view: { id, state, account: name, reason: "host 재시작 · 생성 자동 반복 없음", outputs: [], total_tokens: null }, allocation: { generation: sealed.generation, ended_at_ms: null, released: false, reserve_percent: 5, selection: { size: "small", group: name, unobserved: false, reason: "fixture", selected_at_ms: 123456789 } }, launched: true };
  saveJob(job);
  return job;
}
async function start(name) {
  const t = aaa("thread", "start", "demo", "--title", "Codex 이름 변경", "--machine", "labA", "--profile", "fake-codex", "--account", name, "--kind", "dir", "--task-file", task);
  const rec = await until("Codex pane", () => { const r = aaa("thread", "show", t.id); return r.pane && r; });
  await waitOutput(rec.pane, "fake>");
  return rec;
}
async function check(engine, width) {
  const name = `codex-${engine.browserType().name()}-${width}`, after = `renamed-${name}`;
  for (let i = 0; i < 3; i++) aaa("account", "import", name, "--harness", "codex", "--file", auth);
  aaa("account", "default", "codex", name);
  const existing = await start(name);
  const unknown = seed(name, "unknown"), running = seed(name, "running");
  const { page, errors } = await pairPage(engine, aaa("web", "pair").code, { width, height: 844 }, { serviceWorkers: "block" });
  try {
    const settings = await openSettings(page, "계정");
    const row = settings.locator(`[data-account="${name}"]`);
    const edit = async () => { await row.getByRole("button", { name: `${name} 이름 바꾸기`, exact: true }).click(); await row.getByLabel(`${name} 새 이름`, { exact: true }).fill(after); };
    await edit();
    const list = row.getByTestId("account-image-jobs"), item = list.locator(`[data-image-job="${unknown.begin.id}"]`);
    await item.waitFor();
    assert.equal(await list.locator(`[data-image-job="${running.begin.id}"]`).getByRole("button", { name: "정리", exact: true }).count(), 0);
    await row.getByRole("button", { name: "이름 저장", exact: true }).click();
    await row.getByRole("alert").waitFor();
    assert.match(await row.getByRole("alert").textContent(), /이미지 작업/);
    await item.getByRole("button", { name: "정리", exact: true }).click();
    assert.equal(readJob(unknown.begin.id).view.state, "unknown", "확인 전에는 바꾸지 않는다");
    await item.getByRole("button", { name: "취소", exact: true }).click();
    assert.equal(readJob(unknown.begin.id).view.state, "unknown", "취소는 원장 보존");

    // 실제 host의 옛 Unknown 응답을 보류한 동안 정리를 끝내고 최신 목록을 먼저 반영한다.
    let capture, release;
    const captured = new Promise(resolve => { capture = resolve; });
    const held = new Promise(resolve => { release = resolve; });
    let holdNext = true;
    const route = async route => {
      const req = route.request().postDataJSON();
      if (req.op === "account_image_jobs" && req.name === name && holdNext) {
        holdNext = false;
        const reply = await route.fetch();
        capture();
        await held;
        await route.fulfill({ response: reply });
      } else await route.continue();
    };
    await page.route("**/api/v1/call", route);
    try {
      aaa("account", "default", "codex", name);
      await captured;
      await item.getByRole("button", { name: "정리", exact: true }).click();
      await item.getByRole("button", { name: "확인하고 정리", exact: true }).click();
      await item.waitFor({ state: "detached" });
      assert.equal(readJob(unknown.begin.id).view.state, "forgotten");
      assert.deepEqual(readJob(unknown.begin.id).allocation, unknown.allocation, "사용량 예약 보존");
      const oldFinished = page.waitForEvent("requestfinished", { predicate: r => r.url().endsWith("/api/v1/call") && r.postDataJSON()?.op === "account_image_jobs" });
      release();
      await oldFinished;
      await row.getByLabel(`${name} 새 이름`, { exact: true }).fill(`${after}-updated`);
      assert.equal(await item.count(), 0, "늦은 조회가 정리한 행을 되살리지 않는다");
    } finally { release(); await page.unroute("**/api/v1/call", route); }

    await row.getByRole("button", { name: "이름 저장", exact: true }).click();
    await row.getByRole("alert").waitFor();
    assert.match(await row.getByRole("alert").textContent(), /생성 중 · 완료 뒤 이름 변경 가능/);
    running.view.state = "succeeded"; saveJob(running);
    await row.getByTestId("account-rename").getByRole("button", { name: "취소", exact: true }).click();
    await edit();
    await row.getByTestId("account-image-jobs").waitFor({ state: "detached" });
    await row.getByRole("button", { name: "이름 저장", exact: true }).click();
    await settings.locator(`[data-account="${after}"]`).getByTestId("account-renamed").waitFor();
    assert.equal(aaa("thread", "show", existing.id).account, name);
    assert.equal(aaa("thread", "show", existing.id).pane, existing.pane);
    assert.equal(aaa("account", "default", "codex").defaults[0].account, after);
    assert.ok(aaa("account", "pool", "list").pool.members.some(m => m.account === after));
    assert.equal(aaa("account", "list").accounts.find(a => a.name === after).version, 3);
    assert.equal((await start(after)).account, after);
    assert.equal(readJob(unknown.begin.id).view.state, "forgotten");
    assert.equal(readJob(unknown.begin.id).view.account, after);
    assert.deepEqual(errors, []);
  } finally { await page.context().close(); }
}
export default async function () {
  await step("Codex 이미지 계정 실험실", async () => { aaa("project", "create", "demo"); });
  await step("Chromium Codex 결과 불명 확인·정리·이름 변경", () => check(browser, 1280));
  await step("WebKit Codex 결과 불명 확인·정리·이름 변경", async () => { const wk = await webkit.launch(); try { await check(wk, 1280); } finally { await wk.close(); } });
  await step("휴대폰 Codex 결과 불명 확인·정리·이름 변경", async () => { const wk = await webkit.launch(); try { await check(wk, 390); } finally { await wk.close(); } });
};
