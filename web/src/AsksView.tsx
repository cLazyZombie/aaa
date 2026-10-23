import { ignoredFailure } from "./errorDiagnostic.ts";
import { ListId } from "./ListId";
// 「답할 것」(plan §8.345, 사용자 결정 10-21 1B·1A 2B): 사이드바 패널(AsksPanel)에 답할 것·답한 것 목록, 본문(AsksView)에 고른 것
// 하나의 카드. 카드 본문은 thread가 질문에 실은 Markdown(그림·표·code·링크)이라 웹페이지처럼 보인다: HTML은 해석하지 않고
// (Markdown html=false), 그림·링크는 host가 준 그 질문의 파일 주소(`files_base`) 아래 `library/…`만 연다. 그림을 누르면 크게 본다.
// 선택지에 그림이 있으면 선택지마다 큰 그림 카드(데스크톱 2열, 2B)다. 답한 것은 답할 때의 원문(질문·본문·선택지 전체와 까닭·
// 고른 답·때·기기)과 「그때 보고」를 보인다. 질문 카드 꼴은 화면 선택 창 카드(PromptCard, herdr-web-ui)를 따른다.
import { QuestionReports } from "./QuestionReports";
import { useCallback, useEffect, useId, useMemo, useRef, useState, type MouseEvent } from "react";
import { Check, ChevronRight, CircleQuestionMark, ListChecks, Maximize2, MessageSquareReply, Send, Trash2, Bot, X } from "lucide-react";
import type { AskRecord, FrontResponse, ThreadQuestion, ThreadQuestions, ThreadView, UserRequestView } from "./gen/proto";
import { ApiError, call, onAsksChanged } from "./api";
import { Markdown, MarkdownImageContext, MarkdownAttachmentContext } from "./Markdown";
import { OpenFileContext } from "./herdr/filePaths";
import { DrawerToggle, HeadMore, HeadNote } from "./drawer";
import { ConfirmDialog } from "./ConfirmDialog";
import { useLightbox } from "./Lightbox";
import { markdownImages } from "./reportImages";
import { askFileUrl, choicePicture, doneMeta, doneOf, doneTitle, openMeta, type AsksData, type OpenAsk } from "./asksModel";
import "./herdr/PromptCard.css";
import "./AsksView.css";

const letter = (i: number) => String.fromCharCode(65 + i);
const when = (ms: number) =>
  new Date(ms).toLocaleString("ko-KR", { month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit" });

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

const KIND_ICON = { question: CircleQuestionMark, request: Bot, task: ListChecks } as const;
const KIND_LABEL = { question: "질문", request: "확인 필요", task: "할 일 확인 필요" } as const;

// ───────────── 사이드바 목록 ─────────────

export function AsksPanel({
  open,
  data,
  selected,
  canControl,
  onPick,
  onForgetAll,
}: {
  open: OpenAsk[];
  data: AsksData | null;
  /** 본문에 연 것의 열쇠 */
  selected: string | null;
  canControl: boolean;
  onPick: (key: string) => void;
  onForgetAll: () => Promise<void>;
}) {
  const [confirm, setConfirm] = useState(false);
  const now = Date.now();
  const done = data?.done ?? [];
  const Row = ({ k, kind, title, meta, ids = [], isDone }: { k: string; kind: keyof typeof KIND_ICON; title: string; meta: string; ids?: string[]; isDone?: boolean }) => {
    const Icon = isDone ? Check : KIND_ICON[kind];
    return (
      <button type="button" className={`ask-row is-${kind}${isDone ? " is-done" : ""}${selected === k ? " is-selected" : ""}`} data-testid="ask-row" data-key={k}
        aria-current={selected === k ? "true" : undefined} onClick={() => onPick(k)}>
        <span className="ask-row-mark" aria-hidden="true"><Icon /></span>
        <span className="ask-row-text">
          {ids.length > 0 && <span className="ask-row-ids list-id-line">{[...new Set(ids)].map((id) => <ListId key={id} id={id} testId="ask-short-id" />)}</span>}
          <span className="ask-row-title">{title}</span>
          <span className="ask-row-meta">{meta}</span>
        </span>
      </button>
    );
  };
  return (
    <div className="asks-panel" data-testid="asks-panel">
      <h3 className="asks-panel-head">답할 것 <span className={open.length > 0 ? "is-ask" : undefined} data-testid="asks-open-count">{open.length}</span></h3>
      {open.length === 0 && <p className="asks-panel-empty">답할 것이 없다</p>}
      {open.map((a) =>
        a.kind === "question" ? (
          <Row key={a.key} k={a.key} kind="question" ids={a.thread.questions.items.flatMap((q) => q.id ? [q.id] : [])} title={`${a.thread.id} · ${a.thread.title || a.thread.id}`} meta={openMeta(a, now)} />
        ) : a.kind === "request" ? (
          <Row key={a.key} k={a.key} kind="request" ids={[...(a.request.question_id ? [a.request.question_id] : []), ...(a.request.questions?.items ?? []).flatMap((q) => q.id ? [q.id] : [])]} title={a.request.title} meta={openMeta(a, now)} />
        ) : (
          <Row key={a.key} k={a.key} kind="task" ids={a.task.short_id ? [a.task.short_id] : []} title={a.task.title} meta={openMeta(a, now)} />
        ),
      )}
      <h3 className="asks-panel-head">
        답한 것 <span data-testid="asks-done-count">{done.length}</span>
        {canControl && done.length > 0 && (
          <button type="button" className="ghost small asks-forget-all" data-testid="asks-forget-all" onClick={() => setConfirm(true)}>
            <Trash2 aria-hidden="true" /> 모두 지우기
          </button>
        )}
      </h3>
      {done.map((r) => (
        <Row key={r.id} k={`d:${r.id}`} kind={r.kind} ids={[...(r.question_id ? [r.question_id] : []), ...r.items.flatMap((q) => q.id ? [q.id] : [])]} title={doneTitle(r)} meta={doneMeta(r, now)} isDone />
      ))}
      {data && <p className="asks-panel-note">답한 것은 {data.keepDays}일 지나면 자동으로 지운다. 지우면 모든 기기에서 사라진다</p>}
      {confirm && (
        <ConfirmDialog
          title="답한 것 모두 지우기"
          body={`답한 것 ${done.length}개와 보존한 원문·그림을 모든 기기에서 지운다. 되돌릴 수 없다.`}
          confirmLabel="모두 지우기"
          onConfirm={onForgetAll}
          onClose={() => setConfirm(false)}
        />
      )}
    </div>
  );
}

// ───────────── 본문 ─────────────

export type AskSelection = { kind: "open"; ask: OpenAsk } | { kind: "done"; record: AskRecord } | { kind: "missing"; id: string } | { kind: "none" };

export function AsksView({
  shown,
  onClose,
  selection,
  canControl,
  onAnswer,
  onHandle,
  onRequestAnswer,
  onReply,
  onForget,
  onOpenTask,
  onThread,
}: {
  shown: boolean;
  onClose: () => void;
  selection: AskSelection;
  canControl: boolean;
  onAnswer: (t: ThreadView, picks: (number | null)[], text: string | null) => Promise<void>;
  onRequestAnswer: (r: UserRequestView, picks: (number | null)[], text: string | null) => Promise<void>;
  /** 확인 필요 post 「답함으로 표시」 */
  onHandle: (item: string) => Promise<void>;
  /** 「대화에서 답하기」: coordinator 입력칸에 머리글을 채우고 그 pane을 연다 */
  onReply: (title: string) => Promise<void>;
  onForget: (id: string) => Promise<void>;
  onOpenTask: (title: string) => void;
  onThread: (id: string) => void;
}) {
  const lightbox = useLightbox();
  const setZoom = (z: { src: string; alt: string }) => lightbox.open({ items: [z], at: 0 });
  const body = useRef<HTMLDivElement>(null);
  const key = selection.kind === "open" ? selection.ask.key : selection.kind === "done" ? `d:${selection.record.id}` : "";
  useEffect(() => {
    if (body.current) body.current.scrollTop = 0;
  }, [key]);
  const head =
    selection.kind === "open"
      ? selection.ask.kind === "question"
        ? { title: `${selection.ask.thread.id} · ${selection.ask.thread.title || selection.ask.thread.id}`, sub: `답할 것 · 질문 ${selection.ask.thread.questions.items.length}개` }
        : selection.ask.kind === "request"
          ? { title: selection.ask.request.title, sub: "답할 것 · coordinator 확인 필요" }
          : { title: selection.ask.task.title, sub: "답할 것 · 할 일 확인 필요" }
      : selection.kind === "done"
        ? { title: doneTitle(selection.record), sub: `답한 것 · ${selection.record.summary}` }
        : { title: "답할 것", sub: "질문·확인 필요·할 일" };
  // 본문 그림을 누르면 크게, 같은 본문의 다른 그림으로 넘긴다(선택지 단추 안의 그림은 그 단추가 고르기다: 따로 「크게」 단추가 있다)
  const onBodyClick = (e: MouseEvent<HTMLDivElement>) => {
    const root = e.target instanceof Element ? e.target.closest(".ask-markdown") : null;
    const got = root ? markdownImages(root, e.target) : null;
    if (!got) return;
    e.preventDefault();
    lightbox.open(got);
  };
  return (
    <div className={`thread-view asks-view${shown ? "" : " is-hidden"}`} inert={!shown} data-testid="asks-view" data-shown={shown ? "true" : "false"} data-key={key}>
      <header className="term-head">
        <DrawerToggle />
        <HeadNote />
        <div className="term-title">
          <strong>{head.title}</strong>
          <span className="muted">{head.sub}</span>
        </div>
        <button type="button" className="icon-button" aria-label="답할 것 닫기" title="답할 것 닫기" data-testid="asks-close" onClick={onClose}>
          <X aria-hidden="true" />
        </button>
        <HeadMore />
      </header>
      <div className="thread-body" ref={body} onClick={onBodyClick} data-testid="asks-scroll">
        <div className="thread-col asks-col">
          {selection.kind === "missing" && <p className="asks-empty" data-testid="asks-missing"><code>{selection.id}</code> 질문을 찾을 수 없다. 왼쪽 「답할 것」 목록에서 다른 질문을 열 수 있다.</p>}
          {selection.kind === "none" && <p className="asks-empty" data-testid="asks-empty">답할 것이 없다. 왼쪽 「답할 것」 목록에서 답한 것을 다시 볼 수 있다.</p>}
          {selection.kind === "open" && selection.ask.kind === "question" && (
            <QuestionCard key={`${selection.ask.thread.id} ${selection.ask.thread.questions.report_hash} ${JSON.stringify(selection.ask.thread.questions.draft ?? null)}`} t={selection.ask.thread} q={selection.ask.thread.questions}
              canControl={canControl} onAnswer={(p, text) => onAnswer((selection.ask as { thread: ThreadView }).thread, p, text)} onZoom={setZoom} onThread={onThread} />
          )}
          {selection.kind === "open" && selection.ask.kind === "request" && (
            selection.ask.request.questions ? (
              <QuestionCard key={`${selection.ask.key} ${JSON.stringify(selection.ask.request.questions.draft ?? null)}`} request={selection.ask.request} q={selection.ask.request.questions}
                canControl={canControl} onAnswer={(p, text) => onRequestAnswer((selection.ask as { request: UserRequestView }).request, p, text)} onZoom={setZoom} onThread={onThread} />
            ) : <RequestCard key={selection.ask.key} questionId={selection.ask.request.question_id} title={selection.ask.request.title} body={selection.ask.request.body} at={selection.ask.request.created_ms}
              canControl={canControl} onHandle={() => onHandle((selection.ask as { request: { id: string } }).request.id)} onReply={() => onReply(head.title)} />
          )}
          {selection.kind === "open" && selection.ask.kind === "task" && (
            <section className="prompt-card thread-ask-card ask-card" data-testid="ask-card" data-kind="task">
              <header className="prompt-card-header">
                <KindChip kind="task" />
                <span className="prompt-card-where">할 일</span>
              </header>
              <h2 className="ask-card-title">{selection.ask.task.title}</h2>
              <p className="ask-card-text">{selection.ask.task.attention}</p>
              <div className="thread-ask-foot">
                <span className="muted">끝남으로 지우기·다시 맡기기·연결만 풀기는 할 일 화면에서 정한다</span>
                <button type="button" className="btn btn-primary" data-testid="ask-open-task" onClick={() => onOpenTask((selection.ask as { task: { title: string } }).task.title)}>
                  <ChevronRight aria-hidden="true" /> 할 일에서 정하기
                </button>
              </div>
            </section>
          )}
          {selection.kind === "done" && (
            <DoneCard key={selection.record.id} r={selection.record} canControl={canControl} onForget={() => onForget(selection.record.id)} onZoom={setZoom} onThread={onThread} />
          )}
        </div>
      </div>
      {lightbox.node}
    </div>
  );
}

function KindChip({ kind, done }: { kind: keyof typeof KIND_ICON; done?: string }) {
  const Icon = done ? Check : KIND_ICON[kind];
  return (
    <span className={`ask-kind is-${done ? "done" : kind}`}>
      <Icon aria-hidden="true" />
      {done ?? KIND_LABEL[kind]}
    </span>
  );
}

/** 질문 본문 Markdown: 그림은 `files_base` 아래 `library/…`만, 링크한 library 파일은 새 탭(결과물처럼 sandbox) */
function AskMarkdown({ base, children, className }: { base: string | null | undefined; children: string; className?: string }) {
  const attachment = useCallback((src: string) => askFileUrl(base, src), [base]);
  const resolve = useCallback(async (src: string) => {
    const url = askFileUrl(base, src);
    return url === null ? null : ({ kind: "inline", url } as const);
  }, [base]);
  const open = useCallback((path: string) => {
    const url = askFileUrl(base, path);
    if (url !== null) window.open(url, "_blank", "noopener,noreferrer");
  }, [base]);
  return (
    <MarkdownAttachmentContext.Provider value={attachment}><OpenFileContext.Provider value={open}>
      <MarkdownImageContext.Provider value={resolve}>
        <div className={`ask-markdown${className ? ` ${className}` : ""}`} data-testid="ask-markdown">
          <Markdown>{children}</Markdown>
        </div>
      </MarkdownImageContext.Provider>
    </OpenFileContext.Provider></MarkdownAttachmentContext.Provider>
  );
}

/** 선택지 위의 큰 그림(2B). 단추 안이라 그림을 눌러도 고르기다: 「크게」는 옆 단추다 */
function ChoicePicture({ base, body, onZoom }: { base: string | null | undefined; body: string; onZoom: (z: { src: string; alt: string }) => void }) {
  const pic = choicePicture(body);
  const url = pic ? askFileUrl(base, pic.src) : null;
  if (!pic || url === null) return null;
  return (
    <span className="ask-choice-picture">
      <img src={url} alt={pic.alt} loading="lazy" data-testid="ask-choice-image" />
      <span role="button" tabIndex={0} className="ask-zoom" aria-label={`${pic.alt || "그림"} 크게 보기`} title="크게 보기" data-testid="ask-zoom"
        onClick={(e) => {
          e.stopPropagation();
          onZoom({ src: url, alt: pic.alt });
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            e.stopPropagation();
            onZoom({ src: url, alt: pic.alt });
          }
        }}>
        <Maximize2 aria-hidden="true" />
      </span>
    </span>
  );
}

/** 선택지 줄 아래 글(그림 한 장 뒤의 나머지 Markdown) */
function choiceRest(body: string | null | undefined): string | null {
  if (!body) return null;
  const pic = choicePicture(body);
  const rest = pic ? pic.rest : body;
  return rest.trim() === "" ? null : rest;
}

function QuestionText({ n, item, base }: { n: number; item: ThreadQuestion; base: string | null | undefined }) {
  return (
    <>
      <h3 className="prompt-card-question ask-question"><span className="thread-ask-qno">{n + 1}</span>{item.id && <code>{item.id}</code>} {item.text}</h3>
      {item.body && <AskMarkdown base={base}>{item.body}</AskMarkdown>}
    </>
  );
}

function QuestionCard({
  t,
  request,
  q,
  canControl,
  onAnswer,
  onZoom,
  onThread,
}: {
  t?: ThreadView;
  request?: UserRequestView;
  q: ThreadQuestions;
  canControl: boolean;
  onAnswer: (picks: (number | null)[], text: string | null) => Promise<void>;
  onZoom: (z: { src: string; alt: string }) => void;
  onThread: (id: string) => void;
}) {
  const [picks, setPicks] = useState<(number | null)[]>(() => q.draft?.picks ?? q.items.map(() => null));
  const [custom, setCustom] = useState(q.draft?.text ?? "");
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
  const off = pending || !canControl || !!q.format_warning;
  const text = custom.trim();
  const got = picks.filter((p) => p !== null).length;
  const all = got === q.items.length;
  const needsReviewText = q.review && picks[0] === 1 && !text;
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
  const submit = () => { if (!off) void (all || text ? send(picks) : filled && send(filled)); };
  const label = all ? `답 보내기 (${q.items.length})` : text ? "답 보내기" : filled ? "추천대로 보내기" : "답 보내기";
  const base = q.files_base;
  return (
    <section className="prompt-card thread-ask-card ask-card" role="region" aria-label={`${t?.id ?? request?.title} 질문`} aria-busy={pending} data-testid={t ? `thread-ask-${t.id}` : `request-ask-${request?.id}`} data-state="open" data-kind={t ? "question" : "request"}>
      <header className="prompt-card-header">
        <KindChip kind={t ? "question" : "request"} />
        {t ? <button type="button" className="linkish prompt-card-where" onClick={() => onThread(t.id)} title="thread 열기">{t.id} · {t.title || t.id}</button>
          : <span className="prompt-card-where">coordinator · {when(request!.created_ms)}</span>}
      </header>
      {q.format_warning && <p className="prompt-card-error" role="alert" data-testid="question-format-warning">{q.format_warning} 보고를 고친 뒤 답할 수 있다.</p>}
      <QuestionReports items={q.items} body={request?.body} reportId={t?.report_id} project={t?.project} base={base} />
      {request && <h2 className="ask-card-title">{request.title}</h2>}
      {q.items.map((item, n) => {
        const pictures = item.choices.some((c) => choicePicture(c.body) !== null);
        return (
          <div className="thread-ask-q" key={n} role="radiogroup" aria-label={`${n + 1}. ${item.text}`}>
            <QuestionText n={n} item={item} base={base} />
            <div className={`prompt-card-options${pictures ? " ask-choices has-pictures" : " ask-choices"}`}>
              {item.choices.map((c, i) => {
                const checked = picks[n] === i;
                const rest = choiceRest(c.body) ?? "";
                const video = /\.(mp4|webm)(?:[?#\s)]|$)/i.test(rest);
                const option = (
                  <button key={i} type="button" role="radio" aria-checked={checked} data-testid={`thread-ask-pick-${n + 1}${letter(i)}`}
                    className={`prompt-card-option ask-choice${c.description || rest ? " has-description" : ""}${checked ? " is-checked" : ""}`} disabled={off}
                    onClick={() => setPicks((ps) => ps.map((p, k) => (k === n ? (p === i ? null : i) : p)))}>
                    {pictures && <ChoicePicture base={base} body={c.body ?? ""} onZoom={onZoom} />}
                    <span className="ask-choice-line">
                      <span className="prompt-card-number"><span aria-hidden="true">{letter(i)}</span><span className="visually-hidden">{letter(i)}.</span></span>{" "}
                      <span className="prompt-card-option-text">
                        <span className="prompt-card-option-label">
                          {c.label}
                          {c.recommended && <>{" "}<span className="prompt-card-tag">추천</span></>}
                        </span>
                        {c.description && <span className="prompt-card-option-description">{c.description}</span>}
                      </span>
                    </span>
                    {rest && !video && <span className="ask-choice-rest"><AskMarkdown base={base}>{rest}</AskMarkdown></span>}
                  </button>
                );
                return video ? <div key={i} className="ask-choice-video">{option}<AskMarkdown base={base}>{rest}</AskMarkdown></div> : option;
              })}
            </div>
            {item.after && <AskMarkdown className="ask-after" base={base}>{item.after}</AskMarkdown>}
          </div>
        );
      })}
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
        <button type="button" className={`btn${all || text ? " btn-primary" : ""}`} data-testid="thread-ask-send" disabled={off || needsReviewText || !(all || text || filled)} onClick={submit}>
          <Send aria-hidden="true" /> {label}
        </button>
      </div>
      {!canControl && <p className="prompt-card-hint">보기 권한이라 답할 수 없다</p>}
      {needsReviewText && <p className="muted" data-testid="review-needs-text">고칠 점을 글로 적어 답한다.</p>}
      {error !== null && <p className="prompt-card-error" role="alert">{error}</p>}
    </section>
  );
}

function RequestCard({ questionId, title, body, at, canControl, onHandle, onReply }: {
  questionId?: string | null; title: string; body: string; at: number; canControl: boolean; onHandle: () => Promise<void>; onReply: () => Promise<void>;
}) {
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = (f: () => Promise<void>) => async () => {
    setPending(true);
    setError(null);
    try {
      await f();
    } catch (e) {
      setError(e instanceof ApiError ? e.message : String(e));
      setPending(false);
    }
  };
  return (
    <section className="prompt-card thread-ask-card ask-card" data-testid="ask-card" data-kind="request" aria-busy={pending}>
      <header className="prompt-card-header">
        <KindChip kind="request" />
        <span className="prompt-card-where">coordinator · {when(at)}</span>
      </header>
      <h2 className="ask-card-title">{questionId && <code>{questionId} </code>}{title}</h2>
      <AskMarkdown base={null}>{body}</AskMarkdown>
      <div className="thread-ask-foot">
        <span className="muted">답은 coordinator 대화로 한다</span>
        {canControl && (
          <span className="ask-buttons">
            <button type="button" className="btn" disabled={pending} data-testid="ask-handle" onClick={run(onHandle)}>
              <Check aria-hidden="true" /> 답함으로 표시
            </button>
            <button type="button" className="btn btn-primary" disabled={pending} data-testid="ask-reply" onClick={run(onReply)}>
              <MessageSquareReply aria-hidden="true" /> 대화에서 답하기
            </button>
          </span>
        )}
      </div>
      {!canControl && <p className="prompt-card-hint">보기 권한이라 답할 수 없다</p>}
      {error !== null && <p className="prompt-card-error" role="alert">{error}</p>}
    </section>
  );
}

function DoneCard({ r, canControl, onForget, onZoom, onThread }: {
  r: AskRecord; canControl: boolean; onForget: () => Promise<void>; onZoom: (z: { src: string; alt: string }) => void; onThread: (id: string) => void;
}) {
  const [report, setReport] = useState<string | null | "loading" | "missing">(null);
  const [confirm, setConfirm] = useState(false);
  const base = r.files_base;
  const toggleReport = () => {
    if (report !== null) return setReport(null);
    if (!base) return setReport("missing");
    setReport("loading");
    fetch(`${base}report.md`, { credentials: "same-origin" })
      .then((res) => (res.ok ? res.text() : Promise.reject(new Error(String(res.status)))))
      .then(setReport, () => setReport("missing"));
  };
  return (
    <section className="prompt-card thread-ask-card ask-card is-answered" data-testid="ask-done-card" data-kind={r.kind}>
      <header className="prompt-card-header">
        <KindChip kind={r.kind} done={r.answer ? `답함 ${r.summary}` : r.summary} />
        {r.kind === "question" ? (
          <button type="button" className="linkish prompt-card-where" onClick={() => onThread(r.subject)} title="thread 열기">{r.subject} · {r.title || r.subject}</button>
        ) : (
          <span className="prompt-card-where">{KIND_LABEL[r.kind]}</span>
        )}
      </header>
      <QuestionReports items={r.items} body={r.body} reportId={r.report_id} base={base} />
      {r.kind !== "question" && <h2 className="ask-card-title">{r.items.length === 0 && r.question_id && <code>{r.question_id} </code>}{r.title}</h2>}
      <p className="ask-done-when" data-testid="ask-done-when">물은 때 {when(r.asked_ms)} · {r.kind === "question" && !r.answer ? "닫힌 때" : "답한 때"} {when(r.done_ms)} · {r.by}</p>
      {r.items.map((item, n) => {
        const pick = r.answer?.picks[n];
        const pictures = item.choices.some((c) => choicePicture(c.body) !== null);
        return (
          <div className="thread-ask-q" key={n}>
            <QuestionText n={n} item={item} base={base} />
            <div className={`prompt-card-options ask-choices${pictures ? " has-pictures" : ""}`}>
              {item.choices.map((c, i) => {
                const picked = pick === i;
                const rest = choiceRest(c.body);
                return (
                  <div key={i} className={`prompt-card-option ask-choice is-static${picked ? " is-picked" : " is-other"}`} data-testid={picked ? "ask-done-picked" : undefined}>
                    {pictures && <ChoicePicture base={base} body={c.body ?? ""} onZoom={onZoom} />}
                    <span className="ask-choice-line">
                      <span className="prompt-card-number"><span>{letter(i)}</span></span>{" "}
                      <span className="prompt-card-option-text">
                        <span className="prompt-card-option-label">
                          {c.label}
                          {c.recommended && <>{" "}<span className="prompt-card-tag">추천</span></>}
                          {picked && <>{" "}<span className="ask-picked">고름</span></>}
                        </span>
                        {c.description && <span className="prompt-card-option-description">{c.description}</span>}
                      </span>
                    </span>
                    {rest && <span className="ask-choice-rest"><AskMarkdown base={base}>{rest}</AskMarkdown></span>}
                  </div>
                );
              })}
            </div>
            {item.after && <AskMarkdown className="ask-after" base={base}>{item.after}</AskMarkdown>}
            {pick === null && <p className="ask-done-text">직접 쓴 답으로 대신함</p>}
          </div>
        );
      })}
      {r.answer?.text && <p className="ask-done-text" data-testid="ask-done-text"><span className="muted">직접 쓴 답</span> <strong>{r.answer.text}</strong></p>}
      {r.kind !== "question" && r.items.length === 0 && r.body && <AskMarkdown base={base}>{r.body}</AskMarkdown>}
      {report !== null && (
        <div className="ask-report" data-testid="ask-report">
          {report === "loading" ? <p className="muted">불러오는 중</p> : report === "missing" ? <p className="muted">그때 보고 사본이 없다</p> : <AskMarkdown base={base}>{report}</AskMarkdown>}
        </div>
      )}
      <div className="thread-ask-foot">
        <span className="muted">{r.kind === "question" ? r.answer ? "답할 때의 사본이다. 그 뒤 새 보고가 와도 남는다" : "새 보고로 질문이 닫혔다. 선택한 답은 없다" : "처리한 때의 사본이다"}</span>
        <span className="ask-buttons">
          {r.items.length > 0 && base && (
            <button type="button" className="ghost small" aria-expanded={report !== null} data-testid="ask-report-toggle" onClick={toggleReport}>
              {report !== null ? "그때 보고 접기" : "그때 보고 보기"}
            </button>
          )}
          {canControl && (
            <button type="button" className="ghost small ask-forget" data-testid="ask-forget" onClick={() => setConfirm(true)}>
              <Trash2 aria-hidden="true" /> 지우기
            </button>
          )}
        </span>
      </div>
      {confirm && (
        <ConfirmDialog title="답한 것 지우기" body={`「${doneTitle(r)}」 기록과 보존한 원문·그림을 모든 기기에서 지운다.`} confirmLabel="지우기"
          onConfirm={onForget} onClose={() => setConfirm(false)} />
      )}
    </section>
  );
}

/** host 「답할 것」 자료(열린 확인 필요 post·답한 것 기록): 연결할 때와 host `asks` 사건마다 읽는다. 조작 응답도 같은 꼴이라 `put`으로 바로 넣는다 */
export function useAsksData(up: boolean): { data: AsksData | null; put: (r: FrontResponse) => void } {
  const [data, setData] = useState<AsksData | null>(null);
  const seq = useRef(0);
  const put = useCallback((r: FrontResponse) => {
    if (r.kind === "asks") setData({ requests: r.requests, done: r.done, keepDays: r.keep_days });
  }, []);
  const load = useCallback(() => {
    const n = ++seq.current;
    call({ op: "asks" })
      .then((r) => {
        if (n === seq.current) put(r);
      })
      .catch(ignoredFailure("AsksView.tsx:563", () => {}));
  }, [put]);
  useEffect(() => {
    if (up) load();
  }, [up, load]);
  useEffect(() => onAsksChanged(load), [load]);
  return { data, put };
}

/** 열쇠 → 본문에 그릴 것 */
export function useAskSelection(key: string | null, open: OpenAsk[], done: AskRecord[]): AskSelection {
  return useMemo<AskSelection>(() => {
    if (key?.startsWith("d:")) {
      const r = done.find((x) => x.id === key.slice(2));
      if (r) return { kind: "done", record: r };
    }
    const a = key ? open.find((x) => x.key === key) : undefined;
    if (a) return { kind: "open", ask: a };
    // 방금 답한 것(그 카드가 답할 것에서 빠졌다): 그때 남긴 기록을 연다
    const answered = key ? doneOf(key, done) : null;
    if (answered) return { kind: "done", record: answered };
    // 영속 ID 링크가 닫혔거나 지워졌어도 다른 질문을 그 ID의 카드처럼 열지 않는다.
    if (/^q:q-\d{4,}$/.test(key ?? "")) return { kind: "missing", id: key!.slice(2) };
    return open[0] ? { kind: "open", ask: open[0] } : { kind: "none" };
  }, [key, open, done]);
}

