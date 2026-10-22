// A3: 실제 host·agent·session·PTY, 가짜 Claude 상태줄. 운영 로그인·인증·모델 작업은 없다.
import fs from "node:fs";
import assert from "node:assert/strict";
import { randomBytes } from "node:crypto";
import { root, aaa, step, browser, pairPage, openSettings, waitOutput, until, domIs, shot, start, stop } from "../lib/harness.mjs";

const modes = `${root}/pool-modes.json`;
const fake = `${root}/pool-claude.mjs`;
const task = `${root}/pool-task.md`;
fs.writeFileSync(modes, JSON.stringify({ a: [20, 75], b: [40, 40], cold: null }));
fs.writeFileSync(task, "허용한 계정으로 독립 작업을 한다");
fs.writeFileSync(fake, `#!${process.execPath}
import fs from 'node:fs';
import { execSync } from 'node:child_process';
const token=process.env.CLAUDE_CODE_OAUTH_TOKEN;
if(!token?.startsWith('fixture-pool-'))throw new Error('계정 인증 전달 실패');
const who=token.split('-')[2];
const settings=JSON.parse(fs.readFileSync(process.argv[process.argv.indexOf('--settings')+1],'utf8'));
let n=0;
function sample(){
 const v=JSON.parse(fs.readFileSync(${JSON.stringify(modes)},'utf8'))[who];
 if(v){const reset=Math.floor(Date.now()/1000)+3600;execSync(settings.statusLine.command,{input:JSON.stringify({rate_limits:{five_hour:{used_percentage:v[0],resets_at:reset},seven_day:{used_percentage:v[1],resets_at:reset+604800}}}),stdio:['pipe','pipe','pipe']});}
}
sample();process.stdout.write('POOL-READY '+who+'\\r\\n❯ ');
process.stdin.setRawMode(true);process.stdin.on('data',b=>{if(b.toString().includes('\\x1b[17~')){sample();process.stdout.write('POOL-UPDATED '+who+' '+(++n)+'\\r\\n❯ ');}});
`, { mode: 0o755 });
let host = fs.readFileSync(`${root}/c/host.toml`, "utf8");
host = host.replace('[profiles.fake]\nagent = "custom"', '[profiles.fake]\nagent = "claude"').replace(`command = ["/bin/sh", "${root}/fake.sh"]`, `command = [${JSON.stringify(fake)}]`);
fs.writeFileSync(`${root}/c/host.toml`, host);
export const labEnv = { AAA_TEST_USAGE_REFRESH_MS: "0" };
const canaries = [];
function register(name) {
  const file = `${root}/${name}.token`;
  // 비밀의 8바이트 조각과 별칭이 같으면 기존 금고가 별칭을 거부한다. 새 계정의 canary는 별칭을 담지 않는다.
  const value = `fixture-pool-${name === "unapproved" ? "u" : name}-${randomBytes(24).toString("hex")}`;
  canaries.push(value); fs.writeFileSync(file, value);
  aaa("account", "import", name, "--harness", "claude", "--file", file);
}
const threads = () => aaa("thread", "list", "demo").threads;
const jobs = () => aaa("account", "starts", "demo").starts;
const launch = (id, title = id, extra = []) => aaa("thread", "start", "demo", "--title", title, "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", task, "--account-pool", "--start-id", id, ...extra);
const refresh = (p) => p.evaluate(() => fetch('/api/v1/account-usage?refresh=1').then(r=>r.json()));
const sample = async (pane, who, n) => { aaa("pane", "send-keys", pane, "F6"); await waitOutput(pane, `POOL-UPDATED ${who} ${n}`); };

export default async function ({ page }) {
  let panel, pinnedA, pinnedB, chosen, cold, resumed, count;
  await step("기존 기본·명시 계정 시작과 두 실제 상태줄 관측", async () => {
    aaa("project", "create", "demo");
    aaa("project", "set", "demo", "max_parallel_threads", "20");
    for (const name of ["a", "b", "cold"]) register(name);
    aaa("account", "default", "claude", "a");
    pinnedA = aaa("thread", "start", "demo", "--title", "기존 기본", "--machine", "labA", "--profile", "fake", "--task-file", task);
    assert.equal(pinnedA.account, "a"); await waitOutput(pinnedA.pane, "POOL-READY a");
    pinnedB = aaa("thread", "start", "demo", "--title", "기존 명시", "--machine", "labA", "--profile", "fake", "--account", "b", "--task-file", task);
    assert.equal(pinnedB.account, "b"); await waitOutput(pinnedB.pane, "POOL-READY b");
    const dialog = await openSettings(page, "계정"); panel = dialog.getByTestId("account-pool");
    const q = (await refresh(page)).ok;
    assert.equal(q.accounts.find(a=>a.account === "a").windows.length, 2);
  });
  await step("웹의 host 공통 풀 편집·프로젝트 참여와 신규 로그인 자동 편입 없음", async () => {
    for (const name of ["a", "b", "cold"]) {
      await panel.getByTestId("pool-account").selectOption(name);
      await panel.getByTestId("pool-group").fill(name);
      await panel.getByTestId("pool-save").click();
      await panel.getByTestId("account-pool-members").locator(`[data-account="${name}"]`).waitFor();
    }
    await panel.getByTestId("pool-project-enabled").check();
    await until("host의 프로젝트 풀 참여", () => aaa("account", "pool", "list").pool.projects.includes("demo"));
    register("unapproved");
    assert.deepEqual(aaa("account", "pool", "list").pool.members.map(m=>m.account), ["a", "b", "cold"]);
  });
  await step("최신 잔량 후보 우선·기존 작업 예약·명시 별칭 우선·같은 시작 영수증", async () => {
    chosen = launch("choose-once");
    assert.equal(chosen.account, "b"); assert.equal(chosen.account_selection.unobserved, false);
    assert.match(chosen.account_selection.reason, /최신 잔량/); await waitOutput(chosen.pane, "POOL-READY b");
    count = threads().length;
    assert.equal(launch("choose-once").id, chosen.id); assert.equal(threads().length, count);
    const fixed = aaa("thread", "start", "demo", "--title", "풀보다 명시 우선", "--machine", "labA", "--profile", "fake", "--account", "a", "--task-file", task);
    assert.equal(fixed.account, "a"); await waitOutput(fixed.pane, "POOL-READY a");
    assert.equal(aaa("account", "default", "claude").defaults[0].account, "a");
  });
  await step("잔량 부족 뒤 미관측 자동 첫 실행은 한 번·내구 대기는 하나", async () => {
    fs.writeFileSync(modes, JSON.stringify({ a: [98, 98], b: [98, 98], cold: null }));
    await sample(pinnedA.pane, "a", 1); await sample(pinnedB.pane, "b", 1); await refresh(page);
    cold = launch("cold-first"); assert.equal(cold.account, "cold"); assert.equal(cold.account_selection.unobserved, true);
    await waitOutput(cold.pane, "POOL-READY cold");
    const pending = launch("wait-once"); assert.equal(pending.kind, "account_start"); assert.equal(pending.start.state, "queued");
    count = threads().length; assert.equal(launch("wait-once").start.id, "wait-once");
    assert.equal(jobs().filter(s=>s.id === "wait-once").length, 1); assert.equal(threads().length, count);
    await panel.getByTestId("account-pool-starts").locator('[data-start-id="wait-once"]').waitFor();
  });
  await step("host 재시작 뒤 풀·관측·첫 실행·대기 보존과 신선한 새 관측 뒤 한 번 기동", async () => {
    await stop("host"); start("host", "host");
    await until("host와 agent 재연결", () => aaa("machine", "list").machines.some(m=>m.name === "labA" && m.connection === "connected"));
    assert.equal(threads().length, count); assert.equal(jobs().find(s=>s.id === "wait-once").state, "queued");
    assert.equal(aaa("account", "pool", "list").pool.members.length, 3);
    fs.writeFileSync(modes, JSON.stringify({ a: [98, 98], b: [20, 20], cold: null }));
    await sample(pinnedB.pane, "b", 2); await refresh(page); aaa("ticker", "tick");
    const id = await until("대기 작업의 새 관측 후 기동", () => jobs().find(s=>s.id === "wait-once")?.thread);
    resumed = await until("기동 영수증 뒤 실제 pane 생성", () => { const t = aaa("thread", "show", "demo", id); return t.pane ? t : null; });
    assert.equal(resumed.account, "b"); await waitOutput(resumed.pane, "POOL-READY b");
    assert.equal(threads().length, count+1); assert.equal(launch("wait-once").id, id); assert.equal(threads().length, count+1);
  });
  await step("할 일의 웹 시작도 계정 대기를 표시하고 취소하면 기동하지 않는다", async () => {
    fs.writeFileSync(modes, JSON.stringify({ a: [98, 98], b: [98, 98], cold: null }));
    await sample(pinnedB.pane, "b", 3); await sample(chosen.pane, "b", 1); await sample(resumed.pane, "b", 1); await refresh(page);
    fs.writeFileSync(`${root}/projects/demo/TASKS.md`, "## Backlog\n- [ ] 풀 대기 task (fake@labA) [작업 폴더]\n  독립 작업을 끝내고 결과를 쓴다\n"); aaa("ticker", "tick");
    await page.getByRole("button", { name: "설정 닫기", exact: true }).click();
    await page.getByRole("dialog", { name: "설정", exact: true }).waitFor({ state: "hidden" });
    await page.getByTestId("activity-tasks").click();
    await page.locator('.task-row[data-title="풀 대기 task"] .task-pick').click();
    await page.getByTestId("task-handoff-start").click();
    await page.getByTestId("task-account-size").selectOption("small");
    await page.getByTestId("task-start-go").click();
    await page.getByTestId("task-account-wait").waitFor();
    try {
      await page.getByTestId("task-account-wait").getByRole("button", { name: "배정 대기 취소" }).click();
      await page.getByTestId("task-start").waitFor({ state: "hidden" });
    } catch (error) {
      const dialog = await page.evaluate(() => ({
        error: document.querySelector('[data-testid="task-start-error"]')?.textContent ?? null,
        waiting: document.querySelector('[data-testid="task-account-wait"]')?.textContent ?? null,
        state: document.querySelector('.task-start-form')?.getAttribute("data-start-state") ?? null,
      }));
      console.error("배정 대기 취소 실패:", JSON.stringify({ ...dialog, starts: jobs() }));
      throw error;
    }
    assert.equal(jobs().find(s=>s.title === "풀 대기 task").state, "cancelled");
    aaa("ticker", "tick"); assert.equal(threads().length, count+1);
  });
  await step("WebKit 390px·Reader 풀 조회·쓰기 거부·비밀 비노출", async () => {
    const { webkit } = await import("../lib/harness.mjs");
    const engine = await webkit.launch({ headless: true });
    try {
      const { page: phone } = await pairPage(engine, aaa("web", "pair", "--reader").code, { width: 390, height: 844 });
      const d = await openSettings(phone, "계정"); const p = d.getByTestId("account-pool");
      await p.getByTestId("account-pool-members").locator('[data-account="cold"]').waitFor();
      assert.equal(await p.getByTestId("pool-save").count(), 0);
      const denied = await phone.evaluate(async () => (await fetch('/api/v1/call', { method:'POST', headers:{'content-type':'application/json'}, body:JSON.stringify({op:'account_pool_member',revision:'',account:'a',member:null}) })).json());
      assert.equal(denied.err.code, "forbidden");
      assert.equal(await p.evaluate(e=>e.scrollWidth > e.clientWidth), false);
      const dump = await phone.evaluate(async () => JSON.stringify(await (await fetch('/api/v1/call', { method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({op:'account_pool'}) })).json()));
      for (const token of canaries) assert.ok(!dump.includes(token));
      assert.ok(!(await p.innerText()).includes("@example.com"));
      await shot(phone, "81-account-pool-phone"); await phone.context().close();
    } finally { await engine.close(); }
    const scan = (dir) => { for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
      const path = `${dir}/${e.name}`; if(e.isDirectory())scan(path);
      else if(e.isFile()){const bytes=fs.readFileSync(path);for(const token of canaries)assert.ok(!bytes.includes(Buffer.from(token)), `인증 원문이 ${e.name}에 남음`);}
    }};
    scan(`${root}/s/host`);
  });
}
