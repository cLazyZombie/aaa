import "./DesignView.css";
import { CircleDot, Hammer, History, Pencil, RefreshCw, RotateCcw } from "lucide-react";
import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import { ApiError, call, onTasksChanged } from "./api";
import { splitAppIds, type AppId } from "./appIds";
import { DesignEditRequest } from "./DesignEditRequest";
import { handable, HandoffDialog, type HandoffBlock } from "./DesignHandoff";
import { blockDiff, calloutOf, designFileUrl, designTarget, idLineOf, isMarkdown, markCounts, marksOf, mdBlocks, resolveDocPath, sectionsOf, splitTarget, taskState, withoutIdLines, type ArchiveLookup, type Callout } from "./designModel";
import { DrawerToggle, HeadMore, HeadNote } from "./drawer";
import type { DesignDoc, DesignLogEntry, DesignRepoView, TaskView, ThreadView } from "./gen/proto";
import { OpenFileContext } from "./herdr/filePaths.ts";
import { useLightbox } from "./Lightbox";
import { AppIdChipContext } from "./Markdown";
import { ago } from "./machineDetail";
import { ReportMarkdown } from "./ReportsView";
import { useMedia } from "./settings";
import { useMinute } from "./Sidebar";

type Page = "tree" | "doc";
/** 문서 보기: 전체, 지금 모습(표지 블록을 뺀 구현된 사실), 남은 것(표지 블록 목록) */
type Look = "all" | "now" | "left";

/** 할 일 목록(넘긴 블록의 상태): 화면이 보일 때·트리를 다시 읽을 때 읽는다. 목록에 없는 d-ID는 보관에서 찾는다(끝난 할 일) */
/** 보관 찾기: d-ID마다 이만큼만 넘겨 본다(찾지 못하고 더 남으면 모름으로 둔다) */
const ARCHIVE_PAGES = 4;
const ARCHIVE_PAGE = 50;

/** 표지·글 속 d-ID의 할 일 보드와 보관 찾기. 할 일이 바뀐 사건마다 다시 읽는다. 문서를 바꾸면 앞 문서의 늦은 답은 버린다 */
function useDesignTasks(shown: boolean, rev: unknown, ids: readonly string[]): { tasks: TaskView[] | null; lookup: ArchiveLookup } {
  const [tasks, setTasks] = useState<TaskView[] | null>(null);
  const [lookup, setLookup] = useState<ArchiveLookup>(() => ({ archived: new Set(), missing: new Set() }));
  const [changed, setChanged] = useState(0);
  useEffect(() => onTasksChanged(() => setChanged((n) => n + 1)), []);
  const want = [...new Set(ids)].join(" ");
  useEffect(() => {
    if (!shown || !want) return;
    let live = true;
    (async () => {
      const r = await call({ op: "tasks" });
      if (!live || r.kind !== "task_board") return;
      setTasks(r.tasks);
      const archived = new Set<string>(), missing = new Set<string>();
      for (const id of want.split(" ").filter((id) => !r.tasks.some((t) => t.short_id === id))) {
        // 보관 검색은 번호·제목·메모의 부분 일치라 같은 번호가 든 다른 할 일이 앞에 올 수 있다: 정확한 번호를 몇 쪽 넘겨 찾는다
        let after: string | null = null;
        for (let n = 0; n < ARCHIVE_PAGES && live; n++) {
          const a = await call({ op: "task_archive", restore: null, view: { search: { query: id, repo: null }, filter: null, after, limit: ARCHIVE_PAGE, profile: null, machine: null, since_ms: null, until_ms: null } });
          if (a.kind !== "task_archive") break;
          if (a.entries.some((e) => e.task.short_id === id)) { archived.add(id); break; }
          if (!a.next) { missing.add(id); break; }
          after = a.next;
        }
      }
      if (live) setLookup({ archived, missing });
    })().catch(() => {
      // 읽지 못하면 모름이다: 「할 일 없음」으로 그리지 않는다
      if (live) setLookup({ archived: new Set(), missing: new Set() });
    });
    return () => { live = false; };
  }, [shown, rev, want, changed]);
  return { tasks, lookup };
}

/** 표지 블록 하나: 계획(노랑)·미정(보라)·넘김(할 일 상태 칩) */
function DesignCallout({ c, tasks, lookup, threads, onDiscuss, onHandoff, children }: {
  c: Callout;
  tasks: TaskView[] | null;
  lookup: ArchiveLookup;
  threads: ThreadView[];
  onDiscuss?: () => void;
  /** 넘기지 않은 계획 블록만 */
  onHandoff?: () => void;
  children: ReactNode;
}) {
  const state = c.task ? taskState(c.task, tasks, threads, lookup) : null;
  const tone = c.kind === "open" ? "open" : state ? `task tone-${state.tone}` : "plan";
  return (
    <div className={`design-callout is-${tone}`} data-testid="design-callout" data-kind={c.kind} data-task={c.task ?? undefined}>
      <div className="design-callout-side">
        {state ? (
          <a className={`design-state is-${state.tone}`} href={`#task=${encodeURIComponent(c.task!)}`} data-testid="design-state">
            {(state.tone === "work" || state.tone === "landing") && <CircleDot className="i" aria-hidden="true" />}{state.label}
          </a>
        ) : c.task ? (
          <span className="design-state is-wait">{c.task}</span>
        ) : (
          <span className={`design-pill is-${c.kind}`}>{c.kind === "open" ? "미정" : "계획"}</span>
        )}
        {c.image && <span className="design-pill is-draft">시안</span>}
        {c.pending && (c.pendingBranch
          ? <span className="design-pending" title="기획 branch에서 맡겼다: 표지는 그 branch에 있고 착지하면 main에 들어온다">표지는 design/{c.pendingBranch}에</span>
          : <span className="design-pending" title="main 문서에서 맡겼다: 표지는 구현 branch 첫 commit에 있고 착지하면 main에 들어온다">표지는 구현 branch에</span>)}
      </div>
      <div className="design-callout-body">{children}</div>
      {c.kind === "open" && onDiscuss && (
        <button type="button" className="btn small design-discuss" data-testid="design-discuss" onClick={onDiscuss}><Pencil className="i" aria-hidden="true" />편집 요청</button>
      )}
      {onHandoff && (
        <button type="button" className="btn small design-discuss" data-testid="design-handoff" onClick={onHandoff}><Hammer className="i" aria-hidden="true" />구현 맡기기</button>
      )}
    </div>
  );
}

const stamp = (ms: number) => {
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
};

/** 문서 이력(main이 가졌던 판)과 고른 옛 판 */
type History = { entries: DesignLogEntry[] | null; at: string | null; old: DesignDoc | null; error: string | null };

export function DesignView({ shown, target, repos, threads, canControl, tree, onOpen, onReload }: {
  shown: boolean;
  /** `<저장소>/<경로>`, 없으면 고른 문서 없음 */
  target: string | null;
  /** 트리(문서 안 링크가 가리키는 문서가 있는지 본다). 트리를 다시 읽어 판이 바뀌면 문서도 그 판으로 다시 읽는다 */
  repos: DesignRepoView[] | null;
  threads: ThreadView[];
  canControl: boolean;
  /** 좁은 화면 「트리」 칸에 그릴 트리 */
  tree: ReactNode;
  onOpen: (target: string) => void;
  /** 트리 다시 읽기(origin을 받는다) */
  onReload: () => void;
}) {
  const at = target ? splitTarget(target) : null;
  const key = at ? `${at.repo}/${at.path}` : null;
  const repo = repos?.find((r) => r.repo === at?.repo) ?? (repos?.length === 1 ? repos[0] : undefined);
  const treeCommit = repo?.tree?.commit ?? null;
  const narrow = useMedia("(max-width: 768px)");
  const [page, setPage] = useState<Page>("doc");
  const now = useMinute();
  const lightbox = useLightbox();
  // 받은 문서(늘 main 판)
  const [doc, setDoc] = useState<{ key: string; doc: DesignDoc } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [rev, setRev] = useState(0);
  const docKey = key;
  useEffect(() => {
    if (!at || !docKey) return;
    let live = true;
    setBusy(true);
    setError(null);
    call({ op: "design_read", repo: at.repo, path: at.path }).then(
      (r) => { if (live && r.kind === "design_doc") setDoc({ key: docKey, doc: r.doc }); },
      (e: unknown) => { if (live) { setDoc(null); setError(e instanceof ApiError ? e.message : String(e)); } },
    ).finally(() => { if (live) setBusy(false); });
    return () => { live = false; };
  }, [docKey, rev, treeCommit]); // eslint-disable-line react-hooks/exhaustive-deps
  const current = doc && doc.key === docKey ? doc.doc : null;
  const docs = repo?.tree?.docs;
  const imageAt = useCallback((d: DesignDoc | null) => (src: string) => {
    if (!d) return null;
    const path = resolveDocPath(d.path, src);
    return path && !isMarkdown(path) ? designFileUrl(d.project, d.repo, path, d.commit) : null;
  }, []);
  const image = useMemo(() => imageAt(current), [imageAt, current]);
  /** 문서 안 경로: 문서 폴더 기준, 없으면 저장소 기준으로 찾아 트리에 있는 문서면 연다 */
  const openPath = useCallback((raw: string) => {
    if (!current) return;
    const tries = [resolveDocPath(current.path, raw), resolveDocPath("x", raw)];
    const hit = tries.find((p) => p && isMarkdown(p) && (docs ?? []).some((d) => d.path === p));
    if (hit) onOpen(designTarget(current.repo, hit));
  }, [current, docs, onOpen]);
  const [edit, setEdit] = useState<{ doc: DesignDoc; block?: string; restore?: string } | null>(null);
  useEffect(() => setEdit(null), [key]);
  // 이력: main이 가졌던 판을 새것부터. 판을 고르면 지금 판과 다른 덩어리를 보이고 그 판으로 되돌린다
  const [hist, setHist] = useState<History | null>(null);
  useEffect(() => { setHist(null); }, [key]);
  const openHistory = () => {
    if (!at) return;
    setHist({ entries: null, at: null, old: null, error: null });
    call({ op: "design_log", repo: at.repo, path: at.path }).then(
      (r) => { if (r.kind === "design_log") setHist((h) => (h ? { ...h, entries: r.entries } : h)); },
      (e: unknown) => setHist((h) => (h ? { ...h, error: e instanceof ApiError ? e.message : String(e) } : h)),
    );
  };
  const pickVersion = (commit: string) => {
    if (!at) return;
    // 이 문서의 가장 새 판(첫 줄)이 지금 판이다: main 끝 commit은 이 문서를 바꾸지 않은 commit일 수 있다
    const latest = hist?.entries?.[0]?.commit;
    setHist((h) => (h ? { ...h, at: commit, old: null, error: null } : h));
    if (commit === latest) return;
    call({ op: "design_read", repo: at.repo, path: at.path, at: commit }).then(
      (r) => { if (r.kind === "design_doc") setHist((h) => (h && h.at === commit ? { ...h, old: r.doc } : h)); },
      (e: unknown) => setHist((h) => (h && h.at === commit ? { ...h, error: e instanceof ApiError ? e.message : String(e) } : h)),
    );
  };
  /** 이력 줄의 누가: 기획 대화 thread는 그 대화 이름, 사람, 구현 맡기기, 기획 저장이 아닌 commit은 작성자 */
  const whoOf = (e: DesignLogEntry): { label: string; tone: string } => {
    if (e.other) return { label: e.by, tone: "other" };
    if (/구현 맡김/.test(e.subject)) return { label: "구현 맡기기", tone: "host" };
    if (e.by === "사람") return { label: "사람", tone: "man" };
    const conv = repo?.branches.find((b) => b.thread === e.by || b.events.some((ev) => ev.commit === e.commit));
    return { label: conv?.title ?? e.by, tone: "ai" };
  };
  // 덩어리: 표지 블록을 그리려고 늘 나눈다
  const blocks = useMemo(() => (current ? mdBlocks(current.text) : null), [current]);
  const sections = useMemo(() => (blocks && current ? sectionsOf(blocks, current.title) : []), [blocks, current]);
  // 옛 맡김 기록(t-2002 전: 표지가 구현 branch나 기획 branch에만 있다)은 그 블록 글로 찾아 그 할 일로 본다.
  // t-2002부터 맡기면 표지를 main에 바로 적으므로 표지대로 보인다
  const pendingNotes = useMemo(() => {
    const out = new Map<string, { task: string; branch: string }>();
    if (!at) return out;
    for (const n of repo?.handoffs ?? []) if (n.path === at.path) for (const b of n.blocks) out.set(b.trim(), { task: n.task, branch: n.branch });
    return out;
  }, [repo?.handoffs, at?.path]); // eslint-disable-line react-hooks/exhaustive-deps
  const callouts = useMemo(() => (blocks ? blocks.map((b, i) => {
    const c = calloutOf(b, sections[i] ?? "");
    const note = c && c.kind === "plan" && !c.task ? pendingNotes.get(b.trim()) : undefined;
    return c && note ? { ...c, task: note.task, pending: true, pendingBranch: note.branch } : c;
  }) : []), [blocks, sections, pendingNotes]);
  const [look, setLook] = useState<Look>("all");
  // 글 어디든(표 칸 포함) `d-NNNN`은 표지와 같은 상태 칩이다(확정 계획 §3)
  const inlineTasks = useMemo(() => (current ? splitAppIds(current.text, (id) => id.kind === "task").flatMap((p) => (typeof p === "string" ? [] : [p.target])) : []), [current]);
  const { tasks, lookup } = useDesignTasks(shown, repos, [...callouts.flatMap((c) => (c?.task ? [c.task] : [])), ...inlineTasks]);
  const chipOf = useCallback((id: AppId) => {
    if (id.kind !== "task") return null;
    const state = taskState(id.target, tasks, threads, lookup);
    if (!state) return <span className="design-state is-wait is-inline">{id.target}</span>;
    return (
      <a className={`design-state is-${state.tone} is-inline`} href={`#task=${encodeURIComponent(id.target)}`} data-testid="design-inline-state">
        {(state.tone === "work" || state.tone === "landing") && <CircleDot className="i" aria-hidden="true" />}{state.label}
      </a>
    );
  }, [tasks, threads, lookup]);
  // 트리 줄과 같은 셈(같은 d-ID는 한 번)
  const counts = useMemo(() => markCounts([marksOf(callouts)]), [callouts]);
  const canHand = canControl;
  const [checked, setChecked] = useState<Set<number>>(() => new Set());
  // 창은 연 때의 판과 블록으로 맡긴다: 그 사이 문서가 다시 읽혀도 고른 번호가 다른 블록을 가리키지 않는다(host가 판을 다시 본다)
  const [handoff, setHandoff] = useState<{ initial: number[]; blob: string; blocks: HandoffBlock[] } | null>(null);
  useEffect(() => { setChecked(new Set()); setHandoff(null); }, [docKey]);
  // 판이 바뀌면 「남은 것」에서 고른 것은 다른 블록일 수 있다: 푼다
  useEffect(() => { setChecked(new Set()); }, [current?.blob]);
  const openHandoff = (initial: number[]) => {
    if (!current || !blocks) return;
    const list = callouts.flatMap((c, i) => (c ? [{ c, i }] : [])).map((b, ordinal) => ({ ...b, ordinal, head: (blocks[b.i] ?? "").split("\n")[0]!.trim() }));
    setHandoff({ initial, blob: current.blob, blocks: list });
  };
  const flipCheck = (i: number) => setChecked((old) => { const next = new Set(old); if (!next.delete(i)) next.add(i); return next; });
  const discuss = (c: Callout) => { if (current) setEdit({ doc: current, block: c.body }); };
  // 「지금 모습」은 표지 블록만 든 절의 제목도 숨긴다(확정 계획 §3)
  const onlyPlans = (i: number) => {
    if (!blocks || !/^#{1,6}\s/.test(blocks[i]!)) return false;
    let plans = false;
    for (let j = i + 1; j < blocks.length && !/^#{1,6}\s/.test(blocks[j]!); j++) {
      if (!callouts[j]) return false;
      plans = true;
    }
    return plans;
  };
  const renderBlock = (b: string, i: number) => {
    // ID 표지 줄은 그리지 않는다(머리의 ID로 보인다, t-1957)
    if (idLineOf(b) !== null) return null;
    const c = callouts[i];
    const md = <ReportMarkdown base={null} image={image} text={c ? c.body : b} onImages={lightbox.open} captions />;
    return (
      <div key={i} className="design-block">
        {c ? <DesignCallout c={c} tasks={tasks} lookup={lookup} threads={threads} onDiscuss={canControl ? () => discuss(c) : undefined}
          onHandoff={canHand && handable(c) ? () => openHandoff([i]) : undefined}>{md}</DesignCallout> : md}
      </div>
    );
  };
  // 본문: main 판은 표지 블록 사이의 보통 글을 이어 한 번에 그린다(목록 이어짐·참조식 링크·각주가 덩어리마다 끊기지 않게).
  // 표지가 없으면 문서 전체를 그대로 그린다
  const renderBody = (): ReactNode => {
    if (!blocks || !current) return null;
    if (callouts.every((c) => !c)) return <ReportMarkdown base={null} image={image} text={withoutIdLines(current.text)} onImages={lightbox.open} captions />;
    const shown = (i: number) => !(look === "now" && (callouts[i] || onlyPlans(i)));
    const out: ReactNode[] = [];
    let run: string[] = [];
    let start = 0;
    const flush = () => {
      if (run.length) out.push(<div key={`text-${start}`} className="design-block"><ReportMarkdown base={null} image={image} text={run.join("\n\n")} onImages={lightbox.open} captions /></div>);
      run = [];
    };
    blocks.forEach((b, i) => {
      if (!shown(i) || idLineOf(b) !== null) return;
      if (callouts[i]) { flush(); out.push(renderBlock(b, i)); return; }
      if (run.length === 0) start = i;
      run.push(b);
    });
    flush();
    return out;
  };
  // 남은 것: 절마다 표지 블록. 절에 표지 없는 글도 있으면 「일부 구현」
  const left = useMemo(() => {
    const groups: { section: string; some: boolean; items: { c: Callout; i: number }[] }[] = [];
    if (!blocks) return groups;
    // 제목마다 새 절이다: 같은 이름의 절이 둘이어도 따로 둔다
    let g: (typeof groups)[number] | null = null;
    blocks.forEach((b, i) => {
      const c = callouts[i];
      if (/^#{1,6}\s/.test(b)) { g = null; return; }
      if (!g) { g = { section: sections[i] ?? "", some: false, items: [] }; groups.push(g); }
      if (c) g.items.push({ c, i });
      else g.some = true;
    });
    return groups.filter((g) => g.items.length > 0);
  }, [blocks, sections, callouts]);
  // 이력에서 고른 판과 지금 판의 차이(덩어리 단위, ID 표지 줄은 빼고)
  const diff = useMemo(() => {
    if (!hist?.old || !current) return null;
    const plain = (t: string) => mdBlocks(t).filter((b) => idLineOf(b) === null);
    return blockDiff(plain(hist.old.text), plain(current.text));
  }, [hist?.old, current]);
  const histPicked = hist?.entries && hist.at ? hist.entries.findIndex((e) => e.commit === hist.at) : -1;
  const histImage = useMemo(() => imageAt(hist?.old ?? null), [imageAt, hist?.old]);
  const show = (p: Page) => !narrow || page === p;
  return (
    <div className={`design-screen${shown ? "" : " is-hidden"}`} inert={!shown} data-testid="design-view" data-shown={shown ? "true" : "false"}>
      {narrow && (
        <nav className="design-pages" aria-label="기획 칸">
          {(["tree", "doc"] as const).map((p) => (
            <button key={p} type="button" className={page === p ? "is-on" : undefined} aria-pressed={page === p} data-testid={`design-page-${p}`} onClick={() => setPage(p)}>
              {p === "tree" ? "트리" : "문서"}
            </button>
          ))}
        </nav>
      )}
      <div className="design-columns">
        {narrow && page === "tree" && <section className="design-tree-pane" aria-label="기획 트리">{tree}</section>}
        <section className="thread-view design-view" aria-label="기획 문서" hidden={!show("doc")}>
          <header className="term-head design-head">
            <DrawerToggle />
            <HeadNote />
            <div className="term-title design-title">
              {at ? <span className="design-crumb" data-testid="design-crumb">{at.repo} / {at.path.split("/").join(" / ")}</span> : <strong>기획</strong>}
              {hist && <span className="design-pill is-plan" data-testid="design-history-on">이력</span>}
            </div>
            {at && !hist && canControl && current && (
              <button type="button" className="btn design-edit" aria-label="편집 요청" title="편집 요청" data-testid="design-edit" onClick={() => setEdit({ doc: current })}><Pencil className="i" aria-hidden="true" /><span className="design-btn-label">편집 요청</span></button>
            )}
            {at && !hist && current && (
              <button type="button" className="btn design-edit" aria-label="이력" title="이력" data-testid="design-history" onClick={openHistory}><History className="i" aria-hidden="true" /><span className="design-btn-label">이력</span></button>
            )}
            {hist && <button type="button" className="btn" data-testid="design-history-close" onClick={() => setHist(null)}>닫기</button>}
            {at && !hist && (
              <button type="button" className="btn icon" aria-label="새로 읽기" title="새로 읽기" disabled={busy} data-testid="design-doc-reload" onClick={() => { onReload(); setRev((n) => n + 1); }}>
                <RefreshCw className={`i${busy ? " is-spinning" : ""}`} aria-hidden="true" />
              </button>
            )}
            <HeadMore />
          </header>
          {!at && <p className="design-empty" data-testid="design-none">고른 문서 없음</p>}
          {error && <div className="banner err" role="alert">{error}</div>}
          {hist && current ? (
            <div className="design-history" data-testid="design-history-view">
              <div className="design-history-list" data-testid="design-history-list">
                <p className="design-history-head">이 문서의 main 판 · 새것부터</p>
                {repo?.branches.filter((b) => b.thread).map((b) => <a key={b.slug} className="design-history-row" href={`#thread=${encodeURIComponent(b.thread!)}`}>지난 대화 · {b.title}</a>)}
                {hist.error && <p className="banner err" role="alert">{hist.error}</p>}
                {!hist.entries && !hist.error && <p className="design-empty">읽는 중</p>}
                {hist.entries?.map((e, i) => {
                  const who = whoOf(e);
                  const now = i === 0;
                  return (
                    <button key={e.commit} type="button" className={`design-history-row${hist.at === e.commit ? " is-on" : ""}`} data-testid="design-history-row" data-commit={e.commit} onClick={() => pickVersion(e.commit)}>
                      <span className="design-history-who"><span className={`design-who is-${who.tone}`}>{who.label}</span>{stamp(e.committed_ms)} · <code>{e.commit.slice(0, 7)}</code></span>
                      <span className="design-history-what">{e.subject.replace(/^기획: /, "").replace(/ \([^()]*\)$/, "")}{now && <span className="design-history-now"> · 지금 판</span>}</span>
                    </button>
                  );
                })}
              </div>
              <div className="design-history-view">
                {!hist.at ? <p className="design-empty">판을 고르면 지금 판과 다른 곳이 보인다</p> : hist.at === hist.entries?.[0]?.commit ? <p className="design-empty">지금 판이다</p> : (
                  <>
                    <div className="design-history-bar">
                      <span>{histPicked >= 0 && hist.entries ? stamp(hist.entries[histPicked]!.committed_ms) : ""} 판 <code>{hist.at.slice(0, 7)}</code></span>
                      <span className="spacer" />
                      {canControl && hist.old && <button type="button" className="btn small primary" data-testid="design-revert" onClick={() => setEdit({ doc: current, restore: hist.at! })}><RotateCcw className="i" aria-hidden="true" />이 판으로 복원 요청</button>}
                    </div>
                    {hist.error && <p className="banner err" role="alert">{hist.error}</p>}
                    {diff && (
                      <div className="design-text design-diff">
                        <p className="design-diff-legend"><span className="is-back">되돌리면 생김</span><span className="is-gone">되돌리면 사라짐</span></p>
                        {diff.map((d, i) => (
                          <div key={i} className={`design-block is-${d.kind}`} data-testid={d.kind === "same" ? undefined : `design-diff-${d.kind}`}>
                            <ReportMarkdown base={null} image={d.kind === "gone" ? image : histImage} text={d.text} onImages={lightbox.open} captions />
                          </div>
                        ))}
                      </div>
                    )}
                  </>
                )}
              </div>
            </div>
          ) : current && (
            <div className="design-body" data-testid="design-body">
              <div className="design-meta" data-testid="design-meta">
                {current.id && <span className="design-id-chip" title="기획서 ID" data-testid="design-id">{current.id}</span>}
                <span title={current.commit}>main <code>{current.commit.slice(0, 7)}</code></span>
                <span>{ago(current.committed_ms, now)}</span>
              </div>
              <div className="design-looks">
                {counts.work > 0 && <span className="design-count-chip is-work" data-testid="design-count-work">진행 {counts.work}</span>}
                {counts.plan > 0 && <span className="design-count-chip is-plan" data-testid="design-count-plan">계획 {counts.plan}</span>}
                {counts.open > 0 && <span className="design-count-chip is-open" data-testid="design-count-open">미정 {counts.open}</span>}
                <span className="spacer" />
                <div className="design-tabs" role="tablist" aria-label="보기" data-testid="design-looks">
                  {([["all", "전체"], ["now", "지금 모습"], ["left", "남은 것"]] as const).map(([k, label]) => (
                    <button key={k} type="button" role="tab" aria-selected={look === k} className={`design-tab${look === k ? " is-on" : ""}`} data-testid="design-look" data-look={k} onClick={() => setLook(k)}>{label}</button>
                  ))}
                </div>
              </div>
              <div className="design-text">
                <OpenFileContext.Provider value={openPath}>
                <AppIdChipContext.Provider value={chipOf}>
                  {look === "left" ? (
                    <div className="design-left" data-testid="design-left">
                      {left.length === 0 && <p className="design-empty">표지 블록 없음</p>}
                      {canHand && checked.size > 0 && (
                        <div className="design-left-bar" data-testid="design-left-bar">
                          <span>{checked.size}개 고름 · 할 일 하나로</span><span className="spacer" />
                          <button type="button" className="ghost small" onClick={() => setChecked(new Set())}>풀기</button>
                          <button type="button" className="btn small primary" data-testid="design-left-handoff" onClick={() => openHandoff([...checked])}><Hammer className="i" aria-hidden="true" />구현 맡기기</button>
                        </div>
                      )}
                      {left.map((g, gi) => (
                        <section key={gi} className="design-left-group">
                          <h3>{g.section}<span className="design-left-some">{g.some ? "일부 구현" : "계획만"}</span></h3>
                          {g.items.map(({ c, i }) => {
                            const state = c.task ? taskState(c.task, tasks, threads, lookup) : null;
                            return (
                              <div key={i} className={`design-left-row${checked.has(i) ? " is-checked" : ""}`} data-testid="design-left-row">
                                {canHand && (handable(c)
                                  ? <input type="checkbox" className="design-left-check" aria-label={`${c.title} 고르기`} checked={checked.has(i)}
                                      onChange={() => flipCheck(i)} data-testid="design-left-check" />
                                  : <span className="design-left-check" aria-hidden="true" />)}
                                {state ? <a className={`design-state is-${state.tone}`} href={`#task=${encodeURIComponent(c.task!)}`}>{state.label}</a>
                                  : <span className={`design-pill is-${c.kind}`}>{c.kind === "open" ? "미정" : "계획"}</span>}
                                <span className="design-left-title">{c.title}</span>
                                <span className="spacer" />
                                {c.kind === "open" && canControl && <button type="button" className="btn small design-discuss" data-testid="design-discuss" onClick={() => discuss(c)}><Pencil className="i" aria-hidden="true" />편집 요청</button>}
                              </div>
                            );
                          })}
                        </section>
                      ))}
                    </div>
                  ) : blocks ? renderBody()
                    : <ReportMarkdown base={null} image={image} text={current.text} onImages={lightbox.open} captions />}
                </AppIdChipContext.Provider>
                </OpenFileContext.Provider>
              </div>
            </div>
          )}
        </section>
      </div>
      {handoff && repo && at && (
        <HandoffDialog repo={repo} path={at.path} blob={handoff.blob} initial={handoff.initial} blocks={handoff.blocks}
          onClose={() => setHandoff(null)} onDone={() => { setChecked(new Set()); onReload(); setRev((n) => n + 1); }} />
      )}
      {edit && <DesignEditRequest doc={edit.doc} machine={repo?.machine ?? null} block={edit.block} restore={edit.restore} onClose={() => setEdit(null)} />}
      {lightbox.node}
    </div>
  );
}
