import { useEffect, useState, type ReactNode } from "react";
import type {
  AgentActivity,
  DeviceRole,
  EvaluatorView,
  MachineView,
  PaneView,
  QuestionView,
  ThreadView,
} from "./gen/proto";
import { ApiError, call, me, pair, watchEvents } from "./api";
import { TermView } from "./Term";
import { GROUP_LABEL, ThreadPanel } from "./ThreadPanel";
import { Icon } from "./icons";
import { setThemePref, useScheme, type ThemePref } from "./theme";

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
        <span className="logo big">aa</span>
        <h1>이 기기 연결하기</h1>
        <p>
          host 머신에서 <code>aa web pair</code>를 실행하고 나온 코드를 넣는다. 한 번 연결하면 이 브라우저는 다시 묻지 않는다.
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
          보기만 할 기기는 <code>aa web pair --reader</code>로 만든 코드를 쓴다.
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
function harnessOf(p: PaneView, t?: ThreadView): string {
  const h = (p.agent?.harness ?? t?.profile ?? p.argv[0] ?? "").toLowerCase();
  if (h.includes("claude")) return "claude";
  if (h.includes("codex")) return "codex";
  if (p.agent || t) return "agent";
  return "shell";
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

  useEffect(
    () =>
      watchEvents(
        (ev) => {
          setMachines(ev.machines);
          setPanes(ev.panes);
          setLoaded(true);
        },
        putQuestion,
        (on) => {
          setUp(on);
          if (on)
            call({ op: "questions", all: false })
              .then((r) => {
                if (r.kind === "questions") setQuestions(new Map(r.questions.map((q) => [q.id, q])));
              })
              .catch(() => {});
          if (on) loadEvaluators();
          if (on)
            call({ op: "threads", project: null, role: null })
              .then((r) => {
                if (r.kind === "threads") setThreads(new Map(r.threads.map((t) => [threadKey(t), t])));
              })
              .catch(() => {});
        },
        putThread,
      ),
    [],
  );

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
            <span className="logo">aa</span>
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
        </header>
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
                  <ThreadRow t={t} sel={sel === `thread:${threadKey(t)}`} onClick={() => chooseThread(t)} />
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
                        <span className={`mark ${harnessOf(p, t)}`}>
                          <Icon name={harnessOf(p, t)} />
                        </span>
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
            <span className="logo big">aa</span>
            <h2>{open.length > 0 ? `답을 기다리는 질문 ${open.length}개` : "왼쪽에서 하나를 고른다"}</h2>
            <p>
              Thread를 고르면 보고와 결과물을, pane을 고르면 터미널을 본다.
              <br />
              질문은 목록 맨 위에서 바로 답할 수 있다.
            </p>
          </div>
        )}
      </main>
    </div>
  );
}

const GROUP_DOT: Record<string, string> = {
  working: "working",
  waiting_on_you: "blocked",
  ready_for_review: "done",
  resolved: "",
};

function ThreadRow({ t, sel, onClick }: { t: ThreadView; sel: boolean; onClick: () => void }) {
  const pct = t.agent?.percent;
  return (
    <button className={`pane ${sel ? "sel" : ""}`} onClick={onClick} data-testid={`thread-${t.project}-${t.id}`}>
      <span className="mark thread">
        <Icon name="thread" />
      </span>
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
