// 사이드바 오른쪽 경계 손잡이(사용자 요청 10-19 「왼쪽 창 … 좌우 resize 기능도 넣어줘」, plan §8.255). herdr-web-ui aa624cb
// `src/components/SidebarResizer.tsx`를 옮겼다: 끌면 폭이 바뀌고 두 번 누르면 기본 폭, focus 뒤 ←→ 16px·Home·End, role="separator".
// 활동 막대가 아니라 그 오른쪽 사이드바 칸(#sidebar-column)의 폭이다. 넓은 창에서만 보이고(휴대폰 drawer는 제 폭), 접으면 칸과 함께 숨는다.
// 끄는 동안은 화면에만 놓고 놓을 때 한 번 저장한다(t3code `useResizableWidth`: 매 tick 쓰지 않는다). 저장은 host 공유 설정이다.
import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import { clampSidebarWidth, keyedSidebarWidth, sidebarMaxWidth, SIDEBAR_MIN_WIDTH } from "./sidebarWidth";

export function SidebarResizer({
  width,
  onResize,
}: {
  /** 고른 폭 px. null이면 기본 폭(토큰) */
  width: number | null;
  /** `save`가 false면 끄는 중(화면에만), true면 저장할 값이다. null은 기본 폭으로 */
  onResize: (width: number | null, save: boolean) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const drag = useRef<{ left: number; grip: number; width: number; moved: boolean } | null>(null);
  const [dragging, setDragging] = useState(false);
  const column = () => ref.current?.parentElement?.querySelector<HTMLElement>("#sidebar-column") ?? null;
  // 지금 칸의 폭: 기본 폭은 토큰이고 좁은 창은 고른 폭도 줄인다
  const [shown, setShown] = useState(width ?? SIDEBAR_MIN_WIDTH);
  const [limit, setLimit] = useState(() => sidebarMaxWidth(window.innerWidth));
  const measure = (): number => {
    const measured = Math.round(column()?.getBoundingClientRect().width ?? shown);
    // 접힌 칸은 0이다: 마지막 폭을 값으로 둔다
    if (measured > 0) setShown(measured);
    setLimit(sidebarMaxWidth(window.innerWidth));
    return measured > 0 ? measured : shown;
  };
  useLayoutEffect(() => {
    measure();
  }, [width]);
  // 기본 폭은 조밀도 설정을 따르고 창 크기도 폭을 줄인다: 칸을 지켜본다
  useEffect(() => {
    const target = column();
    if (!target || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => measure());
    observer.observe(target);
    const onWindow = () => measure();
    window.addEventListener("resize", onWindow);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", onWindow);
    };
  }, []);

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    const box = column()?.getBoundingClientRect();
    if (!box) return;
    // 끄는 동안 본문 글이 선택되지 않게
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = { left: box.left, grip: event.clientX - box.right, width: Math.round(box.width), moved: false };
    setDragging(true);
  };
  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const state = drag.current;
    if (!state) return;
    const next = clampSidebarWidth(event.clientX - state.grip - state.left, window.innerWidth);
    if (next === state.width) return;
    state.width = next;
    state.moved = true;
    onResize(next, false);
  };
  const endDrag = (event: PointerEvent<HTMLDivElement>) => {
    const state = drag.current;
    if (!state) return;
    drag.current = null;
    setDragging(false);
    if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
    // 움직이지 않은 누름은 폭을 고르지 않는다(두 번 누름의 앞 누름)
    if (state.moved) onResize(state.width, true);
  };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const next = keyedSidebarWidth(measure(), event.key, window.innerWidth);
    if (next === null) return;
    event.preventDefault();
    onResize(next, true);
  };

  return (
    <div
      ref={ref}
      className={`sidebar-resizer${dragging ? " is-dragging" : ""}`}
      role="separator"
      tabIndex={0}
      aria-orientation="vertical"
      aria-controls="sidebar-column"
      aria-label="사이드바 폭"
      aria-valuemin={SIDEBAR_MIN_WIDTH}
      aria-valuemax={limit}
      aria-valuenow={shown}
      title="끌어서 폭 바꾸기 · 두 번 누르면 기본 폭"
      data-testid="sidebar-resizer"
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={endDrag}
      onPointerCancel={endDrag}
      onDoubleClick={() => onResize(null, true)}
      onFocus={measure}
      onKeyDown={onKeyDown}
    />
  );
}
