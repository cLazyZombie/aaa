// 왼쪽 목록(herdr-web-ui MachineSidebar·Sidebar·AgentSidebar, aa624cb). 모양(class)과 CSS는 참고 것을 그대로 쓴다(herdr/*.css).
// 구성(차이 목록 L1): 머신 묶음마다 머리(이름·host·연결 점·접기·+) → 프로젝트 머리(접기, 묶음 대표 상태) 아래 그 머신의 thread 줄 →
// thread가 아닌 pane 줄. 그 아래 모든 머신의 「Agents」 구역, 맨 아래 바닥 줄(설정·사용량, App.tsx ListFoot).
// aaa에는 herdr workspace가 없다: 참고의 workspace 줄 자리에 pane 줄을, worktree 묶음 자리에 프로젝트 묶음을 둔다.
// thread는 자기 줄 하나로만 보인다(L4, herdr-projects sidebar.rs: thread는 자기 agent 줄이고 이름이 「id · 제목」).
import { useEffect, useId, useState, type AnimationEvent, type KeyboardEvent, type ReactNode } from "react";
import { ChevronDown, ChevronRight, Folder, FolderOpen, Monitor, Plus, Terminal, TriangleAlert } from "lucide-react";
import type { MachineView, PaneView, ThreadView } from "./gen/proto";
import { AgentMark } from "./herdr/AgentMark.tsx";
import { indicatorGlyph, knownStatus, rollupStatus, STATUS_WORD, type AgentStatus, type KnownStatus } from "./herdr/status.ts";
import { useAppearance } from "./theme";
import { GROUP_WORD, machineRoster, paneAgentStatus, paneEnded, paneProblem, projectLine, shortPath, stateLine, threadKey } from "./sidebarModel";
import "./herdr/primitives.css";
import "./herdr/Sidebar.css";
import "./herdr/AgentSidebar.css";
import "./herdr/Machines.css";
import "./Sidebar.css";

/** 상태 말(참고 status.ts STATUS_WORD는 영어 대문자 낱말이다). 화면 낭독·tooltip에 쓴다 */
const STATUS_KO: Record<KnownStatus, string> = {
  idle: "입력 대기",
  working: "작업 중",
  blocked: "답을 기다림",
  done: "끝남(안 봄)",
  waiting: "배경 작업 대기",
  unknown: "—",
};
void STATUS_WORD;

/** 숨쉬는 점은 모두 문서 시계의 같은 박자(참고 Sidebar.tsx `alignBreath`) */
function alignBreath(event: AnimationEvent<HTMLSpanElement>) {
  if (event.animationName !== "sidebar-breathe") return;
  for (const animation of event.currentTarget.getAnimations()) animation.startTime = 0;
}

/**
 * 상태 칸(참고 Sidebar.tsx `StatusBadge` compact). 기본(classic)은 볼 일이 있는 상태만 그린다: 답을 기다리면 빨간 ?, 끝나고 안 봤으면
 * 초록 점, 작업 중이면 숨쉬는 점, 배경 작업을 기다리면 멈춘 고리. 입력 대기·모름은 칸만 두고 말은 aria-label·title로.
 * 설정 「상태 표시」가 점·기호면 herdr TUI 글자를 herdr 색으로.
 */
export function StatusBadge({ status, compact = false, testid }: { status?: AgentStatus; compact?: boolean; testid?: string }) {
  const { appearance } = useAppearance();
  const value = knownStatus(status);
  const label = STATUS_KO[value];
  const description = value === "waiting" ? "agent가 배경 작업을 기다린다" : `agent ${label}`;
  const indicators = appearance.statusIndicators;
  const dot = value === "working" || value === "done" || value === "waiting";
  return (
    <span
      className={`badge badge-${value}${compact ? " sidebar-status" : ""}`}
      data-status={value}
      data-testid={testid}
      role={compact ? "status" : undefined}
      aria-label={compact ? description : undefined}
      title={value === "unknown" ? undefined : description}
    >
      {compact && indicators !== "classic" && (
        <span className="sidebar-status-glyph" aria-hidden="true" onAnimationStart={value === "working" ? alignBreath : undefined}>
          {indicatorGlyph(value, indicators)}
        </span>
      )}
      {compact && indicators === "classic" && value === "blocked" && (
        <span className="sidebar-status-mark" aria-hidden="true">
          ?
        </span>
      )}
      {compact && indicators === "classic" && dot && <span className="sidebar-status-dot" aria-hidden="true" onAnimationStart={value === "working" ? alignBreath : undefined} />}
      <span className={compact ? "visually-hidden" : undefined}>{label}</span>
    </span>
  );
}

/** 상태 칸 대신 경고 하나, 이유는 hover(참고 Sidebar.tsx `RestoreErrorBadge`) */
function ProblemBadge({ reason, testid }: { reason: string; testid?: string }) {
  return (
    <span className="badge badge-restore-error sidebar-status" data-status="restore-error" data-testid={testid} role="status" aria-label={reason} title={reason}>
      <TriangleAlert aria-hidden="true" />
      <span className="visually-hidden">{reason}</span>
    </span>
  );
}

/** 줄 앞 표시: 그 줄이 여는 pane의 agent, 셸이면 터미널(참고 Sidebar.tsx `rowMark`) */
function RowMark({ agent }: { agent: string | null }) {
  return (
    <span className="sidebar-mark" aria-hidden="true" data-agent={agent ?? "shell"}>
      {agent !== null ? <AgentMark agent={agent} size={18} /> : <Terminal />}
    </span>
  );
}

/** 기기별로 접힌 묶음을 기억한다(참고 MachineSidebar `herdr-web-ui:pc-collapsed:<id>`, Sidebar `worktree-group-collapsed`) */
function useFold(key: string, initial = false): [boolean, () => void] {
  const [folded, setFolded] = useState(() => {
    try {
      const v = localStorage.getItem(key);
      return v === null ? initial : v === "1";
    } catch {
      return initial;
    }
  });
  const toggle = () => {
    setFolded(!folded);
    try {
      localStorage.setItem(key, folded ? "0" : "1");
    } catch {
      // 저장이 막히면 이 화면 동안만
    }
  };
  return [folded, toggle];
}

const onRowKey = (event: KeyboardEvent, open: () => void) => {
  if (event.key !== "Enter" && event.key !== " ") return;
  event.preventDefault();
  open();
};

export interface SidebarProps {
  machines: MachineView[];
  panes: PaneView[];
  /** 끝난(resolved)·평가자 thread는 뺀 것 */
  threads: ThreadView[];
  /** `pane:<id>` 또는 `thread:<project>/<id>` */
  selected: string | null;
  canControl: boolean;
  agentOf: (p: PaneView | undefined, t?: ThreadView) => string | null;
  onPane: (id: string) => void;
  onThread: (t: ThreadView) => void;
  onNewShell: (m: MachineView) => void;
  /** 목록 위에 띄울 알림 띠(연결 끊김·오류) */
  banners?: ReactNode;
}

export function Sidebar(props: SidebarProps) {
  const { machines } = props;
  return (
    <>
      <div className="list-body machine-list" aria-label="머신과 pane">
        {props.banners}
        {machines.map((m) => (
          <MachineGroup key={m.id} {...props} machine={m} />
        ))}
      </div>
      <AgentSection {...props} />
    </>
  );
}

function MachineGroup({ machine: m, ...props }: SidebarProps & { machine: MachineView }) {
  const [collapsed, toggle] = useFold(`aaa:pc-collapsed:${m.id}`);
  const online = m.connection === "connected";
  // 줄 차례는 단축키 「이전·다음 패널」과 같은 계산(sidebarModel machineRoster)
  const { threads, projects, panes } = machineRoster(m, props.panes, props.threads);
  const problems = online && m.problems.length > 0 ? m.problems.map((p) => p.text).join("\n") : null;
  return (
    <section className={`machine-group${online ? "" : " is-offline"}`} aria-label={`머신 ${m.name}`} data-testid={`machine-${m.name}`}>
      <header className="machine-header">
        <div className="machine-title">
          <span className="sidebar-mark" aria-hidden="true">
            <Monitor />
          </span>
          <span className="machine-name">{m.name}</span>
          {m.local && (
            <span className="machine-kind" title="host가 도는 머신">
              host
            </span>
          )}
          <span className={`machine-dot is-${online ? (problems ? "error" : "connected") : "disconnected"}`} title={online ? (problems ? "점검 이상" : "연결됨") : "끊김"} aria-hidden="true" />
        </div>
        <button
          type="button"
          className="sidebar-row-action machine-toggle"
          aria-expanded={!collapsed}
          aria-label={collapsed ? `${m.name} 펼치기` : `${m.name} 접기`}
          title={collapsed ? `${m.name} 펼치기` : `${m.name} 접기`}
          onClick={toggle}
        >
          {collapsed ? <ChevronRight aria-hidden="true" /> : <ChevronDown aria-hidden="true" />}
        </button>
        {props.canControl && (
          <button
            type="button"
            className="sidebar-row-action machine-new"
            disabled={!online}
            aria-label={`${m.name}에 새 셸`}
            title="새 셸"
            data-testid={`new-shell-${m.name}`}
            onClick={() => props.onNewShell(m)}
          >
            <Plus aria-hidden="true" />
          </button>
        )}
      </header>
      {/* 연결됨은 평소라 말하지 않는다. 그 밖의 상태와 점검 이상은 이름 아래 글로(참고 machine-state) */}
      {!online ? (
        <p className="machine-state is-disconnected" role="status">
          <span className="machine-state-word">끊김</span>
          <span className="machine-state-detail">pane은 agent에서 계속 돈다. 다시 붙으면 이어 본다.</span>
        </p>
      ) : problems ? (
        <p className="machine-state is-error" role="status" title={problems} data-testid={`health-${m.name}`}>
          <span className="machine-state-word">점검 이상: 새 세션을 시작하지 않는다</span>
          <span className="machine-state-detail">{problems}</span>
        </p>
      ) : (
        <p className="machine-state is-connected visually-hidden" role="status">
          <span className="machine-state-word">연결됨</span>
        </p>
      )}
      {!collapsed && (
        <div className={online ? "" : "machine-offline"} inert={!online}>
          <PaneRoster {...props} machine={m} online={online} mThreads={threads} projects={projects} mPanes={panes} />
        </div>
      )}
    </section>
  );
}

function PaneRoster({
  machine: m,
  online,
  mThreads: threads,
  projects,
  mPanes: panes,
  ...props
}: SidebarProps & { machine: MachineView; online: boolean; projects: string[]; mThreads: ThreadView[]; mPanes: PaneView[] }) {
  const { appearance } = useAppearance();
  const twoLine = appearance.sidebarRows === "two";
  if (threads.length === 0 && panes.length === 0) {
    return (
      <div className="machine-workspaces">
      <nav className="sidebar-list" aria-label={`${m.name} pane`}>
        <div className="tree-state-empty">
          <p className="tree-state" role="status">
            {online ? "pane 없음" : "저장된 pane 없음"}
          </p>
          {props.canControl && online && (
            <button type="button" className="btn" onClick={() => props.onNewShell(m)}>
              <Plus aria-hidden="true" />새 셸
            </button>
          )}
        </div>
      </nav>
      </div>
    );
  }
  return (
    <div className={`machine-workspaces${twoLine ? " is-two-line" : ""}`}>
      <nav className="sidebar-list" aria-label={`${m.name} pane`}>
        <ul className="workspace-list">
          {projects.map((project) => (
            <ProjectGroup key={project} {...props} machine={m} online={online} project={project} pThreads={threads.filter((t) => t.project === project)} twoLine={twoLine} />
          ))}
          {panes.map((p) => (
            <PaneRow key={p.id} {...props} online={online} pane={p} twoLine={twoLine} />
          ))}
        </ul>
      </nav>
    </div>
  );
}

/** 상태 칸: 경고 하나(중단·유실·harness 오류)가 상태보다 먼저(참고: restore_error면 RestoreErrorBadge) */
function PaneState({ pane, online, testid }: { pane: PaneView | undefined; online: boolean; testid?: string }) {
  const problem = pane && online ? paneProblem(pane) : null;
  return (
    <span className="sidebar-pane-meta">
      {problem ? <ProblemBadge reason={problem} testid={testid} /> : <StatusBadge compact status={online && pane ? paneAgentStatus(pane) : undefined} testid={testid} />}
    </span>
  );
}

function PaneRow({ pane: p, online, twoLine, ...props }: SidebarProps & { pane: PaneView; online: boolean; twoLine: boolean }) {
  const selected = props.selected === `pane:${p.id}`;
  const agent = props.agentOf(p);
  const title = p.name || p.argv.join(" ") || p.id.slice(0, 8);
  const ended = paneEnded(p);
  const where = [p.cwd ? shortPath(p.cwd) : null, ended].filter(Boolean).join(" · ");
  const tooltip = [p.id, title, p.cwd ? shortPath(p.cwd) : null, p.agent?.harness, ended].filter(Boolean).join("\n");
  const open = () => props.onPane(p.id);
  return (
    <li className={`workspace workspace-group pane-item${selected ? " is-selected" : ""}${p.state !== "running" ? " is-ended" : ""}`} data-pane={p.id}>
      <div className="workspace-header">
        <div
          className="pane-select workspace-select"
          role="button"
          tabIndex={0}
          aria-current={selected ? "true" : undefined}
          title={tooltip}
          data-testid={`pane-${p.name || p.id}`}
          onClick={open}
          onKeyDown={(e) => onRowKey(e, open)}
        >
          <RowMark agent={agent} />
          {twoLine ? (
            <span className="workspace-copy is-two-line">
              <span className="workspace-line">
                <span className="workspace-name">{title}</span>
              </span>
              {where && <span className="workspace-place">{where}</span>}
            </span>
          ) : (
            <span className="workspace-copy">
              <span className="workspace-name">{title}</span>
              {ended && <span className="worktree-workspace-label">{ended}</span>}
            </span>
          )}
          <span className="visually-hidden">{agent ?? "셸"}</span>
          <PaneState pane={p} online={online} testid={p.agent ? `agent-${p.name || p.id}` : undefined} />
        </div>
      </div>
    </li>
  );
}

function ProjectGroup({
  machine: m,
  online,
  project,
  pThreads: threads,
  twoLine,
  ...props
}: SidebarProps & { machine: MachineView; online: boolean; project: string; pThreads: ThreadView[]; twoLine: boolean }) {
  const [collapsed, toggle] = useFold(`aaa:project-collapsed:${m.id}:${project}`);
  const contentsId = `${useId()}-threads`;
  const paneOf = (id: string) => props.panes.find((p) => p.id === id);
  // 접힌 묶음도 고른 thread는 보인다(참고: 접힌 묶음은 열린 checkout을 보이고, 셈은 치운 것만)
  const isSel = (t: ThreadView) => props.selected === `thread:${threadKey(t)}` || (t.pane != null && props.selected === `pane:${t.pane}`);
  const shown = collapsed ? threads.filter(isSel) : threads;
  const foldedCount = threads.length - shown.length;
  // 접힌 머리는 자식 상태까지 대표한다(참고 rollupStatus: blocked > done > working > waiting > idle)
  const status = rollupStatus(threads.map((t) => (online && t.pane ? (paneOf(t.pane) ? paneAgentStatus(paneOf(t.pane)!) : undefined) : undefined)));
  const line = projectLine(threads.map((t) => t.group));
  const label = collapsed ? `${project} thread 펼치기` : `${project} thread 접기`;
  return (
    <>
      <li className={`workspace workspace-group pane-item project-group${collapsed ? " is-collapsed" : ""}`} data-project={project} data-testid={`project-${m.name}-${project}`}>
        <div className="workspace-header">
          <button type="button" className="sidebar-row-action workspace-toggle" aria-label={label} title={label} aria-expanded={!collapsed} aria-controls={contentsId} onClick={toggle}>
            {collapsed ? <Folder aria-hidden="true" /> : <FolderOpen aria-hidden="true" />}
          </button>
          <div className="pane-select workspace-select project-select" role="button" tabIndex={0} title={`${project} · ${line}`} onClick={toggle} onKeyDown={(e) => onRowKey(e, toggle)}>
            {twoLine ? (
              <span className="workspace-copy is-two-line">
                <span className="workspace-line">
                  <span className="workspace-name project-name">{project}</span>
                  {foldedCount > 0 && (
                    <span className="workspace-fold-count" aria-hidden="true">
                      +{foldedCount}
                    </span>
                  )}
                </span>
                <span className="workspace-place">{line}</span>
              </span>
            ) : (
              <span className="workspace-copy">
                <span className="workspace-name project-name">{project}</span>
                {foldedCount > 0 && (
                  <span className="workspace-fold-count" aria-hidden="true">
                    +{foldedCount}
                  </span>
                )}
              </span>
            )}
            <span className="sidebar-pane-meta">
              <StatusBadge compact status={collapsed ? status : undefined} testid={`project-state-${project}`} />
            </span>
          </div>
        </div>
      </li>
      {shown.length > 0 && (
        <li className="worktree-children" id={contentsId}>
          <ul className="workspace-list">
            {shown.map((t) => (
              <ThreadRow key={threadKey(t)} {...props} online={online} t={t} twoLine={twoLine} />
            ))}
          </ul>
        </li>
      )}
    </>
  );
}

/** 지금 시각, 1분마다 다시 그린다(「n분 조용」이 화면에서 늘어나게. herdr-projects는 ticker가 줄을 다시 쓴다) */
function useMinute(): number {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 60_000);
    return () => window.clearInterval(id);
  }, []);
  return now;
}

function ThreadRow({ t, online, twoLine, ...props }: SidebarProps & { t: ThreadView; online: boolean; twoLine: boolean }) {
  const now = useMinute();
  const pane = t.pane ? props.panes.find((p) => p.id === t.pane) : undefined;
  const paneOpen = pane?.state === "running";
  const selected = props.selected === `thread:${threadKey(t)}` || (pane != null && props.selected === `pane:${pane.id}`);
  const agent = props.agentOf(pane, t);
  // herdr-projects thread_display: 「id · 제목」
  const title = `${t.id} · ${t.title}`;
  const line = stateLine(t, paneOpen, now);
  const tooltip = [`${t.project}/${t.id}`, t.title, line, `${t.profile}@${t.machine}`, t.branch].filter(Boolean).join("\n");
  // 줄은 그 thread의 agent pane을 연다(herdr-projects: thread는 자기 agent 줄). pane이 없으면 보고서
  const open = () => (paneOpen && pane ? props.onPane(pane.id) : props.onThread(t));
  return (
    <li className={`workspace workspace-group pane-item${selected ? " is-selected" : ""}`} data-thread={threadKey(t)} data-group={t.group}>
      <div className="workspace-header">
        <div
          className="pane-select workspace-select"
          role="button"
          tabIndex={0}
          aria-current={selected ? "true" : undefined}
          title={tooltip}
          data-testid={`thread-${t.project}-${t.id}`}
          onClick={open}
          onKeyDown={(e) => onRowKey(e, open)}
        >
          <RowMark agent={agent} />
          {twoLine ? (
            <span className="workspace-copy is-two-line">
              <span className="workspace-line">
                <span className="workspace-name">{title}</span>
              </span>
              <span className={`workspace-place thread-state is-${t.group}`} data-testid={`thread-state-${t.project}-${t.id}`}>
                {line}
              </span>
            </span>
          ) : (
            <span className="workspace-copy">
              <span className="workspace-name">{title}</span>
              <span className={`worktree-workspace-label thread-state is-${t.group}`} data-testid={`thread-state-${t.project}-${t.id}`}>
                {GROUP_WORD[t.group] ?? t.group}
              </span>
            </span>
          )}
          <span className="visually-hidden">{agent ?? "agent"}</span>
          <PaneState pane={paneOpen ? pane : undefined} online={online} testid={`thread-agent-${t.project}-${t.id}`} />
        </div>
      </div>
    </li>
  );
}

/** 휴대폰 drawer 폭 아래에서는 Agents가 접힌 채 시작한다(참고 AgentSidebar `DRAWER_QUERY`) */
const DRAWER_QUERY = "(max-width: 768px)";

/**
 * 모든 머신의 살아 있는 agent를 한 목록으로(참고 AgentSidebar). 머신·프로젝트 접기와 상관없이 보인다.
 * agent는 harness hook이 상태를 알리는 pane이나 agent 명령으로 뜬 pane이다(셸은 상태를 알려도 넣지 않는다).
 */
function AgentSection(props: SidebarProps) {
  const listId = useId();
  const [collapsed, setCollapsed] = useState(() => window.matchMedia?.(DRAWER_QUERY).matches === true);
  const threadOf = new Map<string, ThreadView>();
  for (const t of props.threads) if (t.pane) threadOf.set(t.pane, t);
  const rows = props.machines.flatMap((m) =>
    props.panes
      .filter((p) => p.machine_id === m.id && p.state === "running" && p.role !== "evaluator")
      .flatMap((p) => {
        const t = threadOf.get(p.id);
        const agent = props.agentOf(p, t);
        return agent !== null ? [{ m, p, t, agent }] : [];
      }),
  );
  // 참고 agentContext: 이름이 아닌 말(종류)·머신(둘 이상일 때만)·자리. 같은 말을 두 번 하지 않는다
  const several = props.machines.length > 1;
  return (
    <section className={`agents-sidebar${collapsed ? " is-collapsed" : ""}${rows.length === 0 ? " is-empty" : ""}`} aria-label="Agents" data-testid="agents">
      <button type="button" className="agent-section-toggle sidebar-section-label" aria-expanded={!collapsed} aria-controls={listId} onClick={() => setCollapsed(!collapsed)}>
        {collapsed ? <ChevronRight className="agent-section-caret" aria-hidden="true" /> : <ChevronDown className="agent-section-caret" aria-hidden="true" />}
        <span>Agents</span>
        {collapsed && rows.length > 0 && <span className="agent-section-count">{rows.length}</span>}
      </button>
      <div className="agent-list-contents" id={listId} hidden={collapsed}>
        {rows.length === 0 ? (
          <p className="agent-empty" role="status">
            도는 agent 없음
          </p>
        ) : (
          <ul className="agent-list">
            {rows.map(({ m, p, t, agent }) => {
              const online = m.connection === "connected";
              const selected = props.selected === `pane:${p.id}` || (t != null && props.selected === `thread:${threadKey(t)}`);
              const title = t ? `${t.id} · ${t.title}` : p.name || p.argv.join(" ") || p.id.slice(0, 8);
              const kind = p.agent?.harness ?? agent;
              const place = t ? `${t.project}` : p.cwd ? shortPath(p.cwd) : null;
              const context = [kind && kind.toLowerCase() !== agent.toLowerCase() && kind !== title ? kind : null, several ? m.name : null, place].filter(Boolean).join(" · ");
              const tooltip = [...new Set([p.id, title, context, p.cwd ? shortPath(p.cwd) : null, online ? null : "끊김"].filter(Boolean))].join("\n");
              return (
                <li key={p.id} className={`agent-item${selected ? " is-selected" : ""}${online ? "" : " is-offline"}`} data-machine={m.id} data-pane={p.id}>
                  <button type="button" className="agent-select agent-row" disabled={!online} aria-current={selected ? "true" : undefined} title={tooltip} data-testid={`agent-row-${p.name || p.id}`} onClick={() => props.onPane(p.id)}>
                    <span className="sidebar-mark" aria-hidden="true">
                      <AgentMark agent={agent} size={18} />
                    </span>
                    <span className="agent-copy">
                      <span className="visually-hidden">{agent}: </span>
                      <span className="agent-title-line">
                        <span className="agent-title">{title}</span>
                      </span>
                      {context && <span className="agent-context">{context}</span>}
                    </span>
                    <span className="agent-row-state">
                      {online && paneProblem(p) ? <ProblemBadge reason={paneProblem(p)!} /> : <StatusBadge compact status={online ? paneAgentStatus(p) : undefined} />}
                    </span>
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </section>
  );
}
