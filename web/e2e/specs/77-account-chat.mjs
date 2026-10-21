// 계정별 실제 CODEX_HOME → agent → host → 웹. hook ID 불일치·아직 없는 기록·앞 쪽·감시를 함께 본다.
import fs from "node:fs";
import assert from "node:assert/strict";
import { root, AA, aaa, step, inList, domIs, waitOutput, shot, pairPage, browser, webkit } from "../lib/harness.mjs";

const fake = `${root}/chat-codex`;
fs.writeFileSync(fake, `#!/usr/bin/env python3
import os,sys,json,pathlib,subprocess,datetime,tty
if sys.argv[1:]==['--version']:
 print('codex-cli 0.163.0');sys.exit(0)
tty.setraw(sys.stdin.fileno())
tid=os.environ['AAA_THREAD']
sid='019a4c5e-1f2b-7c3d-8e4f-a5b6c7d8e9f0'
hook='019a4c5e-1f2b-7c3d-8e4f-a5b6c7d8e9f1'
roll=pathlib.Path(os.environ['CODEX_HOME'])/'sessions/2026/10/21'/('rollout-test-'+sid+'.jsonl')
subprocess.run([${JSON.stringify(AA)},'hook','--agent','codex',json.dumps({'type':'agent-turn-complete','thread-id':hook})],check=True,stdout=subprocess.PIPE)
print('CHAT-READY\\r\\n❯ ',end='',flush=True)
def line(n):return json.dumps(dict(type='response_item',payload=dict(type='message',role='assistant',content=[dict(type='output_text',text=tid+' 기록 '+str(n))])))+'\\n'
n=0
pending=b''
while True:
 b=os.read(0,4096)
 if not b:break
 pending=(pending+b)[-8192:]
 if b'\\x1b[17~' not in pending:continue
 pending=b''
 if n==0:
  roll.parent.mkdir(parents=True,exist_ok=True)
  meta=dict(type='session_meta',payload=dict(id=sid,session_id=sid,source='cli',cwd=os.getcwd(),timestamp=datetime.datetime.now(datetime.timezone.utc).isoformat(timespec='milliseconds').replace('+00:00','Z')))
  roll.write_text(json.dumps(meta)+'\\n'+''.join(line(i) for i in range(301)))
  n=301
 else:
  with roll.open('a') as f:f.write(line(n))
  n+=1
 print('CHAT-WRITTEN-'+str(n)+'\\r\\n❯ ',end='',flush=True)
`, { mode: 0o755 });
const config = `${root}/c/host.toml`;
fs.writeFileSync(config, fs.readFileSync(config, "utf8")
  .replace('[profiles.fake]\nagent = "custom"', '[profiles.fake]\nagent = "codex"')
  .replace(`command = ["/bin/sh", "${root}/fake.sh"]`, `command = [${JSON.stringify(fake)}]`));
fs.writeFileSync(`${root}/task.md`, "계정 대화 조회 fixture");
export const labEnv = { CODEX_HOME: `${root}/unused-codex` };

export default async function () {
  aaa("project", "create", "demo");
  const threads = [];
  for (const name of ["work", "spare"]) {
    const auth = `${root}/${name}-auth.json`;
    fs.writeFileSync(auth, JSON.stringify({ tokens: { access_token: `${name}-fixture-access-0123456789`, refresh_token: `${name}-fixture-refresh-0123456789` } }));
    aaa("account", "import", name, "--harness", "codex", "--file", auth);
    const thread = aaa("thread", "start", "demo", "--machine", "labA", "--profile", "fake", "--account", name, "--kind", "dir", "--title", `${name} 기록`, "--task-file", `${root}/task.md`);
    await waitOutput(thread.pane, "CHAT-READY");
    threads.push(thread);
  }
  let safari;
  try {
    const check = async (engine, width, index) => {
        const thread = threads[index];
        const { page, errors } = await pairPage(engine, aaa("web", "pair").code, { width, height: 844 });
        try {
          await inList(page, `thread-demo-${thread.id}`);
          const toggle = page.getByTestId("chat-toggle");
          if (await toggle.getAttribute("aria-pressed") !== "true") await toggle.click();
          const chat = page.getByTestId("chat");
          await chat.getByText("Codex 기록 파일을 찾지 못했다", { exact: true }).waitFor();
          assert.match(await chat.locator(".chat-note").first().innerText(), /^Codex가 남긴/);
          await shot(page, `77-account-chat-${width}-missing`);
          aaa("pane", "send-keys", thread.pane, "F6");
          await chat.getByText(`${thread.id} 기록 300`, { exact: true }).waitFor();
          await domIs(page, "오류를 지움", () => !document.querySelector('[data-testid="chat"] .banner.warn'));
          // 버튼을 향한 자동 스크롤도 앞 쪽을 읽는다. 그 순간 사라진 버튼에 click을 보내지 않고 실제 scroll 길을 쓴다.
          const earlier = () => chat.evaluate((el) => { el.scrollTop = 0; el.dispatchEvent(new Event("scroll", { bubbles: true })); });
          await earlier();
          await domIs(page, "앞 쪽 commit", () => document.querySelectorAll('[data-testid="chat"] [data-chat-id]').length >= 300);
          await earlier();
          await chat.getByText("처음이다", { exact: true }).waitFor();
          await chat.evaluate((el) => { el.scrollTop = 0; });
          await chat.getByText(`${thread.id} 기록 0`, { exact: true }).waitFor();
          aaa("pane", "send-keys", thread.pane, "F6");
          await chat.evaluate((el) => { el.scrollTop = el.scrollHeight; });
          await chat.getByText(`${thread.id} 기록 301`, { exact: true }).waitFor();
          const other = threads[1 - index].id;
          assert.ok(!(await chat.innerText()).includes(`${other} 기록`));
          assert.ok(!fs.existsSync(`${root}/unused-codex/sessions`), "공유 홈에는 기록이 없다");
          await shot(page, `77-account-chat-${width}-loaded`);
          assert.deepEqual(errors, []);
        } finally { await page.context().close(); }
    };
  await step("계정 Codex 대화: Chromium 1280 오류 머리말·기록 생성·앞 쪽·새 글", async () => check(browser, 1280, 0));
  await step("계정 Codex 대화: WebKit 390 오류 머리말·기록 생성·앞 쪽·새 글", async () => check(safari = await webkit.launch({ headless: true }), 390, 1));
  } finally { await safari?.close(); }
}
