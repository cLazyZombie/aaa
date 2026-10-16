import { useEffect, useState } from "react";
import type { DeviceRole, MachineView, PaneView } from "./gen/proto";
import { ApiError, call, me, pair, watchState } from "./api";
import { TermView } from "./Term";

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

function selectedFromHash(): string | null {
  const m = /^#pane=(.+)$/.exec(location.hash);
  return m ? decodeURIComponent(m[1]) : null;
}

function Main({ role }: { role: DeviceRole }) {
  const [machines, setMachines] = useState<MachineView[]>([]);
  const [panes, setPanes] = useState<PaneView[]>([]);
  const [up, setUp] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [sel, setSel] = useState<string | null>(selectedFromHash());
  const [msg, setMsg] = useState<string | null>(null);

  useEffect(
    () =>
      watchState(
        (ev) => {
          setMachines(ev.machines);
          setPanes(ev.panes);
          setLoaded(true);
        },
        setUp,
      ),
    [],
  );
  useEffect(() => {
    const on = () => setSel(selectedFromHash());
    window.addEventListener("hashchange", on);
    return () => window.removeEventListener("hashchange", on);
  }, []);

  const choose = (id: string | null) => {
    location.hash = id ? `pane=${encodeURIComponent(id)}` : "";
    setSel(id);
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

  const current = panes.find((p) => p.id === sel) ?? null;
  const canControl = role !== "reader";

  return (
    <div className={`app ${current ? "has-sel" : ""}`}>
      <aside className="list">
        <header className="list-head">
          <strong>aa</strong>
          <span className={`dot ${up ? "on" : "off"}`} title={up ? "host 연결됨" : "host 연결 끊김"} />
          <span className="muted">{role === "reader" ? "보기 권한" : "제어 권한"}</span>
        </header>
        {!up && loaded && <div className="banner warn">host와 연결이 끊겼다. 다시 연결하는 중</div>}
        {msg && <div className="banner err">{msg}</div>}
        {!loaded && <p className="muted pad">불러오는 중</p>}
        {machines.map((m) => {
          const mine = panes.filter((p) => p.machine_id === m.id);
          return (
            <section key={m.id} className="machine" data-testid={`machine-${m.name}`}>
              <h2>
                <span className={`dot ${m.connection === "connected" ? "on" : "off"}`} />
                {m.name}
                {m.local && <span className="tag">host</span>}
                {m.connection === "disconnected" && <span className="tag warn">끊김</span>}
                {canControl && m.connection === "connected" && (
                  <button className="small" onClick={() => newShell(m)}>
                    새 셸
                  </button>
                )}
              </h2>
              {mine.length === 0 && <p className="muted pad">pane 없음</p>}
              <ul>
                {mine.map((p) => (
                  <li key={p.id}>
                    <button
                      className={`pane ${p.id === sel ? "sel" : ""}`}
                      onClick={() => choose(p.id)}
                      data-testid={`pane-${p.name || p.id}`}
                    >
                      <span className="pane-name">{p.name || p.argv.join(" ") || p.id}</span>
                      <span className={`badge ${p.state} ${p.connection}`}>{paneBadge(p)}</span>
                    </button>
                  </li>
                ))}
              </ul>
            </section>
          );
        })}
      </aside>
      <main className="view">
        {current ? (
          <TermView key={current.id} pane={current} role={role} onBack={() => choose(null)} />
        ) : (
          <p className="center muted">왼쪽에서 pane을 고른다</p>
        )}
      </main>
    </div>
  );
}
