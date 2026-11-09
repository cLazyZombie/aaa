// E109의 rAF 입력 유실을 실제 패치 xterm·native scroll로 재현한다.
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import path from 'node:path';
import { before, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { build } from 'vite';
import { buildXtermSource } from '../xterm-source.mjs';
import { launchChromium } from './lib/chromium-launch.mjs';
import { scrollHistoryWhenReady } from './lib/history-scroll.mjs';

const require = createRequire(path.join(process.env.NODE_PATH ?? process.cwd(), 'noop.js'));
const { chromium, webkit } = require('playwright');
assert.equal(require('playwright/package.json').version, '1.64.1');
let script;
before(async () => {
  const source = buildXtermSource(fileURLToPath(new URL('../', import.meta.url)));
  const bundle = await build({
    configFile: false, logLevel: 'error',
    resolve: { alias: [
      { find: /^browser\//, replacement: `${source}/browser/` },
      { find: /^common\//, replacement: `${source}/common/` },
    ] },
    build: { write: false, minify: false,
      lib: { entry: `${source}/browser/public/Terminal.js`, formats: ['iife'], name: 'Xterm' } },
  });
  script = (Array.isArray(bundle) ? bundle[0] : bundle).output[0].code;
});

for (const [engine, width] of [[chromium, 1280], [webkit, 390]]) {
  test(`E109 ${engine.name()} ${width}: 늦은 xterm rAF에도 앞 기록 이동은 한 번 도착한다`, { timeout: 5000 }, async t => {
    const browser = engine === chromium ? await launchChromium(engine) : await engine.launch({ headless: true });
    t.after(() => browser.close());
    const page = await browser.newPage({ viewport: { width, height: 900 } });
    await page.setContent('<div data-testid="terminal"></div>');
    await page.addStyleTag({ path: fileURLToPath(new URL('../node_modules/@xterm/xterm/css/xterm.css', import.meta.url)) });
    await page.addScriptTag({ content: script });
    await page.evaluate(async () => {
      const box = document.querySelector('[data-testid="terminal"]');
      const term = window.term = new Xterm.Terminal({ cols: 47, rows: 38, scrollback: 6000, fontSize: 13 });
      term.open(box);
      term.onRender(() => Object.assign(box.dataset, { readonly: 'false', fit: 'same', historyMore: '1', historyWaiting: '0',
        viewTop: String(term.buffer.active.viewportY), viewBase: String(term.buffer.active.baseY) }));
      await new Promise(done => term.write(Array.from({ length: 2040 }, (_, n) => `row-${n}\r\n`).join(''), done));
    });
    await page.waitForFunction(() => {
      const viewport = document.querySelector('.xterm-viewport');
      const row = document.querySelector('.xterm-rows > div');
      return viewport.scrollTop > 0 && viewport.scrollTop === term.buffer.active.baseY * row.getBoundingClientRect().height;
    });
    await page.evaluate(() => {
      const viewport = document.querySelector('.xterm-viewport');
      let prototype = viewport, descriptor;
      while (!descriptor) { prototype = Object.getPrototypeOf(prototype); descriptor = Object.getOwnPropertyDescriptor(prototype, 'scrollTop'); }
      window.scrollWrites = [];
      Object.defineProperty(viewport, 'scrollTop', {
        get: () => descriptor.get.call(viewport),
        set: value => { scrollWrites.push(value); descriptor.set.call(viewport, value); },
      });
      window.topEvents = 0;
      viewport.addEventListener('scroll', () => {
        if (term.buffer.active.viewportY === 0) topEvents++;
      });
    });
    // 원래 waitForFunction과 같은 rAF에서 predicate를 부른 직후 resize의
    // viewport 동기화를 끝낸다. DOM 좌표·scroll 사건·buffer 값은 위조하지 않는다.
    const waitForFunction = page.waitForFunction.bind(page);
    page.waitForFunction = (predicate, argument) => page.evaluateHandle(({ expression, argument }) => new Promise(done => {
      requestAnimationFrame(() => {
        const ready = (0, eval)(`(${expression})`)(argument);
        term._core.viewport._innerRefresh();
        done(ready);
      });
    }), { expression: predicate.toString(), argument });
    let before;
    try { before = await scrollHistoryWhenReady(page, '[data-testid="terminal"]'); }
    finally { page.waitForFunction = waitForFunction; }
    assert.ok(before.top > 0);
    // 다음 rAF는 native scroll 처리 뒤의 관측 경계다. 고정 sleep은 없다.
    const result = await page.evaluate(() => new Promise(done => requestAnimationFrame(() => done({
      top: document.querySelector('.xterm-viewport').scrollTop, row: term.buffer.active.viewportY,
      topEvents, writes: scrollWrites.filter(value => value === 0).length,
    }))));
    assert.deepEqual(result, { top: 0, row: 0, topEvents: 1, writes: 1 });
  });
}
