import type { ActionProgress } from "./bulkAction";
import { useCallback, useRef, useState } from "react";

/** 확인 창·일괄 변경의 공통 수명. 같은 렌더 안의 연속 입력도 한 번만 실행한다.
 * 요청을 자동 재시도하지 않는다. 실패하면 원인을 남기고 조작 가능 상태로 돌아온다. */
export function useAsyncAction() {
  const running = useRef(false);
  const [pending, setPending] = useState(false);
  const [progress, setProgress] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const run = useCallback(async (action: (progress: ActionProgress) => Promise<void>) => {
    if (running.current) return;
    running.current = true;
    setPending(true);
    setError(null);
    setProgress(null);
    try {
      await action(setProgress);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      running.current = false;
      setPending(false);
    }
  }, []);
  return { pending, error, progress, run };
}
