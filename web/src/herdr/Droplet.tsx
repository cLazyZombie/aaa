import { useCallback, useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type PointerEvent as ReactPointerEvent } from "react";
import { Icon } from "../icons";
import { flickVelocity, onDroplet, type PressSample, type QueuedDroplet } from "./droplet.ts";
import { AgentMark } from "./AgentMark.tsx";
import "./Droplet.css";

/**
 * The in-app alert (lib/droplet.ts): a pill appears and spreads into a card,
 * below the native safe area and app header, without imitating the physical camera cutout.
 * One shows at a time; a newer one folds the current one away and takes its place.
 * A tap opens the pane, a flick up or its close button puts it away, and it leaves by itself after a while.
 */

/** enter: the drop falls, then spreads, then its text shows (Droplet.css runs the same clock) */
const REVEAL_MS = 560;
/** how long it stays once its text shows */
export const DROPLET_HOLD_MS = 3600;
/** exit: the text fades, the card folds back into a drop, the drop rises */
const EXIT_MS = 820;
/** a drag up this far, or a flick up this fast, puts it away */
const DISMISS_DRAG_PX = -18;
const DISMISS_VELOCITY = -0.42; // px per ms
/** a press that moved less than this is a tap */
const TAP_SLOP_PX = 6;

type Phase = "in" | "out";

function reducedMotion(): boolean {
  return typeof window !== "undefined" && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true;
}

export function Droplet({ onOpen }: { onOpen: (target: string) => void }) {
  const [current, setCurrent] = useState<QueuedDroplet | null>(null);
  const [phase, setPhase] = useState<Phase>("in");
  const [drag, setDrag] = useState(0);
  const [top, setTop] = useState<number | null>(null);
  const [fit, setFit] = useState<number | null>(null);
  const probe = useRef<HTMLDivElement | null>(null);
  const card = useRef<HTMLButtonElement | null>(null);
  const pending = useRef<QueuedDroplet | null>(null);
  const currentRef = useRef(current); currentRef.current = current;
  const phaseRef = useRef(phase); phaseRef.current = phase;
  const holdTimer = useRef<number | null>(null);
  const exitTimer = useRef<number | null>(null);
  // a press that dragged ends in a click on a mouse: that click is not a tap
  const dragged = useRef(false);
  const press = useRef<{ id: number; y: number; samples: PressSample[]; moved: boolean } | null>(null);

  useLayoutEffect(() => {
    // aaa: 머리가 하나가 아니다(목록 머리·보기 머리). 보이는 머리 가운데 가장 아래 끝 밑에 둔다
    const headers = () => [...document.querySelectorAll<HTMLElement>(".list-head, .term-head")].filter((h) => h.offsetParent !== null);
    const measure = () => {
      const safeTop = Number.parseFloat(getComputedStyle(probe.current!).paddingTop) || 0;
      setTop(Math.max(safeTop, ...headers().map((h) => h.getBoundingClientRect().bottom)) + 12);
    };
    measure();
    const observer = new ResizeObserver(measure);
    for (const header of headers()) observer.observe(header);
    window.addEventListener("resize", measure);
    window.visualViewport?.addEventListener("resize", measure);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", measure);
      window.visualViewport?.removeEventListener("resize", measure);
    };
  }, []);

  // the phone layout's card is as wide as its text: the shape under it takes that width
  useLayoutEffect(() => {
    const element = card.current;
    if (!element) return;
    const measure = () => setFit(Math.ceil(element.getBoundingClientRect().width));
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [current]);

  const clearHold = () => {
    if (holdTimer.current !== null) window.clearTimeout(holdTimer.current);
    holdTimer.current = null;
  };

  const leave = useCallback(() => {
    if (!currentRef.current || phaseRef.current === "out") return;
    clearHold();
    setPhase("out");
    phaseRef.current = "out";
    exitTimer.current = window.setTimeout(() => {
      exitTimer.current = null;
      const next = pending.current;
      pending.current = null;
      setDrag(0);
      setCurrent(next);
      setPhase("in");
      phaseRef.current = "in";
    }, reducedMotion() ? 160 : EXIT_MS);
  }, []);

  const hold = useCallback((ms: number) => {
    clearHold();
    holdTimer.current = window.setTimeout(leave, ms);
  }, [leave]);

  useEffect(() => onDroplet((notice) => {
    if (!currentRef.current) {
      setCurrent(notice);
      setPhase("in");
      phaseRef.current = "in";
      return;
    }
    // one at a time: the newest waits for the current one to fold away
    pending.current = notice;
    leave();
  }), [leave]);

  useEffect(() => {
    if (!current || phase !== "in") return;
    hold(REVEAL_MS + DROPLET_HOLD_MS);
  }, [current, phase, hold]);

  useEffect(() => () => {
    clearHold();
    if (exitTimer.current !== null) window.clearTimeout(exitTimer.current);
  }, []);

  const onPointerDown = (event: ReactPointerEvent<HTMLButtonElement>) => {
    // a right-click ends in no click: holding for it would leave the card up for good
    if (phaseRef.current === "out" || event.button !== 0) return;
    dragged.current = false;
    press.current = { id: event.pointerId, y: event.clientY, samples: [{ y: event.clientY, t: event.timeStamp }], moved: false };
    event.currentTarget.setPointerCapture?.(event.pointerId);
    clearHold(); // held while touched
  };
  const onPointerMove = (event: ReactPointerEvent<HTMLButtonElement>) => {
    const p = press.current;
    if (!p || p.id !== event.pointerId) return;
    const dy = event.clientY - p.y;
    if (Math.abs(dy) > TAP_SLOP_PX) p.moved = true;
    p.samples.push({ y: event.clientY, t: event.timeStamp });
    // up follows the finger, down only gives a little
    setDrag(Math.max(-120, Math.min(24, dy < 0 ? dy : dy * 0.35)));
  };
  const endPress = (event: ReactPointerEvent<HTMLButtonElement>, cancelled: boolean) => {
    const p = press.current;
    if (!p || p.id !== event.pointerId) return;
    press.current = null;
    const dy = event.clientY - p.y;
    const velocity = flickVelocity(p.samples, event.clientY, event.timeStamp);
    if (!cancelled && !p.moved) return; // a tap: onClick opens it
    dragged.current = true;
    if (!cancelled && (dy <= DISMISS_DRAG_PX || velocity <= DISMISS_VELOCITY)) {
      leave();
      return;
    }
    setDrag(0);
    hold(DROPLET_HOLD_MS);
  };

  if (!current) return <div ref={probe} className="droplet-probe" aria-hidden="true" />;
  const what = current.kind === "blocked" ? "입력 필요" : current.kind === "done" ? "작업 완료" : "터미널 종료됨";
  const detail = current.machine ? `${current.machine} · ${what}` : what;
  return (
    <div className="droplet" role="status" aria-live="polite" data-phase={phase} data-kind={current.kind} data-dragging={drag !== 0 ? "" : undefined} style={{ "--droplet-drag": `${drag}px`, "--droplet-top": `${top ?? 12}px`, ...(fit === null ? {} : { "--droplet-fit": `${fit}px` }), visibility: top === null ? "hidden" : "visible" } as CSSProperties} key={current.id}>
      <div ref={probe} className="droplet-probe" aria-hidden="true" />
      <svg className="droplet-defs" width="0" height="0" aria-hidden="true" focusable="false">
        <filter id="droplet-goo" x="-50%" y="-50%" width="200%" height="200%" colorInterpolationFilters="sRGB">
          <feGaussianBlur in="SourceGraphic" stdDeviation="9" result="blur" />
          {/* alpha sharpened back into an edge: shapes that blur into each other read as one liquid */}
          <feColorMatrix in="blur" mode="matrix" values="1 0 0 0 0  0 1 0 0 0  0 0 1 0 0  0 0 0 22 -9" />
        </filter>
      </svg>
      <div className="droplet-goo" aria-hidden="true">
        <span className="droplet-blob" />
      </div>
      <button
        type="button"
        ref={card}
        className="droplet-card"
        aria-label={`${current.title}, ${detail}. 패널 열기`}
        onClick={() => {
          if (dragged.current) {
            dragged.current = false;
            return;
          }
          if (phaseRef.current === "out") return;
          onOpen(current.target);
          leave();
        }}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={(event) => endPress(event, false)}
        onPointerCancel={(event) => endPress(event, true)}
      >
        <span className="droplet-mark">{current.agent ? <AgentMark agent={current.agent} size={22} /> : <span className="droplet-mark-blank" />}</span>
        <span className="droplet-text">
          <span className="droplet-title">{current.title}</span>
          <span className="droplet-detail" title={current.libraryUrl ?? undefined}>{current.libraryUrl ? `보기: ${current.libraryUrl}` : detail}</span>
        </span>
        <span className="droplet-dot" aria-hidden="true" />
      </button>
      <button type="button" className="droplet-close" aria-label="닫기" title="닫기" onClick={leave}>
        <Icon name="close" />
      </button>
    </div>
  );
}
