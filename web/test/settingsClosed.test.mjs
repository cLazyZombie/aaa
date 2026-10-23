import assert from 'node:assert/strict';
import test from 'node:test';
import vm from 'node:vm';
import { waitForSettingsExit } from '../e2e/lib/settings-closed.mjs';

function lab({ shown = true, entry = true, query = true } = {}) {
  const window = new EventTarget();
  let changed;
  let disconnected = false;
  const timers = new Map();
  const state = {
    shown,
    history: { state: entry ? { 'aaa:settings': { depth: 1 } } : {} },
    location: { search: query ? '?settings=chat' : '' },
  };
  const sandbox = {
    window, history: state.history, location: state.location, URLSearchParams,
    document: { querySelector: () => state.shown ? {} : null },
    MutationObserver: class {
      constructor(callback) { changed = callback; }
      observe() {}
      disconnect() { disconnected = true; }
    },
    setTimeout: (callback, ms) => { assert.equal(ms, 20_000); timers.set(1, callback); return 1; },
    clearTimeout: (id) => timers.delete(id),
  };
  const result = vm.runInNewContext(`(${waitForSettingsExit})(20000)`, sandbox);
  let done = false;
  result.then(() => { done = true; }, () => {});
  return {
    state, result, timers,
    changed: () => changed?.(),
    pop: () => window.dispatchEvent(new Event('popstate')),
    done: () => done, disconnected: () => disconnected,
  };
}

test('설정 DOM만 닫혔으면 기다리고 실제 popstate로 끝낸다', async () => {
  const l = lab();
  l.state.shown = false;
  l.changed();
  await Promise.resolve();
  assert.equal(l.done(), false);
  l.state.history.state = {};
  l.pop();
  await Promise.resolve();
  assert.equal(l.done(), false, 'query도 설정 밖으로 돌아와야 한다');
  l.state.location.search = '';
  l.pop();
  await l.result;
  assert.equal(l.done(), true);
  assert.equal(l.disconnected(), true);
  assert.equal(l.timers.size, 0);
});

test('popstate가 DOM보다 먼저 오면 DOM 제거 사건으로 끝낸다', async () => {
  const l = lab();
  l.state.history.state = {};
  l.state.location.search = '';
  l.pop();
  await Promise.resolve();
  assert.equal(l.done(), false);
  l.state.shown = false;
  l.changed();
  await l.result;
  assert.equal(l.timers.size, 0);
});

test('이미 도착했으면 사건이나 타이머를 더 만들지 않는다', async () => {
  const l = lab({ shown: false, entry: false, query: false });
  await l.result;
  assert.equal(l.timers.size, 0);
});

test('도착하지 않으면 원래 20초 한도로 실패하고 정리한다', async () => {
  const l = lab();
  const callback = l.timers.get(1);
  callback();
  await assert.rejects(l.result, /20000ms 안에 설정의 기록 이동이 끝나지 않았다/);
  assert.equal(l.disconnected(), true);
  assert.equal(l.timers.size, 0);
  l.pop();
});
