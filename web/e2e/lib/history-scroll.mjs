// waitForFunction의 rAF에서는 준비 상태만 읽는다. 같은 frame의 뒤쪽 xterm 동기화가
// scrollTop 쓰기를 native scroll 사건 전에 덮을 수 있으므로 여기서는 움직이지 않는다.
export function historyScrollReady(selector) {
  const box = document.querySelector(selector);
  const viewport = box?.querySelector('.xterm-viewport');
  const row = box?.querySelector('.xterm-rows > div');
  if (!box || !viewport || !row) return false;
  const data = box.dataset;
  if (data.readonly !== 'false' || data.fit !== 'same' || data.fitPending === '1' ||
      data.historyMore !== '1' || data.historyWaiting === '1') return false;
  const base = Number(data.viewBase);
  const top = Number(data.viewTop);
  const rowHeight = row.getBoundingClientRect().height;
  // 본문 renderer와 Viewport는 따로 반영된다. 아직 0인 scrollTop에 0을 쓰면 scroll 사건이 없다.
  // 크기 응답·본문·스크롤 영역이 같은 맨 아래 자리를 가리킨 뒤 실제 DOM 이동을 한 번 만든다.
  if (!(base > 0 && top === base && rowHeight > 0 && viewport.clientHeight > 0 &&
        viewport.scrollHeight > viewport.clientHeight && viewport.scrollTop > 0 &&
        Math.abs(viewport.scrollTop - top * rowHeight) <= 1)) return false;
  return viewport;
}

export async function scrollHistoryWhenReady(page, selector) {
  const ready = await page.waitForFunction(historyScrollReady, selector);
  try {
    // 별도 브라우저 task에서 한 번만 옮긴다. 다음 rendering의 native scroll 처리가
    // xterm의 rAF보다 먼저 버퍼 위치를 바꾼다. 고정 frame·sleep·재입력은 없다.
    return await ready.evaluate(viewport => {
      const box = viewport.closest('[data-testid="terminal"]');
      const before = { top: viewport.scrollTop, height: viewport.scrollHeight, client: viewport.clientHeight,
        rowHeight: box.querySelector('.xterm-rows > div').getBoundingClientRect().height, dataset: { ...box.dataset } };
      viewport.scrollTop = 0;
      return before;
    });
  } finally { await ready.dispose(); }
}
