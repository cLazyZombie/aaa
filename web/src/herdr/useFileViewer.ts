import { useCallback, useEffect, useRef, useState } from "react";
import { outsideSettings } from "./settingsHistory.ts";

// herdr-web-ui src/lib/useFileViewer.ts(aa624cb). aaa: pane ID가 host 전체에서 하나라 머신 ID 대신 pane만 둔다.
// thread 결과물(차이 목록 R1)은 pane 대신 그 thread(`library`)를 둔다: 경로는 결과물 안 상대 경로다.
// 보고서 안 파일 링크(plan §8.170)는 그 thread(`worktree`)를 둔다: 경로는 thread 작업 폴더 기준이다.

type ThreadRef = { project: string; thread: string };
interface FilePreview {
  path: string;
  pane: string | null;
  library?: ThreadRef;
  worktree?: ThreadRef;
}

function threadRef(value: unknown): ThreadRef | null {
  if (value === null || typeof value !== "object") return null;
  const v = value as Record<string, unknown>;
  return typeof v.project === "string" && typeof v.thread === "string" ? { project: v.project, thread: v.thread } : null;
}
const HISTORY_KEY = "aaa:file-preview";

function previewFromState(state: unknown): FilePreview | null {
  if (state === null || typeof state !== "object") return null;
  const preview = (state as Record<string, unknown>)[HISTORY_KEY];
  if (preview === null || typeof preview !== "object") return null;
  const value = preview as Record<string, unknown>;
  if (typeof value.path !== "string" || (value.pane !== null && typeof value.pane !== "string")) return null;
  const shown: FilePreview = { path: value.path, pane: value.pane };
  for (const key of ["library", "worktree"] as const) {
    if (value[key] === undefined) continue;
    const ref = threadRef(value[key]);
    if (ref === null) return null;
    shown[key] = ref;
  }
  return shown;
}

/** Android Back traverses history: a preview must be an entry above the chat, not just a modal. */
export function useFileViewer() {
  const [viewing, setViewing] = useState(() => previewFromState(window.history.state));
  const closing = useRef(false);

  useEffect(() => {
    const onPop = (event: PopStateEvent): void => {
      closing.current = false;
      // Forward restores the preview with its original pane, including after reload.
      setViewing(previewFromState(event.state));
    };
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, []);

  const openFile = useCallback((preview: FilePreview) => {
    if (closing.current) return;
    // a preview opened over Settings is no entry of Settings: a reload on it closes Settings and keeps the file
    const state = { ...outsideSettings(window.history.state), [HISTORY_KEY]: preview };
    // Choosing a different file inside an open preview must not add another Back step.
    if (previewFromState(window.history.state)) window.history.replaceState(state, "");
    else window.history.pushState(state, "");
    setViewing(preview);
  }, []);

  const closeFile = useCallback(() => {
    if (closing.current) return;
    if (previewFromState(window.history.state)) {
      closing.current = true;
      // X, Escape and scrim clicks consume the same entry as the system Back button.
      window.history.back();
    } else setViewing(null);
  }, []);

  return { viewing, openFile, closeFile };
}
