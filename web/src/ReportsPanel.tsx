import { useCallback, useEffect, useState } from "react";
import type { DeviceRole, LibraryReport } from "./gen/proto";
import { call } from "./api";
import { ReportLink } from "./LibraryLink";
import { formatBytes } from "./formatBytes";

/** 목록·크기·링크 상태는 host 정본이다. 파일 경로나 등록 원본 경로를 웹에 싣지 않는다. */
export function ReportsPanel({ role }: { role: DeviceRole }) {
  const [reports, setReports] = useState<LibraryReport[] | null>(null);
  const [problem, setProblem] = useState("");
  const [project, setProject] = useState("");
  const [opening, setOpening] = useState<string | null>(null);
  const load = useCallback(async () => {
    setProblem("");
    try {
      const r = await call({ op: "library_reports", project: null });
      if (r.kind === "library_reports") setReports(r.reports);
    } catch (e) { setProblem(String(e)); }
  }, []);
  useEffect(() => { void load(); }, [load]);
  const open = async (r: LibraryReport) => {
    // 사용자 동작에서 먼저 창을 연다(휴대폰 popup 차단). 만료·폐기돼도 pairing 권한으로 열린다.
    const windowRef = window.open("about:blank", "_blank");
    if (windowRef) windowRef.opener = null;
    setOpening(`${r.project}/${r.name}`);
    try {
      const v = await call({ op: "library_open", project: r.project, name: r.name });
      if (v.kind === "library" && windowRef) windowRef.location.href = v.base;
      else { windowRef?.close(); setProblem("보고서를 열 창이 차단됐다. 브라우저의 팝업을 허용한다"); }
    } catch (e) { windowRef?.close(); setProblem(String(e)); }
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
    {problem && <p role="alert">{problem}</p>}
    {reports === null ? <p>보고서를 읽고 있다…</p> : reports.length === 0 ? <p>보고서가 없다</p> : reports.filter((r) => !project || r.project === project).map((r) => {
      const key = `${r.project}/${r.name}`;
      return <article className="report-card" key={key} data-testid={`report-${r.project}-${r.name}`}>
        <div className="report-heading"><strong>{key}</strong><span>{formatBytes(r.size)}{r.live ? " · 실시간" : ""}</span></div>
        <p className="muted">갱신 {new Date(r.updated_at_ms).toLocaleString("ko-KR")}</p>
        {r.problem && <p role="alert">{r.problem}</p>}
        <button className="ghost small" disabled={opening !== null} onClick={() => void open(r)}>앱에서 열기</button>
        {role !== "reader" && <ReportLink project={r.project} name={r.name} link={r.link} onChange={(link) => setReports((rows) => rows?.map((row) => row.project === r.project && row.name === r.name ? { ...row, link } : row) ?? null)} />}
      </article>;
    })}
  </div>;
}
