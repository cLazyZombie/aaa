// 변경 파일 카드의 트리·숫자(plan §8.305). t3code 4b1024da `apps/web/src/lib/turnDiffTree.ts`와
// `components/chat/DiffStatLabel.tsx:4-20`을 그대로 옮겼다. 다른 곳: 숫자 줄임이 반올림으로 1000이 되면 다음 단위로 올린다
// (t3code는 999,950을 `1000k`로 쓴다).
import type { DiffFile } from "./gen/proto";

export interface DiffStat {
  additions: number;
  deletions: number;
}

export interface DiffTreeDirectory {
  kind: "directory";
  name: string;
  path: string;
  stat: DiffStat;
  children: DiffTreeNode[];
}

export interface DiffTreeFile {
  kind: "file";
  name: string;
  path: string;
  /** 바이너리면 없다 */
  stat: DiffStat | null;
}

export type DiffTreeNode = DiffTreeDirectory | DiffTreeFile;

interface MutableDirectory {
  name: string;
  path: string;
  stat: DiffStat;
  directories: Map<string, MutableDirectory>;
  files: DiffTreeFile[];
}

const SORT: Intl.CollatorOptions = { numeric: true, sensitivity: "base" };

export function hasNonZeroStat(stat: DiffStat): boolean {
  return stat.additions > 0 || stat.deletions > 0;
}

const UNITS: [number, string][] = [[1e9, "b"], [1e6, "m"], [1e3, "k"]];

/** 1000 밑은 그대로, 위는 k·m·b(10 밑은 소수 한 자리, 끝의 .0은 뺀다) */
export function formatCompactDiffCount(value: number): string {
  if (value < 1000) return String(value);
  for (let i = 0; i < UNITS.length; i++) {
    const [size, unit] = UNITS[i];
    if (value < size) continue;
    const n = value / size;
    const text = n < 10 ? n.toFixed(1).replace(/\.0$/, "") : String(Math.round(n));
    // 반올림해 1000이 되면 다음 단위(999,950 → 1m)
    if (text === "1000" && i > 0) return `1${UNITS[i - 1][1]}`;
    return `${text}${unit}`;
  }
  return String(value);
}

function segments(path: string): string[] {
  return path.replaceAll("\\", "/").split("/").filter((s) => s.length > 0);
}

function byName(a: { name: string }, b: { name: string }): number {
  return a.name.localeCompare(b.name, undefined, SORT);
}

function readStat(file: DiffFile): DiffStat | null {
  return typeof file.additions === "number" && typeof file.deletions === "number" ? { additions: file.additions, deletions: file.deletions } : null;
}

/** 자식이 폴더 하나뿐인 폴더는 `a/b/c` 한 줄로(파일이 섞이면 합치지 않는다) */
function compact(node: DiffTreeDirectory): DiffTreeDirectory {
  let n: DiffTreeDirectory = { ...node, children: node.children.map((c) => (c.kind === "directory" ? compact(c) : c)) };
  while (n.children.length === 1 && n.children[0]?.kind === "directory") {
    const only = n.children[0];
    n = { kind: "directory", name: `${n.name}/${only.name}`, path: only.path, stat: only.stat, children: only.children };
  }
  return n;
}

function toNodes(dir: MutableDirectory): DiffTreeNode[] {
  const dirs = [...dir.directories.values()]
    .sort(byName)
    .map<DiffTreeDirectory>((d) => ({ kind: "directory", name: d.name, path: d.path, stat: { ...d.stat }, children: toNodes(d) }))
    .map(compact);
  return [...dirs, ...[...dir.files].sort(byName)];
}

export function summarizeDiffStats(files: readonly DiffFile[]): DiffStat {
  return files.reduce<DiffStat>((acc, f) => {
    const s = readStat(f);
    return s ? { additions: acc.additions + s.additions, deletions: acc.deletions + s.deletions } : acc;
  }, { additions: 0, deletions: 0 });
}

/** 폴더가 먼저, 이름은 자연수 차례·대소문자 무시. 폴더 숫자는 그 아래 파일의 합 */
export function buildDiffTree(files: readonly DiffFile[]): DiffTreeNode[] {
  const root: MutableDirectory = { name: "", path: "", stat: { additions: 0, deletions: 0 }, directories: new Map(), files: [] };
  for (const file of files) {
    const parts = segments(file.path);
    const name = parts.at(-1);
    if (name === undefined) continue;
    const stat = readStat(file);
    const ancestors: MutableDirectory[] = [root];
    let current = root;
    for (const part of parts.slice(0, -1)) {
      let next = current.directories.get(part);
      if (!next) {
        next = { name: part, path: current.path ? `${current.path}/${part}` : part, stat: { additions: 0, deletions: 0 }, directories: new Map(), files: [] };
        current.directories.set(part, next);
      }
      current = next;
      ancestors.push(current);
    }
    current.files.push({ kind: "file", name, path: parts.join("/"), stat });
    if (stat) {
      for (const a of ancestors) {
        a.stat.additions += stat.additions;
        a.stat.deletions += stat.deletions;
      }
    }
  }
  return toNodes(root);
}

export interface DiffTreeRow {
  node: DiffTreeNode;
  depth: number;
}

/** 보이는 줄: 펼친 폴더 안만. `overrides`는 폴더마다 사람이 바꾼 것, 없으면 `allExpanded`(t3code 기본 접힘) */
export function visibleRows(nodes: readonly DiffTreeNode[], overrides: Record<string, boolean>, allExpanded: boolean, depth = 0): DiffTreeRow[] {
  const rows: DiffTreeRow[] = [];
  for (const node of nodes) {
    rows.push({ node, depth });
    if (node.kind === "directory" && (overrides[node.path] ?? allExpanded)) rows.push(...visibleRows(node.children, overrides, allExpanded, depth + 1));
  }
  return rows;
}

export function directoryPaths(nodes: readonly DiffTreeNode[]): string[] {
  return nodes.flatMap((n) => (n.kind === "directory" ? [n.path, ...directoryPaths(n.children)] : []));
}
