// 놀면 치우는 공유 자원(src/herdr/idleLease.ts, 가상화한 소스의 worker pool이 쓴다). herdr-web-ui `src/lib/idleLease.test.ts`
// (aa624cb)를 node:test로 옮겼다: bun:test → node:test와 bunExpect.mjs, 형 표기를 지움. 시계는 손으로 돌리는 timer다.
import { describe, it } from "node:test";

import { expect } from "./bunExpect.mjs";
import { createIdleLease } from "../src/herdr/idleLease.ts";

function manualTimers() {
  let next = 0;
  const queued = new Map();
  return {
    set: (run) => { next += 1; queued.set(next, run); return next; },
    clear: (handle) => { queued.delete(handle); },
    fire: () => { const runs = [...queued.values()]; queued.clear(); for (const run of runs) run(); },
    pending: () => queued.size,
  };
}

function setup() {
  const timers = manualTimers();
  let made = 0;
  const disposed = [];
  const lease = createIdleLease(() => { made += 1; return made; }, (value) => { disposed.push(value); }, 30_000, timers);
  return { lease, timers, disposed, made: () => made };
}

describe("createIdleLease", () => {
  it("is made lazily and shared by every holder", () => {
    const { lease, made } = setup();
    expect(lease.current()).toBeUndefined();
    expect(made()).toBe(0);
    expect(lease.get()).toBe(1);
    lease.hold();
    lease.hold();
    expect(lease.get()).toBe(1);
    expect(made()).toBe(1);
  });

  it("a holder that comes back before the idle time takes the same resource", () => {
    const { lease, timers, disposed, made } = setup();
    lease.get();
    lease.hold()();
    expect(timers.pending()).toBe(1);
    lease.get();
    const release = lease.hold();
    expect(timers.pending()).toBe(0);
    timers.fire();
    expect(disposed).toEqual([]);
    expect(made()).toBe(1);
    release();
  });

  it("is disposed only once the last holder has been gone the idle time, and made fresh after", () => {
    const { lease, timers, disposed } = setup();
    lease.get();
    const a = lease.hold();
    const b = lease.hold();
    a();
    expect(timers.pending()).toBe(0);
    b();
    expect(disposed).toEqual([]);
    timers.fire();
    expect(disposed).toEqual([1]);
    expect(lease.current()).toBeUndefined();
    expect(lease.get()).toBe(2);
  });

  it("a get nobody holds is disposed when idle", () => {
    const { lease, timers, disposed } = setup();
    lease.get();
    timers.fire();
    expect(disposed).toEqual([1]);
  });

  it("a second release of one hold counts once", () => {
    const { lease, timers, disposed } = setup();
    const a = lease.hold();
    const b = lease.hold();
    a();
    a();
    timers.fire();
    expect(disposed).toEqual([]);
    b();
    timers.fire();
    expect(disposed).toEqual([1]);
  });
});
