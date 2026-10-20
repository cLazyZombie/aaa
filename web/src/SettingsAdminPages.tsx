// 관리 GUI W5c(plan §8.356): 설정 「안전」(safety 실효값·출처, 웹은 yolo·start_threads·trust_screens·coordinator_deploy만 바꾼다),
// 「profile」(profile 더하기·고치기·지우기와 허용 목록·기본값, 미리보기 → 적용), 「머신」의 작업 폴더(보기·점검·전환)와 Codex hook
// (설치·제거). host가 판정한다(조작 기기만, custom profile·명령 인자 거부, 본 판과 다르면 conflict, 절대 경로만). 웹 조작 기기가 임의
// 명령을 심지 못하게 custom profile의 실행 명령과 `*_agent_args`·`routine_commands`는 host CLI로만 바꾼다(사용자 답 1A·2A).
import "./ThreadActions.css";
import "./AdminPages.css";

import { useCallback, useEffect, useRef, useState } from "react";
import type { DeviceRole, HookView, MachineView, ProfileAdminRow, ProfileAdminView, ProfileChangeReq, ProfilePlanView, SafetyView, SettingsScope, WorkRootView } from "./gen/proto";
import { ApiError, call, me } from "./api";
import { AdminDialog } from "./AdminPages";
import { ConfirmDialog } from "./ConfirmDialog";
import { SettingsGroup } from "./herdr/SettingsControls.tsx";
import {
  SAFETY_CHOICES,
  SAFETY_WORDS,
  WEB_HARNESSES,
  absolutePath,
  allowText,
  canSettings,
  canSwitch,
  draftOf,
  effortValues,
  hasModel,
  needsYoloConfirm,
  profileChange,
  profileNameOk,
  type ProfileDraft,
} from "./settingsModel.ts";

const say = (e: unknown) => (e instanceof ApiError ? e.message : e instanceof Error ? e.message : String(e));
const conflictNote = "그사이 host 설정이 바뀌어 쓰지 않았다. 새로 읽었으니 다시 본다.";

/** 1.81 이상 host인가(모르면 null) */
function useSettingsAdmin(): boolean | null {
  const [ok, setOk] = useState<boolean | null>(null);
  useEffect(() => {
    let live = true;
    me()
      .then((m) => live && setOk(canSettings(m?.front_api)))
      .catch(() => live && setOk(false));
    return () => {
      live = false;
    };
  }, []);
  return ok;
}

function Gate({ role, admin, what }: { role: DeviceRole; admin: boolean | null; what: string }) {
  if (role === "reader") return <p className="op-muted">보기 전용 기기에는 {what} 설정을 보이지 않는다(host 설정과 실행 인자가 들어 있다).</p>;
  if (admin === false) return <p className="op-muted">이 host는 웹 {what} 관리를 모른다. host를 새 판으로 바꾸면 보인다(지금은 host 터미널의 aaa 명령).</p>;
  return null;
}

function Notes({ items, testid }: { items: string[]; testid?: string }) {
  if (items.length === 0) return null;
  return (
    <ul className="settings-notes" data-testid={testid}>
      {items.map((n) => (
        <li key={n}>{n}</li>
      ))}
    </ul>
  );
}

function ScopePick({ scope, project, onChange, disabled }: { scope: SettingsScope; project: string; onChange: (s: SettingsScope) => void; disabled?: boolean }) {
  return (
    <div className="admin-line" role="radiogroup" aria-label="고칠 표">
      {(["project", "global"] as const).map((s) => (
        <label key={s} className="op-check">
          <input type="radio" name="settings-scope" checked={scope === s} disabled={disabled} onChange={() => onChange(s)} data-testid={`scope-${s}`} />
          <span>{s === "project" ? `이 프로젝트${project ? `(${project})` : ""}` : "모든 프로젝트 기본"}</span>
        </label>
      ))}
    </div>
  );
}

// ───────────── 안전 ─────────────

export function SafetyPage({ role }: { role: DeviceRole }) {
  const admin = useSettingsAdmin();
  const [scope, setScope] = useState<SettingsScope>("project");
  const [view, setView] = useState<SafetyView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  /** 확인을 기다리는 yolo 값(on 또는 「기본」으로 켜짐) */
  const [yolo, setYolo] = useState<string | null>(null);
  const seq = useRef(0);
  const shown = role !== "reader" && admin === true;
  const load = useCallback(
    (keep = false) => {
      if (!shown) return;
      const mine = ++seq.current;
      call({ op: "safety_show", scope })
        .then((r) => {
          if (mine !== seq.current || r.kind !== "safety") return;
          setView(r);
          if (!keep) setError(null);
        })
        .catch((e: unknown) => mine === seq.current && setError(say(e)));
    },
    [shown, scope],
  );
  useEffect(() => load(), [load]);
  const set = async (key: string, value: string | null) => {
    if (!view) return;
    setBusy(true);
    setError(null);
    try {
      const r = await call({
        op: "safety_set",
        scope,
        key,
        value,
        expected: view.rev,
      });
      if (r.kind === "safety") setView(r);
    } catch (e) {
      setError(e instanceof ApiError && e.body.code === "conflict" ? conflictNote : say(e));
      load(true);
    } finally {
      setBusy(false);
    }
  };
  const gate = <Gate role={role} admin={admin} what="safety" />;
  if (!shown) return <div className="op-page">{gate}</div>;
  return (
    <div className="op-page" data-testid="settings-safety">
      {error && (
        <p className="op-warn" role="alert">
          {error}
        </p>
      )}
      <SettingsGroup title="safety" note="사람만 바꾼다(agent는 바꾸지 못한다). 웹은 아래 넷만 바꾼다. 인자·명령 목록은 host의 aaa 세션 밖 터미널에서 aaa safety set으로 바꾼다.">
        <div className="settings-item">
          <ScopePick scope={scope} project={view?.project ?? ""} onChange={setScope} disabled={busy} />
          {view && (
            <ul className="admin-list" data-testid="safety-rows">
              {view.rows.map((r) => (
                <li key={r.key} data-key={r.key}>
                  <span className="admin-name mono">{r.key}</span>
                  <span className="op-muted">{SAFETY_WORDS[r.key] ?? ""}</span>
                  <span data-testid="safety-value">
                    {r.value}
                    {r.note ? ` (${r.note})` : ""}
                  </span>
                  <span className="op-muted" data-testid="safety-source">
                    [{r.source}]
                  </span>
                  {r.editable ? (
                    <select
                      className="admin-select"
                      aria-label={`${r.key} 바꾸기`}
                      value=""
                      disabled={busy}
                      data-testid={`safety-set-${r.key}`}
                      onChange={(e) => {
                        const v = e.target.value === "default" ? null : e.target.value;
                        if (needsYoloConfirm(r.key, v, r)) setYolo(v ?? "default");
                        else void set(r.key, v);
                      }}
                    >
                      <option value="" disabled>
                        바꾸기…
                      </option>
                      {(SAFETY_CHOICES[r.key] ?? []).map((c) => (
                        <option key={c} value={c}>
                          {c}
                        </option>
                      ))}
                      <option value="default">기본(아래 층을 따른다)</option>
                    </select>
                  ) : (
                    <span className="op-muted">host 터미널에서</span>
                  )}
                </li>
              ))}
            </ul>
          )}
          {view && <Notes items={[...view.warnings.map((w) => `경고: ${w}`), ...view.notes]} testid="safety-notes" />}
          {view && (
            <p className="op-muted">
              기본 profile — thread {view.thread_profile} · coordinator {view.coordinator_profile}
            </p>
          )}
        </div>
      </SettingsGroup>
      {yolo && view && (
        <ConfirmDialog
          title={`${scope === "global" ? "모든 프로젝트" : view.project}의 yolo를 켤까요?`}
          body={`${yolo === "default" ? "이 표의 yolo를 지우면 아래 층 값(on)을 따라 켜진다. " : ""}thread를 묻지 않고 시작하고, thread·coordinator agent를 harness의 권한 건너뛰기 인자로 띄운다. agent가 묻지 않고 명령을 실행하고 파일을 고친다. 이미 도는 agent는 다시 띄울 때까지 옛 인자다.`}
          confirmLabel="yolo 켜기"
          onConfirm={async () => {
            await set("yolo", yolo === "default" ? null : "on");
            setYolo(null);
          }}
          onClose={() => setYolo(null)}
        />
      )}
    </div>
  );
}

// ───────────── profile ─────────────

export function ProfilesPage({ role }: { role: DeviceRole }) {
  const admin = useSettingsAdmin();
  const [view, setView] = useState<ProfileAdminView | null>(null);
  const [safety, setSafety] = useState<SafetyView | null>(null);
  const [scope, setScope] = useState<SettingsScope>("project");
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<{
    before: ProfileAdminRow | null;
  } | null>(null);
  const [planned, setPlanned] = useState<{
    change: ProfileChangeReq;
    plan: ProfilePlanView;
  } | null>(null);
  const [done, setDone] = useState<string[]>([]);
  const seq = useRef(0);
  const shown = role !== "reader" && admin === true;
  const load = useCallback(() => {
    if (!shown) return;
    const mine = ++seq.current;
    // 따로 읽는다: 허용 목록(프로젝트 표)을 읽지 못해도 profile 목록은 보인다
    call({ op: "profile_admin" })
      .then((p) => mine === seq.current && p.kind === "profile_admin" && setView(p))
      .catch((e: unknown) => mine === seq.current && setError(say(e)));
    call({ op: "safety_show", scope })
      .then((s) => mine === seq.current && s.kind === "safety" && setSafety(s))
      .catch((e: unknown) => mine === seq.current && setError(say(e)));
  }, [shown, scope]);
  useEffect(() => load(), [load]);
  /**
   * 미리보기를 받아 확인 창을 연다(쓰지 않는다). `base`는 이 바꾸기를 고른 화면이 읽은 host.toml 판이다: 그사이 바뀌었으면 옛 화면을
   * 바탕으로 미리 보지 않고 다시 읽는다(적용은 미리보기 판과 대조한다)
   */
  const preview = async (change: ProfileChangeReq, base: string | undefined) => {
    const r = await call({ op: "profile_plan", change });
    if (r.kind !== "profile_plan") return;
    if (base !== undefined && r.rev !== base) {
      load();
      throw new Error(conflictNote);
    }
    setPlanned({ change, plan: r });
  };
  const ask = (change: ProfileChangeReq, base: string | undefined) => {
    setError(null);
    preview(change, base).catch((e: unknown) => setError(say(e)));
  };
  if (!shown)
    return (
      <div className="op-page">
        <Gate role={role} admin={admin} what="profile" />
      </div>
    );
  const names = view?.profiles.map((p) => p.name) ?? [];
  return (
    <div className="op-page" data-testid="settings-profiles">
      {error && (
        <p className="op-warn" role="alert">
          {error}
        </p>
      )}
      <Notes items={done} testid="profile-done" />
      <SettingsGroup
        title="profile"
        note="thread·coordinator를 띄우는 harness와 인자. 웹은 claude·codex·shell profile을 더하고 고치고 지운다. custom profile(실행 명령)은 host의 aaa 세션 밖 터미널에서 aaa profile로 한다. env는 이름만 보인다."
      >
        <div className="settings-item">
          <ul className="admin-list" data-testid="profile-list">
            {view?.profiles.map((p) => (
              <li key={p.name} data-name={p.name}>
                <span className="admin-name">{p.name}</span>
                <span className="op-muted">
                  {p.harness}
                  {p.builtin ? " · 기본" : ""}
                  {p.description ? ` — ${p.description}` : ""}
                </span>
                <code className="mono admin-argv" data-testid="profile-launch">
                  {[p.harness === "custom" ? "(실행 명령)" : p.harness, ...p.launch].join(" ")}
                </code>
                {p.env_names.length > 0 && <span className="op-muted">env {p.env_names.join(", ")}</span>}
                {p.local ? (
                  <span className="op-muted">host.local.toml에서</span>
                ) : p.custom ? (
                  <span className="op-muted">host 터미널에서</span>
                ) : (
                  <span className="admin-actions">
                    <button type="button" className="btn small" onClick={() => setEditing({ before: p })} aria-label={`${p.name} 고치기`}>
                      고치기
                    </button>
                    {!p.builtin && (
                      <button type="button" className="btn small" onClick={() => ask({ change: "remove", name: p.name }, view?.rev)} aria-label={`${p.name} 지우기`}>
                        지우기
                      </button>
                    )}
                  </span>
                )}
              </li>
            ))}
          </ul>
          <button type="button" className="btn btn-primary" onClick={() => setEditing({ before: null })} data-testid="profile-add">
            profile 더하기
          </button>
        </div>
      </SettingsGroup>
      {/* 고치던 칸은 그 값이 바뀔 때만 새로 채운다(profile을 더해 판만 바뀌면 그대로) */}
      {safety && (
        <AllowGroup
          key={JSON.stringify([scope, safety.thread_profiles, safety.coordinator_profiles, safety.thread_profile, safety.coordinator_profile])}
          safety={safety}
          scope={scope}
          onScope={setScope}
          names={names}
          onPreview={(c) => ask(c, safety.rev)}
        />
      )}
      {editing && (
        <ProfileDialog
          before={editing.before}
          onClose={() => setEditing(null)}
          onPreview={async (change) => {
            await preview(change, view?.rev);
            setEditing(null);
          }}
        />
      )}
      {planned && (
        <PlanDialog
          planned={planned}
          onClose={() => setPlanned(null)}
          onApplied={(message, warnings, next) => {
            setView(next);
            setDone([message, ...warnings.map((w) => `경고: ${w}`), "host가 다음에 profile을 쓸 때 읽는다. 이미 도는 thread는 다시 띄울 때까지 옛 설정이다."]);
            setPlanned(null);
            load();
          }}
          onConflict={() => {
            setPlanned(null);
            setError(conflictNote);
            load();
          }}
        />
      )}
    </div>
  );
}

function AllowGroup({
  safety,
  scope,
  onScope,
  names,
  onPreview,
}: {
  safety: SafetyView;
  scope: SettingsScope;
  onScope: (s: SettingsScope) => void;
  names: string[];
  onPreview: (c: ProfileChangeReq) => void;
}) {
  return (
    <SettingsGroup title="허용 목록·기본 profile" note="thread·coordinator가 고를 수 있는 profile과, 정하지 않았을 때 쓰는 profile. 이 목록은 사람만 바꾼다.">
      <div className="settings-item">
        <ScopePick scope={scope} project={safety.project} onChange={onScope} />
        {(["thread", "coordinator"] as const).map((role) => (
          <AllowRow key={role} role={role} current={role === "thread" ? safety.thread_profiles : safety.coordinator_profiles} names={names} scope={scope} onPreview={onPreview} />
        ))}
        {(["thread", "coordinator"] as const).map((role) => (
          <DefaultRow key={role} role={role} current={role === "thread" ? safety.thread_profile : safety.coordinator_profile} names={names} onPreview={onPreview} />
        ))}
        <p className="op-muted">기본 profile은 host.toml [defaults]다. 프로젝트 설정(aaa.toml)에 따로 적혀 있으면 그것이 앞선다.</p>
      </div>
    </SettingsGroup>
  );
}

function AllowRow({
  role,
  current,
  names,
  scope,
  onPreview,
}: {
  role: "thread" | "coordinator";
  current: string[] | null;
  names: string[];
  scope: SettingsScope;
  onPreview: (c: ProfileChangeReq) => void;
}) {
  const [all, setAll] = useState(current === null);
  const [picked, setPicked] = useState<string[]>(current ?? []);
  const choices = [...new Set([...names, ...picked])];
  return (
    <fieldset className="admin-fieldset" data-testid={`allow-${role}`}>
      <legend>
        {role} 허용 — 지금 {allowText(current)}
      </legend>
      <label className="op-check">
        <input type="checkbox" checked={all} onChange={(e) => setAll(e.target.checked)} />
        <span>모든 profile</span>
      </label>
      {!all && (
        <div className="admin-line">
          {choices.map((n) => (
            <label key={n} className="op-check">
              <input type="checkbox" checked={picked.includes(n)} onChange={(e) => setPicked(e.target.checked ? [...picked, n] : picked.filter((x) => x !== n))} />
              <span>{n}</span>
            </label>
          ))}
        </div>
      )}
      <button
        type="button"
        className="btn small"
        onClick={() =>
          onPreview({
            change: "allow",
            scope,
            role,
            names: all ? null : picked,
          })
        }
      >
        미리보기…
      </button>
    </fieldset>
  );
}

function DefaultRow({ role, current, names, onPreview }: { role: "thread" | "coordinator"; current: string; names: string[]; onPreview: (c: ProfileChangeReq) => void }) {
  const [name, setName] = useState(current);
  return (
    <div className="admin-line" data-testid={`default-${role}`}>
      <label className="op-field">
        <span>{role} 기본 profile</span>
        <select value={name} onChange={(e) => setName(e.target.value)}>
          {[...new Set([current, ...names])].map((n) => (
            <option key={n} value={n}>
              {n}
            </option>
          ))}
        </select>
      </label>
      <button type="button" className="btn small" disabled={name === current} onClick={() => onPreview({ change: "default", role, name })}>
        미리보기…
      </button>
    </div>
  );
}

function ProfileDialog({ before, onClose, onPreview }: { before: ProfileAdminRow | null; onClose: () => void; onPreview: (c: ProfileChangeReq) => Promise<void> }) {
  const [d, setD] = useState<ProfileDraft>(() => draftOf(before));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const put = (k: keyof ProfileDraft) => (e: { target: { value: string } }) => setD({ ...d, [k]: e.target.value });
  const efforts = effortValues(d.harness);
  const nameOk = before !== null || profileNameOk(d.name.trim());
  return (
    <AdminDialog label={before ? `${before.name} 고치기` : "profile 더하기"} busy={busy} onClose={onClose}>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          if (!nameOk) return;
          setBusy(true);
          setError(null);
          onPreview(profileChange(before, d)).catch((e: unknown) => {
            setError(say(e));
            setBusy(false);
          });
        }}
      >
        <div className="modal-body">
          {before?.builtin && <p className="op-muted">기본 profile이다. 고치면 같은 이름의 profile을 host.toml에 더해 대신한다(지우면 기본이 돌아온다).</p>}
          {!before && (
            <label className="op-field">
              <span>이름</span>
              <input autoFocus value={d.name} onChange={put("name")} disabled={busy} data-testid="profile-name" />
            </label>
          )}
          <label className="op-field">
            <span>harness</span>
            <select value={d.harness} onChange={put("harness")} disabled={busy} data-testid="profile-harness">
              {WEB_HARNESSES.map((h) => (
                <option key={h} value={h}>
                  {h}
                </option>
              ))}
            </select>
          </label>
          {hasModel(d.harness) && (
            <label className="op-field">
              <span>model(비우면 harness 기본)</span>
              <input value={d.model} onChange={put("model")} disabled={busy} data-testid="profile-model" />
            </label>
          )}
          {efforts.length > 0 && (
            <label className="op-field">
              <span>effort</span>
              <select value={d.effort} onChange={put("effort")} disabled={busy}>
                <option value="">기본</option>
                {efforts.map((v) => (
                  <option key={v} value={v}>
                    {v}
                  </option>
                ))}
              </select>
            </label>
          )}
          <label className="op-field">
            <span>설명</span>
            <input value={d.description} onChange={put("description")} disabled={busy} />
          </label>
          <label className="op-field">
            <span>인자(줄마다 하나)</span>
            <textarea rows={3} value={d.args} onChange={put("args")} disabled={busy} className="mono" data-testid="profile-args" />
          </label>
          <p className="op-muted">인자는 harness 실행 파일 뒤에 그대로 붙는다. 다음 화면에서 실제 실행 인자를 보고 적용한다.</p>
          {!nameOk && d.name !== "" && <p className="op-warn">이름은 영문·숫자·. _ -로 40자까지 쓴다(-·.로 시작하지 않는다).</p>}
          {error && (
            <p className="op-warn" role="alert">
              {error}
            </p>
          )}
        </div>
        <footer className="modal-footer">
          <button type="button" className="btn" onClick={onClose} disabled={busy}>
            닫기
          </button>
          <button type="submit" className="btn btn-primary" disabled={busy || !nameOk} data-testid="profile-preview">
            미리보기
          </button>
        </footer>
      </form>
    </AdminDialog>
  );
}

const argv = (p: ProfileAdminRow | null) => (p ? [p.harness, ...p.launch].join(" ") : "(없음)");

function PlanDialog({
  planned,
  onClose,
  onApplied,
  onConflict,
}: {
  planned: { change: ProfileChangeReq; plan: ProfilePlanView };
  onClose: () => void;
  onApplied: (message: string, warnings: string[], view: ProfileAdminView) => void;
  onConflict: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { plan } = planned;
  return (
    <AdminDialog label="적용할까요?" busy={busy} onClose={onClose}>
      <div className="modal-body" data-testid="profile-plan">
        <p className="op-muted">적용 뒤 알림: {plan.message}</p>
        {(plan.before || plan.after) && (
          <dl className="admin-plan">
            <dt>지금 실행 인자</dt>
            <dd className="mono">{argv(plan.before)}</dd>
            <dt>적용 뒤</dt>
            <dd className="mono" data-testid="plan-after">
              {argv(plan.after)}
            </dd>
          </dl>
        )}
        <Notes items={plan.warnings.map((w) => `경고: ${w}`)} />
        {error && (
          <p className="op-warn" role="alert">
            {error}
          </p>
        )}
      </div>
      <footer className="modal-footer">
        <button type="button" className="btn" onClick={onClose} disabled={busy}>
          닫기
        </button>
        <button
          type="button"
          className="btn btn-primary"
          disabled={busy}
          data-testid="profile-apply"
          onClick={() => {
            setBusy(true);
            setError(null);
            call({
              op: "profile_apply",
              change: planned.change,
              expected: plan.rev,
            })
              .then((r) => {
                if (r.kind === "profile_applied") onApplied(r.message, r.warnings, r.view);
              })
              .catch((e: unknown) => {
                if (e instanceof ApiError && e.body.code === "conflict") onConflict();
                else {
                  setError(say(e));
                  setBusy(false);
                }
              });
          }}
        >
          적용
        </button>
      </footer>
    </AdminDialog>
  );
}

// ───────────── 머신 작업 폴더·hook ─────────────

/** 「머신」의 머신 하나 설정: 작업 폴더(답 3A)와 Codex hook(답 4A). 그 머신 agent가 한다 */
export function MachineSetup({ role, machines }: { role: DeviceRole; machines: MachineView[] }) {
  const admin = useSettingsAdmin();
  const live = machines.filter((m) => m.connection === "connected");
  const [name, setName] = useState<string>(() => live.find((m) => m.local)?.name ?? live[0]?.name ?? "");
  if (role === "reader" || admin !== true) return null;
  if (live.length === 0) return null;
  const chosen = live.some((m) => m.name === name) ? name : live[0]!.name;
  return (
    <SettingsGroup title="작업 폴더·hook" note="머신마다 thread 작업 폴더(worktree가 생기는 곳)와 harness hook 연동. 그 머신의 agent가 바꾼다.">
      <div className="settings-item">
        <label className="op-field">
          <span>머신</span>
          <select value={chosen} onChange={(e) => setName(e.target.value)} data-testid="setup-machine">
            {live.map((m) => (
              <option key={m.id} value={m.name}>
                {m.name}
              </option>
            ))}
          </select>
        </label>
        <WorkRoot key={`root-${chosen}`} machine={chosen} />
        <Hook key={`hook-${chosen}`} machine={chosen} />
      </div>
    </SettingsGroup>
  );
}

function WorkRoot({ machine }: { machine: string }) {
  const [view, setView] = useState<WorkRootView | null>(null);
  const [path, setPath] = useState("");
  const [mount, setMount] = useState("");
  const [checked, setChecked] = useState<{
    path: string;
    mount: string;
    real: string;
  } | null>(null);
  const [asking, setAsking] = useState<"check" | "set" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    call({ op: "work_root_show", machine })
      .then((r) => {
        if (!live || r.kind !== "work_root") return;
        setView(r);
        setPath(r.path);
        setMount(r.mount ?? "");
      })
      .catch((e: unknown) => live && setError(say(e)));
    return () => {
      live = false;
    };
  }, [machine]);
  const ok = absolutePath(path);
  return (
    <div className="admin-block" data-testid="work-root">
      <h4 className="admin-sub">작업 폴더</h4>
      {error && (
        <p className="op-warn" role="alert">
          {error}
        </p>
      )}
      {view && (
        <dl className="admin-plan">
          <dt>다음 agent 시작</dt>
          <dd className="mono" data-testid="work-root-next">
            {view.path}
            {view.mount ? ` (mount ${view.mount})` : ""}
          </dd>
          <dt>마지막 시작 기록</dt>
          <dd className="mono" data-testid="work-root-last">
            {view.last_start ?? "없음"}
          </dd>
          {view.previous_roots.length > 0 && (
            <>
              <dt>이전 폴더(작업 보존)</dt>
              <dd className="mono">{view.previous_roots.join(", ")}</dd>
            </>
          )}
        </dl>
      )}
      {view?.restart_required && (
        <p className="op-muted" data-testid="work-root-restart">
          다음 시작 설정이 마지막 시작 기록과 다르다: 그 머신 agent를 다시 띄우면 닿는다.
        </p>
      )}
      {note && (
        <p className="op-muted" role="status" data-testid="work-root-note">
          {note}
        </p>
      )}
      <div className="admin-line">
        <label className="op-field">
          <span>절대 경로</span>
          <input value={path} onChange={(e) => setPath(e.target.value)} className="mono" data-testid="work-root-path" />
        </label>
        <label className="op-field">
          <span>mount(외장 볼륨이면)</span>
          <input value={mount} onChange={(e) => setMount(e.target.value)} className="mono" />
        </label>
        <button type="button" className="btn" disabled={!ok} onClick={() => setAsking("check")} data-testid="work-root-check">
          점검…
        </button>
        <button type="button" className="btn btn-primary" disabled={!canSwitch(checked, path, mount)} onClick={() => setAsking("set")} data-testid="work-root-set">
          전환…
        </button>
      </div>
      {!ok && path !== "" && <p className="op-warn">절대 경로만 받는다(~·상대 경로는 안 된다).</p>}
      {checked && canSwitch(checked, path, mount) && (
        <p className="op-muted" data-testid="work-root-checked">
          점검 통과: {checked.real}
        </p>
      )}
      {asking === "check" && (
        <ConfirmDialog
          title="작업 폴더를 점검할까요?"
          body={`검사 파일을 만듭니다: ${machine}의 ${path.trim()}에 작은 파일을 쓰고 읽고 지운다. 폴더가 없으면 만든다.`}
          confirmLabel="점검"
          onConfirm={async () => {
            const p = path.trim();
            const m = mount.trim();
            const r = await call({
              op: "work_root_check",
              machine,
              path: p,
              mount: m || null,
            });
            if (r.kind === "work_root_checked") setChecked({ path: p, mount: m, real: r.real_path });
            setAsking(null);
          }}
          onClose={() => setAsking(null)}
        />
      )}
      {asking === "set" && checked && (
        <ConfirmDialog
          title="작업 폴더를 바꿀까요?"
          body={`${machine}의 다음 agent 시작부터 ${checked.path}에 thread 작업 폴더를 만든다. 지금 도는 thread와 이전 폴더의 작업은 그대로 둔다. agent가 저장 전에 한 번 더 점검한다.`}
          confirmLabel="전환"
          onConfirm={async () => {
            const r = await call({
              op: "work_root_set",
              machine,
              path: checked.path,
              mount: checked.mount || null,
            });
            if (r.kind === "work_root") {
              setView(r);
              setNote(r.changed ? "저장했다. 다음 agent 시작부터 쓴다." : "이미 그 설정이다.");
            }
            setChecked(null);
            setAsking(null);
          }}
          onClose={() => setAsking(null)}
        />
      )}
    </div>
  );
}

function Hook({ machine }: { machine: string }) {
  const [view, setView] = useState<HookView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [asking, setAsking] = useState<boolean | null>(null);
  useEffect(() => {
    let live = true;
    call({ op: "hook_status", machine })
      .then((r) => live && r.kind === "hook" && setView(r))
      .catch((e: unknown) => live && setError(say(e)));
    return () => {
      live = false;
    };
  }, [machine]);
  return (
    <div className="admin-block" data-testid="hook">
      <h4 className="admin-sub">harness hook</h4>
      <p className="op-muted">Claude: aaa가 실행마다 hook을 넣는다(설치할 것 없음).</p>
      {error && (
        <p className="op-warn" role="alert">
          {error}
        </p>
      )}
      {view && (
        <div className="admin-line">
          <span data-testid="hook-codex">Codex 전역 hook: {view.codex_installed ? "설치됨" : "없음"}</span>
          <button type="button" className="btn small" onClick={() => setAsking(!view.codex_installed)} data-testid="hook-toggle">
            {view.codex_installed ? "제거…" : "설치…"}
          </button>
        </div>
      )}
      {view?.codex_problem && <p className="op-warn">{view.codex_problem}</p>}
      {view?.codex_installed && <p className="op-muted">Codex가 hook을 처음 만나면 Codex 화면에서 신뢰를 묻는다. 그 머신에서 사람이 답한다.</p>}
      {asking !== null && (
        <ConfirmDialog
          title={asking ? `${machine}에 Codex hook을 설치할까요?` : `${machine}의 Codex hook을 제거할까요?`}
          body={
            asking
              ? "그 머신 Codex 설정에 aaa 상태 hook을 넣고 hooks 기능을 켠다(aaa integration install codex와 같다). 고치기 전 설정은 백업한다."
              : "그 머신 Codex 설정에서 aaa hook만 뺀다(aaa integration uninstall codex와 같다). 다른 hook은 그대로 둔다."
          }
          confirmLabel={asking ? "설치" : "제거"}
          onConfirm={async () => {
            const r = await call({ op: "hook_set", machine, install: asking });
            if (r.kind === "hook") setView(r);
            setAsking(null);
          }}
          onClose={() => setAsking(null)}
        />
      )}
    </div>
  );
}
