import { useCallback, useEffect, useRef, useState } from "react";
import { outsideSettings } from "./settingsHistory.ts";

// herdr-web-ui src/lib/useFileViewer.ts(aa624cb). aaa: pane ID가 host 전체에서 하나라 머신 ID 대신 pane만 둔다.
// thread 결과물(차이 목록 R1)은 pane 대신 그 thread(`library`)를 둔다: 경로는 결과물 안 상대 경로다.
// 보고서 안 파일 링크(plan §8.171)는 그 thread(`worktree`)를 둔다: 경로는 thread 작업 폴더 기준이다.
// 대화의 보고서 주소(plan §8.300)는 그 보기 경로 맨 위(`view`, `/v/<cap>/`)를 둔다: 경로는 그 안 상대 경로다.

type ThreadRef = { project: string; thread: string };
interface FilePreview {
  path: string;
  pane: string | null;
  library?: ThreadRef;
  worktree?: ThreadRef;
  view?: string;
  image?: File;
}

function threadRef(value: unknown): ThreadRef | null {
  if (value === null || typeof value !== "object") return null;
  const v = value as Record<string, unknown>;
  return typeof v.project === "string" && typeof v.thread === "string" ? { project: v.project, thread: v.thread } : null;
}
const HISTORY_KEY = "aaa:file-preview";

export function filePreviewState(state: unknown, preview: FilePreview): Record<string, unknown> {
  // 아직 올리는 그림은 이 문서의 메모리에만 둔다. 기록에는 뒤로 닫을 칸만 남긴다.
  return { ...outsideSettings(state), [HISTORY_KEY]: preview.image ? true : preview };
}

export function filePreviewEntry(state: unknown): FilePreview | true | null {
  if (state === null || typeof state !== "object") return null;
  const preview = (state as Record<string, unknown>)[HISTORY_KEY];
  if (preview === true) return true;
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
  if (value.view !== undefined) {
    if (typeof value.view !== "string" || !value.view.startsWith("/v/")) return null;
    shown.view = value.view;
  }
  return shown;
}

function restorablePreview(state: unknown): FilePreview | null {
  const entry = filePreviewEntry(state);
  return entry === true ? null : entry;
}

/** Android Back traverses history: a preview must be an entry above the chat, not just a modal. */
export function useFileViewer() {
  const [viewing, setViewing] = useState(() => restorablePreview(window.history.state));
  const closing = useRef(false);

  useEffect(() => {
    const onPop = (event: PopStateEvent): void => {
      closing.current = false;
      // 서버 파일은 앞으로 가거나 새로고침해도 원래 위치에서 다시 연다.
      setViewing(restorablePreview(event.state));
    };
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, []);

  const openFile = useCallback((preview: FilePreview) => {
    if (closing.current) return;
    // a preview opened over Settings is no entry of Settings: a reload on it closes Settings and keeps the file
    const previous = window.history.state;
    const state = filePreviewState(previous, preview);
    // Choosing a different file inside an open preview must not add another Back step.
    if (filePreviewEntry(previous) !== null) window.history.replaceState(state, "");
    else window.history.pushState(state, "");
    setViewing(preview);
  }, []);

  const closeFile = useCallback(() => {
    if (closing.current) return;
    if (filePreviewEntry(window.history.state) !== null) {
      closing.current = true;
      // X, Escape and scrim clicks consume the same entry as the system Back button.
      window.history.back();
    } else setViewing(null);
  }, []);

  return { viewing, openFile, closeFile };
}
