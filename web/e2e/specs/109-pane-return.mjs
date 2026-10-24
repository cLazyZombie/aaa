// 소켓 open 뒤 snapshot 전을 실제 메시지 gate로 분리한다. 고정 sleep 없이 복귀의 매 frame을 검사한다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { aaa, step, browser, webkit, pairPage, until, inList, shot, root, sentCount, sentFrames, waitOutput } from "../lib/harness.mjs";
import { prepare, go, frames } from "../lib/chat-retention.mjs";

function instrument() {
  const media = window.matchMedia.bind(window);
  window.matchMedia = query => {
    const value = media(query);
    if (query === '(display-mode: standalone)') Object.defineProperty(value, 'matches', { value: true });
    return value;
  };
  const Native = window.WebSocket;
  window.returnFrames = [];
  window.holdReturn = false;
  window.WebSocket = class extends Native {
    constructor(url, protocols) {
      super(url, protocols);
      if (!String(url).includes('/panes/')) return;
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
  const frame = () => { sample(); requestAnimationFrame(frame); };
  requestAnimationFrame(frame);
}

export default async function () {
  let fixture, safari;
  await step("화면 복귀: 실제 coordinator·긴 기록 준비", async () => {
    fixture = await prepare();
    fs.writeFileSync(`${root}/return-screen.sh`, "printf '\\033[3J\\033[2J\\033[Hcoordinator 복귀 시험\\n이 본문은 새 snapshot이 오기 전에도 남아야 한다.\\n'\n");
    aaa('pane', 'run', fixture.coordinator, `post-probe ${root}/return-screen.sh`);
    await waitOutput(fixture.coordinator, 'coordinator 복귀 시험');
  }, { always: true });
  const check = async (engine, width) => {
    const { page, errors } = await pairPage(engine, aaa('web', 'pair').code, { width, height: 900 }, {
      init: instrument, line: true, hash: `#pane=${fixture.coordinator}`,
      setup: context => context.addInitScript(keys => { for (const key of keys) localStorage.setItem(`aaa:view:${key}`, 'terminal'); }, fixture.keys),
    });
    const terminal = page.getByTestId('terminal');
    const ready = async () => {
      await page.locator('[data-testid="terminal"][data-readonly="false"]').waitFor({ state: 'attached' });
      await terminal.locator('.terminal-preview').waitFor({ state: 'detached' });
      await until('coordinator 본문', () => terminal.locator('.xterm-rows').textContent().then(text => text.includes('fake>')));
    };
    const results = [];
    try {
      await ready();
      for (const destination of ['thread', 'tasks', 'reports']) {
        for (const method of ['normal', 'ctrl-tab']) {
          for (let round = 0; round < 2; round++) {
            await page.evaluate(() => { window.watchReturn = null; });
            if (destination === 'thread') await go(page, `pane=${fixture.thread}`);
            else await inList(page, `activity-${destination}`);
            await page.locator(`.term[data-pane="${fixture.coordinator}"][hidden]`).waitFor({ state: 'attached' });
            await frames(page);
            await page.evaluate(id => { window.holdReturn = true; window.returnHeld = 0; window.returnFrames = []; window.watchReturn = id; }, fixture.coordinator);
            if (method === 'ctrl-tab') await page.keyboard.press('Control+Tab');
            else await inList(page, 'activity-coordinator');
            await page.waitForFunction(() => window.returnHeld > 0);
            await frames(page);
            const before = await page.evaluate(() => window.returnFrames.slice());
            if (round === 0 && destination === 'tasks') await shot(page, `return-${engine.browserType().name()}-${width}-${method}-pending`);
            await page.evaluate(() => window.releaseReturn());
            await ready();
            await frames(page);
            const samples = await page.evaluate(() => window.returnFrames.slice());
            results.push({ destination, method, round, before, samples });
            // 권한이 확인된 뒤 실제 입력이 host·PTY에 도착한다.
            const marker = `return-${destination}-${method}-${round}`;
            await page.getByLabel('보낼 글').fill(marker);
            await page.getByLabel('보낼 글').press('Enter');
            await until('복귀 뒤 입력 반영', () => terminal.locator('.xterm-rows').textContent().then(text => text.includes(marker)));
          }
        }
      }
      const bad = results.filter(r => r.samples.some(s => s.banner.includes('읽기 전용')) || r.before.some(s => !s.body));
      fs.writeFileSync(`${root}/pane-return-${engine.browserType().name()}-${width}.json`, JSON.stringify(results, null, 2));
      if (process.env.AAA_RETURN_TRACE) fs.writeFileSync(`${process.env.AAA_RETURN_TRACE}/${engine.browserType().name()}-${width}.json`, JSON.stringify(results, null, 2));
      assert.deepEqual(bad.map(r => `${r.destination}/${r.method}/${r.round}`), [], '복귀 중 거짓 읽기 전용·빈 본문');
      assert.deepEqual(errors, []);
      const reader = await pairPage(engine, aaa('web', 'pair', '--reader').code, { width, height: 900 }, { hash: `#pane=${fixture.coordinator}` });
      try {
        await reader.page.getByTestId('readonly-banner').getByText('보기 권한', { exact: true }).waitFor();
        await go(reader.page, 'tasks');
        await reader.page.getByTestId('terminal').waitFor({ state: 'detached' });
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
      } finally { await reader.page.context().close(); }
    } finally { await page.context().close(); }
  };
  try {
  await step("화면 복귀: Chromium 1280 일반 이동·Ctrl+Tab 12회와 실제 입력·Reader", async () => check(browser, 1280));
  await step("화면 복귀: WebKit 390 일반 이동·Ctrl+Tab 12회와 실제 입력·Reader", async () => check(safari = await webkit.launch({ headless: true }), 390));
  } finally { await safari?.close(); }
}
