// 조합 상태는 xterm controller, 표시 좌표·글꼴·색은 이 adapter 하나가 소유한다(t-1075).
// xterm·herdr의 MIT 고지와 기존 실제 칸 크기·색 계약을 유지한다.
import type { Terminal } from "@xterm/xterm";
import { glyphFit } from "./herdr/terminalGlyphs";
import { terminalComposition } from "./terminalComposition";

const TRANSPARENT = /^(transparent|rgba\(0, 0, 0, 0\))$/;

/** (x, y)에 그려진 칸 요소(줄 안 글자 묶음, 없으면 그 줄). 자리로 찾는다: 터치 화면에서는 줄이 터치를 받지 않아(style.css
 *  `.xterm-rows { pointer-events: none }`, plan §8.55) elementsFromPoint가 줄을 건너뛴다. */
function cellAt(rows: HTMLElement, x: number, y: number): HTMLElement | undefined {
  const inside = (e: Element) => {
    const b = e.getBoundingClientRect();
    return b.left <= x && x < b.right && b.top <= y && y < b.bottom;
  };
  const row = [...rows.children].find(inside) as HTMLElement | undefined;
  if (!row) return undefined;
  return ([...row.children].find(inside) as HTMLElement | undefined) ?? row;
}

/** 조합 상자 아래 칸의 배경·글자색. 커서 칸은 커서 색이므로 옆 칸을 본다. 찾지 못하면 테마 색. */
function cellColors(term: Terminal, view: HTMLElement): { bg: string; fg: string } {
  const theme = term.options.theme ?? {};
  let bg = theme.background ?? "";
  let fg = theme.foreground ?? "";
  const rows = term.element?.querySelector<HTMLElement>(".xterm-rows");
  if (!rows) return { bg, fg };
  const r = view.getBoundingClientRect();
  let cell: HTMLElement | null | undefined = cellAt(rows, r.left + 1, r.top + r.height / 2);
  if (cell?.classList.contains("xterm-cursor")) {
    cell = (cell.previousElementSibling ?? cell.nextElementSibling) as HTMLElement | null;
  }
  // 배경은 칸에서 위로 올라가며 처음 칠해진 것(칸·줄에 없으면 터미널 상자)
  for (let e: HTMLElement | null | undefined = cell ?? rows; e && e !== document.documentElement; e = e.parentElement) {
    const c = getComputedStyle(e).backgroundColor;
    if (c && !TRANSPARENT.test(c)) {
      bg = c;
      break;
    }
  }
  if (cell && rows.contains(cell)) fg = getComputedStyle(cell).color || fg;
  return { bg, fg };
}

/** controller가 고른 줄과 열을 한 번에 적용한다. 다른 observer가 이 상자를 덮어쓰지 않는다. */
export function matchCompositionView(term: Terminal): () => void {
  const view = term.element?.querySelector<HTMLElement>(".composition-view");
  const screen = term.element?.querySelector<HTMLElement>(".xterm-screen");
  const rows = term.element?.querySelector<HTMLElement>(".xterm-rows");
  const input = term.textarea;
  const measure = document.createElement("canvas").getContext("2d");
  if (!view || !screen || !rows || !input || !measure) throw new Error("IME 표시 adapter를 연결할 수 없습니다");
  const controller = terminalComposition(term);
  const renderer = controller.setRenderer(state => {
    view.textContent = state.text;
    view.classList.toggle("active", state.active && !!state.text);
    view.style.visibility = state.visible ? "" : "hidden";
    if (!state.active || !state.visible) return;
    const { fontFamily, fontSize = 13, fontWeight } = term.options;
    const cellWidth = screen.getBoundingClientRect().width / term.cols;
    const row = rows.children[state.row];
    const rect = row?.getBoundingClientRect();
    const top = rect ? rect.top - screen.getBoundingClientRect().top : state.row * screen.getBoundingClientRect().height / term.rows;
    const height = rect?.height ?? screen.getBoundingClientRect().height / term.rows;
    const left = state.col * cellWidth;
    // herdr terminalGlyphs의 fitView 계산을 이 adapter로 이동했다(MIT). 실제 줄과 같은 폭 측정이다.
    let size = fontSize, spacing = 0;
    const first = String.fromCodePoint(state.text.codePointAt(0) ?? 0x20);
    const xtermMeasure = term.element?.querySelector<HTMLElement>(".xterm-width-cache-measure-container .xterm-char-measure-element");
    if (xtermMeasure && cellWidth > 0) {
      const held = xtermMeasure.textContent;
      xtermMeasure.textContent = first.repeat(32);
      const drawn = xtermMeasure.offsetWidth / 32;
      xtermMeasure.textContent = held;
      measure.font = `${fontWeight} ${fontSize}px ${fontFamily}`;
      const cells = drawn > cellWidth + 1 ? 2 : 1;
      const fit = glyphFit(cells * cellWidth - drawn, measure.measureText(first).width, cellWidth);
      if (fit) { size *= fit.scale; spacing = fit.spacing; }
    }
    Object.assign(view.style, {
      left: `${left}px`, top: `${top}px`, height: `${height}px`, lineHeight: `${height}px`,
      fontFamily, fontSize: `${size}px`, letterSpacing: `${spacing}px`, paddingLeft: `${spacing / 2}px`,
      border: "0", outline: "none", boxShadow: "none"
    });
    const { bg, fg } = cellColors(term, view);
    view.style.background = bg;
    view.style.color = fg;
    // 후보 창의 textarea도 같은 최종 좌표·높이를 쓴다(DPR의 실제 DOM 행 반올림 포함).
    const bounds = view.getBoundingClientRect();
    Object.assign(input.style, { left: `${left}px`, top: `${top}px`,
      width: `${Math.max(bounds.width, 1)}px`, height: `${Math.max(bounds.height, 1)}px`, lineHeight: `${bounds.height}px` });
  });
  return () => renderer.dispose();
}
