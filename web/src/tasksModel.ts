// 할 일 화면의 순수 규칙(할 일 보기 T1, plan §8.306): 행 상태, 거르기·검색, 활동 막대 배지, 하위 task 깊이. 편집(T2, §8.311)은
// 화면 동작 → host 동작(TaskOp), 「나중」 항목 나누기, 기록 차이. thread 연결(T3, §8.316)은 확인 필요 상태, coordinator에게 맡기는
// 글, 맡길 수 있는 task, 잇기·풀기 동작, 자동 정리 알림.
// host `tasks`(TaskBoard)는 TASKS.md를 그대로 읽은 것이고, 여기서는 판단하지 않고 보이는 차례와 표지만 정한다.
import type { TaskAction, TaskAnchor, TaskBoard, TaskLogEntry, TaskOp, TaskView, ThreadGroup, ThreadStartPreview } from "./gen/proto";

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

/**
 * 담당 칩 글: 미할당·나·사람 이름. agent에게 맡긴 task는 없음이다(모델·머신 칩이 따로 보인다, `agentParts`). 형식 어긋남으로 owner를
 * 읽지 못한 task도 없음이다
 */
export function ownerLabel(t: TaskView, agents: readonly string[] = []): string | null {
  if (t.owner === "me") return "나";
  if (unassigned(t)) return "미할당";
  return agentParts(t, agents) ? null : t.owner;
}

/** agent에게 맡긴 task의 모델·머신 칸(plan §8.335, 빈 칸은 null). agent 일이 아니면(미할당·나·사람) null */
export interface AgentParts {
  profile: string | null;
  machine: string | null;
}

/**
 * owner → 모델·머신 칸. host가 나눠 준 값(FRONT_API 1.75 `TaskView.profile`·`machine`)을 쓰고, 옛 host(칸이 없다)면 `모델@머신` 글을
 * 나눈다(맨 이름은 맡길 수 있는 profile 목록에 있을 때만 모델, `ownerKindOf`)
 */
export function agentParts(t: Pick<TaskView, "owner" | "profile" | "machine">, agents: readonly string[] = []): AgentParts | null {
  if (t.profile != null || t.machine != null) return { profile: t.profile ?? null, machine: t.machine ?? null };
  if (t.profile === null && t.machine === null) return null;
  if (ownerKindOf(t.owner, agents) !== "agent") return null;
  const o = t.owner ?? "";
  const at = o.indexOf("@");
  return at < 0 ? { profile: o, machine: null } : { profile: at > 0 ? o.slice(0, at) : null, machine: o.slice(at + 1) || null };
}

/** 모델·머신 칸 → owner 글(host `tasks::agent_owner`): `모델@머신`·`모델`·`@머신`, 둘 다 비면 빈 글(미할당) */
export function agentOwner(profile: string, machine: string): string {
  const p = profile.trim();
  const m = machine.trim();
  return m ? `${p}@${m}` : p;
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

/** owner 글의 종류: 미할당·나·agent(모델·머신)·사람 이름 */
export type OwnerKind = "none" | "me" | "agent" | "person";

/** 편집 폼의 담당: agent(모델·머신 칸, 둘 다 비면 미할당)·나·사람 이름 */
export type DraftWho = "agent" | "me" | "person";

/** 편집 폼의 값 */
export interface TaskDraft {
  title: string;
  list: string;
  who: DraftWho;
  /** 모델(profile) 칸, 빈 글은 정하지 않음(시작할 때 프로젝트 기본) */
  profile: string;
  /** 머신 칸, 빈 글은 정하지 않음(시작할 때 그 저장소가 있는 머신 중 덜 붐비는 곳) */
  machine: string;
  person: string;
  notes: string;
  /** 상위 task(고른 판의 블록 고정점, 없으면 맨 위 단계). 줄 번호로 들면 그사이 줄이 밀렸을 때 다른 task 밑으로 간다 */
  parent: TaskAnchor | null;
  /** 새 task에 이을 thread(「task로 만들기」, T3) */
  thread: string | null;
  /** 저장소 칸(plan §8.330): 저장소 이름·`작업 폴더`, 빈 글은 정하지 않음 */
  repo: string;
}

/** 저장소 칸의 「저장소 없는 작업 폴더」(host `tasks::REPO_DIR`) */
export const REPO_DIR = "작업 폴더";

/** 저장소 경로의 마지막 이름(host `task_repo::repo_name`, `\`도 구분자) */
export function repoName(path: string): string {
  return path.replace(/[\/\\]+$/, "").split(/[\/\\]/).pop() ?? "";
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
  const parts = agentParts(t, agents);
  const kind = parts ? "agent" : ownerKindOf(t.owner, agents);
  return {
    title: t.title,
    list: t.list,
    who: kind === "me" || kind === "person" ? kind : "agent",
    profile: parts?.profile ?? "",
    machine: parts?.machine ?? "",
    person: kind === "person" ? (t.owner ?? "") : "",
    notes: t.notes.join("\n"),
    parent,
    thread: null,
    repo: t.repo ?? "",
  };
}

export function emptyDraft(list: string, parent: TaskAnchor | null = null): TaskDraft {
  return { title: "", list, who: "agent", profile: "", machine: "", person: "", notes: "", parent, thread: null, repo: "" };
}

/**
 * 폼 담당 → host owner 글(빈 글은 미할당)과 「사람으로 저장」. 모델·머신은 표기 그대로 `모델@머신` 한 칸으로 보낸다(옛 host도 같은
 * 판정을 한다)
 */
export function draftOwner(d: TaskDraft): { owner: string; person: boolean } {
  switch (d.who) {
    case "me":
      return { owner: "me", person: false };
    case "person":
      return { owner: d.person.trim(), person: true };
    default:
      return { owner: agentOwner(d.profile, d.machine), person: false };
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
    return [{ op: "add", list: parent ? parent.list : d.list.trim(), title: d.title.trim(), owner: owner || null, person, profile: null, machine: null, notes, parent, before: null, thread: d.thread, repo: d.repo || null }];
  }
  const ops: TaskOp[] = [];
  const title = d.title.trim() !== t.title ? d.title.trim() : null;
  const was = t.owner === "agent" ? "" : (t.owner ?? "");
  const ownerChanged = owner !== was;
  const notesChanged = !sameNotes(notes, t.notes);
  const repoChanged = d.repo !== (t.repo ?? "");
  if (title !== null || ownerChanged || notesChanged || repoChanged) {
    ops.push({ op: "update", at: anchorOf(t), title, owner: ownerChanged ? owner : null, person: ownerChanged && person, profile: null, machine: null, notes: notesChanged ? notes : null, thread: null, done: null, repo: repoChanged ? d.repo : null });
  }
  if ((parent?.block_sha256 ?? null) !== (wasParent?.block_sha256 ?? null) || (parent === null && d.list.trim() !== t.list)) {
    ops.push(parent ? { op: "move", at: anchorOf(t), to_list: "", before: null, parent } : move(t, { to_list: d.list.trim() }));
  }
  return ops;
}

/** 정규형 줄 미리보기(host `task_line`과 같은 꼴) */
export function canonicalLine(title: string, owner: string | null, repo: string | null, thread: string | null, done = false): string {
  const o = owner ? ` (${owner})` : "";
  const r = repo ? ` [${repo}]` : "";
  const th = thread ? ` · ${thread}` : "";
  return `- [${done ? "x" : " "}] ${title}${o}${r}${th}`;
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
  return unassigned(t) || agentParts(t, agents) !== null;
}

/**
 * task의 모델·머신 칸이 정한 것. host `--from-task`는 이와 다른 값을 거부하므로 「바로 시작」 창이 고르기를 잠근다. 옛 host(칸이
 * 없다)면 owner 글을 그대로 나눈다(사람은 맡길 수 없어 창이 열리지 않는다)
 */
export function ownerFixes(t: Pick<TaskView, "owner" | "profile" | "machine">): { profile: boolean; machine: boolean } {
  const owner = t.owner;
  if (owner === null || owner === "" || owner === "agent" || owner === "me") return { profile: false, machine: false };
  const parts = agentParts(t, [owner.split("@")[0]]);
  return { profile: !!parts?.profile, machine: !!parts?.machine };
}

/** 「바로 시작」의 저장소 이름마다 그 저장소가 있는 머신과 경로(plan §8.330). 머신이 빈 저장소 줄은 host 로컬 머신이다(없으면 뺀다) */
export function repoMachines(p: Pick<ThreadStartPreview, "repos" | "local_machine">): Map<string, { machine: string; path: string }[]> {
  const out = new Map<string, { machine: string; path: string }[]>();
  for (const r of p.repos) {
    const machine = r.machine ?? p.local_machine ?? null;
    const name = repoName(r.path);
    if (machine === null || name === "") continue;
    out.set(name, [...(out.get(name) ?? []), { machine, path: r.path }]);
  }
  return out;
}

/** 「바로 시작」 기본 머신: 덜 붐비는 차례(`machine_order`)에서 후보에 든 첫 머신, 아니면 후보가 하나면 그것, 아니면 빈 글(host가 고른다) */
export function defaultMachine(cands: readonly string[], order: readonly string[]): string {
  return order.find((m) => cands.includes(m)) ?? (cands.length === 1 ? cands[0] : "");
}

/** 「task에 잇기」·「연결만 풀기」: thread 칸만 바꾼다(빈 글은 풀기) */
export function linkOp(t: TaskView, thread: string): TaskOp {
  return { op: "update", at: anchorOf(t), title: null, owner: null, person: false, profile: null, machine: null, notes: null, thread, done: null, repo: null };
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

// task 첨부(plan §8.327): 메모의 한 줄이 첨부 하나다. 그림 `![그림](uploads/<16 hex>.<ext>)`, 올린 문서 `[제목](uploads/<16 hex>.md|html)`,
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
  let target = text.trim();
  if (/\s/.test(target)) return null;
  const kind: TaskAttachmentKind | null = REPORT_DOC.test(target) ? "report" : URL_TARGET.test(target) ? "url" : null;
  if (!kind) return null;
  // 괄호는 첨부 줄 `[제목](대상)`을 깬다: 주소는 풀어 쓴 꼴로 바꾸고, 괄호 든 보고서 경로는 받지 않는다
  if (kind === "url") target = target.replace(/\(/g, "%28").replace(/\)/g, "%29");
  else if (/[()]/.test(target)) return null;
  const tail = target.replace(/[?#].*$/, "").replace(/\/+$/, "").split("/").pop() ?? target;
  return { kind, target, label: label?.trim() || (kind === "url" ? target.replace(/^https?:\/\//i, "") : tail) };
}

/**
 * 붙인 주소가 aaa 보고서 주소면 그 꼴(plan §8.327): 보기 경로 `/v/<cap>/<경로>`(공유 링크·앱 보기 경로. cap 48자가 보고서를 가리므로
 * host 이름은 보지 않는다)와 이 앱의 `/#report=<프로젝트>/<이름>`(reportLinks `inAppLink`와 같은 꼴). 첨부 `library/<이름>/<경로>`로
 * 바꾸는 것은 host가 cap을 풀어 준 뒤다(TaskAttachments `resolveReportLink`)
 */
export type ReportUrl = { kind: "view"; cap: string; path: string } | { kind: "report"; project: string; name: string };
export function reportUrlOf(target: string, origin: string): ReportUrl | null {
  let url: URL;
  try {
    url = new URL(target);
  } catch {
    return null;
  }
  try {
    const view = /^\/v\/([0-9a-f]{48})\/(.*)$/.exec(url.pathname);
    if (view) return { kind: "view", cap: view[1]!, path: view[2]!.split("/").map(decodeURIComponent).join("/") };
    const report = /^#report=(.+)$/.exec(url.hash);
    if (url.origin !== origin || url.pathname !== "/" || !report) return null;
    const key = decodeURIComponent(report[1]!);
    const at = key.indexOf("/");
    return at > 0 && at < key.length - 1 ? { kind: "report", project: key.slice(0, at), name: key.slice(at + 1) } : null;
  } catch {
    return null;
  }
}

/** 보고서 안 문서의 첨부(「할 일에 추가」). 첨부 줄로 쓸 수 없는 경로(md·html이 아님, 공백·괄호)는 null: 그 줄에는 단추를 두지 않는다 */
export function reportDocLink(name: string, path: string): TaskAttachment | null {
  return linkFromText(`library/${name}/${path}`, path === "index.html" ? name : undefined);
}

/** 「할 일에 추가」(보고서·thread 결과물의 문서)로 링크를 붙일 task: 형식이 맞고 그 문서를 아직 링크하지 않은 것, 제목·메모 검색 */
export function docLinkCandidates(board: TaskBoard, link: TaskAttachment, query: string): TaskView[] {
  const needle = query.trim().toLowerCase();
  return board.tasks.filter(
    (t) =>
      t.problems.length === 0 &&
      !attachmentsOf(t.notes.join("\n")).some((a) => a.target === link.target) &&
      (needle === "" || has(needle, t.title) || t.notes.some((n) => has(needle, n))),
  );
}

/** 문서를 링크한 새 task의 처음 제목: 링크 이름에서 문서 확장자를 뺀 것 */
export function docTaskTitle(link: TaskAttachment): string {
  return link.label.replace(/\.(md|markdown|html?)$/i, "");
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
