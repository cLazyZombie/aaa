import { token as fixtureToken, freshToken } from "../../../scripts/fixture-secret.mjs";
// A3: 실제 host·agent·session·PTY, 가짜 Claude 상태줄. 운영 로그인·인증·모델 작업은 없다.
import fs from "node:fs";
import assert from "node:assert/strict";
import { root, aaa, step, browser, webkit, pairPage, openSettings, waitOutput, until, domIs, inTime, shot, start, stop } from "../lib/harness.mjs";

const modes = `${root}/pool-modes.json`;
const fake = `${root}/pool-claude.mjs`;
const task = `${root}/pool-task.md`;
fs.writeFileSync(modes, JSON.stringify({ a: [20, 75], b: [40, 40], cold: null }));
fs.writeFileSync(task, "허용한 계정으로 독립 작업을 한다");
fs.writeFileSync(fake, `#!${process.execPath}
import fs from 'node:fs';
import { execSync } from 'node:child_process';
const token=process.env.CLAUDE_CODE_OAUTH_TOKEN;
const who=JSON.parse(fs.readFileSync(${JSON.stringify(`${root}/pool-identities.json`)},'utf8'))[token];
if(!who)throw new Error('계정 인증 전달 실패');
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
const identities = `${root}/pool-identities.json`;
fs.writeFileSync(identities, "{}");
function register(name) {
  const file = `${root}/${name}.token`;
  // 계정명은 토큰 밖의 시험 전용 대응표에 둔다.
  const value = freshToken();
  const names = JSON.parse(fs.readFileSync(identities, "utf8"));
  names[value] = name === "new-account" ? "n" : name;
  fs.writeFileSync(identities, JSON.stringify(names));
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

// 실제 응답의 전달 순서만 제어한다. Service Worker 여부와 관계없이 원래 fetch를 그대로 쓴다.
function switchProbe() {
  const trace = [];
  const held = new Map(), waiting = new Map();
  const probe = window.__poolSwitch = { trace, reads: false, count: 0, beforeMember: false,
    wait: id => held.has(id) ? Promise.resolve() : new Promise(done => waiting.set(id, done)),
    release: id => { const done = held.get(id); held.delete(id); done(); } };
  const hold = id => new Promise(done => { held.set(id, done); waiting.get(id)?.(); waiting.delete(id); });
  const record = kind => {
    const e = document.querySelector('[data-account="cold"] [data-testid="account-auto-assign"]');
    trace.push({ at: performance.now(), kind, checked: e?.checked, disabled: e?.disabled });
  };
  for (const kind of ["click", "input", "change"]) for (const capture of [true, false])
    document.addEventListener(kind, e => { if (e.target.dataset?.testid === "account-auto-assign") record(`${kind}/${capture}`); }, capture);
  let previous;
  new MutationObserver(() => {
    const e = document.querySelector('[data-account="cold"] [data-testid="account-auto-assign"]');
    const value = JSON.stringify([e?.checked, e?.disabled]);
    if (value !== previous) { previous = value; record("DOM"); }
  }).observe(document, { subtree: true, childList: true, attributes: true });
  const original = window.fetch.bind(window);
  window.fetch = async (...args) => {
    const body = typeof args[1]?.body === "string" ? JSON.parse(args[1].body) : null;
    if (!["account_pool", "account_pool_member"].includes(body?.op)) return original(...args);
    const id = probe.reads ? body.op === "account_pool_member" ? "member" : `read${++probe.count}` : null;
    trace.push({ at: performance.now(), kind: "request", ...body });
    if (probe.beforeMember && body.op === "account_pool_member") { probe.beforeMember = false; await hold("request"); }
    const response = await original(...args);
    if (id) await hold(id);
    const data = await response.clone().json();
    trace.push({ at: performance.now(), kind: "response", op: body.op, id, revision: data.ok?.pool?.revision,
      members: data.ok?.pool?.members.map(m => m.account), error: data.err?.code });
    return response;
  };
}
const switchState = (p, checked) => domIs(p, "저장한 스위치와 다음 조작 준비", want => {
  const input = document.querySelector('[data-account="cold"] [data-testid="account-auto-assign"]');
  return input && input.checked === want && !input.disabled;
}, checked);
const heldSwitch = (p, id) => inTime(p.evaluate(id => window.__poolSwitch.wait(id), id), 20_000, `계정 ${id} 응답 도착`);
const releaseSwitch = (p, id) => p.evaluate(id => window.__poolSwitch.release(id), id);

// Reader도 목록 생성과 사용 설정 응답은 별개다. 실제 응답만 보류해 그 경계를 지킨다.
function readerPoolProbe() {
  let arrive, release;
  const arrived = new Promise(done => { arrive = done; });
  const gate = new Promise(done => { release = done; });
  const trace = [];
  window.__readerPool = { arrived, release, trace };
  const original = window.fetch.bind(window);
  window.fetch = async (...args) => {
    const body = typeof args[1]?.body === "string" ? JSON.parse(args[1].body) : null;
    if (!["account_pool", "account_pool_member"].includes(body?.op)) return original(...args);
    trace.push({ kind: "request", op: body.op });
    const response = await original(...args);
    const data = await response.clone().json();
    trace.push({ kind: "response", op: body.op, status: response.status,
      members: data.ok?.pool?.members.map(m => m.account), error: data.err?.code });
    if (body.op === "account_pool") { arrive(); await gate; }
    trace.push({ kind: "delivered", op: body.op });
    return response;
  };
}

export default async function ({ page }) {
  let pinnedA, pinnedB, chosen, count, resumed;
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
    const accessToken = fixtureToken("pool-codex-access"), refreshToken = fixtureToken("pool-codex-refresh");
    const value = JSON.stringify({tokens:{access_token:accessToken,refresh_token:refreshToken,id_token:"e30.e30.c2ln"}});
    canaries.push(value, accessToken, refreshToken); fs.writeFileSync(auth, value);
    aaa("account", "import", "codex-default", "--harness", "codex", "--file", auth);
    aaa("account", "default", "codex", "codex-default"); aaa("account", "pool", "remove", "codex-default");
    const fallback = aaa("thread", "start", "demo", "--title", "다른 harness 기본", "--machine", "labA", "--profile", "unpooled", "--task-file", task);
    assert.equal(fallback.account, "codex-default"); await waitOutput(fallback.pane, "CODEX-DEFAULT-READY");
    const dialog = await openSettings(page, "계정");
    await dialog.getByTestId("accounts-claude").waitFor();
    // 자동 배정 설명 칸과 끝난 시작 기록은 계정 화면에 두지 않는다. 대기는 할 일 화면이 보인다.
    assert.equal(await dialog.getByTestId("account-pool").count(), 0);
    for (const help of ["골고루", "관측이 없으면"]) assert.ok(!(await dialog.innerText()).includes(help), help);
    await page.getByRole("button", { name: "설정 닫기", exact: true }).click();
    await page.getByRole("region", { name: "설정", exact: true }).waitFor({ state: "hidden" });
    await page.getByTestId("activity-tasks").click();
    const q = (await refresh(page)).ok;
    assert.equal(q.accounts.find(a => a.account === "a").windows.length, 2);
  });
  await step("Chromium·WebKit 1280·390px의 harness별 계정 스위치와 그룹·모델·프로젝트 제거", async () => {
    const engine = await webkit.launch({ headless: true });
    try {
      for (const [name, b] of [["chromium", browser], ["webkit", engine]]) for (const width of [1280, 390]) {
        const { page: p } = await pairPage(b, aaa("web", "pair").code, { width, height: 900 }, { init: switchProbe });
        try {
          const d = await openSettings(p, "계정");
          const rows = d.getByTestId("accounts-claude");
          await rows.locator('[data-account="cold"]').waitFor();
          assert.equal(await d.getByTestId("pool-group").count(), 0);
          assert.equal(await d.getByTestId("pool-models").count(), 0);
          assert.equal(await d.getByTestId("pool-project-enabled").count(), 0);
          const toggle = rows.locator('[data-account="cold"]').getByTestId("account-auto-assign");
          await switchState(p, true);
          const orderReads = name === "chromium" && width === 1280;
          if (orderReads) await p.evaluate(() => { window.__poolSwitch.reads = true; });
          await toggle.uncheck(); await until("계정 사용 해제", () => !enabled("cold"));
          if (orderReads) {
            // 사건 읽기1, 저장 뒤 읽기2, 실제 accounts 사건의 읽기3을 순서대로 붙잡는다.
            await heldSwitch(p, "read1"); await releaseSwitch(p, "member");
            await heldSwitch(p, "read2");
            aaa("account", "default", "claude", "a");
            await heldSwitch(p, "read3"); await releaseSwitch(p, "read2");
            await p.evaluate(() => new Promise(requestAnimationFrame));
            assert.equal(await toggle.isDisabled(), true, "버려진 읽기2가 끝나도 최신 읽기3까지 저장 중이다");
            assert.equal(await toggle.isChecked(), false, "최신 읽기 전에도 사용자의 해제를 보존한다");
            await releaseSwitch(p, "read3"); await switchState(p, false);
            await releaseSwitch(p, "read1");
            await p.evaluate(() => { window.__poolSwitch.reads = false; });
          }
          await switchState(p, false);
          // root capture/bubble 사이의 실제 부모 갱신. 체크 값·응답은 직접 바꾸지 않는다.
          await p.evaluate(() => {
            const input = document.querySelector('[data-account="cold"] [data-testid="account-auto-assign"]');
            const interleave = event => {
              if (event.target !== input) return;
              document.getElementById("root").removeEventListener("click", interleave, true);
              const refresh = [...document.querySelectorAll('[data-testid="settings-accounts"] button')].find(b => b.textContent === "사용량 다시 읽기");
              refresh.dispatchEvent(new MouseEvent("click", { bubbles: true }));
              window.__poolSwitch.trace.push({ kind: "parent-refresh", same: input === document.querySelector('[data-account="cold"] [data-testid="account-auto-assign"]') });
            };
            document.getElementById("root").addEventListener("click", interleave, true);
          });
          await toggle.check(); await until("계정 사용", () => enabled("cold"));
          await switchState(p, true);
          const writes = await p.evaluate(() => window.__poolSwitch.trace.filter(e => e.kind === "request" && e.op === "account_pool_member"));
          assert.deepEqual(writes.map(w => !!w.member), [false, true], "실제 누름마다 한 번만 저장한다");
          assert.deepEqual(await p.evaluate(() => window.__poolSwitch.trace.filter(e => e.kind === "parent-refresh").map(e => e.same)), [true]);
          if (name === "webkit" && width === 390) {
            await p.evaluate(() => { window.__poolSwitch.beforeMember = true; });
            // 거절을 기대하는 실제 클릭: uncheck의 성공 후 조건으로 거절을 덮지 않는다.
            await toggle.click(); await heldSwitch(p, "request");
            aaa("account", "pool", "remove", "a");
            await releaseSwitch(p, "request");
            await d.getByRole("alert").filter({ hasText: "설정이 바뀌었다" }).waitFor();
            await switchState(p, true); assert.ok(enabled("cold"), "거절된 변경은 원래 사용 설정으로 돌아간다");
            assert.equal((await p.evaluate(() => window.__poolSwitch.trace.filter(e => e.kind === "response" && e.op === "account_pool_member"))).at(-1).error, "conflict");
            aaa("account", "pool", "add", "a");
          }
          assert.equal(threads().find(t => t.id === pinnedA.id).account, "a");
          assert.equal(threads().find(t => t.id === pinnedB.id).account, "b");
          assert.equal(await d.getByTestId("accounts-codex").getByTestId("account-auto-assign").isChecked(), false);
          assert.equal(await d.evaluate(e => e.scrollWidth > e.clientWidth), false);
          await shot(p, `81-account-pool-${name}-${width}`);
        } catch (error) {
          const trace = await p.evaluate(() => window.__poolSwitch.trace).catch(e => [{ diagnostic: String(e) }]);
          fs.mkdirSync(`${root}/diagnostics`, { recursive: true });
          fs.writeFileSync(`${root}/diagnostics/account-switch-${name}-${width}.json`, JSON.stringify(trace, null, 2));
          throw new Error(`${name}/${width}: ${error.message}`, { cause: error });
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
    const row = page.getByTestId("tasks-account-waits").locator('[data-start-id="wait-once"]');
    await row.waitFor(); assert.match(await row.textContent(), /사용 한도 도달/);
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
    await page.getByTestId("tasks-account-waits").waitFor({ state: "hidden" });
    await page.locator('.task-row[data-title="풀 대기 task"] .task-pick').click();
    await page.getByTestId("task-handoff-start").click();
    assert.equal(await page.getByTestId("task-account-size").count(), 0);
    await page.getByTestId("task-start-go").click(); await page.getByTestId("task-account-wait").waitFor();
    await page.getByTestId("tasks-account-waits").getByText("풀 대기 task").waitFor();
    await page.getByTestId("task-account-wait").getByRole("button", { name: "배정 대기 취소" }).click();
    await page.getByTestId("task-start").waitFor({ state: "hidden" });
    await page.getByTestId("tasks-account-waits").waitFor({ state: "hidden" });
    assert.equal(jobs().find(s => s.title === "풀 대기 task").state, "cancelled");
    aaa("ticker", "tick"); assert.equal(threads().length, count + 1);
  });
  await step("Reader 사용 설정 조회·쓰기 거부·비밀 비노출", async () => {
    const { page: reader } = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 844 }, { init: readerPoolProbe });
    try {
      const d = await openSettings(reader, "계정");
      const toggle = d.getByTestId("accounts-claude").locator('[data-account="cold"]').getByTestId("account-auto-assign");
      await toggle.waitFor();
      await inTime(reader.evaluate(() => window.__readerPool.arrived), 5_000, "Reader 실제 사용 설정 응답");
      assert.equal(await toggle.isDisabled(), true, "설정을 읽는 동안도 Reader는 쓸 수 없다");
      await reader.evaluate(() => window.__readerPool.release());
      await domIs(reader, "Reader 사용 설정 DOM 반영", () => {
        const input = document.querySelector('[data-account="cold"] [data-testid="account-auto-assign"]');
        return input?.checked === true && input.disabled === true;
      }, null, 5_000);
      assert.equal(await toggle.isChecked(), true); assert.equal(await toggle.isDisabled(), true);
      const denied = await reader.evaluate(async () => (await fetch('/api/v1/call', { method:'POST', headers:{'content-type':'application/json'}, body:JSON.stringify({op:'account_pool_member',revision:'',account:'a',member:null}) })).json());
      assert.equal(denied.err.code, "forbidden");
      const dump = await reader.evaluate(async () => JSON.stringify(await (await fetch('/api/v1/call', { method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({op:'account_pool'}) })).json()));
      for (const token of canaries) assert.ok(!dump.includes(token));
      assert.ok(!(await d.innerText()).includes("@example.com"));
      await shot(reader, "81-reader-pool");
    } catch (error) {
      fs.mkdirSync(`${root}/diagnostics`, { recursive: true });
      const state = await reader.evaluate(() => {
        const input = document.querySelector('[data-account="cold"] [data-testid="account-auto-assign"]');
        return { trace: window.__readerPool.trace, checked: input?.checked, disabled: input?.disabled };
      }).catch(diagnostic => ({ diagnostic: String(diagnostic) }));
      fs.writeFileSync(`${root}/diagnostics/reader-pool.json`, JSON.stringify({ error: { message: error.message, stack: error.stack }, state }, null, 2));
      await reader.screenshot({ path: `${root}/diagnostics/reader-pool.png` }).catch(() => {});
      throw error;
    } finally { await reader.context().close(); }
    const scan = (dir) => { for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
      const path = `${dir}/${e.name}`; if (e.isDirectory()) scan(path);
      else if (e.isFile()) { const bytes=fs.readFileSync(path); for (const token of canaries) assert.ok(!bytes.includes(Buffer.from(token)), `인증 원문이 ${e.name}에 남음`); }
    }};
    scan(`${root}/s/host`);
  });
}
