import { useCallback, useEffect, useRef, useState } from "react";
import { outsideSettings } from "./settingsHistory.ts";

// herdr-web-ui src/lib/useFileViewer.ts(aa624cb). aaa: pane ID가 host 전체에서 하나라 머신 ID 대신 pane만 둔다.
// thread 결과물(차이 목록 R1)은 pane 대신 그 thread(`library`)를 둔다: 경로는 결과물 안 상대 경로다.

interface FilePreview {
  path: string;
  pane: string | null;
  library?: { project: string; thread: string };
}
const HISTORY_KEY = "aaa:file-preview";

function previewFromState(state: unknown): FilePreview | null {
  if (state === null || typeof state !== "object") return null;
  const preview = (state as Record<string, unknown>)[HISTORY_KEY];
  if (preview === null || typeof preview !== "object") return null;
  const value = preview as Record<string, unknown>;
  if (typeof value.path !== "string" || (value.pane !== null && typeof value.pane !== "string")) return null;
  const library = value.library as Record<string, unknown> | null | undefined;
  if (library === undefined) return { path: value.path, pane: value.pane };
  return library !== null && typeof library === "object" && typeof library.project === "string" && typeof library.thread === "string"
    ? { path: value.path, pane: value.pane, library: { project: library.project, thread: library.thread } }
    : null;
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
