// 보관 정책은 제어한 시계·응답 gate로 검증한다. 실제 5분을 기다리지 않는다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { root, AA, aaa, step, browser, webkit, pairPage, domIs, until, waitOutput, inTime } from "../lib/harness.mjs";
import { frames, shown, go, prepare, line } from "../lib/chat-retention.mjs";
import { retentionFailure } from "../lib/retention-failure.mjs";
import { installRetentionTimers } from "../lib/retention-timers.mjs";
function instrument() {
  const listeners = new Map();
  window.retainedSockets = [];
  window.retainedSocketPeak = 0;
  const Native = window.WebSocket;
  window.WebSocket = class extends Native {
    constructor(url, protocols) {
      super(url, protocols);
      if (!String(url).includes('/panes/')) return;
      window.retainedSockets.push(this);
      this.addEventListener('open', () => {
        window.retainedSocketPeak = Math.max(window.retainedSocketPeak, window.retainedSockets.filter(s => s.readyState === WebSocket.OPEN).length);
      });
    }
  };
  const add = EventTarget.prototype.addEventListener, remove = EventTarget.prototype.removeEventListener;
  const lane = (target, type, options) => `${target === window ? 'window' : 'document'}:${type}:${Boolean(typeof options === 'boolean' ? options : options?.capture)}`;
  EventTarget.prototype.addEventListener = function(type, fn, options) {
    if (this === window || this === document) { const key = lane(this, type, options); if (!listeners.has(key)) listeners.set(key, new Set()); listeners.get(key).add(fn); }
    return add.call(this, type, fn, options);
  };
  EventTarget.prototype.removeEventListener = function(type, fn, options) {
    if (this === window || this === document) listeners.get(lane(this, type, options))?.delete(fn);
    return remove.call(this, type, fn, options);
  };
  window.retentionMetrics = () => ({ ...window.retentionTimerMetrics(), listeners: [...listeners.values()].reduce((n, v) => n + v.size, 0), listenerTypes: Object.fromEntries([...listeners].map(([k, v]) => [k, v.size])), terms: document.querySelectorAll('.term').length, nodes: document.querySelectorAll('*').length });
}
const term = (page, pane) => page.locator(`.term[data-pane="${pane}"]`);
const call = (page, req) => page.evaluate(async req => (await fetch('/api/v1/call', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(req) })).json(), req);
async function check(engine, width, fixture) {
  const requests = [], sizes = new Map(), timeline = [];
  const context = { engine: engine.browserType().name(), width, phase: 'pair', opening: 0, pane: null, budget: 0 };
  const directory = `${root}/diagnostics`;
  const file = `${directory}/chat-lifecycle-${context.engine}-${width}.jsonl`;
  fs.mkdirSync(directory, { recursive: true });
  const record = event => {
    const row = { at: Date.now(), ...context, ...event };
    timeline.push(row);
    return row;
  };
  const at = (phase, details = {}) => {
    if (!phase.startsWith('open-')) context.scenario = phase;
    Object.assign(context, details, { phase }); record({ event: 'phase' });
  };
  let writeFailed = false;
  const flush = () => {
    if (writeFailed) return;
    try { fs.writeFileSync(file, timeline.map(row => JSON.stringify(row)).join('\n') + '\n'); }
    catch (error) { writeFailed = true; throw error; }
  };
  // 합성 fixture의 본문은 앞 96자만 기록한다. 용량 시험의 수 MiB payload를 진단에 다시 복사하지 않는다.
  const summary = body => ({ history: body.ok?.history, before: body.ok?.before, messages: body.ok?.messages?.length,
    last: body.ok?.messages?.at(-1) ? { id: body.ok.messages.at(-1).id, role: body.ok.messages.at(-1).role, text: body.ok.messages.at(-1).text.slice(0, 96) } : null,
    error: body.error });
  let hold = null, page = null, budgetRecheck = false;
  try {
  const ph = await pairPage(engine, aaa("web", "pair").code, { width, height: 844 }, { serviceWorkers: "block", hash: "#tasks", init: { content: `(${installRetentionTimers})();(${instrument})();` }, setup: async browserContext => {
    await browserContext.addInitScript(keys => { for (const key of keys) localStorage.setItem(`aaa:view:${key}`, 'chat'); }, fixture.keys);
    await browserContext.route('**/api/v1/call', async route => {
      const req = route.request().postDataJSON();
      if (req?.op !== 'pane_chat') return route.continue();
      requests.push(req);
      const request = { request: requests.length, pane: req.pane, before: req.before ?? null, thinking: req.thinking, started: { ...context } };
      record({ event: 'request', ...request });
      // 이전 요청의 늦은 응답이 다음 활성화의 gate를 가져가지 않게 요청 진입 때 고정한다.
      const gate = hold?.pane === req.pane && !req.before ? hold : null;
      if (gate) hold = null;
      const response = await route.fetch();
      if (gate) { gate.arrive(); await gate.release; }
      const body = await response.json();
      record({ event: 'response', ...request, status: response.status(), ...summary(body) });
      if (body.ok?.messages?.length) body.ok.messages.at(-1).text = `${req.pane} ${body.ok.messages.at(-1).text}`;
      if (sizes.has(req.pane)) {
        // 실제 재검증 commit을 식별한다. 첫 응답의 본문만 보고 다음 단계로 가지 않는다.
        if (budgetRecheck && sizes.get(req.pane) === 3 * 1024 * 1024 && !req.before) {
          body.ok.messages.at(-1).text += ' · 용량 재검증 반영';
        }
        body.ok.before = null;
        // agent의 history:byte(20자리):block(10자리) 정렬을 지킨다. :budget은 재검증 병합 때 답 뒤로 간다.
        body.ok.messages = [{ role: 'thinking', id: `${body.ok.history}:00000000000000000000:0000000000`, text: '가'.repeat(sizes.get(req.pane)) }, body.ok.messages.at(-1)];
        record({ event: 'fulfill-start', ...request, budget: sizes.get(req.pane), ...summary(body) });
        return route.fulfill({ response, json: body });
      }
      record({ event: 'fulfill-start', ...request, budget: sizes.get(req.pane) ?? 0, ...summary(body) });
      return route.fulfill({ response, json: body });
    });
  } });
  page = ph.page;
  await page.clock.install();
  await page.clock.resume();
  await page.evaluate(() => window.installRetentionTimers());
  const ids = [fixture.coordinator, fixture.thread, ...fixture.extra];
  const open = async id => {
    at('open-navigation', { opening: context.opening + 1, pane: id, budget: sizes.get(id) ?? 0 });
    await go(page, `pane=${id}`);
    at('open-chat-ready');
    await term(page, id).locator('[data-testid="chat"][aria-busy="false"]').waitFor({ state: 'attached' });
    if (sizes.get(id) === 3 * 1024 * 1024) {
      at('open-budget-recheck');
      budgetRecheck = true;
      // 활성 대화의 backstop을 가상 시계로 진행하고 그 응답이 실제 그려진 사건을 기다린다.
      await page.clock.runFor(10_000);
      await term(page, id).locator('.chat-row').getByText('용량 재검증 반영', { exact: false }).waitFor();
      budgetRecheck = false;
    }
    at('open-latest-row');
    await term(page, id).locator('.chat-row:last-child').getByText('기록 2399 ·', { exact: false }).waitFor();
    at('open-controller');
    await term(page, id).locator('[data-testid="terminal"][data-readonly="false"]').waitFor({ state: 'attached' });
    at('open-frames');
    await frames(page);
    at('open-pane-text');
    assert.ok((await term(page, id).locator(".chat-row:last-child").textContent()).includes(id), "선택한 pane의 본문");
    record({ event: 'open-complete' });
  };
  at('lru');
  // coordinator 1 + 일반 3. 네 번째 일반 대화는 일반 LRU만 버린다.
  await open(ids[0]); await open(ids[1]); await open(ids[2]);
  assert.equal(await page.locator('.term').count(), 3);
  await open(ids[0]); await open(ids[3]);
  assert.equal(await page.locator(".term").count(), 4);
  await page.waitForFunction(() => window.retainedSockets.filter(s => s.readyState === WebSocket.OPEN).length === 4);
  await open(ids[4]);
  await page.waitForFunction(() => window.retainedSockets.filter(s => s.readyState === WebSocket.OPEN).length === 4);
  assert.equal(await term(page, ids[0]).count(), 1, "coordinator는 일반 LRU에서 제외");
  assert.equal(await term(page, ids[1]).count(), 0);
  assert.equal(await page.locator('.term:not([hidden])').count(), 1);
  assert.equal(await page.locator('.term[hidden]:not([inert])').count(), 0);
  at('revalidation');
  // 재검증 응답을 gate 뒤에 두고 본문·읽던 자리·초안이 먼저 보이는지 검사한다.
  await open(ids[0]);
  await term(page, ids[0]).locator('textarea[aria-label="메시지"]').fill('초안 유지');
  await page.evaluate(() => { window.originalChat = document.querySelector('.term:not([hidden]) .chat'); });
  await go(page, 'tasks'); await frames(page);
  let arrived, release;
  const arrival = new Promise(r => { arrived = r; });
  const released = new Promise(r => { release = r; });
  hold = { pane: ids[0], arrive: arrived, release: released };
  const from = requests.length;
  await go(page, `pane=${ids[0]}`);
  await arrival;
  try {
    await term(page, ids[0]).locator('.chat-row:last-child').getByText('기록 2399 ·', { exact: false }).waitFor();
    assert.ok(await page.evaluate(() => window.originalChat === document.querySelector('.term:not([hidden]) .chat')));
    assert.equal(await term(page, ids[0]).locator('textarea[aria-label="메시지"]').inputValue(), '초안 유지');
    assert.equal(requests.slice(from).filter(r => r.pane === ids[0] && !r.before).length, 1, '이번 활성화의 재검증 요청 한 개');
    // gate의 늦은 응답은 숨은 화면에 commit하지 않는다. 다음 활성화가 최신을 읽는다.
    await go(page, 'tasks'); await frames(page);
  } finally { release(); }
  const hiddenFrom = requests.length;
  const sentFrom = await page.evaluate(() => window.__aaaSent.length);
  at('hidden-input');
  // 등록한 타이머가 실행돼도 숨은 대화의 HTTP/입력이 생기지 않는다.
  await page.clock.runFor(11_000);
  assert.equal(requests.slice(hiddenFrom).filter(r => ids.includes(r.pane)).length, 0);
  await page.evaluate(() => { window.dispatchEvent(new Event('focus')); window.dispatchEvent(new Event('resize')); });
  await page.keyboard.press('Control+Enter'); await page.keyboard.press('Control+f');
  await frames(page);
  const sent = await page.evaluate(from => window.__aaaSent.slice(from), sentFrom);
  assert.equal(sent.filter(s => ['input', 'submit', 'resize', 'touch'].includes(s.t)).length, 0, `숨김 송신: ${JSON.stringify(sent)}`);
  at('expiry');
  // 보관 만료는 숨긴 시각 기준이다. 활성 대화는 남는다.
  await open(ids[0]); await go(page, 'tasks'); await frames(page);
  await page.clock.runFor(5 * 60_000); await frames(page);
  assert.equal(await term(page, ids[0]).count(), 0, "대화 만료/용량 퇴거");
  await page.waitForFunction(() => window.retainedSockets.every(s => s.readyState === WebSocket.CLOSED));
  assert.ok(await page.evaluate(() => window.retainedSocketPeak <= 4), "동시 pane 연결 최대 4");
  await open(ids[0]);
  at('width-restore');
  // 숨은 동안 폭이 바뀌어도 같은 row와 그 안의 거리를 복원한다.
  const readRow = shown(page).locator('.chat-row').nth(80);
  await readRow.evaluate(row => { const chat = row.closest('.chat'); chat.scrollTop += row.getBoundingClientRect().top - chat.getBoundingClientRect().top + 20; chat.dispatchEvent(new Event('scroll', { bubbles: true })); });
  await frames(page);
  const position = await readRow.evaluate(row => ({ id: row.dataset.chatId, offset: row.getBoundingClientRect().top - row.closest('.chat').getBoundingClientRect().top }));
  await go(page, 'tasks'); await frames(page);
  await page.setViewportSize({ width: width === 1280 ? 900 : 430, height: 844 });
  await open(ids[0]);
  const changedOffset = await shown(page).locator(`[data-chat-id="${position.id}"]`).evaluate(row => row.getBoundingClientRect().top - row.closest('.chat').getBoundingClientRect().top);
  assert.ok(Math.abs(changedOffset - position.offset) <= 1, `숨김 폭 변경 row 위치 ${position.offset} → ${changedOffset}`);
  await page.setViewportSize({ width, height: 844 }); await frames(page);
  at('round-trips');
  // 변경 없는 20회 왕복: 본문 한 개와 WS 한 개를 유지하고 과거 쪽을 다시 읽지 않는다.
  const olderFrom = requests.length, metrics = [];
  const cdp = engine.browserType().name() === "chromium" ? await page.context().newCDPSession(page) : null;
  for (let n = 0; n < 20; n++) {
    await go(page, 'tasks'); await frames(page); await open(ids[0]);
    if (n === 9 || n === 19) {
      if (cdp) await cdp.send('HeapProfiler.collectGarbage');
      // 정상 단발 예약이 완료·취소되는 사건을 기다린다. 전체 타이머 수가 맞을 때까지 기다리지는 않는다.
      const sample = await page.evaluate(async () => { await window.retentionTimersSettled(); return window.retentionMetrics(); });
      sample.heap = cdp ? await cdp.send('Runtime.getHeapUsage') : null;
      metrics.push(sample);
    }
  }
  console.log('retention-listener-diagnostic', JSON.stringify(metrics));
  assert.equal(metrics[1].listeners, metrics[0].listeners, '전역 listener 증가 없음');
  assert.equal(metrics[1].terms, metrics[0].terms, '보관 수 증가 없음');
  assert.ok(metrics[1].timers <= metrics[0].timers + 1, '타이머 수 증가가 멎음');
  console.log('retention-metrics', engine.browserType().name(), width, JSON.stringify(metrics));
  assert.equal(await page.locator('.term').count(), 1);
  assert.equal(requests.slice(olderFrom).filter(r => r.before).length, 0);
  at('budget-lru');
  // 3 MiB씩 세 대화는 8 MiB를 넘으므로 비활성 LRU부터 빠진다.
  await go(page, 'tasks'); await page.clock.runFor(5 * 60_000); await frames(page);
  for (const id of ids.slice(0, 3)) sizes.set(id, 1024 * 1024);
  await open(ids[0]); await open(ids[1]); await open(ids[2]);
  assert.equal(await term(page, ids[0]).count(), 0, "대화 만료/용량 퇴거");
  assert.equal(await page.locator('.term').count(), 2);
  at('budget-single');
  // 단독 9 MiB는 읽고, 떠나는 순간 제거한다.
  await go(page, 'tasks'); await page.clock.runFor(5 * 60_000); await frames(page);
  sizes.set(ids[0], 3 * 1024 * 1024);
  await open(ids[0]); assert.equal(await term(page, ids[0]).count(), 1);
  await go(page, 'tasks'); await frames(page); assert.equal(await term(page, ids[0]).count(), 0, "대화 만료/용량 퇴거");
  sizes.clear();
  at('transcript-append');
  // 숨은 동안 새 답·파일 교체·확인된 세션 교체는 기존 기록과 섞이지 않는다.
  const saved = fs.readFileSync(fixture.transcript, 'utf8');
  await open(ids[0]); await go(page, 'tasks'); await frames(page);
  fs.appendFileSync(fixture.transcript, line(2400));
  await go(page, `pane=${ids[0]}`);
  await shown(page).getByText('기록 2400 ·', { exact: false }).waitFor();
  await go(page, 'tasks'); await frames(page);
  at('transcript-replace');
  fs.writeFileSync(`${fixture.transcript}.replacement`, line(9999));
  fs.renameSync(`${fixture.transcript}.replacement`, fixture.transcript);
  await go(page, `pane=${ids[0]}`);
  await shown(page).getByText('기록 9999 ·', { exact: false }).waitFor();
  assert.equal(await shown(page).getByText('기록 2399 ·', { exact: false }).count(), 0);
  fs.writeFileSync(`${fixture.transcript}.replacement`, saved);
  fs.renameSync(`${fixture.transcript}.replacement`, fixture.transcript);
  await go(page, 'tasks'); await frames(page);
  at('session-replace');
  const payload = { ...JSON.parse(fixture.hook), session_id: `6e5d4c3b-2a19-4f08-8e7d-${String(Date.now()).slice(-12)}` };
  const hookFile = `${fixture.dir}/new-hook.sh`;
  fs.writeFileSync(hookFile, `'${AA}' hook --agent claude '${JSON.stringify(payload)}'\nprintf 'session-changed>\\n'\n`);
  aaa('pane', 'run', ids[0], `post-probe ${hookFile}`);
  await waitOutput(ids[0], 'session-changed>');
  await domIs(page, '확인된 세션 교체 퇴거', id => !document.querySelector(`.term[data-pane="${id}"]`), ids[0]);
  await open(ids[0]);
  at('thinking');
  // 표시 옵션 변경은 같은 화면 안의 데이터 초기화 경계를 사용한다.
  const thinkingFetch = page.waitForRequest(r => {
    if (!r.url().endsWith('/api/v1/call')) return false;
    const body = r.postDataJSON();
    return body?.op === 'pane_chat' && body.pane === ids[0] && body.thinking === true && !body.before;
  });
  assert.ok((await call(page, { op: 'preferences_set', settings: { showThinking: true }, pins: null })).ok);
  await thinkingFetch;
  assert.ok((await call(page, { op: 'preferences_set', settings: { showThinking: false }, pins: null })).ok);
  at('delete');
  // 삭제된 pane은 보관 목록에서도 즉시 사라진다.
  const label = `retention-delete-${engine.browserType().name()}-${width}`;
  aaa('tab', 'create', '--machine', 'labA', '--label', label, '--', '/bin/sh', '-c', '"$0" hook --agent claude "$1"; exec cat', AA, fixture.hook);
  const deleted = await until('삭제 시험 대화 hook', () => aaa('pane', 'list').panes.find(p => p.name === label && p.agent));
  await page.evaluate(p => localStorage.setItem(`aaa:view:${p.machine_id}:${p.id}`, 'chat'), deleted);
  await open(deleted.id); await go(page, 'tasks'); await frames(page);
  aaa('pane', 'kill', deleted.id);
  await until('삭제 pane 종료 사건', () => aaa('pane', 'list').panes.find(p => p.id === deleted.id)?.state !== 'running');
  aaa('pane', 'forget', deleted.id);
  await domIs(page, '삭제 pane 퇴거', id => !document.querySelector(`.term[data-pane="${id}"]`), deleted.id);
  at('logout');
  // 로그아웃 뒤 인증 실패 사건은 보관 화면 전체를 내린다.
  await page.evaluate(() => fetch('/api/v1/me', { method: 'DELETE' }));
  await call(page, { op: 'preferences' });
  await page.getByLabel('페어링 코드').waitFor();
  assert.equal(await page.locator('.term').count(), 0);
  assert.deepEqual(ph.errors, []);
  record({ event: 'complete' });
  flush();
  await page.context().close();
  } catch (error) {
    // 원래 locator·호출 줄·직전 사건을 renderer에 다시 묻기 전에 저장한다.
    // 이후 진단이나 파일 쓰기 실패가 최초 오류를 덮지 않는다.
    await retentionFailure(error, context, requests.slice(-10), event => {
      const row = record(event);
      console.log('chat-lifecycle-failure', JSON.stringify(row));
    }, () => inTime(page.evaluate(() => {
      const rect = el => el?.getBoundingClientRect().toJSON();
      const row = el => el ? { id: el.dataset.chatId, text: el.textContent.slice(0, 96), box: rect(el), display: getComputedStyle(el).display, visibility: getComputedStyle(el).visibility } : null;
      return { hash: location.hash, now: Date.now(), visible: document.visibilityState,
        terms: [...document.querySelectorAll('.term')].map(el => {
          const chat = el.querySelector('.chat'), rows = chat?.querySelectorAll('.chat-row');
          return { pane: el.dataset.pane, hidden: el.hidden, inert: el.inert, box: rect(el),
            chat: chat ? { busy: chat.getAttribute('aria-busy'), box: rect(chat), top: chat.scrollTop, height: chat.scrollHeight, client: chat.clientHeight,
              rows: rows.length, first: row(rows[0]), last: row(rows[rows.length - 1]), lastChild: row(chat.querySelector('.chat-row:last-child')),
              tail: [...(chat.querySelector('.chat-transcript')?.children ?? [])].slice(-4).map(child => ({ tag: child.tagName, className: child.className, id: child.getAttribute('data-testid') })),
              note: chat.querySelector('.chat-note')?.textContent, error: chat.querySelector('.banner')?.textContent } : null };
        }) };
    }), 5000, '대화 수명 실패 진단'), flush).finally(() => {
      try { flush(); }
      catch (writeError) { console.error('chat-lifecycle-write-failure', String(writeError)); }
    });
  }
}
export default async function () {
  let fixture, safari;
  try {
  await step("대화 수명: coordinator와 일반 네 pane 준비", async () => {
    fixture = await prepare(); fixture.extra = [];
    for (let n = 0; n < 3; n++) {
      const label = `retention-extra-${n}`;
      aaa('tab', 'create', '--machine', 'labA', '--label', label, '--', '/bin/sh', '-c', '"$0" hook --agent claude "$1"; exec cat', AA, fixture.hook);
      fixture.extra.push((await until('추가 대화 hook', () => aaa('pane', 'list').panes.find(p => p.name === label && p.agent))).id);
    }
    fixture.keys = aaa('pane', 'list').panes.map(p => `${p.machine_id}:${p.id}`);
  }, { always: true });
  await step("대화 수명: Chromium 1280 LRU·만료·숨김·응답·용량", async () => { await check(browser, 1280, fixture); });
  await step("대화 수명: Chromium 390 LRU·만료·숨김·응답·용량", async () => { await check(browser, 390, fixture); });
  await step("대화 수명: WebKit 1280 LRU·만료·숨김·응답·용량", async () => { safari = await webkit.launch({ headless: true }); await check(safari, 1280, fixture); });
  await step("대화 수명: WebKit 390 LRU·만료·숨김·응답·용량", async () => { await check(safari, 390, fixture); await safari.close(); });
  } finally {
    if (safari?.isConnected()) await safari.close();
  }
}
