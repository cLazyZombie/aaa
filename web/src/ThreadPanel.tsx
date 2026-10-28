import { ThreadFollowups } from "./ThreadFollowups";
import { ignoredFailure } from "./errorDiagnostic.ts";
import { ResponsibilityBadge } from "./ResponsibilityBadge";
import { ThreadLimitBadge } from "./ThreadLimitBadge";
import { AutoMergeBadge, AutoMergeSwitch, canAutoMerge } from "./AutoMerge";
import { ThreadStageBadge } from "./ThreadStageBadge";
import { lazy, Suspense, useEffect, useRef, useState } from "react";
import type { DeviceRole, DiffView, LibraryView, ThreadView } from "./gen/proto";
import { ApiError, call, me } from "./api";
import { Icon } from "./icons";
import { OpenFileContext } from "./herdr/filePaths.ts";
import { DrawerToggle, HeadMore, HeadNote } from "./drawer";
import { LIBRARY_SANDBOX } from "./FileViewer";
import { LibraryLink } from "./LibraryLink";
import { LibraryCopyNotice } from "./LibraryCopyNotice";
import { ReportMarkdown, TaskAddButton, isDoc } from "./ReportsView";
import { useFrameImages, useLightbox } from "./Lightbox";
import { SummaryButton } from "./ThreadSummary";
import { ResponsibilityBanner, ParkBanner, ThreadActions } from "./ThreadActions";
import { ChangedFilesCard } from "./ChangedFiles";
import { useDiffPanel } from "./useDiffPanel";
import { useMediaQuery } from "./herdr/useMediaQuery";

// `@pierre/diffs`는 diff를 열 때만 받는다(파일 보기의 VirtualSource와 같다)
const DiffPanel = lazy(() => import("./DiffPanel"));

export const GROUP_LABEL: Record<string, string> = {
  waiting_on_you: "확인 필요",
  ready_for_review: "검토 대기",
  working: "작업 중",
  idle: "쉼",
  parked: "보류",
  resolved: "끝남",
};


const fmtSize = (n: number) =>
  n >= 1 << 20 ? `${(n / (1 << 20)).toFixed(1)}MB` : n >= 1024 ? `${(n / 1024).toFixed(1)}KB` : `${n}B`;

const encodePath = (p: string) => p.split("/").map(encodeURIComponent).join("/");

/** thread 보고서와 결과물. 결과물 HTML은 sandbox iframe(opaque origin)으로 보여 이 화면의 권한에 닿지 못한다.
 *  결과물 파일은 앱 안 파일 보기로 연다(차이 목록 R1, herdr-projects popup의 파일 줄 ↵): 같은 보기 경로로 읽는다. */
export function ThreadPanel({
  t,
  role,
  onBack,
  backTo,
  onOpenPane,
  onOpenFile,
  onOpenReportFile,
  onAddDoc,
  reportsRev = 0,
}: {
  t: ThreadView;
  /** host reports 사건 수: 링크 발급·폐기(다른 화면의 「낸 링크」 포함)·열람 뒤 결과물 칸을 다시 읽는다 */
  reportsRev?: number;
  role: DeviceRole;
  onBack: () => void;
  /** 돌아갈 화면 이름(모든 thread 화면에서 열었으면): 그때 「뒤로」는 넓은 화면에도 보인다 */
  backTo?: string;
  /** 그 thread의 agent pane이 돌고 있으면 터미널로(목록의 thread 줄과 같은 곳) */
  onOpenPane?: () => void;
  /** 결과물 파일을 앱 안 파일 보기로(차이 목록 R1, 결과물 안 상대 경로) */
  onOpenFile: (path: string) => void;
  /** 보고서 안 파일 경로·링크를 그 thread의 머신·작업 폴더 기준으로 연다(plan §8.171). 보기 권한 기기는 없다(열 수 없다) */
  onOpenReportFile?: (path: string) => void;
  /** 「할 일에 추가」: 결과물 md·html 문서를 링크한 task(조작 권한만, plan §8.327) */
  onAddDoc?: (path: string) => void;
}) {
  const statusLabel = t.status === "starting" ? "준비 중" : t.cleanup_pending ? "정리 중" : GROUP_LABEL[t.group] ?? t.group;
  // 자동 병합(plan §8.521): 사람이 맡긴 열린 Worker의 이어진 할 일 값. 조작 기기는 여기서 켜고 끈다
  const [frontApi, setFrontApi] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    me().then((m) => live && setFrontApi(m?.front_api ?? null), ignoredFailure("ThreadPanel.tsx:me", () => undefined));
    return () => {
      live = false;
    };
  }, []);
  const mergeSwitch = role !== "reader" && canAutoMerge(frontApi) && t.role === "worker" && !t.automation && t.status !== "resolved";
  const [lib, setLib] = useState<LibraryView | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    setMsg(null);
    call({ op: "thread_library", project: t.project, thread: t.id })
      .then((r) => {
        if (live && r.kind === "library") setLib(r);
      })
      .catch((e: unknown) => live && setMsg(e instanceof ApiError ? e.message : String(e)));
    return () => {
      live = false;
    };
  }, [t.project, t.id, t.report_hash, t.copy, reportsRev]);

  // 변경 파일 카드(plan §8.305): 소스를 보이므로 조작 기기만, git 저장소 thread만. 못 읽으면 카드를 그리지 않는다(t3code와 같다)
  const [diff, setDiff] = useState<DiffView | null>(null);
  const canDiff = role !== "reader" && !!t.repo;
  useEffect(() => {
    let live = true;
    if (!canDiff) {
      setDiff(null);
      return;
    }
    call({ op: "thread_diff", project: t.project, thread: t.id, query: { scope: "branch" } })
      .then((r) => live && r.kind === "thread_diff" && setDiff(r.diff))
      .catch(ignoredFailure("ThreadPanel.tsx:96", () => live && setDiff(null)));
    return () => {
      live = false;
    };
  }, [canDiff, t.project, t.id, t.report_hash, t.group, t.status]);
  const diffPanel = useDiffPanel(t.project, t.id);
  // t3code `rightPanelLayout.ts`: 980px 위는 thread 옆 칸, 밑은 덮는 sheet
  const sheet = useMediaQuery("(max-width: 980px)");

  const ack = async () => {
    setMsg(null);
    try {
      await call({ op: "thread_ack", project: t.project, thread: t.id });
    } catch (e) {
      setMsg(e instanceof ApiError ? e.message : String(e));
    }
  };
  const index = lib?.files.find((f) => f.path === "index.html" && !f.on_agent);
  // 보고 글·결과물 index.html 그림을 누르면 크게(plan §8.353)
  const lightbox = useLightbox();
  const frame = useRef<HTMLIFrameElement>(null);
  const page = lib && index ? lib.base + "index.html" : null;
  useFrameImages(frame, page, lightbox.open);
  return (
    <div className="thread-view" data-testid="thread-view">
      <header className="term-head">
        <DrawerToggle />
        <HeadNote />
        <button className={`back ghost icon${backTo ? " is-shown" : ""}`} onClick={onBack} aria-label={backTo ? `${backTo}로` : "목록으로"} data-testid="thread-back">
          <Icon name="back" />
        </button>
        <div className="term-title">
          <strong>
            {t.project}/{t.id}
          </strong>
          <span className="muted">
            {t.profile}@{t.machine}
          </span>
        </div>
        <span className={`chip head-chip ${t.group}`}>
          {t.group === "working" && <span className="dot working" aria-hidden />}
          {statusLabel}
        </span>
        <SummaryButton t={t} />
        {onOpenPane && (
          <button className="ghost small" data-testid="open-terminal" onClick={onOpenPane}>
            <Icon name="shell" />
            터미널
          </button>
        )}
        {role !== "reader" && t.group === "ready_for_review" && (
          <button className="primary" onClick={ack}>
            <Icon name="check" />
            확인(ack)
          </button>
        )}
        {role !== "reader" && <ThreadActions t={t} running={!!onOpenPane} />}
        <HeadMore />
      </header>
      {msg && <div className="banner err">{msg}</div>}
      <ThreadFollowups key={t.id} t={t} canAct={role !== "reader"} />
      <ResponsibilityBanner t={t} canAct={role !== "reader"} running={!!onOpenPane} />
      <ParkBanner t={t} canAct={role !== "reader"} running={!!onOpenPane} />
      {t.copy === "failed" && <div className="banner" data-testid="report-copy-failed">보고·결과물 사본을 다시 받는 중이다. 마지막으로 받은 내용과 작업 상태를 유지한다.</div>}
      <div className={`thread-split${diffPanel.target && !sheet ? " with-diff" : ""}`}>
      <div className="thread-body">
        <div className="thread-col">
          {t.copy === "partial" && <div className="banner" data-testid="report-copy-partial">보고·결과물 사본에 빠진 파일이 있다.</div>}
          {lib && <LibraryCopyNotice files={lib.files} />}

          <div className="t-hero">
            <h1><ResponsibilityBadge value={t.responsibility} /><ThreadStageBadge stage={t.stage} /><ThreadLimitBadge start={t.over_limit} />{!mergeSwitch && <AutoMergeBadge on={t.auto_merge} />}{t.title}</h1>
            {t.over_limit && <p className="muted" data-testid="thread-over-limit-origin">{t.over_limit.actor} · {new Date(t.over_limit.at_ms).toLocaleString()}{t.over_limit.user_request && ` · 사용자 지시: ${t.over_limit.user_request}`}</p>}
            <div className="t-meta">
              {t.account && <span data-testid="thread-account">계정 {t.account}</span>}
              <span className={`chip meta-chip ${t.group}`}>{statusLabel}</span>
              <span className="mono">
                {t.project}/{t.id}
              </span>
              <span>
                {t.profile}@{t.machine}
              </span>
              {t.branch && <span className="mono">{t.branch}</span>}
              {t.agent?.percent != null && <span>{t.agent.percent}%</span>}
              {mergeSwitch && <AutoMergeSwitch project={t.project} target={t.id} on={!!t.auto_merge} />}
            </div>
            {t.account_selection && <p className="muted" data-testid="thread-account-selection">{t.account_selection.reason} · 한도 그룹 {t.account_selection.group} · 예상 예약 {({ small: 5, medium: 15, large: 30 })[t.account_selection.size]}%p</p>}
          </div>
          {t.automation && <section className="card" data-testid="thread-automation-origin">
            <h2>자동화 thread</h2>
            <p>자동화: {t.automation_title ?? t.automation} ({t.automation})</p>
            <p>무엇이 띄웠는지: {t.automation_trigger ?? "기동 사건 기록 없음(이전 판에서 시작)"}</p>
          </section>}
          <section className="card report" data-testid="report">
            <h2>보고 {t.report_id && <a href={`#report=${encodeURIComponent(`${t.project}/${t.report_id}`)}`}><code>{t.report_id}</code></a>}</h2>
            {lib?.report ? (
              <OpenFileContext.Provider value={onOpenReportFile ?? null}>
                <ReportMarkdown base={lib.base} text={lib.report} onImages={lightbox.open} />
              </OpenFileContext.Provider>
            ) : (
              <p className="muted">아직 회수한 보고가 없다</p>
            )}
          </section>
          {diff?.files && diff.files.length > 0 && (
            <ChangedFilesCard key={`${t.project}/${t.id}`} project={t.project} thread={t.id} base={diff.base} files={diff.files} onOpen={(path) => diffPanel.open(path ?? null)} />
          )}
          {lib && (lib.files.length > 0 || lib.link !== null) && (
            <section className="card files" data-testid="files">
              <h2>
                결과물 <span className="muted">{lib.files.length}</span>
              </h2>
              {role !== "reader" && <LibraryLink key={`${t.project}/${t.id}`} library={lib} onChange={setLib} />}
              {index && (
                <iframe
                  ref={frame}
                  className="library-frame"
                  data-testid="library-frame"
                  title="결과물 index.html"
                  sandbox={LIBRARY_SANDBOX}
                  src={page ?? undefined}
                />
              )}
              <ul>
                {lib.files.map((f) => (
                  <li key={f.path}>
                    <Icon name="file" />
                    {/* 그냥 누르면 앱 안 파일 보기, 보조 키·가운데 단추는 브라우저대로 새 탭 */}
                    <a
                      href={lib.base + encodePath(f.path)}
                      target="_blank"
                      rel="noopener noreferrer"
                      data-testid="library-file"
                      onClick={(e) => {
                        if (e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
                        e.preventDefault();
                        onOpenFile(f.path);
                      }}
                    >
                      {f.path}
                    </a>
                    {f.on_agent && <span className="tag">사본에 없음 · agent에서 읽음</span>}
                    <span className="size">{fmtSize(f.size)}</span>
                    {onAddDoc && isDoc(t.id, f) && <TaskAddButton path={f.path} onAdd={onAddDoc} />}
                  </li>
                ))}
              </ul>
            </section>
          )}
        </div>
      </div>
      {diffPanel.target && canDiff && (
        <Suspense fallback={null}>
          <DiffPanel
            project={t.project}
            thread={t.id}
            path={diffPanel.target.path}
            reveal={diffPanel.target.reveal}
            reportHash={t.report_hash}
            sheet={sheet}
            onClose={diffPanel.close}
            onOpenFile={onOpenReportFile}
          />
        </Suspense>
      )}
      </div>
      {lightbox.node}
    </div>
  );
}
