// 터미널 치우기 차례(src/herdr/terminalDispose.ts). herdr-web-ui `src/lib/terminalDispose.test.ts`를 node:test로 옮겼다(바뀐 것은
// bun:test → node:assert뿐). xterm이 이미 걸어 둔 frame과 그 뒤 task가 돈 다음에 치워야, 치운 renderer를 읽는 TypeError가 없다.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { disposeAfterPendingFrame } from "../src/herdr/terminalDispose.ts";

/** Frames and tasks run only when requested by the test, in registration order. */
function scheduler() {
  const frames = [];
  const timers = [];
  return {
    requestAnimationFrame: (callback) => {
      frames.push(callback);
      return frames.length;
    },
    setTimeout: (callback) => {
      timers.push(callback);
      return timers.length;
    },
    frame: () => {
      for (const callback of frames.splice(0)) callback();
    },
    tick: () => {
      for (const callback of timers.splice(0)) callback();
    },
  };
}

describe("terminal disposal", () => {
  it("waits for the frame xterm already asked for, and a task after it", () => {
    const clock = scheduler();
    const order = [];
    clock.requestAnimationFrame(() => {
      order.push("viewport frame");
      clock.setTimeout(() => order.push("viewport task"));
    });

    disposeAfterPendingFrame({ dispose: () => order.push("dispose") }, clock);

    assert.deepEqual(order, []);
    clock.tick();
    assert.deepEqual(order, []);
    clock.frame();
    assert.deepEqual(order, ["viewport frame"]);
    clock.tick();
    assert.deepEqual(order, ["viewport frame", "viewport task", "dispose"]);
  });

  it("keeps the renderer alive for pending frame and constructor task reads", () => {
    const clock = scheduler();
    // Model RenderService's dimensions accessor: disposal removes its renderer.
    let renderer = { dimensions: 80 };
    const reads = [];
    const syncScrollArea = () => {
      if (renderer === undefined) throw new TypeError("renderer disposed before viewport sync");
      reads.push(renderer.dimensions);
    };
    clock.setTimeout(syncScrollArea);
    clock.requestAnimationFrame(syncScrollArea);

    disposeAfterPendingFrame(
      {
        dispose: () => {
          renderer = undefined;
        },
      },
      clock,
    );

    // A hidden tab can run tasks without delivering its animation frames.
    clock.tick();
    assert.deepEqual(reads, [80]);
    clock.frame();
    assert.deepEqual(reads, [80, 80]);
    clock.tick();
    assert.equal(renderer, undefined);
  });

  it("reproduces the pending viewport read failure with immediate disposal", () => {
    const clock = scheduler();
    let renderer = { dimensions: 80 };
    clock.requestAnimationFrame(() => {
      if (renderer === undefined) throw new TypeError("renderer disposed before viewport sync");
      return renderer.dimensions;
    });

    renderer = undefined;

    assert.throws(() => clock.frame(), TypeError);
  });

  it("disposes once", () => {
    const clock = scheduler();
    let disposed = 0;

    disposeAfterPendingFrame(
      {
        dispose: () => {
          disposed++;
        },
      },
      clock,
    );

    clock.frame();
    clock.tick();
    clock.frame();
    clock.tick();
    assert.equal(disposed, 1);
  });
});
