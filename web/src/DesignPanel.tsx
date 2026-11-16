// 기획 트리(t-1894, 확정 계획 plan §2): 활동 막대 「기획」의 사이드바 칸. 프로젝트 저장소 → 폴더 → 문서이고 폴더가 먼저 온다.
// 기획 폴더를 정하지 않은 저장소도 한 줄로 보인다. 1단계는 읽기 전용이다(origin 기본 branch).
import { ChevronDown, ChevronRight, FileText, Folder, FolderOpen, GitBranch, Plus, RefreshCw, TriangleAlert } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { ApiError, call } from "./api";
import { addedDocs, ancestors, branchesByPath, designTarget, designTree, docLabel, splitTarget, stem, type DesignNode } from "./designModel";
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
  return (
    <section className="design-panel" aria-label="기획 트리" data-testid="design-panel">
      <button type="button" className="sidebar-row-action design-reload" disabled={design.loading} data-testid="design-reload" onClick={design.reload}>
        <span className="sidebar-mark" aria-hidden="true"><RefreshCw className={design.loading ? "is-spinning" : undefined} /></span>새로 읽기
      </button>
      {design.error && <p className="banner err" role="alert">{design.error}</p>}
      {design.repos === null && !design.error && <p className="tree-state" role="status">불러오는 중</p>}
      {design.repos?.length === 0 && <p className="tree-state" data-testid="design-empty">저장소 없음</p>}
      <ul className="design-tree" role="tree">
        {repos.map((r) => (
          <RepoRow key={r.repo} repo={r} collapsed={shut.has(r.repo)} onToggle={() => flip(setShut, r.repo)}
            openDirs={open} onToggleDir={(p) => flip(setOpen, `${r.repo}:${p}`)} selected={at?.repo === r.repo ? at.path : null} onOpen={onOpen} />
        ))}
      </ul>
    </section>
  );
}

function RepoRow({ repo: r, collapsed, onToggle, openDirs, onToggleDir, selected, onOpen }: {
  repo: DesignRepoView;
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
    ? designTree(r.folder, [...r.tree.docs, ...added.map((a) => ({ path: a.path, title: "", bytes: 0 }))])
    : []), [r.folder, r.tree, added]);
  const fresh = useMemo(() => new Set(added.map((a) => a.path)), [added]);
  const rootReadme = nodes.find((n) => n.kind === "doc" && /^readme\.md$/i.test(n.name));
  const has = r.folder !== null && r.tree !== null;
  // 머신 오류, origin을 받지 못해 가진 판으로 읽음, 문서가 많아 앞부분만 실음
  const warn = [r.error, r.tree?.fetch_error ? `origin을 받지 못해 가진 판: ${r.tree.fetch_error}` : null, r.tree?.truncated ? `문서 ${r.tree.docs.length}개까지만` : null]
    .filter((w): w is string => !!w).join(" · ");
  return (
    <li className="design-repo" role="treeitem" aria-expanded={has ? !collapsed : undefined} data-testid="design-repo" data-repo={r.repo}>
      <div className={`design-row is-repo${has ? "" : " is-empty"}`} role={has ? "button" : undefined} tabIndex={has ? 0 : undefined}
        title={[r.repo, r.folder ? `${r.folder}/` : null, r.machine || null, warn || null].filter(Boolean).join(" · ")}
        onClick={has ? onToggle : undefined} onKeyDown={has ? (e) => onRowKey(e, onToggle) : undefined}>
        <span className="design-mark" aria-hidden="true">{has ? (collapsed ? <ChevronRight /> : <ChevronDown />) : null}</span>
        <span className="design-name"><strong>{r.repo}</strong>{r.folder && <span className="design-file">{r.folder}/</span>}</span>
        {r.folder === null && <span className="design-note">기획 폴더 없음</span>}
        {r.branches.length > 0 && <span className="design-count" title={r.branches.map((b) => `design/${b.slug}`).join(" · ")} data-testid="design-repo-branches"><GitBranch aria-hidden="true" />{r.branches.length}</span>}
        {warn && <span className="design-warn" data-testid="design-repo-error"><TriangleAlert aria-label={warn} /></span>}
      </div>
      {r.error && !r.tree && <p className="design-error" role="alert">{r.error}</p>}
      {has && !collapsed && (
        <ul role="group">
          {nodes.length === 0 && <li className="design-note design-none">문서 없음</li>}
          {rootReadme && <NodeRow node={rootReadme} depth={1} repo={r.repo} marks={marks} fresh={fresh} openDirs={openDirs} onToggleDir={onToggleDir} selected={selected} onOpen={onOpen} />}
          {nodes.filter((n) => n !== rootReadme).map((n) => (
            <NodeRow key={n.kind === "dir" ? n.path : n.doc.path} node={n} depth={1} repo={r.repo} marks={marks} fresh={fresh} openDirs={openDirs} onToggleDir={onToggleDir} selected={selected} onOpen={onOpen} />
          ))}
        </ul>
      )}
    </li>
  );
}

function NodeRow({ node, depth, repo, marks, fresh, openDirs, onToggleDir, selected, onOpen }: {
  node: DesignNode;
  depth: number;
  repo: string;
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
      </div>
      {open && (
        <ul role="group">
          {node.children.map((c) => (
            <NodeRow key={c.kind === "dir" ? c.path : c.doc.path} node={c} depth={depth + 1} repo={repo} marks={marks} fresh={fresh} openDirs={openDirs} onToggleDir={onToggleDir} selected={selected} onOpen={onOpen} />
          ))}
        </ul>
      )}
    </li>
  );
}
