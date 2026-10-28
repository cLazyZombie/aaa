// 표시 정렬만 바꾼다. host의 파일·예약 차례는 정본으로 보존한다.
import type { TaskBoard, TaskView } from "./gen/proto";
import { taskState, type TaskListModel, type ThreadGroups } from "./tasksModel.ts";
import { logIgnoredFailure } from "./errorDiagnostic.ts";

export type ActiveTaskSort = "manual" | "number" | "title" | "state" | "profile" | "machine";
export const ACTIVE_TASK_SORTS: readonly { key: ActiveTaskSort; label: string; hint: string }[] = [
  { key: "manual", label: "손 차례", hint: "예약 차례 뒤 원문 차례" },
  { key: "number", label: "번호", hint: "큰 번호가 위 · 번호 없음은 끝" },
  { key: "title", label: "제목", hint: "가나다순" },
  { key: "state", label: "상태", hint: "확인 필요 → 진행 중 → 대기 → 보류 → 완료" },
  { key: "profile", label: "모델", hint: "모델 이름순 · 미지정은 끝" },
  { key: "machine", label: "머신", hint: "머신 이름순 · 미지정은 끝" },
];
export function readTaskSort(scope: "active"): ActiveTaskSort {
  try {
    const v = localStorage.getItem(`aaa:tasks:${scope}-sort`);
    return ACTIVE_TASK_SORTS.some(s => s.key === v) ? v as ActiveTaskSort : "manual";
  } catch (error) {
    logIgnoredFailure(error, "taskSortModel.readTaskSort");
    return "manual";
  }
}
export function writeTaskSort(scope: "active", sort: ActiveTaskSort): void {
  try { localStorage.setItem(`aaa:tasks:${scope}-sort`, sort); }
  catch (error) { logIgnoredFailure(error, "taskSortModel.writeTaskSort"); }
}

const collator = new Intl.Collator("ko", { numeric: true, sensitivity: "base", ignorePunctuation: true });
const textCompare = (a: string | null | undefined, b: string | null | undefined) =>
  !a ? (!b ? 0 : 1) : !b ? -1 : collator.compare(a, b);
const numberOf = (t: TaskView) => /^d-\d+$/.test(t.short_id ?? "") ? BigInt(t.short_id!.slice(2)) : null;
const STATE_RANK = { problem: 0, check: 0, run: 1, wait: 2, parked: 3, done: 4 };

/** 예약 묶음은 그대로, 일반 작업은 상하위를 보존하여 형제끼리 정렬한다. 거르기로 상위가 숨겨져도 같은 묶음이다. */
export function sortActiveTasks(model: TaskListModel, board: TaskBoard, sort: ActiveTaskSort, groups: ThreadGroups): TaskListModel {
  if (sort === "manual") return model;
  const byLine = new Map(board.tasks.map(t => [t.line, t]));
  const compare = (a: TaskView, b: TaskView): number => {
    let value = 0;
    if (sort === "number") {
      const an = numberOf(a), bn = numberOf(b);
      value = an === null ? (bn === null ? 0 : 1) : bn === null ? -1 : an > bn ? -1 : an < bn ? 1 : 0;
    } else if (sort === "state") value = STATE_RANK[taskState(a, groups)] - STATE_RANK[taskState(b, groups)];
    else value = textCompare(a[sort], b[sort]);
    return value || a.line - b.line;
  };
  const path = (t: TaskView): TaskView[] => {
    const out = [t], seen = new Set([t.line]);
    let up = t.parent === null ? undefined : byLine.get(t.parent);
    while (up && !seen.has(up.line)) {
      seen.add(up.line); out.unshift(up);
      up = up.parent === null ? undefined : byLine.get(up.parent);
    }
    return out;
  };
  const paths = new Map(board.tasks.map(t => [t.line, path(t)]));
  const rest = [...model.rest].sort((a, b) => {
    if (a.kind === "gap" || b.kind === "gap") return 0;
    const ap = paths.get(a.t.line)!, bp = paths.get(b.t.line)!;
    for (let i = 0; i < Math.min(ap.length, bp.length); i++) {
      if (ap[i].line !== bp[i].line) return compare(ap[i], bp[i]);
    }
    return ap.length - bp.length;
  });
  return { ...model, rest };
}
