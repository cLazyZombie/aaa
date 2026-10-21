// A2: 실제 host·agent, 가짜 공식 RPC와 상태줄. 실제 로그인·인증 파일·모델 작업은 없다.
import fs from "node:fs";
import assert from "node:assert/strict";
import { randomBytes } from "node:crypto";
import { root, AA, aaa, step, browser, pairPage, openSettings, domIs, waitOutput, shot, stop, until } from "../lib/harness.mjs";

const fake = `${root}/quota-codex`;
const modeFile = `${root}/quota-modes.json`;
fs.writeFileSync(modeFile, "{}");
fs.writeFileSync(fake, `#!/usr/bin/env python3
import sys,os,json,time
if sys.argv[1:]==['--version']:
 print('codex-cli 0.163.0');sys.exit(0)
assert sys.argv[1:]==['app-server']
auth=json.load(open(os.environ['CODEX_HOME']+'/auth.json'))
who=auth['tokens']['access_token'].split('-')[0]
for line in sys.stdin:
 v=json.loads(line)
 with open(${JSON.stringify(`${root}/quota-rpc.methods`)},'a') as f:f.write(who+' '+v['method']+'\\n')
 if v['method']=='initialize':print(json.dumps({'id':v['id'],'result':{}}),flush=True)
 elif v['method']=='account/rateLimits/read':
  mode=json.load(open(${JSON.stringify(modeFile)})).get(who,'ok')
  if mode!='ok':print(json.dumps({'id':v['id'],'error':{'code':-32603,'message':'discard-this-raw-response','data':{'status':int(mode),'retryAfterSeconds':600}}}),flush=True)
  else:print(json.dumps({'id':v['id'],'result':{'private':'discard-this-raw-response','rateLimitsByLimitId':{'codex':{'primary':{'usedPercent':25 if who=='work' else 60,'windowDurationMins':300,'resetsAt':int(time.time())+3600},'secondary':{'usedPercent':50,'windowDurationMins':10080,'resetsAt':int(time.time())+604800}}}}}),flush=True)
 else:assert v['method']=='initialized'
`, { mode: 0o755 });
fs.writeFileSync(`${root}/quota-task.md`, "사용량 관측 fixture");
const claude = `${root}/quota-claude`;
fs.writeFileSync(claude, `#!${process.execPath}
import fs from 'node:fs';
import { execSync } from 'node:child_process';
const settings=JSON.parse(fs.readFileSync(process.argv[process.argv.indexOf('--settings')+1],'utf8'));
const send=(p)=>execSync(settings.statusLine.command,{input:JSON.stringify({rate_limits:p}),stdio:['pipe','pipe','pipe']});
const reset=Math.floor(Date.now()/1000)+10000;
send({five_hour:{used_percentage:40,resets_at:reset},seven_day:{used_percentage:20,resets_at:reset+604800}});
process.stdout.write('QUOTA-OBSERVED\\r\\n❯ ');
process.stdin.setRawMode(true);
process.stdin.on('data',b=>{if(b.toString().includes('\\x1b[17~')){send({five_hour:{used_percentage:70,resets_at:reset}});process.stdout.write('QUOTA-UPDATED\\r\\n❯ ');}});
`, { mode: 0o755 });
let host = fs.readFileSync(`${root}/c/host.toml`, "utf8");
host = host.replace('[profiles.fake]\nagent = "custom"', '[profiles.fake]\nagent = "claude"').replace(`command = ["/bin/sh", "${root}/fake.sh"]`, `command = [${JSON.stringify(claude)}]`);
fs.writeFileSync(`${root}/c/host.toml`, host);
export const labEnv = { AAA_TEST_QUOTA_CODEX: fake, AAA_TEST_USAGE_REFRESH_MS: "0" };

const row = (p, name) => p.locator(`[data-testid="account-row"][data-account="${name}"]`);
const quota = (p, name) => row(p, name).getByTestId("account-quota");
function register(name, harness = "codex") {
  const file = `${root}/${name}-auth.json`;
  const claims = Buffer.from(JSON.stringify({ email: `${name}@example.com`, "https://api.openai.com/auth": { chatgpt_plan_type: "plus" } })).toString("base64url");
  fs.writeFileSync(file, harness === "codex" ? JSON.stringify({ tokens: { id_token: `e30.${claims}.c2ln`, access_token: `${name}-canary-access-0123456789`, refresh_token: `${name}-canary-refresh-0123456789` } }) : `fixture-oauth-${randomBytes(24).toString("hex")}`);
  aaa("account", "import", name, "--harness", harness, "--file", file);
}
async function refresh(page) {
  const reply = page.waitForResponse((r) => r.url().includes("/api/v1/account-usage?refresh=1"));
  await page.getByRole("button", { name: "사용량 다시 읽기", exact: true }).click();
  const body = await (await reply).json();
  await page.getByRole("button", { name: "사용량 다시 읽기", exact: true }).waitFor({ state: "visible" });
  await domIs(page, "사용량 읽기 끝", () => !Array.from(document.querySelectorAll('[data-testid="settings-accounts"] button')).find(b=>b.textContent === "사용량 다시 읽기")?.disabled);
  return body.ok;
}
export default async function ({ page }) {
  let panel, pane;
  await step("등록한 두 Codex 계정의 공식 무작업 조회와 관측 없는 Claude", async () => {
    aaa("project", "create", "demo");
    register("work"); register("spare"); register("observer", "claude");
    const dialog = await openSettings(page, "계정"); panel = dialog.getByTestId("settings-accounts");
    await quota(panel, "work").getByText("75% 남음", { exact: true }).waitFor();
    await quota(panel, "spare").getByText("40% 남음", { exact: true }).waitFor();
    await quota(panel, "observer").getByText("5시간 사용량 모름", { exact: true }).waitFor();
    const data = await page.evaluate(() => fetch('/api/v1/account-usage').then(r=>r.json()));
    assert.equal(data.ok.accounts.length, 3);
    assert.ok(!JSON.stringify(data).includes("canary") && !JSON.stringify(data).includes("@example.com"));
    const methods = fs.readFileSync(`${root}/quota-rpc.methods`, "utf8").trim().split("\n").map((s) => s.split(" ")[1]);
    assert.ok(methods.length >= 6);
    assert.ok(methods.every((m) => ["initialize", "initialized", "account/rateLimits/read"].includes(m)), methods.join(","));
  });
  await step("계정 pane의 상태줄 → agent snapshot → host 카드, 빠진 주간 창의 관측 시각 보존", async () => {
    const started = aaa("thread", "start", "demo", "--machine", "labA", "--profile", "fake", "--account", "observer", "--title", "사용량 관측", "--task-file", `${root}/quota-task.md`);
    pane = started.pane;
    await waitOutput(pane, "QUOTA-OBSERVED");
    const first = (await refresh(page)).accounts.find((a) => a.account === "observer");
    assert.equal(first.windows.length, 2);
    await quota(panel, "observer").getByText("60% 남음", { exact: true }).waitFor();
    aaa("pane", "send-keys", pane, "F6"); await waitOutput(pane, "QUOTA-UPDATED");
    const next = (await refresh(page)).accounts.find((a) => a.account === "observer");
    assert.equal(next.windows.find((w) => w.duration_mins === 10080).observed_at_ms, first.windows.find((w) => w.duration_mins === 10080).observed_at_ms);
    await quota(panel, "observer").getByText("30% 남음", { exact: true }).waitFor();
  });
  await step("401·429는 마지막 관측과 까닭, 제한 시각 전 다시 읽기는 RPC를 더 보내지 않는다", async () => {
    fs.writeFileSync(modeFile, JSON.stringify({ work: "429", spare: "401" }));
    await refresh(page);
    await quota(panel, "work").getByTestId("quota-problem").filter({ hasText: "조회 제한" }).waitFor();
    await quota(panel, "spare").getByTestId("quota-problem").filter({ hasText: "인증 만료" }).waitFor();
    await quota(panel, "work").getByText("마지막 75% 남음", { exact: true }).waitFor();
    const before = fs.readFileSync(`${root}/quota-rpc.methods`, "utf8").split("\n").filter((s) => s.startsWith("work ")).length;
    await refresh(page);
    assert.equal(fs.readFileSync(`${root}/quota-rpc.methods`, "utf8").split("\n").filter((s) => s.startsWith("work ")).length, before);
  });
  await step("320·390·768 폭 카드와 보기 전용 기기에는 메일·인증 원문 없음", async () => {
    for (const width of [320, 390, 768]) {
      const { page: mobile } = await pairPage(browser, aaa("web", "pair", "--reader").code, { width, height: 900 });
      const d = await openSettings(mobile, "계정"); const p = d.getByTestId("settings-accounts");
      await quota(p, "work").getByText("마지막 75% 남음", { exact: true }).waitFor();
      const over = await p.getByTestId("account-quota").evaluateAll((cards) => cards.some((c) => c.scrollWidth > c.clientWidth || c.getBoundingClientRect().right > innerWidth));
      assert.equal(over, false, `${width} 카드 폭`);
      assert.ok(!(await p.innerText()).includes("canary") && !(await p.innerText()).includes("@example.com"));
      await shot(mobile, `65-account-quota-${width}`); await mobile.context().close();
    }
  });
  await step("가짜 금고 잠김과 agent 연결 끊김은 마지막 숫자·관측 시각을 새 값으로 바꾸지 않는다", async () => {
    const key = `${root}/s/host/secrets.key`;
    fs.renameSync(key, `${root}/held-key`);
    try {
      await refresh(page);
      await quota(panel, "spare").getByTestId("quota-problem").filter({ hasText: "인증 잠김" }).waitFor();
      await quota(panel, "spare").getByText("마지막 40% 남음", { exact: true }).waitFor();
    } finally { fs.renameSync(`${root}/held-key`, key); }
    const before = await page.evaluate(() => fetch('/api/v1/account-usage').then(r=>r.json()));
    await stop("agent");
    await until("agent 끊김", () => aaa("machine", "list").machines.find((m) => m.name === "labA")?.connection === "disconnected");
    const after = await refresh(page);
    const old = before.ok.accounts.find((a) => a.account === "observer");
    const next = after.accounts.find((a) => a.account === "observer");
    assert.deepEqual(next.windows.map((w) => w.observed_at_ms), old.windows.map((w) => w.observed_at_ms));
    await quota(panel, "observer").getByTestId("quota-problem").filter({ hasText: "머신 연결 끊김" }).waitFor();
    await quota(panel, "observer").getByText("마지막 30% 남음", { exact: true }).waitFor();
  });
  await step("로그인 세대 교체는 살아 있는 옛 pane의 숫자와 실패·backoff를 새 인증에 붙이지 않는다", async () => {
    register("observer", "claude");
    await quota(panel, "observer").getByText("5시간 사용량 모름", { exact: true }).waitFor();
    assert.equal(await quota(panel, "observer").getByTestId("quota-window").count(), 0);
    fs.writeFileSync(modeFile, "{}"); register("work");
    await quota(panel, "work").getByText("75% 남음", { exact: true }).waitFor();
    const dump = await page.evaluate(() => fetch('/api/v1/account-usage').then(r=>r.text()));
    assert.ok(!dump.includes("canary") && !dump.includes("discard-this-raw-response"));
    assert.equal(fs.readdirSync(`${root}/s/host/quota-probes`).length, 0);
    const scan = (dir) => { for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
      const path = `${dir}/${e.name}`;
      if (e.isDirectory()) scan(path);
      else if (e.isFile()) { const bytes = fs.readFileSync(path); assert.ok(!bytes.includes(Buffer.from("canary-access")) && !bytes.includes(Buffer.from("canary-refresh")) && !bytes.includes(Buffer.from("discard-this-raw-response")), `원문이 상태·DB·로그에 남음: ${e.name}`); }
    } };
    scan(`${root}/s`);
  });
}
