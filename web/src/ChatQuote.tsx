// 고른 글 끝에 띄우는 띠(t-1204, 참고 t3code `AssistantSelectionToolbar.tsx`·`selectionActions.ts`, MIT). 대화 기록에서 고르면
// 「메시지에 인용」·「의견」, 보고 글에서 고르면 「의견」(t-1222)이다. 끄는 동안(누른 채)은 띄우지 않고 놓은 뒤에 띄운다.
// 단추를 눌러도 고른 글이 풀리지 않는다(마우스 pointerdown 막음). 의견 칸을 연 동안은 고른 글이 바뀌어도 띠를 그대로 둔다
// (참고 observeSelectionActions의 getActionElement: 칸에 쓰는 동안 잡아 둔 고른 글을 바꾸지 않는다).
import { MessageSquarePlus, Quote } from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode, type RefObject } from "react";
import { createPortal } from "react-dom";
import { chatSelection, type QuoteSpeaker } from "./selectionQuote.ts";

/** 대화 칸이 보이는 범위 안에 고른 글의 줄이 하나라도 있는가 */
function visibleIn(range: Range, box: DOMRect): DOMRect | null {
  const lines = [...range.getClientRects()].filter((r) => r.width > 0 && r.bottom > box.top && r.top < box.bottom && r.right > box.left && r.left < box.right);
  return lines.at(-1) ?? null;
}

/** 마우스만 막는다: WebKit은 터치 pointerdown을 막으면 tap의 click을 내지 않는다(t-1204 수리) */
export const keepSelection = (event: React.PointerEvent) => { if (event.pointerType === "mouse") event.preventDefault(); };

export interface SelectionControl {
  /** 고른 글을 풀고 띠를 닫는다 */
  close: () => void;
  /** 의견 칸을 연 동안 고른 글이 바뀌어도 띠를 두고, 닫으면 다시 고른 글을 따른다 */
  hold: (on: boolean) => void;
}

/** 고른 글 끝 아래에 뜨는 띠. `pick`이 container 안 고른 글을 읽고, children이 단추를 그린다 */
export function SelectionActions<T extends { range: Range }>({ container, active, pick, children, testId }: {
  container: RefObject<HTMLElement | null>;
  active: boolean;
  pick: (el: HTMLElement, selection: Selection | null) => T | null;
  children: (picked: T, control: SelectionControl) => ReactNode;
  testId?: string;
}) {
  const [shown, setShown] = useState<{ picked: T; x: number; y: number } | null>(null);
  const bar = useRef<HTMLDivElement>(null);
  const held = useRef(false);
  const recheck = useRef<(() => void) | null>(null);
  const pickRef = useRef(pick);
  pickRef.current = pick;

  useEffect(() => {
    const el = container.current;
    if (!active || !el) { held.current = false; setShown(null); return; }
    let pressed = false;
    let frame: number | null = null;
    const update = () => {
      frame = null;
      if (pressed || held.current) return;
      const picked = pickRef.current(el, window.getSelection());
      const line = picked && visibleIn(picked.range, el.getBoundingClientRect());
      if (!picked || !line) { setShown(null); return; }
      setShown({ picked, x: line.right, y: line.bottom + 6 });
    };
    const schedule = () => {
      if (frame === null) frame = window.requestAnimationFrame(update);
    };
    recheck.current = schedule;
    const onDown = (event: PointerEvent) => {
      if (bar.current?.contains(event.target as Node) || held.current) return;
      pressed = event.isPrimary && el.contains(event.target as Node);
      if (pressed) setShown(null);
    };
    const onUp = () => {
      if (!pressed) return;
      pressed = false;
      schedule();
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !held.current) setShown(null);
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
      recheck.current = null;
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

  // 화면 끝에 걸리면 안쪽으로 당긴다(참고 resolveSelectionActionPosition의 8px 여백). 의견 칸을 열어 커져도 다시 잰다
  const place = useCallback(() => {
    const b = bar.current;
    if (!b || !shown) return;
    const rect = b.getBoundingClientRect();
    b.style.left = `${Math.max(8, Math.min(shown.x - rect.width, window.innerWidth - rect.width - 8))}px`;
    b.style.top = `${Math.max(8, Math.min(shown.y, window.innerHeight - rect.height - 8))}px`;
  }, [shown]);
  useLayoutEffect(() => {
    place();
    const b = bar.current;
    if (!b) return;
    const observer = new ResizeObserver(place);
    observer.observe(b);
    return () => observer.disconnect();
  }, [place]);

  const control: SelectionControl = {
    close: () => {
      held.current = false;
      window.getSelection()?.removeAllRanges();
      setShown(null);
    },
    hold: (on) => {
      held.current = on;
      if (!on) recheck.current?.();
    },
  };
  if (!shown) return null;
  return createPortal(
    <div ref={bar} className="selection-actions" data-testid={testId} style={{ left: shown.x, top: shown.y }}>
      {children(shown.picked, control)}
    </div>,
    document.body,
  );
}

/**
 * 인용에 다는 의견 칸(t-1222, 참고 Orca 줄 의견의 글 칸·GitHub 줄 의견). 고른 원문 첫 줄들을 위에 보이고, ⌘/Ctrl+Enter로 넣고
 * Esc로 닫는다. `onSubmit`이 false면(넘침) 칸과 쓴 글을 그대로 둔다
 */
export function NoteEditor({ quote, initial = "", submitLabel, onSubmit, onCancel, autoFocus = true }: {
  quote?: string;
  initial?: string;
  submitLabel: string;
  onSubmit: (comment: string) => boolean;
  onCancel: () => void;
  autoFocus?: boolean;
}) {
  const [text, setText] = useState(initial);
  const submit = () => { onSubmit(text); };
  return (
    <div className="quote-note" data-testid="quote-note" onKeyDown={(e) => {
      if (e.key === "Escape" && !e.nativeEvent.isComposing) {
        e.preventDefault();
        e.stopPropagation();
        onCancel();
      } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && !e.nativeEvent.isComposing) {
        e.preventDefault();
        e.stopPropagation();
        submit();
      }
    }}>
      {quote !== undefined && <blockquote className="quote-note-text">{quote}</blockquote>}
      <textarea className="quote-note-input" aria-label="의견" placeholder="의견" rows={3} value={text} autoFocus={autoFocus} onChange={(e) => setText(e.target.value)} />
      <div className="quote-note-actions">
        <button type="button" className="ghost small" onClick={onCancel}>취소</button>
        <button type="button" className="primary small" data-testid="quote-note-submit" onClick={submit}>{submitLabel}</button>
      </div>
    </div>
  );
}

/** 「의견」 단추 */
export function NoteButton({ onClick, testId }: { onClick: () => void; testId?: string }) {
  return (
    <button type="button" className="chat-quote" data-testid={testId} onPointerDown={keepSelection} onClick={onClick}>
      <MessageSquarePlus aria-hidden="true" />
      의견
    </button>
  );
}

export function ChatQuoteButton({ container, active, onQuote }: {
  container: RefObject<HTMLElement | null>;
  active: boolean;
  /** 고른 글(과 단 의견)을 초안에 붙였으면 true(넘쳐 못 붙이면 false: 단추와 고른 글을 그대로 둔다) */
  onQuote: (text: string, speaker: QuoteSpeaker, comment?: string) => boolean;
}) {
  const [noting, setNoting] = useState(false);
  const onQuoteRef = useRef(onQuote);
  onQuoteRef.current = onQuote;
  useEffect(() => { if (!active) setNoting(false); }, [active]);
  return (
    <SelectionActions container={container} active={active} pick={chatSelection}>
      {(picked, control) => noting ? (
        <NoteEditor quote={picked.text} submitLabel="메시지에 넣기"
          onSubmit={(comment) => {
            if (!onQuoteRef.current(picked.text, picked.speaker, comment)) return false;
            setNoting(false);
            control.close();
            return true;
          }}
          onCancel={() => { setNoting(false); control.hold(false); }} />
      ) : (
        <>
          <button
            type="button"
            className="chat-quote"
            data-testid="chat-quote"
            onPointerDown={keepSelection}
            onClick={() => {
              if (!onQuoteRef.current(picked.text, picked.speaker)) return;
              control.close();
            }}
          >
            <Quote aria-hidden="true" />
            메시지에 인용
          </button>
          <NoteButton testId="chat-quote-note" onClick={() => { control.hold(true); setNoting(true); }} />
        </>
      )}
    </SelectionActions>
  );
}
