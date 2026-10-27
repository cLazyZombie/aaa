import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { Zap } from "lucide-react";
import { call } from "./api";
import type { AutoDecision, AutoMode, FrontResponse } from "./gen/proto";
import type { AsksData } from "./asksModel";
import { AskMarkdown } from "./AsksView";
import "./AutoMode.css";

type Value = {
  mode: AutoMode | null; decisions: AutoDecision[]; exceptions: number; canControl: boolean; busy: boolean;
  error: string | null; change: (on: boolean) => Promise<void>; open: (id?: string) => void;
  review: (id: string, answer: string | null) => Promise<void>;
};
const Context = createContext<Value | null>(null);
export const useAutoMode = () => useContext(Context);
export function AutoModeProvider({ data, put, canControl, onPick, children }: {
  data: AsksData | null; put: (r: FrontResponse) => void; canControl: boolean; onPick: (key: string) => void; children: ReactNode;
}) {
  const mode = data?.autoMode ?? null;
  const decisions = data?.autoDecisions ?? [];
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const previous = useRef<AutoMode | null>(null);
  const [summary, setSummary] = useState<AutoMode | null>(null);
  useEffect(() => {
    if (previous.current?.on && mode && !mode.on && mode.run === previous.current.run) setSummary(mode);
    if (mode?.on) setSummary(null);
    else if (mode) setSummary(old => old?.run === mode.run ? mode : old);
    previous.current = mode;
  }, [mode]);
  const open = (id?: string) => onPick(`a:${id ?? decisions[0]?.id ?? ""}`);
  const change = async (on: boolean) => {
    if (busy || !canControl) return;
    setBusy(true); setError(null);
    try { put(await call({ op: "auto_mode_set", project: "", on })); }
    catch (e) { setError(e instanceof Error ? e.message : String(e)); }
    finally { setBusy(false); }
  };
  const review = async (id: string, answer: string | null) => {
    put(await call({ op: "auto_decision_review", project: "", id, answer }));
  };
  return <Context.Provider value={{ mode, decisions, exceptions: data?.autoExceptions ?? 0, canControl, busy, error, change, open, review }}>
    {children}
    {error && <div className="auto-summary auto-error" role="alert">{error}<button aria-label="auto mode 오류 닫기" onClick={() => setError(null)}>×</button></div>}
    {summary && <div className="auto-summary" role="status" data-testid="auto-off-summary">
      <strong>auto mode를 껐다</strong>
      <span>{Math.floor(Math.max(0, summary.ended_ms - summary.since_ms) / 60000)}분 동안 {summary.decisions}개를 정했다</span>
      <button onClick={() => { open(); setSummary(null); }}>보기</button>
      <button aria-label="auto mode 요약 닫기" onClick={() => setSummary(null)}>×</button>
    </div>}
  </Context.Provider>;
}
export function AutoModeMenu({ close }: { close: () => void }) {
  const auto = useAutoMode();
  if (!auto?.mode || !auto.canControl) return null;
  return <button role="menuitem" data-testid="auto-mode-menu" disabled={auto.busy} onClick={() => { close(); void auto.change(!auto.mode!.on); }}>
    <Zap aria-hidden="true" /><span className="more-label">auto mode {auto.mode.on ? "끄기" : "켜기"}
      {auto.mode.on && <small>{`${new Date(auto.mode.since_ms).toLocaleTimeString("ko-KR", { hour: "2-digit", minute: "2-digit" })}부터 · 자동 결정 ${auto.mode.decisions}`}</small>}
    </span>
  </button>;
}
export function AutoModeChip() {
  return useAutoMode()?.mode?.on ? <span className="auto-chip" data-testid="auto-chip">⏵⏵ AUTO</span> : null;
}
export function AutoModeSwitch() {
  const auto = useAutoMode();
  if (!auto?.mode?.on) return null;
  return <button type="button" role="switch" aria-checked="true" aria-label="auto mode 끄기" className="auto-switch" data-testid="auto-mode-switch"
    disabled={!auto.canControl || auto.busy} onClick={() => void auto.change(false)}>AUTO <span /></button>;
}
export function AutoModeStatus() {
  const auto = useAutoMode();
  if (!auto) return null;
  return <>
    {auto.mode?.on && <div className="auto-status" data-testid="auto-status">
      <strong className="auto-status-wide">⏵⏵ auto mode on</strong><strong className="auto-status-phone">⏵⏵ auto on</strong>
      <span className="auto-status-wide"> · 질문은 coordinator가 정함</span>
      <span> · </span><button onClick={() => auto.open()}>자동 결정 {auto.mode.decisions}</button>
      {auto.exceptions > 0 && <span className="auto-exceptions"> · 예외 {auto.exceptions} 기다림</span>}
      {auto.canControl && <><span> · </span><button disabled={auto.busy} onClick={() => void auto.change(false)}>눌러서 끄기</button></>}
    </div>}
  </>;
}
export function recommendation(d: AutoDecision): string {
  if (!d.question.choices.some((c) => c.recommended)) return "추천 없음";
  return d.pick !== null && d.question.choices[d.pick]?.recommended ? "추천과 같음" : "추천과 다름";
}
export function AutoDecisionLines() {
  const auto = useAutoMode();
  const rows = auto?.decisions.filter((d) => d.outcome === "answered").slice(0, 20).reverse() ?? [];
  return rows.length > 0 && <div className="auto-lines" aria-label="자동 결정 기록">
    {rows.map((d) => <button key={d.id} className="auto-line" onClick={() => auto?.open(d.id)}>
      <Zap aria-hidden="true" /><span><strong>자동으로 정함</strong> · {d.question.id} {d.question.text} → <strong>{d.choice}</strong> <span className="muted">({recommendation(d)} · {d.reason})</span></span>
    </button>)}
  </div>;
}
export function AutoDecisionCard({ decision: d }: { decision: AutoDecision }) {
  const auto = useAutoMode();
  const [editing, setEditing] = useState(false);
  const [answer, setAnswer] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const submit = async (value: string | null) => {
    setBusy(true); setError(null);
    try { await auto?.review(d.id, value); setEditing(false); }
    catch (e) { setError(e instanceof Error ? e.message : String(e)); }
    finally { setBusy(false); }
  };
  return <section className="prompt-card ask-card auto-card" data-testid="auto-decision-card">
    <div className="auto-card-head"><span className="auto-decision-label"><Zap /> 자동 결정</span><span>{d.subject} · {new Date(d.at_ms).toLocaleString("ko-KR")} · {d.decided_by}</span></div>
    <h3>{d.question.id} {d.question.text}</h3>
    {d.question.body && <AskMarkdown base={d.files_base}>{d.question.body}</AskMarkdown>}
    <div className="auto-choices">{d.question.choices.map((c, n) => <div key={n} className={n === d.pick ? "is-chosen" : ""}>
      <span>{c.letter ?? String.fromCharCode(65 + n)}</span><span>{c.label}{c.description && <small>{c.description}</small>}{c.body && <AskMarkdown base={d.files_base}>{c.body}</AskMarkdown>}</span>
      {c.recommended && <span className="badge">추천</span>}{n === d.pick && <strong>coordinator가 고름</strong>}
    </div>)}</div>
    {d.pick === null && <p><strong>고른 답:</strong> {d.choice}</p>}
    {d.question.after && <AskMarkdown base={d.files_base}>{d.question.after}</AskMarkdown>}
    <div className="auto-reason"><strong>까닭</strong> — {d.reason}</div>
    <p className="muted">진행함: {d.outcome === "pending" ? "답 전달 여부 확인 필요 · 자동 재전송 안 함" : d.outcome === "failed" ? "답 전달 실패" : d.action} · {d.reversible === null ? "되돌릴 수 있는지 coordinator 확인 필요" : d.reversible ? "되돌릴 수 있음" : "되돌릴 수 없음"}</p>
    {d.error && <p role="alert">{d.error}</p>}
    {d.reconsideration && <p>다시 정한 답: {d.reconsideration} · coordinator에 전달함</p>}
    {auto?.canControl && <div className="auto-card-actions">
      <button className="btn btn-primary" disabled={busy || d.reviewed} onClick={() => void submit(null)}>{d.reviewed ? "확인함" : "괜찮음"}</button>
      <button className="btn" disabled={busy} onClick={() => setEditing(!editing)}>다른 답으로 다시 정하기</button>
    </div>}
    {editing && <form className="auto-reconsider" onSubmit={(e) => { e.preventDefault(); void submit(answer.trim()); }}>
      <p>이미 진행한 작업은 자동으로 되돌리지 않는다.</p>
      <label>새 답<textarea value={answer} onChange={(e) => setAnswer(e.target.value)} required maxLength={16384} /></label>
      <button className="btn btn-primary" disabled={busy || !answer.trim()}>새 답 보내기</button>
    </form>}
    {error && <p role="alert">{error}</p>}
  </section>;
}
