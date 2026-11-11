import { logIgnoredFailure } from "./errorDiagnostic.ts";
// 「답할 것」 패널의 목록(plan §8.345, 사용자 결정 10-21 1B 2A 3A 4A 5A): thread 질문·coordinator 확인 필요 post·할 일 확인 필요를
// 한 목록에 모으고, 답한 것은 host 기록(답할 때의 원문)이다. 주소 `#ask=<열쇠>`가 고른 것 하나를 본문 카드로 연다. 열쇠는 종류 글자와
// 대상이다: `q:<thread>` 열린 질문, `r:<inbox 항목>` 확인 필요 post, `t:<task 제목>` 할 일, `d:<기록>` 답한 것.
import type { AutoMode, AutoDecision, AskRecord, AskRecordSummary, TaskView, ThreadQuestions, ThreadView, UserRequestView } from "./gen/proto";
import { normalizeAppTarget } from "./appIds.ts";
import { appRefOf } from "./appRef.ts";
import type { AppRefLook } from "./AppRefChip";
import { isVideoPath } from "./reportMedia.ts";

export type AsksData = { requests: UserRequestView[]; done: AskRecord[]; index?: AskRecordSummary[]; doneNext?: string | null; keepDays: number; autoMode?: AutoMode; autoDecisions?: AutoDecision[]; autoExceptions?: number; autoNext?: string | null };

export type AskBrief = AskRecord | AskRecordSummary;
export const questionIds = (r: AskBrief): string[] => "question_ids" in r ? r.question_ids : r.items.flatMap(q => q.id ? [q.id] : []);
export const answerIndex = (data: AsksData | null): AskBrief[] => data?.index ?? data?.done ?? [];

/** 새 색인이 삭제의 정본이다. 이미 읽은 원문은 아직 남은 ID만 보존한다. */
export function mergeAskPage(old: AskRecord[], fresh: AskRecord[], index?: AskRecordSummary[]): AskRecord[] {
  const known = index && new Set(index.map(r => r.id));
  const records = new Map([...old, ...fresh].filter(r => !known || known.has(r.id)).map(r => [r.id, r]));
  if (index) return index.flatMap(r => records.has(r.id) ? [records.get(r.id)!] : []);
  return [...records.values()].sort((a, b) => b.done_ms - a.done_ms);
}

/** host 첫 쪽으로 자동 결정 목록을 바꾼다. 「더 보기」로 읽은 이전 쪽(첫 쪽보다 오래된 것)은 이전 쪽이 남아 있을 때만 둔다:
 * 다른 기기에서 지운 결정이 첫 쪽에서 빠져도 이 기기에 남지 않게 한다. */
export function freshAuto(fresh: AutoDecision[], next: string | null, old: AutoDecision[] = []): AutoDecision[] {
  const oldest = fresh.at(-1);
  const older = next && oldest ? old.filter(d => d.at_ms < oldest.at_ms && !fresh.some(f => f.id === d.id)) : [];
  return [...fresh, ...older].sort((a, b) => b.at_ms - a.at_ms);
}

export type OpenAsk =
  | { kind: "question"; key: string; at: number; thread: ThreadView & { questions: ThreadQuestions } }
  | { kind: "request"; key: string; at: number; request: UserRequestView }
  | { kind: "task"; key: string; at: number; task: TaskView };

/** 답할 질문이 남은 thread인가 */
export const asks = (t: ThreadView): t is ThreadView & { questions: ThreadQuestions } => !!t.questions && t.questions.answer === null;

/** 답할 것: 질문, 확인 필요 post, 할 일 확인 필요 순, 그 안에서는 오래된 것부터 */
export function openAsks(threads: ThreadView[], data: AsksData | null, tasks: TaskView[]): OpenAsk[] {
  const questionThreads = threads.filter(asks);
  const qs: OpenAsk[] = questionThreads.map((t) => ({ kind: "question", key: `q:${t.id}`, at: t.updated_at_ms, thread: t }));
  // 제목의 질문 참조는 그 카드의 알림이다. 카드가 없으면 평소 확인 필요 post로 남긴다.
  const questionCards = new Set([
    ...threads.filter((t) => t.questions).flatMap((t) => [t.id, ...t.questions!.items.flatMap((q) => q.id ? [q.id] : [])]),
    ...answerIndex(data).filter((r) => r.kind === "question").flatMap((r) => [r.subject, ...questionIds(r)]),
  ].map(id => normalizeAppTarget(id, "qt")));
  const rs: OpenAsk[] = (data?.requests ?? []).filter((r) => !r.title.split(/\s+/).some((word) => {
    const ref = appRefOf(word);
    return ref?.kind === "question" && questionCards.has(ref.target);
  })).map((r) => ({ kind: "request", key: `r:${r.id}`, at: r.created_ms, request: r }));
  const ts: OpenAsk[] = tasks.filter((t) => t.attention).map((t) => ({ kind: "task", key: `t:${t.title}`, at: 0, task: t }));
  const byAt = (a: OpenAsk, b: OpenAsk) => a.at - b.at;
  return [...qs.sort(byAt), ...rs.sort(byAt), ...ts];
}

const latest = <T extends AskBrief>(done: T[], kind: AskRecord["kind"], subject: string): T | null => {
  let best: T | null = null;
  for (const r of done) if (r.kind === kind && r.subject === subject && (best === null || r.done_ms > best.done_ms)) best = r;
  return best;
};

/** thread의 가장 최근 답한 질문 기록(대화 칩·`#question=` 주소가 새 보고 뒤에도 「답함」을 본다) */
export const lastAnswer = <T extends AskBrief>(done: T[], thread: string): T | null => latest(done, "question", thread);

/**
 * 방금 답한 질문(t-1767): host가 thread에 답을 기록하면(thread 사건) 「답한 것」 기록을 다시 읽기 전에도 그 질문과 답으로 같은 모양의
 * 기록을 만든다. host 기록이 오면 그것으로 바뀐다. 물은 때와 지우기는 host 기록에만 있다(`asked_ms` 0은 모름이다).
 */
export function answeredNow(t: ThreadView): AskRecord | null {
  const q = t.questions;
  if (!q?.answer) return null;
  return {
    id: `now:${t.project}/${t.id}`, kind: "question", subject: t.id, title: t.title, report_id: t.report_id,
    asked_ms: 0, done_ms: q.answer.at_ms, by: q.answer.by, summary: q.answer.summary, items: q.items, answer: q.answer, files_base: q.files_base,
  };
}

/** 답한 것 카드의 React key: 방금 답한 것(`answeredNow`)과 뒤에 온 host 기록이 같은 카드로 이어진다 */
export const doneCardKey = (r: AskRecord): string => (r.kind === "question" && r.answer ? `q:${r.subject}:${r.done_ms}` : r.id);

const KEY_KIND: Record<string, AskRecord["kind"] | undefined> = { "q:": "question", "r:": "request", "t:": "task" };

/** 답할 것 열쇠(`q:`·`r:`·`t:`)가 답해서 목록에서 빠졌으면 그때 남긴 기록(카드가 그 기록으로 바뀐다) */
export function doneOf<T extends AskBrief>(key: string, done: T[]): T | null {
  const kind = KEY_KIND[key.slice(0, 2)];
  return kind ? latest(done, kind, key.slice(2)) : null;
}

/**
 * 주소의 질문(`#question=<thread>`, 알림·칩·터미널 링크)을 열쇠로: 답할 질문이 있으면 그 카드, 없으면 마지막으로 답한 그 질문,
 * 둘 다 없으면 null(목록만)
 */
export function questionKey(thread: string, threads: ThreadView[], done: AskBrief[], requests: UserRequestView[] = []): string | null {
  const t = threads.find((t) => asks(t) && t.questions.items.some((q) => q.id === thread));
  if (t) return `q:${t.id}`;
  const request = requests.find((r) => r.question_id === thread || r.questions?.items.some((q) => q.id === thread));
  if (request) return `r:${request.id}`;
  const record = done.find((r) => r.question_id === thread || questionIds(r).includes(thread));
  if (record) return `d:${record.id}`;
  if (threads.some((t) => t.id === thread && asks(t))) return `q:${thread}`;
  const r = lastAnswer(done, thread);
  return r ? `d:${r.id}` : null;
}

const choiceLetter = (c: { letter?: string }, i: number) => c.letter ?? String.fromCharCode(65 + i);

/** 칩이 가리킨 질문 하나(q ID, 질문이 하나뿐인 카드는 thread·post 번호도): 일부 답이 있으면 그 답, 없으면 칩에서 고를 선택지 */
function itemLook(target: string, label: string, q: ThreadQuestions, project?: string): AppRefLook | null {
  const n = q.items.length === 1 && q.items[0]!.id && !q.items.some((item) => item.id === target) ? 0 : q.items.findIndex((item) => item.id === target);
  const item = q.items[n];
  if (!item?.id) return null;
  const number = item.source_number ?? n + 1;
  const pick = q.draft?.picks[n];
  if (pick != null) return { label, tone: "done", state: `답함 · ${number}${choiceLetter(item.choices?.[pick] ?? {}, pick)}` };
  if (q.draft?.comments?.[n]) return { label, tone: "done", state: `답함 · ${number}직접` };
  if (!item.choices?.length) return null;
  return {
    label, tone: "ask", state: "답할 것",
    ask: {
      id: item.id, project, text: item.text,
      // 검수 카드의 B(고칠 점)는 글이 필요하다: 칩에서 고르면 카드를 연다.
      choices: item.choices.map((c, i) => ({ letter: choiceLetter(c, i), label: c.label, recommended: c.recommended, card: q.review === true && i === 1 })),
    },
  };
}

/** 카드와 같은 대상으로 상태를 찾는다. q ID는 thread·coordinator·보존한 답 모두에 속할 수 있다. */
export function questionRefLook(target: string, threads: ThreadView[], done: AskBrief[], requests: UserRequestView[] = []): AppRefLook {
  const label = `${target} 질문`;
  const key = questionKey(target, threads, done, requests);
  if (key?.startsWith("q:")) {
    const t = threads.find((t) => t.id === key.slice(2))!;
    return itemLook(target, label, t.questions!, t.project) ?? { label: `${label} ${t.questions!.items.length}개`, tone: "ask", state: "답할 것" };
  }
  if (key?.startsWith("r:")) {
    const r = requests.find((r) => r.id === key.slice(2));
    return (r?.questions && itemLook(target, label, r.questions)) || { label, tone: "ask", state: "답할 것" };
  }
  if (key?.startsWith("d:")) {
    const rec = done.find((r) => r.id === key.slice(2))!;
    const summary = "question_ids" in rec ? rec.answer_summary : rec.answer?.summary;
    return { label, tone: "done", state: summary ? `답함 · ${summary}` : rec.summary };
  }
  // 옛 host가 답 기록을 주지 않아도 현재 thread에 남은 답은 표시한다.
  const q = threads.find((t) => t.id === target || t.questions?.items.some((q) => q.id === target))?.questions;
  if (q?.answer) return { label, tone: "done", state: `답함 · ${q.answer.summary}` };
  return { label, tone: "plain", state: null };
}

/** 본문 Markdown의 `library/…`·`report.md`를 host 주소로(그 밖은 null: 그림은 대체 글, 링크는 보통 글) */
export function askFileUrl(base: string | null | undefined, src: string): string | null {
  if (!base) return null;
  const path = src.trim().replace(/^\.\//, "");
  if (path !== "report.md" && !path.startsWith("library/")) return null;
  const clean = path.replace(/[?#].*$/s, "");
  let parts: string[];
  try {
    parts = clean.split("/").map((p) => decodeURIComponent(p));
  } catch (ignoredError) { logIgnoredFailure(ignoredError, "asksModel.ts:99");
    return null;
  }
  if (parts.some((p) => p === "" || p === "." || p === ".." || p.startsWith("."))) return null;
  const fragment = path.includes("#") ? path.slice(path.indexOf("#")) : "";
  return base + parts.map(encodeURIComponent).join("/") + fragment;
}

/** 선택지 그림만 든 본문인가(카드 위 큰 그림으로 그린다) */
export function choicePicture(body: string | null | undefined): { src: string; alt: string; rest: string } | null {
  if (!body) return null;
  const m = /^!\[([^\]]*)\]\(([^)\s]+)\)\s*/.exec(body);
  if (!m) return null;
  if (isVideoPath(m[2]!)) return null;
  return { alt: m[1]!, src: m[2]!, rest: body.slice(m[0].length).trim() };
}

const ago = (ms: number, now: number) => {
  const d = Math.floor((now - ms) / 60000);
  if (d < 1) return "방금";
  if (d < 60) return `${d}분 전`;
  if (d < 60 * 24) return `${Math.floor(d / 60)}시간 전`;
  return `${Math.floor(d / 1440)}일 전`;
};

/** 목록 줄의 아래 글(종류 · 상태 · 때) */
export function openMeta(a: OpenAsk, now: number): string {
  if (a.kind === "question") return `질문 ${a.thread.questions.items.length}개 · ${ago(a.at, now)}`;
  if (a.kind === "request") return `coordinator · 확인 필요 · ${ago(a.at, now)}`;
  return "할 일 · 확인 필요";
}

export function doneMeta(r: AskBrief, now: number): string {
  return `${r.summary} · ${ago(r.done_ms, now)} · ${r.by}`;
}

export function doneTitle(r: AskBrief): string {
  return r.kind === "question" ? `${r.subject} · ${r.title || r.subject}` : r.title;
}
