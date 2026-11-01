// 기존 SidebarResizer와 같은 pointer capture·키보드 조작. 모바일 페이지 쓸기와는 별개다.
import { useRef, useState, type PointerEvent } from "react";

export function IdeaResizer({ percent, onResize }: { percent: number; onResize: (percent: number) => void }) {
  const drag = useRef<{ id: number; start: number; percent: number; width: number } | null>(null);
  const [dragging, setDragging] = useState(false);
  const resize = (value: number) => onResize(Math.max(25, Math.min(75, value)));
  const end = (e: PointerEvent<HTMLDivElement>) => {
    if (drag.current?.id !== e.pointerId) return;
    drag.current = null;
    setDragging(false);
    if (e.currentTarget.hasPointerCapture(e.pointerId)) e.currentTarget.releasePointerCapture(e.pointerId);
  };
  return <div role="separator" tabIndex={0} aria-label="노트와 대화 크기" aria-orientation="vertical"
    aria-controls="idea-note-pane" aria-valuemin={25} aria-valuemax={75} aria-valuenow={Math.round(percent)}
    className={`idea-resizer${dragging ? " is-dragging" : ""}`} data-testid="idea-resizer"
    onPointerDown={(e) => {
      const width = e.currentTarget.parentElement?.getBoundingClientRect().width;
      if (e.button !== 0 || !width) return;
      e.preventDefault();
      e.currentTarget.setPointerCapture(e.pointerId);
      drag.current = { id: e.pointerId, start: e.clientX, percent, width };
      setDragging(true);
    }}
    onPointerMove={(e) => {
      const state = drag.current;
      if (state?.id === e.pointerId) resize(state.percent + (e.clientX - state.start) / state.width * 100);
    }}
    onPointerUp={end} onPointerCancel={end} onLostPointerCapture={end}
    onDoubleClick={() => onResize(60)}
    onKeyDown={(e) => {
      const value = e.key === "ArrowLeft" ? percent - 2 : e.key === "ArrowRight" ? percent + 2 : e.key === "Home" ? 25 : e.key === "End" ? 75 : null;
      if (value === null) return;
      e.preventDefault(); resize(value);
    }} />;
}
