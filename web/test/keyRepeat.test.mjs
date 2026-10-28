// 키 막대 꾹 누름 되풀이(src/herdr/keyRepeat.ts, herdr-web-ui `src/lib/keyRepeat.test.ts`를 옮겼다). 타이머로 늦게 보내는 일의
// 「없음」(뗀 뒤 한 번 더, 밀어서 취소한 누름)은 E2E가 sleep으로 어림하지 않고 여기서 가짜 시계로 본다(docs/testing-rules.md 「웹 E2E」).
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  createKeyRepeat, isRepeatableKeyBarItem, KEY_REPEAT_DELAY_MS, KEY_REPEAT_INTERVAL_MS, KEY_REPEAT_MOVE_SLOP_PX,
} from "../src/herdr/keyRepeat.ts";

function fakeClock() {
  let now = 0;
  let nextId = 0;
  const timers = new Map();
  const clock = {
    setTimeout(callback, delay) {
      const id = ++nextId;
      timers.set(id, { at: now + delay, callback });
      return id;
    },
    clearTimeout(id) {
      timers.delete(id);
    },
  };
  return {
    clock,
    now: () => now,
    pending: () => timers.size,
    advance(ms) {
      const end = now + ms;
      while (true) {
        let next;
        for (const [id, timer] of timers) {
          if (timer.at <= end && (!next || timer.at < next.timer.at)) next = { id, timer };
        }
        if (!next) break;
        now = next.timer.at;
        timers.delete(next.id);
        next.timer.callback();
      }
      now = end;
    },
  };
}

function fixture() {
  const clock = fakeClock();
  const sends = [];
  const repeat = createKeyRepeat(() => sends.push(clock.now()), clock.clock);
  return { clock, repeat, sends };
}

describe("키 막대 꾹 누름 되풀이", () => {
  it("짧게 누르면 click에서 한 번만 보내고, 누름이나 기다림 타이머로는 보내지 않는다", () => {
    const { clock, repeat, sends } = fixture();
    repeat.press(20, 30);
    assert.deepEqual(sends, []);
    clock.advance(KEY_REPEAT_DELAY_MS - 1);
    repeat.release();
    clock.advance(KEY_REPEAT_DELAY_MS + KEY_REPEAT_INTERVAL_MS);
    assert.deepEqual(sends, []);
    assert.equal(clock.pending(), 0);
    if (repeat.takeClick()) sends.push(clock.now());
    assert.equal(sends.length, 1);
  });

  it("기다림 경계에서 처음 보내고 그 뒤 간격 경계마다 보낸다", () => {
    const { clock, repeat, sends } = fixture();
    repeat.press(0, 0);
    clock.advance(KEY_REPEAT_DELAY_MS - 1);
    assert.deepEqual(sends, []);
    clock.advance(1);
    assert.deepEqual(sends, [KEY_REPEAT_DELAY_MS]);
    clock.advance(KEY_REPEAT_INTERVAL_MS - 1);
    assert.equal(sends.length, 1);
    clock.advance(1);
    assert.deepEqual(sends, [KEY_REPEAT_DELAY_MS, KEY_REPEAT_DELAY_MS + KEY_REPEAT_INTERVAL_MS]);
    clock.advance(2 * KEY_REPEAT_INTERVAL_MS);
    assert.deepEqual(sends, [
      KEY_REPEAT_DELAY_MS, KEY_REPEAT_DELAY_MS + KEY_REPEAT_INTERVAL_MS,
      KEY_REPEAT_DELAY_MS + 2 * KEY_REPEAT_INTERVAL_MS, KEY_REPEAT_DELAY_MS + 3 * KEY_REPEAT_INTERVAL_MS,
    ]);
  });

  it("떼면 앞으로 보낼 것을 모두 멈추고, 뒤따르는 click은 한 번만 삼킨다", () => {
    const { clock, repeat, sends } = fixture();
    repeat.press(0, 0);
    clock.advance(KEY_REPEAT_DELAY_MS + KEY_REPEAT_INTERVAL_MS);
    repeat.release();
    repeat.release();
    clock.advance(10 * KEY_REPEAT_INTERVAL_MS);
    assert.equal(sends.length, 2);
    assert.equal(clock.pending(), 0);
    assert.equal(repeat.takeClick(), false);
    assert.equal(repeat.takeClick(), true);

    repeat.press(0, 0);
    clock.advance(KEY_REPEAT_DELAY_MS - 1);
    repeat.release();
    if (repeat.takeClick()) sends.push(clock.now());
    assert.equal(sends.length, 3);
  });

  it("새로 누르면 앞 꾹 누름의 click 삼킴을 지운다", () => {
    const { clock, repeat, sends } = fixture();
    repeat.press(0, 0);
    clock.advance(KEY_REPEAT_DELAY_MS);
    repeat.release();
    // 브라우저는 첫 꾹 누름 뒤에 click을 주지 않을 수 있다
    repeat.press(0, 0);
    clock.advance(KEY_REPEAT_DELAY_MS - 1);
    repeat.release();
    assert.equal(repeat.takeClick(), true);
    assert.equal(sends.length, 1);
  });

  it("누름 없이 온 보통 click은 통과한다", () => {
    const { repeat } = fixture();
    assert.equal(repeat.takeClick(), true);
    assert.equal(repeat.takeClick(), true);
    repeat.release();
    assert.equal(repeat.takeClick(), true);
  });

  it("기다림 전에 취소하면 보내지 않고 타이머도 남기지 않는다", () => {
    const { clock, repeat, sends } = fixture();
    repeat.press(0, 0);
    clock.advance(KEY_REPEAT_DELAY_MS - 1);
    repeat.cancel();
    repeat.cancel();
    repeat.release();
    clock.advance(2 * KEY_REPEAT_DELAY_MS);
    assert.deepEqual(sends, []);
    assert.equal(clock.pending(), 0);
    assert.equal(repeat.takeClick(), true);
  });

  it("되풀이 중에 취소하면 더 보내지 않는다", () => {
    const { clock, repeat, sends } = fixture();
    repeat.press(0, 0);
    clock.advance(KEY_REPEAT_DELAY_MS);
    repeat.cancel();
    repeat.release();
    clock.advance(2 * KEY_REPEAT_DELAY_MS);
    assert.equal(sends.length, 1);
    assert.equal(clock.pending(), 0);
    assert.equal(repeat.takeClick(), true);
  });

  it("되풀이 전에는 허용 거리까지 움직여도 두고, 넘으면 취소한다", () => {
    const { clock, repeat, sends } = fixture();
    repeat.press(20, 30);
    assert.equal(repeat.move(20 + KEY_REPEAT_MOVE_SLOP_PX, 30), false);
    clock.advance(KEY_REPEAT_DELAY_MS - 1);
    assert.equal(repeat.move(20 + KEY_REPEAT_MOVE_SLOP_PX + 1, 30), true);
    assert.equal(repeat.move(20 + KEY_REPEAT_MOVE_SLOP_PX + 2, 30), false);
    repeat.release();
    clock.advance(KEY_REPEAT_DELAY_MS);
    assert.deepEqual(sends, []);
    assert.equal(clock.pending(), 0);
  });

  it("허용 거리는 처음 누른 곳에서 두 축으로 잰다", () => {
    const { clock, repeat, sends } = fixture();
    repeat.press(20, 30);
    assert.equal(repeat.move(20, 30 - KEY_REPEAT_MOVE_SLOP_PX), false);
    assert.equal(repeat.move(20 + KEY_REPEAT_MOVE_SLOP_PX, 31), true);
    clock.advance(KEY_REPEAT_DELAY_MS);
    assert.deepEqual(sends, []);
  });

  it("되풀이가 시작된 뒤에는 움직여도 취소하지 않는다", () => {
    const { clock, repeat, sends } = fixture();
    repeat.press(0, 0);
    clock.advance(KEY_REPEAT_DELAY_MS);
    assert.equal(repeat.move(10 * KEY_REPEAT_MOVE_SLOP_PX, 10 * KEY_REPEAT_MOVE_SLOP_PX), false);
    clock.advance(KEY_REPEAT_INTERVAL_MS);
    assert.equal(sends.length, 2);
    repeat.release();
    assert.equal(repeat.takeClick(), false);
  });

  it("새로 누르면 기다림을 다시 시작하고 앞 타이머를 버린다", () => {
    const { clock, repeat, sends } = fixture();
    repeat.press(0, 0);
    clock.advance(KEY_REPEAT_DELAY_MS - 1);
    repeat.press(10, 20);
    clock.advance(KEY_REPEAT_DELAY_MS - 1);
    assert.deepEqual(sends, []);
    clock.advance(1);
    assert.deepEqual(sends, [2 * KEY_REPEAT_DELAY_MS - 1]);
    assert.equal(clock.pending(), 1);
  });

  it("보내는 중에 그 꾹 누름을 떼면 다음 tick을 걸지 않는다", () => {
    const clock = fakeClock();
    let sends = 0;
    let repeat;
    repeat = createKeyRepeat(() => {
      sends++;
      repeat.release();
    }, clock.clock);
    repeat.press(0, 0);
    clock.advance(KEY_REPEAT_DELAY_MS + 10 * KEY_REPEAT_INTERVAL_MS);
    assert.equal(sends, 1);
    assert.equal(clock.pending(), 0);
    assert.equal(repeat.takeClick(), false);
  });
});

describe("되풀이하는 키 막대 항목", () => {
  it("화살표와 편집·쪽 키를 되풀이한다(저장한 보조 키가 있어도)", () => {
    for (const key of ["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Backspace", "Delete", "Home", "End", "PageUp", "PageDown"]) {
      assert.equal(isRepeatableKeyBarItem({ type: "key", key }), true);
      assert.equal(isRepeatableKeyBarItem({ type: "key", key, modifiers: { ctrl: true, alt: true, shift: true } }), true);
      assert.equal(isRepeatableKeyBarItem({ type: "key", key, modifiers: { ctrl: false, alt: false, shift: false } }), true);
    }
  });

  it("목록 키·글자 키·보조 키 단추는 되풀이하지 않는다", () => {
    for (const key of ["Escape", "Tab", "BackTab", "Enter", "Insert", "ctrl-c", "ctrl-d", "F1", "a", " "]) {
      assert.equal(isRepeatableKeyBarItem({ type: "key", key }), false);
      assert.equal(isRepeatableKeyBarItem({ type: "key", key, modifiers: { ctrl: true, alt: false, shift: false } }), false);
    }
    for (const modifier of ["ctrl", "alt", "shift"]) {
      assert.equal(isRepeatableKeyBarItem({ type: "modifier", modifier }), false);
    }
  });
});
