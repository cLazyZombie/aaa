// 화면이 보이는지(참고 lib/visibility.ts usePageVisible): 구독 사용량 다시 읽기와 coordinator 입력창의 도는 빛이 쓴다.
import { useCallback, useSyncExternalStore } from "react";

export function usePageVisible(): boolean {
  return useSyncExternalStore(
    useCallback((f: () => void) => {
      document.addEventListener("visibilitychange", f);
      return () => document.removeEventListener("visibilitychange", f);
    }, []),
    () => document.visibilityState === "visible",
  );
}
