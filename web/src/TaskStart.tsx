// 할 일 thread 연결의 창(할 일 T3, plan §8.312, t-0469 §3.4): 「바로 시작」 확인창과 「task에 잇기」 고르기. 창 틀은 TaskDialogs의
// TaskModal(herdr-web-ui PaneRename)이다. 「바로 시작」은 host `thread_start_preview`가 `thread start --from-task`와 같은 규칙으로 채운
// 값(owner → profile·머신, 메모를 붙인 할 일 글)을 보이고, profile 고르기는 herdr-projects popup `profile_picker`(owner의 profile을 맨
// 위에)와 같다. 열린 thread가 `max_parallel_threads` 이상이면 한 번 더 확인받는다(host는 경고만 한다). 시작하면 host가 같은 요청 안에서
// 그 task 줄 끝에 thread를 잇는다(`link_task`, 화면이 본 블록 hash로).
import { useEffect, useMemo, useState } from "react";
import { Search } from "lucide-react";
import type { OwnerChoice, TaskBoard, TaskThread, TaskView, ThreadStartPreview, ThreadView } from "./gen/proto";
import { ApiError, call } from "./api";
import { Markdown } from "./Markdown";
import { TaskModal } from "./TaskDialogs";
import { linkCandidates, ownerLabel, type ThreadGroups } from "./tasksModel";

const errText = (e: unknown) => (e instanceof ApiError || e instanceof Error ? e.message : String(e));

/** 「바로 시작」 확인창 */
export function StartDialog({ task, owners, onStarted, onClose }: { task: TaskView; owners: OwnerChoice[] | null; onStarted: (v: ThreadView) => void; onClose: () => void }) {
  const [preview, setPreview] = useState<ThreadStartPreview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [profile, setProfile] = useState("");
  const [machine, setMachine] = useState("");
  // 고른 저장소(빈 글이면 저장소 없는 작업 폴더)
  const [repo, setRepo] = useState("");
  const [overLimit, setOverLimit] = useState(false);
  useEffect(() => {
    let live = true;
    call({ op: "thread_start_preview", from_task: task.title, task_block: task.block_sha256 })
      .then((r) => {
        if (!live || r.kind !== "thread_start_preview") return;
        const { kind: _kind, ...p } = r;
        setPreview(p);
        setProfile(p.profile);
        // owner가 머신을 정했으면 그 머신의 저장소, 아니면 첫 저장소(host 기본: 저장소가 있으면 worktree)
        const first = p.repos.find((x) => p.machine === null || x.machine === p.machine) ?? null;
        setRepo(first?.path ?? "");
        setMachine(p.machine ?? first?.machine ?? "");
      })
      .catch((e: unknown) => live && setError(errText(e)));
    return () => {
      live = false;
    };
  }, [task.title, task.block_sha256]);
  const repoMachine = preview?.repos.find((x) => x.path === repo)?.machine ?? null;
  // 저장소가 머신을 정하면 그 머신이다(다른 머신에서 그 경로를 찾지 않는다)
  const where = repoMachine ?? machine;
  const machines = (owners ?? []).filter((o) => o.connected);
  const profiles = useMemo(() => {
    const on = where ? machines.filter((o) => o.machine === where) : machines;
    const names = [...new Set(on.flatMap((o) => o.profiles))];
    if (profile && !names.includes(profile)) names.unshift(profile);
    return names;
  }, [machines, where, profile]); // eslint-disable-line react-hooks/exhaustive-deps
  const over = preview !== null && preview.open_threads >= preview.max_parallel_threads;
  const start = async () => {
    if (!preview || busy || (over && !overLimit)) return;
    setBusy(true);
    setError(null);
    try {
      const r = await call({
        op: "thread_start",
        holds: [],
        background: true,
        title: "",
        machine: where || null,
        profile,
        kind: repo ? "worktree" : "dir",
        repo: repo || null,
        base: null,
        task: "",
        from_task: preview.title,
        link_task: true,
        task_block: preview.task_block,
      });
      if (r.kind !== "thread") throw new Error(`예상하지 못한 응답: ${r.kind}`);
      const { kind: _kind, ...view } = r;
      onStarted(view);
    } catch (e) {
      setError(errText(e));
      setBusy(false);
    }
  };
  const owner = ownerLabel(task);
  return (
    <TaskModal label={`「${task.title}」 바로 시작`} busy={busy} onClose={onClose} wide testid="task-start">
      <form
        className="task-start-form"
        onSubmit={(e) => {
          e.preventDefault();
          void start();
        }}
        data-start-state={preview ? (busy ? "starting" : "ready") : error ? "failed" : "loading"}
      >
        <header className="modal-header">
          <h2 className="modal-title">thread로 바로 시작</h2>
        </header>
        <div className="modal-body task-start-body">
          <p className="task-start-title">
            <strong>{task.title}</strong>
            {owner && <span className="task-chip task-owner">owner · {owner}</span>}
          </p>
          {!preview && !error && <p className="muted">시작할 값을 읽는 중</p>}
          {preview && (
            <>
              <div className="task-field-row">
                <label className="task-field">
                  <span>저장소</span>
                  <select className="input" value={repo} disabled={busy} onChange={(e) => setRepo(e.target.value)} data-testid="task-start-repo">
                    {preview.repos.map((x) => (
                      <option key={`${x.machine ?? ""}:${x.path}`} value={x.path}>
                        {x.path}
                        {x.machine ? ` @${x.machine}` : ""}(worktree)
                      </option>
                    ))}
                    <option value="">저장소 없음(작업 폴더)</option>
                  </select>
                </label>
                <label className="task-field">
                  <span>머신</span>
                  <select className="input" value={where} disabled={busy || repoMachine !== null} onChange={(e) => setMachine(e.target.value)} data-testid="task-start-machine">
                    <option value="">host 머신</option>
                    {machines.map((o) => (
                      <option key={o.machine} value={o.machine}>
                        {o.machine}
                      </option>
                    ))}
                    {where && !machines.some((o) => o.machine === where) && <option value={where}>{where}(끊김)</option>}
                  </select>
                </label>
                <label className="task-field">
                  <span>profile</span>
                  <select className="input" value={profile} disabled={busy} onChange={(e) => setProfile(e.target.value)} data-testid="task-start-profile">
                    {profiles.length === 0 && <option value="">기본 profile</option>}
                    {profiles.map((p) => (
                      <option key={p} value={p}>
                        {p}
                      </option>
                    ))}
                  </select>
                </label>
              </div>
              {preview.warnings.map((w) => (
                <p key={w} className="banner task-start-warning" role="status">
                  {w}
                </p>
              ))}
              {over && (
                <label className="banner task-start-limit" data-testid="task-start-limit">
                  <input type="checkbox" checked={overLimit} disabled={busy} onChange={(e) => setOverLimit(e.target.checked)} data-testid="task-start-over" />
                  <span>
                    열린 thread가 {preview.open_threads}개다(한도 {preview.max_parallel_threads}개). 시작하면 한도를 넘긴다
                  </span>
                </label>
              )}
              <section className="task-start-preview" aria-label="thread에 줄 할 일">
                <h3>thread에 줄 할 일</h3>
                <div className="task-start-task" data-testid="task-start-task">
                  <Markdown className="task-notes">{preview.task}</Markdown>
                </div>
              </section>
              <p className="muted task-start-note">시작하면 이 task 줄 끝에 thread 번호를 붙이고 「진행 중」으로 보인다. thread가 끝나면 완료(PR 병합·보고 [완료])일 때 저절로 지운다.</p>
            </>
          )}
          {error && (
            <p className="task-form-error" role="alert" data-testid="task-start-error">
              {error}
            </p>
          )}
        </div>
        <footer className="modal-footer">
          <button type="button" className="btn" disabled={busy} onClick={onClose}>
            취소
          </button>
          <button className="btn btn-primary" disabled={!preview || busy || (over && !overLimit)} data-testid="task-start-go">
            {busy ? "시작하는 중" : over ? "한도를 넘겨 시작" : "시작"}
          </button>
        </footer>
      </form>
    </TaskModal>
  );
}

/** 「task에 잇기」: task 없는 열린 thread를 이을 task를 고른다(형식 어긋남·열린 thread와 이어진 task는 빠진다) */
export function LinkDialog({ thread, board, groups, onLink, onClose }: { thread: TaskThread; board: TaskBoard; groups: ThreadGroups; onLink: (t: TaskView) => Promise<void>; onClose: () => void }) {
  const [query, setQuery] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const choices = linkCandidates(board, groups, query);
  const pick = (t: TaskView) => {
    if (busy) return;
    setBusy(true);
    setError(null);
    void onLink(t).then(onClose, (e: unknown) => {
      setError(errText(e));
      setBusy(false);
    });
  };
  return (
    <TaskModal label={`${thread.id} task에 잇기`} busy={busy} onClose={onClose} testid="task-link">
      <header className="modal-header">
        <h2 className="modal-title">
          {thread.id} · {thread.title}
        </h2>
      </header>
      <div className="modal-body task-link-body">
        <p className="muted">이 thread를 이을 task를 고른다. 그 줄 끝에 「 · {thread.id}」가 붙는다.</p>
        <label className="tasks-search">
          <Search aria-hidden="true" />
          <input className="input" type="search" value={query} autoFocus placeholder="제목·메모 검색" aria-label="이을 task 검색" data-testid="task-link-search" onChange={(e) => setQuery(e.target.value)} />
        </label>
        <ul className="task-link-list">
          {choices.length === 0 && <li className="muted">이을 수 있는 task가 없다</li>}
          {choices.map((t) => (
            <li key={`${t.line}:${t.title}`}>
              <button type="button" className="task-link-pick" disabled={busy} data-testid="task-link-pick" data-title={t.title} onClick={() => pick(t)}>
                <span className="task-title">{t.title}</span>
                <span className="muted">{t.list || "(목록 없음)"}</span>
                {t.thread && <span className="task-chip is-dim">지난 {t.thread}</span>}
              </button>
            </li>
          ))}
        </ul>
        {error && (
          <p className="task-form-error" role="alert">
            {error}
          </p>
        )}
      </div>
      <footer className="modal-footer">
        <button type="button" className="btn" disabled={busy} onClick={onClose}>
          닫기
        </button>
      </footer>
    </TaskModal>
  );
}
