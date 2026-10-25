// 보관 정책은 제어한 시계·응답 gate로 검증한다. 실제 5분을 기다리지 않는다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { root, AA, aaa, step, browser, webkit, pairPage, domIs, until, waitOutput } from "../lib/harness.mjs";
import { frames, shown, go, prepare, line } from "../lib/chat-retention.mjs";
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
  const requests = [], sizes = new Map();
  let hold = null;
  const ph = await pairPage(engine, aaa("web", "pair").code, { width, height: 844 }, { serviceWorkers: "block", hash: "#tasks", init: { content: `(${installRetentionTimers})();(${instrument})();` }, setup: async context => {
    await context.addInitScript(keys => { for (const key of keys) localStorage.setItem(`aaa:view:${key}`, 'chat'); }, fixture.keys);
    await context.route('**/api/v1/call', async route => {
      const req = route.request().postDataJSON();
      if (req?.op !== 'pane_chat') return route.continue();
      requests.push(req);
      // 이전 요청의 늦은 응답이 다음 활성화의 gate를 가져가지 않게 요청 진입 때 고정한다.
      const gate = hold?.pane === req.pane && !req.before ? hold : null;
      if (gate) hold = null;
      const response = await route.fetch();
      if (gate) { gate.arrive(); await gate.release; }
      const body = await response.json();
      if (body.ok?.messages?.length) body.ok.messages.at(-1).text = `${req.pane} ${body.ok.messages.at(-1).text}`;
      if (sizes.has(req.pane)) {
        body.ok.before = null;
        body.ok.messages = [{ role: 'thinking', id: `${body.ok.history}:budget`, text: '가'.repeat(sizes.get(req.pane)) }, body.ok.messages.at(-1)];
        return route.fulfill({ response, json: body });
      }
      return route.fulfill({ response, json: body });
    });
  } });
  const page = ph.page;
  await page.clock.install();
  await page.clock.resume();
  await page.evaluate(() => window.installRetentionTimers());
  const ids = [fixture.coordinator, fixture.thread, ...fixture.extra];
  const open = async id => {
    await go(page, `pane=${id}`);
    try {
      await term(page, id).locator('[data-testid="chat"][aria-busy="false"]').waitFor({ state: 'attached' });
    } catch (error) {
      const state = await page.evaluate(() => [...document.querySelectorAll('.chat')].map(el => ({ busy: el.getAttribute('aria-busy'), top: el.scrollTop, height: el.scrollHeight, client: el.clientHeight, rows: el.querySelectorAll('.chat-row').length, note: el.querySelector('[data-testid="chat-history-start"]')?.textContent, style: el.getAttribute('style'), first: el.querySelector('.chat-row')?.getBoundingClientRect().toJSON() })));
      console.log('retention-open-failure', JSON.stringify(state), error.stack);
      throw error;
    }
    await term(page, id).locator('.chat-row:last-child').getByText('기록 2399 ·', { exact: false }).waitFor();
    await term(page, id).locator('[data-testid="terminal"][data-readonly="false"]').waitFor({ state: 'attached' });
    await frames(page);
    assert.ok((await term(page, id).locator(".chat-row:last-child").textContent()).includes(id), "선택한 pane의 본문");
  };
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
  // 등록한 타이머가 실행돼도 숨은 대화의 HTTP/입력이 생기지 않는다.
  await page.clock.runFor(11_000);
  assert.equal(requests.slice(hiddenFrom).filter(r => ids.includes(r.pane)).length, 0);
  await page.evaluate(() => { window.dispatchEvent(new Event('focus')); window.dispatchEvent(new Event('resize')); });
  await page.keyboard.press('Control+Enter'); await page.keyboard.press('Control+f');
  await frames(page);
  const sent = await page.evaluate(from => window.__aaaSent.slice(from), sentFrom);
  assert.equal(sent.filter(s => ['input', 'submit', 'resize', 'touch'].includes(s.t)).length, 0, `숨김 송신: ${JSON.stringify(sent)}`);
  // 보관 만료는 숨긴 시각 기준이다. 활성 대화는 남는다.
  await open(ids[0]); await go(page, 'tasks'); await frames(page);
  await page.clock.runFor(5 * 60_000); await frames(page);
  assert.equal(await term(page, ids[0]).count(), 0, "대화 만료/용량 퇴거");
  await page.waitForFunction(() => window.retainedSockets.every(s => s.readyState === WebSocket.CLOSED));
  assert.ok(await page.evaluate(() => window.retainedSocketPeak <= 4), "동시 pane 연결 최대 4");
  await open(ids[0]);
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
  // 3 MiB씩 세 대화는 8 MiB를 넘으므로 비활성 LRU부터 빠진다.
  await go(page, 'tasks'); await page.clock.runFor(5 * 60_000); await frames(page);
  for (const id of ids.slice(0, 3)) sizes.set(id, 1024 * 1024);
  await open(ids[0]); await open(ids[1]); await open(ids[2]);
  assert.equal(await term(page, ids[0]).count(), 0, "대화 만료/용량 퇴거");
  assert.equal(await page.locator('.term').count(), 2);
  // 단독 9 MiB는 읽고, 떠나는 순간 제거한다.
  await go(page, 'tasks'); await page.clock.runFor(5 * 60_000); await frames(page);
  sizes.set(ids[0], 3 * 1024 * 1024);
  await open(ids[0]); assert.equal(await term(page, ids[0]).count(), 1);
  await go(page, 'tasks'); await frames(page); assert.equal(await term(page, ids[0]).count(), 0, "대화 만료/용량 퇴거");
  sizes.clear();
  // 숨은 동안 새 답·파일 교체·확인된 세션 교체는 기존 기록과 섞이지 않는다.
  const saved = fs.readFileSync(fixture.transcript, 'utf8');
  await open(ids[0]); await go(page, 'tasks'); await frames(page);
  fs.appendFileSync(fixture.transcript, line(2400));
  await go(page, `pane=${ids[0]}`);
  await shown(page).getByText('기록 2400 ·', { exact: false }).waitFor();
  await go(page, 'tasks'); await frames(page);
  fs.writeFileSync(`${fixture.transcript}.replacement`, line(9999));
  fs.renameSync(`${fixture.transcript}.replacement`, fixture.transcript);
  await go(page, `pane=${ids[0]}`);
  await shown(page).getByText('기록 9999 ·', { exact: false }).waitFor();
  assert.equal(await shown(page).getByText('기록 2399 ·', { exact: false }).count(), 0);
  fs.writeFileSync(`${fixture.transcript}.replacement`, saved);
  fs.renameSync(`${fixture.transcript}.replacement`, fixture.transcript);
  await go(page, 'tasks'); await frames(page);
  const payload = { ...JSON.parse(fixture.hook), session_id: `6e5d4c3b-2a19-4f08-8e7d-${String(Date.now()).slice(-12)}` };
  const hookFile = `${fixture.dir}/new-hook.sh`;
  fs.writeFileSync(hookFile, `'${AA}' hook --agent claude '${JSON.stringify(payload)}'\nprintf 'session-changed>\\n'\n`);
  aaa('pane', 'run', ids[0], `post-probe ${hookFile}`);
  await waitOutput(ids[0], 'session-changed>');
  await domIs(page, '확인된 세션 교체 퇴거', id => !document.querySelector(`.term[data-pane="${id}"]`), ids[0]);
  await open(ids[0]);
  // 표시 옵션 변경은 같은 화면 안의 데이터 초기화 경계를 사용한다.
  assert.ok((await call(page, { op: 'preferences_set', settings: { showThinking: true }, pins: null })).ok);
  await domIs(page, '생각 표시 옵션', () => document.querySelector('.term:not([hidden]) .chat-note')?.textContent.includes('도구 결과는'));
  assert.ok((await call(page, { op: 'preferences_set', settings: { showThinking: false }, pins: null })).ok);
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
  // 로그아웃 뒤 인증 실패 사건은 보관 화면 전체를 내린다.
  await page.evaluate(() => fetch('/api/v1/me', { method: 'DELETE' }));
  await call(page, { op: 'preferences' });
  await page.getByLabel('페어링 코드').waitFor();
  assert.equal(await page.locator('.term').count(), 0);
  assert.deepEqual(ph.errors, []);
  await page.context().close();
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
