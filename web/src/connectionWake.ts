import { pageClosing } from "./apiRequest";

// 상태 연결에서 쓰던 복귀 기준. OS가 close 없이 버린 소켓은 readyState가 여전히 OPEN일 수 있다.
const RECONNECT_AFTER_HIDDEN_MS = 10_000;

/** 열린 연결을 교체할 복귀인지, 예약된 재시도만 깨울 짧은 복귀인지 알린다. */
export function watchConnectionWake(wake: (replace: boolean) => void, hide?: () => void): () => void {
  let hiddenAt: number | null = document.visibilityState === "hidden" ? Date.now() : null;
  let lostNetwork = !navigator.onLine;
  const hidden = () => {
    hiddenAt ??= Date.now();
    hide?.();
  };
  const offline = () => { lostNetwork = true; };
  const show = () => {
    if (document.visibilityState === "hidden") return hidden();
    if (pageClosing() || !navigator.onLine) return;
    const longAbsence = hiddenAt !== null && Date.now() - hiddenAt >= RECONNECT_AFTER_HIDDEN_MS;
    const replace = longAbsence || lostNetwork;
    hiddenAt = null;
    lostNetwork = false;
    wake(replace);
  };
  document.addEventListener("visibilitychange", show);
  window.addEventListener("pagehide", hidden);
  window.addEventListener("pageshow", show);
  window.addEventListener("offline", offline);
  window.addEventListener("online", show);
  return () => {
    document.removeEventListener("visibilitychange", show);
    window.removeEventListener("pagehide", hidden);
    window.removeEventListener("pageshow", show);
    window.removeEventListener("offline", offline);
    window.removeEventListener("online", show);
  };
}
