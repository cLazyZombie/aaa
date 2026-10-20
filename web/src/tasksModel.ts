// 할 일 화면의 순수 규칙(할 일 보기 T1, plan §8.305): 행 상태, 거르기·검색, 활동 막대 배지, 하위 task 깊이.
// host `tasks`(TaskBoard)는 TASKS.md를 그대로 읽은 것이고, 여기서는 판단하지 않고 보이는 차례와 표지만 정한다.
import type { TaskBoard, TaskView, ThreadGroup } from "./gen/proto";

/** 행 상태(`data-task-state`): 형식 어긋남 ▲ > 진행 중 ◉(열린 thread와 이어짐) > 대기 ○ */
export type TaskState = "problem" | "run" | "wait";

/** 거르기 칩: 전체·내 일(me)·미할당·진행 중 */
export type TaskFilter = "all" | "mine" | "unassigned" | "running";

export const TASK_FILTERS: readonly { id: TaskFilter; label: string }[] = [
  { id: "all", label: "전체" },
  { id: "mine", label: "내 일" },
  { id: "unassigned", label: "미할당" },
  { id: "running", label: "진행 중" },
];

export const TASK_STATE_LABEL: Record<TaskState, string> = { problem: "형식 어긋남", run: "진행 중", wait: "대기" };

/** thread id → 묶음(App이 가진 thread 목록). 없으면 모르는 thread다 */
export type ThreadGroups = ReadonlyMap<string, ThreadGroup>;

export function taskState(t: TaskView, groups: ThreadGroups): TaskState {
  if (t.problems.length > 0) return "problem";
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
  const needle = query.trim().toLowerCase();
  if (needle === "") return true;
  return has(needle, t.title) || has(needle, t.owner) || t.notes.some((n) => has(needle, n));
}

/** 활동 막대 「할 일」 배지: 형식 어긋남 task 수(확인 필요·새 me task는 T3에서 더한다) */
export function taskBadge(board: TaskBoard | null): number {
  return board ? board.tasks.filter((t) => t.problems.length > 0).length : 0;
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
