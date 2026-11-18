// 기획 트리(t-1894): 저장소 기획 폴더의 Markdown 문서 목록을 폴더 트리로 묶고, 문서 안 상대 경로(그림·다른 문서)를 저장소 기준 경로로 푼다.
import { canonicalShortId } from "./appIds.ts";
import { imageMention, imagePaths } from "./herdr/composerImages.ts";
import type { DesignDocEntry, DesignEvent, TaskView, ThreadView } from "./gen/proto";

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



/** branch 이름(주제)의 꼴(host `design_slug_ok`와 같다): 영문 소문자·숫자·`-`, 1~48자, `-`로 시작·끝나지 않고 `--`가 없다. `main`은 판 고르기와 겹쳐 쓰지 않는다 */
export const SLUG = /^(?!main$)(?!.*--)[a-z0-9](?:[a-z0-9-]{0,46}[a-z0-9])?$/;

/** 주제에서 branch 이름을 짓는다(영문·숫자만 남긴다. 한글뿐이면 빈 글) */
export function slugSuggest(title: string): string {
  const s = title.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 48).replace(/-+$/, "");
  return s === "main" ? "main-1" : s;
}

/** 기획서 ID 표지 줄(`<!-- aaa-id: k-0012 -->`, t-1957)이면 정규형 ID. Rust `design_id_line`과 같은 규칙이다 */
export function idLineOf(line: string): string | null {
  const m = /^\s*<!--\s*aaa-id:\s*(k-\d+)\s*-->\s*$/.exec(line);
  return m ? canonicalShortId(m[1]!, "k") : null;
}

/** 그릴 글: 코드 울타리 밖의 ID 표지 줄을 뺀다(표지는 화면에 보이지 않고 머리의 ID로 보인다) */
export function withoutIdLines(text: string): string {
  let fence: string | null = null;
  return text.split("\n").filter((line) => {
    const mark = /^\s*(`{3,}|~{3,})/.exec(line)?.[1] ?? null;
    if (fence) {
      if (mark && mark[0] === fence[0] && mark.length >= fence.length && line.trim() === mark) fence = null;
      return true;
    }
    if (mark) { fence = mark; return true; }
    return idLineOf(line) === null;
  }).join("\n");
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
    // ID 표지 줄도 홀로 한 덩어리다(Rust `design_marks`와 같다): 그리지 않는다
    if (idLineOf(line) !== null) { flush(); out.push(line); continue; }
    cur.push(line);
  }
  flush();
  return out;
}



const short = (commit: string | null | undefined) => (commit ?? "").slice(0, 7);
const clock = (ms: number) => new Date(ms).toLocaleTimeString("ko-KR", { hour: "2-digit", minute: "2-digit", hour12: false });
const fileOf = (path: string) => path.slice(path.lastIndexOf("/") + 1);

/** 대화에 끼우는 한 줄. 같은 자리의 thread 저장은 하나로 묶는다(「문서 2 고침 · 1 새로」) */
export type DesignMark = { key: string; after: string | null; kind: string; text: string };

export function designMarks(events: readonly DesignEvent[]): DesignMark[] {
  const out: DesignMark[] = [];
  let saves: { after: string | null; changed: Set<string>; created: Set<string>; commit: string; at: number } | null = null;
  const flush = () => {
    if (!saves) return;
    const parts = [saves.changed.size > 0 ? `문서 ${saves.changed.size} 고침` : null, saves.created.size > 0 ? `${saves.created.size} 새로` : null].filter(Boolean);
    out.push({ key: `save-${saves.at}`, after: saves.after, kind: "save", text: `main에 올림 · ${parts.join(" · ")} · ${short(saves.commit)} · ${clock(saves.at)}` });
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
    // 옛 기록(t-2002 전 기획 branch)의 사건 글도 그대로 그린다
    const text = e.kind === "open" ? `새 대화 · main ${short(e.commit)}`
      : e.kind === "push" ? `origin에 올림 · ${clock(e.at_ms)}`
        : e.kind === "edit" ? (/\.(png|jpe?g|gif|webp|mp4|webm)$/i.test(e.text) ? `그림 ${fileOf(e.text)} 붙임 · ${short(e.commit)}`
          : / 되돌림 ← /.test(e.text) ? `${e.text} · ${short(e.commit)}` : `사람이 ${fileOf(e.text)} 고침 · main ${short(e.commit)} · ${clock(e.at_ms)}`)
          : e.kind === "apply" ? `main에 반영 · 착지 창 요청함 · ${clock(e.at_ms)}`
          : e.kind === "apply_wait" ? `main에 반영 예약 · 자리 나면 시작 · ${clock(e.at_ms)}`
          : e.kind === "apply_cancel" ? `반영 예약 취소 · ${clock(e.at_ms)}`
          : e.kind === "apply_fail" ? `예약한 반영을 시작하지 못함 · ${e.text}`
            : e.kind === "handoff" ? `구현 맡기기 ${e.text} · main에 표지 ${short(e.commit)}`
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

/** 트리 저장소 필터(t-1946): 이 브라우저에만 남기는 열쇠. 값은 고른 저장소 이름의 JSON 배열이고 없으면 전부 */
export const REPO_FILTER_KEY = "herdr-web-ui:design-repos";

/** 남긴 값 읽기: 없거나·깨졌거나·빈 배열이면 null(전부) */
export function parseRepoFilter(raw: string | null): string[] | null {
  if (!raw) return null;
  try {
    const v: unknown = JSON.parse(raw);
    if (!Array.isArray(v)) return null;
    const names = [...new Set(v.filter((x): x is string => typeof x === "string" && x !== ""))];
    return names.length > 0 ? names : null;
  } catch { return null; }
}

/** 트리에 보일 저장소 이름. 고른 것 중 지금 목록에 있는 것만 보이고, 하나도 없으면(저장소가 빠짐) 전부 */
export function shownRepos(names: readonly string[], picked: readonly string[] | null): Set<string> {
  const hit = picked ? names.filter((n) => picked.includes(n)) : [];
  return new Set(hit.length > 0 ? hit : names);
}

/** 칩 누름 뒤 고른 것. `name`이 null이면 「전체」. 전부 보이는 중에 누르면 그 저장소만, 다 고르거나 다 빼면 전체(null).
 *  지금 목록에 없는 저장소(머신이 잠시 빠짐)는 누름으로 지우지 않는다 */
export function pickRepo(names: readonly string[], picked: readonly string[] | null, name: string | null): string[] | null {
  if (name === null) return null;
  const now = names.filter((n) => picked?.includes(n));
  if (now.length === 0) return [name];
  const next = now.includes(name) ? (picked ?? []).filter((n) => n !== name) : [...(picked ?? []), name];
  const on = names.filter((n) => next.includes(n));
  return on.length === 0 || on.length === names.length ? null : next;
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


/** 고를 수 있는 진행 모델: 그 머신이 맡을 수 있는 profile(`assignable`). 지금 값이 목록에 없으면 앞에 둔다 */
export function modelChoices(owners: readonly { machine: string; profiles: string[] }[], hostMachine: string | null, chosen: string): string[] {
  const usable = owners.find((o) => o.machine === hostMachine)?.profiles ?? [];
  return [...new Set([chosen, ...usable])];
}

/** 옛 할 일 메모의 아이디어 링크(`ideas/n-0001`) → `n-0001` */
export function ideaLinkId(path: string): string | null {
  const m = /^ideas\/(n-\d+)$/.exec(path.trim());
  return m ? m[1]! : null;
}

/** 대화의 저장 수(그 대화가 main에 올린 저장·사람 고침·맡김, t-2002) */
export function savesOf(events: readonly DesignEvent[]): number {
  return events.filter((e) => e.kind === "save" || e.kind === "create" || e.kind === "edit" || e.kind === "handoff" || e.kind === "remove").length;
}

/** 이력 보기(t-2002): 옛 판 덩어리와 지금 덩어리를 차례대로 맞춘다(LCS). `back`은 되돌리면 생기는 덩어리(옛 판에만),
 *  `gone`은 되돌리면 사라지는 덩어리(지금 판에만), `same`은 둘 다에 있다 */
export function blockDiff(old: readonly string[], now: readonly string[]): { text: string; kind: "same" | "back" | "gone" }[] {
  const n = old.length, m = now.length;
  // 문서 하나라 덩어리 수가 작다(수백). 넘치면 같은 줄 비교 없이 통째로 보인다
  if (n * m > 4_000_000) return [...old.map((text) => ({ text, kind: "back" as const })), ...now.map((text) => ({ text, kind: "gone" as const }))];
  const lcs: number[][] = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--) lcs[i]![j] = old[i] === now[j] ? lcs[i + 1]![j + 1]! + 1 : Math.max(lcs[i + 1]![j]!, lcs[i]![j + 1]!);
  const out: { text: string; kind: "same" | "back" | "gone" }[] = [];
  let i = 0, j = 0;
  while (i < n && j < m) {
    if (old[i] === now[j]) { out.push({ text: old[i]!, kind: "same" }); i++; j++; }
    else if (lcs[i + 1]![j]! >= lcs[i]![j + 1]!) out.push({ text: old[i++]!, kind: "back" });
    else out.push({ text: now[j++]!, kind: "gone" });
  }
  while (i < n) out.push({ text: old[i++]!, kind: "back" });
  while (j < m) out.push({ text: now[j++]!, kind: "gone" });
  return out;
}
