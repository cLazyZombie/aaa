// 본문(오른쪽) 보고서 화면(사용자 요청 10-19 「보고서 누르면 오른쪽 창이 보고서로 바뀌게」, plan §8.240). 예전에는 설정 안의 쪽이었다.
// 활동 막대 「보고서」·`#reports`가 목록을, `#report=<프로젝트>/<이름>`이 보고서 하나를 연다. 한 번 연 뒤에는 다른 화면으로 가도
// 그려 둔 채 숨긴다(App): 돌아오면 열어 둔 보고서·굴린 자리가 그대로다. 보고서 HTML은 thread 결과물처럼 sandbox iframe(opaque origin)이다.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { ExternalLink, ListPlus } from "lucide-react";
import type { DeviceRole, LibraryFile, LibraryView } from "./gen/proto";
import { ApiError, call } from "./api";
import { Icon } from "./icons";
import { Markdown } from "./Markdown";
import { DrawerToggle, HeadMore, HeadNote } from "./drawer";
import { LIBRARY_SANDBOX } from "./FileViewer";
import { ReportThread, ReportsPanel, openReportWindow } from "./ReportsPanel";
import { formatBytes } from "./formatBytes";
import { reportDocLink } from "./tasksModel";

export type ReportRef = { project: string; name: string };

const encodePath = (p: string) => p.split("/").map(encodeURIComponent).join("/");

/** task에 링크할 수 있는 결과물: host 사본이 있고(agent에만 있는 것은 thread로 넘길 사본이 없다) 첨부 줄로 쓸 수 있는 md·html */
export const isDoc = (name: string, f: LibraryFile) => !f.on_agent && reportDocLink(name, f.path) !== null;

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
  /** 「할 일에 추가」: 보고서 안 md·html 문서를 링크한 task(조작 권한만, plan §8.323) */
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
            <span className="muted">보고서와 공유 링크</span>
          </div>
          <HeadMore />
        </header>
        <div className="thread-body" ref={listScroll} data-testid="reports-scroll">
          <div className="thread-col">
            <ReportsPanel role={role} rev={rev} onView={openReport} />
          </div>
        </div>
      </div>
      {open !== null && <ReportDetail key={`${open.project}/${open.name}`} report={open} onBack={() => onOpen(null)} onAddDoc={onAddDoc && ((path) => onAddDoc(open.name, path))} />}
    </div>
  );
}

/** 보고서 하나: 머리(뒤로·이름·새 창)와 index.html(없으면 보고 글·파일 목록). 공유 링크 조작은 목록 카드에 있다 */
function ReportDetail({ report, onBack, onAddDoc }: { report: ReportRef; onBack: () => void; onAddDoc?: (path: string) => void }) {
  const [lib, setLib] = useState<LibraryView | null>(null);
  const [problem, setProblem] = useState("");
  useEffect(() => {
    let live = true;
    call({ op: "library_open", project: report.project, name: report.name })
      .then((v) => {
        if (live && v.kind === "library") setLib(v);
      })
      .catch((e: unknown) => live && setProblem(e instanceof ApiError ? e.message : String(e)));
    return () => {
      live = false;
    };
  }, [report.project, report.name]);
  const index = lib?.files.find((f) => f.path === "index.html" && !f.on_agent);
  const size = lib ? lib.files.reduce((n, f) => n + f.size, 0) : null;
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
            <strong>
              {report.project}/{report.name}
            </strong>
          </span>
          {size !== null && <span className="muted">{formatBytes(size)} · 파일 {lib!.files.length}</span>}
        </div>
        {onAddDoc && index && (
          <button className="ghost small report-task-add" data-testid="report-task-add" title="이 보고서를 링크한 task를 만들거나 있는 task에 링크한다" aria-label="할 일에 추가" onClick={() => onAddDoc("index.html")}>
            <ListPlus className="i" aria-hidden="true" />
            <span className="label">할 일에 추가</span>
          </button>
        )}
        <button className="ghost small" data-testid="report-window" onClick={() => void openReportWindow(report).catch((e: unknown) => setProblem(String(e)))}>
          <ExternalLink className="i" aria-hidden="true" />
          새 창
        </button>
        <HeadMore />
      </header>
      {problem && <div className="banner err" role="alert">{problem}</div>}
      {lib && index ? (
        <iframe className="report-frame" data-testid="report-frame" title={`${report.project}/${report.name} index.html`} sandbox={LIBRARY_SANDBOX} src={lib.base + "index.html"} />
      ) : lib ? (
        <div className="thread-body">
          <div className="thread-col">
            {lib.report && (
              <section className="card report">
                <h2>보고</h2>
                <Markdown className="report-markdown">{lib.report}</Markdown>
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
    </div>
  );
}
