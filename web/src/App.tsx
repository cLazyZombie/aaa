import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type {
  AgentActivity,
  DeviceRole,
  EvaluatorView,
  MachineView,
  PaneView,
  QuestionView,
  ThreadView,
} from "./gen/proto";
import { ApiError, call, me, onSignedOut, pushInfo, signOut, watchEvents } from "./api";
import { removePushSubscription } from "./push";
import { AccessGate } from "./AccessGate";
import { takePairCode } from "./herdr/phone";
import { usePreferencesSync } from "./preferencesSync";
import { TermView } from "./Term";
import { ThreadPanel } from "./ThreadPanel";
import { ProjectThreads } from "./ProjectThreads";
import { Folder } from "lucide-react";
import { Icon } from "./icons";
import { hasAgentMark } from "./herdr/AgentMark.tsx";
import { Sidebar } from "./Sidebar";
import { machineRows, shortPath, threadKey } from "./sidebarModel";
import { useShortcuts } from "./herdr/shortcuts.ts";
import type { AppActions, PaneControls } from "./actions";
import { UsageMeters } from "./UsageMeters";
import { SettingsDialog } from "./SettingsDialog";
import { onSettingsHistory, recordSettings } from "./herdr/settingsHistory.ts";
import { useInstallPrompt } from "./herdr/install";
import { useKeepScreenOn } from "./settings";
import { useScreenWakeLock } from "./herdr/wakeLock.ts";
import { raiseAlert, useAlerts, useAlertSoundUnlock } from "./alerts";
import { Droplet } from "./herdr/Droplet.tsx";
import { NewSessionDialog } from "./NewSessionDialog";
import { CommandPalette, type PaletteActions } from "./CommandPalette";
import { updateAppearance, useAppearance } from "./theme";
import { passingFolder, recentDirectories } from "./herdr/recent-directories.ts";

type Auth = { state: "loading" } | { state: "pair" } | { state: "in"; role: DeviceRole; device: string } | { state: "error"; msg: string };

export function App() {
  const [auth, setAuth] = useState<Auth>({ state: "loading" });
  // QR 코드가 연 주소의 `?pair=CODE`: 읽자마자 주소에서 지운다(기록·referrer에 남지 않게, herdr-web-ui takePairCode)
  const [pairCode] = useState(() => takePairCode());
  const check = useCallback(() => {
    me()
      .then((m) => setAuth(m ? { state: "in", role: m.role, device: m.device } : { state: "pair" }))
      // fetch가 던진 TypeError는 네트워크에 닿지 못한 것이다(브라우저마다 말이 다르다: Failed to fetch·Load failed)
      .catch((e: unknown) => setAuth({ state: "error", msg: e instanceof TypeError ? "네트워크에 닿지 않는다." : String(e) }));
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
  const title = [a.harness, a.progress, a.message].filter(Boolean).join(" · ");
  const quiet = a.activity === "idle" || a.activity === "started" || a.activity === "exited";
  return (
    <span
      className={`chip agent ${a.activity} ${quiet ? "plain" : ""}`}
      title={title}
      data-testid={testid ? `agent-${p.name || p.id}` : undefined}
    >
      {a.activity === "working" && <span className="dot working" aria-hidden />}
      {ACTIVITY_LABEL[a.activity]}
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

/** 고른 것: `#pane=<id>`, `#thread=<project>/<id>`, 프로젝트 thread 보기 `#project=<name>`, 프로젝트 고르기 `#projects` */
function selectedFromHash(): string | null {
  if (location.hash === "#projects") return "projects:";
  const m = /^#(pane|thread|project)=(.+)$/.exec(location.hash);
  return m ? `${m[1]}:${decodeURIComponent(m[2])}` : null;
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
  const [panes, setPanes] = useState<PaneView[]>([]);
  const [up, setUp] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [sel, setSel] = useState<string | null>(selectedFromHash());
  const [msg, setMsg] = useState<string | null>(null);
  // 열린 질문(id → 질문). 연결할 때마다 목록을 새로 읽고, 이후는 질문 사건으로 따라간다.
  const [questions, setQuestions] = useState<Map<string, QuestionView>>(new Map());
  // thread(project/id → 기록). 연결할 때 목록을 읽고 thread 사건으로 따라간다.
  const [threads, setThreads] = useState<Map<string, ThreadView>>(new Map());
  // 사후 평가자: 목록에 섞지 않고 header badge로만 보인다(§8.13)
  const [evaluators, setEvaluators] = useState<EvaluatorView[]>([]);
  const loadEvaluators = () =>
    call({ op: "evaluator_status", project: null })
      .then((r) => {
        if (r.kind === "evaluators") setEvaluators(r.evaluators);
      })
      .catch(() => {});
  const putThread = (t: ThreadView) => {
    if (t.role === "evaluator") loadEvaluators();
    setThreads((old) => {
      const next = new Map(old);
      next.set(threadKey(t), t);
      return next;
    });
  };

  const putQuestion = (q: QuestionView) =>
    setQuestions((old) => {
      const next = new Map(old);
      if (q.state === "open") next.set(q.id, q);
      else next.delete(q.id);
      return next;
    });

  // 이 기기의 알림(plan §8.70): host가 알리는 종류(`notify.kinds`의 `thread-state`)에만 띄운다. 질문이 열리면 바로
  // 「입력 필요」(host push는 30초 넘게 뜬 확인창만 보낸다), thread가 검토 대기가 된 새 보고는 「작업 완료」(host 「new report」
  // 항목과 같은 규칙: 검토 대기일 때 보고 hash당 한 번, 작업 중에 고쳐 쓴 보고는 아니다, plan §8.85).
  // 연결할 때 읽은 목록(처음 본 것)은 새 소식이 아니다.
  const alertKinds = useRef<Set<string> | null>(null);
  const seenQuestions = useRef(new Set<string>());
  const seenReports = useRef(new Map<string, string | null>());
  // 이 화면이 한 번이라도 붙었었나: 다시 붙을 때 읽은 목록은 끊긴 동안 놓친 사건이다(아래)
  const linkedOnce = useRef({ questions: false, threads: false });
  const now = useRef({ sel, panes, threads, machines, role });
  now.current = { sel, panes, threads, machines, role };
  const questionNews = (q: QuestionView) => {
    if (q.state !== "open" || seenQuestions.current.has(q.id)) return;
    seenQuestions.current.add(q.id);
    if (!alertKinds.current?.has("thread-state")) return;
    const { sel, panes, threads, machines } = now.current;
    const pane = panes.find((p) => p.id === q.pane);
    const thread = q.project && q.thread ? threads.get(`${q.project}/${q.thread}`) : undefined;
    const name = thread?.title || (pane ? pane.name || pane.argv.join(" ") || q.pane.slice(0, 8) : q.pane.slice(0, 8));
    raiseAlert({
      kind: "blocked",
      target: `pane=${encodeURIComponent(q.pane)}`,
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
  };
  // seenReports: thread마다 알린(또는 처음 볼 때 이미 검토 대기였던) 보고 hash
  const reviewHash = (t: ThreadView) => (t.group === "ready_for_review" ? t.report_hash : null);
  const reportNews = (t: ThreadView) => {
    const key = threadKey(t);
    if (!seenReports.current.has(key)) {
      seenReports.current.set(key, reviewHash(t));
      return;
    }
    const hash = reviewHash(t);
    if (hash === null || hash === seenReports.current.get(key)) return;
    seenReports.current.set(key, hash);
    if (t.role === "evaluator" || !alertKinds.current?.has("thread-state")) return;
    const { sel, panes } = now.current;
    raiseAlert({
      kind: "done",
      target: `thread=${encodeURIComponent(key)}`,
      showing: sel === `thread:${key}`,
      title: t.title || t.id,
      machine: null,
      agent: agentOf(t.pane ? panes.find((x) => x.id === t.pane) : undefined, t),
      notice: { title: `${t.project} · ${t.title || t.id}`, body: "보고 준비", tag: `aaa-${t.project}/${t.id}` },
    });
  };

  // 공유 설정과 고정(herdr-web-ui App.tsx usePreferencesSync, plan §8.79): 들어온 뒤에만 읽고, events 사건으로 따라간다
  const syncPreferences = usePreferencesSync(true);
  const receivePreferences = useRef(syncPreferences);
  receivePreferences.current = syncPreferences;
  useEffect(
    () =>
      watchEvents(
        (ev) => {
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
          if (on)
            pushInfo()
              .then((i) => (alertKinds.current = new Set(i.kinds)))
              .catch(() => {});
          if (on)
            call({ op: "questions", all: false })
              .then((r) => {
                if (r.kind !== "questions") return;
                // 처음 붙을 때 읽은 것은 새 소식이 아니다. 다시 붙을 때는 끊긴 동안 열린 질문을 알린다(t3code(MIT)
                // packages/client-runtime/src/state/threads.ts `afterSequence`처럼 놓친 사건을 잇는다, plan §8.84)
                if (linkedOnce.current.questions) for (const q of r.questions) questionNews(q);
                else for (const q of r.questions) seenQuestions.current.add(q.id);
                linkedOnce.current.questions = true;
                setQuestions(new Map(r.questions.map((q) => [q.id, q])));
              })
              .catch(() => {});
          if (on) loadEvaluators();
          if (on)
            call({ op: "threads", project: null, role: null })
              .then((r) => {
                if (r.kind !== "threads") return;
                // 위 질문과 같다: 다시 붙을 때는 끊긴 동안 검토 대기가 된 새 보고를 알린다(처음 보는 thread는 새 소식이 아니다)
                // 끊긴 동안 새로 생긴 thread도 처음 붙을 때 목록에 없었으니 새 소식이다
                if (linkedOnce.current.threads)
                  for (const t of r.threads) {
                    if (!seenReports.current.has(threadKey(t))) seenReports.current.set(threadKey(t), null);
                    reportNews(t);
                  }
                else for (const t of r.threads) seenReports.current.set(threadKey(t), reviewHash(t));
                linkedOnce.current.threads = true;
                setThreads(new Map(r.threads.map((t) => [threadKey(t), t])));
              })
              .catch(() => {});
        },
        (t) => {
          reportNews(t);
          putThread(t);
        },
        (p) => receivePreferences.current(p),
      ),
    [],
  );
  useAlertSoundUnlock();

  // 실패는 그 카드가 보인다(QuestionCard): 목록 머리 띠는 휴대폰에서 pane을 보는 동안 숨는다
  const answer = async (q: QuestionView, choice: string) => {
    const r = await call({ op: "question_answer", id: q.id, choice });
    if (r.kind === "question") putQuestion(r);
  };
  // 프로젝트 thread 보기에서 연 보고는 「뒤로」가 그 프로젝트로 돌아간다(herdr-projects popup: detail에서 Esc는 목록으로).
  // 그 thread를 연 주소 바꿈이 아닌 다른 이동(앱 안 알림·브라우저 뒤로·직접 친 주소)이면 돌아갈 곳을 잊는다
  const [backTo, setBackTo] = useState<string | null>(null);
  const openedHash = useRef<string | null>(null);
  useEffect(() => {
    const on = () => {
      if (location.hash !== openedHash.current) setBackTo(null);
      openedHash.current = null;
      setSel(selectedFromHash());
    };
    window.addEventListener("hashchange", on);
    return () => window.removeEventListener("hashchange", on);
  }, []);

  const choose = (id: string | null) => {
    location.hash = id ? `pane=${encodeURIComponent(id)}` : "";
    setSel(id ? `pane:${id}` : null);
  };
  const chooseThread = (t: ThreadView, from: string | null = null) => {
    setBackTo(from);
    openedHash.current = `#thread=${encodeURIComponent(threadKey(t))}`;
    location.hash = openedHash.current;
    setSel(`thread:${threadKey(t)}`);
  };
  const chooseProject = (name: string | null) => {
    location.hash = name === null ? "projects" : `project=${encodeURIComponent(name)}`;
    setSel(name === null ? "projects:" : `project:${name}`);
  };

  // 새 pane 창(herdr-web-ui App.tsx `newSessionOpen`·`newSessionMachineId`): 머신 머리 +와 단축키가 그 머신으로 연다
  const [newPaneOn, setNewPaneOn] = useState<MachineView | null>(null);
  // 명령 팔레트(herdr-web-ui App.tsx 365~367줄 `paletteOpen`, plan §8.132)
  const [paletteOpen, setPaletteOpen] = useState(false);
  const closePalette = useCallback(() => setPaletteOpen(false), []);
  const { resolvedTheme } = useAppearance();
  const newShell = (m: MachineView) => {
    setMsg(null);
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
        // 고른 줄의 머신 안에서, 목록 차례로, 끝에서 처음으로(참고는 고른 PC의 snapshot pane 차례). 끊긴 머신의 줄은 열 수 없다
        const { sel, panes, threads, machines } = now.current;
        const shown = [...threads.values()].filter((t) => t.group !== "resolved" && t.role !== "evaluator");
        const groups = machines.filter((m) => m.connection === "connected").map((m) => machineRows(m, panes, shown));
        const rows = sel === null ? groups.find((g) => g.length > 0) : groups.find((g) => g.some((r) => r.keys.includes(sel)));
        if (!rows || rows.length === 0) return;
        const index = rows.findIndex((r) => sel !== null && r.keys.includes(sel));
        const next = rows[(index + direction + rows.length) % rows.length];
        if (!next) return;
        if ("pane" in next.open) choose(next.open.pane);
        else chooseThread(next.open.thread);
      },
      toggleView: () => paneControls.current?.toggleView(),
      focusInput: () => paneControls.current?.focusInput(),
      scrollPage: (direction) => paneControls.current?.scrollPage(direction),
      openSettings: () => setSettingsOpen(true),
      openPalette: () => setPaletteOpen(true),
      openFind: none,
      toggleSidebar: none,
      openNewSession: () => {
        // 참고는 고른 PC로 연다: 고른 pane·thread의 머신, 없으면 붙은 첫 머신. 보기 권한 기기는 띄울 수 없다
        const { sel, panes, threads, machines, role } = now.current;
        if (role === "reader") return;
        const pane = sel?.startsWith("pane:") ? panes.find((p) => `pane:${p.id}` === sel) : undefined;
        const thread = sel?.startsWith("thread:") ? threads.get(sel.slice(7)) : undefined;
        const onId = pane?.machine_id ?? (thread?.pane ? panes.find((p) => p.id === thread.pane)?.machine_id : undefined);
        const connected = machines.filter((m) => m.connection === "connected");
        const m = connected.find((x) => x.id === onId) ?? connected[0];
        if (m) setNewPaneOn(m);
      },
      openNewTab: none,
      splitPane: null,
    };
    // choose·chooseThread는 주소와 setSel만 쓴다
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  useShortcuts(actions, true);

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
  // 목록은 열린 thread만(herdr-projects sidebar.rs), 프로젝트 thread 보기는 끝난 것까지(popup.rs Section::Threads)
  const allThreads = [...threads.values()].filter((t) => t.role !== "evaluator");
  const shownThreads = allThreads.filter((t) => t.group !== "resolved");
  const viewProject = sel === "projects:" ? null : sel?.startsWith("project:") ? sel.slice(8) : undefined;
  // pane → 그 pane에서 도는 thread(목록에서 pane 이름 대신 thread 제목을 보인다)
  const threadOfPane = new Map<string, ThreadView>();
  for (const t of threads.values()) if (t.pane) threadOfPane.set(t.pane, t);
  const canControl = role !== "reader";
  const open = [...questions.values()].sort((a, b) => a.created_at_ms - b.created_at_ms);
  const currentQ = current ? open.filter((q) => q.pane === current.id) : [];
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
    toggleTheme: () => updateAppearance({ theme: resolvedTheme === "dark" ? "light" : "dark" }),
    openSettings: () => setSettingsOpen(true),
    openProjects: () => chooseProject(null),
  };

  return (
    <div className={`app ${current || currentThread || viewProject !== undefined ? "has-sel" : ""}`}>
      <aside className="list sidebar-shell">
        <header className="list-head">
          <span className="brand">
            <img className="logo" src="/icon.svg" alt="aaa" />
          </span>
          <span className="conn" title={up ? "host 연결됨" : "host 연결 끊김"}>
            <span className={`dot ${up ? "on" : "off"}`} />
            {role === "reader" ? "보기 권한" : "제어 권한"}
          </span>
          <span className="spacer" />
          {evaluators.map((v) => (
            <EvaluatorBadge key={v.project} v={v} pane={panes.find((p) => p.id === v.thread.pane)} onOpen={choose} />
          ))}
          <button
            className="ghost icon palette-button"
            data-testid="palette-open"
            aria-label="명령 팔레트"
            title="명령 팔레트 (⌘⇧K)"
            onClick={() => setPaletteOpen(true)}
          >
            <Icon name="search" />
          </button>
          <MoreMenu onProjects={() => chooseProject(null)} />
        </header>
        {/* 목록(machine-list)만 굴러가고 머리와 바닥 줄(설정·사용량)은 늘 제자리(herdr-web-ui MachineSidebar) */}
        <Sidebar
          machines={machines}
          panes={panes}
          threads={shownThreads}
          selected={sel}
          canControl={canControl}
          agentOf={agentOf}
          onPane={(id) => choose(id)}
          onThread={(t) => chooseThread(t)}
          onNewShell={newShell}
          onProject={chooseProject}
          banners={
            <>
              {!up && loaded && <div className="banner warn">host와 연결이 끊겼다. 다시 연결하는 중</div>}
              {msg && <div className="banner err">{msg}</div>}
              {!loaded && <p className="tree-state">불러오는 중</p>}
            </>
          }
        />
        <ListFoot role={role} device={device} onSignedOut={signedOut} settingsOpen={settingsOpen} onSettings={setSettingsOpen} />
      </aside>
      <main className="view">
        {currentThread ? (
          <ThreadPanel
            key={threadKey(currentThread)}
            t={currentThread}
            role={role}
            backTo={backTo?.startsWith("project:") ? backTo.slice(8) : undefined}
            onBack={() => (backTo !== null && backTo.startsWith("project:") ? chooseProject(backTo.slice(8)) : choose(null))}
            onOpenPane={
              currentThread.pane && panes.some((p) => p.id === currentThread.pane && p.state === "running") ? () => choose(currentThread.pane!) : undefined
            }
          />
        ) : viewProject !== undefined ? (
          <ProjectThreads
            project={viewProject}
            threads={allThreads}
            onProject={chooseProject}
            onThread={(t) => chooseThread(t, `project:${t.project}`)}
            onBack={() => choose(null)}
          />
        ) : current ? (
          <TermView
            key={current.id}
            controls={paneControls}
            pane={current}
            role={role}
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
              </>
            }
            dock={
              currentQ.length > 0 ? (
                <div className="dock">
                  {currentQ.map((q) => (
                    <QuestionCard key={q.id} q={q} where={whereOf(q)} canControl={canControl} onPick={answer} docked />
                  ))}
                </div>
              ) : null
            }
            onBack={() => choose(null)}
          />
        ) : (
          <div className="empty-view">
            <img className="logo big" src="/icon.svg" alt="" />
            <h2>{open.length > 0 ? `답을 기다리는 질문 ${open.length}개` : "왼쪽에서 하나를 고른다"}</h2>
            <p>
              pane이나 thread를 고르면 터미널을 본다. 질문은 목록에 빨간 ?로 보이고, 그 pane을 열면 답할 수 있다.
            </p>
          </div>
        )}
      </main>
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
      <CommandPalette
        open={paletteOpen}
        onClose={closePalette}
        machines={machines}
        panes={panes}
        threads={shownThreads}
        selected={sel}
        agentOf={agentOf}
        onOpen={(target) => ("pane" in target ? choose(target.pane) : chooseThread(target.thread))}
        actions={paletteActions}
      />
      {/* 앱 안 알림(참고 Droplet): 누르면 그 pane·thread로 간다 */}
      <Droplet
        onOpen={(target) => {
          location.hash = target;
        }}
      />
    </div>
  );
}

/**
 * 머리의 「더 보기」 메뉴(참고 App.tsx moreItems·header-more): 링크 복사와 「알림」(이 기기 상태를 말하고 누르면 켜고 끈다).
 * 알림이 꺼져 있으면 메뉴 단추에 점을 단다. 찾기·파일 보기 항목은 그 기능을 옮길 때 넣는다.
 */
function MoreMenu({ onProjects }: { onProjects: () => void }) {
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
          {/* 프로젝트마다 모든 thread(끝난 것까지): herdr-projects popup(prefix+a)을 여는 자리. 참고 moreItems에 없는 항목이라 맨 끝 */}
          <button
            role="menuitem"
            data-testid="projects-item"
            onClick={() => {
              setOpen(false);
              onProjects();
            }}
          >
            <Folder className="i" aria-hidden="true" />
            <span className="more-label">프로젝트</span>
          </button>
        </div>
      )}
    </span>
  );
}

/** 목록 맨 아래: 홈 화면에 설치(PWA)와 설정. 설치 단추는 설치 전엔 늘 보인다(참고 MachineSidebar). 알림은 머리 「더 보기」 메뉴에,
 * 입력 기록(실기 키보드 진단)은 설정 「진단」에 있다(참고 목록에 그런 자리가 없다) */
function ListFoot({
  role,
  device,
  onSignedOut: signedOut,
  settingsOpen,
  onSettings: setSettingsOpen,
}: {
  role: DeviceRole;
  device: string;
  onSignedOut: () => void;
  /** 설정 창(단축키도 연다, Main이 갖는다) */
  settingsOpen: boolean;
  onSettings: (open: boolean) => void;
}) {
  const { canInstall, installed, install, help } = useInstallPrompt();
  const [installHelpOpen, setInstallHelpOpen] = useState(false);
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
    <div className="list-foot">
      {/* 참고 MachineSidebar 41~43줄: 설치 신호가 없는 브라우저(iOS, HTTP)는 누르면 방법을 펼친다(문구는 참고 i18n.ko.ts) */}
      {!installed && (
        <button
          className="ghost"
          data-testid="install-app"
          aria-expanded={canInstall ? undefined : installHelpOpen}
          onClick={() => {
            if (canInstall) void install();
            else setInstallHelpOpen(!installHelpOpen);
          }}
        >
          <Icon name="download" />
          앱 설치
        </button>
      )}
      {!installed && !canInstall && installHelpOpen && (
        <p className="foot-msg" role="status" data-testid="install-help">
          {help}
        </p>
      )}
      {/* 설정과 그 옆 구독 사용량(herdr-web-ui MachineSidebar sidebar-footer-row) */}
      <div className="foot-row">
        <button className="ghost foot-settings" data-testid="settings-open" onClick={() => setSettingsOpen(true)}>
          <Icon name="settings" />
          설정
        </button>
        <UsageMeters />
      </div>
      {settingsOpen && <SettingsDialog onClose={closeSettings} role={role} device={device} onSignOut={signOutHere} />}
    </div>
  );
}

/** 사후 평가자 표시: 동작 상태와 정하지 않은 개선안 수. 누르면 평가자 pane을 연다(닫아도 배경에서 계속 돈다). */
function EvaluatorBadge({ v, pane, onOpen }: { v: EvaluatorView; pane?: PaneView; onOpen: (id: string | null) => void }) {
  const running = v.running && pane !== undefined;
  const act = pane?.agent?.activity;
  const state = !running ? "ended" : act === "working" ? "working" : "idle";
  const label = state === "ended" ? "세션 끝남" : state === "working" ? "평가 중" : "대기";
  return (
    <button
      className={`eval-badge ${state}`}
      data-testid={`evaluator-${v.project}`}
      title={`${v.project} 사후 평가자 ${v.thread.id} · ${label} · 받은 평가 ${v.evaluations} · 정하지 않은 개선안 ${v.undecided}`}
      disabled={!v.thread.pane}
      onClick={() => v.thread.pane && onOpen(v.thread.pane)}
    >
      평가 <span className="eval-dot">●</span> {v.undecided}
    </button>
  );
}
