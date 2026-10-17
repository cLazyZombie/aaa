// 왼쪽 목록(herdr-web-ui MachineSidebar·Sidebar·AgentSidebar, aa624cb). 모양(class)과 CSS는 참고 것을 그대로 쓴다(herdr/*.css).
// 구성(차이 목록 L1): 머신 묶음마다 머리(이름·host·연결 점·접기·+) → 프로젝트 머리(접기, 묶음 대표 상태) 아래 그 머신의 thread 줄 →
// thread가 아닌 pane 줄. 그 아래 모든 머신의 「Agents」 구역, 맨 아래 바닥 줄(설정·사용량, App.tsx ListFoot).
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
  type KeyboardEvent,
  type MouseEvent,
  type ReactNode,
} from "react";
import { ChevronDown, ChevronRight, Ellipsis, Eraser, FileText, Folder, FolderOpen, ListChecks, Monitor, Pencil, Pin, PinOff, Plus, RotateCcw, Terminal, Trash2, TriangleAlert, X } from "lucide-react";
import type { FrontRequest, MachineView, PaneView, ThreadView } from "./gen/proto";
import { ApiError, call } from "./api";
import { ConfirmDialog } from "./ConfirmDialog";
import { PaneRename } from "./PaneRename";
import { RowMenu, type RowMenuItem } from "./RowMenu";
import { currentSidebarPins, setSidebarPins, sidebarPinKey, splitPinnedRows, subscribeSidebarPins, toggleSidebarPin } from "./herdr/sidebarPins.ts";
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
}

export function Sidebar(props: SidebarProps) {
  const { machines } = props;
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
      <div className="list-body machine-list" aria-label="머신과 pane">
        {props.banners}
        {machines.map((m) => (
          <MachineGroup key={m.id} {...props} machine={m} />
        ))}
      </div>
      <AgentSection {...props} />
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

/** 그 줄의 일이 실패했을 때 줄 아래 한 줄(참고 `sidebar-inline-error`) */
function RowError({ rowKey }: { rowKey: string }) {
  const { error } = useRows();
  return error?.key === rowKey ? (
    <p className="sidebar-inline-error" role="alert">
      {error.message}
    </p>
  ) : null;
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
  const { pins } = useRows();
  const twoLine = appearance.sidebarRows === "two";
  // 고정한 pane은 맨 앞 카드에 고정한 순서로(참고 Sidebar.tsx `splitPinnedGroups`·`sidebar-pinned`)
  const split = splitPinnedRows(panes, pins, (p) => sidebarPinKey(m.id, p.id));
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
        {split.pinned.length > 0 && (
          <ul className="workspace-list workspace-list-pinned sidebar-pinned" aria-label="고정됨">
            {split.pinned.map((p) => (
              <PaneRow key={p.id} {...props} online={online} pane={p} twoLine={twoLine} />
            ))}
          </ul>
        )}
        <ul className="workspace-list">
          {projects.map((project) => (
            <ProjectGroup key={project} {...props} machine={m} online={online} project={project} pThreads={threads.filter((t) => t.project === project)} twoLine={twoLine} />
          ))}
          {split.rest.map((p) => (
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
  const rows = useRows();
  const pinKey = sidebarPinKey(p.machine_id, p.id);
  const pinned = rows.pins.includes(pinKey);
  const menuKey = `pane:${p.id}`;
  // 참고 workspace 메뉴(Sidebar.tsx `menuItems`) 차례: 고정 → 이름 바꾸기 → 닫기. 이름은 pane 이름(참고 PaneCanvas `itemsFor`의
  // 「Rename pane」·붙인 이름이 있을 때 「Clear pane name」). 끝난 pane은 닫는 대신 다시 열기·기록 지우기(aaa PaneResume·PaneForget)
  const items = (): RowMenuItem[] => [
    pinned
      ? { id: "unpin", label: "pane 고정 해제", icon: PinOff, run: () => rows.togglePin(pinKey, menuFrom.current) }
      : { id: "pin", label: "pane 고정", icon: Pin, run: () => rows.togglePin(pinKey, menuFrom.current) },
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
    <li className={`workspace workspace-group pane-item${selected ? " is-selected" : ""}${p.state !== "running" ? " is-ended" : ""}`} data-pane={p.id} data-pin-key={pinKey}>
      <div className="workspace-header" onMouseDownCapture={keepFocusOnMenuPress} onContextMenu={(e) => onRowContextMenu(e, toggleMenu)}>
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
                {pinned && <Pin className="sidebar-pin-mark" aria-hidden="true" />}
              </span>
              {where && <span className="workspace-place">{where}</span>}
            </span>
          ) : (
            <span className="workspace-copy">
              <span className="workspace-name">{title}</span>
              {pinned && <Pin className="sidebar-pin-mark" aria-hidden="true" />}
              {ended && <span className="worktree-workspace-label">{ended}</span>}
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
  const rows = useRows();
  const menuKey = `project:${m.id}:${project}`;
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
      <li className={`workspace workspace-group pane-item project-group${collapsed ? " is-collapsed" : ""}`} data-project={project} data-testid={`project-${m.name}-${project}`}>
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
          <RowMenuButton title={project} open={rows.menuKey === menuKey} onToggle={(anchor) => toggleMenu(anchor)} />
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
    <li className={`workspace workspace-group pane-item${selected ? " is-selected" : ""}`} data-thread={threadKey(t)} data-group={t.group}>
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
        <RowMenuButton title={title} open={rows.menuKey === menuKey} onToggle={(anchor) => toggleMenu(anchor)} />
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
  const shared = useRows();
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
