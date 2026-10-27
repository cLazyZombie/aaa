// 실제 HTTP 헤더/문서 교체와 실제 실패 자식으로 수명을 검사한다. 고정 대기는 없다.
import assert from 'node:assert/strict';
import http from 'node:http';
import fs from 'node:fs';
import { once } from 'node:events';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { browser, root, env, inTime } from './harness.mjs';
import { threadPageTrace, threadTraceDocument } from './thread-page-trace.mjs';

// Term 정리와 같은 CONNECTING → close 순서. 앱의 send는 한 번도 부르지 않는다.
// Playwright 대체 소켓은 이 순서 뒤 passthrough를 열고 snapshot 수신에서 예외를 냈다.
export async function threadConnectingPaneLifetime(page, pane) {
  const failed = Promise.withResolvers();
  const onError = error => failed.reject(error);
  page.on('pageerror', onError);
  try {
    const result = await inTime(Promise.race([
      page.evaluate(async pane => {
        const url = new URL(`/api/v1/panes/${encodeURIComponent(pane)}/ws?flow=ack`, location.href);
        url.protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
        const socket = new WebSocket(url);
        const before = socket.readyState;
        const closed = new Promise(resolve => socket.addEventListener('close', () => resolve(socket.readyState), { once: true }));
        socket.close();
        return { before, after: await closed };
      }, pane),
      failed.promise,
    ]), 5000, '연결 중 취소한 pane 소켓의 종료');
    assert.deepEqual(result, { before: 0, after: 3 });
  } finally { page.off('pageerror', onError); }
}

export async function threadBodyLifetime() {
  let partial, complete = false;
  const server = http.createServer((req, res) => {
    if (req.url !== '/api/v1/call') return res.end('<!doctype html><p>fixture</p>');
    if (complete) {
      res.writeHead(200, { 'content-type': 'application/json' });
      return res.end(JSON.stringify({ok:{kind:'threads',threads:[],next_after:null}}));
    }
    partial = res;
    res.writeHead(200, { 'content-type': 'application/json' });
    res.write('{"ok":'); // 헤더만 관측된 상태에서 문서를 교체한다.
  });
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  const context = await browser.newContext();
  let releaseBody;
  try {
    await context.addInitScript(threadTraceDocument);
    const page = await context.newPage();
    const trace = threadPageTrace(`${root}/diagnostics`, 'body-lifetime');
    trace.onPage(page);
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    const response = page.waitForEvent('response', r => r.url().endsWith('/api/v1/call'));
    await page.evaluate(() => {
      void fetch('/api/v1/call', { method: 'POST', headers: { 'x-e2e-document': document.__aaaThreadTraceDocument,
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
    // 다운로드는 끝났지만 Node의 body protocol 호출이 문서 교체 뒤에 도착하는 경계.
    // 실제 Chrome의 사라진 resource 오류를 쓰며 오류나 HTTP 응답을 만들어 내지 않는다.
    complete = true;
    const finishedPage = await context.newPage();
    const release = Promise.withResolvers();
    releaseBody = release.resolve;
    let bodyFailure;
    finishedPage.on('response', response => {
      if (!response.url().endsWith('/api/v1/call')) return;
      const original = response.body.bind(response);
      response.body = () => release.promise.then(original).catch(error => { bodyFailure = error; throw error; });
    });
    const finishedTrace = threadPageTrace(`${root}/diagnostics`, 'body-finished-navigation');
    finishedTrace.onPage(finishedPage);
    await finishedPage.goto(`http://127.0.0.1:${server.address().port}`);
    const finished = finishedPage.waitForEvent('requestfinished', request => request.url().endsWith('/api/v1/call'));
    await finishedPage.evaluate(() => {
      void fetch('/api/v1/call', {method:'POST',headers:{'x-e2e-document':document.__aaaThreadTraceDocument,
        'x-e2e-connection':'1','x-e2e-request':'1'},body:JSON.stringify({op:'threads'})}).then(r=>r.json());
    });
    await finished;
    await finishedPage.goto('about:blank');
    release.resolve();
    const finishedReads = await inTime(finishedTrace.reads([]), 20000, '완료한 다운로드의 문서 교체 꼬리 회수');
    assert.match(String(bodyFailure), /No resource with given identifier found/);
    assert.equal(finishedReads.length, 1);
    assert.equal(finishedReads[0].pages, 0, '문서 교체 뒤 잃은 본문을 성공 페이지로 세지 않는다');
    assert.equal(browser.isConnected(), true);
    // 같은 문서의 body 오류는 navigation 꼬리로 버리지 않는다.
    const currentPage = await context.newPage();
    currentPage.on('response', response => {
      if (response.url().endsWith('/api/v1/call')) response.body = async () => { throw new Error('current-document-body-failure'); };
    });
    const currentTrace = threadPageTrace(`${root}/diagnostics`, 'body-current-document');
    currentTrace.onPage(currentPage);
    await currentPage.goto(`http://127.0.0.1:${server.address().port}`);
    const currentFinished = currentPage.waitForEvent('requestfinished', request => request.url().endsWith('/api/v1/call'));
    await currentPage.evaluate(() => {
      void fetch('/api/v1/call', {method:'POST',headers:{'x-e2e-document':document.__aaaThreadTraceDocument,
        'x-e2e-connection':'1','x-e2e-request':'1'},body:JSON.stringify({op:'threads'})}).then(r=>r.json());
    });
    await currentFinished;
    await assert.rejects(currentTrace.reads([]), /current-document-body-failure/);
  } finally {
    releaseBody?.();
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
