// 대화 기록에서 글을 고르면 그 끝에 「인용」 단추를 띄운다(t-1204, 참고 t3code `AssistantSelectionToolbar.tsx`·
// `selectionActions.ts`, MIT). 누르면 고른 글과 출처를 이 pane 입력 초안 끝에 붙인다(selectionQuote.ts).
// 끄는 동안(누른 채)은 띄우지 않고 놓은 뒤에 띄운다. 단추를 눌러도 고른 글이 풀리지 않는다(pointerdown 막음).
import { Quote } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState, type RefObject } from "react";
import { createPortal } from "react-dom";
import { chatSelection, type QuoteSpeaker } from "./selectionQuote.ts";

/** 대화 칸이 보이는 범위 안에 고른 글의 줄이 하나라도 있는가 */
function visibleIn(range: Range, box: DOMRect): DOMRect | null {
  const lines = [...range.getClientRects()].filter((r) => r.width > 0 && r.bottom > box.top && r.top < box.bottom && r.right > box.left && r.left < box.right);
  return lines.at(-1) ?? null;
}

export function ChatQuoteButton({ container, active, onQuote }: {
  container: RefObject<HTMLElement | null>;
  active: boolean;
  /** 고른 글을 초안에 붙였으면 true(넘쳐 못 붙이면 false: 단추와 고른 글을 그대로 둔다) */
  onQuote: (text: string, speaker: QuoteSpeaker) => boolean;
}) {
  const [shown, setShown] = useState<{ text: string; speaker: QuoteSpeaker; x: number; y: number } | null>(null);
  const button = useRef<HTMLButtonElement>(null);
  const onQuoteRef = useRef(onQuote);
  onQuoteRef.current = onQuote;

  useEffect(() => {
    const el = container.current;
    if (!active || !el) { setShown(null); return; }
    let pressed = false;
    let frame: number | null = null;
    const update = () => {
      frame = null;
      if (pressed) return;
      const picked = chatSelection(el, window.getSelection());
      const line = picked && visibleIn(picked.range, el.getBoundingClientRect());
      if (!picked || !line) { setShown(null); return; }
      setShown({ text: picked.text, speaker: picked.speaker, x: line.right, y: line.bottom + 6 });
    };
    const schedule = () => {
      if (frame === null) frame = window.requestAnimationFrame(update);
    };
    const onDown = (event: PointerEvent) => {
      if (button.current?.contains(event.target as Node)) return;
      pressed = event.isPrimary && el.contains(event.target as Node);
      if (pressed) setShown(null);
    };
    const onUp = () => {
      if (!pressed) return;
      pressed = false;
      schedule();
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") setShown(null);
    };
    document.addEventListener("selectionchange", schedule);
    document.addEventListener("pointerdown", onDown, true);
    window.addEventListener("pointerup", onUp, true);
    window.addEventListener("pointercancel", onUp, true);
    document.addEventListener("keydown", onKey);
    el.addEventListener("scroll", schedule, { passive: true });
    window.addEventListener("resize", schedule);
    schedule();
    return () => {
      if (frame !== null) window.cancelAnimationFrame(frame);
      document.removeEventListener("selectionchange", schedule);
      document.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("pointerup", onUp, true);
      window.removeEventListener("pointercancel", onUp, true);
      document.removeEventListener("keydown", onKey);
      el.removeEventListener("scroll", schedule);
      window.removeEventListener("resize", schedule);
    };
  }, [active, container]);

  // 화면 끝에 걸리면 안쪽으로 당긴다(참고 resolveSelectionActionPosition의 8px 여백)
  useLayoutEffect(() => {
    const b = button.current;
    if (!b || !shown) return;
    const rect = b.getBoundingClientRect();
    b.style.left = `${Math.max(8, Math.min(shown.x - rect.width, window.innerWidth - rect.width - 8))}px`;
    b.style.top = `${Math.max(8, Math.min(shown.y, window.innerHeight - rect.height - 8))}px`;
  }, [shown]);

  if (!shown) return null;
  return createPortal(
    <button
      ref={button}
      type="button"
      className="chat-quote"
      data-testid="chat-quote"
      style={{ left: shown.x, top: shown.y }}
      onPointerDown={(event) => event.preventDefault()}
      onClick={() => {
        if (!onQuoteRef.current(shown.text, shown.speaker)) return;
        window.getSelection()?.removeAllRanges();
        setShown(null);
      }}
    >
      <Quote aria-hidden="true" />
      메시지에 인용
    </button>,
    document.body,
  );
}
