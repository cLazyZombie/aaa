import { AsksReload } from "./asksReload";
import { AutoDecisionCard, useAutoMode } from "./AutoMode";
import { ignoredFailure } from "./errorDiagnostic.ts";
import { ListId } from "./ListId";
// 「답할 것」(plan §8.345, 사용자 결정 10-21 1B·1A 2B): 사이드바 패널(AsksPanel)에 답할 것·답한 것 목록, 본문(AsksView)에 고른 것
// 하나의 카드. 카드 본문은 thread가 질문에 실은 Markdown(그림·표·code·링크)이라 웹페이지처럼 보인다: HTML은 해석하지 않고
// (Markdown html=false), 그림·링크는 host가 준 그 질문의 파일 주소(`files_base`) 아래 `library/…`만 연다. 그림을 누르면 크게 본다.
// 선택지에 그림이 있으면 선택지마다 큰 그림 카드(데스크톱 2열, 2B)다. 답한 것은 답할 때의 원문(질문·본문·선택지 전체와 까닭·
// 고른 답·때·기기)과 「그때 보고」를 보인다. 질문 카드 꼴은 화면 선택 창 카드(PromptCard, herdr-web-ui)를 따른다.
import { QuestionReports } from "./QuestionReports";
import { useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type MouseEvent } from "react";
import { Check, ChevronRight, CircleQuestionMark, ListChecks, Maximize2, MessageSquareReply, Send, Trash2, Bot, X, Zap } from "lucide-react";
import type { TaskAttentionAction, TaskView, AutoDecision, AskRecord, FrontResponse, ThreadQuestion, ThreadQuestions, ThreadView, UserRequestView } from "./gen/proto";
import { ApiError, call, onAsksChanged } from "./api";
import { Markdown, MarkdownImageContext, MarkdownAttachmentContext } from "./Markdown";
import { OpenFileContext } from "./herdr/filePaths";
import { DrawerToggle, HeadMore, HeadNote } from "./drawer";
import { ConfirmDialog } from "./ConfirmDialog";
import { useLightbox } from "./Lightbox";
import { markdownImages } from "./reportImages";
import { askFileUrl, choicePicture, doneMeta, doneOf, doneTitle, freshAuto, answerIndex, mergeAskPage, questionIds, openMeta, type AsksData, type OpenAsk } from "./asksModel";
import "./herdr/PromptCard.css";
import "./AsksView.css";
import { withRecommended } from "./questionAnswer";
import { mergeQuestionDraft, questionDrafts, questionIdentity, type QuestionDraft } from "./questionDraft";
import { AnswerField } from "./AnswerField";
import { addAnswerImage, answerBody, answerImages, projectOfBase, removeAnswerImage, withAnswerBody } from "./answerImages";
import { taskImageUrl } from "./tasksModel";
export { withRecommended } from "./questionAnswer";

const letter = (c: { letter?: string }, i: number) => c.letter ?? String.fromCharCode(65 + i);
const when = (ms: number) =>
  new Date(ms).toLocaleString("ko-KR", { month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit" });

const KIND_ICON = { question: CircleQuestionMark, request: Bot, task: ListChecks } as const;
const KIND_LABEL = { question: "질문", request: "확인 필요", task: "할 일 확인 필요" } as const;

const AskRow = ({ k, kind, title, meta, ids = [], isDone, selected, onPick }: { selected: string | null; onPick: (key: string) => void; k: string; kind: keyof typeof KIND_ICON; title: string; meta: string; ids?: string[]; isDone?: boolean }) => {
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

// ───────────── 사이드바 목록 ─────────────

export function AsksPanel({
  open,
  data,
  selected,
  canControl,
  onPick,
  onForgetAll,
  onMoreAuto,
  onMoreDone,
}: {
  open: OpenAsk[];
  data: AsksData | null;
  /** 본문에 연 것의 열쇠 */
  selected: string | null;
  canControl: boolean;
  onPick: (key: string) => void;
  onForgetAll: (ids: string[]) => Promise<void>;
  onMoreAuto: () => Promise<void>;
  onMoreDone: () => Promise<void>;
}) {
  const [confirm, setConfirm] = useState<string[] | null>(null);
  const [confirmAuto, setConfirmAuto] = useState(false);
  const forgetAuto = useAutoMode()?.forget;
  const [moreBusy, setMoreBusy] = useState(false);
  const [moreError, setMoreError] = useState<string | null>(null);
  const [visible, setVisible] = useState(50);
  const now = Date.now();
  const auto = data?.autoDecisions ?? [];
  const autoIds = new Set(auto.filter(d => d.outcome === "answered").map(d => d.question.id));
  const shown = (r: Parameters<typeof questionIds>[0]) => !questionIds(r).length || !questionIds(r).every(id => autoIds.has(id));
  // 자동 답변을 뺀 뒤 50줄씩 표시한다. 원문 페이지의 바이트 경계와 목록 행 수는 독립이다.
  const done = answerIndex(data).filter(shown);
  const autoRow = (d: AutoDecision) => <button type="button" className={`ask-row is-auto${selected === `a:${d.id}` ? " is-selected" : ""}`} key={d.id} data-testid="auto-decision-row" data-key={`a:${d.id}`} onClick={() => onPick(`a:${d.id}`)}>
    <span className="ask-row-mark"><Zap /></span><span className="ask-row-text"><span className="ask-row-title">{d.question.id} {d.question.text} → {d.choice}</span><span className="ask-row-meta">{d.outcome === "answered" ? (d.reviewed ? "확인함" : "자동 결정") : "전달 확인 필요"} · {when(d.at_ms)}</span></span>
  </button>;
  return (
    <div className="asks-panel" data-testid="asks-panel">
      <h3 className="asks-panel-head">답할 것 <span className={open.length > 0 ? "is-ask" : undefined} data-testid="asks-open-count">{open.length}</span></h3>
      {open.length === 0 && <p className="asks-panel-empty">답할 것이 없다</p>}
      {open.map((a) =>
        a.kind === "question" ? (
          <AskRow selected={selected} onPick={onPick} key={a.key} k={a.key} kind="question" ids={a.thread.questions.items.flatMap((q) => q.id ? [q.id] : [])} title={`${a.thread.id} · ${a.thread.title || a.thread.id}`} meta={openMeta(a, now)} />
        ) : a.kind === "request" ? (
          <AskRow selected={selected} onPick={onPick} key={a.key} k={a.key} kind="request" ids={[...(a.request.question_id ? [a.request.question_id] : []), ...(a.request.questions?.items ?? []).flatMap((q) => q.id ? [q.id] : [])]} title={a.request.title} meta={openMeta(a, now)} />
        ) : (
          <AskRow selected={selected} onPick={onPick} key={a.key} k={a.key} kind="task" ids={a.task.short_id ? [a.task.short_id] : []} title={a.task.title} meta={openMeta(a, now)} />
        ),
      )}
      <h3 className="asks-panel-head">
        자동으로 정한 것 <span data-testid="asks-auto-count">{auto.length}</span>
        {canControl && forgetAuto && auto.length > 0 && (
          <button type="button" className="ghost small asks-forget-all" data-testid="asks-auto-forget-all" onClick={() => setConfirmAuto(true)}>
            <Trash2 aria-hidden="true" /> 모두 지우기
          </button>
        )}
      </h3>
      {auto.map(autoRow)}
      {data?.autoNext && <button disabled={moreBusy} onClick={() => { setMoreBusy(true); setMoreError(null); void onMoreAuto().catch(e => setMoreError(String(e))).finally(() => setMoreBusy(false)); }}>이전 자동 결정 더 보기</button>}
      {moreError && <p role="alert">{moreError}</p>}
      <h3 className="asks-panel-head">
        답한 것 <span data-testid="asks-done-count">{done.length}</span>
        {canControl && done.length > 0 && (
          <button type="button" className="ghost small asks-forget-all" data-testid="asks-forget-all" onClick={() => setConfirm(done.map(r => r.id))}>
            <Trash2 aria-hidden="true" /> 모두 지우기
          </button>
        )}
      </h3>
      {done.slice(0, visible).map((r) => (
        <AskRow selected={selected} onPick={onPick} key={r.id} k={`d:${r.id}`} kind={r.kind} ids={[...(r.question_id ? [r.question_id] : []), ...questionIds(r)]} title={doneTitle(r)} meta={doneMeta(r, now)} isDone />
      ))}
      {done.length > visible && <button type="button" data-testid="asks-done-more" disabled={moreBusy} onClick={() => {
        setMoreBusy(true); setMoreError(null);
        void onMoreDone().then(() => setVisible(n => n + 50)).catch(e => setMoreError(String(e))).finally(() => setMoreBusy(false));
      }}>답한 것 더 보기</button>}
      {data && <p className="asks-panel-note">답한 것은 {data.keepDays}일 지나면 자동으로 지운다. 지우면 모든 기기에서 사라진다</p>}
      {confirmAuto && forgetAuto && (
        <ConfirmDialog
          title="자동으로 정한 것 모두 지우기"
          body={`자동 결정 기록${data?.autoNext ? " 전부" : ` ${auto.length}개`}와 보존한 원문·그림을 모든 기기에서 지운다. 이미 진행한 일은 되돌리지 않는다. 되돌릴 수 없다.`}
          confirmLabel="모두 지우기"
          onConfirm={() => forgetAuto([], true)}
          onClose={() => setConfirmAuto(false)}
        />
      )}
      {confirm && (
        <ConfirmDialog
          title="답한 것 모두 지우기"
          body={`답한 것 ${confirm.length}개와 보존한 원문·그림을 모든 기기에서 지운다. 되돌릴 수 없다.`}
          confirmLabel="모두 지우기"
          onConfirm={() => onForgetAll(confirm)}
          onClose={() => setConfirm(null)}
        />
      )}
    </div>
  );
}

// ───────────── 본문 ─────────────

export type AskSelection = { kind: "auto"; decision: AutoDecision } | { kind: "open"; ask: OpenAsk } | { kind: "done"; record: AskRecord } | { kind: "missing"; id: string } | { kind: "loading"; error?: string | null; retry?: () => void } | { kind: "none" };

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
  onTaskAnswer,
  onThread,
}: {
  shown: boolean;
  onClose: () => void;
  selection: AskSelection;
  canControl: boolean;
  onAnswer: (t: ThreadView, picks: (number | null)[], text: string | null, comments: (string | null)[]) => Promise<void>;
  onRequestAnswer: (r: UserRequestView, picks: (number | null)[], text: string | null, comments: (string | null)[]) => Promise<void>;
  /** 확인 필요 post 「답함으로 표시」 */
  onHandle: (item: string) => Promise<void>;
  /** 「대화에서 답하기」: coordinator 입력칸에 머리글을 채우고 그 pane을 연다 */
  onReply: (title: string) => Promise<void>;
  onForget: (id: string) => Promise<void>;
  onOpenTask: (title: string) => void;
  onTaskAnswer: (task: TaskView, action: TaskAttentionAction) => Promise<void>;
  onThread: (id: string) => void;
}) {
  const lightbox = useLightbox();
  const setZoom = (z: { src: string; alt: string }) => lightbox.open({ items: [z], at: 0 });
  const body = useRef<HTMLDivElement>(null);
  const key = selection.kind === "auto" ? `a:${selection.decision.id}` : selection.kind === "open" ? selection.ask.key : selection.kind === "done" ? `d:${selection.record.id}` : "";
  useEffect(() => {
    if (body.current) body.current.scrollTop = 0;
  }, [key]);
  const head =
    selection.kind === "auto" ? { title: "자동으로 정한 것", sub: selection.decision.question.text } : selection.kind === "open"
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
          {selection.kind === "loading" && (selection.error ? <p role="alert">{selection.error} <button onClick={selection.retry}>다시 읽기</button></p> : <p role="status">답변을 불러오는 중…</p>)}
          {selection.kind === "none" && <p className="asks-empty" data-testid="asks-empty">답할 것이 없다. 왼쪽 「답할 것」 목록에서 답한 것을 다시 볼 수 있다.</p>}
          {selection.kind === "open" && selection.ask.kind === "question" && (
            <QuestionCard key={JSON.stringify(["question", selection.ask.thread.project, selection.ask.thread.id, questionIdentity(selection.ask.thread.questions)])} t={selection.ask.thread} q={selection.ask.thread.questions}
              active={shown} canControl={canControl} onAnswer={(p, text, comments) => onAnswer((selection.ask as { thread: ThreadView }).thread, p, text, comments)} onZoom={setZoom} onThread={onThread} />
          )}
          {selection.kind === "open" && selection.ask.kind === "request" && (
            selection.ask.request.questions ? (
              <QuestionCard key={JSON.stringify([selection.ask.key, questionIdentity(selection.ask.request.questions)])} request={selection.ask.request} q={selection.ask.request.questions}
                active={shown} canControl={canControl} onAnswer={(p, text, comments) => onRequestAnswer((selection.ask as { request: UserRequestView }).request, p, text, comments)} onZoom={setZoom} onThread={onThread} />
            ) : <RequestCard key={selection.ask.key} questionId={selection.ask.request.question_id} title={selection.ask.request.title} body={selection.ask.request.body} at={selection.ask.request.created_ms}
              canControl={canControl} onHandle={() => onHandle((selection.ask as { request: { id: string } }).request.id)} onReply={() => onReply(head.title)} />
          )}
          {selection.kind === "open" && selection.ask.kind === "task" && (
            <TaskAttentionCard key={selection.ask.key} task={selection.ask.task} canControl={canControl}
              onAnswer={onTaskAnswer} onOpenTask={onOpenTask} onThread={onThread} />
          )}
          {selection.kind === "auto" && <AutoDecisionCard key={selection.decision.id} decision={selection.decision} />}
          {selection.kind === "done" && (
            <DoneCard key={selection.record.id} r={selection.record} canControl={canControl} onForget={() => onForget(selection.record.id)} onZoom={setZoom} onThread={onThread} />
          )}
        </div>
      </div>
      {lightbox.node}
    </div>
  );
}

function TaskAttentionCard({ task, canControl, onAnswer, onOpenTask, onThread }: {
  task: TaskView; canControl: boolean;
  onAnswer: (task: TaskView, action: TaskAttentionAction) => Promise<void>;
  onOpenTask: (title: string) => void; onThread: (id: string) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const pending = useRef(false);
  const evidence = task.attention_evidence;
  const answer = async (action: TaskAttentionAction) => {
    if (pending.current) return;
    pending.current = true;
    setBusy(true); setError(null);
    try { await onAnswer(task, action); }
    catch (e) { setError(e instanceof Error ? e.message : String(e)); }
    finally { pending.current = false; setBusy(false); }
  };
  return <section className="prompt-card thread-ask-card ask-card" data-testid="ask-card" data-kind="task" aria-busy={busy}>
    <header className="prompt-card-header"><KindChip kind="task" /><span className="prompt-card-where">할 일</span></header>
    <h2 className="ask-card-title">{task.short_id && <ListId id={task.short_id} />} {task.title}</h2>
    <p className="ask-card-text">{task.attention}</p>
    {evidence && <dl className="ask-task-evidence" data-testid="ask-task-evidence">
      <dt>thread</dt><dd><button className="linkish" onClick={() => onThread(evidence.thread)}>{evidence.thread}</button> · {evidence.status === "resolved" ? "끝남" : evidence.status}</dd>
      <dt>할 일 저장소</dt><dd>{task.repo ?? "미정"}</dd>
      <dt>마지막 보고</dt><dd>{evidence.report_first_line ?? (evidence.report_hash ? "보고 사본 확인 필요" : "보고가 없다")}</dd>
      {evidence.landing_lines.length > 0 && <><dt>착지</dt><dd>{evidence.landing_lines.map((line, i) => <div key={i}>{line}</div>)}</dd></>}
    </dl>}
    {error && <p role="alert">{error}</p>}
    <div className="thread-ask-foot ask-task-actions">
      {canControl && evidence && <>
        <button type="button" className="btn btn-primary" disabled={busy} data-testid="ask-task-complete" onClick={() => void answer("complete")}>완료</button>
        <button type="button" className="btn" disabled={busy} data-testid="ask-task-reassign" onClick={() => void answer("reassign")}>다시 맡기기</button>
        <button type="button" className="btn btn-ghost" disabled={busy} data-testid="ask-task-unlink" onClick={() => void answer("unlink")}>연결만 풀기</button>
      </>}
      <button type="button" className="linkish" disabled={busy} data-testid="ask-open-task" onClick={() => onOpenTask(task.title)}><ChevronRight aria-hidden="true" /> 할 일 열기</button>
    </div>
  </section>;
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
export function AskMarkdown({ base, children, className }: { base: string | null | undefined; children: string; className?: string }) {
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
      <h3 className="prompt-card-question ask-question"><span className="thread-ask-qno">{item.source_number ?? n + 1}</span>{item.id && <code>{item.id}</code>} {item.text}</h3>
      {item.body && <AskMarkdown base={base}>{item.body}</AskMarkdown>}
    </>
  );
}

function QuestionCard({
  t,
  request,
  q,
  active,
  canControl,
  onAnswer,
  onZoom,
  onThread,
}: {
  t?: ThreadView;
  request?: UserRequestView;
  q: ThreadQuestions;
  active: boolean;
  canControl: boolean;
  onAnswer: (picks: (number | null)[], text: string | null, comments: (string | null)[]) => Promise<void>;
  onZoom: (z: { src: string; alt: string }) => void;
  onThread: (id: string) => void;
}) {
  const owner = JSON.stringify(t ? ["question", t.project, t.id] : ["request", request!.id]);
  const [local, setLocal] = useState(() => ({ value: questionDrafts.read(owner, q), server: mergeQuestionDraft(q) }));
  const server = mergeQuestionDraft(q);
  const changed = JSON.stringify(server) !== JSON.stringify(local.server);
  const draft = changed ? mergeQuestionDraft(q, local.value, local.server) : local.value;
  if (changed) setLocal({ value: draft, server });
  const { picks, custom, comments } = draft;
  const current = useRef(draft);
  current.current = draft;
  // input capture와 React onChange 모두 같은 최신 초안을 갱신한다. 이벤트 안에서
  // 바로 저장해 재렌더·새로고침·unmount 전에 글을 보존한다.
  const update = useCallback((change: (before: QuestionDraft) => QuestionDraft) => {
    const next = change(current.current);
    if (JSON.stringify(next) === JSON.stringify(current.current)) return;
    current.current = next;
    questionDrafts.write(owner, q, next);
    setLocal({ value: next, server: mergeQuestionDraft(q) });
  }, [owner, q]);
  const multiple = q.items.length > 1;
  const commentInputs = useRef<(HTMLTextAreaElement | null)[]>([]);
  const customInput = useRef<HTMLTextAreaElement>(null);
  // 올리는 중인 그림 수(칸마다). 올리는 동안은 보내지 않는다: 그림 줄이 빠진 답이 가지 않게 한다
  const [uploads, setUploads] = useState<Record<string, number>>({});
  const uploading = Object.values(uploads).some((n) => n > 0);
  const onUploading = useCallback((field: string) => (count: number) => setUploads((u) => (u[field] === count ? u : { ...u, [field]: count })), []);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const revision = useRef(q.report_hash);
  revision.current = q.report_hash;
  useLayoutEffect(() => { setPending(false); setError(null); }, [q.report_hash]);
  const shown = useRef(false);
  useEffect(() => {
    shown.current = true;
    return () => {
      shown.current = false;
    };
  }, []);
  const customId = useId();
  const off = pending || !canControl;
  useLayoutEffect(() => {
    if (!active || off) return;
    const view = (customInput.current ?? commentInputs.current.find(Boolean))?.ownerDocument.defaultView;
    if (!view) return;
    // 대화 입력창과 같은 경계: root capture의 앞선 commit이 controlled 값을
    // 복원하기 전에 새 글을 받는다. bubble onChange도 기존대로 유지한다.
    const input = (event: Event) => {
      if (event.target === customInput.current) {
        const value = customInput.current!.value;
        update(d => ({ ...d, custom: withAnswerBody(d.custom, value) }));
      }
      const n = commentInputs.current.findIndex(element => element !== null && element === event.target);
      if (n >= 0) {
        const value = commentInputs.current[n]!.value;
        update(d => ({ ...d, comments: d.comments.map((old, i) => i === n ? withAnswerBody(old ?? "", value) : old) }));
      }
    };
    view.addEventListener("input", input, true);
    return () => view.removeEventListener("input", input, true);
  }, [active, off, update]);
  const text = custom.trim();
  const got = picks.filter((p) => p !== null).length;
  const all = q.items.every((_, n) => picks[n] !== null || (multiple && !!comments[n]?.trim()));
  const needsReviewText = q.review && picks[0] === 1 && !text && !(multiple && comments[0]?.trim());
  const filled = all ? null : withRecommended(q, picks, multiple ? comments : []);
  const send = async (p: (number | null)[]) => {
    const sent = questionDrafts.snapshot(owner);
    const sentRevision = q.report_hash;
    setPending(true);
    setError(null);
    try {
      await onAnswer(p, text || null, multiple ? comments.map(c => c?.trim() || null) : []);
      questionDrafts.settle(owner, sent);
    } catch (e) {
      if (shown.current && revision.current === sentRevision) setError(e instanceof ApiError ? e.message : String(e));
    }
    if (shown.current && revision.current === sentRevision) setPending(false);
  };
  // 다 골랐거나 직접 쓴 답이 있으면 그대로, 아니면 남은 질문을 추천으로 채워 보낸다
  const submit = () => { if (!off && !uploading && !needsReviewText) void (all || text ? send(picks) : filled && send(filled)); };
  const label = all ? `답 보내기 (${q.items.length})` : text ? "답 보내기" : filled ? "추천대로 보내기" : "답 보내기";
  const base = q.files_base;
  const project = projectOfBase(base) ?? t?.project ?? null;
  const setComment = (n: number, f: (old: string) => string) => update(d => ({ ...d, comments: d.comments.map((old, i) => i === n ? f(old ?? "") : old) }));
  return (
    <section className="prompt-card thread-ask-card ask-card" role="region" aria-label={`${t?.id ?? request?.title} 질문`} aria-busy={pending} data-testid={t ? `thread-ask-${t.id}` : `request-ask-${request?.id}`} data-state="open" data-kind={t ? "question" : "request"}>
      <header className="prompt-card-header">
        <KindChip kind={t ? "question" : "request"} />
        {t ? <button type="button" className="linkish prompt-card-where" onClick={() => onThread(t.id)} title="thread 열기">{t.id} · {t.title || t.id}</button>
          : <span className="prompt-card-where">coordinator · {when(request!.created_ms)}</span>}
      </header>
      {q.format_warning?.split("\n").map((line, i) => /^(근거|파일) 확인 필요/.test(line)
        ? <p key={i} className="prompt-card-error" role="alert" data-testid={line.startsWith("근거") ? "question-evidence-warning" : "question-file-warning"}>{line}</p>
        : <p key={i} className="prompt-card-error" role="alert" data-testid="question-format-warning">형식 오류: {line} 아래에 보이는 선택지를 고르거나 직접 답을 입력할 수 있다.</p>)}
      <QuestionReports items={q.items} body={request?.body} reportId={t?.report_id} project={t?.project} base={base} />
      {request && <h2 className="ask-card-title">{request.title}</h2>}
      {q.items.map((item, n) => {
        const pictures = item.choices.some((c) => choicePicture(c.body) !== null);
        return (
          <div className="thread-ask-q" key={n}>
            <QuestionText n={n} item={item} base={base} />
            <div role="radiogroup" aria-label={`${item.source_number ?? n + 1}. ${item.text}`} className={`prompt-card-options${pictures ? " ask-choices has-pictures" : " ask-choices"}`}>
              {item.choices.map((c, i) => {
                const checked = picks[n] === i;
                const rest = choiceRest(c.body) ?? "";
                const video = /\.(mp4|webm)(?:[?#\s)]|$)/i.test(rest);
                const option = (
                  <button key={i} type="button" role="radio" aria-checked={checked} data-testid={`thread-ask-pick-${item.source_number ?? n + 1}${letter(c, i)}`}
                    className={`prompt-card-option ask-choice${c.description || rest ? " has-description" : ""}${checked ? " is-checked" : ""}`} disabled={off}
                    onClick={() => update(d => ({ ...d, picks: d.picks.map((p, k) => (k === n ? (p === i ? null : i) : p)) }))}>
                    {pictures && <ChoicePicture base={base} body={c.body ?? ""} onZoom={onZoom} />}
                    <span className="ask-choice-line">
                      <span className="prompt-card-number"><span aria-hidden="true">{letter(c, i)}</span><span className="visually-hidden">{letter(c, i)}.</span></span>{" "}
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
            {multiple && <div className="prompt-card-custom ask-question-comment">
              <label className="prompt-card-custom-label" htmlFor={`${customId}-${n}`}>직접 쓴 답</label>
              <AnswerField id={`${customId}-${n}`} inputRef={element => { commentInputs.current[n] = element; }} value={comments[n] ?? ""} project={project}
                disabled={off} label={`${item.source_number ?? n + 1}번 직접 쓴 답`} testId={`thread-ask-text-${item.source_number ?? n + 1}`}
                onBody={body => setComment(n, old => withAnswerBody(old, body))}
                onImages={targets => setComment(n, old => targets.reduce(addAnswerImage, old))}
                onRemoveImage={target => setComment(n, old => removeAnswerImage(old, target))}
                onSubmit={submit} onUploading={onUploading(`c${n}`)} onZoom={onZoom} />
            </div>}
            {item.after && <AskMarkdown className="ask-after" base={base}>{item.after}</AskMarkdown>}
          </div>
        );
      })}
      {(!multiple || custom) && <div className="prompt-card-custom">
        <span className="prompt-card-custom-label" id={customId}>또는 직접 답을 입력(고르지 않은 질문을 대신한다)</span>
        <AnswerField inputRef={customInput} value={custom} project={project} disabled={off} placeholder="예: 2 둘 다 보이게" labelledBy={customId} testId="thread-ask-text"
          onBody={body => update(d => ({ ...d, custom: withAnswerBody(d.custom, body) }))}
          onImages={targets => update(d => ({ ...d, custom: targets.reduce(addAnswerImage, d.custom) }))}
          onRemoveImage={target => update(d => ({ ...d, custom: removeAnswerImage(d.custom, target) }))}
          onSubmit={submit} onUploading={onUploading("custom")} onZoom={onZoom} />
      </div>}
      <div className="thread-ask-foot">
        <span className="muted">{got}/{q.items.length} 고름{!all && filled ? " · 고르지 않은 것은 추천대로" : ""}</span>
        <button type="button" className={`btn${all || text ? " btn-primary" : ""}`} data-testid="thread-ask-send" disabled={off || uploading || needsReviewText || !(all || text || filled)} onClick={submit}>
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
      <div className="thread-ask-foot is-end">
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

/** 답한 카드의 직접 쓴 답: 줄바꿈 그대로, 끝의 그림 줄은 썸네일(누르면 크게)로 */
function DoneAnswer({ text, project, testId, onZoom }: { text: string; project: string | null; testId: string; onZoom: (z: { src: string; alt: string }) => void }) {
  const body = answerBody(text).trim();
  const images = project ? answerImages(text) : [];
  return (
    <>
      <p className="ask-done-text" data-testid={testId}><span className="muted">직접 쓴 답</span> <strong>{images.length === 0 ? text : body}</strong></p>
      {images.length > 0 && (
        <ul className="ask-answer-images is-done" aria-label="답에 붙인 그림" data-testid={`${testId}-images`}>
          {images.map((target, i) => (
            <li key={target} className="ask-answer-image" data-path={target}>
              <button type="button" className="ask-answer-thumb" aria-label={`답 그림 ${i + 1} 크게 보기`} onClick={() => onZoom({ src: taskImageUrl(project!, target), alt: `답 그림 ${i + 1}` })}>
                <img src={taskImageUrl(project!, target)} alt="" loading="lazy" decoding="async" />
              </button>
            </li>
          ))}
        </ul>
      )}
    </>
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
                      <span className="prompt-card-number"><span>{letter(c, i)}</span></span>{" "}
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
            {r.answer?.comments?.[n] ? <DoneAnswer text={r.answer.comments[n]} project={projectOfBase(base)} testId={`ask-done-text-${item.source_number ?? n + 1}`} onZoom={onZoom} />
              : pick === null && <p className="ask-done-text">직접 쓴 답으로 대신함</p>}
          </div>
        );
      })}
      {r.answer?.text && <DoneAnswer text={r.answer.text} project={projectOfBase(base)} testId="ask-done-text" onZoom={onZoom} />}
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
export function useAsksData(): { data: AsksData | null; put: (r: FrontResponse) => void; moreAuto: () => Promise<void>; moreDone: () => Promise<void>; loadDone: (id: string) => Promise<void>; forgetAuto: (ids: string[], all: boolean) => Promise<void> } {
  const [data, setData] = useState<AsksData | null>(null);
  const seq = useRef(0);
  const reloader = useRef<AsksReload<FrontResponse> | null>(null);
  const mergeAuto = (fresh: AutoDecision[], old: AutoDecision[] = []) => [...new Map([...old, ...fresh].map(d => [d.id, d])).values()].sort((a, b) => b.at_ms - a.at_ms);
  const accept = useCallback((r: FrontResponse) => {
    if (r.kind === "asks") {
      ++seq.current;
      reloader.current?.invalidate();
      setData(old => {
        const fresh = r.auto_decisions ?? [];
        const autoDecisions = freshAuto(fresh, r.auto_next ?? null, old?.autoDecisions);
        // 이전 쪽을 남겼으면 그 다음 cursor를 그대로 쓴다
        const autoNext = autoDecisions.length > fresh.length ? old?.autoNext : r.auto_next;
        return { requests: r.requests, done: r.done_index ? mergeAskPage(old?.done ?? [], r.done, r.done_index) : r.done,
          index: r.done_index, doneNext: r.done_next, keepDays: r.keep_days, autoMode: r.auto_mode, autoDecisions, autoExceptions: r.auto_exceptions, autoNext };
      });
    } else if (r.kind === "auto_decisions") {
      setData(old => old ? { ...old, autoDecisions: mergeAuto(r.decisions, old.autoDecisions) } : old);
    }
  }, []);
  const put = useCallback((r: FrontResponse) => {
    if (r.kind === "asks" && reloader.current) {
      // 조작 응답의 전체 사본은 전송 중 더 낡아질 수 있다. 새 사건 목록을 덮지 않고 최신 조회로 잇는다.
      ++seq.current;
      void reloader.current.refresh();
    } else accept(r);
  }, [accept]);
  const moreAuto = useCallback(async () => {
    if (!data?.autoNext) return;
    const r = await call({ op: "auto_decisions", project: "", before: data.autoNext, id: null });
    put(r);
    if (r.kind === "auto_decisions") setData(old => old ? { ...old, autoNext: r.next } : old);
  }, [data?.autoNext, put]);
  const forgetAuto = useCallback(async (ids: string[], all: boolean) => {
    const r = await call({ op: "auto_decision_forget", project: "", ids, all });
    put(r);
    if (!all) setData(old => old ? { ...old, autoDecisions: (old.autoDecisions ?? []).filter(d => !ids.includes(d.id)) } : old);
  }, [put]);
  const moreDone = useCallback(async () => {
    if (!data?.doneNext) return;
    const n = seq.current, cursor = data.doneNext;
    const r = await call({ op: "ask_history", before: cursor, id: null });
    if (r.kind !== "ask_history" || n !== seq.current) return;
    setData(old => old && old.doneNext === cursor ? { ...old, done: mergeAskPage(old.done, r.done, old.index), doneNext: r.next } : old);
  }, [data?.doneNext]);
  const loadDone = useCallback(async (id: string) => {
    const r = await call({ op: "ask_history", before: null, id });
    if (r.kind !== "ask_history") throw new Error("답변 이력 응답이 아니다");
    if (!r.done.length) throw new Error("답변 기록을 찾을 수 없다");
    // 원문은 답할 때의 불변 사본이다. 새 색인에서 삭제된 ID는 mergeAskPage가 거른다.
    setData(old => old ? { ...old, done: mergeAskPage(old.done, r.done, old.index) } : old);
  }, []);
  useEffect(() => {
    const reader = new AsksReload(() => call({ op: "asks", paged: true }), accept, ignoredFailure("답할 것 새로 읽기", () => {}));
    reloader.current = reader;
    // host는 매 연결마다 첫 asks를 보낸다. up과 첫 사건으로 같은 목록을 두 번 요청하지 않는다.
    const unwatch = onAsksChanged(() => { ++seq.current; void reader.refresh(); });
    return () => { unwatch(); reader.close(); reloader.current = null; };
  }, [accept]);
  return { data, put, moreAuto, moreDone, loadDone, forgetAuto };
}

/** 열쇠 → 본문에 그릴 것 */
export function useAskSelection(key: string | null, open: OpenAsk[], done: AskRecord[], auto: AutoDecision[] = []): AskSelection {
  const previous = useRef<{ key: string | null; ask: OpenAsk } | null>(null);
  const selection = useMemo<AskSelection>(() => {
    if (key?.startsWith("a:")) {
      const decision = auto.find(d => d.id === key.slice(2)) ?? (key === "a:" ? auto[0] : undefined);
      return decision ? { kind: "auto", decision } : { kind: "none" };
    }
    if (key?.startsWith("d:")) {
      const r = done.find((x) => x.id === key.slice(2));
      if (r) return { kind: "done", record: r };
      return { kind: "missing", id: key.slice(2) };
    }
    const a = key ? open.find((x) => x.key === key) : undefined;
    if (a) return { kind: "open", ask: a };
    // 방금 답한 것(그 카드가 답할 것에서 빠졌다): 그때 남긴 기록을 연다
    const answered = key ? doneOf(key, done) : null;
    if (answered) return { kind: "done", record: answered };
    // q-ID 링크로 답하던 카드가 새 q-ID로 재게시돼도 같은 thread·같은 질문이면 잇는다.
    const before = previous.current;
    if (before?.ask.kind === "question" && key?.startsWith("q:")
      && (before.key === key || before.ask.thread.questions.items.some(item => item.id === key.slice(2)))) {
      const old = before.ask;
      const repost = open.find(x => x.kind === "question" && x.key === old.key && x.thread.project === old.thread.project
        && questionIdentity(x.thread.questions) === questionIdentity(old.thread.questions));
      if (repost) return { kind: "open", ask: repost };
    }
    // 영속 ID 링크가 닫혔거나 지워졌어도 다른 질문을 그 ID의 카드처럼 열지 않는다.
    if (/^q:(?:[a-z0-9_][a-z0-9_-]{0,47}\/)?[qt]-\d+$/.test(key ?? "")) return { kind: "missing", id: key!.slice(2) };
    return open[0] ? { kind: "open", ask: open[0] } : { kind: "none" };
  }, [key, open, done, auto]);
  useLayoutEffect(() => {
    previous.current = selection.kind === "open" ? { key, ask: selection.ask } : null;
  }, [key, selection]);
  return selection;
}

