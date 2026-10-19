// 왼쪽 목록(herdr-web-ui MachineSidebar·Sidebar·AgentSidebar, aa624cb). 모양(class)과 CSS는 참고 것을 그대로 쓴다(herdr/*.css).
// 패널 셋(활동 막대가 하나를 고른다, plan §8.240): 「프로젝트」는 프로젝트 머리(접기, 셈, 묶음 대표 상태) 아래 모든 머신의 thread 줄
// (둘째 줄 왼쪽 상태 말, 오른쪽 머신 이름, t-0352 시안 2안, plan §8.195) → thread가 아닌 pane의 「기타」 묶음(줄에 머신 이름).
// 「머신」은 머신 줄(이름·host·연결 점·+, 끊김·점검 글), 「에이전트」는 모든 머신의 agent 목록. 머신은 트리 단계가 아니다.
// 셋 다 늘 그려 두고 고르지 않은 것은 숨긴다(굴린 자리를 지킨다). 머리·띠·바닥 줄(사용량, App.tsx ListFoot)은 패널과 상관없다.
// aaa에는 herdr workspace가 없다: 참고의 workspace 줄 자리에 pane 줄을, worktree 묶음 자리에 프로젝트 묶음을 둔다.
// thread는 자기 줄 하나로만 보인다(L4, herdr-projects sidebar.rs: thread는 자기 agent 줄이고 이름이 「id · 제목」).
import {
  createContext,
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
import { ChevronDown, ChevronRight, Ellipsis, Eraser, FileText, Folder, FolderOpen, Layers, ListChecks, Monitor, Pencil, Pin, PinOff, Plus, RotateCcw, Terminal, Trash2, TriangleAlert, X } from "lucide-react";
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
import { activityOrder, GROUP_WORD, needsYou, paneAgentStatus, paneEnded, paneProblem, projectLine, roster, shortPath, stateLine, threadKey } from "./sidebarModel";
import "./herdr/primitives.css";
import "./herdr/Sidebar.css";
import "./herdr/AgentSidebar.css";
import "./herdr/Machines.css";
import "./Sidebar.css";
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
  /** 그 프로젝트의 모든 thread(끝난 것까지) 화면 */
  onProject: (name: string) => void;
  /** 목록 위에 띄울 알림 띠(연결 끊김·오류) */
  banners?: ReactNode;
  /** 보이는 패널(활동 막대) */
  panel: SidebarPanel;
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
  // 같은 열쇠의 줄이 머신 목록과 Agents에 함께 있을 수 있어 연 구역도 기억한다
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
        const control = ["row-menu-toggle", "agent-select"].find((c) => from.classList.contains(c)) ?? "pane-select";
        refocus.current = { key, scope: from.closest(".agents-sidebar") ? ".agents-sidebar" : ".machine-list", control };
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
      <div {...panelProps("projects", props.panel)} className="list-body machine-list sidebar-panel">
        <Roster {...props} />
      </div>
      <div {...panelProps("machines", props.panel)} className="sidebar-panel machines-panel">
        <MachineStrip {...props} />
      </div>
      <div {...panelProps("agents", props.panel)} className="sidebar-panel agents-panel">
        <AgentSection {...props} />
      </div>
      {menu && <RowMenu anchor={menu.anchor} point={menu.point} restoreFocusTo={menu.previousFocus} title={menu.title} subtitle={menu.place} items={menu.items()} onClose={closeMenu} />}
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
function MachineStrip(props: SidebarProps) {
  return (
    <section className="machine-strip" aria-label={PANEL_LABEL.machines}>
      {props.machines.map((m) => {
        const online = m.connection === "connected";
        const problems = online && m.problems.length > 0 ? m.problems.map((p) => p.text).join("\n") : null;
        const stats = machineStats(m);
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
              {/* CPU·MEM(plan §8.245): 연결 중일 때만 */}
              {stats && <MachineMeters stats={stats} name={m.name} />}
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
 * 목록 줄들(t-0352 시안 2안, plan §8.195): 프로젝트 묶음(모든 머신의 thread, 둘째 줄에 머신 이름) 다음 thread에 딸리지 않은 pane의
 * 「기타」 묶음(줄마다 머신 이름). 차례는 단축키 「이전·다음 패널」과 같은 계산(sidebarModel `roster`, 사람이 옮긴 pane 차례 포함).
 * 끊긴 머신의 줄은 흐리고 누를 수 없다(참고 machine-offline·inert를 줄마다).
 */
function Roster(props: SidebarProps) {
  const { appearance } = useAppearance();
  const { pins } = useRows();
  const twoLine = appearance.sidebarRows === "two";
  const { paneOrder } = useSidebarShared();
  const { threads, projects, coordinators, panes, machineOf } = roster(props.machines, props.panes, props.threads, paneOrder);
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
        {split.pinned.length > 0 && (
          <ul className="workspace-list workspace-list-pinned sidebar-pinned" aria-label="고정됨">
            {split.pinned.map((p) => paneRow(p))}
          </ul>
        )}
        <ul className="workspace-list">
          {projects.map((project) => (
            <ProjectGroup key={project} {...props} project={project} pThreads={threads.filter((t) => t.project === project)} coordinators={coordinators.filter((p) => p.coordinator_project === project)} machineOf={machineOf} twoLine={twoLine} />
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
      className={`workspace workspace-group pane-item${selected ? " is-selected" : ""}${p.state !== "running" ? " is-ended" : ""}${online ? "" : " is-offline"}${move?.dragging ? " is-dragging" : ""}${move?.lifted ? " is-lifted" : ""}`}
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
              </span>
              {where && <span className="workspace-place">{where}</span>}
            </span>
          ) : (
            <span className="workspace-copy">
              <span className="workspace-name">{title}</span>
              {pinned && <Pin className="sidebar-pin-mark" aria-hidden="true" />}
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

function ProjectGroup({
  project,
  pThreads: threads,
  coordinators,
  machineOf,
  twoLine,
  ...props
}: SidebarProps & { project: string; pThreads: ThreadView[]; coordinators: PaneView[]; machineOf: (t: ThreadView) => MachineView | undefined; twoLine: boolean }) {
  // 프로젝트 하나에 묶음 하나(머신마다 나뉘지 않는다, plan §8.195)
  const [collapsed, toggle] = useFold(`aaa:project-collapsed:${project}`);
  const online = (t: ThreadView) => machineOf(t)?.connection === "connected";
  const contentsId = `${useId()}-threads`;
  const paneOf = (id: string) => props.panes.find((p) => p.id === id);
  // 접힌 묶음도 고른 thread는 보인다(참고: 접힌 묶음은 열린 checkout을 보이고, 셈은 치운 것만)
  const isSel = (t: ThreadView) => props.selected === `thread:${threadKey(t)}` || (t.pane != null && props.selected === `pane:${t.pane}`);
  const shown = collapsed ? threads.filter(isSel) : threads;
  const shownCoordinators = collapsed ? coordinators.filter((p) => props.selected === `pane:${p.id}`) : coordinators;
  const foldedCount = threads.length + coordinators.length - shown.length - shownCoordinators.length;
  // 접힌 머리는 자식 상태까지 대표한다(참고 rollupStatus: blocked > done > working > waiting > idle)
  const status = rollupStatus([
    ...coordinators.map((p) => p.connection === "connected" ? paneAgentStatus(p) : undefined),
    ...threads.map((t) => (online(t) && t.pane ? (paneOf(t.pane) ? paneAgentStatus(paneOf(t.pane)!) : undefined) : undefined)),
  ]);
  const need = threads.some((t) => needsYou(t.group));
  const line = projectLine(
    threads.map((t) => t.group),
    threads.some((t) => t.project_status === "paused"),
  );
  const label = collapsed ? `${project} thread 펼치기` : `${project} thread 접기`;
  const rows = useRows();
  const menuKey = `project:${project}`;
  // 참고 worktree 묶음 메뉴의 접기·펼치기, 그리고 끝난 것까지 보는 프로젝트 화면(herdr-projects popup, plan §8.89)
  const toggleMenu = (anchor: HTMLElement, point?: { x: number; y: number }, previousFocus?: HTMLElement | null) =>
    rows.toggleMenu({
      key: menuKey,
      anchor,
      point,
      previousFocus,
      title: project,
      place: line,
      items: () => [
        { id: "threads", label: "프로젝트 thread 보기", icon: ListChecks, run: () => props.onProject(project) },
        { id: "fold", label: collapsed ? "펼치기" : "접기", icon: collapsed ? ChevronRight : ChevronDown, run: toggle },
      ],
    });
  return (
    <>
      <li className={`workspace workspace-group pane-item project-group${collapsed ? " is-collapsed" : ""}`} data-project={project} data-testid={`project-${project}`}>
        <div className="workspace-header" onMouseDownCapture={keepFocusOnMenuPress} onContextMenu={(e) => onRowContextMenu(e, toggleMenu)}>
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
                <span className={`workspace-place project-place${need ? " is-need" : ""}`}>{line}</span>
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
          <RowMenuButton title={project} open={rows.menuKey === menuKey} onToggle={(anchor) => toggleMenu(anchor)} />
        </div>
      </li>
      {(shown.length > 0 || shownCoordinators.length > 0) && (
        <li className="worktree-children" id={contentsId}>
          <ul className="workspace-list">
            {shownCoordinators.map((p) => (
              <PaneRow key={p.id} {...props} pane={p} machine={props.machines.find((m) => m.id === p.machine_id)} online={p.connection === "connected"} twoLine={twoLine} />
            ))}
            {shown.map((t) => (
              <ThreadRow key={threadKey(t)} {...props} online={online(t)} machine={machineOf(t)} t={t} twoLine={twoLine} />
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

function ThreadRow({ t, online, machine, twoLine, ...props }: SidebarProps & { t: ThreadView; online: boolean; machine: MachineView | undefined; twoLine: boolean }) {
  const now = useMinute();
  const pane = t.pane ? props.panes.find((p) => p.id === t.pane) : undefined;
  const paneOpen = pane?.state === "running";
  const selected = props.selected === `thread:${threadKey(t)}` || (pane != null && props.selected === `pane:${pane.id}`);
  const agent = props.agentOf(pane, t);
  // herdr-projects thread_display: 「id · 제목」
  const title = `${t.id} · ${t.title}`;
  const line = stateLine(t, paneOpen, now);
  // 머신은 트리 단계가 아니라 둘째 줄 오른쪽 글자로(plan §8.195), 끊겼으면 「이름 끊김」
  const machineName = machine?.name ?? t.machine;
  const tooltip = [`${t.project}/${t.id}`, t.title, line, `${t.profile}@${machineName}`, t.branch].filter(Boolean).join("\n");
  // 줄은 그 thread의 agent pane을 연다(herdr-projects: thread는 자기 agent 줄). pane이 없으면 보고서
  const open = () => (paneOpen && pane ? props.onPane(pane.id) : props.onThread(t));
  const rows = useRows();
  const menuKey = `thread:${threadKey(t)}`;
  // thread 줄은 그 agent pane을 연다: 보고와 그 프로젝트의 모든 thread는 메뉴에서
  const toggleMenu = (anchor: HTMLElement, point?: { x: number; y: number }, previousFocus?: HTMLElement | null) =>
    rows.toggleMenu({
      key: menuKey,
      anchor,
      point,
      previousFocus,
      title,
      place: line,
      items: () => [
        { id: "report", label: "보고", icon: FileText, run: () => props.onThread(t) },
        { id: "threads", label: "프로젝트 thread 보기", icon: ListChecks, run: () => props.onProject(t.project) },
      ],
    });
  return (
    <li className={`workspace workspace-group pane-item${selected ? " is-selected" : ""}${online ? "" : " is-offline"}`} inert={!online} data-thread={threadKey(t)} data-group={t.group}>
      <div className="workspace-header" onMouseDownCapture={keepFocusOnMenuPress} onContextMenu={(e) => onRowContextMenu(e, toggleMenu)}>
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
              <span className="workspace-place thread-place">
                <span className={`thread-state is-${t.group}`} data-testid={`thread-state-${t.project}-${t.id}`}>
                  {line}
                </span>
                <span className={`thread-machine${online ? "" : " is-offline"}`} data-testid={`thread-machine-${t.project}-${t.id}`}>
                  {online ? machineName : `${machineName} 끊김`}
                </span>
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
        <RowMenuButton title={title} open={rows.menuKey === menuKey} onToggle={(anchor) => toggleMenu(anchor)} />
      </div>
    </li>
  );
}

/**
 * 에이전트 패널: 모든 머신의 살아 있는 agent를 한 목록으로(참고 AgentSidebar). 참고는 머신 목록 아래 접는 구역이고(휴대폰은 접힌 채
 * 시작) aaa는 활동 막대의 패널 하나라 접기 단추가 없다(plan §8.240).
 * agent는 harness hook이 상태를 알리는 pane이나 agent 명령으로 뜬 pane이다(셸은 상태를 알려도 넣지 않는다).
 */
function AgentSection(props: SidebarProps) {
  const shared = useRows();
  const { appearance } = useAppearance();
  const threadOf = new Map<string, ThreadView>();
  for (const t of props.threads) if (t.pane) threadOf.set(t.pane, t);
  // 설정 「에이전트 정렬」: 머신 목록 차례, 또는 머신마다 활동순(참고 AgentSidebar `byActivity`, 각 PC 안에서)
  const byActivity = appearance.agentOrder === "activity";
  const rows = props.machines.flatMap((m) => {
    const agents = props.panes
      .filter((p) => p.machine_id === m.id && p.state === "running" && p.role !== "evaluator")
      .flatMap((p) => {
        const t = threadOf.get(p.id);
        const agent = props.agentOf(p, t);
        return agent !== null ? [{ m, p, t, agent }] : [];
      });
    return byActivity ? activityOrder(agents, (r) => r.p) : agents;
  });
  // 고정한 agent는 맨 앞 카드에(참고 AgentSidebar `splitPinnedRows`)
  const split = splitPinnedRows(rows, shared.pins, (r) => sidebarPinKey(r.m.id, r.p.id));
  // 참고 agentContext: 이름이 아닌 말(종류)·머신(둘 이상일 때만)·자리. 같은 말을 두 번 하지 않는다
  const several = props.machines.length > 1;
  const agentItem = ({ m, p, t, agent }: (typeof rows)[number]) => {
    const online = m.connection === "connected";
    const selected = props.selected === `pane:${p.id}` || (t != null && props.selected === `thread:${threadKey(t)}`);
    const title = t ? `${t.id} · ${t.title}` : p.name || p.argv.join(" ") || p.id.slice(0, 8);
    const kind = p.agent?.harness ?? agent;
    const place = t ? `${t.project}` : p.cwd ? shortPath(p.cwd) : null;
    const context = [kind && kind.toLowerCase() !== agent.toLowerCase() && kind !== title ? kind : null, several ? m.name : null, place].filter(Boolean).join(" · ");
    const tooltip = [...new Set([p.id, title, context, p.cwd ? shortPath(p.cwd) : null, online ? null : "끊김"].filter(Boolean))].join("\n");
    const pinKey = sidebarPinKey(m.id, p.id);
    const pinned = shared.pins.includes(pinKey);
    const menuKey = `agent:${pinKey}`;
    let from: HTMLElement | null = null;
    // 참고 AgentSidebar 메뉴는 고정·고정 해제 하나다
    const toggleMenu = (anchor: HTMLElement, point?: { x: number; y: number }, previousFocus?: HTMLElement | null) => {
      from = previousFocus === undefined ? anchor : previousFocus;
      shared.toggleMenu({
        key: menuKey,
        anchor,
        point,
        previousFocus,
        title,
        place: context,
        items: () => [
          pinned
            ? { id: "unpin", label: "에이전트 고정 해제", icon: PinOff, run: () => shared.togglePin(pinKey, from) }
            : { id: "pin", label: "에이전트 고정", icon: Pin, run: () => shared.togglePin(pinKey, from) },
        ],
      });
    };
    return (
      <li
        key={p.id}
        className={`agent-item${selected ? " is-selected" : ""}${online ? "" : " is-offline"}`}
        data-machine={m.id}
        data-pane={p.id}
        data-pin-key={pinKey}
        onMouseDownCapture={keepFocusOnMenuPress}
        onContextMenu={(e) => onRowContextMenu(e, toggleMenu)}
      >
        <button type="button" className="agent-select agent-row" disabled={!online} aria-current={selected ? "true" : undefined} title={tooltip} data-testid={`agent-row-${p.name || p.id}`} onClick={() => props.onPane(p.id)}>
          <span className="sidebar-mark" aria-hidden="true">
            <AgentMark agent={agent} size={18} />
          </span>
          <span className="agent-copy">
            <span className="visually-hidden">{agent}: </span>
            <span className="agent-title-line">
              <span className="agent-title">{title}</span>
              {pinned && <Pin className="sidebar-pin-mark" aria-hidden="true" />}
            </span>
            {context && <span className="agent-context">{context}</span>}
          </span>
          {online && (
            <span className="agent-row-status">
              <BackgroundBadge count={p.agent?.background} />
              <PlanBadge plan={p.agent?.plan} />
            </span>
          )}
          <span className="agent-row-state">
            {online && paneProblem(p) ? <ProblemBadge reason={paneProblem(p)!} /> : <StatusBadge compact status={online ? paneAgentStatus(p) : undefined} />}
          </span>
        </button>
        <div className="agent-actions">
          <button type="button" className="sidebar-row-action row-menu-toggle" aria-label={`${title} 더보기`} aria-haspopup="menu" aria-expanded={shared.menuKey === menuKey} onClick={(e) => toggleMenu(e.currentTarget)}>
            <Ellipsis aria-hidden="true" />
          </button>
        </div>
      </li>
    );
  };
  return (
    <section className={`agents-sidebar${rows.length === 0 ? " is-empty" : ""}`} aria-label={PANEL_LABEL.agents} data-testid="agents">
      <div className="agent-list-contents">
        {rows.length === 0 ? (
          <p className="agent-empty" role="status">
            도는 agent 없음
          </p>
        ) : (
          <>
            {split.pinned.length > 0 && (
              <ul className="agent-list agent-list-pinned sidebar-pinned" aria-label="고정됨">
                {split.pinned.map(agentItem)}
              </ul>
            )}
            {split.rest.length > 0 && <ul className="agent-list">{split.rest.map(agentItem)}</ul>}
          </>
        )}
      </div>
    </section>
  );
}
