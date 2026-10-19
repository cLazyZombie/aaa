import { useCallback, useEffect, useState } from "react";
import type { DeviceRole, FrontResponse, LibraryReport } from "./gen/proto";
import { call } from "./api";
import { ReportLink } from "./LibraryLink";
import { formatBytes } from "./formatBytes";

function storageSize(bytes: number): string {
  return bytes >= 1024 ** 3 ? `${Math.round(bytes / 1024 ** 3 * 10) / 10} GB` : formatBytes(bytes);
}

/**
 * 보고서를 새 창으로 연다. 사용자 동작에서 먼저 창을 연다(휴대폰 popup 차단). 만료·폐기돼도 pairing 권한으로 열린다.
 * 창이 막히면 던진다.
 */
export async function openReportWindow(r: { project: string; name: string }): Promise<void> {
  const windowRef = window.open("about:blank", "_blank");
  if (windowRef) windowRef.opener = null;
  try {
    const v = await call({ op: "library_open", project: r.project, name: r.name });
    if (v.kind === "library" && windowRef) windowRef.location.href = v.base;
    else {
      windowRef?.close();
      throw new Error("보고서를 열 창이 차단됐다. 브라우저의 팝업을 허용한다");
    }
  } catch (e) {
    windowRef?.close();
    throw e;
  }
}

/** 목록·크기·링크 상태는 host 정본이다. 파일 경로나 등록 원본 경로를 웹에 싣지 않는다. 「열기」는 본문에서(`onView`), 「새 창」은 따로 */
export function ReportsPanel({ role, onView }: { role: DeviceRole; onView: (r: LibraryReport) => void }) {
  const [reports, setReports] = useState<LibraryReport[] | null>(null);
  const [usage, setUsage] = useState<Extract<FrontResponse, { kind: "library_usage" }> | null>(null);
  const [problem, setProblem] = useState("");
  const [project, setProject] = useState("");
  const [opening, setOpening] = useState<string | null>(null);
  const load = useCallback(async () => {
    setProblem("");
    try {
      const [r, u] = await Promise.all([
        call({ op: "library_reports", project: null }), call({ op: "library_usage" }),
      ]);
      if (u.kind === "library_usage") setUsage(u);
      if (r.kind === "library_reports") setReports(r.reports);
    } catch (e) { setProblem(String(e)); }
  }, []);
  useEffect(() => { void load(); }, [load]);
  const open = async (r: LibraryReport) => {
    setOpening(`${r.project}/${r.name}`);
    try { await openReportWindow(r); }
    catch (e) { setProblem(e instanceof Error ? e.message : String(e)); }
    finally { setOpening(null); }
  };
  return <div className="reports-panel" data-testid="reports-panel" data-status={reports === null ? "loading" : "ready"}>
    <div className="reports-toolbar">
      <label>프로젝트 <select aria-label="보고서 프로젝트" value={project} onChange={(e) => setProject(e.target.value)}>
        <option value="">전체</option>
        {[...new Set((reports ?? []).map((r) => r.project))].sort().map((p) => <option key={p}>{p}</option>)}
      </select></label>
      <button className="ghost small" onClick={() => void load()}>새로고침</button>
    </div>
    {usage && <p className="muted" data-testid="reports-usage">합계 {storageSize(usage.size)} / {usage.max_size ? storageSize(usage.max_size) : "상한 꺼짐"}, 기한 {usage.max_days ? `${usage.max_days}일` : "꺼짐"}</p>}
    {usage && usage.max_size > 0 && usage.protected_size > usage.max_size && <p role="alert">보호 사본만으로 상한을 넘었다. 자동 삭제하지 않는다</p>}
    {problem && <p role="alert">{problem}</p>}
    {reports === null ? <p>보고서를 읽고 있다…</p> : reports.length === 0 ? <p>보고서가 없다</p> : reports.filter((r) => !project || r.project === project).map((r) => {
      const key = `${r.project}/${r.name}`;
      return <article className="report-card" key={key} data-testid={`report-${r.project}-${r.name}`}>
        <div className="report-heading"><strong>{key}</strong><span>{formatBytes(r.size)}{r.live ? " · 실시간" : ""}</span></div>
        <p className="muted">갱신 {new Date(r.updated_at_ms).toLocaleString("ko-KR")}</p>
        {r.problem && <p role="alert">{r.problem}</p>}
        <div className="report-actions">
          <button className="small" data-testid="report-open" onClick={() => onView(r)}>열기</button>
          <button className="ghost small" disabled={opening !== null} onClick={() => void open(r)}>새 창</button>
        </div>
        {role !== "reader" && <ReportLink project={r.project} name={r.name} link={r.link} onChange={(link) => setReports((rows) => rows?.map((row) => row.project === r.project && row.name === r.name ? { ...row, link } : row) ?? null)} />}
      </article>;
    })}
  </div>;
}
