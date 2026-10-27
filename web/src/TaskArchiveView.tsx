import { ignoredFailure } from "./errorDiagnostic.ts";
// 완료 목록은 host의 보관 원장을 검색한다. 필터는 페이지를 자르기 전에 적용한다.
import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { ArrowDownWideNarrow, CalendarCheck, CalendarPlus, ChevronRight, FolderGit2, Hash, type LucideIcon } from "lucide-react";
import type { ArchivedTask, LibraryReport, TaskArchiveFilter, TaskArchiveSort, TaskHistoryRepo } from "./gen/proto";
import { call, onTasksChanged } from "./api";
import { Markdown } from "./Markdown";
import { ListId } from "./ListId";
import { TaskAttachments } from "./TaskAttachments";
import { withoutAttachments, type TaskAttachment } from "./tasksModel";
import { TaskChoiceChip, TaskFilterChips, TaskSearchInput, historyChoices } from "./TaskHistoryTools";
import { RowMenu } from "./RowMenu";
import { ARCHIVE_SORTS, archiveGroups, archiveTime, fullTime, readArchiveSort, reasonLabel, rowTime, spanText, writeArchiveSort } from "./taskArchiveModel";

const REASONS = [
  { key: "main_merged", label: "main 병합" },
  { key: "report_done", label: "보고 완료" },
  { key: "restored", label: "되살린 항목" },
];
const PERIODS = [{ key: "7", label: "최근 7일" }, { key: "30", label: "최근 30일" }, { key: "90", label: "최근 90일" }];
const choicesOf = (values: string[]) => values.map((key) => ({ key, label: key || "정하지 않음" }));
const SORT_ICON: Record<TaskArchiveSort, LucideIcon> = { completed: CalendarCheck, started: CalendarPlus, number: Hash };
const timeTitle = (a: ArchivedTask) => `끝 ${fullTime(a.completed_ms)}${a.started_ms === undefined ? "" : ` · 시작 ${fullTime(a.started_ms)}`}`;

export function TaskArchiveView({ project, canEdit, rev, toolbar, repos, extendedFilters, sortable, onThread, onOpenAttachment }: {
  project: string; canEdit: boolean; rev: string; toolbar: HTMLElement | null; repos: string[]; extendedFilters: boolean;
  /** FRONT_API 1.126: host가 정렬하고 시작 시각을 준다. 옛 host는 보관 ID 순서라 날짜로 묶지 않는다 */
  sortable: boolean;
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
  const [sort, setSort] = useState<TaskArchiveSort>(readArchiveSort);
  const [sortMenu, setSortMenu] = useState<HTMLElement | null>(null);
  const [open, setOpen] = useState<ReadonlySet<string>>(new Set());
  // 날짜 머리(오늘·어제)의 기준. 목록을 다시 읽을 때 새로 잡는다
  const [now, setNow] = useState(() => Date.now());
  const seq = useRef(0);
  const pickSort = (next: TaskArchiveSort) => { setSort(next); writeArchiveSort(next); };
  const toggle = (id: string) => setOpen((old) => {
    const next = new Set(old);
    if (!next.delete(id)) next.add(id);
    return next;
  });
  const load = useCallback(async (after: string | null) => {
    const n = ++seq.current;
    setLoading(true); setError("");
    if (after === null) { setEntries(null); setNext(null); }
    try {
      const r = await call({ op: "task_archive", project, restore: null, view: {
        search: { query, repo }, filter, after, limit: 50,
        profile: extendedFilters ? profile : null, machine: extendedFilters ? machine : null,
        since_ms: extendedFilters ? since : null, until_ms: null, ...(sortable ? { sort } : {}),
      } });
      if (seq.current !== n || r.kind !== "task_archive") return;
      if (after === null) setNow(Date.now());
      setEntries((old) => after === null ? r.entries : [...(old ?? []), ...r.entries]);
      setNext(r.next ?? null); setPastRepos(r.repos ?? []); setProfiles(r.profiles ?? []); setMachines(r.machines ?? []);
    } catch (e) { if (seq.current === n) setError(String(e)); }
    finally { if (seq.current === n) setLoading(false); }
  }, [project, query, repo, filter, profile, machine, since, extendedFilters, sortable, sort]);
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
    }).catch(ignoredFailure("TaskArchiveView.tsx:70", () => { /* 보관 원문과 thread 연결은 보고서 목록이 없어도 볼 수 있다. */ }));
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
  const groups = archiveGroups(entries ?? [], sortable ? sort : "number", now);
  const sorted = ARCHIVE_SORTS.find((s) => s.key === sort)!;
  return <section className="tasks-scroll task-completed" aria-label="완료한 할 일" data-testid="task-archive-list" data-sort={sortable ? sort : ""} aria-busy={loading || busy}>
    {toolbar && createPortal(<>
      <TaskSearchInput value={query} onChange={setQuery} label="완료 검색" testId="task-completed-search" />
      <TaskFilterChips label="완료 거르기" repo={<TaskChoiceChip label="저장소" value={repo} onChange={setRepo} choices={historyChoices(repos, pastRepos)} testId="task-completed-repo" />} sort={sortable && <>
        <button type="button" className="task-chip tasks-sort" aria-haspopup="menu" aria-expanded={sortMenu !== null} aria-label={`정렬: ${sorted.label}, ${sorted.hint}`} title={`${sorted.label}순 · ${sorted.hint}`} data-testid="task-completed-sort" onClick={(e) => setSortMenu(e.currentTarget)}>
          <ArrowDownWideNarrow aria-hidden="true" />{sorted.label}
        </button>
        {sortMenu && <RowMenu anchor={sortMenu} title="정렬" subtitle="늘 최근·큰 번호가 위" onClose={() => setSortMenu(null)} items={ARCHIVE_SORTS.map((s) => ({
          id: s.key, label: s.label, hint: s.hint, icon: SORT_ICON[s.key], checked: sort === s.key, run: () => pickSort(s.key),
        }))} />}
      </>}>
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
    {groups.map((g) => <div className="task-completed-group" key={g.key}>
      {g.label && <h3 className="task-completed-day">{g.label}</h3>}
      <ul className="task-completed-rows">
        {g.entries.map((a) => {
          const report = reports.find((r) => r.name === a.report || (a.task.thread && (r.name === a.task.thread || r.from_thread === a.task.thread)));
          const notes = a.task.notes.join("\n");
          const isOpen = open.has(a.task_id);
          const at = sortable ? archiveTime(a, sort) : a.completed_ms;
          const detailId = `archive-detail-${a.task_id}`;
          return <li className={`task-completed-row${isOpen ? " is-open" : ""}`} key={a.task_id} data-testid={`archive-${a.task_id}`}>
            <button type="button" className="task-completed-pick" aria-expanded={isOpen} aria-controls={detailId} onClick={() => toggle(a.task_id)}>
              <ChevronRight className="task-completed-caret" aria-hidden="true" />
              <ListId id={a.task.short_id ?? a.task_id} />
              <span className="task-completed-title" title={a.task.title}>{a.task.title}</span>
            </button>
            <div className="task-completed-chips">
              {a.restored && <span className="task-chip">되살림</span>}
              {!a.committed && <span className="task-chip task-problem" title="보관 확정을 확인하지 못했다">확인 필요</span>}
              {a.task.repo && <span className="task-chip is-dim task-completed-repo" title={`저장소: ${a.task.repo}`}><FolderGit2 aria-hidden="true" />{a.task.repo}</span>}
              {a.task.thread && <button type="button" className="task-chip task-thread" title={`${a.task.thread} 열기`} onClick={() => onThread(a.task.thread!)}>{a.task.thread}</button>}
              <span className="task-chip is-dim" title={`완료 근거: ${a.reason}`}>{reasonLabel(a.reason)}</span>
            </div>
            {at !== null && <time className="task-completed-time" dateTime={new Date(at).toISOString()} title={timeTitle(a)}>{rowTime(at, !sortable || sort === "number", now)}</time>}
            {isOpen && <div className="task-completed-detail" id={detailId}>
              <dl className="task-completed-facts">
                <div><dt>끝</dt><dd>{fullTime(a.completed_ms)}</dd></div>
                <div><dt>시작</dt><dd>{a.started_ms === undefined ? "기록 없음" : `${fullTime(a.started_ms)} · ${spanText(a.started_ms, a.completed_ms)} 걸림`}</dd></div>
                <div><dt>완료 근거</dt><dd>{a.reason}</dd></div>
                {a.task.profile && <div><dt>모델</dt><dd>{a.task.profile}</dd></div>}
                {a.task.machine && <div><dt>머신</dt><dd>{a.task.machine}</dd></div>}
              </dl>
              {notes && <Markdown>{withoutAttachments(notes)}</Markdown>}
              <TaskAttachments project={project} notes={notes} canEdit={false} onOpen={onOpenAttachment ? (v) => { void onOpenAttachment(project, v); } : undefined} />
              {report && <a className="task-completed-report" href={`#report=${encodeURIComponent(`${report.project}/${report.name}`)}`}><ListId id={report.report_id ?? report.name} /> 연결 보고서</a>}
              {!report && a.report && <p className="muted">연결 보고서가 목록에 없다 · {a.report}</p>}
              {canEdit && !a.restored && <button className="btn btn-ghost" type="button" disabled={busy || loading} onClick={() => void restore(a.task_id)}>되살리기</button>}
            </div>}
          </li>;
        })}
      </ul>
    </div>)}
    {next !== null && <button className="btn task-completed-more" type="button" disabled={loading || busy} data-testid="task-archive-more" onClick={() => void load(next)}>더 보기</button>}
  </section>;
}
