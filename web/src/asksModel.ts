// 「답할 것」 패널의 목록(plan §8.345, 사용자 결정 10-21 1B 2A 3A 4A 5A): thread 질문·coordinator 확인 필요 post·할 일 확인 필요를
// 한 목록에 모으고, 답한 것은 host 기록(답할 때의 원문)이다. 주소 `#ask=<열쇠>`가 고른 것 하나를 본문 카드로 연다. 열쇠는 종류 글자와
// 대상이다: `q:<thread>` 열린 질문, `r:<inbox 항목>` 확인 필요 post, `t:<task 제목>` 할 일, `d:<기록>` 답한 것.
import type { AskRecord, TaskView, ThreadQuestions, ThreadView, UserRequestView } from "./gen/proto";
import { appRefOf } from "./appRef.ts";

export type AsksData = { requests: UserRequestView[]; done: AskRecord[]; keepDays: number };

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
    ...threads.filter((t) => t.questions).map((t) => t.id),
    ...(data?.done ?? []).filter((r) => r.kind === "question").map((r) => r.subject),
  ]);
  const rs: OpenAsk[] = (data?.requests ?? []).filter((r) => !r.title.split(/\s+/).some((word) => {
    const ref = appRefOf(word);
    return ref?.kind === "question" && questionCards.has(ref.target);
  })).map((r) => ({ kind: "request", key: `r:${r.id}`, at: r.created_ms, request: r }));
  const ts: OpenAsk[] = tasks.filter((t) => t.attention).map((t) => ({ kind: "task", key: `t:${t.title}`, at: 0, task: t }));
  const byAt = (a: OpenAsk, b: OpenAsk) => a.at - b.at;
  return [...qs.sort(byAt), ...rs.sort(byAt), ...ts];
}

const latest = (done: AskRecord[], kind: AskRecord["kind"], subject: string): AskRecord | null => {
  let best: AskRecord | null = null;
  for (const r of done) if (r.kind === kind && r.subject === subject && (best === null || r.done_ms > best.done_ms)) best = r;
  return best;
};

/** thread의 가장 최근 답한 질문 기록(대화 칩·`#question=` 주소가 새 보고 뒤에도 「답함」을 본다) */
export const lastAnswer = (done: AskRecord[], thread: string): AskRecord | null => latest(done, "question", thread);

const KEY_KIND: Record<string, AskRecord["kind"] | undefined> = { "q:": "question", "r:": "request", "t:": "task" };

/** 답할 것 열쇠(`q:`·`r:`·`t:`)가 답해서 목록에서 빠졌으면 그때 남긴 기록(카드가 그 기록으로 바뀐다) */
export function doneOf(key: string, done: AskRecord[]): AskRecord | null {
  const kind = KEY_KIND[key.slice(0, 2)];
  return kind ? latest(done, kind, key.slice(2)) : null;
}

/**
 * 주소의 질문(`#question=<thread>`, 알림·칩·터미널 링크)을 열쇠로: 답할 질문이 있으면 그 카드, 없으면 마지막으로 답한 그 질문,
 * 둘 다 없으면 null(목록만)
 */
export function questionKey(thread: string, threads: ThreadView[], done: AskRecord[]): string | null {
  if (threads.some((t) => t.id === thread && asks(t))) return `q:${thread}`;
  const r = lastAnswer(done, thread);
  return r ? `d:${r.id}` : null;
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
  } catch {
    return null;
  }
  if (parts.some((p) => p === "" || p === "." || p === ".." || p.startsWith("."))) return null;
  return base + parts.map(encodeURIComponent).join("/");
}

/** 선택지 그림만 든 본문인가(카드 위 큰 그림으로 그린다) */
export function choicePicture(body: string | null | undefined): { src: string; alt: string; rest: string } | null {
  if (!body) return null;
  const m = /^!\[([^\]]*)\]\(([^)\s]+)\)\s*/.exec(body);
  if (!m) return null;
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

export function doneMeta(r: AskRecord, now: number): string {
  return `${r.summary} · ${ago(r.done_ms, now)} · ${r.by}`;
}

export function doneTitle(r: AskRecord): string {
  return r.kind === "question" ? `${r.subject} · ${r.title || r.subject}` : r.title;
}
