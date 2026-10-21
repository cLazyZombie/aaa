// 설정 「머신」의 「SSH로 추가」(웹 머신 추가 M2, plan §8.375): host가 이미 ssh로 로그인되는 머신에 접속해 host key 지문을 보이고, 사람이
// 믿으면 OS·CPU를 감지한다. 원격에 쓰지 않는다(설치는 M3). 작업은 host DB에 남고 진행은 host `enroll` 사건으로 다시 읽는다.
// 보기 기기는 목록만 본다.
import { useCallback, useEffect, useRef, useState } from "react";
import type { DeviceRole, MachineSetupView } from "./gen/proto";
import { ApiError, call, me, onEnrollChanged } from "./api";
import { ConfirmDialog } from "./ConfirmDialog";
import { SettingsGroup } from "./herdr/SettingsControls.tsx";
import { busy, canSsh, detectedLine, needsPerson, PHASE, sshLine, TARGETS, targetLabel, targetProblem } from "./machineSetupModel.ts";

const say = (e: unknown) => (e instanceof ApiError ? e.message : e instanceof Error ? e.message : String(e));

export function MachineSetupPanel({ role }: { role: DeviceRole }) {
  const [ok, setOk] = useState<boolean | null>(null);
  const [setups, setSetups] = useState<MachineSetupView[]>([]);
  const [target, setTarget] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const [trusting, setTrusting] = useState<{ job: MachineSetupView; sha256: string } | null>(null);
  const seq = useRef(0);
  const canEdit = role !== "reader";
  useEffect(() => {
    let live = true;
    me()
      .then((m) => live && setOk(canSsh(m?.front_api)))
      .catch(() => live && setOk(false));
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
      note="host Mac에서 이미 ssh로 로그인되는 머신을 넣는다. host의 SSH 키·ssh-agent·~/.ssh/config만 쓰고 비밀번호는 받지 않는다. 지금은 접속·host key 확인·OS 감지까지이고 원격에는 쓰지 않는다."
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
                placeholder="mac-b 또는 me@mac-b.example.ts.net:22"
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
                  {PHASE[s.phase]}
                  {busy(s.phase) ? "…" : ""}
                </span>
                {canEdit && (
                  <span className="admin-actions">
                    <button type="button" className="btn small" disabled={sending} onClick={() => void run(() => call({ op: "machine_setup_cancel", job: s.job }))} aria-label={`${s.target} 작업 지우기`}>
                      지우기
                    </button>
                  </span>
                )}
                <SetupBody s={s} canEdit={canEdit} sending={sending} onTrust={(sha256) => setTrusting({ job: s, sha256 })} onChoose={(os, arch) => void run(() => call({ op: "machine_setup_choose", job: s.job, os, arch }))} />
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
    </SettingsGroup>
  );
}

function SetupBody({
  s,
  canEdit,
  sending,
  onTrust,
  onChoose,
}: {
  s: MachineSetupView;
  canEdit: boolean;
  sending: boolean;
  onTrust: (sha256: string) => void;
  onChoose: (os: string, arch: string) => void;
}) {
  const [pick, setPick] = useState(0);
  const via = sshLine(s);
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
          {s.phase === "ready" && <span className="op-muted"> — 설치는 다음 단계에서 이어진다</span>}
        </p>
      )}
    </div>
  );
}
