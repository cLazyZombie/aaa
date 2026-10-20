// 목록(sidebar)이 그리는 값: thread 묶음 순서·상태 줄, pane 상태를 herdr 상태 말로, 머신·프로젝트 묶음.
// 참고: herdr-projects `thread.rs` Group::rank·`sidebar.rs` word·state_line·project_line(1f05c00),
// herdr-web-ui `lib/status.ts`(aa624cb, herdr/status.ts에 그대로 있다)·`components/Sidebar.tsx`·`AgentSidebar.tsx`.
import type { MachineView, PaneView, ThreadGroup, ThreadView } from "./gen/proto";
import { sidebarPinKey } from "./herdr/sidebarPins.ts";
import { threadRepo, workFolders } from "./workFolders.ts";
import type { AgentStatus } from "./herdr/status.ts";

/** herdr-projects `Group::rank`: 확인 필요 1 → 검토 2 → 병합 중 3 → 작업 4 → 쉼 5 → 끝남 6. 모르는 묶음은 작업과 쉼 사이 */
const RANK: Record<string, number> = { waiting_on_you: 1, ready_for_review: 2, landing: 3, working: 4, idle: 5, resolved: 6 };
export const groupRank = (g: ThreadGroup | string): number => RANK[g] ?? 4.5;

/** herdr-projects `sidebar::word`(목록·탭 줄의 짧은 말) */
export const GROUP_WORD: Record<string, string> = {
  waiting_on_you: "확인 필요",
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

/** PR 주소 끝의 번호(herdr-projects `pr_number`) */
export function prNumber(url: string | null | undefined): string | null {
  const n = url?.split("/").pop() ?? "";
  return /^[0-9]+$/.test(n) ? n : null;
}

/**
 * thread 줄 상태 말: 묶음 말과 사실 하나(herdr-projects `state_line`). 진행률은 100 미만만 「~n%」.
 * 검토·병합 중의 사실은 따라가는 PR이 있으면 「PR #n」, 없으면 「보고」다.
 */
export function stateLine(t: ThreadView, paneOpen: boolean, now: number): string {
  if (t.status === "starting") return "준비 중";
  if (t.cleanup_pending) return t.group === "waiting_on_you" ? "정리 확인 필요" : "정리 중";
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
    case "landing": {
      const n = prNumber(t.pr?.url);
      fact = n ? `PR #${n}` : "보고";
      break;
    }
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
  }
  const word = GROUP_WORD[t.group] ?? t.group;
  return fact ? `${word} · ${fact}` : word;
}

/**
 * herdr-projects `project_line`(「2 need you · 3 working」): 「확인 필요 1 · 검토 대기 1 · 작업 중 3」, 없으면 「쉼」.
 * 참고의 「need you」 한 수(needsYou: 확인 필요·검토)는 thread 줄 글과 같은 말로 나눠 보인다(QA t-0362 O1, plan §8.203). 0인 쪽은 뺀다
 */
export function projectLine(groups: readonly string[], paused = false): string {
  // 멈춘 프로젝트는 묶음 대신 「멈춤」(herdr-projects sidebar.rs `project_line` 「paused」)
  if (paused) return "멈춤";
  const parts: string[] = [];
  for (const [g, label] of [
    ["waiting_on_you", GROUP_WORD.waiting_on_you],
    ["ready_for_review", GROUP_WORD.ready_for_review],
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
    // 쉬지만 그 턴이 띄운 배경 작업을 기다리면 끝남·쉼 대신 BG(참고 status.ts `paneStatus`, 판정은 host bgwait.rs, plan §8.189)
    case "idle":
      return a.background_wait ? "waiting" : a.seen ? "idle" : "done";
    case "started":
      return a.background_wait ? "waiting" : "idle";
    default:
      return undefined;
  }
}

/**
 * 「활동순」(herdr-web-ui `src/lib/sidebarOrder.ts` `activityOrder`): blocked가 먼저, 그다음 가장 최근 상태 변화, 같으면
 * 받은 차례. 상태로 따로 순위를 매기지 않는다(참고: 메시지를 보낸 줄이 도는 동안 DONE 아래로 떨어졌다 끝나면 튀어 오르던 것을
 * 막는다). 최근은 참고의 herdr `state_change_seq` 자리에 host가 상태(activity)가 바뀔 때 적는 `AgentStatus.since_ms`를 쓴다.
 * pane이 없는 줄(pane이 닫힌 thread)은 상태 변화가 없는 줄로 본다.
 */
export function activityOrder<T>(rows: readonly T[], paneOf: (row: T) => PaneView | undefined): T[] {
  return rows
    .map((row, index) => {
      const pane = paneOf(row);
      return { row, index, rank: pane && paneAgentStatus(pane) === "blocked" ? 0 : 1, recent: pane?.agent?.since_ms ?? -1 };
    })
    .sort((a, b) => a.rank - b.rank || b.recent - a.recent || a.index - b.index)
    .map((entry) => entry.row);
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

/** thread가 도는 머신. pane이 있으면 pane의 머신을 믿고, 없으면 thread의 머신 이름이나 id로 찾는다 */
export function machineOfThread(t: ThreadView, machines: readonly MachineView[], paneOf: (id: string) => PaneView | undefined): MachineView | undefined {
  const p = t.pane ? paneOf(t.pane) : undefined;
  return p ? machines.find((m) => m.id === p.machine_id) : machines.find((m) => t.machine === m.name || t.machine === m.id);
}

export const threadKey = (t: Pick<ThreadView, "project" | "id">) => `${t.project}/${t.id}`;

/** 「작업」 트리의 작업 공간 폴더 하나(plan §8.280): 프로젝트 안에서 thread가 일하는 저장소 이름. 저장소를 아는 thread가 하나도 없으면 이름 없이 하나 */
export interface WorkFolder {
  project: string;
  /** 저장소 이름(마지막 폴더), 저장소 없는 thread는 「작업 폴더」. null이면 폴더 머리 없이 그린다 */
  name: string | null;
  threads: ThreadView[];
}

/**
 * 목록 전체의 줄 차례(「작업」 트리, plan §8.280): 프로젝트를 먼저 묶고(프로젝트가 둘 이상일 때만 머리가 보인다) 그 안을 coordinator →
 * 작업 공간 폴더(저장소 이름순, 「작업 폴더」 끝, plan §8.260) → 폴더 안 thread(묶음 rank·최근 바뀐 순, 또는 활동순)로 늘어놓는다.
 * 머신은 줄에 글자로만 적는다(plan §8.195). 프로젝트 차례는 가장 앞 rank의 thread가 있는 프로젝트가 먼저다. 그다음 thread에 딸리지 않은
 * pane의 「기타」 묶음: 사람이 옮긴 차례(공유 설정 `paneOrder`, plan §8.196)를 먼저, 옮긴 적 없는 pane은 머신 차례·host 차례로 그 뒤에.
 * 종료된 옛 평가자·임무 pane은 「기타」에서 뺀다. 기록은 host에 남겨 직접 열 수 있고, 살아 있거나 복원할 pane과 thread 줄은 유지한다.
 * 목록(Sidebar)·「이전·다음 패널」 단축키·명령 팔레트가 같이 쓴다.
 */
export function roster(machines: readonly MachineView[], panes: readonly PaneView[], threads: readonly ThreadView[], paneOrder: readonly string[] = [], byActivity = false) {
  const paneOf = (id: string) => panes.find((p) => p.id === id);
  const machineOf = (t: ThreadView) => machineOfThread(t, machines, paneOf);
  const known = sortThreads(threads.filter((t) => machineOf(t) !== undefined));
  const threadPanes = new Set(known.flatMap((t) => (t.pane ? [t.pane] : [])));
  const coordinators = panes.filter((p) => p.coordinator_project && !threadPanes.has(p.id) && machines.some((m) => m.id === p.machine_id));
  const coordinatorPanes = new Set(coordinators.map((p) => p.id));
  const rank = new Map(paneOrder.map((key, index) => [key, index]));
  const last = Number.MAX_SAFE_INTEGER;
  const loose = machines
    .flatMap((m) => panes.filter((p) => p.machine_id === m.id && !threadPanes.has(p.id) && !coordinatorPanes.has(p.id)))
    .filter((p) => !(p.role === "evaluator" && p.state === "exited"))
    .map((p, index) => ({ p, index, rank: rank.get(sidebarPinKey(p.machine_id, p.id)) ?? last }))
    .sort((a, b) => a.rank - b.rank || a.index - b.index)
    .map((entry) => entry.p);
  // 프로젝트 묶음은 처음 나온 순서(가장 앞 rank의 thread가 있는 프로젝트가 먼저)
  const projects: string[] = [];
  for (const t of known) if (!projects.includes(t.project)) projects.push(t.project);
  for (const p of coordinators) if (!projects.includes(p.coordinator_project!)) projects.push(p.coordinator_project!);
  const folders: WorkFolder[] = [];
  for (const project of projects) {
    const mine = known.filter((t) => t.project === project);
    // 폴더 안 「활동순」(설정 「폴더 안 정렬」): 그 thread의 agent pane으로
    const ordered = byActivity ? activityOrder(mine, (t) => (t.pane ? paneOf(t.pane) : undefined)) : mine;
    const grouped = workFolders(ordered, threadRepo);
    if (grouped.folders.length > 0) for (const f of grouped.folders) folders.push({ project, name: f.name, threads: f.rows });
    else if (grouped.flat.length > 0) folders.push({ project, name: null, threads: grouped.flat });
  }
  // thread는 보이는 차례(프로젝트 → 폴더 → 폴더 안)로
  return { threads: folders.flatMap((f) => f.threads), folders, projects, coordinators, panes: loose, machineOf };
}

/** 목록 줄 하나: 누르면 여는 것과, 그 줄이 골랐다고 보는 선택 값들(thread 줄은 그 thread와 그 pane) */
export interface RosterRow {
  open: { pane: string } | { thread: ThreadView };
  keys: string[];
}

/**
 * 「이전·다음 패널」 단축키가 밟는 줄(herdr-web-ui App.tsx `selectAdjacentPane`·`rosterPanes`): 목록에 보이는 차례 그대로,
 * 접힌 묶음 안의 줄도 센다. 끊긴 머신의 줄은 열 수 없어 뺀다. thread 줄은 그 agent pane이 돌면 pane을, 아니면 보고서를 연다
 * (ThreadRow와 같다).
 */
export function rosterRows(machines: readonly MachineView[], panes: readonly PaneView[], threads: readonly ThreadView[], paneOrder: readonly string[] = [], byActivity = false): RosterRow[] {
  const r = roster(machines, panes, threads, paneOrder, byActivity);
  const online = (id: string | undefined) => machines.some((m) => m.id === id && m.connection === "connected");
  const rows: RosterRow[] = [];
  for (const project of r.projects) {
    for (const p of r.coordinators.filter((p) => p.coordinator_project === project))
      if (online(p.machine_id)) rows.push({ open: { pane: p.id }, keys: [`pane:${p.id}`] });
    for (const t of r.threads.filter((x) => x.project === project)) {
      if (!online(r.machineOf(t)?.id)) continue;
      const pane = t.pane ? panes.find((p) => p.id === t.pane) : undefined;
      rows.push({ open: pane?.state === "running" ? { pane: pane.id } : { thread: t }, keys: [`thread:${threadKey(t)}`, ...(pane ? [`pane:${pane.id}`] : [])] });
    }
  }
  for (const p of r.panes) if (online(p.machine_id)) rows.push({ open: { pane: p.id }, keys: [`pane:${p.id}`] });
  return rows;
}

/**
 * thread 줄 상태 말의 색(data-tone, plan §8.266). t3code(MIT) `apps/web/src/components/Sidebar.logic.ts:1098`
 * `resolveThreadStatusPill`(1451e7cb2)의 차례를 aaa 상태로 옮겼다: 승인·입력 대기 → 작업 중 → 배경 대기 → 실패(안 봄) → 끝남(안 봄).
 * 다른 곳(aaa에만 있는 상태): 참고의 「Plan Ready」(사람이 할 일) 자리에 확인 필요·검토 대기·병합 중을 둔다. 승인과 입력 대기는 화면
 * 감지(PTY)로는 같은 확인창이라 하나(question)다. 「연결 중」은 aaa 상태에 없다. 묶음 말(herdr-projects)은 그대로 두고 색만 정한다.
 * 사용량 한도(plan §8.267, 참고 `Sidebar.logic.ts:939` 「limited」는 실패 자리)는 실패 다음이다: host가 말한 풀리는 때(`limited_until_ms`)가
 * `now` 뒤일 때만이라 다시 차오르면 host가 다시 알리기 전에도 걷힌다.
 */
export type StateTone = "question" | "working" | "background" | "failed" | "limited" | "attention" | "review" | "unseen" | "idle";
export function stateTone(t: ThreadView, pane: PaneView | undefined, now = Date.now()): StateTone {
  const status = pane ? paneAgentStatus(pane) : undefined;
  if (status === "blocked") return "question";
  if (status === "working") return "working";
  if (status === "waiting") return "background";
  if (t.status === "failed") return "failed";
  if (t.limited_until_ms != null && t.limited_until_ms > now) return "limited";
  if (t.group === "waiting_on_you") return "attention";
  if (t.group === "ready_for_review" || t.group === "landing") return "review";
  if (status === "done") return "unseen";
  return "idle";
}

/**
 * 작업 시간의 시작(참고 Sidebar.tsx:2360~2367): 작업 중이면 그 턴이 시작된 때, 배경 대기면 턴이 끝난 때. 참고는 run 시작·끝 시각을
 * 쓰고, aaa는 화면 감지가 상태를 바꾼 시각(`AgentStatus.since_ms`)이다
 */
export function workingStartedAt(tone: StateTone, pane: PaneView | undefined): number | null {
  return (tone === "working" || tone === "background") && pane?.agent ? pane.agent.since_ms : null;
}

/** 참고 `formatWorkingDurationLabel`(Sidebar.logic.ts:1090): 60초 밑은 초, 60분 밑은 분, 그 위는 시간과 분 */
export function formatWorkingDuration(elapsedMs: number): string {
  const seconds = Number.isFinite(elapsedMs) ? Math.max(0, Math.floor(elapsedMs / 1000)) : 0;
  if (seconds < 60) return `${seconds}초`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}분`;
  return `${Math.floor(minutes / 60)}시간 ${minutes % 60}분`;
}
