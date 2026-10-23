import { ignoredFailure, logIgnoredFailure } from "./errorDiagnostic.ts";
// thread diff 패널(plan §8.305). t3code 4b1024da `apps/web/src/components/DiffPanel.tsx`(193~1180줄)를 옮겼다.
// - 범위: 「main 대비」(branch, 기본)·「미커밋」(t3code Changes·Uncommitted). 턴 범위는 없다(t-0403).
// - 그리기: `@pierre/diffs` CodeView(파일마다 접기, 붙는 머리, 가상화), `lineDiffType: "none"`, 줄바꿈·공백 무시 토글(패널 안에서만,
//   t3code처럼 처음 값은 설정 줄바꿈·공백 무시 켬), 한 줄·나란히(나란히는 넓은 화면만). 파일은 모두 접힌 채 시작하고
//   카드에서 고른 파일은 펼쳐 데려온다(t3code `revealDiffFile`).
// - 큰 비교: agent가 전체 patch 120,000바이트를 넘으면 목록만 준다 → 파일 4개씩 읽고, 아래 자리가 600px 안에 들면 4개 더
//   (t3code `useReviewFilePatches`·`DiffFileLoadingBoundary`). 파일 하나가 잘리면 머리에 「일부」.
// - 다시 읽기: 다시 읽기 단추, 창 focus, thread 보고가 바뀔 때(t3code는 workspace 변경 신호).
// 다른 곳: 파일 이름을 누르면 aaa 파일 보기(worktree 기준)를 연다(t3code는 오른쪽 Files 탭). diff 안 찾기·줄 의견·파일 트리 곁칸은 옮기지
// 않았다(카드가 트리다). 머리 높이는 손가락 기기에서 44px(§8.300).
import { DEFAULT_VIRTUAL_FILE_METRICS, parsePatchFiles, type FileDiffMetadata } from "@pierre/diffs";
import { CodeView, type CodeViewHandle } from "@pierre/diffs/react";
import type { CodeViewDiffItem, CodeViewItem } from "@pierre/diffs";
import { ChevronRight, ChevronsDownUp, ChevronsUpDown, Columns2, Pilcrow, RefreshCw, Rows3, TextWrap, X } from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";

import { ApiError, call } from "./api";
import { DiffStat, PATH_ORDER as COLLATOR } from "./ChangedFiles";
import { hasNonZeroStat, summarizeDiffStats } from "./diffTree";
import type { DiffFile, DiffScope, DiffView } from "./gen/proto";
import { DiffWorkerPool } from "./herdr/DiffWorkerPool";
import { contentKey, SHADOW_CSS, SOURCE_THEME } from "./herdr/sourceTheme";
import { useMediaQuery } from "./herdr/useMediaQuery";
import { useTerminalToggle } from "./settings";
import { useAppearance } from "./theme";

type Load = { status: "pending" } | { status: "failed"; message: string } | { status: "ready"; view: DiffView };

/** 파일 하나 읽기 결과(큰 비교) */
type FileLoad = { status: "pending" } | { status: "failed" } | { status: "ready"; fileDiff: FileDiffMetadata | null; truncated: boolean };

const BATCH = 4;
const STYLE_KEY = "aaa:diff-style";

/** t3code `StyledDiffCodeView`의 머리·구분선 크기. 접힌 파일은 다시 재지 않으므로 그린 높이가 이 값과 같아야 한다 */
function shadowCss(header: number): string {
  return `${SHADOW_CSS}
:host {
  --diffs-light-addition-color: var(--syntax-inserted);
  --diffs-dark-addition-color: var(--syntax-inserted);
  --diffs-light-deletion-color: var(--syntax-deleted);
  --diffs-dark-deletion-color: var(--syntax-deleted);
  --diffs-light-modified-color: var(--syntax-meta);
  --diffs-dark-modified-color: var(--syntax-meta);
  --diffs-bg-separator-override: var(--bg-elevated);
  --diffs-bg-context-override: var(--bg);
  --diffs-bg-context-gutter-override: var(--bg);
  --diffs-header-font-family: var(--font-ui);
}
[data-diff-type="split"][data-overflow="wrap"] {
  --diffs-code-grid: var(--diffs-grid-number-column-width) minmax(0, 1fr);
}
[data-diffs-header] {
  position: sticky !important;
  top: 0;
  z-index: 4;
  background-color: var(--bg-elevated) !important;
  border-bottom: 1px solid var(--border) !important;
  box-sizing: border-box;
  align-items: center !important;
  font-size: var(--fs-xs) !important;
  line-height: 1 !important;
  height: ${header}px !important;
  min-height: ${header}px !important;
  padding-block: 0 !important;
  padding-inline: 4px 12px !important;
  cursor: pointer;
}
[data-title] { cursor: pointer; }
[data-title]:hover { text-decoration: underline; }
[data-additions-count], [data-deletions-count] { display: none; }
:is([data-separator="line-info"], [data-separator="line-info-basic"]) {
  height: 24px !important;
  margin-block: 0 !important;
}
`;
}

function readStyle(): "unified" | "split" {
  try {
    return window.localStorage.getItem(STYLE_KEY) === "split" ? "split" : "unified";
  } catch (ignoredError) { logIgnoredFailure(ignoredError, "DiffPanel.tsx:83");
    return "unified";
  }
}

function filePath(d: FileDiffMetadata): string {
  return d.name;
}

function parse(patch: string, key: string): FileDiffMetadata[] {
  try {
    return parsePatchFiles(patch, contentKey(key, patch)).flatMap((p) => p.files);
  } catch (ignoredError) { logIgnoredFailure(ignoredError, "DiffPanel.tsx:95");
    return [];
  }
}

/** 아직 읽지 않은 파일의 자리(t3code `useReviewFilePatches` 108~117줄) */
function placeholder(f: DiffFile, scope: string): FileDiffMetadata {
  return {
    name: f.path,
    ...(f.old_path ? { prevName: f.old_path } : {}),
    type: f.old_path ? "rename-changed" : f.status === "added" || f.status === "untracked" ? "new" : f.status === "deleted" ? "deleted" : "change",
    hunks: [],
    additionLines: [],
    deletionLines: [],
    splitLineCount: 0,
    unifiedLineCount: 0,
    isPartial: true,
    cacheKey: `${scope}:${f.path}:pending`,
  };
}

function hash(s: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}

function errorText(e: unknown): string {
  return e instanceof ApiError ? e.message : String(e);
}

export default function DiffPanel({
  project,
  thread,
  path,
  reveal,
  reportHash,
  sheet,
  onClose,
  onOpenFile,
}: {
  project: string;
  thread: string;
  /** 데려올 파일 */
  path: string | null;
  reveal: number;
  /** 보고가 바뀌면 다시 읽는다 */
  reportHash: string | null | undefined;
  /** 좁은 화면: 화면 위에 덮는 sheet(넓으면 thread 옆 칸) */
  sheet: boolean;
  onClose: () => void;
  /** 파일 자체를 파일 보기로(worktree 기준 경로) */
  onOpenFile?: (path: string) => void;
}) {
  const { resolvedTheme } = useAppearance();
  const coarse = useMediaQuery("(pointer: coarse)");
  const narrow = useMediaQuery("(max-width: 768px)");
  const wrapSetting = useTerminalToggle("wrapCode");
  const [scope, setScope] = useState<DiffScope>("branch");
  const [ignoreWhitespace, setIgnoreWhitespace] = useState(true);
  const [wrap, setWrap] = useState(wrapSetting);
  const [style, setStyle] = useState(readStyle);
  const [tick, setTick] = useState(0);
  const [load, setLoad] = useState<Load>({ status: "pending" });
  const [fileLoads, setFileLoads] = useState<Map<string, FileLoad>>(new Map());
  // 접힘은 범위·공백마다 처음(모두 접힘)부터다(t3code collapseScopeKey)
  const scopeKey = `${scope}:${ignoreWhitespace ? "w" : ""}`;
  const [opened, setOpened] = useState<{ key: string; paths: Set<string> }>({ key: scopeKey, paths: new Set() });
  const viewer = useRef<CodeViewHandle<undefined, undefined> | null>(null);
  const header = coarse ? 44 : 32;
  // 줄 높이는 앱 글꼴(shadow CSS의 --diffs-line-height)로 잰다: 라이브러리 기본 20px로 어림하면 펼친 파일 높이가 실제보다 커서
  // 내용이 화면보다 짧을 때 붙는 창이 아래로 밀린다(herdr-web-ui RepoDiff도 재서 넘긴다)
  const hostRef = useRef<HTMLDivElement>(null);
  const [lineHeight, setLineHeight] = useState<number | null>(null);
  useLayoutEffect(() => {
    const host = hostRef.current;
    if (host === null || lineHeight !== null) return;
    const measured = Number.parseFloat(getComputedStyle(host).lineHeight);
    setLineHeight(Number.isFinite(measured) && measured > 0 ? measured : DEFAULT_VIRTUAL_FILE_METRICS.lineHeight);
  });
  const shownStyle = narrow ? "unified" : style;

  useEffect(() => {
    let live = true;
    setLoad((cur) => (cur.status === "ready" && tick > 0 ? cur : { status: "pending" }));
    call({ op: "thread_diff", project, thread, query: { scope, ignore_whitespace: ignoreWhitespace, patch: true } })
      .then((r) => {
        if (!live || r.kind !== "thread_diff") return;
        setLoad({ status: "ready", view: r.diff });
        setFileLoads(new Map());
      })
      .catch((e: unknown) => live && setLoad({ status: "failed", message: errorText(e) }));
    return () => {
      live = false;
    };
  }, [project, thread, scope, ignoreWhitespace, tick, reportHash]);

  useEffect(() => {
    const onFocus = () => setTick((n) => n + 1);
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, []);

  useEffect(() => {
    if (!sheet) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [sheet, onClose]);

  const view = load.status === "ready" ? load.view : null;
  const files = view?.files ?? null;
  const lazy = view !== null && view.patch === null && files !== null && files.length > 0;
  const parsed = useMemo(() => {
    if (view?.patch == null) return [];
    return parse(view.patch, `thread-diff:${scopeKey}`).sort((a, b) => COLLATOR.compare(filePath(a), filePath(b)));
  }, [view, scopeKey]);
  const stats = useMemo(() => new Map((files ?? []).map((f) => [f.path, f])), [files]);

  // 큰 비교: 읽을 파일 차례
  const order = useMemo(() => (lazy && files ? [...files].sort((a, b) => COLLATOR.compare(a.path, b.path)) : []), [lazy, files]);
  const requestFiles = useCallback(
    (paths: string[]) => {
      const wanted = paths.filter((p) => !fileLoads.has(p));
      if (wanted.length === 0) return;
      setFileLoads((cur) => {
        const next = new Map(cur);
        for (const p of wanted) next.set(p, { status: "pending" });
        return next;
      });
      for (const p of wanted) {
        const f = stats.get(p);
        call({
          op: "thread_diff",
          project,
          thread,
          query: { scope, ignore_whitespace: ignoreWhitespace, patch: true, file: { path: p, ...(f?.old_path ? { old_path: f.old_path } : {}) } },
        })
          .then((r) => {
            if (r.kind !== "thread_diff") return;
            const fileDiff = parse(r.diff.patch ?? "", `thread-diff:${scopeKey}:${p}`)[0] ?? null;
            setFileLoads((cur) => new Map(cur).set(p, { status: "ready", fileDiff, truncated: r.diff.truncated }));
          })
          .catch(ignoredFailure("DiffPanel.tsx:243", () => setFileLoads((cur) => new Map(cur).set(p, { status: "failed" }))));
      }
    },
    [fileLoads, stats, project, thread, scope, ignoreWhitespace, scopeKey],
  );
  const settled = order.findIndex((f) => {
    const l = fileLoads.get(f.path);
    return l === undefined || l.status === "pending";
  });
  const settledCount = settled < 0 ? order.length : settled;
  const loadNext = useCallback(() => {
    requestFiles(order.slice(settledCount, settledCount + BATCH).map((f) => f.path));
  }, [order, settledCount, requestFiles]);
  useEffect(() => {
    if (lazy && fileLoads.size === 0) requestFiles(order.slice(0, BATCH).map((f) => f.path));
  }, [lazy, fileLoads.size, order, requestFiles]);

  const diffs: FileDiffMetadata[] = lazy
    ? order.flatMap((f) => {
        const l = fileLoads.get(f.path);
        if (l?.status === "ready") return [l.fileDiff ?? placeholder(f, scopeKey)];
        if (l?.status === "failed") return [placeholder(f, scopeKey)];
        return [];
      })
    : parsed;

  const openPaths = opened.key === scopeKey ? opened.paths : new Set<string>();
  const setOpen = (p: string, value: boolean) =>
    setOpened((cur) => {
      const paths = new Set(cur.key === scopeKey ? cur.paths : []);
      if (value) paths.add(p);
      else paths.delete(p);
      return { key: scopeKey, paths };
    });

  // 카드에서 고른 파일: 펼치고(큰 비교면 읽고) 데려온다
  const revealed = useRef<string | null>(null);
  useEffect(() => {
    if (path === null || view === null) return;
    const mark = `${scopeKey}:${path}:${reveal}`;
    if (revealed.current === mark) return;
    if (lazy && !fileLoads.has(path)) {
      requestFiles([path]);
      return;
    }
    if (!diffs.some((d) => filePath(d) === path)) return;
    revealed.current = mark;
    setOpen(path, true);
    requestAnimationFrame(() => viewer.current?.scrollTo({ type: "item", id: path, align: "start", behavior: "instant" }));
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 고른 파일·읽기가 바뀔 때만
  }, [path, reveal, view, scopeKey, lazy, fileLoads, diffs.length]);

  const items: CodeViewDiffItem[] = diffs.map((d) => {
    const p = filePath(d);
    const pending = d.cacheKey?.endsWith(":pending") === true;
    const collapsed = pending || !openPaths.has(p);
    return { id: p, type: "diff", fileDiff: d, collapsed, version: hash(`${d.cacheKey ?? p}:${collapsed ? 1 : 0}`) };
  });
  const allCollapsed = items.every((i) => i.collapsed);
  const toggleAll = () =>
    setOpened({ key: scopeKey, paths: allCollapsed ? new Set(items.filter((i) => !i.fileDiff.cacheKey?.endsWith(":pending")).map((i) => i.id)) : new Set() });

  const onBodyClick = (event: ReactMouseEvent) => {
    const nodes = event.nativeEvent.composedPath();
    for (const n of nodes) {
      if (n instanceof HTMLButtonElement || n instanceof HTMLAnchorElement) return;
    }
    const title = nodes.find((n): n is HTMLElement => n instanceof HTMLElement && n.hasAttribute("data-title"));
    const name = title?.textContent?.trim();
    if (name && onOpenFile && stats.get(name)?.status !== "deleted") {
      onOpenFile(name);
      return;
    }
    const head = nodes.find((n): n is HTMLElement => n instanceof HTMLElement && n.hasAttribute("data-diffs-header"));
    const headName = head?.querySelector("[data-title]")?.textContent?.trim();
    const item = items.find((i) => i.id === headName);
    if (item && !item.fileDiff.cacheKey?.endsWith(":pending")) setOpen(item.id, item.collapsed === true);
  };

  const total = files ? summarizeDiffStats(files) : null;
  const status = load.status === "pending" ? "pending" : load.status === "failed" ? "failed" : files?.length === 0 || view?.patch === "" ? "empty" : "ready";
  const css = useMemo(() => shadowCss(header), [header]);
  const toggleLabel = (on: boolean, yes: string, no: string) => (on ? yes : no);

  const body = (() => {
    if (load.status === "pending") return <DiffSkeleton />;
    if (load.status === "failed") return <p className="diff-note" role="alert">{load.message}</p>;
    if (status === "empty") return <p className="diff-note">{scope === "branch" ? "바뀐 것이 없다" : "커밋하지 않은 변경이 없다"}</p>;
    return (
      <div ref={hostRef} className="diff-scroll-host" onClickCapture={onBodyClick}>
        {lineHeight !== null && <DiffWorkerPool>
          <CodeView<undefined>
            ref={viewer}
            className="diff-code-view"
            items={items}
            options={{
              diffStyle: shownStyle,
              lineDiffType: "none",
              overflow: wrap ? "wrap" : "scroll",
              theme: SOURCE_THEME,
              themeType: resolvedTheme,
              stickyHeaders: true,
              hunkSeparators: "line-info",
              lineHoverHighlight: "disabled",
              unsafeCSS: css,
              itemMetrics: { lineHeight, diffHeaderHeight: header, hunkSeparatorHeight: 24, spacing: 0, paddingTop: 0, paddingBottom: 8 },
              layout: { paddingTop: 0, paddingBottom: 0, gap: 0 },
            }}
            renderHeaderPrefix={(item: CodeViewItem<undefined>) => {
              if (item.type !== "diff") return null;
              const pending = item.fileDiff.cacheKey?.endsWith(":pending") === true;
              const closed = item.collapsed === true;
              return (
                <button
                  type="button"
                  className="diff-file-toggle"
                  data-type={item.fileDiff.type}
                  aria-label={`${closed ? "펼치기" : "접기"} ${item.id}`}
                  aria-expanded={!closed}
                  disabled={pending}
                  onClick={(e) => {
                    e.stopPropagation();
                    setOpen(item.id, closed);
                  }}
                >
                  <ChevronRight aria-hidden="true" className={closed ? "" : "open"} />
                </button>
              );
            }}
            renderHeaderMetadata={(item: CodeViewItem<undefined>) => {
              const f = stats.get(item.id);
              if (!f || f.additions === null || f.deletions === null) return f ? <span className="diff-tag">바이너리</span> : null;
              return <DiffStat additions={f.additions} deletions={f.deletions} />;
            }}
            renderHeaderFilenameSuffix={(item: CodeViewItem<undefined>) => {
              const l = fileLoads.get(item.id);
              if (l?.status === "failed") return <span className="diff-tag warn" title="이 파일을 읽지 못했다">읽기 실패</span>;
              if (l?.status === "ready" && l.truncated) return <span className="diff-tag warn" title="너무 커서 앞부분만 보인다. 숫자는 모든 변경이다">일부</span>;
              return null;
            }}
            renderCodeViewFooter={() => (lazy && settledCount < order.length ? <LoadingBoundary load={loadNext} count={order.length - settledCount} /> : null)}
          />
        </DiffWorkerPool>}
      </div>
    );
  })();

  const panel = (
    <section
      className={`diff-panel${sheet ? " modal sheet" : " inline"}`}
      role={sheet ? "dialog" : "region"}
      aria-modal={sheet ? true : undefined}
      aria-label="변경 diff"
      data-testid="diff-panel"
      data-diff-status={status}
      data-diff-lazy={lazy ? "" : undefined}
      data-diff-loaded={lazy ? settledCount : undefined}
    >
      <header className="diff-panel-head">
        <select className="diff-scope" aria-label="비교 범위" value={scope} onChange={(e) => setScope(e.target.value as DiffScope)}>
          <option value="branch">{view && scope === "branch" ? `${view.base} 대비` : "main 대비"}</option>
          <option value="uncommitted">미커밋</option>
        </select>
        {total && hasNonZeroStat(total) && <DiffStat additions={total.additions} deletions={total.deletions} layout="inline" />}
        <span className="diff-panel-gap" />
        <button type="button" className="icon-button diff-action" aria-label="다시 읽기" title="다시 읽기" onClick={() => setTick((n) => n + 1)}>
          <RefreshCw aria-hidden="true" />
        </button>
        <button type="button" className="icon-button diff-action" aria-label={allCollapsed ? "모든 파일 펼치기" : "모든 파일 접기"} title={allCollapsed ? "모든 파일 펼치기" : "모든 파일 접기"} disabled={items.length === 0} onClick={toggleAll}>
          {allCollapsed ? <ChevronsUpDown aria-hidden="true" /> : <ChevronsDownUp aria-hidden="true" />}
        </button>
        {!narrow && (
          <button
            type="button"
            className="icon-button diff-action"
            aria-pressed={style === "split"}
            aria-label={toggleLabel(style === "split", "한 줄로 보기", "나란히 보기")}
            title={toggleLabel(style === "split", "한 줄로 보기", "나란히 보기")}
            onClick={() => {
              const next = style === "split" ? "unified" : "split";
              setStyle(next);
              try {
                window.localStorage.setItem(STYLE_KEY, next);
              } catch (ignoredError) { logIgnoredFailure(ignoredError, "DiffPanel.tsx:426");
                // 이 화면 동안만
              }
            }}
          >
            {style === "split" ? <Rows3 aria-hidden="true" /> : <Columns2 aria-hidden="true" />}
          </button>
        )}
        <button type="button" className="icon-button diff-action" aria-pressed={wrap} aria-label="긴 줄 바꿈" title={wrap ? "긴 줄 바꿈 끄기" : "긴 줄 바꿈"} onClick={() => setWrap((v) => !v)}>
          <TextWrap aria-hidden="true" />
        </button>
        <button type="button" className="icon-button diff-action" aria-pressed={ignoreWhitespace} aria-label="공백 변경 숨기기" title={ignoreWhitespace ? "공백 변경 보이기" : "공백 변경 숨기기"} onClick={() => setIgnoreWhitespace((v) => !v)}>
          <Pilcrow aria-hidden="true" />
        </button>
        <button type="button" className="icon-button diff-action diff-close" aria-label="diff 닫기" title="diff 닫기" onClick={onClose}>
          <X aria-hidden="true" />
        </button>
      </header>
      {view?.untracked_omitted && <div className="banner diff-banner">새 파일이 너무 많아 새 파일은 뺐다</div>}
      {view && view.truncated && !lazy && <div className="banner diff-banner">이 비교는 크기 상한을 넘어 일부만 보인다</div>}
      <div className="diff-panel-body">{body}</div>
    </section>
  );
  if (!sheet) return panel;
  return (
    <div className="modal-scrim diff-scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      {panel}
    </div>
  );
}

function DiffSkeleton() {
  return (
    <div className="diff-loading" role="status" aria-label="diff를 읽는 중">
      {[72, 54, 88, 40].map((w, i) => (
        <div key={i} className="diff-loading-row" style={{ width: `${w}%` }} aria-hidden="true" />
      ))}
    </div>
  );
}

/** 아래 자리가 600px 안에 들면 다음 파일들을 읽는다(t3code `DiffFileLoadingBoundary`) */
function LoadingBoundary({ load, count }: { load: () => void; count: number }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const io = new IntersectionObserver((entries) => {
      if (entries.some((e) => e.isIntersecting)) load();
    }, { rootMargin: "600px" });
    io.observe(el);
    return () => io.disconnect();
  }, [load]);
  return (
    <div ref={ref} className="diff-loading" role="status" aria-label="diff를 더 읽는 중" data-testid="diff-more">
      {Array.from({ length: Math.min(count, BATCH) }, (_, i) => (
        <div key={i} className="diff-loading-row head" aria-hidden="true" />
      ))}
    </div>
  );
}
