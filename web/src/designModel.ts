// 기획 트리(t-1894): 저장소 기획 폴더의 Markdown 문서 목록을 폴더 트리로 묶고, 문서 안 상대 경로(그림·다른 문서)를 저장소 기준 경로로 푼다.
import { canonicalShortId } from "./appIds.ts";
import { imageMention, imagePaths } from "./herdr/composerImages.ts";
import type { DesignBranchView, DesignDocEntry, DesignEvent, DesignRepoView, TaskView, ThreadView } from "./gen/proto";

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
  // CRLF 문서도 같은 덩어리다(agent `count_marks`와 같은 규칙)
  for (const line of text.split(/\r?\n/)) {
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
        : e.kind === "edit" ? (/\.(png|jpe?g|gif|webp|mp4|webm)$/i.test(e.text) ? `그림 ${fileOf(e.text)} 붙임 · ${short(e.commit)}` : `사람이 ${fileOf(e.text)} 고침 · ${short(e.commit)}`)
          : e.kind === "apply" ? `main에 반영 · 착지 창 요청함 · ${clock(e.at_ms)}`
            : e.kind === "handoff" ? `구현 맡김 ${e.text} · ${short(e.commit)}`
            : e.kind === "relay" ? `넘긴 블록 고침 ${e.text} · ${short(e.commit)}`
            : e.text;
    out.push({ key: `${e.kind}-${e.at_ms}`, after: e.after ?? null, kind: e.kind, text });
  }
  flush();
  return out;
}

// 3단계 블록 표지(t-1894, 확정 계획 §3): `> [!계획]`·`> [!계획] d-NNNN`·`> [!미정]` 블록과 그 상태

/** 표지 블록 하나: 종류, 넘긴 할 일, 줄 이름(첫 굵은 낱말, 없으면 첫 줄), 표지를 뺀 본문, 속한 절, 그림이 있나 */
export type Callout = {
  kind: "plan" | "open"; task: string | null; title: string; body: string; section: string; image: boolean;
  /** main 문서에서 맡겨 표지가 아직 구현 branch에만 있다(3-3, 맡김 기록으로 단 할 일) */
  pending?: boolean;
  /** 기획 branch에서 맡겼으면 그 주제(표지는 그 branch에), 비면 main 맡김(표지는 구현 branch에) */
  pendingBranch?: string;
};

/** 덩어리 첫 줄의 표지: 종류, 넘긴 d-ID(계획만, 정규형), 표지 뒤 글. 정규식 되돌기 없이 글자로 읽는다(agent `count_marks`와 같다) */
function markOf(line: string): { kind: "plan" | "open"; task: string | null; rest: string } | null {
  const t = line.trimStart();
  if (!t.startsWith(">")) return null;
  const head = t.slice(1).trimStart();
  if (head.startsWith("[!미정]")) return { kind: "open", task: null, rest: head.slice(5).trim() };
  if (!head.startsWith("[!계획]")) return null;
  const rest = head.slice(5).trimStart();
  const id = /^d-(\d+)/.exec(rest);
  if (!id) return { kind: "plan", task: null, rest: rest.trim() };
  return { kind: "plan", task: canonicalShortId(id[0], "d"), rest: rest.slice(id[0].length).trim() };
}

/** 그림 줄(`![…](`, 한 줄 안): 글자를 한 번씩만 본다(되돌기 없음) */
function hasImage(text: string): boolean {
  let open = false;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (c === "!" && text[i + 1] === "[") { open = true; i++; }
    else if (c === "\n") open = false;
    else if (c === "]") { if (open && text[i + 1] === "(") return true; open = false; }
  }
  return false;
}

/** 덩어리가 표지 블록이면 그 내용. 인용 줄(`>`)로만 된 덩어리여야 한다 */
export function calloutOf(block: string, section: string): Callout | null {
  const lines = block.split("\n");
  const head = markOf(lines[0] ?? "");
  if (!head || !lines.every((l) => l.trimStart().startsWith(">"))) return null;
  const body = [head.rest, ...lines.slice(1).map((l) => l.trimStart().slice(1).replace(/^\s/, ""))].join("\n").trim();
  const end = body.startsWith("**") ? body.indexOf("**", 2) : -1;
  const bold = end > 2 && !body.slice(2, end).includes("\n") ? body.slice(2, end) : "";
  const first = body.split("\n").find((l) => l.trim() !== "") ?? "";
  const plain = first.replace(/!\[[^\]\n]{0,300}\]\([^)\n]{0,1000}\)/g, "").replace(/[*_`]/g, "");
  const title = (bold || plain).trim().slice(0, 80) || section;
  return { kind: head.kind, task: head.task, title, body, section, image: hasImage(body) };
}

/** 제목 줄(`#`~`######` 뒤 빈칸)의 글, 아니면 null. 닫는 `#`은 뺀다 */
export function headingOf(line: string): string | null {
  const n = line.length - line.replace(/^#+/, "").length;
  if (n < 1 || n > 6 || !/^\s/.test(line.slice(n))) return null;
  return line.slice(n).trim().replace(/\s#+$/, "").replace(/^#+$/, "").trim();
}

/** 덩어리마다 속한 절(가장 가까운 앞 제목, 없으면 문서 제목) */
export function sectionsOf(blocks: readonly string[], fallback: string): string[] {
  let now = fallback;
  return blocks.map((b) => {
    const h = headingOf(b);
    if (h !== null) now = h;
    return now;
  });
}

/** 넘긴 블록의 지금 상태(할 일·thread에서 읽는다): 칩 색과 글 */
export type CalloutState = { tone: "wait" | "work" | "review" | "ready" | "landing" | "stale" | "missing"; label: string };

/** 보드에 없는 d-ID를 보관에서 찾은 결과. 둘 다 아니면 아직 모른다(읽는 중·읽지 못함) */
export type ArchiveLookup = { archived: ReadonlySet<string>; missing: ReadonlySet<string> };
const NO_LOOKUP: ArchiveLookup = { archived: new Set(), missing: new Set() };

export function taskState(id: string, tasks: readonly TaskView[] | null, threads: readonly ThreadView[], lookup: ArchiveLookup = NO_LOOKUP): CalloutState | null {
  if (tasks === null) return null;
  const task = tasks.find((t) => t.short_id === id);
  // 끝난 할 일은 보관으로 옮겨진다: 표지가 남아 있으면 「표지 남음」, 보관에도 없으면 「할 일 없음」, 아직 모르면 칩 없이 번호만
  if (!task) {
    if (lookup.archived.has(id)) return { tone: "stale", label: `표지 남음 ${id}` };
    return lookup.missing.has(id) ? { tone: "missing", label: `할 일 없음 ${id}` } : null;
  }
  if (task.done) return { tone: "stale", label: `표지 남음 ${id}` };
  const thread = task.thread ? threads.find((t) => t.id === task.thread) : undefined;
  if (!thread || thread.status === "resolved" || thread.status === "failed") return { tone: "wait", label: `대기 ${id}` };
  if (thread.stage === "landing") return { tone: "landing", label: `착지 중 ${id} · ${thread.id}` };
  if (thread.stage === "merge_ready") return { tone: "ready", label: `병합 준비됨 ${id} · ${thread.id}` };
  if (thread.group === "ready_for_review" || thread.group === "waiting_on_you") return { tone: "review", label: `검수 대기 ${id} · ${thread.id}` };
  return { tone: "work", label: `작업 중 ${id} · ${thread.id}` };
}

/** 문서들의 표지 수: 진행(넘긴 계획, 같은 d-ID는 한 번)·계획(아직 안 넘긴 것)·미정. 폴더·저장소 줄은 그 아래 문서 전부다 */
export function markCounts(docs: readonly (Pick<DesignDocEntry, "plans" | "open" | "tasks"> | undefined)[]): { work: number; plan: number; open: number } {
  const ids = new Set<string>();
  let plan = 0, open = 0;
  for (const d of docs) {
    if (!d) continue;
    const tasks = d.tasks ?? [];
    for (const id of tasks) ids.add(id);
    plan += Math.max(0, (d.plans ?? 0) - tasks.length);
    open += d.open ?? 0;
  }
  return { work: ids.size, plan, open };
}

/** 문서 하나의 표지 블록에서 트리와 같은 꼴의 수(agent `DesignDocEntry.plans`·`open`·`tasks`) */
export function marksOf(callouts: readonly (Callout | null)[]): Pick<DesignDocEntry, "plans" | "open" | "tasks"> {
  return {
    plans: callouts.filter((c) => c?.kind === "plan").length,
    open: callouts.filter((c) => c?.kind === "open").length,
    tasks: callouts.flatMap((c) => (c?.task ? [c.task] : [])),
  };
}

/** 트리 마디 아래의 문서 전부 */
export function docsUnder(node: DesignNode): DesignDocEntry[] {
  return node.kind === "doc" ? [node.doc] : node.children.flatMap(docsUnder);
}

/** 보낸 말 앞의 맥락 줄(「(보는 문서: …)」·「(문서 고침: …)」·「(고른 블록: …)」)을 떼어 칩 글로. 말풍선에는 남은 글만 보인다 */
export type ContextChip = { label: string; value: string; path: boolean };

export function splitContext(text: string): { chips: ContextChip[]; text: string } {
  const lines = text.split("\n");
  const chips: ContextChip[] = [];
  while (lines.length > 1) {
    const m = /^\((보는 문서|문서 고침|고른 블록|그림): (.+)\)$/.exec(lines[0]!.trim());
    if (!m) break;
    if (m[1] === "고른 블록") chips.push({ label: "고른 블록", value: m[2]!, path: false });
    else if (m[1] === "그림") chips.push({ label: "그림", value: m[2]!.slice(m[2]!.lastIndexOf("/") + 1), path: true });
    else if (m[1] === "문서 고침") chips.push({ label: "사람 고침", value: m[2]!.split(", ").map((p) => p.slice(p.lastIndexOf("/") + 1)).join(", "), path: true });
    else chips.push({ label: "보는 문서", value: m[2]!.split("/").slice(1).join("/") || m[2]!, path: true });
    lines.shift();
  }
  return { chips, text: lines.join("\n") };
}

/** 기획 대화 입력창이 올린 그림(t-1950): 초안의 `@<저장소 경로>`를 빼고 그 경로들을 돌려준다. 보낼 말 앞에 「(그림: …)」으로 붙인다 */
export function takeImages(text: string): { paths: string[]; text: string } {
  const paths = imagePaths(text);
  if (paths.length === 0) return { paths, text };
  let rest = text;
  // 입력창이 넣은 꼴(`@경로 `) 그대로만 뗀다: 말의 빈칸·들여쓰기는 건드리지 않는다
  for (const p of paths) rest = rest.split(imageMention(p)).join("").split(`@${p}`).join("");
  return { paths, text: rest };
}

/** 기획 대화 그림을 둘 폴더(저장소 기준): 보는 문서의 폴더가 기획 폴더 안이면 거기, 아니면 기획 폴더 */
export function chatImageDir(folder: string, viewing: string | null): string {
  const dir = viewing && viewing.includes("/") ? viewing.slice(0, viewing.lastIndexOf("/")) : "";
  return dir === folder || dir.startsWith(`${folder}/`) ? dir : folder;
}

/** 그 대화의 commit이 모두 main에 들어갔다(t-1950): agent가 그 branch를 읽었고, 제 commit이 있었고(끝이 갈라진 자리가 아니다)
 * main보다 앞선 것이 없다. 읽지 못한 0(머신 끊김·시한)은 들어간 것이 아니다 */
export function mergedToMain(b: Pick<DesignBranchView, "state" | "ahead" | "head" | "base" | "read">): boolean {
  return b.state === "open" && b.read && b.ahead === 0 && !!b.head && !!b.base && b.head !== b.base;
}
