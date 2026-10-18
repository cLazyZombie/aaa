import { useEffect, useState } from "react";
import type { DeviceRole, LibraryView, ThreadView } from "./gen/proto";
import { ApiError, call } from "./api";
import { Icon } from "./icons";
import { Markdown } from "./Markdown";
import { OpenFileContext } from "./herdr/filePaths.ts";
import { DrawerToggle, HeadMore, HeadNote } from "./drawer";
import { LIBRARY_SANDBOX } from "./FileViewer";

export const GROUP_LABEL: Record<string, string> = {
  waiting_on_you: "사람 차례",
  ready_for_review: "검토 대기",
  landing: "병합 중",
  working: "작업 중",
  idle: "쉼",
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
}: {
  t: ThreadView;
  role: DeviceRole;
  onBack: () => void;
  /** 돌아갈 화면 이름(프로젝트 thread 보기에서 열었으면): 그때 「뒤로」는 넓은 화면에도 보인다 */
  backTo?: string;
  /** 그 thread의 agent pane이 돌고 있으면 터미널로(목록의 thread 줄과 같은 곳) */
  onOpenPane?: () => void;
  /** 결과물 파일을 앱 안 파일 보기로(차이 목록 R1, 결과물 안 상대 경로) */
  onOpenFile: (path: string) => void;
  /** 보고서 안 파일 경로·링크를 그 thread의 머신·작업 폴더 기준으로 연다(plan §8.170). 보기 권한 기기는 없다(열 수 없다) */
  onOpenReportFile?: (path: string) => void;
}) {
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
  }, [t.project, t.id, t.report_hash]);

  const ack = async () => {
    setMsg(null);
    try {
      await call({ op: "thread_ack", project: t.project, thread: t.id });
    } catch (e) {
      setMsg(e instanceof ApiError ? e.message : String(e));
    }
  };
  const index = lib?.files.find((f) => f.path === "index.html" && !f.on_agent);
  return (
    <div className="thread-view" data-testid="thread-view">
      <header className="term-head">
        <DrawerToggle />
        <HeadNote />
        <button className={`back ghost icon${backTo ? " is-shown" : ""}`} onClick={onBack} aria-label={backTo ? `${backTo} 프로젝트로` : "목록으로"} data-testid="thread-back">
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
          {GROUP_LABEL[t.group] ?? t.group}
        </span>
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
        <HeadMore />
      </header>
      {msg && <div className="banner err">{msg}</div>}
      <div className="thread-body">
        <div className="thread-col">
          <div className="t-hero">
            <h1>{t.title}</h1>
            <div className="t-meta">
              <span className={`chip meta-chip ${t.group}`}>{GROUP_LABEL[t.group] ?? t.group}</span>
              <span className="mono">
                {t.project}/{t.id}
              </span>
              <span>
                {t.profile}@{t.machine}
              </span>
              {t.branch && <span className="mono">{t.branch}</span>}
              {t.agent?.percent != null && <span>{t.agent.percent}%</span>}
            </div>
          </div>
          <section className="card report" data-testid="report">
            <h2>보고</h2>
            {lib?.report ? (
              <OpenFileContext.Provider value={onOpenReportFile ?? null}>
                <Markdown className="report-markdown">{lib.report}</Markdown>
              </OpenFileContext.Provider>
            ) : (
              <p className="muted">아직 회수한 보고가 없다</p>
            )}
          </section>
          {lib && lib.files.length > 0 && (
            <section className="card files" data-testid="files">
              <h2>
                결과물 <span className="muted">{lib.files.length}</span>
              </h2>
              {index && (
                <iframe
                  className="library-frame"
                  data-testid="library-frame"
                  title="결과물 index.html"
                  sandbox={LIBRARY_SANDBOX}
                  src={lib.base + "index.html"}
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
                    {f.on_agent && <span className="tag">agent에서 읽음</span>}
                    <span className="size">{fmtSize(f.size)}</span>
                  </li>
                ))}
              </ul>
            </section>
          )}
        </div>
      </div>
    </div>
  );
}
