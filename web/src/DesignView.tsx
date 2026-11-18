// 기획 문서 보기(t-1894, t-2002 확정 시안 library/mock): 사이드바 「기획」 트리에서 고른 문서 하나와 오른쪽 기획 대화.
// 문서는 늘 main 판이다. 「고치기」는 main에 바로 저장하고(저장소 문서 검사를 지나야 올라간다, 실패하면 까닭을 보이고 글은 남긴다),
// 「이력」은 main이 가졌던 판과 지금 판의 차이를 보이고 고른 판으로 되돌린다. 그림·영상은 같은 commit의 저장소 파일이고,
// 문서 안 다른 문서 링크(상대 경로 .md)는 앱 안에서 연다.
import "./DesignView.css";
import { Check, CircleDot, Hammer, History, ImagePlus, MessageSquare, Pencil, RefreshCw, RotateCcw, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { ApiError, call, onTasksChanged, uploadDesignFile } from "./api";
import { splitAppIds, type AppId } from "./appIds";
import { DesignChat, NewChatDialog } from "./DesignChat";
import { handable, HandoffDialog, type HandoffBlock } from "./DesignHandoff";
import { blockDiff, calloutOf, designFileUrl, designTarget, idLineOf, isMarkdown, markCounts, marksOf, mdBlocks, resolveDocPath, sectionsOf, splitTarget, taskState, withoutIdLines, type ArchiveLookup, type Callout } from "./designModel";
import { DrawerToggle, HeadMore, HeadNote } from "./drawer";
import type { DesignDoc, DesignLogEntry, DesignRepoView, PaneView, TaskView, ThreadView } from "./gen/proto";
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

const stamp = (ms: number) => {
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
};

/** 문서 이력(main이 가졌던 판)과 고른 옛 판 */
type History = { entries: DesignLogEntry[] | null; at: string | null; old: DesignDoc | null; error: string | null };

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
  // 고른 대화(저장소마다): 저장·그림·맡기기의 사건을 그 대화에 남긴다(없으면 남기지 않는다)
  const [chatSlug, setChatSlug] = useState<string | null>(null);
  useEffect(() => {
    if (!repo) return;
    const kept = storedChat(repo.repo);
    const recent = [...repo.branches].sort((a, b) => b.updated_ms - a.updated_ms);
    setChatSlug((cur) => (cur && repo.branches.some((b) => b.slug === cur) ? cur
      : kept && repo.branches.some((b) => b.slug === kept) ? kept : recent[0]?.slug ?? null));
  }, [repo]);
  const chat = repo?.branches.find((b) => b.slug === chatSlug) ?? null;
  const pick = useCallback((slug: string) => {
    setChatSlug(slug);
    if (repo) storeChat(repo.repo, slug);
  }, [repo]);
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
  // 고치기: main에 바로 저장한다. 고른 대화가 있으면 그 대화에 「사람이 고침」으로 남는다
  const [editing, setEditing] = useState<{ text: string; base: string; was: string } | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [newChat, setNewChat] = useState(false);
  const [saving, setSaving] = useState(false);
  const editor = useRef<HTMLTextAreaElement | null>(null);
  const picker = useRef<HTMLInputElement | null>(null);
  // 그림: main의 문서 폴더(저장소 `design_media`가 있으면 그 아래 손 그림 폴더)에 commit하고 글 커서 자리에 그림 줄을 넣는다
  const attach = async (files: FileList | null) => {
    if (!at || !editing || !current || !files?.length) return;
    // 올리는 동안 글은 읽기만 한다(그 사이 친 글을 잃지 않게). 올린 것만큼은 실패해도 줄을 넣는다
    setSaving(true);
    const cursor = editor.current?.selectionStart ?? editing.text.length;
    const lines: string[] = [];
    try {
      const dir = at.path.slice(0, at.path.lastIndexOf("/"));
      for (const file of [...files]) {
        const r = await uploadDesignFile(current.project, at.repo, chat?.slug ?? "main", dir, file);
        // 저장소가 손 그림 폴더(`design_media`)를 정했으면 문서 폴더 아래 그 폴더에 들어온다: 문서 기준 상대 경로로 넣는다
        const rel = dir && r.path.startsWith(`${dir}/`) ? r.path.slice(dir.length + 1) : r.path.slice(r.path.lastIndexOf("/") + 1);
        lines.push(`![${file.name.replace(/\.[^.]+$/, "").replace(/[[\]]/g, "")}](${rel})`);
      }
    } catch (e) {
      setSaveError(e instanceof ApiError ? e.message : String(e));
    } finally {
      // 지금 고치는 글에 넣는다: 그 사이 취소했거나 다른 문서를 열었으면 넣지 않는다
      if (lines.length > 0) {
        setEditing((now) => {
          if (!now) return now;
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
  useEffect(() => { setEditing(null); setSaveError(null); }, [key]);
  const startEdit = async () => {
    if (!at) return;
    try {
      const r = await call({ op: "design_read", repo: at.repo, path: at.path });
      if (r.kind === "design_doc") { setSaveError(null); setEditing({ text: r.doc.text, base: r.doc.blob, was: r.doc.text }); }
    } catch (e) {
      setError(e instanceof ApiError ? e.message : String(e));
    }
  };
  const save = async () => {
    if (!at || !editing) return;
    setSaving(true);
    setSaveError(null);
    try {
      await call({ op: "design_save", repo: at.repo, branch: chat?.slug ?? "", path: at.path, base: editing.base, text: editing.text });
      setEditing(null);
      onReload();
      setRev((n) => n + 1);
    } catch (e) {
      // 검사 실패·그 사이 다른 판: main은 그대로이고 쓴 글은 남는다. 트리는 다시 읽어 다른 기기가 올린 판을 안다
      setSaveError(e instanceof ApiError ? e.message : String(e));
      onReload();
    } finally {
      setSaving(false);
    }
  };
  // 이력: main이 가졌던 판을 새것부터. 판을 고르면 지금 판과 다른 덩어리를 보이고 그 판으로 되돌린다
  const [hist, setHist] = useState<History | null>(null);
  const [revert, setRevert] = useState(false);
  useEffect(() => { setHist(null); setRevert(false); }, [key]);
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
  // 대화로 정하기: 고른 블록을 대화 입력의 맥락으로 넘긴다
  const [picked, setPicked] = useState<{ path: string; section: string; title: string } | null>(null);
  useEffect(() => setPicked(null), [key]);
  // 구현 맡기기: 고른 블록 표지에 d-ID를 적어 main에 바로 저장한다(t-2002 q-4224 1A). 「남은 것」에서 고른 블록과 창
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
          {(["tree", "doc", "chat"] as const).map((p) => (
            <button key={p} type="button" className={page === p ? "is-on" : undefined} aria-pressed={page === p} data-testid={`design-page-${p}`} onClick={() => setPage(p)}>
              {p === "tree" ? "트리" : p === "doc" ? "문서" : "대화"}
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
            {at && !editing && !hist && canControl && current && (
              <button type="button" className="btn design-edit" aria-label="고치기" title="고치기" data-testid="design-edit" onClick={() => void startEdit()}><Pencil className="i" aria-hidden="true" /><span className="design-btn-label">고치기</span></button>
            )}
            {at && !editing && !hist && current && (
              <button type="button" className="btn design-edit" aria-label="이력" title="이력" data-testid="design-history" onClick={openHistory}><History className="i" aria-hidden="true" /><span className="design-btn-label">이력</span></button>
            )}
            {hist && <button type="button" className="btn" data-testid="design-history-close" onClick={() => setHist(null)}>닫기</button>}
            {editing && (
              <>
                <input ref={picker} type="file" accept="image/png,image/jpeg,image/gif,image/webp,video/mp4,video/webm" multiple hidden data-testid="design-attach-input"
                  onChange={(e) => void attach(e.target.files)} />
                <button type="button" className="btn design-edit" aria-label="그림" title="그림" disabled={saving} data-testid="design-attach" onClick={() => picker.current?.click()}>
                  <ImagePlus className="i" aria-hidden="true" /><span className="design-btn-label">그림</span>
                </button>
                <button type="button" className="btn" disabled={saving} data-testid="design-edit-cancel"
                  onClick={() => { setEditing(null); setSaveError(null); setRev((n) => n + 1); }}>취소</button>
                <button type="button" className="btn primary" disabled={saving || editing.text === editing.was} data-testid="design-edit-save" onClick={() => void save()}>
                  <Check className="i" aria-hidden="true" />main에 저장
                </button>
              </>
            )}
            {at && !editing && !hist && (
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
              {saveError ? (
                <div className="design-save-error" role="alert" data-testid="design-save-error">
                  <strong><X className="i" aria-hidden="true" />저장하지 않았다</strong>
                  <span>main은 그대로다. 고친 뒤 다시 저장한다. 쓴 글은 그대로 남아 있다.</span>
                  <pre>{saveError}</pre>
                </div>
              ) : (
                <p className="design-save-note" data-testid="design-save-note">저장하면 저장소 문서 검사를 지나 바로 main에 들어간다 · 기준 판 <code>{editing.base.slice(0, 7)}</code></p>
              )}
              <textarea ref={editor} className="design-editor" value={editing.text} readOnly={saving} spellCheck={false} autoFocus aria-label="문서 글" data-testid="design-editor"
                onChange={(e) => setEditing({ ...editing, text: e.target.value })} />
            </div>
          ) : hist && current ? (
            <div className="design-history" data-testid="design-history-view">
              <div className="design-history-list" data-testid="design-history-list">
                <p className="design-history-head">이 문서의 main 판 · 새것부터</p>
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
                      {canControl && hist.old && <button type="button" className="btn small primary" data-testid="design-revert" onClick={() => setRevert(true)}><RotateCcw className="i" aria-hidden="true" />이 판으로 되돌리기</button>}
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
            {/* 프로젝트는 판을 다시 읽는 동안에도 남는 앞 문서에서 얻는다: 대화를 바꾸면 판이 바뀌어 `current`가 잠시 빈다(t-1950 착지 실패) */}
            <DesignChat project={doc?.doc.project ?? null} repo={repo} branch={chat} threads={threads} panes={panes} canControl={canControl} viewing={at?.path ?? null}
              picked={picked} onUnpick={() => setPicked(null)}
              active={shown && show("chat")} onPick={(s) => { pick(s); }} onNew={() => setNewChat(true)} onChanged={onReload} onError={setChatError} />
          </section>
        )}
      </div>
      {handoff && repo && at && (
        <HandoffDialog repo={repo} chat={chat} path={at.path} blob={handoff.blob} initial={handoff.initial} blocks={handoff.blocks}
          onClose={() => setHandoff(null)} onDone={() => { setChecked(new Set()); onReload(); setRev((n) => n + 1); }} />
      )}
      {revert && hist?.at && hist.old && current && at && (
        <RevertDialog repo={at.repo} path={at.path} at={hist.at} base={current.blob} chat={chat?.slug ?? ""}
          lost={(hist.entries ?? []).slice(0, Math.max(0, histPicked)).map((e) => ({ e, who: whoOf(e) }))}
          onClose={() => setRevert(false)} onDone={() => { setRevert(false); setHist(null); onReload(); setRev((n) => n + 1); }} />
      )}
      {newChat && repo && <NewChatDialog repo={repo} onClose={() => setNewChat(false)} onDone={(slug) => { setNewChat(false); pick(slug); onReload(); }} />}
      {lightbox.node}
    </div>
  );
}

/** 이 판으로 되돌리기: 옛 판 글을 지금 main 위에 새로 저장한다. 사라지는 저장(이력에는 남는다)을 보인다 */
function RevertDialog({ repo, path, at, base, chat, lost, onClose, onDone }: {
  repo: string;
  path: string;
  at: string;
  base: string;
  chat: string;
  lost: { e: DesignLogEntry; who: { label: string; tone: string } }[];
  onClose: () => void;
  onDone: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const submit = async () => {
    setBusy(true);
    setProblem(null);
    try {
      await call({ op: "design_revert", repo, path, at, base, branch: chat });
      onDone();
    } catch (e) {
      setProblem(e instanceof ApiError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="modal-scrim" onMouseDown={(e) => { if (e.target === e.currentTarget && !busy) onClose(); }}>
      <div className="modal design-dialog" role="dialog" aria-modal="true" aria-label="이 판으로 되돌리기" data-testid="design-revert-dialog">
        <header className="modal-header"><h2 className="modal-title"><RotateCcw className="i" aria-hidden="true" />이 판으로 되돌리기 <code>{path.slice(path.indexOf("/") + 1)}</code></h2></header>
        <div className="modal-body">
          <p>판 <code>{at.slice(0, 7)}</code>의 글을 지금 main 위에 새로 저장한다.</p>
          {lost.length > 0 && (
            <ul className="design-files">
              {lost.map(({ e, who }) => (
                <li key={e.commit} className="design-file-row">
                  <span className={`design-who is-${who.tone}`}>{who.label}</span>
                  <span>{stamp(e.committed_ms)} {e.subject.replace(/^기획: /, "").replace(/ \([^()]*\)$/, "")}</span>
                  <span className="spacer" /><span className="design-minus">사라짐</span>
                </li>
              ))}
            </ul>
          )}
          <ul className="design-checks">
            <li className="is-ok"><Check className="i" aria-hidden="true" />사라지는 저장도 이력에 남아 다시 되돌릴 수 있다</li>
            <li className="is-ok"><Check className="i" aria-hidden="true" />넘긴 블록 표지(<code>[!계획] d-NNNN</code>)는 그대로 둔다</li>
            <li className="is-ok"><Check className="i" aria-hidden="true" />저장할 때 저장소 문서 검사를 한다</li>
          </ul>
          {problem && <p className="confirm-error" role="alert">{problem}</p>}
        </div>
        <footer className="modal-footer">
          <span className="design-hand-note">main에 바로 들어간다</span>
          <span className="spacer" />
          <button type="button" className="btn" disabled={busy} onClick={onClose}>취소</button>
          <button type="button" className="btn primary" disabled={busy} data-testid="design-revert-submit" onClick={() => void submit()}><RotateCcw className="i" aria-hidden="true" />되돌리기</button>
        </footer>
      </div>
    </div>
  );
}
