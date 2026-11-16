// 기획 트리(t-1894, 확정 계획 plan §2): 활동 막대 「기획」의 사이드바 칸. 프로젝트 저장소 → 폴더 → 문서이고 폴더가 먼저 온다.
// 기획 폴더를 정하지 않은 저장소도 한 줄로 보인다. 1단계는 읽기 전용이다(origin 기본 branch).
import { ChevronDown, ChevronRight, FileText, Folder, FolderOpen, GitBranch, Plus, RefreshCw, TriangleAlert } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { ApiError, call } from "./api";
import { addedDocs, ancestors, branchesByPath, designTarget, designTree, docLabel, docsUnder, markCounts, splitTarget, stem, type DesignNode } from "./designModel";
import type { DesignBranchView, DesignRepoView } from "./gen/proto";
import { onRowKey } from "./Sidebar";

export type Design = { repos: DesignRepoView[] | null; error: string | null; loading: boolean; reload: () => void };

/** host 기획 트리. 「기획」 칸을 처음 열 때와 「새로 읽기」에서 읽는다(origin은 agent가 1분에 한 번 받는다) */
export function useDesign(up: boolean, wanted: boolean): Design {
  const [repos, setRepos] = useState<DesignRepoView[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [rev, setRev] = useState(0);
  const reload = useCallback(() => setRev((n) => n + 1), []);
  const [asked, setAsked] = useState(false);
  useEffect(() => { if (wanted) setAsked(true); }, [wanted]);
  useEffect(() => {
    if (!up || !asked) return;
    let live = true;
    setLoading(true);
    call({ op: "design_tree" }).then(
      (r) => { if (live && r.kind === "design_tree") { setRepos(r.repos); setError(null); } },
      (e: unknown) => { if (live) setError(e instanceof ApiError ? e.message : String(e)); },
    ).finally(() => { if (live) setLoading(false); });
    return () => { live = false; };
  }, [up, asked, rev]);
  return { repos, error, loading, reload };
}

export function DesignPanel({ design, selected, onOpen }: {
  design: Design;
  /** 본문에 연 문서(`<저장소>/<경로>`) */
  selected: string | null;
  onOpen: (target: string) => void;
}) {
  const at = selected ? splitTarget(selected) : null;
  // 접은 것(저장소 이름·폴더 경로). 처음엔 저장소는 펼치고 폴더는 접는다. 연 문서의 폴더는 펼친다
  const [open, setOpen] = useState<Set<string>>(() => new Set());
  const [shut, setShut] = useState<Set<string>>(() => new Set());
  useEffect(() => {
    if (!at) return;
    setOpen((s) => {
      const want = ancestors(at.path).map((p) => `${at.repo}:${p}`);
      return want.every((k) => s.has(k)) ? s : new Set([...s, ...want]);
    });
    setShut((s) => (s.has(at.repo) ? new Set([...s].filter((k) => k !== at.repo)) : s));
  }, [at?.repo, at?.path]); // eslint-disable-line react-hooks/exhaustive-deps
  const flip = (set: React.Dispatch<React.SetStateAction<Set<string>>>, key: string) =>
    set((s) => { const n = new Set(s); if (n.has(key)) n.delete(key); else n.add(key); return n; });
  const repos = design.repos ?? [];
  // 모두 | 할 것(표지 블록이 있는 문서만). 기기에 남긴다
  const [todo, setTodoState] = useState<boolean>(() => { try { return localStorage.getItem("herdr-web-ui:design-todo") === "1"; } catch { return false; } });
  const setTodo = (v: boolean) => { setTodoState(v); try { localStorage.setItem("herdr-web-ui:design-todo", v ? "1" : "0"); } catch { /* 이 기기만의 편의 */ } };
  return (
    <section className="design-panel" aria-label="기획 트리" data-testid="design-panel">
      <div className="design-panel-top">
        <button type="button" className="sidebar-row-action design-reload" disabled={design.loading} data-testid="design-reload" onClick={design.reload}>
          <span className="sidebar-mark" aria-hidden="true"><RefreshCw className={design.loading ? "is-spinning" : undefined} /></span>새로 읽기
        </button>
        <div className="design-tabs is-small" role="tablist" aria-label="문서 보기" data-testid="design-todo">
          <button type="button" role="tab" aria-selected={!todo} className={`design-tab${!todo ? " is-on" : ""}`} onClick={() => setTodo(false)}>모두</button>
          <button type="button" role="tab" aria-selected={todo} className={`design-tab${todo ? " is-on" : ""}`} data-testid="design-todo-on" onClick={() => setTodo(true)}>할 것</button>
        </div>
      </div>
      {design.error && <p className="banner err" role="alert">{design.error}</p>}
      {design.repos === null && !design.error && <p className="tree-state" role="status">불러오는 중</p>}
      {design.repos?.length === 0 && <p className="tree-state" data-testid="design-empty">저장소 없음</p>}
      <ul className="design-tree" role="tree">
        {repos.map((r) => (
          <RepoRow key={r.repo} repo={r} todo={todo} onAll={() => setTodo(false)} collapsed={shut.has(r.repo)} onToggle={() => flip(setShut, r.repo)}
            openDirs={open} onToggleDir={(p) => flip(setOpen, `${r.repo}:${p}`)} selected={at?.repo === r.repo ? at.path : null} onOpen={onOpen} />
        ))}
      </ul>
    </section>
  );
}

/** 노드 아래 문서들의 표지 수 합 */
const sumCounts = (node: DesignNode) => markCounts(docsUnder(node));

function Counts({ n }: { n: { work: number; plan: number; open: number } }) {
  if (n.work + n.plan + n.open === 0) return null;
  return (
    <span className="design-marks" data-testid="design-marks">
      {n.work > 0 && <span className="is-work" title="진행(할 일로 넘긴 계획)">{n.work}</span>}
      {n.plan > 0 && <span className="is-plan" title="계획">{n.plan}</span>}
      {n.open > 0 && <span className="is-open" title="미정">{n.open}</span>}
    </span>
  );
}

function RepoRow({ repo: r, todo, onAll, collapsed, onToggle, openDirs, onToggleDir, selected, onOpen }: {
  repo: DesignRepoView;
  /** 할 것: 표지 블록이 있는 문서만 */
  todo: boolean;
  onAll: () => void;
  collapsed: boolean;
  onToggle: () => void;
  openDirs: Set<string>;
  onToggleDir: (path: string) => void;
  selected: string | null;
  onOpen: (target: string) => void;
}) {
  // branch에서 새로 만든 문서도 트리에 더한다(「+」)
  const added = useMemo(() => addedDocs(r), [r]);
  const marks = useMemo(() => branchesByPath(r), [r]);
  const nodes = useMemo(() => (r.folder && r.tree
    ? designTree(r.folder, [...r.tree.docs, ...added.map((a) => ({ path: a.path, title: "", bytes: 0, plans: 0, open: 0, tasks: [] }))])
    : []), [r.folder, r.tree, added]);
  const fresh = useMemo(() => new Set(added.map((a) => a.path)), [added]);
  const total = useMemo(() => markCounts(nodes.flatMap(docsUnder)), [nodes]);
  const plain = useMemo(() => (r.tree?.docs ?? []).filter((d) => { const n = markCounts([d]); return n.work + n.plan + n.open === 0; }).length, [r.tree]);
  // 표지 수를 세지 못한 트리(옛 agent·세기 실패)는 「할 것」으로 거르지 않는다: 0과 모름은 다르다
  const counted = r.tree?.marks ?? false;
  const todoHere = todo && counted;
  const keep = (n: DesignNode) => !todoHere || (() => { const c = sumCounts(n); return c.work + c.plan + c.open > 0; })();
  const rootReadme = nodes.find((n) => n.kind === "doc" && /^readme\.md$/i.test(n.name));
  const has = r.folder !== null && r.tree !== null;
  // 머신 오류, origin을 받지 못해 가진 판으로 읽음, 문서가 많아 앞부분만 실음
  const warn = [r.error, r.tree?.fetch_error ? `origin을 받지 못해 가진 판: ${r.tree.fetch_error}` : null, r.tree?.truncated ? `문서 ${r.tree.docs.length}개까지만` : null,
    r.tree && !counted ? "표지 수를 세지 못함" : null]
    .filter((w): w is string => !!w).join(" · ");
  return (
    <li className="design-repo" role="treeitem" aria-expanded={has ? !collapsed : undefined} data-testid="design-repo" data-repo={r.repo}>
      <div className={`design-row is-repo${has ? "" : " is-empty"}`} role={has ? "button" : undefined} tabIndex={has ? 0 : undefined}
        title={[r.repo, r.folder ? `${r.folder}/` : null, r.machine || null, warn || null].filter(Boolean).join(" · ")}
        onClick={has ? onToggle : undefined} onKeyDown={has ? (e) => onRowKey(e, onToggle) : undefined}>
        <span className="design-mark" aria-hidden="true">{has ? (collapsed ? <ChevronRight /> : <ChevronDown />) : null}</span>
        <span className="design-name"><strong>{r.repo}</strong>{r.folder && <span className="design-file">{r.folder}/</span>}</span>
        {r.folder === null && <span className="design-note">기획 폴더 없음</span>}
        <Counts n={total} />
        {r.branches.length > 0 && <span className="design-count" title={r.branches.map((b) => `design/${b.slug}`).join(" · ")} data-testid="design-repo-branches"><GitBranch aria-hidden="true" />{r.branches.length}</span>}
        {warn && <span className="design-warn" data-testid="design-repo-error"><TriangleAlert aria-label={warn} /></span>}
      </div>
      {r.error && !r.tree && <p className="design-error" role="alert">{r.error}</p>}
      {has && !collapsed && (
        <ul role="group">
          {nodes.length === 0 && <li className="design-note design-none">문서 없음</li>}
          {rootReadme && keep(rootReadme) && <NodeRow node={rootReadme} depth={1} repo={r.repo} todo={todoHere} marks={marks} fresh={fresh} openDirs={openDirs} onToggleDir={onToggleDir} selected={selected} onOpen={onOpen} />}
          {nodes.filter((n) => n !== rootReadme && keep(n)).map((n) => (
            <NodeRow key={n.kind === "dir" ? n.path : n.doc.path} node={n} depth={1} repo={r.repo} todo={todoHere} marks={marks} fresh={fresh} openDirs={openDirs} onToggleDir={onToggleDir} selected={selected} onOpen={onOpen} />
          ))}
          {todo && !counted && <li className="design-note" data-testid="design-uncounted">표지 수를 세지 못해 모두 보인다</li>}
          {todoHere && plain > 0 && (
            <li className="design-plain" data-testid="design-plain">
              <span>표지 없는 문서 {plain}</span><span className="spacer" />
              <button type="button" className="ghost small" onClick={onAll}>모두에서</button>
            </li>
          )}
        </ul>
      )}
    </li>
  );
}

function NodeRow({ node, depth, repo, todo, marks, fresh, openDirs, onToggleDir, selected, onOpen }: {
  node: DesignNode;
  depth: number;
  repo: string;
  todo: boolean;
  /** 문서마다 그 문서를 고친 기획 branch */
  marks: Map<string, DesignBranchView[]>;
  /** branch에서 새로 만든 문서 */
  fresh: Set<string>;
  openDirs: Set<string>;
  onToggleDir: (path: string) => void;
  selected: string | null;
  onOpen: (target: string) => void;
}) {
  const pad = { "--design-depth": depth } as React.CSSProperties;
  if (node.kind === "doc") {
    const target = designTarget(repo, node.doc.path);
    const go = () => onOpen(target);
    const sel = selected === node.doc.path;
    const by = marks.get(node.doc.path) ?? [];
    const isNew = fresh.has(node.doc.path);
    return (
      <li role="treeitem" aria-selected={sel}>
        <div className={`design-row is-doc${sel ? " is-selected" : ""}`} style={pad} role="button" tabIndex={0} aria-current={sel ? "true" : undefined}
          title={node.doc.path} data-testid="design-doc" data-path={node.doc.path} onClick={go} onKeyDown={(e) => onRowKey(e, go)}>
          <span className={`design-mark${isNew ? " is-new" : ""}`} aria-hidden="true">{isNew ? <Plus /> : <FileText />}</span>
          <span className="design-name">{docLabel(node.doc)}{node.doc.title && <span className="design-file">{stem(node.doc.path)}</span>}</span>
          <Counts n={markCounts([node.doc])} />
          {by.length === 1 && <span className={`design-branch-chip${by[0]!.state === "rest" ? " is-dim" : ""}`} data-testid="design-doc-branch"><GitBranch aria-hidden="true" />{by[0]!.slug}</span>}
          {by.length > 1 && <span className="design-count" title={by.map((b) => b.slug).join(" · ")} data-testid="design-doc-branch"><GitBranch aria-hidden="true" />{by.length}</span>}
        </div>
      </li>
    );
  }
  const open = openDirs.has(`${repo}:${node.path}`);
  const toggle = () => onToggleDir(node.path);
  return (
    <li role="treeitem" aria-expanded={open}>
      <div className="design-row is-dir" style={pad} role="button" tabIndex={0} title={node.path} data-testid="design-dir" data-path={node.path}
        onClick={toggle} onKeyDown={(e) => onRowKey(e, toggle)}>
        <span className="design-mark" aria-hidden="true">{open ? <FolderOpen /> : <Folder />}</span>
        <span className="design-name">{node.title || node.name}<span className="design-file">{node.name}/</span></span>
        <Counts n={sumCounts(node)} />
      </div>
      {open && (
        <ul role="group">
          {node.children.filter((c) => !todo || (() => { const n = sumCounts(c); return n.work + n.plan + n.open > 0; })()).map((c) => (
            <NodeRow key={c.kind === "dir" ? c.path : c.doc.path} node={c} depth={depth + 1} repo={repo} todo={todo} marks={marks} fresh={fresh} openDirs={openDirs} onToggleDir={onToggleDir} selected={selected} onOpen={onOpen} />
          ))}
        </ul>
      )}
    </li>
  );
}
