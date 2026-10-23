import { drawerTrace } from "./drawerTrace.ts";

/** 메뉴가 없는 이전 본문을 그릴 기회를 준 뒤 기록과 본문을 함께 바꾼다.
 * 첫 rAF도 paint 이전이므로 그 안에서 기록을 쓰지 않는다. OS 스냅숏 완료 신호는 아니다.
 */
export function watchDrawerNavigation(needsPaint: () => boolean, close: () => void, internal: (event: PopStateEvent) => boolean) {
  let frame: number | null = null;
  let generation = 0;
  const cancel = (reason: string) => {
    if (frame === null) return;
    window.cancelAnimationFrame(frame);
    frame = null;
    generation++;
    drawerTrace("navigation-cancel", { reason });
  };
  const navigate = (apply: () => void) => {
    const wait = frame !== null || needsPaint();
    cancel("superseded");
    close();
    if (!wait) { apply(); return; }
    const current = ++generation;
    drawerTrace("navigation-wait", { generation: current });
    frame = window.requestAnimationFrame(() => {
      if (current !== generation) return;
      drawerTrace("navigation-frame", { generation: current, stage: 1 });
      frame = window.requestAnimationFrame(() => {
        if (current !== generation) return;
        frame = null;
        if (needsPaint()) { drawerTrace("navigation-cancel", { reason: "reopened" }); return; }
        drawerTrace("navigation-ready", { generation: current, stage: 2 });
        apply();
      });
    });
  };
  const pop = (event: PopStateEvent) => { if (!internal(event)) cancel("popstate"); };
  const hash = () => cancel("hashchange");
  const hide = () => cancel("pagehide");
  const visibility = () => { if (document.visibilityState === "hidden") cancel("hidden"); };
  window.addEventListener("popstate", pop);
  window.addEventListener("hashchange", hash);
  window.addEventListener("pagehide", hide);
  document.addEventListener("visibilitychange", visibility);
  return { navigate, dispose: () => {
    cancel("cleanup");
    window.removeEventListener("popstate", pop);
    window.removeEventListener("hashchange", hash);
    window.removeEventListener("pagehide", hide);
    document.removeEventListener("visibilitychange", visibility);
  } };
}
