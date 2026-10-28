import { logIgnoredFailure } from "./errorDiagnostic.ts";
// thread 의견 모음(t-1222). 보고 글에서 고른 글·diff에서 고른 줄에 단 의견을 thread마다 이 기기에 쌓고, 「메시지에 넣기」로
// 한꺼번에 그 thread pane 입력 초안에 붙인다. 참고 Orca `src/shared/diff-comment-types.ts`(MIT, cbbaa2d7: 원천 diff·markdown,
// 고른 원문·줄·diff 기준을 남김)·`DiffNotesSendMenu.tsx`(아직 보내지 않은 의견을 묶어 보내고, 보낸 것은 지움)·
// `markdown-review-notes.ts`(파일·줄 차례로 정렬). 다른 곳: Orca는 worktree 메타에 저장하고 바로 agent에 보낸다. aaa는 이 기기
// localStorage에 두고, 보내기는 사람이 초안에서 누른다(t-1204 인용과 같은 규칙).
import type { DiffScope } from "./gen/proto";
import { diffScopeLabel, formatDiffQuote, formatReportQuote, withComment, type DiffQuoteRange } from "./selectionQuote.ts";
import type { TextAnchor } from "./textAnchor.ts";

export type ReviewSource =
  | { kind: "report"; report: string; anchor: TextAnchor }
  | { kind: "diff"; scope: DiffScope; base: string; base_sha: string; path: string; range: DiffQuoteRange; rows: { kind: " " | "-" | "+"; text: string }[] };

export interface ReviewNote {
  id: string;
  source: ReviewSource;
  comment: string;
  at: number;
}

/** 한 thread에 쌓는 의견 상한(넘치면 더하지 않는다) */
export const MAX_NOTES = 100;
const KEY = "aaa:review-notes:";
const keyOf = (project: string, thread: string) => `${KEY}${project}/${thread}`;

function valid(v: unknown): v is ReviewNote {
  if (typeof v !== "object" || v === null) return false;
  const n = v as Record<string, unknown>;
  const s = n.source as Record<string, unknown> | null;
  if (typeof n.id !== "string" || typeof n.comment !== "string" || typeof n.at !== "number" || typeof s !== "object" || s === null) return false;
  if (s.kind === "report") return typeof s.report === "string" && typeof s.anchor === "object" && s.anchor !== null && typeof (s.anchor as TextAnchor).text === "string";
  return s.kind === "diff" && typeof s.path === "string" && Array.isArray(s.rows) && typeof s.range === "object" && s.range !== null;
}

const listeners = new Set<() => void>();
const cache = new Map<string, { raw: string | null; notes: ReviewNote[] }>();

export function readNotes(project: string, thread: string, storage: Pick<Storage, "getItem"> = window.localStorage): ReviewNote[] {
  const key = keyOf(project, thread);
  let raw: string | null = null;
  try {
    raw = storage.getItem(key);
  } catch (ignoredError) { logIgnoredFailure(ignoredError, "reviewNoteStore.ts:readNotes"); }
  const hit = cache.get(key);
  if (hit && hit.raw === raw) return hit.notes;
  let notes: ReviewNote[] = [];
  try {
    const parsed: unknown = raw === null ? [] : JSON.parse(raw);
    notes = Array.isArray(parsed) ? parsed.filter(valid) : [];
  } catch (ignoredError) { logIgnoredFailure(ignoredError, "reviewNoteStore.ts:parse"); }
  cache.set(key, { raw, notes });
  return notes;
}

/** 바꾼 목록을 쓴다. 쓰지 못하면(사생활 모드·꽉 참) false */
export function writeNotes(project: string, thread: string, notes: readonly ReviewNote[], storage: Pick<Storage, "setItem" | "removeItem"> = window.localStorage): boolean {
  try {
    if (notes.length === 0) storage.removeItem(keyOf(project, thread));
    else storage.setItem(keyOf(project, thread), JSON.stringify(notes));
  } catch (ignoredError) { logIgnoredFailure(ignoredError, "reviewNoteStore.ts:writeNotes");
    return false;
  }
  for (const listener of listeners) listener();
  return true;
}

export function subscribeNotes(listener: () => void): () => void {
  listeners.add(listener);
  const onStorage = (e: StorageEvent) => { if (e.key === null || e.key.startsWith(KEY)) listener(); };
  window.addEventListener("storage", onStorage);
  return () => {
    listeners.delete(listener);
    window.removeEventListener("storage", onStorage);
  };
}

export function newNoteId(): string {
  return `n${Date.now().toString(36)}${Math.random().toString(36).slice(2, 8)}`;
}

/** 원문(보고 글은 고른 글, diff는 표지 없는 줄) */
export function noteText(note: ReviewNote): string {
  return note.source.kind === "report" ? note.source.anchor.text : note.source.rows.map((r) => r.text).join("\n");
}

/** 의견 하나의 글(인용 머리·원문·의견) */
export function formatNote(thread: string, note: ReviewNote): string | null {
  const s = note.source;
  const block = s.kind === "report"
    ? formatReportQuote({ source: thread, report: s.report, text: s.anchor.text })
    : formatDiffQuote({ source: thread, scope: diffScopeLabel(s.scope, s.base, s.base_sha), path: s.path, range: s.range, rows: s.rows });
  return block === null ? null : withComment(block, note.comment);
}

/** 보낼 차례(참고 sortMarkdownReviewNotes): 보고 글이 먼저(판·글 차례), diff는 파일·줄 차례, 같으면 단 차례 */
export function sortNotes(notes: readonly ReviewNote[]): ReviewNote[] {
  const rank = (n: ReviewNote) => (n.source.kind === "report" ? 0 : 1);
  return [...notes].sort((a, b) => {
    if (rank(a) !== rank(b)) return rank(a) - rank(b);
    const sa = a.source, sb = b.source;
    if (sa.kind === "report" && sb.kind === "report") {
      if (sa.report !== sb.report) return sa.report.localeCompare(sb.report);
      if (sa.anchor.start !== sb.anchor.start) return sa.anchor.start - sb.anchor.start;
    } else if (sa.kind === "diff" && sb.kind === "diff") {
      if (sa.path !== sb.path) return sa.path.localeCompare(sb.path);
      if (sa.range.start !== sb.range.start) return sa.range.start - sb.range.start;
    }
    return a.at - b.at;
  });
}

/** 모은 의견을 빈 줄로 이은 글 */
export function formatNotes(thread: string, notes: readonly ReviewNote[]): string {
  return sortNotes(notes).flatMap((n) => formatNote(thread, n) ?? []).join("\n\n");
}
