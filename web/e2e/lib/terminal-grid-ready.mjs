// page.evaluate/domIs에서 실행한다. 설정을 가린 동안의 fit="same"·ACK·행 수는
// 서로 맞더라도 복귀한 실제 칸보다 클 수 있다. ResizeObserver가 오기 전에도 거절한다.
export function fittedTerminalGrid() {
  const term = document.querySelector('[data-testid="terminal"]');
  const rows = term?.querySelector('.xterm-rows');
  const row = rows?.firstElementChild;
  const xterm = term?.querySelector('.xterm');
  const title = term?.closest('.term')?.querySelector('.term-title .muted');
  const size = /(\d+)×(\d+)/.exec(title?.textContent ?? '');
  if (!term || !row || !xterm || !size || term.dataset.fit !== 'same' || term.hasAttribute('data-fit-pending')) return false;
  const [cols, height] = size.slice(1).map(Number);
  const parentStyle = getComputedStyle(term), style = getComputedStyle(xterm);
  const available = parseInt(parentStyle.height) - parseInt(style.paddingTop) - parseInt(style.paddingBottom);
  const cell = row.getBoundingClientRect().height;
  if (!(cell > 0) || rows.childElementCount !== height || Math.floor(available / cell) !== height) return false;
  return { cols, rows: height };
}
