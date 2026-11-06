import { ProviderError } from "./ProviderError";
// 아이디어 본문(t-1260, 기획 t-1255 결정 6·8·10·12·16~18·24, 시안 r-4673): 노트와 전용 thread 대화를 나란히 둔다(목록은 사이드바).
// 노트 보기는 보고 글과 같은 Markdown 렌더러(그림·영상), 고치기는 그냥 textarea다. 노트 글·그림에 단 의견은 이 기기에 모았다가
// 「입력창에 넣기」로 대화 입력창에 한 번에 붙인다(t-1222 의견 모음). 대화창은 thread pane의 대화 보기와 입력창을 그대로 쓴다.
// 휴대폰은 「목록 | 노트 | 대화」가 한 줄로 놓인 페이지처럼 쓸어 넘긴다(목록은 앱 drawer).
// 참고(구성만): ChatGPT Canvas·Open Canvas(MIT, 문서와 대화 나란히), Hypothesis(BSD-2, 고른 글 자리 다시 찾기 — textAnchor),
// GitHub Discussions·Jira Product Discovery(아이디어 → 할 일 여럿, 양쪽 링크), spec-kit `/clarify`(MIT, 정리본 모양).
import "./IdeaView.css";

import { ArrowDown, ArrowUp, ClipboardPaste, Copy, Download, Ellipsis, ImagePlus, ListTodo, MessageSquarePlus, Pencil, RefreshCw, Trash2, X } from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore, type ClipboardEvent, type DragEvent } from "react";
import { ApiError, call, uploadIdeaFile } from "./api";
import { ChatComposer, MAX_COMPOSER_CHARS, restorePending, savePending, type PendingSend } from "./ChatComposer";
import { ChatPanel } from "./Chat";
import { NoteEditor } from "./ChatQuote";
import { DrawerToggle, HeadMore, HeadNote } from "./drawer";
import type { ChatMessage, IdeaNote, IdeaSummary, IdeaTaskDraft, PaneView, ThreadView } from "./gen/proto";
import { composerDrafts } from "./herdr/composerDraft.ts";
import { MODAL, editable, scrolledAside, selecting } from "./herdr/edgeSwipe.ts";
import {
  IDEA_MEDIA_ACCEPT, ideaFileUrl, ideaMediaFile, ideaMediaSource, ideaMediaSpot, ideaSwipe, movedMedia, sectionBefore, firstDraft,
  withEditNote, withMediaAlt, withMediaName, withoutMedia, type IdeaPage,
} from "./ideasModel";
import { useLightbox } from "./Lightbox";
import { ReportMarkdown } from "./ReportsView";
import { formatNotes, noteText, sortNotes } from "./reviewNoteStore.ts";
import { noteLabel, useReviewNotes } from "./ReviewNotes";
import { appendQuote } from "./selectionQuote.ts";
import { useMedia } from "./settings";
import { paneAgentStatus } from "./sidebarModel";
import { filesOf } from "./TaskAttachments";
import { IdeaResizer } from "./IdeaResizer";
import { usePageVisible } from "./pageVisible";

/** 노트 하나를 읽고, host가 바꿨다고 알리면(`ideas` 사건 → 목록 판이 바뀜) 다시 읽는다 */
function useIdeaNote(target: string | null, summary: IdeaSummary | undefined) {
  const [note, setNote] = useState<IdeaNote | null>(null);
  const [error, setError] = useState<string | null>(null);
  const stamp = summary ? `${summary.rev}:${summary.unseen}:${summary.status}:${summary.updated_ms}:${summary.thread ?? ""}` : "";
  const [again, setAgain] = useState(0);
  useEffect(() => {
    if (!target) { setNote(null); return; }
    let live = true;
    call({ op: "idea_read", idea: target }).then(
      (r) => { if (live && r.kind === "idea") { setNote(r.note); setError(null); } },
      (e: unknown) => { if (live) setError(e instanceof ApiError ? e.message : String(e)); },
    );
    return () => { live = false; };
  }, [target, stamp, again]);
  return { note, setNote, error, setError, reread: () => setAgain((n) => n + 1) };
}

/** 올리는 중·실패한 파일 하나. `place`는 올린 이름을 받아 노트에 둔다(보기: 노트 끝에 붙여 저장, 고치기: 커서 자리) */
type MediaUpload = { id: number; name: string; file: File | null; state: "uploading" | "failed"; error?: string; place: (names: string[]) => Promise<void> | void };

/** 그림·영상 올리기(t-1575): 하나씩 차례로 올리고 파일마다 상태를 둔다. 못 받는 파일은 올리지 않고 까닭을 보인다 */
function useMediaUploads(project: string, idea: string, onError: (e: string | null) => void) {
  const [items, setItems] = useState<MediaUpload[]>([]);
  const seq = useRef(0);
  const queue = useRef<Promise<void>>(Promise.resolve());
  const run = (entries: MediaUpload[], place: MediaUpload["place"]) => {
    queue.current = queue.current.then(async () => {
      const names: string[] = [];
      for (const e of entries) {
        try {
          names.push(await uploadIdeaFile(project, idea, e.file!));
          setItems((cur) => cur.filter((x) => x.id !== e.id));
        } catch (err) {
          const error = err instanceof ApiError ? err.message : String(err);
          setItems((cur) => cur.map((x) => x.id === e.id ? { ...x, state: "failed", error } : x));
        }
      }
      if (names.length === 0) return;
      try {
        await place(names);
        onError(null);
      } catch (err) {
        onError(`그림을 노트에 넣지 못했다: ${err instanceof ApiError ? err.message : String(err)}`);
      }
    });
    return queue.current;
  };
  const add = (files: File[], place: MediaUpload["place"]) => {
    const fresh: MediaUpload[] = [];
    const rejected: MediaUpload[] = [];
    for (const f of files) {
      const ok = ideaMediaFile(f);
      const name = f.name || "붙여 넣은 파일";
      if (typeof ok === "string") rejected.push({ id: ++seq.current, name, file: null, state: "failed", error: ok, place });
      else fresh.push({ id: ++seq.current, name, file: ok, state: "uploading", place });
    }
    setItems((cur) => [...cur, ...fresh, ...rejected]);
    return fresh.length > 0 ? run(fresh, place) : Promise.resolve();
  };
  const retry = (id: number) => {
    const e = items.find((x) => x.id === id);
    if (!e?.file) return;
    const again = { ...e, state: "uploading" as const, error: undefined };
    setItems((cur) => cur.map((x) => x.id === id ? again : x));
    void run([again], e.place);
  };
  const dismiss = (id: number) => setItems((cur) => cur.filter((x) => x.id !== id));
  return { items, add, retry, dismiss, busy: items.some((x) => x.state === "uploading") };
}
type MediaUploads = ReturnType<typeof useMediaUploads>;

function UploadList({ uploads }: { uploads: MediaUploads }) {
  if (uploads.items.length === 0) return null;
  return (
    <ul className="idea-uploads" data-testid="idea-uploads">
      {uploads.items.map((u) => (
        <li key={u.id} data-state={u.state} data-testid="idea-upload">
          <span className="idea-upload-name">{u.name}</span>
          <span className="muted">{u.state === "uploading" ? "올리는 중…" : u.error}</span>
          {u.state === "failed" && u.file && <button type="button" className="ghost small" data-testid="idea-upload-retry" onClick={() => uploads.retry(u.id)}>다시</button>}
          {u.state === "failed" && <button type="button" className="ghost small icon" aria-label="닫기" onClick={() => uploads.dismiss(u.id)}><X className="i" aria-hidden="true" /></button>}
        </li>
      ))}
    </ul>
  );
}

/** 클립보드의 그림(iOS는 그 자리에 「붙여넣기」 허용 말풍선을 띄운다). 읽을 수 없는 브라우저면 null */
async function clipboardMedia(): Promise<File[] | null> {
  const clip = navigator.clipboard as Clipboard | undefined;
  if (!clip || typeof clip.read !== "function") return null;
  const out: File[] = [];
  for (const item of await clip.read()) {
    const type = item.types.find((t) => /^image\/(png|jpeg|gif|webp)$/.test(t));
    if (!type) continue;
    out.push(new File([await item.getType(type)], `clipboard.${type === "image/jpeg" ? "jpg" : type.slice(6)}`, { type }));
  }
  return out;
}

export function IdeaView({ shown, target, ideas, threads, panes, canControl, onOpen, onOpenLink }: {
  shown: boolean;
  /** 연 아이디어(`n-0001`) */
  target: string | null;
  ideas: IdeaSummary[];
  /** 모든 thread(아이디어 thread 포함) */
  threads: ThreadView[];
  panes: PaneView[];
  canControl: boolean;
  onOpen: (id: string) => void;
  onOpenLink?: (href: string) => boolean;
}) {
  const summary = ideas.find((i) => i.short_id === target);
  const { note, setNote, error, setError, reread } = useIdeaNote(target, summary);
  const narrow = useMedia("(max-width: 768px)");
  const [page, setPage] = useState<IdeaPage>("note");
  const done = note?.idea.status === "done";
  const [editing, setEditing] = useState<{ base: number; text: string } | null>(null);
  const [handoff, setHandoff] = useState(false);
  const [busy, setBusy] = useState(false);
  useEffect(() => { setEditing(null); setHandoff(false); setPage("note"); }, [target]);
  // 화면을 열면 쉬느라 내린 thread를 조용히 잇는다(결정 20). 아직 말하지 않은 아이디어는 그대로 둔다
  const woke = useRef<string | null>(null);
  // 다른 화면에 다녀오면 다시 연 것이다(본문은 숨긴 채 남아 있다)
  useEffect(() => { if (!shown) woke.current = null; }, [shown]);
  useEffect(() => {
    if (!shown || !canControl || !note || note.idea.status !== "open" || !note.idea.thread || woke.current === note.idea.short_id) return;
    woke.current = note.idea.short_id;
    call({ op: "idea_wake", idea: note.idea.short_id }).then(
      (r) => { if (r.kind === "idea") setNote(r.note); },
      (e: unknown) => setError(e instanceof ApiError ? e.message : String(e)),
    );
  }, [shown, canControl, note, setNote, setError]);
  const notes = useReviewNotes(note?.project ?? "", `idea:${target ?? ""}`);
  const columns = useRef<HTMLDivElement>(null);
  const [notePercent, setNotePercent] = useState(60);
  const seen = useRef<string | null>(null);
  const visible = usePageVisible();
  useEffect(() => {
    if (!shown || !visible || editing || (narrow && page !== "note") || !note || note.idea.short_id !== target || !note.idea.unseen) return;
    const key = `${note.project}/${note.idea.id}/${note.rev}`;
    if (seen.current === key) return;
    seen.current = key;
    call({ op: "idea_seen", idea: note.idea.short_id, rev: note.rev }).catch((e: unknown) => {
      seen.current = null;
      setError(e instanceof ApiError ? e.message : String(e));
    });
  }, [shown, visible, editing, narrow, page, note, target, setError]);
  usePageSwipe(columns, narrow && shown && !editing, page, setPage);

  const save = async () => {
    if (!note || !editing || busy) return;
    setBusy(true);
    try {
      const r = await call({ op: "idea_save", idea: note.idea.short_id, base_rev: editing.base, text: editing.text });
      if (r.kind === "idea") { setNote(r.note); setEditing(null); setError(null); }
    } catch (e) {
      setError(e instanceof ApiError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  const copy = async () => {
    if (!note || busy) return;
    setBusy(true);
    try {
      const r = await call({ op: "idea_copy", idea: note.idea.short_id });
      if (r.kind === "idea") onOpen(r.note.idea.short_id);
    } catch (e) {
      setError(e instanceof ApiError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  const uploads = useMediaUploads(note?.project ?? "", note?.idea.short_id ?? "", setError);
  const picker = useRef<HTMLInputElement>(null);
  // 1.162 host는 노트에 그림 목록을 싣는다(옛 host에는 idea_attach를 보내지 않는다)
  const canAttach = !!note && canControl && !done && Array.isArray(note.media) && note.merging === null;
  /** 보기 화면에서 고른·붙인 그림: 올린 뒤 노트 끝에 붙여 바로 새 판으로(사용자 11-06 2A) */
  const attach = (files: File[]) => {
    if (!note || files.length === 0) return;
    const idea = note.idea.short_id;
    void uploads.add(files, async (names) => {
      const r = await call({ op: "idea_attach", idea, names });
      if (r.kind === "idea_attached") setNote(r.note);
    });
  };
  const pasteFromClipboard = async () => {
    try {
      const files = await clipboardMedia();
      if (files === null) setError("이 브라우저는 클립보드 그림을 읽지 못한다");
      else if (files.length === 0) setError("클립보드에 그림이 없다");
      else attach(files);
    } catch (e) {
      setError(`클립보드를 읽지 못했다: ${e instanceof Error ? e.message : String(e)}`);
    }
  };
  /** 그림 메뉴의 고침(설명·바꾸기·순서·지우기): 지금 판을 기준으로 새 판을 쓴다 */
  const edit = async (text: string) => {
    if (!note || busy) return;
    setBusy(true);
    try {
      const r = await call({ op: "idea_save", idea: note.idea.short_id, base_rev: note.rev, text });
      if (r.kind === "idea") { setNote(r.note); setError(null); }
    } catch (e) {
      setError(e instanceof ApiError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  if (!target) {
    return (
      <div className={`idea-screen${shown ? "" : " is-hidden"}`} inert={!shown} data-testid="idea-view" data-shown={shown ? "true" : "false"}>
        <div className="thread-view idea-view">
          <header className="term-head"><DrawerToggle /><HeadNote /><div className="term-title"><strong>아이디어</strong></div><HeadMore /></header>
          <p className="idea-empty" data-testid="idea-none">고른 아이디어 없음</p>
        </div>
      </div>
    );
  }
  const tools = note && canControl && (done ? (
    <button type="button" className="btn btn-primary" disabled={busy} data-testid="idea-copy" onClick={() => void copy()}><Copy className="i" aria-hidden="true" />복사해서 새 아이디어</button>
  ) : editing ? null : (
    <>
      {canAttach && (
        <>
          <button type="button" className="btn" data-testid="idea-media-add" disabled={busy} onClick={() => picker.current?.click()}><ImagePlus className="i" aria-hidden="true" />그림</button>
          <button type="button" className="btn icon" data-testid="idea-media-paste" aria-label="클립보드 그림 붙이기" title="클립보드 그림 붙이기" disabled={busy} onClick={() => void pasteFromClipboard()}><ClipboardPaste className="i" aria-hidden="true" /></button>
        </>
      )}
      <button type="button" className="btn" data-testid="idea-edit" disabled={note.merging !== null} onClick={() => setEditing({ base: note.rev, text: note.text })}><Pencil className="i" aria-hidden="true" />고치기</button>
      <button type="button" className="btn btn-primary" data-testid="idea-handoff" disabled={note.merging !== null} onClick={() => setHandoff(true)}><ListTodo className="i" aria-hidden="true" />할 일로 넘기기</button>
    </>
  ));
  const badge = notes.notes.length;
  return (
    <div className={`idea-screen${shown ? "" : " is-hidden"}`} inert={!shown} data-testid="idea-view" data-shown={shown ? "true" : "false"} data-page={narrow ? page : "both"}>
      <div className="thread-view idea-view">
        <header className="term-head idea-head">
          <DrawerToggle />
          <HeadNote />
          <div className="term-title idea-title">
            <strong data-testid="idea-title">{note?.idea.title ?? target}</strong>
            {note && <IdeaStatusChip note={note} />}
          </div>
          {narrow ? (
            <span className="idea-tabs" role="tablist" aria-label="노트와 대화">
              <button type="button" role="tab" aria-selected={page === "note"} data-testid="idea-tab-note" onClick={() => setPage("note")}>노트</button>
              <button type="button" role="tab" aria-selected={page === "chat"} data-testid="idea-tab-chat" onClick={() => setPage("chat")}>
                대화{badge > 0 && <span className="idea-badge" data-testid="idea-badge" aria-label={`의견 ${badge}개`}>{badge}</span>}
              </button>
            </span>
          ) : <span className="idea-head-tools">{tools}</span>}
          <HeadMore />
        </header>
        {error && <div className="banner err" role="alert">{error}</div>}
        {note?.media_error && <div className="banner err" role="alert">그림을 옮기지 못했다: {note.media_error}</div>}
        <div className="idea-columns" ref={columns} style={{ "--idea-note-percent": `${notePercent}%` } as React.CSSProperties}>
          <section id="idea-note-pane" className="idea-note-pane" aria-label="아이디어 노트" hidden={narrow && page !== "note"} data-testid="idea-note-pane"
            onPaste={(e: ClipboardEvent<HTMLElement>) => {
              if (editing || !canAttach || editable(e.target)) return;
              const files = filesOf(e.clipboardData);
              if (files.length) { e.preventDefault(); attach(files); }
            }}
            onDragOver={(e: DragEvent) => { if (!editing && canAttach && e.dataTransfer.types.includes("Files")) e.preventDefault(); }}
            onDrop={(e: DragEvent<HTMLElement>) => {
              if (editing || !canAttach) return;
              const files = filesOf(e.dataTransfer);
              if (files.length) { e.preventDefault(); attach(files); }
            }}>
            {narrow && tools && <div className="idea-note-tools">{tools}</div>}
            <input ref={picker} type="file" multiple hidden accept={IDEA_MEDIA_ACCEPT} data-testid="idea-media-input"
              onChange={(e) => { const files = Array.from(e.target.files ?? []); e.target.value = ""; attach(files); }} />
            <UploadList uploads={uploads} />
            {note && (editing
              ? <NoteEditorPane note={note} editing={editing} busy={busy} uploads={uploads} onChange={(text) => setEditing((cur) => cur && { ...cur, text })} onCancel={() => setEditing(null)} onSave={() => void save()} />
              : <NoteBody note={note} canNote={canControl && !done} canEdit={canAttach && !busy} uploads={uploads} onEdit={edit} onAdd={notes.add} anchors={notes.notes.flatMap((n) => n.source.kind === "note" ? [n.source.anchor] : [])} onOpenLink={onOpenLink} />)}
          </section>
          {!narrow && <IdeaResizer percent={notePercent} onResize={setNotePercent} />}
          <section className="idea-chat-pane" aria-label="대화" hidden={narrow && page !== "chat"} data-testid="idea-chat-pane"
            data-drawer-swipe={narrow && page === "chat" ? "off" : undefined}>
            {note && <IdeaChat note={note} threads={threads} panes={panes} canControl={canControl} notes={notes} active={shown && (!narrow || page === "chat")}
              onSaid={(n) => setNote(n)} onError={setError} onOpenLink={onOpenLink} />}
          </section>
        </div>
      </div>
      {handoff && note && <HandoffDialog note={note} onClose={() => setHandoff(false)} onDone={(n) => { setNote(n); setHandoff(false); reread(); }} />}
    </div>
  );
}

function IdeaStatusChip({ note }: { note: IdeaNote }) {
  if (note.idea.status === "done") {
    return (
      <span className="idea-chip is-done" data-testid="idea-status">
        완료{note.idea.tasks.length > 0 && " · "}
        {note.idea.tasks.map((d, i) => <span key={d}>{i > 0 && ", "}<a href={`#task=${encodeURIComponent(d)}`} className="app-id">{d}</a></span>)}
      </span>
    );
  }
  return (
    <span className="idea-chip" data-testid="idea-status">
      {note.merging !== null ? "합치는 중" : "다듬는 중"}
    </span>
  );
}

/** 노트 보기: 고른 글에 의견(t-1222 보고 글과 같은 띠), 그림·영상은 통째로 의견 단추 */
function NoteBody({ note, canNote, canEdit, uploads, onEdit, anchors, onAdd, onOpenLink }: {
  note: IdeaNote;
  canNote: boolean;
  /** 그림 메뉴로 노트를 고칠 수 있다(t-1575) */
  canEdit: boolean;
  uploads: MediaUploads;
  onEdit: (text: string) => Promise<void>;
  anchors: readonly import("./textAnchor.ts").TextAnchor[];
  onAdd: ReturnType<typeof useReviewNotes>["add"];
  onOpenLink?: (href: string) => boolean;
}) {
  const root = useRef<HTMLDivElement>(null);
  const lightbox = useLightbox();
  const image = useCallback((src: string) => ideaFileUrl(note.project, note.idea.id, src), [note.project, note.idea.id]);
  const idea = note.idea.short_id;
  return (
    <div className="idea-note-body" data-testid="idea-note" data-rev={note.rev}
      onClickCapture={(e) => {
        // 할 일 메모와 같은 앱 링크(#task=·#idea=)는 앱 안에서 연다
        const a = e.target instanceof Element ? e.target.closest("a[href]") : null;
        const href = a?.getAttribute("href");
        if (href && onOpenLink?.(href)) e.preventDefault();
      }}>
      <div ref={root} className="idea-note-text">
        <ReportMarkdown base={null} image={image} text={note.text} onImages={lightbox.open} captions
          notes={canNote ? { anchors, onAdd: (anchor, comment, range) => onAdd({ kind: "note", idea, section: root.current ? sectionBefore(root.current, range) : null, anchor }, comment) } : undefined} />
      </div>
      {(canNote || canEdit) && <MediaNotes root={root} note={note} canNote={canNote} canEdit={canEdit} uploads={uploads} onEdit={onEdit}
        onAdd={(name, label, section, comment) => onAdd({ kind: "media", idea, section, name, label }, comment)} />}
      {lightbox.node}
    </div>
  );
}

/** 그림 단추 묶음의 왼쪽 끝이 노트 칸 왼쪽에서 떨어질 최소 거리: 펼친 메뉴(min-width 180px)가 칸 밖으로 나가지 않게 */
const MENU_ROOM = 192;

/** 그림·영상마다 오른쪽 위 「의견」(결정 17: 통째로 인용)과 「⋯」 그림 메뉴(t-1575: 설명·바꾸기·위로·아래로·지우기·내려받기) */
function MediaNotes({ root, note, canNote, canEdit, uploads, onEdit, onAdd }: {
  root: React.RefObject<HTMLDivElement | null>;
  note: IdeaNote;
  canNote: boolean;
  canEdit: boolean;
  uploads: MediaUploads;
  onEdit: (text: string) => Promise<void>;
  onAdd: (name: string, label: string, section: string | null, comment: string) => boolean;
}) {
  const text = note.text;
  const [spots, setSpots] = useState<{ name: string; nth: number; label: string; section: string | null; top: number; right: number }[]>([]);
  const [open, setOpen] = useState<string | null>(null);
  const [menu, setMenu] = useState<{ key: string; mode: "menu" | "alt" | "remove"; alt: string } | null>(null);
  const latest = useRef(note);
  latest.current = note;
  const replacing = useRef<{ name: string; nth: number } | null>(null);
  const replacer = useRef<HTMLInputElement>(null);
  useEffect(() => { setMenu(null); }, [text]);
  useLayoutEffect(() => {
    const el = root.current;
    if (!el) return;
    const measure = () => {
      // 단추는 노트 칸(.idea-note-body, position: relative) 기준이다. 그림 오른쪽 위에 두되 칸 밖으로 나가지 않게 한다
      const box = (el.parentElement ?? el).getBoundingClientRect();
      const count = new Map<string, number>();
      const found = Array.from(el.querySelectorAll<HTMLElement>("img, video")).flatMap((m) => {
        const src = m.getAttribute("src") ?? m.querySelector("source")?.getAttribute("src") ?? "";
        const name = decodeURIComponent(src.split("/").pop() ?? "");
        if (!/^[0-9a-f]{16}\.(?:png|jpg|gif|webp|mp4|webm)$/.test(name)) return [];
        // 같은 그림이 여러 번 나오면 글의 몇째 것인지로 가른다(렌더러는 글 차례로 그린다)
        const nth = count.get(name) ?? 0;
        count.set(name, nth + 1);
        const r = m.getBoundingClientRect();
        const range = document.createRange();
        range.selectNode(m);
        return [{ name, nth, label: m.getAttribute("alt") ?? m.getAttribute("aria-label") ?? "", section: sectionBefore(el, range), top: r.top - box.top + 6, right: Math.max(6, Math.min(box.right - r.right + 6, box.width - (r.left - box.left) - MENU_ROOM)) }];
      });
      setSpots(found);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    el.addEventListener("load", measure, true);
    return () => { observer.disconnect(); el.removeEventListener("load", measure, true); };
  }, [root, text]);
  /** 지금 판의 같은 그림에 고침을 얹는다(그사이 글이 바뀌었으면 그 글에서 다시 찾는다) */
  const change = (name: string, nth: number, how: (text: string, spot: NonNullable<ReturnType<typeof ideaMediaSpot>>) => string | null) => {
    const now = latest.current.text;
    const spot = ideaMediaSpot(now, name, nth);
    const next = spot ? how(now, spot) : null;
    setMenu(null);
    if (next !== null && next !== now) void onEdit(next);
  };
  return (
    <>
      <input ref={replacer} type="file" hidden accept={IDEA_MEDIA_ACCEPT} data-testid="idea-media-replace-input"
        onChange={(e) => {
          const files = Array.from(e.target.files ?? []).slice(0, 1);
          e.target.value = "";
          const which = replacing.current;
          if (!which || files.length === 0) return;
          void uploads.add(files, (names) => change(which.name, which.nth, (t, spot) => withMediaName(t, spot, names[0]!)));
        }} />
      {spots.map((s) => {
        const key = `${s.name}:${s.nth}`;
        const spot = ideaMediaSpot(text, s.name, s.nth);
        const info = note.media?.find((m) => m.name === s.name);
        const url = ideaFileUrl(note.project, note.idea.id, s.name) ?? "";
        const m = menu?.key === key ? menu : null;
        return (
          <div key={`${key}:${s.top}`} className="idea-media-note" style={{ top: s.top, right: s.right }} data-media={s.name}>
            {open === key ? (
              <NoteEditor quote={`${/\.(mp4|webm)$/.test(s.name) ? "영상" : "그림"} ${s.label || s.name}`} submitLabel="의견 추가"
                onSubmit={(comment) => { if (!onAdd(s.name, s.label, s.section, comment)) return false; setOpen(null); return true; }}
                onCancel={() => setOpen(null)} />
            ) : (
              <span className="idea-media-buttons">
                {canNote && (
                  <button type="button" className="chat-quote" data-testid="idea-media-note" onClick={() => { setMenu(null); setOpen(key); }}>
                    <MessageSquarePlus aria-hidden="true" />의견
                  </button>
                )}
                {canEdit && spot && (
                  <button type="button" className="chat-quote" data-testid="idea-media-menu-open" aria-label="그림 메뉴" aria-expanded={m !== null}
                    onClick={() => setMenu(m ? null : { key, mode: "menu", alt: spot.alt })}><Ellipsis aria-hidden="true" /></button>
                )}
              </span>
            )}
            {m && spot && (
              <div className="idea-media-menu" role="menu" data-testid="idea-media-menu">
                {info && ideaMediaSource(info) && <p className="muted idea-media-source" data-testid="idea-media-source">{ideaMediaSource(info)}</p>}
                {m.mode === "alt" ? (
                  <form className="idea-media-alt" onSubmit={(e) => { e.preventDefault(); change(s.name, s.nth, (t, sp) => withMediaAlt(t, sp, m.alt)); }}>
                    <input className="input" value={m.alt} autoFocus aria-label="설명" data-testid="idea-media-alt-input" maxLength={200}
                      onChange={(e) => setMenu({ ...m, alt: e.target.value })} />
                    <button type="submit" className="btn btn-primary small" data-testid="idea-media-alt-save">저장</button>
                    <button type="button" className="btn small" onClick={() => setMenu({ ...m, mode: "menu" })}>취소</button>
                  </form>
                ) : (
                  <>
                    <button type="button" role="menuitem" data-testid="idea-media-alt" onClick={() => setMenu({ ...m, mode: "alt" })}><Pencil aria-hidden="true" />설명</button>
                    <button type="button" role="menuitem" data-testid="idea-media-replace" onClick={() => { replacing.current = { name: s.name, nth: s.nth }; setMenu(null); replacer.current?.click(); }}><RefreshCw aria-hidden="true" />바꾸기</button>
                    <button type="button" role="menuitem" data-testid="idea-media-up" disabled={movedMedia(text, spot, -1) === null} onClick={() => change(s.name, s.nth, (t, sp) => movedMedia(t, sp, -1))}><ArrowUp aria-hidden="true" />위로</button>
                    <button type="button" role="menuitem" data-testid="idea-media-down" disabled={movedMedia(text, spot, 1) === null} onClick={() => change(s.name, s.nth, (t, sp) => movedMedia(t, sp, 1))}><ArrowDown aria-hidden="true" />아래로</button>
                    {m.mode === "remove" ? (
                      <button type="button" role="menuitem" className="danger" data-testid="idea-media-remove-confirm" onClick={() => change(s.name, s.nth, withoutMedia)}><Trash2 aria-hidden="true" />지우기 확인</button>
                    ) : (
                      <button type="button" role="menuitem" data-testid="idea-media-remove" onClick={() => setMenu({ ...m, mode: "remove" })}><Trash2 aria-hidden="true" />지우기</button>
                    )}
                    <a role="menuitem" href={url} download={s.name} data-testid="idea-media-download" onClick={() => setMenu(null)}><Download aria-hidden="true" />내려받기</a>
                  </>
                )}
              </div>
            )}
          </div>
        );
      })}
    </>
  );
}

/** 직접 고치기(결정 18: 그냥 textarea). 그림·영상은 붙여 넣기·끌어 놓기·「그림」 단추로 올려 커서 자리에 `![그림](<이름>)` 줄을 넣는다 */
function NoteEditorPane({ note, editing, busy, uploads, onChange, onCancel, onSave }: {
  note: IdeaNote;
  editing: { base: number; text: string };
  busy: boolean;
  uploads: MediaUploads;
  onChange: (text: string) => void;
  onCancel: () => void;
  onSave: () => void;
}) {
  const area = useRef<HTMLTextAreaElement>(null);
  const picker = useRef<HTMLInputElement>(null);
  const latest = useRef(editing.text);
  latest.current = editing.text;
  const insert = (files: File[]) => {
    // 올리는 동안 커서가 움직여도 올린 때의 자리(지금 커서)에 넣는다
    void uploads.add(files, (names) => {
      let text = latest.current;
      const el = area.current;
      let at = el ? el.selectionEnd : text.length;
      for (const name of names) {
        const line = `![${/\.(mp4|webm)$/.test(name) ? "영상" : "그림"}](${name})`;
        const before = text.slice(0, at);
        const gap = before === "" || before.endsWith("\n") ? "" : "\n";
        text = `${before}${gap}${line}\n${text.slice(at)}`;
        at = before.length + gap.length + line.length + 1;
      }
      latest.current = text;
      onChange(text);
    });
  };
  return (
    <div className="idea-editor" data-testid="idea-editor">
      <textarea ref={area} className="input idea-editor-text" value={editing.text} autoFocus aria-label="노트" data-testid="idea-editor-text"
        onChange={(e) => onChange(e.target.value)}
        onPaste={(e: ClipboardEvent<HTMLTextAreaElement>) => { const files = filesOf(e.clipboardData); if (files.length) { e.preventDefault(); insert(files); } }}
        onDragOver={(e: DragEvent) => { if (e.dataTransfer.types.includes("Files")) e.preventDefault(); }}
        onDrop={(e: DragEvent<HTMLTextAreaElement>) => { const files = filesOf(e.dataTransfer); if (files.length) { e.preventDefault(); insert(files); } }}
        onKeyDown={(e) => { if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && !e.nativeEvent.isComposing) { e.preventDefault(); onSave(); } }} />
      <input ref={picker} type="file" multiple hidden accept={IDEA_MEDIA_ACCEPT} data-testid="idea-editor-media-input"
        onChange={(e) => { const files = Array.from(e.target.files ?? []); e.target.value = ""; insert(files); }} />
      <div className="idea-editor-foot">
        <button type="button" className="btn" data-testid="idea-editor-media" onPointerDown={(e) => e.preventDefault()} onClick={() => picker.current?.click()}><ImagePlus className="i" aria-hidden="true" />그림</button>
        <span className="muted" data-testid="idea-editor-base">판 {editing.base}에서 고치는 중{uploads.busy && " · 올리는 중…"}{note.rev !== editing.base && ` · 지금 판 ${note.rev}`}</span>
        <span className="spacer" />
        <button type="button" className="btn" disabled={busy} onClick={onCancel}>취소</button>
        <button type="button" className="btn btn-primary" data-testid="idea-editor-save" disabled={busy || uploads.busy} onClick={onSave}>저장</button>
      </div>
    </div>
  );
}

/** 전용 thread 대화: 돌고 있는 pane이면 그 대화 기록, 입력창은 host(`idea_say`)로 보낸다(없으면 띄우고, 쉬면 잇는다) */
function IdeaChat({ note, threads, panes, canControl, notes, active, onSaid, onError, onOpenLink }: {
  note: IdeaNote;
  threads: ThreadView[];
  panes: PaneView[];
  canControl: boolean;
  notes: ReturnType<typeof useReviewNotes>;
  active: boolean;
  onSaid: (note: IdeaNote) => void;
  onError: (e: string | null) => void;
  onOpenLink?: (href: string) => boolean;
}) {
  const thread = threads.find((t) => t.id === note.idea.thread && t.idea === note.idea.short_id) ?? null;
  const pane = thread?.pane ? panes.find((p) => p.id === thread.pane && p.state === "running") ?? null : null;
  const owner = `idea:${note.project}/${note.idea.short_id}`;
  const harness = pane?.agent?.harness === "codex" ? "codex" : "claude";
  // Claude·Codex pane만 대화 기록을 읽는다(터미널 보기와 같은 규칙). 그 밖의 harness는 thread 화면에서 터미널로 본다
  const chatable = pane?.agent?.harness === "claude" || pane?.agent?.harness === "codex";
  const [pending, setPending] = useState<PendingSend[]>(() => restorePending(owner));
  useEffect(() => savePending(owner, pending), [owner, pending]);
  const [messages, setMessages] = useState<ChatMessage[] | null>(null);
  const [sent, setSent] = useState(0);
  const noteRef = useRef(note);
  noteRef.current = note;
  const done = note.idea.status === "done";
  const draftKey = `herdr-web-ui:composer-draft:${owner}`;
  const draft = useSyncExternalStore(composerDrafts.subscribe, () => composerDrafts.read(draftKey));
  const [trayProblem, setTrayProblem] = useState<string | null>(null);
  const sorted = useMemo(() => sortNotes(notes.notes), [notes.notes]);
  const insertNotes = () => {
    const next = appendQuote(draft.text, formatNotes(note.idea.short_id, sorted), MAX_COMPOSER_CHARS);
    if (next === null) { setTrayProblem("입력창이 넘친다"); return; }
    composerDrafts.set(draftKey, next);
    notes.remove(sorted.map((n) => n.id));
    setTrayProblem(null);
  };
  const status = pane ? paneAgentStatus(pane) : undefined;
  return (
    <div className="idea-chat" data-testid="idea-chat">
      <div className="idea-chat-head">
        {thread ? <a className="idea-thread-chip" href={`#thread=${encodeURIComponent(thread.id)}`} data-testid="idea-thread">{thread.id} · {thread.machine}</a> : <span className="muted">대화 없음</span>}
      </div>
      <div className="idea-chat-body">
        <ProviderError agent={pane?.agent} />
        {pane && chatable ? (
          <ChatPanel active={active} pane={pane.id} status={status} compacting={pane.agent?.compacting ?? false} compaction={pane.agent?.context?.compaction ?? null}
            readNow={sent} onMessages={setMessages} pending={pending} harness={harness} onOpenLink={onOpenLink}
            onDismissPending={(id) => setPending((list) => list.filter((p) => p.id !== id))} />
        ) : (
          <div className="idea-chat-idle" data-testid="idea-chat-idle">
            {pending.map((p) => <div key={p.id} className="chat-msg user chat-bubble is-pending" data-testid="idea-pending">{p.text}</div>)}
            {pending.length === 0 && note.pending.length === 0 && <p className="chat-note">{pane ? "대화 기록을 읽지 못하는 harness다" : thread ? "잇는 중…" : "아직 대화가 없다."}</p>}
          </div>
        )}
        {note.error && <p className="banner err idea-chat-error" role="alert" data-testid="idea-error">thread를 띄우지 못했다: {note.error}</p>}
        {note.pending.length > 0 && <p className="idea-chat-wait" role="status" data-testid="idea-waiting">thread 준비 중 · 보낼 말 {note.pending.length}개</p>}
      </div>
      {sorted.length > 0 && (
        <div className="idea-tray" data-testid="idea-tray">
          <div className="idea-tray-head">
            <span>의견 {sorted.length}개</span>
            <span className="spacer" />
            <button type="button" className="btn" data-testid="idea-tray-clear" onClick={() => notes.remove(sorted.map((n) => n.id))}>모두 지우기</button>
            <button type="button" className="btn btn-primary" data-testid="idea-tray-insert" disabled={done || !canControl} onClick={insertNotes}>입력창에 넣기</button>
          </div>
          {trayProblem && <p className="muted" role="status">{trayProblem}</p>}
          <ul className="idea-tray-list">
            {sorted.map((n) => (
              <li key={n.id} className="idea-tray-item" data-testid="idea-tray-item">
                <span className="idea-tray-where">{noteLabel(n)}</span>
                <blockquote className="review-note-text">{noteText(n)}</blockquote>
                {n.comment && <span className="idea-tray-comment">{n.comment}</span>}
                <button type="button" className="ghost small icon" aria-label="의견 지우기" onClick={() => notes.remove([n.id])}><X className="i" aria-hidden="true" /></button>
              </li>
            ))}
          </ul>
        </div>
      )}
      {canControl && !done && (
        <ChatComposer key={owner} active={active} owner={owner} pane={pane?.id ?? ""} touch={false}
          imagesOff={pane ? null : "대화를 시작한 뒤 그림을 붙인다"}
          agentLabel={harness === "codex" ? "Codex" : "Claude"} harness={harness}
          pending={pending} setPending={setPending}
          connected working={status === "working"} compacting={pane?.agent?.compacting ?? false} asking={status === "blocked"}
          context={pane?.agent?.context ?? null} messages={messages}
          prepare={(text) => withEditNote(noteRef.current, text)}
          onSend={(text) => call({ op: "idea_say", idea: note.idea.short_id, text }).then((r) => {
            if (r.kind === "idea") onSaid(r.note);
            onError(null);
            return true as const;
          }, (e: unknown) => (e instanceof ApiError ? e.message : String(e)))}
          onCtrlEnter={() => false} onInputTrace={() => {}} onCancelSend={() => Promise.resolve(null)}
          onSent={() => setSent((n) => n + 1)} onInspect={() => {}} />
      )}
    </div>
  );
}

/** 할 일로 넘기기(결정 3·27): 초안 여럿. 메모 첫 줄의 아이디어 링크는 host가 붙인다 */
function HandoffDialog({ note, onClose, onDone }: { note: IdeaNote; onClose: () => void; onDone: (note: IdeaNote) => void }) {
  const [drafts, setDrafts] = useState<IdeaTaskDraft[]>(() => {
    const d = firstDraft(note);
    return [{ title: d.title, repo: null, notes: d.notes }];
  });
  const [repos, setRepos] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  useEffect(() => {
    call({ op: "tasks" }).then((r) => { if (r.kind === "task_board") setRepos(r.repos); }, () => {});
  }, []);
  const set = (i: number, d: Partial<IdeaTaskDraft>) => setDrafts((list) => list.map((x, k) => (k === i ? { ...x, ...d } : x)));
  const submit = async () => {
    if (busy || drafts.some((d) => !d.title.trim())) return;
    setBusy(true);
    setProblem(null);
    try {
      const r = await call({ op: "idea_handoff", idea: note.idea.short_id, base_rev: note.rev, tasks: drafts });
      if (r.kind === "idea") onDone(r.note);
    } catch (e) {
      setProblem(e instanceof ApiError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="modal-scrim" onMouseDown={(e) => { if (e.target === e.currentTarget && !busy) onClose(); }}>
      <div className="modal idea-handoff" role="dialog" aria-modal="true" aria-label="할 일로 넘기기" data-testid="idea-handoff-dialog">
        <header className="modal-header"><h2 className="modal-title">할 일로 넘기기 <code>{note.idea.short_id}</code></h2></header>
        <div className="modal-body">
          {drafts.map((d, i) => (
            <fieldset key={i} className="idea-draft" data-testid="idea-draft">
              <label>제목<input className="input" value={d.title} onChange={(e) => set(i, { title: e.target.value })} data-testid="idea-draft-title" /></label>
              <label>저장소
                <select className="input" value={d.repo ?? ""} onChange={(e) => set(i, { repo: e.target.value || null })} data-testid="idea-draft-repo">
                  <option value="">정하지 않음</option>
                  {repos.map((r) => <option key={r} value={r}>{r}</option>)}
                </select>
              </label>
              <label>메모<textarea className="input" rows={5} value={d.notes} onChange={(e) => set(i, { notes: e.target.value })} data-testid="idea-draft-notes" /></label>
              {drafts.length > 1 && <button type="button" className="ghost small" onClick={() => setDrafts((list) => list.filter((_, k) => k !== i))}>빼기</button>}
            </fieldset>
          ))}
          {problem && <p className="confirm-error" role="alert">{problem}</p>}
        </div>
        <footer className="modal-footer">
          <button type="button" className="btn idea-draft-add" data-testid="idea-draft-add" disabled={busy} onClick={() => setDrafts((list) => [...list, { title: "", repo: null, notes: "" }])}>할 일 더하기</button>
          <span className="spacer" />
          <button type="button" className="btn" disabled={busy} onClick={onClose}>취소</button>
          <button type="button" className="btn btn-primary" data-testid="idea-handoff-submit" disabled={busy || drafts.some((d) => !d.title.trim())} onClick={() => void submit()}>넘기기</button>
        </footer>
      </div>
    </div>
  );
}

/**
 * 휴대폰 「페이지처럼」 쓸기(결정 24, ideasModel `ideaSwipe`): 노트 ← 대화, 대화 → 노트. 노트 →(목록)은 앱 drawer가 받는다.
 * 글을 고르는 중·입력칸·가로로 밀린 표·창 위는 쓸지 않는다(edgeSwipe와 같은 규칙)
 */
function usePageSwipe(el: React.RefObject<HTMLElement | null>, on: boolean, page: IdeaPage, setPage: (p: IdeaPage) => void) {
  const pageRef = useRef(page);
  pageRef.current = page;
  useEffect(() => {
    const box = el.current;
    if (!on || !box) return;
    let start: { x: number; y: number } | null = null;
    let claimed = false;
    const onStart = (e: TouchEvent) => {
      const t = e.touches[0];
      start = e.touches.length === 1 && t && document.querySelector(MODAL) === null && !editable(e.target) && !selecting() && !scrolledAside(e.target)
        ? { x: t.clientX, y: t.clientY } : null;
      claimed = false;
    };
    const onMove = (e: TouchEvent) => {
      const t = e.touches[0];
      if (!start || !t || !e.cancelable) return;
      if (selecting()) { start = null; return; }
      const v = ideaSwipe(pageRef.current, start.x, t.clientX - start.x, t.clientY - start.y);
      if (v === "pending") return;
      if (v === "ignore") { if (!claimed) start = null; return; }
      claimed = true;
      e.preventDefault();
      if (v !== "claim") { setPage(v); start = null; }
    };
    const onEnd = () => { start = null; claimed = false; };
    box.addEventListener("touchstart", onStart, { passive: true });
    box.addEventListener("touchmove", onMove, { passive: false });
    box.addEventListener("touchend", onEnd, { passive: true });
    box.addEventListener("touchcancel", onEnd, { passive: true });
    return () => {
      box.removeEventListener("touchstart", onStart);
      box.removeEventListener("touchmove", onMove);
      box.removeEventListener("touchend", onEnd);
      box.removeEventListener("touchcancel", onEnd);
    };
  }, [el, on, setPage]);
}
