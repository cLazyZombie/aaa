// 대화 입력창을 끌어도 화면이 통째로 따라가지 않게 한다(사용자 iPad 10-18 「입력창 왼쪽을 잡고 올리면 창이 통째로 올라간다」).
// iOS는 굴러갈 것이 없는 자리(입력창 둘레 띠·짧은 글 칸)를 끌면 문서를 고무줄처럼 끌고, 화면 키보드가 올라온 동안은 문서를 굴린다.
// html·body `overscroll-behavior-y: none`(style.css)이 고무줄을 막지만 iOS가 그것을 늘 따르는지는 headless로 볼 수 없다: 그래서
// 입력창 안에서 시작한 한 손가락 끌기가 그 방향으로 굴러갈 것(넘치는 글 칸·가로로 넘치는 빠른 답장 띠)을 지나지 않으면 막는다.
// 참고 herdr-web-ui에는 없는 막음이다(참고는 html·body `overscroll-behavior-y: contain`만, 당겨 새로 읽기 막기).
// 막지 않는 것: 두 손가락(확대), 글을 고르는 중(고른 곳 손잡이를 끈다), 굴러갈 수 있는 것 위의 끌기.

/** el이 손가락이 (dx, dy)만큼 움직일 때 더 굴러갈 수 있다(손가락이 위로 가면 글은 아래로 간다) */
function canScroll(el: Element, dx: number, dy: number): boolean {
  const css = getComputedStyle(el);
  const field = el instanceof HTMLTextAreaElement;
  if (Math.abs(dy) >= Math.abs(dx)) {
    if (!field && !/(auto|scroll)/.test(css.overflowY)) return false;
    return dy < 0 ? el.scrollTop + el.clientHeight < el.scrollHeight - 1 : el.scrollTop > 0;
  }
  if (!field && !/(auto|scroll)/.test(css.overflowX)) return false;
  return dx < 0 ? el.scrollLeft + el.clientWidth < el.scrollWidth - 1 : el.scrollLeft > 0;
}

/** node 안에서 시작한 끌기 중 굴러갈 것이 없는 것은 문서로 넘기지 않는다. 돌려준 함수가 뗀다 */
export function holdPage(node: HTMLElement): () => void {
  let last: { x: number; y: number } | null = null;
  const onStart = (event: TouchEvent): void => {
    const touch = event.touches[0];
    last = event.touches.length === 1 && touch ? { x: touch.clientX, y: touch.clientY } : null;
  };
  const onMove = (event: TouchEvent): void => {
    const touch = event.touches[0];
    if (last === null || event.touches.length !== 1 || !touch || !event.cancelable) return;
    if ((window.getSelection()?.toString() ?? "").length > 0) return;
    const dx = touch.clientX - last.x;
    const dy = touch.clientY - last.y;
    last = { x: touch.clientX, y: touch.clientY };
    if (dx === 0 && dy === 0) return;
    for (let at = event.target instanceof Element ? event.target : null; at !== null; at = at.parentElement) {
      if (canScroll(at, dx, dy)) return;
      if (at === node) break;
    }
    event.preventDefault();
  };
  const onEnd = (): void => {
    last = null;
  };
  node.addEventListener("touchstart", onStart, { passive: true });
  // passive면 preventDefault가 듣지 않는다(React onTouchMove는 passive로 붙는다)
  node.addEventListener("touchmove", onMove, { passive: false });
  node.addEventListener("touchend", onEnd);
  node.addEventListener("touchcancel", onEnd);
  return () => {
    node.removeEventListener("touchstart", onStart);
    node.removeEventListener("touchmove", onMove);
    node.removeEventListener("touchend", onEnd);
    node.removeEventListener("touchcancel", onEnd);
  };
}

/** ref 자리에 둔다(React 19는 ref 함수가 돌려준 정리를 뗄 때 부른다) */
export const holdPageRef = (node: HTMLElement | null): (() => void) | undefined => (node ? holdPage(node) : undefined);
