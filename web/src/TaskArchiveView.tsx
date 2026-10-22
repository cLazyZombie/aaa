import { useCallback, useEffect, useRef, useState } from "react";
import type { ArchivedTask, TaskArchiveFilter, TaskHistoryRepo } from "./gen/proto";
import { call, onTasksChanged } from "./api";
import { Markdown } from "./Markdown";

export const ARCHIVE_FILTERS = [
  { id: "completed", label: "완료 보관" },
  { id: "pr_merged", label: "PR 병합" },
  { id: "main_merged", label: "main 병합" },
  { id: "report_done", label: "보고 완료" },
  { id: "archive_restored", label: "보관 복원됨" },
] as const;

export function TaskArchiveView({ project, canEdit, rev, query, repo, filter, onRepos, onThread }: {
  project: string; canEdit: boolean; rev: string; query: string; repo: string | null; filter: TaskArchiveFilter | null;
  onRepos: (repos: TaskHistoryRepo[]) => void; onThread: (id: string) => void;
}) {
  const [entries, setEntries] = useState<ArchivedTask[] | null>(null);
  const [next, setNext] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const seq = useRef(0);
  const load = useCallback(async (after: string | null, restore: string | null = null) => {
    const n = ++seq.current;
    setLoading(true); setError("");
    if (after === null) { setEntries(null); setNext(null); }
    try {
      const r = await call({ op: "task_archive", project, restore, view: { search: { query, repo }, filter, after, limit: 50 } });
      if (seq.current !== n || r.kind !== "task_archive") return;
      setEntries((old) => after === null ? r.entries : [...(old ?? []), ...r.entries]);
      setNext(r.next ?? null); onRepos(r.repos ?? []);
    } catch (e) { if (seq.current === n) setError(String(e)); }
    finally { if (seq.current === n) setLoading(false); }
  }, [project, query, repo, filter, onRepos]);
  useEffect(() => {
    void load(null);
    const off = onTasksChanged(() => { void load(null); });
    return () => { ++seq.current; off(); };
  }, [load, rev]);
  return <section aria-label="완료 보관" data-testid="task-archive-list" aria-busy={loading}>
    {error && <p role="alert">{error}</p>}
    {entries === null && !error && <p className="tasks-empty">불러오는 중</p>}
    {entries?.length === 0 && <p className="tasks-empty">조건에 맞는 완료 보관이 없다</p>}
    <ul className="task-log-rows">
      {entries?.map((a) => <li className="task-log-row" key={a.task_id} data-testid={`archive-${a.task_id}`}>
        <div className="task-log-line">
          <details className="task-archive-detail"><summary><span className="task-chip task-log-action">{a.restored ? "복원됨" : "완료 보관"}</span> <span className="task-short-id">{a.task.short_id}</span>{a.task.title}</summary><Markdown>{a.body}</Markdown></details>
          {canEdit && !a.restored && <button className="btn btn-ghost task-log-restore" type="button" disabled={busy || loading} onClick={() => { setBusy(true); void load(null, a.task_id).finally(() => setBusy(false)); }}>활성 목록으로 복원</button>}
        </div>
        <p className="task-log-meta"><span>{a.committed ? a.reason : `보관 확정 확인 필요 · ${a.reason}`}</span><span>{a.task.repo ?? "정하지 않음"}</span><span>{new Date(a.completed_ms).toLocaleString()}</span>{a.task.thread && <button className="linkish" type="button" onClick={() => onThread(a.task.thread!)}>{a.task.thread} 결과</button>}</p>
      </li>)}
    </ul>
    {next !== null && <button className="btn task-log-more" type="button" disabled={loading || busy} data-testid="task-archive-more" onClick={() => void load(next)}>더 보기</button>}
  </section>;
}
