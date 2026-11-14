// E109의 실제 관측기를 패치 xterm·제품 덮개 CSS와 실행한다. 뒤쪽 DOM과 보이는 본문을 구별한다.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';
import { before, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { build } from 'vite';
import { buildXtermSource } from '../xterm-source.mjs';
import { launchChromium } from './lib/chromium-launch.mjs';

const require = createRequire(path.join(process.env.NODE_PATH ?? process.cwd(), 'noop.js'));
const { chromium, webkit } = require('playwright');
assert.equal(require('playwright/package.json').version, '1.64.1');
const spec = readFileSync(new URL('./specs/109-pane-return.mjs', import.meta.url), 'utf8');
const instrument = spec.slice(spec.indexOf('function instrument()'), spec.indexOf('export default'));
const coverStyle = readFileSync(new URL('../src/style.css', import.meta.url), 'utf8')
  .match(/\.xterm \.xterm-screen\.term-history-cover\s*\{[^}]+\}/)[0];
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
  test(`E109 ${engine.name()} ${width}: 덮개 뒤 reset은 본문 소실이 아니며 실제 빈 화면·읽기 전용은 감지한다`, { timeout: 5000 }, async t => {
    const browser = engine === chromium ? await launchChromium(engine) : await engine.launch({ headless: true });
    t.after(() => browser.close());
    const page = await browser.newPage({ viewport: { width, height: 900 } });
    await page.setContent('<section class="term" data-pane="fixture"><div class="term-box" data-testid="terminal" data-readonly="false"></div></section>');
    await page.addStyleTag({ path: fileURLToPath(new URL('../node_modules/@xterm/xterm/css/xterm.css', import.meta.url)) });
    await page.addStyleTag({ content: coverStyle });
    await page.addScriptTag({ content: script });
    await page.addScriptTag({ content: `(${instrument})();` });
    await page.evaluate(async () => {
      const term = window.term = new Xterm.Terminal({ cols: 40, rows: 12 });
      term.open(document.querySelector('.term-box'));
      await new Promise(done => term.write('coordinator 복귀 시험\r\nfake> ', done));
      window.watchReturns('fixture');
    });
    await page.waitForFunction(() => window.returnFrames.some(s => s.body));
    const covered = await page.evaluate(async () => {
      const screen = term.element.querySelector('.xterm-screen');
      const cover = screen.cloneNode(true);
      cover.classList.add('term-history-cover');
      cover.style.left = `${screen.offsetLeft}px`;
      cover.style.top = `${screen.offsetTop}px`;
      screen.parentElement.appendChild(cover);
      window.returnFrames = [];
      term.reset();
      // reset의 실제 DOM 변경을 관측한 뒤 읽는다. 시간을 자거나 새 렌더를 기다리지 않는다.
      await Promise.resolve();
      const rect = node => { const r = node.getBoundingClientRect(); return [r.x, r.y, r.width, r.height]; };
      return { samples: window.returnFrames, live: screen.querySelector('.xterm-rows').textContent,
        cover: cover.textContent, screenRect: rect(screen), coverRect: rect(cover) };
    });
    assert.ok(covered.cover.includes('fake>'));
    assert.equal(covered.live.includes('fake>'), false, '실제 xterm reset이 뒤쪽 본문을 지웠다');
    assert.deepEqual(covered.coverRect, covered.screenRect, '제품 덮개는 같은 화면을 가린다');
    assert.ok(covered.samples.length > 0, '기존 관측기가 실제 변경을 읽었다');
    assert.equal(covered.samples.some(s => !s.body), false, '보이는 덮개를 빈 본문으로 오인하지 않는다');
    const blank = await page.evaluate(async () => {
      window.returnFrames = [];
      document.querySelector('.term-history-cover').remove();
      await Promise.resolve();
      return window.returnFrames;
    });
    assert.ok(blank.some(s => !s.body), '덮개 없는 실제 빈 본문은 계속 실패한다');
    await page.evaluate(() => new Promise(done => term.write('fake> ', done)));
    // write 콜백은 파싱 완료다. 뒤쪽 본문이 실제로 그려진 뒤 빈 덮개와 비교한다.
    await page.waitForFunction(() => document.querySelector('.xterm-screen .xterm-rows').textContent.includes('fake>'));
    const blankCover = await page.evaluate(async () => {
      const screen = term.element.querySelector('.xterm-screen');
      const cover = screen.cloneNode(true);
      cover.classList.add('term-history-cover');
      cover.querySelector('.xterm-rows').replaceChildren();
      window.returnFrames = [];
      screen.parentElement.appendChild(cover);
      await Promise.resolve();
      return window.returnFrames;
    });
    assert.ok(blankCover.some(s => s.liveBody && s.coverBody === false && !s.body), '빈 덮개도 뒤쪽 본문으로 통과시키지 않는다');
    const readonly = await page.evaluate(async () => {
      window.returnFrames = [];
      const banner = document.createElement('div');
      banner.dataset.testid = 'readonly-banner';
      banner.textContent = '읽기 전용';
      document.querySelector('.term').appendChild(banner);
      await Promise.resolve();
      return window.returnFrames;
    });
    assert.ok(readonly.some(s => s.banner.includes('읽기 전용')), '잘못된 읽기 전용도 계속 실패한다');
  });
}
