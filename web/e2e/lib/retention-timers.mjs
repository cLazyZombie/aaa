// E2E104에서 관측한 단발 예약: ChatFind.useDebounced 100ms, Term ResizeObserver 120ms.
// 이 둘의 완료·취소 사건만 기다린다. 다른 타이머는 수량 검사에 그대로 남긴다.
export function installRetentionTimers(host = window) {
  const pending = new Map(), waiters = new Set();
  const transient = timer => timer.delay === 100 || timer.delay === 120;
  const notify = () => queueMicrotask(() => {
    if ([...pending.values()].some(transient)) return;
    for (const resolve of waiters) resolve();
    waiters.clear();
  });
  host.installRetentionTimers = () => {
    pending.clear();
    const timeout = host.setTimeout.bind(host), clear = host.clearTimeout.bind(host);
    host.setTimeout = (fn, ms, ...args) => {
      let id;
      id = timeout(() => {
        pending.delete(id);
        try { fn(...args); } finally { notify(); }
      }, ms);
      pending.set(id, { delay: ms, callback: String(fn), stack: new Error().stack });
      return id;
    };
    host.clearTimeout = id => { pending.delete(id); clear(id); notify(); };
  };
  host.retentionTimerMetrics = () => ({ timers: pending.size, pendingTimers: [...pending.values()] });
  host.retentionTimersSettled = () => new Promise(resolve => { waiters.add(resolve); notify(); });
}
