// 상태 정본은 host DB다. 웹은 받은 요약만 표시한다. 이 카드는 배포 요청을 보내지 않는다(시작은 설정 「배포」, 관리 GUI W6a).
import { useId, useState } from "react";
import { ChevronDown, ChevronUp } from "lucide-react";
import type { Outcome, RolloutCard as Card, RolloutPhase, RolloutState, DeployGuardPhase } from "./gen/proto";
import "./RolloutCard.css";
import { aaaVersion } from "./rolloutStatus";
export type RolloutSnapshot = Outcome<Card | null> | null;
const states: Record<RolloutState, string> = {
  running: "배포 중", recovering: "복구 중", stopped: "배포 중단", stopped_rolled_back: "배포 실패 · 복구 완료",
  recovery_failed: "배포 실패 · 복구 확인 필요", done: "배포 완료",
};
const phases: Record<RolloutPhase, string> = {
  transfer: "전송·대조", preflight: "사전 검사", host_update: "host 교체", host_restart: "host 재접속 확인",
  agent_update: "agent 교체", agent_restart: "agent 재접속 확인",
};
const recovery: Record<DeployGuardPhase, string> = {
  armed: "복구 준비 중", awaiting_new: "복구 요청 중", new_ready: "복구 요청 중", restoring: "이전 파일 복구 중",
  restarting: "이전 판 재시작 중", awaiting_old: "이전 실행 확인 중", restored: "이전 실행 확인 완료",
  failed: "복구 확인 필요", cancelled: "설치 전 중단", verified_new: "새 실행 확인 완료",
};
export function RolloutCard({ snapshot, connected, collapsible = true }: { snapshot: RolloutSnapshot; connected: boolean; collapsible?: boolean }) {
  const card = snapshot && "ok" in snapshot ? snapshot.ok : null;
  // 접기는 이 화면의 표시 상태뿐이다. 새 배포·상태 전이는 다시 펼치며, 실패·복구는 접기 단추를 두지 않는다.
  return <RolloutStatus key={`${card?.id}/${card?.state}`} snapshot={snapshot} connected={connected} collapsible={collapsible} />;
}
function RolloutStatus({ snapshot, connected, collapsible }: { snapshot: RolloutSnapshot; connected: boolean; collapsible: boolean }) {
  const card = snapshot && "ok" in snapshot ? snapshot.ok : null;
  const [folded, setFolded] = useState(false);
  const detailsId = useId();
  return <section className="rollout-card" data-testid="rollout-card" data-state={card?.state ?? "none"} data-updated={card?.updated_ms} aria-label="배포 상태">
    <div className="rollout-heading"><h3>배포</h3>{card && <strong role="status">{states[card.state]}</strong>}
      {collapsible && card?.state === "done" && <button type="button" className="icon-button" aria-label={folded ? "배포 상세 펼치기" : "배포 상세 접기"}
        title={folded ? "배포 상세 펼치기" : "배포 상세 접기"} aria-expanded={!folded} aria-controls={detailsId} onClick={() => setFolded(!folded)}>
        {folded ? <ChevronDown aria-hidden="true" /> : <ChevronUp aria-hidden="true" />}
      </button>}
    </div>
    {!connected && <p className="rollout-offline" role="status">연결 끊김 · 마지막 수신 상태</p>}
    <div id={detailsId} hidden={folded}>
    {snapshot === null ? <p>배포 기록을 읽는 중…</p> : "err" in snapshot ? <p role="alert">상태 조회 · {snapshot.err.code} · {snapshot.err.message}</p> : !card ? <p>배포 기록이 없습니다.</p> : <>
      <p className="rollout-version">{card.version ? `배포 대상 ${aaaVersion(card.version)}` : "후보 검사 중"}</p>
      <p className="rollout-meta">{card.commit ? `커밋 ${card.commit.slice(0, 12)} · ` : ""}SHA {card.sha256.slice(0, 12)}</p>
      <ol className="rollout-steps">
        {card.steps.map((step, index) => {
          const current = index === card.cursor;
          const failed = current && ["stopped", "stopped_rolled_back", "recovery_failed"].includes(card.state);
          const label = step.completed ? "완료" : current ? card.state === "recovering" ? "복구 중" : card.state === "stopped_rolled_back" ? "복구 완료" : failed ? "중단" : "진행 중" : ["running", "done"].includes(card.state) ? "대기" : "미진행";
          const percent = step.total_bytes > 0 ? Math.min(100, Math.max(0, Math.floor(step.transferred_bytes * 100 / step.total_bytes))) : null;
          return <li key={`${step.machine}/${step.phase}`} aria-current={current ? "step" : undefined} data-completed={step.completed} data-failed={failed}>
            <span className="rollout-mark" aria-label={label}>{step.completed ? "✓" : current ? "●" : "○"}</span>
            <div><span>{step.machine} · {phases[step.phase]}</span>
              {step.phase === "transfer" && percent !== null && <div className="rollout-transfer"><progress max={100} value={percent} aria-label={`${step.machine} 전송률`} /><span>{percent}%</span></div>}
            </div>
            <span className="rollout-step-state">{label}</span>
          </li>;
        })}
      </ol>
      {card.recovery_phase && <p className="rollout-recovery">{recovery[card.recovery_phase]}</p>}
      {card.error && <p className="rollout-error" role="alert">{card.error}</p>}
      <details><summary>검증 정보</summary><dl>
        <dt>배포 ID</dt><dd>{card.id}</dd><dt>후보 SHA-256</dt><dd>{card.sha256}</dd>
        {card.commit && <><dt>커밋</dt><dd>{card.commit}</dd></>}
        {card.recovery_sha256 && <><dt>복구 SHA-256</dt><dd>{card.recovery_sha256}</dd></>}
        <dt>시작</dt><dd>{new Date(card.created_ms).toLocaleString()}</dd>
        <dt>마지막 변경</dt><dd>{new Date(card.updated_ms).toLocaleString()}</dd>
      </dl></details>
    </>}
    </div>
  </section>;
}
