// 본문(오른쪽) 할 일 화면(할 일 보기 T1, 사용자 10-20 「하기로 했던 기능들을 다 볼수 있고 (왼쪽에 아이콘이 있어서 거기서 들어가게)」,
// plan §8.306). 활동 막대 「할 일」·`#tasks`가 연다. host `tasks`(TaskBoard)를 읽기만 한다: 고치기는 T2, thread로 맡기기는 T3다.
// 행 모양과 상태 아이콘은 t3code ComposerTasksBadge.tsx(Circle·CircleDot, 상태별 글자색, 낭독기용 상태 문구), 목록 머리(이름·수)와 메모 표지
// `≡`은 herdr-projects popup.rs `build`(Section::Tasks)·`task_detail`을 따른다. 상세는 넓은 본문에서 오른쪽 칸, 좁은 본문(패드 세로)에서
// 오른쪽 시트, 휴대폰에서 전체 화면 시트(Back으로 닫기, herdr-web-ui·sheetHistory.ts)다.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ChevronDown, ChevronLeft, ChevronRight, Circle, CircleDot, Search, Triangle, X } from "lucide-react";
import type { TaskBoard, TaskNotesFile, TaskThread, TaskView, ThreadGroup } from "./gen/proto";
import { ApiError, call, onTasksChanged } from "./api";
import { DrawerToggle, HeadMore, HeadNote } from "./drawer";
import { Markdown } from "./Markdown";
import { GROUP_WORD } from "./sidebarModel";
import { usePhoneSheetBack } from "./herdr/sheetHistory";
import { TASK_FILTERS, TASK_STATE_LABEL, findTask, ownerLabel, taskDepth, taskSections, taskState, type TaskFilter, type TaskState, type ThreadGroups } from "./tasksModel";
import "./TasksView.css";

type Tab = "tasks" | "later";

/**
 * 할 일 board(활동 막대 배지와 할 일 화면이 함께 쓴다). host 연결이 오를 때, host `tasks` 사건(TASKS.md가 바뀜)마다, 열린 thread
 * 모음이 바뀔 때(「task 없는 열린 thread」) 다시 읽는다. 늦게 돌아온 옛 응답은 버린다.
 */
export function useTaskBoard(up: boolean, openThreads: string): { board: TaskBoard | null; error: string | null } {
  const [board, setBoard] = useState<TaskBoard | null>(null);
  const [error, setError] = useState<string | null>(null);
  const seq = useRef(0);
  const load = useCallback(() => {
    const n = ++seq.current;
    call({ op: "tasks" })
      .then((r) => {
        if (n !== seq.current || r.kind !== "task_board") return;
        const { kind: _kind, ...b } = r;
        setBoard(b);
        setError(null);
      })
      .catch((e: unknown) => {
        if (n === seq.current) setError(e instanceof ApiError ? e.message : String(e));
      });
  }, []);
  useEffect(() => {
    if (up) load();
  }, [up, openThreads, load]);
  useEffect(() => onTasksChanged(() => load()), [load]);
  return { board, error };
}
type Key = { title: string; line: number };

function StateIcon({ state }: { state: TaskState }) {
  const Icon = state === "problem" ? Triangle : state === "run" ? CircleDot : Circle;
  return <Icon className={`task-state is-${state}`} aria-hidden="true" />;
}

/** thread 칩: `t-0468 · 작업 중`, 누르면 그 thread로. 모르는 thread는 누를 수 없다 */
function ThreadChip({ id, groups, onThread }: { id: string; groups: ThreadGroups; onThread: (id: string) => void }) {
  const g = groups.get(id);
  const word = g ? (GROUP_WORD[g] ?? g) : "모름";
  return (
    <button
      type="button"
      className={`task-chip task-thread is-${g ?? "unknown"}`}
      disabled={g === undefined}
      aria-label={`${id} ${word}, thread 열기`}
      data-testid={`task-thread-${id}`}
      onClick={(e) => {
        e.stopPropagation();
        onThread(id);
      }}
    >
      {id} · {word}
    </button>
  );
}

export function TasksView({
  shown,
  board,
  error,
  groups,
  onThread,
}: {
  shown: boolean;
  /** 마지막으로 읽은 TaskBoard(App이 링크가 오를 때와 `tasks` 사건마다 읽는다) */
  board: TaskBoard | null;
  error: string | null;
  /** thread id → 묶음(App의 thread 목록, 바뀌면 바로 따라간다) */
  groups: ThreadGroups;
  onThread: (id: string) => void;
}) {
  const [tab, setTab] = useState<Tab>("tasks");
  const [filter, setFilter] = useState<TaskFilter>("all");
  const [query, setQuery] = useState("");
  const [folded, setFolded] = useState<ReadonlySet<string>>(new Set());
  const [picked, setPicked] = useState<Key | null>(null);
  const notesFiles = board?.notes_files ?? [];
  // 「나중」 파일이 설정에서 빠지면 할 일 탭으로
  useEffect(() => {
    if (tab === "later" && notesFiles.length === 0) setTab("tasks");
  }, [tab, notesFiles.length]);
  const byLine = useMemo(() => new Map((board?.tasks ?? []).map((t) => [t.line, t])), [board]);
  const sections = useMemo(() => (board ? taskSections(board, filter, query, groups) : []), [board, filter, query, groups]);
  const found = findTask(board, picked);
  // 숨은 동안(다른 화면)은 상세를 내린다: 휴대폰 시트의 뒤로 기록과 Esc를 남기지 않는다. 돌아오면 고른 task가 그대로 열린다
  const current = shown ? found : null;
  // 고른 task가 파일에서 빠지면 상세를 닫는다
  useEffect(() => {
    if (picked && board && !found) setPicked(null);
  }, [picked, board, found]);
  const shownCount = sections.reduce((n, s) => n + s.tasks.length, 0);
  const total = board?.tasks.length ?? 0;
  const toggle = (name: string) =>
    setFolded((f) => {
      const next = new Set(f);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  return (
    <div
      className={`tasks-view${shown ? "" : " is-hidden"}`}
      inert={!shown}
      data-testid="tasks-view"
      data-shown={shown ? "true" : "false"}
      data-tasks-sha={board?.sha256 ?? ""}
    >
      <header className="term-head">
        <DrawerToggle />
        <HeadNote />
        <div className="term-title">
          <strong>할 일</strong>
          <span className="muted">TASKS.md · {total}개</span>
        </div>
        <HeadMore />
      </header>
      <div className="tasks-toolbar">
        {notesFiles.length > 0 && (
          <div className="tasks-tabs" role="tablist" aria-label="할 일 탭">
            <button type="button" role="tab" aria-selected={tab === "tasks"} className={`tasks-tab${tab === "tasks" ? " is-on" : ""}`} data-testid="tasks-tab-tasks" onClick={() => setTab("tasks")}>
              할 일 <span className="tasks-count">{total}</span>
            </button>
            <button type="button" role="tab" aria-selected={tab === "later"} className={`tasks-tab${tab === "later" ? " is-on" : ""}`} data-testid="tasks-tab-later" onClick={() => setTab("later")}>
              나중
            </button>
          </div>
        )}
        {tab === "tasks" && (
          <>
            <label className="tasks-search">
              <Search aria-hidden="true" />
              <input className="input" type="search" value={query} placeholder="제목·메모·owner 검색" aria-label="할 일 검색" data-testid="tasks-search" onChange={(e) => setQuery(e.target.value)} />
            </label>
            <div className="tasks-filters" role="group" aria-label="거르기">
              {TASK_FILTERS.map((f) => (
                <button key={f.id} type="button" className={`task-chip tasks-filter${filter === f.id ? " is-on" : ""}`} aria-pressed={filter === f.id} data-testid={`tasks-filter-${f.id}`} onClick={() => setFilter(f.id)}>
                  {f.label}
                </button>
              ))}
            </div>
          </>
        )}
      </div>
      {error && (
        <div className="banner err" role="alert" data-testid="tasks-error">
          {error}
        </div>
      )}
      {tab === "later" ? (
        <div className="tasks-scroll" data-testid="tasks-later">
          {notesFiles.map((f) => (
            <NotesFile key={f.path} file={f} />
          ))}
        </div>
      ) : (
        <div className={`tasks-main${current ? " has-detail" : ""}`}>
          <div className="tasks-scroll" data-testid="tasks-list">
            {board === null && !error && <p className="tasks-empty">불러오는 중</p>}
            {board && total === 0 && <p className="tasks-empty">할 일이 없다. coordinator에게 더해 달라고 한다</p>}
            {board && total > 0 && shownCount === 0 && <p className="tasks-empty">맞는 할 일이 없다</p>}
            {sections.map((s) => {
              const closed = folded.has(s.name);
              // 거르는 동안 빈 목록은 숨긴다(전체·빈 검색이면 task 없는 목록도 머리를 보인다)
              if (s.tasks.length === 0 && (filter !== "all" || query.trim() !== "")) return null;
              return (
                <section key={s.name} className="task-list" aria-label={s.name || "목록 없음"} data-testid={`task-list-${s.name}`}>
                  <h3 className="task-list-head">
                    <button type="button" className="task-list-toggle" aria-expanded={!closed} onClick={() => toggle(s.name)}>
                      {closed ? <ChevronRight aria-hidden="true" /> : <ChevronDown aria-hidden="true" />}
                      <span className="task-list-name">{s.name || "(목록 없음)"}</span>
                      <span className="tasks-count">{s.tasks.length === s.total ? s.total : `${s.tasks.length}/${s.total}`}</span>
                    </button>
                  </h3>
                  {!closed && (
                    <>
                      {s.notes.map((n, i) => (
                        <p key={i} className="task-list-note">
                          {n}
                        </p>
                      ))}
                      <ul className="task-rows">
                        {s.tasks.map((t) => (
                          <TaskRow
                            key={`${t.line}:${t.title}`}
                            t={t}
                            depth={taskDepth(t, byLine)}
                            state={taskState(t, groups)}
                            selected={current?.line === t.line}
                            groups={groups}
                            onPick={() => setPicked({ title: t.title, line: t.line })}
                            onThread={onThread}
                          />
                        ))}
                      </ul>
                    </>
                  )}
                </section>
              );
            })}
            {board && board.unlinked_threads.length > 0 && <Unlinked threads={board.unlinked_threads} onThread={onThread} />}
          </div>
          {current ? (
            <TaskDetail key={`${current.line}:${current.title}`} t={current} byLine={byLine} groups={groups} onThread={onThread} onPick={(t) => setPicked({ title: t.title, line: t.line })} onClose={() => setPicked(null)} />
          ) : (
            <aside className="task-detail is-empty" aria-hidden="true">
              <p className="tasks-empty">task를 고르면 메모·원문 줄·형식 어긋남이 여기 보인다</p>
            </aside>
          )}
          {current && <div className="tasks-scrim" aria-hidden="true" onClick={() => setPicked(null)} />}
        </div>
      )}
    </div>
  );
}

function TaskRow({
  t,
  depth,
  state,
  selected,
  groups,
  onPick,
  onThread,
}: {
  t: TaskView;
  depth: number;
  state: TaskState;
  selected: boolean;
  groups: ThreadGroups;
  onPick: () => void;
  onThread: (id: string) => void;
}) {
  const owner = ownerLabel(t);
  return (
    <li className={`task-row is-${state}${selected ? " is-selected" : ""}${t.done ? " is-done" : ""}`} style={{ paddingLeft: `calc(${depth} * var(--space-5))` }} data-task-state={state} data-testid={`task-${t.line}`}>
      <button type="button" className="task-pick" aria-current={selected ? "true" : undefined} onClick={onPick}>
        <StateIcon state={state} />
        <span className="task-title">
          <span className="sr-only">{TASK_STATE_LABEL[state]}: </span>
          {t.title}
        </span>
        {state === "problem" && <span className="task-chip task-problem">형식 어긋남</span>}
        {owner && <span className={`task-chip task-owner${owner === "미할당" ? " is-dim" : ""}`}>{owner}</span>}
        {t.notes.length > 0 && (
          <span className="task-notes-mark" role="img" aria-label="메모 있음" title="메모 있음">
            ≡
          </span>
        )}
      </button>
      {t.thread && <ThreadChip id={t.thread} groups={groups} onThread={onThread} />}
    </li>
  );
}

function Unlinked({ threads, onThread }: { threads: TaskThread[]; onThread: (id: string) => void }) {
  return (
    <section className="task-list task-unlinked" aria-label="task 없는 열린 thread" data-testid="tasks-unlinked">
      <h3 className="task-list-head">
        <span className="task-list-static">
          task 없는 열린 thread <span className="tasks-count">{threads.length}</span>
        </span>
      </h3>
      <ul className="task-rows">
        {threads.map((t) => (
          <li key={t.id} className="task-row">
            <button type="button" className="task-pick" data-testid={`tasks-unlinked-${t.id}`} onClick={() => onThread(t.id)}>
              <CircleDot className="task-state is-run" aria-hidden="true" />
              <span className="task-title">
                {t.id} · {t.title}
              </span>
              <span className={`task-chip task-thread is-${t.group}`}>{GROUP_WORD[t.group as ThreadGroup] ?? t.group}</span>
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

/** 상세(읽기 전용): 제목·owner·메모(Markdown)·원문 줄·형식 어긋남·연결 thread·상위·하위 task */
function TaskDetail({
  t,
  byLine,
  groups,
  onThread,
  onPick,
  onClose,
}: {
  t: TaskView;
  byLine: ReadonlyMap<number, TaskView>;
  groups: ThreadGroups;
  onThread: (id: string) => void;
  onPick: (t: TaskView) => void;
  onClose: () => void;
}) {
  // 휴대폰은 전체 화면 시트: 시스템 Back이 닫는다(herdr-web-ui, sheetHistory.ts)
  usePhoneSheetBack(() => onClose());
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // 위에 뜬 창(팔레트·설정·확인창)의 Esc는 그 창 것이다
      if (e.key !== "Escape" || e.defaultPrevented || document.querySelector('[aria-modal="true"]')) return;
      e.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  const parent = t.parent !== null ? byLine.get(t.parent) : undefined;
  const children = [...byLine.values()].filter((c) => c.parent === t.line);
  const owner = ownerLabel(t);
  const state = taskState(t, groups);
  return (
    <aside className="task-detail" role="dialog" aria-label={`할 일 ${t.title}`} data-testid="task-detail" data-task-state={state}>
      <header className="task-detail-head">
        {/* 휴대폰은 「‹ 할 일」(전체 화면 시트의 뒤로), 넓은 화면은 닫기 X */}
        <button type="button" className="task-detail-close" aria-label="할 일 목록으로" title="할 일 목록으로" data-testid="task-detail-close" onClick={onClose}>
          <X className="task-close-x" aria-hidden="true" />
          <ChevronLeft className="task-close-back" aria-hidden="true" />
          <span className="task-close-back">할 일</span>
        </button>
        <span className="task-detail-list">{t.list || "(목록 없음)"} · {t.line}줄</span>
      </header>
      <div className="task-detail-body">
        <h2 className="task-detail-title">
          <StateIcon state={state} />
          <span>{t.title}</span>
        </h2>
        <div className="task-detail-chips">
          <span className="task-chip">{TASK_STATE_LABEL[state]}</span>
          {owner && <span className={`task-chip task-owner${owner === "미할당" ? " is-dim" : ""}`}>owner · {owner}</span>}
          {t.thread && <ThreadChip id={t.thread} groups={groups} onThread={onThread} />}
          {t.done && <span className="task-chip is-dim">[x] 완료 표시</span>}
        </div>
        {t.problems.length > 0 && (
          <section className="task-detail-section task-problems" data-testid="task-problems">
            <h3>형식 어긋남</h3>
            <ul>
              {t.problems.map((p) => (
                <li key={p}>{p}</li>
              ))}
            </ul>
            <p className="muted">
              owner·thread로 읽지 않고 원문 그대로 보인다. 고치려면 TASKS.md의 이 줄을 정해진 꼴 <code>- [ ] 제목 (owner) · t-번호</code>로 바꾼다.
            </p>
          </section>
        )}
        <section className="task-detail-section">
          <h3>메모</h3>
          {t.notes.length > 0 ? <Markdown className="task-notes">{t.notes.join("\n")}</Markdown> : <p className="muted">메모 없음</p>}
        </section>
        <section className="task-detail-section">
          <h3>원문 줄</h3>
          <pre className="task-raw" data-testid="task-raw">
            <code>{t.raw}</code>
          </pre>
        </section>
        {(parent || children.length > 0) && (
          <section className="task-detail-section">
            <h3>{parent ? "상위 task" : "하위 task"}</h3>
            <ul className="task-related">
              {parent && (
                <li>
                  <button type="button" className="linkish" onClick={() => onPick(parent)}>
                    {parent.title}
                  </button>
                </li>
              )}
            </ul>
            {parent && children.length > 0 && <h3>하위 task</h3>}
            {children.length > 0 && (
              <ul className="task-related">
                {children.map((c) => (
                  <li key={c.line}>
                    <StateIcon state={taskState(c, groups)} />
                    <button type="button" className="linkish" onClick={() => onPick(c)}>
                      {c.title}
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </section>
        )}
      </div>
    </aside>
  );
}

/** 「나중」 파일 하나: `##` 절을 접는 묶음으로, 줄은 Markdown 그대로(읽기만, 「할 일로 옮기기」는 T2) */
function NotesFile({ file }: { file: TaskNotesFile }) {
  return (
    <section className="task-notes-file" aria-label={file.path} data-testid={`tasks-notes-${file.path}`}>
      <h3 className="task-list-head">
        <span className="task-list-static">{file.path}</span>
      </h3>
      {file.error ? (
        <p className="tasks-empty" data-testid="tasks-notes-error">
          {file.error}
        </p>
      ) : (
        file.sections.map((s, i) => (
          <details key={`${i}:${s.name}`} className="task-notes-section">
            <summary>
              <ChevronRight className="task-notes-chevron" aria-hidden="true" />
              <span className="task-list-name">{s.name || "(머리 없음)"}</span>
              <span className="tasks-count">{s.lines.filter((l) => l.trim() !== "").length}</span>
            </summary>
            {s.lines.length > 0 ? <Markdown className="task-notes">{s.lines.join("\n")}</Markdown> : <p className="muted">비었음</p>}
          </details>
        ))
      )}
    </section>
  );
}
