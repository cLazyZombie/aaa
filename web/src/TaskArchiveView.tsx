// 완료 목록은 host의 보관 원장을 검색한다. 필터는 페이지를 자르기 전에 적용한다.
import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { ArchivedTask, LibraryReport, TaskArchiveFilter, TaskHistoryRepo } from "./gen/proto";
import { call, onTasksChanged } from "./api";
import { Markdown } from "./Markdown";
import { ListId } from "./ListId";
import { TaskAttachments } from "./TaskAttachments";
import { withoutAttachments, type TaskAttachment } from "./tasksModel";
import { TaskChoiceChip, TaskFilterChips, TaskSearchInput, historyChoices } from "./TaskHistoryTools";

const REASONS = [
  { key: "pr_merged", label: "PR 병합" },
  { key: "main_merged", label: "main 병합" },
  { key: "report_done", label: "보고 완료" },
  { key: "restored", label: "되살린 항목" },
];
const PERIODS = [{ key: "7", label: "최근 7일" }, { key: "30", label: "최근 30일" }, { key: "90", label: "최근 90일" }];
const choicesOf = (values: string[]) => values.map((key) => ({ key, label: key || "정하지 않음" }));

export function TaskArchiveView({ project, canEdit, rev, toolbar, repos, extendedFilters, onThread, onOpenAttachment }: {
  project: string; canEdit: boolean; rev: string; toolbar: HTMLElement | null; repos: string[]; extendedFilters: boolean;
  onThread: (id: string) => void; onOpenAttachment?: (project: string, a: TaskAttachment) => Promise<void>;
}) {
  const [query, setQuery] = useState("");
  const [repo, setRepo] = useState<string | null>(null);
  const [filter, setFilter] = useState<TaskArchiveFilter | null>(null);
  const [profile, setProfile] = useState<string | null>(null);
  const [machine, setMachine] = useState<string | null>(null);
  const [period, setPeriod] = useState<string | null>(null);
  const [since, setSince] = useState<number | null>(null);
  const [pastRepos, setPastRepos] = useState<TaskHistoryRepo[]>([]);
  const [profiles, setProfiles] = useState<string[]>([]);
  const [machines, setMachines] = useState<string[]>([]);
  const [reports, setReports] = useState<LibraryReport[]>([]);
  const [entries, setEntries] = useState<ArchivedTask[] | null>(null);
  const [next, setNext] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const seq = useRef(0);
  const load = useCallback(async (after: string | null) => {
    const n = ++seq.current;
    setLoading(true); setError("");
    if (after === null) { setEntries(null); setNext(null); }
    try {
      const r = await call({ op: "task_archive", project, restore: null, view: {
        search: { query, repo }, filter, after, limit: 50,
        profile: extendedFilters ? profile : null, machine: extendedFilters ? machine : null,
        since_ms: extendedFilters ? since : null, until_ms: null,
      } });
      if (seq.current !== n || r.kind !== "task_archive") return;
      setEntries((old) => after === null ? r.entries : [...(old ?? []), ...r.entries]);
      setNext(r.next ?? null); setPastRepos(r.repos ?? []); setProfiles(r.profiles ?? []); setMachines(r.machines ?? []);
    } catch (e) { if (seq.current === n) setError(String(e)); }
    finally { if (seq.current === n) setLoading(false); }
  }, [project, query, repo, filter, profile, machine, since, extendedFilters]);
  const latestLoad = useRef(load);
  useEffect(() => {
    latestLoad.current = load;
    void load(null);
    const off = onTasksChanged(() => { void load(null); });
    return () => { ++seq.current; off(); };
  }, [load, rev]);
  useEffect(() => {
    let live = true;
    void call({ op: "library_reports", project }).then((r) => {
      if (live && r.kind === "library_reports") setReports(r.reports);
    }).catch(() => { /* 보관 원문과 thread 연결은 보고서 목록이 없어도 볼 수 있다. */ });
    return () => { live = false; };
  }, [project, rev]);
  const restore = async (id: string) => {
    setBusy(true); setError("");
    try {
      await call({ op: "task_archive", project, restore: id });
      await latestLoad.current(null);
    } catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  };
  return <section className="tasks-scroll task-completed" aria-label="완료한 할 일" data-testid="task-archive-list" aria-busy={loading || busy}>
    {toolbar && createPortal(<>
      <TaskSearchInput value={query} onChange={setQuery} label="완료 검색" testId="task-completed-search" />
      <TaskFilterChips label="완료 거르기" repo={<TaskChoiceChip label="저장소" value={repo} onChange={setRepo} choices={historyChoices(repos, pastRepos)} testId="task-completed-repo" />}>
        {extendedFilters && <>
          <TaskChoiceChip label="모델" value={profile} onChange={setProfile} choices={choicesOf(profiles)} testId="task-completed-profile" />
          <TaskChoiceChip label="머신" value={machine} onChange={setMachine} choices={choicesOf(machines)} testId="task-completed-machine" />
          <TaskChoiceChip label="기간" value={period} onChange={(v) => { setPeriod(v); setSince(v === null ? null : Date.now() - Number(v) * 86400000); }} choices={PERIODS} testId="task-completed-period" />
        </>}
        <TaskChoiceChip label="완료 근거" value={filter} onChange={(v) => setFilter(v as TaskArchiveFilter | null)} choices={REASONS} testId="task-completed-reason" />
      </TaskFilterChips>
    </>, toolbar)}
    {error && <p role="alert">{error}</p>}
    {entries === null && !error && <p className="tasks-empty">불러오는 중</p>}
    {entries?.length === 0 && <p className="tasks-empty">조건에 맞는 완료한 할 일이 없다</p>}
    <ul className="task-completed-rows">
      {entries?.map((a) => {
        const report = reports.find((r) => r.name === a.report || (a.task.thread && (r.name === a.task.thread || r.from_thread === a.task.thread)));
        const notes = a.task.notes.join("\n");
        return <li className="task-completed-row" key={a.task_id} data-testid={`archive-${a.task_id}`}>
          <details className="task-archive-detail">
            <summary><ListId id={a.task.short_id ?? a.task_id} /><span className="task-completed-title">{a.task.title}</span>{a.restored && <span className="task-chip">되살림</span>}</summary>
            <div className="task-completed-detail">
              {notes && <Markdown>{withoutAttachments(notes)}</Markdown>}
              <TaskAttachments project={project} notes={notes} canEdit={false} onOpen={onOpenAttachment ? (v) => { void onOpenAttachment(project, v); } : undefined} />
              {report && <a className="task-completed-report" href={`#report=${encodeURIComponent(`${report.project}/${report.name}`)}`}><ListId id={report.report_id ?? report.name} /> 연결 보고서</a>}
              {!report && a.report && <p className="muted">연결 보고서가 목록에 없다 · {a.report}</p>}
              {canEdit && !a.restored && <button className="btn btn-ghost" type="button" disabled={busy || loading} onClick={() => void restore(a.task_id)}>되살리기</button>}
            </div>
          </details>
          <div className="task-completed-meta">
            {a.task.thread && <button className="linkish" type="button" onClick={() => onThread(a.task.thread!)}>{a.task.thread}</button>}
            <span>{a.committed ? a.reason : `보관 확정 확인 필요 · ${a.reason}`}</span>
            <time dateTime={new Date(a.completed_ms).toISOString()}>{new Date(a.completed_ms).toLocaleString()}</time>
            <span>저장소: {a.task.repo || "정하지 않음"}</span><span>모델: {a.task.profile || "정하지 않음"}</span><span>머신: {a.task.machine || "정하지 않음"}</span>
          </div>
        </li>;
      })}
    </ul>
    {next !== null && <button className="btn task-completed-more" type="button" disabled={loading || busy} data-testid="task-archive-more" onClick={() => void load(next)}>더 보기</button>}
  </section>;
}
