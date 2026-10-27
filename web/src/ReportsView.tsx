// 본문(오른쪽) 보고서 화면(사용자 요청 10-19 「보고서 누르면 오른쪽 창이 보고서로 바뀌게」, plan §8.240). 예전에는 설정 안의 쪽이었다.
// 활동 막대 「보고서」·`#reports`가 목록을, `#report=<프로젝트>/<이름>`이 보고서 하나를 연다. 한 번 연 뒤에는 다른 화면으로 가도
// 그려 둔 채 숨긴다(App): 돌아오면 열어 둔 보고서·굴린 자리가 그대로다. 보고서 HTML은 thread 결과물처럼 sandbox iframe(opaque origin)이다.
// 보고서 안 그림을 누르면 앱 크게 보기로 연다(plan §8.353: host가 붙인 알림을 그 iframe에서만 받는다).
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { ExternalLink, ListPlus, RefreshCw } from "lucide-react";
import type { DeviceRole, LibraryFile, LibraryView } from "./gen/proto";
import { ApiError, call } from "./api";
import { Icon } from "./icons";
import { Markdown, MarkdownImageContext, MarkdownAttachmentContext } from "./Markdown";
import { DrawerToggle, HeadMore, HeadNote } from "./drawer";
import { LIBRARY_SANDBOX } from "./FileViewer";
import { useFrameImages, useLightbox, type LightboxState } from "./Lightbox";
import { markdownImages, reportMarkdownImage } from "./reportImages";
import { ReportThread, ReportsPanel, openReportWindow } from "./ReportsPanel";
import { formatBytes } from "./formatBytes";
import { reportDocLink } from "./tasksModel";
import { diagnosticReason } from "./errorDiagnostic";

export type ReportRef = { project: string; name: string };

const encodePath = (p: string) => p.split("/").map(encodeURIComponent).join("/");

/** task에 링크할 수 있는 결과물: host 사본이 있고(agent에만 있는 것은 thread로 넘길 사본이 없다) 첨부 줄로 쓸 수 있는 md·html */
export const isDoc = (name: string, f: LibraryFile) => !f.on_agent && reportDocLink(name, f.path) !== null;

/** 보고 글(report.md): `library/…` 그림을 그 보고서 보기 경로(`base`)로 그리고, 누르면 크게 본다(같은 글의 다른 그림으로 넘긴다) */
export function ReportMarkdown({ base, text, onImages }: { base: string | null; text: string; onImages: (s: LightboxState) => void }) {
  const attachment = useCallback((src: string) => base === null ? null : reportMarkdownImage(base, src), [base]);
  const resolve = useCallback(async (src: string) => {
    const url = base === null ? null : reportMarkdownImage(base, src);
    return url === null ? null : ({ kind: "inline", url } as const);
  }, [base]);
  return (
    <MarkdownAttachmentContext.Provider value={attachment}><MarkdownImageContext.Provider value={resolve}>
      <div
        onClick={(e) => {
          const got = markdownImages(e.currentTarget, e.target);
          if (!got) return;
          e.preventDefault();
          onImages(got);
        }}
      >
        <Markdown className="report-markdown">{text}</Markdown>
      </div>
    </MarkdownImageContext.Provider></MarkdownAttachmentContext.Provider>
  );
}

/** 결과물 파일 줄의 「할 일에」 단추(보고서 화면·thread 결과물) */
export function TaskAddButton({ path, onAdd }: { path: string; onAdd: (path: string) => void }) {
  return (
    <button type="button" className="ghost small file-task-add" data-testid="file-task-add" data-path={path} title={`${path}를 할 일에 추가`} aria-label={`${path} 할 일에 추가`} onClick={() => onAdd(path)}>
      <ListPlus className="i" aria-hidden="true" />
      <span className="label">할 일에</span>
    </button>
  );
}

export function ReportsView({
  shown,
  open,
  role,
  rev,
  onOpen,
  onAddDoc,
}: {
  shown: boolean;
  /** 연 보고서(없으면 목록) */
  open: ReportRef | null;
  role: DeviceRole;
  /** host가 보고서 목록이 바뀌었다고 알린 횟수(다시 읽는다) */
  rev: number;
  /** 보고서 하나를 연다·목록으로(주소를 바꾼다) */
  onOpen: (report: ReportRef | null) => void;
  /** 「할 일에 추가」: 보고서 안 md·html 문서를 링크한 task(조작 권한만, plan §8.327) */
  onAddDoc?: (name: string, path: string) => void;
}) {
  // 목록은 보고서를 연 동안에도 그려 둔다. 숨으면 굴린 자리를 잃는 브라우저가 있어(display:none) 열 때 적고 돌아올 때 놓는다
  const listScroll = useRef<HTMLDivElement>(null);
  const listTop = useRef(0);
  useLayoutEffect(() => {
    if (open === null && listScroll.current) listScroll.current.scrollTop = listTop.current;
  }, [open]);
  const openReport = (r: ReportRef) => {
    listTop.current = listScroll.current?.scrollTop ?? 0;
    onOpen(r);
  };
  return (
    <div className={`reports-view${shown ? "" : " is-hidden"}`} inert={!shown} data-testid="reports-view" data-shown={shown ? "true" : "false"}>
      <div className="thread-view reports-list" hidden={open !== null}>
        <header className="term-head">
          <DrawerToggle />
          <HeadNote />
          <div className="term-title">
            <strong>보고서</strong>
            <span className="muted">thread 보고서</span>
          </div>
          <HeadMore />
        </header>
        <div className="thread-body" ref={listScroll} data-testid="reports-scroll">
          <div className="thread-col">
            <ReportsPanel role={role} rev={rev} onView={openReport} />
          </div>
        </div>
      </div>
      {open !== null && <ReportDetail onVersion={(name) => onOpen({ project: open.project, name })} key={`${open.project}/${open.name}`} report={open} onBack={() => onOpen(null)} onAddDoc={onAddDoc && ((path) => onAddDoc(open.name, path))} />}
    </div>
  );
}

/** 백그라운드 탭의 timer가 늦어져도 복귀 때 절대 만료 시각을 확인한다. 자동 재요청은 하지 않는다. */
function useReportExpiry(until: number | null): boolean {
  const [expiredUntil, setExpiredUntil] = useState<number | null>(null);
  useEffect(() => {
    if (until === null) return;
    let timer: ReturnType<typeof setTimeout>;
    const check = () => {
      clearTimeout(timer);
      const left = until - Date.now();
      setExpiredUntil(left <= 0 ? until : null);
      if (left > 0) timer = setTimeout(check, Math.min(left, 2_147_483_647));
    };
    check();
    window.addEventListener("focus", check);
    document.addEventListener("visibilitychange", check);
    return () => { clearTimeout(timer); window.removeEventListener("focus", check); document.removeEventListener("visibilitychange", check); };
  }, [until]);
  return until !== null && expiredUntil === until;
}

/** 보고서 하나: 머리(뒤로·이름·다시 열기·새 창)와 index.html(없으면 보고 글·파일 목록). */
function ReportDetail({ report, onBack, onAddDoc, onVersion }: { report: ReportRef; onBack: () => void; onAddDoc?: (path: string) => void; onVersion: (id: string) => void }) {
  const [lib, setLib] = useState<LibraryView | null>(null);
  const [problem, setProblem] = useState("");
  const [busy, setBusy] = useState(true);
  const [reload, setReload] = useState(0);
  const requestedName = useRef(report.name);
  const expired = useReportExpiry(lib?.expires_at_ms ?? null);
  useEffect(() => {
    let live = true;
    setBusy(true); setProblem("");
    call({ op: "library_open", project: report.project, name: requestedName.current })
      .then((v) => {
        if (live && v.kind === "library") setLib(v);
      })
      .catch((e: unknown) => live && setProblem(e instanceof ApiError ? e.message : diagnosticReason(e)))
      .finally(() => { if (live) setBusy(false); });
    return () => {
      live = false;
    };
  }, [report.project, report.name, reload]);
  const index = lib?.files.find((f) => f.path === "index.html" && !f.on_agent);
  const size = lib ? lib.files.reduce((n, f) => n + f.size, 0) : null;
  const lightbox = useLightbox();
  const frame = useRef<HTMLIFrameElement>(null);
  const page = lib && index ? lib.base + "index.html" : null;
  useFrameImages(frame, page, lightbox.open);
  return (
    <div className="thread-view report-detail" data-testid="report-detail" data-status={lib ? "ready" : problem ? "error" : "loading"}>
      <header className="term-head">
        <DrawerToggle />
        <HeadNote />
        <button className="back ghost icon is-shown" onClick={onBack} aria-label="보고서 목록으로" title="보고서 목록으로" data-testid="report-back">
          <Icon name="back" />
        </button>
        <div className="term-title">
          <span className="term-title-line">
            <ReportThread id={lib?.from_thread} />
            {lib?.report_id && <a className="report-short-id" data-testid="report-id" href={`#report=${encodeURIComponent(`${report.project}/${lib.report_id}`)}`}><code>{lib.report_id}</code></a>}
            {report.name !== lib?.report_id && <strong>{report.project}/{report.name}</strong>}
          </span>
          <span className="report-title-meta">
            {size !== null && <span className="muted">{formatBytes(size)} · 파일 {lib!.files.length}</span>}
            {(lib?.versions?.length ?? 0) > 1 && <select aria-label="보고 판" value={lib?.report_id ?? ""} onChange={(e) => onVersion(e.target.value)}>
              {lib!.versions.map((id) => <option key={id} value={id}>{id}</option>)}
            </select>}
          </span>
        </div>
        {onAddDoc && index && (
          <button className="ghost small report-task-add" data-testid="report-task-add" title="이 보고서를 링크한 task를 만들거나 있는 task에 링크한다" aria-label="할 일에 추가" onClick={() => onAddDoc("index.html")}>
            <ListPlus className="i" aria-hidden="true" />
            <span className="label">할 일에 추가</span>
          </button>
        )}
        <button className="ghost small" data-testid="report-reopen" aria-label="보고서 다시 열기" title="현재 보고 판을 새 보기 권한으로 다시 열기" disabled={busy} onClick={() => {
          // 최신 t 판이 바뀌어도 지금 선택한 불변 r 판을 유지한다.
          requestedName.current = lib?.report_id ?? report.name;
          setReload((n) => n + 1);
        }}><RefreshCw className="i" aria-hidden="true" /><span className="head-button-label">다시 열기</span></button>
        <button className="ghost small" data-testid="report-window" aria-label="새 창" title="새 창" onClick={() => void openReportWindow(report).catch((e: unknown) => setProblem(diagnosticReason(e)))}>
          <ExternalLink className="i" aria-hidden="true" />
          <span className="head-button-label">새 창</span>
        </button>
        <HeadMore />
      </header>
      {problem && <div className="banner err" role="alert">{problem}</div>}
      {expired && <div className="banner" role="status" data-testid="report-expired">보기 시간이 끝났습니다. 아직 읽지 않은 이미지나 영상은 위의 다시 열기를 눌러 확인해 주세요.</div>}
      {lib && index ? (
        <iframe ref={frame} className="report-frame" data-testid="report-frame" title={`${report.project}/${report.name} index.html`} sandbox={LIBRARY_SANDBOX} src={page ?? undefined} />
      ) : lib ? (
        <div className="thread-body">
          <div className="thread-col">
            {lib.report && (
              <section className="card report">
                <h2>보고</h2>
                <ReportMarkdown base={lib.base} text={lib.report} onImages={lightbox.open} />
              </section>
            )}
            <section className="card files">
              <h2>
                파일 <span className="muted">{lib.files.length}</span>
              </h2>
              <ul>
                {lib.files.map((f) => (
                  <li key={f.path}>
                    <Icon name="file" />
                    <a href={lib.base + encodePath(f.path)} target="_blank" rel="noopener noreferrer">
                      {f.path}
                    </a>
                    <span className="size">{formatBytes(f.size)}</span>
                    {onAddDoc && isDoc(report.name, f) && <TaskAddButton path={f.path} onAdd={onAddDoc} />}
                  </li>
                ))}
              </ul>
            </section>
          </div>
        </div>
      ) : null}
      {lightbox.node}
    </div>
  );
}
