// 작업 중인 agent에게 보낸 말이 Enter 직후(host 답 전) 대화 끝에 뜨고, 같은 말풍선이 보내는 중 → 대기 중 → 들어감으로 바뀌는지,
// 새로고침 뒤에도 남는지 본다(t-1069). 실제 host·agent·PTY와 Claude 감지기의 가짜 harness를 쓴다.
// 완료 사건은 PTY 수신·submit ACK·대화 DOM이며 정한 시간을 자지 않는다. 첫 말은 ACK를 붙잡아 「ACK 전 표시」를 결정적으로 보고,
// 둘째 말은 붙잡지 않고 Enter부터 각 상태까지의 실제 ms를 적는다(보고의 전후 측정표).
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import { aaa, step, browser, webkit, pairPage, domIs, root, AA, waitOutput, shot, shots, sentFrames, paneHash, inTime } from "../lib/harness.mjs";
import { ChatDiagnostics } from "../lib/chatDiagnostics.mjs";

const line = value => JSON.stringify(value) + '\n';
// 대화 끝의 사람 말 상태(없음·단계·들어감)와 같은 글 말풍선 수를 바뀔 때마다 적는다
const watchBubble = () => {
  window.sendTimeline = [];
  const state = () => {
    const marker = window.sendMarker;
    if (!marker) return null;
    const turns = [...document.querySelectorAll('[data-testid="chat"] .chat-turn-user')].filter(t => t.textContent.includes(marker));
    const turn = turns.at(-1);
    if (!turn) return { state: 'none', count: 0 };
    const tag = turn.querySelector('[data-testid="chat-pending"]');
    return { state: tag ? tag.dataset.stage : 'solid', text: tag?.querySelector('span')?.textContent ?? null, count: turns.length };
  };
  let last = null;
  const check = () => {
    const now = state();
    const key = JSON.stringify(now);
    if (now !== null && key !== last) { last = key; window.sendTimeline.push({ at: Date.now(), ...now }); }
  };
  new MutationObserver(check).observe(document, { subtree: true, childList: true, characterData: true, attributes: true });
  // 보내기 시작: 데스크톱 Enter, 휴대폰은 보내기 단추(터치 Enter는 줄바꿈)
  document.addEventListener('keydown', event => {
    if (event.key === 'Enter' && event.target.getAttribute?.('aria-label') === '메시지') window.sendTimeline.push({ at: Date.now(), state: 'enter' });
  }, true);
  document.addEventListener('click', event => {
    if (event.target.closest?.('[aria-label="메시지 보내기"]')) window.sendTimeline.push({ at: Date.now(), state: 'enter' });
  }, true);
};
const turnIs = (p, label, marker, stage) => domIs(p, label, ([m, want]) => {
  const turns = [...document.querySelectorAll('[data-testid="chat"] .chat-turn-user')].filter(t => t.textContent.includes(m));
  if (turns.length !== 1) return false;
  const tag = turns[0].querySelector('[data-testid="chat-pending"]');
  return want === 'solid' ? !tag && !turns[0].querySelector('.is-pending') : tag?.dataset.stage === want && turns[0].querySelector('.chat-msg.user.is-pending');
}, [marker, stage]);

async function check(engineName, engine, width, program, timings) {
  const name = `send-${engineName}-${width}`;
  const diagnostics = new ChatDiagnostics(root, name);
  const dir = `${root}/claude/projects/-${name}`;
  fs.mkdirSync(dir, { recursive: true });
  const transcript = `${dir}/s.jsonl`, receipts = `${dir}/received`;
  fs.writeFileSync(transcript, line({ type: 'user', message: { content: '앞 질문' } }) + line({ type: 'assistant', message: { content: [{ type: 'text', text: '앞 답' }] } }));
  aaa('tab', 'create', '--machine', 'labA', '--label', name, '--', program, transcript, receipts, AA, crypto.randomUUID());
  const pane = name;
  await waitOutput(pane, 'SEND_READY');
  let hold = null;
  const route = socket => {
    const server = socket.connectToServer();
    socket.onMessage(raw => server.send(raw));
    server.onMessage(raw => {
      let message;
      try { message = JSON.parse(String(raw)); } catch { /* binary output */ }
      if (message?.t === 'submitted' && hold) hold.resolve(() => socket.send(raw));
      else socket.send(raw);
    });
  };
  const capture = shots && width === 390;
  let videoAt = null;
  const recordingEngine = capture ? { newContext: options => engine.newContext({ ...options, recordVideo: { dir: `${shots}/video`, size: { width: 390, height: 844 } } }) } : engine;
  const dev = await pairPage(recordingEngine, aaa('web', 'pair').code, { width, height: 844 }, { hash: paneHash(pane), touch: width === 390, wsRoute: [/\/api\/v1\/panes\/[^/]+\/ws/, route],
    setup: context => { diagnostics.watch(context); videoAt = Date.now(); }, init: watchBubble });
  const p = dev.page;
  diagnostics.page = p;
  diagnostics.errors = dev.errors;
  const box = p.getByRole('textbox', { name: '메시지', exact: true });
  const openChat = async () => {
    // 새 문서는 대화·터미널 보기를 정하기 전 화면부터 그린다: 단추 이름으로 지금 보기를 확인한다
    const toggle = p.getByTestId('chat-toggle');
    await toggle.waitFor();
    if (await toggle.getAttribute('aria-pressed') !== 'true') await toggle.click();
    await box.waitFor();
    await domIs(p, '앞 기록', () => document.querySelector('[data-testid="chat"]')?.textContent.includes('앞 답'));
    await p.getByTestId('chat-interrupt').waitFor(); // 실제 감지된 working 상태에서 보낸다
  };
  const send = async (marker) => {
    await p.evaluate(m => { window.sendMarker = m; window.sendTimeline = []; }, marker);
    await box.fill(marker);
    if (width === 390) await p.getByRole('button', { name: '메시지 보내기', exact: true }).click();
    else await box.press('Enter');
  };
  try {
    await openChat();
    // 1. ACK 전: host 답을 붙잡아도 Enter 직후 「보내는 중」 말풍선이 대화 끝에 있고 입력창은 비어 다음 글을 쓴다
    const first = '작업 중 추가 지시 1';
    let release;
    hold = {};
    const held = new Promise(resolve => { hold.resolve = resolve; });
    await send(first);
    await turnIs(p, 'ACK 전 보내는 중', first, 'sending');
    assert.equal(await box.inputValue(), '');
    await waitOutput(pane, 'SEND_RECEIVED_1');
    let deadline;
    try {
      release = await Promise.race([held, new Promise((_, reject) => { deadline = setTimeout(() => reject(new Error('submit ACK가 오지 않았다')), 20_000); })]);
    } finally { clearTimeout(deadline); }
    hold = null;
    await turnIs(p, 'ACK를 붙잡는 동안 보내는 중', first, 'sending');
    await shot(p, `${name}-sending`);
    release();
    // 2. ACK 뒤 작업 중: 같은 말풍선이 대기 중
    await turnIs(p, 'ACK 뒤 대기 중', first, 'sent');
    assert.match(await p.getByTestId('chat-pending').first().textContent(), /대기 중/);
    await shot(p, `${name}-queued`);
    // 3. 새로고침해도 기록에 없는 대기 말이 남는다(같은 탭)
    // 새 문서의 첫 화면을 이 pane으로 연다(처음 연 주소와 같다)
    await p.evaluate(h => history.replaceState(history.state, '', h), paneHash(pane));
    await p.reload();
    await openChat();
    await turnIs(p, '새로고침 뒤 대기 말', first, 'sent');
    fs.appendFileSync(transcript, line({ type: 'user', message: { content: first } }));
    await turnIs(p, '기록에 오르면 보통 말', first, 'solid');
    // 4. 붙잡지 않은 실제 흐름(Claude 대기열 기록): 보내는 중 → 대기 중 → 기록 대기열 → 들어감. 같은 글 말풍선은 늘 하나
    const second = '작업 중 추가 지시 2';
    fs.writeFileSync(`${transcript}.queue`, '');
    await send(second);
    await waitOutput(pane, 'SEND_RECEIVED_2');
    await turnIs(p, '기록 대기열 말', second, 'queued');
    fs.unlinkSync(`${transcript}.queue`);
    fs.appendFileSync(transcript, line({ type: 'queue-operation', operation: 'dequeue' }) + line({ type: 'user', message: { content: second } }));
    await turnIs(p, '흡수 뒤 보통 말', second, 'solid');
    const timeline = await p.evaluate(() => window.sendTimeline);
    const enter = timeline.find(e => e.state === 'enter').at;
    assert.ok(timeline.every(e => e.state === 'enter' || e.count <= 1), `같은 글 말풍선이 겹쳤다: ${JSON.stringify(timeline)}`);
    assert.ok(timeline.some(e => e.state === 'sending'), '보내는 중 단계가 보이지 않았다');
    const received = fs.readFileSync(receipts, 'utf8').trim().split('\n').map(JSON.parse).find(r => r.text === second).at;
    timings.push({ name, ptyMs: received - enter, videoMs: capture ? enter - videoAt : null,
      timeline: timeline.filter(e => e.state !== 'enter').map(e => ({ ms: e.at - enter, state: e.state })) });
    if (dev.errors.length) throw new Error(`페이지 오류: ${dev.errors.join(' / ')}`);
  } catch (error) {
    diagnostics.remember([{ at: Date.now(), receipts: fs.existsSync(receipts) ? fs.readFileSync(receipts, 'utf8') : null, sent: await sentFrames(p) }]);
    await diagnostics.save(error, inTime);
    throw error;
  } finally {
    diagnostics.stop();
    await p.context().close();
    if (capture) await p.video().saveAs(`${shots}/${name}.webm`);
    aaa('pane', 'kill', pane); // 이 조합에서 직접 만든 pane만 닫는다
  }
}

export const labEnv = { AAA_TEST_DETECT_GRACE_MS: "0" };

export default async function () {
  let program;
  const timings = [];
  await step("전송 상태 실험실 준비", async () => {
    fs.mkdirSync(`${root}/send-harness`, { recursive: true });
    // 이름이 claude라 화면 감지가 Claude 규칙으로 본다. `.queue` 표지가 있으면 Claude처럼 작업 중 받은 글을 기록 대기열에 적는다
    program = `${root}/send-harness/claude`;
    fs.writeFileSync(program, `#!/usr/bin/env python3
import os, sys, json, subprocess, tty, time
transcript, receipts, aa, session = sys.argv[1:]
subprocess.run([aa,'hook','--agent','claude',json.dumps({'hook_event_name':'SessionStart','session_id':session,'source':'startup','transcript_path':transcript})],check=True,stdout=subprocess.DEVNULL)
tty.setraw(0)
text=b''; count=0
print('SEND_READY\\r\\n✻ Working… (1s · esc to interrupt)',flush=True)
while True:
    ch=os.read(0,1)
    if not ch: break
    if ch==b'\\r':
        message=text.decode(); text=b''; count+=1
        with open(receipts,'a') as f: f.write(json.dumps({'at':int(time.time()*1000),'text':message},ensure_ascii=False)+'\\n')
        print('\\r\\nSEND_RECEIVED_'+str(count)+'\\r\\n✻ Working… (1s · esc to interrupt)',flush=True)
        if os.path.exists(transcript+'.queue'):
            with open(transcript,'a') as f: f.write(json.dumps({'type':'queue-operation','operation':'enqueue','content':message},ensure_ascii=False)+'\\n')
    else: text+=ch
`, { mode: 0o755 });
  }, { always: true });
  const wk = await webkit.launch();
  try {
  await step("즉시 표시·전송 상태·새로고침: chromium 1280", () => check("chromium", browser, 1280, program, timings));
  await step("즉시 표시·전송 상태·새로고침: chromium 390", () => check("chromium", browser, 390, program, timings));
  await step("즉시 표시·전송 상태·새로고침: webkit 1280", () => check("webkit", wk, 1280, program, timings));
  await step("즉시 표시·전송 상태·새로고침: webkit 390", () => check("webkit", wk, 390, program, timings));
  } finally { await wk.close(); }
  console.log(`SEND_TIMINGS ${JSON.stringify(timings)}`);
  if (shots) fs.writeFileSync(`${shots}/send-timings.json`, JSON.stringify(timings, null, 2));
}
