import assert from "node:assert/strict";
import test from "node:test";
import { installRetentionTimers } from "../e2e/lib/retention-timers.mjs";

function clock() {
  let next = 0;
  const callbacks = new Map();
  const host = {
    setTimeout(fn) { const id = ++next; callbacks.set(id, fn); return id; },
    clearTimeout(id) { callbacks.delete(id); },
  };
  installRetentionTimers(host);
  host.installRetentionTimers();
  return { host, fire(id) { const fn = callbacks.get(id); assert.ok(fn); callbacks.delete(id); fn(); } };
}

test("정상 단발 예약의 실행·취소를 기다리고 다른 예약은 수량에 남긴다", async () => {
  const { host, fire } = clock();
  host.setTimeout(() => {}, 1974);
  const find = host.setTimeout(() => {}, 100);
  const fit = host.setTimeout(() => {}, 120);
  const before = host.retentionTimerMetrics();
  assert.equal(before.timers, 3);
  let settled = false;
  const done = host.retentionTimersSettled().then(() => { settled = true; });
  fire(find);
  await Promise.resolve();
  assert.equal(settled, false);
  host.clearTimeout(fit);
  await done;
  assert.equal(host.retentionTimerMetrics().timers, 1);
});

test("완료 callback이나 취소 뒤 같은 사건에서 새로 건 단발 예약도 기다린다", async () => {
  const { host, fire } = clock();
  let replacement;
  const first = host.setTimeout(() => { replacement = host.setTimeout(() => {}, 120); }, 100);
  let settled = false;
  const done = host.retentionTimersSettled().then(() => { settled = true; });
  fire(first);
  await Promise.resolve();
  assert.equal(settled, false);
  host.clearTimeout(replacement);
  replacement = host.setTimeout(() => {}, 100);
  await Promise.resolve();
  assert.equal(settled, false);
  fire(replacement);
  await done;
  assert.equal(host.retentionTimerMetrics().timers, 0);
});

test("단발 예약이 끝나도 남은 누수는 기다려 숨기지 않고 기존 증가 단언이 검출한다", async () => {
  const { host, fire } = clock();
  host.setTimeout(() => {}, 1974);
  const before = host.retentionTimerMetrics();
  const find = host.setTimeout(() => {}, 100);
  host.setTimeout(() => {}, 5000);
  host.setTimeout(() => {}, 5000);
  const done = host.retentionTimersSettled();
  fire(find);
  await done;
  const after = host.retentionTimerMetrics();
  assert.equal(after.timers, 3);
  assert.throws(() => assert.ok(after.timers <= before.timers + 1));
  assert.ok(after.pendingTimers.every(timer => timer.stack.includes("retentionTimers.test.mjs")));
});

test("단발 지연값으로 계속 다시 예약하는 타이머는 정착한 것으로 보지 않는다", async () => {
  const { host, fire } = clock();
  let id;
  const repeat = () => { id = host.setTimeout(repeat, 100); };
  repeat();
  let settled = false;
  const done = host.retentionTimersSettled().then(() => { settled = true; });
  fire(id);
  await Promise.resolve();
  assert.equal(settled, false);
  fire(id);
  await Promise.resolve();
  assert.equal(settled, false);
  host.clearTimeout(id);
  await done;
});
