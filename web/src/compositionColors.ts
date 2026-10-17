// 조합 상자(xterm .composition-view)의 색을 그 자리 터미널 칸과 맞춘다(사용자 지시 10-17, plan §8.37).
//
// herdr-web-ui(aa624cb)는 이 상자 색을 정하지 않는다: xterm.css의 검은 배경·흰 글자 그대로라, 밝은 테마나
// 배경이 있는 줄(입력줄)에서 조합 중인 글자가 검은 사각형으로 보인다. 크기·자간·여백은 herdr-web-ui의
// terminalGlyphs.ts fitView가 맞추고, 여기서는 색만 바꾼다(배경 = 그 칸의 배경, 글자 = 그 칸의 글자색, 테두리 없음).
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

/** 색 맞추기를 건다. 끌 함수를 돌려준다. */
export function matchCompositionColors(term: Terminal): () => void {
  const view = term.element?.querySelector<HTMLElement>(".composition-view");
  if (!view) return () => {};
  const probe = document.createElement("span").style;
  const set = (k: "background" | "color" | "border" | "outline" | "boxShadow", v: string) => {
    probe[k] = v;
    // 쓰기마다 이 observer가 다시 보므로 다를 때만 쓴다(브라우저가 저장한 모양으로 비교)
    if (view.style[k] !== probe[k]) view.style[k] = probe[k];
  };
  const fit = () => {
    if (!view.classList.contains("active")) return;
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
