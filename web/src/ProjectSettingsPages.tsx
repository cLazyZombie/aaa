// 설정 창의 「이 aaa」·「저장소」(관리 GUI W4, plan §8.343). host가 맡은 프로젝트 하나의 설정(`aaa.toml`, 없으면 `project.toml`)을
// `project_settings`로 읽고, 값 하나씩 `project_set`(CLI `aaa project set`과 같은 key)으로 고친다. 저장소 고치기는
// `project_repo_edit` 한 번이라 차례가 그대로다. 멈춤·재개는 `project_lifecycle`, 정리는 미리보기를 본 뒤 확인 창으로 실행한다.
// 검사·거부(범위·허용 profile·없는 경로)는 host가 한다. 보기 권한 기기는 값만 본다.
import "./ThreadActions.css";

import { useCallback, useEffect, useRef, useState } from "react";
import type { DeviceRole, LibraryPrunePlan, MachineView, ProfileView, ProjectSettingsView } from "./gen/proto";
import { ApiError, call, me } from "./api";
import { ConfirmDialog } from "./ConfirmDialog";
import { KeyRotate, LinkList } from "./AdminPages";
import { SettingsGroup, SettingsRow, Toggle } from "./herdr/SettingsControls.tsx";
import { canOperate, checkSetting, parseRepoArg, repoArg, repoGroups, REPO_DIR } from "./operateModel.ts";

const say = (e: unknown) => (e instanceof ApiError ? e.message : e instanceof Error ? e.message : String(e));
const fmtSize = (n: number) => (n >= 1 << 30 ? `${(n / (1 << 30)).toFixed(1)}GB` : n >= 1 << 20 ? `${(n / (1 << 20)).toFixed(1)}MB` : n >= 1024 ? `${(n / 1024).toFixed(1)}KB` : `${n}B`);
const STATUS_WORD: Record<string, string> = { active: "돌고 있음", paused: "멈춤", archived: "보관" };

/** 설정 읽기: 옛 host(1.77 전)는 `old`다. 겹친 읽기는 마지막 것만 반영하고, 값을 가진 뒤의 실패는 `error`로 함께 보인다 */
function useSettings(): { view: ProjectSettingsView | null; state: "loading" | "ready" | "old" | "error"; error: string | null; reload: () => void } {
  const [view, setView] = useState<ProjectSettingsView | null>(null);
  const [state, setState] = useState<"loading" | "ready" | "old" | "error">("loading");
  const [error, setError] = useState<string | null>(null);
  const seq = useRef(0);
  const reload = useCallback(() => {
    const mine = ++seq.current;
    me()
      .then((m) => {
        if (mine !== seq.current) return;
        if (!canOperate(m?.front_api)) {
          setState("old");
          return;
        }
        return call({ op: "project_settings" }).then((r) => {
          if (mine !== seq.current || r.kind !== "project_settings") return;
          const { kind: _, ...v } = r;
          setView(v);
          setError(null);
          setState("ready");
        });
      })
      .catch((e: unknown) => {
        if (mine !== seq.current) return;
        setError(say(e));
        setState("error");
      });
  }, []);
  useEffect(reload, [reload]);
  return { view, state, error, reload };
}

/** 값을 가진 뒤 다시 읽기가 실패했다: 보이는 값이 옛 것일 수 있다 */
function Stale({ error }: { error: string | null }) {
  return error ? (
    <p className="op-warn" role="alert" data-testid="settings-stale">
      설정을 다시 읽지 못했다(보이는 값이 옛 것일 수 있다): {error}
    </p>
  ) : null;
}

function Unavailable({ state, error }: { state: string; error: string | null }) {
  if (state === "loading") return <p className="op-muted">읽는 중…</p>;
  if (state === "old") return <p className="op-muted">이 host는 설정 읽기를 모른다. host를 새 판으로 바꾸면 보인다(지금은 `aaa project set`).</p>;
  return <p className="op-warn">{error ?? "읽지 못했다"}</p>;
}

/** 값 하나: 고치면 「저장」이 생긴다. 실패는 그 줄 아래에 */
function SettingText({ id, label, description, value, canEdit, kind = "text", check, onSave, options }: {
  id: string;
  label: string;
  description?: string;
  value: string;
  canEdit: boolean;
  kind?: "text" | "number" | "select";
  check?: (v: string) => string | null;
  onSave: (v: string) => Promise<void>;
  options?: string[];
}) {
  const [draft, setDraft] = useState(value);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => setDraft(value), [value]);
  const bad = check ? check(draft) : null;
  const changed = draft.trim() !== value.trim();
  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      await onSave(draft.trim());
    } catch (e) {
      setError(say(e));
    } finally {
      setBusy(false);
    }
  };
  const input =
    kind === "select" ? (
      <select id={id} value={draft} disabled={!canEdit || busy} onChange={(e) => setDraft(e.target.value)} data-testid={id}>
        {[...new Set([value, ...(options ?? [])])].map((o) => (
          <option key={o} value={o}>
            {o === "" ? "(정하지 않음 · 기본 사슬)" : o}
          </option>
        ))}
      </select>
    ) : (
      <input id={id} type={kind} value={draft} disabled={!canEdit || busy} onChange={(e) => setDraft(e.target.value)} data-testid={id} inputMode={kind === "number" ? "numeric" : undefined} />
    );
  return (
    <SettingsRow label={label} description={error ?? bad ?? description} htmlFor={id} wide>
      <div className="op-row">
        <div className="op-field">{input}</div>
        {canEdit && changed && (
          <button type="button" className="btn btn-primary" disabled={busy || bad !== null} onClick={() => void save()} data-testid={`${id}-save`}>
            {busy ? "저장 중…" : "저장"}
          </button>
        )}
      </div>
    </SettingsRow>
  );
}

export function ThisAaaPage({ role }: { role: DeviceRole }) {
  const { view, state, error, reload } = useSettings();
  const canEdit = role !== "reader";
  const [profiles, setProfiles] = useState<string[]>([]);
  const [rowError, setRowError] = useState<string | null>(null);
  const [ask, setAsk] = useState<null | { title: string; body: string; action: string; run: () => Promise<void> }>(null);
  const [sweep, setSweep] = useState<string[] | null>(null);
  const [prune, setPrune] = useState<LibraryPrunePlan | null>(null);
  const [cleanBusy, setCleanBusy] = useState<string | null>(null);
  const [cleanNote, setCleanNote] = useState<string | null>(null);
  const [note, setNote] = useState("");
  const [noteError, setNoteError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    call({ op: "profiles", machine: null })
      .then((r) => live && r.kind === "profiles" && setProfiles(r.profiles.map((p: ProfileView) => p.name)))
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, []);
  if (!view) return <Unavailable state={state} error={error} />;
  const set = async (key: string, value: string) => {
    await call({ op: "project_set", project: view.project, key, value });
    reload();
  };
  const toggle = async (key: "nudge" | "mute", on: boolean) => {
    setRowError(null);
    try {
      await set(key, on ? "true" : "false");
    } catch (e) {
      setRowError(say(e));
    }
  };
  // 할 일 「나중」 탭이 읽는 파일(`task_notes.add|remove`, 관리 GUI W8): 경로 검사는 host가 한다
  const editNote = async (key: "task_notes.add" | "task_notes.remove", path: string) => {
    setNoteError(null);
    try {
      await set(key, path);
      if (key === "task_notes.add") setNote("");
    } catch (e) {
      setNoteError(say(e));
    }
  };
  const previewSweep = async () => {
    setCleanBusy("sweep");
    setCleanNote(null);
    try {
      const r = await call({ op: "project_sweep", project: view.project, dry_run: true });
      if (r.kind === "project_change") setSweep(r.notes);
    } catch (e) {
      setCleanNote(say(e));
    } finally {
      setCleanBusy(null);
    }
  };
  const previewPrune = async () => {
    setCleanBusy("prune");
    setCleanNote(null);
    try {
      const r = await call({ op: "library_prune_preview", project: view.project, older_than_days: null });
      if (r.kind === "library_prune_preview") {
        const { kind: _, ...plan } = r;
        setPrune(plan);
      }
    } catch (e) {
      setCleanNote(say(e));
    } finally {
      setCleanBusy(null);
    }
  };
  const sweepEmpty = sweep !== null && sweep.some((n) => n.endsWith("치울 것이 없다"));
  const paused = view.status === "paused";
  return (
    <div className="op-page" data-testid="settings-this-aaa">
      <Stale error={error} />
      <SettingsGroup title={view.project} note={`${STATUS_WORD[view.status] ?? view.status} · 설정 파일 ${view.file}`}>
        <SettingText id="set-goal" label="목표" description="thread brief 머리에 들어간다" value={view.goal} canEdit={canEdit} onSave={(v) => set("goal", v)} />
        <SettingText id="set-thread-profile" label="thread 기본 profile" description="시작할 때 profile을 고르지 않으면 쓴다(허용 목록 안에서)" kind="select" options={profiles} value={view.thread_profile} canEdit={canEdit} onSave={(v) => set("thread_profile", v)} />
        <SettingText id="set-coordinator-profile" label="coordinator profile" description="coordinator를 띄울 때 쓴다" kind="select" options={profiles} value={view.coordinator_profile} canEdit={canEdit} onSave={(v) => set("coordinator_profile", v)} />
        <SettingText id="set-max-parallel" label="동시 thread 한도" description="사용자·coordinator가 띄운 thread를 센다. 자동화·병합 대기와 착지 한 자리는 제외한다. 다음 단계 작업은 다시 센다" kind="number" check={(v) => checkSetting("max_parallel_threads", v)} value={String(view.max_parallel_threads)} canEdit={canEdit} onSave={(v) => set("max_parallel_threads", v)} />
        <SettingText id="set-auto-resolve" label="쉼 뒤 자동 끝내기(일)" description="쉼으로 이만큼 지난 thread를 마지막 사본 뒤 끝낸다. 0이면 하지 않는다" kind="number" check={(v) => checkSetting("auto_resolve_days", v)} value={String(view.auto_resolve_days)} canEdit={canEdit} onSave={(v) => set("auto_resolve_days", v)} />
        <SettingsRow label="coordinator에 묶음 알림(nudge)" description="처음 보는 알림함 항목 묶음을 coordinator에 한 줄로 알린다">
          <Toggle label="coordinator에 묶음 알림" checked={view.nudge} disabled={!canEdit} onChange={(on) => void toggle("nudge", on)} />
        </SettingsRow>
        <SettingsRow label="밖으로 가는 알림 끄기(mute)" description="webhook·웹 푸시를 오류·머신 이상만 남기고 끈다. 알림함과 앱 안 알림은 그대로다">
          <Toggle label="밖으로 가는 알림 끄기" checked={view.mute} disabled={!canEdit} onChange={(on) => void toggle("mute", on)} />
        </SettingsRow>
        {rowError && <p className="op-warn settings-item">{rowError}</p>}
      </SettingsGroup>
      <SettingsGroup title="할 일 「나중」 파일" note="할 일의 「나중」 탭이 읽기만 하는 Markdown 파일. 프로젝트 폴더 안 상대 경로다.">
        <div className="settings-item op-section" data-testid="task-notes">
          {view.task_notes.length === 0 ? (
            <p className="op-muted">정한 파일이 없다</p>
          ) : (
            <ul className="op-prune">
              {view.task_notes.map((n) => (
                <li key={n} data-note={n}>
                  <span className="mono">{n}</span>
                  {canEdit && (
                    <button type="button" className="btn small" onClick={() => void editNote("task_notes.remove", n)} aria-label={`${n} 빼기`}>
                      빼기
                    </button>
                  )}
                </li>
              ))}
            </ul>
          )}
          {canEdit && (
            <div className="op-row">
              <div className="op-field">
                <input value={note} placeholder="memory/later.md" aria-label="더할 「나중」 파일" onChange={(e) => setNote(e.target.value)} data-testid="task-notes-path" />
              </div>
              <button type="button" className="btn" disabled={note.trim() === ""} onClick={() => void editNote("task_notes.add", note.trim())} data-testid="task-notes-add">
                더하기
              </button>
            </div>
          )}
          {noteError && (
            <p className="op-warn" role="alert">
              {noteError}
            </p>
          )}
        </div>
      </SettingsGroup>
      <SettingsGroup title="멈춤" note="멈추면 ticker가 이 프로젝트를 건너뛰고 thread 시작을 거부한다. 도는 agent는 멈추지 않는다.">
        <SettingsRow label={paused ? "멈춰 있다" : "돌고 있다"}>
          {canEdit && view.status !== "archived" && (
            <button
              type="button"
              className="btn"
              data-testid="project-pause"
              onClick={() =>
                setAsk({
                  title: paused ? "다시 돌리기" : "멈추기",
                  body: paused ? `${view.project}를 다시 돌린다. ticker와 thread 시작이 돌아온다.` : `${view.project}를 멈춘다. ticker가 건너뛰고 thread 시작을 거부한다. 도는 agent는 멈추지 않는다.`,
                  action: paused ? "다시 돌리기" : "멈추기",
                  run: async () => {
                    await call({ op: "project_lifecycle", project: view.project, action: paused ? "resume" : "pause" });
                    reload();
                  },
                })
              }
            >
              {paused ? "다시 돌리기" : "멈추기"}
            </button>
          )}
        </SettingsRow>
      </SettingsGroup>
      <SettingsGroup title="정리" note="미리 보고, 확인 창에서 실행한다.">
        <div className="settings-item op-section" data-testid="sweep-section">
          <h3>끝난 thread 자리</h3>
          <p className="op-muted">끝난 thread가 남긴 worktree·병합된 branch를 그 머신에서 치운다(force 없음).</p>
          <div className="op-buttons">
            <button type="button" className="btn" disabled={cleanBusy !== null} onClick={() => void previewSweep()} data-testid="sweep-preview">
              {cleanBusy === "sweep" ? "보는 중…" : "미리 보기"}
            </button>
            {canEdit && sweep !== null && !sweepEmpty && (
              <button
                type="button"
                className="btn btn-danger"
                data-testid="sweep-run"
                onClick={() =>
                  setAsk({
                    title: "끝난 thread 자리 치우기",
                    body: `미리 본 ${sweep.length}줄을 치운다:\n${sweep.join("\n")}`,
                    action: "치우기",
                    run: async () => {
                      const r = await call({ op: "project_sweep", project: view.project, dry_run: false });
                      setSweep(null);
                      setCleanNote(r.kind === "project_change" ? r.notes.join("\n") : "치웠다");
                    },
                  })
                }
              >
                치우기
              </button>
            )}
          </div>
          {sweep !== null && (
            <ul className="op-prune" data-testid="sweep-notes">
              {sweep.map((n) => (
                <li key={n}>{n}</li>
              ))}
            </ul>
          )}
        </div>
        <div className="settings-item op-section" data-testid="prune-section">
          <h3>보고서 사본</h3>
          <p className="op-muted">기한·용량을 넘은 host 보고서 사본을 지운다. report.md 사본과 thread 기록은 남는다.</p>
          <div className="op-buttons">
            <button type="button" className="btn" disabled={cleanBusy !== null} onClick={() => void previewPrune()} data-testid="prune-preview">
              {cleanBusy === "prune" ? "보는 중…" : "미리 보기"}
            </button>
            {canEdit && prune !== null && prune.candidates.length > 0 && (
              <button
                type="button"
                className="btn btn-danger"
                data-testid="prune-run"
                onClick={() =>
                  setAsk({
                    title: "보고서 사본 지우기",
                    body: `미리 본 ${prune.candidates.length}개(${fmtSize(prune.candidates.reduce((a, c) => a + c.size, 0))})를 지운다. 그 사이 후보가 바뀌면 지우지 않고 다시 미리 보게 한다.`,
                    action: `${prune.candidates.length}개 지우기`,
                    run: async () => {
                      try {
                        const r = await call({ op: "library_prune_apply", project: view.project, older_than_days: null, expected: prune.candidates.map((c) => `${c.project}/${c.name}`) });
                        setPrune(null);
                        setCleanNote(r.kind === "library_pruned" ? `${r.removed.length}개를 지웠다` : "지웠다");
                      } catch (e) {
                        if (!(e instanceof ApiError && e.body.code === "conflict")) throw e;
                        // 이 창은 본 목록을 쥐고 있다: 닫고 새로 미리 본 목록에서 다시 고르게 한다
                        await previewPrune();
                        setCleanNote("미리 본 뒤 후보가 바뀌었다. 새 목록을 보고 다시 지운다");
                      }
                    },
                  })
                }
              >
                {prune.candidates.length}개 지우기
              </button>
            )}
          </div>
          {prune !== null && (
            <>
              <p className="op-muted" data-testid="prune-total">
                합계 {fmtSize(prune.size)} / {prune.max_size === 0 ? "상한 없음" : fmtSize(prune.max_size)} · 기한 {prune.max_days === 0 ? "없음" : `${prune.max_days}일`} · 정리 뒤 {fmtSize(prune.remaining_size)}
              </p>
              {prune.candidates.length === 0 ? (
                <p className="op-muted">지울 사본이 없다</p>
              ) : (
                <ul className="op-prune" data-testid="prune-candidates">
                  {prune.candidates.map((c) => (
                    <li key={`${c.project}/${c.name}`}>
                      <span className="mono">{c.name}</span>
                      <span>
                        {fmtSize(c.size)} · {c.reason}
                      </span>
                    </li>
                  ))}
                </ul>
              )}
            </>
          )}
          {cleanNote && (
            <p className="op-note" role="status" data-testid="clean-note">
              {cleanNote}
            </p>
          )}
        </div>
      </SettingsGroup>
      {canEdit && (
        <SettingsGroup title="공유 링크">
          <div className="settings-item">
            <LinkList />
            <KeyRotate />
          </div>
        </SettingsGroup>
      )}
      {ask && (
        <ConfirmDialog
          title={ask.title}
          body={ask.body}
          confirmLabel={ask.action}
          onConfirm={async () => {
            await ask.run();
            setAsk(null);
          }}
          onClose={() => setAsk(null)}
        />
      )}
    </div>
  );
}

export function ReposPage({ role }: { role: DeviceRole }) {
  const { view, state, error, reload } = useSettings();
  const canEdit = role !== "reader";
  const [machines, setMachines] = useState<MachineView[]>([]);
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [addPath, setAddPath] = useState("");
  const [addMachine, setAddMachine] = useState("");
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [ask, setAsk] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    call({ op: "machines" })
      .then((r) => live && r.kind === "machines" && setMachines(r.machines))
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, []);
  if (!view) return <Unavailable state={state} error={error} />;
  const run = async (work: () => Promise<unknown>) => {
    setBusy(true);
    setMsg(null);
    try {
      await work();
      reload();
      return true;
    } catch (e) {
      setMsg(say(e));
      return false;
    } finally {
      setBusy(false);
    }
  };
  const local = view.local_machine;
  const names = machines.filter((m) => !m.local).map((m) => m.name);
  const groups = repoGroups(view.repos, local);
  return (
    <div className="op-page" data-testid="settings-repos">
      <Stale error={error} />
      <SettingsGroup title="저장소" note="할 일의 저장소 칸이 고르는 목록이다. 이름(경로의 마지막 폴더)이 같으면 한 저장소의 머신별 자리다.">
        <div className="settings-item">
          <ul className="op-repos" data-testid="repo-list">
            {groups.map((g) => (
              <li key={g.name} data-testid={`repo-group-${g.name}`}>
                <span className="op-repo-name">{g.name}</span>
                {g.rows.map(({ repo, machine }) => {
                  const arg = repoArg(repo);
                  return (
                    <div key={arg}>
                      <div className="op-repo-row" data-testid="repo-row">
                        <span className="op-repo-machine">{machine ?? "host 머신"}</span>
                        <span className="mono">{repo.path}</span>
                        {canEdit && (
                          <span className="op-buttons">
                            <button type="button" className="btn small" disabled={busy} onClick={() => { setEditing(arg); setDraft(arg); }} aria-label={`${arg} 고치기`}>
                              고치기
                            </button>
                            <button type="button" className="btn small" disabled={busy} onClick={() => setAsk(arg)} aria-label={`${arg} 빼기`}>
                              빼기
                            </button>
                          </span>
                        )}
                      </div>
                      {editing === arg && (
                        <form
                          className="op-repo-edit"
                          onSubmit={(e) => {
                            e.preventDefault();
                            void run(() => call({ op: "project_repo_edit", project: view.project, from: arg, to: draft })).then((ok) => ok && setEditing(null));
                          }}
                        >
                          <input value={draft} onChange={(e) => setDraft(e.target.value)} aria-label="저장소 경로(PATH@MACHINE)" disabled={busy} data-testid="repo-edit-input" />
                          <span className="op-buttons">
                            <button type="submit" className="btn btn-primary" disabled={busy || !parseRepoArg(draft).path || draft.trim() === arg}>
                              저장
                            </button>
                            <button type="button" className="btn" disabled={busy} onClick={() => setEditing(null)}>
                              취소
                            </button>
                          </span>
                        </form>
                      )}
                    </div>
                  );
                })}
              </li>
            ))}
            <li data-testid="repo-workdir">
              <span className="op-repo-name">{REPO_DIR}(저장소 없음)</span>
              <p className="op-muted">할 일의 저장소 칸에서 고르면 저장소 없이 그 머신의 작업 폴더에서 일한다. 고칠 것이 없다.</p>
            </li>
          </ul>
        </div>
        {canEdit && (
          <form
            className="settings-item op-section"
            data-testid="repo-add"
            onSubmit={(e) => {
              e.preventDefault();
              const path = addPath.trim();
              if (!path) return;
              void run(() => call({ op: "project_set", project: view.project, key: "repos.add", value: addMachine ? `${path}@${addMachine}` : path })).then((ok) => ok && setAddPath(""));
            }}
          >
            <h3>저장소 더하기</h3>
            <div className="op-repo-edit">
              <label className="op-field">
                <span>그 머신의 경로</span>
                <input value={addPath} onChange={(e) => setAddPath(e.target.value)} placeholder="/work/app" disabled={busy} data-testid="repo-add-path" />
              </label>
              <label className="op-field">
                <span>머신</span>
                <select value={addMachine} onChange={(e) => setAddMachine(e.target.value)} disabled={busy} data-testid="repo-add-machine">
                  <option value="">{local ? `${local}(host 머신)` : "host 머신"}</option>
                  {names.map((n) => (
                    <option key={n} value={n}>
                      {n}
                    </option>
                  ))}
                </select>
              </label>
            </div>
            <div className="op-buttons">
              <button type="submit" className="btn btn-primary" disabled={busy || !addPath.trim()} data-testid="repo-add-save">
                더하기
              </button>
            </div>
          </form>
        )}
        {msg && (
          <p className="op-warn settings-item" role="alert" data-testid="repo-error">
            {msg}
          </p>
        )}
      </SettingsGroup>
      {ask && (
        <ConfirmDialog
          title="저장소 빼기"
          body={`${ask}를 저장소 목록에서 뺀다. 폴더와 그 안의 것은 그대로다. 이 저장소로 정한 할 일은 「(목록에 없음)」으로 보인다.`}
          confirmLabel="빼기"
          onConfirm={async () => {
            await call({ op: "project_set", project: view.project, key: "repos.remove", value: ask });
            setAsk(null);
            reload();
          }}
          onClose={() => setAsk(null)}
        />
      )}
    </div>
  );
}
