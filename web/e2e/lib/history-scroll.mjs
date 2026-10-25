// waitForFunction의 rAF에서 호출한다. 준비 전에는 입력하지 않고, 준비된 한 번만 맨 위로 옮긴다.
export function scrollHistoryWhenReady(selector) {
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
  const before = { top: viewport.scrollTop, height: viewport.scrollHeight, client: viewport.clientHeight,
    rowHeight, dataset: { ...data } };
  viewport.scrollTop = 0;
  return before;
}
