// 글 속 파일 경로를 파일 보기로 여는 단추(herdr-web-ui `src/components/FilePathLink.tsx` aa624cb, 문구만 참고 ko).
import type { ReactNode } from "react";

import { splitFilePaths } from "./herdr/filePaths.ts";

/**
 * Text past this many characters stays plain: a whole tool output can be megabytes, and every
 * path in it would be an element.
 */
const FILE_LINK_LIMIT = 256 * 1024;
/** Nor does a text with more paths than this link any: a listing of thousands would be thousands of buttons. */
const FILE_LINK_COUNT = 500;

/** Whether `text` links its paths at all: short enough, and not a listing of thousands. */
export function linksFilePaths(text: string): boolean {
  return text.length <= FILE_LINK_LIMIT && splitFilePaths(text).filter((part) => typeof part !== "string").length <= FILE_LINK_COUNT;
}

/** A file path the viewer opens: a button that reads as the text or code it replaced. */
export function FilePathLink({ path, code, open }: { path: string; code: boolean; open: (path: string) => void }) {
  const label = code ? <code>{path}</code> : path;
  return (
    <button type="button" className={`markdown-file${code ? " is-code" : ""}`} title={`${path} 열기`} onClick={() => open(path.replace(/^@(?=\/|~\/|\.{1,2}\/)/, ""))}>
      {label}
    </button>
  );
}

/**
 * `text` with the file paths in it as links to the viewer (a tool's output, a log in a code
 * block): `/tmp/shot/t0.png (1600, 1000)` opens the picture. Plain where nothing can open one.
 */
export function linkFilePaths(text: string, open: ((path: string) => void) | null): ReactNode {
  if (open === null || text.length > FILE_LINK_LIMIT) return text;
  const parts = splitFilePaths(text);
  if (parts.length === 1 && typeof parts[0] === "string") return text;
  if (parts.filter((part) => typeof part !== "string").length > FILE_LINK_COUNT) return text;
  return parts.map((part, index) => (typeof part === "string" ? part : <FilePathLink key={index} path={part.path} code={false} open={open} />));
}
