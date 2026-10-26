// 할 일 thread 연결의 창(할 일 T3, plan §8.316, t-0469 §3.4): 「바로 시작」 확인창과 「task에 잇기」 고르기. 창 틀은 TaskDialogs의
// TaskModal(herdr-web-ui PaneRename)이다. 「바로 시작」은 host `thread_start_preview`가 `thread start --from-task`와 같은 규칙으로 채운
// 값(owner → profile·머신, 메모를 붙인 할 일 글)을 보이고, profile 고르기는 herdr-projects popup `profile_picker`(owner의 profile을 맨
// 위에)와 같다. 자동화·병합 대기·착지 한 자리를 뺀 열린 Worker가 `max_parallel_threads` 이상이면 예약과 사용자가 확인한 한도 무시 시작을 안내한다. 시작하면 host가 같은 요청 안에서
// 그 task 줄 끝에 thread를 잇는다(`link_task`, 화면이 본 블록 hash로).
import { useEffect, useMemo, useRef, useState } from "react";
import { Search } from "lucide-react";
import type { AccountStartView, FrontRequest, OwnerChoice, TaskBoard, TaskThread, TaskView, ThreadStartPreview, ThreadView } from "./gen/proto";
import { ApiError, call, me } from "./api";
import { apiAtLeast } from "./automationModel";
import { Markdown } from "./Markdown";
import { TaskModal } from "./TaskDialogs";
import { WhoChips } from "./TaskDetail";
import { defaultMachine, docLinkCandidates, linkCandidates, ownerFixes, REPO_DIR, repoMachines, type TaskAttachment, type ThreadGroups } from "./tasksModel";

const errText = (e: unknown) => (e instanceof ApiError || e instanceof Error ? e.message : String(e));

/** 「바로 시작」 확인창 */
export function StartDialog({ project, task, owners, allowOverLimit, onStarted, onClose }: { project: string; task: TaskView; allowOverLimit: boolean; owners: OwnerChoice[] | null; onStarted: (v: ThreadView) => void; onClose: () => void }) {
  const requestId = useRef(crypto.randomUUID());
  const [poolEnabled, setPoolEnabled] = useState(false);
  const [waiting, setWaiting] = useState<AccountStartView | null>(null);
  const [preview, setPreview] = useState<ThreadStartPreview | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let active = true;
    void me().then(async (m) => {
      if (!apiAtLeast(m?.front_api, 1, 102)) return;
      const r = await call({ op: "account_pool" });
      if (active && r.kind === "account_pool") setPoolEnabled(apiAtLeast(m?.front_api, 1, 129) ? r.pool.members.length > 0 : r.pool.projects.includes(project));
    })
      .catch((e) => active && setError(errText(e)));
    return () => { active = false; };
  }, [project]);
  const [busy, setBusy] = useState(false);
  const [confirmOver, setConfirmOver] = useState(false);
  const [profile, setProfile] = useState("");
  // 사용자가 고른 머신(없으면 기본값: owner의 머신 → 덜 붐비는 머신)
  const [picked, setPicked] = useState<string | null>(null);
  // 저장소 칸(plan §8.330): 저장소 이름이나 `작업 폴더`. task 칸이 비면 빈 글로 시작해 고를 때까지 「시작」을 막는다
  const [choice, setChoice] = useState("");
  useEffect(() => {
    let live = true;
    call({ op: "thread_start_preview", from_task: task.short_id ?? task.title, task_block: task.block_sha256 })
      .then((r) => {
        if (!live || r.kind !== "thread_start_preview") return;
        const { kind: _kind, ...p } = r;
        setPreview(p);
        setProfile(p.profile);
        setChoice(p.repo ?? "");
      })
      .catch((e: unknown) => live && setError(errText(e)));
    return () => {
      live = false;
    };
  }, [task.title, task.block_sha256]);
  // task의 모델·머신 칸이 정한 값은 고르지 못한다(host가 다른 값을 거부한다)
  const fixed = ownerFixes(task);
  const machines = (owners ?? []).filter((o) => o.connected);
  const byName = useMemo(() => (preview ? repoMachines(preview) : new Map<string, { machine: string; path: string }[]>()), [preview]);
  // 고른 저장소가 있는 머신(작업 폴더는 연결된 모든 머신)
  const cands = choice === REPO_DIR ? machines.map((o) => o.machine) : (byName.get(choice) ?? []).map((x) => x.machine);
  const ownerMachine = fixed.machine ? (preview?.machine ?? null) : null;
  const allowed = preview?.workspace_access;
  const autoCands = allowed ? cands.filter(m => allowed[m]?.includes(choice)) : cands;
  const explicitMachine = ownerMachine ?? (picked && cands.includes(picked) ? picked : null);
  const where = explicitMachine ?? defaultMachine(autoCands, preview?.machine_order ?? []);
  const accessWarning = explicitMachine && allowed && !allowed[explicitMachine]?.includes(choice)
    ? "이 머신의 자동 배정 허용은 꺼져 있습니다. 고정 머신 지정을 따라 시작합니다." : null;
  // 고른 머신의 그 저장소 경로. 한 머신에 같은 이름이 둘이면 이름을 보내 host가 까닭과 함께 거부하게 한다(말없이 고르지 않는다)
  const onWhere = (byName.get(choice) ?? []).filter((x) => x.machine === where);
  const repoPath = onWhere.length === 1 ? onWhere[0].path : choice;
  const problem =
    choice === ""
      ? "저장소나 작업 폴더를 고른다"
      : choice !== REPO_DIR && !byName.has(choice)
        ? `project.toml repos에 없는 저장소다: ${choice}`
        : ownerMachine !== null && choice !== REPO_DIR && !cands.includes(ownerMachine)
          ? `task 머신 ${ownerMachine}에 ${choice} 저장소가 없다(project.toml repos)`
          : explicitMachine === null && autoCands.length === 0 && choice !== ""
            ? "자동 배정이 허용된 머신이 없습니다. 기기 정보에서 workspace 허용을 켜거나 머신을 직접 고릅니다."
            : null;
  const profiles = useMemo(() => {
    const on = where ? machines.filter((o) => o.machine === where) : machines;
    const names = [...new Set(on.flatMap((o) => o.profiles))];
    if (profile && !names.includes(profile)) names.unshift(profile);
    return names;
  }, [machines, where, profile]); // eslint-disable-line react-hooks/exhaustive-deps
  const over = preview !== null && preview.open_threads >= preview.max_parallel_threads;
  const start = async (overLimit = false) => {
    if (!preview || busy || waiting || problem !== null || (over && !overLimit) || (overLimit && (!allowOverLimit || !confirmOver))) return;
    setBusy(true);
    setError(null);
    try {
      const request: FrontRequest = {
        op: "thread_start",
        project,
        over_limit: overLimit,
        holds: [],
        background: true,
        title: "",
        machine: explicitMachine,
        profile,
        kind: choice === REPO_DIR ? "dir" : "worktree",
        repo: choice === REPO_DIR ? null : explicitMachine ? repoPath : choice,
        base: null,
        task: "",
        from_task: task.short_id ?? task.title,
        link_task: true,
        task_block: preview.task_block,
      };
      const r = await call(poolEnabled ? { op: "thread_start_pool", id: requestId.current, size: "medium", start: request } : request);
      if (r.kind === "account_start") { setWaiting(r.start); setBusy(false); return; }
      if (r.kind !== "thread") throw new Error(`예상하지 못한 응답: ${r.kind}`);
      const { kind: _kind, ...view } = r;
      onStarted(view);
    } catch (e) {
      setError(errText(e));
      setBusy(false);
    }
  };
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
            <strong>{task.short_id && <span className="task-short-id">{task.short_id}</span>}{task.title}</strong>
            <WhoChips t={task} labeled />
          </p>
          {!preview && !error && <p className="muted">시작할 값을 읽는 중</p>}
          {waiting && <div role="status" data-testid="task-account-wait"><p>{waiting.reason}</p><p>배정 요청 {waiting.id} · 설정 → 계정에서 상태를 확인할 수 있습니다.</p>{waiting.state === "queued" && <button type="button" className="btn" disabled={busy} onClick={async () => {
            setBusy(true);
            try { await call({ op: "account_start_cancel", project, id: waiting.id }); onClose(); }
            catch (e) { setError(errText(e)); }
            finally { setBusy(false); }
          }}>배정 대기 취소</button>}</div>}
          {preview && (
            <>
              <div className="task-field-row">
                <label className="task-field">
                  <span>저장소</span>
                  <select
                    className="input"
                    value={choice}
                    disabled={busy || confirmOver || !!preview.repo}
                    onChange={(e) => {
                      setChoice(e.target.value);
                      setPicked(null);
                    }}
                    data-testid="task-start-repo"
                  >
                    {choice === "" && <option value="">고르기…</option>}
                    {[...byName.keys()].map((name) => (
                      <option key={name} value={name}>
                        {name}(worktree)
                      </option>
                    ))}
                    {choice !== "" && choice !== REPO_DIR && !byName.has(choice) && <option value={choice}>{choice}(project.toml에 없음)</option>}
                    <option value={REPO_DIR}>작업 폴더(저장소 없음)</option>
                  </select>
                </label>
                <label className="task-field">
                  <span>머신</span>
                  <select className="input" value={explicitMachine ?? ""} disabled={busy || confirmOver || ownerMachine !== null} onChange={(e) => setPicked(e.target.value)} data-testid="task-start-machine">
                    <option value="">자동{where ? ` · ${where}` : ""}</option>
                    {machines
                      .filter((o) => choice === "" || cands.includes(o.machine))
                      .map((o) => (
                        <option key={o.machine} value={o.machine}>
                          {o.machine}
                          {o.machine === defaultMachine(cands, preview.machine_order ?? []) && cands.length > 1 ? "(덜 붐빔)" : ""}
                        </option>
                      ))}
                    {where && !machines.some((o) => o.machine === where) && <option value={where}>{where}(끊김)</option>}
                  </select>
                </label>
                <label className="task-field">
                  <span>모델</span>
                  <select className="input" value={profile} disabled={busy || confirmOver || fixed.profile} onChange={(e) => setProfile(e.target.value)} data-testid="task-start-profile">
                    {profiles.length === 0 && <option value="">기본 profile</option>}
                    {profiles.map((p) => (
                      <option key={p} value={p}>
                        {p}
                      </option>
                    ))}
                  </select>
                </label>
              </div>
              {problem !== null && choice !== "" && (
                <p className="banner task-start-warning" role="alert" data-testid="task-start-problem">
                  {problem}
                </p>
              )}
              {accessWarning && <p className="banner task-start-warning" role="status">{accessWarning}</p>}
              {preview.warnings.map((w) => (
                <p key={w} className="banner task-start-warning" role="status">
                  {w}
                </p>
              ))}
              {over && <p className="banner task-start-limit" data-testid="task-start-limit">열린 thread {preview.open_threads}개가 한도 {preview.max_parallel_threads}개를 사용 중이다. 상세의 「자리 나면 시작 예약」으로 예약한다.</p>}
              {confirmOver && <section className="banner task-start-warning task-start-over-confirm" role="alert" data-testid="task-start-over-confirm">
                <h3>한도를 무시하고 시작할까요?</h3>
                <p>「{task.title}」을 바로 시작합니다. 현재 확인한 열린 thread는 {preview.open_threads}개, 한도는 {preview.max_parallel_threads}개입니다. 이 thread도 열린 수에 포함하며, 한도 아래로 자리가 날 때까지 예약은 기다립니다. 누가 언제 시작했는지 기록합니다.</p>
                <div className="task-start-over-actions">
                  <button type="button" className="btn" disabled={busy} onClick={() => setConfirmOver(false)}>돌아가기</button>
                  <button type="button" className="btn btn-primary" disabled={busy || problem !== null} data-testid="task-start-over-go" onClick={() => void start(true)}>확인하고 시작</button>
                </div>
              </section>}
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
          {over && allowOverLimit && <button type="button" className="btn" disabled={busy || problem !== null || confirmOver} data-testid="task-start-over" onClick={() => setConfirmOver(true)}>한도 무시하고 시작</button>}
          <button className="btn btn-primary" disabled={!preview || busy || !!waiting || problem !== null || over} title={problem ?? undefined} data-testid="task-start-go">
            {busy ? "시작하는 중" : over ? "자리 대기" : "시작"}
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

/**
 * 「할 일에 추가」(plan §8.327 2단계): 보고서·thread 결과물의 md·html 문서를 링크한 새 task 폼을 열거나(「새 task로」), 있는 task의
 * 메모에 그 문서 첨부 줄을 붙인다(「task에 링크」). 이미 그 문서를 링크한 task와 형식 어긋난 task는 고를 수 없다
 */
export function DocAddDialog({ link, board, onNew, onLink, onClose }: { link: TaskAttachment; board: TaskBoard; onNew: () => void; onLink: (t: TaskView) => Promise<void>; onClose: () => void }) {
  const [query, setQuery] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const choices = docLinkCandidates(board, link, query);
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
    <TaskModal label="할 일에 추가" busy={busy} onClose={onClose} testid="task-docadd">
      <header className="modal-header">
        <h2 className="modal-title">할 일에 추가 · {link.label}</h2>
      </header>
      <div className="modal-body task-link-body">
        <p className="muted task-docadd-target">
          <code>{link.target}</code>
        </p>
        <button type="button" className="btn btn-primary" disabled={busy} data-testid="task-docadd-new" onClick={onNew}>
          이 문서로 새 task 만들기
        </button>
        <p className="muted">또는 이 문서를 링크할 task를 고른다. 그 task 메모 끝에 첨부 줄이 붙는다.</p>
        <label className="tasks-search">
          <Search aria-hidden="true" />
          <input className="input" type="search" value={query} placeholder="제목·메모 검색" aria-label="링크할 task 검색" data-testid="task-docadd-search" onChange={(e) => setQuery(e.target.value)} />
        </label>
        <ul className="task-link-list">
          {choices.length === 0 && <li className="muted">링크할 수 있는 task가 없다</li>}
          {choices.map((t) => (
            <li key={`${t.line}:${t.title}`}>
              <button type="button" className="task-link-pick" disabled={busy} data-testid="task-docadd-pick" data-title={t.title} onClick={() => pick(t)}>
                <span className="task-title">{t.title}</span>
                <span className="muted">{t.list || "(목록 없음)"}</span>
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
