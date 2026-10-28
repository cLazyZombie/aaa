// thread 의견 모음 화면(t-1222). 보고 글·diff 줄에 단 의견을 thread 화면에 모아 보이고, 자리로 돌아가거나 고치거나 지우고,
// 「메시지에 넣기」로 모두를 그 thread pane 입력 초안에 붙인다(참고 Orca `DiffNotesSendMenu.tsx`의 「All unsent notes」, 보낸 의견은 지움).
import { CornerUpLeft, Pencil, Trash2 } from "lucide-react";
import { useEffect, useMemo, useRef, useState, useSyncExternalStore, type RefObject } from "react";
import { NoteEditor } from "./ChatQuote";
import { MAX_NOTES, formatNotes, newNoteId, noteText, readNotes, sortNotes, subscribeNotes, writeNotes, type ReviewNote, type ReviewSource } from "./reviewNoteStore.ts";
import { diffRangeLabel } from "./selectionQuote.ts";
import { resolveAnchor, revealRange, type TextAnchor } from "./textAnchor.ts";
import { subscribeHandoff, takeReveal } from "./quoteHandoff.ts";

export interface ReviewNotesApi {
  notes: ReviewNote[];
  /** 더했으면 true(상한·저장 실패면 false) */
  add: (source: ReviewSource, comment: string) => boolean;
  remove: (ids: readonly string[]) => void;
  edit: (id: string, comment: string) => boolean;
}

export function useReviewNotes(project: string, thread: string): ReviewNotesApi {
  const notes = useSyncExternalStore(subscribeNotes, () => readNotes(project, thread));
  return useMemo(() => ({
    notes,
    add: (source, comment) => {
      const list = readNotes(project, thread);
      if (list.length >= MAX_NOTES) return false;
      return writeNotes(project, thread, [...list, { id: newNoteId(), source, comment: comment.trim(), at: Date.now() }]);
    },
    remove: (ids) => { writeNotes(project, thread, readNotes(project, thread).filter((n) => !ids.includes(n.id))); },
    edit: (id, comment) => writeNotes(project, thread, readNotes(project, thread).map((n) => (n.id === id ? { ...n, comment: comment.trim() } : n))),
  }), [notes, project, thread]);
}

/** 의견 자리 이름: 「보고 r-4001」·「src/a.rs 새 12~15줄」 */
export function noteLabel(note: ReviewNote): string {
  const s = note.source;
  if (s.kind === "note" || s.kind === "media") return s.section ? `노트 「${s.section}」` : "노트";
  return s.kind === "report" ? `보고 ${s.report}` : `${s.path} ${diffRangeLabel(s.range)}`;
}

const HIGHLIGHT = "aaa-review-note";
const painted = new Map<symbol, Range[]>();
function repaint() {
  if (typeof CSS === "undefined" || !("highlights" in CSS) || typeof Highlight === "undefined") return;
  const ranges = [...painted.values()].flat();
  if (ranges.length === 0) CSS.highlights.delete(HIGHLIGHT);
  else CSS.highlights.set(HIGHLIGHT, new Highlight(...ranges));
}

/** 의견을 단 글을 칠한다(CSS Highlight, 없는 브라우저는 칠하지 않는다). 글이 바뀌면 다시 찾는다 */
export function useNoteHighlights(root: RefObject<HTMLElement | null>, anchors: readonly TextAnchor[], text: string) {
  const id = useRef(Symbol("notes"));
  useEffect(() => {
    const el = root.current;
    const key = id.current;
    painted.set(key, el ? anchors.flatMap((a) => resolveAnchor(el, a) ?? []) : []);
    repaint();
    return () => {
      painted.delete(key);
      repaint();
    };
  }, [root, anchors, text]);
}

/** 이 보고 판을 그린 화면: 맡겨진 인용 자리를 고른다(보낸 말·의견 모음의 「위치」, t-1222) */
export function useReportReveal(project: string, report: string | null | undefined, root: RefObject<HTMLElement | null>, ready: boolean) {
  useEffect(() => {
    if (!ready || !report) return;
    const run = () => {
      const target = takeReveal((t) => t.kind === "report" && t.project === project && t.report === report);
      if (target?.kind !== "report") return;
      requestAnimationFrame(() => {
        const el = root.current?.querySelector(".report-markdown");
        const range = el ? resolveAnchor(el, target.anchor) : null;
        if (range) revealRange(range);
      });
    };
    run();
    return subscribeHandoff(run);
  }, [project, report, root, ready]);
}

export function ReviewNotesCard({ thread, api, onReveal, sendNotes }: {
  thread: string;
  api: ReviewNotesApi;
  onReveal: (note: ReviewNote) => void;
  /** 모은 글을 thread pane 초안에 넣는다. 없으면(돌고 있는 pane 없음) 넣을 수 없다 */
  sendNotes?: (text: string, done: (ok: boolean) => void) => void;
}) {
  const [editing, setEditing] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const notes = sortNotes(api.notes);
  if (notes.length === 0) return null;
  const send = () => {
    if (!sendNotes) return;
    setProblem(null);
    const ids = notes.map((n) => n.id);
    sendNotes(formatNotes(thread, notes), (ok) => {
      if (ok) api.remove(ids);
      else setProblem("메시지 초안에 넣지 못했다");
    });
  };
  return (
    <section className="card review-notes" data-testid="review-notes">
      <h2>의견 <span className="muted">{notes.length}</span></h2>
      <ul>
        {notes.map((n) => (
          <li key={n.id} className="review-note" data-testid="review-note">
            <div className="review-note-head">
              <code title={noteLabel(n)}>{noteLabel(n)}</code>
              <button type="button" className="ghost small" data-testid="review-note-reveal" aria-label={`${noteLabel(n)} 위치로`} onClick={() => onReveal(n)}>
                <CornerUpLeft className="i" aria-hidden="true" />위치
              </button>
              <button type="button" className="ghost small icon" aria-label="의견 고치기" title="의견 고치기" onClick={() => setEditing(n.id)}>
                <Pencil className="i" aria-hidden="true" />
              </button>
              <button type="button" className="ghost small icon" data-testid="review-note-remove" aria-label="의견 지우기" title="의견 지우기" onClick={() => api.remove([n.id])}>
                <Trash2 className="i" aria-hidden="true" />
              </button>
            </div>
            <blockquote className="review-note-text">{noteText(n)}</blockquote>
            {editing === n.id ? (
              <NoteEditor initial={n.comment} submitLabel="저장" onCancel={() => setEditing(null)}
                onSubmit={(comment) => { if (!api.edit(n.id, comment)) return false; setEditing(null); return true; }} />
            ) : n.comment && <p className="review-note-comment">{n.comment}</p>}
          </li>
        ))}
      </ul>
      <div className="review-notes-foot">
        {problem ? <span className="muted" role="status">{problem}</span> : !sendNotes && <span className="muted" role="status">돌고 있는 thread pane이 없다</span>}
        <button type="button" className="primary small" data-testid="review-notes-send" disabled={!sendNotes} onClick={send}>메시지에 넣기</button>
      </div>
    </section>
  );
}
