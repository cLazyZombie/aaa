import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { Check, ChevronDown, Trash2, Zap } from "lucide-react";
import { ConfirmDialog } from "./ConfirmDialog";
import { call } from "./api";
import type { AutoDecision, AutoMode, FrontResponse } from "./gen/proto";
import type { AsksData } from "./asksModel";
import { AskMarkdown } from "./AsksView";
import "./AutoMode.css";

type Value = {
  mode: AutoMode | null; decisions: AutoDecision[]; exceptions: number; canControl: boolean; busy: boolean;
  error: string | null; change: (on: boolean) => Promise<void>; open: (id?: string) => void;
  review: (id: string, answer: string | null) => Promise<void>;
  /** host 원장에서 지운다(모든 기기). `all`이면 모두 */
  forget: (ids: string[], all: boolean) => Promise<void>;
};
const Context = createContext<Value | null>(null);
export const useAutoMode = () => useContext(Context);
export function AutoModeProvider({ data, put, forget, canControl, onPick, children }: {
  data: AsksData | null; put: (r: FrontResponse) => void; forget: Value["forget"]; canControl: boolean; onPick: (key: string) => void; children: ReactNode;
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
  return <Context.Provider value={{ mode, decisions, exceptions: data?.autoExceptions ?? 0, canControl, busy, error, change, open, review, forget }}>
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
// 대화창에는 확인하지 않은 자동 결정만 접은 한 줄로 둔다(사용자 10-28 「대화창에 계속 뜨는건」). 대화 기록에는 시각이 없어
// 결정한 자리에 끼우지 못한다. 펼치면 줄마다 열기·확인·지우기다. 질문 화면과 같은 host 원장을 읽어 지우면 함께 빠진다.
export function AutoDecisionLines() {
  const auto = useAutoMode();
  const [expanded, setExpanded] = useState(false);
  const [confirm, setConfirm] = useState<AutoDecision | null>(null);
  const [error, setError] = useState<string | null>(null);
  const rows = auto?.decisions.filter((d) => d.outcome === "answered" && !d.reviewed) ?? [];
  if (!auto || rows.length === 0) return null;
  const review = (id: string) => {
    setError(null);
    void auto.review(id, null).catch((e) => setError(e instanceof Error ? e.message : String(e)));
  };
  return <div className="auto-lines" data-testid="auto-lines" aria-label="자동 결정 기록">
    <button type="button" className="auto-lines-toggle" data-testid="auto-lines-toggle" aria-expanded={expanded} onClick={() => setExpanded(!expanded)}>
      <Zap aria-hidden="true" /><span><strong>자동으로 정함</strong> · 확인 안 함 {rows.length}</span><ChevronDown aria-hidden="true" className="auto-lines-chevron" />
    </button>
    {expanded && <div className="auto-lines-list">{rows.map((d) => <div key={d.id} className="auto-line" data-testid="auto-line" data-id={d.id}>
      <button type="button" className="auto-line-open" title={`${d.question.text} → ${d.choice}`} onClick={() => auto.open(d.id)}>
        <span>{d.question.id && <>{d.question.id} </>}{d.question.text} → <strong>{d.choice}</strong> <span className="muted">({recommendation(d)})</span></span>
      </button>
      {auto.canControl && <>
        <button type="button" className="icon-button" aria-label="확인" title="확인" data-testid="auto-line-review" onClick={() => review(d.id)}><Check aria-hidden="true" /></button>
        <button type="button" className="icon-button" aria-label="지우기" title="지우기" data-testid="auto-line-forget" onClick={() => setConfirm(d)}><Trash2 aria-hidden="true" /></button>
      </>}
    </div>)}</div>}
    {error && <p role="alert">{error}</p>}
    {confirm && <ConfirmDialog title="자동 결정 지우기" body={`「${confirm.question.text}」 자동 결정 기록을 모든 기기에서 지운다. 이미 진행한 일은 되돌리지 않는다.`}
      confirmLabel="지우기" onConfirm={() => auto.forget([confirm.id], false)} onClose={() => setConfirm(null)} />}
  </div>;
}
export function AutoDecisionCard({ decision: d }: { decision: AutoDecision }) {
  const auto = useAutoMode();
  const [editing, setEditing] = useState(false);
  const [answer, setAnswer] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirm, setConfirm] = useState(false);
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
      <button className="ghost small ask-forget" data-testid="auto-decision-forget" disabled={busy} onClick={() => setConfirm(true)}><Trash2 aria-hidden="true" /> 지우기</button>
    </div>}
    {confirm && auto && <ConfirmDialog title="자동 결정 지우기" body={`「${d.question.text}」 자동 결정 기록과 보존한 원문·그림을 모든 기기에서 지운다. 이미 진행한 일은 되돌리지 않는다.`}
      confirmLabel="지우기" onConfirm={() => auto.forget([d.id], false)} onClose={() => setConfirm(false)} />}
    {editing && <form className="auto-reconsider" onSubmit={(e) => { e.preventDefault(); void submit(answer.trim()); }}>
      <p>이미 진행한 작업은 자동으로 되돌리지 않는다.</p>
      <label>새 답<textarea value={answer} onChange={(e) => setAnswer(e.target.value)} required maxLength={16384} /></label>
      <button className="btn btn-primary" disabled={busy || !answer.trim()}>새 답 보내기</button>
    </form>}
    {error && <p role="alert">{error}</p>}
  </section>;
}
