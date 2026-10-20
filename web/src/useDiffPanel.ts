import { useCallback, useEffect, useRef, useState } from "react";

// thread diff 패널(plan §8.305)은 파일 보기(`herdr/useFileViewer.ts`)처럼 같은 주소의 기록 한 칸이다: 닫기(X)·Esc·브라우저
// 뒤로·iOS 가장자리 쓸기가 같은 결과다(§8.300·§8.301). t3code는 오른쪽 패널 상태(zustand)라 기록에 없다. 열린 패널에서 다른
// 파일을 고르면 칸을 더하지 않고 바꾼다(`reveal`이 늘어 같은 파일도 다시 데려온다, t3code `revealRequestId`).

export interface DiffPanelTarget {
  project: string;
  thread: string;
  /** 데려올 파일(없으면 맨 위) */
  path: string | null;
  reveal: number;
}

const HISTORY_KEY = "aaa:thread-diff";

function targetFromState(state: unknown): DiffPanelTarget | null {
  if (state === null || typeof state !== "object") return null;
  const v = (state as Record<string, unknown>)[HISTORY_KEY];
  if (v === null || typeof v !== "object") return null;
  const t = v as Record<string, unknown>;
  if (typeof t.project !== "string" || typeof t.thread !== "string" || typeof t.reveal !== "number") return null;
  if (t.path !== null && typeof t.path !== "string") return null;
  return { project: t.project, thread: t.thread, path: t.path, reveal: t.reveal };
}

export function useDiffPanel(project: string, thread: string) {
  const [target, setTarget] = useState(() => targetFromState(window.history.state));
  const closing = useRef(false);

  useEffect(() => {
    const onPop = (event: PopStateEvent): void => {
      closing.current = false;
      setTarget(targetFromState(event.state));
    };
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, []);

  const open = useCallback(
    (path: string | null) => {
      if (closing.current) return;
      const shown = targetFromState(window.history.state);
      const next = { project, thread, path, reveal: (shown?.reveal ?? 0) + 1 };
      const state = { ...(window.history.state ?? {}), [HISTORY_KEY]: next };
      if (shown) window.history.replaceState(state, "");
      else window.history.pushState(state, "");
      setTarget(next);
    },
    [project, thread],
  );

  const close = useCallback(() => {
    if (closing.current) return;
    if (targetFromState(window.history.state)) {
      closing.current = true;
      window.history.back();
    } else setTarget(null);
  }, []);

  // 다른 thread의 칸이면 이 화면에서는 닫힌 것이다
  const shown = target !== null && target.project === project && target.thread === thread ? target : null;
  return { target: shown, open, close };
}
