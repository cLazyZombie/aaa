// 할 일 화면의 순수 규칙(할 일 보기 T1, plan §8.306): 행 상태, 거르기·검색, 활동 막대 배지, 하위 task 깊이. 편집(T2, §8.311)은
// 화면 동작 → host 동작(TaskOp), 「나중」 항목 나누기, 기록 차이. thread 연결(T3, §8.316)은 확인 필요 상태, coordinator에게 맡기는
// 글, 맡길 수 있는 task, 잇기·풀기 동작, 자동 정리 알림.
// host `tasks`(TaskBoard)는 TASKS.md를 그대로 읽은 것이고, 여기서는 판단하지 않고 보이는 차례와 표지만 정한다.
import type { TaskAction, TaskAnchor, TaskBoard, TaskLogEntry, TaskOp, TaskView, ThreadGroup } from "./gen/proto";

/** 행 상태(`data-task-state`): 형식 어긋남 ▲ > 확인 필요 ⚠(이어진 thread가 끝났는데 완료로 판정하지 못함, host `attention`) >
 * 진행 중 ◉(열린 thread와 이어짐) > 대기 ○ */
export type TaskState = "problem" | "check" | "run" | "wait";

/** 거르기 칩: 전체·내 일(me)·미할당·진행 중·확인 필요 */
export type TaskFilter = "all" | "mine" | "unassigned" | "running" | "check";

export const TASK_FILTERS: readonly { id: TaskFilter; label: string }[] = [
  { id: "all", label: "전체" },
  { id: "mine", label: "내 일" },
  { id: "unassigned", label: "미할당" },
  { id: "running", label: "진행 중" },
  { id: "check", label: "확인 필요" },
];

export const TASK_STATE_LABEL: Record<TaskState, string> = { problem: "형식 어긋남", check: "확인 필요", run: "진행 중", wait: "대기" };

/** thread id → 묶음(App이 가진 thread 목록). 없으면 모르는 thread다 */
export type ThreadGroups = ReadonlyMap<string, ThreadGroup>;

export function taskState(t: TaskView, groups: ThreadGroups): TaskState {
  if (t.problems.length > 0) return "problem";
  if (t.attention) return "check";
  const g = t.thread ? groups.get(t.thread) : undefined;
  return g !== undefined && g !== "resolved" ? "run" : "wait";
}

/** 미할당: 비움·`agent`(tasks.rs `Owner::Unassigned`). 형식 어긋남으로 owner를 읽지 못한 task는 미할당으로 세지 않는다 */
export function unassigned(t: TaskView): boolean {
  return (t.owner === null || t.owner === "agent") && !t.problems.some((p) => p.startsWith("owner 칸이 owner 꼴이 아님"));
}

/** owner 칩 글: 미할당·나·그 밖 원문 */
export function ownerLabel(t: TaskView): string | null {
  if (t.owner === "me") return "나";
  if (unassigned(t)) return "미할당";
  return t.owner;
}

const has = (needle: string, text: string | null | undefined) => (text ?? "").toLowerCase().includes(needle);

/** 검색(제목·메모·owner)과 거르기 칩에 맞나 */
export function taskMatches(t: TaskView, filter: TaskFilter, query: string, groups: ThreadGroups): boolean {
  if (filter === "mine" && t.owner !== "me") return false;
  if (filter === "unassigned" && !unassigned(t)) return false;
  if (filter === "running" && taskState(t, groups) !== "run") return false;
  if (filter === "check" && taskState(t, groups) !== "check") return false;
  const needle = query.trim().toLowerCase();
  if (needle === "") return true;
  return has(needle, t.title) || has(needle, t.owner) || t.notes.some((n) => has(needle, n));
}

/**
 * 활동 막대 「할 일」 배지(t-0469 §3.1): 사람이 볼 task 수. 확인 필요·형식 어긋남과, 이 기기가 할 일 화면에서 아직 보지 못한 `me`
 * task다. 진행 중·대기는 세지 않는다. `seenMine`이 없으면(이 기기가 처음 읽는다) 새 me task를 세지 않는다
 */
export function taskBadge(board: TaskBoard | null, seenMine: ReadonlySet<string> | null = null): number {
  if (!board) return 0;
  return board.tasks.filter((t) => t.problems.length > 0 || t.attention || (seenMine !== null && t.owner === "me" && !seenMine.has(mineKey(t)))).length;
}

/** 본 me task를 가리키는 열쇠(제목은 파일 안에서 하나다) */
export function mineKey(t: TaskView): string {
  return t.title.toLowerCase();
}

/** 이 판의 me task 열쇠(할 일 화면을 열면 모두 본 것이 된다) */
export function mineKeys(board: TaskBoard): string[] {
  return board.tasks.filter((t) => t.owner === "me").map(mineKey);
}

/** 하위 task 깊이(상위 task 차례를 따라 올라간다). 없는 상위는 거기서 멈춘다 */
export function taskDepth(t: TaskView, byLine: ReadonlyMap<number, TaskView>): number {
  let depth = 0;
  let up = t.parent;
  const seen = new Set<number>();
  while (up !== null && !seen.has(up)) {
    seen.add(up);
    const p = byLine.get(up);
    if (!p) break;
    depth += 1;
    up = p.parent;
  }
  return depth;
}

/** 목록(`##`)마다 맞는 task. 모든 목록을 파일 차례대로 둔다(거르기로 비면 `tasks`가 빈 목록) */
export function taskSections(board: TaskBoard, filter: TaskFilter, query: string, groups: ThreadGroups): { name: string; notes: string[]; total: number; tasks: TaskView[] }[] {
  return board.lists.map((l) => {
    const mine = board.tasks.filter((t) => t.list === l.name);
    return { name: l.name, notes: l.notes, total: mine.length, tasks: mine.filter((t) => taskMatches(t, filter, query, groups)) };
  });
}

/** 상세가 가리키는 task(제목은 파일 안에서 하나다: `--from-task`가 제목으로 찾는다). 같은 제목이 둘이면 줄이 같은 것 */
export function findTask(board: TaskBoard | null, key: { title: string; line: number } | null): TaskView | null {
  if (!board || !key) return null;
  const same = board.tasks.filter((t) => t.title === key.title);
  return same.find((t) => t.line === key.line) ?? same[0] ?? null;
}

// ───────────── 편집(할 일 편집 T2, plan §8.311) ─────────────
// host `task_edit`는 대상 task를 블록 hash로 찾는다(그사이 바뀌었으면 충돌). 여기서는 화면 동작을 host 동작(TaskOp)으로 바꾸기만 한다.

/** 동작의 대상(host가 이 hash의 블록을 찾는다) */
export function anchorOf(t: TaskView): TaskAnchor {
  return { list: t.list, title: t.title, block_sha256: t.block_sha256, line: t.line };
}

/** 같은 목록·같은 상위의 task(파일 차례) */
export function siblings(board: TaskBoard, t: TaskView): TaskView[] {
  return board.tasks.filter((x) => x.list === t.list && x.parent === t.parent);
}

/** 그 task 아래의 모든 하위 task 줄 */
export function descendants(board: TaskBoard, t: TaskView): Set<number> {
  const out = new Set<number>();
  let grew = true;
  while (grew) {
    grew = false;
    for (const x of board.tasks) {
      if (x.parent !== null && (x.parent === t.line || out.has(x.parent)) && !out.has(x.line)) {
        out.add(x.line);
        grew = true;
      }
    }
  }
  return out;
}

const move = (t: TaskView, place: { to_list?: string; before?: TaskView; parent?: TaskView }): TaskOp => ({
  op: "move",
  at: anchorOf(t),
  to_list: place.to_list ?? "",
  before: place.before ? anchorOf(place.before) : null,
  parent: place.parent ? anchorOf(place.parent) : null,
});

/** 그 자리(같은 상위)의 끝으로: 하위면 상위의 하위 끝, 아니면 목록 끝 */
function toEnd(board: TaskBoard, t: TaskView, of: TaskView): TaskOp {
  const parent = of.parent !== null ? board.tasks.find((x) => x.line === of.parent) : undefined;
  return parent ? move(t, { parent }) : move(t, { to_list: of.list });
}

export type MoveDir = "up" | "down" | "top";

/** 행 메뉴의 위로·아래로·맨 위로(휴대폰은 끌기 대신 이것, t-0469 §3.3). 그 자리면 없음 */
export function moveOp(board: TaskBoard, t: TaskView, dir: MoveDir): TaskOp | null {
  const sibs = siblings(board, t);
  const i = sibs.findIndex((x) => x.line === t.line);
  if (i < 0) return null;
  if (dir === "up" || dir === "top") return i > 0 ? move(t, { before: sibs[dir === "up" ? i - 1 : 0] }) : null;
  if (i >= sibs.length - 1) return null;
  return i + 2 < sibs.length ? move(t, { before: sibs[i + 2] }) : toEnd(board, t, t);
}

/** 끌어 놓기: `target` 앞이나 뒤(그 task와 같은 상위가 된다). 자기·자기 하위 위, 지금 자리면 없음 */
export function dropOp(board: TaskBoard, dragged: TaskView, target: TaskView, side: "before" | "after"): TaskOp | null {
  if (target.line === dragged.line || descendants(board, dragged).has(target.line)) return null;
  const sibs = siblings(board, target).filter((x) => x.line !== dragged.line);
  const i = sibs.findIndex((x) => x.line === target.line);
  const next = side === "before" ? sibs[i] : sibs[i + 1];
  const same = siblings(board, dragged);
  const at = same.findIndex((x) => x.line === dragged.line);
  // 같은 자리에 다시 놓았다
  if (dragged.list === target.list && dragged.parent === target.parent && (next ? same[at + 1]?.line === next.line : at === same.length - 1)) return null;
  return next ? move(dragged, { before: next }) : toEnd(board, dragged, target);
}

/** 목록 머리에 놓기·「다른 목록으로」: 그 목록 끝(맨 위 단계) */
export function toListOp(t: TaskView, list: string): TaskOp | null {
  return t.list === list && t.parent === null ? null : move(t, { to_list: list });
}

/** owner 칸 고르기: 미할당·나·agent(profile@machine)·사람 이름 */
export type OwnerKind = "none" | "me" | "agent" | "person";

/** 편집 폼의 값 */
export interface TaskDraft {
  title: string;
  list: string;
  ownerKind: OwnerKind;
  /** agent owner(`claude`·`@mac-b`·`codex@mac-b`) */
  agent: string;
  person: string;
  notes: string;
  /** 상위 task(고른 판의 블록 고정점, 없으면 맨 위 단계). 줄 번호로 들면 그사이 줄이 밀렸을 때 다른 task 밑으로 간다 */
  parent: TaskAnchor | null;
  /** 새 task에 이을 thread(「task로 만들기」, T3) */
  thread: string | null;
}

/** 이 owner 글을 agent로 볼까: 맡길 수 있는 profile 목록에 있거나 `@`가 든 것. 그 밖의 이름은 사람이다 */
export function ownerKindOf(owner: string | null, agents: readonly string[]): OwnerKind {
  if (owner === null || owner === "" || owner === "agent") return "none";
  if (owner === "me") return "me";
  return owner.includes("@") || agents.includes(owner) ? "agent" : "person";
}

/** 이 판(`board`)에서 t의 상위 task 고정점 */
export function parentAnchor(board: TaskBoard, t: TaskView): TaskAnchor | null {
  const p = t.parent !== null ? board.tasks.find((x) => x.line === t.parent) : undefined;
  return p ? anchorOf(p) : null;
}

export function draftOf(t: TaskView, agents: readonly string[], parent: TaskAnchor | null): TaskDraft {
  const kind = ownerKindOf(t.owner, agents);
  return {
    title: t.title,
    list: t.list,
    ownerKind: kind,
    agent: kind === "agent" ? (t.owner ?? "") : "",
    person: kind === "person" ? (t.owner ?? "") : "",
    notes: t.notes.join("\n"),
    parent,
    thread: null,
  };
}

export function emptyDraft(list: string, parent: TaskAnchor | null = null): TaskDraft {
  return { title: "", list, ownerKind: "none", agent: "", person: "", notes: "", parent, thread: null };
}

/** 폼 owner → host owner 글(빈 글은 미할당)과 「사람으로 저장」 */
export function draftOwner(d: TaskDraft): { owner: string; person: boolean } {
  switch (d.ownerKind) {
    case "me":
      return { owner: "me", person: false };
    case "agent":
      return { owner: d.agent.trim(), person: false };
    case "person":
      return { owner: d.person.trim(), person: true };
    default:
      return { owner: "", person: false };
  }
}

/** 메모 글 → 줄(끝 공백·앞뒤 빈 줄을 뗀다, host와 같다) */
export function noteLines(text: string): string[] {
  const lines = text.split("\n").map((l) => l.replace(/\s+$/, ""));
  while (lines.length > 0 && lines[lines.length - 1] === "") lines.pop();
  while (lines.length > 0 && lines[0] === "") lines.shift();
  return lines;
}

const sameNotes = (a: string[], b: string[]) => {
  const n = (x: string[]) => x.map((l) => l.trim()).filter((l) => l !== "");
  return JSON.stringify(n(a)) === JSON.stringify(n(b));
};

/**
 * 폼 → host 동작. 새 task는 더하기 하나, 있는 task는 바뀐 칸만 고치기와(목록·상위가 바뀌었으면) 옮기기다. 같은 요청의 옮기기는
 * 고치기가 바꾼 hash를 host가 따라간다. 바뀐 것이 없으면 빈 목록. `wasParent`는 편집을 시작한 판의 상위 고정점이다(상위를 바꿨는지
 * 블록 hash로 본다). 고른 상위가 그사이 바뀌었으면 host가 쓰지 않고 충돌로 돌려준다.
 */
export function draftOps(t: TaskView | null, d: TaskDraft, wasParent: TaskAnchor | null = null): TaskOp[] {
  const { owner, person } = draftOwner(d);
  const notes = noteLines(d.notes);
  const parent = d.parent;
  if (!t) {
    return [{ op: "add", list: parent ? parent.list : d.list.trim(), title: d.title.trim(), owner: owner || null, person, notes, parent, before: null, thread: d.thread }];
  }
  const ops: TaskOp[] = [];
  const title = d.title.trim() !== t.title ? d.title.trim() : null;
  const was = t.owner === "agent" ? "" : (t.owner ?? "");
  const ownerChanged = owner !== was;
  const notesChanged = !sameNotes(notes, t.notes);
  if (title !== null || ownerChanged || notesChanged) {
    ops.push({ op: "update", at: anchorOf(t), title, owner: ownerChanged ? owner : null, person: ownerChanged && person, notes: notesChanged ? notes : null, thread: null, done: null });
  }
  if ((parent?.block_sha256 ?? null) !== (wasParent?.block_sha256 ?? null) || (parent === null && d.list.trim() !== t.list)) {
    ops.push(parent ? { op: "move", at: anchorOf(t), to_list: "", before: null, parent } : move(t, { to_list: d.list.trim() }));
  }
  return ops;
}

/** 정규형 줄 미리보기(host `task_line`과 같은 꼴) */
export function canonicalLine(title: string, owner: string | null, thread: string | null, done = false): string {
  const o = owner ? ` (${owner})` : "";
  const th = thread ? ` · ${thread}` : "";
  return `- [${done ? "x" : " "}] ${title}${o}${th}`;
}

/** 「나중」 절의 줄을 항목으로 묶는다: 들여쓰지 않은 글머리(`- `·`* `·`1. `)가 새 항목이고, 들여쓴 줄·이어진 줄은 그 항목에 붙는다 */
export function noteItems(lines: readonly string[]): string[][] {
  const out: string[][] = [];
  for (const l of lines) {
    const starts = /^([-*]|\d+\.)\s/.test(l);
    if (l.trim() === "") {
      if (out.length > 0) out[out.length - 1].push(l);
      continue;
    }
    if (starts || out.length === 0) out.push([l]);
    else out[out.length - 1].push(l);
  }
  return out.map((item) => {
    while (item.length > 0 && item[item.length - 1].trim() === "") item.pop();
    return item;
  });
}

/** 「할 일로 옮기기」: 항목 첫 줄(글머리를 뗌)이 제목 후보, 나머지 줄이 메모. 제목은 한 줄 120자까지 자르고 넘친 것은 메모 앞에 둔다 */
export function itemToTask(item: readonly string[]): { title: string; notes: string[] } {
  const first = (item[0] ?? "").replace(/^([-*]|\d+\.)\s+/, "").trim();
  const rest = item.slice(1).map((l) => l.replace(/^ {2}/, ""));
  if ([...first].length <= 120) return { title: first, notes: rest };
  const cut = [...first].slice(0, 120).join("");
  return { title: cut, notes: [first, ...rest] };
}

export const TASK_ACTION_LABEL: Record<TaskAction, string> = {
  add: "더함",
  update: "고침",
  delete: "지움",
  move: "옮김",
  list_add: "목록 더함",
  list_rename: "목록 이름 바꿈",
  list_merge: "목록 합침",
  list_delete: "목록 지움",
  list_move: "목록 옮김",
  restore: "되살림",
  raw: "원문 편집",
  auto_clean: "자동 정리",
};

/** 되살릴 수 있는 기록(삭제·자동 정리, 아직 되살리지 않음) */
export function restorable(e: TaskLogEntry): boolean {
  return (e.action === "delete" || e.action === "auto_clean") && e.restored_by === null;
}

/** 전후 원문의 줄 차이(가장 긴 공통 줄 차례). 작은 블록용이다 */
export function lineDiff(before: string | null, after: string | null): { kind: "same" | "del" | "add"; text: string }[] {
  const a = before === null ? [] : before.split("\n");
  const b = after === null ? [] : after.split("\n");
  const n = a.length;
  const m = b.length;
  const lcs: number[][] = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--) lcs[i][j] = a[i] === b[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
  const out: { kind: "same" | "del" | "add"; text: string }[] = [];
  let i = 0;
  let j = 0;
  while (i < n || j < m) {
    if (i < n && j < m && a[i] === b[j]) {
      out.push({ kind: "same", text: a[i] });
      i++;
      j++;
    } else if (j < m && (i >= n || lcs[i][j + 1] >= lcs[i + 1][j])) out.push({ kind: "add", text: b[j++] });
    else out.push({ kind: "del", text: a[i++] });
  }
  return out;
}

/** 기록의 때(지금부터): 방금·N분 전·N시간 전·N일 전 */
export function ago(atMs: number, now: number): string {
  const s = Math.max(0, Math.floor((now - atMs) / 1000));
  if (s < 60) return "방금";
  if (s < 3600) return `${Math.floor(s / 60)}분 전`;
  if (s < 86_400) return `${Math.floor(s / 3600)}시간 전`;
  return `${Math.floor(s / 86_400)}일 전`;
}

// ───────────── thread 연결(할 일 T3, plan §8.316) ─────────────

/** 「coordinator에게 맡기기」가 coordinator 입력칸에 채우는 글(t-0469 §3.4, 사람이 보내기를 누른다). 「다시 맡기기」는 앞 thread와 까닭을 붙인다 */
export function handOffText(t: TaskView, again = false): string {
  if (!again) return `「${t.title}」 task를 thread로 맡겨 줘`;
  const why = t.attention ? `(앞 ${t.thread ?? "thread"}: ${t.attention})` : "";
  return `「${t.title}」 task를 thread로 다시 맡겨 줘${why}`;
}

/**
 * thread로 맡길 수 있는 task: owner가 미할당·agent이고(`me`·사람은 host가 거부한다, `--from-task`) 형식 어긋남이 없고 열린 thread와
 * 이어지지 않았다. agent·사람 구분은 맡길 수 있는 profile 목록으로 한다(`ownerKindOf`)
 */
export function startable(t: TaskView, groups: ThreadGroups, agents: readonly string[]): boolean {
  if (t.problems.length > 0 || taskState(t, groups) === "run") return false;
  const kind = ownerKindOf(t.owner, agents);
  return kind === "none" || kind === "agent";
}

/** owner 칸이 정한 profile·머신(`profile@머신`). host `--from-task`는 이와 다른 값을 거부하므로 「바로 시작」 창이 고르기를 잠근다 */
export function ownerFixes(owner: string | null): { profile: boolean; machine: boolean } {
  if (owner === null || owner === "" || owner === "agent" || owner === "me") return { profile: false, machine: false };
  const at = owner.indexOf("@");
  return at < 0 ? { profile: true, machine: false } : { profile: at > 0, machine: at < owner.length - 1 };
}

/** 「task에 잇기」·「연결만 풀기」: thread 칸만 바꾼다(빈 글은 풀기) */
export function linkOp(t: TaskView, thread: string): TaskOp {
  return { op: "update", at: anchorOf(t), title: null, owner: null, person: false, notes: null, thread, done: null };
}

/** 「task에 잇기」 고르기 목록: 형식 어긋남이 없고 열린 thread와 이어지지 않은 task(줄을 다시 쓰면 제목 검사를 거친다) */
export function linkCandidates(board: TaskBoard, groups: ThreadGroups, query: string): TaskView[] {
  const needle = query.trim().toLowerCase();
  return board.tasks.filter((t) => t.problems.length === 0 && taskState(t, groups) !== "run" && (needle === "" || has(needle, t.title) || t.notes.some((n) => has(needle, n))));
}

/** 아직 알리지 않은 자동 정리(되살리지 않은 것, `seen`보다 새 기록). 최신이 앞이다 */
export function newAutoCleans(log: readonly TaskLogEntry[], seen: number): TaskLogEntry[] {
  return log.filter((e) => e.action === "auto_clean" && e.restored_by === null && e.id > seen);
}

/** 기록의 가장 큰 id(처음 읽은 기기는 여기까지를 본 것으로 둔다) */
export function lastLogId(log: readonly TaskLogEntry[]): number {
  return log.reduce((n, e) => Math.max(n, e.id), 0);
}

// task 첨부(plan §8.319): 메모의 한 줄이 첨부 하나다. 그림 `![그림](uploads/<16 hex>.<ext>)`, 올린 문서 `[제목](uploads/<16 hex>.md|html)`,
// 보고서 안 문서 `[제목](library/<보고서>/<경로>.md|html)`, 바깥 주소 `[제목](https://…)`. 올린 파일은 프로젝트 uploads/에 있고(host가
// 내용 hash로 이름을 정한다) 메모 줄을 고쳐 붙이고 뗀다. 줄 가운데 섞인 참조는 메모 글로 둔다(Markdown이 그린다).
const IMAGE_LINE = /^\s*!\[([^\]\n]*)\]\((uploads\/[0-9a-f]{16}\.(?:png|jpg|gif|webp))\)\s*$/;
const LINK_LINE = /^\s*\[([^\]\n]+)\]\(([^()\s]+)\)\s*$/;
const UPLOAD_DOC = /^uploads\/[0-9a-f]{16}\.(?:md|html)$/;
const REPORT_DOC = /^library\/([a-z0-9_-][a-z0-9_.-]*)\/((?:[^/.][^/]*\/)*[^/.][^/]*\.(?:md|html|htm))$/i;
const URL_TARGET = /^https?:\/\/[^\s<>]+$/i;
/** image: 올린 그림, upload: 올린 문서, report: 보고서(library) 안 문서, url: 바깥 주소 */
export type TaskAttachmentKind = "image" | "upload" | "report" | "url";
export type TaskAttachment = { kind: TaskAttachmentKind; target: string; label: string };
/** 받는 그림 형식과 한 개 상한(host와 같다) */
export const ATTACH_TYPES = ["image/png", "image/jpeg", "image/gif", "image/webp"] as const;
export const DOC_ACCEPT = ".md,.markdown,.html,.htm,text/markdown,text/html";
export const ATTACH_MAX_BYTES = 16 * 1024 * 1024;

/** 줄 하나가 첨부면 그 첨부 */
export function attachmentOfLine(line: string): TaskAttachment | null {
  const img = IMAGE_LINE.exec(line);
  if (img) return { kind: "image", target: img[2] ?? "", label: img[1] ?? "" };
  const link = LINK_LINE.exec(line);
  if (!link) return null;
  const [, label = "", target = ""] = link;
  const kind: TaskAttachmentKind | null = UPLOAD_DOC.test(target) ? "upload" : REPORT_DOC.test(target) ? "report" : URL_TARGET.test(target) ? "url" : null;
  return kind ? { kind, target, label } : null;
}

/** 메모의 첨부들(줄 차례, 같은 대상은 한 번) */
export function attachmentsOf(notes: string): TaskAttachment[] {
  const out: TaskAttachment[] = [];
  for (const line of notes.split("\n")) {
    const a = attachmentOfLine(line);
    if (a && !out.some((x) => x.target === a.target)) out.push(a);
  }
  return out;
}

/** 첨부 줄을 뺀 메모(그린 메모는 첨부를 따로 보인다) */
export function withoutAttachments(notes: string): string {
  return noteLines(notes.split("\n").filter((l) => attachmentOfLine(l) === null).join("\n")).join("\n");
}

/** 메모에 적을 첨부 줄. 제목의 `[`·`]`·줄바꿈은 줄 꼴을 깨므로 바꾼다 */
export function attachmentLine(a: TaskAttachment): string {
  if (a.kind === "image") return `![그림](${a.target})`;
  const label = a.label.replace(/[[\]\s]+/g, " ").trim() || a.target;
  return `[${label}](${a.target})`;
}

/** 첨부 줄 하나를 메모 끝에 더한다. 이미 있으면 그대로 */
export function addAttachment(notes: string, a: TaskAttachment): string {
  if (attachmentsOf(notes).some((x) => x.target === a.target)) return notes;
  return notes === "" ? attachmentLine(a) : `${notes}\n${attachmentLine(a)}`;
}

/** 메모 입력칸이 보이는 글: 첨부 줄을 뺀 나머지(첨부는 첨부 칸이 보인다). 다듬지 않는다(치는 중인 빈 줄을 지키려고) */
export function memoText(notes: string): string {
  return notes.split("\n").filter((l) => attachmentOfLine(l) === null).join("\n");
}

/** 입력칸 글을 고친 메모: 글 뒤에 첨부 줄을 차례대로 둔다(첨부 줄이 메모 가운데 있었으면 끝으로 옮겨진다) */
export function withMemoText(notes: string, text: string): string {
  const lines = notes.split("\n").filter((l) => attachmentOfLine(l) !== null);
  if (lines.length === 0) return text;
  return text === "" ? lines.join("\n") : `${text}\n${lines.join("\n")}`;
}

/** 그 대상의 첨부 줄을 뗀다(파일은 uploads/에 남는다: 기록의 되살리기가 줄을 돌려놓으면 다시 보인다) */
export function removeAttachment(notes: string, target: string): string {
  return noteLines(notes.split("\n").filter((l) => attachmentOfLine(l)?.target !== target).join("\n")).join("\n");
}

/** 올린 파일의 주소 바탕(host `GET /api/v1/projects/<p>/uploads/<이름>`, 보기 권한도) */
export function uploadsBase(project: string): string {
  return `/api/v1/projects/${encodeURIComponent(project)}/uploads/`;
}
export function taskImageUrl(project: string, path: string): string {
  return uploadsBase(project) + encodeURIComponent(path.replace(/^uploads\//, ""));
}

/** 보고서 안 문서 대상의 (보고서 이름, 보고서 안 경로) */
export function reportDoc(target: string): { name: string; path: string } | null {
  const m = REPORT_DOC.exec(target);
  return m ? { name: m[1] ?? "", path: m[2] ?? "" } : null;
}

/** 붙인 글(주소·`library/…` 경로)을 링크 첨부로. 첨부 꼴이 아니면 null */
export function linkFromText(text: string, label?: string): TaskAttachment | null {
  const target = text.trim();
  if (/\s/.test(target)) return null;
  const kind: TaskAttachmentKind | null = REPORT_DOC.test(target) ? "report" : URL_TARGET.test(target) ? "url" : null;
  if (!kind) return null;
  const tail = target.replace(/[?#].*$/, "").replace(/\/+$/, "").split("/").pop() ?? target;
  return { kind, target, label: label?.trim() || (kind === "url" ? target.replace(/^https?:\/\//i, "") : tail) };
}

/** 파일 종류: 그림·md·html, 받지 않는 것은 null(브라우저가 md 형식을 비워 보내는 일이 흔해 이름도 본다) */
export function fileKind(f: { name: string; type: string }): "image" | "md" | "html" | null {
  if ((ATTACH_TYPES as readonly string[]).includes(f.type)) return "image";
  const ext = /\.([a-z0-9]+)$/i.exec(f.name)?.[1]?.toLowerCase() ?? "";
  if (f.type === "text/markdown" || ext === "md" || ext === "markdown") return "md";
  if (f.type === "text/html" || ext === "html" || ext === "htm") return "html";
  return null;
}

/** 클립보드·끌어 놓기·고르기로 온 파일 중 첨부할 것과, 뺀 까닭 */
export function pickAttachable(files: readonly { name: string; type: string; size: number }[]): { ok: number[]; skipped: string[] } {
  const ok: number[] = [];
  const skipped: string[] = [];
  files.forEach((f, i) => {
    if (fileKind(f) === null) skipped.push(`${f.name || "이름 없음"}: 그림(PNG·JPEG·GIF·WebP)·md·html만`);
    else if (f.size === 0 || f.size > ATTACH_MAX_BYTES) skipped.push(`${f.name || "이름 없음"}: 1바이트부터 16MiB까지`);
    else ok.push(i);
  });
  return { ok, skipped };
}
