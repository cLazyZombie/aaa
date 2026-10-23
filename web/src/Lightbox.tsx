import { logIgnoredFailure } from "./errorDiagnostic.ts";
// 그림 크게 보기(plan §8.353). 「답할 것」 본문 그림(t-0540)·task 첨부 그림·보고서 그림이 모두 이것으로 연다. 화면을 덮고(문서 맨 위),
// 닫기·Esc·바깥 누름·아래로 쓸기·휴대폰 뒤로가 닫는다. 여럿이면 ←→·앞뒤 단추·옆으로 쓸기로 넘긴다. 두 손가락으로 키우고
// 끌며, 두 번 누르면 그 자리를 키운다(데스크톱 trackpad 벌리기·ctrl+바퀴도). 손짓 계산은 lightboxGesture.ts에 있다.
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type PointerEvent as ReactPointerEvent, type ReactNode, type Ref, type RefObject } from "react";
import { createPortal } from "react-dom";
import { ChevronLeft, ChevronRight, ExternalLink, X } from "lucide-react";
import { useFocusTrap } from "./herdr/useFocusTrap";
import { usePhoneSheetBack } from "./herdr/sheetHistory";
import { DOUBLE_TAP_MS, FIT, TAP_SLOP, bound, doubleTap, pinch, swipeOf, wheel, type Point, type View } from "./lightboxGesture";
import { reportImages, viewRoot, type LightboxItem } from "./reportImages";
import "./Lightbox.css";

export type { LightboxItem };
/** 열린 크게 보기: 그림들과 보는 자리 */
export type LightboxState = { items: LightboxItem[]; at: number };

type Gesture =
  | { kind: "pinch"; start: View; a: number; b: number; a0: Point; b0: Point }
  | { kind: "pan" | "swipe"; start: View; id: number; p0: Point };

export function Lightbox({ items, at, onAt, onClose }: { items: readonly LightboxItem[]; at: number; onAt: (i: number) => void; onClose: () => void }) {
  usePhoneSheetBack(() => onClose());
  const close = useRef<HTMLButtonElement>(null);
  const surface = useFocusTrap<HTMLDivElement>(true, { initialFocus: close });
  const stage = useRef<HTMLDivElement>(null);
  const img = useRef<HTMLImageElement>(null);
  const [view, setView] = useState<View>(FIT);
  /** 맞춘 크기에서 쓰는 동안 끈 거리 */
  const [drag, setDrag] = useState<Point | null>(null);
  const [moving, setMoving] = useState(false);
  const pointers = useRef(new Map<number, Point>());
  const gesture = useRef<Gesture | null>(null);
  /** 이번 누름이 끌기·쓸기·확대였다: 뒤따르는 click은 바깥 누름(닫기)이 아니다 */
  const moved = useRef(false);
  /** 이번 누름이 그림 위에서 시작했다(capture한 뒤 사건은 무대로 온다): 그림 누름은 닫기가 아니다 */
  const onImage = useRef(false);
  const lastTap = useRef<{ t: number; p: Point } | null>(null);
  const viewRef = useRef(view);
  viewRef.current = view;
  const count = items.length;
  const item = items[at];
  const go = useCallback((i: number) => {
    if (i >= 0 && i < count) onAt(i);
  }, [count, onAt]);
  // 다른 그림은 맞춘 크기에서 시작한다
  useLayoutEffect(() => {
    setView(FIT);
    setDrag(null);
    pointers.current.clear();
    gesture.current = null;
    lastTap.current = null;
  }, [at]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onClose();
      } else if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
        e.preventDefault();
        e.stopPropagation();
        go(e.key === "ArrowLeft" ? at - 1 : at + 1);
      }
    };
    // 뒤에 깔린 화면(상세의 Esc 닫기 등)보다 먼저 받는다
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [at, go, onClose]);
  const sizes = () => ({
    image: { width: img.current?.offsetWidth ?? 0, height: img.current?.offsetHeight ?? 0 },
    stage: { width: stage.current?.clientWidth ?? 0, height: stage.current?.clientHeight ?? 0 },
  });
  const fit = (v: View) => {
    const s = sizes();
    return bound(v, s.image, s.stage);
  };
  /** 무대 가운데를 0으로 한 자리 */
  const local = (x: number, y: number): Point => {
    const r = stage.current?.getBoundingClientRect();
    return r ? { x: x - (r.left + r.width / 2), y: y - (r.top + r.height / 2) } : { x: 0, y: 0 };
  };
  // 바퀴는 passive가 아니어야 막는다(React onWheel은 passive): ctrl+바퀴(trackpad 벌리기)는 확대, 키운 동안 바퀴는 옮기기
  useEffect(() => {
    const node = stage.current;
    if (!node) return;
    const onWheel = (e: WheelEvent) => {
      const v = viewRef.current;
      if (e.ctrlKey) {
        e.preventDefault();
        setView(fit(wheel(v, local(e.clientX, e.clientY), e.deltaY)));
      } else if (v.scale > 1) {
        e.preventDefault();
        setView(fit({ ...v, x: v.x - e.deltaX, y: v.y - e.deltaY }));
      }
    };
    node.addEventListener("wheel", onWheel, { passive: false });
    return () => node.removeEventListener("wheel", onWheel);
  });
  const down = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (e.pointerType === "mouse" && e.button !== 0) return;
    try {
      e.currentTarget.setPointerCapture(e.pointerId);
    } catch (ignoredError) { logIgnoredFailure(ignoredError, "Lightbox.tsx:104");
      // 이미 끝난 손가락(capture할 것이 없다)
    }
    const p = local(e.clientX, e.clientY);
    if (pointers.current.size === 0) {
      moved.current = false;
      onImage.current = e.target === img.current;
    }
    pointers.current.set(e.pointerId, p);
    const v = viewRef.current;
    const ids = [...pointers.current.keys()];
    if (ids.length === 2) {
      const [a, b] = ids as [number, number];
      gesture.current = { kind: "pinch", start: v, a, b, a0: pointers.current.get(a)!, b0: pointers.current.get(b)! };
      setDrag(null);
      moved.current = true;
    } else if (ids.length === 1) {
      gesture.current = { kind: v.scale > 1 ? "pan" : "swipe", start: v, id: e.pointerId, p0: p };
    }
    setMoving(true);
  };
  const move = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!pointers.current.has(e.pointerId)) return;
    const p = local(e.clientX, e.clientY);
    pointers.current.set(e.pointerId, p);
    const g = gesture.current;
    if (!g) return;
    if (g.kind === "pinch") {
      const a1 = pointers.current.get(g.a);
      const b1 = pointers.current.get(g.b);
      if (a1 && b1) setView(fit(pinch(g.start, g.a0, g.b0, a1, b1)));
      return;
    }
    if (g.id !== e.pointerId) return;
    const dx = p.x - g.p0.x;
    const dy = p.y - g.p0.y;
    if (Math.hypot(dx, dy) > TAP_SLOP) moved.current = true;
    if (g.kind === "pan") setView(fit({ ...g.start, x: g.start.x + dx, y: g.start.y + dy }));
    else if (moved.current) setDrag({ x: dx, y: Math.max(0, dy) });
  };
  const up = (e: ReactPointerEvent<HTMLDivElement>, cancelled: boolean) => {
    const p = pointers.current.get(e.pointerId);
    if (!p) return;
    pointers.current.delete(e.pointerId);
    const g = gesture.current;
    const left = [...pointers.current.entries()];
    if (g?.kind === "pinch") {
      // 한 손가락이 남으면 그 손가락으로 이어 끈다
      const rest = left[0];
      gesture.current = rest ? { kind: "pan", start: viewRef.current, id: rest[0], p0: rest[1] } : null;
      if (!rest) setMoving(false);
      return;
    }
    gesture.current = null;
    setMoving(false);
    setDrag(null);
    if (!g || g.id !== e.pointerId || cancelled) return;
    if (moved.current) {
      if (g.kind !== "swipe") return;
      const s = swipeOf(p.x - g.p0.x, p.y - g.p0.y, stage.current?.clientWidth ?? 0);
      if (s === "close") onClose();
      else if (s === "next") go(at + 1);
      else if (s === "prev") go(at - 1);
      return;
    }
    // 그림을 두 번 누르면 그 자리를 키운다·맞춘 크기로
    if (!onImage.current) return;
    const last = lastTap.current;
    if (last && e.timeStamp - last.t <= DOUBLE_TAP_MS && Math.hypot(p.x - last.p.x, p.y - last.p.y) <= TAP_SLOP * 3) {
      lastTap.current = null;
      setView(fit(doubleTap(viewRef.current, p)));
    } else lastTap.current = { t: e.timeStamp, p };
  };
  if (!item) return null;
  const shift = drag ?? { x: 0, y: 0 };
  return createPortal(
    <div className="lightbox" role="dialog" aria-modal="true" aria-label={count > 1 ? `그림 ${at + 1}/${count}` : item.alt || "그림"}
      ref={surface as Ref<HTMLDivElement>} data-testid="lightbox" data-at={at} data-count={count} data-scale={view.scale.toFixed(2)}>
      <div className="lightbox-bar">
        <span className="lightbox-count">{count > 1 ? `${at + 1} / ${count}` : ""}</span>
        <span className="lightbox-caption" title={item.alt}>{item.alt}</span>
        <a className="btn btn-ghost" href={item.src} target="_blank" rel="noreferrer">
          <ExternalLink aria-hidden="true" />
          원본
        </a>
        <button ref={close} type="button" className="btn btn-ghost" aria-label="닫기" data-testid="lightbox-close" onClick={onClose}>
          <X aria-hidden="true" />
        </button>
      </div>
      <div ref={stage} className={`lightbox-stage${moving ? " is-moving" : ""}`} data-testid="lightbox-stage"
        onPointerDown={down} onPointerMove={move} onPointerUp={(e) => up(e, false)} onPointerCancel={(e) => up(e, true)}
        onClick={(e) => {
          // 바깥 누름은 닫기(끌기·쓸기·확대 끝의 click은 아니다)
          if (e.target === e.currentTarget && !moved.current && !onImage.current) onClose();
        }}>
        <img ref={img} key={item.src} className="lightbox-img" src={item.src} alt={item.alt} draggable={false} data-testid="lightbox-img"
          style={{ transform: `translate(${view.x + shift.x}px, ${view.y + shift.y}px) scale(${view.scale})` }} />
      </div>
      {at > 0 && (
        <button type="button" className="lightbox-nav is-prev" aria-label="앞 그림" data-testid="lightbox-prev" onClick={() => go(at - 1)}>
          <ChevronLeft aria-hidden="true" />
        </button>
      )}
      {at < count - 1 && (
        <button type="button" className="lightbox-nav is-next" aria-label="다음 그림" data-testid="lightbox-next" onClick={() => go(at + 1)}>
          <ChevronRight aria-hidden="true" />
        </button>
      )}
    </div>,
    document.body,
  );
}

/** 크게 보기 하나를 연 상태와 그 화면. `open`으로 열고 `node`를 그린다 */
export function useLightbox(): { open: (s: LightboxState | null) => void; shown: boolean; node: ReactNode } {
  const [state, setState] = useState<LightboxState | null>(null);
  const onAt = useCallback((at: number) => setState((s) => (s ? { ...s, at } : s)), []);
  const onClose = useCallback(() => setState(null), []);
  const open = useCallback((s: LightboxState | null) => {
    if (s && s.items[s.at]) setState(s);
  }, []);
  const node = state ? <Lightbox items={state.items} at={state.at} onAt={onAt} onClose={onClose} /> : null;
  return { open, shown: state !== null, node };
}

/**
 * 보고서 iframe(보기 경로 `/v/<cap>/…`)의 그림 누름 알림을 받는다(host가 HTML 탐색 응답에 붙인 것, plan §8.353). 그 iframe에서 온
 * 메시지만, 그 보고서 보기 경로 아래 같은 origin 그림만 연다.
 */
export function useFrameImages(frame: RefObject<HTMLIFrameElement | null>, src: string | null | undefined, open: (s: LightboxState) => void): void {
  useEffect(() => {
    if (!src) return;
    const origin = window.location.origin;
    const root = viewRoot(src, origin);
    if (root === null) return;
    const onMessage = (e: MessageEvent) => {
      const node = frame.current;
      if (!node) return;
      const got = node.contentWindow !== null && e.source === node.contentWindow ? reportImages(e.data, root, origin) : null;
      // 표지: 본 메시지·연 크게 보기 수(거른 것도 사건으로 보인다, 시험이 기다린다)
      node.dataset.messages = String(Number(node.dataset.messages ?? 0) + 1);
      if (!got) return;
      node.dataset.opened = String(Number(node.dataset.opened ?? 0) + 1);
      open(got);
    };
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, [frame, src, open]);
}
