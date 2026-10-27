// 최초 오류를 보존해야 다음 CI의 E109 종료 원인을 판단할 수 있다. 실제 headless WebKit만 쓴다.
import assert from 'node:assert/strict';
import { once } from 'node:events';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createRequire } from 'node:module';
import { test } from 'node:test';
import { returnDiagnostics } from './lib/return-diagnostics.mjs';

const require = createRequire(path.join(process.env.NODE_PATH ?? process.cwd(), 'noop.js'));
const { webkit } = require('playwright');
const read = (dir, name) => JSON.parse(fs.readFileSync(path.join(dir, name), 'utf8'));

test('E109: 실제 detached 완료와 명시적 정리·browser exit를 구별한다', { timeout: 5000 }, async (t) => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'aar-'));
  const browser = await webkit.launch({ headless: true });
  t.after(() => browser.close());
  try {
    let phase = { stage: 'hide' };
    const diagnostic = returnDiagnostics(browser, dir, 390, () => phase);
    const page = await browser.newPage();
    diagnostic.watch(page, 'controller');
    await page.setContent('<div data-testid="terminal">복귀 본문</div>');
    await page.getByTestId('terminal').waitFor();
    await page.evaluate(() => document.querySelector('[data-testid="terminal"]').remove());
    await page.getByTestId('terminal').waitFor({ state: 'detached' });
    assert.deepEqual(read(dir, 'return-lifecycle-390.json').filter(row => row.event === 'page.close'), []);
    phase = { stage: 'done' };
    await diagnostic.cleanup('controller', () => page.context().close());
    const child = browser._connection.toImpl(browser).options.browserProcess.process;
    const exited = once(child, 'exit');
    await browser.close();
    await exited;
    const events = read(dir, 'return-lifecycle-390.json');
    assert.ok(events.findIndex(row => row.event === 'cleanup.start') < events.findIndex(row => row.event === 'page.close'));
    const exit = events.find(row => row.event === 'browser.exit');
    assert.equal(exit.code, 0); assert.equal(exit.signal, null);
    assert.equal(exit.phase.stage, 'done');
  } finally { await browser.close(); fs.rmSync(dir, { recursive: true, force: true }); }
});

test('E109: 기다리는 실제 WebKit 종료 뒤에도 최초 오류·조회 오류·정리 오류를 보존한다', { timeout: 5000 }, async (t) => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'aar-'));
  const browser = await webkit.launch({ headless: true });
  t.after(() => browser.close());
  try {
    const diagnostic = returnDiagnostics(browser, dir, 390, () => ({ stage: 'hidden-history', pane: 'fixture' }));
    const page = await browser.newPage();
    diagnostic.watch(page, 'controller');
    await page.setContent('<div data-testid="terminal">복귀 본문</div>');
    // 실제 selector 도착 뒤 공개 browser.close로 종료한다. 사건 순서만 제어한다.
    let arrive;
    const waiting = new Promise(done => { arrive = done; });
    await page.exposeFunction('selectorStarted', arrive);
    const { selectors } = require('playwright');
    await selectors.register('returnWait', () => ({
      query: root => root.querySelector('[data-testid="terminal"]'),
      queryAll: root => { window.selectorStarted(); return [...root.querySelectorAll('[data-testid="terminal"]')]; },
    }));
    const result = page.locator('returnWait=terminal').waitFor({ state: 'detached' }).then(() => null, error => error);
    await waiting;
    const child = browser._connection.toImpl(browser).options.browserProcess.process;
    const exited = once(child, 'exit');
    const closed = browser.close();
    const primary = await result;
    await closed;
    await exited;
    assert.match(primary.message, /Target page, context or browser has been closed/);
    diagnostic.failed(primary);
    const collect = async () => ({ screen: await page.evaluate(() => document.body.textContent) });
    await diagnostic.capture(primary, collect, 'reader');
    await diagnostic.capture(primary, collect);
    const reader = read(dir, 'return-failure-390-reader.json');
    assert.equal(reader.role, 'reader');
    assert.equal(reader.error.message, primary.message);
    const saved = read(dir, 'return-failure-390.json');
    assert.equal(saved.error.message, primary.message);
    assert.equal(saved.error.stack, primary.stack);
    assert.match(saved.diagnosticError.message, /page.evaluate: Target page/);
    await diagnostic.cleanup('controller', () => Promise.reject(new Error('정리도 실패')));
    const events = read(dir, 'return-lifecycle-390.json');
    assert.ok(events.findIndex(row => row.event === 'page.close') < events.findIndex(row => row.event === 'cleanup.start'));
    assert.ok(events.some(row => row.event === 'browser.disconnected'));
    assert.ok(events.some(row => row.event === 'browser.exit' && row.code === 0 && row.signal === null));
    assert.equal(events.find(row => row.event === 'cleanup.failure').error.message, '정리도 실패');
    assert.equal(events.find(row => row.event === 'failure').error.message, primary.message);
    if (process.env.AAA_RETURN_TRACE) fs.cpSync(dir, path.join(process.env.AAA_RETURN_TRACE, 'controlled-close'), { recursive: true });
  } finally { await browser.close(); fs.rmSync(dir, { recursive: true, force: true }); }
});
