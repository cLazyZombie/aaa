// 보고서 화면 위의 「지금」 줄과 「답할 것」(plan §8.324, 사용자 결정 10-20 20:3x·1A~4A). thread가 보고 `## Questions`에 남긴
// 질문을 카드 하나로 답한다: 질문마다 고르고(추천 표시), 모자라면 「추천대로 보내기」, 직접 쓴 답은 고르지 않은 질문을 대신한다.
// 칩·알림으로 연 질문(`#question=<thread>`)만 펼치고 나머지는 한 줄이다(2A). 답은 host가 한 번만 받아 thread에 보내고 모든 기기가
// 「답함」을 본다. 카드 그리기는 화면 선택 창 카드(PromptCard, herdr-web-ui)의 꼴을 쓴다. 선택지 글자는 A·B·C(「1A 2B」로 답한다).
import { useEffect, useId, useRef, useState, type Ref } from "react";
import { ChevronRight, Send } from "lucide-react";
import { ApiError } from "./api";
import type { ThreadQuestions, ThreadView } from "./gen/proto";
import "./herdr/PromptCard.css";

const letter = (i: number) => String.fromCharCode(65 + i);
const at = (ms: number) => new Date(ms).toLocaleTimeString("ko-KR", { hour: "2-digit", minute: "2-digit" });

/** 답할 질문이 남은 thread인가 */
export const asks = (t: ThreadView): t is ThreadView & { questions: ThreadQuestions } => !!t.questions && t.questions.answer === null;

/** 고르지 않은 질문을 추천으로 채운다. 추천이 없는 질문이 남으면 null */
export function withRecommended(q: ThreadQuestions, picks: (number | null)[]): number[] | null {
  const out: number[] = [];
  for (const [n, item] of q.items.entries()) {
    const pick = picks[n] ?? item.choices.findIndex((c) => c.recommended);
    if (pick === null || pick < 0) return null;
    out.push(pick);
  }
  return out;
}

export function ThreadAsks({
  threads,
  focus,
  focusSeq,
  unseen,
  canControl,
  onAnswer,
}: {
  /** 끝나지 않은 thread(질문이 없는 것도: 「작업 중」 수) */
  threads: ThreadView[];
  /** 칩·알림으로 연 질문의 thread 번호: 그 카드만 펼친다 */
  focus: string | null;
  focusSeq: number;
  /** 안 본 사용자 보고서 수 */
  unseen: number;
  canControl: boolean;
  onAnswer: (t: ThreadView, picks: (number | null)[], text: string | null) => Promise<void>;
}) {
  const [open, setOpen] = useState<string | null>(focus);
  useEffect(() => setOpen(focus), [focus, focusSeq]);
  const focused = useRef<HTMLElement>(null);
  useEffect(() => {
    if (focus !== null) focused.current?.scrollIntoView({ block: "nearest" });
  }, [focus, focusSeq, open]);
  const waiting = threads.filter(asks);
  // 답한 질문은 다음 보고가 올 때까지 한 줄로 남는다(다른 기기에서 답한 것도 보인다)
  const answered = threads.filter((t) => t.questions?.answer);
  const working = threads.filter((t) => t.group === "working").length;
  const rows = [...waiting, ...answered];
  return (
    <section className="thread-asks" data-testid="thread-asks" aria-label="지금과 답할 것">
      <div className="thread-asks-now" data-testid="thread-asks-now">
        <span>작업 중 <b>{working}</b></span>
        <span className={waiting.length > 0 ? "is-ask" : undefined} data-testid="thread-asks-count">답할 것 <b>{waiting.length}</b></span>
        <span>안 본 보고 <b>{unseen}</b></span>
      </div>
      {rows.length > 0 && <h3 className="thread-asks-title">답할 것</h3>}
      {rows.map((t) => {
        const expanded = open === t.id;
        const ref = focus === t.id ? focused : undefined;
        if (!expanded)
          return (
            <button key={t.id} type="button" ref={ref as Ref<HTMLButtonElement>} className={`thread-ask-row${t.questions!.answer ? " is-done" : ""}`}
              data-testid={`thread-ask-${t.id}`} data-state={t.questions!.answer ? "answered" : "open"} aria-expanded="false" onClick={() => setOpen(t.id)}>
              <span className="report-tag report-thread">{t.id}</span>
              <span className="thread-ask-row-title">{t.title || t.id}</span>
              <span className="thread-ask-row-meta">{t.questions!.answer ? `답함 · ${t.questions!.answer.summary}` : `질문 ${t.questions!.items.length}개`}</span>
              <ChevronRight className="i" aria-hidden="true" />
            </button>
          );
        return (
          <AskCard key={`${t.id} ${t.questions!.report_hash}`} t={t} q={t.questions!} focusRef={ref} canControl={canControl}
            onAnswer={(picks, text) => onAnswer(t, picks, text)} onFold={() => setOpen(null)} />
        );
      })}
    </section>
  );
}

function AskCard({
  t,
  q,
  focusRef,
  canControl,
  onAnswer,
  onFold,
}: {
  t: ThreadView;
  q: ThreadQuestions;
  focusRef?: Ref<HTMLElement>;
  canControl: boolean;
  onAnswer: (picks: (number | null)[], text: string | null) => Promise<void>;
  onFold: () => void;
}) {
  const [picks, setPicks] = useState<(number | null)[]>(() => q.items.map(() => null));
  const [custom, setCustom] = useState("");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const shown = useRef(false);
  useEffect(() => {
    shown.current = true;
    return () => {
      shown.current = false;
    };
  }, []);
  const customId = useId();
  const where = `${t.id} · ${t.title || t.id}`;
  const a = q.answer;
  if (a) {
    return (
      <section ref={focusRef} className="prompt-card thread-ask-card is-answered" role="region" aria-label={`${t.id} 질문`} data-testid={`thread-ask-${t.id}`} data-state="answered">
        <header className="prompt-card-header">
          <h2>답함</h2>
          <span className="prompt-card-where">{where} · {at(a.at_ms)} {a.by}</span>
          <button type="button" className="ghost small thread-ask-fold" onClick={onFold}>접기</button>
        </header>
        <ul className="thread-ask-answers">
          {q.items.map((item, n) => {
            const pick = a.picks[n];
            return (
              <li key={n}>
                <span className="muted">{n + 1}. {item.text}</span> →{" "}
                <strong>{pick === null || pick === undefined ? "직접 쓴 답" : `${letter(pick)} ${item.choices[pick]?.label ?? ""}`}</strong>
              </li>
            );
          })}
          {a.text && <li><span className="muted">직접 쓴 답</span> <strong>{a.text}</strong></li>}
        </ul>
      </section>
    );
  }
  const off = pending || !canControl;
  const text = custom.trim();
  const got = picks.filter((p) => p !== null).length;
  const all = got === q.items.length;
  const filled = all ? null : withRecommended(q, picks);
  const send = async (p: (number | null)[]) => {
    setPending(true);
    setError(null);
    try {
      await onAnswer(p, text || null);
    } catch (e) {
      if (shown.current) setError(e instanceof ApiError ? e.message : String(e));
    }
    if (shown.current) setPending(false);
  };
  // 다 골랐거나 직접 쓴 답이 있으면 그대로, 아니면 남은 질문을 추천으로 채워 보낸다
  const submit = () => void (all || text ? send(picks) : filled && send(filled));
  const label = all ? `답 보내기 (${q.items.length})` : text ? "답 보내기" : filled ? "추천대로 보내기" : "답 보내기";
  return (
    <section ref={focusRef} className="prompt-card thread-ask-card" role="region" aria-label={`${t.id} 질문`} aria-busy={pending} data-testid={`thread-ask-${t.id}`} data-state="open">
      <header className="prompt-card-header">
        <h2>답할 질문 {q.items.length}</h2>
        <span className="prompt-card-where">{where}</span>
        <button type="button" className="ghost small thread-ask-fold" onClick={onFold}>접기</button>
      </header>
      {q.items.map((item, n) => (
        <div className="thread-ask-q" key={n} role="radiogroup" aria-label={`${n + 1}. ${item.text}`}>
          <p className="prompt-card-question"><span className="thread-ask-qno">{n + 1}</span>{item.text}</p>
          <div className="prompt-card-options">
            {item.choices.map((c, i) => {
              const checked = picks[n] === i;
              return (
                <button key={i} type="button" role="radio" aria-checked={checked} data-testid={`thread-ask-pick-${n + 1}${letter(i)}`}
                  className={`prompt-card-option${c.description ? " has-description" : ""}${checked ? " is-checked" : ""}`} disabled={off}
                  onClick={() => setPicks((ps) => ps.map((p, k) => (k === n ? (p === i ? null : i) : p)))}>
                  <span className="prompt-card-number"><span aria-hidden="true">{letter(i)}</span><span className="visually-hidden">{letter(i)}.</span></span>{" "}
                  <span className="prompt-card-option-text">
                    <span className="prompt-card-option-label">
                      {c.label}
                      {c.recommended && <>{" "}<span className="prompt-card-tag">추천</span></>}
                    </span>
                    {c.description && <span className="prompt-card-option-description">{c.description}</span>}
                  </span>
                </button>
              );
            })}
          </div>
        </div>
      ))}
      <div className="prompt-card-custom">
        <span className="prompt-card-custom-label" id={customId}>또는 직접 답을 입력(고르지 않은 질문을 대신한다)</span>
        <div className="prompt-card-custom-row">
          <input className="input" value={custom} disabled={off} placeholder="예: 2 둘 다 보이게" aria-labelledby={customId} data-testid="thread-ask-text"
            onChange={(e) => setCustom(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.nativeEvent.isComposing && e.nativeEvent.keyCode !== 229 && text.length > 0) submit();
            }} />
        </div>
      </div>
      <div className="thread-ask-foot">
        <span className="muted">{got}/{q.items.length} 고름{!all && filled ? " · 고르지 않은 것은 추천대로" : ""}</span>
        <button type="button" className={`btn${all || text ? " btn-primary" : ""}`} data-testid="thread-ask-send" disabled={off || !(all || text || filled)} onClick={submit}>
          <Send aria-hidden="true" /> {label}
        </button>
      </div>
      {!canControl && <p className="prompt-card-hint">보기 권한이라 답할 수 없다</p>}
      {error !== null && <p className="prompt-card-error" role="alert">{error}</p>}
    </section>
  );
}
