// 다른 pane의 실제 inbox 알림이 현재 입력과 바뀌지 않은 Markdown 본문을 방해하지 않는다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { execFile } from "node:child_process";
import { DatabaseSync } from "node:sqlite";
import { root, AA, env, aaa, until, step, browser, webkit, pairPage, waitOutput, domIs } from "../lib/harness.mjs";

const config = `${root}/c/host.toml`;
fs.writeFileSync(config, fs.readFileSync(config, "utf8")
  .replace("[projects]\n", "[projects]\nnudge_idle_secs = 0\nnudge_quiet_secs = 0\n")
  .replace('[profiles.fake]\nagent = "custom"', '[profiles.fake]\nagent = "claude"'));
const records = `${root}/claude/projects/-inbox-typing`;
fs.mkdirSync(records, { recursive: true });
const body = "[기록](https://example.com/inbox-typing/record)과 `context.txt`를 확인한다. **같은 본문**을 유지한다.";
const line = (role, text) => JSON.stringify({ type: role, message: { content: role === "assistant" ? [{ type: "text", text }] : text } }) + "\n";
for (const [name, count] of [["main", 20], ["typing", 8]]) {
  fs.writeFileSync(`${records}/${name}.jsonl`, line("user", "앞선 요청") + Array.from({ length: count }, (_, i) => line("assistant", `기록 ${i}: ${body}`)).join(""));
}
const control = `${root}/inbox-control`;
const receipts = `${root}/inbox-received.jsonl`;
fs.writeFileSync(receipts, "");
fs.writeFileSync(`${root}/inbox.py`, `import os,sys,json,select,subprocess
buf=b''; count=0
os.mkfifo(sys.argv[4]); ctl=os.open(sys.argv[4],os.O_RDWR|os.O_NONBLOCK)
subprocess.run([sys.argv[1],'hook','--agent','claude',json.dumps({'hook_event_name':'SessionStart','session_id':'6a1f0c2e-3b4d-4e5f-8a9b-0c1d2e3f4a99','source':'startup','transcript_path':sys.argv[2]})],check=True,stdout=subprocess.DEVNULL)
print('\\x1b[?2004hinbox-ready\\r\\n❯ ',end='',flush=True)
while True:
    ready,_,_=select.select([0,ctl],[],[])
    if ctl in ready:
        os.read(ctl,4096)
        print('\\x1b[2J\\x1b[HINBOX_IDLE_'+str(count)+'\\r\\n────────────────────────\\r\\n❯ \\r\\n────────────────────────\\r\\n  ? for shortcuts',end='',flush=True)
    if 0 not in ready: continue
    data=os.read(0,4096)
    if not data: break
    os.write(1,data); buf+=data
    while b'\\r' in buf:
        message,buf=buf.split(b'\\r',1); count+=1
        text=message.replace(b'\\x1b[200~',b'').replace(b'\\x1b[201~',b'').decode()
        with open(sys.argv[3],'a') as f:f.write(json.dumps(text)+'\\n')
        with open(sys.argv[2],'a') as f:f.write(json.dumps({'type':'user','message':{'content':text}})+'\\n')
        print('\\x1b[2J\\x1b[HINBOX_RECEIVED_'+str(count)+'\\r\\n✻ Working… (1s · esc to interrupt)\\r\\n\\r\\n────────────────────────\\r\\n❯ \\r\\n────────────────────────\\r\\n  ? for shortcuts\\r\\n',end='',flush=True)
`);
const quote = s => `'${s.replaceAll("'", "'\\''")}'`;
fs.writeFileSync(`${root}/fake.sh`, `#!/bin/sh
if [ "$1" = "--version" ]; then printf 'Claude Code test\\n'; exit 0; fi
stty raw -echo
exec python3 ${[`${root}/inbox.py`, AA, `${records}/main.jsonl`, receipts, control].map(quote).join(" ")}
`);
const tick = () => new Promise((resolve, reject) => execFile(AA, ["--json", "ticker", "tick"], { env }, error => error ? reject(error) : resolve()));
let main, typing, sequence = 0;
const read = () => fs.readFileSync(receipts, "utf8").split("\n").filter(Boolean).map(s => JSON.parse(s));
function seed() {
  const id = `typing-${++sequence}`;
  const db = new DatabaseSync(`${root}/s/host/host.db`);
  try {
    const item = { id, project: "demo", item_kind: "thread-state", subject: "t-0001", event: "new report", summary: "새 보고", body: "", done: false, seen: false, created_at_ms: Date.now(), done_at_ms: null };
    db.prepare("INSERT INTO inbox(id,project,kind,subject,event,done,created_at,data) VALUES(?,'demo','thread-state','t-0001','new report',0,?,?)").run(id, item.created_at_ms, JSON.stringify(item));
  } finally { db.close(); }
}
async function openChat(p, id, count) {
  await p.evaluate(id => { location.hash = `pane=${id}`; }, id);
  await p.getByTestId("chat-toggle").waitFor();
  if (!await p.getByTestId("chat").count()) await p.getByTestId("chat-toggle").click();
  await domIs(p, "대화 본문 준비", n => document.querySelectorAll('[data-testid="chat"] [data-role="assistant"]').length >= n, count);
}
async function scenario(engine, width) {
  const client = await pairPage(engine, aaa("web", "pair").code, { width, height: 844 }, {
    serviceWorkers: "block",
    init: () => {
      // 고정 본문의 링크를 다시 파싱하면 실제 URL 생성이 발생한다. 시간 임계값 대신 불필요한 본문 처리 자체를 잡는다.
      window.__inboxLinkChecks = 0;
      const NativeURL = window.URL;
      window.URL = class extends NativeURL {
        constructor(input, base) {
          super(input, base);
          if (String(input).startsWith("https://example.com/inbox-typing/")) window.__inboxLinkChecks++;
        }
      };
    },
  });
  const p = client.page;
  try {
    await openChat(p, main, 20);
    await openChat(p, typing, 8);
    const box = p.getByRole("textbox", { name: "메시지", exact: true });
    await box.fill("앞뒤");
    await box.evaluate(el => el.setSelectionRange(1, 1));
    await p.keyboard.insertText("가");
    await box.dispatchEvent("compositionstart", { data: "" });
    assert.ok(await p.evaluate(() => window.__inboxLinkChecks) > 0, "본문 처리 탐침이 실제로 실행됐다");
    await p.evaluate(() => { window.__inboxLinkChecks = 0; });
    const before = read().length;
    seed();
    const ticking = tick().then(tick);
    await waitOutput(main, `INBOX_RECEIVED_${before + 1}`);
    await ticking;
    // 실제 상태가 DOM에 반영된 뒤 검사한다. 알림은 초안·IME 때문에 미뤄지지 않았다.
    await p.locator('.is-coordinator [data-status="working"]').first().waitFor({ state: "attached" });
    assert.match(read()[before], /^\[aaa inbox\] /);
    assert.equal(read().length, before + 1);
    assert.equal(await box.inputValue(), "앞가뒤");
    assert.equal(await box.evaluate(el => document.activeElement === el && !el.readOnly && !el.disabled), true);
    await p.keyboard.insertText("나");
    assert.equal(await box.inputValue(), "앞가나뒤", "다른 pane의 알림 중에도 커서와 새 글을 지킨다");
    assert.equal(await p.evaluate(() => window.__inboxLinkChecks), 0, "다른 pane의 알림으로 바뀌지 않은 본문을 재처리하지 않는다");
    await box.dispatchEvent("compositionend", { data: "" });
    // 참조를 유지한 열기 함수는 현재 pane의 파일을 계속 연다.
    await p.getByTestId("chat").getByRole("button", { name: "context.txt", exact: true }).last().click();
    await domIs(p, "현재 pane의 파일", () => document.querySelector('[data-testid="file-viewer"]')?.textContent.includes("현재 입력 pane"));
    await p.getByRole("button", { name: "파일 닫기", exact: true }).click();
    await p.getByTestId("file-viewer").waitFor({ state: "detached" });
    assert.equal(await box.inputValue(), "앞가나뒤");
    assert.deepEqual(client.errors, []);
  } finally {
    await p.context().close();
    fs.writeFileSync(control, "idle\n");
    await until("coordinator 입력 대기", () => aaa("pane", "list").panes.find(p => p.id === main)?.agent?.activity === "idle");
  }
}
export default async function () {
  await step("다른 pane inbox 입력 실험실 준비", async () => {
    aaa("project", "create", "demo");
    main = aaa("project", "open", "demo", "--profile", "fake").pane.id;
    await waitOutput(main, "inbox-ready");
    const cwd = `${root}/projects/demo/typing`;
    fs.mkdirSync(cwd, { recursive: true }); fs.writeFileSync(`${cwd}/context.txt`, "현재 입력 pane\n");
    const hook = { hook_event_name: "SessionStart", session_id: "0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b", source: "startup", transcript_path: `${records}/typing.jsonl` };
    aaa("tab", "create", "--machine", "labA", "--label", "inbox-typing", "--cwd", cwd, "--", "/bin/sh", "-c", `${quote(AA)} hook --agent claude ${quote(JSON.stringify(hook))}; printf 'typing-ready\\n❯ '; exec cat`);
    typing = aaa("pane", "list").panes.find(p => p.name === "inbox-typing").id;
    await waitOutput(typing, "typing-ready");
  }, { always: true });
  await step("다른 pane inbox 타이핑: Chromium 데스크톱", () => scenario(browser, 1280));
  await step("다른 pane inbox 타이핑: Chromium 휴대폰", () => scenario(browser, 390));
  const safari = await webkit.launch({ headless: true });
  try {
    await step("다른 pane inbox 타이핑: WebKit 데스크톱", () => scenario(safari, 1280));
    await step("다른 pane inbox 타이핑: WebKit 휴대폰", () => scenario(safari, 390));
  } finally { await safari.close(); }
}
