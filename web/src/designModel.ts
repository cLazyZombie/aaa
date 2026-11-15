// 기획 트리(t-1894): 저장소 기획 폴더의 Markdown 문서 목록을 폴더 트리로 묶고, 문서 안 상대 경로(그림·다른 문서)를 저장소 기준 경로로 푼다.
import type { DesignDocEntry } from "./gen/proto";

export type DesignDir = { kind: "dir"; name: string; path: string; title: string; children: DesignNode[] };
export type DesignFile = { kind: "doc"; name: string; doc: DesignDocEntry };
export type DesignNode = DesignDir | DesignFile;

const README = /^readme\.md$/i;

/** 폴더 하나 안을 그린다: 폴더 먼저(이름 차례), 그다음 문서(README 먼저, 이름 차례). 폴더 제목은 그 폴더 README의 제목 */
export function designTree(folder: string, docs: readonly DesignDocEntry[]): DesignNode[] {
  const root: DesignDir = { kind: "dir", name: "", path: folder, title: "", children: [] };
  const dirs = new Map<string, DesignDir>([[folder, root]]);
  const dirOf = (path: string): DesignDir => {
    const found = dirs.get(path);
    if (found) return found;
    const cut = path.lastIndexOf("/");
    const parent = dirOf(path.slice(0, cut));
    const dir: DesignDir = { kind: "dir", name: path.slice(cut + 1), path, title: "", children: [] };
    parent.children.push(dir);
    dirs.set(path, dir);
    return dir;
  };
  for (const doc of docs) {
    if (!doc.path.startsWith(`${folder}/`)) continue;
    const cut = doc.path.lastIndexOf("/");
    const dir = dirOf(doc.path.slice(0, cut));
    const name = doc.path.slice(cut + 1);
    dir.children.push({ kind: "doc", name, doc });
    if (README.test(name)) dir.title = doc.title;
  }
  const sort = (d: DesignDir) => {
    d.children.sort((a, b) => {
      if (a.kind !== b.kind) return a.kind === "dir" ? -1 : 1;
      const ra = README.test(a.name), rb = README.test(b.name);
      if (ra !== rb) return ra ? -1 : 1;
      return a.name.localeCompare(b.name);
    });
    for (const c of d.children) if (c.kind === "dir") sort(c);
  };
  sort(root);
  return root.children;
}

/** 문서 줄 이름: 제목, 없으면 확장자 뺀 파일 이름 */
export function docLabel(doc: DesignDocEntry): string {
  return doc.title || stem(doc.path);
}

/** 확장자 뺀 파일 이름(`README`, `skill-tree`) */
export function stem(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1).replace(/\.md$/i, "");
}

/** 그 경로를 품은 폴더 경로들(트리에서 펼칠 것) */
export function ancestors(path: string): string[] {
  const out: string[] = [];
  for (let i = path.indexOf("/"); i >= 0; i = path.indexOf("/", i + 1)) out.push(path.slice(0, i));
  return out;
}

/**
 * 문서 안 상대 경로를 저장소 기준 경로로 푼다. 주소(`https:` 등)·절대 경로·`#`만인 것·저장소 밖으로 나가는 것은 null.
 * `#절`·`?…`은 뗀다.
 */
export function resolveDocPath(docPath: string, href: string): string | null {
  const raw = href.trim();
  if (raw === "" || raw.startsWith("#") || raw.startsWith("/") || /^[a-z][a-z0-9+.-]*:/i.test(raw)) return null;
  let rel = raw.replace(/[?#].*$/, "");
  try { rel = decodeURIComponent(rel); } catch { /* 그대로 쓴다 */ }
  const parts = docPath.split("/").slice(0, -1);
  for (const seg of rel.split("/")) {
    if (seg === "" || seg === ".") continue;
    if (seg === "..") {
      if (parts.length === 0) return null;
      parts.pop();
    } else parts.push(seg);
  }
  return parts.length > 0 ? parts.join("/") : null;
}

/** 그림·영상 주소(host가 그 저장소 머신 agent에서 읽는다). commit을 실어 같은 판의 파일을 받는다 */
export function designFileUrl(project: string, repo: string, path: string, commit: string): string {
  const segs = path.split("/").map(encodeURIComponent).join("/");
  return `/api/v1/projects/${encodeURIComponent(project)}/design/${encodeURIComponent(repo)}/${segs}${commit ? `?c=${commit}` : ""}`;
}

/** 앱 주소의 기획 문서(`design=<저장소>/<경로>`) */
export function designTarget(repo: string, path: string): string {
  return `${repo}/${path}`;
}

export function splitTarget(target: string): { repo: string; path: string } | null {
  const cut = target.indexOf("/");
  if (cut <= 0 || cut === target.length - 1) return null;
  return { repo: target.slice(0, cut), path: target.slice(cut + 1) };
}

export function isMarkdown(path: string): boolean {
  return /\.md$/i.test(path);
}
