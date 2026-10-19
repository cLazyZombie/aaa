// 사이드바 폭(사용자 요청 10-19 「왼쪽 창 … 좌우 resize 기능도 넣어줘」, plan §8.257). 규칙은 herdr-web-ui aa624cb
// `src/lib/sidebarWidth.ts`를 옮겼다: 240~520px, 창 폭의 절반을 넘지 않고, 화살표 한 번에 16px, Home·End는 양 끝.
// 참고(herdr-web-ui·t3code)는 기기 localStorage에 두지만 aaa는 TUI 사이드바 폭(`tuiSidebarWidth`)처럼 host 공유 설정에 둔다
// (preferences.ts `SidebarShared.sidebarWidth`). 넓은 창마다 다른 폭은 창 절반 한도가 받아 낸다.

/** 고른 폭이 없을 때는 디자인 토큰 `--sidebar-w`(320px, 조밀하면 300px)다 */
export const SIDEBAR_MIN_WIDTH = 240;
export const SIDEBAR_MAX_WIDTH = 520;
/** 손잡이에 focus가 있을 때 화살표 한 번 */
export const SIDEBAR_WIDTH_STEP = 16;

/** 이 창에서 사이드바가 가질 수 있는 가장 넓은 폭: 창의 절반을 넘지 않는다 */
export function sidebarMaxWidth(viewportWidth: number): number {
  return Math.max(SIDEBAR_MIN_WIDTH, Math.min(SIDEBAR_MAX_WIDTH, Math.floor(viewportWidth / 2)));
}

export function clampSidebarWidth(width: number, viewportWidth = Number.POSITIVE_INFINITY): number {
  return Math.round(Math.max(SIDEBAR_MIN_WIDTH, Math.min(sidebarMaxWidth(viewportWidth), width)));
}

/** 키가 손잡이를 옮긴 폭. 손잡이 키가 아니면 null */
export function keyedSidebarWidth(current: number, key: string, viewportWidth = Number.POSITIVE_INFINITY): number | null {
  if (key === "ArrowLeft") return clampSidebarWidth(current - SIDEBAR_WIDTH_STEP, viewportWidth);
  if (key === "ArrowRight") return clampSidebarWidth(current + SIDEBAR_WIDTH_STEP, viewportWidth);
  if (key === "Home") return SIDEBAR_MIN_WIDTH;
  if (key === "End") return sidebarMaxWidth(viewportWidth);
  return null;
}

/** 공유 설정 값을 폭으로. 쓸 수 없는 값은 null(기본 폭) */
export function parseSidebarWidth(raw: unknown): number | null {
  return typeof raw === "number" && Number.isFinite(raw) ? clampSidebarWidth(raw) : null;
}
