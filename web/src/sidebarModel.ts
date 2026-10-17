// 목록(sidebar)이 그리는 값: thread 묶음 순서·상태 줄, pane 상태를 herdr 상태 말로, 머신·프로젝트 묶음.
// 참고: herdr-projects `thread.rs` Group::rank·`sidebar.rs` word·state_line·project_line(1f05c00),
// herdr-web-ui `lib/status.ts`(aa624cb, herdr/status.ts에 그대로 있다)·`components/Sidebar.tsx`·`AgentSidebar.tsx`.
import type { MachineView, PaneView, ThreadGroup, ThreadView } from "./gen/proto";
import type { AgentStatus } from "./herdr/status.ts";

/** herdr-projects `Group::rank`: 사람 차례 1 → 검토 2 → 병합 중 3 → 작업 4 → 쉼 5 → 끝남 6. 모르는 묶음은 작업과 쉼 사이 */
const RANK: Record<string, number> = { waiting_on_you: 1, ready_for_review: 2, landing: 3, working: 4, idle: 5, resolved: 6 };
export const groupRank = (g: ThreadGroup | string): number => RANK[g] ?? 4.5;

/** herdr-projects `sidebar::word`(목록·탭 줄의 짧은 말) */
export const GROUP_WORD: Record<string, string> = {
  waiting_on_you: "사람 차례",
  ready_for_review: "검토 대기",
  landing: "병합 중",
  working: "작업 중",
  idle: "쉼",
  resolved: "끝남",
};

/** herdr-projects `sidebar::needs_you`: 프로젝트 줄과 탭 줄이 세는 묶음 */
export const needsYou = (g: ThreadGroup | string) => g === "waiting_on_you" || g === "ready_for_review";

/** 5분 넘게 진행 보고가 없으면 작업 중 줄이 「n분 조용」(herdr-projects `QUIET_SECS`) */
export const QUIET_MS = 300_000;

/**
 * thread 줄 상태 말: 묶음 말과 사실 하나(herdr-projects `state_line`). 진행률은 100 미만만 「~n%」.
 * aaa에는 PR이 아직 없어 검토·병합의 사실은 「보고」다(D11로 PR이 생기면 「PR #n」).
 */
export function stateLine(t: ThreadView, paneOpen: boolean, now: number): string {
  const a = t.agent ?? null;
  const pct = a?.percent != null && a.percent < 100 ? `~${a.percent}%` : null;
  let fact: string | null = null;
  switch (t.group) {
    case "waiting_on_you":
      if (!paneOpen) fact = "pane 닫힘";
      else if (t.status === "failed") fact = "실패";
      else if (a?.activity === "question") fact = "확인창";
      else fact = pct;
      break;
    case "ready_for_review":
      fact = "보고";
      break;
    case "working": {
      const reportedAt = a?.progress_at_ms ?? null;
      const fresh = reportedAt != null && now - reportedAt < QUIET_MS;
      if (pct && fresh) fact = pct;
      else {
        const since = now - (reportedAt ?? t.updated_at_ms);
        if (since >= QUIET_MS) fact = `${Math.floor(since / 60_000)}분 조용`;
      }
      break;
    }
    default:
      if ((t.group as string) === "landing") fact = "보고";
  }
  const word = GROUP_WORD[t.group] ?? t.group;
  return fact ? `${word} · ${fact}` : word;
}

/** herdr-projects `project_line`(「2 need you · 3 working」): 「사람 차례 2 · 작업 중 3」, 없으면 「쉼」 */
export function projectLine(groups: readonly string[]): string {
  const need = groups.filter(needsYou).length;
  const parts: string[] = [];
  if (need > 0) parts.push(`사람 차례 ${need}`);
  for (const [g, label] of [
    ["working", "작업 중"],
    ["landing", "병합 중"],
  ] as const) {
    const n = groups.filter((x) => x === g).length;
    if (n > 0) parts.push(`${label} ${n}`);
  }
  return parts.length ? parts.join(" · ") : "쉼";
}

/** 목록 순서: 묶음 rank, 같은 rank 안은 최근 바뀐 것 먼저(차이 목록 L2 계획) */
export function sortThreads(threads: readonly ThreadView[]): ThreadView[] {
  return [...threads].sort((a, b) => groupRank(a.group) - groupRank(b.group) || b.updated_at_ms - a.updated_at_ms);
}

/**
 * pane이 herdr 상태 말로 무엇인가(참고 status.ts `knownStatus`의 입력). 셸은 상태가 없다(참고: 셸은 working·blocked를 알려도 agent가 아니다).
 * 확인창 → blocked, 턴 진행 → working, 쉼·시작 → idle. 끝나고 아직 안 본 쉼(`seen` false, host seen.rs)은 done.
 * harness 오류·끝남은 herdr에 없는 상태라 상태 칸에 쓰지 않는다(오류는 `paneProblem`이 경고로).
 */
export function paneAgentStatus(p: PaneView): AgentStatus | undefined {
  const a = p.agent;
  if (!a || p.state !== "running") return undefined;
  switch (a.activity) {
    case "working":
      return "working";
    case "question":
      return "blocked";
    case "idle":
      return a.seen ? "idle" : "done";
    case "started":
      return "idle";
    default:
      return undefined;
  }
}

/**
 * 상태 칸 대신 경고 하나로 보일 일(참고 Sidebar.tsx `RestoreErrorBadge`: 복원 실패는 경고 아이콘, 이유는 hover).
 * aaa는 agent backend가 PTY를 가져 중단·유실이 더 있고(결정 3), harness 오류도 있다.
 */
export function paneProblem(p: PaneView): string | null {
  if (p.state === "interrupted") return p.restore_error ? `중단: ${p.restore_error}` : "중단: agent backend가 다시 떴을 때 이 세션이 없었다";
  if (p.state === "lost") return "유실: 세션 기록을 찾지 못했다";
  if (p.state === "running" && p.agent?.activity === "error") return `harness 오류${p.agent.message ? `: ${p.agent.message}` : ""}`;
  return null;
}

/** 끝난 pane의 한 마디(둘째 줄 끝): 「종료(0)」 */
export function paneEnded(p: PaneView): string | null {
  if (p.state !== "exited") return null;
  return p.code == null ? "종료" : `종료(${p.code})`;
}

/** 홈 폴더를 ~로 줄인다(화면에 사용자 이름을 늘어놓지 않는다) */
export const shortPath = (p: string) => p.replace(/^\/(Users|home)\/[^/]+/, "~");

/** 그 머신(이름이나 id)의 thread인가. pane이 있으면 pane의 머신을 믿는다 */
export function threadMachine(t: ThreadView, m: MachineView, paneOf: (id: string) => PaneView | undefined): boolean {
  const p = t.pane ? paneOf(t.pane) : undefined;
  return p ? p.machine_id === m.id : t.machine === m.name || t.machine === m.id;
}

export const threadKey = (t: Pick<ThreadView, "project" | "id">) => `${t.project}/${t.id}`;

/** 머신 묶음의 줄 차례: 프로젝트(처음 나온 차례) 묶음의 thread, 그다음 thread에 딸리지 않은 pane. 목록(Sidebar)과 단축키가 같이 쓴다 */
export function machineRoster(m: MachineView, panes: readonly PaneView[], threads: readonly ThreadView[]) {
  const paneOf = (id: string) => panes.find((p) => p.id === id);
  const mThreads = sortThreads(threads.filter((t) => threadMachine(t, m, paneOf)));
  const threadPanes = new Set(mThreads.flatMap((t) => (t.pane ? [t.pane] : [])));
  const mPanes = panes.filter((p) => p.machine_id === m.id && p.role !== "evaluator" && !threadPanes.has(p.id));
  // 프로젝트 묶음은 처음 나온 순서(가장 앞 rank의 thread가 있는 프로젝트가 먼저)
  const projects: string[] = [];
  for (const t of mThreads) if (!projects.includes(t.project)) projects.push(t.project);
  return { threads: mThreads, projects, panes: mPanes };
}

/** 목록 줄 하나: 누르면 여는 것과, 그 줄이 골랐다고 보는 선택 값들(thread 줄은 그 thread와 그 pane) */
export interface RosterRow {
  open: { pane: string } | { thread: ThreadView };
  keys: string[];
}

/**
 * 「이전·다음 패널」 단축키가 밟는 줄(herdr-web-ui App.tsx `selectAdjacentPane`·`rosterPanes`): 목록에 보이는 차례 그대로,
 * 접힌 묶음 안의 줄도 센다. thread 줄은 그 agent pane이 돌면 pane을, 아니면 보고서를 연다(ThreadRow와 같다).
 */
export function machineRows(m: MachineView, panes: readonly PaneView[], threads: readonly ThreadView[]): RosterRow[] {
  const roster = machineRoster(m, panes, threads);
  const rows: RosterRow[] = [];
  for (const project of roster.projects)
    for (const t of roster.threads.filter((x) => x.project === project)) {
      const pane = t.pane ? panes.find((p) => p.id === t.pane) : undefined;
      rows.push({ open: pane?.state === "running" ? { pane: pane.id } : { thread: t }, keys: [`thread:${threadKey(t)}`, ...(pane ? [`pane:${pane.id}`] : [])] });
    }
  for (const p of roster.panes) rows.push({ open: { pane: p.id }, keys: [`pane:${p.id}`] });
  return rows;
}
