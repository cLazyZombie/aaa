// resize의 버퍼 변경·본문 render·native viewport는 서로 다른 차례에 반영된다.
// 바닥이라는 두 표지의 자체 일치만으로 옛 render를 현재 위치로 기억하지 않는다.
export function pageScrollStart() {
  const el = document.querySelector('[data-testid="terminal"]');
  const rows = el?.querySelector(".xterm-rows");
  const vp = el?.querySelector(".xterm-viewport");
  const screen = el?.querySelector(".xterm-screen");
  if (!el || !rows?.firstElementChild || !vp || !screen || !rows.childElementCount) return false;
  const data = el.dataset;
  const top = Number(data.viewTop), base = Number(data.viewBase);
  const cell = screen.getBoundingClientRect().height / rows.childElementCount;
  if (data.readonly !== "false" || data.fit !== "same" || data.fitPending ||
      document.querySelector(".term-scroll")?.hasAttribute("data-adopted-grid") ||
      !(base > 0 && top === base && base === Number(data.parsedBase) && cell > 0 &&
        Math.abs(vp.scrollTop - top * cell) <= 1 &&
        vp.scrollTop >= vp.scrollHeight - vp.clientHeight - 1 && rows.textContent.includes("s-200"))) return false;
  return {
    first: Number(/s-(\d{3})/.exec(rows.firstElementChild.textContent)?.[1]),
    rows: rows.childElementCount, top, base, nativeTop: vp.scrollTop,
    bottom: vp.scrollHeight - vp.clientHeight,
  };
}
