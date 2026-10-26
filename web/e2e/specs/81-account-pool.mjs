// A3: 실제 host·agent·session·PTY, 가짜 Claude 상태줄. 운영 로그인·인증·모델 작업은 없다.
import fs from "node:fs";
import assert from "node:assert/strict";
import { randomBytes } from "node:crypto";
import { root, aaa, step, browser, webkit, pairPage, openSettings, waitOutput, until, domIs, shot, start, stop } from "../lib/harness.mjs";

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
 if(v){const reset=Math.floor(Date.now()/1000)+3600;execSync(settings.statusLine.command,{input:JSON.stringify({rate_limits:{five_hour:v[0]===null?null:{used_percentage:v[0],resets_at:reset},seven_day:{used_percentage:v[1],resets_at:reset+604800}}}),stdio:['pipe','pipe','pipe']});}
}
sample();process.stdout.write('POOL-READY '+who+'\\r\\n❯ ');
process.stdin.setRawMode(true);process.stdin.on('data',b=>{if(b.toString().includes('\\x1b[17~')){sample();process.stdout.write('POOL-UPDATED '+who+' '+(++n)+'\\r\\n❯ ');}});
`, { mode: 0o755 });
let host = fs.readFileSync(`${root}/c/host.toml`, "utf8");
host = host.replace('[profiles.fake]\nagent = "custom"', '[profiles.fake]\nagent = "claude"').replace(`command = ["/bin/sh", "${root}/fake.sh"]`, `command = [${JSON.stringify(fake)}]`);
const codex = `${root}/pool-codex.mjs`;
fs.writeFileSync(codex, `#!${process.execPath}
import fs from 'node:fs';
if(!fs.existsSync(process.env.CODEX_HOME+'/auth.json'))throw new Error('기본 Codex 인증 없음');
process.stdout.write('CODEX-DEFAULT-READY\\r\\n');
process.stdin.setRawMode(true);process.stdin.resume();
`, { mode: 0o755 });
const rpc = `${root}/pool-quota.mjs`;
fs.writeFileSync(rpc, `#!${process.execPath}
import readline from 'node:readline';
for await(const l of readline.createInterface({input:process.stdin})) { const v=JSON.parse(l); if(v.id!==undefined)console.log(JSON.stringify({id:v.id,result:{}})); }
`, { mode: 0o755 });
host = host.replace('allow_profiles = ["fake"]', 'allow_profiles = ["fake", "unpooled"]');
host += `\n[profiles.unpooled]\nagent = "codex"\ncommand = [${JSON.stringify(codex)}]\n`;
fs.writeFileSync(`${root}/c/host.toml`, host);
export const labEnv = { AAA_TEST_USAGE_REFRESH_MS: "0", AAA_TEST_QUOTA_CODEX: rpc };
const canaries = [];
function register(name) {
  const file = `${root}/${name}.token`;
  // 비밀의 8바이트 조각과 별칭이 같으면 기존 금고가 별칭을 거부한다. 새 계정의 canary는 별칭을 담지 않는다.
  const value = `fixture-pool-${name === "new-account" ? "n" : name}-${randomBytes(24).toString("hex")}`;
  canaries.push(value); fs.writeFileSync(file, value);
  aaa("account", "import", name, "--harness", "claude", "--file", file);
}
const threads = () => aaa("thread", "list", "demo").threads;
const jobs = () => aaa("account", "starts", "demo").starts;
const launch = (id, title = id, extra = []) => aaa("thread", "start", "demo", "--title", title, "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", task, "--account-pool", "--start-id", id, ...extra);
const refresh = (p) => p.evaluate(() => fetch('/api/v1/account-usage?refresh=1').then(r=>r.json()));
const samples = new Map();
const sample = async (pane, who) => {
  const n = (samples.get(pane) ?? 0) + 1; samples.set(pane, n);
  aaa("pane", "send-keys", pane, "F6"); await waitOutput(pane, `POOL-UPDATED ${who} ${n}`);
};
const sampleAll = async (page, values) => {
  fs.writeFileSync(modes, JSON.stringify(values));
  for (const t of threads().filter(t => t.profile === "fake" && t.pane)) await sample(t.pane, t.account);
  return (await refresh(page)).ok;
};
const enabled = name => aaa("account", "pool", "list").pool.members.some(m => m.account === name);

export default async function ({ page }) {
  let panel, pinnedA, pinnedB, chosen, count, resumed;
  await step("계정 등록만으로 참여·기존 기본과 명시 시작·다른 harness 기본 fallback", async () => {
    aaa("project", "create", "demo");
    aaa("project", "set", "demo", "max_parallel_threads", "20");
    for (const name of ["a", "b", "cold"]) { register(name); assert.ok(enabled(name)); }
    for (const name of ["a", "b", "cold"]) aaa("account", "pool", "remove", name);
    aaa("account", "default", "claude", "a");
    pinnedA = aaa("thread", "start", "demo", "--title", "기존 기본", "--machine", "labA", "--profile", "fake", "--task-file", task);
    assert.equal(pinnedA.account, "a"); await waitOutput(pinnedA.pane, "POOL-READY a");
    pinnedB = aaa("thread", "start", "demo", "--title", "기존 명시", "--machine", "labA", "--profile", "fake", "--account", "b", "--task-file", task);
    assert.equal(pinnedB.account, "b"); await waitOutput(pinnedB.pane, "POOL-READY b");
    for (const name of ["a", "b", "cold"]) aaa("account", "pool", "add", name);
    const auth = `${root}/codex.token`;
    const value = JSON.stringify({tokens:{access_token:"fixture-codex-access-01234567890123456789",refresh_token:"fixture-codex-refresh-01234567890123456789",id_token:"e30.e30.c2ln"}});
    canaries.push(value); fs.writeFileSync(auth, value);
    aaa("account", "import", "codex-default", "--harness", "codex", "--file", auth);
    aaa("account", "default", "codex", "codex-default"); aaa("account", "pool", "remove", "codex-default");
    const fallback = aaa("thread", "start", "demo", "--title", "다른 harness 기본", "--machine", "labA", "--profile", "unpooled", "--task-file", task);
    assert.equal(fallback.account, "codex-default"); await waitOutput(fallback.pane, "CODEX-DEFAULT-READY");
    const dialog = await openSettings(page, "계정"); panel = dialog.getByTestId("account-pool");
    const q = (await refresh(page)).ok;
    assert.equal(q.accounts.find(a => a.account === "a").windows.length, 2);
  });
  await step("Chromium·WebKit 1280·390px의 harness별 계정 스위치와 그룹·모델·프로젝트 제거", async () => {
    const engine = await webkit.launch({ headless: true });
    try {
      for (const [name, b] of [["chromium", browser], ["webkit", engine]]) for (const width of [1280, 390]) {
        const { page: p } = await pairPage(b, aaa("web", "pair").code, { width, height: 900 });
        try {
          const d = await openSettings(p, "계정");
          const rows = d.getByTestId("accounts-claude");
          await rows.locator('[data-account="cold"]').waitFor();
          assert.equal(await d.getByTestId("pool-group").count(), 0);
          assert.equal(await d.getByTestId("pool-models").count(), 0);
          assert.equal(await d.getByTestId("pool-project-enabled").count(), 0);
          const toggle = rows.locator('[data-account="cold"]').getByTestId("account-auto-assign");
          await toggle.uncheck(); await until("계정 사용 해제", () => !enabled("cold"));
          await domIs(p, "스위치 저장 뒤 활성", () => !document.querySelector('[data-account="cold"] [data-testid="account-auto-assign"]').disabled);
          await toggle.check(); await until("계정 사용", () => enabled("cold"));
          assert.equal(await d.getByTestId("accounts-codex").getByTestId("account-auto-assign").isChecked(), false);
          assert.equal(await d.evaluate(e => e.scrollWidth > e.clientWidth), false);
          await shot(p, `81-account-pool-${name}-${width}`);
        } finally { await p.context().close(); }
      }
    } finally { await engine.close(); }
    register("new-account"); assert.ok(enabled("new-account"), "새 등록은 별도 참여 단계가 없다");
    aaa("account", "pool", "remove", "new-account");
    assert.deepEqual(aaa("account", "pool", "list").pool.projects, []);
  });
  await step("작업 수 우선의 고른 배정·미관측 반복 시작·같은 영수증·명시 계정 우선", async () => {
    chosen = launch("choose-once");
    assert.equal(chosen.account, "cold"); assert.equal(chosen.account_selection.unobserved, true);
    await waitOutput(chosen.pane, "POOL-READY cold");
    count = threads().length;
    assert.equal(launch("choose-once").id, chosen.id); assert.equal(threads().length, count);
    for (const name of ["a", "b"]) aaa("account", "pool", "remove", name);
    const second = launch("unknown-again"); assert.equal(second.account, "cold");
    assert.equal(second.account_selection.unobserved, true); await waitOutput(second.pane, "POOL-READY cold");
    for (const name of ["a", "b"]) aaa("account", "pool", "add", name);
    const fixed = aaa("thread", "start", "demo", "--title", "명시 계정", "--machine", "labA", "--profile", "fake", "--account", "b", "--task-file", task);
    assert.equal(fixed.account, "b"); await waitOutput(fixed.pane, "POOL-READY b");
    assert.equal(aaa("account", "default", "claude").defaults[0].account, "a");
  });
  await step("5시간 창 없음·주간 10%는 시작하고 주간 100%는 배정 대기", async () => {
    const q = await sampleAll(page, { a: [100, 100], b: [100, 100], cold: [null, 10] });
    const cold = q.accounts.find(q => q.account === "cold");
    assert.equal(cold.windows.length, 1); assert.equal(cold.windows[0].duration_mins, 10080);
    assert.equal(cold.windows[0].used_percent, 10);
    const weekOnly = launch("week-only");
    assert.equal(weekOnly.account, "cold"); assert.equal(weekOnly.account_selection.unobserved, false);
    await waitOutput(weekOnly.pane, "POOL-READY cold");
    await sampleAll(page, { a: [100, 100], b: [100, 100], cold: [null, 100] });
    const pending = launch("wait-once"); assert.equal(pending.kind, "account_start"); assert.equal(pending.start.state, "queued");
    assert.match(pending.start.reason, /사용 한도 도달/);
    count = threads().length; assert.equal(launch("wait-once").start.id, "wait-once");
    assert.equal(jobs().filter(s => s.id === "wait-once").length, 1); assert.equal(threads().length, count);
    await panel.getByTestId("account-pool-starts").locator('[data-start-id="wait-once"]').waitFor();
  });
  await step("host 재시작 뒤 설정·대기·영수증 보존과 한도 해제 뒤 한 번 기동", async () => {
    await stop("host"); start("host", "host");
    await until("host와 agent 재연결", () => aaa("machine", "list").machines.some(m => m.name === "labA" && m.connection === "connected"));
    assert.equal(threads().length, count); assert.equal(jobs().find(s => s.id === "wait-once").state, "queued");
    assert.equal(aaa("account", "pool", "list").pool.members.length, 3);
    await sampleAll(page, { a: [100, 100], b: [10, 10], cold: [null, 100] }); aaa("ticker", "tick");
    const id = await until("대기 작업의 한도 해제 후 기동", () => jobs().find(s => s.id === "wait-once")?.thread);
    resumed = await until("영수증 뒤 pane 생성", () => { const t = aaa("thread", "show", "demo", id); return t.pane ? t : null; });
    assert.equal(resumed.account, "b"); await waitOutput(resumed.pane, "POOL-READY b");
    assert.equal(threads().length, count + 1); assert.equal(launch("wait-once").id, id); assert.equal(threads().length, count + 1);
  });
  await step("할 일 웹 시작의 배정 대기·취소와 예상 소비량 선택 제거", async () => {
    await sampleAll(page, { a: [100, 100], b: [100, 100], cold: [null, 100] });
    fs.writeFileSync(`${root}/projects/demo/TASKS.md`, "## Backlog\n- [ ] 풀 대기 task (fake@labA) [작업 폴더]\n  독립 작업을 끝내고 결과를 쓴다\n"); aaa("ticker", "tick");
    await page.getByRole("button", { name: "설정 닫기", exact: true }).click();
    await page.getByRole("region", { name: "설정", exact: true }).waitFor({ state: "hidden" });
    await page.getByTestId("activity-tasks").click();
    await page.locator('.task-row[data-title="풀 대기 task"] .task-pick').click();
    await page.getByTestId("task-handoff-start").click();
    assert.equal(await page.getByTestId("task-account-size").count(), 0);
    await page.getByTestId("task-start-go").click(); await page.getByTestId("task-account-wait").waitFor();
    await page.getByTestId("task-account-wait").getByRole("button", { name: "배정 대기 취소" }).click();
    await page.getByTestId("task-start").waitFor({ state: "hidden" });
    assert.equal(jobs().find(s => s.title === "풀 대기 task").state, "cancelled");
    aaa("ticker", "tick"); assert.equal(threads().length, count + 1);
  });
  await step("Reader 사용 설정 조회·쓰기 거부·비밀 비노출", async () => {
    const { page: reader } = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 844 });
    try {
      const d = await openSettings(reader, "계정");
      const toggle = d.getByTestId("accounts-claude").locator('[data-account="cold"]').getByTestId("account-auto-assign");
      await toggle.waitFor(); assert.equal(await toggle.isChecked(), true); assert.equal(await toggle.isDisabled(), true);
      const denied = await reader.evaluate(async () => (await fetch('/api/v1/call', { method:'POST', headers:{'content-type':'application/json'}, body:JSON.stringify({op:'account_pool_member',revision:'',account:'a',member:null}) })).json());
      assert.equal(denied.err.code, "forbidden");
      const dump = await reader.evaluate(async () => JSON.stringify(await (await fetch('/api/v1/call', { method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({op:'account_pool'}) })).json()));
      for (const token of canaries) assert.ok(!dump.includes(token));
      assert.ok(!(await d.innerText()).includes("@example.com"));
    } finally { await reader.context().close(); }
    const scan = (dir) => { for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
      const path = `${dir}/${e.name}`; if (e.isDirectory()) scan(path);
      else if (e.isFile()) { const bytes=fs.readFileSync(path); for (const token of canaries) assert.ok(!bytes.includes(Buffer.from(token)), `인증 원문이 ${e.name}에 남음`); }
    }};
    scan(`${root}/s/host`);
  });
}
