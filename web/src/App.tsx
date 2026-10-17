import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import type {
  AgentActivity,
  DeviceRole,
  EvaluatorView,
  MachineView,
  PaneView,
  QuestionView,
  ThreadView,
} from "./gen/proto";
import { ApiError, call, me, pair, pushInfo, watchEvents } from "./api";
import { TermView } from "./Term";
import { GROUP_LABEL, ThreadPanel } from "./ThreadPanel";
import { Icon } from "./icons";
import { AgentMark, hasAgentMark } from "./herdr/AgentMark.tsx";
import { UsageMeters } from "./UsageMeters";
import { SettingsDialog } from "./SettingsDialog";
import { setThemePref, useScheme, type ThemePref } from "./theme";
import { useInstallPrompt } from "./herdr/install";
import { useKeepScreenOn } from "./settings";
import { useScreenWakeLock } from "./herdr/wakeLock.ts";
import { raiseAlert, useAlerts, useAlertSoundUnlock } from "./alerts";
import { Droplet } from "./herdr/Droplet.tsx";

type Auth = { state: "loading" } | { state: "pair" } | { state: "in"; role: DeviceRole } | { state: "error"; msg: string };

export function App() {
  const [auth, setAuth] = useState<Auth>({ state: "loading" });
  useEffect(() => {
    me()
      .then((m) => setAuth(m ? { state: "in", role: m.role } : { state: "pair" }))
      .catch((e: unknown) => setAuth({ state: "error", msg: String(e) }));
  }, []);
  if (auth.state === "loading") return <p className="center muted">불러오는 중</p>;
  if (auth.state === "error") return <p className="center err">{auth.msg}</p>;
  if (auth.state === "pair") return <Pair onDone={(role) => setAuth({ state: "in", role })} />;
  return <Main role={auth.role} />;
}

function Pair({ onDone }: { onDone: (r: DeviceRole) => void }) {
  const [code, setCode] = useState("");
  const [msg, setMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  return (
    <div className="pair-wrap">
      <main className="pair">
        <img className="logo big" src="/icon.svg" alt="" />
        <h1>이 기기 연결하기</h1>
        <p>
          host 머신에서 <code>aaa web pair</code>를 실행하고 나온 코드를 넣는다. 한 번 연결하면 이 브라우저는 다시 묻지 않는다.
        </p>
        <form
          onSubmit={async (e) => {
            e.preventDefault();
            setBusy(true);
            setMsg(null);
            try {
              const r = await pair(code.trim());
              onDone(r.role);
            } catch (err) {
              setMsg(err instanceof ApiError ? err.message : String(err));
            } finally {
              setBusy(false);
            }
          }}
        >
          <input
            value={code}
            onChange={(e) => setCode(e.target.value)}
            placeholder="pairing 코드"
            aria-label="pairing 코드"
            autoComplete="one-time-code"
            autoCapitalize="characters"
            spellCheck={false}
            autoFocus
          />
          <button type="submit" className="primary" disabled={busy || code.trim() === ""}>
            연결
          </button>
        </form>
        {msg && <p className="err">{msg}</p>}
        <p className="foot muted">
          보기만 할 기기는 <code>aaa web pair --reader</code>로 만든 코드를 쓴다.
        </p>
      </main>
    </div>
  );
}

/** 「실행」은 평소 상태라 적지 않는다. 끝났거나 끊긴 것만 보인다. */
function paneBadge(p: PaneView): { text: string; cls: string } | null {
  if (p.state === "running" && p.connection === "disconnected") return { text: "실행·끊김(읽기 전용)", cls: "warn" };
  if (p.state === "running") return null;
  if (p.state === "exited") return { text: p.code == null ? "종료" : `종료(${p.code})`, cls: "" };
  return { text: p.state === "interrupted" ? "중단" : "유실", cls: "warn" };
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
  onPick: (q: QuestionView, c: string) => void;
  docked?: boolean;
}) {
  const { tool, body } = splitQuestion(q.text);
  return (
    <div className="question" data-testid={docked ? `dock-question-${q.id}` : `question-${q.id}`}>
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
            <button key={o} className={o === "yes" ? "primary" : ""} onClick={() => onPick(q, o)}>
              {docked && <span className="key">{n + 1}</span>}
              {CHOICE_LABEL[o] ?? o}
            </button>
          ))}
        </div>
      ) : (
        <p className="muted small-text">보기 권한이라 답할 수 없다</p>
      )}
    </div>
  );
}

/** 고른 것: `#pane=<id>` 또는 `#thread=<project>/<id>` */
function selectedFromHash(): string | null {
  const m = /^#(pane|thread)=(.+)$/.exec(location.hash);
  return m ? `${m[1]}:${decodeURIComponent(m[2])}` : null;
}

const threadKey = (t: ThreadView) => `${t.project}/${t.id}`;

/** 홈 폴더를 ~로 줄인다(화면에 사용자 이름을 늘어놓지 않는다) */
const shortPath = (p: string) => p.replace(/^\/(Users|home)\/[^/]+/, "~");

/** 목록 앞 표시: harness 종류 */
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

/** 목록 줄 앞 표시(herdr-web-ui Sidebar.tsx `rowMark`·AgentSidebar `AgentRowBody`): 그 줄이 여는 pane의 agent, 셸이면 터미널 */
function RowMark({ agent }: { agent: string | null }) {
  return (
    <span className="mark" aria-hidden="true" data-agent={agent ?? "shell"}>
      {agent !== null ? <AgentMark agent={agent} size={18} /> : <Icon name="shell" />}
    </span>
  );
}

function ThemeButton() {
  const [scheme, pref] = useScheme();
  const next: Record<ThemePref, ThemePref> = { system: scheme === "dark" ? "light" : "dark", light: "dark", dark: "system" };
  const label: Record<ThemePref, string> = { system: "시스템", light: "밝음", dark: "어두움" };
  return (
    <button
      className="ghost icon theme-btn"
      onClick={() => setThemePref(next[pref])}
      aria-label={`테마: ${label[pref]}`}
      title={`테마: ${label[pref]} (누르면 ${label[next[pref]]})`}
    >
      <Icon name={pref === "system" ? "auto" : pref === "light" ? "sun" : "moon"} />
    </button>
  );
}

function Section({ title, count, alert, children, testid }: { title: string; count?: number; alert?: boolean; children: ReactNode; testid?: string }) {
  return (
    <section className={`sec ${testid ?? ""}`} data-testid={testid}>
      <h2 className={`sec-head ${alert ? "alert" : ""}`}>
        {title}
        {count != null && <span className="count">{count}</span>}
      </h2>
      {children}
    </section>
  );
}

function Main({ role }: { role: DeviceRole }) {
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

  // 이 기기의 알림(plan §8.70): host push와 같은 사건, 같은 종류(`notify.kinds`)에만 띄운다. 질문이 열리면 「입력 필요」,
  // thread에 확인하지 않은 새 보고가 오면 「작업 완료」. 연결할 때 읽은 목록(처음 본 것)은 새 소식이 아니다.
  const alertKinds = useRef<Set<string> | null>(null);
  const seenQuestions = useRef(new Set<string>());
  const seenReports = useRef(new Map<string, string | null>());
  const now = useRef({ sel, panes, threads, machines });
  now.current = { sel, panes, threads, machines };
  const questionNews = (q: QuestionView) => {
    if (q.state !== "open" || seenQuestions.current.has(q.id)) return;
    seenQuestions.current.add(q.id);
    if (!alertKinds.current?.has("question")) return;
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
  const reportNews = (t: ThreadView) => {
    const key = threadKey(t);
    const before = seenReports.current.get(key);
    seenReports.current.set(key, t.report_hash);
    const fresh = before !== undefined && t.report_hash !== null && t.report_hash !== before && t.report_hash !== t.acked_hash;
    if (!fresh || t.role === "evaluator" || !alertKinds.current?.has("report")) return;
    const { sel, panes } = now.current;
    raiseAlert({
      kind: "done",
      target: `thread=${encodeURIComponent(key)}`,
      showing: sel === `thread:${key}`,
      title: t.title || t.id,
      machine: null,
      agent: agentOf(t.pane ? panes.find((x) => x.id === t.pane) : undefined, t),
      notice: { title: `${t.project} · ${t.title || t.id}`, body: t.report_state ?? "보고 준비", tag: `aaa-${t.project}/${t.id}` },
    });
  };

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
                for (const q of r.questions) seenQuestions.current.add(q.id);
                setQuestions(new Map(r.questions.map((q) => [q.id, q])));
              })
              .catch(() => {});
          if (on) loadEvaluators();
          if (on)
            call({ op: "threads", project: null, role: null })
              .then((r) => {
                if (r.kind !== "threads") return;
                for (const t of r.threads) seenReports.current.set(threadKey(t), t.report_hash);
                setThreads(new Map(r.threads.map((t) => [threadKey(t), t])));
              })
              .catch(() => {});
        },
        (t) => {
          reportNews(t);
          putThread(t);
        },
      ),
    [],
  );
  useAlertSoundUnlock();

  const answer = async (q: QuestionView, choice: string) => {
    setMsg(null);
    try {
      const r = await call({ op: "question_answer", id: q.id, choice });
      if (r.kind === "question") putQuestion(r);
    } catch (e) {
      setMsg(e instanceof ApiError ? e.message : String(e));
    }
  };
  useEffect(() => {
    const on = () => setSel(selectedFromHash());
    window.addEventListener("hashchange", on);
    return () => window.removeEventListener("hashchange", on);
  }, []);

  const choose = (id: string | null) => {
    location.hash = id ? `pane=${encodeURIComponent(id)}` : "";
    setSel(id ? `pane:${id}` : null);
  };
  const chooseThread = (t: ThreadView) => {
    location.hash = `thread=${encodeURIComponent(threadKey(t))}`;
    setSel(`thread:${threadKey(t)}`);
  };

  const newShell = async (m: MachineView) => {
    setMsg(null);
    try {
      const r = await call({ op: "pane_run", machine: m.id, name: null, argv: [], cwd: null, cols: null, rows: null });
      if (r.kind === "pane") choose(r.id);
    } catch (e) {
      setMsg(e instanceof ApiError ? e.message : String(e));
    }
  };

  const current = panes.find((p) => `pane:${p.id}` === sel) ?? null;
  const currentThread = sel?.startsWith("thread:") ? (threads.get(sel.slice(7)) ?? null) : null;
  // 화면 켜짐 유지: pane이나 thread를 보는 동안(herdr-web-ui App.tsx 842줄, 보이는 동안만 건다)
  useScreenWakeLock(useKeepScreenOn() && (current !== null || currentThread !== null));
  const shownThreads = [...threads.values()]
    .filter((t) => t.group !== "resolved" && t.role !== "evaluator")
    .sort((a, b) => b.updated_at_ms - a.updated_at_ms);
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
  // thread 줄 표시: 그 thread pane의 agent. pane이 없으면(아직·끝남) profile로 본다
  const agentOfThread = (t: ThreadView) => agentOf(t.pane ? panes.find((x) => x.id === t.pane) : undefined, t);
  const paneTitle = (p: PaneView) => threadOfPane.get(p.id)?.title ?? (p.name || p.argv.join(" ") || p.id);
  const paneSub = (p: PaneView) => {
    const t = threadOfPane.get(p.id);
    if (t) return `${t.project}/${t.id} · ${t.profile}`;
    const parts = [p.agent?.harness, p.cwd ? shortPath(p.cwd) : p.argv.join(" ")].filter(Boolean);
    return parts.join(" · ") || p.id.slice(0, 8);
  };

  return (
    <div className={`app ${current || currentThread ? "has-sel" : ""}`}>
      <aside className="list">
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
          <ThemeButton />
          <MoreMenu />
        </header>
        {/* 목록만 굴러가고 머리와 바닥 줄(설정·사용량)은 늘 제자리(herdr-web-ui MachineSidebar: machine-list가 굴러가고 sidebar-footer는 맨 아래) */}
        <div className="list-body">
          {!up && loaded && <div className="banner warn">host와 연결이 끊겼다. 다시 연결하는 중</div>}
          {msg && <div className="banner err">{msg}</div>}
          {!loaded && <p className="muted pad">불러오는 중</p>}
          {open.length > 0 && (
            <Section title="답을 기다림" count={open.length} alert testid="questions">
              {open.map((q) => (
                <QuestionCard key={q.id} q={q} where={whereOf(q)} canControl={canControl} onPick={answer} />
              ))}
            </Section>
          )}
          {shownThreads.length > 0 && (
            <Section title="Thread" count={shownThreads.length} testid="threads">
              <ul>
                {shownThreads.map((t) => (
                  <li key={threadKey(t)}>
                    <ThreadRow
                      t={t}
                      agent={agentOfThread(t)}
                      sel={sel === `thread:${threadKey(t)}`}
                      onClick={() => chooseThread(t)}
                    />
                  </li>
                ))}
              </ul>
            </Section>
          )}
          {machines.map((m) => {
            const mine = panes.filter((p) => p.machine_id === m.id && p.role !== "evaluator");
            const on = m.connection === "connected";
            return (
              <section key={m.id} className="sec machine" data-testid={`machine-${m.name}`}>
                <div className="machine-head">
                  <Icon name="machine" />
                  <span className="name">{m.name}</span>
                  {m.local && <span className="tag">host</span>}
                  <span className={`dot ${on ? "on" : "off"}`} title={on ? "연결됨" : "끊김"} />
                  {!on && <span className="chip warn">끊김</span>}
                  {on && m.problems.length > 0 && (
                    <span className="chip warn" title={m.problems.map((p) => p.text).join("\n")} data-testid={`health-${m.name}`}>
                      점검 이상
                    </span>
                  )}
                  {canControl && on && (
                    <button className="ghost small" onClick={() => newShell(m)}>
                      <Icon name="plus" />
                      새 셸
                    </button>
                  )}
                </div>
                {on && m.problems.length > 0 && (
                  <p className="note">새 세션을 시작하지 않는다: {m.problems.map((p) => p.text).join("; ")}</p>
                )}
                {mine.length === 0 && <p className="empty">pane 없음</p>}
                <ul>
                  {mine.map((p) => {
                    const badge = paneBadge(p);
                    const t = threadOfPane.get(p.id);
                    return (
                      <li key={p.id}>
                        <button
                          className={`pane ${`pane:${p.id}` === sel ? "sel" : ""} ${p.state !== "running" ? "dim" : ""}`}
                          onClick={() => choose(p.id)}
                          data-testid={`pane-${p.name || p.id}`}
                        >
                          <RowMark agent={agentOf(p, t)} />
                          <span className="row-text">
                            <span className="row-title">{paneTitle(p)}</span>
                            <span className="row-sub">{paneSub(p)}</span>
                          </span>
                          <span className="row-end">
                            <AgentChip p={p} testid />
                            {badge && <span className={`chip ${badge.cls}`}>{badge.text}</span>}
                          </span>
                        </button>
                      </li>
                    );
                  })}
                </ul>
              </section>
            );
          })}
        </div>
        <ListFoot />
      </aside>
      <main className="view">
        {currentThread ? (
          <ThreadPanel key={threadKey(currentThread)} t={currentThread} role={role} onBack={() => choose(null)} />
        ) : current ? (
          <TermView
            key={current.id}
            pane={current}
            role={role}
            title={paneTitle(current)}
            agent={agentOf(current, threadOfPane.get(current.id))}
            crumb={`${current.machine} › ${paneSub(current)}`}
            status={<AgentChip p={current} />}
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
              Thread를 고르면 보고와 결과물을, pane을 고르면 터미널을 본다.
              <br />
              질문은 목록 맨 위에서 바로 답할 수 있다.
            </p>
          </div>
        )}
      </main>
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
function MoreMenu() {
  const alerts = useAlerts();
  const [open, setOpen] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const box = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    if (!open) return;
    const away = (e: Event) => {
      if (!box.current?.contains(e.target as Node)) setOpen(false);
    };
    const esc = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("pointerdown", away, true);
    document.addEventListener("keydown", esc);
    return () => {
      document.removeEventListener("pointerdown", away, true);
      document.removeEventListener("keydown", esc);
    };
  }, [open]);
  useEffect(() => {
    if (!note) return;
    const t = window.setTimeout(() => setNote(null), 2000);
    return () => window.clearTimeout(t);
  }, [note]);
  const label = alerts.offDot ? "더 보기 · 알림 꺼짐" : "더 보기";
  return (
    <span className="more" ref={box}>
      <button
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
        <div className="more-menu" role="menu">
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
        </div>
      )}
    </span>
  );
}

/** 목록 맨 아래: 홈 화면에 설치(PWA)와 설정. 설치 단추는 설치 전엔 늘 보인다(참고 MachineSidebar). 알림은 머리 「더 보기」 메뉴에,
 * 입력 기록(실기 키보드 진단)은 설정 「진단」에 있다(참고 목록에 그런 자리가 없다) */
function ListFoot() {
  const { canInstall, installed, install, help } = useInstallPrompt();
  const [installHelpOpen, setInstallHelpOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const closeSettings = useCallback(() => setSettingsOpen(false), []);
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
      {settingsOpen && <SettingsDialog onClose={closeSettings} />}
    </div>
  );
}

const GROUP_DOT: Record<string, string> = {
  working: "working",
  waiting_on_you: "blocked",
  ready_for_review: "done",
  idle: "",
  resolved: "",
};

function ThreadRow({ t, agent, sel, onClick }: { t: ThreadView; agent: string | null; sel: boolean; onClick: () => void }) {
  const pct = t.agent?.percent;
  return (
    <button className={`pane ${sel ? "sel" : ""}`} onClick={onClick} data-testid={`thread-${t.project}-${t.id}`}>
      {/* 줄이 여는 pane의 agent(herdr-web-ui Sidebar.tsx `rowMark`: 표시는 누르면 닿는 agent만 약속한다) */}
      <RowMark agent={agent} />
      <span className="row-text">
        <span className="row-title">{t.title}</span>
        <span className="row-sub">
          {t.project}/{t.id} · {t.machine}
        </span>
      </span>
      <span className="row-end">
        <span className={`chip ${t.group}`}>
          {t.group === "working" && <span className={`dot ${GROUP_DOT[t.group]}`} aria-hidden />}
          {GROUP_LABEL[t.group] ?? t.group}
          {t.group === "working" && pct != null && <span className="pct"> {pct}%</span>}
        </span>
      </span>
    </button>
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
