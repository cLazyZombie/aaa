// 정의·승인·기록의 정본은 host. 이 패널은 목록·enabled 편집·자동 시작 범위 승인을 제공한다.
import { useCallback, useEffect, useRef, useState } from "react";
import { call } from "./api";
import { ConfirmDialog } from "./ConfirmDialog";
import type { RoutineRun, RoutineView } from "./gen/proto";
import "./AutomationPanel.css";

const statusWord: Record<string, string> = { done: "처리됨", skipped: "건너뜀", waiting: "대기", failed: "실패" };
const time = (at: number) => new Date(at).toLocaleString();
function Result({ run }: { run: RoutineRun }) {
  return <span>{time(run.at_ms)} · {statusWord[run.status] ?? run.status}: {run.detail}{run.thread && ` · ${run.thread}`}</span>;
}
export function AutomationPanel({ canControl }: { canControl: boolean }) {
  const [rows, setRows] = useState<Array<{ project: string; rules: RoutineView[] }>>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [approval,setApproval]=useState<{project:string;rule:RoutineView}|null>(null);
  const [loading, setLoading] = useState(true);
  const generation = useRef(0);
  const load = useCallback(async () => {
    const ticket = ++generation.current;
    try {
      const projects = await call({ op: "projects" });
      if (projects.kind !== "projects") throw new Error("프로젝트 목록 응답이 아니다");
      const next = await Promise.all(projects.projects.map(async (p) => {
        const result = await call({ op: "routines", project: p.name });
        if (result.kind !== "routines") throw new Error("자동화 목록 응답이 아니다");
        return { project: p.name, rules: result.routines };
      }));
      if (ticket === generation.current) { setRows(next); setError(null); }
    } catch (e) {
      if (ticket === generation.current) setError(String(e));
    } finally {
      if (ticket === generation.current) setLoading(false);
    }
  }, []);
  useEffect(() => {
    void load();
    const refresh = () => { if (!document.hidden) void load(); };
    const timer = window.setInterval(refresh, 30_000);
    window.addEventListener("focus", refresh);
    document.addEventListener("visibilitychange", refresh);
    return () => { ++generation.current; clearInterval(timer); window.removeEventListener("focus", refresh); document.removeEventListener("visibilitychange", refresh); };
  }, [load]);
  const toggle = async (project: string, rule: RoutineView) => {
    setBusy(`${project}/${rule.name}`);
    try {
      await call({ op: "routine_toggle", project, name: rule.name, on: !rule.enabled });
      await load();
    } catch (e) { setError(String(e)); }
    finally { setBusy(null); }
  };
  return <section className="automations" aria-label="자동화 목록" data-testid="automations">
    <div className="automation-tools"><span>조건 → 동작</span><button type="button" onClick={() => void load()}>다시 읽기</button></div>
    {error && <p role="alert">{error}</p>}
    {loading && <p role="status">읽는 중…</p>}
    {!loading && rows.every((p) => p.rules.length === 0) && <p>자동화가 없습니다.</p>}
    {rows.filter((p) => p.rules.length > 0).map(({ project, rules }) => <section key={project} aria-label={`${project} 자동화`}>
      <h3>{project} · {rules.length}개</h3>
      {rules.map((r) => <article className="automation-row" key={r.name} data-testid={`automation-${project}-${r.name}`} data-status={!r.enabled ? "off" : r.error ? "failed" : r.last_result?.status ?? "ready"}>
        <div className="automation-heading"><span className="automation-indicator" role="img" aria-label={!r.enabled ? "꺼짐" : r.error ? "실패" : r.last_result ? statusWord[r.last_result.status] ?? r.last_result.status : "기록 없음"} /><strong>{r.title || r.name.replace(/^mission:/, "")}</strong>{r.origin === "mission" && <span>임무 파일에서</span>}
          <button type="button" role="switch" aria-checked={r.enabled} aria-label={`${project}/${r.name} 켜기`} disabled={!canControl || busy !== null || r.error != null} onClick={() => void toggle(project, r)}>{r.enabled ? "켬" : "끔"}</button>
        </div>
        <p className="automation-summary">{r.trigger || r.schedule} → {r.action || "동작 정보 없음"}</p>
        {r.origin === "mission" && <p>임무 파일의 켜짐 상태입니다. 적용 사본이 다르면 특수 패널에서 다시 읽기를 확인하세요.</p>}
        {r.error ? <p role="alert">{r.error}</p> : <>
          {!r.enabled && <p>꺼짐</p>}
          {r.origin === "mission" && !r.approved && <p className="automation-approval">임무 승인 필요</p>}
          {r.start_approved === false && <p className="automation-approval">자동 시작 승인 필요 {canControl && <button type="button" disabled={busy !== null} onClick={() => setApproval({project,rule:r})}>승인 검토</button>}</p>}
          {r.command && (!r.approved || !r.commands_allowed) && <p>{!r.approved ? "승인 필요" : "routine_commands 꺼짐"}</p>}
          <p>지난 {r.last_result ? <Result run={r.last_result} /> : "기록 없음"}</p>
          <p>다음: {r.enabled ? r.next_run_ms === null ? "사건 오면" : time(r.next_run_ms) : "꺼짐"} · 오늘 {r.today ?? "—"}{`/${r.max_per_day || "무제한"}`}</p>
          {r.last_skip && <p className="automation-skip">마지막 건너뜀: {r.last_skip.detail}</p>}
        </>}
        <details><summary>최근 기록 ({Math.min((r.recent ?? []).length, 20)})</summary>
          <p className="automation-file">{r.file || `routines/${r.name}.md`}</p>
          <ol>{(r.recent ?? []).slice(0, 20).map((run) => <li key={run.id}><Result run={run} /></li>)}</ol>
        </details>
      </article>)}
    </section>)}
    {approval && <ConfirmDialog title={`${approval.rule.title || approval.rule.name} 자동 시작 범위`} body={`${approval.rule.start_scope || ""}\n\n처음·넓힌 조건과 대상에만 승인합니다. 작업 본문과 선택 예산은 승인 범위에 포함하지 않습니다.`} confirmLabel="승인" onClose={() => setApproval(null)} onConfirm={async () => {
      if (!approval.rule.start_approval_sha256) throw new Error("승인할 범위 hash가 없습니다.");
      await call({op:"automation_approve",project:approval.project,name:approval.rule.name,approval_sha256:approval.rule.start_approval_sha256});
      setApproval(null);await load();
    }} />}
  </section>;
}
