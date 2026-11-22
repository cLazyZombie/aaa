// 「앱 다시 열기」가 host의 지금 셸을 받게 하는 주소 표지(t-2150). service worker는 앱 첫 화면을 캐시한 셸로 바로 연다.

/** 「앱 다시 열기」 주소 표지(public/sw.js FRESH와 같다). service worker는 앱 첫 화면을 캐시한 셸로 바로 열지만(t-2150) 이 표지가
 * 붙은 이동은 host의 지금 셸을 받는다. 다시 연 페이지는 읽자마자 주소에서 지운다(dropFreshParam) */
export const FRESH_PARAM = "fresh";

/** 지금 주소에 표지를 붙인 주소(자리 hash는 그대로) */
export function freshUrl(href: string): string {
  const url = new URL(href);
  url.searchParams.set(FRESH_PARAM, "1");
  return url.href;
}

/** 표지를 지운 주소. 없으면 null(바꿀 것 없음) */
export function withoutFresh(href: string): string | null {
  const url = new URL(href);
  if (!url.searchParams.has(FRESH_PARAM)) return null;
  url.searchParams.delete(FRESH_PARAM);
  return url.pathname + url.search + url.hash;
}

let openedFresh = false;

/** 다시 연 페이지가 표지를 기록·공유 주소에 남기지 않게 지운다(takePairCode처럼 replaceState) */
export function dropFreshParam(): void {
  const next = withoutFresh(window.location.href);
  if (next === null) return;
  openedFresh = true;
  window.history.replaceState(window.history.state, "", next);
}

/**
 * host의 지금 셸로 다시 연다. 기록 칸의 state(설정·sheet 칸 표지)는 그대로 둔다: 주소만 바꾼 뒤 reload한다.
 * once: 이 페이지가 이미 표지로 열렸으면 하지 않고 false(host의 셸이 다시 같은 답이면 되풀이하지 않는다)
 */
export function reopenFresh({ once = false } = {}): boolean {
  if (once && openedFresh) return false;
  window.history.replaceState(window.history.state, "", freshUrl(window.location.href));
  window.location.reload();
  return true;
}
