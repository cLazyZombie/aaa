// 실제 host 주입(pane run → send_prompt)·PTY에서 알림 턴의 기록 반영을 보류한다.
// 완료 사건은 PTY 수신·submit ACK·대화 DOM이다. 모델 응답 시간을 sleep으로 흉내 내지 않는다.
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import { aaa, step, browser, webkit, pairPage, domIs, root, AA, waitOutput, shot, shots, sentFrames, paneHash, inTime } from "../lib/harness.mjs";
import { ChatDiagnostics } from "../lib/chatDiagnostics.mjs";

// coordinator도 실제 Claude 감지기를 쓰게 한다. 이 spec만의 격리 host 설정이다.
const config = `${root}/c/host.toml`;
fs.writeFileSync(config, fs.readFileSync(config, 'utf8').replace('[profiles.fake]\nagent = "custom"', '[profiles.fake]\nagent = "claude"'));

const line = text => JSON.stringify({ type: 'user', message: { content: text } }) + '\n';
async function check(engineName, engine, width, role, program) {
  const name = `inbox-${engineName}-${width}-${role}`;
  const diagnostics = new ChatDiagnostics(root, name);
  const dir = `${root}/claude/projects/-${name}`;
  fs.mkdirSync(dir, { recursive: true });
  const transcript = `${dir}/s.jsonl`, receipts = `${dir}/received`;
  fs.writeFileSync(transcript, line('앞 질문'));
  const script = `${dir}/run.sh`;
  const session = crypto.randomUUID();
  fs.writeFileSync(script, `exec '${program}' '${transcript}' '${receipts}' '${AA}' '${session}'\n`);
  let pane;
  if (role === 'coordinator') {
    pane = aaa('project', 'open', 'inbox-demo', '--profile', 'fake').pane.id;
    await waitOutput(pane, 'fake>');
    aaa('pane', 'run', pane, `post-probe ${script}`);
  } else {
    aaa('tab', 'create', '--machine', 'labA', '--label', name, '--', program, transcript, receipts, AA, session);
    pane = name;
  }
  await waitOutput(pane, 'INBOX_READY');
  let releaseAck;
  let heldAck;
  const ackReady = new Promise(resolve => { heldAck = resolve; });
  let firstAck = true;
  const route = socket => {
    const server = socket.connectToServer();
    socket.onMessage(raw => server.send(raw));
    server.onMessage(raw => {
      let message;
      try { message = JSON.parse(String(raw)); } catch { /* binary output */ }
      if (message) diagnostics.remember([{ at: Date.now(), type: message.t }]);
      if (firstAck && message?.t === 'submitted') {
        firstAck = false;
        releaseAck = () => socket.send(raw);
        heldAck();
      } else socket.send(raw);
    });
  };
  const capture = shots && width === 390 && role === 'coordinator';
  const recordingEngine = capture ? { newContext: options => engine.newContext({ ...options, recordVideo: { dir: `${shots}/video`, size: { width: 390, height: 844 } } }) } : engine;
  const dev = await pairPage(recordingEngine, aaa('web', 'pair').code, { width, height: 844 }, { hash: role === 'thread' ? paneHash(pane) : `#pane=${encodeURIComponent(pane)}`, touch: width === 390, wsRoute: [/\/api\/v1\/panes\/[^/]+\/ws/, route],
    setup: context => diagnostics.watch(context),
    init: () => {
      const states = [];
      window.takeChatStates = () => states.splice(0);
      const remember = state => {
        states.push({ at: Date.now(), ...state });
        if (states.length > 256) states.shift();
      };
      window.inboxSelectionInterleavings = 0;
      // selection commit을 input bubble 앞에 끼워 WebKit의 초안 유실을 재현한다.
      // 새 글보다 앞선 선택 갱신으로 commit하지 않고 onChange가 초안을 받아야 한다.
      document.addEventListener('DOMContentLoaded', () => {
        document.getElementById('root').addEventListener('input', event => {
          if (event.target.getAttribute('aria-label') !== '메시지' || event.target.value !== '아직 보내지 않은 초안') return;
          window.inboxSelectionInterleavings++;
          event.target.dispatchEvent(new MouseEvent('mouseup', { bubbles: true }));
          remember({ event: 'selection commit', value: event.target.value });
        }, true);
      });
      for (const type of ['input', 'pointerdown', 'pointerup', 'click', 'focusin', 'focusout']) {
        document.addEventListener(type, event => {
          if (!event.target.closest?.('[data-testid="chat-composer"]')) return;
          remember({ event: type, tag: event.target.tagName,
            label: event.target.getAttribute('aria-label'), value: event.target.value,
            disabled: event.target.disabled });
        }, true);
      }
    },
  });
  const p = dev.page;
  diagnostics.page = p;
  diagnostics.errors = dev.errors;
  try {
    if (!await p.getByTestId('chat-composer').count()) await p.getByTestId('chat-toggle').click();
    const box = p.getByRole('textbox', { name: '메시지', exact: true });
    await box.waitFor();
    aaa('pane', 'run', pane, '[aaa inbox] t-0001 completed (aaa inbox list demo)');
    await waitOutput(pane, 'INBOX_RECEIVED_1');
    await domIs(p, '주입 알림 기록', () => document.querySelector('[data-testid="chat"]')?.textContent.includes('[aaa inbox]'));
    assert.equal(await p.getByTestId('chat-inbox-label').textContent(), 'aaa inbox 안내');
    await p.getByTestId('chat-interrupt').waitFor(); // 실제 감지된 working 상태에서 보낸다.
    await box.fill('첫 사람 메시지');
    await p.getByRole('button', { name: '메시지 보내기', exact: true }).click();
    await waitOutput(pane, 'INBOX_RECEIVED_2');
    let deadline;
    try {
      await Promise.race([ackReady, new Promise((_, reject) => { deadline = setTimeout(() => reject(new Error('submit ACK가 오지 않았다')), 20_000); })]);
    } finally { clearTimeout(deadline); }
    // ACK가 브라우저에 도착하기 전에도 다음 초안을 실제로 쓴다.

    await shot(p, `${name}-received`);
    assert.equal(await box.evaluate(el => el.readOnly), false, '주입 턴의 수신 확인이 입력창을 잠그면 안 된다');
    await box.fill('두 번째 사람 메시지');
    releaseAck();
    await domIs(p, '첫 메시지는 기록을 기다리는 대기 말', () => document.querySelector('.chat-msg.user.is-pending')?.textContent.includes('첫 사람 메시지'));
    await box.fill('두 번째 사람 메시지');
    await p.getByRole('button', { name: '메시지 보내기', exact: true }).click();
    await waitOutput(pane, 'INBOX_RECEIVED_3');
    await domIs(p, '두 메시지가 순서대로 대기', () => document.querySelectorAll('.chat-msg.user.is-pending').length === 2);
    await box.fill('아직 보내지 않은 초안');
    await shot(p, `${name}-draft`);
    assert.deepEqual(fs.readFileSync(receipts, 'utf8').trim().split('\n').map(JSON.parse).slice(1), ['첫 사람 메시지', '두 번째 사람 메시지']);
    assert.equal((await sentFrames(p)).filter(f => f.t === 'submit').length, 2);
    fs.appendFileSync(transcript, line('첫 사람 메시지') + line('두 번째 사람 메시지'));
    await domIs(p, '늦은 기록은 대기 말만 정리', () => !document.querySelector('.chat-msg.user.is-pending'));
    assert.equal(await box.inputValue(), '아직 보내지 않은 초안');
    assert.equal(await p.evaluate(() => window.inboxSelectionInterleavings), 1, 'input 중 selection commit도 검사했다');
    assert.equal(await box.evaluate(el => document.activeElement === el), true);
    // 같은 글 두 번도 서로 다른 전송이다. 첫 수신은 첫 대기 말만 정리한다.
    for (let n = 0; n < 2; n++) {
      await box.fill('같은 추가 지시');
      await p.getByRole('button', { name: '메시지 보내기', exact: true }).click();
      await waitOutput(pane, `INBOX_RECEIVED_${n + 4}`);
      await domIs(p, '반복 전송 대기', n => document.querySelectorAll('.chat-msg.user.is-pending').length === n, n + 1);
    }
    fs.appendFileSync(transcript, line('같은 추가 지시'));
    await domIs(p, '첫 수신은 하나만 정리', () => document.querySelectorAll('.chat-msg.user.is-pending').length === 1);
    fs.appendFileSync(transcript, line('같은 추가 지시'));
    await domIs(p, '두 번째 수신 뒤 정리', () => !document.querySelector('.chat-msg.user.is-pending'));
    assert.deepEqual(dev.errors, []);
  } catch (error) {
    diagnostics.remember([{ at: Date.now(), receipts: fs.existsSync(receipts) ? fs.readFileSync(receipts, 'utf8') : null,
      sent: await sentFrames(p) }]);
    await diagnostics.save(error, inTime);
    throw error;
  } finally {
    diagnostics.stop();
    await p.context().close();
    if (capture) await p.video().saveAs(`${shots}/${name}.webm`);
    aaa('pane', 'kill', pane); // 이 조합에서 직접 만든 pane만 닫는다.
  }
}

export const labEnv = { AAA_TEST_DETECT_GRACE_MS: "0" };

export default async function () {
  let program;
  await step("inbox 연속 입력 실험실 준비", async () => {
    fs.mkdirSync(`${root}/inbox-harness`, { recursive: true });
    program = `${root}/inbox-harness/claude`;
    fs.writeFileSync(program, `#!/usr/bin/env python3
import os, sys, json, subprocess, tty
transcript, receipts, aa, session = sys.argv[1:]
subprocess.run([aa,'hook','--agent','claude',json.dumps({'hook_event_name':'SessionStart','session_id':session,'source':'startup','transcript_path':transcript})],check=True,stdout=subprocess.DEVNULL)
tty.setraw(0)
text=b''; count=0
print('INBOX_READY',flush=True)
while True:
    ch=os.read(0,1)
    if not ch: break
    if ch==b'\\r':
        message=text.decode(); text=b''; count+=1
        with open(receipts,'a') as f: f.write(json.dumps(message,ensure_ascii=False)+'\\n')
        if message.startswith('[aaa inbox]'):
            with open(transcript,'a') as f: f.write(json.dumps({'type':'user','message':{'content':message}},ensure_ascii=False)+'\\n')
            subprocess.run([aa,'hook','--agent','claude',json.dumps({'hook_event_name':'UserPromptSubmit','session_id':session})],check=True,stdout=subprocess.DEVNULL)
        print('\\r\\nINBOX_RECEIVED_'+str(count)+'\\r\\n✻ Working… (1s · esc to interrupt)',flush=True)
    else: text+=ch
`, { mode: 0o755 });
    aaa('project', 'create', 'inbox-demo');
  }, { always: true });
  const wk = await webkit.launch();
  try {
  await step("inbox 연속 입력: chromium 1280 coordinator", () => check("chromium", browser, 1280, "coordinator", program));
  await step("inbox 연속 입력: chromium 1280 thread", () => check("chromium", browser, 1280, "thread", program));
  await step("inbox 연속 입력: chromium 390 coordinator", () => check("chromium", browser, 390, "coordinator", program));
  await step("inbox 연속 입력: chromium 390 thread", () => check("chromium", browser, 390, "thread", program));
  await step("inbox 연속 입력: webkit 1280 coordinator", () => check("webkit", wk, 1280, "coordinator", program));
  await step("inbox 연속 입력: webkit 1280 thread", () => check("webkit", wk, 1280, "thread", program));
  await step("inbox 연속 입력: webkit 390 coordinator", () => check("webkit", wk, 390, "coordinator", program));
  await step("inbox 연속 입력: webkit 390 thread", () => check("webkit", wk, 390, "thread", program));
  } finally { await wk.close(); }
}
