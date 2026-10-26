// 실제 HTTP 헤더/문서 교체와 실제 실패 자식으로 수명을 검사한다. 고정 대기는 없다.
import assert from 'node:assert/strict';
import http from 'node:http';
import fs from 'node:fs';
import { once } from 'node:events';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { browser, root, env, inTime } from './harness.mjs';
import { threadPageTrace } from './thread-page-trace.mjs';

export async function threadBodyLifetime() {
  let partial;
  const server = http.createServer((req, res) => {
    if (req.url !== '/api/v1/call') return res.end('<!doctype html><p>fixture</p>');
    partial = res;
    res.writeHead(200, { 'content-type': 'application/json' });
    res.write('{"ok":'); // 헤더만 관측된 상태에서 문서를 교체한다.
  });
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  const context = await browser.newContext();
  try {
    const page = await context.newPage();
    const trace = threadPageTrace(`${root}/diagnostics`, 'body-lifetime');
    trace.onPage(page);
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    const response = page.waitForEvent('response', r => r.url().endsWith('/api/v1/call'));
    await page.evaluate(() => {
      void fetch('/api/v1/call', { method: 'POST', headers: { 'x-e2e-document': String(performance.timeOrigin),
        'x-e2e-connection': '1', 'x-e2e-request': '1' }, body: JSON.stringify({ op: 'threads' }) }).then(r => r.json()).catch(() => {});
    });
    await response;
    await page.goto('about:blank');
    const reads = await inTime(trace.reads([]), 20000, '문서 교체 뒤 본문 관측 회수');
    assert.equal(browser.isConnected(), true, 'browser.close 전에 끝나야 한다');
    assert.equal(reads.length, 1);
    assert.equal(reads[0].pages, 0, '미완료 본문을 완전 페이지로 세지 않는다');
    assert.equal(reads[0].next, 'aborted-before-body');
    partial.end('{"kind":"threads","threads":[],"next_after":null}}');
  } finally {
    await context.close();
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
  }
}

export async function threadFailureLifetime() {
  const fd = fs.openSync(`${root}/diagnostics/thread-lifetime-child.log`, 'w');
  const child = spawn(process.execPath, [fileURLToPath(new URL('../fixtures/thread-lifetime.mjs', import.meta.url))], {
    env: { ...process.env, AAA_BIN: env.AAA_BIN ?? process.env.AAA_BIN }, stdio: ['ignore', fd, fd, 'ipc'],
  });
  fs.closeSync(fd);
  const exited = once(child, 'exit');
  try {
    const [message] = await inTime(once(child, 'message'), 20000, '실패한 실험실 정리 완료');
    // 소유권 회귀에서도 진단을 보존하고 해당 fixture만 끝내며, 통과시키지는 않는다.
    if (message.exit === null) child.send('close-leaked');
    const [code, signal] = await inTime(exited, 20000, '실패한 실험실의 자연 종료');
    assert.notEqual(message.exit, null, 'owner 종료 때 WebKit 프로세스도 종료되어야 한다');
    assert.equal(signal, null);
    assert.equal(code, 1);
    assert.match(fs.readFileSync(`${root}/diagnostics/thread-lifetime-child.log`, 'utf8'), /thread-lifetime-owner-failed/);
  } finally {
    if (child.exitCode === null && child.signalCode === null) { child.kill('SIGTERM'); await exited; }
  }
}
