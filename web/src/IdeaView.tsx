import { ProviderError } from "./ProviderError";
// 옛 아이디어 본문(t-1260, 읽기 전용 t-1894 4단계): 노트와 옛 전용 thread 대화를 나란히 둔다(목록은 기획 트리 「옛 아이디어」).
// 아이디어는 저장소 기획 문서로 옮겼다: 고치기·말하기·그림 붙이기·할 일로 넘기기는 거뒀고, 다듬는 중이던 노트는 「기획으로 옮기기」로
// 그 저장소의 기획 대화에 넘긴다(IdeaMove.tsx). 노트 보기는 보고 글과 같은 Markdown 렌더러(그림·영상)다.
// 휴대폰은 「목록 | 노트 | 대화」가 한 줄로 놓인 페이지처럼 쓸어 넘긴다(목록은 앱 drawer).
import "./IdeaView.css";

import { ArrowRightLeft, BookOpen } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { ApiError, call } from "./api";
import { ChatPanel } from "./Chat";
import { DrawerToggle, HeadMore, HeadNote } from "./drawer";
import type { IdeaMoved, IdeaNote, IdeaSummary, PaneView, ThreadView } from "./gen/proto";
import { MODAL, editable, scrolledAside, selecting } from "./herdr/edgeSwipe.ts";
import { IdeaResizer } from "./IdeaResizer";
import { ideaFileUrl, ideaLegacy, ideaSwipe, type IdeaPage } from "./ideasModel";
import { useLightbox } from "./Lightbox";
import { usePageVisible } from "./pageVisible";
import { ReportMarkdown } from "./ReportsView";
import { useMedia } from "./settings";
import { paneAgentStatus } from "./sidebarModel";

/** 노트 하나를 읽고, host가 바꿨다고 알리면(`ideas` 사건 → 목록 판·옮긴 곳이 바뀜) 다시 읽는다 */
function useIdeaNote(target: string | null, summary: IdeaSummary | undefined) {
  const [note, setNote] = useState<IdeaNote | null>(null);
  const [error, setError] = useState<string | null>(null);
  const moved = summary?.moved;
  const stamp = summary
    ? `${summary.rev}:${summary.unseen}:${summary.status}:${summary.updated_ms}:${summary.thread ?? ""}:${moved ? `${moved.branch}:${moved.done}:${moved.path}` : ""}`
    : "";
  useEffect(() => {
    if (!target) { setNote(null); return; }
    let live = true;
    call({ op: "idea_read", idea: target }).then(
      (r) => { if (live && r.kind === "idea") { setNote(r.note); setError(null); } },
      (e: unknown) => { if (live) setError(e instanceof ApiError ? e.message : String(e)); },
    );
    return () => { live = false; };
  }, [target, stamp]);
  return { note, error, setError };
}

export function IdeaView({ shown, target, ideas, threads, panes, canControl, onOpenLink, onMove, onOpenDesign }: {
  /** 옛 아이디어 「기획으로 옮기기」 창을 연다 */
  onMove?: (idea: IdeaSummary) => void;
  /** 옮긴 곳(옮기는 중이면 그 기획 대화, 옮겼으면 그 문서) 열기 */
  onOpenDesign?: (moved: IdeaMoved) => void;
  shown: boolean;
  /** 연 아이디어(`n-0001`) */
  target: string | null;
  ideas: IdeaSummary[];
  /** 모든 thread(옛 아이디어 thread 포함) */
  threads: ThreadView[];
  panes: PaneView[];
  canControl: boolean;
  onOpenLink?: (href: string) => boolean;
}) {
  const summary = ideas.find((i) => i.short_id === target);
  const { note, error, setError } = useIdeaNote(target, summary);
  const narrow = useMedia("(max-width: 768px)");
  const [page, setPage] = useState<IdeaPage>("note");
  const legacy = note ? ideaLegacy(note.idea) : null;
  useEffect(() => { setPage("note"); }, [target]);
  const columns = useRef<HTMLDivElement>(null);
  const [notePercent, setNotePercent] = useState(60);
  const seen = useRef<string | null>(null);
  const visible = usePageVisible();
  useEffect(() => {
    if (!shown || !visible || (narrow && page !== "note") || !note || note.idea.short_id !== target || !note.idea.unseen) return;
    const key = `${note.project}/${note.idea.id}/${note.rev}`;
    if (seen.current === key) return;
    seen.current = key;
    call({ op: "idea_seen", idea: note.idea.short_id, rev: note.rev }).catch((e: unknown) => {
      seen.current = null;
      setError(e instanceof ApiError ? e.message : String(e));
    });
  }, [shown, visible, narrow, page, note, target, setError]);
  usePageSwipe(columns, narrow && shown, page, setPage);

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
  const moved = note?.idea.moved;
  const tools = note && legacy && (legacy.kind === "open" ? (canControl && onMove ? (
    <button type="button" className="btn btn-primary" data-testid="idea-move" onClick={() => onMove(note.idea)}><ArrowRightLeft className="i" aria-hidden="true" />기획으로 옮기기</button>
  ) : null) : moved && onOpenDesign ? (
    <button type="button" className="btn" data-testid="idea-open-design" onClick={() => onOpenDesign(moved)}>
      <BookOpen className="i" aria-hidden="true" />{legacy.kind === "moved" ? "기획에서 보기" : "기획 대화 보기"}</button>
  ) : null);
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
              <button type="button" role="tab" aria-selected={page === "chat"} data-testid="idea-tab-chat" onClick={() => setPage("chat")}>대화</button>
            </span>
          ) : <span className="idea-head-tools">{tools}</span>}
          <HeadMore />
        </header>
        {legacy && (
          <div className={`banner idea-legacy is-${legacy.kind}`} role="status" data-testid="idea-legacy" data-state={legacy.kind}>
            {legacy.kind === "open" && <>옛 아이디어다(읽기 전용). 이제 아이디어는 저장소의 기획 문서로 다듬는다: 「기획으로 옮기기」로 그 저장소의 기획 대화에 넘긴다.</>}
            {legacy.kind === "moving" && <>기획으로 옮기는 중이다: <code>{moved?.repo}</code> · <code>design/{moved?.branch}</code>의 대화가 판 {moved?.rev || note?.idea.rev} 노트를 기획 트리에 녹인다. 그 대화를 main에 반영하면 「옮김」이 된다.</>}
            {legacy.kind === "moved" && <>기획으로 옮겼다: <code>{moved?.repo} {moved?.path}</code>. 이제 그 문서가 정본이고 이 노트는 읽기 전용이다.</>}
            {legacy.kind === "done" && <>완료한 옛 아이디어다(읽기 전용).</>}
          </div>
        )}
        {error && <div className="banner err" role="alert">{error}</div>}
        {note?.media_error && <div className="banner err" role="alert">그림을 옮기지 못했다: {note.media_error}</div>}
        <div className="idea-columns" ref={columns} style={{ "--idea-note-percent": `${notePercent}%` } as React.CSSProperties}>
          <section id="idea-note-pane" className="idea-note-pane" aria-label="아이디어 노트" hidden={narrow && page !== "note"} data-testid="idea-note-pane">
            {narrow && tools && <div className="idea-note-tools">{tools}</div>}
            {note && <NoteBody note={note} onOpenLink={onOpenLink} />}
          </section>
          {!narrow && <IdeaResizer percent={notePercent} onResize={setNotePercent} />}
          <section className="idea-chat-pane" aria-label="대화" hidden={narrow && page !== "chat"} data-testid="idea-chat-pane"
            data-drawer-swipe={narrow && page === "chat" ? "off" : undefined}>
            {note && <IdeaChat note={note} threads={threads} panes={panes} active={shown && (!narrow || page === "chat")} onOpenLink={onOpenLink} />}
          </section>
        </div>
      </div>
    </div>
  );
}

function IdeaStatusChip({ note }: { note: IdeaNote }) {
  const legacy = ideaLegacy(note.idea);
  if (legacy.kind === "moving" || legacy.kind === "moved") {
    return <span className={`idea-chip is-${legacy.kind}`} data-testid="idea-status">{legacy.kind === "moving" ? "옮기는 중" : "옮김"}</span>;
  }
  if (legacy.kind === "done") {
    return (
      <span className="idea-chip is-done" data-testid="idea-status">
        완료{note.idea.tasks.length > 0 && " · "}
        {note.idea.tasks.map((d, i) => <span key={d}>{i > 0 && ", "}<a href={`#task=${encodeURIComponent(d)}`} className="app-id">{d}</a></span>)}
      </span>
    );
  }
  return <span className="idea-chip" data-testid="idea-status">다듬는 중</span>;
}

/** 노트 보기(읽기만). 그림·영상은 눌러 크게 본다 */
function NoteBody({ note, onOpenLink }: { note: IdeaNote; onOpenLink?: (href: string) => boolean }) {
  const lightbox = useLightbox();
  const image = useCallback((src: string) => ideaFileUrl(note.project, note.idea.id, src), [note.project, note.idea.id]);
  return (
    <div className="idea-note-body" data-testid="idea-note" data-rev={note.rev}
      onClickCapture={(e) => {
        // 할 일 메모와 같은 앱 링크(#task=·#idea=)는 앱 안에서 연다
        const a = e.target instanceof Element ? e.target.closest("a[href]") : null;
        const href = a?.getAttribute("href");
        if (href && onOpenLink?.(href)) e.preventDefault();
      }}>
      <div className="idea-note-text">
        <ReportMarkdown base={null} image={image} text={note.text} onImages={lightbox.open} captions />
      </div>
      {lightbox.node}
    </div>
  );
}

/** 옛 전용 thread 대화(읽기만): 아직 돌고 있는 pane이면 그 대화 기록. 끝난 thread는 thread 화면에서 본다 */
function IdeaChat({ note, threads, panes, active, onOpenLink }: {
  note: IdeaNote;
  threads: ThreadView[];
  panes: PaneView[];
  active: boolean;
  onOpenLink?: (href: string) => boolean;
}) {
  const thread = threads.find((t) => t.id === note.idea.thread && t.idea === note.idea.short_id) ?? null;
  const pane = thread?.pane ? panes.find((p) => p.id === thread.pane && p.state === "running") ?? null : null;
  const harness = pane?.agent?.harness === "codex" ? "codex" : "claude";
  // Claude·Codex pane만 대화 기록을 읽는다(터미널 보기와 같은 규칙). 그 밖의 harness는 thread 화면에서 터미널로 본다
  const chatable = pane?.agent?.harness === "claude" || pane?.agent?.harness === "codex";
  return (
    <div className="idea-chat" data-testid="idea-chat">
      <div className="idea-chat-head">
        {thread ? <a className="idea-thread-chip" href={`#thread=${encodeURIComponent(thread.id)}`} data-testid="idea-thread">{thread.id} · {thread.machine}</a> : <span className="muted">대화 없음</span>}
      </div>
      <div className="idea-chat-body">
        <ProviderError agent={pane?.agent} />
        {pane && chatable ? (
          <ChatPanel active={active} pane={pane.id} status={paneAgentStatus(pane)} compacting={pane.agent?.compacting ?? false}
            compaction={pane.agent?.context?.compaction ?? null} harness={harness} onOpenLink={onOpenLink} />
        ) : (
          <div className="idea-chat-idle" data-testid="idea-chat-idle">
            <p className="chat-note">{pane ? "대화 기록을 읽지 못하는 harness다" : thread ? "끝난 대화다: 기록은 thread 화면에서 본다." : "아직 대화가 없다."}</p>
          </div>
        )}
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
