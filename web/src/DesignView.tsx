// 기획 문서 보기(t-1894, 확정 시안 library/mock2): 사이드바 「기획」 트리에서 고른 문서 하나와 오른쪽 기획 대화.
// 문서는 main이나 그 문서를 고친 기획 branch의 판으로 보고, branch 판은 main과 다른 덩어리에 표시를 단다. 「고치기」는 그 대화의
// 기획 branch에 저장한다. 그림·영상은 같은 commit의 저장소 파일이고, 문서 안 다른 문서 링크(상대 경로 .md)는 앱 안에서 연다.
import "./DesignView.css";
import { Check, GitBranch, ImagePlus, Pencil, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { ApiError, call, uploadDesignFile } from "./api";
import { DesignChat, NewChatDialog } from "./DesignChat";
import { branchesByPath, changedBlocks, designFileUrl, designTarget, isMarkdown, mdBlocks, resolveDocPath, restDays, splitTarget } from "./designModel";
import { DrawerToggle, HeadMore, HeadNote } from "./drawer";
import type { DesignBranchView, DesignDoc, DesignRepoView, PaneView, ThreadView } from "./gen/proto";
import { OpenFileContext } from "./herdr/filePaths.ts";
import { useLightbox } from "./Lightbox";
import { ago } from "./machineDetail";
import { ReportMarkdown } from "./ReportsView";
import { useMedia } from "./settings";
import { useMinute } from "./Sidebar";

type Page = "tree" | "doc" | "chat";

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
  const blocks = useMemo(() => (current && tab !== "main" ? mdBlocks(current.text) : null), [current, tab]);
  const marked = useMemo(() => (blocks ? changedBlocks(doc?.main ?? null, blocks) : null), [blocks, doc?.main]);
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
                  {blocks && marked ? blocks.map((b, i) => (
                    <div key={i} className={`design-block${marked[i] ? " is-changed" : ""}`} data-testid={marked[i] ? "design-changed" : undefined}>
                      <ReportMarkdown base={null} image={image} text={b} onImages={lightbox.open} captions />
                    </div>
                  )) : <ReportMarkdown base={null} image={image} text={current.text} onImages={lightbox.open} captions />}
                </OpenFileContext.Provider>
              </div>
            </div>
          )}
        </section>
        {repo && repo.folder && (
          <section className="design-chat-pane" aria-label="기획 대화" hidden={!show("chat")} data-testid="design-chat-pane">
            {chatError && <div className="banner err" role="alert">{chatError}</div>}
            <DesignChat repo={repo} branch={chat} threads={threads} panes={panes} canControl={canControl} viewing={at?.path ?? null}
              active={shown && show("chat")} now={now} onPick={(s) => { pick(s); }} onNew={() => setNewChat(true)} onChanged={onReload} onError={setChatError} />
          </section>
        )}
      </div>
      {newChat && repo && <NewChatDialog repo={repo} onClose={() => setNewChat(false)} onDone={(slug) => { setNewChat(false); pick(slug); onReload(); }} />}
      {lightbox.node}
    </div>
  );
}
