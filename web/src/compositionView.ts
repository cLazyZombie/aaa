// 조합 상자(xterm .composition-view)를 그 자리 터미널 칸에 맞춘다(사용자 지시 10-17, plan §8.37).
//
// herdr-web-ui(aa624cb)는 이 상자의 색·자리를 따로 정하지 않는다: xterm.css의 검은 배경·흰 글자, xterm이 계산한 자리 그대로다.
// 크기·자간·여백은 herdr-web-ui의 terminalGlyphs.ts fitView가 맞추고, 여기서는 두 가지를 더 한다.
// - 색: 배경 = 그 칸의 배경, 글자 = 그 칸의 글자색, 테두리 없음(밝은 테마·배경 있는 줄에서 검은 사각형이 보였다).
// - 세로 자리: xterm은 top을 `줄 번호 × cell 높이`로 계산한다. cell 높이가 1/64px로 나눠지지 않으면(DPR 3의 14.6667px 등)
//   브라우저가 쌓은 실제 줄은 조금씩 덜 내려가, 아래 줄일수록 상자가 그 줄보다 낮아진다. 그러면 상자 위로 커서
//   블록이 가는 줄로 보인다(사용자 캡처 12:05, iPhone 크기 맨 아래 줄 0.45px). 그래서 top·높이를 실제 커서 줄에 맞춘다.
import type { Terminal } from "@xterm/xterm";

const TRANSPARENT = /^(transparent|rgba\(0, 0, 0, 0\))$/;

/** 조합 상자 아래 칸의 배경·글자색. 커서 칸은 커서 색이므로 옆 칸을 본다. 찾지 못하면 테마 색. */
function cellColors(term: Terminal, view: HTMLElement): { bg: string; fg: string } {
  const theme = term.options.theme ?? {};
  let bg = theme.background ?? "";
  let fg = theme.foreground ?? "";
  const rows = term.element?.querySelector<HTMLElement>(".xterm-rows");
  if (!rows) return { bg, fg };
  const r = view.getBoundingClientRect();
  const under = document
    .elementsFromPoint(r.left + 1, r.top + r.height / 2)
    .find((e) => e !== view && rows.contains(e) && e !== rows) as HTMLElement | undefined;
  let cell: HTMLElement | null | undefined = under;
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

/** 커서가 있는 줄 요소의 화면(.xterm-screen) 안 top·높이. 커서가 화면 밖이면 없다. */
function cursorRow(term: Terminal): { top: number; height: number } | null {
  const screen = term.element?.querySelector<HTMLElement>(".xterm-screen");
  const rows = term.element?.querySelector<HTMLElement>(".xterm-rows");
  const b = term.buffer.active;
  const row = rows?.children[b.cursorY + b.baseY - b.viewportY];
  if (!screen || !(row instanceof HTMLElement)) return null;
  const r = row.getBoundingClientRect();
  return { top: r.top - screen.getBoundingClientRect().top, height: r.height };
}

/** 조합 상자 맞추기를 건다. 끌 함수를 돌려준다. */
export function matchCompositionView(term: Terminal): () => void {
  const view = term.element?.querySelector<HTMLElement>(".composition-view");
  if (!view) return () => {};
  const probe = document.createElement("span").style;
  type Key = "background" | "color" | "border" | "outline" | "boxShadow" | "top" | "height" | "lineHeight";
  const set = (k: Key, v: string) => {
    probe[k] = v;
    // 쓰기마다 이 observer가 다시 보므로 다를 때만 쓴다(브라우저가 저장한 모양으로 비교)
    if (view.style[k] !== probe[k]) view.style[k] = probe[k];
  };
  const fit = () => {
    if (!view.classList.contains("active")) return;
    const row = cursorRow(term);
    if (row) {
      set("top", `${row.top}px`);
      set("height", `${row.height}px`);
      set("lineHeight", `${row.height}px`);
    }
    const { bg, fg } = cellColors(term, view);
    set("background", bg);
    set("color", fg);
    set("border", "0");
    set("outline", "none");
    set("boxShadow", "none");
  };
  const obs = new MutationObserver(fit);
  obs.observe(view, { childList: true, characterData: true, subtree: true, attributes: true, attributeFilter: ["style", "class"] });
  return () => obs.disconnect();
}
