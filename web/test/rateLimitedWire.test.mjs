import assert from "node:assert/strict";
import test from "node:test";
import { RateLimitedWire } from "../e2e/lib/rate-limited-wire.mjs";

// Node처럼 소수 지연을 버린다. 두 번째 등록 전에 처리 시간이 지나 상대 지연의 정수 경계가 바뀐다.
function manualClock() {
  let now = 0, id = 0;
  const timers = new Map();
  const reads = [];
  return {
    now: () => { if (reads.length) now = reads.shift(); return now; },
    setTimeout(fn, delay) { const key = ++id; timers.set(key, { at: now + Math.max(1, Math.trunc(delay)), fn }); return key; },
    clearTimeout(key) { timers.delete(key); },
    reads(...values) { reads.push(...values); },
    advance(at) {
      for (;;) {
        const next = [...timers].sort((a, b) => a[1].at - b[1].at || a[0] - b[0])[0];
        if (!next || next[1].at > at) break;
        now = next[1].at; timers.delete(next[0]); next[1].fn();
      }
      now = at;
    },
    pending: () => timers.size,
    time: () => now,
  };
}

test("회선은 타이머 반올림·등록 지연에도 입력 8키를 FIFO로 전달한다", () => {
  const clock = manualClock(), received = [];
  const wire = new RateLimitedWire((data) => received.push(data), 1e6, 100, clock);
  clock.reads(0, 0); wire.push("a", 25);
  clock.reads(0.05, 0.5); wire.push("b", 25);
  for (const key of "cdefgh") wire.push(key, 25);
  clock.advance(100);
  assert.equal(received.join(""), "abcdefgh");
  assert.equal(clock.pending(), 0);
});

test("큰 history 뒤 binary·state는 순서를 지키고 반대 방향 입력은 독립이다", () => {
  const clock = manualClock(), received = [], input = [];
  const down = new RateLimitedWire((data) => received.push({ data, at: clock.time() }), 1e6, 100, clock);
  const up = new RateLimitedWire((data) => input.push(data), 1e6, 100, clock);
  down.push("history", 524274); down.push("binary", 47); down.push("state", 100);
  up.push("a", 25);
  clock.advance(100);
  assert.deepEqual(input, ["a"]); assert.deepEqual(received, []);
  clock.advance(5000);
  assert.deepEqual(received.map((item) => item.data), ["history", "binary", "state"]);
  assert(received[0].at >= 50 + 524274 * 8000 / 1e6);
  assert(received[1].at >= 50 + (524274 + 47) * 8000 / 1e6);
});

test("연결 종료는 대기 중인 자료와 타이머를 폐기한다", () => {
  const clock = manualClock(), received = [];
  const wire = new RateLimitedWire((data) => received.push(data), 1e6, 100, clock);
  wire.push("history", 500000); wire.push("binary", 47); wire.close(); wire.close();
  wire.push("late", 47);
  clock.advance(10000);
  assert.deepEqual(received, []); assert.equal(clock.pending(), 0);
});
