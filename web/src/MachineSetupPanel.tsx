// 설정 「머신」의 「SSH로 추가」(웹 머신 추가 M2 plan §8.384, M3 §8.420): host가 이미 ssh로 로그인되는 머신에 접속해 host key 지문을
// 보이고, 사람이 믿으면 OS·CPU를 감지한다. 1.103 host면 이어서 설치 설정 → 원격 install 계획 확인 → 「설치하고 등록」까지 한다.
// 작업은 host DB에 남고 진행은 host `enroll` 사건으로 다시 읽는다. 보기 기기는 목록만 본다.
import { useCallback, useEffect, useRef, useState } from "react";
import type { DeviceRole, EnrollAddress, InstallHarness, MachineSetupSettings, MachineSetupView } from "./gen/proto";
import { ApiError, call, me, onEnrollChanged } from "./api";
import { ConfirmDialog } from "./ConfirmDialog";
import { SettingsGroup } from "./herdr/SettingsControls.tsx";
import { canGuide, SOURCE } from "./enrollGuide.ts";
import {
  busy,
  canCancel,
  canInstall,
  canSsh,
  defaultSettings,
  detectedLine,
  HARNESSES,
  LOGIN,
  loginHarnesses,
  needsPerson,
  operationLabel,
  PHASE,
  settingsProblem,
  shortHash,
  sshLine,
  TARGETS,
  targetLabel,
  targetProblem,
} from "./machineSetupModel.ts";

const say = (e: unknown) => (e instanceof ApiError ? e.message : e instanceof Error ? e.message : String(e));

export function MachineSetupPanel({
  role,
  machines = [],
  onOpenPane,
}: {
  role: DeviceRole;
  machines?: string[];
  /** 설정에서 연 pane(끝 확인·로그인)으로 간다 */
  onOpenPane?: (id: string) => void;
}) {
  const [api, setApi] = useState<string | null | undefined>(undefined);
  const [setups, setSetups] = useState<MachineSetupView[]>([]);
  const [target, setTarget] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const [trusting, setTrusting] = useState<{ job: MachineSetupView; sha256: string } | null>(null);
  const [applying, setApplying] = useState<MachineSetupView | null>(null);
  const [addresses, setAddresses] = useState<EnrollAddress[] | null>(null);
  const seq = useRef(0);
  const canEdit = role !== "reader";
  const ok = api === undefined ? null : canSsh(api);
  const install = canInstall(api);
  useEffect(() => {
    let live = true;
    me()
      .then((m) => live && setApi(m?.front_api ?? null))
      .catch(() => live && setApi(null));
    return () => {
      live = false;
    };
  }, []);
  const load = useCallback(() => {
    if (!ok) return;
    const mine = ++seq.current;
    call({ op: "machine_setup_list" })
      .then((r) => mine === seq.current && r.kind === "machine_setups" && setSetups(r.setups))
      .catch((e: unknown) => mine === seq.current && setError(say(e)));
  }, [ok]);
  useEffect(() => {
    load();
    return onEnrollChanged(load);
  }, [load]);
  // 설치 설정의 host 주소 후보(직접 설치 안내와 같은 host 답): 설정을 넣을 작업이 생기면 한 번 읽는다
  const wantsAddresses = canEdit && install && canGuide(api) && setups.some((s) => s.phase === "ready");
  useEffect(() => {
    if (!wantsAddresses || addresses !== null) return;
    let live = true;
    call({ op: "enroll_guide" })
      .then((r) => live && r.kind === "enroll_guide" && setAddresses(r.guide.addresses))
      .catch(() => live && setAddresses([]));
    return () => {
      live = false;
    };
  }, [wantsAddresses, addresses]);
  if (!ok) return null;
  const run = async (work: () => Promise<unknown>) => {
    setSending(true);
    setError(null);
    try {
      await work();
      load();
    } catch (e) {
      setError(say(e));
    } finally {
      setSending(false);
    }
  };
  const problem = target === "" ? null : targetProblem(target);
  return (
    <SettingsGroup
      title="SSH로 추가"
      note={
        install
          ? "host Mac에서 이미 ssh로 로그인되는 머신을 넣는다. host의 SSH 키·ssh-agent·~/.ssh/config만 쓰고 비밀번호는 받지 않는다. 접속·지문·감지 뒤 설치 설정과 계획을 확인하고 설치·등록한다."
          : "host Mac에서 이미 ssh로 로그인되는 머신을 넣는다. host의 SSH 키·ssh-agent·~/.ssh/config만 쓰고 비밀번호는 받지 않는다. 지금은 접속·host key 확인·OS 감지까지이고 원격에는 쓰지 않는다."
      }
    >
      <div className="settings-item" data-testid="machine-setup">
        {error && (
          <p className="op-warn" role="alert">
            {error}
          </p>
        )}
        {canEdit && (
          <form
            className="admin-line"
            onSubmit={(e) => {
              e.preventDefault();
              if (targetProblem(target)) return;
              void run(async () => {
                await call({ op: "machine_setup_start", target: target.trim() });
                setTarget("");
              });
            }}
          >
            <label className="op-field admin-grow">
              <span>SSH 대상</span>
              <input
                value={target}
                onChange={(e) => setTarget(e.target.value)}
                placeholder="mac-b 또는 me@mac-b:22"
                autoCapitalize="off"
                autoCorrect="off"
                spellCheck={false}
                disabled={sending}
                data-testid="machine-setup-target"
              />
            </label>
            <button type="submit" className="btn btn-primary" disabled={sending || target.trim() === "" || problem !== null} data-testid="machine-setup-start">
              접속 시험
            </button>
          </form>
        )}
        {problem && <p className="machine-warn">{problem}</p>}
        {setups.length === 0 ? (
          <p className="op-muted">SSH 추가 작업이 없다</p>
        ) : (
          <ul className="admin-list setup-list" data-testid="machine-setup-list">
            {setups.map((s) => (
              <li key={s.job} data-job={s.job} data-target={s.target} data-phase={s.phase} className={needsPerson(s.phase) ? "setup-needs" : undefined}>
                <span className="admin-name">{s.target}</span>
                <span className={s.phase === "failed" || s.phase === "unsupported" ? "machine-warn" : "op-muted"} aria-live="polite">
                  {PHASE[s.phase] ?? s.phase}
                  {busy(s.phase) ? "…" : ""}
                </span>
                {canEdit && canCancel(s.phase) && (
                  <span className="admin-actions">
                    <button type="button" className="btn small" disabled={sending} onClick={() => void run(() => call({ op: "machine_setup_cancel", job: s.job }))} aria-label={`${s.target} 작업 지우기`}>
                      지우기
                    </button>
                  </span>
                )}
                <SetupBody
                  s={s}
                  canEdit={canEdit}
                  install={install}
                  sending={sending}
                  addresses={addresses ?? []}
                  machines={machines}
                  onTrust={(sha256) => setTrusting({ job: s, sha256 })}
                  onChoose={(os, arch) => void run(() => call({ op: "machine_setup_choose", job: s.job, os, arch }))}
                  onPlan={(settings) => void run(() => call({ op: "machine_setup_plan", job: s.job, settings }))}
                  onApply={() => setApplying(s)}
                  onCheck={() => void run(() => call({ op: "machine_setup_check", job: s.job }))}
                  onLogin={(harness) =>
                    void run(async () => {
                      const r = await call({ op: "machine_setup_login", job: s.job, harness });
                      if (r.kind === "pane") onOpenPane?.(r.id);
                    })
                  }
                  onOpenPane={onOpenPane}
                />
              </li>
            ))}
          </ul>
        )}
      </div>
      {trusting && (
        <ConfirmDialog
          title={`${trusting.job.target}의 host key를 믿을까요?`}
          body={`지문 ${trusting.sha256} — 그 머신에서 ssh-keygen -lf로 본 host key 지문과 같은지 맞춘다. 믿으면 aaa 전용 known_hosts에만 넣는다(~/.ssh/known_hosts는 그대로).`}
          confirmLabel="믿음"
          onConfirm={async () => {
            await call({ op: "machine_setup_trust", job: trusting.job.job, host_key_sha256: trusting.sha256 });
            setTrusting(null);
            load();
          }}
          onClose={() => setTrusting(null)}
        />
      )}
      {applying?.review && (
        <ConfirmDialog
          title={`${applying.target}에 aaa를 설치하고 ${applying.settings?.name ?? ""}로 등록할까요?`}
          body={`계획 ${shortHash(applying.review.plan_sha256)} — host가 이 계획에만 서명하고, 그 머신의 install이 적용한 뒤 이 host에 등록한다. 승인 범위: ${applying.review.operations.map(operationLabel).join(", ") || "없음"}.`}
          confirmLabel="설치하고 등록"
          onConfirm={async () => {
            await call({ op: "machine_setup_apply", job: applying.job, plan_sha256: applying.review!.plan_sha256 });
            setApplying(null);
            load();
          }}
          onClose={() => setApplying(null)}
        />
      )}
    </SettingsGroup>
  );
}

function SetupBody({
  s,
  canEdit,
  install,
  sending,
  addresses,
  machines,
  onTrust,
  onChoose,
  onPlan,
  onApply,
  onCheck,
  onLogin,
  onOpenPane,
}: {
  s: MachineSetupView;
  canEdit: boolean;
  install: boolean;
  sending: boolean;
  addresses: EnrollAddress[];
  machines: string[];
  onTrust: (sha256: string) => void;
  onChoose: (os: string, arch: string) => void;
  onPlan: (settings: MachineSetupSettings) => void;
  onApply: () => void;
  onCheck: () => void;
  onLogin: (harness: InstallHarness) => void;
  onOpenPane?: (id: string) => void;
}) {
  const [pick, setPick] = useState(0);
  const via = sshLine(s);
  const r = s.review;
  const i = s.install;
  return (
    <div className="setup-body">
      {via && <p className="op-muted mono">{via}</p>}
      {s.error && <p className="machine-warn">{s.error}</p>}
      {s.phase === "trust" && (
        <ul className="setup-keys" data-testid="machine-setup-keys">
          {s.host_keys.map((k) => (
            <li key={k.sha256}>
              <span className="op-muted">{k.kind}</span>
              <code className="mono">{k.sha256}</code>
              {canEdit && (
                <button type="button" className="btn btn-primary small" disabled={sending} onClick={() => onTrust(k.sha256)}>
                  이 지문을 믿음…
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
      {s.phase === "choose" && canEdit && (
        <form
          className="admin-line"
          onSubmit={(e) => {
            e.preventDefault();
            const t = TARGETS[pick]!;
            onChoose(t.os, t.arch);
          }}
        >
          <label className="op-field">
            <span>OS · CPU</span>
            <select value={pick} onChange={(e) => setPick(Number(e.target.value))} data-testid="machine-setup-choose">
              {TARGETS.map((t, i) => (
                <option key={i} value={i}>
                  {targetLabel(t)}
                </option>
              ))}
            </select>
          </label>
          <button type="submit" className="btn btn-primary" disabled={sending}>
            고름
          </button>
        </form>
      )}
      {s.detected && (
        <p data-testid="machine-setup-detected">
          {detectedLine(s.detected)}
          {s.phase === "ready" && !install && <span className="op-muted"> — 설치는 다음 단계에서 이어진다</span>}
        </p>
      )}
      {s.phase === "ready" && install && canEdit && (
        <SettingsForm s={s} sending={sending} addresses={addresses} machines={machines} onPlan={onPlan} />
      )}
      {r && (s.phase === "review" || s.phase === "needs_user" || s.phase === "installing" || s.phase === "installed") && (
        <div className="setup-review" data-testid="machine-setup-review">
          <p>
            계획 <code className="mono" data-testid="machine-setup-plan">{shortHash(r.plan_sha256)}</code>
            <span className="op-muted">
              {" "}
              · 보낸 {r.binary_version}(SHA {shortHash(r.binary_sha256)}, 그 머신에서 확인)
            </span>
          </p>
          {r.changes.length > 0 && (
            <ul className="setup-changes">
              {r.changes.map((c, n) => (
                <li key={n}>
                  <span className="mono">{c.kind}</span> {c.action}
                </li>
              ))}
            </ul>
          )}
          <p className="op-muted">승인 범위: {r.operations.map(operationLabel).join(", ") || "없음"}</p>
          {r.needs_user.length > 0 && (
            <ul className="setup-changes machine-warn" data-testid="machine-setup-needs">
              {r.needs_user.map((n, k) => (
                <li key={k}>{n.message}</li>
              ))}
            </ul>
          )}
          {s.phase === "review" && canEdit && (
            <div className="admin-line">
              <button type="button" className="btn btn-primary" disabled={sending} onClick={onApply} data-testid="machine-setup-apply">
                설치하고 등록…
              </button>
            </div>
          )}
        </div>
      )}
      {i && (
        <p data-testid="machine-setup-install" aria-live="polite">
          {s.phase === "installed" && i.machine ? `${i.machine}로 등록됐다 · install ${i.result?.status ?? ""}` : (i.last_event ?? "설치를 시작한다")}
          {i.result?.needs_user.map((n, k) => (
            <span key={k} className="machine-warn">
              {" "}
              · {n.message}
            </span>
          ))}
        </p>
      )}
      {s.phase === "installed" && i && (
        <div className="setup-review" data-testid="machine-setup-after">
          {i.checks && i.checks.length > 0 && (
            <ul className="setup-changes mono" data-testid="machine-setup-checks">
              {i.checks.map((l, k) => (
                <li key={k}>{l}</li>
              ))}
            </ul>
          )}
          {canEdit && (
            <div className="admin-line">
              <button type="button" className="btn" disabled={sending} onClick={onCheck} data-testid="machine-setup-check">
                끝 확인(shell pane)
              </button>
              {loginHarnesses(s).map((h) => (
                <button key={h} type="button" className="btn" disabled={sending} onClick={() => onLogin(h)}>
                  {h} 로그인 pane
                </button>
              ))}
              {i.check_pane && onOpenPane && (
                <button type="button" className="btn small" onClick={() => onOpenPane(i.check_pane!)}>
                  끝 확인 pane 열기
                </button>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

/** 감지가 끝난 작업의 설치 설정(이름·host 주소·작업 root·harness·로그인) */
function SettingsForm({
  s,
  sending,
  addresses,
  machines,
  onPlan,
}: {
  s: MachineSetupView;
  sending: boolean;
  addresses: EnrollAddress[];
  machines: string[];
  onPlan: (settings: MachineSetupSettings) => void;
}) {
  const first = (addresses.find((a) => a.source !== "loopback") ?? addresses[0])?.address ?? "";
  const [form, setForm] = useState<MachineSetupSettings>(() => s.settings ?? defaultSettings(s.target, s.detected ?? null, first));
  // 주소 후보가 늦게 오면 빈 칸만 채운다
  useEffect(() => {
    if (first) setForm((f) => (f.host ? f : { ...f, host: first }));
  }, [first]);
  const problem = settingsProblem(form, machines);
  const set = (patch: Partial<MachineSetupSettings>) => setForm((f) => ({ ...f, ...patch }));
  const picked = addresses.find((a) => a.address === form.host);
  return (
    <form
      className="setup-form"
      data-testid="machine-setup-settings"
      onSubmit={(e) => {
        e.preventDefault();
        if (!problem) onPlan({ ...form, host: form.host.trim(), work_mount: form.work_mount?.trim() || null });
      }}
    >
      <label className="op-field">
        <span>머신 이름</span>
        <input value={form.name} onChange={(e) => set({ name: e.target.value })} autoCapitalize="off" spellCheck={false} data-testid="machine-setup-name" />
      </label>
      <label className="op-field">
        <span>host 주소(그 머신이 붙을 곳)</span>
        <input value={form.host} onChange={(e) => set({ host: e.target.value })} list={`setup-hosts-${s.job}`} autoCapitalize="off" spellCheck={false} data-testid="machine-setup-host" />
        <datalist id={`setup-hosts-${s.job}`}>
          {addresses.map((a) => (
            <option key={a.address} value={a.address}>
              {SOURCE[a.source] ?? a.source}
            </option>
          ))}
        </datalist>
      </label>
      {picked?.source === "loopback" && <p className="machine-warn">이 주소는 host 머신 안에서만 닿는다</p>}
      <label className="op-field">
        <span>작업 root(worktree 위치)</span>
        <input value={form.work_root} onChange={(e) => set({ work_root: e.target.value })} spellCheck={false} data-testid="machine-setup-work-root" />
      </label>
      <label className="op-field">
        <span>외장 디스크 mount(비우면 내장)</span>
        <input value={form.work_mount ?? ""} onChange={(e) => set({ work_mount: e.target.value || null })} placeholder="/Volumes/<disk>" spellCheck={false} />
      </label>
      <fieldset className="setup-harness">
        <legend>harness</legend>
        {HARNESSES.map((h) => (
          <label key={h}>
            <input
              type="checkbox"
              checked={form.harness.includes(h)}
              onChange={(e) => set({ harness: e.target.checked ? [...form.harness, h] : form.harness.filter((x) => x !== h) })}
            />{" "}
            {h}
          </label>
        ))}
        <label>
          <span>로그인 </span>
          <select value={form.login} onChange={(e) => set({ login: e.target.value as MachineSetupSettings["login"] })}>
            {Object.entries(LOGIN).map(([k, v]) => (
              <option key={k} value={k}>
                {v}
              </option>
            ))}
          </select>
        </label>
      </fieldset>
      {problem && <p className="machine-warn">{problem}</p>}
      <div className="admin-line">
        <button type="submit" className="btn btn-primary" disabled={sending || problem !== null} data-testid="machine-setup-plan-start">
          계획 보기
        </button>
        <span className="op-muted">같은 판 aaa를 그 머신 임시 폴더에 보내고, 그 머신의 install이 계획만 낸다(아직 설치하지 않는다)</span>
      </div>
    </form>
  );
}
