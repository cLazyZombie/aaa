// 관리 GUI W6a(plan §8.361): 설정 「배포」. 빌드 머신에 이미 있는 후보(옆에 provenance.json이 있는 aaa)를 점검(provenance·SHA 대조와
// dry-run self-check, 대상 순서와 준비)한 뒤 확인 창 하나로 rollout을 시작하고, 같은 rollout ID를 기록에서 다시 연다. 서비스 재시작은
// 머신마다 확인 창으로 연다. host가 판정한다(조작 기기만, 웹은 실행 파일을 올리지 않는다, 진행 중 rollout이 있으면 거부). 웹은 상태를
// 갖지 않는다: 기록·작업은 host에서 다시 읽는다.
import "./ThreadActions.css";
import "./AdminPages.css";

import { useCallback, useEffect, useRef, useState } from "react";
import type { DeployJobCard, DeviceRole, MachineView, RolloutCard as Card, RolloutPlanView } from "./gen/proto";
import { ApiError, call, me, onRolloutChanged } from "./api";
import { ConfirmDialog } from "./ConfirmDialog";
import { SettingsGroup } from "./herdr/SettingsControls.tsx";
import { RolloutCard } from "./RolloutCard";
import { canDeploy, canStart, candidatePathOk, jobState, jobWhat, newDeployId, refused, restartAction, rolloutSpec, rolloutTargets, sizeText, type PlanInput } from "./deployModel.ts";

const say = (e: unknown) => (e instanceof ApiError ? e.message : e instanceof Error ? e.message : String(e));

/** 1.82 이상 host인가(모르면 null) */
function useDeployAdmin(): boolean | null {
  const [ok, setOk] = useState<boolean | null>(null);
  useEffect(() => {
    let live = true;
    me()
      .then((m) => live && setOk(canDeploy(m?.front_api)))
      .catch(() => live && setOk(false));
    return () => {
      live = false;
    };
  }, []);
  return ok;
}

export function DeployPage({ role, machines }: { role: DeviceRole; machines: MachineView[] }) {
  const admin = useDeployAdmin();
  const [cards, setCards] = useState<Card[] | null>(null);
  const [jobs, setJobs] = useState<DeployJobCard[]>([]);
  const [error, setError] = useState<string | null>(null);
  const seq = useRef(0);
  const canEdit = role !== "reader" && admin === true;
  // 연결 바뀜(재시작 뒤 다시 붙음)도 다시 읽는다
  const links = machines.map((m) => `${m.id}:${m.connection}`).join(",");
  const load = useCallback(() => {
    if (admin !== true) return;
    const mine = ++seq.current;
    Promise.all([call({ op: "rollouts" }), call({ op: "deploy_jobs" })])
      .then(([r, j]) => {
        if (mine !== seq.current) return;
        if (r.kind === "rollout_cards") setCards(r.cards);
        if (j.kind === "deploy_jobs") setJobs(j.jobs);
        setError(null);
      })
      .catch((e: unknown) => mine === seq.current && setError(say(e)));
  }, [admin]);
  useEffect(() => {
    load();
    return onRolloutChanged(load);
  }, [load, links]);
  if (admin === false) return <p className="op-muted">이 host는 웹 배포를 모른다. host를 새 판으로 바꾸면 보인다(지금은 coordinator나 host 터미널의 aaa deploy).</p>;
  return (
    <div data-testid="settings-deploy">
      {error && (
        <p className="op-warn" role="alert">
          {error}
        </p>
      )}
      {canEdit && <NewRollout machines={machines} onStarted={load} />}
      <SettingsGroup title="배포 기록" note="새것부터. 화면이 끊겨도 같은 rollout ID의 상태를 host에서 다시 읽는다.">
        <div className="settings-item" data-testid="rollout-history">
          {cards === null ? (
            <p className="op-muted">읽는 중…</p>
          ) : cards.length === 0 ? (
            <p className="op-muted">배포 기록이 없습니다.</p>
          ) : (
            cards.map((c) => (
              <details key={c.id} className="admin-block" data-rollout={c.id} data-state={c.state}>
                <summary className="mono">
                  {new Date(c.created_ms).toLocaleString()} · {c.version ? `aaa ${c.version}` : "후보 검사 중"} · {c.id}
                </summary>
                <RolloutCard snapshot={{ ok: c }} connected help={false} />
              </details>
            ))
          )}
        </div>
      </SettingsGroup>
      <SettingsGroup title="서비스 재시작" note="그 머신 agent가 감독자(launchd)로 다시 띄운다. 세션은 이어진다. host를 다시 띄우면 이 화면은 잠시 끊겼다 다시 붙는다.">
        <div className="settings-item">
          {canEdit && <Restarts machines={machines} onStarted={load} />}
          <ul className="admin-list" data-testid="deploy-jobs">
            {jobs.map((j) => (
              <li key={j.id} data-job={j.id} data-state={j.state ?? "unknown"}>
                <span>
                  {j.machine} · {jobWhat(j.work)}
                  {j.web ? "" : " (coordinator)"}
                </span>
                <span className="op-muted">
                  {" "}
                  · {jobState(j)} · {new Date(j.created_ms).toLocaleString()}
                </span>
                {j.error && <p className="op-warn">{j.error}</p>}
              </li>
            ))}
          </ul>
          {jobs.length === 0 && <p className="op-muted">재시작 기록이 없습니다.</p>}
        </div>
      </SettingsGroup>
    </div>
  );
}

function NewRollout({ machines, onStarted }: { machines: MachineView[]; onStarted: () => void }) {
  const live = machines.filter((m) => m.connection === "connected");
  const host = machines.find((m) => m.local);
  const [picking, setSource] = useState<string>(() => host?.name ?? live[0]?.name ?? "");
  // 처음 그릴 때 머신 목록이 비었거나 고른 머신이 끊기면 연결된 머신으로
  const source = live.some((m) => m.name === picking) ? picking : (live.find((m) => m.local)?.name ?? live[0]?.name ?? "");
  const [path, setPath] = useState("");
  // host 머신은 늘 맨 앞이라 고르지 않는다. 처음에는 등록된 모든 머신(CLI의 all과 같다)
  const [picked, setPicked] = useState<Set<string>>(() => new Set(machines.filter((m) => !m.local).map((m) => m.name)));
  const [plan, setPlan] = useState<{ input: PlanInput; view: RolloutPlanView } | null>(null);
  const [checking, setChecking] = useState(false);
  // 확인 창 하나에 rollout ID 하나: 응답을 잃고 다시 눌러도 같은 ID라 host가 같은 rollout을 돌려준다
  const [asking, setAsking] = useState<string | null>(null);
  const [started, setStarted] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const others = machines.filter((m) => !m.local);
  const input: PlanInput = {
    source,
    path,
    machines: rolloutTargets(
      host?.name,
      others.filter((m) => picked.has(m.name)).map((m) => m.name),
    ),
  };
  const check = async () => {
    setChecking(true);
    setError(null);
    setPlan(null);
    const asked = { ...input, path: input.path.trim() };
    try {
      const r = await call({ op: "rollout_plan", source_machine: asked.source, path: asked.path, machines: asked.machines });
      if (r.kind === "rollout_plan") setPlan({ input: asked, view: r.plan });
    } catch (e) {
      setError(say(e));
    } finally {
      setChecking(false);
    }
  };
  const ready = canStart(plan, input);
  const view = plan?.view;
  return (
    <SettingsGroup title="새 배포" note="빌드 머신에 이미 있는 후보(scripts/deploy-build.sh의 출력 폴더, 옆에 provenance.json)만 고른다. 웹은 실행 파일을 올리지 않는다.">
      <div className="settings-item" data-testid="new-rollout">
        <div className="admin-line">
          <label className="op-field">
            <span>빌드 머신</span>
            <select value={source} onChange={(e) => setSource(e.target.value)} data-testid="deploy-source">
              {live.map((m) => (
                <option key={m.id} value={m.name}>
                  {m.name}
                </option>
              ))}
            </select>
          </label>
          <label className="op-field">
            <span>후보 경로</span>
            <input value={path} onChange={(e) => setPath(e.target.value)} className="mono" placeholder="~/aaa-candidate/aaa" data-testid="deploy-path" />
          </label>
        </div>
        {path !== "" && !candidatePathOk(path) && <p className="op-warn">빌드 머신의 절대 경로나 ~/로 시작하는 …/aaa만 받는다.</p>}
        {others.length > 0 && (
          <fieldset className="admin-line" data-testid="deploy-targets">
            <legend>대상(host 머신 {host?.name ?? ""}은 늘 먼저)</legend>
            {others.map((m) => (
              <label key={m.id} className="op-check">
                <input
                  type="checkbox"
                  checked={picked.has(m.name)}
                  onChange={(e) => {
                    const next = new Set(picked);
                    if (e.target.checked) next.add(m.name);
                    else next.delete(m.name);
                    setPicked(next);
                  }}
                />
                <span>{m.name}</span>
              </label>
            ))}
          </fieldset>
        )}
        <div className="admin-line">
          <button type="button" className="btn" disabled={checking || !candidatePathOk(path) || source === ""} onClick={check} data-testid="deploy-check">
            {checking ? "점검 중…" : "점검"}
          </button>
          <button type="button" className="btn btn-primary" disabled={!ready} onClick={() => setAsking(newDeployId())} data-testid="deploy-start">
            배포 시작…
          </button>
        </div>
        {error && (
          <p className="op-warn" role="alert">
            {error}
          </p>
        )}
        {view && (
          <div className="admin-block" data-testid="deploy-plan">
            <dl className="admin-plan">
              <dt>후보</dt>
              <dd className="mono">
                {view.candidate.machine}:{view.candidate.path}
              </dd>
              <dt>판(dry-run self-check)</dt>
              <dd className="mono">aaa {view.candidate.version}</dd>
              <dt>SHA-256</dt>
              <dd className="mono" data-testid="deploy-sha">
                {view.candidate.sha256}
              </dd>
              <dt>커밋</dt>
              <dd className="mono">{view.candidate.commit ?? "provenance에 없음"}</dd>
              <dt>크기</dt>
              <dd>{sizeText(view.candidate.total_bytes)}</dd>
              {view.candidate.built_at && (
                <>
                  <dt>빌드 시각</dt>
                  <dd>{new Date(view.candidate.built_at).toLocaleString()}</dd>
                </>
              )}
            </dl>
            <ol className="admin-list" data-testid="deploy-plan-targets">
              {view.targets.map((t) => (
                <li key={t.machine} data-problem={t.problem ?? ""}>
                  {t.machine}
                  {t.host ? " (host 먼저)" : ""} · {t.problem ?? "준비됨"}
                </li>
              ))}
            </ol>
            {view.busy && <p className="op-warn">진행 중인 rollout이 있다. 끝난 뒤 다시 점검한다.</p>}
            {plan && canStart(plan, plan.input) && !ready && <p className="op-muted">점검한 뒤 후보·대상을 바꿨다. 다시 점검한다.</p>}
          </div>
        )}
        {started && (
          <p className="op-muted" role="status" data-testid="deploy-started">
            rollout {started}을 시작했다. 응답이 끊겨도 같은 ID의 상태를 아래 「배포 기록」에서 이어 본다.
          </p>
        )}
        {asking && plan && (
          <ConfirmDialog
            title={`aaa ${plan.view.candidate.version}을 배포할까요?`}
            body={`SHA-256 ${plan.view.candidate.sha256}. 대상: ${plan.view.targets.map((t) => t.machine).join(" → ")}. 모든 대상에 후보를 보내 사전 검사(dry-run)를 마친 뒤 host부터 하나씩 교체하고 다시 띄운다. 새 판이 확인되지 않으면 그 머신을 이전 실행으로 되돌린다.`}
            confirmLabel="배포 시작"
            onConfirm={async () => {
              // ID는 보내기 전에 보인다: 응답이 끊겨도 같은 ID로 이어 본다. host가 확정으로 거절했으면 시작하지 않은 것이다
              const id = asking;
              setStarted(id);
              try {
                await call({ op: "rollout_start", id, spec: rolloutSpec(plan.view, plan.input.machines) });
              } catch (e) {
                if (e instanceof ApiError && refused(e.body.code)) setStarted(null);
                throw e;
              }
              setAsking(null);
              setPlan(null);
              onStarted();
            }}
            onClose={() => setAsking(null)}
          />
        )}
      </div>
    </SettingsGroup>
  );
}

function Restarts({ machines, onStarted }: { machines: MachineView[]; onStarted: () => void }) {
  const [asking, setAsking] = useState<{ machine: string; service: "host" | "agent"; id: string } | null>(null);
  const live = machines.filter((m) => m.connection === "connected");
  return (
    <>
      <ul className="admin-list" data-testid="deploy-restarts">
        {live.map((m) => (
          <li key={m.id} data-machine={m.name}>
            <span>{m.name}</span>
            <button type="button" className="btn" onClick={() => setAsking({ machine: m.name, service: "agent", id: newDeployId() })}>
              agent 재시작…
            </button>
            {m.local && (
              <button type="button" className="btn" onClick={() => setAsking({ machine: m.name, service: "host", id: newDeployId() })}>
                host 재시작…
              </button>
            )}
          </li>
        ))}
      </ul>
      {asking && (
        <ConfirmDialog
          title={`${asking.machine}의 ${asking.service}를 다시 띄울까요?`}
          body={
            asking.service === "host"
              ? "host를 감독자(launchd)로 다시 띄운다. 세션은 이어지고, 이 화면은 잠시 끊겼다가 다시 붙는다. 실행 파일은 바꾸지 않는다."
              : `${asking.machine}의 agent를 감독자(launchd)로 다시 띄운다. 세션은 이어지고, 그 머신은 잠시 끊김으로 보인다. 실행 파일은 바꾸지 않는다.`
          }
          confirmLabel="다시 띄우기"
          onConfirm={async () => {
            await call({ op: "deploy_start", machine: asking.machine, id: asking.id, action: restartAction(asking.service) });
            setAsking(null);
            onStarted();
          }}
          onClose={() => setAsking(null)}
        />
      )}
    </>
  );
}
