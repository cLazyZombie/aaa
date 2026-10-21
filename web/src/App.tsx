import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import type {
  AgentActivity,
  DeviceRole,
  LibraryReport,
  MachineView,
  MenuPick,
  NoticeView,
  PaneView,
  QuestionMenu,
  QuestionView,
  ThreadView,
} from "./gen/proto";
import { ActivityBar, type CoordinatorButton } from "./ActivityBar";
import { SidebarResizer } from "./SidebarResizer";
import { PANEL_LABEL, revealPanel, storedCollapsed, storedPanel, storeSidebar, type SidebarPanel } from "./sidebarPanel";
import { ReportsView, type ReportRef } from "./ReportsView";
import { TasksView, useSeenMine, useTaskBoard, type TaskAddRequest, type TaskFocusRequest } from "./TasksView";
import { composerDrafts } from "./herdr/composerDraft.ts";
import { readTerminalDraft, writeTerminalDraft } from "./herdr/terminalDraft.ts";
import { reportDoc, reportDocLink, taskBadge, uploadsBase, type TaskAttachment } from "./tasksModel";
import { UpdateNotice } from "./UpdateNotice";
import { ApiError, call, me, onSignedOut, pushedSince, pushInfo, signOut, tellAlert, watchEvents } from "./api";
import { removePushSubscription } from "./push";
import { AccessGate } from "./AccessGate";
import { takePairCode } from "./herdr/phone";
import { usePreferencesSync } from "./preferencesSync";
import { TermView } from "./Term";
import { QuestionReload } from "./questionReload";
import { ThreadReload } from "./threadReload";
import { SummaryButton } from "./ThreadSummary";
import { ThreadPanel } from "./ThreadPanel";
import { PromptCard } from "./PromptCard";
import { ProjectThreads } from "./ProjectThreads";
import { ListChecks, Menu, Settings } from "lucide-react";
import { Icon } from "./icons";
import { hasAgentMark } from "./herdr/AgentMark.tsx";
import { Sidebar } from "./Sidebar";
import type { RolloutSnapshot } from "./RolloutCard";
import { needsYou, paneAgentStatus, rosterRows, shortPath, threadKey } from "./sidebarModel";
import { useShortcuts } from "./herdr/shortcuts.ts";
import type { AppActions, PaneControls } from "./actions";
import { UsageMeters } from "./UsageMeters";
import { SettingsDialog } from "./SettingsDialog";
import { onSettingsHistory, recordSettings } from "./herdr/settingsHistory.ts";
import { currentSidebarShared, updateSidebarShared, useKeepScreenOn, useSidebarShared } from "./settings";
import { useScreenWakeLock } from "./herdr/wakeLock.ts";
import { justWoke, raiseAlert, unableSince, useAlerts, useAlertSoundUnlock } from "./alerts";
import { Droplet } from "./herdr/Droplet.tsx";
import { showDroplet } from "./herdr/droplet";
import { useFileViewer } from "./herdr/useFileViewer.ts";
import { FileViewer } from "./FileViewer.tsx";
import { NewSessionDialog } from "./NewSessionDialog";
import { CommandPalette, type PaletteActions } from "./CommandPalette";
import { currentAppearance, updateAppearance, useAppearance } from "./theme";
import { passingFolder, recentDirectories } from "./herdr/recent-directories.ts";
import { useMediaQuery } from "./herdr/useMediaQuery.ts";
import { inAppLink } from "./reportLinks.ts";
import type { AppRef } from "./appRef";
import type { AppId } from "./appIds";
import { AppRefChip, AppRefContext, type AppRefLook, type AppRefs } from "./AppRefChip";
import { AsksPanel, AsksView, useAskSelection, useAsksData } from "./AsksView";
import { lastAnswer, openAsks, questionKey } from "./asksModel";
import { watchDrawerSwipe } from "./herdr/edgeSwipe.ts";
import { DrawerContext, DrawerToggle, HeadContext, HeadMore, HeadNote } from "./drawer";
import { highMachines, HIGH_STAT } from "./machineStats";
import { automationAttention } from "./automationModel";
import { ErrorBoundary } from "./ErrorBoundary";
import { setAppBadge } from "./appBadge";
import { watchBadgeFocus } from "./faviconBadge";
import { PasskeyNotice, usePasskeyState } from "./PasskeyPanel";

type Auth = { state: "loading" } | { state: "pair" } | { state: "in"; role: DeviceRole; device: string } | { state: "error"; msg: string };

export function App() {
  const [auth, setAuth] = useState<Auth>({ state: "loading" });
  // QR 코드가 연 주소의 `?pair=CODE`: 읽자마자 주소에서 지운다(기록·referrer에 남지 않게, herdr-web-ui takePairCode)
  const [pairCode] = useState(() => takePairCode());
  const check = useCallback(() => {
    me()
      .then((m) => setAuth(m ? { state: "in", role: m.role, device: m.device } : { state: "pair" }))
      // fetch가 던진 TypeError는 네트워크에 닿지 못한 것이다(브라우저마다 말이 다르다: Failed to fetch·Load failed).
      // host의 답(안전 모드의 unavailable 등)은 그 말을 그대로 보인다
      .catch((e: unknown) =>
        setAuth({ state: "error", msg: e instanceof TypeError ? "네트워크에 닿지 않는다." : e instanceof ApiError ? e.message : String(e) }),
      );
  }, []);
  useEffect(check, [check]);
  // 취소·로그아웃(401, WebSocket 4401): host에 다시 물어 gate를 보인다
  useEffect(() => onSignedOut(check), [check]);
  // host에 닿지 못했다(끊긴 채 캐시한 셸로 연 홈 화면 앱, plan §8.115): 네트워크가 돌아오거나 다시 보이면 다시 묻는다.
  // 참고는 셸이 그대로 뜬 채 주기로 다시 묻는다. aaa는 첫 물음이 기기를 고르므로 그 사건에서 다시 묻는다
  const failed = auth.state === "error";
  useEffect(() => {
    if (!failed) return;
    const again = () => document.visibilityState === "visible" && check();
    window.addEventListener("online", again);
    document.addEventListener("visibilitychange", again);
    return () => {
      window.removeEventListener("online", again);
      document.removeEventListener("visibilitychange", again);
    };
  }, [failed, check]);
  if (auth.state === "loading") return <p className="center muted">불러오는 중</p>;
  if (auth.state === "error") return <Unreachable reason={auth.msg} onRetry={check} />;
  if (auth.state === "pair") return <AccessGate initialCode={pairCode} onUnlocked={check} />;
  return <Main role={auth.role} device={auth.device} onSignedOut={check} />;
}

/** host에 닿지 못한 첫 화면(모양은 연결 화면 카드, 문구는 offline.html과 같다). 네트워크가 돌아오면 App이 다시 묻는다 */
function Unreachable({ reason, onRetry }: { reason: string; onRetry: () => void }) {
  return (
    <main className="access-gate-screen">
      <div className="access-gate" data-testid="unreachable">
        <img src="/icon.svg" alt="" width="44" height="44" className="access-gate-mark" />
        <h1 className="access-gate-title">host에 연결할 수 없다</h1>
        <p className="access-gate-copy">네트워크(tailnet) 연결과 host가 떠 있는지 확인한다. 연결이 돌아오면 다시 붙는다.</p>
        <button type="button" className="access-gate-submit" onClick={onRetry}>
          다시 시도
        </button>
        <p className="access-gate-error" role="status">
          {reason}
        </p>
      </div>
    </main>
  );
}

const ACTIVITY_LABEL: Record<AgentActivity, string> = {
  started: "시작",
  working: "작업 중",
  idle: "입력 대기",
  question: "질문",
  error: "오류",
  exited: "끝남",
};

/** harness 상태(hook 사건으로만 바뀐다). 진행률을 보고했으면 함께 보인다. 색과 글을 함께 쓴다. */
export function AgentChip({ p, testid }: { p: PaneView; testid?: boolean }) {
  const a = p.agent;
  if (!a || p.state !== "running") return null;
  // 대화 정리 중(plan §8.309)도 작업 중이다: 같은 색·숨쉬는 점에 말만 「정리 중」
  const compacting = a.activity === "working" && a.compacting;
  const title = [a.harness, compacting ? "대화 정리 중: 끝나면 보낸 글을 읽습니다" : null, a.progress, a.message].filter(Boolean).join(" · ");
  const quiet = a.activity === "idle" || a.activity === "started" || a.activity === "exited";
  return (
    <span
      className={`chip agent ${a.activity} ${quiet ? "plain" : ""}`}
      title={title}
      data-testid={testid ? `agent-${p.name || p.id}` : "agent-chip"}
      data-compacting={compacting || undefined}
    >
      {a.activity === "working" && <span className="dot working" aria-hidden />}
      {compacting ? "정리 중" : ACTIVITY_LABEL[a.activity]}
      {a.percent != null && <span className="pct"> {a.percent}%</span>}
    </span>
  );
}

const KIND_LABEL: Record<string, string> = { trust: "폴더를 믿을까?", permission: "실행해도 될까?", confirm: "확인" };
const CHOICE_LABEL: Record<string, string> = { yes: "예", no: "아니오" };

/** 「Bash {"command":"…"}」 같은 권한 질문을 도구 이름과 사람이 읽을 본문으로 나눈다. JSON이 잘렸으면 글 그대로 둔다. */
export function splitQuestion(text: string): { tool: string | null; body: string } {
  const m = /^([A-Za-z][\w.:-]*)\s+(\{[\s\S]*)$/.exec(text.trim());
  if (!m) return { tool: null, body: text };
  try {
    const v = JSON.parse(m[2]) as Record<string, unknown>;
    for (const k of ["command", "file_path", "path", "url", "pattern", "query", "prompt"]) {
      if (typeof v[k] === "string") return { tool: m[1], body: v[k] as string };
    }
    return { tool: m[1], body: JSON.stringify(v, null, 2) };
  } catch {
    return { tool: m[1], body: m[2] };
  }
}

/**
 * 확인창 질문 카드. 답은 host가 질문이 아직 열려 있는지 다시 확인한 뒤 harness 키로 보낸다.
 * 목록에서는 작게(예·아니오 버튼), 터미널 아래에서는 선택지를 한 줄씩 번호와 함께 보인다.
 * 답이 가는 동안과 실패는 카드 안에 보인다(herdr-web-ui PromptCard.tsx: `aria-busy`, 선택지 `disabled={pending}`,
 * 카드 안 `role="alert"`). 휴대폰은 pane을 보는 동안 목록(머리 띠)이 숨으므로 오류가 카드 밖이면 아무도 못 본다.
 * 카드는 질문 하나에 하나(`key`)라, 카드가 사라진 뒤 온 답은 아무것도 바꾸지 않는다.
 */
function QuestionCard({
  q,
  where,
  canControl,
  onPick,
  docked,
}: {
  q: QuestionView;
  where: string;
  canControl: boolean;
  onPick: (q: QuestionView, c: string) => Promise<void>;
  docked?: boolean;
}) {
  const { tool, body } = splitQuestion(q.text);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const shown = useRef(false);
  useEffect(() => {
    shown.current = true;
    return () => {
      shown.current = false;
    };
  }, []);
  const pick = async (choice: string) => {
    setPending(true);
    setError(null);
    try {
      await onPick(q, choice);
    } catch (e) {
      if (shown.current) setError(e instanceof ApiError ? e.message : String(e));
    }
    if (shown.current) setPending(false);
  };
  return (
    <div className="question" aria-busy={pending} data-testid={docked ? `dock-question-${q.id}` : `question-${q.id}`}>
      <div className="q-head">
        <span className="q-title">{KIND_LABEL[q.question_kind] ?? q.question_kind}</span>
        <span className="q-where">
          {where} · {q.machine}
        </span>
      </div>
      <div className="q-cmd">
        {tool && <span className="tool">{tool}</span>}
        {body}
      </div>
      {canControl ? (
        <div className="q-actions">
          {q.options.map((o, n) => (
            <button key={o} className={o === "yes" ? "primary" : ""} disabled={pending} onClick={() => void pick(o)}>
              {docked && <span className="key">{n + 1}</span>}
              {CHOICE_LABEL[o] ?? o}
            </button>
          ))}
        </div>
      ) : (
        <p className="muted small-text">보기 권한이라 답할 수 없다</p>
      )}
      {error !== null && (
        <p className="q-error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}

/** 마지막으로 연 pane·thread(herdr-web-ui App.tsx 97~120줄 `SELECTION_KEY`): 이 창 것(sessionStorage, 새로 고쳐도 남는다),
 * 없으면 아무 창이나 마지막으로 연 것(localStorage, 새로 연 창) */
const SELECTION_KEY = "aaa:selection";

function storedSelection(): string | null {
  for (const storage of ["sessionStorage", "localStorage"] as const) {
    try {
      const value = window[storage].getItem(SELECTION_KEY);
      if (value !== null && /^(pane|thread):./.test(value)) return value;
    } catch {
      /* 사생활 보호 창 */
    }
  }
  return null;
}

function storeSelection(sel: string): void {
  for (const storage of ["sessionStorage", "localStorage"] as const) {
    try {
      window[storage].setItem(SELECTION_KEY, sel);
    } catch {
      /* 사생활 보호 창 */
    }
  }
}

/**
 * 고른 것: `#pane=<id>`, `#thread=<id>`, 모든 thread `#threads`, 보고서 화면 `#reports`(목록)·`#report=<project>/<name>`(하나,
 * plan §8.240. 프로젝트 없는 `#report=<이름>`은 보고서 목록을 받은 뒤 채운다), 할 일 화면 `#tasks`(plan §8.306), 「답할 것」 `#asks`·
 * `#ask=<열쇠>`(asksModel, plan §8.345), thread 질문 `#question=<thread>`(「답할 것」의 그 질문 카드, 답했으면 그때 기록, plan §8.329). host는
 * 프로젝트 하나만 맡아(plan §8.291) 주소에 프로젝트가 없다. 옛 주소(이미 보낸 알림·저장한 링크)도 연다:
 * `#thread=<project>/<id>`는 그대로 받고, 프로젝트 고르기 `#projects`·프로젝트 thread 보기 `#project=<name>`은 `#threads`로 바꾼다.
 * 짧은 thread 주소는 thread 목록을 받은 뒤 내부 열쇠 `thread:<project>/<id>`로 바뀐다(App의 `sel` 정리).
 */
function selectedFromHash(): string | null {
  if (location.hash === "#projects" || location.hash.startsWith("#project=")) history.replaceState(history.state, "", "#threads");
  if (location.hash === "#threads") return "threads:";
  if (location.hash === "#reports") return "reports:";
  if (location.hash === "#tasks") return "tasks:";
  if (location.hash === "#asks") return "asks:";
  const r = /^#report=(.+)$/.exec(location.hash);
  if (r) return `reports:${decodeURIComponent(r[1])}`;
  const m = /^#(pane|thread|task|question|ask)=(.+)$/.exec(location.hash);
  return m ? `${m[1]}:${decodeURIComponent(m[2])}` : null;
}

/** 고른 것의 주소(selectedFromHash의 반대). thread는 프로젝트 없이 `thread=<id>` */
function hashOf(sel: string | null): string {
  if (sel === null) return "";
  if (sel === "threads:") return "threads";
  if (sel === "reports:") return "reports";
  if (sel === "tasks:") return "tasks";
  if (sel === "asks:") return "asks";
  if (sel.startsWith("reports:")) return `report=${encodeURIComponent(sel.slice(8))}`;
  if (sel.startsWith("thread:")) {
    const key = sel.slice("thread:".length);
    return `thread=${encodeURIComponent(key.slice(key.indexOf("/") + 1))}`;
  }
  const at = sel.indexOf(":");
  return `${sel.slice(0, at)}=${encodeURIComponent(sel.slice(at + 1))}`;
}

/** 보고서 화면의 연 보고서(`reports:<project>/<name>`): 프로젝트 이름에는 /가 없다 */
function reportOf(sel: string): ReportRef | null {
  const rest = sel.slice(8);
  const at = rest.indexOf("/");
  return at > 0 ? { project: rest.slice(0, at), name: rest.slice(at + 1) } : null;
}

/**
 * pane에서 도는 coding agent의 이름(herdr의 `pane.agent`처럼 표시 이름): hook이 알린 harness, thread profile, 명령 이름 순으로
 * 본다. 표시가 있는 이름(herdr/AgentMark `hasAgentMark`)이거나 claude·codex가 든 이름(`claude-opus` profile, 경로 붙은 명령)이면
 * 그 이름이다. 그 밖의 agent는 그 harness 이름(FallbackMark 머리글자), agent가 아니면 null(셸, 터미널 표시).
 */
function agentOf(p: PaneView | undefined, t?: ThreadView): string | null {
  for (const raw of [p?.agent?.harness, t?.profile, p?.argv[0]]) {
    const name = (raw ?? "").toLowerCase().split("/").pop() ?? "";
    if (!name) continue;
    if (hasAgentMark(name)) return name;
    if (name.includes("claude")) return "claude";
    if (name.includes("codex")) return "codex";
  }
  if (p?.agent || t) return p?.agent?.harness || t?.profile || "agent";
  return null;
}

function Main({ role, device, onSignedOut: signedOut }: { role: DeviceRole; device: string; onSignedOut: () => void }) {
  const [machines, setMachines] = useState<MachineView[]>([]);
  const [rollout, setRollout] = useState<RolloutSnapshot>(null);
  const [panes, setPanes] = useState<PaneView[]>([]);
  const [up, setUp] = useState(false);
  // host가 주는 웹 빌드(events 연결마다 온다): 이 페이지와 다르면 다시 열기 줄(plan §8.166)
  const [hostWeb, setHostWeb] = useState<string | null>(null);
  // 짝지은 뒤 10분 안 passkey 등록 안내(관리 GUI W7)
  const [passkey, reloadPasskey] = usePasskeyState();
  const [loaded, setLoaded] = useState(false);
  const [sel, setSel] = useState<string | null>(selectedFromHash());
  const [msg, setMsg] = useState<string | null>(null);
  // 768px 이하에서 목록은 pane 위의 drawer다(herdr-web-ui App.tsx 290~316줄, drawer.tsx). 769px부터는 늘 옆 칸이라
  // 좁은 창이 열어 둔 drawer가 다음에 좁아질 때 다시 나오지 않게 닫는다
  const [drawerOpen, setDrawerOpen] = useState(false);
  const drawerOpenRef = useRef(drawerOpen);
  drawerOpenRef.current = drawerOpen;
  const wideScreen = useMediaQuery("(min-width: 769px)");
  useEffect(() => {
    if (wideScreen) setDrawerOpen(false);
  }, [wideScreen]);
  // 왼쪽 가장자리에서 쓸면 열고 왼쪽으로 쓸면 닫는다(herdr/edgeSwipe.ts)
  useEffect(() => watchDrawerSwipe(() => drawerOpenRef.current, setDrawerOpen), []);
  // Esc는 열린 drawer를 닫고 더 가지 않는다: capture 단계라 focus를 쥔 터미널은 받지 않는다. 위에 뜬 창(설정·확인창·
  // sheet)과 IME 조합은 제 Esc를 쓴다. 참고는 `[role="dialog"]`만 보지만 aaa 확인창은 alertdialog라 aria-modal로 본다.
  // aaa는 「더 보기」 메뉴가 drawer 안(목록 머리)에 있어 열린 메뉴도 제 Esc를 쓴다: 한 번은 메뉴, 다음은 drawer
  useEffect(() => {
    const onEscape = (event: KeyboardEvent): void => {
      if (event.key !== "Escape" || !drawerOpenRef.current || event.isComposing || event.keyCode === 229) return;
      if (document.querySelector("[aria-modal='true'], dialog[open], [role='menu']")) return;
      event.preventDefault();
      event.stopPropagation();
      // drawer 안에 있던 focus는 숨는 drawer와 함께 사라지므로 연 단추로 돌린다
      const inside = document.getElementById("workspace-drawer")?.contains(document.activeElement) === true;
      setDrawerOpen(false);
      if (inside) document.querySelector<HTMLElement>(".view .drawer-toggle")?.focus({ preventScroll: true });
    };
    window.addEventListener("keydown", onEscape, true);
    return () => window.removeEventListener("keydown", onEscape, true);
  }, []);
  const drawer = useMemo(() => ({ open: drawerOpen, toggle: () => setDrawerOpen((open) => !open) }), [drawerOpen]);
  // 활동 막대(plan §8.240): 사이드바가 보이는 패널 하나, 넓은 창에서 접었나. 둘 다 기기마다 기억한다(sidebarPanel.ts).
  // 접힘은 769px부터만 뜻이 있다(좁은 창의 사이드바는 drawer이고 같은 단추가 drawer를 닫는다)
  const [panel, setPanel] = useState<SidebarPanel>(storedPanel);
  const [collapsed, setCollapsed] = useState(storedCollapsed);
  // 자동화 패널에 열어 달라고 한 규칙 상세(thread 줄 「자동 · 규칙」 표지). 패널이 받으면 지운다
  const [ruleAsk, setRuleAsk] = useState<{ project: string; name: string } | null>(null);
  useEffect(() => storeSidebar(panel, collapsed), [panel, collapsed]);
  const panelRef = useRef(panel);
  panelRef.current = panel;
  const collapsedRef = useRef(collapsed);
  collapsedRef.current = collapsed;
  /** 사이드바를 접는다·편다. 사이드바 안의 focus는 숨는 칸과 함께 사라지므로 그 패널의 막대 단추로 옮긴다 */
  const setSidebarCollapsed = (next: boolean) => {
    if (next && document.getElementById("sidebar-column")?.contains(document.activeElement))
      document.getElementById(`activity-tab-${panelRef.current}`)?.focus({ preventScroll: true });
    setCollapsed(next);
  };
  const sidebarToggleRef = useRef(() => {});
  sidebarToggleRef.current = () => setSidebarCollapsed(!collapsedRef.current);
  /** 막대의 패널 단추(VS Code activity bar): 다른 패널이면 그것을 보이고, 보이는 패널을 다시 누르면 접는다(좁은 창은 drawer를 닫는다) */
  const pickPanel = (next: SidebarPanel) => {
    if (!wideScreen) {
      if (next === panelRef.current) setDrawerOpen(false);
      else setPanel(next);
      return;
    }
    if (collapsed) {
      setPanel(next);
      setCollapsed(false);
    } else if (next === panelRef.current) setSidebarCollapsed(true);
    else setPanel(next);
  };
  // 열린 질문(id → 질문). 연결할 때마다 목록을 새로 읽고, 이후는 질문 사건으로 따라간다.
  const [questions, setQuestions] = useState<Map<string, QuestionView>>(new Map());
  // HTTP 질문 목록을 화면에 반영한 횟수. 사건과 목록 응답의 완료 경계를 관찰한다.
  const [questionsSync, setQuestionsSync] = useState(0);
  const questionReload = useRef(new QuestionReload());
  // thread(project/id → 기록). 연결할 때 목록을 읽고 thread 사건으로 따라간다.
  const [threads, setThreads] = useState<Map<string, ThreadView>>(new Map());
  const [threadsSync, setThreadsSync] = useState(0);
  const threadReload = useRef(new ThreadReload());
  const [automationBadge, setAutomationBadge] = useState(0);
  const automationLoading = useRef(false);
  const automationReload = useRef(false);
  const automationActive = useRef(true);
  const loadAutomations = () => {
    if (!automationActive.current) return;
    if (automationLoading.current) { automationReload.current = true; return; }
    automationLoading.current = true;
    call({ op: "projects" }).then(async (r) => {
      if (!automationActive.current || r.kind !== "projects") return;
      const lists = await Promise.all(r.projects.map((p) => call({ op: "routines", project: p.name })));
      if (automationActive.current) setAutomationBadge(lists.reduce((n, r) => n + (r.kind === "routines" ? automationAttention(r.routines) : 0), 0));
    }).catch(() => {
      // WebKit은 reload 때 pagehide 전에 요청을 취소할 수 있다. 실패한 읽기의 예약을 재실행하지 않는다.
      automationReload.current = false;
    }).finally(() => {
      automationLoading.current = false;
      if (automationReload.current) { automationReload.current = false; loadAutomations(); }
    });
  };
  useEffect(() => {
    automationActive.current = true;
    // reload가 진행 중 요청을 취소해도 finally의 예약 조회가 끝나는 문서에서 새 fetch를 시작하면 안 된다.
    const hide = () => { automationActive.current = false; automationReload.current = false; };
    const show = () => { automationActive.current = true; loadAutomations(); };
    window.addEventListener("pagehide", hide);
    window.addEventListener("pageshow", show);
    return () => {
      hide();
      window.removeEventListener("pagehide", hide);
      window.removeEventListener("pageshow", show);
    };
  }, []);
  const putThread = (t: ThreadView) => {
    threadReload.current.note(t);
    setThreads((old) => {
      const next = new Map(old);
      next.set(threadKey(t), t);
      return next;
    });
  };

  const putQuestion = (q: QuestionView) => {
    questionReload.current.note(q);
    setQuestions((old) => {
      const next = new Map(old);
      if (q.state === "open") next.set(q.id, q);
      else next.delete(q.id);
      return next;
    });
  };

  // 이 기기의 알림(plan §8.70): host가 알리는 종류(`notify.kinds`의 `thread-state`)에만 띄운다. 질문이 열리면 바로
  // 「입력 필요」, thread가 검토 대기가 된 새 보고는 「작업 완료」(외부 푸시는 coordinator 사용자 post만, host 「new report」
  // 항목과 같은 규칙: 검토 대기일 때 보고 hash당 한 번, 작업 중에 고쳐 쓴 보고는 아니다, plan §8.85).
  // 연결할 때 읽은 목록(처음 본 것)은 새 소식이 아니다.
  const alertKinds = useRef<Set<string> | null>(null);
  const seenQuestions = useRef(new Set<string>());
  const seenReports = useRef(new Map<string, string | null>());
  // 이 화면이 한 번이라도 붙었었나: 다시 붙을 때 읽은 목록은 끊긴 동안 놓친 사건이다(아래)
  const linkedOnce = useRef({ questions: false, threads: false });
  const now = useRef({ sel, panes, threads, machines, role, loaded });
  now.current = { sel, panes, threads, machines, role, loaded };
  // 늦은 알림(멈췄다 깬 페이지가 받은 사건, 다시 붙어 읽은 놓친 사건): 이 기기에 그 뒤 이미 push로 보낸 항목이면 앱 안에서
  // 다시 띄우지 않는다. push가 안 간 항목(push 꺼짐·다른 기기가 쓰는 중이라 건너뜀)만 띄운다(사용자 결정 10-18 「A」, plan §8.141).
  // 목록의 안 읽음 표시는 그대로다. host에 묻지 못하면 띄운다(놓치지 않는 쪽)
  const linkDownAt = useRef<number | null>(null);
  // 늦은지는 사건을 받은 때 정한다(host에 묻는 동안 시간이 간다)
  type Raise = (late: boolean) => string;
  const alertLate = (replayFrom: number | null, project: string | null, subject: string | null, raise: Raise, told: (o: string) => void) => {
    // 늦은 알림: 다시 붙어 읽은 놓친 사건이거나 멈췄다 깬 직후 받은 사건. 가려진 채 살아 있는 페이지(데스크톱 탭)가 제때 받은
    // 사건은 늦지 않다(소리를 낸다). 늦은 알림은 알림을 띄우지 못한 동안의 시작부터 이 기기에 간 push를 본다
    const late = replayFrom !== null || justWoke();
    const since = replayFrom ?? (late ? unableSince() : null);
    if (!late || since === null || project === null || subject === null) return told(raise(late));
    void pushedSince(since)
      .then((pushed) => pushed.some((p) => p.project === project && p.subject === subject))
      .catch(() => false)
      .then((got) => told(got ? "push로 받음 · 안 띄움" : raise(true)));
  };
  /** `replayFrom`: 다시 붙어 읽은 놓친 사건이면 끊긴 때 */
  const questionNews = (q: QuestionView, replayFrom: number | null = null) => {
    if (q.state !== "open" || seenQuestions.current.has(q.id)) return;
    seenQuestions.current.add(q.id);
    const target = `pane=${encodeURIComponent(q.pane)}`;
    // 어떻게 했는지 host에 알린다(push 판정 근거, 10-18 실기)
    const told = (outcome: string) => tellAlert({ project: q.project ?? null, target, kind: "blocked", outcome });
    if (!alertKinds.current?.has("thread-state")) return told(alertKinds.current ? "host 알림 종류에 없음" : "host 알림 종류 아직 모름");
    const { sel, panes, threads, machines } = now.current;
    const pane = panes.find((p) => p.id === q.pane);
    const thread = q.project && q.thread ? threads.get(`${q.project}/${q.thread}`) : undefined;
    const name = thread?.title || (pane ? pane.name || pane.argv.join(" ") || q.pane.slice(0, 8) : q.pane.slice(0, 8));
    const raise: Raise = (late) => raiseAlert({
      kind: "blocked",
      target,
      late,
      showing: sel === `pane:${q.pane}`,
      title: name,
      machine: machines.length > 1 ? q.machine : null,
      agent: agentOf(pane, thread),
      notice: {
        title: q.project && q.thread ? `${q.project} · ${thread?.title || q.thread}` : `${q.machine} · ${name}`,
        body: q.text,
        tag: q.project && q.thread ? `aaa-${q.project}/${q.thread}` : `aaa-pane-${q.pane}`,
      },
    });
    alertLate(replayFrom, q.project ?? null, q.thread ?? null, raise, told);
  };
  // 사용량 한도가 다시 차오름(plan §8.267): 한도였던 thread의 표지가 풀리는 때가 지나 걷히면 「한도 풀림」으로 그 thread에. host는 같은
  // 때에 coordinator inbox 항목을 만든다. 숫자가 내려가 일찍 걷힌 것은 알리지 않는다(host `reset_due`와 같다). 목록을 다시 읽을 때는
  // 표지만 맞춘다(놓친 바뀜을 알리지 않는다). 저절로 잇지 않는다
  const limitedUntil = useRef(new Map<string, number>());
  const limitNews = (t: ThreadView) => {
    const key = threadKey(t);
    const until = limitedUntil.current.get(key);
    if (t.limited_until_ms != null) return void limitedUntil.current.set(key, t.limited_until_ms);
    limitedUntil.current.delete(key);
    if (until === undefined || until > Date.now()) return;
    const { sel, panes } = now.current;
    raiseAlert({
      kind: "reset",
      target: `thread=${encodeURIComponent(key)}`,
      late: justWoke(),
      showing: sel === `thread:${key}` || (t.pane != null && sel === `pane:${t.pane}`),
      title: t.title || t.id,
      machine: null,
      agent: agentOf(t.pane ? panes.find((x) => x.id === t.pane) : undefined, t),
      notice: { title: `${t.project} · ${t.title || t.id}`, body: "사용량 한도 풀림(저절로 잇지 않았다)", tag: `aaa-${t.project}/${t.id}` },
    });
  };
  // seenReports: thread마다 알린(또는 처음 볼 때 이미 검토 대기였던) 보고 hash
  // 자동화 완료는 host가 저절로 확인한다. 사본 사건과 ack 사이에도 알림을 띄우지 않는다(A2).
  const reviewHash = (t: ThreadView) => (t.automation && t.report_attention === "completed" ? null : t.report_attention && t.report_hash !== t.acked_hash ? t.report_hash : null);
  const reportNews = (t: ThreadView, replayFrom: number | null = null) => {
    const key = threadKey(t);
    const target = `thread=${encodeURIComponent(key)}`;
    const kind = t.report_attention === "needs_user" ? "input" : "done";
    const told = (outcome: string) => tellAlert({ project: t.project, target, kind, outcome });
    if (!seenReports.current.has(key)) {
      seenReports.current.set(key, reviewHash(t));
      // 처음 보는 thread는 새 소식이 아니다. 검토 대기 보고면 무엇을 하지 않았는지 남긴다(목록을 읽기 전에 온 사건도 여기다)
      if (reviewHash(t) !== null) told("처음 본 thread라 알리지 않음");
      return;
    }
    const hash = reviewHash(t);
    if (hash === null || hash === seenReports.current.get(key)) return;
    seenReports.current.set(key, hash);
    if (!alertKinds.current?.has("thread-state")) return told(alertKinds.current ? "host 알림 종류에 없음" : "host 알림 종류 아직 모름");
    const show = (libraryUrl: string | null) => {
      const { sel, panes } = now.current;
      const raise: Raise = (late) => raiseAlert({
        kind: kind === "input" ? "blocked" : "done", target, late, libraryUrl,
        showing: sel === `thread:${key}`,
        title: t.title || t.id,
        machine: null,
        agent: agentOf(t.pane ? panes.find((x) => x.id === t.pane) : undefined, t),
        notice: { title: `${t.project} · ${t.title || t.id}`, body: `${kind === "input" ? "확인 필요" : "맡긴 일 완료"}${libraryUrl ? `\n보기: ${libraryUrl}` : ""}`, tag: `aaa-${t.project}/${t.id}` },
      });
      alertLate(replayFrom, t.project, t.id, raise, told);
    };
    // 공유 링크는 조작 기기에만 준다. 조회 실패도 새 보고 알림을 없애지 않는다.
    if (role === "reader") show(null);
    else void call({ op: "thread_library", project: t.project, thread: t.id })
      .then((r) => show(r.kind === "library" && r.link && !r.link.revoked && r.link.expires_at_ms > Date.now() ? r.link.url : null))
      .catch(() => show(null));
  };

  // 다른 agent의 턴 끝남(host `notice` 사건, plan §8.185): 「작업 완료」로 그 pane에. host는 보는 화면이 없던 턴만, 같은 턴의
  // thread 새 보고는 새 보고로만 알린다. 웹은 긴 턴(herdr-web-ui `dropletAllows` 기본 `done: "long"`, 60초 이상이거나 길이를
  // 모름)만 띄운다. 휴대폰 push는 없다. 새 보고는 위 thread 길이, 입력 필요는 질문 길이 맡는다. 기록하지 않는 사건이라 다시
  // 붙을 때 놓친 것을 읽지 않는다(참고 `seedStatuses`: 목록은 알리지 않는다)
  const noticeNews = (n: NoticeView) => {
    if (n.kind === "host_issue" || n.kind === "host_recovered") {
      showDroplet({ target: "threads", title: n.title, kind: n.kind === "host_recovered" ? "done" : "blocked", machine: null, agent: null });
      return;
    }

    if (n.kind === "rollout_finished" || n.kind === "rollout_failed") {
      showDroplet({ target: "deployment", title: n.title, kind: n.kind === "rollout_finished" ? "done" : "blocked", machine: null, agent: null });
      return;
    }

    if (n.kind === "post_muted" || n.kind === "automation") {
      // 누르면 모든 thread(host는 프로젝트 하나, plan §8.291)
      showDroplet({ target: "threads", title: n.title, kind: n.kind === "automation" ? "done" : "blocked", machine: null, agent: null });
      return;
    }
    if (n.kind !== "finished" || n.pane === null) return;
    const target = `pane=${encodeURIComponent(n.pane)}`;
    const told = (outcome: string) => tellAlert({ project: n.project, target, kind: "done", outcome });
    if (!n.long) return told(`짧은 턴(${Math.round((n.turn_ms ?? 0) / 1000)}초)`);
    const { sel, panes, threads, machines } = now.current;
    const pane = panes.find((p) => p.id === n.pane);
    const key = n.project && n.thread ? `${n.project}/${n.thread}` : null;
    const thread = key ? threads.get(key) : undefined;
    const name = thread?.title || n.title || (pane ? pane.name || pane.argv.join(" ") : n.pane.slice(0, 8));
    const machine = pane?.machine ?? n.machine;
    told(
      raiseAlert({
        kind: "done",
        target,
        late: justWoke(),
        showing: sel === `pane:${n.pane}` || (key !== null && sel === `thread:${key}`),
        title: name,
        machine: machines.length > 1 ? machine : null,
        agent: n.agent ?? agentOf(pane, thread),
        notice: {
          title: key ? `${n.project} · ${thread?.title || n.thread}` : `${machine} · ${name}`,
          body: "작업 완료",
          tag: key ? `aaa-${key}` : `aaa-pane-${n.pane}`,
        },
      }),
    );
  };

  // 공유 설정과 고정(herdr-web-ui App.tsx usePreferencesSync, plan §8.79): 들어온 뒤에만 읽고, events 사건으로 따라간다
  const syncPreferences = usePreferencesSync(true);
  const receivePreferences = useRef(syncPreferences);
  receivePreferences.current = syncPreferences;
  useEffect(
    () =>
      watchEvents(
        (ev) => {
          loadAutomations();
          setMachines(ev.machines);
          setPanes(ev.panes);
          setLoaded(true);
        },
        (q) => {
          questionNews(q);
          putQuestion(q);
        },
        (on) => {
          setUp(on);
          if (!on) linkDownAt.current ??= unableSince() ?? Date.now();
          // 끊긴 때(놓친 사건을 다시 읽을 때 그 뒤 이 기기에 간 push를 본다)
          const down = on ? (linkDownAt.current ?? Date.now()) : 0;
          if (on) linkDownAt.current = null;
          if (on)
            pushInfo()
              .then((i) => (alertKinds.current = new Set(i.kinds)))
              .catch(() => {});
          if (on) {
            const read = questionReload.current.begin();
            call({ op: "questions", all: false })
              .then((r) => {
                if (r.kind !== "questions") return questionReload.current.failed(read);
                const next = questionReload.current.finish(read, r.questions.filter((q) => q.pane !== ""));
                if (next === null) return; // 다시 붙은 뒤 더 늦게 돌아온 옛 연결의 응답
                // 처음 붙을 때 읽은 것은 새 소식이 아니다. 다시 붙을 때는 끊긴 동안 열린 질문을 알린다(t3code(MIT)
                // packages/client-runtime/src/state/threads.ts `afterSequence`처럼 놓친 사건을 잇는다, plan §8.84)
                if (linkedOnce.current.questions) for (const q of next.values()) questionNews(q, down);
                else for (const q of next.values()) seenQuestions.current.add(q.id);
                linkedOnce.current.questions = true;
                setQuestions(next);
                setQuestionsSync((n) => n + 1);
              })
              .catch(() => questionReload.current.failed(read));
          }
          if (on) { loadAutomations(); }
          if (on) {
            const read = threadReload.current.begin();
            call({ op: "threads", project: null, role: null })
              .then((r) => {
                if (r.kind !== "threads") return threadReload.current.failed(read);
                const next = threadReload.current.finish(read, r.threads);
                if (next === null) return;
                // 위 질문과 같다: 다시 붙을 때는 끊긴 동안 검토 대기가 된 새 보고를 알린다(처음 보는 thread는 새 소식이 아니다)
                // 끊긴 동안 새로 생긴 thread도 처음 붙을 때 목록에 없었으니 새 소식이다
                if (linkedOnce.current.threads)
                  for (const t of next.values()) {
                    if (!seenReports.current.has(threadKey(t))) seenReports.current.set(threadKey(t), null);
                    reportNews(t, down);
                  }
                else for (const t of next.values()) seenReports.current.set(threadKey(t), reviewHash(t));
                limitedUntil.current = new Map([...next.values()].flatMap((t) => (t.limited_until_ms != null ? [[threadKey(t), t.limited_until_ms]] : [])));
                linkedOnce.current.threads = true;
                setThreads(next);
                setThreadsSync((n) => n + 1);
              })
              .catch(() => threadReload.current.failed(read));
          }
        },
        (t) => {
          reportNews(t);
          limitNews(t);
          putThread(t);
        },
        (p) => receivePreferences.current(p),
        setHostWeb,
        (n) => noticeNews(n),
        (unseen) => {
          setUnseenReports(unseen);
          setReportsRev((n) => n + 1);
        },
        setAppBadge,
        setRollout,
      ),
    [],
  );
  useAlertSoundUnlock();

  // 실패는 그 카드가 보인다(QuestionCard): 목록 머리 띠는 휴대폰에서 pane을 보는 동안 숨는다
  const answer = async (q: QuestionView, choice: string) => {
    const r = await call({ op: "question_answer", id: q.id, choice, menu: null, pick: null });
    if (r.kind === "question") putQuestion(r);
  };
  // 화면 선택 창 답(plan §8.172): 카드가 본 창(id)에. host가 화면을 다시 읽어 다르면 거절하고 질문의 창을 새로 붙인다
  const answerMenu = async (q: QuestionView, menu: QuestionMenu, pick: MenuPick) => {
    const r = await call({ op: "question_answer", id: q.id, choice: "", menu: menu.id, pick });
    if (r.kind === "question") putQuestion(r);
  };
  // 모든 thread 화면에서 연 보고는 「뒤로」가 그 화면으로 돌아간다(herdr-projects popup: detail에서 Esc는 목록으로).
  // 그 thread를 연 주소 바꿈이 아닌 다른 이동(앱 안 알림·브라우저 뒤로·직접 친 주소)이면 돌아갈 곳을 잊는다
  const [backTo, setBackTo] = useState<string | null>(null);
  const openedHash = useRef<string | null>(null);
  // 앱이 스스로 바꾼 주소(목록·단추·첫 고르기). 그 밖의 주소 바꿈(앱 안 알림 Droplet, 브라우저 뒤로·앞으로, 붙여 넣은 링크)은
  // 그 줄이 있는 패널을 보인다(VS Code explorer autoReveal, plan §8.240)
  const ownHash = useRef<string | null>(null);
  // 다른 pane·thread·화면으로 가는 것만 기록 한 칸이다(뒤로가 앞 화면으로 돌아간다). 앱이 스스로 고른 첫 화면과 「고른 것 없음」은
  // 지금 칸을 바꾼다: 뒤로 끝에 빈 「하나를 고른다」 칸이 남지 않는다(herdr-web-ui App.tsx 1091줄 첫 쓰기·autoSelected는
  // replaceState, deepLink.ts `isNavigation`은 pane 없는 곳을 이동으로 치지 않는다, plan §8.301)
  const goHash = (hash: string, replace = false) => {
    const next = hash ? `#${hash}` : "";
    if (location.hash === next) return;
    if (replace) {
      // replaceState는 hashchange를 내지 않는다
      ownHash.current = null;
      openedHash.current = null;
      history.replaceState(history.state, "", next || location.pathname + location.search);
      return;
    }
    ownHash.current = next;
    location.hash = hash;
  };
  /**
   * 단축키·팔레트·알림이 연 pane·thread가 있는 패널로 바꾼다(sidebarPanel.ts `revealPanel`). 접어 둔 사이드바는 펼치지 않고 휴대폰
   * drawer도 열지 않는다: VS Code도 숨긴 사이드바를 열지 않고, 펼치면 터미널 폭(PTY 크기)이 바뀐다. 다음에 펼치면 그 패널이다
   */
  const reveal = (target: string | null) => {
    if (target === null || !(target.startsWith("pane:") || target.startsWith("thread:"))) return;
    const { panes, threads, machines } = now.current;
    const listed = [...threads.values()].filter((t) => t.group !== "resolved" && t.project_status !== "archived");
    // 「작업」 트리의 줄(Sidebar Roster): 예전 에이전트 패널의 줄(도는 agent pane)도 모두 이 트리에 있다
    const inWork = rosterRows(machines, panes, listed, currentSidebarShared().paneOrder).some((r) => r.keys.includes(target));
    setPanel((current) => revealPanel(current, inWork));
  };
  const revealRef = useRef(reveal);
  revealRef.current = reveal;
  useEffect(() => {
    const on = () => {
      if (location.hash !== openedHash.current) setBackTo(null);
      openedHash.current = null;
      const own = location.hash === ownHash.current;
      ownHash.current = null;
      const next = selectedFromHash();
      if (!own) {
        // 브라우저 뒤로·앞으로는 이동 직전 drawer 상태를 유지한다(창마다 기억하지 않는다, plan §8.340). 목록 줄을 직접 고를 때는
        // choose·chooseThread 등이 닫는다. 가장자리 쓸기가 drawer를 열고 Safari 뒤로와 겹쳐도 그 열린 상태를 유지한다.
        // 고른 것이 없는 칸(옛 첫 칸·「고른 것 없음」)이나 닫힌 pane의 칸에 닿으면 지금 화면에 머물고 그 칸에 지금 주소를 쓴다(참고
        // onPop: pane이 없으면 그대로, 닿은 칸에는 보이는 곳을 쓴다). 빈 「하나를 고른다」로 떨어지지 않는다
        const { sel: shown, panes, loaded } = now.current;
        const closed = next?.startsWith("pane:") && loaded && !panes.some((p) => `pane:${p.id}` === next || p.replaced?.includes(next.slice(5)));
        if (shown !== null && (next === null || closed)) {
          history.replaceState(history.state, "", `#${hashOf(shown)}`);
          if (closed) setHeadNote("그 pane은 닫혔습니다");
          return;
        }
      }
      setSel(next);
      if (!own) revealRef.current(next);
    };
    window.addEventListener("hashchange", on);
    return () => window.removeEventListener("hashchange", on);
  }, []);

  // 고르면 drawer를 닫는다(참고 selectTarget·selectPane의 setDrawerOpen(false))
  // App이 스스로 고른 pane(주소의 pane이 닫혀 첫 줄로 왔다): 사용자가 고를 때까지 키보드를 가져가지 않는다(참고 autoSelected)
  const [autoSelected, setAutoSelected] = useState(false);
  // replace: 지금 칸을 바꾼다(앱이 고른 첫 화면, 고른 것 없음. goHash)
  const choose = (id: string | null, auto = false, replace = auto || id === null) => {
    goHash(id ? `pane=${encodeURIComponent(id)}` : "", replace);
    setSel(id ? `pane:${id}` : null);
    setAutoSelected(auto);
    setDrawerOpen(false);
  };
  // 보던 pane이 새 pane으로 바뀌었다(`pane resume`·재부팅 복원은 새 세션을 띄우고 옛 기록을 지운다, PaneView.replaced): 그 자리를
  // 이은 pane을 따라가고 주소도 바꾼다(뒤로 가기에 옛 주소를 남기지 않는다, QA Q19)
  useEffect(() => {
    if (!sel?.startsWith("pane:")) return;
    const id = sel.slice("pane:".length);
    if (panes.some((p) => p.id === id)) return;
    const next = panes.find((p) => p.replaced?.includes(id));
    if (!next) return;
    history.replaceState(history.state, "", `#pane=${encodeURIComponent(next.id)}`);
    setSel(`pane:${next.id}`);
  }, [panes, sel]);
  const chooseThread = (t: ThreadView, from: string | null = null, replace = false) => {
    setAutoSelected(false);
    setDrawerOpen(false);
    setBackTo(from);
    openedHash.current = `#${hashOf(`thread:${threadKey(t)}`)}`;
    goHash(openedHash.current.slice(1), replace);
    setSel(`thread:${threadKey(t)}`);
  };
  // 할 일 「coordinator에게 맡기기」(t-0469 §3.4, plan §8.316): 도는 coordinator(없으면 기본 profile로 띄운 것, `project open`)의
  // 입력칸에 글을 채우고 그 pane을 연다. 보내기는 사람이 누른다. 대화 보기 입력창과 터미널 입력줄 초안을 함께 채운다(그 pane을 어느
  // 보기로 열든 보인다). 이미 쓴 초안은 덮지 않고 뒤에 붙인다
  const handOff = async (text: string) => {
    const r = await call({ op: "project_open" });
    if (r.kind !== "project_opened") throw new Error(`예상하지 못한 응답: ${r.kind}`);
    const p = r.pane;
    const add = (cur: string) => (cur.includes(text) ? cur : cur.trim() === "" ? text : `${cur.trimEnd()}\n\n${text}`);
    composerDrafts.set(`herdr-web-ui:composer-draft:${p.machine_id}:${p.id}`, add);
    const line = `${p.machine}:${p.id}`;
    writeTerminalDraft(line, add(readTerminalDraft(line)));
    choose(p.id);
  };
  // 모든 thread(끝난 것까지, herdr-projects popup Threads). 프로젝트 고르기는 없다(host는 프로젝트 하나, plan §8.291)
  const chooseThreads = () => {
    setDrawerOpen(false);
    goHash("threads");
    setSel("threads:");
  };
  // 짧은 thread 주소(`#thread=<id>`)는 thread 목록을 받으면 내부 열쇠로 바꾼다. 보관한 프로젝트의 thread는 고르지 않는다
  useEffect(() => {
    if (!sel?.startsWith("thread:") || sel.includes("/")) return;
    const id = sel.slice("thread:".length);
    const t = [...threads.values()].find((x) => x.id === id && x.project_status !== "archived");
    if (t) setSel(`thread:${threadKey(t)}`);
  }, [sel, threads]);
  // 보고서 화면(plan §8.240): 막대 「보고서」는 마지막으로 본 보고서 자리(목록이나 연 보고서)로, 보고서 화면에서 다시 누르면 그 앞
  // 화면으로 간다. 한 번 연 보고서 화면은 다른 화면으로 가도 그려 둔다(ReportsView): 돌아오면 연 보고서·굴린 자리가 그대로다
  const reportsShown = sel?.startsWith("reports:") === true;
  // 안 본 사용자 보고서 수와 목록이 다시 읽을 때(host `reports` 사건: 올림·열어 봄·지움, plan §8.256)
  const [unseenReports, setUnseenReports] = useState(0);
  const [reportsRev, setReportsRev] = useState(0);
  const [reportsAt, setReportsAt] = useState<string | null>(reportsShown ? sel : null);
  const beforeReports = useRef<string | null>(null);
  useEffect(() => {
    if (sel?.startsWith("reports:")) setReportsAt(sel);
    else beforeReports.current = sel;
  }, [sel]);
  // 「답할 것」 화면(plan §8.345, 사용자 결정 10-21 1B): 사이드바 「답할 것」 패널이 목록이고 본문은 고른 것 하나의 카드다. 질문 주소
  // (`question:<thread>`, 칩·알림·터미널 링크)도 이 화면이다. 한 번 연 뒤에는 숨긴 채 그려 둔다(쓰던 답·굴린 자리가 그대로)
  const asksShown = sel === "asks:" || sel?.startsWith("ask:") === true || sel?.startsWith("question:") === true;
  const [asksOpened, setAsksOpened] = useState(asksShown);
  const asksAt = useRef<string>("asks:");
  const beforeAsks = useRef<string | null>(null);
  useEffect(() => {
    // 보고서·할 일과 같이 앞 화면을 기억한다. 카드 사이 이동은 돌아갈 곳을 바꾸지 않는다.
    if (!asksShown) {
      beforeAsks.current = sel;
      return;
    }
    setAsksOpened(true);
    asksAt.current = sel!;
  }, [asksShown, sel]);
  const goSel = (next: string | null) => {
    setAutoSelected(false);
    setDrawerOpen(false);
    goHash(hashOf(next));
    setSel(next);
  };
  const toggleReports = () => {
    if (reportsShown) goSel(beforeReports.current);
    else goSel(reportsAt ?? "reports:");
  };
  // 할 일 화면(plan §8.306): 보고서와 같은 꼴. 다시 누르면 그 앞 화면으로, 한 번 연 화면은 숨긴 채 그려 둔다(거르기·접은 목록이 그대로)
  const tasksShown = sel === "tasks:" || sel?.startsWith("task:") === true;
  const taskAt = sel?.startsWith("task:") ? sel.slice(5) : null;
  const [tasksOpened, setTasksOpened] = useState(tasksShown);
  const beforeTasks = useRef<string | null>(null);
  useEffect(() => {
    if (tasksShown) setTasksOpened(true);
    else beforeTasks.current = sel;
  }, [sel, tasksShown]);
  const toggleTasks = () => goSel(tasksShown ? beforeTasks.current : "tasks:");
  // 명령 팔레트 「할 일 더하기」: 할 일 화면을 열고 새 task 폼을 연다(plan §8.311). 보고서·thread 결과물의 「할 일에 추가」는 그 문서를
  // 링크한 새 task나 있는 task에 링크하는 창을 연다(plan §8.327)
  const [taskAdds, setTaskAdds] = useState<TaskAddRequest>({ seq: 0, link: null });
  const addTask = (link: TaskAttachment | null) => {
    if (!tasksShown) toggleTasks();
    setTaskAdds((a) => ({ seq: a.seq + 1, link }));
  };
  /** 보고서 안 md·html 문서를 할 일에(조작 권한만) */
  const addReportDoc =
    role === "reader" ? undefined : (name: string, path: string) => {
      const link = reportDocLink(name, path);
      if (link) addTask(link);
    };

  // 새 pane 창(herdr-web-ui App.tsx `newSessionOpen`·`newSessionMachineId`): 머신 머리 +와 단축키가 그 머신으로 연다
  const [newPaneOn, setNewPaneOn] = useState<MachineView | null>(null);
  // 명령 팔레트(herdr-web-ui App.tsx 365~367줄 `paletteOpen`, plan §8.132)
  const [paletteOpen, setPaletteOpen] = useState(false);
  const closePalette = useCallback(() => setPaletteOpen(false), []);
  const { resolvedTheme } = useAppearance();
  const newShell = (m: MachineView) => {
    setMsg(null);
    // 참고 MachineSidebar onNew: 창이 drawer 위에 남지 않게 닫는다
    setDrawerOpen(false);
    setNewPaneOn(m);
  };
  // 새 pane의 최근 폴더는 머신의 pane이 있는 곳도 안다(참고 App.tsx 185~197줄): 시작한 폴더 뒤에 둔다. thread·평가자 pane은
  // 작업 사본(참고의 linked worktree)이고 임시 폴더는 시작할 곳이 아니다
  useEffect(() => {
    for (const m of machines) {
      if (m.connection !== "connected") continue;
      recentDirectories.remember(
        m.id,
        panes
          .filter((p) => p.machine_id === m.id && !p.role && !(p.cwd && passingFolder(p.cwd)))
          .map((p) => p.cwd)
          .reverse(),
      );
    }
  }, [machines, panes]);

  // 앱 단축키(herdr-web-ui App.tsx 1185~1279줄 `actions`·`useShortcuts`): 목록 단추와 같은 일을 부른다. 고른 pane의 일은 그
  // TermView가 단 paneControls로. aaa에 그 화면이 없는 동작은 단축키가 꺼져 있어(settings.ts AAA_SHORTCUTS) 불리지 않는다.
  const [settingsOpen, setSettingsOpen] = useState(false);
  const paneControls = useRef<PaneControls | null>(null);
  const actions = useMemo<AppActions>(() => {
    const none = () => {};
    return {
      selectAdjacentPane: (direction) => {
        // 목록 차례로, 끝에서 처음으로(참고는 고른 PC의 snapshot pane 차례). 목록이 프로젝트 먼저라(plan §8.195) 머신을 넘어
        // 목록 전체를 돈다. 끊긴 머신의 줄은 열 수 없다
        const { sel, panes, threads, machines } = now.current;
        const shown = [...threads.values()].filter((t) => t.group !== "resolved" && t.project_status !== "archived");
        const rows = rosterRows(machines, panes, shown, currentSidebarShared().paneOrder, currentAppearance().agentOrder === "activity");
        if (rows.length === 0) return;
        const index = rows.findIndex((r) => sel !== null && r.keys.includes(sel));
        const next = rows[(index + direction + rows.length) % rows.length];
        if (!next) return;
        if ("pane" in next.open) choose(next.open.pane);
        else chooseThread(next.open.thread);
        revealRef.current(next.keys[0] ?? null);
      },
      toggleView: () => paneControls.current?.toggleView(),
      focusInput: () => paneControls.current?.focusInput(),
      scrollPage: (direction) => paneControls.current?.scrollPage(direction),
      openSettings: () => {
        setDrawerOpen(false);
        setSettingsOpen(true);
      },
      openPalette: () => setPaletteOpen(true),
      openFind: () => paneControls.current?.openFind(),
      // 좁은 창에서는 drawer를 여닫고 넓은 창에서는 사이드바를 접는다·편다(참고 App.tsx 1246~1249줄)
      toggleSidebar: () => {
        if (window.matchMedia("(max-width: 768px)").matches) setDrawerOpen((open) => !open);
        else sidebarToggleRef.current();
      },
      openNewSession: () => {
        // 참고는 고른 PC로 연다: 고른 pane·thread의 머신, 없으면 붙은 첫 머신. 보기 권한 기기는 띄울 수 없다
        const { sel, panes, threads, machines, role } = now.current;
        if (role === "reader") return;
        const pane = sel?.startsWith("pane:") ? panes.find((p) => `pane:${p.id}` === sel) : undefined;
        const thread = sel?.startsWith("thread:") ? threads.get(sel.slice(7)) : undefined;
        const onId = pane?.machine_id ?? (thread?.pane ? panes.find((p) => p.id === thread.pane)?.machine_id : undefined);
        const connected = machines.filter((m) => m.connection === "connected");
        const m = connected.find((x) => x.id === onId) ?? connected[0];
        if (m) {
          setDrawerOpen(false);
          setNewPaneOn(m);
        }
      },
      openNewTab: none,
      splitPane: null,
    };
    // choose·chooseThread는 주소와 setSel만 쓴다
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  useShortcuts(actions, true);

  // 머리 알림(참고 headerNote): 8초 뒤 거둔다(참고 App.tsx 281~284줄)
  const [headNote, setHeadNote] = useState<string | null>(null);
  useEffect(() => {
    if (headNote === null) return;
    const timer = window.setTimeout(() => setHeadNote(null), 8000);
    return () => window.clearTimeout(timer);
  }, [headNote]);
  // 첫 화면은 마지막으로 연 pane·thread를 바로 연다(herdr-web-ui: 고른 것이 없으면 기억한 것, 없거나 닫혔으면 PC의 첫 pane
  // `fallback`). 주소가 무엇을 가리키면 그것이다. 목록(pane·thread)을 다 받은 뒤 한 번만 고른다
  const firstPick = useRef(sel?.startsWith("pane:") ? false : sel !== null || location.hash !== "");
  useEffect(() => {
    if (firstPick.current || !loaded || !linkedOnce.current.threads) return;
    firstPick.current = true;
    // 주소는 지금 것을 읽는다: 연 뒤에 주소가 thread·프로젝트를 가리키게 됐는데(알림·링크·주소 바꿈) 그 hashchange를 앱이 아직
    // 처리하지 않았을 수 있다(짐 아래, t-0335 찾음). 그때 첫 고르기가 그 주소를 덮지 않는다(주소가 이긴다)
    const asked = selectedFromHash();
    if (asked !== null && !asked.startsWith("pane:")) return;
    // 목록 차례로 첫 줄(붙은 머신의 첫 thread·pane)
    const pickFirst = (auto: boolean) => {
      const shown = [...threads.values()].filter((t) => t.group !== "resolved");
      const first = rosterRows(machines, panes, shown, currentSidebarShared().paneOrder, currentAppearance().agentOrder === "activity")[0];
      if (!first) return;
      if ("pane" in first.open) choose(first.open.pane, auto, true);
      else chooseThread(first.open.thread, null, true);
    };
    // 주소가 가리킨 pane이 없다(닫혔다): 첫 줄을 열고 머리에 말한다. 키보드는 사용자가 고를 때까지 둔다(참고 resolveLink
    // 「workspace-closed」·fallback·autoSelected. aaa 주소에는 workspace가 없어 그 workspace의 pane을 여는 갈래는 없다)
    const linked = asked?.startsWith("pane:") ? asked.slice(5) : null;
    if (linked !== null) {
      if (panes.some((p) => p.id === linked)) return;
      setHeadNote("그 링크의 pane은 닫혔습니다");
      return pickFirst(true);
    }
    const stored = storedSelection();
    if (stored?.startsWith("pane:") && panes.some((p) => `pane:${p.id}` === stored && p.state === "running")) return choose(stored.slice(5), false, true);
    const storedThread = stored?.startsWith("thread:") ? threads.get(stored.slice(7)) : undefined;
    if (storedThread && storedThread.group !== "resolved") return chooseThread(storedThread, null, true);
    pickFirst(false);
    // choose·chooseThread는 주소와 setSel만 쓴다
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loaded, panes, threads, machines]);
  useEffect(() => {
    if (sel?.startsWith("pane:") || sel?.startsWith("thread:")) storeSelection(sel);
  }, [sel]);

  const current = panes.find((p) => `pane:${p.id}` === sel) ?? null;
  const currentThread = sel?.startsWith("thread:") ? (threads.get(sel.slice(7)) ?? null) : null;
  // 화면 켜짐 유지: pane이나 thread를 보는 동안(herdr-web-ui App.tsx 842줄, 보이는 동안만 건다)
  const keepScreenOn = useKeepScreenOn();
  const holdScreen = keepScreenOn && (current !== null || currentThread !== null);
  useScreenWakeLock(holdScreen);
  // 표지 `data-wake-lock`: 위 wake lock effect가 이 상태(설정·걸지·보는 것)로 돈 뒤에 단다(한 컴포넌트의 effect는 선언 순서대로
  // 돈다). 시험이 「요청하지 않았다」를 sleep 없이 본다(웹 E2E 04 「화면 켜짐 유지」)
  const viewing = current ? `pane:${current.id}` : currentThread ? `thread:${currentThread.id}` : "-";
  useEffect(() => {
    document.documentElement.dataset.wakeLock = `${keepScreenOn ? "on" : "off"} ${holdScreen ? "hold" : "free"} ${viewing}`;
  }, [keepScreenOn, holdScreen, viewing]);
  // 파일 보기(결정 20 D7, herdr-web-ui useFileViewer): 휴대폰 뒤로가 닫도록 기록 한 칸이다
  const { viewing: fileViewing, openFile, closeFile } = useFileViewer();
  // 대화 글의 보고서·앱 주소(plan §8.300): 휴대폰과 홈 화면 앱에서는 새 창 대신 앱 안에서 연다. 새 창으로 연 보고서에서는 돌아올
  // 길이 없다(Safari 새 탭은 뒤로 칸이 없고, 홈 화면 앱은 뒤로 단추가 없다). 보고서는 파일 보기의 기록 한 칸이라 닫기·뒤로·가장자리
  // 쓸기가 대화로 돌아오고, 대화는 그 밑에 그대로 있어 같은 pane·같은 굴림 자리다. 넓은 창은 새 탭 습관 그대로다
  const homeScreenApp = useMediaQuery("(display-mode: standalone)") || (navigator as { standalone?: boolean }).standalone === true;
  // 대화의 Markdown이 매 그리기마다 새 문맥 값을 받지 않게 ref로 읽는다(FileViewer onOpenRef와 같다)
  const openLinkRef = useRef<(href: string) => boolean>(() => false);
  const openLink = useCallback((href: string) => openLinkRef.current(href), []);
  openLinkRef.current = (href: string): boolean => {
    if (wideScreen && !homeScreenApp) return false;
    const link = inAppLink(href, location.origin);
    if (link === null) return false;
    if (link.kind === "view") openFile({ path: link.path, pane: null, view: link.base });
    else if (link.kind === "report")
      // 보고서 이름의 보기 경로를 받아 연다(그 index.html, 없으면 host가 그린 목록). 못 받으면 앱 보고서 화면으로 간다
      void call({ op: "library_open", project: link.project, name: link.name }).then(
        (v) => (v.kind === "library" ? openFile({ path: "", pane: null, view: v.base }) : undefined),
        () => (location.hash = hashOf(`reports:${link.project}/${link.name}`)),
      );
    else location.hash = link.hash;
    return true;
  };
  // 목록은 열린 thread만(herdr-projects sidebar.rs), 모든 thread 화면은 끝난 것까지(popup.rs Section::Threads).
  // 보관한(지웠거나 이름을 바꾼) 프로젝트의 thread는 어디에도 보이지 않는다(참고 archive는 sidebar token을 지운다)
  const allThreads = [...threads.values()].filter((t) => t.project_status !== "archived");
  /** thread 번호로 연다(자동화 패널의 「열림」 칩·기록 번호, plan t-0475 A1): 목록 줄과 같게 agent pane이 돌면 그 pane, 아니면 보고 */
  const openThreadId = (project: string, id: string) => {
    const t = allThreads.find((x) => x.project === project && x.id === id);
    if (!t) return;
    const pane = t.pane ? panes.find((p) => p.id === t.pane && p.state === "running") : undefined;
    if (pane) choose(pane.id);
    else chooseThread(t);
  };
  /** 줄의 「자동 · 규칙」 표지: 자동화 패널을 보이고 그 규칙 상세를 연다(접은 사이드바는 편다) */
  const openRule = (project: string, name: string) => {
    setPanel("automations");
    setCollapsed(false);
    setRuleAsk({ project, name });
  };
  // 할 일 화면의 thread 칩(묶음)과 「task 없는 열린 thread」: 열린 thread 모음이 바뀌면 board를 다시 읽는다
  const threadGroups = useMemo(() => new Map(allThreads.map((t) => [t.id, t.group])), [threads]); // eslint-disable-line react-hooks/exhaustive-deps
  const openThreadIds = allThreads.filter((t) => t.group !== "resolved").map((t) => `${t.id}:${t.stage ?? ""}`).sort().join(" ");
  const taskBoard = useTaskBoard(up, openThreadIds, tasksShown);
  const seenMine = useSeenMine(taskBoard.board);
  const shownThreads = allThreads.filter((t) => t.group !== "resolved");
  // 「답할 것」(plan §8.345): thread 질문·coordinator 확인 필요 post·할 일 확인 필요와 host가 남긴 답한 것 기록. 활동 막대 빨강 수,
  // 대화의 칩(답한 질문은 새 보고 뒤에도 그 기록으로 「답함」)이 이것을 본다. 질문 답은 host가 한 번만 받고 thread에 보낸다
  const { data: asksData, put: putAsks } = useAsksData(up);
  const asksDone = asksData?.done ?? [];
  const askList = useMemo(() => openAsks(shownThreads, asksData, taskBoard.board?.tasks ?? []), [threads, asksData, taskBoard.board]); // eslint-disable-line react-hooks/exhaustive-deps
  const askKey = sel?.startsWith("ask:")
    ? sel.slice("ask:".length)
    : sel?.startsWith("question:")
      ? (questionKey(sel.slice("question:".length), shownThreads, asksDone, asksData?.requests ?? []) ?? `q:${sel.slice("question:".length)}`)
      : null;
  const askSelection = useAskSelection(askKey, askList, asksDone);
  const askSelected = askSelection.kind === "open" ? askSelection.ask.key : askSelection.kind === "done" ? `d:${askSelection.record.id}` : null;
  // 질문 링크로 들어오면 사이드바도 「답할 것」 목록으로 바꾼다(목록 + 카드, 1B. 휴대폰은 drawer를 열면 그 목록이다)
  useEffect(() => {
    if (asksShown && panelRef.current !== "asks") setPanel("asks");
  }, [asksShown]); // eslint-disable-line react-hooks/exhaustive-deps
  // 넓은 창에서 막대 「답할 것」으로 그 패널을 펼치면 본문도 「답할 것」(마지막으로 본 카드)
  const showPanel = (next: SidebarPanel) => {
    const opening = collapsed || next !== panelRef.current;
    pickPanel(next);
    if (next === "asks" && wideScreen && opening && !asksShown) goSel(asksAt.current);
  };
  const [taskFocus, setTaskFocus] = useState<TaskFocusRequest>({ seq: 0, title: null });
  const answerThread = async (t: ThreadView, picks: (number | null)[], text: string | null) => {
    const r = await call({ op: "thread_answer", project: t.project, thread: t.id, report_hash: t.questions!.report_hash, picks, text });
    if (r.kind === "thread") putThread(r);
  };
  // `aaa:report/<이름>`의 제목·프로젝트: 보고서 목록이 바뀔 때(host `reports` 사건) 다시 읽는다. 못 읽으면 이름 그대로다
  const [reportNames, setReportNames] = useState<Map<string, LibraryReport>>(new Map());
  useEffect(() => {
    if (!up) return;
    let live = true;
    call({ op: "library_reports", project: null })
      .then((r) => {
        if (live && r.kind === "library_reports") setReportNames(new Map(r.reports.flatMap((x) => [[x.name, x] as const, ...(x.versions ?? []).map((id) => [id, { ...x, name: id, report_id: id }] as const)])));
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [up, reportsRev]);
  // 프로젝트 없는 보고서 주소(`#report=<이름>`, coordinator가 채팅에 쓰는 링크)는 보고서 목록을 받으면 채운다
  useEffect(() => {
    if (!sel?.startsWith("reports:") || sel === "reports:" || sel.includes("/")) return;
    const r = reportNames.get(sel.slice("reports:".length));
    if (r) setSel(`reports:${r.project}/${r.name}`);
  }, [sel, reportNames]); // eslint-disable-line react-hooks/exhaustive-deps
  const knownIds = useMemo(() => new Set([
    ...allThreads.map((t) => t.id),
    ...(taskBoard.board?.tasks ?? []).flatMap((t) => t.short_id ? [t.short_id] : []),
    ...[...reportNames.values()].flatMap((r) => r.report_id ? [r.report_id] : []),
    ...askList.flatMap((a) => a.kind === "question" ? a.thread.questions.items.map((q) => q.id) : []),
    ...(asksData?.requests ?? []).flatMap((r) => [r.question_id, ...r.questions?.items.map((q) => q.id) ?? []]),
    ...asksDone.flatMap((r) => [r.question_id, ...r.items.map((q) => q.id)]),
  ]), [threads, taskBoard.board, reportNames, asksData]); // eslint-disable-line react-hooks/exhaustive-deps
  // 실제 대상·칩의 표시가 바뀔 때만 대화 Markdown을 다시 그린다.
  const refKey = [
    ...knownIds,
    ...shownThreads.filter((t) => t.questions).map((t) => `${t.id}:${t.questions!.answer ? `done ${t.questions!.answer.summary}` : `ask ${t.questions!.items.length}`}`),
    ...asksDone.filter((r) => r.kind === "question").map((r) => `${r.subject}:record ${r.id} ${r.summary}`),
    ...[...reportNames.values()].map((r) => `${r.project}/${r.name}:${r.title ?? ""}`),
  ].join("\n");
  const openRefRef = useRef<(ref: AppRef | AppId) => void>(() => {});
  openRefRef.current = (ref) => {
    if (ref.kind === "thread") {
      // 내부 project/id로 고른다. 짧은 주소의 hashchange와 정규화가 선택을 덮지 않게 한다.
      const t = allThreads.find((t) => t.id === ref.target);
      if (t) chooseThread(t);
      return;
    }
    if (ref.kind === "task") return goSel(`task:${ref.target}`);
    if (ref.kind === "question") return goSel(`question:${ref.target}`);
    const r = reportNames.get(ref.target);
    goSel(r ? `reports:${r.project}/${r.name}` : "reports:");
    if (!r) setHeadNote(`보고서 ${ref.target}을(를) 찾지 못했다`);
  };
  const appRefs = useMemo<AppRefs>(() => {
    const byId = new Map(shownThreads.map((t) => [t.id, t]));
    return {
      exists: (id) => knownIds.has(id.target),
      openId: (id) => openRefRef.current(id),
      look: (ref): AppRefLook => {
        if (ref.kind === "report") {
          const r = reportNames.get(ref.target);
          return { label: r?.title ?? ref.target, tone: "report", state: r?.from_thread && r.from_thread !== ref.target ? r.from_thread : null };
        }
        const q = byId.get(ref.target)?.questions;
        if (q && !q.answer) return { label: `${ref.target} 질문 ${q.items.length}개`, tone: "ask", state: "답할 것" };
        if (q?.answer) return { label: `${ref.target} 질문`, tone: "done", state: `답함 · ${q.answer.summary}` };
        // 새 보고로 질문·답이 비어도 host가 남긴 답한 기록이 「답함」이다(누르면 그때 원문)
        const rec = lastAnswer(asksDone, ref.target);
        if (rec) return { label: `${ref.target} 질문`, tone: "done", state: rec.answer ? `답함 · ${rec.answer.summary}` : rec.summary };
        return { label: `${ref.target} 질문`, tone: "plain", state: null };
      },
      open: (ref) => openRefRef.current(ref),
    };
  }, [refKey]); // eslint-disable-line react-hooks/exhaustive-deps
  // pane → 그 pane에서 도는 thread(목록에서 pane 이름 대신 thread 제목을 보인다)
  const threadOfPane = new Map<string, ThreadView>();
  for (const t of threads.values()) if (t.pane) threadOfPane.set(t.pane, t);
  const canControl = role !== "reader";
  const open = [...questions.values()].sort((a, b) => a.created_at_ms - b.created_at_ms);
  const currentQ = current ? open.filter((q) => q.pane === current.id) : [];
  // 그 thread 보고의 질문 칩(입력칸 위): 지금 보고에 질문이 있을 때만(답한 기록은 대화 칩이 보인다)
  const dockAsk = current ? threadOfPane.get(current.id)?.questions != null : false;
  const whereOf = (q: QuestionView) => {
    if (q.project && q.thread) return threads.get(`${q.project}/${q.thread}`)?.title ?? `${q.project}/${q.thread}`;
    const p = panes.find((x) => x.id === q.pane);
    return p ? p.name || p.argv.join(" ") || q.pane.slice(0, 8) : q.pane.slice(0, 8);
  };
  const paneTitle = (p: PaneView) => threadOfPane.get(p.id)?.title ?? (p.name || p.argv.join(" ") || p.id);
  const paneSub = (p: PaneView) => {
    const t = threadOfPane.get(p.id);
    if (t) return `${t.project}/${t.id} · ${t.profile}`;
    const parts = [p.agent?.harness, p.cwd ? shortPath(p.cwd) : p.argv.join(" ")].filter(Boolean);
    return parts.join(" · ") || p.id.slice(0, 8);
  };

  // 팔레트 동작(참고 App.tsx 1220~1260줄 `actions` 가운데 aaa에 있는 것). 보기 전환은 고른 pane이 대화를 볼 수 있을 때만
  const paneView = current ? (paneControls.current?.view() ?? null) : null;
  const paletteActions: PaletteActions = {
    openNewSession: canControl ? actions.openNewSession : null,
    toggleView: paneView ? actions.toggleView : null,
    view: paneView,
    openFind: paneView === "chat" ? actions.openFind : null,
    toggleTheme: () => updateAppearance({ theme: resolvedTheme === "dark" ? "light" : "dark" }),
    openSettings: () => setSettingsOpen(true),
    openThreads: chooseThreads,
    openReports: () => {
      if (!reportsShown) toggleReports();
    },
    openTasks: () => {
      if (!tasksShown) toggleTasks();
    },
    addTask:
      role === "reader"
        ? null
        : () => addTask(null),
    toggleSidebar: actions.toggleSidebar,
  };

  // 막대 단추의 배지(VS Code activity bar badge): 접었거나 다른 패널을 보는 동안에도 볼 일이 보인다
  // 「작업」은 예전 프로젝트·에이전트 두 배지의 합집합이다(plan §8.280): 확인 필요·검토 대기 thread와, 그 밖에 답을 기다리는 agent pane.
  const needThreads = shownThreads.filter((t) => needsYou(t.group));
  const needPanes = new Set(needThreads.flatMap((t) => (t.pane ? [t.pane] : [])));
  const waitingAgents = panes.filter(
    (p) => p.connection === "connected" && !needPanes.has(p.id) && paneAgentStatus(p) === "blocked" && agentOf(p, threadOfPane.get(p.id)) !== null,
  );
  useEffect(() => { const refresh = () => loadAutomations(); window.addEventListener("aaa:automations-changed", refresh); return () => window.removeEventListener("aaa:automations-changed", refresh); }, []);
  const badges = {
    asks: { count: askList.length, label: "질문·확인 필요·할 일", ask: true },
    work: { count: needThreads.length + waitingAgents.length, label: "확인 필요·검토 대기·답을 기다림" },
    machines: { count: highMachines(machines), label: `CPU·MEM ${HIGH_STAT}% 넘음` },
    automations: { count: automationBadge, label: "막힌 큐·승인 필요" },
  };
  // 막대 맨 위 「coordinator」(t-0621): 도는 coordinator pane(「작업」 트리의 coordinator 줄, sidebarModel `roster`와 같은 판정)을 연다.
  // 없으면 기본 profile로 띄워 연다(할 일 「coordinator에게 맡기기」의 `project_open`). 보기 권한은 띄울 수 없어 그때 단추가 없다
  const coordinatorPane = panes.find((p) => p.coordinator_project && p.state === "running" && !threadOfPane.has(p.id)) ?? null;
  const [coordinatorBusy, setCoordinatorBusy] = useState(false);
  const openCoordinator = async () => {
    if (coordinatorPane) return choose(coordinatorPane.id);
    if (coordinatorBusy) return;
    setCoordinatorBusy(true);
    setMsg(null);
    try {
      const r = await call({ op: "project_open" });
      if (r.kind === "project_opened") choose(r.pane.id);
    } catch (e) {
      setMsg(e instanceof Error ? e.message : String(e));
    } finally {
      setCoordinatorBusy(false);
    }
  };
  const coordinatorStatus = coordinatorPane?.connection === "connected" ? paneAgentStatus(coordinatorPane) : undefined;
  const coordinatorButton: CoordinatorButton | null =
    coordinatorPane || canControl
      ? {
          active: coordinatorPane !== null && current?.id === coordinatorPane.id,
          // 새 답은 보는 동안 host가 본 것으로 적는다(seen): 그 사이 점이 깜빡이지 않게 보는 pane은 뺀다
          badge: coordinatorStatus === "blocked" ? "ask" : coordinatorStatus === "done" && current?.id !== coordinatorPane?.id ? "reply" : null,
          busy: coordinatorBusy,
          onOpen: () => void openCoordinator(),
        }
      : null;
  // 탭 아이콘의 안 본 알림 수(faviconBadge.ts, plan §8.266): 창에 focus가 오면 지운다
  useEffect(() => watchBadgeFocus(), []);
  // 넓은 창에서 접었으면 사이드바 칸이 없다(좁은 창은 drawer라 늘 칸이 있다)
  const sidebarShown = !wideScreen || !collapsed;
  // 사이드바 폭(SidebarResizer, plan §8.257): 끄는 동안은 화면에만 두고 놓으면 공유 설정에 남긴다
  const savedSidebarWidth = useSidebarShared().sidebarWidth;
  const [draggedSidebarWidth, setDraggedSidebarWidth] = useState<number | null>(null);
  const sidebarWidth = draggedSidebarWidth ?? savedSidebarWidth;
  const resizeSidebar = (width: number | null, save: boolean) => {
    if (!save) return setDraggedSidebarWidth(width);
    updateSidebarShared({ sidebarWidth: width });
    setDraggedSidebarWidth(null);
  };

  return (
    <DrawerContext.Provider value={drawer}>
    <HeadContext.Provider value={{ more: <MoreMenu onThreads={chooseThreads} />, note: headNote }}>
    <AppRefContext.Provider value={appRefs}>
    <div
      className={`app${collapsed ? " sidebar-collapsed" : ""}`}
      style={sidebarWidth === null ? undefined : ({ "--sidebar-user-w": `${sidebarWidth}px` } as CSSProperties)}
      data-questions-sync={questionsSync}
      data-threads-sync={threadsSync}
      data-sidebar={panel}
    >
      <aside id="workspace-drawer" className={`list${drawerOpen ? " is-open" : ""}`}>
        <ActivityBar
          coordinator={coordinatorButton}
          panel={panel}
          shown={sidebarShown}
          onPanel={showPanel}
          badges={badges}
          reports={reportsShown}
          onReports={toggleReports}
          unseenReports={unseenReports}
          tasks={tasksShown}
          onTasks={toggleTasks}
          // 할 일 화면이 보이는 동안 me task는 이미 본 것이다(본 기록은 effect가 그린 뒤에 남겨 그 사이 배지가 튄다)
          taskBadge={taskBadge(taskBoard.board, tasksShown ? null : seenMine.seen)}
          up={up}
          reader={role === "reader"}
          settings={<SettingsGear role={role} device={device} machines={machines} onSignedOut={signedOut} open={settingsOpen} onOpen={setSettingsOpen} />}
        />
        <div id="sidebar-column" className="sidebar-shell">
        <header className="list-head">
          <h2 className="list-title" data-testid="sidebar-title">{PANEL_LABEL[panel]}</h2>
          {role === "reader" && <span className="list-role">보기 권한</span>}
          <span className="spacer" />
          <button
            className="ghost icon palette-button"
            data-testid="palette-open"
            aria-label="명령 팔레트"
            title="명령 팔레트 (⌘⇧K)"
            onClick={() => setPaletteOpen(true)}
          >
            <Icon name="search" />
          </button>
        </header>
        {/* 패널(목록)만 굴러가고 머리와 바닥 줄(사용량)은 늘 제자리(herdr-web-ui MachineSidebar) */}
        <ErrorBoundary area="목록" resetKeys={[panel]}>
        <Sidebar
          panel={panel}
          machines={machines}
          rollout={rollout}
          connected={up}
          panes={panes}
          threads={shownThreads}
          selected={sel}
          canControl={canControl}
          agentOf={agentOf}
          onPane={(id) => choose(id)}
          onThread={(t) => chooseThread(t)}
          onNewShell={newShell}
          onThreads={chooseThreads}
          onAutomation={openRule}
          onThreadId={openThreadId}
          ruleAsk={ruleAsk}
          onRuleAsked={() => setRuleAsk(null)}
          onAsks={() => showPanel("asks")}
          asks={
            <AsksPanel
              open={askList}
              data={asksData}
              selected={asksShown ? askSelected : null}
              canControl={canControl}
              onPick={(k) => goSel(`ask:${k}`)}
              onForgetAll={async () => putAsks(await call({ op: "ask_forget", ids: [], all: true }))}
            />
          }
          banners={
            <>
              {!up && loaded && <div className="banner warn">host와 연결이 끊겼다. 다시 연결하는 중</div>}
              {msg && <div className="banner err">{msg}</div>}
              {!loaded && <p className="tree-state">불러오는 중</p>}
            </>
          }
        />
        </ErrorBoundary>
        <ListFoot />
        </div>
        <SidebarResizer width={sidebarWidth} onResize={resizeSidebar} />
      </aside>
      {/* 닫혀도 페이지에 둔다: 그 위에서 시작한 쓸기가 drawer를 닫는다(참고 styles.css) */}
      <div className={`scrim${drawerOpen ? " is-open" : ""}`} aria-hidden="true" onClick={() => setDrawerOpen(false)} />
      <main className="view">
        <UpdateNotice hostBuild={hostWeb} />
        {role !== "reader" && <PasskeyNotice state={passkey} onChanged={reloadPasskey} />}
        {/* 그리기 오류는 본문 자리에서 멈춘다(ErrorBoundary, plan §8.266): 다른 화면으로 가면 다시 그린다 */}
        <ErrorBoundary area="본문" resetKeys={[sel, reportsShown, tasksShown, asksShown]}>
        {/* 보고서 화면은 한 번 연 뒤 늘 그려 두고 숨긴다(ReportsView) */}
        {reportsAt !== null && (
          <ReportsView
            shown={reportsShown}
            open={reportOf(reportsAt)}
            role={role}
            rev={reportsRev}
            onOpen={(r) => goSel(r ? `reports:${r.project}/${r.name}` : "reports:")}
            onAddDoc={addReportDoc}
          />
        )}
        {asksOpened && (
          <AsksView
            shown={asksShown}
            onClose={() => goSel(beforeAsks.current)}
            selection={askSelection}
            canControl={canControl}
            onAnswer={answerThread}
            onRequestAnswer={async (r, picks, text) => putAsks(await call({ op: "ask_answer", item: r.id, body_hash: r.questions!.report_hash, picks, text }))}
            onHandle={async (item) => putAsks(await call({ op: "ask_handle", item }))}
            onReply={(title) => handOff(`「${title}」에 대한 답: `)}
            onForget={async (id) => putAsks(await call({ op: "ask_forget", ids: [id], all: false }))}
            onOpenTask={(title) => {
              goSel("tasks:");
              setTaskFocus((f) => ({ seq: f.seq + 1, title }));
            }}
            onThread={(id) => {
              const t = allThreads.find((x) => x.id === id);
              if (t) chooseThread(t);
            }}
          />
        )}
        {tasksOpened && (
          <TasksView
            shown={tasksShown}
            board={taskBoard.board}
            error={taskBoard.error}
            groups={threadGroups}
            machines={machines}
            stages={new Map(allThreads.map(t => [t.id, t.stage]))}
            overLimits={new Map(allThreads.map(t => [t.id, t.over_limit]))}
            reader={role === "reader"}
            addRequest={taskAdds}
            focusRequest={taskFocus}
            target={taskAt}
            onTarget={(id) => goSel(id ? `task:${id}` : "tasks:")}
            onBoard={taskBoard.replace}
            onThread={(id) => {
              const t = allThreads.find((x) => x.id === id);
              if (t) chooseThread(t);
            }}
            onSeenMine={seenMine.see}
            fileOpen={fileViewing !== null}
            onHandOff={canControl ? handOff : undefined}
            onOpenAttachment={async (project, a) => {
              // task 첨부 문서(plan §8.327): 올린 md·html은 uploads 주소, 보고서 문서는 그 보고서 보기 경로 안에서 파일 보기로 연다
              if (a.kind === "upload") openFile({ path: a.target.replace(/^uploads\//, ""), pane: null, view: uploadsBase(project) });
              const doc = a.kind === "report" ? reportDoc(a.target) : null;
              if (!doc) return;
              const r = await call({ op: "library_open", project, name: doc.name });
              if (r.kind === "library") openFile({ path: doc.path, pane: null, view: r.base });
            }}
          />
        )}
        {reportsShown || tasksShown || asksShown ? null : currentThread ? (
          <ThreadPanel
            key={threadKey(currentThread)}
            t={currentThread}
            role={role}
            backTo={backTo === "threads:" ? "모든 thread" : undefined}
            onBack={() => (backTo === "threads:" ? chooseThreads() : choose(null))}
            onOpenPane={
              currentThread.pane && panes.some((p) => p.id === currentThread.pane && p.state === "running") ? () => choose(currentThread.pane!) : undefined
            }
            onOpenFile={(path) => openFile({ path, pane: null, library: { project: currentThread.project, thread: currentThread.id } })}
            onOpenReportFile={role !== "reader" ? (path) => openFile({ path, pane: null, worktree: { project: currentThread.project, thread: currentThread.id } }) : undefined}
            onAddDoc={addReportDoc && ((path) => addReportDoc(currentThread.id, path))}
          />
        ) : sel === "threads:" ? (
          <ProjectThreads
            threads={allThreads}
            onThread={(t) => chooseThread(t, "threads:")}
            onBack={() => choose(null)}
            onPane={canControl ? (id) => choose(id) : undefined}
          />
        ) : current ? (
          <TermView
            key={current.id}
            controls={paneControls}
            pane={current}
            role={role}
            autoSelected={autoSelected}
            onOpenFile={role !== "reader" ? (path) => openFile({ path, pane: current.id }) : undefined}
            onOpenLink={openLink}
            title={paneTitle(current)}
            agent={agentOf(current, threadOfPane.get(current.id))}
            crumb={`${current.machine} › ${paneSub(current)}`}
            status={
              <>
                <AgentChip p={current} />
                {/* thread pane이면 그 보고서로(목록의 thread 줄은 pane을 연다, 차이 목록 L4) */}
                {threadOfPane.get(current.id) && (
                  <button className="ghost small" data-testid="open-report" onClick={() => chooseThread(threadOfPane.get(current.id)!)}>
                    <Icon name="file" />
                    보고
                  </button>
                )}
                {/* 작업 요약(plan §8.259): 데스크톱 팝오버, 휴대폰 아래 시트 */}
                {threadOfPane.get(current.id) && <SummaryButton t={threadOfPane.get(current.id)!} />}
              </>
            }
            dock={
              currentQ.length > 0 || dockAsk ? (
                <div className="dock">
                  {currentQ.map((q) =>
                    q.menu ? (
                      // 창이 바뀌면 고른 것·쓴 글을 새 창에 넘기지 않는다(참고: 질문마다 새 카드)
                      <PromptCard
                        key={`${q.id} ${q.menu.id}`}
                        menu={q.menu}
                        where={`${whereOf(q)} · ${q.machine}`}
                        canControl={canControl}
                        onAnswer={(pick) => answerMenu(q, q.menu!, pick)}
                        testid={`dock-question-${q.id}`}
                      />
                    ) : (
                      <QuestionCard key={q.id} q={q} where={whereOf(q)} canControl={canControl} onPick={answer} docked />
                    ),
                  )}
                  {/* 그 thread 보고의 질문(plan §8.329, 사용자 결정 10-20 4A): 입력칸 위 한 줄 칩, 누르면 질문 카드 */}
                  {dockAsk && (
                    <div className="dock-ref" data-testid="dock-question">
                      <AppRefChip at={{ kind: "question", target: threadOfPane.get(current.id)!.id }} />
                    </div>
                  )}
                </div>
              ) : null
            }
          />
        ) : (
          <>
          {/* 보일 것이 없을 때의 머리: 좁은 창의 목록 단추·머리 알림·「더 보기」 */}
          <header className="term-head empty-head">
            <DrawerToggle />
            <HeadNote />
            <HeadMore />
          </header>
          <div className="empty-view">
            <img className="logo big" src="/icon.svg" alt="" />
            {/* 휴대폰(768px 이하)에는 왼쪽 칸이 없고 목록은 drawer다: 목록을 가리키고 여는 단추를 둔다(plan §8.301) */}
            <h2>{open.length > 0 ? `답을 기다리는 질문 ${open.length}개` : wideScreen ? "왼쪽에서 하나를 고른다" : "목록에서 하나를 고른다"}</h2>
            <p>
              pane이나 thread를 고르면 터미널을 본다. 질문은 목록에 빨간 ?로 보이고, 그 pane을 열면 답할 수 있다.
            </p>
            {!wideScreen && (
              <button type="button" className="btn btn-primary empty-open-list" data-testid="empty-open-list" onClick={() => setDrawerOpen(true)}>
                <Menu aria-hidden="true" />
                목록 열기
              </button>
            )}
          </div>
          </>
        )}
        </ErrorBoundary>
      </main>
      <ErrorBoundary area="파일 보기" resetKeys={[fileViewing?.path, fileViewing?.pane]} onDismiss={closeFile}>
      {fileViewing?.pane && (
        <FileViewer key={`${fileViewing.pane} ${fileViewing.path}`} path={fileViewing.path} source={{ pane: fileViewing.pane }} onClose={closeFile} onOpen={(path) => openFile({ path, pane: fileViewing.pane })} />
      )}
      {/* thread 결과물(차이 목록 R1): 고른 다른 파일도 그 결과물 안이다 */}
      {fileViewing && fileViewing.pane === null && fileViewing.library && (
        <FileViewer
          key={`${fileViewing.library.project}/${fileViewing.library.thread} ${fileViewing.path}`}
          path={fileViewing.path}
          source={fileViewing.library}
          onClose={closeFile}
          onOpen={(path) => openFile({ path, pane: null, library: fileViewing.library })}
        />
      )}
      {/* 보고서 안 파일 링크(plan §8.171): thread의 머신·작업 폴더 기준. 고른 다른 파일도 같은 곳이다 */}
      {fileViewing && fileViewing.pane === null && fileViewing.worktree && (
        <FileViewer
          key={`${fileViewing.worktree.project}/${fileViewing.worktree.thread} worktree ${fileViewing.path}`}
          path={fileViewing.path}
          source={{ worktree: fileViewing.worktree }}
          onClose={closeFile}
          onOpen={(path) => openFile({ path, pane: null, worktree: fileViewing.worktree })}
        />
      )}
      {/* 대화의 보고서 주소(plan §8.300): 보기 경로 안. 고른 다른 파일도 같은 보기 경로 안이다 */}
      {fileViewing && fileViewing.pane === null && fileViewing.view && (
        <FileViewer
          key={`${fileViewing.view} ${fileViewing.path}`}
          path={fileViewing.path}
          source={{ view: fileViewing.view }}
          onClose={closeFile}
          onOpen={(path) => openFile({ path, pane: null, view: fileViewing.view })}
        />
      )}
      </ErrorBoundary>
      {newPaneOn && (
        <NewSessionDialog
          machine={newPaneOn}
          onClose={() => setNewPaneOn(null)}
          onCreated={(id) => {
            setNewPaneOn(null);
            choose(id);
          }}
        />
      )}
      <ErrorBoundary area="명령 팔레트" resetKeys={[paletteOpen]} onDismiss={closePalette}>
      <CommandPalette
        open={paletteOpen}
        onClose={closePalette}
        machines={machines}
        panes={panes}
        threads={shownThreads}
        selected={sel}
        agentOf={agentOf}
        onOpen={(target) => {
          if ("pane" in target) choose(target.pane);
          else chooseThread(target.thread);
          reveal("pane" in target ? `pane:${target.pane}` : `thread:${threadKey(target.thread)}`);
        }}
        actions={paletteActions}
      />
      </ErrorBoundary>
      {/* 앱 안 알림(참고 Droplet): 누르면 그 pane·thread로 간다 */}
      <Droplet
        onOpen={(target) => {
          if (target === "deployment") {
            setPanel("machines");
            setCollapsed(false);
            if (!wideScreen) setDrawerOpen(true);
          } else location.hash = target;
        }}
      />
    </div>
    </AppRefContext.Provider>
    </HeadContext.Provider>
    </DrawerContext.Provider>
  );
}

/**
 * 머리의 「더 보기」 메뉴(참고 App.tsx moreItems·header-more): 링크 복사와 「알림」(이 기기 상태를 말하고 누르면 켜고 끈다).
 * 알림이 꺼져 있으면 메뉴 단추에 점을 단다. 찾기·파일 보기 항목은 그 기능을 옮길 때 넣는다.
 */
function MoreMenu({ onThreads }: { onThreads: () => void }) {
  const alerts = useAlerts();
  const [open, setOpen] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const box = useRef<HTMLSpanElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  // 닫기와 키(herdr-web-ui RowMenu.tsx 104~165줄): 열면 첫 항목에 focus(키보드 사건이 메뉴로 온다), Esc는 window에서
  // 먼저 받아 닫고, 바깥 누름·focus가 메뉴 밖으로 나가면 닫는다. 위·아래·Home·End로 옮기고 Tab은 닫는다. 닫히면 ⋯로 돌아간다
  const close = useCallback(() => {
    setOpen(false);
    if (menu.current?.contains(document.activeElement)) button.current?.focus({ preventScroll: true });
  }, []);
  useEffect(() => {
    if (!open) return;
    window.requestAnimationFrame(() => menu.current?.querySelector<HTMLElement>("[role^=menuitem]")?.focus());
    const away = (e: Event) => {
      if (!box.current?.contains(e.target as Node)) setOpen(false);
    };
    const esc = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.stopPropagation();
      e.preventDefault();
      close();
    };
    window.addEventListener("pointerdown", away, true);
    window.addEventListener("keydown", esc, true);
    return () => {
      window.removeEventListener("pointerdown", away, true);
      window.removeEventListener("keydown", esc, true);
    };
  }, [open, close]);
  const onMenuKey = (e: React.KeyboardEvent<HTMLDivElement>) => {
    const items = [...(menu.current?.querySelectorAll<HTMLElement>("[role^=menuitem]") ?? [])];
    const at = items.indexOf(document.activeElement as HTMLElement);
    const move = (next: number) => {
      e.preventDefault();
      items[(next + items.length) % items.length]?.focus();
    };
    if (e.key === "ArrowDown") move(at + 1);
    else if (e.key === "ArrowUp") move(at - 1);
    else if (e.key === "Home") move(0);
    else if (e.key === "End") move(items.length - 1);
    else if (e.key === "Tab") {
      e.preventDefault();
      close();
    }
  };
  const onMenuBlur = (e: React.FocusEvent<HTMLDivElement>) => {
    const next = e.relatedTarget as Node | null;
    if (next && box.current?.contains(next)) return;
    // Mac Safari는 누른 단추에 focus를 주지 않아 relatedTarget이 없다: 누름은 pointerdown이 판단한다
    if (next) setOpen(false);
  };
  useEffect(() => {
    if (!note) return;
    const t = window.setTimeout(() => setNote(null), 2000);
    return () => window.clearTimeout(t);
  }, [note]);
  const label = alerts.offDot ? "더 보기 · 알림 꺼짐" : "더 보기";
  return (
    <span className="more" ref={box}>
      <button
        ref={button}
        className="ghost icon"
        data-testid="more"
        aria-label={label}
        title={label}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
      >
        <Icon name="more" />
      </button>
      {alerts.offDot && <span className="more-dot" aria-hidden="true" />}
      {note && (
        <span className="more-note" role="status">
          {note}
        </span>
      )}
      {open && (
        <div className="more-menu" role="menu" aria-label="더 보기" ref={menu} onKeyDown={onMenuKey} onBlur={onMenuBlur}>
          <button
            role="menuitem"
            onClick={() => {
              setOpen(false);
              void navigator.clipboard?.writeText(location.href).then(
                () => setNote("링크를 복사했습니다"),
                () => undefined,
              );
            }}
          >
            <Icon name="link" />
            <span className="more-label">링크 복사</span>
          </button>
          {alerts.visible && (
            <button
              role="menuitemcheckbox"
              aria-checked={alerts.bell.on}
              data-testid="alerts-item"
              title={alerts.bell.title}
              onClick={() => {
                setOpen(false);
                void alerts.bell.run();
              }}
            >
              <Icon name="bell" />
              <span className="more-label">알림</span>
              <span className="more-hint">{alerts.bell.state}</span>
            </button>
          )}
          {/* 모든 thread(끝난 것까지): herdr-projects popup(prefix+a)을 여는 자리. 참고 moreItems에 없는 항목이라 맨 끝 */}
          <button
            role="menuitem"
            data-testid="threads-item"
            onClick={() => {
              setOpen(false);
              onThreads();
            }}
          >
            <ListChecks className="i" aria-hidden="true" />
            <span className="more-label">모든 thread</span>
          </button>
        </div>
      )}
    </span>
  );
}

/**
 * 활동 막대 맨 아래 설정 톱니(사용자 요청 10-19 「설정도 거기에 넣어줘. 왼쪽 밑에.」, VS Code Manage 자리, plan §8.240)와 설정 창.
 * 예전에는 목록 바닥 줄의 「설정」 단추였다. 단축키·팔레트도 같은 창을 연다(Main이 열림을 갖는다)
 */
function SettingsGear({
  role,
  device,
  machines,
  onSignedOut: signedOut,
  open: settingsOpen,
  onOpen: setSettingsOpen,
}: {
  role: DeviceRole;
  device: string;
  machines: MachineView[];
  onSignedOut: () => void;
  open: boolean;
  onOpen: (open: boolean) => void;
}) {
  const closeSettings = useCallback(() => setSettingsOpen(false), [setSettingsOpen]);
  // 설정은 history 안에 있다(herdr/settingsHistory.ts, 참고 App.tsx 398~405줄): 마지막 칸에서 「뒤로」면 닫히고,
  // 「앞으로」면 다시 열리며, 다른 길(X·Esc·바깥)로 닫으면 그 칸들을 지운다
  useEffect(
    () =>
      onSettingsHistory((entry, own) => {
        if (own) return;
        if (entry === null) closeSettings();
        else setSettingsOpen(true);
      }),
    [closeSettings, setSettingsOpen],
  );
  useEffect(() => {
    if (!settingsOpen) recordSettings([]);
  }, [settingsOpen]);
  // 설정 → 기기 「로그아웃」(참고 App.tsx `lock`): 구독을 먼저 지운다(cookie가 있어야 지워지고, 잠긴 기기에 알림이 가지 않게). 답이 오다 끊겨도 host에 다시 묻는다
  const signOutHere = useCallback(async () => {
    await removePushSubscription().catch(() => undefined);
    try {
      await signOut();
    } catch {
      throw new Error("로그아웃하지 못했습니다. 다시 시도해 주세요.");
    } finally {
      signedOut();
    }
  }, [signedOut]);
  return (
    <>
      <button
        type="button"
        className="activity-item"
        data-testid="settings-open"
        aria-label="설정"
        aria-haspopup="dialog"
        aria-expanded={settingsOpen}
        title="설정"
        onClick={() => setSettingsOpen(true)}
      >
        <Settings aria-hidden="true" />
      </button>
      {settingsOpen && (
        <ErrorBoundary area="설정" onDismiss={closeSettings}>
          <SettingsDialog onClose={closeSettings} role={role} device={device} machines={machines} onSignOut={signOutHere} />
        </ErrorBoundary>
      )}
    </>
  );
}

/** 사이드바 바닥 줄(모든 패널 공통): 구독 사용량, 오른쪽 끝에 붙는다(사용자 요청 10-19 「사용량은 오른쪽으로 붙여」).
 * 설정·보고서는 활동 막대에, 앱 설치(PWA)는 설정 「폰 및 기기」에, 알림은 머리 「더 보기」 메뉴에, 입력 기록(실기 키보드 진단)은 설정 「진단」에 있다 */
function ListFoot() {
  return (
    <div className="list-foot">
      {/* 구독 사용량(herdr-web-ui MachineSidebar sidebar-footer-row. 그 옆 설정은 활동 막대 톱니로 옮겼다) */}
      <div className="foot-row">
        <UsageMeters />
      </div>
    </div>
  );
}

