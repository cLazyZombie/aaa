import { parsePatchFiles, type FileDiffMetadata } from "@pierre/diffs";
import type { DiffView } from "./gen/proto";

// 맥락 줄 수가 달라도 실제 변경의 위치·내용은 같아야 한다. 읽는 사이 바뀐 파일을
// 옛 hunk와 섞으면 새 변경이 평범한 맥락으로 보이므로 hydration 전에 거절한다.
function changes(diff: FileDiffMetadata): string {
  return JSON.stringify(diff.hunks.flatMap((h) => h.hunkContent.flatMap((c) => c.type === "context" ? [] : [{
    old: Math.max(h.deletionStart - 1, 0) + c.deletionLineIndex - h.deletionLineIndex,
    next: Math.max(h.additionStart - 1, 0) + c.additionLineIndex - h.additionLineIndex,
    deleted: diff.deletionLines.slice(c.deletionLineIndex, c.deletionLineIndex + c.deletions),
    added: diff.additionLines.slice(c.additionLineIndex, c.additionLineIndex + c.additions),
  }])));
}

/** 전체 맥락 patch를 라이브러리의 loadDiffFiles가 받는 양쪽 원문으로 돌린다. */
export function diffContextFiles(diff: FileDiffMetadata, baseSha: string, view: DiffView) {
  if (view.truncated) throw new Error("파일이 너무 커서 맥락을 모두 읽지 못했다");
  if (!view.full_context) throw new Error("그 머신 agent가 전체 맥락 읽기를 지원하지 않는다(agent 갱신 필요)");
  const full = parsePatchFiles(view.patch ?? "", undefined, true).flatMap((p) => p.files).find((f) => f.name === diff.name && f.prevName === diff.prevName);
  if (!full || view.base_sha !== baseSha || changes(full) !== changes(diff)) {
    throw new Error("비교 중 파일이 바뀌었다. 다시 읽어야 한다");
  }
  return {
    oldFile: { name: diff.prevName ?? diff.name, contents: full.deletionLines.join("") },
    newFile: { name: diff.name, contents: full.additionLines.join("") },
  };
}
