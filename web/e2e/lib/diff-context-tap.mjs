// E42: 실제 focus 재조회로 처음 보인 가상 단추와 최종 터치 단추 사이를 고정한다.
export async function reflowContextButton(page, handle) {
  await handle.evaluate((button) => {
    const gate = window.__contextReflow = { before: { connected: button.isConnected, box: button.getBoundingClientRect().toJSON() }, events: [], detached: false };
    const record = () => {
      if (button.isConnected || gate.detached) return;
      gate.detached = true;
      gate.after = { connected: button.isConnected, box: button.getBoundingClientRect().toJSON() };
      gate.events.push({ event: "old-detached", at: performance.now() });
      observer.disconnect();
    };
    const observer = new MutationObserver(record);
    observer.observe(document.documentElement, { childList: true, subtree: true });
    observer.observe(button.getRootNode(), { childList: true, subtree: true });
    gate.cleanup = () => observer.disconnect();
    gate.events.push({ event: "old-resolved", at: performance.now() });
    // handle을 얻은 뒤 이미 제거됐으면 과거 MutationObserver 사건은 오지 않는다.
    // 관측을 설치한 같은 task에서 현재 상태도 읽어 그 제거를 놓치지 않는다.
    record();
  });
  try {
    const response = page.waitForResponse((r) => {
      if (r.request().method() !== "POST") return false;
      const body = r.request().postDataJSON();
      return body?.op === "thread_diff" && body.query?.patch && !body.query?.full_context;
    });
    await page.evaluate(() => {
      window.__contextReflow.events.push({ event: "focus-refresh", at: performance.now() });
      window.dispatchEvent(new Event("focus"));
    });
    const result = await response;
    await result.finished();
    if (!result.ok()) throw new Error(`맥락 재조회 HTTP ${result.status()}`);
    // 시간으로 쉬지 않는다. 실제 재그리기가 옛 단추를 제거한 사건으로 끝낸다.
    await page.waitForFunction(() => window.__contextReflow.detached, undefined, { timeout: 5000 });
    return await page.evaluate(() => {
      const { before, after, events, detached } = window.__contextReflow;
      return { before, after, events, detached };
    });
  } finally {
    await page.evaluate(() => window.__contextReflow?.cleanup());
  }
}

/** 자동화가 찾은 옛 handle 대신 실제 터치·click이 받은 같은 DOM 단추의 크기를 잰다. */
export async function tapContextButton(page, more) {
  await page.evaluate(() => {
    let down;
    const trace = window.__contextTap = { events: [] };
    const capture = (event) => {
      const button = event.composedPath().find((el) => el instanceof HTMLElement && el.matches('[data-expand-button][aria-label="위로 20줄 펼치기"]'));
      if (!button) return;
      if (event.type === "touchstart") down = button;
      trace.events.push({ event: event.type, at: performance.now(), connected: button.isConnected, same: down === button, box: button.getBoundingClientRect().toJSON() });
    };
    for (const type of ["touchstart", "touchend", "click"]) document.addEventListener(type, capture, true);
    trace.cleanup = () => { for (const type of ["touchstart", "touchend", "click"]) document.removeEventListener(type, capture, true); };
  });
  try {
    const old = await more.elementHandle();
    try { await reflowContextButton(page, old); } finally { await old.dispose(); }
    // 기존 실제 tap과 Playwright의 안정된 배치/히트 대상 확인을 그대로 쓴다(force·재시도 없음).
    await more.tap();
    const trace = await page.evaluate(() => window.__contextTap.events);
    if (trace.map((e) => e.event).join(",") !== "touchstart,touchend,click") throw new Error(`맥락 실제 터치 사건 ${JSON.stringify(trace)}`);
    for (const event of trace) {
      if (!event.connected || !event.same || event.box.width < 44 || event.box.height < 44) throw new Error(`맥락 터치 단추 ${JSON.stringify(event)}`);
    }
    return trace;
  } finally {
    await page.evaluate(() => window.__contextTap?.cleanup());
  }
}
