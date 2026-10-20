// 크게 보기(Lightbox)의 손짓 계산(plan §8.353): 두 손가락 확대·끌기·두 번 누름·쓸기. 좌표는 모두 무대 가운데를 0으로 한 화면
// px이고, 그림은 `translate(x, y) scale(scale)`(가운데 기준)으로 그린다. 판정만 여기 두고 사건 처리는 Lightbox.tsx가 한다.

export type Point = { x: number; y: number };
export type View = { scale: number; x: number; y: number };
/** 그림이 scale 1에서 차지한 크기 */
export type Size = { width: number; height: number };

export const FIT: View = { scale: 1, x: 0, y: 0 };
export const MAX_SCALE = 6;
/** 두 번 누름이 키우는 배율 */
export const TAP_SCALE = 2.5;
/** 이만큼 움직이지 않은 누름은 누름이다(끌기·쓸기가 아니다) */
export const TAP_SLOP = 10;
/** 두 번 누름의 간격 */
export const DOUBLE_TAP_MS = 300;

const clampScale = (s: number) => Math.min(MAX_SCALE, Math.max(1, s));
const dist = (a: Point, b: Point) => Math.hypot(a.x - b.x, a.y - b.y);
const mid = (a: Point, b: Point): Point => ({ x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 });

/** 그림이 무대 밖으로 빠지지 않게 옮김을 묶는다. 화면보다 작은 쪽은 가운데에 둔다. scale 1이면 늘 제자리다 */
export function bound(v: View, image: Size, stage: Size): View {
  const scale = clampScale(v.scale);
  if (scale === 1) return FIT;
  const room = (img: number, box: number) => Math.max(0, (img * scale - box) / 2);
  const within = (n: number, r: number) => (r === 0 ? 0 : Math.min(r, Math.max(-r, n)));
  return { scale, x: within(v.x, room(image.width, stage.width)), y: within(v.y, room(image.height, stage.height)) };
}

/** 점 `p` 아래 그림 자리를 그대로 두고 배율만 `scale`로 */
export function zoomAt(v: View, p: Point, scale: number): View {
  const s = clampScale(scale);
  const k = s / v.scale;
  return { scale: s, x: p.x - (p.x - v.x) * k, y: p.y - (p.y - v.y) * k };
}

/** 두 손가락: 시작 때 두 점(a0·b0)과 지금 두 점(a1·b1). 손가락 사이 거리만큼 키우고, 두 점 가운데를 따라 옮긴다 */
export function pinch(start: View, a0: Point, b0: Point, a1: Point, b1: Point): View {
  const d0 = dist(a0, b0);
  if (d0 < 1) return start;
  const scale = clampScale((start.scale * dist(a1, b1)) / d0);
  const m0 = mid(a0, b0);
  const m1 = mid(a1, b1);
  const k = scale / start.scale;
  return { scale, x: m1.x - (m0.x - start.x) * k, y: m1.y - (m0.y - start.y) * k };
}

/** 두 번 누름: 맞춘 크기면 그 자리를 키우고, 키운 것은 맞춘 크기로 */
export function doubleTap(v: View, p: Point): View {
  return v.scale > 1 ? FIT : zoomAt(v, p, TAP_SCALE);
}

/** 바퀴(데스크톱 trackpad 두 손가락 벌리기는 ctrl+wheel로 온다): 아래로 굴리면 줄인다 */
export function wheel(v: View, p: Point, deltaY: number): View {
  return zoomAt(v, p, v.scale * Math.exp(-deltaY / 200));
}

export type Swipe = "next" | "prev" | "close" | null;

/** 맞춘 크기에서 한 손가락으로 끈 거리(dx·dy, 무대 너비 width) → 넘기기·닫기. 옆으로는 다음·앞, 아래로는 닫기 */
export function swipeOf(dx: number, dy: number, width: number): Swipe {
  const side = Math.max(48, width * 0.12);
  if (Math.abs(dx) >= side && Math.abs(dx) > Math.abs(dy) * 1.5) return dx < 0 ? "next" : "prev";
  if (dy >= 80 && dy > Math.abs(dx) * 1.5) return "close";
  return null;
}
