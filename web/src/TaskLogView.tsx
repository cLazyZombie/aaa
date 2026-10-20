// 할 일 기록 화면(할 일 편집 T2, plan §8.309, t-0469 §3.5): 최근 변경을 시간순으로 누가(기기 이름·coordinator·터미널·파일 직접·
// 자동 정리)·언제·무엇·전후 차이로 보인다. 삭제 항목에는 「되살리기」, 거르기는 삭제만·자동 정리만. 목록 모양은 t3code
// SettingsPanels.tsx `ArchivedThreadsPanel`(3294–3560: 묶음 목록, 줄마다 되살리기 단추, 누른 뒤 다시 읽기)을 따른다.
import { useCallback, useEffect, useRef, useState } from "react";
import { ChevronDown, ChevronRight, RotateCcw } from "lucide-react";
import type { TaskLogEntry, TaskLogFilter } from "./gen/proto";
import { ApiError, call } from "./api";
import { ConfirmDialog } from "./ConfirmDialog";
import { TASK_ACTION_LABEL, ago, lineDiff, restorable } from "./tasksModel";

const FILTERS: readonly { id: TaskLogFilter | null; label: string }[] = [
  { id: null, label: "전체" },
  { id: "deleted", label: "삭제만" },
  { id: "auto", label: "자동 정리만" },
];

/** 오래된 기록 지우기의 기준(일) */
const PRUNE_DAYS = 30;

export function TaskLogView({ canEdit, rev, onRestore }: { canEdit: boolean; rev: string; onRestore: (e: TaskLogEntry) => Promise<void> }) {
  const [filter, setFilter] = useState<TaskLogFilter | null>(null);
  const [entries, setEntries] = useState<TaskLogEntry[] | null>(null);
  const [next, setNext] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState<ReadonlySet<number>>(new Set());
  const [busy, setBusy] = useState<number | null>(null);
  const [prune, setPrune] = useState(false);
  const seq = useRef(0);
  const load = useCallback(
    (before: number | null) => {
      const n = ++seq.current;
      call({ op: "task_log", before, limit: 50, filter })
        .then((r) => {
          if (n !== seq.current || r.kind !== "task_log") return;
          setEntries((old) => (before === null ? r.entries : [...(old ?? []), ...r.entries]));
          setNext(r.next);
          setError(null);
        })
        .catch((e: unknown) => {
          if (n === seq.current) setError(e instanceof ApiError ? e.message : String(e));
        });
    },
    [filter],
  );
  // 파일 판(`rev`)이 바뀌면 처음부터 다시 읽는다
  useEffect(() => load(null), [load, rev]);
  const toggle = (id: number) =>
    setOpen((o) => {
      const n = new Set(o);
      if (n.has(id)) n.delete(id);
      else n.add(id);
      return n;
    });
  const now = Date.now();
  return (
    <div className="tasks-scroll task-log" data-testid="task-log">
      <div className="task-log-tools">
        <div className="tasks-filters" role="group" aria-label="기록 거르기">
          {FILTERS.map((f) => (
            <button key={f.label} type="button" className={`task-chip tasks-filter${filter === f.id ? " is-on" : ""}`} aria-pressed={filter === f.id} data-testid={`task-log-filter-${f.id ?? "all"}`} onClick={() => setFilter(f.id)}>
              {f.label}
            </button>
          ))}
        </div>
        <span className="spacer" />
        {canEdit && (
          <button type="button" className="btn btn-ghost" onClick={() => setPrune(true)}>
            {PRUNE_DAYS}일 지난 기록 지우기
          </button>
        )}
      </div>
      {error && (
        <div className="banner err" role="alert">
          {error}
        </div>
      )}
      {entries === null && !error && <p className="tasks-empty">불러오는 중</p>}
      {entries?.length === 0 && <p className="tasks-empty">기록이 없다</p>}
      <ul className="task-log-rows">
        {entries?.map((e) => {
          const expanded = open.has(e.id);
          const where = e.from_list !== null && e.from_list !== e.list ? `${e.from_list || "(목록 없음)"} → ${e.list || "(목록 없음)"}` : e.list;
          const hasBody = e.before !== null || e.after !== null;
          return (
            <li key={e.id} className={`task-log-row is-${e.action}`} data-testid={`task-log-${e.id}`} data-action={e.action}>
              <div className="task-log-line">
                <button type="button" className="task-log-toggle" aria-expanded={expanded} disabled={!hasBody} onClick={() => toggle(e.id)}>
                  {hasBody ? expanded ? <ChevronDown aria-hidden="true" /> : <ChevronRight aria-hidden="true" /> : <span className="task-log-dot" aria-hidden="true" />}
                  <span className="task-chip task-log-action">{TASK_ACTION_LABEL[e.action]}</span>
                  <span className="task-log-title">{e.title || where || "TASKS.md"}</span>
                </button>
                {canEdit && restorable(e) && (
                  <button
                    type="button"
                    className="btn btn-ghost task-log-restore"
                    disabled={busy !== null}
                    data-testid={`task-log-restore-${e.id}`}
                    onClick={() => {
                      setBusy(e.id);
                      void onRestore(e)
                        .then(() => load(null))
                        .catch((err: unknown) => setError(err instanceof Error ? err.message : String(err)))
                        .finally(() => setBusy(null));
                    }}
                  >
                    <RotateCcw aria-hidden="true" />
                    되살리기
                  </button>
                )}
                {e.restored_by !== null && <span className="task-chip is-dim">되살림</span>}
              </div>
              <p className="task-log-meta">
                <span>{e.by}</span>
                <span title={new Date(e.at_ms).toLocaleString()}>{ago(e.at_ms, now)}</span>
                {e.title && where && <span>{where}</span>}
                {e.thread && <span>{e.thread}</span>}
              </p>
              {expanded && hasBody && (
                <pre className="task-log-diff" aria-label="전후 차이">
                  {lineDiff(e.before, e.after).map((d, i) => (
                    <span key={i} className={`is-${d.kind}`}>
                      {d.kind === "add" ? "+ " : d.kind === "del" ? "- " : "  "}
                      {d.text}
                      {"\n"}
                    </span>
                  ))}
                </pre>
              )}
            </li>
          );
        })}
      </ul>
      {next !== null && (
        <button type="button" className="btn task-log-more" onClick={() => load(next)}>
          더 보기
        </button>
      )}
      {prune && (
        <ConfirmDialog
          title="오래된 기록 지우기"
          body={`${PRUNE_DAYS}일보다 오래된 할 일 기록을 지운다. 지운 기록의 task는 되살릴 수 없게 된다.`}
          confirmLabel="지우기"
          onConfirm={async () => {
            await call({ op: "task_log_prune", before_ms: Date.now() - PRUNE_DAYS * 86_400_000 });
            setPrune(false);
            load(null);
          }}
          onClose={() => setPrune(false)}
        />
      )}
    </div>
  );
}
