// 왼쪽 목록(herdr-web-ui MachineSidebar·Sidebar·AgentSidebar, aa624cb). 모양(class)과 CSS는 참고 것을 그대로 쓴다(herdr/*.css).
// 패널 넷(활동 막대가 하나를 고른다, plan §8.240·§8.258·§8.280): 「작업」은 coordinator → 작업 공간(저장소) 폴더 → thread 줄(둘째 줄 왼쪽
// 상태 말, 오른쪽 머신 이름, plan §8.195) → thread가 아닌 pane의 「기타」 묶음(줄에 머신 이름). 프로젝트가 둘 이상일 때만 프로젝트 머리가
// 그 위에 선다. 예전 「에이전트」 패널(같은 pane을 저장소 폴더로 다시 보이던 것)은 이 트리로 합쳤다(사용자 결정 10-19 안 A).
// 「머신」은 머신 줄(이름·host·연결 점·+, 끊김·점검 글), 「특수」는 임무·사후 평가자. 머신은 트리 단계가 아니다. 넷 다 늘 그려 두고
// 「자동화」는 규칙·실행 기록을 보인다(plan §8.281). 고르지 않은 것은 숨긴다(굴린 자리를 지킨다). 머리·띠·바닥 줄(사용량, App.tsx ListFoot)은 패널과 상관없다.
// aaa에는 herdr workspace가 없다: 참고의 workspace 줄 자리에 pane 줄을, worktree 묶음 자리에 프로젝트 묶음을 둔다.
// thread는 자기 줄 하나로만 보인다(L4, herdr-projects sidebar.rs: thread는 자기 agent 줄이고 이름이 「id · 제목」).
import {
  createContext,
  Fragment,
  useCallback,
  useContext,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type AnimationEvent,
  type DragEvent,
  type KeyboardEvent,
  type MouseEvent,
  type ReactNode,
} from "react";
import { ChevronDown, ChevronRight, Ellipsis, Eraser, FileText, Folder, FolderOpen, Layers, ListChecks, Monitor, Pencil, Pin, PinOff, Plus, RotateCcw, Terminal, Trash2, TriangleAlert, Workflow, X } from "lucide-react";
import type { FrontRequest, MachineView, PaneView, PlanSummary, ThreadView } from "./gen/proto";
import { ApiError, call } from "./api";
import { ConfirmDialog } from "./ConfirmDialog";
import { PaneRename } from "./PaneRename";
import { RowMenu, type RowMenuItem } from "./RowMenu";
import { currentSidebarPins, setSidebarPins, sidebarPinKey, splitPinnedRows, subscribeSidebarPins, toggleSidebarPin } from "./herdr/sidebarPins.ts";
import { AgentMark } from "./herdr/AgentMark.tsx";
import { indicatorGlyph, knownStatus, rollupStatus, STATUS_WORD, type AgentStatus, type KnownStatus } from "./herdr/status.ts";
import { useAppearance } from "./theme";
import { PANEL_LABEL, type SidebarPanel } from "./sidebarPanel";
import { updateSidebarShared, useMedia, useSidebarShared } from "./settings";
import { watchTouchReorder } from "./herdr/touchReorder.ts";
import { placeBlock } from "./herdr/workspaceOrder.ts";
import { GROUP_WORD, needsYou, paneAgentStatus, paneEnded, paneProblem, projectLine, roster, shortPath, stateLine, stateTone, threadKey, workingStartedAt, type WorkFolder } from "./sidebarModel";
import { useSummaryHover } from "./ThreadSummary";
import { formatResetIn } from "./usage";
import { WorkingDuration } from "./WorkingDuration";
import "./herdr/primitives.css";
import "./herdr/Sidebar.css";
import "./herdr/Machines.css";
import "./Sidebar.css";
import { AutomationPanel } from "./AutomationPanel";
import { RolloutCard, type RolloutSnapshot } from "./RolloutCard";
import { MachineMeters } from "./MachineMeters";
import { machineStats } from "./machineStats";

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
/**
 * 그 agent가 띄워 아직 도는 배경 작업(하위 agent·배경 명령) 수(herdr-web-ui Sidebar.tsx `BackgroundBadge`): 상태 옆의 조용한 수다.
 * 상태가 아니다: agent가 답했으면 「끝남」은 그대로다.
 */
export function BackgroundBadge({ count }: { count?: number | null }) {
  if (!count || count <= 0) return null;
  const label = `배경 작업 ${count}개 실행 중`;
  return (
    <span className="background-count" title={label} aria-label={label} data-testid="background-tasks">
      <Layers aria-hidden="true" />
      {count}
    </span>
  );
}

/** pane 계획이 어디까지 왔나(herdr-web-ui `PlanBadge`): 상태 옆의 n/m, 모든 단계가 끝나면 조용해진다. */
export function PlanBadge({ plan }: { plan?: PlanSummary | null }) {
  if (!plan || plan.total <= 0) return null;
  const label = `계획 ${plan.total}단계 중 ${plan.done}단계 끝남`;
  return (
    <span className={`plan-count${plan.done === plan.total ? " is-done" : ""}`} title={plan.current ? `${label} · ${plan.current}` : label} aria-label={label} data-testid="plan-progress">
      <ListChecks aria-hidden="true" />
      {plan.done}/{plan.total}
    </span>
  );
}

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
function useFold(key: string, initial = false, oldKey?: string): [boolean, () => void] {
  const [folded, setFolded] = useState(() => {
    try {
      // 예전 이름의 기억(에이전트 패널 저장소 폴더 접힘)은 새 이름이 없을 때 읽는다
      const v = localStorage.getItem(key) ?? (oldKey ? localStorage.getItem(oldKey) : null);
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

/**
 * 줄의 ⋯ 메뉴를 연다(참고 Sidebar.tsx `onRowContextMenu`). 오른쪽 클릭은 마우스일 때만(터치의 길게 누르기는 브라우저 몫),
 * 메뉴는 줄의 ⋯ 단추 자리(데스크톱에서 단추가 숨으면 누른 자리)에 열리고, 닫히면 누르기 전 focus로 돌아간다.
 */
function onRowContextMenu(event: MouseEvent<HTMLElement>, toggle: (anchor: HTMLElement, point?: { x: number; y: number }, previousFocus?: HTMLElement | null) => void): void {
  if ((event.target as HTMLElement).closest("input")) return;
  const pointer = (event.nativeEvent as PointerEvent).pointerType;
  if (pointer ? pointer === "touch" : window.matchMedia("(pointer: coarse)").matches) return;
  const toggleButton = event.currentTarget.querySelector<HTMLElement>(".row-menu-toggle");
  if (!toggleButton) return;
  const anchor = toggleButton.getClientRects().length > 0 ? toggleButton : event.currentTarget;
  event.preventDefault();
  const box = anchor.getBoundingClientRect();
  toggle(anchor, event.clientX || event.clientY ? { x: event.clientX, y: event.clientY } : { x: box.left, y: box.bottom }, document.activeElement instanceof HTMLElement ? document.activeElement : null);
}

/** 오른쪽 버튼(맥의 Ctrl+클릭)은 focus를 옮기지 않는다: 메뉴가 닫히면 누르기 전 자리로 돌아간다(참고 onMouseDownCapture) */
const keepFocusOnMenuPress = (event: MouseEvent<HTMLElement>) => {
  if ((event.button === 2 || (event.button === 0 && event.ctrlKey)) && !(event.target as HTMLElement).closest("input")) event.preventDefault();
};

/** 열린 줄 메뉴: 어느 줄의 것인지(`key`), 어디에, 무엇을 */
interface MenuState {
  key: string;
  anchor: HTMLElement;
  point?: { x: number; y: number };
  previousFocus?: HTMLElement | null;
  title: string;
  place: string;
  items: () => RowMenuItem[];
  /** 제목 대신 둘 머리(특수 줄: 제목과 셈 한 줄) */
  header?: ReactNode;
}
/** 되돌릴 수 없는 일 앞의 확인(참고 Sidebar.tsx `setConfirm`) */
interface ConfirmState {
  title: string;
  body: string;
  action: string;
  run: () => Promise<void>;
}
/** 줄들이 함께 쓰는 것: 고정(host 공유 문서, preferencesSync), 메뉴, 확인 창, 줄 아래 오류 */
interface Rows {
  pins: string[];
  togglePin: (key: string, from: HTMLElement | null | undefined) => void;
  menuKey: string | null;
  toggleMenu: (state: MenuState) => void;
  ask: (c: ConfirmState) => void;
  /** pane 이름 창(참고 PaneCanvas `setRenaming`) */
  rename: (pane: PaneView) => void;
  act: (key: string, req: FrontRequest) => void;
  error: { key: string; message: string } | null;
}
const RowsContext = createContext<Rows | null>(null);
const useRows = () => useContext(RowsContext)!;

/** ⋯ 단추(참고 `row-menu-toggle`): 터치·좁은 화면에서 보이고, 데스크톱은 오른쪽 클릭이 같은 메뉴를 연다 */
function RowMenuButton({ title, open, onToggle }: { title: string; open: boolean; onToggle: (anchor: HTMLElement) => void }) {
  return (
    <div className="workspace-actions">
      <button type="button" className="sidebar-row-action row-menu-toggle" aria-label={`${title} 더보기`} aria-haspopup="menu" aria-expanded={open} onClick={(e) => onToggle(e.currentTarget)}>
        <Ellipsis aria-hidden="true" />
      </button>
    </div>
  );
}

export interface SidebarProps {
  machines: MachineView[];
  rollout?: RolloutSnapshot;
  connected?: boolean;
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
  /** 모든 thread(끝난 것까지) 화면 */
  onThreads: () => void;
  /** 자동화 패널을 보이고 그 규칙 상세를 연다(thread 줄 「자동 · 규칙」 표지, plan t-0475 A1) */
  onAutomation: (project: string, name: string) => void;
  /** thread 번호로 연다(자동화 패널의 「열림」 칩·기록 번호): 끝난 thread는 보고 */
  onThreadId: (project: string, id: string) => void;
  /** 자동화 패널이 열 규칙 상세. 패널이 받으면 `onRuleAsked`로 지운다 */
  ruleAsk: { project: string; name: string } | null;
  onRuleAsked: () => void;
  /** 목록 위에 띄울 알림 띠(연결 끊김·오류) */
  banners?: ReactNode;
  /** 보이는 패널(활동 막대) */
  panel: SidebarPanel;
  /** 「답할 것」 패널 목록(App이 그린다, plan §8.333) */
  asks?: ReactNode;
  /** 특수 패널: 임무·사후 평가자(App이 읽는다) */
  /** 임무 동작 뒤 다시 읽기 */
}

export function Sidebar(props: SidebarProps) {
  // 고정은 이 기기 사본(localStorage)이고 host 공유 문서로 나간다(preferencesSync, plan §8.79)
  const pins = useSyncExternalStore(subscribeSidebarPins, currentSidebarPins, currentSidebarPins);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [confirm, setConfirm] = useState<ConfirmState | null>(null);
  const [renaming, setRenaming] = useState<PaneView | null>(null);
  const [error, setError] = useState<{ key: string; message: string } | null>(null);
  const closeMenu = useCallback(() => setMenu(null), []);
  // 고정하면 줄이 다른 목록(고정 카드)으로 옮겨 다시 그려진다: 메뉴를 연 곳(줄이나 ⋯)으로 focus를 되돌린다(참고 AgentSidebar refocusPin)
  const refocus = useRef<{ key: string; scope: string; control: string } | null>(null);
  useLayoutEffect(() => {
    const r = refocus.current;
    if (!r) return;
    refocus.current = null;
    document.querySelector<HTMLElement>(`${r.scope} [data-pin-key="${CSS.escape(r.key)}"] .${r.control}`)?.focus({ preventScroll: true });
  }, [pins]);
  const rows: Rows = {
    pins,
    togglePin: (key, from) => {
      if (from?.closest(`[data-pin-key="${CSS.escape(key)}"]`)) {
        const control = from.classList.contains("row-menu-toggle") ? "row-menu-toggle" : "pane-select";
        refocus.current = { key, scope: ".machine-list", control };
      }
      setSidebarPins(toggleSidebarPin(pins, key));
    },
    menuKey: menu?.key ?? null,
    toggleMenu: (state) => setMenu(menu?.key === state.key ? null : state),
    ask: setConfirm,
    rename: setRenaming,
    act: (key, req) => {
      setError(null);
      call(req).catch((e: unknown) => setError({ key, message: e instanceof ApiError ? e.message : String(e) }));
    },
    error,
  };
  return (
    <RowsContext.Provider value={rows}>
      {props.banners}
      <div {...panelProps("asks", props.panel)} className="sidebar-panel asks-sidebar">
        {props.panel === "asks" && props.asks}
      </div>
      <div {...panelProps("work", props.panel)} className="list-body machine-list sidebar-panel">
        <Roster {...props} />
      </div>
      <div {...panelProps("machines", props.panel)} className="sidebar-panel machines-panel">
        <RolloutCard snapshot={props.rollout ?? null} connected={props.connected ?? false} />
        <MachineStrip {...props} />
      </div>
      <div {...panelProps("automations", props.panel)} className="sidebar-panel automations-panel">
        {props.panel === "automations" && <AutomationPanel canControl={props.canControl} threads={props.threads} onThread={props.onThreadId} ask={props.ruleAsk} onAsked={props.onRuleAsked} />}
      </div>
      {menu && <RowMenu anchor={menu.anchor} point={menu.point} restoreFocusTo={menu.previousFocus} title={menu.title} subtitle={menu.place} header={menu.header} items={menu.items()} onClose={closeMenu} />}
      {confirm && (
        <ConfirmDialog
          title={confirm.title}
          body={confirm.body}
          confirmLabel={confirm.action}
          onConfirm={async () => {
            await confirm.run();
            setConfirm(null);
          }}
          onClose={() => setConfirm(null)}
        />
      )}
      {renaming && (
        <PaneRename
          label={renaming.label ?? null}
          onClose={() => setRenaming(null)}
          onSave={async (label) => void (await call({ op: "pane_rename", pane: renaming.id, label: label || null }))}
        />
      )}
    </RowsContext.Provider>
  );
}

/** 활동 막대 단추가 여닫는 패널(VS Code는 tabpanel이 아닌 view container지만 단추가 tab이라 그 짝으로 둔다) */
function panelProps(panel: SidebarPanel, shown: SidebarPanel) {
  return {
    id: `sidebar-panel-${panel}`,
    role: "tabpanel",
    "aria-labelledby": `activity-tab-${panel}`,
    "data-panel": panel,
    hidden: panel !== shown,
  } as const;
}

/** 그 줄의 일이 실패했을 때 줄 아래 한 줄(참고 `sidebar-inline-error`) */
function RowError({ rowKey }: { rowKey: string }) {
  const { error } = useRows();
  return error?.key === rowKey ? (
    <p className="sidebar-inline-error" role="alert">
      {error.message}
    </p>
  ) : null;
}

/**
 * 머신 패널(plan §8.195·§8.240·§8.245): 목록이 프로젝트 먼저라 머신은 트리 단계가 아니다. 머신마다 머리 한 줄(이름·host·연결 점·CPU·MEM·+ 새 pane)과
 * 끊김·점검 이상 글만 둔다(참고 MachineSidebar 머리·machine-state). 그 머신의 줄은 프로젝트 패널에 머신 글자로 있다.
 */
/** 그 머신에서 도는 agent(머신 줄 agent 수, plan §8.256): thread·coordinator·thread 아닌 agent pane, 평가자 빼고 */
function machineAgents(props: SidebarProps, m: MachineView, threadOf: Map<string, ThreadView>) {
  return props.panes
    .filter((p) => p.machine_id === m.id && p.state === "running")
    .flatMap((p) => {
      const t = threadOf.get(p.id);
      const agent = props.agentOf(p, t);
      return agent !== null ? [{ m, p, t, agent }] : [];
    });
}

function threadsByPane(threads: ThreadView[]) {
  const threadOf = new Map<string, ThreadView>();
  for (const t of threads) if (t.pane) threadOf.set(t.pane, t);
  return threadOf;
}

function MachineStrip(props: SidebarProps) {
  const threadOf = threadsByPane(props.threads);
  return (
    <section className="machine-strip" aria-label={PANEL_LABEL.machines}>
      {props.machines.map((m) => {
        const online = m.connection === "connected";
        const problems = online && m.problems.length > 0 ? m.problems.map((p) => p.text).join("\n") : null;
        const stats = machineStats(m);
        const agents = machineAgents(props, m, threadOf);
        return (
          <section key={m.id} className={`machine-group${online ? "" : " is-offline"}`} aria-label={`머신 ${m.name}`} data-testid={`machine-${m.name}`}>
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
              {/* agent·CPU·MEM(plan §8.245·§8.256): 연결 중일 때만 */}
              {stats && (
                <MachineMeters
                  stats={stats}
                  name={m.name}
                  agents={{
                    count: agents.length,
                    working: agents.filter((r) => r.p.agent?.activity === "working").length,
                    waiting: agents.filter((r) => r.p.agent?.activity === "question").length,
                  }}
                />
              )}
              {props.canControl && (
                <button
                  type="button"
                  className="sidebar-row-action machine-new"
                  disabled={!online}
                  aria-label={`${m.name}에 새 pane`}
                  title="새 pane"
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
          </section>
        );
      })}
    </section>
  );
}

/**
 * 「작업」 트리(plan §8.280): coordinator → 작업 공간 폴더 → thread(둘째 줄에 머신 이름), 다음 thread에 딸리지 않은 pane의 「기타」
 * 묶음(줄마다 머신 이름). 프로젝트가 둘 이상이면 프로젝트 머리가 그 위에 선다. 차례는 단축키 「이전·다음 패널」과 같은 계산(sidebarModel
 * `roster`, 사람이 옮긴 pane 차례·폴더 안 정렬 포함). 끊긴 머신의 줄은 흐리고 누를 수 없다(참고 machine-offline·inert를 줄마다).
 */
function Roster(props: SidebarProps) {
  const { appearance } = useAppearance();
  const { pins } = useRows();
  const twoLine = appearance.sidebarRows === "two";
  const { paneOrder } = useSidebarShared();
  const { threads, folders, projects, coordinators, panes, machineOf } = roster(props.machines, props.panes, props.threads, paneOrder, appearance.agentOrder === "activity");
  // 고정한 thread는 맨 앞 카드에(예전 에이전트 패널처럼 thread 줄도 고정한다, 열쇠는 그 agent pane). 폴더에서는 빠진다
  const threadPin = (t: ThreadView) => {
    const p = t.pane ? props.panes.find((x) => x.id === t.pane) : undefined;
    return p ? sidebarPinKey(p.machine_id, p.id) : "";
  };
  const pinnedThreads = splitPinnedRows(threads, pins, threadPin).pinned;
  const unpinned = (f: WorkFolder): WorkFolder => ({ ...f, threads: f.threads.filter((t) => !pinnedThreads.includes(t)) });
  const shownFolders = folders.map(unpinned).filter((f) => f.threads.length > 0);
  const machineById = (id: string) => props.machines.find((m) => m.id === id);
  const online = (m: MachineView | undefined) => m?.connection === "connected";
  const [otherCollapsed, toggleOther] = useFold("aaa:other-collapsed");
  // 고정한 pane은 맨 앞 카드에 고정한 순서로(참고 Sidebar.tsx `splitPinnedGroups`·`sidebar-pinned`)
  const split = splitPinnedRows(panes, pins, (p) => sidebarPinKey(p.machine_id, p.id));
  // 줄 옮기기(참고 Sidebar.tsx `reorderWorkspaces`·`moveVisible`·`dropOrder`·`watchTouchReorder`, plan §8.196): 마우스는 끌기,
  // Alt+↑↓, 손가락은 길게 눌러 끌기. 「기타」의 고정하지 않은 pane 줄끼리 옮긴다(머신을 넘어서도, thread 줄은 묶음 차례를 따른다).
  // 참고의 순서는 herdr에 보내는 요청이라 조작 권한 기기만 옮긴다. 순서는 공유 설정 `paneOrder`로 이 host의 다른 화면에도 간다
  const movable = props.canControl;
  const coarsePointer = useMedia("(pointer: coarse)");
  const restKeys = split.rest.map((p) => sidebarPinKey(p.machine_id, p.id));
  const reorder = (next: string[]) => {
    if (next.length === restKeys.length && next.every((key, index) => key === restKeys[index])) return;
    updateSidebarShared({ paneOrder: [...paneOrder.filter((key) => !next.includes(key)), ...next] });
  };
  /** 놓은 줄이 만드는 차례: 아래로 옮기면 놓은 줄 뒤, 위로 옮기면 앞 */
  const dropOrder = (source: string, target: string): string[] | null => {
    const from = restKeys.indexOf(source);
    const to = restKeys.indexOf(target);
    if (from < 0 || to < 0 || from === to) return null;
    return placeBlock(restKeys, [source], [target], from < to ? "after" : "before");
  };
  const moveVisible = (key: string, direction: -1 | 1) => {
    const index = restKeys.indexOf(key);
    const target = restKeys[index + direction];
    if (index < 0 || target === undefined) return;
    reorder(placeBlock(restKeys, [key], [target], direction === 1 ? "after" : "before"));
  };
  const list = useRef<HTMLUListElement>(null);
  const [dragKey, setDragKey] = useState<string | null>(null);
  const [touchDrag, setTouchDrag] = useState<{ source: string; over: string | null } | null>(null);
  const touchHandlers = useRef<Parameters<typeof watchTouchReorder>[1] | null>(null);
  touchHandlers.current = {
    canLift: () => movable,
    onLift: (id) => setTouchDrag({ source: id, over: id }),
    onOver: (id) => setTouchDrag((current) => current && { ...current, over: id }),
    onDrop: (id, target) => {
      setTouchDrag(null);
      const next = target === null || target === id ? null : dropOrder(id, target);
      if (next !== null) reorder(next);
    },
    onCancel: () => setTouchDrag(null),
  };
  // 「기타」 줄 목록(ul)은 줄이 있고 펼쳤을 때만 있다: 그때마다 손가락 감시를 다시 붙인다
  const shown = split.rest.length > 0 && !otherCollapsed;
  useEffect(() => {
    const root = list.current;
    if (!root) return;
    return watchTouchReorder(root, {
      canLift: (id) => touchHandlers.current?.canLift(id) ?? false,
      onLift: (id) => touchHandlers.current?.onLift(id),
      onOver: (id) => touchHandlers.current?.onOver(id),
      onDrop: (id, target) => touchHandlers.current?.onDrop(id, target),
      onCancel: () => touchHandlers.current?.onCancel(),
    });
  }, [shown]);
  /** 들어 올린 줄이 놓일 자리(줄 앞이나 뒤의 선) */
  const dropMark = ((): { id: string; side: "before" | "after" } | null => {
    if (!touchDrag?.over || touchDrag.over === touchDrag.source || dropOrder(touchDrag.source, touchDrag.over) === null) return null;
    return { id: touchDrag.over, side: restKeys.indexOf(touchDrag.source) < restKeys.indexOf(touchDrag.over) ? "after" : "before" };
  })();
  const moveOf = (p: PaneView): RowMove | undefined => {
    if (!movable) return undefined;
    const key = sidebarPinKey(p.machine_id, p.id);
    return {
      key,
      draggable: !coarsePointer,
      dragging: dragKey === key,
      lifted: touchDrag?.source === key,
      drop: dropMark?.id === key ? dropMark.side : undefined,
      onDragStart: (event) => {
        setDragKey(key);
        event.dataTransfer.effectAllowed = "move";
        event.dataTransfer.setData(PANE_ROW_DRAG, JSON.stringify({ key }));
      },
      onDragEnd: () => setDragKey(null),
      onDrop: (event) => {
        event.preventDefault();
        let payload: { key?: string };
        try {
          payload = JSON.parse(event.dataTransfer.getData(PANE_ROW_DRAG));
        } catch {
          return;
        }
        setDragKey(null);
        if (typeof payload.key !== "string") return;
        const next = dropOrder(payload.key, key);
        if (next !== null) reorder(next);
      },
      onMove: (direction) => moveVisible(key, direction),
    };
  };
  const paneRow = (p: PaneView, move?: RowMove) => {
    const m = machineById(p.machine_id);
    return <PaneRow key={p.id} {...props} online={online(m)} machine={m} pane={p} twoLine={twoLine} move={move} />;
  };
  if (projects.length === 0 && panes.length === 0) {
    return (
      <div className="machine-workspaces">
        <nav className="sidebar-list" aria-label="목록">
          <div className="tree-state-empty">
            <p className="tree-state" role="status">
              pane 없음
            </p>
          </div>
        </nav>
      </div>
    );
  }
  const otherLabel = otherCollapsed ? "기타 pane 펼치기" : "기타 pane 접기";
  return (
    <div className={`machine-workspaces${twoLine ? " is-two-line" : ""}`}>
      <nav className="sidebar-list" aria-label="목록">
        {(pinnedThreads.length > 0 || split.pinned.length > 0) && (
          <ul className="workspace-list workspace-list-pinned sidebar-pinned" aria-label="고정됨">
            {pinnedThreads.map((t) => (
              <ThreadRow key={threadKey(t)} {...props} online={machineOf(t)?.connection === "connected"} machine={machineOf(t)} t={t} twoLine={twoLine} />
            ))}
            {split.pinned.map((p) => paneRow(p))}
          </ul>
        )}
        <ul className="workspace-list">
          {/* 프로젝트 머리는 없다(host는 프로젝트 하나, plan §8.291): 옮겨 온 기록만 있는 다른 이름이 남았으면 그 안을 차례로 잇는다 */}
          {projects.map((project) => (
            <WorkTree key={project} {...props} project={project} folders={shownFolders.filter((f) => f.project === project)} coordinators={projects.length === 1 ? coordinators : coordinators.filter((p) => p.coordinator_project === project)} machineOf={machineOf} twoLine={twoLine} />
          ))}
          {split.rest.length > 0 && (
            <>
              <li className={`workspace workspace-group pane-item project-group other-group${otherCollapsed ? " is-collapsed" : ""}`} data-testid="other-group">
                <div className="workspace-header">
                  <button type="button" className="sidebar-row-action workspace-toggle" aria-label={otherLabel} title={otherLabel} aria-expanded={!otherCollapsed} onClick={toggleOther}>
                    {otherCollapsed ? <Folder aria-hidden="true" /> : <FolderOpen aria-hidden="true" />}
                  </button>
                  <div className="pane-select workspace-select project-select" role="button" tabIndex={0} title={otherLabel} onClick={toggleOther} onKeyDown={(e) => onRowKey(e, toggleOther)}>
                    <span className={`workspace-copy${twoLine ? " is-two-line" : ""}`}>
                      <span className="workspace-line">
                        <span className="workspace-name project-name">기타</span>
                        {otherCollapsed && (
                          <span className="workspace-fold-count" aria-hidden="true">
                            +{split.rest.length}
                          </span>
                        )}
                      </span>
                      {twoLine && <span className="workspace-place">pane {split.rest.length}</span>}
                    </span>
                    <span className="sidebar-pane-meta">
                      <StatusBadge compact status={undefined} />
                    </span>
                  </div>
                </div>
              </li>
              {shown && (
                <li className="worktree-children">
                  <ul className="workspace-list" ref={list}>
                    {split.rest.map((p) => paneRow(p, moveOf(p)))}
                  </ul>
                </li>
              )}
            </>
          )}
        </ul>
      </nav>
    </div>
  );
}

/** 끌어 옮기는 pane 줄의 끌기 자료 형식(「기타」 목록 안에서만 받는다) */
const PANE_ROW_DRAG = "application/x-aaa-pane-row";

/** 옮길 수 있는 줄이 받는 것(Roster가 만든다) */
interface RowMove {
  key: string;
  draggable: boolean;
  dragging: boolean;
  lifted: boolean;
  drop: "before" | "after" | undefined;
  onDragStart: (event: DragEvent<HTMLElement>) => void;
  onDragEnd: () => void;
  onDrop: (event: DragEvent<HTMLElement>) => void;
  onMove: (direction: -1 | 1) => void;
}

/** 첫 줄 끝의 조용한 수(예전 에이전트 패널 줄, herdr-web-ui AgentSidebar): 배경 작업 수·계획 n/m */
function AgentCounts({ pane }: { pane: PaneView | undefined }) {
  if (!pane || pane.state !== "running") return null;
  return (
    <>
      <BackgroundBadge count={pane.agent?.background} />
      <PlanBadge plan={pane.agent?.plan} />
    </>
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

function PaneRow({ pane: p, online, machine, twoLine, move, ...props }: SidebarProps & { pane: PaneView; online: boolean; machine: MachineView | undefined; twoLine: boolean; move?: RowMove }) {
  const selected = props.selected === `pane:${p.id}`;
  const agent = props.agentOf(p);
  const title = p.name || p.argv.join(" ") || p.id.slice(0, 8);
  const ended = paneEnded(p);
  // 머신은 트리 단계가 아니라 줄에 글자로(plan §8.195)
  const machineName = machine?.name ?? p.machine;
  const where = [machineName, p.cwd ? shortPath(p.cwd) : null, ended].filter(Boolean).join(" · ");
  const tooltip = [p.id, title, p.cwd ? shortPath(p.cwd) : null, p.agent?.harness, ended].filter(Boolean).join("\n");
  const open = () => props.onPane(p.id);
  const rows = useRows();
  const pinKey = sidebarPinKey(p.machine_id, p.id);
  const pinned = rows.pins.includes(pinKey);
  const menuKey = `pane:${p.id}`;
  // 참고 workspace 메뉴(Sidebar.tsx `menuItems`) 차례: 고정 → 이름 바꾸기 → 닫기. 이름은 pane 이름(참고 PaneCanvas `itemsFor`의
  // 「Rename pane」·붙인 이름이 있을 때 「Clear pane name」). 끝난 pane은 닫는 대신 다시 열기·기록 지우기(aaa PaneResume·PaneForget)
  const items = (): RowMenuItem[] => [
    ...(p.coordinator_project ? [] : [pinned
      ? { id: "unpin", label: "pane 고정 해제", icon: PinOff, run: () => rows.togglePin(pinKey, menuFrom.current) }
      : { id: "pin", label: "pane 고정", icon: Pin, run: () => rows.togglePin(pinKey, menuFrom.current) }]),
    ...(!props.canControl
      ? []
      : [
          { id: "rename", label: "pane 이름 바꾸기", icon: Pencil, run: () => rows.rename(p) },
          ...(p.label ? [{ id: "clear-name", label: "pane 이름 지우기", icon: Eraser, run: () => rows.act(menuKey, { op: "pane_rename", pane: p.id, label: null }) }] : []),
        ]),
    ...(!props.canControl
      ? []
      : p.state === "running"
        ? [
            {
              id: "close",
              label: "pane 닫기",
              icon: X,
              danger: true,
              divider: true,
              run: () =>
                rows.ask({
                  title: `${title} pane을 닫을까요?`,
                  body: "pane에서 실행 중인 프로세스가 종료됩니다.",
                  action: "pane 닫기",
                  run: async () => void (await call({ op: "pane_close", pane: p.id })),
                }),
            },
          ]
        : [
            { id: "resume", label: "다시 열기", icon: RotateCcw, run: () => rows.act(menuKey, { op: "pane_resume", pane: p.id }) },
            {
              id: "forget",
              label: "기록 지우기",
              icon: Trash2,
              danger: true,
              divider: true,
              run: () =>
                rows.ask({
                  title: `${title} pane 기록을 지울까요?`,
                  body: "끝난 pane이 목록에서 사라지고 다시 열 수 없습니다.",
                  action: "지우기",
                  run: async () => void (await call({ op: "pane_forget", pane: p.id })),
                }),
            },
          ]),
  ];
  // 메뉴를 연 곳(⋯이나 오른쪽 클릭 전 focus): 고정으로 줄이 옮겨 가도 focus를 되돌린다
  const menuFrom = useRef<HTMLElement | null>(null);
  const toggleMenu = (anchor: HTMLElement, point?: { x: number; y: number }, previousFocus?: HTMLElement | null) => {
    menuFrom.current = previousFocus === undefined ? anchor : previousFocus;
    rows.toggleMenu({ key: menuKey, anchor, point, previousFocus, title, place: where || p.machine, items });
  };
  return (
    <li
      className={`workspace workspace-group pane-item${p.coordinator_project ? " is-coordinator" : ""}${selected ? " is-selected" : ""}${p.state !== "running" ? " is-ended" : ""}${online ? "" : " is-offline"}${move?.dragging ? " is-dragging" : ""}${move?.lifted ? " is-lifted" : ""}`}
      data-origin={p.coordinator_project ? "coordinator" : undefined}
      inert={!online}
      data-pane={p.id}
      data-pin-key={pinKey}
      data-workspace={move?.key}
      data-drop={move?.drop}
      onDragOver={
        move &&
        ((event) => {
          if (!event.dataTransfer.types.includes(PANE_ROW_DRAG)) return;
          event.preventDefault();
          event.dataTransfer.dropEffect = "move";
        })
      }
      onDrop={move?.onDrop}
    >
      <div className="workspace-header" onMouseDownCapture={keepFocusOnMenuPress} onContextMenu={(e) => onRowContextMenu(e, toggleMenu)}>
        <div
          className="pane-select workspace-select"
          role="button"
          tabIndex={0}
          aria-current={selected ? "true" : undefined}
          title={tooltip}
          data-testid={`pane-${p.name || p.id}`}
          draggable={move?.draggable}
          onDragStart={move?.onDragStart}
          onDragEnd={move?.onDragEnd}
          onClick={open}
          onKeyDown={(e) => {
            // 참고 `onRowKeyDown`: Alt+↑↓는 보이는 이웃 줄 너머로 옮긴다
            if (move && e.altKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
              e.preventDefault();
              move.onMove(e.key === "ArrowUp" ? -1 : 1);
              return;
            }
            onRowKey(e, open);
          }}
        >
          <RowMark agent={agent} />
          {twoLine ? (
            <span className="workspace-copy is-two-line">
              <span className="workspace-line">
                <span className="workspace-name">{title}</span>
                {pinned && <Pin className="sidebar-pin-mark" aria-hidden="true" />}
                {online && <AgentCounts pane={p} />}
              </span>
              {where && <span className="workspace-place">{where}</span>}
            </span>
          ) : (
            <span className="workspace-copy">
              <span className="workspace-name">{title}</span>
              {pinned && <Pin className="sidebar-pin-mark" aria-hidden="true" />}
              {online && <AgentCounts pane={p} />}
              <span className="worktree-workspace-label">{[machineName, ended].filter(Boolean).join(" · ")}</span>
            </span>
          )}
          <span className="visually-hidden">{agent ?? "셸"}</span>
          <PaneState pane={p} online={online} testid={p.agent ? `agent-${p.name || p.id}` : undefined} />
        </div>
        <RowMenuButton title={title} open={rows.menuKey === menuKey} onToggle={(anchor) => toggleMenu(anchor)} />
      </div>
      <RowError rowKey={menuKey} />
    </li>
  );
}

type TreeProps = SidebarProps & { project: string; folders: WorkFolder[]; coordinators: PaneView[]; machineOf: (t: ThreadView) => MachineView | undefined; twoLine: boolean };

/** 묶음 머리의 이름·접힌 수(+n)·둘째 줄 셈(확인 필요가 있으면 빨강) */
function GroupCopy({ name, folded, line, need, twoLine }: { name: string; folded: number; line: string; need: boolean; twoLine: boolean }) {
  const count = folded > 0 && (
    <span className="workspace-fold-count" aria-hidden="true">
      +{folded}
    </span>
  );
  return twoLine ? (
    <span className="workspace-copy is-two-line">
      <span className="workspace-line">
        <span className="workspace-name project-name">{name}</span>
        {count}
      </span>
      <span className={`workspace-place project-place${need ? " is-need" : ""}`}>{line}</span>
    </span>
  ) : (
    <span className="workspace-copy">
      <span className="workspace-name project-name">{name}</span>
      {count}
    </span>
  );
}

/**
 * 프로젝트 하나의 안(plan §8.280): coordinator 줄(보라 막대, plan §8.258) → 작업 공간 폴더. 저장소를 아는 thread가 없으면(옛 host) 폴더
 * 없이 「thread n」 머리줄 아래 thread 줄(plan §8.258 m2). 폴더 머리가 나눔이라 폴더가 있으면 그 머리줄은 없다.
 */
function WorkTree({ project, folders, coordinators, machineOf, twoLine, ...props }: TreeProps) {
  const online = (t: ThreadView) => machineOf(t)?.connection === "connected";
  const threadRow = (t: ThreadView) => <ThreadRow key={threadKey(t)} {...props} online={online(t)} machine={machineOf(t)} t={t} twoLine={twoLine} />;
  return (
    <>
      {coordinators.map((p) => (
        <PaneRow key={p.id} {...props} pane={p} machine={props.machines.find((m) => m.id === p.machine_id)} online={p.connection === "connected"} twoLine={twoLine} />
      ))}
      {folders.map((f) =>
        f.name === null ? (
          <Fragment key={`${f.project}/`}>
            {/* coordinator와 그 아래 thread를 나누는 머리줄(사용자 결정 10-19 m2, plan §8.258) */}
            {coordinators.length > 0 && f.threads.length > 0 && (
              <li className="project-thread-head" aria-hidden="true" data-testid={`project-thread-head-${project}`}>
                thread {f.threads.length}
              </li>
            )}
            {f.threads.map(threadRow)}
          </Fragment>
        ) : (
          <WorkFolderGroup key={`${f.project}/${f.name}`} name={f.name} threads={f.threads} panes={props.panes} selected={props.selected} machineOf={machineOf} twoLine={twoLine} threadRow={threadRow} />
        ),
      )}
    </>
  );
}

/**
 * 작업 공간 폴더 하나(plan §8.260·§8.280): 프로젝트 묶음 머리와 같은 모양·class라 자식 줄 들여쓰기(8px)·접힘 표시(+n, 대표 상태 점)가 같은
 * 규칙을 탄다. 접힌 폴더도 고른 줄은 보인다. 접힘은 기기마다 `aaa:workspace-collapsed:<이름>`(예전 에이전트 패널의 같은 폴더 접힘을 이어받는다).
 */
function WorkFolderGroup({ name, threads, panes, selected, machineOf, twoLine, threadRow }: { name: string; threads: ThreadView[]; panes: PaneView[]; selected: string | null; machineOf: (t: ThreadView) => MachineView | undefined; twoLine: boolean; threadRow: (t: ThreadView) => ReactNode }) {
  const [collapsed, toggle] = useFold(`aaa:workspace-collapsed:${name}`, false, `aaa:agent-folder-collapsed:${name}`);
  const contentsId = `${useId()}-threads`;
  const isSel = (t: ThreadView) => selected === `thread:${threadKey(t)}` || (t.pane != null && selected === `pane:${t.pane}`);
  const shown = collapsed ? threads.filter(isSel) : threads;
  const paneOf = (id: string | null) => (id ? panes.find((p) => p.id === id) : undefined);
  const status = rollupStatus(threads.map((t) => (machineOf(t)?.connection === "connected" && paneOf(t.pane) ? paneAgentStatus(paneOf(t.pane)!) : undefined)));
  const need = threads.some((t) => needsYou(t.group));
  const line = projectLine(threads.map((t) => t.group));
  const label = collapsed ? `${name} 펼치기` : `${name} 접기`;
  const rows = useRows();
  const menuKey = `workspace-folder:${name}`;
  const toggleMenu = (anchor: HTMLElement, point?: { x: number; y: number }, previousFocus?: HTMLElement | null) =>
    rows.toggleMenu({
      key: menuKey,
      anchor,
      point,
      previousFocus,
      title: name,
      place: line,
      items: () => [{ id: "fold", label: collapsed ? "펼치기" : "접기", icon: collapsed ? ChevronRight : ChevronDown, run: toggle }],
    });
  return (
    <>
      <li className={`workspace workspace-group pane-item project-group workspace-folder${collapsed ? " is-collapsed" : ""}`} data-testid={`workspace-folder-${name}`}>
        <div className="workspace-header" onMouseDownCapture={keepFocusOnMenuPress} onContextMenu={(e) => onRowContextMenu(e, toggleMenu)}>
          <button type="button" className="sidebar-row-action workspace-toggle" aria-label={label} title={label} aria-expanded={!collapsed} aria-controls={contentsId} onClick={toggle}>
            {collapsed ? <Folder aria-hidden="true" /> : <FolderOpen aria-hidden="true" />}
          </button>
          <div className="pane-select workspace-select project-select" role="button" tabIndex={0} title={`${name} · ${line}`} onClick={toggle} onKeyDown={(e) => onRowKey(e, toggle)}>
            <GroupCopy name={name} folded={threads.length - shown.length} line={line} need={need} twoLine={twoLine} />
            <span className="sidebar-pane-meta">
              <StatusBadge compact status={collapsed ? status : undefined} testid={`workspace-folder-state-${name}`} />
            </span>
          </div>
          <RowMenuButton title={name} open={rows.menuKey === menuKey} onToggle={(anchor) => toggleMenu(anchor)} />
        </div>
      </li>
      {shown.length > 0 && (
        <li className="worktree-children" id={contentsId}>
          <ul className="workspace-list">{shown.map(threadRow)}</ul>
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

/** 사용량 한도 표지(참고 Sidebar.tsx:1691 「Limited」 경고 색, UsageLimitRecoveryBanner 「Resets …」): 다시 차오르기까지 남은 때 */
function UsageLimited({ untilMs, now }: { untilMs: number; now: number }) {
  const left = formatResetIn(new Date(untilMs).toISOString(), now);
  return <span className="usage-limited" data-testid="usage-limited">{left ? `한도 · ${left} 뒤 리셋` : "한도"}</span>;
}

/**
 * 줄 이름 「id · 제목」(herdr-projects thread_display). 일반 thread 줄과 특수 줄(사용자 요청 10-20 「특수스레드도 번호 … 왼쪽에」)이 같은
 * 조각을 써서 번호의 글꼴·색·폭이 같다. 줄이 좁으면 제목 끝이 잘리고 왼쪽 번호는 남는다. 번호가 없으면(시작 전 임무) 제목만.
 */
function ThreadName({ id, title }: { id: string | undefined; title: string }) {
  return (
    <span className="workspace-name">
      {id && (
        <>
          <span className="thread-no" data-testid="thread-no">{id}</span>
          {" · "}
        </>
      )}
      {title}
    </span>
  );
}

function ThreadRow({ t, online, machine, twoLine, ...props }: SidebarProps & { t: ThreadView; online: boolean; machine: MachineView | undefined; twoLine: boolean }) {
  const now = useMinute();
  const pane = t.pane ? props.panes.find((p) => p.id === t.pane) : undefined;
  const paneOpen = pane?.state === "running";
  const selected = props.selected === `thread:${threadKey(t)}` || (pane != null && props.selected === `pane:${pane.id}`);
  const agent = props.agentOf(pane, t);
  // herdr-projects thread_display: 「id · 제목」
  const title = `${t.id} · ${t.title}`;
  const line = stateLine(t, paneOpen, now);
  const tone = stateTone(t, paneOpen ? pane : undefined, now);
  // 사용량 한도(plan §8.267): 상태 말 뒤에 다시 차오르기까지 남은 때
  const limited = tone === "limited" && t.limited_until_ms != null ? <UsageLimited untilMs={t.limited_until_ms} now={now} /> : null;
  // 머신은 트리 단계가 아니라 둘째 줄 오른쪽 글자로(plan §8.195), 끊겼으면 「이름 끊김」
  const machineName = machine?.name ?? t.machine;
  const tooltip = [`${t.project}/${t.id}`, t.title, line, `${t.profile}@${machineName}`, t.branch].filter(Boolean).join("\n");
  // 줄은 그 thread의 agent pane을 연다(herdr-projects: thread는 자기 agent 줄). pane이 없으면 보고서
  const open = () => (paneOpen && pane ? props.onPane(pane.id) : props.onThread(t));
  const rows = useRows();
  const menuKey = `thread:${threadKey(t)}`;
  // 고정은 그 agent pane 열쇠로(예전 에이전트 패널의 고정을 이어받는다, plan §8.280)
  const pinKey = pane ? sidebarPinKey(pane.machine_id, pane.id) : null;
  const pinned = pinKey !== null && rows.pins.includes(pinKey);
  const menuFrom = useRef<HTMLElement | null>(null);
  // 작업 요약 hover 카드(마우스 기기만, plan §8.259): 줄의 title tooltip을 대신하고 ⋯ 메뉴가 열리면 숨는다
  const hover = useSummaryHover(t, rows.menuKey === menuKey);
  // thread 줄은 그 agent pane을 연다: 보고·고정·모든 thread는 메뉴에서
  const toggleMenu = (anchor: HTMLElement, point?: { x: number; y: number }, previousFocus?: HTMLElement | null) => {
    menuFrom.current = previousFocus === undefined ? anchor : previousFocus;
    rows.toggleMenu({
      key: menuKey,
      anchor,
      point,
      previousFocus,
      title,
      place: line,
      items: () => [
        { id: "report", label: "보고", icon: FileText, run: () => props.onThread(t) },
        ...(pinKey === null
          ? []
          : [pinned
            ? { id: "unpin", label: "고정 해제", icon: PinOff, run: () => rows.togglePin(pinKey, menuFrom.current) }
            : { id: "pin", label: "고정", icon: Pin, run: () => rows.togglePin(pinKey, menuFrom.current) }]),
        ...(t.automation ? [{ id: "automation", label: "자동화 규칙", icon: Workflow, run: () => props.onAutomation(t.project, t.automation!) }] : []),
        { id: "threads", label: "모든 thread", icon: ListChecks, run: props.onThreads },
      ],
    });
  };
  // 자동화가 띄운 thread(plan t-0475 A1): 둘째 줄 앞 「자동 · 규칙 제목」. 누르면 줄(pane 열기) 대신 그 규칙 상세. 키보드는 ⋯ 메뉴
  // 「자동화 규칙」으로 간다(줄 안에 단추를 겹쳐 두지 않는다)
  const rule = t.automation_title ?? t.automation;
  const origin = t.automation ? (
    <span
      className="thread-origin"
      data-testid={`thread-origin-${t.project}-${t.id}`}
      title={`자동화 「${rule}」가 띄운 thread · 눌러 규칙 보기`}
      onClick={(e) => {
        e.stopPropagation();
        props.onAutomation(t.project, t.automation!);
      }}
    >
      {/* 줄 앞 공백은 flex 항목에서 사라지므로 떨어지지 않는 공백으로 띄운다 */}
      자동<span className="thread-origin-rule">{"\u00a0· "}{rule}</span>
    </span>
  ) : null;
  return (
    <li className={`workspace workspace-group pane-item${selected ? " is-selected" : ""}${online ? "" : " is-offline"}`} inert={!online} data-thread={threadKey(t)} data-group={t.group} data-origin={t.automation ? "automation" : undefined} data-pin-key={pinKey ?? undefined}>
      <div className="workspace-header" onMouseDownCapture={keepFocusOnMenuPress} onContextMenu={(e) => onRowContextMenu(e, toggleMenu)}>
        <div
          className="pane-select workspace-select"
          role="button"
          tabIndex={0}
          aria-current={selected ? "true" : undefined}
          title={hover.enabled ? undefined : tooltip}
          {...hover.handlers}
          data-testid={`thread-${t.project}-${t.id}`}
          onClick={open}
          onKeyDown={(e) => onRowKey(e, open)}
        >
          <RowMark agent={agent} />
          {twoLine ? (
            <span className="workspace-copy is-two-line">
              <span className="workspace-line">
                <ThreadName id={t.id} title={t.title} />
                {pinned && <Pin className="sidebar-pin-mark" aria-hidden="true" />}
                {online && paneOpen && <AgentCounts pane={pane} />}
              </span>
              <span className="workspace-place thread-place">
                {origin}
                <span className={`thread-state is-${t.group}`} data-tone={tone} data-testid={`thread-state-${t.project}-${t.id}`}>
                  {line}
                  <WorkingDuration startedMs={workingStartedAt(tone, paneOpen ? pane : undefined)} />
                  {limited}
                </span>
                <span className={`thread-machine${online ? "" : " is-offline"}`} data-testid={`thread-machine-${t.project}-${t.id}`}>
                  {online ? machineName : `${machineName} 끊김`}
                </span>
              </span>
            </span>
          ) : (
            <span className="workspace-copy">
              <ThreadName id={t.id} title={t.title} />
              {pinned && <Pin className="sidebar-pin-mark" aria-hidden="true" />}
              {online && paneOpen && <AgentCounts pane={pane} />}
              <span className={`worktree-workspace-label thread-state is-${t.group}`} data-tone={tone} data-testid={`thread-state-${t.project}-${t.id}`}>
                {t.status === "starting" ? "준비 중" : t.cleanup_pending ? "정리 중" : GROUP_WORD[t.group] ?? t.group}
                <WorkingDuration startedMs={workingStartedAt(tone, paneOpen ? pane : undefined)} />
                {limited}
              </span>
            </span>
          )}
          <span className="visually-hidden">{agent ?? "agent"}</span>
          <PaneState pane={paneOpen ? pane : undefined} online={online} testid={`thread-agent-${t.project}-${t.id}`} />
        </div>
        <RowMenuButton title={title} open={rows.menuKey === menuKey} onToggle={(anchor) => toggleMenu(anchor)} />
      </div>
      {hover.card}
    </li>
  );
}
