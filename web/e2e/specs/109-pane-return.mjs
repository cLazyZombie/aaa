// 보관 복귀는 같은 연결·xterm, 만료 복귀는 새 snapshot으로 검사한다. 고정 sleep은 없다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { aaa, step, browser, webkit, pairPage, until, inList, shot, root, sentCount, sentFrames, waitOutput, inTime } from "../lib/harness.mjs";
import { prepare, go, frames } from "../lib/chat-retention.mjs";
import { returnDiagnostics } from "../lib/return-diagnostics.mjs";
import { scrollHistoryWhenReady } from "../lib/history-scroll.mjs";

function instrument() {
  const media = window.matchMedia.bind(window);
  window.matchMedia = query => {
    const value = media(query);
    if (query === '(display-mode: standalone)') Object.defineProperty(value, 'matches', { value: true });
    return value;
  };
  const Native = window.WebSocket;
  window.returnFrames = [];
  window.returnConnections = {};
  window.returnSnapshots = {};
  window.returnScrolls = [];
  window.returnHistoryRequests = [];
  document.addEventListener('scroll', event => {
    const viewport = event.target;
    if (!viewport.matches?.('.xterm-viewport')) return;
    const box = viewport.closest('.term-box');
    window.returnScrolls.push({ at: performance.now(), top: viewport.scrollTop, height: viewport.scrollHeight,
      client: viewport.clientHeight, dataset: { ...box?.dataset } });
    if (window.returnScrolls.length > 100) window.returnScrolls.shift();
  }, true);
  window.holdReturn = false;
  window.WebSocket = class extends Native {
    constructor(url, protocols) {
      super(url, protocols);
      if (!String(url).includes('/panes/')) return;
      this.returnPane = decodeURIComponent(String(url).match(/panes\/([^/]+)\/ws/)[1]);
      window.returnConnections[this.returnPane] = (window.returnConnections[this.returnPane] ?? 0) + 1;
      this.addEventListener('message', event => { if (typeof event.data === 'string' && JSON.parse(event.data).t === 'snapshot') window.returnSnapshots[this.returnPane] = (window.returnSnapshots[this.returnPane] ?? 0) + 1; });
      const queued = [];
      this.addEventListener('message', event => {
        if (!window.holdReturn) return;
        event.stopImmediatePropagation();
        queued.push(new MessageEvent('message', { data: event.data }));
        window.returnHeld = queued.length;
      });
      window.releaseReturn = () => {
        window.holdReturn = false;
        for (const event of queued.splice(0)) this.dispatchEvent(event);
      };
    }
    send(raw) {
      if (this.returnPane && typeof raw === 'string' && JSON.parse(raw).t === 'history')
        window.returnHistoryRequests.push({ at: performance.now(), pane: this.returnPane, request: JSON.parse(raw) });
      super.send(raw);
    }
  };
  const sample = () => {
    if (!window.watchReturn) return;
    const term = document.querySelector('.term:not([hidden])');
    if (term?.dataset.pane !== window.watchReturn) return;
    window.returnFrames.push({
      at: performance.now(), held: window.returnHeld,
      banner: term.querySelector('[data-testid="readonly-banner"]')?.textContent ?? '',
      body: term.querySelector('.xterm-rows')?.textContent.includes('fake>') ?? false,
      readonly: term.querySelector('[data-testid="terminal"]')?.dataset.readonly,
    });
  };
  new MutationObserver(sample).observe(document, { childList: true, subtree: true, attributes: true, characterData: true });
  let frameId = null;
  window.watchReturns = id => {
    window.watchReturn = id;
    if (frameId !== null) cancelAnimationFrame(frameId);
    frameId = null;
    if (id) {
      const frame = () => { sample(); frameId = requestAnimationFrame(frame); };
      frameId = requestAnimationFrame(frame);
    }
  };
}

export default async function () {
  let fixture, safari;
  await step("화면 복귀: 실제 coordinator·긴 기록 준비", async () => {
    fixture = await prepare();
    fs.writeFileSync(`${root}/return-input.bytes`, '');
    fs.writeFileSync(`${root}/return-input.cjs`, `
const fs = require('node:fs');
process.stdin.setRawMode(true);
process.stdout.write('\\x1b[3J\\x1b[2J\\x1b[H\\x1b[?1000h\\x1b[?1006h' + Array.from({ length: 6000 }, (_, n) => 'return-history-' + n + '\\r\\n').join('') + 'coordinator 복귀 시험\\r\\nfake> ');
process.stdin.on('data', bytes => {
  fs.appendFileSync(${JSON.stringify(`${root}/return-input.bytes`)}, bytes);
  process.stdout.write('\\r\\n' + JSON.stringify(bytes.toString()) + '\\r\\nfake> ');
});
`);
    fs.writeFileSync(`${root}/return-screen.sh`, `${JSON.stringify(process.execPath)} ${JSON.stringify(`${root}/return-input.cjs`)}\n`);
    aaa('pane', 'run', fixture.coordinator, `post-probe ${root}/return-screen.sh`);
    await waitOutput(fixture.coordinator, 'coordinator 복귀 시험');
  }, { always: true });
  const check = async (engine, width) => {
    let phase = { stage: 'pairing', pane: fixture.coordinator };
    const dir = `${root}/diagnostics`;
    const diagnostic = returnDiagnostics(engine, dir, width, () => phase);
    let gateHistory = false, historyArrived, releaseHistory;
    const historyArrival = new Promise(resolve => { historyArrived = resolve; });
    const { page, errors } = await pairPage(engine, aaa('web', 'pair').code, { width, height: 900 }, {
      wsRoute: [new RegExp(`/api/v1/panes/${fixture.coordinator}/ws`), socket => {
        const server = socket.connectToServer();
        const queued = [];
        let held = false;
        socket.onMessage(raw => {
          if (gateHistory && typeof raw === 'string' && JSON.parse(raw).t === 'history') {
            held = true;
            gateHistory = false;
            historyArrived();
          }
          server.send(raw);
        });
        server.onMessage(raw => held ? queued.push(raw) : socket.send(raw));
        releaseHistory = () => { held = false; for (const raw of queued.splice(0)) socket.send(raw); };
      }],
      onPage: page => diagnostic.watch(page, "controller"),
      init: instrument, line: true, hash: `#pane=${fixture.coordinator}`,
      setup: context => context.addInitScript(keys => { for (const key of keys) localStorage.setItem(`aaa:view:${key}`, 'terminal'); }, fixture.keys),
    });
    const terminal = page.getByTestId('terminal');
    phase = { stage: 'initial', pane: fixture.coordinator };
    const receivedWheels = () => (fs.readFileSync(`${root}/return-input.bytes`, 'utf8').match(/\x1b\[<64;\d+;\d+M/g) ?? []).length;
    const wheel = () => terminal.evaluate(box => {
      const area = box.getBoundingClientRect();
      box.querySelector('.xterm').dispatchEvent(new WheelEvent('wheel', { bubbles: true, cancelable: true, deltaY: -100,
        clientX: area.left + area.width / 2, clientY: area.top + area.height / 2 }));
    });
    const wheelInputs = async from => (await sentFrames(page, from)).filter(f => f.t === 'input' && f.data.includes('\x1b[<64;'));
    const ready = async () => {
      await page.locator('[data-testid="terminal"][data-readonly="false"]').waitFor({ state: 'attached' });
      await until('coordinator 본문', () => terminal.locator('.xterm-rows').textContent().then(text => text.includes('fake>')));
    };
    const results = [];
    try {
      await ready();
      // 앞 기록 재구성이 마우스 추적뿐 아니라 SGR 좌표 인코딩도 보존해야 한다.
      // 실제 history 응답을 먼저 붙여야 다음 첫 휠이 재구성 뒤 상태를 결정적으로 검사한다.
      phase = { stage: 'initial-history-ready', pane: fixture.coordinator };
      const initialScroll = await page.waitForFunction(scrollHistoryWhenReady, '[data-testid="terminal"]');
      phase = { stage: 'initial-history', pane: fixture.coordinator, before: await initialScroll.jsonValue() };
      await initialScroll.dispose();
      await page.waitForFunction(() => Number(document.querySelector('[data-testid="terminal"]')?.dataset.historyApplied) > 0);
      await page.getByTestId('jump-bottom').click();
      await until('기록 재구성 뒤 맨 아래', () => terminal.evaluate(el => el.dataset.viewTop === el.dataset.viewBase));
      await page.clock.install();
      await page.clock.resume();
      for (const destination of ['thread', 'tasks', 'reports']) {
        for (const method of ['normal', 'ctrl-tab']) {
          const round = 0;
          phase = { stage: "retained", pane: fixture.coordinator, destination, method, round };
          await page.evaluate(() => { window.watchReturns(null); window.returnTerminal = document.querySelector('.term:not([hidden]) .xterm'); });
          if (destination === 'thread') await go(page, `pane=${fixture.thread}`);
          else await inList(page, `activity-${destination}`);
          await page.locator(`.term[data-pane="${fixture.coordinator}"][hidden]`).waitFor({ state: 'attached' });
          await frames(page);
          const connections = await page.evaluate(id => { window.returnFrames = []; window.watchReturns(id); return { connections: window.returnConnections[id], snapshots: window.returnSnapshots[id] }; }, fixture.coordinator);
          if (method === 'ctrl-tab') await page.keyboard.press('Control+Tab');
          else await inList(page, 'activity-coordinator');
          await ready();
          assert.equal(await page.evaluate(id => window.returnConnections[id], fixture.coordinator), connections.connections, "보관 복귀는 새 연결 없음");
          assert.equal(await page.evaluate(id => window.returnSnapshots[id], fixture.coordinator), connections.snapshots, "보관 복귀는 새 snapshot 없음");
          assert.equal(await terminal.evaluate(el => el.querySelector(".xterm") === window.returnTerminal), true, "같은 xterm으로 복귀");
          await frames(page);
          // 보관한 실제 xterm이 첫 휠의 대상이며 같은 마우스 모드로 바로 보낸다.
          assert.equal(await terminal.evaluate(box => {
            const live = box.querySelector('.xterm');
            const rect = live.querySelector('.xterm-screen').getBoundingClientRect();
            const area = box.getBoundingClientRect();
            const hit = document.elementFromPoint((rect.left + Math.min(rect.right, area.right)) / 2, (rect.top + Math.min(rect.bottom, area.bottom)) / 2);
            return live.contains(hit);
          }), true, '복귀 중 실제 터미널이 휠 입력 대상이다');
          const before = await page.evaluate(() => window.returnFrames.slice());
          const wheelFrom = await sentCount(page);
          const receivedBefore = receivedWheels();
          phase.sentFrom = wheelFrom;
          phase.receivedBefore = receivedBefore;
          await wheel();
          if (round === 0 && destination === 'tasks') await shot(page, `return-${engine.browserType().name()}-${width}-${method}-pending`);
          await until('복귀 중 휠 한 번 전달', async () => (await wheelInputs(wheelFrom)).length > 0);
          assert.equal((await wheelInputs(wheelFrom)).length, 1, '복귀 휠 정확히 한 프레임');
          await until('복귀 휠 실제 PTY 수신', () => receivedWheels() > receivedBefore);
          assert.equal(receivedWheels(), receivedBefore + 1, 'PTY 휠 정확히 한 번');
          await frames(page);
          assert.equal((await wheelInputs(wheelFrom)).length, 1, '복귀 휠은 중복 전달하지 않는다');
          const samples = await page.evaluate(() => window.returnFrames.slice());
          results.push({ destination, method, round, before, samples });
          // 권한이 확인된 뒤 실제 입력이 host·PTY에 도착한다.
          const marker = `return-${destination}-${method}-${round}`;
          await page.getByLabel('보낼 글').fill(marker);
          await page.getByLabel('보낼 글').press('Enter');
          await until('복귀 뒤 입력 반영', () => terminal.locator('.xterm-rows').textContent().then(text => text.includes(marker)));
          assert.equal(receivedWheels(), receivedBefore + 1, '다음 입력까지 휠 중복 없음');
        }
      }
      const bad = results.filter(r => r.samples.some(s => s.banner.includes('읽기 전용')) || r.before.some(s => !s.body));
      fs.writeFileSync(`${root}/pane-return-${engine.browserType().name()}-${width}.json`, JSON.stringify(results, null, 2));
      if (process.env.AAA_RETURN_TRACE) fs.writeFileSync(`${process.env.AAA_RETURN_TRACE}/${engine.browserType().name()}-${width}.json`, JSON.stringify(results, null, 2));
      assert.deepEqual(bad.map(r => `${r.destination}/${r.method}/${r.round}`), [], '복귀 중 거짓 읽기 전용·빈 본문');
      await page.evaluate(() => window.watchReturns(null));
      // 앞 기록 요청 뒤 숨긴다. 숨은 xterm의 render를 기다리면 이후 출력도 ACK도 멈춘다.
      phase = { stage: "hidden-history", pane: fixture.coordinator };
      gateHistory = true;
      const hiddenScroll = await page.waitForFunction(scrollHistoryWhenReady, '[data-testid="terminal"]');
      const appliedBeforeHide = Number((await hiddenScroll.jsonValue()).dataset.historyApplied);
      await hiddenScroll.dispose();
      await inTime(historyArrival, 20_000, '터미널 앞 기록 요청 도착');
      await go(page, 'tasks');
      await terminal.waitFor({ state: 'detached' });
      releaseHistory();
      await page.waitForFunction(({ id, applied }) => Number(document.querySelector(`.term[data-pane="${id}"] .term-box`)?.dataset.historyApplied) > applied,
        { id: fixture.coordinator, applied: appliedBeforeHide });
      await until('숨긴 coordinator 크기 점유 해제', () => aaa('pane', 'list').panes.find(p => p.id === fixture.coordinator)?.lease === null);
      const hiddenConnections = await page.evaluate(id => window.returnConnections[id], fixture.coordinator);
      const marker = `hidden-output-${width}`;
      aaa('pane', 'run', fixture.coordinator, marker);
      await waitOutput(fixture.coordinator, marker);
      await inList(page, 'activity-coordinator');
      await page.getByTestId('jump-bottom').click();
      await until('숨긴 동안 받은 출력 표시', () => terminal.locator('.xterm-rows').textContent().then(text => text.includes(marker)));
      assert.equal(await page.evaluate(id => window.returnConnections[id], fixture.coordinator), hiddenConnections, '숨김 출력 뒤에도 같은 연결');
      phase = { stage: "expired", pane: fixture.coordinator };
      // 5분 만료는 DOM·연결을 함께 버리고 새 snapshot을 받아야 입력을 연다.
      await page.evaluate(() => { window.watchReturns(null); });
      await go(page, 'tasks');
      await terminal.waitFor({ state: 'detached' });
      // 만료 사건만 진행한다. 5분치 애니메이션 frame을 합성할 필요는 없다.
      await page.clock.fastForward(5 * 60_000);
      await page.locator(`.term[data-pane="${fixture.coordinator}"]`).waitFor({ state: 'detached' });
      const beforeExpiry = await page.evaluate(id => window.returnConnections[id], fixture.coordinator);
      await page.evaluate(() => { window.holdReturn = true; window.returnHeld = 0; });
      await inList(page, 'activity-coordinator');
      await page.waitForFunction(() => window.returnHeld > 0);
      assert.equal(await page.evaluate(id => window.returnConnections[id], fixture.coordinator), beforeExpiry + 1);
      const cancelledFrom = await sentCount(page);
      await wheel();
      assert.equal((await wheelInputs(cancelledFrom)).length, 0, '새 snapshot 전 입력 금지');
      await go(page, 'tasks');
      await terminal.waitFor({ state: 'detached' });
      await page.evaluate(() => window.releaseReturn());
      await inList(page, 'activity-coordinator');
      await ready();
      await frames(page);
      assert.equal((await wheelInputs(cancelledFrom)).length, 0, '준비 전 휠은 다음 복귀에 재전송하지 않는다');
      const expiredBefore = receivedWheels();
      await wheel();
      await until('만료 복귀 첫 휠 한 번', async () => (await wheelInputs(cancelledFrom)).length > 0);
      assert.equal((await wheelInputs(cancelledFrom)).length, 1);
      await until('만료 복귀 휠 PTY 수신', () => receivedWheels() > expiredBefore);
      assert.equal(receivedWheels(), expiredBefore + 1);
      assert.deepEqual(errors, []);
      phase = { stage: "reader", pane: fixture.coordinator };
      const reader = await pairPage(engine, aaa('web', 'pair', '--reader').code, { width, height: 900 }, { onPage: page => diagnostic.watch(page, "reader"), hash: `#pane=${fixture.coordinator}` });
      try {
        await reader.page.getByTestId('readonly-banner').getByText('보기 권한', { exact: true }).waitFor();
        phase = { stage: 'reader-hide', pane: fixture.coordinator };
        await go(reader.page, 'tasks');
        await reader.page.getByTestId('terminal').waitFor({ state: 'detached' });
        phase = { stage: 'reader-return', pane: fixture.coordinator };
        await go(reader.page, `pane=${fixture.coordinator}`);
        await reader.page.getByTestId('readonly-banner').getByText('보기 권한', { exact: true }).waitFor();
        if (await reader.page.getByTestId('chat-toggle').getAttribute('aria-pressed') === 'true') await reader.page.getByTestId('chat-toggle').click();
        await reader.page.getByTestId('terminal').locator('.xterm-screen').waitFor();
        const from = await sentCount(reader.page);
        await reader.page.getByTestId('terminal').click();
        await reader.page.keyboard.type('reader-must-not-send\n');
        assert.deepEqual((await sentFrames(reader.page, from)).filter(f => ['input', 'submit', 'bin'].includes(f.t)), []);
        await shot(reader.page, `return-${engine.browserType().name()}-${width}-reader`);
        assert.deepEqual(reader.errors, []);
      } catch (error) {
        diagnostic.failed(error, 'reader');
        await diagnostic.capture(error, async () => ({
          sent: await sentFrames(reader.page),
          screen: await reader.page.evaluate(() => ({ hash: location.hash,
            terminals: [...document.querySelectorAll('.term')].map(el => ({ pane: el.dataset.pane, hidden: el.hidden })),
            log: window.__aaaTermLog })),
        }), 'reader').catch(secondary => diagnostic.record('diagnostic.failure', { role: 'reader', error: String(secondary) }));
        throw error;
      } finally { await diagnostic.cleanup('reader', () => reader.page.context().close()); }
    } catch (error) {
      diagnostic.failed(error);
      await diagnostic.capture(error, async () => ({
        receivedWheels: receivedWheels(), sent: await sentFrames(page, phase.sentFrom ?? 0),
        screen: await page.evaluate(() => {
          const box = document.querySelector('[data-testid="terminal"]');
          const viewport = box?.querySelector('.xterm-viewport');
          return { hash: location.hash, pane: document.querySelector(".term:not([hidden])")?.dataset.pane, dataset: { ...box?.dataset },
            viewport: viewport && { top: viewport.scrollTop, height: viewport.scrollHeight, client: viewport.clientHeight },
            historyRequests: window.returnHistoryRequests, scrolls: window.returnScrolls, frames: window.returnFrames, log: window.__aaaTermLog };
        }),
      })).catch(secondary => diagnostic.record('diagnostic.failure', { error: { message: secondary.message, stack: secondary.stack } }));
      throw error;
    } finally { await diagnostic.cleanup('controller', () => page.context().close()); }
  };
  try {
  await step("화면 복귀: Chromium 1280 일반 이동·Ctrl+Tab 6회와 실제 입력·Reader", async () => check(browser, 1280));
  await step("화면 복귀: WebKit 390 일반 이동·Ctrl+Tab 6회와 실제 입력·Reader", async () => check(safari = await webkit.launch({ headless: true }), 390));
  } finally { await safari?.close(); }
}
