// 기획 문서 보기(t-1894, 확정 시안 library/mock2): 사이드바 「기획」 트리에서 고른 문서 하나와 오른쪽 기획 대화.
// 문서는 main이나 그 문서를 고친 기획 branch의 판으로 보고, branch 판은 main과 다른 덩어리에 표시를 단다. 「고치기」는 그 대화의
// 기획 branch에 저장한다. 그림·영상은 같은 commit의 저장소 파일이고, 문서 안 다른 문서 링크(상대 경로 .md)는 앱 안에서 연다.
import "./DesignView.css";
import { Check, CircleDot, GitBranch, Hammer, ImagePlus, MessageSquare, Pencil, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { ApiError, call, onTasksChanged, uploadDesignFile } from "./api";
import { splitAppIds, type AppId } from "./appIds";
import { DesignChat, NewChatDialog } from "./DesignChat";
import { handable, HandoffDialog, type HandoffBlock } from "./DesignHandoff";
import { branchesByPath, calloutOf, changedBlocks, designFileUrl, designTarget, isMarkdown, markCounts, marksOf, mdBlocks, resolveDocPath, restDays, sectionsOf, splitTarget, taskState, type ArchiveLookup, type Callout } from "./designModel";
import { DrawerToggle, HeadMore, HeadNote } from "./drawer";
import type { DesignBranchView, DesignDoc, DesignRepoView, PaneView, TaskView, ThreadView } from "./gen/proto";
import { OpenFileContext } from "./herdr/filePaths.ts";
import { useLightbox } from "./Lightbox";
import { AppIdChipContext } from "./Markdown";
import { ago } from "./machineDetail";
import { ReportMarkdown } from "./ReportsView";
import { useMedia } from "./settings";
import { useMinute } from "./Sidebar";

type Page = "tree" | "doc" | "chat";
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
  /** 기획 branch 판의 넘기지 않은 계획 블록만 */
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
        <button type="button" className="btn small design-discuss" data-testid="design-discuss" onClick={onDiscuss}><MessageSquare className="i" aria-hidden="true" />대화로 정하기</button>
      )}
      {onHandoff && (
        <button type="button" className="btn small design-discuss" data-testid="design-handoff" onClick={onHandoff}><Hammer className="i" aria-hidden="true" />구현 맡기기</button>
      )}
    </div>
  );
}

/** 저장소마다 고른 대화(기기에 남긴다) */
function storedChat(repo: string): string | null {
  try { return localStorage.getItem(`herdr-web-ui:design-chat:${repo}`); } catch { return null; }
}
function storeChat(repo: string, slug: string) {
  try { localStorage.setItem(`herdr-web-ui:design-chat:${repo}`, slug); } catch { /* 이 기기만의 편의 */ }
}

const clock = (ms: number) => new Date(ms).toLocaleTimeString("ko-KR", { hour: "2-digit", minute: "2-digit", hour12: false });

export function DesignView({ shown, target, repos, threads, panes, canControl, tree, onOpen, onReload }: {
  shown: boolean;
  /** `<저장소>/<경로>`, 없으면 고른 문서 없음 */
  target: string | null;
  /** 트리(문서 안 링크가 가리키는 문서가 있는지 본다). 트리를 다시 읽어 판이 바뀌면 문서도 그 판으로 다시 읽는다 */
  repos: DesignRepoView[] | null;
  threads: ThreadView[];
  panes: PaneView[];
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
  // 고른 대화(저장소마다)와 문서 판(main 또는 branch)
  const [chatSlug, setChatSlug] = useState<string | null>(null);
  useEffect(() => {
    if (!repo) return;
    const kept = storedChat(repo.repo);
    const open = repo.branches.filter((b) => b.state === "open").sort((a, b) => b.updated_ms - a.updated_ms);
    setChatSlug((cur) => (cur && repo.branches.some((b) => b.slug === cur) ? cur
      : kept && repo.branches.some((b) => b.slug === kept) ? kept : open[0]?.slug ?? repo.branches[0]?.slug ?? null));
  }, [repo]);
  const chat = repo?.branches.find((b) => b.slug === chatSlug) ?? null;
  const touching = useMemo(() => (at ? branchesByPath(repo).get(at.path) ?? [] : []), [repo, at?.path]); // eslint-disable-line react-hooks/exhaustive-deps
  const [tab, setTab] = useState<string>("main");
  // 문서나 대화가 바뀌면: 그 대화 branch가 이 문서를 고쳤으면 그 판, 아니면 main
  useEffect(() => { setTab(chat && touching.some((b) => b.slug === chat.slug) ? chat.slug : "main"); }, [key, chat?.slug]); // eslint-disable-line react-hooks/exhaustive-deps
  const pick = useCallback((slug: string) => {
    setChatSlug(slug);
    if (repo) storeChat(repo.repo, slug);
  }, [repo]);
  const branch = tab === "main" ? null : repo?.branches.find((b) => b.slug === tab) ?? null;
  // 받은 문서(branch 판이면 main 판도 함께: 다른 덩어리 표시)
  const [doc, setDoc] = useState<{ key: string; doc: DesignDoc; main: string | null } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [rev, setRev] = useState(0);
  const docKey = key ? `${key}@${tab}` : null;
  useEffect(() => {
    if (!at || !docKey) return;
    let live = true;
    setBusy(true);
    setError(null);
    const read = (b: string | null) => call({ op: "design_read", repo: at.repo, path: at.path, ...(b ? { branch: b } : {}) })
      .then((r) => (r.kind === "design_doc" ? r.doc : null));
    const wantMain = tab !== "main";
    Promise.all([read(tab === "main" ? null : tab), wantMain ? read(null).catch(() => null) : Promise.resolve(null)]).then(
      ([d, m]) => { if (live && d) setDoc({ key: docKey, doc: d, main: wantMain ? m?.text ?? null : null }); },
      (e: unknown) => { if (live) { setDoc(null); setError(e instanceof ApiError ? e.message : String(e)); } },
    ).finally(() => { if (live) setBusy(false); });
    return () => { live = false; };
  }, [docKey, rev, treeCommit, branch?.head]); // eslint-disable-line react-hooks/exhaustive-deps
  const current = doc && doc.key === docKey ? doc.doc : null;
  const docs = repo?.tree?.docs;
  const image = useCallback((src: string) => {
    if (!current) return null;
    const path = resolveDocPath(current.path, src);
    return path && !isMarkdown(path) ? designFileUrl(current.project, current.repo, path, current.commit, current.branch) : null;
  }, [current]);
  /** 문서 안 경로: 문서 폴더 기준, 없으면 저장소 기준으로 찾아 트리에 있는 문서면 연다 */
  const openPath = useCallback((raw: string) => {
    if (!current) return;
    const tries = [resolveDocPath(current.path, raw), resolveDocPath("x", raw)];
    const branchDocs = new Set(touching.flatMap((b) => b.changed.map((c) => c.path)));
    const hit = tries.find((p) => p && isMarkdown(p) && ((docs ?? []).some((d) => d.path === p) || branchDocs.has(p)));
    if (hit) onOpen(designTarget(current.repo, hit));
  }, [current, docs, onOpen, touching]);
  // 고치기: 그 판(branch)이나 고른 대화의 branch에 저장한다. 대화가 없으면 새 대화부터
  const [editing, setEditing] = useState<{ slug: string; text: string; base: string | null; was: string } | null>(null);
  const [newChat, setNewChat] = useState(false);
  const [saving, setSaving] = useState(false);
  const editor = useRef<HTMLTextAreaElement | null>(null);
  const picker = useRef<HTMLInputElement | null>(null);
  // 그림: 그 branch의 문서 폴더에 commit하고 글 커서 자리에 그림 줄을 넣는다(같은 폴더라 이름만 쓴다)
  const attach = async (files: FileList | null) => {
    if (!at || !editing || !current || !files?.length) return;
    // 올리는 동안 글은 읽기만 한다(그 사이 친 글을 잃지 않게). 올린 것만큼은 실패해도 줄을 넣는다
    setSaving(true);
    const slug = editing.slug;
    const cursor = editor.current?.selectionStart ?? editing.text.length;
    const lines: string[] = [];
    try {
      const dir = at.path.slice(0, at.path.lastIndexOf("/"));
      for (const file of [...files]) {
        const r = await uploadDesignFile(current.project, at.repo, slug, dir, file);
        const name = r.path.slice(r.path.lastIndexOf("/") + 1);
        lines.push(`![${file.name.replace(/\.[^.]+$/, "").replace(/[[\]]/g, "")}](${name})`);
      }
    } catch (e) {
      setError(e instanceof ApiError ? e.message : String(e));
    } finally {
      // 지금 고치는 글(같은 branch일 때만)에 넣는다: 그 사이 취소했거나 다른 문서를 열었으면 넣지 않는다
      if (lines.length > 0) {
        setEditing((now) => {
          if (!now || now.slug !== slug) return now;
          const at = Math.min(cursor, now.text.length);
          const block = `${at > 0 && now.text[at - 1] !== "\n" ? "\n" : ""}${lines.join("\n")}\n`;
          return { ...now, text: now.text.slice(0, at) + block + now.text.slice(at) };
        });
        onReload();
      }
      setSaving(false);
      if (picker.current) picker.current.value = "";
    }
  };
  const [chatError, setChatError] = useState<string | null>(null);
  useEffect(() => setEditing(null), [key]);
  const startEdit = async () => {
    if (!at) return;
    const slug = branch?.slug ?? (chat?.state === "open" ? chat.slug : null);
    if (!slug) { setNewChat(true); return; }
    try {
      const r = await call({ op: "design_read", repo: at.repo, path: at.path, branch: slug });
      if (r.kind === "design_doc") setEditing({ slug, text: r.doc.text, base: r.doc.blob, was: r.doc.text });
    } catch (e) {
      // 그 branch에 없는 문서(main에만 있다)는 main 판의 글로 그 branch에 새로 만든다(기준 판 없음)
      if (current && tab === "main") setEditing({ slug, text: current.text, base: null, was: current.text });
      else setError(e instanceof ApiError ? e.message : String(e));
    }
  };
  const save = async () => {
    if (!at || !editing) return;
    setSaving(true);
    try {
      await call({ op: "design_save", repo: at.repo, branch: editing.slug, path: at.path, ...(editing.base ? { base: editing.base } : {}), text: editing.text });
      setTab(editing.slug);
      pick(editing.slug);
      setEditing(null);
      onReload();
      setRev((n) => n + 1);
    } catch (e) {
      setError(e instanceof ApiError ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };
  // 덩어리: 표지 블록을 그리려고 늘 나눈다. branch 판이면 main과 다른 덩어리도 표시한다
  const blocks = useMemo(() => (current ? mdBlocks(current.text) : null), [current]);
  const marked = useMemo(() => (blocks && tab !== "main" ? changedBlocks(doc?.main ?? null, blocks) : null), [blocks, doc?.main, tab]);
  const sections = useMemo(() => (blocks && current ? sectionsOf(blocks, current.title) : []), [blocks, current]);
  // main 문서에서 맡긴 블록(3-3): 표지는 구현 branch에만 있어 main 글에는 없다. 맡김 기록의 블록 글로 찾아 그 할 일로 본다
  // 맡긴 블록은 다른 판에 아직 표지가 없을 수 있다(main 맡김은 구현 branch에, 기획 branch 맡김은 그 branch에): 같은 글이면
  // 어느 판에서든 그 할 일로 보이고 다시 맡기지 않는다. 그 판에 표지가 있는 블록은 표지대로다
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
  // 대화로 정하기: 고른 블록을 대화 입력의 맥락으로 넘긴다
  const [picked, setPicked] = useState<{ path: string; section: string; title: string } | null>(null);
  useEffect(() => setPicked(null), [key]);
  // 구현 맡기기(3-2): 기획 branch 판에서만. 「남은 것」에서 고른 블록과 창
  // 기획 branch 판, 또는 main 판(3-3: 기획 branch 없이 맡기고 표지는 구현 branch 첫 commit에)
  const canHand = canControl && (branch ? branch.state === "open" : tab === "main");
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
  const discuss = (c: Callout) => { if (at) { setPicked({ path: at.path, section: c.section, title: c.title }); if (narrow) setPage("chat"); } };
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
    const c = callouts[i];
    const md = <ReportMarkdown base={null} image={image} text={c ? c.body : b} onImages={lightbox.open} captions />;
    return (
      <div key={i} className={`design-block${marked?.[i] ? " is-changed" : ""}`} data-testid={marked?.[i] ? "design-changed" : undefined}>
        {c ? <DesignCallout c={c} tasks={tasks} lookup={lookup} threads={threads} onDiscuss={canControl ? () => discuss(c) : undefined}
          onHandoff={canHand && handable(c) ? () => openHandoff([i]) : undefined}>{md}</DesignCallout> : md}
      </div>
    );
  };
  // 본문: main 판은 표지 블록 사이의 보통 글을 이어 한 번에 그린다(목록 이어짐·참조식 링크·각주가 덩어리마다 끊기지 않게).
  // 표지가 없으면 문서 전체를 그대로 그린다. branch 판은 main과 다른 덩어리를 표시하므로 덩어리마다 그린다(2-3과 같다)
  const renderBody = (): ReactNode => {
    if (!blocks || !current) return null;
    if (!marked && callouts.every((c) => !c)) return <ReportMarkdown base={null} image={image} text={current.text} onImages={lightbox.open} captions />;
    const shown = (i: number) => !(look === "now" && (callouts[i] || onlyPlans(i)));
    if (marked) return blocks.map((b, i) => (shown(i) ? renderBlock(b, i) : null));
    const out: ReactNode[] = [];
    let run: string[] = [];
    let start = 0;
    const flush = () => {
      if (run.length) out.push(<div key={`text-${start}`} className="design-block"><ReportMarkdown base={null} image={image} text={run.join("\n\n")} onImages={lightbox.open} captions /></div>);
      run = [];
    };
    blocks.forEach((b, i) => {
      if (!shown(i)) return;
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
  const others = touching.filter((b) => b.slug !== tab);
  const stateOf = (b: DesignBranchView) => {
    const apply = b.apply ? threads.find((t) => t.id === b.apply) : undefined;
    if (apply && apply.status !== "resolved" && apply.status !== "failed") return "반영 대기";
    return b.state === "rest" ? `쉼 ${restDays(b, now)}일` : "대화 중";
  };
  const tabs = ["main", ...touching.map((b) => b.slug), ...(chat && chat.state === "open" && !touching.some((b) => b.slug === chat.slug) && editing?.slug === chat.slug ? [chat.slug] : [])];
  const show = (p: Page) => !narrow || page === p;
  return (
    <div className={`design-screen${shown ? "" : " is-hidden"}`} inert={!shown} data-testid="design-view" data-shown={shown ? "true" : "false"}>
      {narrow && (
        <nav className="design-pages" aria-label="기획 칸">
          {(["tree", "doc", "chat"] as const).map((p) => (
            <button key={p} type="button" className={page === p ? "is-on" : undefined} aria-pressed={page === p} data-testid={`design-page-${p}`} onClick={() => setPage(p)}>
              {p === "tree" ? "트리" : p === "doc" ? "문서" : "대화"}
            </button>
          ))}
          <span className="spacer" />
          {chat && <span className="design-branch-chip"><GitBranch aria-hidden="true" />{chat.slug}</span>}
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
              {editing && <span className="design-branch-chip"><GitBranch aria-hidden="true" />{editing.slug}</span>}
            </div>
            {at && !editing && tabs.length > 1 && (
              <div className="design-tabs" role="tablist" aria-label="판" data-testid="design-tabs">
                {tabs.map((t) => {
                  const b = repo?.branches.find((x) => x.slug === t);
                  return (
                    <button key={t} type="button" role="tab" aria-selected={tab === t} className={`design-tab${tab === t ? " is-on" : ""}${b?.state === "rest" ? " is-rest" : ""}`}
                      data-testid="design-tab" data-tab={t} onClick={() => { setTab(t); if (t !== "main") pick(t); }}>
                      {t !== "main" && <GitBranch aria-hidden="true" />}{t}
                    </button>
                  );
                })}
              </div>
            )}
            {at && !editing && canControl && current && (
              <button type="button" className="btn design-edit" aria-label="고치기" title="고치기" data-testid="design-edit" onClick={() => void startEdit()}><Pencil className="i" aria-hidden="true" /><span className="design-btn-label">고치기</span></button>
            )}
            {editing && (
              <>
                <input ref={picker} type="file" accept="image/png,image/jpeg,image/gif,image/webp,video/mp4,video/webm" multiple hidden data-testid="design-attach-input"
                  onChange={(e) => void attach(e.target.files)} />
                <button type="button" className="btn design-edit" aria-label="그림" title="그림" disabled={saving} data-testid="design-attach" onClick={() => picker.current?.click()}>
                  <ImagePlus className="i" aria-hidden="true" /><span className="design-btn-label">그림</span>
                </button>
                <button type="button" className="btn" disabled={saving} data-testid="design-edit-cancel" onClick={() => setEditing(null)}>취소</button>
                <button type="button" className="btn primary" disabled={saving || editing.text === editing.was} data-testid="design-edit-save" onClick={() => void save()}>
                  <Check className="i" aria-hidden="true" />{editing.slug}에 저장
                </button>
              </>
            )}
            {at && !editing && (
              <button type="button" className="btn icon" aria-label="새로 읽기" title="새로 읽기" disabled={busy} data-testid="design-doc-reload" onClick={() => { onReload(); setRev((n) => n + 1); }}>
                <RefreshCw className={`i${busy ? " is-spinning" : ""}`} aria-hidden="true" />
              </button>
            )}
            <HeadMore />
          </header>
          {!at && <p className="design-empty" data-testid="design-none">고른 문서 없음</p>}
          {error && <div className="banner err" role="alert">{error}</div>}
          {editing ? (
            <div className="design-body">
              <textarea ref={editor} className="design-editor" value={editing.text} readOnly={saving} spellCheck={false} autoFocus aria-label="문서 글" data-testid="design-editor"
                onChange={(e) => setEditing({ ...editing, text: e.target.value })} />
            </div>
          ) : current && (
            <div className="design-body" data-testid="design-body">
              <div className="design-meta" data-testid="design-meta">
                {branch ? (
                  <>
                    <span className="design-branch-chip"><GitBranch aria-hidden="true" />design/{branch.slug}</span>
                    <span title={current.commit}>판 <code>{current.commit.slice(0, 7)}</code> · main보다 {branch.ahead} 앞{branch.pushed && branch.pushed === branch.head && branch.pushed_ms > 0 ? ` · ${clock(branch.pushed_ms)} 올림` : branch.state === "rest" ? ` · 쉼 ${restDays(branch, now)}일` : " · 올리기 전"} · 대화 「{branch.title}」</span>
                  </>
                ) : (
                  <>
                    <span title={current.commit}>판 <code>{current.commit.slice(0, 7)}</code></span>
                    <span>{ago(current.committed_ms, now)}</span>
                  </>
                )}
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
              {others.length > 0 && (
                <div className="design-others">
                  {others.map((b) => (
                    <button key={b.slug} type="button" className="design-other" data-testid="design-other" onClick={() => { setTab(b.slug); pick(b.slug); }}>
                      <GitBranch aria-hidden="true" /><code>{b.slug}</code>도 이 문서를 고침 · {stateOf(b)}
                    </button>
                  ))}
                </div>
              )}
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
                                {c.kind === "open" && canControl && <button type="button" className="btn small design-discuss" data-testid="design-discuss" onClick={() => discuss(c)}><MessageSquare className="i" aria-hidden="true" />대화로 정하기</button>}
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
        {repo && repo.folder && (
          <section className="design-chat-pane" aria-label="기획 대화" hidden={!show("chat")} data-testid="design-chat-pane">
            {chatError && <div className="banner err" role="alert">{chatError}</div>}
            <DesignChat repo={repo} branch={chat} threads={threads} panes={panes} canControl={canControl} viewing={at?.path ?? null}
              picked={picked} onUnpick={() => setPicked(null)}
              active={shown && show("chat")} now={now} onPick={(s) => { pick(s); }} onNew={() => setNewChat(true)} onChanged={onReload} onError={setChatError} />
          </section>
        )}
      </div>
      {handoff && repo && at && (
        <HandoffDialog repo={repo} branch={branch} path={at.path} blob={handoff.blob} initial={handoff.initial} blocks={handoff.blocks}
          onClose={() => setHandoff(null)} onDone={() => { setChecked(new Set()); onReload(); setRev((n) => n + 1); }} />
      )}
      {newChat && repo && <NewChatDialog repo={repo} onClose={() => setNewChat(false)} onDone={(slug) => { setNewChat(false); pick(slug); onReload(); }} />}
      {lightbox.node}
    </div>
  );
}
