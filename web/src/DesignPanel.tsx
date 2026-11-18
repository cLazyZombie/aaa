// 기획 트리(t-1894, 확정 계획 plan §2): 활동 막대 「기획」의 사이드바 칸. 프로젝트 저장소 → 폴더 → 문서이고 폴더가 먼저 온다.
// 기획 폴더를 정하지 않은 저장소도 한 줄로 보인다. 트리는 origin 기본 branch(main) 하나다(t-2002).
import { ChevronDown, ChevronRight, FileText, Folder, FolderGit2, FolderOpen, Layers, RefreshCw, TriangleAlert } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { ApiError, call } from "./api";
import { loadDesignRepos } from "./designLoad";
import { designTarget, designTree, docLabel, docsUnder, markCounts, parseRepoFilter, pickRepo, REPO_FILTER_KEY, shownRepos, splitTarget, stem, type DesignNode } from "./designModel";
import { useDesignTreeScroll, useDesignTreeState, type DesignTreeState } from "./designTreeState";
import type { DesignIdView, DesignRepoView } from "./gen/proto";
import { RowMenu } from "./RowMenu";
import { onRowKey } from "./Sidebar";

export type Design = {
  repos: DesignRepoView[] | null; error: string | null; loading: boolean; reload: () => void;
  /** 트리 저장소 필터(t-1946): 고른 저장소 이름, null이면 전부. 사이드바·drawer·문서 화면 「트리」 칸이 함께 쓴다 */
  picked: string[] | null; setPicked: (v: string[] | null) => void;
  treeState: DesignTreeState;
};

/** 고른 저장소는 이 브라우저에만 남긴다(서버에 싣지 않는다). 저장소를 못 쓰면 이번 화면에서만 */
function usePickedRepos(): [string[] | null, (v: string[] | null) => void] {
  const [picked, setState] = useState<string[] | null>(() => { try { return parseRepoFilter(localStorage.getItem(REPO_FILTER_KEY)); } catch { return null; } });
  const set = useCallback((v: string[] | null) => {
    setState(v);
    try { if (v) localStorage.setItem(REPO_FILTER_KEY, JSON.stringify(v)); else localStorage.removeItem(REPO_FILTER_KEY); } catch { /* 이 기기만의 편의 */ }
  }, []);
  return [picked, set];
}

/** host 기획 트리. 「기획」 칸을 처음 열 때와 「새로 읽기」에서 읽는다(origin은 agent가 1분에 한 번 받는다) */
export function useDesign(up: boolean, wanted: boolean, target: string | null): Design {
  const [repos, setRepos] = useState<DesignRepoView[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [rev, setRev] = useState(0);
  const reload = useCallback(() => setRev((n) => n + 1), []);
  const [asked, setAsked] = useState(false);
  const [picked, setPicked] = usePickedRepos();
  const treeState = useDesignTreeState(target);
  useEffect(() => { if (wanted) setAsked(true); }, [wanted]);
  useEffect(() => {
    if (!up || !asked) return;
    let live = true;
    setLoading(true);
    loadDesignRepos(call,
      (repos) => { setRepos(repos); setError(null); },
      (repo) => setRepos((repos) => repos?.map((r) => r.repo === repo.repo ? repo : r) ?? null),
      () => live,
    ).catch((e: unknown) => { if (live) setError(e instanceof ApiError ? e.message : String(e)); })
      .finally(() => { if (live) setLoading(false); });
    return () => { live = false; };
  }, [up, asked, rev]);
  return { repos, error, loading, reload, picked, setPicked, treeState };
}

/**
 * 기획서 ID(`k-NNNN`, t-1957): 지금 main 트리에 있는 문서의 ID. 대화·보고 글의 `k-NNNN`은 이 목록에 있을 때만 링크다. host 원장에서
 * 읽고(agent를 거치지 않는다), 기획 트리를 새로 읽을 때마다(원장이 맞춰진다) 다시 읽는다
 */
export function useDesignIds(up: boolean, repos: DesignRepoView[] | null): Map<string, DesignIdView> {
  const [ids, setIds] = useState<Map<string, DesignIdView>>(() => new Map());
  useEffect(() => {
    if (!up) return;
    let live = true;
    call({ op: "design_ids" }).then(
      (r) => { if (live && r.kind === "design_ids") setIds(new Map(r.ids.map((i) => [i.id, i]))); },
      () => { /* 옛 host: 기획서 ID 링크 없이 */ },
    );
    return () => { live = false; };
  }, [up, repos]);
  return ids;
}

/** 지운 옛 아이디어의 n-ID → 옮긴 기획 문서(`<저장소>/<경로>`, t-1959). 연결될 때 읽는다. 못 읽으면 빈 표다(n-ID는 글자로 보인다) */
export function useIdeaLinks(up: boolean): Map<string, string> | null {
  const [links, setLinks] = useState<Map<string, string> | null>(null);
  useEffect(() => {
    if (!up) return;
    let live = true;
    call({ op: "idea_links" }).then(
      (r) => { if (live && r.kind === "idea_links") setLinks(new Map(Object.entries(r.links))); },
      () => { if (live) setLinks((old) => old ?? new Map()); },
    );
    return () => { live = false; };
  }, [up]);
  return links;
}

export function DesignPanel({ design, surface, visible, selected, onOpen }: {
  design: Design;
  surface: "sidebar" | "document";
  visible: boolean;
  /** 본문에 연 문서(`<저장소>/<경로>`) */
  selected: string | null;
  onOpen: (target: string) => void;
}) {
  const at = selected ? splitTarget(selected) : null;
  const { expansion, toggle } = design.treeState;
  const scroll = useDesignTreeScroll(design.treeState, surface, visible, selected, design.loading || (design.repos === null && !design.error));
  const repos = design.repos ?? [];
  const names = useMemo(() => repos.map((r) => r.repo), [repos]);
  const shown = useMemo(() => shownRepos(names, design.picked), [names, design.picked]);
  const all = shown.size === names.length;
  const pick = (name: string | null) => design.setPicked(pickRepo(names, design.picked, name));
  // 모두 | 할 것(표지 블록이 있는 문서만). 기기에 남긴다
  const [todo, setTodoState] = useState<boolean>(() => { try { return localStorage.getItem("herdr-web-ui:design-todo") === "1"; } catch { return false; } });
  const setTodo = (v: boolean) => { setTodoState(v); try { localStorage.setItem("herdr-web-ui:design-todo", v ? "1" : "0"); } catch { /* 이 기기만의 편의 */ } };
  return (
    <section className="design-panel" aria-label="기획 트리" data-testid="design-panel" {...scroll}>
      <div className="design-panel-top">
        <button type="button" className="sidebar-row-action design-reload" disabled={design.loading} data-testid="design-reload" onClick={design.reload}>
          <span className="sidebar-mark" aria-hidden="true"><RefreshCw className={design.loading ? "is-spinning" : undefined} /></span>새로 읽기
        </button>
        <div className="design-tabs is-small" role="tablist" aria-label="문서 보기" data-testid="design-todo">
          <button type="button" role="tab" aria-selected={!todo} className={`design-tab${!todo ? " is-on" : ""}`} onClick={() => setTodo(false)}>모두</button>
          <button type="button" role="tab" aria-selected={todo} className={`design-tab${todo ? " is-on" : ""}`} data-testid="design-todo-on" onClick={() => setTodo(true)}>할 것</button>
        </div>
      </div>
      {names.length > 1 && <RepoFilter names={names} shown={shown} all={all} onPick={pick} />}
      {design.error && <p className="banner err" role="alert">{design.error}</p>}
      {design.repos === null && !design.error && <p className="tree-state" role="status">불러오는 중</p>}
      {design.repos?.length === 0 && <p className="tree-state" data-testid="design-empty">저장소 없음</p>}
      <ul className="design-tree" role="tree">
        {repos.filter((r) => shown.has(r.repo)).map((r) => (
          <RepoRow key={r.repo} repo={r} todo={todo} onAll={() => setTodo(false)} collapsed={expansion.closedRepos.has(r.repo)} onToggle={() => toggle("closedRepos", r.repo)}
            openDirs={expansion.openDirs} onToggleDir={(p) => toggle("openDirs", designTarget(r.repo, p))} selected={at?.repo === r.repo ? at.path : null} onOpen={onOpen} />
        ))}
      </ul>
    </section>
  );
}

/** 저장소 필터(t-1946): 「저장소: 전체 ▾」 단추와 체크 메뉴. 저장소가 많아도 한 줄이고, 메뉴는 고를 때마다 반영하며 열려 있다 */
function RepoFilter({ names, shown, all, onPick }: { names: string[]; shown: Set<string>; all: boolean; onPick: (name: string | null) => void }) {
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  const close = useCallback(() => setAnchor(null), []);
  const label = all ? "전체" : names.filter((n) => shown.has(n)).join(", ");
  return (
    <div className="design-filter">
      <button type="button" className={`design-filter-btn${all ? "" : " is-on"}`} aria-haspopup="menu" aria-expanded={anchor !== null} title={`저장소: ${label}`}
        data-testid="design-repo-filter" onClick={(e) => { const el = e.currentTarget; setAnchor((a) => (a ? null : el)); }}>
        <FolderGit2 aria-hidden="true" /><span className="design-filter-label">저장소: {label}</span><ChevronDown aria-hidden="true" />
      </button>
      {anchor && <RowMenu anchor={anchor} align="start" title="저장소로 거르기" closeLabel="닫기" onClose={close} items={[
        { id: "all", label: "전체", icon: Layers, checked: all, stay: true, run: () => onPick(null) },
        ...names.map((n) => ({ id: `repo:${n}`, label: n, icon: FolderGit2, checked: !all && shown.has(n), stay: true, run: () => onPick(n) })),
      ]} />}
    </div>
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
  // 트리는 main 하나다(t-2002: 기획 branch 없음)
  const nodes = useMemo(() => (r.folder && r.tree ? designTree(r.folder, r.tree.docs) : []), [r.folder, r.tree]);
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
        {warn && <span className="design-warn" data-testid="design-repo-error"><TriangleAlert aria-label={warn} /></span>}
      </div>
      {r.error && !r.tree && <p className="design-error" role="alert">{r.error}</p>}
      {has && !collapsed && (
        <ul role="group">
          {nodes.length === 0 && <li className="design-note design-none">문서 없음</li>}
          {rootReadme && keep(rootReadme) && <NodeRow node={rootReadme} depth={1} repo={r.repo} todo={todoHere} openDirs={openDirs} onToggleDir={onToggleDir} selected={selected} onOpen={onOpen} />}
          {nodes.filter((n) => n !== rootReadme && keep(n)).map((n) => (
            <NodeRow key={n.kind === "dir" ? n.path : n.doc.path} node={n} depth={1} repo={r.repo} todo={todoHere} openDirs={openDirs} onToggleDir={onToggleDir} selected={selected} onOpen={onOpen} />
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

function NodeRow({ node, depth, repo, todo, openDirs, onToggleDir, selected, onOpen }: {
  node: DesignNode;
  depth: number;
  repo: string;
  todo: boolean;
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
    return (
      <li role="treeitem" aria-selected={sel}>
        <div className={`design-row is-doc${sel ? " is-selected" : ""}`} style={pad} role="button" tabIndex={0} aria-current={sel ? "true" : undefined}
          title={node.doc.id ? `${node.doc.id} · ${node.doc.path}` : node.doc.path} data-testid="design-doc" data-path={node.doc.path} data-id={node.doc.id} onClick={go} onKeyDown={(e) => onRowKey(e, go)}>
          <span className="design-mark" aria-hidden="true"><FileText /></span>
          <span className="design-name">{docLabel(node.doc)}{node.doc.title && <span className="design-file">{stem(node.doc.path)}</span>}</span>
          <Counts n={markCounts([node.doc])} />
        </div>
      </li>
    );
  }
  const open = openDirs.has(designTarget(repo, node.path));
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
            <NodeRow key={c.kind === "dir" ? c.path : c.doc.path} node={c} depth={depth + 1} repo={repo} todo={todo} openDirs={openDirs} onToggleDir={onToggleDir} selected={selected} onOpen={onOpen} />
          ))}
        </ul>
      )}
    </li>
  );
}
