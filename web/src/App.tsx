import { useEffect, useState } from "react";
import type { AgentActivity, DeviceRole, MachineView, PaneView, QuestionView, ThreadView } from "./gen/proto";
import { ApiError, call, me, pair, watchEvents } from "./api";
import { TermView } from "./Term";
import { GROUP_LABEL, ThreadPanel } from "./ThreadPanel";

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
    <main className="pair">
      <h1>aa</h1>
      <p>
        이 기기를 연결하려면 host 머신에서 <code>aa web pair</code>를 실행하고 나온 코드를 넣는다.
        <br />
        보기만 할 기기는 <code>aa web pair --reader</code>로 만든 코드를 쓴다.
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
          autoFocus
        />
        <button type="submit" disabled={busy || code.trim() === ""}>
          연결
        </button>
      </form>
      {msg && <p className="err">{msg}</p>}
    </main>
  );
}

const STATE_LABEL: Record<PaneView["state"], string> = {
  running: "실행",
  exited: "종료",
  interrupted: "중단",
  lost: "유실",
};

function paneBadge(p: PaneView): string {
  if (p.state === "running" && p.connection === "disconnected") return "실행·끊김(읽기 전용)";
  if (p.state === "exited") return p.code == null ? "종료" : `종료(${p.code})`;
  return STATE_LABEL[p.state];
}

const ACTIVITY_LABEL: Record<AgentActivity, string> = {
  started: "시작",
  working: "작업 중",
  idle: "입력 대기",
  question: "질문",
  error: "오류",
  exited: "끝남",
};

/** harness 상태(hook 사건으로만 바뀐다). 진행률을 보고했으면 함께 보인다. */
function AgentBadge({ p }: { p: PaneView }) {
  const a = p.agent;
  if (!a || p.state !== "running") return null;
  const pct = a.percent != null ? ` ${a.percent}%` : "";
  const title = [a.harness, a.progress, a.message].filter(Boolean).join(" · ");
  return (
    <span className={`badge agent ${a.activity}`} title={title} data-testid={`agent-${p.name || p.id}`}>
      {ACTIVITY_LABEL[a.activity]}
      {pct}
    </span>
  );
}

const KIND_LABEL: Record<string, string> = { trust: "폴더 신뢰", permission: "권한 확인", confirm: "확인" };
const CHOICE_LABEL: Record<string, string> = { yes: "예", no: "아니오" };

/** 확인창 질문 카드. 답은 host가 질문이 아직 열려 있는지 다시 확인한 뒤 harness 키로 보낸다. */
function QuestionCard({
  q,
  paneName,
  canControl,
  onPick,
}: {
  q: QuestionView;
  paneName: string;
  canControl: boolean;
  onPick: (q: QuestionView, c: string) => void;
}) {
  const where = q.project && q.thread ? `${q.project}/${q.thread}` : paneName;
  return (
    <div className="question" data-testid={`question-${q.id}`}>
      <div className="q-head">
        <span className="tag warn">{KIND_LABEL[q.question_kind] ?? q.question_kind}</span>
        <span className="muted">
          {where} · {q.machine}
        </span>
      </div>
      <p className="q-text">{q.text}</p>
      {canControl ? (
        <div className="q-actions">
          {q.options.map((o) => (
            <button key={o} className={o === "yes" ? "primary" : ""} onClick={() => onPick(q, o)}>
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
  const putThread = (t: ThreadView) =>
    setThreads((old) => {
      const next = new Map(old);
      next.set(threadKey(t), t);
      return next;
    });

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
          if (on)
            call({ op: "threads", project: null })
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
    .filter((t) => t.group !== "resolved")
    .sort((a, b) => b.updated_at_ms - a.updated_at_ms);
  const canControl = role !== "reader";
  const open = [...questions.values()].sort((a, b) => a.created_at_ms - b.created_at_ms);
  const currentQ = current ? open.filter((q) => q.pane === current.id) : [];
  const nameOf = (id: string) => {
    const p = panes.find((x) => x.id === id);
    return p ? p.name || p.argv.join(" ") || id.slice(0, 8) : id.slice(0, 8);
  };

  return (
    <div className={`app ${current || currentThread ? "has-sel" : ""}`}>
      <aside className="list">
        <header className="list-head">
          <strong>aa</strong>
          <span className={`dot ${up ? "on" : "off"}`} title={up ? "host 연결됨" : "host 연결 끊김"} />
          <span className="muted">{role === "reader" ? "보기 권한" : "제어 권한"}</span>
        </header>
        {!up && loaded && <div className="banner warn">host와 연결이 끊겼다. 다시 연결하는 중</div>}
        {msg && <div className="banner err">{msg}</div>}
        {!loaded && <p className="muted pad">불러오는 중</p>}
        {open.length > 0 && (
          <section className="questions" data-testid="questions">
            <h2>답을 기다리는 질문 {open.length}</h2>
            {open.map((q) => (
              <QuestionCard key={q.id} q={q} paneName={nameOf(q.pane)} canControl={canControl} onPick={answer} />
            ))}
          </section>
        )}
        {shownThreads.length > 0 && (
          <section className="threads" data-testid="threads">
            <h2>thread {shownThreads.length}</h2>
            <ul>
              {shownThreads.map((t) => (
                <li key={threadKey(t)}>
                  <button
                    className={`pane ${sel === `thread:${threadKey(t)}` ? "sel" : ""}`}
                    onClick={() => chooseThread(t)}
                    data-testid={`thread-${t.project}-${t.id}`}
                  >
                    <span className="pane-name">
                      {t.title} <span className="muted">{t.project}/{t.id}</span>
                    </span>
                    <span className={`badge group ${t.group}`}>{GROUP_LABEL[t.group] ?? t.group}</span>
                  </button>
                </li>
              ))}
            </ul>
          </section>
        )}
        {machines.map((m) => {
          const mine = panes.filter((p) => p.machine_id === m.id);
          return (
            <section key={m.id} className="machine" data-testid={`machine-${m.name}`}>
              <h2>
                <span className={`dot ${m.connection === "connected" ? "on" : "off"}`} />
                {m.name}
                {m.local && <span className="tag">host</span>}
                {m.connection === "disconnected" && <span className="tag warn">끊김</span>}
                {m.connection === "connected" && m.problems.length > 0 && (
                  <span className="tag warn" title={m.problems.join("\n")} data-testid={`health-${m.name}`}>
                    점검 이상
                  </span>
                )}
                {canControl && m.connection === "connected" && (
                  <button className="small" onClick={() => newShell(m)}>
                    새 셸
                  </button>
                )}
              </h2>
              {m.connection === "connected" && m.problems.length > 0 && (
                <p className="muted pad warn-text">새 세션을 시작하지 않는다: {m.problems.join("; ")}</p>
              )}
              {mine.length === 0 && <p className="muted pad">pane 없음</p>}
              <ul>
                {mine.map((p) => (
                  <li key={p.id}>
                    <button
                      className={`pane ${`pane:${p.id}` === sel ? "sel" : ""}`}
                      onClick={() => choose(p.id)}
                      data-testid={`pane-${p.name || p.id}`}
                    >
                      <span className="pane-name">{p.name || p.argv.join(" ") || p.id}</span>
                      <span className="badges">
                        <AgentBadge p={p} />
                        <span className={`badge ${p.state} ${p.connection}`}>{paneBadge(p)}</span>
                      </span>
                    </button>
                  </li>
                ))}
              </ul>
            </section>
          );
        })}
      </aside>
      <main className="view">
        {currentThread ? (
          <ThreadPanel key={threadKey(currentThread)} t={currentThread} role={role} onBack={() => choose(null)} />
        ) : current ? (
          <>
            {currentQ.map((q) => (
              <QuestionCard key={q.id} q={q} paneName={nameOf(q.pane)} canControl={canControl} onPick={answer} />
            ))}
            <TermView key={current.id} pane={current} role={role} onBack={() => choose(null)} />
          </>
        ) : (
          <p className="center muted">왼쪽에서 pane을 고른다</p>
        )}
      </main>
    </div>
  );
}
