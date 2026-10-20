// 변경 파일 카드(plan §8.305). t3code 4b1024da `apps/web/src/components/chat/ChangedFilesTree.tsx:31-252`와
// `DiffStatLabel.tsx:22-53`을 옮겼다: 「파일 N개 +a -d」 머리(붙어 있음), 폴더가 있으면 「모든 폴더 펼치기·접기」, 「diff 열기」
// (카드 폭 24rem 밑에서는 글 없이 아이콘), 폴더는 처음에 접힘(펼침 상태는 thread마다 기억), 파일을 누르면 그 파일로 diff를 연다.
// 다른 곳: 250줄 넘는 가상화는 하지 않았다(접힌 채 시작하고 줄은 단추 하나다). 기준 이름(`origin/main 대비`)을 머리에 적는다
// (t3code 카드는 턴 범위라 쓰지 않는다). 손가락 기기에서 줄은 40px이다.
import { ChevronRight, ChevronsDownUp, ChevronsUpDown, FileDiff, Folder, FolderClosed } from "lucide-react";
import { useState } from "react";

import type { DiffFile } from "./gen/proto";
import { buildDiffTree, directoryPaths, formatCompactDiffCount, hasNonZeroStat, summarizeDiffStats, visibleRows, type DiffTreeRow } from "./diffTree";
import { FileGlyph } from "./herdr/FileGlyph";

/** t3code `DiffStatLabel`: 「+a -d」(ASCII 붙임표), aligned는 두 칸을 4ch씩 오른쪽 맞춤 */
export function DiffStat({ additions, deletions, layout = "aligned" }: { additions: number; deletions: number; layout?: "aligned" | "inline" }) {
  return (
    <span className={`diff-stat ${layout}`} role="group" aria-label={`추가 ${additions}줄, 삭제 ${deletions}줄`}>
      <span aria-hidden="true" className="diff-add">+{formatCompactDiffCount(additions)}</span>
      <span aria-hidden="true" className="diff-del">-{formatCompactDiffCount(deletions)}</span>
    </span>
  );
}

const EMPTY: Record<string, boolean> = {};
export const PATH_ORDER = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

function expandKey(project: string, thread: string): string {
  return `aaa:diff-folders:${project}/${thread}`;
}

/** thread마다 「모든 폴더 펼치기」를 기억한다(t3code `uiStateStore` threadChangedFilesExpandedById). 없거나 막히면 접힘 */
export function readAllExpanded(project: string, thread: string): boolean {
  try {
    return window.localStorage.getItem(expandKey(project, thread)) === "1";
  } catch {
    return false;
  }
}

function writeAllExpanded(project: string, thread: string, value: boolean): void {
  try {
    if (value) window.localStorage.setItem(expandKey(project, thread), "1");
    else window.localStorage.removeItem(expandKey(project, thread));
  } catch {
    // 저장하지 못해도 이 화면 동안은 그대로다
  }
}

export function ChangedFilesCard({
  project,
  thread,
  base,
  files,
  onOpen,
}: {
  project: string;
  thread: string;
  /** 비교 기준 이름(`origin/main`) */
  base: string;
  files: readonly DiffFile[];
  /** diff를 연다. 경로가 있으면 그 파일로 */
  onOpen: (path?: string) => void;
}) {
  const [allExpanded, setAllExpanded] = useState(() => readAllExpanded(project, thread));
  const total = summarizeDiffStats(files);
  const hasDirectories = files.some((f) => /[/\\]/.test(f.path));
  const toggleAll = () => {
    writeAllExpanded(project, thread, !allExpanded);
    setAllExpanded(!allExpanded);
  };
  const allLabel = allExpanded ? "모든 폴더 접기" : "모든 폴더 펼치기";
  // t3code는 서버가 경로를 localeCompare로 늘어놓은 첫 파일을 연다(`Diffs.ts:35`). diff 패널도 같은 차례다
  const first = [...files].sort((a, b) => PATH_ORDER.compare(a.path, b.path))[0]?.path;
  return (
    <section className="card changed-files" data-testid="changed-files" data-files={files.length}>
      <div className="changed-files-head">
        <div className="changed-files-title">
          <span data-testid="changed-files-count">파일 {files.length}개</span>
          {hasNonZeroStat(total) && <DiffStat additions={total.additions} deletions={total.deletions} layout="inline" />}
          <span className="muted changed-files-base">{base} 대비</span>
        </div>
        <div className="changed-files-actions">
          {hasDirectories && (
            <button type="button" className="ghost icon changed-files-button" aria-label={allLabel} title={allLabel} onClick={toggleAll}>
              {allExpanded ? <ChevronsDownUp aria-hidden="true" /> : <ChevronsUpDown aria-hidden="true" />}
            </button>
          )}
          <button type="button" className="ghost changed-files-button changed-files-open" aria-label="diff 열기" title="전체 diff 열기" onClick={() => onOpen(first)}>
            <FileDiff aria-hidden="true" />
            <span className="changed-files-open-word">diff 열기</span>
          </button>
        </div>
      </div>
      <ChangedFilesTree key={String(allExpanded)} files={files} allExpanded={allExpanded} onOpen={onOpen} />
    </section>
  );
}

function ChangedFilesTree({ files, allExpanded, onOpen }: { files: readonly DiffFile[]; allExpanded: boolean; onOpen: (path: string) => void }) {
  // 폴더마다 바꾼 것은 「모두」 상태와 폴더 모양이 같은 동안만 쓴다(t3code와 같다)
  const [state, setState] = useState<{ key: string | null; overrides: Record<string, boolean> }>({ key: null, overrides: {} });
  const nodes = buildDiffTree(files);
  const dirKey = directoryPaths(nodes).join("\u0000");
  const hasDirectories = dirKey.length > 0;
  const key = `${allExpanded ? "expanded" : "collapsed"}\u0000${dirKey}`;
  const overrides = state.key === key ? state.overrides : EMPTY;
  const toggle = (path: string) =>
    setState((cur) => {
      const prev = cur.key === key ? cur.overrides : {};
      return { key, overrides: { ...prev, [path]: !(prev[path] ?? allExpanded) } };
    });
  const row = ({ node, depth }: DiffTreeRow) => {
    const pad = { paddingLeft: `${8 + depth * 14}px` };
    if (node.kind === "directory") {
      const open = overrides[node.path] ?? allExpanded;
      return (
        <button key={`dir:${node.path}`} type="button" className="changed-files-row dir" style={pad} aria-expanded={open} onClick={() => toggle(node.path)}>
          <ChevronRight aria-hidden="true" className={`changed-files-chevron${open ? " open" : ""}`} />
          {open ? <Folder aria-hidden="true" /> : <FolderClosed aria-hidden="true" />}
          <span className="changed-files-name dir">{node.name}</span>
          {hasNonZeroStat(node.stat) && <DiffStat additions={node.stat.additions} deletions={node.stat.deletions} />}
        </button>
      );
    }
    return (
      <button key={`file:${node.path}`} type="button" className="changed-files-row file" style={pad} data-path={node.path} onClick={() => onOpen(node.path)}>
        {(hasDirectories || depth > 0) && <span aria-hidden="true" className="changed-files-indent" />}
        <FileGlyph name={node.name} />
        <span className="changed-files-name">{node.name}</span>
        {node.stat && <DiffStat additions={node.stat.additions} deletions={node.stat.deletions} />}
      </button>
    );
  };
  return <div className="changed-files-tree">{visibleRows(nodes, overrides, allExpanded).map(row)}</div>;
}
