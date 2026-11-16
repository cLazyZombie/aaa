// 기획 트리(t-1894): 저장소 기획 폴더의 Markdown 문서 목록을 폴더 트리로 묶고, 문서 안 상대 경로(그림·다른 문서)를 저장소 기준 경로로 푼다.
import type { DesignBranchView, DesignDocEntry, DesignEvent, DesignRepoView } from "./gen/proto";

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

/** 그림·영상 주소(host가 그 저장소 머신 agent에서 읽는다). commit을 실어 같은 판의 파일을 받는다. 기획 branch 판이면 그 slug도 싣는다 */
export function designFileUrl(project: string, repo: string, path: string, commit: string, branch?: string | null): string {
  const segs = path.split("/").map(encodeURIComponent).join("/");
  const q = [commit ? `c=${commit}` : null, branch ? `b=${encodeURIComponent(branch)}` : null].filter(Boolean).join("&");
  return `/api/v1/projects/${encodeURIComponent(project)}/design/${encodeURIComponent(repo)}/${segs}${q ? `?${q}` : ""}`;
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

/** 문서마다 그 문서를 main과 달리 고친 기획 branch(만든 차례) */
export function branchesByPath(repo: Pick<DesignRepoView, "branches"> | undefined): Map<string, DesignBranchView[]> {
  const out = new Map<string, DesignBranchView[]>();
  for (const b of repo?.branches ?? []) {
    for (const c of b.changed) {
      const list = out.get(c.path) ?? [];
      list.push(b);
      out.set(c.path, list);
    }
  }
  return out;
}

/** main에는 없고 branch에서 새로 만든 문서(트리에 더해 보인다). 같은 경로는 먼저 만든 branch 하나 */
export function addedDocs(repo: Pick<DesignRepoView, "branches" | "tree"> | undefined): { path: string; branch: DesignBranchView }[] {
  const main = new Set((repo?.tree?.docs ?? []).map((d) => d.path));
  const out = new Map<string, DesignBranchView>();
  for (const b of repo?.branches ?? []) {
    for (const c of b.changed) {
      if (c.status === "A" && isMarkdown(c.path) && !main.has(c.path) && !out.has(c.path)) out.set(c.path, b);
    }
  }
  return [...out].map(([path, branch]) => ({ path, branch }));
}

/** branch 이름(주제)의 꼴(host `design_slug_ok`와 같다): 영문 소문자·숫자·`-`, 1~48자, `-`로 시작·끝나지 않고 `--`가 없다. `main`은 판 고르기와 겹쳐 쓰지 않는다 */
export const SLUG = /^(?!main$)(?!.*--)[a-z0-9](?:[a-z0-9-]{0,46}[a-z0-9])?$/;

/** 주제에서 branch 이름을 짓는다(영문·숫자만 남긴다. 한글뿐이면 빈 글) */
export function slugSuggest(title: string): string {
  const s = title.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 48).replace(/-+$/, "");
  return s === "main" ? "main-1" : s;
}

/** markdown 최상위 덩어리: 빈 줄로 나누고, 코드 울타리(```·~~~) 안은 하나로 둔다 */
export function mdBlocks(text: string): string[] {
  const out: string[] = [];
  let cur: string[] = [];
  let fence: string | null = null;
  const flush = () => { if (cur.length) out.push(cur.join("\n")); cur = []; };
  for (const line of text.split("\n")) {
    const mark = /^\s*(`{3,}|~{3,})/.exec(line)?.[1] ?? null;
    if (fence) {
      cur.push(line);
      // 같은 글자로 연 것보다 짧지 않은 울타리만 닫는다(```` 안의 ```는 글이다)
      if (mark && mark[0] === fence[0] && mark.length >= fence.length && line.trim() === mark) { fence = null; flush(); }
      continue;
    }
    if (mark) { flush(); fence = mark; cur.push(line); continue; }
    if (line.trim() === "") { flush(); continue; }
    // 제목 줄은 홀로 한 덩어리(바로 아래 글과 따로 표시한다)
    if (/^#{1,6}\s/.test(line)) { flush(); out.push(line); continue; }
    cur.push(line);
  }
  flush();
  return out;
}

/** branch 판의 덩어리마다 main에 같은 덩어리가 없으면 참(앞뒤 빈칸은 보지 않는다). main이 없으면(새 문서) 모두 참 */
export function changedBlocks(main: string | null, blocks: readonly string[]): boolean[] {
  if (main === null) return blocks.map(() => true);
  const have = new Set(mdBlocks(main).map((b) => b.trim()));
  return blocks.map((b) => !have.has(b.trim()));
}

/** 쉰 날 수(마지막으로 쓴 때부터) */
export function restDays(b: Pick<DesignBranchView, "updated_ms">, now: number): number {
  return Math.max(0, Math.floor((now - b.updated_ms) / 86_400_000));
}

const short = (commit: string | null | undefined) => (commit ?? "").slice(0, 7);
const clock = (ms: number) => new Date(ms).toLocaleTimeString("ko-KR", { hour: "2-digit", minute: "2-digit", hour12: false });
const fileOf = (path: string) => path.slice(path.lastIndexOf("/") + 1);

/** 대화에 끼우는 한 줄. 같은 자리의 thread 저장은 하나로 묶는다(「문서 2 고침 · 1 새로」) */
export type DesignMark = { key: string; after: string | null; kind: string; text: string };

export function designMarks(events: readonly DesignEvent[], slug: string): DesignMark[] {
  const out: DesignMark[] = [];
  let saves: { after: string | null; changed: Set<string>; created: Set<string>; commit: string; at: number } | null = null;
  const flush = () => {
    if (!saves) return;
    const parts = [saves.changed.size > 0 ? `문서 ${saves.changed.size} 고침` : null, saves.created.size > 0 ? `${saves.created.size} 새로` : null].filter(Boolean);
    out.push({ key: `save-${saves.at}`, after: saves.after, kind: "save", text: `${parts.join(" · ")} · ${short(saves.commit)}` });
    saves = null;
  };
  for (const e of events) {
    if (e.kind === "save" || e.kind === "create") {
      if (saves && saves.after !== (e.after ?? null)) flush();
      saves ??= { after: e.after ?? null, changed: new Set(), created: new Set(), commit: "", at: e.at_ms };
      (e.kind === "create" ? saves.created : saves.changed).add(e.text);
      saves.commit = e.commit ?? saves.commit;
      continue;
    }
    flush();
    const text = e.kind === "open" ? `새 대화 · design/${slug} · main ${short(e.commit)} 에서`
      : e.kind === "push" ? `origin에 올림 · ${clock(e.at_ms)}`
        : e.kind === "edit" ? `사람이 ${fileOf(e.text)} 고침 · ${short(e.commit)}`
          : e.kind === "apply" ? `main에 반영 · 착지 창 요청함 · ${clock(e.at_ms)}`
            : e.text;
    out.push({ key: `${e.kind}-${e.at_ms}`, after: e.after ?? null, kind: e.kind, text });
  }
  flush();
  return out;
}
